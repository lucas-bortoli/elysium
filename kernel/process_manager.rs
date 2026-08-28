//! The kernel's scheduler: one table of processes, driven round-robin once
//! per frame by [`ProcessManager::tick`]. A process that finishes is
//! reaped; one that faults (times out, throws, or exhausts its 16 MB heap)
//! is dropped without taking the kernel down; the kernel exits once the
//! table is empty.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::filesystem;
use crate::framebuffer::DrawCommand;
use crate::input::Input;
use crate::process::{Control, Envelope, ProcessChannel, ProcessId, SpawnRequest};
use crate::runtime::{ElysiumRuntime, GuardedError};

/// How long a process has to wind itself down after `requestExit()` (or the
/// window-close broadcast) before the kernel force-reaps it.
pub const GRACE: Duration = Duration::from_millis(300);

/// Hard ceiling on live processes. A `spawn` past this is rejected and
/// logged. With lazy start a runaway `spawn` chain adds at most one process
/// per frame; this stops it entirely and bounds worst-case memory.
const MAX_PROCESSES: usize = 128;

/// Where a process is in its lifecycle. There is no `Poisoned` state: a
/// faulted process is removed from the table in the same pass, so it is
/// never scheduled again.
enum ProcessState {
    Running,
    /// `requestExit`/broadcast delivered; force-reap once `Instant` passes.
    Draining(Instant),
}

struct ProcessEntry {
    id: ProcessId,
    runtime: ElysiumRuntime,
    mailbox: VecDeque<Envelope>,
    state: ProcessState,
    label: String,
    /// `Some(virtual path)` until the process has taken its first turn and
    /// evaluated its entry module; `None` afterward. Startup — resolving
    /// the path, `eval_module`, post-init handlers — is deferred to that
    /// first turn so `apply_pending` runs no program code.
    pending_path: Option<String>,
    /// Set once we've logged that messages are piling up with no
    /// message handler, so the warning isn't repeated every frame.
    warned_no_handler: bool,
}

pub struct ProcessManager {
    entries: Vec<ProcessEntry>,
    draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
    input: Rc<Input>,
    scale: Rc<Cell<u32>>,
    userland_root: PathBuf,
    channel: ProcessChannel,
}

impl ProcessManager {
    pub fn new(
        draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
        input: Rc<Input>,
        scale: Rc<Cell<u32>>,
        userland_root: PathBuf,
    ) -> Self {
        // Canonicalized once here so `resolve_program`'s sandbox walk (and
        // every VM's, since each `ElysiumRuntime` canonicalizes its own
        // copy too) starts from a real, symlink-free root.
        let userland_root = std::fs::canonicalize(&userland_root).unwrap_or_else(|err| {
            panic!("userland root {} is invalid: {err}", userland_root.display())
        });
        Self {
            entries: Vec::new(),
            draw_commands,
            input,
            scale,
            userland_root,
            channel: ProcessChannel::new(),
        }
    }

    /// True once every process has been reaped — the kernel's cue to exit.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Allocates a process for the userland-virtual entry path
    /// `virtual_path` (e.g. `/programs/init/index.ts`) and adds it to the
    /// table as not-yet-started. Used for the init process and, via
    /// [`Self::apply_pending`], for every `spawn`. The program's code
    /// doesn't run until the process's first [`Self::tick`] turn; the only
    /// way this fails is an internal runtime-construction error, not a
    /// program error.
    pub fn spawn_from_path(
        &mut self,
        virtual_path: &str,
        arguments_json: Option<String>,
    ) -> Result<ProcessId, GuardedError> {
        let id = self.channel.allocate_id();
        match self.allocate_process(id, virtual_path, arguments_json) {
            Ok(()) => Ok(id),
            Err(err) => {
                self.channel.forget(id);
                Err(err)
            }
        }
    }

