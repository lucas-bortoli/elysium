//! Path building for the Framebuffer device.
//!
//! Every outline and filled shape a program can draw — a line, a polygon, a
//! circle, a rounded rectangle, an arc — is one path, filled or stroked. A
//! program describes a path one segment at a time, so the kernel keeps a
//! single path under construction for the current draw handler and appends to
//! it as those calls arrive. Filling or stroking snapshots that path into a
//! [`DrawCommand`](super::DrawCommand) and leaves it in place, so the same
//! path can be filled and then stroked without describing it twice.
//!
//! The curved shapes are all approximated with cubics, because that's the
//! only curve the rasterizer takes. That approximation is invisible: nothing
//! is anti-aliased, so a curve only ever has to be accurate to the nearest
//! whole pixel.

use std::cell::RefCell;
use std::rc::Rc;

use crate::bindings::bind;
use rquickjs::{Ctx, Result};

use super::{DrawCommand, resolve_color};

/// The path a program is currently describing. Shared between every path
/// binding below, and cleared by `beginPath`.
pub type PathScratch = Rc<RefCell<tiny_skia::PathBuilder>>;

/// The distance along a quarter-circle's end tangents at which a cubic's
/// control points reproduce that quarter-circle to within a fraction of a
/// pixel, as a fraction of the radius. The standard circle-from-cubics
/// constant, `4/3 * (sqrt(2) - 1)`.
const KAPPA: f32 = 0.552_284_75;

/// The widest sweep [`append_arc`] approximates with a single cubic. A
/// quarter turn is where the error of a one-cubic approximation is still
/// far below one pixel at the radii a 720x360 surface can hold.
const MAX_ARC_SEGMENT: f32 = std::f32::consts::FRAC_PI_2;

/// Appends a rounded rectangle to `builder`, as a closed contour running
/// clockwise from its top-left corner. `radius` is clamped to half the
/// shorter side, so an over-large radius yields a stadium rather than a
/// self-crossing outline; a radius of zero yields a plain rectangle.
pub fn append_rounded_rect(
    builder: &mut tiny_skia::PathBuilder,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
) {
    let r = radius.clamp(0.0, (w.min(h) / 2.0).max(0.0));
    if r == 0.0 {
        if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            builder.push_rect(rect);
        }
        return;
    }

    let (right, bottom) = (x + w, y + h);
    let c = KAPPA * r;

    builder.move_to(x + r, y);
    builder.line_to(right - r, y);
    builder.cubic_to(right - r + c, y, right, y + r - c, right, y + r);
    builder.line_to(right, bottom - r);
    builder.cubic_to(
        right,
        bottom - r + c,
        right - r + c,
        bottom,
        right - r,
        bottom,
    );
    builder.line_to(x + r, bottom);
    builder.cubic_to(x + r - c, bottom, x, bottom - r + c, x, bottom - r);
    builder.line_to(x, y + r);
    builder.cubic_to(x, y + r - c, x + r - c, y, x + r, y);
    builder.close();
}

/// Appends the arc of the circle at `(cx, cy)` with radius `r` running from
/// `start` to `end`, in radians measured from `+x` and increasing toward
/// `+y`. The sweep follows the sign of `end - start`, so a program picks its
/// direction by which way round it names the two angles.
///
/// Connects to whatever the path already holds with a straight line, the way
/// appending any other segment would — so an arc appended straight after a
/// `moveTo` to the circle's centre closes into a pie slice.
pub fn append_arc(
    builder: &mut tiny_skia::PathBuilder,
    cx: f32,
    cy: f32,
    r: f32,
    start: f32,
    end: f32,
) {
    if !(r.is_finite() && start.is_finite() && end.is_finite()) || r <= 0.0 {
        return;
    }

    let point_at = |angle: f32| (cx + r * angle.cos(), cy + r * angle.sin());

    let (sx, sy) = point_at(start);
    if builder.last_point().is_some() {
        builder.line_to(sx, sy);
    } else {
        builder.move_to(sx, sy);
    }

    let sweep = end - start;
    if sweep == 0.0 {
        return;
    }

    // Split into equal segments no wider than a quarter turn, so the cubic
    // approximation below stays accurate however far the arc goes round.
    let segments = (sweep.abs() / MAX_ARC_SEGMENT).ceil() as u32;
    let step = sweep / segments as f32;
    // The tangent lengths that make one cubic match a circular sweep of
    // `step`; the quarter-turn case reduces to KAPPA.
    let handle = 4.0 / 3.0 * (step / 4.0).tan() * r;

    for segment in 0..segments {
        let a0 = start + step * segment as f32;
        let a1 = a0 + step;
        let (x0, y0) = point_at(a0);
        let (x1, y1) = point_at(a1);
        // Control points sit along each end's tangent, which for a circle is
        // its radius turned a quarter turn.
        builder.cubic_to(
            x0 - handle * a0.sin(),
            y0 + handle * a0.cos(),
            x1 + handle * a1.sin(),
            y1 - handle * a1.cos(),
            x1,
            y1,
        );
    }
}

