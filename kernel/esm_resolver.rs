use std::path::{Path, PathBuf};

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
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
///
/// The ambient types userland typechecks programs against (`elysium.d.ts`)
/// describe this same surface independently, since it has no way to see
/// these bindings directly — keep the two in sync by hand when this list, or
/// any of these modules' exported signatures, changes. The one part that is
/// mechanized is the color palette, generated into both files from
/// `kernel/framebuffer/palette.rs` and checked by its tests.
const EMBEDDED_RUNTIME_MODULES: &[(&str, &str)] = &[
    ("jsx", include_str!("runtime_modules/jsx-runtime.ts")),
    (
        "framebuffer",
        include_str!("runtime_modules/framebuffer.ts"),
    ),
    ("lifecycle", include_str!("runtime_modules/lifecycle.ts")),
    ("math", include_str!("runtime_modules/math.ts")),
    ("input", include_str!("runtime_modules/input.ts")),
    ("image", include_str!("runtime_modules/image.ts")),
    ("filesystem", include_str!("runtime_modules/filesystem.ts")),
    ("container", include_str!("runtime_modules/container.ts")),
    ("process", include_str!("runtime_modules/process.ts")),
];

/// The namespace every VM-owned module lives under, whether a program
/// reaches it via an explicit `"ely:<name>"` import or (for `jsx`) an
/// internal bare-specifier rewrite — chosen so it can never collide with
/// anything a real on-disk import would resolve to.
const EMBEDDED_MODULE_SCHEME: &str = "ely:";

/// Extensions a relative import may be written with or without — tried in
/// order when `name` has no extension of its own, mirroring
/// `CompilingLoader`/[`crate::transform::compile`]'s TS(X) support.
const RELATIVE_MODULE_EXTENSIONS: &[&str] = &["ts", "tsx"];

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
/// it out explicitly (`"ely:framebuffer"`, passed through unchanged) or
/// because it's a bare specifier this rewrites internally (`"jsx"` ->
/// `"ely:jsx"`, today used only by `jsx`'s global bootstrap, not written by
/// programs). A relative import (`"./util.ts"`) resolves to a real file's
/// canonical path, the same sandboxed way `ely:image`'s `loadImage` resolves
/// an absolute one — see [`resolve_relative_module`]. Anything else (a bare
/// specifier naming neither) is an error.
pub struct EmbeddedOrFileResolver {
    userland_root: PathBuf,
}

impl EmbeddedOrFileResolver {
    pub fn new(userland_root: PathBuf) -> Self {
        Self { userland_root }
    }
}

impl Resolver for EmbeddedOrFileResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if let Some(embedded_name) = name.strip_prefix(EMBEDDED_MODULE_SCHEME) {
            if embedded_module_source(embedded_name).is_some() {
                return Ok(name.to_string());
            }
        } else if !name.starts_with('.') && embedded_module_source(name).is_some() {
            return Ok(format!("{EMBEDDED_MODULE_SCHEME}{name}"));
        }

        resolve_relative_module(&self.userland_root, base, name).ok_or_else(|| {
            Error::new_resolving_message(
                base,
                name,
                "does not resolve to a TS(X) file inside the userland directory",
            )
        })
    }
}

/// Resolves a relative specifier (`name`, e.g. `"./util.ts"` or `"../lib"`)
/// against `base` — the importing module's own real, absolute path — to the
/// real, canonical path of the file it names, or `None` if it doesn't
/// resolve to one. `name` may omit its extension, in which case each of
/// [`RELATIVE_MODULE_EXTENSIONS`] is tried in turn; an extension it does
/// carry must be one of those, exactly like [`FileResolver`]'s own pattern
/// matching. Canonicalizing and checking `starts_with` — rather than
/// string-matching `../` — is what actually rejects a specifier that
/// resolves outside `userland_root`, the same way `ely:image`'s `loadImage`
/// rejects an escaping absolute path: a symlink inside the tree that points
/// outside it is caught exactly the same way `../../../etc/passwd` is.
///
/// [`FileResolver`]: rquickjs::loader::FileResolver
fn resolve_relative_module(userland_root: &Path, base: &str, name: &str) -> Option<String> {
    let base_dir = Path::new(base).parent()?;
    let joined = base_dir.join(name);

    let candidate = match joined.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if RELATIVE_MODULE_EXTENSIONS.contains(&ext) => joined,
        Some(_) => return None,
        None => RELATIVE_MODULE_EXTENSIONS
            .iter()
            .map(|ext| joined.with_extension(ext))
            .find(|candidate| candidate.is_file())?,
    };

    let canonical = std::fs::canonicalize(&candidate).ok()?;
    canonical
        .starts_with(userland_root)
        .then(|| canonical.to_string_lossy().into_owned())
}

/// Loads a module by name: an `ely:`-prefixed name comes from
/// [`EMBEDDED_RUNTIME_MODULES`], anything else is read straight off disk.
/// Either way the source is compiled (JSX -> `h()`, then TypeScript erased)
/// before being handed to QuickJS; `import`/`export` are left alone by
/// `transform::compile`, so the compiled text is still valid module source.
/// `userland_root` (canonicalized) is used to give a file-backed module's
/// `import.meta.directoryName`/`fileName` a virtual, userland-rooted identity —
/// see [`set_virtual_import_meta`].
pub struct CompilingLoader {
    userland_root: PathBuf,
}

impl CompilingLoader {
    pub fn new(userland_root: PathBuf) -> Self {
        Self { userland_root }
    }
}

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
        let module = Module::declare(ctx.clone(), path, compiled)?;
        set_virtual_import_meta(&module, path, &self.userland_root)?;
        Ok(module)
    }
}

/// Sets `import.meta.directoryName`/`fileName` on `module` to its own location
/// expressed as a virtual path rooted at `userland_root` (e.g.
/// `/programs/init/index.ts`, never the real on-disk path) — the same
/// virtual root `ely:image`'s `loadImage` resolves an absolute path
/// against. `path` is the real absolute path `module` was declared under;
/// a `path` that doesn't canonicalize to somewhere inside `userland_root`
/// (an embedded `ely:` module, a test fixture outside the tree) is left
/// with no `import.meta` fields set at all, rather than a made-up value.
pub fn set_virtual_import_meta(
    module: &Module<'_, Declared>,
    path: &str,
    userland_root: &Path,
) -> Result<()> {
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return Ok(());
    };
    let Ok(relative) = canonical.strip_prefix(userland_root) else {
        return Ok(());
    };

    let file_name = format!("/{}", relative.to_string_lossy());
    let dir_name = match relative.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            format!("/{}", parent.to_string_lossy())
        }
        _ => "/".to_string(),
    };

    let meta = module.meta()?;
    meta.set("fileName", file_name)?;
    meta.set("directoryName", dir_name)?;
    Ok(())
}
