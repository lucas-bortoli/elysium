//! The shape vocabulary, lowered onto [`Surface`]'s path fill and stroke.
//!
//! Every named shape a program can draw — a stroked rectangle, a rounded
//! rectangle, an ellipse, an arc, a polygon — is one path, filled or
//! stroked. This is where that lowering lives, so kernel code draws a circle
//! the same way a program does and the JS module stays a thin shim. The
//! curve segments come from [`super::paths`]'s cubic approximations.

use tiny_skia::{FillRule, LineCap, LineJoin, PathBuilder, Rect, Stroke};

use super::Color;
use super::paths::{append_arc, append_rounded_rect};
use super::surface::Surface;

fn stroke(width: f32, cap: LineCap, join: LineJoin) -> Stroke {
    Stroke {
        width,
        line_cap: cap,
        line_join: join,
        ..Stroke::default()
    }
}

/// Builds a polyline path through `points` (`[x0, y0, x1, y1, ...]`),
/// optionally closed. `None` if there are fewer than two points.
fn polyline(points: &[f32], close: bool) -> Option<tiny_skia::Path> {
    if points.len() < 4 {
        return None;
    }
    let mut builder = PathBuilder::new();
    builder.move_to(points[0], points[1]);
    for pair in points[2..].chunks_exact(2) {
        builder.line_to(pair[0], pair[1]);
    }
    if close {
        builder.close();
    }
    builder.finish()
}

impl Surface {
    /// The outline of an axis-aligned rectangle. Straddles the edge, so it
    /// doesn't land on the pixels [`Surface::fill_rectangle`] would.
    pub fn stroke_rectangle(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
        thickness: f32,
    ) {
        let Some(rect) = Rect::from_xywh(x, y, w, h) else {
            return;
        };
        self.stroke_path(
            &PathBuilder::from_rect(rect),
            &stroke(thickness, LineCap::Butt, LineJoin::Miter),
            color,
        );
    }

    /// A rectangle whose corners are rounded off by `radius`, clamped to
    /// half the shorter side.
    pub fn fill_rounded_rectangle(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: Color,
    ) {
        let mut builder = PathBuilder::new();
        append_rounded_rect(&mut builder, x, y, w, h, radius);
        if let Some(path) = builder.finish() {
            self.fill_path(&path, FillRule::Winding, color);
        }
    }

    /// The outline of a rounded rectangle.
    #[allow(clippy::too_many_arguments)] // one lowering call, spelled out
    pub fn stroke_rounded_rectangle(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: Color,
        thickness: f32,
    ) {
        let mut builder = PathBuilder::new();
        append_rounded_rect(&mut builder, x, y, w, h, radius);
        if let Some(path) = builder.finish() {
            self.stroke_path(
                &path,
                &stroke(thickness, LineCap::Butt, LineJoin::Miter),
                color,
            );
        }
    }

