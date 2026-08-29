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
use std::path::PathBuf;
use std::rc::Rc;

use rquickjs::{Ctx, Function, Result};

use crate::filesystem;
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

    /// Drops every loaded image, so a VM never leaks loaded images past its
    /// own teardown. Called from `ElysiumRuntime`'s `Drop` impl, whose doc
    /// comment explains why the image table is released there alongside the
    /// timer and microtask queues even though it holds no `Persistent` JS
    /// values and so isn't subject to their GC-sweep-ordering hazard.
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

/// Snaps every pixel's straight (un-premultiplied) RGB to its nearest
/// palette entry via `Color::nearest`, and its alpha to fully transparent
/// or fully opaque, cutting at the midpoint. Both halves keep the same
/// promise: a pixel this image contributes to a frame is either absent or
/// an exact palette color, never a blend of one with whatever sits behind
/// it. Runs once, at load time, never per frame. Also reports whether the
/// image ends up fully opaque, so the Framebuffer can blit it with a plain
/// copy.
fn quantize_to_palette(mut pixmap: tiny_skia::Pixmap) -> (tiny_skia::Pixmap, bool) {
    let mut opaque = true;
    for pixel in pixmap.pixels_mut() {
        let straight = pixel.demultiply();
        let alpha = if straight.alpha() >= 128 { 255 } else { 0 };
        opaque &= alpha == 255;
        let matched = Color::nearest(straight.red(), straight.green(), straight.blue());
        let hex = matched.hex();
        let matched_straight = tiny_skia::ColorU8::from_rgba(
            ((hex >> 16) & 0xff) as u8,
            ((hex >> 8) & 0xff) as u8,
            (hex & 0xff) as u8,
            alpha,
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

    /// Replaces one pixel with `rgba` and quantizes, returning that same
    /// pixel back as straight (un-premultiplied) channels plus whether the
    /// whole image came out opaque.
    fn quantize_one(rgba: [u8; 4]) -> (tiny_skia::ColorU8, bool) {
        let mut pixmap = solid(4, 4, [10, 20, 30, 255]);
        pixmap.pixels_mut()[5] =
            tiny_skia::ColorU8::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3]).premultiply();
        let (pixmap, opaque) = quantize_to_palette(pixmap);
        (pixmap.pixels()[5].demultiply(), opaque)
    }

    #[test]
    fn quantize_reports_any_transparency_as_not_opaque() {
        let (_, opaque) = quantize_one([10, 20, 30, 8]);
        assert!(!opaque);
    }

    #[test]
    fn quantize_snaps_alpha_below_the_midpoint_to_transparent() {
        let (pixel, opaque) = quantize_one([10, 20, 30, 127]);
        assert_eq!(pixel.alpha(), 0);
        assert!(!opaque);
    }

    #[test]
    fn quantize_snaps_alpha_at_or_above_the_midpoint_to_opaque() {
        let (pixel, opaque) = quantize_one([10, 20, 30, 128]);
        assert_eq!(pixel.alpha(), 255);
        assert!(opaque);
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
                    let resolved = filesystem::resolve_userland_path(&userland_root, &path)
                        .map_err(|err| {
                            rquickjs::Exception::throw_message(&ctx, &err.to_string())
                        })?;
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
