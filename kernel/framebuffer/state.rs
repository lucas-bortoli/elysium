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
    /// An empty box — drawing confined to it is confined to nothing.
    const NOTHING: ClipRect = ClipRect {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };

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

impl ClipRegion {
    /// A region that confines drawing to nothing.
    fn nothing() -> ClipRegion {
        ClipRegion {
            bounds: ClipRect::NOTHING,
            mask: None,
        }
    }
}

/// The clip in effect, for a caller that enforces it itself rather than
/// through tiny-skia: `bounds` is the box drawing is confined to, and `mask`
/// the coverage within it when the region isn't a bare rectangle.
#[derive(Clone, Copy)]
pub struct Clip<'a> {
    pub bounds: Option<ClipRect>,
    pub mask: Option<&'a tiny_skia::Mask>,
}

/// The drawing state in effect at one point in a frame's command list.
pub struct DrawState {
    /// Each entry already composed with everything below it, so the last is
    /// the transform in effect. Empty means no transform.
    transforms: Vec<tiny_skia::Transform>,
    /// The number of transforms at the bottom of the stack that belong to
    /// the renderer rather than the program — the band offset a parallel
    /// rasterize seeds (see [`DrawState::with_base_transform`]). `pop_transform`
    /// will not pop below this, so an unbalanced program cannot shift its
    /// band's drawing off its rows.
    base_transforms: usize,
    /// Each entry already narrowed by everything below it, so the last is the
    /// region drawing is confined to. Empty means unconfined.
    clips: Vec<ClipRegion>,
    width: u32,
    height: u32,
}

impl DrawState {
    /// A state with nothing pushed, for a surface `width` x `height` — the
    /// size every clip region is rasterized at. The renderer always seeds a
    /// band offset through [`DrawState::with_base_transform`]; this bare
    /// constructor is used by the state tests.
    #[allow(dead_code)]
    pub fn new(width: u32, height: u32) -> DrawState {
        DrawState {
            transforms: Vec::new(),
            base_transforms: 0,
            clips: Vec::new(),
            width,
            height,
        }
    }

    /// A state seeded with `base` already in effect, for rasterizing one
    /// horizontal band of a larger surface. `base` shifts surface-space
    /// coordinates up into the band's own rows; `width` x `height` is the
    /// band's own size, so clip regions are allocated at band height rather
    /// than full-surface height. The seeded transform is the renderer's, not
    /// the program's: `pop_transform` treats it as a floor and will not pop
    /// it away.
    pub fn with_base_transform(width: u32, height: u32, base: tiny_skia::Transform) -> DrawState {
        DrawState {
            transforms: vec![base],
            base_transforms: 1,
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

    /// The clip in effect, for a caller that confines its own drawing.
    pub fn clip(&self) -> Clip<'_> {
        Clip {
            bounds: self.clip_bounds(),
            mask: self.clip_mask(),
        }
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
        if self.transforms.len() > self.base_transforms {
            self.transforms.pop();
        }
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
    /// coordinates currently in effect. An upright rectangle not nested in a
    /// mask stays a bare box; rotation, shear, or a masked parent turns it
    /// into a masked region.
    pub fn push_clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let transform = self.transform();
        let Some(local) = tiny_skia::Rect::from_xywh(x, y, w, h) else {
            self.clips.push(ClipRegion::nothing());
            return;
        };
        let Some(bounds) = self.narrowed_bounds(local, transform) else {
            self.clips.push(ClipRegion::nothing());
            return;
        };

        let upright = transform.kx == 0.0 && transform.ky == 0.0;
        if upright && self.clips.last().is_none_or(|region| region.mask.is_none()) {
            self.clips.push(ClipRegion { bounds, mask: None });
            return;
        }
        self.narrow_to_path(
            bounds,
            &tiny_skia::PathBuilder::from_rect(local),
            tiny_skia::FillRule::Winding,
            transform,
        );
    }

    /// Narrows the clip to the inside of `path` (in the current coordinates),
    /// or to nothing when there is no finishable path. Always a masked
    /// region — an arbitrary shape is never just a box.
    pub fn push_clip(&mut self, path: Option<&tiny_skia::Path>, rule: tiny_skia::FillRule) {
        let transform = self.transform();
        let Some(path) = path else {
            self.clips.push(ClipRegion::nothing());
            return;
        };
        let Some(bounds) = self.narrowed_bounds(path.bounds(), transform) else {
            self.clips.push(ClipRegion::nothing());
            return;
        };
        self.narrow_to_path(bounds, path, rule, transform);
    }

    pub fn pop_clip(&mut self) {
        self.clips.pop();
    }

    /// The clip box for a local-space rectangle placed by `transform`: its
    /// bounding box mapped onto the surface, then narrowed to the parent
    /// clip. `None` if there is no rectangle, or it maps to no area.
    fn narrowed_bounds(
        &self,
        local: tiny_skia::Rect,
        transform: tiny_skia::Transform,
    ) -> Option<ClipRect> {
        let mapped = super::mapped_bounds(local, transform)?;
        Some(ClipRect::covering(mapped, self.surface_bounds()).intersect(self.parent_bounds()))
    }

    /// Pushes a masked clip: `bounds` as its box and `path` — filled with
    /// `rule` under `transform` — as its coverage, intersected with the
    /// parent mask when the clip nests inside one. Confines drawing to
    /// nothing if no mask can be allocated.
    fn narrow_to_path(
        &mut self,
        bounds: ClipRect,
        path: &tiny_skia::Path,
        rule: tiny_skia::FillRule,
        transform: tiny_skia::Transform,
    ) {
        let mask = match self.clips.last().and_then(|region| region.mask.as_ref()) {
            Some(parent) => {
                let mut mask = parent.clone();
                mask.intersect_path(path, rule, false, transform);
                mask
            }
            None => {
                let Some(mut mask) = tiny_skia::Mask::new(self.width, self.height) else {
                    self.clips.push(ClipRegion::nothing());
                    return;
                };
                mask.fill_path(path, rule, false, transform);
                mask
            }
        };
        self.clips.push(ClipRegion {
            bounds,
            mask: Some(mask),
        });
    }
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

    #[test]
    fn a_base_transform_shifts_a_bands_drawing_into_its_own_rows() {
        // A band whose top row is row 40 of the surface: a program drawing at
        // surface y=45 lands at band-local y=5.
        let state = DrawState::with_base_transform(
            64,
            24,
            tiny_skia::Transform::from_translate(0.0, -40.0),
        );
        assert_eq!(state.map_point(10.0, 45.0), (10.0, 5.0));
    }

    #[test]
    fn an_unbalanced_pop_cannot_remove_a_bands_base_transform() {
        let mut state = DrawState::with_base_transform(
            64,
            24,
            tiny_skia::Transform::from_translate(0.0, -40.0),
        );
        state.push_transform(tiny_skia::Transform::from_translate(3.0, 0.0));
        // One legitimate pop, then three the program never earned.
        for _ in 0..4 {
            state.pop_transform();
        }
        assert_eq!(state.map_point(10.0, 45.0), (10.0, 5.0));
    }

    #[test]
    fn a_clip_under_a_base_transform_is_sized_to_the_band() {
        let mut state = DrawState::with_base_transform(
            64,
            24,
            tiny_skia::Transform::from_translate(0.0, -40.0),
        );
        // A clip a program places at surface rows 44..54 confines to band
        // rows 4..14.
        state.push_clip_rect(0.0, 44.0, 64.0, 10.0);
        assert!(state.is_visible(10, 8));
        assert!(!state.is_visible(10, 20));
    }
}