    /// One round-robin pass over the table. `now` is threaded in (rather
    /// than read from `Instant::now()` here) so tests drive draining
    /// deadlines deterministically.
    pub fn tick(&mut self, now: Instant) {
        let mut i = 0;
        'entries: while i < self.entries.len() {
            let id = self.entries[i].id;

            // 0. On its first turn, a process resolves its entry path,
            // evaluates its module, and runs its post-init handlers. Any
            // failure here — a missing file, a top-level throw, a post-init
            // throw — drops the process through the same path as any other
            // fault.
            if let Some(path) = self.entries[i].pending_path.take() {
                match self.resolve_program(&path) {
                    Ok((name, source)) => {
                        if let Err(err) = self.entries[i].runtime.eval_module(&name, &source) {
                            self.fault(i, "module evaluation", err);
                            continue 'entries;
                        }
                        if let Err(err) = self.entries[i].runtime.run_post_init_handlers() {
                            self.fault(i, "post-init handler", err);
                            continue 'entries;
                        }
                        trace_process(id, &format!("started from {path}"));
                    }
                    Err(err) => {
                        self.fault(i, "startup", err);
                        continue 'entries;
                    }
                }
            }

            // 1. Drain the mailbox, but only once a handler exists.
            if self.entries[i].runtime.has_message_handler() {
                while let Some(envelope) = self.entries[i].mailbox.pop_front() {
                    if let Err(err) = self.entries[i].runtime.deliver_message(&envelope.to_json()) {
                        self.fault(i, "message handler", err);
                        continue 'entries;
                    }
                }
            } else if !self.entries[i].mailbox.is_empty() && !self.entries[i].warned_no_handler {
                self.entries[i].warned_no_handler = true;
                trace_process(id, "messages queued; waiting for a message handler");
            }

            // 2. Run timers due this frame.
            if let Err(err) = self.entries[i].runtime.run_due_timers() {
                self.fault(i, "timer", err);
                continue 'entries;
            }

            // 3. Reap check. A process that has registered a message
            // handler is never "idle" — it has said it wants to keep
            // receiving — so a message-driven process ends only via
            // `exit()`, `requestExit`'s grace, or `terminate`.
            let exiting = self.entries[i].runtime.exit_requested();
            let idle = self.entries[i].mailbox.is_empty()
                && !self.entries[i].runtime.has_message_handler()
                && self.entries[i].runtime.has_no_pending_work();
            let reason = if exiting {
                Some("exited via exit()")
            } else {
                match self.entries[i].state {
                    ProcessState::Draining(_) if idle => Some("drained cleanly"),
                    ProcessState::Draining(deadline) if now >= deadline => {
                        Some("force-reaped after grace")
                    }
                    ProcessState::Running if idle => Some("no pending work"),
                    _ => None,
                }
            };
            if let Some(reason) = reason {
                trace_process(id, &format!("reaped: {reason}"));
                self.remove_at(i);
                continue 'entries;
            }

            i += 1;
        }

