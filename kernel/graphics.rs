//! Elysium's drawing: [`Surface`]s, the kernel-wide [`SurfaceTable`] that
//! holds them, and getting the screen surface in front of the viewer.
//!
//! A `Surface` is a retained pixmap that render operations apply to
//! immediately and stay. There is no command list and no frame boundary: a
//! draw call from a program, or from kernel code, mutates a surface's pixels
//! then and there. Surfaces live in one kernel-wide table under plain integer
//! ids, so a handle travels between processes as an ordinary number; the
//! screen is [`SCREEN_ID`] in that table, and [`Display`] presents it once
//! per tick.
//!
//! `ely:graphics`'s bindings resolve a program's colour, font and image ids
//! at the boundary here, then call the matching [`Surface`] method on the
//! surface the leading handle names. The per-shape and text-layout maths
//! still live in the JS module for now.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rquickjs::{Ctx, Result};

mod colors;
mod display;
mod palette;
mod paths;
mod shapes;
mod state;
mod surface;
mod table;

pub use colors::Color;
pub use display::{DEFAULT_SCALE, Display};
pub use surface::Surface;
pub use table::{SCREEN_ID, SurfaceId, SurfaceTable};

use rquickjs::Object;

use crate::bindings::bind;
use crate::process::ProcessId;
use crate::text::{TextAlign, TextLayout};
use table::SurfaceError;

/// The screen surface's logical resolution at boot. Not a hard limit — the
/// screen is a [`Surface`] like any other and can be resized, which is how
/// the window's logical resolution changes.
pub const SCREEN_WIDTH: u32 = 720;
pub const SCREEN_HEIGHT: u32 = 360;

