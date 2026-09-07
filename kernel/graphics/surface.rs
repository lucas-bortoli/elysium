//! A [`Surface`]: a retained pixmap plus the transform and clip stacks that
//! drawing against it is placed through. Every operation applies immediately
//! and stays — there is no command list and no frame boundary. The screen is
//! one of these; so is any offscreen surface a program creates.
//!
//! The per-operation methods here are what `ely:graphics`'s bindings call,
//! and what kernel code calls when it draws. They were once the match arms of
//! a `rasterize(&[DrawCommand])` loop, and the behaviour they carry over is
//! unchanged: nothing is anti-aliased, every pixel written is one whole
//! palette colour, and images are the only thing that composites at all —
//! their transparency snapped to all-or-nothing at load time, so a known
//! opaque one is copied straight in.

use tiny_skia::{
    BlendMode, ColorU8, FillRule, Paint, Path, Pixmap, PixmapPaint, Point, PremultipliedColorU8,
    Rect, Stroke, Transform,
};

use super::state::{Clip, ClipRect, DrawState};
use super::{Color, mapped_bounds};
use crate::text::{self, FontId};

/// A drawing surface: its pixels, and the transform and clip stacks in
/// effect against it. Both stacks persist between calls and between frames —
/// a caller that shares a surface with another is responsible for balancing
/// what it pushes.
pub struct Surface {
    pixmap: Pixmap,
    state: DrawState,
}

impl Surface {
    /// A fresh, fully transparent surface `width` by `height`. `None` if
    /// those dimensions are zero or too large to allocate.
    pub fn new(width: u32, height: u32) -> Option<Surface> {
        Some(Surface {
            pixmap: Pixmap::new(width, height)?,
            state: DrawState::new(width, height),
        })
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// The backing pixels, for presentation to the screen and for tests.
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// Resizes the backing pixmap, keeping the old contents at the top-left
    /// corner and leaving any newly exposed area transparent, so a resize
    /// mid-frame doesn't flash. Empties the transform and clip stacks — a
    /// clip is a region in the surface's own coordinates, and there is no
    /// honest way to reinterpret one against different bounds. A resize to
    /// the current dimensions changes nothing. `None` if the new dimensions
    /// are zero or too large to allocate.
    pub fn resize(&mut self, width: u32, height: u32) -> Option<()> {
        if (width, height) == (self.pixmap.width(), self.pixmap.height()) {
            return Some(());
        }
        let mut next = Pixmap::new(width, height)?;

        let src_w = self.pixmap.width() as usize;
        let dst_w = width as usize;
        let copy_w = src_w.min(dst_w);
        let copy_h = (self.pixmap.height().min(height)) as usize;
        let src = self.pixmap.pixels();
        let dst = next.pixels_mut();
        for row in 0..copy_h {
            dst[row * dst_w..row * dst_w + copy_w]
                .copy_from_slice(&src[row * src_w..row * src_w + copy_w]);
        }

        self.pixmap = next;
        self.state = DrawState::new(width, height);
        Some(())
    }

    /// Fills the whole surface with one palette colour, discarding whatever
    /// it held.
    pub fn clear(&mut self, color: Color) {
        self.pixmap.fill(color.to_skia());
    }

    /// Fills the axis-aligned rectangle at `(x, y)`, `w` by `h`, taken in the
    /// coordinates the transform stack currently maps.
    pub fn fill_rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let Some(rect) = Rect::from_xywh(x, y, w, h) else {
            return; // negative or non-finite size
        };
        let paint = solid_paint(color);
        let t = self.state.transform();
        let clip = self.state.clip_for(mapped_bounds(rect, t));
        self.pixmap.fill_rect(rect, &paint, t, clip);
    }

    /// Fills the inside of `path` — the shape behind every filled circle,
    /// polygon, rounded rectangle and arc.
    pub fn fill_path(&mut self, path: &Path, rule: FillRule, color: Color) {
        let paint = solid_paint(color);
        let t = self.state.transform();
        let clip = self.state.clip_for(mapped_bounds(path.bounds(), t));
        self.pixmap.fill_path(path, &paint, rule, t, clip);
    }

