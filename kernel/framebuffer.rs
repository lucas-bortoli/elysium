//! The Framebuffer device: a CPU-rasterized drawing surface bound to a window.
//!
//! `Framebuffer` never touches `winit`'s event loop itself — it only ever
//! sees a window handle, handed to it by `kernel/window.rs`'s
//! `ElysiumWindow`, which is the kernel's one place that actually owns the
//! OS event loop. That's deliberate: a future Input device needs the same
//! window's keyboard/mouse events, and shouldn't have to reach through
//! Framebuffer to get them.
//!
//! Draw calls from JS never reach here directly either. [`bootstrap_framebuffer_bindings`]
//! binds `ely:framebuffer`'s hidden globals to push [`DrawCommand`]s onto a
//! plain `Vec` shared with the kernel's frame loop; only once a guarded
//! `draw()` call returns does that Vec get handed to [`Framebuffer::render`],
//! which is the only place in the kernel that rasterizes and presents a frame.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use rquickjs::{Ctx, Result};
use winit::dpi::PhysicalSize;
use winit::window::Window;

mod colors;
mod palette;
mod paths;
mod state;
pub use colors::Color;

use state::{Clip, ClipRect, DrawState};

use crate::bindings::bind;
use crate::text;

/// One drawing instruction accumulated during a program's `draw()` call.
/// Colors are always a [`Color`] from the fixed palette, never raw,
/// program-supplied RGBA channels. Not `Copy` — `DrawImage` carries an
/// `Arc<tiny_skia::Pixmap>`, already resolved from a JS-supplied image id at
/// the binding boundary (see `__framebuffer_draw_image` below), the same
/// way `resolve_color` resolves a color id before it ever reaches here.
///
/// The whole enum is `Send`: [`Framebuffer::render`] hands a shared reference
/// to one frame's list to every band thread, so an image's texels are reached
/// through an `Arc` rather than an `Rc`. Every other field is a plain value or
/// an already-`Send` tiny-skia type.
#[derive(Debug, Clone)]
pub enum DrawCommand {
    ClearScreen {
        color: Color,
    },
    FillRectangle {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    },
    DrawImage {
        pixmap: Arc<tiny_skia::Pixmap>,
        /// Fully opaque — blit with a plain copy, no `source-over` blend.
        opaque: bool,
        x: f32,
        y: f32,
    },
    /// Draws part of an image under a transform of its own — a source rect,
    /// a size, a flip, a turn. Kept apart from `DrawImage` so the common
    /// case of a whole image at its natural size stays a straight blit.
    DrawImageTransformed {
        pixmap: Arc<tiny_skia::Pixmap>,
        /// Fully opaque — write texels straight in, no `source-over` blend.
        opaque: bool,
        /// The part of the image to draw, in its own pixels.
        source: tiny_skia::Rect,
        /// Where that part lands, mapping the source rect's own top-left
        /// corner and size onto the surface.
        transform: tiny_skia::Transform,
    },
    DrawText {
        x: f32,
        y: f32,
        text: String,
        /// A built-in font id, already checked valid at the binding
        /// boundary the same way `color` is resolved there.
        font: text::FontId,
        /// How many pixels wide each of the font's own pixels is drawn.
        /// Whole numbers only, so a bigger size is the same bitmap with
        /// bigger pixels and stays as crisp as the font itself.
        scale: u32,
        color: Color,
    },
    /// Fills the inside of a path — the shape behind every filled circle,
    /// polygon, rounded rectangle and arc a program can draw. The path is
    /// built at the binding boundary, so an unfinishable one never becomes
    /// a command at all.
    FillPath {
        path: tiny_skia::Path,
        rule: tiny_skia::FillRule,
        color: Color,
    },
    /// Draws a line along a path, straddling it with the stroke's own
    /// width — the shape behind every outline.
    StrokePath {
        path: tiny_skia::Path,
        stroke: tiny_skia::Stroke,
        color: Color,
    },
    /// Sets a single pixel. Bypasses the rasterizer the way text does, so
    /// it applies the transform and clip in effect itself.
    SetPixel {
        x: f32,
        y: f32,
        color: Color,
    },
    /// Nests a transform inside whatever is already in effect, until the
    /// matching `PopTransform`. See `state.rs` for how the two stacks nest.
    PushTransform {
        transform: tiny_skia::Transform,
    },
    PopTransform,
    /// Narrows the clip to a rectangle in the current coordinates, until the
    /// matching `PopClip`. Stays a bare rectangle under an axis-aligned
    /// transform; a non-positive size confines drawing to nothing.
    PushClipRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// Narrows the clip to the inside of a path, until the matching
    /// `PopClip`. `None` confines it to nothing.
    PushClip {
        path: Option<tiny_skia::Path>,
        rule: tiny_skia::FillRule,
    },
    PopClip,
}

