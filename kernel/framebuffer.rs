//! The Framebuffer device: a GPU-backed drawing surface bound to a window.
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
//! which is the only place in the kernel that speaks `wgpu`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use boa_engine::{Context, JsNativeError, JsResult, JsValue, NativeFunction, js_string};
use boa_gc::{Finalize, Trace, empty_trace};
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
/// needs to know anything about `wgpu`.
pub fn bootstrap_framebuffer_bindings(
    context: &mut Context,
    draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
) -> JsResult<()> {
    let sink = DrawCommandSink(draw_commands);

    let clear_screen = NativeFunction::from_copy_closure_with_captures(
        |_this, args, sink, context| {
            let color = resolve_color(color_arg(args, 0)?, context)?;
            sink.0.borrow_mut().push(DrawCommand::ClearScreen { color });
            Ok(JsValue::undefined())
        },
        sink.clone(),
    );
    context.register_global_builtin_callable(
        js_string!("__framebuffer_clear_screen"),
        1,
        clear_screen,
    )?;

    let fill_rectangle = NativeFunction::from_copy_closure_with_captures(
        |_this, args, sink, context| {
            let x = f32_arg(args, 0, context)?;
            let y = f32_arg(args, 1, context)?;
            let w = f32_arg(args, 2, context)?;
            let h = f32_arg(args, 3, context)?;
            let color = resolve_color(color_arg(args, 4)?, context)?;
            sink.0
                .borrow_mut()
                .push(DrawCommand::FillRectangle { x, y, w, h, color });
            Ok(JsValue::undefined())
        },
        sink,
    );
    context.register_global_builtin_callable(
        js_string!("__framebuffer_fill_rectangle"),
        5,
        fill_rectangle,
    )?;

    Ok(())
}

/// Thin wrapper making the shared draw-command buffer safe to store as
/// [`NativeFunction`] closure state: it holds no GC-managed value (a
/// `DrawCommand` is plain color/coordinate data), so it's sound to tell
/// Boa's collector there's nothing inside for it to trace.
#[derive(Clone)]
struct DrawCommandSink(Rc<RefCell<Vec<DrawCommand>>>);

impl Finalize for DrawCommandSink {}
// SAFETY: `DrawCommand` never holds a GC-managed value.
unsafe impl Trace for DrawCommandSink {
    empty_trace!();
}

fn color_arg(args: &[JsValue], index: usize) -> JsResult<u16> {
    args.get(index)
        .and_then(JsValue::as_number)
        .map(|n| n as u16)
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("expected a color id")
                .into()
        })
}