/// Resolves a numeric colour id (as sent by one of `ely:graphics`'s
/// generated `RED_500`-style constants) to a [`Color`], throwing a
/// `TypeError` if it's out of range — only reachable if a program bypasses
/// the generated constants and passes an arbitrary number instead.
fn resolve_color(ctx: &Ctx<'_>, id: u16) -> Result<Color> {
    Color::from_id(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a valid color")))
}

/// Turns a [`SurfaceError`] into the JS exception the binding boundary
/// raises for it.
fn throw_surface_error(ctx: &Ctx<'_>, err: SurfaceError) -> rquickjs::Error {
    match err {
        SurfaceError::NoSuchSurface(id) => {
            rquickjs::Exception::throw_type(ctx, &format!("{id} is not a live surface"))
        }
        SurfaceError::SelfBlit => {
            rquickjs::Exception::throw_type(ctx, "a surface cannot be drawn onto itself")
        }
        SurfaceError::OutOfMemory => {
            rquickjs::Exception::throw_range(ctx, "the surface memory limit has been reached")
        }
        SurfaceError::BadSize => rquickjs::Exception::throw_range(
            ctx,
            "a surface's width and height must be positive and small enough to allocate",
        ),
    }
}

/// Runs a drawing closure against the surface `id` names, mapping a dead id
/// to a thrown `TypeError`.
fn draw_on(
    surfaces: &SurfaceTable,
    ctx: &Ctx<'_>,
    id: SurfaceId,
    f: impl FnOnce(&mut Surface),
) -> Result<()> {
    surfaces
        .with_mut(id, f)
        .map_err(|err| throw_surface_error(ctx, err))
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

/// Binds the hidden globals `ely:graphics`'s embedded module wraps, path
/// bindings included. A program never names one of these: it calls a
/// `Surface` object's methods, which pass the surface's handle to the
/// matching global, which draws straight onto that surface in the shared
/// table.
pub fn bootstrap_graphics_bindings(
    ctx: &Ctx<'_>,
    surfaces: Rc<SurfaceTable>,
    self_id: ProcessId,
    scale: Rc<Cell<u32>>,
    images: Rc<crate::image::ImageTable>,
) -> Result<()> {
    paths::bootstrap_path_bindings(
        ctx,
        Rc::clone(&surfaces),
        Rc::new(RefCell::new(tiny_skia::PathBuilder::new())),
    )?;

    // --- The surface lifecycle: create, revive, destroy, size, resize. ---

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_create",
            move |ctx: Ctx<'_>, width: u32, height: u32| -> Result<u32> {
                surfaces
                    .create(width, height, self_id)
                    .map_err(|err| throw_surface_error(&ctx, err))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(ctx, "__surface_use", move |id: u32| -> bool {
            surfaces.acquire(id, self_id)
        })?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(ctx, "__surface_destroy", move |id: u32| {
            surfaces.release(id, self_id)
        })?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_dimensions",
            move |ctx: Ctx<'_>, id: u32| -> Result<Vec<u32>> {
                surfaces
                    .dimensions(id)
                    .map(|(w, h)| vec![w, h])
                    .ok_or_else(|| throw_surface_error(&ctx, SurfaceError::NoSuchSurface(id)))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_resize",
            move |ctx: Ctx<'_>, id: u32, width: u32, height: u32| -> Result<()> {
                surfaces
                    .resize(id, width, height)
                    .map_err(|err| throw_surface_error(&ctx, err))
            },
        )?;
    }

    // --- Drawing onto a surface named by its leading handle. ---

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_clear",
            move |ctx: Ctx<'_>, id: u32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_on(&surfaces, &ctx, id, |s| s.clear(color))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_fill_rectangle",
            move |ctx: Ctx<'_>,
                  id: u32,
                  x: f32,
                  y: f32,
                  w: f32,
                  h: f32,
                  color: u16|
                  -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_on(&surfaces, &ctx, id, |s| s.fill_rectangle(x, y, w, h, color))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_set_pixel",
            move |ctx: Ctx<'_>, id: u32, x: f32, y: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_on(&surfaces, &ctx, id, |s| s.set_pixel(x, y, color))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_draw_text",
            move |ctx: Ctx<'_>,
                  id: u32,
                  x: f32,
                  y: f32,
                  text: String,
                  color: u16,
                  options: Object<'_>|
                  -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                let layout = read_text_layout(&ctx, &options)?;
                draw_on(&surfaces, &ctx, id, |s| {
                    s.draw_text(x, y, &text, &layout, color)
                })
            },
        )?;
    }

    bind(
        ctx,
        "__surface_measure_text",
        move |ctx: Ctx<'_>, text: String, options: Object<'_>| -> Result<Vec<u32>> {
            let layout = read_text_layout(&ctx, &options)?;
            let font = crate::text::font_from_id(layout.font)
                .expect("read_text_layout already checked the font id");
            let (_, width, height) = crate::text::lay_out(font, &layout, &text);
            Ok(vec![width.ceil() as u32, height.ceil() as u32])
        },
    )?;

    // --- The transform stack, per surface (clip lives in paths.rs). ---

    {
        let surfaces = Rc::clone(&surfaces);
        // A program's `{ translate, scale, rotate }` reaches here as those
        // three parts; the 2x3 for shift * turn * scale is composed below,
        // not on the JS side.
        bind(
            ctx,
            "__surface_push_transform",
            move |ctx: Ctx<'_>,
                  id: u32,
                  translate: Vec<f32>,
                  scale: Vec<f32>,
                  rotate: f32|
                  -> Result<()> {
                let [tx, ty] = pair(&ctx, &translate, "a translation needs two numbers")?;
                let [sx, sy] = pair(&ctx, &scale, "a scale needs two numbers")?;
                let transform = tiny_skia::Transform::from_translate(tx, ty)
                    .pre_concat(tiny_skia::Transform::from_rotate(rotate.to_degrees()))
                    .pre_concat(tiny_skia::Transform::from_scale(sx, sy));
                draw_on(&surfaces, &ctx, id, |s| s.push_transform(transform))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_pop_transform",
            move |ctx: Ctx<'_>, id: u32| -> Result<()> {
                draw_on(&surfaces, &ctx, id, |s| s.pop_transform())
            },
        )?;
    }

    // --- The shape vocabulary, lowered onto path fill/stroke in the
    // kernel. Geometry with more than three scalars arrives as one array,
    // to stay inside rquickjs's closure-arity ceiling. ---

    bind_shape(
        ctx,
        &surfaces,
        "__surface_stroke_rectangle",
        4,
        |s, r, color, t| s.stroke_rectangle(r[0], r[1], r[2], r[3], color, t),
    )?;
    bind_shape(
        ctx,
        &surfaces,
        "__surface_stroke_rounded_rectangle",
        5,
        |s, r, color, t| s.stroke_rounded_rectangle(r[0], r[1], r[2], r[3], r[4], color, t),
    )?;
    bind_shape(
        ctx,
        &surfaces,
        "__surface_draw_line",
        4,
        |s, r, color, t| s.draw_line(r[0], r[1], r[2], r[3], color, t),
    )?;
    bind_shape(
        ctx,
        &surfaces,
        "__surface_stroke_ellipse",
        4,
        |s, r, color, t| s.stroke_ellipse(r[0], r[1], r[2], r[3], color, t),
    )?;
    bind_shape(ctx, &surfaces, "__surface_draw_arc", 5, |s, r, color, t| {
        s.draw_arc(r[0], r[1], r[2], r[3], r[4], color, t)
    })?;
    bind_shape(
        ctx,
        &surfaces,
        "__surface_draw_polyline",
        4,
        |s, pts, color, t| s.draw_polyline(pts, color, t),
    )?;
    bind_shape(
        ctx,
        &surfaces,
        "__surface_stroke_polygon",
        6,
        |s, pts, color, t| s.stroke_polygon(pts, color, t),
    )?;

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_fill_rounded_rectangle",
            move |ctx: Ctx<'_>, id: u32, rect: Vec<f32>, radius: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                let [x, y, w, h] = quad(&ctx, &rect)?;
                draw_on(&surfaces, &ctx, id, |s| {
                    s.fill_rounded_rectangle(x, y, w, h, radius, color)
                })
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_fill_ellipse",
            move |ctx: Ctx<'_>,
                  id: u32,
                  cx: f32,
                  cy: f32,
                  rx: f32,
                  ry: f32,
                  color: u16|
                  -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                draw_on(&surfaces, &ctx, id, |s| {
                    s.fill_ellipse(cx, cy, rx, ry, color)
                })
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_fill_polygon",
            move |ctx: Ctx<'_>,
                  id: u32,
                  points: Vec<f32>,
                  color: u16,
                  rule: String|
                  -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                let rule = match rule.as_str() {
                    "evenodd" => tiny_skia::FillRule::EvenOdd,
                    "nonzero" => tiny_skia::FillRule::Winding,
                    other => {
                        return Err(rquickjs::Exception::throw_type(
                            &ctx,
                            &format!("{other:?} is not a valid fill rule"),
                        ));
                    }
                };
                draw_on(&surfaces, &ctx, id, |s| {
                    s.fill_polygon(&points, color, rule)
                })
            },
        )?;
    }

    // --- Images, and surfaces drawn as images. ---

    {
        let surfaces = Rc::clone(&surfaces);
        let images = Rc::clone(&images);
        bind(
            ctx,
            "__surface_draw_image",
            move |ctx: Ctx<'_>, id: u32, image_id: u32, x: f32, y: f32| -> Result<()> {
                let image = crate::image::resolve_image(&ctx, &images, image_id)?;
                draw_on(&surfaces, &ctx, id, |s| {
                    s.draw_image(&image.pixmap, image.opaque, x, y)
                })
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_draw_image_transformed",
            move |ctx: Ctx<'_>,
                  id: u32,
                  image_id: u32,
                  source: Vec<f32>,
                  transform: Vec<f32>|
                  -> Result<()> {
                let image = crate::image::resolve_image(&ctx, &images, image_id)?;
                let (source, transform) = decode_blit(&ctx, &source, &transform)?;
                let Some(source) = source else {
                    return Ok(()); // nothing of the image asked for
                };
                draw_on(&surfaces, &ctx, id, |s| {
                    s.draw_image_transformed(&image.pixmap, image.opaque, source, transform)
                })
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_draw_surface",
            move |ctx: Ctx<'_>, dst: u32, src: u32, x: f32, y: f32| -> Result<()> {
                surfaces
                    .draw_surface_whole(dst, src, x, y)
                    .map_err(|err| throw_surface_error(&ctx, err))
            },
        )?;
    }

    {
        let surfaces = Rc::clone(&surfaces);
        bind(
            ctx,
            "__surface_draw_surface_transformed",
            move |ctx: Ctx<'_>,
                  dst: u32,
                  src: u32,
                  source: Vec<f32>,
                  transform: Vec<f32>|
                  -> Result<()> {
                let (source, transform) = decode_blit(&ctx, &source, &transform)?;
                let Some(source) = source else {
                    return Ok(());
                };
                surfaces
                    .draw_surface_transformed(dst, src, source, transform)
                    .map_err(|err| throw_surface_error(&ctx, err))
            },
        )?;
    }

    // --- Palette and window knobs (not surface-scoped). ---

    bind(ctx, "__surface_nearest_color", |r: u8, g: u8, b: u8| {
        Color::nearest(r, g, b) as u16
    })?;

    bind(
        ctx,
        "__surface_set_scale",
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

/// Binds a shape whose geometry arrives as one `[f32]` array (`geom`),
/// followed by a colour and a stroke thickness. `min` is how many numbers
/// the shape needs; a short array is dropped rather than panicking, since
/// these globals are reachable from a program directly.
fn bind_shape<'js, F>(
    ctx: &Ctx<'js>,
    surfaces: &Rc<SurfaceTable>,
    name: &str,
    min: usize,
    f: F,
) -> Result<()>
where
    F: Fn(&mut Surface, &[f32], Color, f32) + 'js,
{
    let surfaces = Rc::clone(surfaces);
    bind(
        ctx,
        name,
        move |ctx: Ctx<'_>, id: u32, geom: Vec<f32>, color: u16, thickness: f32| -> Result<()> {
            let color = resolve_color(&ctx, color)?;
            draw_on(&surfaces, &ctx, id, |s| {
                if geom.len() >= min {
                    f(s, &geom, color, thickness);
                }
            })
        },
    )
}