/// Resolves the name a program gives a fill rule, throwing a `TypeError` for
/// anything else — the same way an out-of-range color id is rejected.
fn resolve_fill_rule(ctx: &Ctx<'_>, rule: &str) -> Result<tiny_skia::FillRule> {
    match rule {
        "nonzero" => Ok(tiny_skia::FillRule::Winding),
        "evenodd" => Ok(tiny_skia::FillRule::EvenOdd),
        other => Err(rquickjs::Exception::throw_type(
            ctx,
            &format!("{other:?} is not a valid fill rule"),
        )),
    }
}

fn resolve_line_cap(ctx: &Ctx<'_>, cap: &str) -> Result<tiny_skia::LineCap> {
    match cap {
        "butt" => Ok(tiny_skia::LineCap::Butt),
        "round" => Ok(tiny_skia::LineCap::Round),
        "square" => Ok(tiny_skia::LineCap::Square),
        other => Err(rquickjs::Exception::throw_type(
            ctx,
            &format!("{other:?} is not a valid line cap"),
        )),
    }
}

fn resolve_line_join(ctx: &Ctx<'_>, join: &str) -> Result<tiny_skia::LineJoin> {
    match join {
        "miter" => Ok(tiny_skia::LineJoin::Miter),
        "round" => Ok(tiny_skia::LineJoin::Round),
        "bevel" => Ok(tiny_skia::LineJoin::Bevel),
        other => Err(rquickjs::Exception::throw_type(
            ctx,
            &format!("{other:?} is not a valid line join"),
        )),
    }
}

