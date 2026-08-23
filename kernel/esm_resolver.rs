use rquickjs::loader::{FileResolver, ImportAttributes, Loader, Resolver};
use rquickjs::{Ctx, Error, Module, Result, Value};

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
    ("lifecycle", include_str!("runtime_modules/lifecycle.ts")),
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

/// Evaluates the embedded `"jsx"` runtime module and copies its exports
/// (`h`, `Fragment`) onto the global object, so every program gets them for
/// free instead of needing an explicit import.
pub fn bootstrap_jsx_runtime(ctx: &Ctx<'_>) -> Result<()> {
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

/// Resolves specifiers naming a [`EMBEDDED_RUNTIME_MODULES`] entry to its
/// canonical `ely:`-prefixed form — either because a program already wrote
/// it out explicitly (`"ely:framebuffer"`, passed through unchanged) or because
/// it's a bare specifier this rewrites internally (`"jsx"` -> `"ely:jsx"`,
/// today used only by `jsx`'s global bootstrap, not written by programs).
/// Everything else (relative imports, unrecognized bare specifiers) falls
/// through to the wrapped [`FileResolver`].
pub struct EmbeddedOrFileResolver(pub FileResolver);

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
pub struct CompilingLoader;

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