        self.apply_pending(now);
    }

    /// Enqueues `ely:exit` to every running process and moves it to
    /// `Draining` — the window-close path. Callers keep pumping [`Self::tick`]
    /// afterward so cooperative processes reap themselves and the rest are
    /// force-reaped at their deadline.
    pub fn broadcast_exit(&mut self, now: Instant) {
        for entry in &mut self.entries {
            if matches!(entry.state, ProcessState::Running) {
                entry.mailbox.push_back(Envelope {
                    kind: "ely:exit".to_string(),
                    from: 0,
                    to: entry.id,
                    data: None,
                });
                entry.state = ProcessState::Draining(now + GRACE);
            }
        }
        trace_process(0, "broadcast ely:exit to all processes");
    }

    /// Applies everything the `ely:process` bindings queued during the pass
    /// just finished: control requests, then spawns, then sends. None of
    /// this runs program code — [`Self::allocate_process`] only builds a
    /// VM — so a spawn can't cascade into more spawns here, and this is a
    /// single straight-line pass. It runs when no process is executing,
    /// which is what makes dropping one here (a `Kill`, a rejected spawn)
    /// safe.
    fn apply_pending(&mut self, now: Instant) {
        for request in self.channel.take_control() {
            match request {
                Control::Kill { target, by } => {
                    if let Some(pos) = self.entries.iter().position(|e| e.id == target) {
                        trace_process(target, &format!("terminated by {by}"));
                        self.remove_at(pos);
                    } else {
                        self.channel.forget(target);
                    }
                }
                Control::ArmDraining { target, by } => {
                    let Some(entry) = self.entries.iter_mut().find(|e| e.id == target) else {
                        continue;
                    };
                    if matches!(entry.state, ProcessState::Running) {
                        entry.state = ProcessState::Draining(now + GRACE);
                        trace_process(target, &format!("exit requested by {by}; draining"));
                    }
                }
            }
        }

        for SpawnRequest {
            id,
            path,
            arguments_json,
        } in self.channel.take_spawns()
        {
            if self.entries.len() >= MAX_PROCESSES {
                trace_process(id, "spawn rejected: process table is full");
                self.channel.forget(id);
                continue;
            }
            if let Err(err) = self.allocate_process(id, &path, arguments_json) {
                trace_process(id, &format!("spawn failed: {}", describe(err)));
                self.channel.forget(id);
            }
        }

        for envelope in self.channel.take_sends() {
            match self.entries.iter_mut().find(|e| e.id == envelope.to) {
                Some(entry) => entry.mailbox.push_back(envelope),
                None => trace_process(envelope.to, "dropped a message for an unknown process"),
            }
        }
    }

    /// Builds a VM for `id` and adds it to the table as not-yet-started.
    /// No program code runs here — the entry module is evaluated on the
    /// process's first [`Self::tick`] turn. The only failure is an internal
    /// runtime-construction error.
    fn allocate_process(
        &mut self,
        id: ProcessId,
        virtual_path: &str,
        arguments_json: Option<String>,
    ) -> Result<(), GuardedError> {
        let runtime = ElysiumRuntime::new(
            Rc::clone(&self.draw_commands),
            Rc::clone(&self.input),
            Rc::clone(&self.scale),
            self.userland_root.clone(),
            id,
            self.channel.clone(),
            arguments_json,
        )
        .map_err(|err| GuardedError::Exception(format!("runtime init failed: {err}")))?;

        self.channel.register(id);
        self.entries.push(ProcessEntry {
            id,
            runtime,
            mailbox: VecDeque::new(),
            state: ProcessState::Running,
            pending_path: Some(virtual_path.to_string()),
            label: virtual_path.to_string(),
            warned_no_handler: false,
        });
        trace_process(id, &format!("allocated for {virtual_path}"));
        Ok(())
    }

    /// Resolves a userland-virtual path to `(real absolute path, source)`
    /// through the same symlink-invisible sandbox walk `ely:filesystem` and
    /// `ely:image` use, so a program path can't escape the userland root or
    /// reach through a symlink. The real path is what `eval_module` needs as
    /// the module name so its relative imports and `import.meta` resolve.
    fn resolve_program(&self, virtual_path: &str) -> Result<(String, String), GuardedError> {
        let resolved = filesystem::resolve_userland_path(&self.userland_root, virtual_path)
            .map_err(|err| {
                GuardedError::Exception(format!("cannot resolve program {virtual_path}: {err}"))
            })?;
        let source = std::fs::read_to_string(&resolved).map_err(|err| {
            GuardedError::Exception(format!("cannot read program {virtual_path}: {err}"))
        })?;
        let name = resolved
            .to_str()
            .ok_or_else(|| GuardedError::Exception("program path is not valid UTF-8".to_string()))?
            .to_string();
        Ok((name, source))
    }

    /// Removes entry `i`, dropping its runtime (whose `Drop` runs the
    /// deterministic VM teardown) and releasing its id.
    fn remove_at(&mut self, i: usize) {
        let entry = self.entries.remove(i);
        self.channel.forget(entry.id);
    }

    /// Logs a process fault and drops it. Timeout and exhausted-heap and
    /// plain uncaught exception all land here — the kernel keeps running.
    fn fault(&mut self, i: usize, phase: &str, err: GuardedError) {
        let id = self.entries[i].id;
        let label = self.entries[i].label.clone();
        match err {
            GuardedError::Timeout => {
                trace_process(id, &format!("dropped: timed out in {phase} ({label})"));
            }
            GuardedError::Exception(message) => {
                trace_process(
                    id,
                    &format!("dropped: uncaught exception in {phase} ({label}): {message}"),
                );
            }
        }
        self.remove_at(i);
    }
}

fn describe(err: GuardedError) -> String {
    match err {
        GuardedError::Timeout => "timed out".to_string(),
        GuardedError::Exception(message) => message,
    }
}

/// One line of process-management tracing to stderr. `id` `0` is the kernel.
fn trace_process(id: ProcessId, event: &str) {
    eprintln!("[process {id}] {event}");
}

#[cfg(test)]
impl ProcessManager {
    /// Reads a `globalThis` value out of process `id`'s VM. Test-only —
    /// the manager deliberately doesn't expose its runtimes otherwise.
    fn eval_in<T>(&self, id: ProcessId, name: &str) -> T
    where
        T: for<'js> rquickjs::FromJs<'js>,
    {
        let entry = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .expect("no such process");
        entry.runtime.eval_in(name).expect("failed to read global")
    }