/// Binds the hidden globals `ely:framebuffer`'s embedded module wraps, path
/// bindings included. A program never names one of these: it calls the
/// module's exported `clearScreen`/`fillRectangle`/`drawText`/... , which
/// calls the matching global, which appends a [`DrawCommand`] to the buffer
/// the kernel renders once the guarded `draw()` call returns.
pub fn bootstrap_framebuffer_bindings(
    ctx: &Ctx<'_>,
    draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
    scale: Rc<Cell<u32>>,
    images: Rc<crate::image::ImageTable>,
) -> Result<()> {
    paths::bootstrap_path_bindings(
        ctx,
        Rc::clone(&draw_commands),
        Rc::new(RefCell::new(tiny_skia::PathBuilder::new())),
    )?;

    {
        let draw_commands = Rc::clone(&draw_commands);
        bind(ctx, "__framebuffer_pop_transform", move || {
            draw_commands.borrow_mut().push(DrawCommand::PopTransform)
        })?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_set_pixel",
            move |ctx: Ctx<'_>, x: f32, y: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::SetPixel { x, y, color });
                Ok(())
            },
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        // The six numbers of a 2x3 matrix, composed on the JS side from
        // whatever mix of shift, scale and rotation a program asked for.
        bind(
            ctx,
            "__framebuffer_push_transform",
            move |sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32| {
                let transform = tiny_skia::Transform::from_row(sx, ky, kx, sy, tx, ty);
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::PushTransform { transform })
            },
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_clear_screen",
            move |ctx: Ctx<'_>, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::ClearScreen { color });
                Ok(())
            },
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_fill_rectangle",
            move |ctx: Ctx<'_>, x: f32, y: f32, w: f32, h: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::FillRectangle { x, y, w, h, color });
                Ok(())
            },
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_draw_text",
            move |ctx: Ctx<'_>,
                  x: f32,
                  y: f32,
                  text: String,
                  font: u16,
                  scale: u32,
                  color: u16|
                  -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                if crate::text::font_from_id(font).is_none() {
                    return Err(rquickjs::Exception::throw_type(
                        &ctx,
                        &format!("{font} is not a valid font"),
                    ));
                }
                if scale == 0 {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        "text scale must be at least 1",
                    ));
                }
                draw_commands.borrow_mut().push(DrawCommand::DrawText {
                    x,
                    y,
                    text,
                    font,
                    scale,
                    color,
                });
                Ok(())
            },
        )?;
    }

    bind(
        ctx,
        "__framebuffer_measure_text",
        move |ctx: Ctx<'_>, text: String, font: u16| -> Result<Vec<u32>> {
            let font = crate::text::font_from_id(font).ok_or_else(|| {
                rquickjs::Exception::throw_type(&ctx, &format!("{font} is not a valid font"))
            })?;
            let (width, height) = crate::text::measure(font, &text);
            Ok(vec![width, height])
        },
    )?;

    {
        let draw_commands = Rc::clone(&draw_commands);
        let images = Rc::clone(&images);
        bind(
            ctx,
            "__framebuffer_draw_image",
            move |ctx: Ctx<'_>, id: u32, x: f32, y: f32| -> Result<()> {
                let image = crate::image::resolve_image(&ctx, &images, id)?;
                draw_commands.borrow_mut().push(DrawCommand::DrawImage {
                    pixmap: image.pixmap,
                    opaque: image.opaque,
                    x,
                    y,
                });
                Ok(())
            },
        )?;
    }

    bind(
        ctx,
        "__framebuffer_draw_image_transformed",
        move |ctx: Ctx<'_>, id: u32, source: Vec<f32>, transform: Vec<f32>| -> Result<()> {
            let image = crate::image::resolve_image(&ctx, &images, id)?;
            // `[x, y, w, h]` and the six numbers of a 2x3 matrix, both
            // assembled on the JS side — too many to pass one by one.
            let ([sx, sy, sw, sh], [a, b, c, d, e, f]) = (
                <[f32; 4]>::try_from(source.as_slice()).map_err(|_| {
                    rquickjs::Exception::throw_type(&ctx, "a source rect needs four numbers")
                })?,
                <[f32; 6]>::try_from(transform.as_slice()).map_err(|_| {
                    rquickjs::Exception::throw_type(&ctx, "a transform needs six numbers")
                })?,
            );
            let Some(source) = tiny_skia::Rect::from_xywh(sx, sy, sw, sh) else {
                return Ok(()); // nothing of the image asked for
            };
            draw_commands
                .borrow_mut()
                .push(DrawCommand::DrawImageTransformed {
                    pixmap: image.pixmap,
                    opaque: image.opaque,
                    source,
                    transform: tiny_skia::Transform::from_row(a, b, c, d, e, f),
                });
            Ok(())
        },
    )?;

    bind(ctx, "__framebuffer_nearest_color", |r: u8, g: u8, b: u8| {
        Color::nearest(r, g, b) as u16
    })?;

    bind(
        ctx,
        "__framebuffer_set_scale",
        move |ctx: Ctx<'_>, new_scale: u32| -> Result<()> {
            if new_scale == 0 {
                return Err(rquickjs::Exception::throw_range(
                    &ctx,
                    "scale must be at least 1",
                ));
            }
            scale.set(new_scale);
            Ok(())
        },
    )?;

    Ok(())
}

