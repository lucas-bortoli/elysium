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

use rquickjs::{Ctx, Function, Result};
use winit::dpi::PhysicalSize;
use winit::window::Window;

mod colors;
mod paths;
mod state;
pub use colors::Color;

use state::DrawState;

use crate::text;

/// One drawing instruction accumulated during a program's `draw()` call.
/// Colors are always a [`Color`] from the fixed palette, never raw,
/// program-supplied RGBA channels. Not `Copy` — `DrawImage` carries an
/// `Rc<tiny_skia::Pixmap>`, already resolved from a JS-supplied image id at
/// the binding boundary (see `__framebuffer_draw_image` below), the same
/// way `resolve_color` resolves a color id before it ever reaches here.
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
        pixmap: Rc<tiny_skia::Pixmap>,
        /// Fully opaque — blit with a plain copy, no `source-over` blend.
        opaque: bool,
        x: f32,
        y: f32,
    },
    DrawText {
        x: f32,
        y: f32,
        text: String,
        /// A built-in font id, already checked valid at the binding
        /// boundary the same way `color` is resolved there.
        font: text::FontId,
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
    /// Narrows the region drawing is confined to, until the matching
    /// `PopClip`. `None` confines it to nothing.
    PushClip {
        path: Option<tiny_skia::Path>,
        rule: tiny_skia::FillRule,
    },
    PopClip,
}

