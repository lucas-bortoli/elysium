//! The transform and clip stacks a frame's drawing is replayed against.
//!
//! Both work the same way: a program pushes one, draws, and pops it, and
//! pushing a second nests inside the first rather than replacing it. So each
//! stack holds values that are already combined with everything beneath them
//! — the top of the stack is the state in effect, and popping restores the
//! previous one without recomputing anything.
//!
//! Nesting means something slightly different for each. Transforms compose:
//! an inner one is applied before the outer one, so a program can hold a
//! camera in the outer transform and place an object within it in the inner.
//! Clips only ever narrow: an inner clip is intersected with the outer one,
//! so drawing can never escape a region an enclosing clip already confined it
//! to.
//!
//! A clip region is kept as its bounding box plus, only when it needs one, a
//! coverage mask. A rectangular clip under an axis-aligned transform — the
//! common case, a panel or a viewport — is exactly its box and carries no
//! mask, so pushing it costs nothing and a shape drawn wholly inside it is
//! drawn unclipped. A rotated or path-shaped clip, or a shape that spills out
//! of a rectangular one, falls back to a full-surface mask.

/// An axis-aligned box in surface pixels, half-open and clamped to the
/// surface: `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl ClipRect {
    fn surface(width: u32, height: u32) -> ClipRect {
        ClipRect {
            x0: 0,
            y0: 0,
            x1: width as i32,
            y1: height as i32,
        }
    }

    /// The tightest whole-pixel box covering `rect`, clamped to `surface`.
    fn covering(rect: tiny_skia::Rect, surface: ClipRect) -> ClipRect {
        ClipRect {
            x0: (rect.left().floor() as i32).clamp(surface.x0, surface.x1),
            y0: (rect.top().floor() as i32).clamp(surface.y0, surface.y1),
            x1: (rect.right().ceil() as i32).clamp(surface.x0, surface.x1),
            y1: (rect.bottom().ceil() as i32).clamp(surface.y0, surface.y1),
        }
    }

    fn intersect(self, other: ClipRect) -> ClipRect {
        let r = ClipRect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        };
        ClipRect {
            x1: r.x1.max(r.x0),
            y1: r.y1.max(r.y0),
            ..r
        }
    }

    /// Whether every pixel `rect` can touch lies inside this box. Rounded
    /// outward, so a `true` is always safe to act on.
    fn contains(self, rect: tiny_skia::Rect) -> bool {
        rect.left().floor() as i32 >= self.x0
            && rect.top().floor() as i32 >= self.y0
            && (rect.right().ceil() as i32) <= self.x1
            && (rect.bottom().ceil() as i32) <= self.y1
    }

    fn contains_point(self, x: i32, y: i32) -> bool {
        x >= self.x0 && y >= self.y0 && x < self.x1 && y < self.y1
    }

    fn to_path(self) -> Option<tiny_skia::Path> {
        tiny_skia::Rect::from_ltrb(
            self.x0 as f32,
            self.y0 as f32,
            self.x1 as f32,
            self.y1 as f32,
        )
        .map(tiny_skia::PathBuilder::from_rect)
    }
}

/// One entry on the clip stack, already narrowed by everything beneath it.
struct ClipRegion {
    bounds: ClipRect,
    /// A coverage mask over the whole surface. `None` means the region is
    /// exactly `bounds` — a plain rectangle. It can be filled in lazily, the
    /// first time a shape straddles the rectangle's edge.
    mask: Option<tiny_skia::Mask>,
}

/// The drawing state in effect at one point in a frame's command list.
pub struct DrawState {
    /// Each entry already composed with everything below it, so the last is
    /// the transform in effect. Empty means no transform.
    transforms: Vec<tiny_skia::Transform>,
    /// Each entry already narrowed by everything below it, so the last is the
    /// region drawing is confined to. Empty means unconfined.
    clips: Vec<ClipRegion>,
    width: u32,
    height: u32,
}

impl DrawState {
    /// A state with nothing pushed, for a surface `width` x `height` — the
    /// size every clip region is rasterized at.
    pub fn new(width: u32, height: u32) -> DrawState {
        DrawState {
            transforms: Vec::new(),
            clips: Vec::new(),
            width,
            height,
        }
    }

    /// The transform in effect, mapping the coordinates a program draws with
    /// onto the surface.
    pub fn transform(&self) -> tiny_skia::Transform {
        self.transforms
            .last()
            .copied()
            .unwrap_or_else(tiny_skia::Transform::identity)
    }

    /// The box drawing is confined to, or `None` while unconfined.
    pub fn clip_bounds(&self) -> Option<ClipRect> {
        self.clips.last().map(|region| region.bounds)
    }