/// Resolves a numeric color id (as sent by one of `ely:framebuffer`'s generated
/// `RED_500`-style constants) to a [`Color`], throwing a `TypeError` if it's
/// out of range — only reachable if a program bypasses the generated
/// constants and passes an arbitrary number instead.
fn resolve_color(ctx: &Ctx<'_>, id: u16) -> Result<Color> {
    Color::from_id(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a valid color")))
}

/// The logical resolution programs draw in — independent of the window's
/// physical pixel size. Mirrored by hand in
/// `kernel/runtime_modules/framebuffer.ts`'s `getWidth`/`getHeight`; unlike
/// the color palette, which is generated from one table, nothing checks
/// these two agree, so change them together.
pub const FRAMEBUFFER_WIDTH: u32 = 720;
pub const FRAMEBUFFER_HEIGHT: u32 = 360;

/// The physical-pixels-per-logical-pixel ratio a `Framebuffer` starts at
/// before any `setScale` call. The live ratio is held in a runtime
/// `Cell<u32>` shared with `ely:framebuffer`'s `setScale` binding, so a
/// program can change it while Elysium is running.
pub const DEFAULT_SCALE: u32 = 2;

pub struct Framebuffer {
    // Always FRAMEBUFFER_WIDTH x FRAMEBUFFER_HEIGHT — logical resolution,
    // never the physical (scaled) window size. Programs already draw in
    // logical pixels, so every DrawCommand's x/y/w/h goes straight into
    // tiny-skia with no per-draw scaling math; the upscale to the window's
    // physical size happens once, in `present`.
    pixmap: tiny_skia::Pixmap,
    // Shared with `ely:framebuffer`'s `setScale` binding — the scale a
    // program most recently requested, checked once per `render` call.
    scale: Rc<Cell<u32>>,
    // The scale `pixmap`/`surface`/`window` are currently configured for.
    // Compared against `scale` each frame; `render` reconfigures everything
    // through `apply_scale` when the two differ.
    applied_scale: u32,
    // Needed to resize the OS window itself when the scale changes — see
    // `apply_scale`. `Framebuffer` never touches the event loop, only this
    // window handle.
    window: Arc<Window>,
    // Never read after construction, but must outlive `surface`: some
    // softbuffer backends (X11 in particular) hold a live connection this
    // surface's presents depend on for as long as it exists.
    #[allow(dead_code)]
    context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

impl Framebuffer {
    /// Allocates the logical-resolution `Pixmap` programs draw into and
    /// the softbuffer surface that presents it, sized to
    /// `FRAMEBUFFER_WIDTH * scale.get()` x `FRAMEBUFFER_HEIGHT * scale.get()`
    /// — not queried from `window`, since Elysium doesn't follow the OS's
    /// DPI scale factor (see `present`'s doc comment). `scale` is shared
    /// with `ely:framebuffer`'s `setScale` binding; `render` notices when
    /// it changes and reconfigures accordingly.
    pub fn new(window: Arc<Window>, scale: Rc<Cell<u32>>) -> Framebuffer {
        let applied_scale = scale.get();
        let physical_width = FRAMEBUFFER_WIDTH * applied_scale;
        let physical_height = FRAMEBUFFER_HEIGHT * applied_scale;

        let pixmap = tiny_skia::Pixmap::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT)
            .expect("failed to allocate the framebuffer's backing pixmap");

        let context = softbuffer::Context::new(Arc::clone(&window))
            .expect("failed to create a softbuffer context for the window");
        let mut surface = softbuffer::Surface::new(&context, Arc::clone(&window))
            .expect("failed to create a softbuffer surface for the window");
        surface
            .resize(
                NonZeroU32::new(physical_width).expect("scale is 0"),
                NonZeroU32::new(physical_height).expect("scale is 0"),
            )
            .expect("failed to size the softbuffer surface to the window");

        Framebuffer {
            pixmap,
            scale,
            applied_scale,
            window,
            context,
            surface,
        }
    }

    /// Draws one frame's worth of accumulated [`DrawCommand`]s and presents
    /// it. All the drawing itself happens in [`rasterize`]; what's left here
    /// is picking up a scale a program asked for since the last frame, and
    /// getting the finished pixels in front of the viewer.
    pub fn render(&mut self, commands: &[DrawCommand]) {
        let requested_scale = self.scale.get();
        if requested_scale != self.applied_scale {
            self.apply_scale(requested_scale);
        }

        let pool = crate::workers::Pool::global();
        rasterize_parallel(pool, &mut self.pixmap, commands);
        self.present(pool);
    }

    /// Reconfigures everything that depends on the physical-pixels-per-
    /// logical-pixel ratio for a newly requested `scale`: resizes the OS
    /// window to match (still not user-resizable — `.with_resizable(false)`
    /// only blocks resize via OS chrome, not a program calling this),
    /// and resizes the softbuffer surface to the new physical size. `pixmap`
    /// itself is untouched — it's always logical resolution, regardless of
    /// scale.
    fn apply_scale(&mut self, scale: u32) {
        self.applied_scale = scale;
        let physical_width = FRAMEBUFFER_WIDTH * scale;
        let physical_height = FRAMEBUFFER_HEIGHT * scale;

        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(physical_width, physical_height));
        self.surface
            .resize(
                NonZeroU32::new(physical_width).expect("scale is 0"),
                NonZeroU32::new(physical_height).expect("scale is 0"),
            )
            .expect("failed to resize the softbuffer surface");
    }

    /// Copies the logical `Pixmap` into the window's physical-resolution
    /// softbuffer surface, replicating each logical pixel into an
    /// `applied_scale` x `applied_scale` block. Assumes the window's
    /// actual physical size is exactly `FRAMEBUFFER_WIDTH * applied_scale`
    /// x `FRAMEBUFFER_HEIGHT * applied_scale` — Elysium doesn't follow the
    /// OS's DPI scale factor, so a host reporting one other than 1.0 will
    /// see a mismatched/clipped presentation.
    ///
    /// The upscale is spread across `pool`: one source row expands to a
    /// self-contained block of `scale` destination rows, so cutting the
    /// destination into runs of whole such blocks gives each thread a
    /// disjoint, contiguous slice to fill from a shared read-only view of the
    /// source.
    fn present(&mut self, pool: &crate::workers::Pool) {
        let mut buffer = self
            .surface
            .buffer_mut()
            .expect("failed to acquire the softbuffer back buffer");

        // Pixmap::data() is tightly packed RGBA8, row-major, premultiplied,
        // no row padding. Every pixel the rasterizer writes is a fully
        // opaque palette color, so premultiplied and straight bytes are the
        // same bytes and nothing here needs to un-premultiply.
        let src = self.pixmap.data();
        let src_w = self.pixmap.width() as usize;
        let src_h = self.pixmap.height() as usize;
        let scale = self.applied_scale as usize;
        let dst_w = src_w * scale;
        // One source row's worth of destination: `scale` rows of `dst_w`.
        let block = dst_w * scale;

        let pieces = band_count(src_h as u32, pool.thread_count());
        let base_rows = src_h / pieces;
        let extra = src_h % pieces;
        let mut ranges = Vec::with_capacity(pieces);
        let mut first = 0usize;
        for p in 0..pieces {
            let rows = base_rows + usize::from(p < extra);
            ranges.push((first, rows));
            first += rows;
        }

        let dst = BandBase(buffer.as_mut_ptr());
        pool.run(pieces, |p| {
            let (first_row, rows) = ranges[p];
            // SAFETY: `ranges` partitions the source rows into disjoint runs,
            // each mapping to `block` destination entries, and `run` gives
            // each `p` to one thread. `src` is shared read-only.
            let chunk = unsafe { dst.range(first_row * block, rows * block) };
            let mut scratch = vec![0u32; dst_w];
            for r in 0..rows {
                let sy = first_row + r;
                for sx in 0..src_w {
                    let i = (sy * src_w + sx) * 4;
                    let (rr, gg, bb) = (src[i], src[i + 1], src[i + 2]);
                    let color = (bb as u32) | ((gg as u32) << 8) | ((rr as u32) << 16); // packed XRGB
                    scratch[sx * scale..sx * scale + scale].fill(color);
                }
                for dy in 0..scale {
                    let start = (r * scale + dy) * dst_w;
                    chunk[start..start + dst_w].copy_from_slice(&scratch);
                }
            }
        });

        buffer.present().expect("failed to present the frame");
    }
}

/// Draws `commands` onto `pixmap`, in order.
///
/// Clearing is opt-in: a frame with no `ClearScreen` command leaves the
/// pixmap exactly as the previous frame left it, so a program that never
/// calls `clearScreen` sees each frame's drawing accumulate. The last
/// `ClearScreen` in `commands` wins, applied before any drawing, regardless
/// of where in the list it falls.
///
/// Nothing here anti-aliases. Every pixel written is one whole palette
/// color, never a blend of one with what was underneath, which is what lets
/// a program rely on the screen only ever holding colors it could have
/// named. Images are the one thing that composites at all, and their
/// transparency was already snapped to all-or-nothing when they were loaded;
/// one known fully opaque is copied straight in.
///
/// Separate from [`Framebuffer::render`] so that a frame can be drawn onto
/// any pixmap, with no window and no presentation — which is how the palette
/// promise above is tested. The kernel itself always goes through
/// [`rasterize_parallel`]; this is the single-band reference the banded
/// version is checked against.
#[allow(dead_code)]
pub fn rasterize(pixmap: &mut tiny_skia::Pixmap, commands: &[DrawCommand]) {
    let mut whole = pixmap.as_mut();
    rasterize_band(&mut whole, commands, 0);
}

/// How many bands to cut a `height`-row surface into for `threads` workers.
/// Capped so a band is never thinner than a floor: below it the fixed
/// per-band cost — a fresh [`DrawState`], the clear-colour scan, one pass
/// over the command list — stops being worth the extra thread.
fn band_count(height: u32, threads: usize) -> usize {
    const MIN_BAND_ROWS: u32 = 16;
    ((height / MIN_BAND_ROWS).max(1) as usize)
        .min(threads)
        .max(1)
}

/// A raw base pointer to a buffer a parallel pass writes. Shareable across
/// the band threads because the caller partitions the buffer into disjoint
/// ranges and [`crate::workers::Pool::run`] hands each range to one thread.
#[derive(Clone, Copy)]
struct BandBase<T>(*mut T);
unsafe impl<T> Send for BandBase<T> {}
unsafe impl<T> Sync for BandBase<T> {}

impl<T> BandBase<T> {
    /// The sub-slice `[offset, offset + len)` of the buffer.
    ///
    /// # Safety
    /// The caller must ensure this range is disjoint from every other range
    /// taken from the same base for the lifetime `'a` — which is what
    /// partitioning the buffer one-piece-per-thread guarantees.
    unsafe fn range<'a>(self, offset: usize, len: usize) -> &'a mut [T] {
        unsafe { std::slice::from_raw_parts_mut(self.0.add(offset), len) }
    }
}

