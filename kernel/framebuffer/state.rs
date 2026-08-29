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

/// The drawing state in effect at one point in a frame's command list.
pub struct DrawState {
    /// Each entry already composed with everything below it, so the last is
    /// the transform in effect. Empty means no transform.
    transforms: Vec<tiny_skia::Transform>,
    /// Each entry already intersected with everything below it, so the last
    /// is the region drawing is confined to. Empty means unconfined.
    clips: Vec<tiny_skia::Mask>,
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

    /// The region drawing is confined to, or `None` while unconfined.
    pub fn clip(&self) -> Option<&tiny_skia::Mask> {
        self.clips.last()
    }

    /// Whether the surface pixel at `(x, y)` is inside the current clip.
    /// Used by the drawing that writes pixels directly instead of going
    /// through the rasterizer, which applies the clip itself.
    pub fn is_visible(&self, x: i32, y: i32) -> bool {
        let Some(mask) = self.clip() else {
            return true;
        };
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }
        mask.data()[(y * self.width as i32 + x) as usize] != 0
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

    /// Narrows the region drawing is confined to by `path`, taken in the
    /// coordinates a program is currently drawing with. `None` confines
    /// drawing to nothing, which is what an empty region means.
    pub fn push_clip(&mut self, path: Option<&tiny_skia::Path>, rule: tiny_skia::FillRule) {
        let transform = self.transform();
        let mut mask = match self.clips.last() {
            Some(parent) => parent.clone(),
            // A fresh mask starts empty, so the first clip is filled in
            // rather than intersected — intersecting against nothing would
            // leave nothing.
            None => {
                let mut mask = match tiny_skia::Mask::new(self.width, self.height) {
                    Some(mask) => mask,
                    None => return,
                };
                if let Some(path) = path {
                    // anti_alias: false — a mask byte is 0 or 255 and never
                    // in between, so a clip edge can't blend a palette color
                    // into something off-palette.
                    mask.fill_path(path, rule, false, transform);
                }
                self.clips.push(mask);
                return;
            }
        };
        match path {
            Some(path) => mask.intersect_path(path, rule, false, transform),
            None => mask.clear(),
        }
        self.clips.push(mask);
    }

    pub fn pop_clip(&mut self) {
        self.clips.pop();
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
        assert!(state.clip().is_none());
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
        assert!(state.clip().is_none());
    }

    #[test]
    fn a_clip_confines_drawing_to_its_own_region() {
        let mut state = DrawState::new(64, 64);
        state.push_clip(
            Some(&rect_path(10.0, 10.0, 20.0, 20.0)),
            tiny_skia::FillRule::Winding,
        );
        assert!(state.is_visible(15, 15));
        assert!(!state.is_visible(5, 5));
        assert!(!state.is_visible(35, 15));
    }

    #[test]
    fn a_nested_clip_narrows_to_the_overlap() {
        let mut state = DrawState::new(64, 64);
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 20.0, 20.0)),
            tiny_skia::FillRule::Winding,
        );
        state.push_clip(
            Some(&rect_path(10.0, 10.0, 20.0, 20.0)),
            tiny_skia::FillRule::Winding,
        );
        assert!(state.is_visible(15, 15), "inside both");
        assert!(!state.is_visible(5, 5), "inside the outer clip only");
        assert!(!state.is_visible(25, 25), "inside the inner clip only");
    }

    #[test]
    fn a_nested_clip_cannot_widen_the_one_it_nests_in() {
        let mut state = DrawState::new(64, 64);
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 10.0, 10.0)),
            tiny_skia::FillRule::Winding,
        );
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 64.0, 64.0)),
            tiny_skia::FillRule::Winding,
        );
        assert!(!state.is_visible(20, 20));
    }

    #[test]
    fn popping_a_clip_restores_the_region_beneath_it() {
        let mut state = DrawState::new(64, 64);
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 20.0, 20.0)),
            tiny_skia::FillRule::Winding,
        );
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 5.0, 5.0)),
            tiny_skia::FillRule::Winding,
        );
        assert!(!state.is_visible(10, 10));
        state.pop_clip();
        assert!(state.is_visible(10, 10));
    }

    #[test]
    fn a_clip_is_taken_in_the_coordinates_current_when_it_is_pushed() {
        let mut state = DrawState::new(64, 64);
        state.push_transform(tiny_skia::Transform::from_translate(20.0, 20.0));
        state.push_clip(
            Some(&rect_path(0.0, 0.0, 10.0, 10.0)),
            tiny_skia::FillRule::Winding,
        );
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
}
