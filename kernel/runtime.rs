use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use boa_engine::interop::JsRest;
use boa_engine::object::builtins::JsFunction;
use boa_engine::{
    Context, Finalize, IntoJsFunctionCopied, JsError, JsNativeErrorKind, JsResult, JsValue, Module,
    Source, Trace, js_string,
};
use boa_gc::{Gc, GcRefCell};

use crate::esm_resolver::{ElysiumModuleLoader, bootstrap_jsx_runtime};
use crate::framebuffer::{self, DrawCommand};
use crate::timers::{TimerArgs, TimerQueue, bootstrap_timers};
use crate::transform;

/// The two ways a guarded call into the VM can fail. On `Timeout`, per
/// Elysium's failure contract, the VM is destroyed but Elysium soldiers on:
/// the owning `ElysiumRuntime` is poisoned and must be dropped, never
/// reused, while the caller (kernel) continues running everything else. An
/// `Exception` is just an ordinary uncaught error and carries no such
/// requirement.
///
/// `Timeout` is raised by Boa's loop-iteration limit (see
/// [`LOOP_ITERATION_LIMIT`]), not a wall-clock deadline — the engine has no
/// hook for the kernel to interrupt a call mid-execution by elapsed time.
/// It catches a program stuck in an unbounded loop, but not a single very
/// expensive (yet loop-bounded, or native-call-heavy) synchronous call.
#[derive(Debug)]
pub enum GuardedError {
    Timeout,
    Exception(String),
}

/// A generous cap on loop iterations within any single call into the VM —
/// the only execution limit Boa exposes to the host. High enough that no
/// reasonable per-frame `update`/`draw` callback should ever brush against
/// it, low enough to still bring down a program stuck in an infinite loop
/// rather than let it spin forever.
const LOOP_ITERATION_LIMIT: u64 = 10_000_000;

pub struct ElysiumRuntime {
    /// Pending `setTimeout`/`setInterval`/`setImmediate`/
    /// `requestAnimationFrame` timers, checked each frame by
    /// [`Self::run_due_timers`].
    timers: Gc<TimerQueue>,
    /// Callbacks queued by `queueMicrotask`, flushed by [`Self::drain_microtasks`].
    microtasks: Gc<GcRefCell<Vec<JsFunction>>>,
    /// Callbacks registered by `ely:lifecycle`'s `addPostInitHandler`, run
    /// once by [`Self::run_post_init_handlers`].
    post_init_handlers: Gc<GcRefCell<Vec<JsFunction>>>,
    context: Context,
}

impl ElysiumRuntime {
    /// `draw_commands` is the Framebuffer device's shared draw-command buffer:
    /// `ely:framebuffer`'s hidden globals push onto it directly rather than
    /// touching any drawing state themselves, keeping the VM's own bindings
    /// ignorant of `wgpu`.
    pub fn new(draw_commands: Rc<RefCell<Vec<DrawCommand>>>) -> JsResult<Self> {
        // Programs are TS(X) files on disk; `import`/`export` resolve to
        // sibling `.ts`/`.tsx` files, each compiled (JSX -> h(), then TS
        // erased) as it's loaded. A bare specifier matching one of the
        // embedded runtime modules resolves to that instead.
        let mut context = Context::builder()
            .module_loader(Rc::new(ElysiumModuleLoader))
            .build()?;

        context
            .runtime_limits_mut()
            .set_loop_iteration_limit(LOOP_ITERATION_LIMIT);

        let timers = Gc::new(TimerQueue::new());
        let microtasks = Gc::new(GcRefCell::new(Vec::new()));
        let post_init_handlers = Gc::new(GcRefCell::new(Vec::new()));

        let print_fn = print.into_js_function_copied(&mut context);
        context.register_global_builtin_callable(js_string!("print"), 0, print_fn)?;

        bootstrap_jsx_runtime(&mut context)?;
        framebuffer::bootstrap_framebuffer_bindings(&mut context, draw_commands)?;
        bootstrap_timers(&mut context, timers.clone(), microtasks.clone())?;
        bootstrap_post_init_handlers(&mut context, post_init_handlers.clone())?;

        Ok(Self {
            timers,
            microtasks,
            post_init_handlers,
            context,
        })
    }

