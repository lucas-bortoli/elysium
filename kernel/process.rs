//! The shared plumbing behind `ely:process`: the identifiers and value
//! types the process manager and the JS bindings both speak, plus the
//! hidden globals `runtime_modules/process.ts` is a thin wrapper over.
//!
//! Nothing here drives scheduling — that's `kernel/process_manager.rs`. The
//! bindings can't reach the `ProcessManager` (they run inside a
//! `context.with` on one process's VM), so every request they make —
//! spawn, send, arm-draining, kill — is pushed onto a shared
//! [`ProcessChannel`] the manager drains between turns.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use rquickjs::function::Opt;
use rquickjs::{Ctx, Function, Persistent, Result};

/// A process's identity in the table. `0` is reserved for the kernel (the
/// `from` of any kernel-originated envelope); real processes start at `1`.
pub type ProcessId = u32;

/// One message in flight between two processes. `data` is the payload's
/// JSON text (as produced by `JSON.stringify` on the sending side), or
/// `None` for an absent payload — reconstructed into an `ely:container`
/// `Option` on delivery.
#[derive(Clone)]
pub struct Envelope {
    pub kind: String,
    pub from: ProcessId,
    pub to: ProcessId,
    pub data: Option<String>,
}

impl Envelope {
    /// Serializes to the object `process.ts`'s `onMessage` handlers
    /// receive: `{ kind, from, to, data }`, with `data` inlined as raw
    /// JSON (or `null`).
    pub fn to_json(&self) -> String {
        format!(
            "{{\"kind\":{},\"from\":{},\"to\":{},\"data\":{}}}",
            json_string(&self.kind),
            self.from,
            self.to,
            self.data.as_deref().unwrap_or("null"),
        )
    }
}

/// A request the manager applies between turns, after the round-robin pass.
pub enum Control {
    /// `requestExit(target)`: move `target` to `Draining` so it is
    /// force-reaped if it doesn't wind itself down within the grace period.
    ArmDraining { target: ProcessId, by: ProcessId },
    /// `terminate(target)`: drop `target` outright.
    Kill { target: ProcessId, by: ProcessId },
}

/// A `spawn(path, args)` waiting to be turned into a live process.
pub struct SpawnRequest {
    pub id: ProcessId,
    pub path: String,
    pub arguments_json: Option<String>,
}

/// Shared, cheaply-cloneable mailbox between every process's `ely:process`
/// bindings and the one [`crate::process_manager::ProcessManager`]. Every
/// field is `Rc`, so a clone is another handle to the same queues.
#[derive(Clone, Default)]
pub struct ProcessChannel {
    spawn_id_counter: Rc<Cell<ProcessId>>,
    live_ids: Rc<RefCell<BTreeSet<ProcessId>>>,
    pending_spawns: Rc<RefCell<Vec<SpawnRequest>>>,
    pending_sends: Rc<RefCell<Vec<Envelope>>>,
    pending_control: Rc<RefCell<Vec<Control>>>,
}

impl ProcessChannel {
    /// A fresh channel with the id counter primed at `1` and the kernel
    /// (`0`) already registered as a valid message target.
    pub fn new() -> Self {
        let channel = Self::default();
        channel.spawn_id_counter.set(1);
        channel.live_ids.borrow_mut().insert(0);
        channel
    }

    /// Hands out the next id and records it as live immediately, so the
    /// caller of `spawn` can use it as a message target before the process
    /// itself has been installed.
    pub fn allocate_id(&self) -> ProcessId {
        let id = self.spawn_id_counter.get().max(1);
        self.spawn_id_counter.set(id + 1);
        self.live_ids.borrow_mut().insert(id);
        id
    }

    pub fn register(&self, id: ProcessId) {
        self.live_ids.borrow_mut().insert(id);
    }

    pub fn forget(&self, id: ProcessId) {
        self.live_ids.borrow_mut().remove(&id);
    }

    pub fn is_live(&self, id: ProcessId) -> bool {
        self.live_ids.borrow().contains(&id)
    }