fn f32_arg(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<f32> {
    let value = args
        .get(index)
        .ok_or_else(|| JsNativeError::typ().with_message("missing argument"))?;
    Ok(value.to_number(context)? as f32)
}

/// Resolves a numeric color id (as sent by one of `ely:framebuffer`'s generated
/// `RED_500`-style constants) to a [`Color`], throwing a `TypeError` if it's
/// out of range — only reachable if a program bypasses the generated
/// constants and passes an arbitrary number instead.
fn resolve_color(id: u16, _context: &mut Context) -> JsResult<Color> {
    Color::from_id(id).ok_or_else(|| {
        JsNativeError::typ()
            .with_message(format!("{id} is not a valid color"))
            .into()
    })
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

const SHADER_SOURCE: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// Initial capacity (in vertices) of the vertex buffer backing
/// [`Framebuffer::render`]'s `FillRectangle` commands; it grows on demand.
const INITIAL_VERTEX_CAPACITY: usize = 1024;

pub struct Framebuffer {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: usize,
}

impl Framebuffer {
    /// Builds the `wgpu` surface/device/pipeline for `window`. Blocks on
    /// `wgpu`'s async adapter/device request via `pollster` so the rest of
    /// the kernel — a per-frame, synchronous loop — never has to think
    /// about async.
    pub fn new(window: Arc<Window>) -> Framebuffer {
        pollster::block_on(Self::new_async(window))
    }

    async fn new_async(window: Arc<Window>) -> Framebuffer {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("failed to create a GPU surface for the window");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("failed to find a GPU adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("failed to open a connection to the GPU");

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: capabilities.present_modes[0],
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("elysium-framebuffer-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("elysium-framebuffer-pipeline-layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("elysium-framebuffer-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(vertex_layout)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = create_vertex_buffer(&device, INITIAL_VERTEX_CAPACITY);

        Framebuffer {
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            vertex_buffer,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
        }
    }

    /// Turns one frame's worth of accumulated [`DrawCommand`]s into a
    /// single GPU render pass and presents it. The last `ClearScreen` in
    /// `commands` wins as the pass's clear color; every `FillRectangle`
    /// becomes two triangles in one vertex buffer, drawn in one call.
    pub fn render(&mut self, commands: &[DrawCommand]) {
        self.reconfigure_if_resized();

        let mut clear_color = wgpu::Color::BLACK;
        let mut vertices: Vec<Vertex> = Vec::new();
        for command in commands {
            match *command {
                DrawCommand::ClearScreen { color } => {
                    let [r, g, b, a] = color.rgba();
                    clear_color = wgpu::Color {
                        r: r as f64,
                        g: g as f64,
                        b: b as f64,
                        a: a as f64,
                    };
                }
                DrawCommand::FillRectangle { x, y, w, h, color } => {
                    push_rectangle(
                        &mut vertices,
                        x,
                        y,
                        w,
                        h,
                        color,
                        self.config.width as f32,
                        self.config.height as f32,
                    );
                }
            }
        }

        if vertices.len() > self.vertex_capacity {
            self.vertex_capacity = vertices.len();
            self.vertex_buffer = create_vertex_buffer(&self.device, self.vertex_capacity);
        }
        if !vertices.is_empty() {
            self.queue
                .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.surface.configure(&self.device, &self.config);
                frame
            }
            // Timeout/Occluded/Outdated/Lost/Validation: skip this frame
            // rather than panic, same as a resize racing a frame acquire.
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("elysium-framebuffer-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("elysium-framebuffer-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !vertices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }

    /// Picks up any window resize since the last frame — the surface must
    /// be reconfigured to the new size before `get_current_texture` will
    /// hand back correctly sized frames.
    fn reconfigure_if_resized(&mut self) {
        let size = self.window.inner_size();
        if size.width != self.config.width || size.height != self.config.height {
            self.config.width = size.width.max(1);
            self.config.height = size.height.max(1);
            self.surface.configure(&self.device, &self.config);
        }
    }
}

fn create_vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("elysium-framebuffer-vertex-buffer"),
        size: (capacity * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Appends the two triangles (six vertices) making up an axis-aligned
/// rectangle at `(x, y)`, `w` by `h` pixels, converting from pixel space
/// (origin top-left, `y` down) to clip space (origin center, `y` up,
/// `-1.0..=1.0`).
fn push_rectangle(
    vertices: &mut Vec<Vertex>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    screen_w: f32,
    screen_h: f32,
) {
    let to_clip = |px: f32, py: f32| [(px / screen_w) * 2.0 - 1.0, 1.0 - (py / screen_h) * 2.0];
    let rgba = color.rgba();
    let vertex = |position: [f32; 2]| Vertex {
        position,
        color: rgba,
    };

    let top_left = to_clip(x, y);
    let top_right = to_clip(x + w, y);
    let bottom_left = to_clip(x, y + h);
    let bottom_right = to_clip(x + w, y + h);

    vertices.push(vertex(top_left));
    vertices.push(vertex(bottom_left));
    vertices.push(vertex(top_right));
    vertices.push(vertex(top_right));
    vertices.push(vertex(bottom_left));
    vertices.push(vertex(bottom_right));
}
