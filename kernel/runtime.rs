use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::function::Rest;
use rquickjs::{
    Context, Ctx, Error, Function, Module, Persistent, Result, Runtime as JsRuntime, Type, Value,
};

use crate::esm_resolver::{
    CompilingLoader, EmbeddedOrFileResolver, bootstrap_jsx_runtime, set_virtual_import_meta,
};
use crate::filesystem;
use crate::framebuffer::{self, DrawCommand};
use crate::image::{self, ImageTable};
use crate::input::{self, Input};
use crate::process::{self, ProcessChannel, ProcessId};
use crate::timers::{TimerArgs, TimerQueue, bootstrap_timers};
use crate::transform;

/// Deadline bookkeeping shared between `ElysiumRuntime` and the interrupt
/// handler closure installed on its `JsRuntime`. `Rc`/`Cell` (not
/// `Arc`/`Mutex`/atomics) are enough since nothing here crosses an OS thread
/// boundary and the crate's `"parallel"` feature isn't enabled.
#[derive(Default)]
struct GuardState {
    /// `None` when no guarded call is in progress; `Some(deadline)` = the
    /// absolute instant the current call must finish by.
    deadline: Cell<Option<Instant>>,
    /// Set by the interrupt handler itself, the moment *it* decides to
    /// interrupt — the ground-truth signal `run_guarded` uses to tell "this
    /// failure was a timeout" apart from "the program threw". Needed because
    /// rquickjs doesn't distinguish the two at the type level: both come
    /// back as the same `Error::Exception`.
    fired: Cell<bool>,
}

/// The two ways a guarded call into the VM can fail. On `Timeout`, per
/// Elysium's failure contract, the VM is destroyed but Elysium soldiers on:
/// the owning `ElysiumRuntime` is poisoned and must be dropped, never
/// reused, while the caller (kernel) continues running everything else. An
/// `Exception` is just an ordinary uncaught error and carries no such
/// requirement.
#[derive(Debug)]
pub enum GuardedError {
    Timeout,
    Exception(String),
}

/// Budget for one top-level module evaluation. Generous relative to
/// [`FRAME_BUDGET`] since program initialization can legitimately take
/// longer than a single frame.
const DEFAULT_EVAL_BUDGET: Duration = Duration::from_secs(5);

/// Budget for one per-frame callback (`update`/`draw`) — tight, since a
/// program's whole frame budget (kernel included) is a single-digit number
/// of milliseconds at any reasonable frame rate.
const FRAME_BUDGET: Duration = Duration::from_millis(16);

pub struct ElysiumRuntime {
    /// This and the field below hold [`Persistent`] JS values, which must be
    /// dropped while `js_runtime` is still alive to free their underlying
    /// GC-tracked values — struct fields drop in declaration order, so these
    /// are declared, and therefore dropped, before `context`/`js_runtime`
    /// below.
    ///
    /// Pending `setTimeout`/`setInterval`/`setImmediate`/
    /// `requestAnimationFrame` timers, checked each frame by
    /// [`Self::run_due_timers`].
    timers: Rc<TimerQueue>,
    /// Callbacks queued by `queueMicrotask`, flushed by [`Self::drain_microtasks`].
    microtasks: Rc<RefCell<Vec<Persistent<Function<'static>>>>>,
    /// Callbacks registered by `ely:lifecycle`'s `addPostInitHandler`, run
    /// once by [`Self::run_post_init_handlers`].
    post_init_handlers: Rc<RefCell<Vec<Persistent<Function<'static>>>>>,
    /// The single dispatch callback `ely:process`'s `addMessageHandler` installs,
    /// invoked by [`Self::deliver_message`] for each queued envelope. Holds
    /// a `Persistent`, so it belongs with the fields above that must drop
    /// before `context`/`js_runtime`.
    message_handler: Rc<RefCell<Option<Persistent<Function<'static>>>>>,
    /// Images loaded by `ely:image`'s `loadImage`, keyed by id. Holds no
    /// `Persistent` JS values (unlike the three fields above), so it's not
    /// subject to the same GC-sweep-ordering hazard — grouped with them in
    /// `Drop` purely so VM teardown has one obvious place every resource
    /// gets released.
    images: Rc<ImageTable>,
    guard: Rc<GuardState>,
    /// Canonicalized once in [`Self::new`]; shared by `ely:image`'s
    /// `loadImage` (which resolves an absolute virtual path against it) and
    /// by [`Self::eval_module`] (which uses it to give the entry module's
    /// own `import.meta.directoryName`/`fileName` a virtual identity, the same
    /// way [`CompilingLoader`] does for every module it loads afterward).
    userland_root: PathBuf,
    /// Set by `ely:process`'s `exit()`; read by the manager's reap check.
    exit_requested: Rc<Cell<bool>>,
    context: Context,
    js_runtime: JsRuntime,
}

