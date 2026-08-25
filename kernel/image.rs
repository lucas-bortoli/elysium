//! Loading PNGs off disk into images a program can draw with the
//! Framebuffer device (see `kernel/framebuffer.rs`'s `DrawCommand::DrawImage`).
//!
//! Loaded images are tracked in a per-VM [`ImageTable`], keyed by an
//! incrementing numeric id handed back to `ely:image`'s `loadImage` —
//! mirroring `kernel/timers.rs`'s `TimerQueue` exactly (a wrapping
//! `Cell<u32>` counter starting at 1, a `RefCell<Vec<_>>` scanned linearly,
//! `retain`-based removal). This is deliberately *not* a JS-visible
//! refcounted/finalizer-backed resource: `crate::runtime::ElysiumRuntime`'s
//! `Drop` impl already documents that freeing values from inside a native
//! finalizer during QuickJS's own GC sweep isn't safely supported, and
//! nothing else in this codebase relies on that pattern.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rquickjs::{Ctx, Function, Result};

use crate::framebuffer::Color;

struct ImageEntry {
    id: u32,
    pixmap: Rc<tiny_skia::Pixmap>,
}

/// The VM's set of loaded images. See the module doc comment for why this
/// mirrors `TimerQueue` rather than being JS-refcounted.
pub struct ImageTable {
    next_id: Cell<u32>,
    images: RefCell<Vec<ImageEntry>>,
}

impl ImageTable {
    pub fn new() -> Self {
        Self {
            next_id: Cell::new(1),
            images: RefCell::new(Vec::new()),
        }
    }

    fn allocate_id(&self) -> u32 {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(1));
        id
    }

    /// Takes ownership of an already-quantized `pixmap`, assigns it a fresh
    /// id, and returns the id.
    fn insert(&self, pixmap: tiny_skia::Pixmap) -> u32 {
        let id = self.allocate_id();
        self.images.borrow_mut().push(ImageEntry {
            id,
            pixmap: Rc::new(pixmap),
        });
        id
    }

    fn get(&self, id: u32) -> Option<Rc<tiny_skia::Pixmap>> {
        self.images
            .borrow()
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| Rc::clone(&entry.pixmap))
    }

    fn remove(&self, id: u32) {
        self.images.borrow_mut().retain(|entry| entry.id != id);
    }

    /// Drops every loaded image. Used by `ElysiumRuntime`'s `Drop` impl,
    /// alongside `timers.clear_all()`/`microtasks.borrow_mut().clear()`, so
    /// a VM never leaks loaded images past its own teardown. Unlike those
    /// two, no entry here holds a `Persistent` JS value, so nothing here
    /// actually needs to run inside `context.with` for GC-safety — it's
    /// grouped with them purely so VM teardown has one obvious place every
    /// resource gets released, not because of the same hazard.
    pub fn clear_all(&self) {
        self.images.borrow_mut().clear();
    }
}

/// Resolves a numeric image id to its `Pixmap`, throwing a `TypeError` if
/// it's never been loaded (or was already unloaded) — mirrors
/// `framebuffer::resolve_color`'s handling of an out-of-range color id.
/// Used both by `__image_width`/`__image_height` below and by
/// `framebuffer::bootstrap_framebuffer_bindings`'s `__framebuffer_draw_image`.
pub fn resolve_image(ctx: &Ctx<'_>, images: &ImageTable, id: u32) -> Result<Rc<tiny_skia::Pixmap>> {
    images
        .get(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a loaded image")))
}

/// Resolves `requested` against `canonical_program_dir` and verifies it
/// doesn't escape it. `PathBuf::join` with an absolute `requested` (e.g.
/// `/etc/passwd`) discards `canonical_program_dir` entirely and returns the
/// absolute path verbatim, so canonicalizing and then checking
/// `starts_with` is what actually catches that case — along with any `../`
/// traversal and any symlink that resolves outside the directory — rather
/// than string-matching `..`/a leading `/`, which a symlink could still get
/// around.
fn resolve_program_path(
    canonical_program_dir: &Path,
    requested: &str,
) -> std::result::Result<PathBuf, String> {
    let candidate = canonical_program_dir.join(requested);
    let canonical_candidate =
        std::fs::canonicalize(&candidate).map_err(|err| format!("{requested}: {err}"))?;
    if !canonical_candidate.starts_with(canonical_program_dir) {
        return Err(format!("{requested} is outside the program's directory"));
    }
    Ok(canonical_candidate)
}

/// Snaps every pixel's straight (un-premultiplied) RGB to its nearest
/// palette entry via `Color::nearest`, then re-premultiplies by that
/// pixel's original alpha — alpha itself is never touched, so transparency
/// survives quantization untouched. Runs once, at load time, never per
/// frame.
fn quantize_to_palette(mut pixmap: tiny_skia::Pixmap) -> tiny_skia::Pixmap {
    for pixel in pixmap.pixels_mut() {
        let straight = pixel.demultiply();
        let matched = Color::nearest(straight.red(), straight.green(), straight.blue());
        let hex = matched.hex();
        let matched_straight = tiny_skia::ColorU8::from_rgba(
            ((hex >> 16) & 0xff) as u8,
            ((hex >> 8) & 0xff) as u8,
            (hex & 0xff) as u8,
            straight.alpha(),
        );
        *pixel = matched_straight.premultiply();
    }
    pixmap
}

/// Binds the *hidden* globals `ely:image`'s embedded module wraps
/// (`__image_load`, `__image_width`, `__image_height`, `__image_unload`) —
/// never called by a program directly, only through `ely:image`'s exported
/// `loadImage`/`unloadImage`. `program_dir` is canonicalized once here, up
/// front: every `loadImage` call resolves against this same canonical
/// root, never the process's current working directory and never wherever
/// the calling `.ts` file happens to live.
pub fn bootstrap_image_bindings(
    ctx: &Ctx<'_>,
    images: Rc<ImageTable>,
    program_dir: PathBuf,
) -> Result<()> {
    let canonical_program_dir = std::fs::canonicalize(&program_dir).unwrap_or_else(|err| {
        panic!(
            "program directory {} is invalid: {err}",
            program_dir.display()
        )
    });
    let global = ctx.globals();

    {
        let images = Rc::clone(&images);
        let program_dir = canonical_program_dir.clone();
        global.set(
            "__image_load",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, path: String| -> Result<u32> {
                    let resolved = resolve_program_path(&program_dir, &path)
                        .map_err(|err| rquickjs::Exception::throw_message(&ctx, &err))?;
                    let bytes = std::fs::read(&resolved).map_err(|err| {
                        rquickjs::Exception::throw_message(&ctx, &format!("{path}: {err}"))
                    })?;
                    let pixmap = tiny_skia::Pixmap::decode_png(&bytes).map_err(|err| {
                        rquickjs::Exception::throw_message(&ctx, &format!("{path}: {err}"))
                    })?;
                    Ok(images.insert(quantize_to_palette(pixmap)))
                },
            )?,
        )?;
    }

    {
        let images = Rc::clone(&images);
        global.set(
            "__image_width",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, id: u32| -> Result<u32> {
                Ok(resolve_image(&ctx, &images, id)?.width())
            })?,
        )?;
    }

    {
        let images = Rc::clone(&images);
        global.set(
            "__image_height",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, id: u32| -> Result<u32> {
                Ok(resolve_image(&ctx, &images, id)?.height())
            })?,
        )?;
    }

    global.set(
        "__image_unload",
        Function::new(ctx.clone(), move |id: u32| {
            images.remove(id);
        })?,
    )?;

    Ok(())
}
