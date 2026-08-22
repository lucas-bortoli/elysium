use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::function::Rest;
use rquickjs::loader::{FileResolver, ImportAttributes, Loader, Resolver};
use rquickjs::{Context, Ctx, Error, Function, Module, Result, Runtime as JsRuntime, Type, Value};

use crate::transform;

/// TS(X) modules that are part of the Elysium VM itself rather than user
/// programs, so their source is baked into the executable at build time
/// instead of being read from disk at runtime (only *building* the VM needs
/// these files to exist under `runtime/`). Each is importable from any
/// program as `import ... from "<name>"`; to add another, drop the file
/// under `runtime/` and add an entry here.
const EMBEDDED_RUNTIME_MODULES: &[(&str, &str)] = &[("jsx", include_str!("../runtime/jsx.tsx"))];

/// Prefix marking a module name as one of [`EMBEDDED_RUNTIME_MODULES`]
/// rather than an on-disk path — chosen so it can never collide with
/// anything [`FileResolver`] would resolve a real import to.
const EMBEDDED_MODULE_SCHEME: &str = "elysium:";

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
pub enum GuardedError {
    Timeout,
    Exception(String),
}

/// Budget for one top-level module evaluation. Generous relative to a
/// future per-callback budget (e.g. a `love.update`/`love.draw`-style call)
/// since program initialization can legitimately take longer than a single
/// frame.
const DEFAULT_EVAL_BUDGET: Duration = Duration::from_secs(5);

pub struct ElysiumRuntime {
    _js_runtime: JsRuntime,
    context: Context,
    guard: Rc<GuardState>,
}

impl ElysiumRuntime {
    pub fn new() -> Result<Self> {
        let js_runtime = JsRuntime::new()?;

        let guard = Rc::new(GuardState::default());
        {
            let guard = Rc::clone(&guard);
            js_runtime.set_interrupt_handler(Some(Box::new(move || {
                match guard.deadline.get() {
                    Some(deadline) if Instant::now() >= deadline => {
                        guard.fired.set(true);
                        true
                    }
                    _ => false,
                }
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

        context.with(|ctx| -> Result<()> {
            let global = ctx.globals();
            global.set("print", Function::new(ctx.clone(), print)?)?;
            bootstrap_jsx_runtime(&ctx)?;
            Ok(())
        })?;

        Ok(Self {
            _js_runtime: js_runtime,
            context,
            guard,
        })
    }

    /// Compiles and evaluates `source` as an ES module named `name` (its
    /// path, used as the base for resolving any relative imports it has).
    pub fn eval_module(&self, name: &str, source: &str) -> std::result::Result<(), GuardedError> {
        let compiled = transform::compile(source).map_err(GuardedError::Exception)?;

        self.run_guarded(DEFAULT_EVAL_BUDGET, |ctx| {
            let (_module, promise) = Module::declare(ctx.clone(), name, compiled)?.eval()?;
            promise.finish::<()>()
        })
    }

    /// The one entry point every call into this VM goes through. Sets a
    /// deadline before running `f`, clears it after, and — on failure —
    /// distinguishes "the interrupt handler fired" (`Timeout`) from any
    /// other thrown/returned error (`Exception`) via [`GuardState::fired`].
    ///
    /// Forward-looking note: a future per-callback entry point (e.g. a
    /// `call_function(name, args)` for `love.update`/`love.draw`-style
    /// kernel calls) would call this same helper with a much tighter budget
    /// than `DEFAULT_EVAL_BUDGET` — no new timeout logic, just a new
    /// constant and closure body.
    fn run_guarded<T>(
        &self,
        budget: Duration,
        f: impl FnOnce(&Ctx<'_>) -> Result<T>,
    ) -> std::result::Result<T, GuardedError> {
        self.guard.fired.set(false);
        self.guard.deadline.set(Some(Instant::now() + budget));

        let result = self.context.with(|ctx| -> std::result::Result<T, GuardedError> {
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

/// Resolves a bare specifier matching one of [`EMBEDDED_RUNTIME_MODULES`] to
/// its virtual `elysium:`-prefixed name; everything else (relative imports,
/// unrecognized bare specifiers) falls through to the wrapped [`FileResolver`].
struct EmbeddedOrFileResolver(FileResolver);

impl Resolver for EmbeddedOrFileResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if !name.starts_with('.') && embedded_module_source(name).is_some() {
            return Ok(format!("{EMBEDDED_MODULE_SCHEME}{name}"));
        }
        self.0.resolve(ctx, base, name, attributes)
    }
}

/// Loads a module by name: an `elysium:`-prefixed name comes from
/// [`EMBEDDED_RUNTIME_MODULES`], anything else is read straight off disk.
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