/// Draws `commands` onto `pixmap` across the worker pool: the surface is cut
/// into horizontal bands and the whole command list is replayed against each
/// on its own thread. Bands share no pixels and each replays commands in list
/// order, so the frame is byte-for-byte what [`rasterize`] produces serially.
pub fn rasterize_parallel(
    pool: &crate::workers::Pool,
    pixmap: &mut tiny_skia::Pixmap,
    commands: &[DrawCommand],
) {
    let width = pixmap.width();
    let height = pixmap.height();
    let row_bytes = width as usize * 4;
    let bands = band_count(height, pool.thread_count());

    // Near-equal row splits, the remainder spread one row at a time over the
    // leading bands. Each entry: byte offset, byte length, top row, row count.
    let base_rows = height as usize / bands;
    let extra = height as usize % bands;
    let mut ranges = Vec::with_capacity(bands);
    let mut top = 0usize;
    for b in 0..bands {
        let rows = base_rows + usize::from(b < extra);
        ranges.push((top * row_bytes, rows * row_bytes, top as i32, rows as u32));
        top += rows;
    }

    // Take the base pointer without keeping a named `&mut [u8]` alive across
    // the parallel section — the band slices below are the only live borrows.
    let base = BandBase(pixmap.data_mut().as_mut_ptr());
    pool.run(bands, |b| {
        let (offset, len, top, rows) = ranges[b];
        // SAFETY: `ranges` partitions the pixmap bytes into disjoint spans and
        // `run` gives each `b` to one thread, so this is the only live
        // reference to that span.
        let bytes = unsafe { base.range(offset, len) };
        let mut band = tiny_skia::PixmapMut::from_bytes(bytes, width, rows)
            .expect("a band's bytes are a whole sub-pixmap");
        rasterize_band(&mut band, commands, top);
    });
}

/// Draws `commands` onto `band`, one horizontal slice of a surface whose top
/// row sits `band_top` rows below the surface's own top. Coordinates are
/// shifted up by `band_top` so a program's surface-space positions land in
/// the band's rows, and clip regions are sized to the band. Called once per
/// band, in parallel, by [`rasterize_parallel`]; [`rasterize`] calls it once
/// for the whole surface with `band_top` zero.
fn rasterize_band(band: &mut tiny_skia::PixmapMut<'_>, commands: &[DrawCommand], band_top: i32) {
    if let Some(color) = commands.iter().rev().find_map(|c| match c {
        DrawCommand::ClearScreen { color } => Some(*color),
        _ => None,
    }) {
        band.fill(color.to_skia());
    }

    let mut paint = tiny_skia::Paint {
        anti_alias: false,
        ..Default::default()
    };
    let blend_paint = tiny_skia::PixmapPaint::default();
    // An opaque image has nothing to composite, so copy its pixels straight
    // in rather than running `source-over` per pixel.
    let copy_paint = tiny_skia::PixmapPaint {
        blend_mode: tiny_skia::BlendMode::Source,
        ..tiny_skia::PixmapPaint::default()
    };

    let base = tiny_skia::Transform::from_translate(0.0, -(band_top as f32));
    let mut state = DrawState::with_base_transform(band.width(), band.height(), base);
    let band_h = band.height() as f32;

    for command in commands {
        match command {
            DrawCommand::ClearScreen { .. } => {}
            DrawCommand::FillRectangle { x, y, w, h, color } => {
                let Some(rect) = tiny_skia::Rect::from_xywh(*x, *y, *w, *h) else {
                    continue; // negative or non-finite size
                };
                let t = state.transform();
                let mb = mapped_bounds(rect, t);
                if outside_band(mb, band_h) {
                    continue;
                }
                paint.set_color(color.to_skia());
                let clip = state.clip_for(mb);
                band.fill_rect(rect, &paint, t, clip);
            }
            DrawCommand::FillPath { path, rule, color } => {
                let t = state.transform();
                let mb = mapped_bounds(path.bounds(), t);
                if outside_band(mb, band_h) {
                    continue;
                }
                paint.set_color(color.to_skia());
                let clip = state.clip_for(mb);
                band.fill_path(path, &paint, *rule, t, clip);
            }
            DrawCommand::StrokePath {
                path,
                stroke,
                color,
            } => {
                let t = state.transform();
                // The outline reaches half the stroke width past the path.
                let reach = stroke.width * 0.5;
                let outset = tiny_skia::Rect::from_ltrb(
                    path.bounds().left() - reach,
                    path.bounds().top() - reach,
                    path.bounds().right() + reach,
                    path.bounds().bottom() + reach,
                );
                let mb = outset.and_then(|rect| mapped_bounds(rect, t));
                if outside_band(mb, band_h) {
                    continue;
                }
                paint.set_color(color.to_skia());
                let clip = state.clip_for(mb);
                band.stroke_path(path, &paint, stroke, t, clip);
            }
            DrawCommand::SetPixel { x, y, color } => {
                let (px, py) = state.map_point(*x, *y);
                let (px, py) = (px.floor() as i32, py.floor() as i32);
                if px >= 0
                    && py >= 0
                    && px < band.width() as i32
                    && py < band.height() as i32
                    && state.is_visible(px, py)
                {
                    let hex = color.hex();
                    let width = band.width() as i32;
                    band.pixels_mut()[(py * width + px) as usize] = tiny_skia::ColorU8::from_rgba(
                        ((hex >> 16) & 0xff) as u8,
                        ((hex >> 8) & 0xff) as u8,
                        (hex & 0xff) as u8,
                        255,
                    )
                    .premultiply();
                }
            }
            DrawCommand::PushTransform { transform } => state.push_transform(*transform),
            DrawCommand::PopTransform => state.pop_transform(),
            DrawCommand::PushClipRect { x, y, w, h } => state.push_clip_rect(*x, *y, *w, *h),
            DrawCommand::PushClip { path, rule } => state.push_clip(path.as_ref(), *rule),
            DrawCommand::PopClip => state.pop_clip(),
            DrawCommand::DrawImage {
                pixmap: image,
                opaque,
                x,
                y,
            } => {
                // draw_pixmap places by whole pixels, so the transform's
                // shift has to be folded into the position itself.
                let (dx, dy) = state.map_point(*x, *y);
                let (dx, dy) = (dx.round(), dy.round());
                let bounds =
                    tiny_skia::Rect::from_xywh(dx, dy, image.width() as f32, image.height() as f32);
                if outside_band(bounds, band_h) {
                    continue;
                }
                let clip = state.clip_for(bounds);
                band.draw_pixmap(
                    dx as i32,
                    dy as i32,
                    (**image).as_ref(),
                    if *opaque { &copy_paint } else { &blend_paint },
                    tiny_skia::Transform::identity(),
                    clip,
                );
            }
            DrawCommand::DrawImageTransformed {
                pixmap: image,
                opaque,
                source,
                transform,
            } => {
                let full = state.transform().pre_concat(*transform);
                let footprint =
                    tiny_skia::Rect::from_xywh(0.0, 0.0, source.width(), source.height())
                        .and_then(|local| mapped_bounds(local, full));
                if outside_band(footprint, band_h) {
                    continue;
                }
                blit_transformed(band, image, *source, full, *opaque, state.clip());
            }
            DrawCommand::DrawText {
                x,
                y,
                text: string,
                font,
                scale,
                color,
            } => {
                let Some(font) = text::font_from_id(*font) else {
                    continue; // font id validated at the binding, but stay total
                };
                // Every lit pixel is one fully opaque palette color written
                // straight into the pixmap — no per-glyph `fill_rect`. Where
                // the text sits moves with the transform, but the glyphs
                // themselves are never turned or resized: they come from
                // fixed bitmaps and stay upright and pixel-crisp.
                let hex = color.hex();
                let solid = tiny_skia::ColorU8::from_rgba(
                    ((hex >> 16) & 0xff) as u8,
                    ((hex >> 8) & 0xff) as u8,
                    (hex & 0xff) as u8,
                    255,
                )
                .premultiply();
                let (ox, oy) = state.map_point(*x, *y);
                let (ox, oy) = (ox.round() as i32, oy.round() as i32);
                let scale = (*scale).max(1) as i32;
                let width = band.width() as i32;
                let height = band.height() as i32;
                // Skip a run of text that lands entirely above or below this
                // band — its glyphs are upright, so a plain height check does.
                let (_, text_h) = text::measure(font, string);
                if oy + text_h as i32 * scale <= 0 || oy >= height {
                    continue;
                }
                let pixels = band.pixels_mut();
                // Walked in the font's own pixels, so each one can be laid
                // down as a `scale` x `scale` block.
                text::for_each_lit_pixel(font, string, 0, 0, |gx, gy| {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = ox + gx * scale + dx;
                            let py = oy + gy * scale + dy;
                            if px >= 0
                                && py >= 0
                                && px < width
                                && py < height
                                && state.is_visible(px, py)
                            {
                                pixels[(py * width + px) as usize] = solid;
                            }
                        }
                    }
                });
            }
        }
    }
}