    /// Draws a line along `path`, straddling it with `stroke`'s own width.
    pub fn stroke_path(&mut self, path: &Path, stroke: &Stroke, color: Color) {
        let paint = solid_paint(color);
        let t = self.state.transform();
        // The outline reaches half the stroke width past the path.
        let reach = stroke.width * 0.5;
        let outset = Rect::from_ltrb(
            path.bounds().left() - reach,
            path.bounds().top() - reach,
            path.bounds().right() + reach,
            path.bounds().bottom() + reach,
        );
        let clip = self
            .state
            .clip_for(outset.and_then(|rect| mapped_bounds(rect, t)));
        self.pixmap.stroke_path(path, &paint, stroke, t, clip);
    }

    /// Sets the single pixel `(x, y)` falls inside. Bypasses the rasterizer,
    /// so it applies the transform and clip itself.
    pub fn set_pixel(&mut self, x: f32, y: f32, color: Color) {
        let (px, py) = self.state.map_point(x, y);
        let (px, py) = (px.floor() as i32, py.floor() as i32);
        if px >= 0
            && py >= 0
            && px < self.pixmap.width() as i32
            && py < self.pixmap.height() as i32
            && self.state.is_visible(px, py)
        {
            let width = self.pixmap.width() as i32;
            self.pixmap.pixels_mut()[(py * width + px) as usize] = solid_pixel(color);
        }
    }

    /// Lays `text` out under `layout` and draws every line, with the block
    /// anchored at `(x, y)` — the top-left, centre or right edge of each
    /// line depending on `layout.align`. Line breaks and wrapping happen
    /// here so `drawText` and `measureText` agree.
    pub fn draw_text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        layout: &text::TextLayout,
        color: Color,
    ) {
        let Some(font) = text::font_from_id(layout.font) else {
            return;
        };
        let (lines, _, _) = text::lay_out(font, layout, text);
        for line in lines {
            self.draw_text_line(
                x + line.dx,
                y + line.dy,
                &line.text,
                layout.font,
                layout.scale,
                color,
            );
        }
    }

    /// Draws one already-laid-out line of `text` with its top-left corner at
    /// `(x, y)`. Every lit pixel is one fully opaque palette colour written
    /// straight in; the text moves with the transform, but the glyphs
    /// themselves are never turned or resized.
    pub fn draw_text_line(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font: FontId,
        scale: u32,
        color: Color,
    ) {
        let Some(font) = text::font_from_id(font) else {
            return; // font id validated at the binding, but stay total
        };
        let solid = solid_pixel(color);
        let (ox, oy) = self.state.map_point(x, y);
        let (ox, oy) = (ox.round() as i32, oy.round() as i32);
        let scale = scale.max(1) as i32;
        let width = self.pixmap.width() as i32;
        let height = self.pixmap.height() as i32;
        let state = &self.state;
        let pixels = self.pixmap.pixels_mut();
        // Walked in the font's own pixels, so each one is laid down as a
        // `scale` x `scale` block.
        text::for_each_lit_pixel(font, text, 0, 0, |gx, gy| {
            for dy in 0..scale {
                for dx in 0..scale {
                    let px = ox + gx * scale + dx;
                    let py = oy + gy * scale + dy;
                    if px >= 0 && py >= 0 && px < width && py < height && state.is_visible(px, py) {
                        pixels[(py * width + px) as usize] = solid;
                    }
                }
            }
        });
    }

    /// Draws `image` whole with its top-left corner at `(x, y)`. The
    /// transform's shift is folded into the position; a non-axis-aligned
    /// transform is left to [`Surface::draw_image_transformed`].
    pub fn draw_image(&mut self, image: &Pixmap, opaque: bool, x: f32, y: f32) {
        let (dx, dy) = self.state.map_point(x, y);
        let (dx, dy) = (dx.round(), dy.round());
        let bounds = Rect::from_xywh(dx, dy, image.width() as f32, image.height() as f32);
        let clip = self.state.clip_for(bounds);
        let paint = if opaque {
            copy_paint()
        } else {
            PixmapPaint::default()
        };
        self.pixmap.draw_pixmap(
            dx as i32,
            dy as i32,
            image.as_ref(),
            &paint,
            Transform::identity(),
            clip,
        );
    }

    /// Draws `source` out of `image` under `transform`, which maps the source
    /// rect's own coordinates onto the surface — a crop, a flip, a resize, a
    /// turn.
    pub fn draw_image_transformed(
        &mut self,
        image: &Pixmap,
        opaque: bool,
        source: Rect,
        transform: Transform,
    ) {
        let full = self.state.transform().pre_concat(transform);
        blit_transformed(
            &mut self.pixmap,
            image,
            source,
            full,
            opaque,
            self.state.clip(),
        );
    }

    pub fn push_transform(&mut self, transform: Transform) {
        self.state.push_transform(transform);
    }

    pub fn pop_transform(&mut self) {
        self.state.pop_transform();
    }

    pub fn push_clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.state.push_clip_rect(x, y, w, h);
    }

    pub fn push_clip(&mut self, path: Option<&Path>, rule: FillRule) {
        self.state.push_clip(path, rule);
    }

    pub fn pop_clip(&mut self) {
        self.state.pop_clip();
    }
}