    /// The coverage mask in effect, present only when the region isn't a
    /// plain rectangle its box already describes.
    pub fn clip_mask(&self) -> Option<&tiny_skia::Mask> {
        self.clips.last().and_then(|region| region.mask.as_ref())
    }

    /// The clip to hand a tiny-skia fill that covers `shape` (a surface-space
    /// box, `None` if its extent isn't known). `None` when the shape is
    /// wholly inside a rectangular clip and needs no clipping at all;
    /// otherwise the coverage mask, materializing one for a rectangle the
    /// shape spills out of.
    pub fn clip_for(&mut self, shape: Option<tiny_skia::Rect>) -> Option<&tiny_skia::Mask> {
        let region = self.clips.last_mut()?;
        if region.mask.is_none() {
            if let Some(shape) = shape
                && region.bounds.contains(shape)
            {
                return None;
            }
            let mut mask = tiny_skia::Mask::new(self.width, self.height)?;
            if let Some(path) = region.bounds.to_path() {
                mask.fill_path(
                    &path,
                    tiny_skia::FillRule::Winding,
                    false,
                    tiny_skia::Transform::identity(),
                );
            }
            region.mask = Some(mask);
        }
        region.mask.as_ref()
    }

    /// Whether the surface pixel at `(x, y)` is inside the current clip.
    /// Used by the drawing that writes pixels directly instead of going
    /// through the rasterizer, which applies the clip itself.
    pub fn is_visible(&self, x: i32, y: i32) -> bool {
        let Some(region) = self.clips.last() else {
            return true;
        };
        if !region.bounds.contains_point(x, y) {
            return false;
        }
        match &region.mask {
            // `bounds` already put `(x, y)` on the surface.
            Some(mask) => mask.data()[(y * self.width as i32 + x) as usize] != 0,
            None => true,
        }
    }

    /// Maps a point from the coordinates a program draws with onto the
    /// surface, under the transform in effect.
    pub fn map_point(&self, x: f32, y: f32) -> (f32, f32) {
        let mut points = [tiny_skia::Point::from_xy(x, y)];
        self.transform().map_points(&mut points);
        (points[0].x, points[0].y)
    }

    /// Nests `transform` inside the one already in effect.
    pub fn push_transform(&mut self, transform: tiny_skia::Transform) {
        self.transforms.push(self.transform().pre_concat(transform));
    }

    pub fn pop_transform(&mut self) {
        self.transforms.pop();
    }

    fn surface_bounds(&self) -> ClipRect {
        ClipRect::surface(self.width, self.height)
    }

    fn parent_bounds(&self) -> ClipRect {
        self.clips
            .last()
            .map(|region| region.bounds)
            .unwrap_or_else(|| self.surface_bounds())
    }

    /// Narrows the clip to the rectangle `(x, y, w, h)`, taken in the
    /// coordinates currently in effect. An axis-aligned transform keeps this
    /// a bare rectangle; rotation or shear turns it into a masked region.
    pub fn push_clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let transform = self.transform();
        let parent_bounds = self.parent_bounds();
        let parent_mask = self.clips.last().and_then(|region| region.mask.as_ref());

