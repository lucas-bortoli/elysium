//! The [`Display`]: what gets the screen [`Surface`](super::Surface)'s pixels
//! in front of the viewer.
//!
//! `Display` never touches `winit`'s event loop itself — it only ever sees a
//! window handle, handed to it by `kernel/window.rs`'s `ElysiumWindow`, which
//! is the kernel's one place that actually owns the OS event loop. That's
//! deliberate: the Input device needs the same window's keyboard/mouse
//! events, and shouldn't have to reach through here to get them.
//!
//! It owns no pixels of its own. Every frame the kernel hands it the screen
//! surface to present; `Display`'s job is only the integer upscale from the
//! surface's logical resolution to the window's physical one, plus picking up
//! a scale a program asked for since the last frame.

use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use super::Surface;

/// The physical-pixels-per-logical-pixel ratio a `Display` starts at before
/// any `setScale` call. The live ratio is held in a runtime `Cell<u32>`
/// shared with `ely:graphics`'s `setScale` binding, so a program can change
/// it while Elysium is running.
pub const DEFAULT_SCALE: u32 = 2;

pub struct Display {
    // Reused every frame in `present`: one packed-XRGB row, built once per
    // source row and duplicated `applied_scale` times into the destination
    // buffer. Reallocated whenever the physical width changes.
    row_scratch: Vec<u32>,
    // Shared with `ely:graphics`'s `setScale` binding — the scale a program
    // most recently requested, checked once per `present` call.
    scale: Rc<Cell<u32>>,
    // The scale `row_scratch`/`surface`/`window` are currently configured
    // for. Compared against `scale` each frame.
    applied_scale: u32,
    // The physical size the softbuffer surface and window are configured
    // for. Compared against the screen surface's size each frame, so a
    // program resizing the screen resizes the window to match.
    applied_size: (u32, u32),
    // Needed to resize the OS window itself when the scale or the screen
    // surface changes size. `Display` never touches the event loop, only
    // this window handle.
    window: Arc<Window>,
    // Never read after construction, but must outlive `surface`: some
    // softbuffer backends (X11 in particular) hold a live connection this
    // surface's presents depend on for as long as it exists.
    #[allow(dead_code)]
    context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
}

impl Display {
    /// Wires a `Display` to `window`, sized for the current `scale` and the
    /// `initial_size` the screen surface was created at. `scale` is shared
    /// with `ely:graphics`'s `setScale` binding; `present` notices when it,
    /// or the screen surface's own size, changes.
    pub fn new(window: Arc<Window>, scale: Rc<Cell<u32>>, initial_size: (u32, u32)) -> Display {
        let applied_scale = scale.get();
        let (logical_w, logical_h) = initial_size;
        let physical_width = logical_w * applied_scale;
        let physical_height = logical_h * applied_scale;

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

        Display {
            row_scratch: vec![0u32; physical_width as usize],
            scale,
            applied_scale,
            applied_size: initial_size,
            window,
            context,
            surface,
        }
    }

    /// Presents `screen` to the window: picks up a scale or size change since
    /// the last frame, then does the integer upscale from the surface's
    /// logical resolution to the window's physical one.
    pub fn present(&mut self, screen: &Surface) {
        let requested_scale = self.scale.get();
        let surface_size = (screen.width(), screen.height());
        if requested_scale != self.applied_scale || surface_size != self.applied_size {
            self.reconfigure(surface_size, requested_scale);
        }
        self.blit(screen.pixmap());
    }

    /// Reconfigures everything that depends on the physical pixel size — the
    /// OS window, the softbuffer surface, and `row_scratch` — for a newly
    /// requested logical `size` and `scale`. The window stays not
    /// user-resizable; `.with_resizable(false)` only blocks resize via OS
    /// chrome, not a program driving it from here.
    fn reconfigure(&mut self, size: (u32, u32), scale: u32) {
        self.applied_scale = scale;
        self.applied_size = size;
        let physical_width = size.0 * scale;
        let physical_height = size.1 * scale;

        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(physical_width, physical_height));
        self.surface
            .resize(
                NonZeroU32::new(physical_width).expect("physical width is 0"),
                NonZeroU32::new(physical_height).expect("physical height is 0"),
            )
            .expect("failed to resize the softbuffer surface");
        self.row_scratch = vec![0u32; physical_width as usize];
    }

    /// Copies the logical `pixmap` into the window's physical-resolution
    /// softbuffer surface, replicating each logical pixel into an
    /// `applied_scale` x `applied_scale` block. Assumes the window's actual
    /// physical size is exactly the logical size times `applied_scale` —
    /// Elysium doesn't follow the OS's DPI scale factor, so a host reporting
    /// one other than 1.0 will see a mismatched presentation.
    fn blit(&mut self, pixmap: &tiny_skia::Pixmap) {
        let mut buffer = self
            .surface
            .buffer_mut()
            .expect("failed to acquire the softbuffer back buffer");

        // Pixmap::data() is tightly packed RGBA8, row-major, premultiplied,
        // no row padding. Every pixel the rasterizer writes is a fully
        // opaque palette color, so premultiplied and straight bytes are the
        // same bytes and nothing here needs to un-premultiply.
        let src = pixmap.data();
        let src_w = pixmap.width() as usize;
        let src_h = pixmap.height() as usize;
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
