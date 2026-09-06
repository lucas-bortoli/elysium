//! Elysium's drawing: [`Surface`]s, and getting the screen surface in front
//! of the viewer.
//!
//! A `Surface` is a retained pixmap that render operations apply to
//! immediately and stay. There is no command list and no frame boundary: a
//! draw call from a program, or from kernel code, mutates a surface's pixels
//! then and there. The screen is one such surface, created at boot and held
//! behind a shared handle ([`ScreenSurface`]); [`Display`] presents it once
//! per tick.
//!
//! `ely:graphics`'s bindings (still named `__framebuffer_*` on the wire until
//! the JS module is rewritten) resolve a program's colour, font and image
//! ids at the boundary here, then call the matching [`Surface`] method. The
//! per-shape and text-layout maths still live in the JS module for now.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rquickjs::{Ctx, Result};

mod colors;
mod display;
mod palette;
mod paths;
mod state;
mod surface;

pub use colors::Color;
pub use display::{DEFAULT_SCALE, Display};
pub use surface::Surface;

use crate::bindings::bind;

/// The screen surface's logical resolution at boot. Not a hard limit — the
/// screen is a [`Surface`] like any other and can be resized, which is how
/// the window's logical resolution changes.
pub const SCREEN_WIDTH: u32 = 720;
pub const SCREEN_HEIGHT: u32 = 360;

/// The shared handle to the screen surface: held by the kernel's frame loop,
/// which presents it, and by every VM's `ely:graphics` bindings, which draw
/// to it.
pub type ScreenSurface = Rc<RefCell<Surface>>;

/// Resolves a numeric colour id (as sent by one of `ely:graphics`'s
/// generated `RED_500`-style constants) to a [`Color`], throwing a
/// `TypeError` if it's out of range — only reachable if a program bypasses
/// the generated constants and passes an arbitrary number instead.
fn resolve_color(ctx: &Ctx<'_>, id: u16) -> Result<Color> {
    Color::from_id(id)
        .ok_or_else(|| rquickjs::Exception::throw_type(ctx, &format!("{id} is not a valid color")))
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
/// bindings included. A program never names one of these: it calls the
/// module's exported `Surface` methods, which call the matching global, which
/// draws straight onto the shared screen surface.
pub fn bootstrap_graphics_bindings(
    ctx: &Ctx<'_>,
    screen: ScreenSurface,
    scale: Rc<Cell<u32>>,
    images: Rc<crate::image::ImageTable>,
) -> Result<()> {
    paths::bootstrap_path_bindings(
        ctx,
        Rc::clone(&screen),
        Rc::new(RefCell::new(tiny_skia::PathBuilder::new())),
    )?;

    {
        let screen = Rc::clone(&screen);
        bind(ctx, "__framebuffer_pop_transform", move || {
            screen.borrow_mut().pop_transform()
        })?;
    }

    {
        let screen = Rc::clone(&screen);
        bind(
            ctx,
            "__framebuffer_set_pixel",
            move |ctx: Ctx<'_>, x: f32, y: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                screen.borrow_mut().set_pixel(x, y, color);
                Ok(())
            },
        )?;
    }

    {
        let screen = Rc::clone(&screen);
        // The six numbers of a 2x3 matrix, composed on the JS side from
        // whatever mix of shift, scale and rotation a program asked for.
        bind(
            ctx,
            "__framebuffer_push_transform",
            move |sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32| {
                let transform = tiny_skia::Transform::from_row(sx, ky, kx, sy, tx, ty);
                screen.borrow_mut().push_transform(transform)
            },
        )?;
    }

    {
        let screen = Rc::clone(&screen);
        bind(
            ctx,
            "__framebuffer_clear_screen",
            move |ctx: Ctx<'_>, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                screen.borrow_mut().clear(color);
                Ok(())
            },
        )?;
    }

    {
        let screen = Rc::clone(&screen);
        bind(
            ctx,
            "__framebuffer_fill_rectangle",
            move |ctx: Ctx<'_>, x: f32, y: f32, w: f32, h: f32, color: u16| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                screen.borrow_mut().fill_rectangle(x, y, w, h, color);
                Ok(())
            },
        )?;
    }

    {
        let screen = Rc::clone(&screen);
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
                screen
                    .borrow_mut()
                    .draw_text(x, y, &text, font, scale, color);
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
        let screen = Rc::clone(&screen);
        let images = Rc::clone(&images);
        bind(
            ctx,
            "__framebuffer_draw_image",
            move |ctx: Ctx<'_>, id: u32, x: f32, y: f32| -> Result<()> {
                let image = crate::image::resolve_image(&ctx, &images, id)?;
                screen
                    .borrow_mut()
                    .draw_image(&image.pixmap, image.opaque, x, y);
                Ok(())
            },
        )?;
    }

    {
        let screen = Rc::clone(&screen);
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
                screen.borrow_mut().draw_image_transformed(
                    &image.pixmap,
                    image.opaque,
                    source,
                    tiny_skia::Transform::from_row(a, b, c, d, e, f),
                );
                Ok(())
            },
        )?;
    }

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