impl ElysiumRuntime {
    /// `draw_commands` is the Framebuffer device's shared draw-command buffer:
    /// `ely:framebuffer`'s hidden globals push onto it directly rather than
    /// touching any drawing state themselves, keeping the VM's own bindings
    /// ignorant of how frames actually get rasterized. `input` is the
    /// Input device's shared pointer state, updated from raw window events
    /// and read by `ely:input`'s hidden globals. `scale` is the
    /// Framebuffer's shared physical-pixels-per-logical-pixel setting;
    /// `ely:framebuffer`'s `setScale` writes to it directly, the same way
    /// `draw_commands` is written to. `userland_root` is the root of the
    /// whole userland tree, shared by every program — `ely:image`'s
    /// `loadImage` resolves an absolute virtual path against it and can
    /// never escape it, regardless of the process's actual working
    /// directory, and every module's `import.meta.directoryName`/`fileName` is
    /// expressed relative to this same root.
    pub fn new(
        draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
        input: Rc<Input>,
        scale: Rc<Cell<u32>>,
        userland_root: PathBuf,
        self_id: ProcessId,
        channel: ProcessChannel,
        arguments_json: Option<String>,
    ) -> Result<Self> {
        let userland_root = std::fs::canonicalize(&userland_root).unwrap_or_else(|err| {
            panic!(
                "userland root {} is invalid: {err}",
                userland_root.display()
            )
        });

        let js_runtime = JsRuntime::new()?;
        // Every process is capped at 16 MB of QuickJS heap; a program that
        // blows past it fails allocation, surfaces as an ordinary uncaught
        // exception, and the manager drops just that process.
        js_runtime.set_memory_limit(16 * 1024 * 1024);

        let guard = Rc::new(GuardState::default());
        {
            let guard = Rc::clone(&guard);
            js_runtime.set_interrupt_handler(Some(Box::new(move || match guard.deadline.get() {
                Some(deadline) if Instant::now() >= deadline => {
                    guard.fired.set(true);
                    true
                }
                _ => false,
            })));
        }

        // Programs are TS(X) files on disk; `import`/`export` resolve to
        // sibling `.ts`/`.tsx` files, each compiled (JSX -> h(), then TS
        // erased) as it's loaded. A bare specifier matching one of
        // EMBEDDED_RUNTIME_MODULES resolves to that embedded source instead.
        js_runtime.set_loader(
            EmbeddedOrFileResolver::new(userland_root.clone()),
            CompilingLoader::new(userland_root.clone()),
        );

        let context = Context::full(&js_runtime)?;

        let timers = Rc::new(TimerQueue::new());
        let microtasks = Rc::new(RefCell::new(Vec::new()));
        let post_init_handlers = Rc::new(RefCell::new(Vec::new()));
        let message_handler = Rc::new(RefCell::new(None));
        let exit_requested = Rc::new(Cell::new(false));
        let images = Rc::new(ImageTable::new());

        context.with(|ctx| -> Result<()> {
            let global = ctx.globals();
            global.set("print", Function::new(ctx.clone(), print)?)?;
            bootstrap_jsx_runtime(&ctx)?;
            framebuffer::bootstrap_framebuffer_bindings(
                &ctx,
                draw_commands,
                scale,
                Rc::clone(&images),
            )?;
            input::bootstrap_input_bindings(&ctx, input)?;
            bootstrap_timers(&ctx, Rc::clone(&timers), Rc::clone(&microtasks))?;
            bootstrap_post_init_handlers(&ctx, Rc::clone(&post_init_handlers))?;
            image::bootstrap_image_bindings(&ctx, Rc::clone(&images), userland_root.clone())?;
            filesystem::bootstrap_filesystem_bindings(&ctx, userland_root.clone())?;
            process::bootstrap_process_bindings(
                &ctx,
                self_id,
                channel,
                Rc::clone(&message_handler),
                Rc::clone(&exit_requested),
                arguments_json,
            )?;
            Ok(())
        })?;

        Ok(Self {
            js_runtime,
            context,
            guard,
            timers,
            microtasks,
            post_init_handlers,
            message_handler,
            userland_root,
            exit_requested,
            images,
        })
    }

    /// Compiles and evaluates `source` as an ES module named `name` (its
    /// path, used as the base for resolving any relative imports it has).
    /// Runs purely for its side effects — a program registers whatever
    /// per-frame work it wants (`ely:lifecycle`'s `addUpdateTicker`,
    /// `ely:framebuffer`'s `addDrawHandler`) during evaluation, plus
    /// whatever it wants deferred to after evaluation via
    /// `addPostInitHandler` (see [`Self::run_post_init_handlers`]).
    /// `transform::compile` already rejects top-level `await` outright, but
    /// since evaluation still blocks on this module's own completion
    /// promise, `finish` is remapped to a clearer message on the rare
    /// rquickjs-level deadlock it exists to prevent.
    pub fn eval_module(&self, name: &str, source: &str) -> std::result::Result<(), GuardedError> {
        let compiled = transform::compile(source).map_err(GuardedError::Exception)?;

        self.run_guarded(DEFAULT_EVAL_BUDGET, |ctx| {
            let module = Module::declare(ctx.clone(), name, compiled)?;
            set_virtual_import_meta(&module, name, &self.userland_root)?;
            let (_module, promise) = module.eval()?;
            promise.finish::<()>()
        })
        .map_err(remap_deadlock_error)
    }

    /// Runs every callback registered by `ely:lifecycle`'s
    /// `addPostInitHandler` exactly once, in registration order, draining
    /// microtasks after each — the same discipline [`Self::run_due_timers`]
    /// follows. Called once, right after [`Self::eval_module`] succeeds and
    /// before the frame loop (and therefore timers) starts running, so a
    /// handler can safely do timer-dependent work a top-level `await`
    /// cannot.
    pub fn run_post_init_handlers(&self) -> std::result::Result<(), GuardedError> {
        let handlers = self.post_init_handlers.borrow_mut().split_off(0);
        for handler in handlers {
            let result = self.run_guarded(FRAME_BUDGET, |ctx| {
                let handler = handler.restore(ctx)?;
                handler.call::<_, ()>(())
            });
            self.drain_microtasks();
            result?;
        }
        Ok(())
    }

    /// Runs every timer (`setTimeout`/`setInterval`/`setImmediate`/
    /// `requestAnimationFrame`) due as of now, draining pending microtasks
    /// after each one — the same "run a task, then drain microtasks to
    /// completion" discipline every callback into the VM follows.
    /// `setInterval` timers are rescheduled for their next firing after
    /// running, unless their own callback cleared them.
    pub fn run_due_timers(&self) -> std::result::Result<(), GuardedError> {
        let now = Instant::now();
        for id in self.timers.due_ids(now) {
            let Some((callback, args, interval)) = self.timers.prepare_run(id) else {
                continue;
            };

            let result = self.run_guarded(FRAME_BUDGET, |ctx| {
                let callback = callback.restore(ctx)?;
                match args {
                    TimerArgs::User(args) => {
                        let args = args
                            .into_iter()
                            .map(|a| a.restore(ctx))
                            .collect::<Result<Vec<Value>>>()?;
                        callback.call::<_, ()>((Rest(args),))
                    }
                    TimerArgs::AnimationFrameTimestamp => {
                        callback.call::<_, ()>((self.timers.elapsed_seconds(),))
                    }
                }
            });

            if let Some(period) = interval {
                self.timers.reschedule_if_still_active(id, now + period);
            }

            self.drain_microtasks();

            if let Err(err) = result {
                return Err(err);
            }
        }
        Ok(())
    }

    /// Whether `ely:process`'s `exit()` has been called from inside this
    /// process. The manager reaps a process the turn after it sets this.
    pub fn exit_requested(&self) -> bool {
        self.exit_requested.get()
    }

    /// Whether `ely:process`'s `addMessageHandler` has registered a dispatch
    /// callback yet. The manager holds a process's mailbox undrained until
    /// it has, so messages sent before a handler is added aren't lost.
    pub fn has_message_handler(&self) -> bool {
        self.message_handler.borrow().is_some()
    }