/// Binds the hidden globals `ely:framebuffer`'s path calls wrap. Split out
/// from `super::bootstrap_framebuffer_bindings` only because the path half of
/// the device is large enough to read on its own; the two are bootstrapped
/// together and share the same draw command list.
pub fn bootstrap_path_bindings(
    ctx: &Ctx<'_>,
    draw_commands: Rc<RefCell<Vec<DrawCommand>>>,
    path: PathScratch,
) -> Result<()> {
    // Each of these appends to the path under construction and is a plain
    // mutation with nothing to reject, so none of them take a `Ctx` to throw
    // with. Coordinates that aren't finite are dropped by `finish` below.
    {
        let path = Rc::clone(&path);
        bind(ctx, "__framebuffer_path_begin", move || {
            path.borrow_mut().clear()
        })?;
    }

    {
        let path = Rc::clone(&path);
        bind(ctx, "__framebuffer_path_move_to", move |x: f32, y: f32| {
            path.borrow_mut().move_to(x, y)
        })?;
    }

    {
        let path = Rc::clone(&path);
        bind(ctx, "__framebuffer_path_line_to", move |x: f32, y: f32| {
            path.borrow_mut().line_to(x, y)
        })?;
    }

    {
        let path = Rc::clone(&path);
        bind(
            ctx,
            "__framebuffer_path_quad_to",
            move |cx: f32, cy: f32, x: f32, y: f32| path.borrow_mut().quad_to(cx, cy, x, y),
        )?;
    }

    {
        let path = Rc::clone(&path);
        bind(
            ctx,
            "__framebuffer_path_cubic_to",
            move |c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32| {
                path.borrow_mut().cubic_to(c1x, c1y, c2x, c2y, x, y)
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        bind(ctx, "__framebuffer_path_close", move || {
            path.borrow_mut().close()
        })?;
    }

    {
        let path = Rc::clone(&path);
        bind(
            ctx,
            "__framebuffer_path_rect",
            move |x: f32, y: f32, w: f32, h: f32| {
                if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
                    path.borrow_mut().push_rect(rect);
                }
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        bind(
            ctx,
            "__framebuffer_path_oval",
            move |cx: f32, cy: f32, rx: f32, ry: f32| {
                if let Some(oval) = tiny_skia::Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry) {
                    path.borrow_mut().push_oval(oval);
                }
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        bind(
            ctx,
            "__framebuffer_path_rounded_rect",
            move |x: f32, y: f32, w: f32, h: f32, radius: f32| {
                append_rounded_rect(&mut path.borrow_mut(), x, y, w, h, radius)
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        bind(
            ctx,
            "__framebuffer_path_arc",
            move |cx: f32, cy: f32, r: f32, start: f32, end: f32| {
                append_arc(&mut path.borrow_mut(), cx, cy, r, start, end)
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_fill_path",
            move |ctx: Ctx<'_>, color: u16, rule: String| -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                let rule = resolve_fill_rule(&ctx, &rule)?;
                // Clone rather than take: a program that fills a path
                // then strokes it describes that path once.
                let Some(path) = path.borrow().clone().finish() else {
                    return Ok(()); // fewer than two points, or not finite
                };
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::FillPath { path, rule, color });
                Ok(())
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_stroke_path",
            move |ctx: Ctx<'_>,
                  color: u16,
                  thickness: f32,
                  cap: String,
                  join: String|
                  -> Result<()> {
                let color = resolve_color(&ctx, color)?;
                if !thickness.is_finite() || thickness <= 0.0 {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        "stroke thickness must be greater than 0",
                    ));
                }
                let stroke = tiny_skia::Stroke {
                    width: thickness,
                    line_cap: resolve_line_cap(&ctx, &cap)?,
                    line_join: resolve_line_join(&ctx, &join)?,
                    ..tiny_skia::Stroke::default()
                };
                let Some(path) = path.borrow().clone().finish() else {
                    return Ok(());
                };
                draw_commands.borrow_mut().push(DrawCommand::StrokePath {
                    path,
                    stroke,
                    color,
                });
                Ok(())
            },
        )?;
    }

    {
        let path = Rc::clone(&path);
        let draw_commands = Rc::clone(&draw_commands);
        bind(
            ctx,
            "__framebuffer_push_clip",
            move |ctx: Ctx<'_>, rule: String| -> Result<()> {
                let rule = resolve_fill_rule(&ctx, &rule)?;
                // An unfinishable path confines drawing to nothing at
                // all, which is what an empty clip region means; still
                // push, so the matching pop stays balanced.
                let path = path.borrow().clone().finish();
                draw_commands
                    .borrow_mut()
                    .push(DrawCommand::PushClip { path, rule });
                Ok(())
            },
        )?;
    }

    bind(ctx, "__framebuffer_pop_clip", move || {
        draw_commands.borrow_mut().push(DrawCommand::PopClip)
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{append_arc, append_rounded_rect};

    /// The bounding box of a finished path, as `(left, top, right, bottom)`.
    fn bounds(builder: tiny_skia::PathBuilder) -> (f32, f32, f32, f32) {
        let path = builder.finish().expect("path should be finishable");
        let b = path.bounds();
        (b.left(), b.top(), b.right(), b.bottom())
    }

    fn rounded(x: f32, y: f32, w: f32, h: f32, radius: f32) -> tiny_skia::PathBuilder {
        let mut builder = tiny_skia::PathBuilder::new();
        append_rounded_rect(&mut builder, x, y, w, h, radius);
        builder
    }

    #[test]
    fn a_rounded_rect_stays_inside_the_rect_it_rounds() {
        // Control points of a corner cubic never leave the corner's own
        // square, so the rounded outline can't bulge past the plain one.
        let (l, t, r, b) = bounds(rounded(10.0, 20.0, 100.0, 50.0, 8.0));
        assert_eq!((l, t, r, b), (10.0, 20.0, 110.0, 70.0));
    }

    #[test]
    fn a_zero_radius_rounded_rect_is_a_plain_rect() {
        let rounded = rounded(10.0, 20.0, 100.0, 50.0, 0.0)
            .finish()
            .expect("path should be finishable");
        let plain = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(10.0, 20.0, 100.0, 50.0).expect("valid rect"),
        );
        assert_eq!(rounded.len(), plain.len(), "no corner curves to add points");
        assert_eq!(rounded.bounds(), plain.bounds());
    }

    #[test]
    fn a_rounded_rect_radius_is_clamped_to_half_the_shorter_side() {
        // An over-large radius would otherwise send the corner curves past
        // each other and cross the outline over itself.
        let huge = bounds(rounded(0.0, 0.0, 100.0, 40.0, 500.0));
        let stadium = bounds(rounded(0.0, 0.0, 100.0, 40.0, 20.0));
        assert_eq!(huge, stadium);
    }

    #[test]
    fn a_rounded_rect_with_no_area_covers_nothing() {
        // A zero-width rectangle is still a describable contour, it just has
        // no inside for a fill to land in.
        let flat = rounded(10.0, 10.0, 0.0, 50.0, 4.0)
            .finish()
            .expect("path should be finishable");
        assert_eq!(flat.bounds().width(), 0.0);
        // A negative size isn't a describable contour at all.
        assert!(rounded(10.0, 10.0, -5.0, 50.0, 4.0).finish().is_none());
    }

    fn arc(cx: f32, cy: f32, r: f32, start: f32, end: f32) -> tiny_skia::PathBuilder {
        let mut builder = tiny_skia::PathBuilder::new();
        append_arc(&mut builder, cx, cy, r, start, end);
        builder
    }

    #[test]
    fn a_full_turn_arc_spans_the_whole_circle() {
        let (l, t, r, b) = bounds(arc(50.0, 50.0, 20.0, 0.0, std::f32::consts::TAU));
        for (actual, expected) in [(l, 30.0), (t, 30.0), (r, 70.0), (b, 70.0)] {
            assert!(
                (actual - expected).abs() < 0.05,
                "{actual} should be within a twentieth of a pixel of {expected}"
            );
        }
    }

    #[test]
    fn a_quarter_turn_arc_runs_from_plus_x_toward_plus_y() {
        // Zero radians points right and angles increase downward, so this
        // quarter turn covers the bottom-right quadrant only.
        let (l, t, r, b) = bounds(arc(50.0, 50.0, 20.0, 0.0, std::f32::consts::FRAC_PI_2));
        assert!((l - 50.0).abs() < 0.05, "left edge should be the centre");
        assert!((t - 50.0).abs() < 0.05, "top edge should be the centre");
        assert!((r - 70.0).abs() < 0.05);
        assert!((b - 70.0).abs() < 0.05);
    }

    #[test]
    fn an_arc_named_backwards_sweeps_backwards() {
        let forward = bounds(arc(50.0, 50.0, 20.0, 0.0, std::f32::consts::FRAC_PI_2));
        let backward = bounds(arc(50.0, 50.0, 20.0, 0.0, -std::f32::consts::FRAC_PI_2));
        assert_ne!(forward, backward);
        // Sweeping the other way covers the top-right quadrant instead.
        assert!(backward.1 < 50.0 && backward.3 <= 50.05);
    }

    #[test]
    fn an_arc_continues_the_path_it_is_appended_to() {
        // A pie slice: out to the centre, round the rim, back again.
        let mut builder = tiny_skia::PathBuilder::new();
        builder.move_to(50.0, 50.0);
        append_arc(
            &mut builder,
            50.0,
            50.0,
            20.0,
            0.0,
            std::f32::consts::FRAC_PI_2,
        );
        builder.close();
        let (l, t, _, _) = bounds(builder);
        assert!((l - 50.0).abs() < 0.05 && (t - 50.0).abs() < 0.05);
    }

    #[test]
    fn a_degenerate_arc_produces_nothing() {
        assert!(arc(50.0, 50.0, 0.0, 0.0, 1.0).finish().is_none());
        assert!(arc(50.0, 50.0, f32::NAN, 0.0, 1.0).finish().is_none());
    }
}