    /// Compiles and evaluates `source` as an ES module named `name` (its
    /// path, used as the base for resolving any relative imports it has).
    /// Runs purely for its side effects — a program registers whatever
    /// per-frame work it wants (`ely:lifecycle`'s `addUpdateTicker`,
    /// `ely:framebuffer`'s `addDrawHandler`) during evaluation, plus
    /// whatever it wants deferred to after evaluation via
    /// `addPostInitHandler` (see [`Self::run_post_init_handlers`]).
    pub fn eval_module(&mut self, name: &str, source: &str) -> Result<(), GuardedError> {
        let compiled = transform::compile(source).map_err(GuardedError::Exception)?;
        let path = PathBuf::from(name);

        self.run_guarded(|context| {
            let src = Source::from_bytes(compiled.as_bytes()).with_path(&path);
            let module = Module::parse(src, None, context)?;
            module.load_link_evaluate(context).await_blocking(context)?;
            Ok(())
        })
    }

    /// Runs every callback registered by `ely:lifecycle`'s
    /// `addPostInitHandler` exactly once, in registration order, draining
    /// microtasks after each — the same discipline [`Self::run_due_timers`]
    /// follows. Called once, right after [`Self::eval_module`] succeeds and
    /// before the frame loop (and therefore timers) starts running, so a
    /// handler can safely do timer-dependent work a top-level `await`
    /// cannot.
    pub fn run_post_init_handlers(&mut self) -> Result<(), GuardedError> {
        let handlers = self.post_init_handlers.borrow_mut().split_off(0);
        for handler in handlers {
            let result =
                self.run_guarded(move |context| handler.call(&JsValue::undefined(), &[], context));
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
    pub fn run_due_timers(&mut self) -> Result<(), GuardedError> {
        let now = Instant::now();
        for id in self.timers.due_ids(now) {
            let Some((callback, args, interval)) = self.timers.prepare_run(id) else {
                continue;
            };

            let timers = self.timers.clone();
            let result = self.run_guarded(move |context| match &args {
                TimerArgs::User(args) => callback.call(&JsValue::undefined(), args, context),
                TimerArgs::AnimationFrameTimestamp => callback.call(
                    &JsValue::undefined(),
                    &[JsValue::from(timers.elapsed_seconds())],
                    context,
                ),
            });

            if let Some(period) = interval {
                self.timers.reschedule_if_still_active(id, now + period);
            }

            self.drain_microtasks();

            result?;
        }
        Ok(())
    }

    /// Runs every callback queued by `queueMicrotask` (including any that
    /// queue further callbacks of their own), then drains Boa's own
    /// pending-job queue. Called after every timer callback so a program's
    /// `.then()` chains and `queueMicrotask` calls observe results in the
    /// same tick they should.
    pub fn drain_microtasks(&mut self) {
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
            if let Err(err) =
                self.run_guarded(move |context| callback.call(&JsValue::undefined(), &[], context))
            {
                match err {
                    GuardedError::Timeout => eprintln!("program timed out inside queueMicrotask()"),
                    GuardedError::Exception(err) => {
                        eprintln!("uncaught exception in queueMicrotask(): {err}")
                    }
                }
            }
        }

        while let Err(err) = self.context.run_jobs() {
            eprintln!("uncaught (in promise): {err}");
        }
    }

    /// The one entry point every call into this VM goes through: runs `f`
    /// against the VM's `Context` and, on failure, distinguishes the
    /// loop-iteration limit firing (`Timeout`) from any other thrown or
    /// returned error (`Exception`).
    fn run_guarded<T>(
        &mut self,
        f: impl FnOnce(&mut Context) -> JsResult<T>,
    ) -> Result<T, GuardedError> {
        f(&mut self.context).map_err(|err| self.classify_error(err))
    }

    fn classify_error(&mut self, err: JsError) -> GuardedError {
        if let Ok(native) = err.try_native(&mut self.context)
            && matches!(native.kind, JsNativeErrorKind::RuntimeLimit)
        {
            return GuardedError::Timeout;
        }
        GuardedError::Exception(err.to_string())
    }
}

/// Wraps `ely:lifecycle`'s `addPostInitHandler` list so it has a distinct
/// type from `ely:framebuffer`'s `queueMicrotask` list (see
/// [`crate::timers::Microtasks`]'s doc comment) in the `Context`'s native
/// data map — both are a `Gc<GcRefCell<Vec<JsFunction>>>`.
#[derive(Clone, Trace, Finalize, boa_engine::JsData)]
struct PostInitHandlers(Gc<GcRefCell<Vec<JsFunction>>>);

/// Registers `__add_post_init_handler` (wrapped by `ely:lifecycle`'s
/// `addPostInitHandler`) as a global that appends onto `handlers`.
fn bootstrap_post_init_handlers(
    context: &mut Context,
    handlers: Gc<GcRefCell<Vec<JsFunction>>>,
) -> JsResult<()> {
    context.insert_data(PostInitHandlers(handlers));
    let f = add_post_init_handler.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("__add_post_init_handler"), 1, f)?;
    Ok(())
}