    /// Whether this process has nothing left to do: no pending timers, no
    /// queued microtasks, and no unsettled Promise jobs. A process in this
    /// state (with an empty mailbox) is reaped. A Promise that never
    /// resolves leaves no pending job, so a process blocked only on one is
    /// considered idle — see `Documentation/Multitasking.md`.
    pub fn has_no_pending_work(&self) -> bool {
        self.timers.is_empty()
            && self.microtasks.borrow().is_empty()
            && !self.js_runtime.is_job_pending()
    }

    /// Delivers one message envelope (already serialized by
    /// [`crate::process::Envelope::to_json`]) to this process's registered
    /// message dispatch callback, draining microtasks after — the same
    /// discipline [`Self::run_due_timers`] follows. A no-op if no handler
    /// is registered; callers gate on [`Self::has_message_handler`] so that
    /// case leaves the message queued instead.
    pub fn deliver_message(&self, envelope_json: &str) -> std::result::Result<(), GuardedError> {
        let result = self.run_guarded(FRAME_BUDGET, |ctx| {
            let Some(handler) = self.message_handler.borrow().clone() else {
                return Ok(());
            };
            let handler = handler.restore(ctx)?;
            let envelope = ctx.json_parse(envelope_json.as_bytes().to_vec())?;
            handler.call::<_, ()>((envelope,))
        });
        self.drain_microtasks();
        result
    }

    /// Runs every callback queued by `queueMicrotask` (including any that
    /// queue further callbacks of their own), then drains rquickjs's own
    /// Promise job queue. Called after every timer callback so a program's
    /// `.then()` chains and `queueMicrotask` calls observe results in the
    /// same tick they should.
    pub fn drain_microtasks(&self) {
        loop {
            let next = {
                let mut microtasks = self.microtasks.borrow_mut();
                if microtasks.is_empty() {
                    None
                } else {
                    Some(microtasks.remove(0))
                }
            };
            let Some(callback) = next else { break };
            if let Err(err) = self.run_guarded(FRAME_BUDGET, |ctx| {
                let callback = callback.restore(ctx)?;
                callback.call::<_, ()>(())
            }) {
                match err {
                    GuardedError::Timeout => eprintln!("program timed out inside queueMicrotask()"),
                    GuardedError::Exception(err) => {
                        eprintln!("uncaught exception in queueMicrotask(): {err}")
                    }
                }
            }
        }

        loop {
            match self.js_runtime.execute_pending_job() {
                Ok(false) => break,
                Ok(true) => continue,
                Err(job_exception) => eprintln!("uncaught (in promise): {job_exception}"),
            }
        }
    }

    /// The one entry point every call into this VM goes through. Sets a
    /// deadline before running `f`, clears it after, and — on failure —
    /// distinguishes "the interrupt handler fired" (`Timeout`) from any
    /// other thrown/returned error (`Exception`) via [`GuardState::fired`].
    fn run_guarded<T>(
        &self,
        budget: Duration,
        f: impl FnOnce(&Ctx<'_>) -> Result<T>,
    ) -> std::result::Result<T, GuardedError> {
        self.guard.fired.set(false);
        self.guard.deadline.set(Some(Instant::now() + budget));

        let result = self
            .context
            .with(|ctx| -> std::result::Result<T, GuardedError> {
                f(&ctx).map_err(|err| {
                    if self.guard.fired.get() {
                        GuardedError::Timeout
                    } else {
                        GuardedError::Exception(describe_exception(&ctx, err))
                    }
                })
            });

        self.guard.deadline.set(None);
        result
    }
}

#[cfg(test)]
impl ElysiumRuntime {
    /// Evaluates `source` as a plain (non-module) script in this VM and
    /// converts the result. Test-only inspection hook.
    pub(crate) fn eval_in<T>(&self, source: &str) -> Result<T>
    where
        T: for<'js> rquickjs::FromJs<'js>,
    {
        self.context.with(|ctx| ctx.eval::<T, _>(source))
    }
}

impl Drop for ElysiumRuntime {
    /// Releases every `Persistent` value this VM's timer/microtask
    /// machinery is still holding, deterministically, before the natural
    /// field-by-field drop starts tearing down `context`/`js_runtime`.
    /// `timers` and `microtasks` are also captured (as `Rc` clones) inside
    /// the `setTimeout`/`queueMicrotask`/etc. closures registered as
    /// globals, so without this, a `Persistent` left in either one only
    /// gets freed once those closures themselves are freed — which happens
    /// from inside a native-closure finalizer during `JS_FreeRuntime`'s own
    /// GC sweep, a context QuickJS's internal bookkeeping doesn't tolerate
    /// freeing further values from. Running this through an ordinary
    /// `context.with` first avoids that entirely.
    fn drop(&mut self) {
        self.context.with(|_ctx| {
            self.timers.clear_all();
            self.microtasks.borrow_mut().clear();
            *self.message_handler.borrow_mut() = None;
            self.images.clear_all();
        });
    }
}

/// Host binding for `print(...values)`: writes any number of JS values,
/// space-separated, to stdout.
/// Registers `__add_post_init_handler` (wrapped by `ely:lifecycle`'s
/// `addPostInitHandler`) as a global that appends onto `handlers`, mirroring
/// `bootstrap_timers`' `queueMicrotask` registration.
fn bootstrap_post_init_handlers<'js>(
    ctx: &Ctx<'js>,
    handlers: Rc<RefCell<Vec<Persistent<Function<'static>>>>>,
) -> Result<()> {
    ctx.globals().set(
        "__add_post_init_handler",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, handler: Function<'js>| {
            handlers.borrow_mut().push(Persistent::save(&ctx, handler));
        })?,
    )
}

fn print<'js>(ctx: Ctx<'js>, values: Rest<Value<'js>>) -> Result<()> {
    let line = values
        .0
        .into_iter()
        .map(|v| describe_value(&ctx, v))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    println!("{line}");
    Ok(())
}

/// Formats any JS value for `print()`. Strings are written as-is (no
/// quoting); most other values go through `JSON.stringify` with 2-space
/// indentation, which covers numbers, booleans, `null`, arrays, and plain
/// objects. Values JSON can't represent (`undefined`, functions, symbols) or
/// that fail to stringify (circular references, bigints) fall back to a
/// short placeholder rather than erroring the whole call.
fn describe_value<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Result<String> {
    Ok(match value.type_of() {
        Type::String => value.get::<String>()?,
        Type::Undefined | Type::Uninitialized => "undefined".to_string(),
        Type::Function | Type::Constructor => "[Function]".to_string(),
        Type::Symbol => "[Symbol]".to_string(),
        _ => match ctx.json_stringify_replacer_space(value, Value::new_null(ctx.clone()), 2) {
            Ok(Some(json)) => json.to_string()?,
            Ok(None) => "undefined".to_string(),
            Err(_) => "[unprintable value]".to_string(),
        },
    })
}