    /// A straight line from `(x1, y1)` to `(x2, y2)`.
    pub fn draw_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, thickness: f32) {
        let mut builder = PathBuilder::new();
        builder.move_to(x1, y1);
        builder.line_to(x2, y2);
        if let Some(path) = builder.finish() {
            self.stroke_path(
                &path,
                &stroke(thickness, LineCap::Butt, LineJoin::Miter),
                color,
            );
        }
    }

    /// Straight lines through `points` (`[x0, y0, ...]`), ends left loose.
    pub fn draw_polyline(&mut self, points: &[f32], color: Color, thickness: f32) {
        if let Some(path) = polyline(points, false) {
            self.stroke_path(
                &path,
                &stroke(thickness, LineCap::Butt, LineJoin::Round),
                color,
            );
        }
    }

    /// A filled axis-aligned ellipse centred on `(cx, cy)`.
    pub fn fill_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
        if let Some(oval) = Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry) {
            let mut builder = PathBuilder::new();
            builder.push_oval(oval);
            if let Some(path) = builder.finish() {
                self.fill_path(&path, FillRule::Winding, color);
            }
        }
    }

    /// The outline of an axis-aligned ellipse.
    pub fn stroke_ellipse(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        color: Color,
        thickness: f32,
    ) {
        if let Some(oval) = Rect::from_ltrb(cx - rx, cy - ry, cx + rx, cy + ry) {
            let mut builder = PathBuilder::new();
            builder.push_oval(oval);
            if let Some(path) = builder.finish() {
                self.stroke_path(
                    &path,
                    &stroke(thickness, LineCap::Butt, LineJoin::Miter),
                    color,
                );
            }
        }
    }

    /// The piece of a circle's rim from `start` to `end` radians.
    #[allow(clippy::too_many_arguments)] // one lowering call, spelled out
    pub fn draw_arc(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        start: f32,
        end: f32,
        color: Color,
        thickness: f32,
    ) {
        let mut builder = PathBuilder::new();
        append_arc(&mut builder, cx, cy, r, start, end);
        if let Some(path) = builder.finish() {
            self.stroke_path(
                &path,
                &stroke(thickness, LineCap::Butt, LineJoin::Round),
                color,
            );
        }
    }

    /// Fills the shape enclosed by `points` (`[x0, y0, ...]`), closed back
    /// to the first. Fewer than three points encloses nothing.
    pub fn fill_polygon(&mut self, points: &[f32], color: Color, rule: FillRule) {
        if points.len() >= 6
            && let Some(path) = polyline(points, true)
        {
            self.fill_path(&path, rule, color);
        }
    }

    /// The outline of the shape enclosed by `points`, closed back to the
    /// first — unlike [`Surface::draw_polyline`], whose ends stay loose.
    pub fn stroke_polygon(&mut self, points: &[f32], color: Color, thickness: f32) {
        if points.len() >= 6
            && let Some(path) = polyline(points, true)
        {
            self.stroke_path(
                &path,
                &stroke(thickness, LineCap::Butt, LineJoin::Miter),
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Surface;
    use crate::graphics::Color;

    fn pixel_at(surface: &Surface, x: u32, y: u32) -> u32 {
        let p = surface.pixmap().pixels()[(y * surface.pixmap().width() + x) as usize].demultiply();
        ((p.red() as u32) << 16) | ((p.green() as u32) << 8) | p.blue() as u32
    }

    #[test]
    fn a_filled_polygon_paints_its_interior() {
        let mut s = Surface::new(64, 64).unwrap();
        s.clear(Color::Slate900);
        // A big triangle covering the centre.
        s.fill_polygon(
            &[4.0, 4.0, 60.0, 4.0, 32.0, 60.0],
            Color::Amber400,
            tiny_skia::FillRule::Winding,
        );
        assert_eq!(pixel_at(&s, 32, 20), Color::Amber400.hex());
        assert_eq!(pixel_at(&s, 2, 60), Color::Slate900.hex());
    }

    #[test]
    fn a_stroked_rectangle_paints_its_edge_but_not_its_middle() {
        let mut s = Surface::new(64, 64).unwrap();
        s.clear(Color::Slate900);
        s.stroke_rectangle(10.0, 10.0, 30.0, 30.0, Color::Teal300, 2.0);
        assert_eq!(pixel_at(&s, 10, 25), Color::Teal300.hex());
        assert_eq!(pixel_at(&s, 25, 25), Color::Slate900.hex());
    }

    #[test]
    fn a_polygon_too_short_to_enclose_anything_draws_nothing() {
        let mut s = Surface::new(32, 32).unwrap();
        s.clear(Color::Slate900);
        s.fill_polygon(
            &[1.0, 1.0, 2.0, 2.0],
            Color::Amber400,
            tiny_skia::FillRule::Winding,
        );
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(pixel_at(&s, x, y), Color::Slate900.hex());
            }
        }
    }
}