    fn ids(&self) -> Vec<ProcessId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    fn mailbox_len(&self, id: ProcessId) -> usize {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map_or(0, |e| e.mailbox.len())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::framebuffer::DEFAULT_SCALE;

    /// A private userland root seeded with `programs`, each entry a
    /// `(relative path, source)` pair written to disk.
    fn userland_with(programs: &[(&str, &str)]) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("elysium-process-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (path, source) in programs {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, source).unwrap();
        }
        root
    }

    fn manager(root: PathBuf) -> ProcessManager {
        let scale = Rc::new(Cell::new(DEFAULT_SCALE));
        let input = Rc::new(Input::new(Rc::clone(&scale)));
        ProcessManager::new(Rc::new(RefCell::new(Vec::new())), input, scale, root)
    }

    #[test]
    fn a_program_with_no_pending_work_is_reaped_on_the_first_tick() {
        let root = userland_with(&[("main.ts", "globalThis.ran = true;")]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/main.ts", None).unwrap();
        assert!(!mgr.is_empty());
        mgr.tick(Instant::now());
        assert!(mgr.is_empty(), "idle process should have been reaped");
    }

    #[test]
    fn a_program_with_a_live_interval_is_kept() {
        let root = userland_with(&[("main.ts", "setInterval(() => {}, 1000);")]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/main.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert!(!mgr.is_empty(), "a pending interval is still work");
    }

    #[test]
    fn exit_reaps_a_process_that_still_has_pending_work() {
        let root = userland_with(&[(
            "main.ts",
            "import { exit } from 'ely:process'; setInterval(() => {}, 1000); exit();",
        )]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/main.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert!(mgr.is_empty(), "exit() overrides the pending interval");
    }

    #[test]
    fn a_module_body_runs_only_on_the_first_tick() {
        let root = userland_with(&[(
            "main.ts",
            "globalThis.ran = true; setInterval(() => {}, 1000);",
        )]);
        let mut mgr = manager(root);
        let id = mgr.spawn_from_path("/main.ts", None).unwrap();
        // Allocated but not started: no globals yet.
        assert!(!mgr.is_empty());
        mgr.tick(Instant::now());
        assert!(mgr.eval_in::<bool>(id, "globalThis.ran === true"));
    }

    #[test]
    fn a_bad_path_is_allocated_then_dropped_on_its_first_tick() {
        let root = userland_with(&[("quiet.ts", "setInterval(() => {}, 1000);")]);
        let mut mgr = manager(root);
        // The spawn succeeds — the VM is built — but the missing file is
        // only discovered on the first turn, which drops the process.
        mgr.spawn_from_path("/nope.ts", None).unwrap();
        mgr.spawn_from_path("/quiet.ts", None).unwrap();
        assert_eq!(mgr.ids().len(), 2);
        mgr.tick(Instant::now());
        assert_eq!(mgr.ids().len(), 1, "only the resolvable program survives");
    }

    #[test]
    fn a_top_level_throw_is_dropped_on_the_first_tick_without_touching_its_sibling() {
        let root = userland_with(&[
            ("boom.ts", "throw new Error('boom');"),
            ("quiet.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/boom.ts", None).unwrap();
        mgr.spawn_from_path("/quiet.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert_eq!(mgr.ids().len(), 1, "the thrower is gone, the sibling stays");
    }

    #[test]
    fn a_faulting_timer_drops_only_that_process() {
        let root = userland_with(&[
            (
                "bad.ts",
                "setTimeout(() => { throw new Error('later'); }, 0);",
            ),
            ("good.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/bad.ts", None).unwrap();
        mgr.spawn_from_path("/good.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert_eq!(mgr.ids(), vec![2], "only the good process remains");
    }

    #[test]
    fn a_spawned_child_joins_on_the_next_tick_and_receives_its_arguments() {
        let root = userland_with(&[
            (
                "parent.ts",
                "import { spawn } from 'ely:process'; \
                 globalThis.child = spawn('/child.ts', { hello: 'world' }); \
                 setInterval(() => {}, 1000);",
            ),
            (
                "child.ts",
                "import { currentArguments } from 'ely:process'; \
                 globalThis.args = currentArguments(); \
                 setInterval(() => {}, 1000);",
            ),
        ]);
        let mut mgr = manager(root);
        let parent = mgr.spawn_from_path("/parent.ts", None).unwrap();
        // Tick 1: parent starts and queues the spawn; the child is
        // allocated in apply_pending but hasn't evaluated.
        mgr.tick(Instant::now());
        let child: u32 = mgr.eval_in(parent, "child");
        assert!(mgr.ids().contains(&child));
        // Tick 2: the child takes its first turn and reads its arguments.
        mgr.tick(Instant::now());
        let hello: String = mgr.eval_in(child, "args.hello");
        assert_eq!(hello, "world");
    }

    #[test]
    fn a_ping_gets_a_pong_across_processes() {
        let root = userland_with(&[
            (
                "parent.ts",
                "import { spawn, postMessage, addMessageHandler } from 'ely:process'; \
                 globalThis.reply = null; \
                 const child = spawn('/child.ts', undefined); \
                 addMessageHandler((env) => { globalThis.reply = env; }); \
                 postMessage(child, { kind: 'ping', data: 41 }); \
                 setInterval(() => {}, 1000);",
            ),
            (
                "child.ts",
                "import { postMessage, addMessageHandler } from 'ely:process'; \
                 addMessageHandler((env) => { \
                     if (env.kind === 'ping') { \
                         postMessage(env.from, { kind: 'pong', data: env.data + 1 }); \
                     } \
                 }); \
                 setInterval(() => {}, 1000);",
            ),
        ]);
        let mut mgr = manager(root);
        let parent = mgr.spawn_from_path("/parent.ts", None).unwrap();
        // tick 1: parent queues spawn + send; child installed.
        mgr.tick(Instant::now());
        // tick 2: child receives ping, queues pong.
        mgr.tick(Instant::now());
        // tick 3: parent receives pong.
        mgr.tick(Instant::now());
        let kind: String = mgr.eval_in(parent, "reply.kind");
        let data: f64 = mgr.eval_in(parent, "reply.data");
        assert_eq!(kind, "pong");
        assert_eq!(data, 42.0);
    }

    #[test]
    fn a_message_sent_before_onmessage_is_queued_then_delivered() {
        let root = userland_with(&[
            (
                "parent.ts",
                "import { spawn, postMessage } from 'ely:process'; \
                 const child = spawn('/child.ts', undefined); \
                 postMessage(child, { kind: 'early', data: 7 }); \
                 setInterval(() => {}, 1000);",
            ),
            (
                "child.ts",
                "import { addMessageHandler } from 'ely:process'; \
                 globalThis.got = null; \
                 globalThis.arm = () => addMessageHandler((env) => { globalThis.got = env.data; }); \
                 setInterval(() => {}, 1000);",
            ),
        ]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/parent.ts", None).unwrap();
        mgr.tick(Instant::now()); // spawn + send applied
        let child = *mgr.ids().last().unwrap();
        mgr.tick(Instant::now()); // child has no handler: message stays queued
        assert_eq!(mgr.mailbox_len(child), 1);
        // Arm the handler from outside, then tick again.
        mgr.eval_in::<()>(child, "arm()");
        mgr.tick(Instant::now());
        assert_eq!(mgr.mailbox_len(child), 0);
        let got: f64 = mgr.eval_in(child, "got");
        assert_eq!(got, 7.0);
    }

    #[test]
    fn request_exit_force_reaps_a_process_that_ignores_it() {
        let root = userland_with(&[
            (
                "parent.ts",
                "import { spawn, requestExit } from 'ely:process'; \
                 globalThis.child = spawn('/stubborn.ts', undefined); \
                 globalThis.ask = () => requestExit(globalThis.child); \
                 setInterval(() => {}, 1000);",
            ),
            // Never adds a message handler, keeps a live interval forever.
            ("stubborn.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        let parent = mgr.spawn_from_path("/parent.ts", None).unwrap();
        let start = Instant::now();
        mgr.tick(start);
        let child: u32 = mgr.eval_in(parent, "child");
        assert!(mgr.ids().contains(&child));

        mgr.eval_in::<()>(parent, "ask()"); // queues ArmDraining
        mgr.tick(start); // apply_pending arms Draining(start + GRACE)
        assert!(mgr.ids().contains(&child), "still within grace");

        mgr.tick(start + GRACE + Duration::from_millis(1));
        assert!(!mgr.ids().contains(&child), "force-reaped after grace");
    }

    #[test]
    fn terminate_drops_the_target_at_the_end_of_the_tick() {
        let root = userland_with(&[
            (
                "parent.ts",
                "import { spawn, terminate } from 'ely:process'; \
                 globalThis.child = spawn('/victim.ts', undefined); \
                 globalThis.kill = () => terminate(globalThis.child); \
                 setInterval(() => {}, 1000);",
            ),
            ("victim.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        let parent = mgr.spawn_from_path("/parent.ts", None).unwrap();
        mgr.tick(Instant::now());
        let child: u32 = mgr.eval_in(parent, "child");
        assert!(mgr.ids().contains(&child));
        mgr.eval_in::<()>(parent, "kill()");
        mgr.tick(Instant::now());
        assert!(!mgr.ids().contains(&child));
        assert!(mgr.ids().contains(&parent), "parent is untouched");
    }

    #[test]
    fn a_message_driven_process_stays_alive_then_drains_cleanly_on_ely_exit() {
        let root = userland_with(&[
            (
                "parent.ts",
                "import { spawn, requestExit } from 'ely:process'; \
                 globalThis.child = spawn('/server.ts', undefined); \
                 globalThis.ask = () => requestExit(globalThis.child); \
                 setInterval(() => {}, 1000);",
            ),
            // No timers at all — kept alive only by its message handler,
            // and exits cooperatively when asked.
            (
                "server.ts",
                "import { addMessageHandler, exit } from 'ely:process'; \
                 addMessageHandler((env) => { if (env.kind === 'ely:exit') exit(); });",
            ),
        ]);
        let mut mgr = manager(root);
        let parent = mgr.spawn_from_path("/parent.ts", None).unwrap();
        let start = Instant::now();
        mgr.tick(start);
        let child: u32 = mgr.eval_in(parent, "child");
        assert!(
            mgr.ids().contains(&child),
            "handler-only process is kept alive"
        );

        mgr.eval_in::<()>(parent, "ask()");
        mgr.tick(start); // delivers ely:exit; server calls exit()
        mgr.tick(start); // reap check collects it — well before the grace deadline
        assert!(!mgr.ids().contains(&child));
    }

    #[test]
    fn post_message_to_a_dead_id_throws_process_not_found() {
        let root = userland_with(&[(
            "main.ts",
            "import { postMessage, ProcessNotFoundError } from 'ely:process'; \
             setInterval(() => {}, 1000); \
             globalThis.correct = false; \
             try { postMessage(9999, { kind: 'x', data: undefined }); } \
             catch (err) { globalThis.correct = err instanceof ProcessNotFoundError; }",
        )]);
        let mut mgr = manager(root);
        let id = mgr.spawn_from_path("/main.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert!(mgr.eval_in::<bool>(id, "correct"));
    }

    #[test]
    fn post_message_with_a_reserved_kind_throws() {
        let root = userland_with(&[
            (
                "main.ts",
                "import { spawn, postMessage, ReservedMessageKindError } from 'ely:process'; \
             const child = spawn('/child.ts', undefined); \
             setInterval(() => {}, 1000); \
             globalThis.correct = false; \
             try { postMessage(child, { kind: 'ely:exit', data: undefined }); } \
             catch (err) { globalThis.correct = err instanceof ReservedMessageKindError; }",
            ),
            ("child.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        let id = mgr.spawn_from_path("/main.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert!(mgr.eval_in::<bool>(id, "correct"));
    }

    #[test]
    fn the_process_table_is_capped() {
        let root = userland_with(&[(
            "swarm.ts",
            "import { spawn } from 'ely:process'; \
             for (let i = 0; i < 500; i++) spawn('/swarm.ts', undefined); \
             setInterval(() => {}, 1000);",
        )]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/swarm.ts", None).unwrap();
        // Tick 1 starts the spawner, which queues 500 spawns; apply_pending
        // fills the table to the cap and rejects the rest.
        mgr.tick(Instant::now());
        assert_eq!(mgr.ids().len(), MAX_PROCESSES);
        // The rejected ids are not live, so messaging them would throw.
        assert!(!mgr.channel.is_live(9_999));
    }

    #[test]
    fn a_self_spawning_chain_grows_one_per_tick_and_plateaus_at_the_cap() {
        let root = userland_with(&[(
            "fork.ts",
            "import { spawn } from 'ely:process'; \
             spawn('/fork.ts', undefined); \
             setInterval(() => {}, 1000);",
        )]);
        let mut mgr = manager(root);
        mgr.spawn_from_path("/fork.ts", None).unwrap();

        mgr.tick(Instant::now()); // spawner starts, queues one child
        assert_eq!(mgr.ids().len(), 2);
        mgr.tick(Instant::now()); // child starts, queues one grandchild
        assert_eq!(mgr.ids().len(), 3);
        mgr.tick(Instant::now());
        assert_eq!(mgr.ids().len(), 4);

        for _ in 0..MAX_PROCESSES + 10 {
            mgr.tick(Instant::now());
        }
        assert_eq!(mgr.ids().len(), MAX_PROCESSES, "growth stops at the cap");
    }
}
