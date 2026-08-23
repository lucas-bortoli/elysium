//! The kernel's one owner of the OS-level windowing/event loop.
//!
//! No other module ever names a `winit` type: `kernel/framebuffer.rs`'s
//! `Framebuffer` only ever sees a shared `Arc<Window>` handle, never the event
//! loop itself. That's deliberate — a future Input device needs the same
//! window's keyboard/mouse events (`winit::event::WindowEvent`s arriving
//! here), and shouldn't have to reach through Framebuffer to get them. This is
//! the one seam both devices attach to.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

pub struct ElysiumWindow {
    title: String,
    width: u32,
    height: u32,
}

impl ElysiumWindow {
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
        }
    }

    /// Runs the OS event loop until the window is closed, calling
    /// `on_frame` once per tick with the window and the time elapsed since
    /// the previous tick. Nothing about `on_frame`'s signature is
    /// Framebuffer-specific — it's the generic per-tick seam `main.rs` wires
    /// `Framebuffer` and `ElysiumRuntime` into.
    pub fn run(self, on_frame: impl FnMut(&Arc<Window>, Duration)) {
        let event_loop = EventLoop::new().expect("failed to create the OS event loop");
        event_loop.set_control_flow(ControlFlow::Poll);

        let mut app = App {
            title: self.title,
            width: self.width,
            height: self.height,
            window: None,
            last_frame: None,
            on_frame,
        };
        event_loop
            .run_app(&mut app)
            .expect("the OS event loop exited with an error");
    }
}

struct App<F> {
    title: String,
    width: u32,
    height: u32,
    window: Option<Arc<Window>>,
    last_frame: Option<Instant>,
    on_frame: F,
}

impl<F: FnMut(&Arc<Window>, Duration)> ApplicationHandler for App<F> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Window creation can only happen once the platform has signaled
        // it's ready (see `ApplicationHandler::resumed`'s docs) — it can't
        // happen eagerly in `ElysiumWindow::run`.
        let attributes = WindowAttributes::default()
            .with_title(self.title.clone())
            .with_inner_size(LogicalSize::new(self.width, self.height));
        let window = event_loop
            .create_window(attributes)
            .expect("failed to create the window");
        window.request_redraw();
        self.window = Some(Arc::new(window));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                let Some(window) = self.window.clone() else {
                    return;
                };
                let now = Instant::now();
                let dt = self.last_frame.map_or(Duration::ZERO, |last| now - last);
                self.last_frame = Some(now);

                (self.on_frame)(&window, dt);
                window.request_redraw();
            }
            _ => {}
        }
    }
}