/// Reads the two numbers of a `[x, y]` array, throwing `message` otherwise.
fn pair(ctx: &Ctx<'_>, values: &[f32], message: &str) -> Result<[f32; 2]> {
    <[f32; 2]>::try_from(values).map_err(|_| rquickjs::Exception::throw_type(ctx, message))
}

/// Reads the four numbers of a `[x, y, w, h]` array.
fn quad(ctx: &Ctx<'_>, values: &[f32]) -> Result<[f32; 4]> {
    <[f32; 4]>::try_from(values)
        .map_err(|_| rquickjs::Exception::throw_type(ctx, "a rectangle needs four numbers"))
}

/// Builds a [`TextLayout`] from `ely:graphics`'s options object, filling in
/// defaults and checking the font id and scale.
fn read_text_layout(ctx: &Ctx<'_>, options: &Object<'_>) -> Result<TextLayout> {
    let font = options.get::<_, Option<u16>>("font")?.unwrap_or(0);
    if crate::text::font_from_id(font).is_none() {
        return Err(rquickjs::Exception::throw_type(
            ctx,
            &format!("{font} is not a valid font"),
        ));
    }
    let scale = options.get::<_, Option<u32>>("scale")?.unwrap_or(1);
    if scale == 0 {
        return Err(rquickjs::Exception::throw_range(
            ctx,
            "text scale must be at least 1",
        ));
    }
    let align = match options.get::<_, Option<String>>("align")?.as_deref() {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ => TextAlign::Left,
    };
    Ok(TextLayout {
        font,
        scale,
        align,
        max_width: options.get::<_, Option<f32>>("maxWidth")?,
        line_spacing: options.get::<_, Option<f32>>("lineSpacing")?.unwrap_or(1.0),
    })
}

/// Decodes the `[x, y, w, h]` source rect and 2x3 placement matrix a blit
/// binding receives — both assembled on the JS side, too many numbers to
/// pass one by one. The source rect is `None` when it has no area.
fn decode_blit(
    ctx: &Ctx<'_>,
    source: &[f32],
    transform: &[f32],
) -> Result<(Option<tiny_skia::Rect>, tiny_skia::Transform)> {
    let [sx, sy, sw, sh] = <[f32; 4]>::try_from(source)
        .map_err(|_| rquickjs::Exception::throw_type(ctx, "a source rect needs four numbers"))?;
    let [a, b, c, d, e, f] = <[f32; 6]>::try_from(transform)
        .map_err(|_| rquickjs::Exception::throw_type(ctx, "a transform needs six numbers"))?;
    Ok((
        tiny_skia::Rect::from_xywh(sx, sy, sw, sh),
        tiny_skia::Transform::from_row(a, b, c, d, e, f),
    ))
}
