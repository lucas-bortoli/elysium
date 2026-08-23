use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::function::Rest;
use rquickjs::loader::{FileResolver, ImportAttributes, Loader, Resolver};
use rquickjs::{
    Context, Ctx, Error, Function, Module, Persistent, Result, Runtime as JsRuntime, Type, Value,
};

use crate::framebuffer::DrawCommand;
use crate::timers::{TimerArgs, TimerQueue, bootstrap_timers};
use crate::transform;

/// TS(X) modules that belong to the VM itself rather than a user program, so
/// their source is baked into the executable at build time instead of being
/// read from disk at runtime (only *building* the VM needs these files to
/// exist under `runtime_modules/`). Every module the VM provides — today
/// `jsx` and `framebuffer` — lives under the one `ely:` namespace: `jsx` reaches
/// it through the bare-specifier rewrite below (and is additionally
/// bootstrapped as globals, see `bootstrap_jsx_runtime`), while `framebuffer` is
/// imported by a program writing the full `"ely:framebuffer"` specifier out
/// explicitly. To add another, drop the file under `runtime_modules/` and
/// add an entry here.
const EMBEDDED_RUNTIME_MODULES: &[(&str, &str)] = &[
    ("jsx", include_str!("runtime_modules/jsx-runtime.ts")),
    (
        "framebuffer",
        include_str!("runtime_modules/framebuffer.ts"),
    ),
    ("loop", include_str!("runtime_modules/loop.ts")),
];

/// The namespace every VM-owned module lives under, whether a program
/// reaches it via an explicit `"ely:<name>"` import or (for `jsx`) an
/// internal bare-specifier rewrite — chosen so it can never collide with
/// anything [`FileResolver`] would resolve a real on-disk import to.
const EMBEDDED_MODULE_SCHEME: &str = "ely:";

fn embedded_module_source(name: &str) -> Option<&'static str> {
    EMBEDDED_RUNTIME_MODULES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, source)| *source)
}

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
    guard: Rc<GuardState>,
    context: Context,
    js_runtime: JsRuntime,
}

impl ElysiumRuntime {
    /// `draw_commands` is the Framebuffer device's shared draw-command buffer:
    /// `ely:framebuffer`'s hidden globals push onto it directly rather than
    /// touching any drawing state themselves, keeping the VM's own bindings
    /// ignorant of `wgpu`.
    pub fn new(draw_commands: Rc<RefCell<Vec<DrawCommand>>>) -> Result<Self> {
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

        context.with(|ctx| -> Result<()> {
            let global = ctx.globals();
            global.set("print", Function::new(ctx.clone(), print)?)?;
            bootstrap_jsx_runtime(&ctx)?;
            bootstrap_framebuffer_bindings(&ctx, draw_commands)?;
            bootstrap_timers(&ctx, Rc::clone(&timers), Rc::clone(&microtasks))?;
            Ok(())
        })?;

        Ok(Self {
            js_runtime,
            context,
            guard,
            timers,
            microtasks,
        })
    }

    /// Compiles and evaluates `source` as an ES module named `name` (its
    /// path, used as the base for resolving any relative imports it has).
    /// Runs purely for its side effects — a program registers whatever
    /// per-frame work it wants (`ely:loop`'s `addUpdateTicker`,
    /// `ely:framebuffer`'s `addDrawHandler`) during evaluation, rather than
    /// exporting callbacks the kernel looks up afterward.
    pub fn eval_module(&self, name: &str, source: &str) -> std::result::Result<(), GuardedError> {
        let compiled = transform::compile(source).map_err(GuardedError::Exception)?;

        self.run_guarded(DEFAULT_EVAL_BUDGET, |ctx| {
            let (_module, promise) = Module::declare(ctx.clone(), name, compiled)?.eval()?;
            promise.finish::<()>()
        })
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
        });
    }
}

/// Evaluates the embedded `"jsx"` runtime module and copies its exports
/// (`h`, `Fragment`) onto the global object, so every program gets them for
/// free instead of needing an explicit import.
fn bootstrap_jsx_runtime(ctx: &Ctx<'_>) -> Result<()> {
    let (module, promise) = declare_embedded_module(ctx, "jsx")?.eval()?;
    promise.finish::<()>()?;

    let namespace = module.namespace()?;
    let global = ctx.globals();
    global.set("h", namespace.get::<_, Value>("h")?)?;
    global.set("Fragment", namespace.get::<_, Value>("Fragment")?)?;
    Ok(())
}