    pub fn take_spawns(&self) -> Vec<SpawnRequest> {
        std::mem::take(&mut self.pending_spawns.borrow_mut())
    }

    pub fn take_sends(&self) -> Vec<Envelope> {
        std::mem::take(&mut self.pending_sends.borrow_mut())
    }

    pub fn take_control(&self) -> Vec<Control> {
        std::mem::take(&mut self.pending_control.borrow_mut())
    }
}

/// Installs the hidden `__process_*` globals `runtime_modules/process.ts`
/// builds its public surface on. `self_id` is this process's own id;
/// `channel` is the shared queue back to the manager; `message_handler`
/// and `exit_requested` are the two pieces of per-process state the
/// manager reads back each turn; `arguments_json` is the JSON passed to
/// `spawn` for this process (`None` for the init process).
#[allow(clippy::too_many_arguments)]
pub fn bootstrap_process_bindings<'js>(
    ctx: &Ctx<'js>,
    self_id: ProcessId,
    channel: ProcessChannel,
    message_handler: Rc<RefCell<Option<Persistent<Function<'static>>>>>,
    exit_requested: Rc<Cell<bool>>,
    arguments_json: Option<String>,
) -> Result<()> {
    let global = ctx.globals();

    global.set(
        "__process_self_id",
        Function::new(ctx.clone(), move || self_id)?,
    )?;

    global.set(
        "__process_raw_arguments",
        Function::new(ctx.clone(), move || arguments_json.clone())?,
    )?;

    {
        let channel = channel.clone();
        global.set(
            "__process_spawn",
            Function::new(
                ctx.clone(),
                move |path: String, args_json: Opt<String>| -> ProcessId {
                    let id = channel.allocate_id();
                    channel.pending_spawns.borrow_mut().push(SpawnRequest {
                        id,
                        path,
                        arguments_json: args_json.0,
                    });
                    id
                },
            )?,
        )?;
    }

    {
        let channel = channel.clone();
        global.set(
            "__process_post_message",
            Function::new(
                ctx.clone(),
                move |target: ProcessId, kind: String, data_json: Opt<String>| {
                    channel.pending_sends.borrow_mut().push(Envelope {
                        kind,
                        from: self_id,
                        to: target,
                        data: data_json.0,
                    });
                },
            )?,
        )?;
    }

    {
        let channel = channel.clone();
        global.set(
            "__process_request_exit",
            Function::new(ctx.clone(), move |target: ProcessId| {
                channel.pending_sends.borrow_mut().push(Envelope {
                    kind: "ely:exit".to_string(),
                    from: 0,
                    to: target,
                    data: None,
                });
                channel
                    .pending_control
                    .borrow_mut()
                    .push(Control::ArmDraining {
                        target,
                        by: self_id,
                    });
            })?,
        )?;
    }

    {
        let channel = channel.clone();
        global.set(
            "__process_terminate",
            Function::new(ctx.clone(), move |target: ProcessId| {
                channel.pending_control.borrow_mut().push(Control::Kill {
                    target,
                    by: self_id,
                });
            })?,
        )?;
    }

    {
        let channel = channel.clone();
        global.set(
            "__process_is_live",
            Function::new(ctx.clone(), move |id: ProcessId| channel.is_live(id))?,
        )?;
    }

    {
        let exit_requested = Rc::clone(&exit_requested);
        global.set(
            "__process_exit",
            Function::new(ctx.clone(), move || exit_requested.set(true))?,
        )?;
    }

    // `ely:process` installs the dispatch function when its first message
    // handler is added and clears it (passing nothing) when the last is
    // removed, so `has_message_handler` tracks whether the process still
    // wants messages.
    global.set(
        "__process_set_message_handler",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, handler: Opt<Function<'js>>| {
                *message_handler.borrow_mut() =
                    handler.0.map(|handler| Persistent::save(&ctx, handler));
            },
        )?,
    )?;

    Ok(())
}

/// Minimal JSON string encoder for envelope `kind`s (which are arbitrary
/// developer-chosen text). Avoids pulling `Ctx` into `Envelope::to_json`.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