fn add_post_init_handler(handler: JsFunction, context: &mut Context) {
    context
        .get_data::<PostInitHandlers>()
        .expect("bootstrap_post_init_handlers must run first")
        .0
        .borrow_mut()
        .push(handler);
}

/// Host binding for `print(...values)`: writes any number of JS values,
/// space-separated, to stdout.
fn print(values: JsRest<'_>, context: &mut Context) -> JsResult<()> {
    let line = values
        .0
        .iter()
        .map(|v| describe_value(v, context))
        .collect::<JsResult<Vec<_>>>()?
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
fn describe_value(value: &JsValue, context: &mut Context) -> JsResult<String> {
    Ok(match value.type_of() {
        "string" => value
            .as_string()
            .expect("type_of() == \"string\" implies as_string() succeeds")
            .to_std_string_escaped(),
        "undefined" => "undefined".to_string(),
        "function" => "[Function]".to_string(),
        "symbol" => "[Symbol]".to_string(),
        _ => match json_stringify(value, context) {
            Ok(Some(json)) => json,
            Ok(None) => "undefined".to_string(),
            Err(_) => "[unprintable value]".to_string(),
        },
    })
}

/// Calls the global `JSON.stringify(value, null, 2)`, returning `None` for
/// the (JSON-legal) case where stringification itself yields `undefined`
/// (e.g. stringifying `undefined` at the top level).
fn json_stringify(value: &JsValue, context: &mut Context) -> JsResult<Option<String>> {
    let json = context.global_object().get(js_string!("JSON"), context)?;
    let json = json
        .as_object()
        .expect("the JSON global object always exists");
    let stringify = json.get(js_string!("stringify"), context)?;
    let stringify = stringify
        .as_function()
        .expect("JSON.stringify is always a function");

    let result = stringify.call(
        &JsValue::undefined(),
        &[value.clone(), JsValue::null(), JsValue::from(2)],
        context,
    )?;

    Ok(if result.is_undefined() {
        None
    } else {
        Some(
            result
                .as_string()
                .expect("JSON.stringify returns a string")
                .to_std_string_escaped(),
        )
    })
}

#[cfg(test)]
mod tests {
    use boa_engine::JsString;
    use boa_engine::value::TryFromJs;

    use super::*;

    /// A fresh VM with no framebuffer bindings exercised, entry module
    /// already evaluated from `source`. Test programs read out results
    /// through `globalThis`, since assigning there is the simplest way for
    /// a plain script body to leave something this helper can inspect
    /// afterward.
    fn eval(source: &str) -> ElysiumRuntime {
        let mut runtime = ElysiumRuntime::new(Rc::new(RefCell::new(Vec::new())))
            .expect("failed to construct runtime");
        runtime
            .eval_module("test.ts", source)
            .expect("module failed to evaluate");
        runtime
    }

    fn global<T: TryFromJs>(runtime: &mut ElysiumRuntime, name: &str) -> T {
        let object = runtime.context.global_object();
        let value = object
            .get(JsString::from(name), &mut runtime.context)
            .expect("failed to read global");
        value
            .try_js_into(&mut runtime.context)
            .expect("failed to convert global")
    }

    #[test]
    fn set_timeout_fires_once_due() {
        let mut runtime =
            eval("globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 0);");
        assert!(!global::<bool>(&mut runtime, "fired"), "fired before due");
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&mut runtime, "fired"), "fired after due");
    }

    #[test]
    fn set_timeout_does_not_fire_before_its_delay() {
        let mut runtime = eval(
            "globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 60_000);",
        );
        runtime.run_due_timers().unwrap();
        assert!(!global::<bool>(&mut runtime, "fired"));
    }

    #[test]
    fn clear_timeout_prevents_firing() {
        let mut runtime = eval(
            "globalThis.fired = false; \
             const id = setTimeout(() => { globalThis.fired = true; }, 0); \
             clearTimeout(id);",
        );
        runtime.run_due_timers().unwrap();
        assert!(!global::<bool>(&mut runtime, "fired"));
    }

    #[test]
    fn set_interval_reschedules_until_cleared() {
        let mut runtime = eval(
            "globalThis.count = 0; \
             const id = setInterval(() => { \
                 globalThis.count += 1; \
                 if (globalThis.count >= 3) clearInterval(id); \
             }, 0);",
        );
        for _ in 0..5 {
            runtime.run_due_timers().unwrap();
        }
        assert_eq!(global::<f64>(&mut runtime, "count"), 3.0);
    }

    #[test]
    fn set_immediate_fires_on_next_tick() {
        let mut runtime =
            eval("globalThis.fired = false; setImmediate(() => { globalThis.fired = true; });");
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&mut runtime, "fired"));
    }

    #[test]
    fn request_animation_frame_receives_a_timestamp() {
        let mut runtime = eval(
            "globalThis.timestamp = -1; \
             requestAnimationFrame((t) => { globalThis.timestamp = t; });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<f64>(&mut runtime, "timestamp") >= 0.0);
    }

    #[test]
    fn cancel_animation_frame_prevents_firing() {
        let mut runtime = eval(
            "globalThis.fired = false; \
             const id = requestAnimationFrame(() => { globalThis.fired = true; }); \
             cancelAnimationFrame(id);",
        );
        runtime.run_due_timers().unwrap();
        assert!(!global::<bool>(&mut runtime, "fired"));
    }

    #[test]
    fn queue_microtask_runs_in_order_including_self_queued_work() {
        let mut runtime = eval(
            "globalThis.order = ''; \
             queueMicrotask(() => { \
                 globalThis.order += '1'; \
                 queueMicrotask(() => { globalThis.order += '2'; }); \
             }); \
             queueMicrotask(() => { globalThis.order += 'a'; });",
        );
        runtime.drain_microtasks();
        assert_eq!(global::<String>(&mut runtime, "order"), "1a2");
    }

    #[test]
    fn promise_then_resolves_after_draining_microtasks() {
        let mut runtime = eval(
            "globalThis.resolved = false; \
             Promise.resolve().then(() => { globalThis.resolved = true; });",
        );
        runtime.drain_microtasks();
        assert!(global::<bool>(&mut runtime, "resolved"));
    }

    #[test]
    fn timer_callback_can_use_a_promise() {
        let mut runtime = eval(
            "globalThis.resolved = false; \
             setTimeout(() => { \
                 Promise.resolve().then(() => { globalThis.resolved = true; }); \
             }, 0);",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&mut runtime, "resolved"));
    }

    #[test]
    fn async_function_resumes_after_an_awaited_timer_resolves() {
        let mut runtime = eval(
            "globalThis.done = false; \
             async function run() { \
                 await new Promise((resolve) => setTimeout(resolve, 0)); \
                 globalThis.done = true; \
             } \
             run();",
        );
        assert!(
            !global::<bool>(&mut runtime, "done"),
            "shouldn't resume yet"
        );
        runtime.run_due_timers().unwrap();
        assert!(
            global::<bool>(&mut runtime, "done"),
            "should resume once the awaited timer fires"
        );
    }

    #[test]
    fn async_function_return_value_is_observable_via_then() {
        let mut runtime = eval(
            "globalThis.result = ''; \
             async function greeting() { \
                 await Promise.resolve(); \
                 return 'hi'; \
             } \
             greeting().then((value) => { globalThis.result = value; });",
        );
        runtime.drain_microtasks();
        assert_eq!(global::<String>(&mut runtime, "result"), "hi");
    }

    #[test]
    fn draw_calls_outside_a_handler_throw_draw_outside_handler_error() {
        let mut runtime = eval(
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
        assert!(global::<bool>(&mut runtime, "threw"));
        assert!(global::<bool>(&mut runtime, "correctType"));
    }

    #[test]
    fn draw_calls_inside_a_registered_handler_succeed() {
        let mut runtime = eval(
            "import { clearScreen, addDrawHandler, Color } from 'ely:framebuffer'; \
             globalThis.drawn = false; \
             addDrawHandler(() => { \
                 clearScreen(Color.Slate900); \
                 globalThis.drawn = true; \
             });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&mut runtime, "drawn"));
    }

    #[test]
    fn update_ticker_fires_once_per_frame_with_a_delta_time() {
        let mut runtime = eval(
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
            assert_eq!(global::<f64>(&mut runtime, "calls"), expected_calls as f64);
            assert!(global::<f64>(&mut runtime, "lastDt") >= 0.0);
        }
    }

    #[test]
    fn remove_update_ticker_stops_further_calls() {
        let mut runtime = eval(
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
        assert_eq!(global::<f64>(&mut runtime, "calls"), 1.0);
    }

    #[test]
    fn post_init_handler_runs_once_after_eval_and_sees_working_timers() {
        let mut runtime = eval(
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
            !global::<bool>(&mut runtime, "ran"),
            "must not run during eval_module itself"
        );

        runtime.run_post_init_handlers().unwrap();
        assert!(global::<bool>(&mut runtime, "ran"));
        assert!(
            !global::<bool>(&mut runtime, "timerFired"),
            "the delay(0) timer hasn't been serviced yet"
        );

        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&mut runtime, "timerFired"));

        // A second call must not re-run anything: the handler list was
        // drained, not just iterated.
        runtime.run_post_init_handlers().unwrap();
    }
}