fn describe_exception(ctx: &Ctx<'_>, err: Error) -> String {
    if let Error::Exception = err {
        ctx.catch().as_exception().unwrap().to_string()
    } else {
        err.to_string()
    }
}

/// `transform::compile` already rejects top-level `await` at compile time,
/// but as a fallback, catches rquickjs's own deadlock exception here too —
/// the only signal available, since rquickjs doesn't distinguish this case
/// at the type level (same reasoning as `GuardedError`'s `Timeout` vs
/// `Exception` split above) — and remaps it to something that actually
/// tells the program author what to do about it.
fn remap_deadlock_error(err: GuardedError) -> GuardedError {
    match err {
        GuardedError::Exception(message) if message.contains("dead lock") => {
            GuardedError::Exception(
                "top-level await only supports work that resolves synchronously during module \
                 evaluation (e.g. an already-resolved promise) — timers, tickers, and draw \
                 handlers aren't running yet. Use `addPostInitHandler` from `ely:lifecycle` to \
                 defer this to after initialization."
                    .to_string(),
            )
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::FromJs;

    use super::*;

    /// A fresh VM with no framebuffer bindings exercised, entry module
    /// already evaluated from `source`. Test programs read out results
    /// through `globalThis`, since assigning there is the simplest way for
    /// a plain script body to leave something this helper can inspect
    /// afterward.
    fn eval(source: &str) -> ElysiumRuntime {
        eval_with_input(source).0
    }

    /// Like [`eval`], but also hands back the `Input` device backing the
    /// VM, so a test can feed it window events before/after evaluating.
    fn eval_with_input(source: &str) -> (ElysiumRuntime, Rc<Input>) {
        eval_named_with_input("test.ts", source)
    }

    /// Like [`eval_with_input`], but lets a test pick the entry module's own
    /// name — needed to exercise `import.meta.directoryName`/`fileName`, since
    /// those are only set when `name` canonicalizes to somewhere inside
    /// [`test_userland_root`].
    fn eval_named_with_input(name: &str, source: &str) -> (ElysiumRuntime, Rc<Input>) {
        let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
        let input = Rc::new(Input::new(Rc::clone(&scale)));
        let runtime = ElysiumRuntime::new(
            Rc::new(RefCell::new(Vec::new())),
            Rc::clone(&input),
            scale,
            test_userland_root(),
            0,
            ProcessChannel::detached(),
            None,
        )
        .expect("failed to construct runtime");
        runtime
            .eval_module(name, source)
            .expect("module failed to evaluate");
        (runtime, input)
    }

    /// `loadImage`'s `userland_root` for every test in this module — a fixed
    /// fixtures directory holding a small, real PNG (`test.png`) and a small
    /// real module (`meta_module.ts`) used to exercise
    /// `import.meta.directoryName`/`fileName`; `kernel/framebuffer.rs`, two levels
    /// up, is a real file outside this directory a path-traversal test can
    /// point at.
    fn test_userland_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("kernel/image/fixtures")
    }

    /// A fresh, writable, uniquely-named directory under the OS temp dir,
    /// for `ely:filesystem` tests that write/delete instead of only
    /// reading. `test_userland_root` is a single directory checked into
    /// git and shared by every test in this module — fine for read-only
    /// use, but `cargo test` runs tests concurrently with no locking, so a
    /// mutating test needs its own root to avoid racing another test.
    fn test_scratch_root() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "elysium-filesystem-test-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("failed to create scratch root");
        dir
    }

    /// Like [`eval`], but against `root` instead of [`test_userland_root`] —
    /// needed by `ely:filesystem` tests that mutate the filesystem, which
    /// must run against a private [`test_scratch_root`].
    fn eval_with_root(root: std::path::PathBuf, source: &str) -> ElysiumRuntime {
        let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
        let input = Rc::new(Input::new(Rc::clone(&scale)));
        let runtime = ElysiumRuntime::new(
            Rc::new(RefCell::new(Vec::new())),
            Rc::clone(&input),
            scale,
            root,
            0,
            ProcessChannel::detached(),
            None,
        )
        .expect("failed to construct runtime");
        runtime
            .eval_module("test.ts", source)
            .expect("module failed to evaluate");
        runtime
    }

    fn global<T>(runtime: &ElysiumRuntime, name: &str) -> T
    where
        T: for<'js> FromJs<'js>,
    {
        runtime
            .context
            .with(|ctx| ctx.globals().get::<_, T>(name))
            .expect("failed to read global")
    }

    #[test]
    fn set_timeout_fires_once_due() {
        let runtime =
            eval("globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 0);");
        assert!(!global::<bool>(&runtime, "fired"), "fired before due");
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "fired"), "fired after due");
    }

    #[test]
    fn set_timeout_does_not_fire_before_its_delay() {
        let runtime = eval(
            "globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 60_000);",
        );
        runtime.run_due_timers().unwrap();
        assert!(!global::<bool>(&runtime, "fired"));
    }

    #[test]
    fn clear_timeout_prevents_firing() {
        let runtime = eval(
            "globalThis.fired = false; \
             const id = setTimeout(() => { globalThis.fired = true; }, 0); \
             clearTimeout(id);",
        );
        runtime.run_due_timers().unwrap();
        assert!(!global::<bool>(&runtime, "fired"));
    }

    #[test]
    fn set_interval_reschedules_until_cleared() {
        let runtime = eval(
            "globalThis.count = 0; \
             const id = setInterval(() => { \
                 globalThis.count += 1; \
                 if (globalThis.count >= 3) clearInterval(id); \
             }, 0);",
        );
        for _ in 0..5 {
            runtime.run_due_timers().unwrap();
        }
        assert_eq!(global::<f64>(&runtime, "count"), 3.0);
    }

    #[test]
    fn set_immediate_fires_on_next_tick() {
        let runtime =
            eval("globalThis.fired = false; setImmediate(() => { globalThis.fired = true; });");
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "fired"));
    }

    #[test]
    fn request_animation_frame_receives_a_timestamp() {
        let runtime = eval(
            "globalThis.timestamp = -1; \
             requestAnimationFrame((t) => { globalThis.timestamp = t; });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<f64>(&runtime, "timestamp") >= 0.0);
    }

    #[test]
    fn cancel_animation_frame_prevents_firing() {
        let runtime = eval(
            "globalThis.fired = false; \
             const id = requestAnimationFrame(() => { globalThis.fired = true; }); \
             cancelAnimationFrame(id);",
        );
        runtime.run_due_timers().unwrap();
        assert!(!global::<bool>(&runtime, "fired"));
    }

    #[test]
    fn queue_microtask_runs_in_order_including_self_queued_work() {
        let runtime = eval(
            "globalThis.order = ''; \
             queueMicrotask(() => { \
                 globalThis.order += '1'; \
                 queueMicrotask(() => { globalThis.order += '2'; }); \
             }); \
             queueMicrotask(() => { globalThis.order += 'a'; });",
        );
        runtime.drain_microtasks();
        assert_eq!(global::<String>(&runtime, "order"), "1a2");
    }

    #[test]
    fn promise_then_resolves_after_draining_microtasks() {
        let runtime = eval(
            "globalThis.resolved = false; \
             Promise.resolve().then(() => { globalThis.resolved = true; });",
        );
        runtime.drain_microtasks();
        assert!(global::<bool>(&runtime, "resolved"));
    }

    #[test]
    fn timer_callback_can_use_a_promise() {
        let runtime = eval(
            "globalThis.resolved = false; \
             setTimeout(() => { \
                 Promise.resolve().then(() => { globalThis.resolved = true; }); \
             }, 0);",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "resolved"));
    }

    #[test]
    fn async_function_resumes_after_an_awaited_timer_resolves() {
        let runtime = eval(
            "globalThis.done = false; \
             async function run() { \
                 await new Promise((resolve) => setTimeout(resolve, 0)); \
                 globalThis.done = true; \
             } \
             run();",
        );
        assert!(!global::<bool>(&runtime, "done"), "shouldn't resume yet");
        runtime.run_due_timers().unwrap();
        assert!(
            global::<bool>(&runtime, "done"),
            "should resume once the awaited timer fires"
        );
    }

    #[test]
    fn async_function_return_value_is_observable_via_then() {
        let runtime = eval(
            "globalThis.result = ''; \
             async function greeting() { \
                 await Promise.resolve(); \
                 return 'hi'; \
             } \
             greeting().then((value) => { globalThis.result = value; });",
        );
        runtime.drain_microtasks();
        assert_eq!(global::<String>(&runtime, "result"), "hi");
    }

    #[test]
    fn draw_calls_outside_a_handler_throw_draw_outside_handler_error() {
        let runtime = eval(
            "import { clearScreen, DrawOutsideHandlerError } from 'ely:framebuffer'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 clearScreen(0); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof DrawOutsideHandlerError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn draw_calls_inside_a_registered_handler_succeed() {
        let runtime = eval(
            "import { clearScreen, addDrawHandler, Color } from 'ely:framebuffer'; \
             globalThis.drawn = false; \
             addDrawHandler(() => { \
                 clearScreen(Color.Slate900); \
                 globalThis.drawn = true; \
             });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "drawn"));
    }

    #[test]
    fn update_ticker_fires_once_per_frame_with_a_delta_time() {
        let runtime = eval(
            "import { addUpdateTicker } from 'ely:lifecycle'; \
             globalThis.calls = 0; \
             globalThis.lastDt = -1; \
             addUpdateTicker((dt) => { \
                 globalThis.calls += 1; \
                 globalThis.lastDt = dt; \
             });",
        );
        for expected_calls in 1..=3 {
            runtime.run_due_timers().unwrap();
            assert_eq!(global::<f64>(&runtime, "calls"), expected_calls as f64);
            assert!(global::<f64>(&runtime, "lastDt") >= 0.0);
        }
    }

    #[test]
    fn remove_update_ticker_stops_further_calls() {
        let runtime = eval(
            "import { addUpdateTicker, removeUpdateTicker } from 'ely:lifecycle'; \
             globalThis.calls = 0; \
             const id = addUpdateTicker(() => { \
                 globalThis.calls += 1; \
                 removeUpdateTicker(id); \
             });",
        );
        for _ in 0..3 {
            runtime.run_due_timers().unwrap();
        }
        assert_eq!(global::<f64>(&runtime, "calls"), 1.0);
    }

    #[test]
    fn post_init_handler_runs_once_after_eval_and_sees_working_timers() {
        let runtime = eval(
            "import { addPostInitHandler, delay } from 'ely:lifecycle'; \
             globalThis.ran = false; \
             globalThis.timerFired = false; \
             addPostInitHandler(async () => { \
                 globalThis.ran = true; \
                 await delay(0); \
                 globalThis.timerFired = true; \
             });",
        );
        assert!(
            !global::<bool>(&runtime, "ran"),
            "must not run during eval_module itself"
        );

        runtime.run_post_init_handlers().unwrap();
        assert!(global::<bool>(&runtime, "ran"));
        assert!(
            !global::<bool>(&runtime, "timerFired"),
            "the delay(0) timer hasn't been serviced yet"
        );

        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "timerFired"));

        // A second call must not re-run anything: the handler list was
        // drained, not just iterated.
        runtime.run_post_init_handlers().unwrap();
    }

    #[test]
    fn pointer_position_reflects_injected_cursor_moved_events() {
        use winit::dpi::PhysicalPosition;
        use winit::event::{DeviceId, WindowEvent};

        let (runtime, input) = eval_with_input(
            "import { getPointerX, getPointerY } from 'ely:input'; \
             globalThis.x = getPointerX(); \
             globalThis.y = getPointerY();",
        );
        assert_eq!(global::<f64>(&runtime, "x"), 0.0);
        assert_eq!(global::<f64>(&runtime, "y"), 0.0);

        input.handle_window_event(&WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(144.0, 72.0),
        });
        runtime
            .eval_module(
                "test2.ts",
                "import { getPointerX, getPointerY } from 'ely:input'; \
                 globalThis.x = getPointerX(); \
                 globalThis.y = getPointerY();",
            )
            .unwrap();
        assert_eq!(global::<f64>(&runtime, "x"), 72.0);
        assert_eq!(global::<f64>(&runtime, "y"), 36.0);
    }

    #[test]
    fn pointer_down_and_pressed_reflect_injected_mouse_input_events() {
        use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

        let (runtime, input) = eval_with_input(
            "import { isPointerDown, isPointerUp, wasPointerPressed, wasPointerReleased } from 'ely:input'; \
             globalThis.readState = () => ({ \
                 down: isPointerDown(), \
                 up: isPointerUp(), \
                 pressed: wasPointerPressed(), \
                 released: wasPointerReleased(), \
             });",
        );

        input.handle_window_event(&WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        });
        runtime
            .eval_module(
                "check1.ts",
                "const s = globalThis.readState(); \
                 globalThis.down1 = s.down; \
                 globalThis.up1 = s.up; \
                 globalThis.pressed1 = s.pressed;",
            )
            .unwrap();
        assert!(global::<bool>(&runtime, "down1"));
        assert!(!global::<bool>(&runtime, "up1"));
        assert!(global::<bool>(&runtime, "pressed1"));

        input.end_frame();
        input.handle_window_event(&WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
        });
        runtime
            .eval_module(
                "check2.ts",
                "const s = globalThis.readState(); \
                 globalThis.down2 = s.down; \
                 globalThis.released2 = s.released;",
            )
            .unwrap();
        assert!(!global::<bool>(&runtime, "down2"));
        assert!(global::<bool>(&runtime, "released2"));
    }

    #[test]
    fn key_down_and_pressed_reflect_injected_keyboard_events() {
        let (runtime, input) = eval_with_input(
            "import { Key, isKeyDown, isKeyUp, wasKeyPressed, wasKeyReleased } from 'ely:input'; \
             globalThis.readState = () => ({ \
                 down: isKeyDown(Key.KeyW), \
                 up: isKeyUp(Key.KeyW), \
                 pressed: wasKeyPressed(Key.KeyW), \
                 released: wasKeyReleased(Key.KeyW), \
             });",
        );

        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);
        runtime
            .eval_module(
                "check1.ts",
                "const s = globalThis.readState(); \
                 globalThis.down1 = s.down; \
                 globalThis.up1 = s.up; \
                 globalThis.pressed1 = s.pressed;",
            )
            .unwrap();
        assert!(global::<bool>(&runtime, "down1"));
        assert!(!global::<bool>(&runtime, "up1"));
        assert!(global::<bool>(&runtime, "pressed1"));

        input.end_frame();
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, false, false);
        runtime
            .eval_module(
                "check2.ts",
                "const s = globalThis.readState(); \
                 globalThis.down2 = s.down; \
                 globalThis.released2 = s.released;",
            )
            .unwrap();
        assert!(!global::<bool>(&runtime, "down2"));
        assert!(global::<bool>(&runtime, "released2"));
    }

    #[test]
    fn load_image_and_draw_image_round_trip_without_throwing() {
        let runtime = eval(
            "import { loadImage } from 'ely:image'; \
             import { addDrawHandler, drawImage } from 'ely:framebuffer'; \
             globalThis.drawn = false; \
             const image = loadImage('/test.png'); \
             addDrawHandler(() => { drawImage(image, 10, 10); globalThis.drawn = true; });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "drawn"));
    }

    #[test]
    fn draw_image_with_an_unknown_id_throws() {
        let runtime = eval(
            "import { addDrawHandler, drawImage } from 'ely:framebuffer'; \
             globalThis.threw = false; \
             addDrawHandler(() => { \
                 try { drawImage(999999, 0, 0); } catch { globalThis.threw = true; } \
             });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "threw"));
    }

    #[test]
    fn load_image_outside_userland_root_throws_image_load_error() {
        let runtime = eval(
            "import { loadImage, ImageLoadError } from 'ely:image'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 loadImage('/../../framebuffer.rs'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof ImageLoadError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn load_image_with_a_relative_path_throws_relative_path_error() {
        let runtime = eval(
            "import { loadImage } from 'ely:image'; \
             import { RelativePathError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 loadImage('test.png'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof RelativePathError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn filesystem_resolve_does_not_double_the_leading_slash() {
        let runtime = eval(
            "import { resolve } from 'ely:filesystem'; \
             globalThis.result = resolve('a', 'b');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "/a/b");
    }

    #[test]
    fn filesystem_join_normalizes_dot_and_dot_dot_segments() {
        let runtime = eval(
            "import { join } from 'ely:filesystem'; \
             globalThis.result = join('/a', 'b/../c', './d');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "/a/c/d");
    }

    #[test]
    fn filesystem_join_clamps_dot_dot_at_the_root() {
        let runtime = eval(
            "import { join } from 'ely:filesystem'; \
             globalThis.result = join('/a', '../../b');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "/b");
    }

    #[test]
    fn filesystem_extract_directory_name_of_root_child_is_root() {
        let runtime = eval(
            "import { extractDirectoryName } from 'ely:filesystem'; \
             globalThis.result = extractDirectoryName('/a');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "/");
    }

    #[test]
    fn filesystem_extract_directory_name_ignores_trailing_slashes() {
        let runtime = eval(
            "import { extractDirectoryName } from 'ely:filesystem'; \
             globalThis.result = extractDirectoryName('/a/b/');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "/a");
    }

    #[test]
    fn filesystem_extract_base_name_ignores_trailing_slashes() {
        let runtime = eval(
            "import { extractBaseName } from 'ely:filesystem'; \
             globalThis.result = extractBaseName('/a/b/');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "b");
    }

    #[test]
    fn filesystem_extract_extension_of_a_dotfile_is_empty() {
        let runtime = eval(
            "import { extractExtension } from 'ely:filesystem'; \
             globalThis.result = extractExtension('.bashrc');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "");
    }

    #[test]
    fn filesystem_extract_extension_of_a_multi_dot_file_is_the_last_extension() {
        let runtime = eval(
            "import { extractExtension } from 'ely:filesystem'; \
             globalThis.result = extractExtension('archive.tar.gz');",
        );
        assert_eq!(global::<String>(&runtime, "result"), ".gz");
    }

    #[test]
    fn filesystem_sanitize_name_replaces_traversal_segments() {
        let runtime = eval(
            "import { sanitizeName } from 'ely:filesystem'; \
             globalThis.dot = sanitizeName('.'); \
             globalThis.dotDot = sanitizeName('..');",
        );
        assert_eq!(global::<String>(&runtime, "dot"), "_");
        assert_eq!(global::<String>(&runtime, "dotDot"), "__");
    }

    #[test]
    fn filesystem_write_file_and_read_file_round_trip_the_whole_file() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { writeFile, readFile } from 'ely:filesystem'; \
             const bytes = Uint8Array.from('hello', (c) => c.charCodeAt(0)); \
             writeFile('/greeting.bin', bytes); \
             const read = readFile('/greeting.bin'); \
             globalThis.result = Array.from(read, (b) => String.fromCharCode(b)).join('');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "hello");
    }

    #[test]
    fn filesystem_write_file_with_a_range_patches_in_place() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { writeFile, readFile } from 'ely:filesystem'; \
             const initial = Uint8Array.from('0123456789', (c) => c.charCodeAt(0)); \
             writeFile('/patch.bin', initial); \
             const patch = Uint8Array.from('XYZ', (c) => c.charCodeAt(0)); \
             writeFile('/patch.bin', patch, { offset: 2, length: 3 }); \
             const read = readFile('/patch.bin'); \
             globalThis.result = Array.from(read, (b) => String.fromCharCode(b)).join('');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "01XYZ56789");
    }

    #[test]
    fn filesystem_read_file_with_a_range_reads_a_slice() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { writeFile, readFile } from 'ely:filesystem'; \
             const bytes = Uint8Array.from('0123456789', (c) => c.charCodeAt(0)); \
             writeFile('/slice.bin', bytes); \
             const read = readFile('/slice.bin', { offset: 3, length: 4 }); \
             globalThis.result = Array.from(read, (b) => String.fromCharCode(b)).join('');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "3456");
    }

    #[test]
    fn filesystem_write_text_file_and_read_text_file_round_trip() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { writeTextFile, readTextFile } from 'ely:filesystem'; \
             writeTextFile('/note.txt', 'hello world'); \
             globalThis.result = readTextFile('/note.txt');",
        );
        assert_eq!(global::<String>(&runtime, "result"), "hello world");
    }

    #[test]
    fn filesystem_remove_deletes_a_file() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { writeTextFile, remove, stat } from 'ely:filesystem'; \
             writeTextFile('/gone.txt', 'bye'); \
             remove('/gone.txt'); \
             globalThis.threw = false; \
             try { \
                 stat('/gone.txt'); \
             } catch (err) { \
                 globalThis.threw = true; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
    }

    #[test]
    fn filesystem_remove_recursively_deletes_a_directory() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { createDirectory, writeTextFile, remove, stat } from 'ely:filesystem'; \
             createDirectory('/tree/nested'); \
             writeTextFile('/tree/nested/file.txt', 'x'); \
             remove('/tree'); \
             globalThis.threw = false; \
             try { \
                 stat('/tree'); \
             } catch (err) { \
                 globalThis.threw = true; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
    }

    #[test]
    fn filesystem_create_directory_creates_nested_missing_parents() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { createDirectory, stat } from 'ely:filesystem'; \
             createDirectory('/a/b/c'); \
             globalThis.result = stat('/a/b/c').kind;",
        );
        assert_eq!(global::<String>(&runtime, "result"), "Directory");
    }

    #[test]
    fn filesystem_list_directory_omits_symlinks_but_lists_regular_entries() {
        let root = test_scratch_root();
        std::fs::write(root.join("real.txt"), b"data").unwrap();
        std::fs::create_dir(root.join("realdir")).unwrap();
        std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

        let runtime = eval_with_root(
            root,
            "import { listDirectory } from 'ely:filesystem'; \
             const entries = listDirectory('/'); \
             globalThis.paths = entries.map((e) => e.path).sort().join(','); \
             globalThis.count = entries.length;",
        );
        assert_eq!(global::<String>(&runtime, "paths"), "/real.txt,/realdir");
        assert_eq!(global::<f64>(&runtime, "count"), 2.0);
    }

    #[test]
    fn filesystem_stat_reports_file_and_directory_shapes() {
        let runtime = eval_with_root(
            test_scratch_root(),
            "import { writeTextFile, createDirectory, stat } from 'ely:filesystem'; \
             writeTextFile('/f.txt', 'abcde'); \
             createDirectory('/d'); \
             const file = stat('/f.txt'); \
             const dir = stat('/d'); \
             globalThis.fileKind = file.kind; \
             globalThis.fileSize = file.size; \
             globalThis.filePath = file.path; \
             globalThis.dirKind = dir.kind; \
             globalThis.dirPath = dir.path;",
        );
        assert_eq!(global::<String>(&runtime, "fileKind"), "File");
        assert_eq!(global::<f64>(&runtime, "fileSize"), 5.0);
        assert_eq!(global::<String>(&runtime, "filePath"), "/f.txt");
        assert_eq!(global::<String>(&runtime, "dirKind"), "Directory");
        assert_eq!(global::<String>(&runtime, "dirPath"), "/d");
    }

    #[test]
    fn filesystem_read_file_through_a_symlink_component_is_not_found() {
        let root = test_scratch_root();
        std::fs::create_dir(root.join("real")).unwrap();
        std::fs::write(root.join("real/target.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();

        let runtime = eval_with_root(
            root,
            "import { readFile, NotFoundError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 readFile('/alias/target.txt'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof NotFoundError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn filesystem_read_file_on_a_directory_throws_is_a_directory_error() {
        let root = test_scratch_root();
        std::fs::create_dir(root.join("adir")).unwrap();

        let runtime = eval_with_root(
            root,
            "import { readFile, IsADirectoryError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 readFile('/adir'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof IsADirectoryError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn filesystem_write_file_on_a_directory_throws_is_a_directory_error() {
        let root = test_scratch_root();
        std::fs::create_dir(root.join("adir")).unwrap();

        let runtime = eval_with_root(
            root,
            "import { writeFile, IsADirectoryError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 writeFile('/adir', new Uint8Array([1])); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof IsADirectoryError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn filesystem_create_directory_blocked_by_a_file_throws_not_a_directory_error() {
        let root = test_scratch_root();
        std::fs::write(root.join("blocker"), b"x").unwrap();

        let runtime = eval_with_root(
            root,
            "import { createDirectory, NotADirectoryError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 createDirectory('/blocker/nested'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof NotADirectoryError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn filesystem_list_directory_on_a_file_throws_not_a_directory_error() {
        let root = test_scratch_root();
        std::fs::write(root.join("f.txt"), b"x").unwrap();

        let runtime = eval_with_root(
            root,
            "import { listDirectory, NotADirectoryError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 listDirectory('/f.txt'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof NotADirectoryError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn filesystem_read_text_file_on_invalid_utf8_throws_text_decode_error() {
        let root = test_scratch_root();
        std::fs::write(root.join("bad.txt"), [0xff, 0xfe, 0xfd]).unwrap();

        let runtime = eval_with_root(
            root,
            "import { readTextFile, TextDecodeError } from 'ely:filesystem'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 readTextFile('/bad.txt'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof TextDecodeError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn container_has_value_and_is_option_narrow_correctly() {
        let runtime = eval(
            "import { hasValue, isOption, none, some } from 'ely:container'; \
             globalThis.presentHasValue = hasValue(some(1)); \
             globalThis.absentHasValue = hasValue(none()); \
             globalThis.isOptionAlways = isOption('anything');",
        );
        assert!(global::<bool>(&runtime, "presentHasValue"));
        assert!(!global::<bool>(&runtime, "absentHasValue"));
        assert!(global::<bool>(&runtime, "isOptionAlways"));
    }

    #[test]
    fn container_get_or_else_returns_fallback_only_when_absent() {
        let runtime = eval(
            "import { getOrElse, none, some } from 'ely:container'; \
             globalThis.present = getOrElse(some(1), 2); \
             globalThis.absent = getOrElse(none(), 2);",
        );
        assert_eq!(global::<f64>(&runtime, "present"), 1.0);
        assert_eq!(global::<f64>(&runtime, "absent"), 2.0);
    }

    #[test]
    fn container_map_transforms_present_and_passes_through_absent() {
        let runtime = eval(
            "import { map, none, some } from 'ely:container'; \
             globalThis.present = map(some(2), (x) => x * 10); \
             globalThis.absentIsUndefined = map(none(), (x) => x * 10) === undefined;",
        );
        assert_eq!(global::<f64>(&runtime, "present"), 20.0);
        assert!(global::<bool>(&runtime, "absentIsUndefined"));
    }

    #[test]
    fn container_unwrap_throws_option_unwrap_error_on_empty_option() {
        let runtime = eval(
            "import { unwrap, none, some, OptionUnwrapError } from 'ely:container'; \
             globalThis.unwrapped = unwrap(some(42)); \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 unwrap(none()); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof OptionUnwrapError; \
             }",
        );
        assert_eq!(global::<f64>(&runtime, "unwrapped"), 42.0);
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
    }

    #[test]
    fn relative_import_resolves_and_evaluates() {
        let entry = test_userland_root().join("entry.ts");
        let (runtime, _input) = eval_named_with_input(
            entry.to_str().unwrap(),
            "import { value } from './relative_import_target.ts'; \
             globalThis.value = value;",
        );
        assert_eq!(global::<f64>(&runtime, "value"), 42.0);
    }

    #[test]
    fn relative_import_escaping_userland_root_fails_to_resolve() {
        let entry = test_userland_root().join("entry.ts");
        let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
        let input = Rc::new(Input::new(Rc::clone(&scale)));
        let runtime = ElysiumRuntime::new(
            Rc::new(RefCell::new(Vec::new())),
            Rc::clone(&input),
            scale,
            test_userland_root(),
            0,
            ProcessChannel::detached(),
            None,
        )
        .expect("failed to construct runtime");
        let result =
            runtime.eval_module(entry.to_str().unwrap(), "import '../../../../etc/passwd';");
        assert!(result.is_err());
    }

    #[test]
    fn import_meta_reports_virtual_userland_paths() {
        let module = test_userland_root().join("meta_module.ts");
        let (runtime, _input) = eval_named_with_input(
            module.to_str().unwrap(),
            "globalThis.directoryName = import.meta.directoryName; \
             globalThis.fileName = import.meta.fileName;",
        );
        assert_eq!(global::<String>(&runtime, "directoryName"), "/");
        assert_eq!(global::<String>(&runtime, "fileName"), "/meta_module.ts");
    }

    #[test]
    fn process_surface_reports_defaults_for_a_detached_runtime() {
        let runtime = eval(
            "import { currentProcessId, currentArguments } from 'ely:process'; \
             globalThis.id = currentProcessId(); \
             globalThis.hasArgs = currentArguments() !== undefined;",
        );
        assert_eq!(global::<f64>(&runtime, "id"), 0.0);
        assert!(!global::<bool>(&runtime, "hasArgs"));
        assert!(!runtime.has_message_handler());
        assert!(!runtime.exit_requested());
    }

    #[test]
    fn on_message_registration_is_visible_to_the_host() {
        let runtime =
            eval("import { addMessageHandler } from 'ely:process'; addMessageHandler(() => {});");
        assert!(runtime.has_message_handler());
    }

    #[test]
    fn exit_binding_sets_the_flag_the_manager_reads() {
        let runtime = eval("import { exit } from 'ely:process'; exit();");
        assert!(runtime.exit_requested());
    }

    #[test]
    fn has_no_pending_work_reflects_a_live_timer() {
        let runtime = eval("globalThis.id = setInterval(() => {}, 1000);");
        assert!(!runtime.has_no_pending_work());
        let id: f64 = global(&runtime, "id");
        runtime
            .context
            .with(|ctx| ctx.eval::<(), _>(format!("clearInterval({id});")))
            .unwrap();
        assert!(runtime.has_no_pending_work());
    }

    #[test]
    fn deliver_message_invokes_the_registered_handler() {
        let runtime = eval(
            "import { addMessageHandler } from 'ely:process'; \
             globalThis.seen = null; \
             addMessageHandler((env) => { globalThis.seen = env.kind + ':' + env.data; });",
        );
        runtime
            .deliver_message(r#"{"kind":"greet","from":2,"to":0,"data":"hi"}"#)
            .unwrap();
        assert_eq!(global::<String>(&runtime, "seen"), "greet:hi");
    }

    #[test]
    fn deadlock_exception_is_remapped_to_a_clear_message() {
        let remapped = remap_deadlock_error(GuardedError::Exception(
            "Error blocking on a promise resulted in a dead lock".to_string(),
        ));
        match remapped {
            GuardedError::Exception(message) => {
                assert!(message.contains("addPostInitHandler"));
                assert!(message.contains("ely:lifecycle"));
            }
            GuardedError::Timeout => panic!("expected an Exception"),
        }
    }

    #[test]
    fn unrelated_exceptions_are_not_remapped() {
        let remapped = remap_deadlock_error(GuardedError::Exception("boom".to_string()));
        match remapped {
            GuardedError::Exception(message) => assert_eq!(message, "boom"),
            GuardedError::Timeout => panic!("expected an Exception"),
        }
    }
}