/// Whether a shape whose surface-space (band-local) vertical extent is
/// `[top, bottom)` falls entirely outside a band `band_h` rows tall, so the
/// band can skip the command outright. `None` bounds mean "extent unknown" —
/// never skipped. This is what keeps a call-bound frame (thousands of tiny
/// fills) from paying its whole command list once per band: a band only does
/// real work for the commands that reach its rows.
fn outside_band(bounds: Option<tiny_skia::Rect>, band_h: f32) -> bool {
    match bounds {
        Some(b) => b.bottom() <= 0.0 || b.top() >= band_h,
        None => false,
    }
}

/// The axis-aligned surface box a shape whose local bounds are `local` lands
/// in under `transform`. `None` when it has no area — a caller treats that as
/// "extent unknown" and clips conservatively.
fn mapped_bounds(
    local: tiny_skia::Rect,
    transform: tiny_skia::Transform,
) -> Option<tiny_skia::Rect> {
    let mut corners = [
        tiny_skia::Point::from_xy(local.left(), local.top()),
        tiny_skia::Point::from_xy(local.right(), local.top()),
        tiny_skia::Point::from_xy(local.right(), local.bottom()),
        tiny_skia::Point::from_xy(local.left(), local.bottom()),
    ];
    transform.map_points(&mut corners);
    let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|p| p.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max);
    tiny_skia::Rect::from_ltrb(min_x, min_y, max_x, max_y)
}