/// A non-anti-aliased solid-colour paint.
fn solid_paint(color: Color) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: false,
        ..Default::default()
    };
    paint.set_color(color.to_skia());
    paint
}

/// An opaque image has nothing to composite, so its pixels are copied
/// straight in rather than run through `source-over` per pixel.
fn copy_paint() -> PixmapPaint {
    PixmapPaint {
        blend_mode: BlendMode::Source,
        ..PixmapPaint::default()
    }
}

/// One fully opaque palette colour, premultiplied, ready to write into a
/// pixmap's pixel buffer.
fn solid_pixel(color: Color) -> PremultipliedColorU8 {
    let hex = color.hex();
    ColorU8::from_rgba(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
        255,
    )
    .premultiply()
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
    dest: &mut Pixmap,
    image: &Pixmap,
    source: Rect,
    transform: Transform,
    opaque: bool,
    clip: Clip<'_>,
) {
    // `inverse` walks a surface pixel back to a point in the source rect's
    // own space; a degenerate transform has no inverse and covers nothing.
    let Some(inverse) = transform.invert() else {
        return;
    };
    let Some(footprint) = Rect::from_xywh(0.0, 0.0, source.width(), source.height())
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
        let mut probe = [Point::from_xy(x0 as f32 + 0.5, py as f32 + 0.5)];
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
fn over(src: PremultipliedColorU8, dst: PremultipliedColorU8) -> PremultipliedColorU8 {
    let inv = 255 - src.alpha() as u32;
    let blend = |s: u8, d: u8| (s as u32 + (d as u32 * inv + 127) / 255) as u8;
    PremultipliedColorU8::from_rgba(
        blend(src.red(), dst.red()),
        blend(src.green(), dst.green()),
        blend(src.blue(), dst.blue()),
        blend(src.alpha(), dst.alpha()),
    )
    .unwrap_or(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> Surface {
        Surface::new(64, 64).expect("failed to allocate a test surface")
    }

    /// The colour at `(x, y)` as `0xRRGGBB`, the same packing `Color::hex`
    /// uses.
    fn pixel_at(surface: &Surface, x: u32, y: u32) -> u32 {
        let pixmap = surface.pixmap();
        let p = pixmap.pixels()[(y * pixmap.width() + x) as usize].demultiply();
        ((p.red() as u32) << 16) | ((p.green() as u32) << 8) | p.blue() as u32
    }

    fn lit_pixels(surface: &Surface, color: Color) -> usize {
        let pixmap = surface.pixmap();
        (0..pixmap.height())
            .flat_map(|y| (0..pixmap.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel_at(surface, x, y) == color.hex())
            .count()
    }

    fn assert_every_pixel_is_a_palette_color(surface: &Surface) {
        let pixmap = surface.pixmap();
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                let found = pixel_at(surface, x, y);
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

    fn circle(cx: f32, cy: f32, r: f32) -> Path {
        let mut builder = tiny_skia::PathBuilder::new();
        builder.push_circle(cx, cy, r);
        builder.finish().expect("a circle should be finishable")
    }

    fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Path {
        tiny_skia::PathBuilder::from_rect(Rect::from_xywh(x, y, w, h).expect("valid rect"))
    }

    #[test]
    fn a_new_surface_starts_fully_transparent() {
        let surface = surface();
        assert!(
            surface.pixmap().pixels().iter().all(|p| p.alpha() == 0),
            "a fresh surface should hold nothing"
        );
    }

    #[test]
    fn operations_accumulate_across_calls_with_no_frame_boundary() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.fill_rectangle(0.0, 0.0, 10.0, 10.0, Color::Amber400);
        surface.fill_rectangle(20.0, 20.0, 10.0, 10.0, Color::Teal300);
        // The first rectangle is still there — nothing cleared between calls.
        assert_eq!(pixel_at(&surface, 5, 5), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 25, 25), Color::Teal300.hex());
        assert_eq!(pixel_at(&surface, 40, 40), Color::Slate900.hex());
    }

    #[test]
    fn every_pixel_a_surface_holds_is_a_palette_color() {
        // The promise the whole device rests on: a program can only name
        // palette colours, so a surface can only ever hold palette colours.
        // Curves and outlines are where that would break if anything
        // anti-aliased or blended.
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.fill_path(
            &circle(20.0, 20.0, 15.0),
            FillRule::Winding,
            Color::Amber400,
        );
        surface.stroke_path(
            &circle(40.0, 40.0, 18.0),
            &Stroke {
                width: 3.0,
                ..Default::default()
            },
            Color::Teal300,
        );
        surface.stroke_path(
            &rect_path(4.0, 44.0, 25.0, 15.0),
            &Stroke {
                width: 1.0,
                ..Default::default()
            },
            Color::Rose500,
        );
        assert_every_pixel_is_a_palette_color(&surface);
    }

    #[test]
    fn a_filled_path_paints_the_color_it_was_given() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.fill_path(
            &circle(32.0, 32.0, 20.0),
            FillRule::Winding,
            Color::Amber400,
        );
        assert_eq!(pixel_at(&surface, 32, 32), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 0, 0), Color::Slate900.hex());
    }

    #[test]
    fn a_clip_confines_what_is_drawn_under_it_and_lifts_when_popped() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.push_clip(Some(&rect_path(0.0, 0.0, 32.0, 64.0)), FillRule::Winding);
        surface.fill_path(
            &rect_path(0.0, 0.0, 64.0, 64.0),
            FillRule::Winding,
            Color::Amber400,
        );
        surface.pop_clip();
        surface.fill_path(
            &rect_path(0.0, 0.0, 64.0, 64.0),
            FillRule::Winding,
            Color::Teal300,
        );
        // Left half: covered by both fills, so the later Teal wins. Right
        // half: only the second fill reached it.
        assert_eq!(pixel_at(&surface, 10, 10), Color::Teal300.hex());
        assert_eq!(pixel_at(&surface, 50, 10), Color::Teal300.hex());
    }

    #[test]
    fn a_clip_and_transform_stack_survive_between_calls() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.push_transform(Transform::from_translate(40.0, 0.0));
        // A separate call, but the transform pushed above is still in effect.
        surface.fill_rectangle(0.0, 0.0, 10.0, 10.0, Color::Amber400);
        surface.pop_transform();
        surface.fill_rectangle(0.0, 20.0, 10.0, 10.0, Color::Teal300);
        assert_eq!(pixel_at(&surface, 45, 5), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 5, 5), Color::Slate900.hex());
        assert_eq!(pixel_at(&surface, 5, 25), Color::Teal300.hex());
    }

    #[test]
    fn a_rectangle_fill_lands_on_exactly_the_pixels_its_corners_name() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.fill_rectangle(10.0, 10.0, 5.0, 5.0, Color::Amber400);
        assert_eq!(pixel_at(&surface, 10, 10), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 14, 14), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 15, 14), Color::Slate900.hex());
        assert_eq!(pixel_at(&surface, 9, 10), Color::Slate900.hex());
    }

    #[test]
    fn a_pixel_lands_on_the_pixel_its_coordinate_falls_inside() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.set_pixel(3.0, 4.0, Color::Amber400);
        surface.set_pixel(10.9, 10.1, Color::Teal300);
        assert_eq!(pixel_at(&surface, 3, 4), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 10, 10), Color::Teal300.hex());
        assert_eq!(pixel_at(&surface, 11, 10), Color::Slate900.hex());
    }

    #[test]
    fn a_pixel_outside_the_surface_or_a_clip_is_dropped() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.set_pixel(-5.0, 10.0, Color::Amber400);
        surface.set_pixel(1000.0, 10.0, Color::Amber400);
        surface.push_clip(Some(&rect_path(0.0, 0.0, 10.0, 10.0)), FillRule::Winding);
        surface.set_pixel(20.0, 20.0, Color::Amber400);
        assert_eq!(lit_pixels(&surface, Color::Amber400), 0);
    }

    #[test]
    fn text_is_confined_by_a_clip_and_moved_by_a_transform() {
        let mut unclipped = surface();
        unclipped.clear(Color::Slate900);
        unclipped.draw_text_line(30.0, 2.0, "Hi", 0, 1, Color::Amber400);
        assert!(
            lit_pixels(&unclipped, Color::Amber400) > 0,
            "the text should have drawn something"
        );

        // Shifted right by 30 from the same origin, it starts past the clip
        // and none of it survives.
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.push_clip(Some(&rect_path(0.0, 0.0, 30.0, 64.0)), FillRule::Winding);
        surface.push_transform(Transform::from_translate(30.0, 0.0));
        surface.draw_text_line(30.0, 2.0, "Hi", 0, 1, Color::Amber400);
        assert_eq!(lit_pixels(&surface, Color::Amber400), 0);
    }

    #[test]
    fn scaled_text_lights_a_whole_block_per_font_pixel() {
        let lit_at = |scale: u32| {
            let mut surface = Surface::new(256, 128).expect("test surface");
            surface.clear(Color::Slate900);
            surface.draw_text_line(2.0, 2.0, "Hi", 0, scale, Color::Amber400);
            lit_pixels(&surface, Color::Amber400)
        };
        let single = lit_at(1);
        assert!(single > 0, "the text should have drawn something");
        assert_eq!(lit_at(3), single * 9);
    }

    /// A 4x4 image with a single distinct pixel at `(1, 1)`, so a source
    /// rect can be told apart from the whole image.
    fn marked_image() -> Pixmap {
        let mut image = Pixmap::new(4, 4).expect("test image");
        image.fill(Color::Teal300.to_skia());
        // Row 1, column 1 of a 4-wide image.
        image.pixels_mut()[4 + 1] = solid_pixel(Color::Rose500);
        image
    }

    #[test]
    fn a_source_rect_crops_to_the_part_of_the_image_it_names() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.draw_image_transformed(
            &marked_image(),
            true,
            Rect::from_xywh(1.0, 1.0, 1.0, 1.0).unwrap(),
            Transform::from_translate(10.0, 10.0),
        );
        assert_eq!(pixel_at(&surface, 10, 10), Color::Rose500.hex());
        assert_eq!(pixel_at(&surface, 11, 10), Color::Slate900.hex());
        assert_eq!(pixel_at(&surface, 9, 10), Color::Slate900.hex());
    }

    #[test]
    fn a_scaled_image_grows_by_whole_pixel_blocks() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.draw_image_transformed(
            &marked_image(),
            true,
            Rect::from_xywh(1.0, 1.0, 1.0, 1.0).unwrap(),
            Transform::from_row(4.0, 0.0, 0.0, 4.0, 10.0, 10.0),
        );
        assert_eq!(lit_pixels(&surface, Color::Rose500), 16);
        assert_eq!(pixel_at(&surface, 13, 13), Color::Rose500.hex());
        assert_eq!(pixel_at(&surface, 14, 14), Color::Slate900.hex());
    }

    #[test]
    fn a_flipped_image_covers_the_same_box_the_other_way_round() {
        let placements = [
            (Transform::identity(), (11, 11)),
            (Transform::from_row(-1.0, 0.0, 0.0, 1.0, 4.0, 0.0), (12, 11)),
        ];
        for (flip, (x, y)) in placements {
            let mut surface = surface();
            surface.clear(Color::Slate900);
            surface.draw_image_transformed(
                &marked_image(),
                true,
                Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
                Transform::from_translate(10.0, 10.0).pre_concat(flip),
            );
            assert_eq!(pixel_at(&surface, x, y), Color::Rose500.hex());
            assert_eq!(lit_pixels(&surface, Color::Rose500), 1);
        }
    }

    #[test]
    fn a_turned_or_resized_image_still_only_paints_palette_colors() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.draw_image_transformed(
            &marked_image(),
            true,
            Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
            Transform::from_translate(20.0, 20.0)
                .pre_concat(Transform::from_rotate(37.0))
                .pre_concat(Transform::from_scale(3.5, 2.25)),
        );
        assert_every_pixel_is_a_palette_color(&surface);
    }

    #[test]
    fn a_fractionally_scaled_axis_aligned_image_only_paints_palette_colors() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.draw_image_transformed(
            &marked_image(),
            true,
            Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
            Transform::from_row(3.5, 0.0, 0.0, 2.25, 12.0, 9.0),
        );
        assert_every_pixel_is_a_palette_color(&surface);
    }

    #[test]
    fn a_transformed_image_is_confined_by_a_clip() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.push_clip(Some(&rect_path(0.0, 0.0, 18.0, 64.0)), FillRule::Winding);
        surface.draw_image_transformed(
            &marked_image(),
            true,
            Rect::from_xywh(0.0, 0.0, 4.0, 4.0).unwrap(),
            Transform::from_row(4.0, 0.0, 0.0, 4.0, 10.0, 10.0),
        );
        surface.pop_clip();
        assert_eq!(pixel_at(&surface, 12, 12), Color::Teal300.hex());
        assert_eq!(pixel_at(&surface, 24, 12), Color::Slate900.hex());
    }

    #[test]
    fn a_clear_texel_in_a_transformed_image_leaves_the_background() {
        let mut image = Pixmap::new(2, 2).expect("test image");
        image.pixels_mut()[0] = solid_pixel(Color::Rose500);

        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.draw_image_transformed(
            &image,
            false,
            Rect::from_xywh(0.0, 0.0, 2.0, 2.0).unwrap(),
            Transform::from_row(5.0, 0.0, 0.0, 5.0, 10.0, 10.0),
        );
        assert_eq!(pixel_at(&surface, 12, 12), Color::Rose500.hex());
        assert_eq!(pixel_at(&surface, 20, 20), Color::Slate900.hex());
    }

    #[test]
    fn the_whole_image_fast_path_places_by_whole_pixels() {
        let mut surface = surface();
        surface.clear(Color::Slate900);
        surface.push_transform(Transform::from_translate(5.0, 6.0));
        surface.draw_image(&marked_image(), true, 0.0, 0.0);
        // The marked pixel sits one in from the image's corner, which the
        // transform put at (5, 6).
        assert_eq!(pixel_at(&surface, 6, 7), Color::Rose500.hex());
        assert_eq!(pixel_at(&surface, 5, 6), Color::Teal300.hex());
    }

    #[test]
    fn a_resize_keeps_the_old_contents_at_the_top_left() {
        let mut surface = Surface::new(32, 32).expect("test surface");
        surface.clear(Color::Slate900);
        surface.fill_rectangle(0.0, 0.0, 8.0, 8.0, Color::Amber400);
        surface.resize(64, 48).expect("resize");
        assert_eq!(surface.width(), 64);
        assert_eq!(surface.height(), 48);
        // The old block is where it was.
        assert_eq!(pixel_at(&surface, 4, 4), Color::Amber400.hex());
        assert_eq!(pixel_at(&surface, 20, 20), Color::Slate900.hex());
        // Newly exposed area is transparent, not carried over.
        assert_eq!(surface.pixmap().pixels()[63].alpha(), 0);
    }

    #[test]
    fn a_resize_empties_the_transform_and_clip_stacks() {
        let mut surface = Surface::new(32, 32).expect("test surface");
        surface.push_transform(Transform::from_translate(10.0, 10.0));
        surface.push_clip(Some(&rect_path(0.0, 0.0, 4.0, 4.0)), FillRule::Winding);
        surface.resize(40, 40).expect("resize");
        surface.clear(Color::Slate900);
        // If the clip or transform had survived, this rectangle would land
        // shifted or be confined to a 4x4 corner.
        surface.fill_rectangle(20.0, 20.0, 4.0, 4.0, Color::Amber400);
        assert_eq!(pixel_at(&surface, 21, 21), Color::Amber400.hex());
    }

    #[test]
    fn a_resize_to_the_current_size_is_a_no_op() {
        let mut surface = Surface::new(32, 32).expect("test surface");
        surface.clear(Color::Amber400);
        let before: Vec<_> = surface.pixmap().pixels().to_vec();
        surface.resize(32, 32).expect("resize");
        assert_eq!(surface.pixmap().pixels(), before.as_slice());
    }
}