/// Binds the *hidden* globals `ely:framebuffer`'s embedded module wraps
/// (`__framebuffer_clear_screen`, `__framebuffer_fill_rectangle`,
/// `__framebuffer_draw_text`, `__framebuffer_measure_text`,
/// `__framebuffer_draw_image`, `__framebuffer_nearest_color`,
/// `__framebuffer_set_scale`) — never called by a program directly, only
/// through `ely:framebuffer`'s exported
/// `clearScreen`/`fillRectangle`/`drawText`/`measureText`/`drawImage`/`nearestColor`/`setScale`.
pub fn bootstrap_framebuffer_bindings(
    ctx: &Ctx<'_>,
    draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
    scale: Rc<Cell<u32>>,
    images: Rc<crate::image::ImageTable>,
) -> Result<()> {
    let global = ctx.globals();

    paths::bootstrap_path_bindings(
        ctx,
        Rc::clone(&draw_commands),
        Rc::new(RefCell::new(tiny_skia::PathBuilder::new())),
    )?;

    {
        let draw_commands = Rc::clone(&draw_commands);
        global.set(
            "__framebuffer_set_pixel",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, x: f32, y: f32, color: u16| -> Result<()> {
                    let color = resolve_color(&ctx, color)?;
                    draw_commands
                        .borrow_mut()
                        .push(DrawCommand::SetPixel { x, y, color });
                    Ok(())
                },
            )?,
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        global.set(
            "__framebuffer_push_transform",
            Function::new(
                ctx.clone(),
                // The six numbers of a 2x3 matrix, composed on the JS side
                // from whatever mix of shift, scale and rotation a program
                // asked for.
                move |sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32| {
                    let transform = tiny_skia::Transform::from_row(sx, ky, kx, sy, tx, ty);
                    draw_commands
                        .borrow_mut()
                        .push(DrawCommand::PushTransform { transform })
                },
            )?,
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        global.set(
            "__framebuffer_pop_transform",
            Function::new(ctx.clone(), move || {
                draw_commands.borrow_mut().push(DrawCommand::PopTransform)
            })?,
        )?;
    }

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

    {
        let draw_commands = Rc::clone(&draw_commands);
        global.set(
            "__framebuffer_fill_rectangle",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, x: f32, y: f32, w: f32, h: f32, color: u16| -> Result<()> {
                    let color = resolve_color(&ctx, color)?;
                    draw_commands.borrow_mut().push(DrawCommand::FillRectangle {
                        x,
                        y,
                        w,
                        h,
                        color,
                    });
                    Ok(())
                },
            )?,
        )?;
    }

    {
        let draw_commands = Rc::clone(&draw_commands);
        global.set(
            "__framebuffer_draw_text",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>,
                      x: f32,
                      y: f32,
                      text: String,
                      font: u16,
                      color: u16|
                      -> Result<()> {
                    let color = resolve_color(&ctx, color)?;
                    if crate::text::font_from_id(font).is_none() {
                        return Err(rquickjs::Exception::throw_type(
                            &ctx,
                            &format!("{font} is not a valid font"),
                        ));
                    }
                    draw_commands.borrow_mut().push(DrawCommand::DrawText {
                        x,
                        y,
                        text,
                        font,
                        color,
                    });
                    Ok(())
                },
            )?,
        )?;
    }

    global.set(
        "__framebuffer_measure_text",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'_>, text: String, font: u16| -> Result<Vec<u32>> {
                let font = crate::text::font_from_id(font).ok_or_else(|| {
                    rquickjs::Exception::throw_type(&ctx, &format!("{font} is not a valid font"))
                })?;
                let (width, height) = crate::text::measure(font, &text);
                Ok(vec![width, height])
            },
        )?,
    )?;

    global.set(
        "__framebuffer_draw_image",
        Function::new(
            ctx.clone(),
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
        )?,
    )?;

    global.set(
        "__framebuffer_nearest_color",
        Function::new(ctx.clone(), move |r: u8, g: u8, b: u8| -> u16 {
            Color::nearest(r, g, b) as u16
        })?,
    )?;

    global.set(
        "__framebuffer_set_scale",
        Function::new(
            ctx.clone(),
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
        )?,
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
/// `kernel/runtime_modules/framebuffer.ts`'s `getWidth`/`getHeight`, the
/// same way `kernel/framebuffer/colors.rs`'s `Color` enum is kept in sync
/// with that file's `Color` constant.
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
    // Reused every frame in `present`: one packed-XRGB row, built once per
    // source row and duplicated `applied_scale` times into the destination
    // buffer, instead of allocating a fresh row each frame. Reallocated
    // whenever `applied_scale` changes.
    row_scratch: Vec<u32>,
    // Shared with `ely:framebuffer`'s `setScale` binding — the scale a
    // program most recently requested, checked once per `render` call.
    scale: Rc<Cell<u32>>,
    // The scale `pixmap`/`row_scratch`/`surface`/`window` are currently
    // configured for. Compared against `scale` each frame; `render`
    // reconfigures everything through `apply_scale` when they differ,
    // rather than redoing that work on every frame regardless.
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
            row_scratch: vec![0u32; physical_width as usize],
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

        rasterize(&mut self.pixmap, commands);
        self.present();
    }

    /// Reconfigures everything that depends on the physical-pixels-per-
    /// logical-pixel ratio for a newly requested `scale`: resizes the OS
    /// window to match (still not user-resizable — `.with_resizable(false)`
    /// only blocks resize via OS chrome, not a program calling this),
    /// resizes the softbuffer surface to the new physical size, and
    /// reallocates `row_scratch` at the new width. `pixmap` itself is
    /// untouched — it's always logical resolution, regardless of scale.
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
        self.row_scratch = vec![0u32; physical_width as usize];
    }

    /// Copies the logical `Pixmap` into the window's physical-resolution
    /// softbuffer surface, replicating each logical pixel into an
    /// `applied_scale` x `applied_scale` block. Assumes the window's
    /// actual physical size is exactly `FRAMEBUFFER_WIDTH * applied_scale`
    /// x `FRAMEBUFFER_HEIGHT * applied_scale` — Elysium doesn't follow the
    /// OS's DPI scale factor, so a host reporting one other than 1.0 will
    /// see a mismatched/clipped presentation.
    fn present(&mut self) {
        let mut buffer = self
            .surface
            .buffer_mut()
            .expect("failed to acquire the softbuffer back buffer");

        // Pixmap::data() is tightly packed RGBA8, row-major, premultiplied,
        // no row padding. Every frame that reaches here started from a
        // fully opaque fill (see `render`), so nothing here needs to
        // un-premultiply.
        let src = self.pixmap.data();
        let src_w = self.pixmap.width() as usize;
        let src_h = self.pixmap.height() as usize;
        let scale = self.applied_scale as usize;
        let dst_w = src_w * scale;

        for sy in 0..src_h {
            for sx in 0..src_w {
                let i = (sy * src_w + sx) * 4;
                let (r, g, b) = (src[i], src[i + 1], src[i + 2]);
                let color = (b as u32) | ((g as u32) << 8) | ((r as u32) << 16); // packed XRGB
                self.row_scratch[sx * scale..sx * scale + scale].fill(color);
            }
            for dy in 0..scale {
                let dst_start = (sy * scale + dy) * dst_w;
                buffer[dst_start..dst_start + dst_w].copy_from_slice(&self.row_scratch);
            }
        }

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
/// promise above is tested.
pub fn rasterize(pixmap: &mut tiny_skia::Pixmap, commands: &[DrawCommand]) {
    if let Some(color) = commands.iter().rev().find_map(|c| match c {
        DrawCommand::ClearScreen { color } => Some(*color),
        _ => None,
    }) {
        pixmap.fill(color.to_skia());
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

    let mut state = DrawState::new(pixmap.width(), pixmap.height());

    for command in commands {
        match command {
            DrawCommand::ClearScreen { .. } => {}
            DrawCommand::FillRectangle { x, y, w, h, color } => {
                let Some(rect) = tiny_skia::Rect::from_xywh(*x, *y, *w, *h) else {
                    continue; // negative or non-finite size
                };
                paint.set_color(color.to_skia());
                pixmap.fill_rect(rect, &paint, state.transform(), state.clip());
            }
            DrawCommand::FillPath { path, rule, color } => {
                paint.set_color(color.to_skia());
                pixmap.fill_path(path, &paint, *rule, state.transform(), state.clip());
            }
            DrawCommand::StrokePath {
                path,
                stroke,
                color,
            } => {
                paint.set_color(color.to_skia());
                pixmap.stroke_path(path, &paint, stroke, state.transform(), state.clip());
            }
            DrawCommand::SetPixel { x, y, color } => {
                let (px, py) = state.map_point(*x, *y);
                let (px, py) = (px.floor() as i32, py.floor() as i32);
                if px >= 0
                    && py >= 0
                    && px < pixmap.width() as i32
                    && py < pixmap.height() as i32
                    && state.is_visible(px, py)
                {
                    let hex = color.hex();
                    let width = pixmap.width() as i32;
                    pixmap.pixels_mut()[(py * width + px) as usize] =
                        tiny_skia::ColorU8::from_rgba(
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
                pixmap.draw_pixmap(
                    dx.round() as i32,
                    dy.round() as i32,
                    (**image).as_ref(),
                    if *opaque { &copy_paint } else { &blend_paint },
                    tiny_skia::Transform::identity(),
                    state.clip(),
                );
            }
            DrawCommand::DrawText {
                x,
                y,
                text: string,
                font,
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
                let width = pixmap.width() as i32;
                let height = pixmap.height() as i32;
                let pixels = pixmap.pixels_mut();
                text::for_each_lit_pixel(
                    font,
                    string,
                    ox.round() as i32,
                    oy.round() as i32,
                    |px, py| {
                        if px >= 0
                            && py >= 0
                            && px < width
                            && py < height
                            && state.is_visible(px, py)
                        {
                            pixels[(py * width + px) as usize] = solid;
                        }
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, DrawCommand, rasterize};

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

        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                let found = pixel_at(&pixmap, x, y);
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
                    color: Color::Amber400,
                },
            ],
        );
        assert_eq!(lit_pixels(&pixmap, Color::Amber400), 0);
    }

    fn lit_pixels(pixmap: &tiny_skia::Pixmap, color: Color) -> usize {
        (0..pixmap.height())
            .flat_map(|y| (0..pixmap.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel_at(pixmap, x, y) == color.hex())
            .count()
    }
}