/// Binds the *hidden* globals `ely:framebuffer`'s embedded module wraps
/// (`__framebuffer_clear_screen`, `__framebuffer_fill_rectangle`) — never called by
/// a program directly, only through `ely:framebuffer`'s exported
/// `clearScreen`/`fillRectangle`. Each closure just resolves its numeric
/// color id to a [`Color`] and pushes a [`DrawCommand`] onto the shared
/// buffer; neither one touches any drawing state itself, so this file never
/// needs to know anything about `wgpu`.
fn bootstrap_framebuffer_bindings(
    ctx: &Ctx<'_>,
    draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
) -> Result<()> {
    let global = ctx.globals();

    {
        let draw_commands = Rc::clone(&draw_commands);
        global.set(
            "__framebuffer_clear_screen",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::ClearScreen { color });
                Ok(())
            })?,
        )?;
    }

    global.set(
        "__framebuffer_fill_rectangle",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'_>, x: f32, y: f32, w: f32, h: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::FillRectangle { x, y, w, h, color });
                Ok(())
            },
        )?,
    )?;

    Ok(())
}

/// Resolves a numeric color id (as sent by one of `ely:framebuffer`'s generated
/// `RED_500`-style constants) to a [`Color`], throwing a `TypeError` if it's
/// out of range — only reachable if a program bypasses the generated
/// constants and passes an arbitrary number instead.
fn resolve_color(ctx: &Ctx<'_>, id: u16) -> Result<crate::framebuffer::Color> {
    crate::framebuffer::Color::from_id(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a valid color")))
}

/// Compiles and declares (but doesn't evaluate) the embedded runtime module
/// registered under `name` in [`EMBEDDED_RUNTIME_MODULES`].
fn declare_embedded_module<'js>(ctx: &Ctx<'js>, name: &str) -> Result<Module<'js>> {
    let module_name = format!("{EMBEDDED_MODULE_SCHEME}{name}");
    let source = embedded_module_source(name).ok_or_else(|| Error::new_loading(&module_name))?;
    let compiled =
        transform::compile(source).map_err(|err| Error::new_loading_message(&module_name, err))?;
    Module::declare(ctx.clone(), module_name, compiled)
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

/// Resolves specifiers naming a [`EMBEDDED_RUNTIME_MODULES`] entry to its
/// canonical `ely:`-prefixed form — either because a program already wrote
/// it out explicitly (`"ely:framebuffer"`, passed through unchanged) or because
/// it's a bare specifier this rewrites internally (`"jsx"` -> `"ely:jsx"`,
/// today used only by `jsx`'s global bootstrap, not written by programs).
/// Everything else (relative imports, unrecognized bare specifiers) falls
/// through to the wrapped [`FileResolver`].
struct EmbeddedOrFileResolver(FileResolver);

impl Resolver for EmbeddedOrFileResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if let Some(embedded_name) = name.strip_prefix(EMBEDDED_MODULE_SCHEME) {
            if embedded_module_source(embedded_name).is_some() {
                return Ok(name.to_string());
            }
        } else if !name.starts_with('.') && embedded_module_source(name).is_some() {
            return Ok(format!("{EMBEDDED_MODULE_SCHEME}{name}"));
        }
        self.0.resolve(ctx, base, name, attributes)
    }
}

/// Loads a module by name: an `ely:`-prefixed name comes from
/// Either way the source is compiled (JSX -> `h()`, then TypeScript erased)
/// before being handed to QuickJS; `import`/`export` are left alone by
/// `transform::compile`, so the compiled text is still valid module source.
struct CompilingLoader;

impl Loader for CompilingLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        path: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js>> {
        if let Some(name) = path.strip_prefix(EMBEDDED_MODULE_SCHEME) {
            return declare_embedded_module(ctx, name);
        }

        let source = std::fs::read_to_string(path)
            .map_err(|err| Error::new_loading_message(path, err.to_string()))?;
        let compiled =
            transform::compile(&source).map_err(|err| Error::new_loading_message(path, err))?;
        Module::declare(ctx.clone(), path, compiled)
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::FromJs;

    use super::*;

    /// A fresh VM with no framebuffer bindings exercised, entry module
    /// already evaluated from `source`. Test programs read out results
    /// through `globalThis`, since assigning there (rather than exporting)
    /// is the simplest way for a plain script body to leave something this
    /// helper can inspect afterward.
    fn eval(source: &str) -> ElysiumRuntime {
        let runtime = ElysiumRuntime::new(Rc::new(RefCell::new(Vec::new())))
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
            "import { clearScreen, addDrawHandler, SLATE_900 } from 'ely:framebuffer'; \
             globalThis.drawn = false; \
             addDrawHandler(() => { \
                 clearScreen(SLATE_900); \
                 globalThis.drawn = true; \
             });",
        );
        runtime.run_due_timers().unwrap();
        assert!(global::<bool>(&runtime, "drawn"));
    }

    #[test]
    fn update_ticker_fires_once_per_frame_with_a_delta_time() {
        let runtime = eval(
            "import { addUpdateTicker } from 'ely:loop'; \
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
            "import { addUpdateTicker, removeUpdateTicker } from 'ely:loop'; \
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
}