        let Some(local) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
            // A non-positive rectangle confines drawing to nothing.
            self.clips.push(ClipRegion {
                bounds: ClipRect {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                },
                mask: None,
            });
            return;
        };

        let axis_aligned = transform.kx == 0.0 && transform.ky == 0.0;
        let mut corners = [
            tiny_skia::Point::from_xy(local.left(), local.top()),
            tiny_skia::Point::from_xy(local.right(), local.top()),
            tiny_skia::Point::from_xy(local.right(), local.bottom()),
            tiny_skia::Point::from_xy(local.left(), local.bottom()),
        ];
        transform.map_points(&mut corners);
        let mapped = bounding_rect(&corners);
        let bounds = mapped
            .map(|rect| ClipRect::covering(rect, self.surface_bounds()))
            .unwrap_or(ClipRect {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            })
            .intersect(parent_bounds);

        if axis_aligned && parent_mask.is_none() {
            self.clips.push(ClipRegion { bounds, mask: None });
            return;
        }

        // Rotated, sheared, or nested inside a mask: the region is no longer
        // a bare rectangle, so it needs its own coverage mask.
        let mask = self.masked_region(parent_mask, |mask| {
            let rect_path = tiny_skia::PathBuilder::from_rect(local);
            match parent_mask {
                Some(_) => {
                    mask.intersect_path(&rect_path, tiny_skia::FillRule::Winding, false, transform)
                }
                None => mask.fill_path(&rect_path, tiny_skia::FillRule::Winding, false, transform),
            }
        });
        self.clips.push(ClipRegion { bounds, mask });
    }

    /// Narrows the clip to the inside of `path` (in the current coordinates),
    /// or to nothing when there is no finishable path. Always a masked
    /// region — an arbitrary shape is never just a box.
    pub fn push_clip(&mut self, path: Option<&tiny_skia::Path>, rule: tiny_skia::FillRule) {
        let transform = self.transform();
        let parent_bounds = self.parent_bounds();
        let parent_mask = self.clips.last().and_then(|region| region.mask.as_ref());

        let Some(path) = path else {
            self.clips.push(ClipRegion {
                bounds: ClipRect {
                    x0: 0,
                    y0: 0,
                    x1: 0,
                    y1: 0,
                },
                mask: None,
            });
            return;
        };

        let mut corners = [
            tiny_skia::Point::from_xy(path.bounds().left(), path.bounds().top()),
            tiny_skia::Point::from_xy(path.bounds().right(), path.bounds().top()),
            tiny_skia::Point::from_xy(path.bounds().right(), path.bounds().bottom()),
            tiny_skia::Point::from_xy(path.bounds().left(), path.bounds().bottom()),
        ];
        transform.map_points(&mut corners);
        let bounds = bounding_rect(&corners)
            .map(|rect| ClipRect::covering(rect, self.surface_bounds()))
            .unwrap_or(ClipRect {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            })
            .intersect(parent_bounds);

        let mask = self.masked_region(parent_mask, |mask| match parent_mask {
            Some(_) => mask.intersect_path(path, rule, false, transform),
            None => mask.fill_path(path, rule, false, transform),
        });
        self.clips.push(ClipRegion { bounds, mask });
    }

    pub fn pop_clip(&mut self) {
        self.clips.pop();
    }

    /// Builds a coverage mask, seeded from `parent` when the clip nests
    /// inside one, and lets `fill` lay the new region into it. `None` only if
    /// the surface is too large to allocate a mask for.
    fn masked_region(
        &self,
        parent: Option<&tiny_skia::Mask>,
        fill: impl FnOnce(&mut tiny_skia::Mask),
    ) -> Option<tiny_skia::Mask> {
        let mut mask = match parent {
            Some(parent) => parent.clone(),
            None => tiny_skia::Mask::new(self.width, self.height)?,
        };
        fill(&mut mask);
        Some(mask)
    }
}

/// The axis-aligned bounding rectangle of a set of points, or `None` if it
/// has no area.
fn bounding_rect(points: &[tiny_skia::Point]) -> Option<tiny_skia::Rect> {
    let min_x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let max_x = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
    let min_y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_y = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    tiny_skia::Rect::from_ltrb(min_x, min_y, max_x, max_y)
}

#[cfg(test)]
mod tests {
    use super::DrawState;

