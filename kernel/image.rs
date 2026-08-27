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
    image: LoadedImage,
}

/// A quantized image plus whether it has any transparency. The Framebuffer
/// blits a fully opaque image with a straight copy instead of a per-pixel
/// `source-over` blend.
#[derive(Clone)]
pub struct LoadedImage {
    pub pixmap: Rc<tiny_skia::Pixmap>,
    pub opaque: bool,
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

    /// Takes ownership of an already-quantized `pixmap` (with `opaque`
    /// recording whether it has any transparency), assigns it a fresh id,
    /// and returns the id.
    fn insert(&self, pixmap: tiny_skia::Pixmap, opaque: bool) -> u32 {
        let id = self.allocate_id();
        self.images.borrow_mut().push(ImageEntry {
            id,
            image: LoadedImage {
                pixmap: Rc::new(pixmap),
                opaque,
            },
        });
        id
    }

    fn get(&self, id: u32) -> Option<LoadedImage> {
        self.images
            .borrow()
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.image.clone())
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
pub fn resolve_image(ctx: &Ctx<'_>, images: &ImageTable, id: u32) -> Result<LoadedImage> {
    images
        .get(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a loaded image")))
}

/// Resolves `requested` — a virtual, userland-rooted path (e.g.
/// `/programs/init/sprite.png`) — against `canonical_userland_root`, and
/// verifies it doesn't escape it. `requested` must start with `/`: this
/// isn't itself the security boundary (a program can't reach this binding
/// except through `ely:image`'s `loadImage`, which already rejects a
/// non-absolute path as a `RelativePathError` before ever calling here) but
/// it keeps a caller that does reach this directly from silently getting a
/// path resolved relative to `canonical_userland_root` by accident.
/// Escaping the root — via `../` traversal or a symlink that resolves
/// outside it — is caught by canonicalizing the joined path and checking
/// `starts_with`, rather than string-matching `..`, which a symlink could
/// still get around.
fn resolve_userland_path(
    canonical_userland_root: &Path,
    requested: &str,
) -> std::result::Result<PathBuf, String> {
    let Some(relative) = requested.strip_prefix('/') else {
        return Err(format!("{requested} is not an absolute path"));
    };
    let candidate = canonical_userland_root.join(relative);
    let canonical_candidate =
        std::fs::canonicalize(&candidate).map_err(|err| format!("{requested}: {err}"))?;
    if !canonical_candidate.starts_with(canonical_userland_root) {
        return Err(format!("{requested} is outside the userland directory"));
    }
    Ok(canonical_candidate)
}

/// Snaps every pixel's straight (un-premultiplied) RGB to its nearest
/// palette entry via `Color::nearest`, then re-premultiplies by that
/// pixel's original alpha — alpha itself is never touched, so transparency
/// survives quantization untouched. Runs once, at load time, never per
/// frame. Also reports whether the image is fully opaque, so the
/// Framebuffer can blit it with a plain copy.
fn quantize_to_palette(mut pixmap: tiny_skia::Pixmap) -> (tiny_skia::Pixmap, bool) {
    let mut opaque = true;
    for pixel in pixmap.pixels_mut() {
        let straight = pixel.demultiply();
        opaque &= straight.alpha() == 255;
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
    (pixmap, opaque)
}

#[cfg(test)]
mod tests {
    use super::quantize_to_palette;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> tiny_skia::Pixmap {
        let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
        let color = tiny_skia::Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        pixmap.fill(color);
        pixmap
    }

    #[test]
    fn quantize_reports_a_fully_opaque_image_as_opaque() {
        let (_, opaque) = quantize_to_palette(solid(4, 4, [10, 20, 30, 255]));
        assert!(opaque);
    }

    #[test]
    fn quantize_reports_any_transparency_as_not_opaque() {
        let mut pixmap = solid(4, 4, [10, 20, 30, 255]);
        // Punch one pixel to partial alpha.
        pixmap.pixels_mut()[5] = tiny_skia::ColorU8::from_rgba(10, 20, 30, 128).premultiply();
        let (_, opaque) = quantize_to_palette(pixmap);
        assert!(!opaque);
    }
}

/// Binds the *hidden* globals `ely:image`'s embedded module wraps
/// (`__image_load`, `__image_width`, `__image_height`, `__image_unload`) —
/// never called by a program directly, only through `ely:image`'s exported
/// `loadImage`/`unloadImage`. `userland_root` is already canonicalized by
/// the caller ([`crate::runtime::ElysiumRuntime::new`]): every `loadImage`
/// call resolves its absolute, virtual path against this same canonical
/// root, never the process's current working directory and never wherever
/// the calling `.ts` file happens to live.
pub fn bootstrap_image_bindings(
    ctx: &Ctx<'_>,
    images: Rc<ImageTable>,
    userland_root: PathBuf,
) -> Result<()> {
    let global = ctx.globals();

    {
        let images = Rc::clone(&images);
        let userland_root = userland_root.clone();
        global.set(
            "__image_load",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, path: String| -> Result<u32> {
                    let resolved = resolve_userland_path(&userland_root, &path)
                        .map_err(|err| rquickjs::Exception::throw_message(&ctx, &err))?;
                    let bytes = std::fs::read(&resolved).map_err(|err| {
                        rquickjs::Exception::throw_message(&ctx, &format!("{path}: {err}"))
                    })?;
                    let pixmap = tiny_skia::Pixmap::decode_png(&bytes).map_err(|err| {
                        rquickjs::Exception::throw_message(&ctx, &format!("{path}: {err}"))
                    })?;
                    let (pixmap, opaque) = quantize_to_palette(pixmap);
                    Ok(images.insert(pixmap, opaque))
                },
            )?,
        )?;
    }

    {
        let images = Rc::clone(&images);
        global.set(
            "__image_width",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, id: u32| -> Result<u32> {
                Ok(resolve_image(&ctx, &images, id)?.pixmap.width())
            })?,
        )?;
    }

    {
        let images = Rc::clone(&images);
        global.set(
            "__image_height",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, id: u32| -> Result<u32> {
                Ok(resolve_image(&ctx, &images, id)?.pixmap.height())
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
