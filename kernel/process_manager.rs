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

use crate::framebuffer::DrawCommand;
use crate::input::Input;
use crate::process::{Control, Envelope, ProcessChannel, ProcessId, SpawnRequest};
use crate::runtime::{ElysiumRuntime, GuardedError};

/// How long a process has to wind itself down after `requestExit()` (or the
/// window-close broadcast) before the kernel force-reaps it.
pub const GRACE: Duration = Duration::from_millis(300);

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

    /// Starts a process from a userland-virtual entry path (e.g.
    /// `/programs/init/index.ts`). Used for the init process and, via
    /// [`Self::apply_pending`], for every `spawn`. On failure nothing is
    /// added to the table and the id is released.
    pub fn spawn_from_path(
        &mut self,
        virtual_path: &str,
        arguments_json: Option<String>,
    ) -> Result<ProcessId, GuardedError> {
        let id = self.channel.allocate_id();
        match self.install(id, virtual_path, arguments_json) {
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
    /// just finished. Control requests and spawns are drained in a loop:
    /// installing a process runs its module body, which can itself spawn or
    /// terminate, and those need to settle in the same frame so a child
    /// spawned mid-install — and any message sent to it — resolves now
    /// rather than a frame late. Sends are delivered only once every spawn
    /// has landed, so their targets exist. All of this runs when no
    /// process is executing, which is what makes dropping one here safe.
    fn apply_pending(&mut self, now: Instant) {
        /// Guards against a process that spawns on every install pinning
        /// the kernel inside this loop.
        const MAX_INSTALLS_PER_FRAME: usize = 1024;
        let mut installed = 0;

        loop {
            let control = self.channel.take_control();
            let spawns = self.channel.take_spawns();
            if control.is_empty() && spawns.is_empty() {
                break;
            }

            for request in control {
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
            } in spawns
            {
                if installed >= MAX_INSTALLS_PER_FRAME {
                    trace_process(id, "spawn skipped: too many spawns in one frame");
                    self.channel.forget(id);
                    continue;
                }
                installed += 1;
                if let Err(err) = self.install(id, &path, arguments_json) {
                    trace_process(id, &format!("spawn failed: {}", describe(err)));
                    self.channel.forget(id);
                }
            }
        }

        for envelope in self.channel.take_sends() {
            match self.entries.iter_mut().find(|e| e.id == envelope.to) {
                Some(entry) => entry.mailbox.push_back(envelope),
                None => trace_process(envelope.to, "dropped a message for an unknown process"),
            }
        }
    }

    /// Builds a runtime for `id`, evaluates its entry module, runs its
    /// post-init handlers, and — only if all of that succeeds — adds it to
    /// the table.
    fn install(
        &mut self,
        id: ProcessId,
        virtual_path: &str,
        arguments_json: Option<String>,
    ) -> Result<(), GuardedError> {
        let (name, source) = self.resolve_program(virtual_path)?;

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

        runtime.eval_module(&name, &source)?;
        runtime.run_post_init_handlers()?;

        self.channel.register(id);
        self.entries.push(ProcessEntry {
            id,
            runtime,
            mailbox: VecDeque::new(),
            state: ProcessState::Running,
            label: virtual_path.to_string(),
            warned_no_handler: false,
        });
        trace_process(id, &format!("created from {virtual_path}"));
        Ok(())
    }

    /// Resolves a userland-virtual path to `(real absolute path, source)`,
    /// rejecting anything that escapes the userland root — the same
    /// sandbox boundary `ely:image`'s `loadImage` enforces. The real path
    /// is what `eval_module` needs as the module name so its relative
    /// imports and `import.meta` resolve.
    fn resolve_program(&self, virtual_path: &str) -> Result<(String, String), GuardedError> {
        let relative = virtual_path.trim_start_matches('/');
        let joined = self.userland_root.join(relative);
        let canonical = std::fs::canonicalize(&joined).map_err(|err| {
            GuardedError::Exception(format!("cannot resolve program {virtual_path}: {err}"))
        })?;
        if !canonical.starts_with(&self.userland_root) {
            return Err(GuardedError::Exception(format!(
                "program {virtual_path} resolves outside the userland root"
            )));
        }
        let source = std::fs::read_to_string(&canonical).map_err(|err| {
            GuardedError::Exception(format!("cannot read program {virtual_path}: {err}"))
        })?;
        let name = canonical
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
    fn spawn_from_a_bad_path_errors_and_adds_nothing() {
        let mut mgr = manager(userland_with(&[]));
        let result = mgr.spawn_from_path("/nope.ts", None);
        assert!(result.is_err());
        assert!(mgr.is_empty());
    }

    #[test]
    fn a_throwing_program_is_dropped_without_touching_its_sibling() {
        let root = userland_with(&[
            ("boom.ts", "throw new Error('boom');"),
            ("quiet.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        // boom fails during install (module eval), so it never enters the
        // table; quiet stays.
        assert!(mgr.spawn_from_path("/boom.ts", None).is_err());
        mgr.spawn_from_path("/quiet.ts", None).unwrap();
        mgr.tick(Instant::now());
        assert_eq!(mgr.ids().len(), 1);
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
        // First tick: parent runs, queues the spawn; child installed in
        // apply_pending but not yet ticked.
        mgr.tick(Instant::now());
        let child: u32 = mgr.eval_in(parent, "child");
        assert!(mgr.ids().contains(&child));
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
             globalThis.correct = false; \
             try { postMessage(9999, { kind: 'x', data: undefined }); } \
             catch (err) { globalThis.correct = err instanceof ProcessNotFoundError; }",
        )]);
        let mut mgr = manager(root);
        let id = mgr.spawn_from_path("/main.ts", None).unwrap();
        assert!(mgr.eval_in::<bool>(id, "correct"));
    }

    #[test]
    fn post_message_with_a_reserved_kind_throws() {
        let root = userland_with(&[
            (
                "main.ts",
                "import { spawn, postMessage, ReservedMessageKindError } from 'ely:process'; \
             const child = spawn('/child.ts', undefined); \
             globalThis.correct = false; \
             try { postMessage(child, { kind: 'ely:exit', data: undefined }); } \
             catch (err) { globalThis.correct = err instanceof ReservedMessageKindError; }",
            ),
            ("child.ts", "setInterval(() => {}, 1000);"),
        ]);
        let mut mgr = manager(root);
        let id = mgr.spawn_from_path("/main.ts", None).unwrap();
        assert!(mgr.eval_in::<bool>(id, "correct"));
    }
}