/// Draws `source` out of `image` onto `dest` under `transform`, which maps
/// the source rect's own coordinates onto the surface. Nearest-neighbour and
/// nothing else: every pixel written is a verbatim texel, so however the
/// image is turned or resized it stays on the palette, and there is no
/// sampler pipeline to pay for per pixel.
///
/// The walk is incremental. The source point under a surface pixel is an
/// affine function of that pixel, so it advances by a fixed vector per column
/// and another per row — a couple of adds a pixel, whether the image is
/// scaled, flipped or turned. `clip.bounds` tightens the walked box (all a
/// rectangular clip needs); `clip.mask`, when the region isn't a rectangle,
/// is tested per pixel on top of it.
fn blit_transformed(
    dest: &mut tiny_skia::PixmapMut<'_>,
    image: &tiny_skia::Pixmap,
    source: tiny_skia::Rect,
    transform: tiny_skia::Transform,
    opaque: bool,
    clip: Clip<'_>,
) {
    // `inverse` walks a surface pixel back to a point in the source rect's
    // own space; a degenerate transform has no inverse and covers nothing.
    let Some(inverse) = transform.invert() else {
        return;
    };
    let Some(footprint) = tiny_skia::Rect::from_xywh(0.0, 0.0, source.width(), source.height())
        .and_then(|local| mapped_bounds(local, transform))
    else {
        return;
    };

    let dest_w = dest.width() as i32;
    let dest_h = dest.height() as i32;
    let bounds = clip.bounds.unwrap_or(ClipRect {
        x0: 0,
        y0: 0,
        x1: dest_w,
        y1: dest_h,
    });
    // The image's footprint, the surface, and any rectangular clip at once.
    let x0 = (footprint.left().floor() as i32).max(0).max(bounds.x0);
    let y0 = (footprint.top().floor() as i32).max(0).max(bounds.y0);
    let x1 = (footprint.right().ceil() as i32).min(dest_w).min(bounds.x1);
    let y1 = (footprint.bottom().ceil() as i32)
        .min(dest_h)
        .min(bounds.y1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let src_w = image.width() as i32;
    let src_h = image.height() as i32;
    let (sl, st, sw, sh) = (source.left(), source.top(), source.width(), source.height());
    let src_pixels = image.pixels();
    let mask = clip.mask.map(|mask| mask.data());
    let dest_pixels = dest.pixels_mut();

    // How the source point moves for one pixel to the right.
    let (du_dx, dv_dx) = (inverse.sx, inverse.ky);

    for py in y0..y1 {
        // The source point under this row's first pixel centre; it then
        // advances by (du_dx, dv_dx) per column.
        let mut probe = [tiny_skia::Point::from_xy(x0 as f32 + 0.5, py as f32 + 0.5)];
        inverse.map_points(&mut probe);
        let (mut u, mut v) = (probe[0].x, probe[0].y);
        let row = (py * dest_w) as usize;
        for px in x0..x1 {
            let (su, sv) = (u, v);
            u += du_dx;
            v += dv_dx;
            let target = row + px as usize;
            if let Some(mask) = mask
                && mask[target] == 0
            {
                continue;
            }
            if su < 0.0 || su >= sw || sv < 0.0 || sv >= sh {
                continue;
            }
            let tx = (sl + su).floor() as i32;
            let ty = (st + sv).floor() as i32;
            if tx < 0 || tx >= src_w || ty < 0 || ty >= src_h {
                continue; // a fractional source rect can round one past its edge
            }
            let texel = src_pixels[(ty * src_w + tx) as usize];
            if opaque || texel.alpha() == 255 {
                dest_pixels[target] = texel;
            } else if texel.alpha() != 0 {
                dest_pixels[target] = over(texel, dest_pixels[target]);
            }
        }
    }
}

/// `source-over` of two premultiplied pixels. Images are quantized so every
/// texel is fully opaque or fully clear before they ever reach here, so this
/// runs only for a partial texel a test constructs by hand.
fn over(
    src: tiny_skia::PremultipliedColorU8,
    dst: tiny_skia::PremultipliedColorU8,
) -> tiny_skia::PremultipliedColorU8 {
    let inv = 255 - src.alpha() as u32;
    let blend = |s: u8, d: u8| (s as u32 + (d as u32 * inv + 127) / 255) as u8;
    tiny_skia::PremultipliedColorU8::from_rgba(
        blend(src.red(), dst.red()),
        blend(src.green(), dst.green()),
        blend(src.blue(), dst.blue()),
        blend(src.alpha(), dst.alpha()),
    )
    .unwrap_or(src)
}

#[cfg(test)]
mod tests {
    use super::{Color, DrawCommand, rasterize, rasterize_parallel};

    fn surface() -> tiny_skia::Pixmap {
        tiny_skia::Pixmap::new(64, 64).expect("failed to allocate a test surface")
    }

    /// The color at `(x, y)` as `0xRRGGBB`, the same packing `Color::hex`
    /// uses.
    fn pixel_at(pixmap: &tiny_skia::Pixmap, x: u32, y: u32) -> u32 {
        let p = pixmap.pixels()[(y * pixmap.width() + x) as usize].demultiply();
        ((p.red() as u32) << 16) | ((p.green() as u32) << 8) | p.blue() as u32
    }

    fn circle(cx: f32, cy: f32, r: f32) -> tiny_skia::Path {
        let mut builder = tiny_skia::PathBuilder::new();
        builder.push_circle(cx, cy, r);
        builder.finish().expect("a circle should be finishable")
    }

    fn rect_path(x: f32, y: f32, w: f32, h: f32) -> tiny_skia::Path {
        tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(x, y, w, h).expect("valid rect"),
        )
    }

    fn fill(path: tiny_skia::Path, color: Color) -> DrawCommand {
        DrawCommand::FillPath {
            path,
            rule: tiny_skia::FillRule::Winding,
            color,
        }
    }

    #[test]
    fn every_pixel_a_frame_leaves_behind_is_a_palette_color() {
        // The promise the whole device rests on: a program can only name
        // palette colors, so the screen can only ever hold palette colors.
        // Curves and outlines are where that would break if anything
        // anti-aliased or blended, so this draws the shapes most likely to.
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                fill(circle(20.0, 20.0, 15.0), Color::Amber400),
                DrawCommand::StrokePath {
                    path: circle(40.0, 40.0, 18.0),
                    stroke: tiny_skia::Stroke {
                        width: 3.0,
                        ..Default::default()
                    },
                    color: Color::Teal300,
                },
                DrawCommand::StrokePath {
                    path: rect_path(4.0, 44.0, 25.0, 15.0),
                    stroke: tiny_skia::Stroke {
                        width: 1.0,
                        ..Default::default()
                    },
                    color: Color::Rose500,
                },
            ],
        );

        assert_every_pixel_is_a_palette_color(&pixmap);
    }

    #[test]
    fn a_filled_path_paints_the_color_it_was_given() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                fill(circle(32.0, 32.0, 20.0), Color::Amber400),
            ],
        );
        assert_eq!(pixel_at(&pixmap, 32, 32), Color::Amber400.hex());
        assert_eq!(pixel_at(&pixmap, 0, 0), Color::Slate900.hex());
    }

    #[test]
    fn a_clip_confines_what_is_drawn_under_it() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::PushClip {
                    path: Some(rect_path(0.0, 0.0, 32.0, 64.0)),
                    rule: tiny_skia::FillRule::Winding,
                },
                fill(rect_path(0.0, 0.0, 64.0, 64.0), Color::Amber400),
                DrawCommand::PopClip,
            ],
        );
        assert_eq!(pixel_at(&pixmap, 10, 10), Color::Amber400.hex());
        assert_eq!(pixel_at(&pixmap, 50, 10), Color::Slate900.hex());
    }

    #[test]
    fn drawing_after_a_clip_is_popped_is_unconfined_again() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::PushClip {
                    path: Some(rect_path(0.0, 0.0, 32.0, 64.0)),
                    rule: tiny_skia::FillRule::Winding,
                },
                DrawCommand::PopClip,
                fill(rect_path(0.0, 0.0, 64.0, 64.0), Color::Amber400),
            ],
        );
        assert_eq!(pixel_at(&pixmap, 50, 10), Color::Amber400.hex());
    }

    #[test]
    fn a_transform_moves_what_is_drawn_under_it() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::PushTransform {
                    transform: tiny_skia::Transform::from_translate(40.0, 0.0),
                },
                fill(rect_path(0.0, 0.0, 10.0, 10.0), Color::Amber400),
                DrawCommand::PopTransform,
                // The same rectangle again, now that the shift is popped.
                fill(rect_path(0.0, 20.0, 10.0, 10.0), Color::Teal300),
            ],
        );
        assert_eq!(pixel_at(&pixmap, 45, 5), Color::Amber400.hex());
        assert_eq!(pixel_at(&pixmap, 5, 5), Color::Slate900.hex());
        assert_eq!(pixel_at(&pixmap, 5, 25), Color::Teal300.hex());
    }

    #[test]
    fn a_rectangle_fill_lands_on_exactly_the_pixels_its_corners_name() {
        // Coordinates name the corners of the pixel grid, so a rectangle at
        // (10, 10) sized 5x5 covers pixels 10 through 14 and no others.
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::FillRectangle {
                    x: 10.0,
                    y: 10.0,
                    w: 5.0,
                    h: 5.0,
                    color: Color::Amber400,
                },
            ],
        );
        assert_eq!(pixel_at(&pixmap, 10, 10), Color::Amber400.hex());
        assert_eq!(pixel_at(&pixmap, 14, 14), Color::Amber400.hex());
        assert_eq!(pixel_at(&pixmap, 15, 14), Color::Slate900.hex());
        assert_eq!(pixel_at(&pixmap, 9, 10), Color::Slate900.hex());
    }

    #[test]
    fn a_pixel_lands_on_the_pixel_its_coordinate_falls_inside() {
        // Coordinates name grid corners, so both of these name the same
        // pixel: the one between (3, 4) and (4, 5).
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::SetPixel {
                    x: 3.0,
                    y: 4.0,
                    color: Color::Amber400,
                },
                DrawCommand::SetPixel {
                    x: 10.9,
                    y: 10.1,
                    color: Color::Teal300,
                },
            ],
        );
        assert_eq!(pixel_at(&pixmap, 3, 4), Color::Amber400.hex());
        assert_eq!(pixel_at(&pixmap, 10, 10), Color::Teal300.hex());
        assert_eq!(pixel_at(&pixmap, 11, 10), Color::Slate900.hex());
    }

    #[test]
    fn a_pixel_outside_the_surface_or_a_clip_is_dropped() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::SetPixel {
                    x: -5.0,
                    y: 10.0,
                    color: Color::Amber400,
                },
                DrawCommand::SetPixel {
                    x: 1000.0,
                    y: 10.0,
                    color: Color::Amber400,
                },
                DrawCommand::PushClip {
                    path: Some(rect_path(0.0, 0.0, 10.0, 10.0)),
                    rule: tiny_skia::FillRule::Winding,
                },
                DrawCommand::SetPixel {
                    x: 20.0,
                    y: 20.0,
                    color: Color::Amber400,
                },
            ],
        );
        assert_eq!(lit_pixels(&pixmap, Color::Amber400), 0);
    }

    #[test]
    fn text_is_confined_by_a_clip_and_moved_by_a_transform() {
        // Text writes pixels directly instead of going through the
        // rasterizer, so it has to honour both stacks by itself.
        let unclipped = {
            let mut pixmap = surface();
            rasterize(
                &mut pixmap,
                &[
                    DrawCommand::ClearScreen {
                        color: Color::Slate900,
                    },
                    DrawCommand::DrawText {
                        x: 30.0,
                        y: 2.0,
                        text: "Hi".to_string(),
                        font: 0,
                        scale: 1,
                        color: Color::Amber400,
                    },
                ],
            );
            lit_pixels(&pixmap, Color::Amber400)
        };
        assert!(unclipped > 0, "the text should have drawn something");

        // Shifted right by 30 from the same origin, it starts past the clip
        // and none of it survives.
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::PushClip {
                    path: Some(rect_path(0.0, 0.0, 30.0, 64.0)),
                    rule: tiny_skia::FillRule::Winding,
                },
                DrawCommand::PushTransform {
                    transform: tiny_skia::Transform::from_translate(30.0, 0.0),
                },
                DrawCommand::DrawText {
                    x: 30.0,
                    y: 2.0,
                    text: "Hi".to_string(),
                    font: 0,
                    scale: 1,
                    color: Color::Amber400,
                },
            ],
        );
        assert_eq!(lit_pixels(&pixmap, Color::Amber400), 0);
    }

    #[test]
    fn scaled_text_lights_a_whole_block_per_font_pixel() {
        // Bigger text is the same bitmap with bigger pixels, so tripling the
        // scale lights exactly nine times as many.
        let lit_at = |scale: u32| {
            let mut pixmap = tiny_skia::Pixmap::new(256, 128).expect("test surface");
            rasterize(
                &mut pixmap,
                &[
                    DrawCommand::ClearScreen {
                        color: Color::Slate900,
                    },
                    DrawCommand::DrawText {
                        x: 2.0,
                        y: 2.0,
                        text: "Hi".to_string(),
                        font: 0,
                        scale,
                        color: Color::Amber400,
                    },
                ],
            );
            lit_pixels(&pixmap, Color::Amber400)
        };
        let single = lit_at(1);
        assert!(single > 0, "the text should have drawn something");
        assert_eq!(lit_at(3), single * 9);
    }

    /// A 4x4 image with a single distinct pixel at `(1, 1)`, so a source
    /// rect can be told apart from the whole image.
    fn marked_image() -> std::sync::Arc<tiny_skia::Pixmap> {
        let mut image = tiny_skia::Pixmap::new(4, 4).expect("test image");
        image.fill(Color::Teal300.to_skia());
        let hex = Color::Rose500.hex();
        image.pixels_mut()[1 * 4 + 1] = tiny_skia::ColorU8::from_rgba(
            ((hex >> 16) & 0xff) as u8,
            ((hex >> 8) & 0xff) as u8,
            (hex & 0xff) as u8,
            255,
        )
        .premultiply();
        std::sync::Arc::new(image)
    }

    fn identity() -> tiny_skia::Transform {
        tiny_skia::Transform::identity()
    }

    #[test]
    fn a_source_rect_crops_to_the_part_of_the_image_it_names() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: marked_image(),
                    opaque: true,
                    // Just the marked pixel.
                    source: tiny_skia::Rect::from_xywh(1.0, 1.0, 1.0, 1.0).unwrap(),
                    transform: tiny_skia::Transform::from_translate(10.0, 10.0),
                },
            ],
        );
        assert_eq!(pixel_at(&pixmap, 10, 10), Color::Rose500.hex());
        // One pixel wide, so its neighbours are untouched.
        assert_eq!(pixel_at(&pixmap, 11, 10), Color::Slate900.hex());
        assert_eq!(pixel_at(&pixmap, 9, 10), Color::Slate900.hex());
    }

    #[test]
    fn a_scaled_image_grows_by_whole_pixel_blocks() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: marked_image(),
                    opaque: true,
                    source: tiny_skia::Rect::from_xywh(1.0, 1.0, 1.0, 1.0).unwrap(),
                    transform: tiny_skia::Transform::from_row(4.0, 0.0, 0.0, 4.0, 10.0, 10.0),
                },
            ],
        );
        // The one marked pixel, four times the size.
        assert_eq!(lit_pixels(&pixmap, Color::Rose500), 16);
        assert_eq!(pixel_at(&pixmap, 13, 13), Color::Rose500.hex());
        assert_eq!(pixel_at(&pixmap, 14, 14), Color::Slate900.hex());
    }

    #[test]
    fn a_flipped_image_covers_the_same_box_the_other_way_round() {
        let placements = [
            // Unflipped: the marked pixel sits one in from the top-left.
            (identity(), (11, 11)),
            // Mirrored left to right within the same 4x4 box.
            (
                tiny_skia::Transform::from_row(-1.0, 0.0, 0.0, 1.0, 4.0, 0.0),
                (12, 11),
            ),
        ];
        for (flip, (x, y)) in placements {
            let mut pixmap = surface();
            rasterize(
                &mut pixmap,
                &[
                    DrawCommand::ClearScreen {
                        color: Color::Slate900,
                    },
                    DrawCommand::DrawImageTransformed {
                        pixmap: marked_image(),
                        opaque: true,
                        source: tiny_skia::Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
                        transform: tiny_skia::Transform::from_translate(10.0, 10.0)
                            .pre_concat(flip),
                    },
                ],
            );
            assert_eq!(pixel_at(&pixmap, x, y), Color::Rose500.hex());
            assert_eq!(lit_pixels(&pixmap, Color::Rose500), 1);
        }
    }

    #[test]
    fn a_turned_or_resized_image_still_only_paints_palette_colors() {
        // Turning and resizing are where a smoothing sampler would blend
        // neighbouring pixels into something no program could have named.
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: marked_image(),
                    opaque: true,
                    source: tiny_skia::Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
                    transform: tiny_skia::Transform::from_translate(20.0, 20.0)
                        .pre_concat(tiny_skia::Transform::from_rotate(37.0))
                        .pre_concat(tiny_skia::Transform::from_scale(3.5, 2.25)),
                },
            ],
        );
        assert_every_pixel_is_a_palette_color(&pixmap);
    }

    #[test]
    fn a_fractionally_scaled_axis_aligned_image_only_paints_palette_colors() {
        // The rotation-free fast path samples the source with its own
        // incremental map; a fractional scale is where an off-grid step
        // would land between texels if anything were being blended.
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: marked_image(),
                    opaque: true,
                    source: tiny_skia::Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
                    transform: tiny_skia::Transform::from_row(3.5, 0.0, 0.0, 2.25, 12.0, 9.0),
                },
            ],
        );
        assert_every_pixel_is_a_palette_color(&pixmap);
    }

    #[test]
    fn a_transformed_image_is_confined_by_a_clip() {
        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::PushClip {
                    // Only the left half of where the scaled image lands.
                    path: Some(rect_path(0.0, 0.0, 18.0, 64.0)),
                    rule: tiny_skia::FillRule::Winding,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: marked_image(),
                    opaque: true,
                    source: tiny_skia::Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
                    transform: tiny_skia::Transform::from_row(4.0, 0.0, 0.0, 4.0, 10.0, 10.0),
                },
                DrawCommand::PopClip,
            ],
        );
        // Inside the clip the image's fill shows; past it the background does.
        assert_eq!(pixel_at(&pixmap, 12, 12), Color::Teal300.hex());
        assert_eq!(pixel_at(&pixmap, 24, 12), Color::Slate900.hex());
    }

    #[test]
    fn a_clear_texel_in_a_transformed_image_leaves_the_background() {
        let mut image = tiny_skia::Pixmap::new(2, 2).expect("test image");
        // One opaque pixel at (0, 0); the rest fully transparent.
        image.pixels_mut()[0] = tiny_skia::ColorU8::from_rgba(
            ((Color::Rose500.hex() >> 16) & 0xff) as u8,
            ((Color::Rose500.hex() >> 8) & 0xff) as u8,
            (Color::Rose500.hex() & 0xff) as u8,
            255,
        )
        .premultiply();

        let mut pixmap = surface();
        rasterize(
            &mut pixmap,
            &[
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: std::sync::Arc::new(image),
                    opaque: false,
                    source: tiny_skia::Rect::from_xywh(0.0, 0.0, 2.0, 2.0).unwrap(),
                    transform: tiny_skia::Transform::from_row(5.0, 0.0, 0.0, 5.0, 10.0, 10.0),
                },
            ],
        );
        // The opaque corner drew; a transparent texel left the clear colour.
        assert_eq!(pixel_at(&pixmap, 12, 12), Color::Rose500.hex());
        assert_eq!(pixel_at(&pixmap, 20, 20), Color::Slate900.hex());
    }

    #[test]
    fn banding_the_surface_never_changes_a_pixel() {
        // Every command type, laid out to straddle band seams: a rectangle
        // spanning the full height, a circle and text crossing a seam, a
        // rotated clip and a rotated blit covering several bands, a lone
        // pixel on a seam row, and a transform stack left unbalanced.
        let commands = || {
            vec![
                DrawCommand::ClearScreen {
                    color: Color::Slate900,
                },
                DrawCommand::FillRectangle {
                    x: 8.0,
                    y: 0.0,
                    w: 20.0,
                    h: 200.0,
                    color: Color::Slate700,
                },
                fill(circle(40.0, 50.0, 22.0), Color::Amber400),
                fill(circle(30.0, 150.0, 18.0), Color::Sky400),
                DrawCommand::StrokePath {
                    path: circle(55.0, 100.0, 25.0),
                    stroke: tiny_skia::Stroke {
                        width: 3.0,
                        ..Default::default()
                    },
                    color: Color::Teal300,
                },
                DrawCommand::PushClipRect {
                    x: 10.0,
                    y: 40.0,
                    w: 40.0,
                    h: 60.0,
                },
                fill(rect_path(0.0, 0.0, 80.0, 200.0), Color::Rose500),
                DrawCommand::PopClip,
                DrawCommand::PushTransform {
                    transform: tiny_skia::Transform::from_translate(45.0, 120.0)
                        .pre_concat(tiny_skia::Transform::from_rotate(28.0)),
                },
                DrawCommand::PushClip {
                    path: Some(rect_path(-15.0, -15.0, 30.0, 30.0)),
                    rule: tiny_skia::FillRule::Winding,
                },
                DrawCommand::FillRectangle {
                    x: -20.0,
                    y: -20.0,
                    w: 40.0,
                    h: 40.0,
                    color: Color::Emerald400,
                },
                DrawCommand::PopClip,
                DrawCommand::PopTransform,
                DrawCommand::DrawText {
                    x: 4.0,
                    y: 92.0,
                    text: "band seam".to_string(),
                    font: 0,
                    scale: 2,
                    color: Color::Teal300,
                },
                DrawCommand::DrawImageTransformed {
                    pixmap: marked_image(),
                    opaque: true,
                    source: tiny_skia::Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
                    transform: tiny_skia::Transform::from_translate(20.0, 60.0)
                        .pre_concat(tiny_skia::Transform::from_rotate(41.0))
                        .pre_concat(tiny_skia::Transform::from_scale(9.0, 14.0)),
                },
                DrawCommand::SetPixel {
                    x: 60.0,
                    y: 50.0,
                    color: Color::Amber400,
                },
                // Popped one time too many — must not shift a band's rows.
                DrawCommand::PopTransform,
                DrawCommand::PopTransform,
            ]
        };

        let mut serial = tiny_skia::Pixmap::new(80, 200).expect("test surface");
        rasterize(&mut serial, &commands());

        let mut parallel = tiny_skia::Pixmap::new(80, 200).expect("test surface");
        let pool = crate::workers::Pool::with_threads(4);
        rasterize_parallel(&pool, &mut parallel, &commands());

        assert_eq!(
            serial.data(),
            parallel.data(),
            "banded rasterization diverged from the serial result"
        );
    }

    fn assert_every_pixel_is_a_palette_color(pixmap: &tiny_skia::Pixmap) {
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                let found = pixel_at(pixmap, x, y);
                let nearest = Color::nearest(
                    ((found >> 16) & 0xff) as u8,
                    ((found >> 8) & 0xff) as u8,
                    (found & 0xff) as u8,
                );
                assert_eq!(
                    found,
                    nearest.hex(),
                    "pixel at ({x}, {y}) is {found:#08x}, which is not a palette color"
                );
            }
        }
    }

    fn lit_pixels(pixmap: &tiny_skia::Pixmap, color: Color) -> usize {
        (0..pixmap.height())
            .flat_map(|y| (0..pixmap.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel_at(pixmap, x, y) == color.hex())
            .count()
    }
}
