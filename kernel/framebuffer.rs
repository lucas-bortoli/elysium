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

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use rquickjs::{Ctx, Function, Result};
use winit::window::Window;

mod colors;
pub use colors::Color;

/// One drawing instruction accumulated during a program's `draw()` call.
/// Colors are always a [`Color`] from the fixed palette, never raw,
/// program-supplied RGBA channels.
#[derive(Debug, Clone, Copy)]
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
}

/// Binds the *hidden* globals `ely:framebuffer`'s embedded module wraps
/// (`__framebuffer_clear_screen`, `__framebuffer_fill_rectangle`) — never called by
/// a program directly, only through `ely:framebuffer`'s exported
/// `clearScreen`/`fillRectangle`. Each closure just resolves its numeric
/// color id to a [`Color`] and pushes a [`DrawCommand`] onto the shared
/// buffer; neither one touches any drawing state itself, so this file never
/// needs to know anything about how frames get rasterized.
pub fn bootstrap_framebuffer_bindings(
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
fn resolve_color(ctx: &Ctx<'_>, id: u16) -> Result<Color> {
    Color::from_id(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a valid color")))
}

/// The logical resolution programs draw in — independent of the window's
/// physical pixel size, which is always [`SCALE`] times this. Mirrored by
/// hand in `kernel/runtime_modules/framebuffer.ts`'s `getWidth`/`getHeight`,
/// the same way `kernel/framebuffer/colors.rs`'s `Color` enum is kept in
/// sync with that file's `Color` constant.
pub const FRAMEBUFFER_WIDTH: u32 = 720;
pub const FRAMEBUFFER_HEIGHT: u32 = 360;
pub const SCALE: u32 = 2;

pub struct Framebuffer {
    // Always FRAMEBUFFER_WIDTH x FRAMEBUFFER_HEIGHT — logical resolution,
    // never the physical (scaled) window size. Programs already draw in
    // logical pixels, so every DrawCommand's x/y/w/h goes straight into
    // tiny-skia with no per-draw scaling math; the upscale to the window's
    // physical size happens once, in `present`.
    pixmap: tiny_skia::Pixmap,
    // Reused every frame in `present`: one packed-XRGB row, built once per
    // source row and duplicated SCALE times into the destination buffer,
    // instead of allocating a fresh row each frame.
    row_scratch: Vec<u32>,
    // Never read after construction, but must outlive `surface`: some
    // softbuffer backends (X11 in particular) hold a live connection this
    // surface's presents depend on for as long as it exists.
    #[allow(dead_code)]
    context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

impl Framebuffer {
    /// Allocates the logical-resolution `Pixmap` programs draw into and
    /// the softbuffer surface that presents it, sized to the fixed
    /// `FRAMEBUFFER_WIDTH * SCALE` x `FRAMEBUFFER_HEIGHT * SCALE` physical
    /// size — not queried from `window`, since Elysium doesn't follow the
    /// OS's DPI scale factor (see `present`'s doc comment).
    pub fn new(window: Arc<Window>) -> Framebuffer {
        let physical_width = FRAMEBUFFER_WIDTH * SCALE;
        let physical_height = FRAMEBUFFER_HEIGHT * SCALE;

        let pixmap = tiny_skia::Pixmap::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT)
            .expect("failed to allocate the framebuffer's backing pixmap");

        let context = softbuffer::Context::new(Arc::clone(&window))
            .expect("failed to create a softbuffer context for the window");
        let mut surface = softbuffer::Surface::new(&context, Arc::clone(&window))
            .expect("failed to create a softbuffer surface for the window");
        surface
            .resize(
                NonZeroU32::new(physical_width).expect("FRAMEBUFFER_WIDTH * SCALE is 0"),
                NonZeroU32::new(physical_height).expect("FRAMEBUFFER_HEIGHT * SCALE is 0"),
            )
            .expect("failed to size the softbuffer surface to the window");

        Framebuffer {
            pixmap,
            row_scratch: vec![0u32; physical_width as usize],
            context,
            surface,
        }
    }

    /// Rasterizes one frame's worth of accumulated [`DrawCommand`]s onto
    /// the logical `Pixmap` and presents it. Clearing is opt-in: a frame
    /// with no `ClearScreen` command leaves the pixmap exactly as the
    /// previous frame left it, rather than forcing a clear every frame —
    /// so a program that never calls `clearScreen` sees each frame's
    /// drawing accumulate rather than get erased first. The last
    /// `ClearScreen` in `commands` wins, applied before any
    /// `FillRectangle`, regardless of where in the command list it falls.
    pub fn render(&mut self, commands: &[DrawCommand]) {
        if let Some(color) = commands.iter().rev().find_map(|c| match *c {
            DrawCommand::ClearScreen { color } => Some(color),
            DrawCommand::FillRectangle { .. } => None,
        }) {
            self.pixmap.fill(color.to_skia());
        }

        // anti_alias: false matches the old backend's hard rectangle edges
        // (no MSAA).
        let mut paint = tiny_skia::Paint {
            anti_alias: false,
            ..Default::default()
        };

        for command in commands {
            if let DrawCommand::FillRectangle { x, y, w, h, color } = *command {
                let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
                    continue; // degenerate (zero/negative/non-finite) size
                };
                paint.set_color(color.to_skia());
                self.pixmap
                    .fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
            }
        }

        self.present();
    }

    /// Copies the logical `Pixmap` into the window's physical-resolution
    /// softbuffer surface, replicating each logical pixel into a fixed
    /// SCALE x SCALE block. Assumes the window's actual physical size is
    /// exactly FRAMEBUFFER_WIDTH*SCALE x FRAMEBUFFER_HEIGHT*SCALE —
    /// Elysium doesn't follow the OS's DPI scale factor, so a host
    /// reporting one other than 1.0 will see a mismatched/clipped
    /// presentation.
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
        let scale = SCALE as usize;
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