    fn rect_path(x: f32, y: f32, w: f32, h: f32) -> tiny_skia::Path {
        tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(x, y, w, h).expect("valid rect"),
        )
    }

    #[test]
    fn an_unpushed_state_draws_untransformed_and_unconfined() {
        let state = DrawState::new(64, 64);
        assert!(state.transform().is_identity());
        assert!(state.clip_bounds().is_none());
        assert!(state.clip_mask().is_none());
        assert!(state.is_visible(10, 10));
    }

    #[test]
    fn a_transform_maps_the_points_drawn_under_it() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(tiny_skia::Transform::from_translate(10.0, 5.0));
        assert_eq!(state.map_point(1.0, 2.0), (11.0, 7.0));
    }

    #[test]
    fn a_nested_transform_applies_before_the_one_it_nests_in() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(tiny_skia::Transform::from_translate(10.0, 0.0));
        state.push_transform(tiny_skia::Transform::from_scale(2.0, 2.0));
        // Scaled first, then shifted — not shifted then scaled, which would
        // put this at 24.
        assert_eq!(state.map_point(2.0, 0.0), (14.0, 0.0));
    }

    #[test]
    fn popping_a_transform_restores_the_one_beneath_it() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(tiny_skia::Transform::from_translate(10.0, 0.0));
        state.push_transform(tiny_skia::Transform::from_translate(100.0, 0.0));
        state.pop_transform();
        assert_eq!(state.map_point(0.0, 0.0), (10.0, 0.0));
    }

    #[test]
    fn popping_more_than_was_pushed_leaves_the_default_state() {
        let mut state = DrawState::new(64, 64);
        state.pop_transform();
        state.pop_clip();
        assert!(state.transform().is_identity());
        assert!(state.clip_bounds().is_none());
    }

    #[test]
    fn an_axis_aligned_rect_clip_carries_no_mask() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(tiny_skia::Transform::from_translate(8.0, 8.0));
        state.push_clip_rect(2.0, 2.0, 20.0, 20.0);
        assert!(
            state.clip_mask().is_none(),
            "a plain rectangle needs no mask"
        );
        let bounds = state.clip_bounds().expect("a confined region");
        assert_eq!(
            (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
            (10, 10, 30, 30)
        );
    }

    #[test]
    fn a_rect_clip_confines_drawing_to_its_own_region() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(10.0, 10.0, 20.0, 20.0);
        assert!(state.is_visible(15, 15));
        assert!(!state.is_visible(5, 5));
        assert!(!state.is_visible(35, 15));
    }

    #[test]
    fn a_shape_wholly_inside_a_rect_clip_is_handed_no_mask() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(10.0, 10.0, 40.0, 40.0);
        let inside = tiny_skia::Rect::from_xywh(15.0, 15.0, 10.0, 10.0).unwrap();
        assert!(state.clip_for(Some(inside)).is_none());
        // It stayed a bare rectangle.
        assert!(state.clip_mask().is_none());
    }

    #[test]
    fn a_shape_that_spills_out_of_a_rect_clip_materialises_a_mask() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(10.0, 10.0, 20.0, 20.0);
        let straddling = tiny_skia::Rect::from_xywh(20.0, 20.0, 30.0, 30.0).unwrap();
        assert!(state.clip_for(Some(straddling)).is_some());
        // A pixel outside the clip box is masked off, one inside is kept.
        assert!(state.is_visible(15, 15));
        assert!(!state.is_visible(40, 40));
    }

    #[test]
    fn a_nested_rect_clip_narrows_to_the_overlap() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(0.0, 0.0, 20.0, 20.0);
        state.push_clip_rect(10.0, 10.0, 20.0, 20.0);
        assert!(state.is_visible(15, 15), "inside both");
        assert!(!state.is_visible(5, 5), "inside the outer clip only");
        assert!(!state.is_visible(25, 25), "inside the inner clip only");
    }

    #[test]
    fn a_nested_rect_clip_cannot_widen_the_one_it_nests_in() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(0.0, 0.0, 10.0, 10.0);
        state.push_clip_rect(0.0, 0.0, 64.0, 64.0);
        assert!(!state.is_visible(20, 20));
    }

    #[test]
    fn a_rect_clip_under_rotation_confines_to_the_turned_rectangle() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(
            tiny_skia::Transform::from_translate(32.0, 32.0)
                .pre_concat(tiny_skia::Transform::from_rotate(45.0)),
        );
        state.push_clip_rect(-10.0, -10.0, 20.0, 20.0);
        assert!(
            state.clip_mask().is_some(),
            "a turned rectangle needs a mask"
        );
        // The centre is in; a corner of the axis-aligned bounding box is out.
        assert!(state.is_visible(32, 32));
        assert!(!state.is_visible(19, 19));
    }

    #[test]
    fn a_path_clip_narrows_to_the_path_and_cannot_widen_it() {
        let mut state = DrawState::new(64, 64);
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 20.0, 20.0)),
            tiny_skia::FillRule::Winding,
        );
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 64.0, 64.0)),
            tiny_skia::FillRule::Winding,
        );
        assert!(state.is_visible(10, 10));
        assert!(!state.is_visible(30, 30));
    }

    #[test]
    fn popping_a_clip_restores_the_region_beneath_it() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(0.0, 0.0, 20.0, 20.0);
        state.push_clip_rect(0.0, 0.0, 5.0, 5.0);
        assert!(!state.is_visible(10, 10));
        state.pop_clip();
        assert!(state.is_visible(10, 10));
    }

    #[test]
    fn a_clip_is_taken_in_the_coordinates_current_when_it_is_pushed() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(tiny_skia::Transform::from_translate(20.0, 20.0));
        state.push_clip_rect(0.0, 0.0, 10.0, 10.0);
        assert!(
            state.is_visible(25, 25),
            "the clip moved with the transform"
        );
        assert!(!state.is_visible(5, 5));
    }

    #[test]
    fn an_empty_clip_region_confines_drawing_to_nothing() {
        let mut state = DrawState::new(64, 64);
        state.push_clip(None, tiny_skia::FillRule::Winding);
        assert!(!state.is_visible(10, 10));
    }

    #[test]
    fn a_non_positive_rect_clip_confines_drawing_to_nothing() {
        let mut state = DrawState::new(64, 64);
        state.push_clip_rect(10.0, 10.0, 0.0, 20.0);
        assert!(!state.is_visible(10, 10));
    }
}
