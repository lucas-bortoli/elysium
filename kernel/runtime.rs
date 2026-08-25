use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::function::Rest;
use rquickjs::loader::FileResolver;
use rquickjs::{
    Context, Ctx, Error, Function, Module, Persistent, Result, Runtime as JsRuntime, Type, Value,
};

use crate::esm_resolver::{CompilingLoader, EmbeddedOrFileResolver, bootstrap_jsx_runtime};
use crate::framebuffer::{self, DrawCommand};
use crate::image::{self, ImageTable};
use crate::input::{self, Input};
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
    /// Images loaded by `ely:image`'s `loadImage`, keyed by id. Holds no
    /// `Persistent` JS values (unlike the three fields above), so it's not
    /// subject to the same GC-sweep-ordering hazard — grouped with them in
    /// `Drop` purely so VM teardown has one obvious place every resource
    /// gets released.
    images: Rc<ImageTable>,
    guard: Rc<GuardState>,
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
    /// `draw_commands` is written to. `program_dir` is the running
    /// program's own root directory — `ely:image`'s `loadImage` resolves
    /// every path against it and can never escape it, regardless of the
    /// process's actual working directory.
    pub fn new(
        draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
        input: Rc<Input>,
        scale: Rc<Cell<u32>>,
        program_dir: PathBuf,
    ) -> Result<Self> {
        let js_runtime = JsRuntime::new()?;

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
            EmbeddedOrFileResolver(
                FileResolver::default()
                    .with_pattern("{}.ts")
                    .with_pattern("{}.tsx"),
            ),
            CompilingLoader,
        );

        let context = Context::full(&js_runtime)?;

        let timers = Rc::new(TimerQueue::new());
        let microtasks = Rc::new(RefCell::new(Vec::new()));
        let post_init_handlers = Rc::new(RefCell::new(Vec::new()));
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
            image::bootstrap_image_bindings(&ctx, Rc::clone(&images), program_dir)?;
            Ok(())
        })?;

        Ok(Self {
            js_runtime,
            context,
            guard,
            timers,
            microtasks,
            post_init_handlers,
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
            let (_module, promise) = Module::declare(ctx.clone(), name, compiled)?.eval()?;
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
        let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
        let input = Rc::new(Input::new(Rc::clone(&scale)));
        let runtime = ElysiumRuntime::new(
            Rc::new(RefCell::new(Vec::new())),
            Rc::clone(&input),
            scale,
            test_program_dir(),
        )
        .expect("failed to construct runtime");
        runtime
            .eval_module("test.ts", source)
            .expect("module failed to evaluate");
        (runtime, input)
    }

    /// `loadImage`'s `program_dir` for every test in this module — a fixed
    /// fixtures directory holding a small, real PNG (`test.png`) plus a
    /// checked-in `.ts` sibling so a path-traversal test has a real file
    /// outside this directory to point at.
    fn test_program_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("kernel/image/fixtures")
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
             const image = loadImage('test.png'); \
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
    fn load_image_outside_program_dir_throws_image_load_error() {
        let runtime = eval(
            "import { loadImage, ImageLoadError } from 'ely:image'; \
             globalThis.threw = false; \
             globalThis.correctType = false; \
             try { \
                 loadImage('../../framebuffer.rs'); \
             } catch (err) { \
                 globalThis.threw = true; \
                 globalThis.correctType = err instanceof ImageLoadError; \
             }",
        );
        assert!(global::<bool>(&runtime, "threw"));
        assert!(global::<bool>(&runtime, "correctType"));
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
