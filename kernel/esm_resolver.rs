use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use boa_engine::module::{Module, ModuleLoader, Referrer};
use boa_engine::property::Attribute;
use boa_engine::{Context, JsNativeError, JsResult, JsString, Source, js_string};

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
/// anything on-disk module resolution would resolve a real import to.
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
pub fn bootstrap_jsx_runtime(context: &mut Context) -> JsResult<()> {
    let module = declare_embedded_module("jsx", context)?;
    module.load_link_evaluate(context).await_blocking(context)?;

    let h = module.get_value(js_string!("h"), context)?;
    let fragment = module.get_value(js_string!("Fragment"), context)?;

    let attribute = Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    context.register_global_property(js_string!("h"), h, attribute)?;
    context.register_global_property(js_string!("Fragment"), fragment, attribute)?;
    Ok(())
}

/// Compiles and parses (but doesn't evaluate) the embedded runtime module
/// registered under `name` in [`EMBEDDED_RUNTIME_MODULES`].
fn declare_embedded_module(name: &str, context: &mut Context) -> JsResult<Module> {
    let module_name = format!("{EMBEDDED_MODULE_SCHEME}{name}");
    let source = embedded_module_source(name).ok_or_else(|| {
        JsNativeError::typ().with_message(format!("unknown embedded module `{module_name}`"))
    })?;
    let compiled =
        transform::compile(source).map_err(|err| JsNativeError::syntax().with_message(err))?;
    let path = PathBuf::from(&module_name);
    let src = Source::from_bytes(compiled.as_bytes()).with_path(&path);
    Module::parse(src, None, context)
}

/// Resolves and loads a program's modules: an `ely:`-prefixed or bare
/// embedded-module-name specifier resolves to the matching
/// [`EMBEDDED_RUNTIME_MODULES`] entry; everything else is read from disk,
/// resolved relative to the referrer, trying the specifier as written, then
/// with a `.ts` suffix, then `.tsx` (programs import sibling TS(X) files by
/// bare name, e.g. `import x from "./foo"` resolving to `./foo.ts`). Either
/// way the source is compiled (JSX -> `h()`, then TypeScript erased) before
/// being handed to Boa; `import`/`export` are left alone by
/// `transform::compile`, so the compiled text is still valid module source.
pub struct ElysiumModuleLoader;

impl ModuleLoader for ElysiumModuleLoader {
    fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        specifier: JsString,
        context: &RefCell<&mut Context>,
    ) -> impl Future<Output = JsResult<Module>> {
        let result = (|| -> JsResult<Module> {
            let specifier = specifier.to_std_string_escaped();

            if let Some(embedded_name) = specifier.strip_prefix(EMBEDDED_MODULE_SCHEME) {
                return declare_embedded_module(embedded_name, &mut context.borrow_mut());
            }
            if !specifier.starts_with('.') && embedded_module_source(&specifier).is_some() {
                return declare_embedded_module(&specifier, &mut context.borrow_mut());
            }

            let path = resolve_file_path(referrer.path(), &specifier)?;
            let source = std::fs::read_to_string(&path).map_err(|err| {
                JsNativeError::typ()
                    .with_message(format!("could not read `{}`: {err}", path.display()))
            })?;
            let compiled = transform::compile(&source)
                .map_err(|err| JsNativeError::syntax().with_message(err))?;
            let src = Source::from_bytes(compiled.as_bytes()).with_path(&path);
            Module::parse(src, None, &mut context.borrow_mut())
        })();

        async { result }
    }
}

/// Resolves `specifier` relative to `referrer_path`'s directory, trying it
/// as written first, then with a `.ts` suffix, then `.tsx`.
fn resolve_file_path(referrer_path: Option<&Path>, specifier: &str) -> JsResult<PathBuf> {
    let referrer_dir = referrer_path
        .and_then(Path::parent)
        .unwrap_or(Path::new(""));
    let joined = referrer_dir.join(specifier);

    for candidate in [
        joined.clone(),
        joined.with_file_name(format!(
            "{}.ts",
            joined.file_name().unwrap_or_default().to_string_lossy()
        )),
        joined.with_file_name(format!(
            "{}.tsx",
            joined.file_name().unwrap_or_default().to_string_lossy()
        )),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(JsNativeError::typ()
        .with_message(format!("could not resolve module `{specifier}`"))
        .into())
}
