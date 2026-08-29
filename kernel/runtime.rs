use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::function::Rest;
use rquickjs::{
    Context, Ctx, Error, Function, Module, Persistent, Result, Runtime as JsRuntime, Type, Value,
};

use crate::bindings::bind;
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

/// The devices the kernel owns and every VM shares a handle to.
///
/// A process gets no private copy of any of these: `draw_commands` is the one
/// buffer `ely:framebuffer`'s bindings append to and the kernel drains once a
/// guarded `draw()` returns, `input` is the pointer and keyboard state fed
/// from raw window events, and `scale` is the physical-pixels-per-logical-pixel
/// setting `setScale` writes straight into. `userland_root` is the root of the
/// whole userland tree — an absolute virtual path resolves against it and can
/// never escape it, whatever the process's own entry path was, and every
/// module's `import.meta` is expressed relative to it.
///
/// Canonicalized once, when this is built, so every sandbox walk downstream
/// starts from a real, symlink-free root and no caller has to re-establish
/// that invariant.
#[derive(Clone)]
pub struct Devices {
    pub draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
    pub input: Rc<Input>,
    pub scale: Rc<Cell<u32>>,
    pub userland_root: PathBuf,
}

impl Devices {
    /// Panics if `userland_root` doesn't resolve — there is no useful Elysium
    /// without a userland tree, and failing here beats every path operation
    /// failing later for a reason that no longer names the cause.
    pub fn new(
        draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
        input: Rc<Input>,
        scale: Rc<Cell<u32>>,
        userland_root: PathBuf,
    ) -> Self {
        let userland_root = std::fs::canonicalize(&userland_root).unwrap_or_else(|err| {
            panic!(
                "userland root {} is invalid: {err}",
                userland_root.display()
            )
        });
        Self {
            draw_commands,
            input,
            scale,
            userland_root,
        }
    }
}

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
    /// Builds a VM wired to `devices`, owned by process `self_id`. `channel`
    /// is the shared queue its `ely:process` bindings push spawn, send and
    /// control requests onto; `arguments_json` is whatever `spawn` was passed
    /// for this process (`None` for init).
    pub fn new(
        devices: &Devices,
        self_id: ProcessId,
        channel: ProcessChannel,
        arguments_json: Option<String>,
    ) -> Result<Self> {
        let Devices {
            draw_commands,
            input,
            scale,
            userland_root,
        } = devices.clone();

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
            bind(&ctx, "print", print)?;
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
                process::ProcessState {
                    message_handler: Rc::clone(&message_handler),
                    exit_requested: Rc::clone(&exit_requested),
                },
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
    /// follows. Called once, right after [`Self::eval_module`] succeeds, on
    /// the process's first frame and before that process's own timers are
    /// serviced, so a handler can safely do timer-dependent work a
    /// top-level `await` cannot.
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

            result?;
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
    /// considered idle — see `documentation/Multitasking.md`.
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

/// Registers `__add_post_init_handler` (wrapped by `ely:lifecycle`'s
/// `addPostInitHandler`) as a global that appends onto `handlers`, mirroring
/// `bootstrap_timers`' `queueMicrotask` registration.
fn bootstrap_post_init_handlers<'js>(
    ctx: &Ctx<'js>,
    handlers: Rc<RefCell<Vec<Persistent<Function<'static>>>>>,
) -> Result<()> {
    bind(
        ctx,
        "__add_post_init_handler",
        move |ctx: Ctx<'js>, handler: Function<'js>| {
            handlers.borrow_mut().push(Persistent::save(&ctx, handler));
        },
    )
}

/// Host binding for `print(...values)`: writes any number of JS values,
/// space-separated, to stdout.
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
mod tests;
