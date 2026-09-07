//! `ely:graphics` as a program sees it: the `Surface` object, its handle
//! lifecycle, the raw path API, the shape vocabulary, the transform and clip
//! stacks, and individual pixels. Text lives in `text.rs`; what an operation
//! actually rasterizes to is tested against a bare surface in
//! `kernel/graphics/surface.rs` and `shapes.rs`.

use super::*;

#[test]
fn a_created_surface_can_be_revived_drawn_to_and_destroyed() {
    let runtime = eval(
        "import { createSurface, useSurface, destroySurface, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             const handle = createSurface(64, 48); \
             const s = useSurface(handle); \
             globalThis.size = [s.width, s.height]; \
             s.clear(Color.Slate900); \
             s.fillRectangle(0, 0, 10, 10, Color.Amber400); \
             s.resize(80, 80); \
             globalThis.resized = [s.width, s.height]; \
             destroySurface(handle); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
    assert_eq!(global::<Vec<u32>>(&runtime, "size"), vec![64, 48]);
    assert_eq!(global::<Vec<u32>>(&runtime, "resized"), vec![80, 80]);
}

#[test]
fn reviving_a_handle_that_names_no_surface_throws() {
    let runtime = eval(
        "import { useSurface } from 'ely:graphics'; \
         globalThis.threw = false; \
         try { useSurface(9999); } \
         catch (err) { globalThis.threw = err instanceof TypeError; }",
    );
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn the_screen_is_a_surface_drawable_from_anywhere() {
    // No draw-handler gate: a program draws whenever it holds a surface.
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.clear(Color.Slate900); \
             screen.fillCircle(20, 20, 8, Color.Amber400); \
             globalThis.handle = screen.handle; \
             globalThis.size = [screen.width, screen.height]; \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
    assert_eq!(global::<u32>(&runtime, "handle"), 0);
    assert_eq!(global::<Vec<u32>>(&runtime, "size"), vec![720, 360]);
}

#[test]
fn drawing_a_surface_onto_itself_throws() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         globalThis.threw = false; \
         try { screen.drawSurface(screen.handle, 0, 0); } \
         catch (err) { globalThis.threw = err instanceof TypeError; }",
    );
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn one_surface_can_be_drawn_onto_another() {
    let runtime = eval(
        "import { createSurface, useSurface, screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             const badge = useSurface(createSurface(16, 16)); \
             badge.clear(Color.Amber400); \
             screen.drawSurface(badge.handle, 4, 4); \
             screen.drawSurfaceRotated(badge.handle, 40, 40, 0.5, { scale: 2 }); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn describing_filling_and_stroking_a_path_succeeds() {
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.beginPath(); \
             screen.moveTo(10, 10); \
             screen.lineTo(50, 10); \
             screen.quadraticTo(60, 20, 50, 30); \
             screen.cubicTo(40, 40, 20, 40, 10, 30); \
             screen.closePath(); \
             screen.fillPath(Color.Amber400, 'evenodd'); \
             screen.strokePath(Color.Teal300, 2, 'round', 'bevel'); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn an_unknown_fill_rule_throws() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         globalThis.threw = false; \
         screen.beginPath(); screen.moveTo(0, 0); screen.lineTo(10, 10); \
         try { screen.fillPath(0, 'inside-ish'); } \
         catch (err) { globalThis.threw = err instanceof TypeError; }",
    );
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn an_unknown_line_cap_or_join_throws() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         globalThis.threw = []; \
         screen.beginPath(); screen.moveTo(0, 0); screen.lineTo(10, 10); \
         for (const [cap, join] of [['flat', 'miter'], ['butt', 'rounded']]) { \
             try { screen.strokePath(0, 1, cap, join); globalThis.threw.push(false); } \
             catch (err) { globalThis.threw.push(err instanceof TypeError); } \
         }",
    );
    assert_eq!(global::<Vec<bool>>(&runtime, "threw"), vec![true, true]);
}

#[test]
fn a_stroke_thickness_of_zero_or_less_throws() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         globalThis.threw = []; \
         screen.beginPath(); screen.moveTo(0, 0); screen.lineTo(10, 10); \
         for (const thickness of [0, -1]) { \
             try { screen.strokePath(0, thickness); globalThis.threw.push(false); } \
             catch (err) { globalThis.threw.push(err instanceof RangeError); } \
         }",
    );
    assert_eq!(global::<Vec<bool>>(&runtime, "threw"), vec![true, true]);
}

#[test]
fn pushing_and_popping_transforms_and_clips_succeeds() {
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.pushTransform({ translate: { x: 10, y: 5 }, scale: 2, rotate: 0.5 }); \
             screen.pushClip(0, 0, 100, 100); \
             screen.beginPath(); screen.moveTo(0, 0); screen.lineTo(10, 0); screen.lineTo(10, 10); \
             screen.pushClipPath('evenodd'); \
             screen.fillRectangle(0, 0, 20, 20, Color.Amber400); \
             screen.popClip(); \
             screen.popClip(); \
             screen.popTransform(); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn popping_more_than_was_pushed_is_a_no_op() {
    // The stacks are the surface's own now; over-popping just leaves it in
    // its default state rather than throwing.
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.popClip(); screen.popTransform(); screen.popClip(); \
             screen.clear(Color.Slate900); \
             screen.fillRectangle(0, 0, 10, 10, Color.Amber400); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn every_shape_call_draws() {
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         const square = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }]; \
         try { \
             screen.strokeRectangle(1, 1, 20, 10, Color.Amber400); \
             screen.fillRoundedRectangle(1, 1, 20, 10, 3, Color.Amber400); \
             screen.strokeRoundedRectangle(1, 1, 20, 10, 3, Color.Amber400, 2); \
             screen.drawLine(0, 0, 30, 30, Color.Teal300, 2); \
             screen.drawPolyline(square, Color.Teal300, 2); \
             screen.fillCircle(20, 20, 8, Color.Rose500); \
             screen.strokeCircle(20, 20, 8, Color.Rose500, 2); \
             screen.fillEllipse(20, 20, 10, 5, Color.Rose500); \
             screen.strokeEllipse(20, 20, 10, 5, Color.Rose500, 2); \
             screen.drawArc(20, 20, 10, 0, 1.5, Color.Amber400, 3); \
             screen.fillTriangle(square[0], square[1], square[2], Color.Teal300); \
             screen.fillPolygon(square, Color.Teal300, 'evenodd'); \
             screen.strokePolygon(square, Color.Teal300, 2); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn a_polygon_or_polyline_too_short_to_enclose_anything_draws_nothing() {
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.drawPolyline([], Color.Amber400); \
             screen.drawPolyline([{ x: 1, y: 1 }], Color.Amber400); \
             screen.fillPolygon([{ x: 1, y: 1 }, { x: 2, y: 2 }], Color.Amber400); \
             screen.strokePolygon([], Color.Amber400); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn setting_pixels_succeeds() {
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.setPixel(3, 4, Color.Amber400); \
             screen.drawPixels([{ x: 1, y: 1 }, { x: 2, y: 2 }], Color.Teal300); \
             screen.drawPixels([], Color.Teal300); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn setting_a_pixel_to_an_unknown_color_throws() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         globalThis.threw = false; \
         try { screen.setPixel(1, 1, 60000); } \
         catch (err) { globalThis.threw = err instanceof TypeError; }",
    );
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn drawing_to_a_resized_screen_uses_its_new_size() {
    let runtime = eval(
        "import { screen, Color } from 'ely:graphics'; \
         screen.resize(200, 120); \
         globalThis.size = [screen.width, screen.height]; \
         screen.clear(Color.Slate900);",
    );
    assert_eq!(global::<Vec<u32>>(&runtime, "size"), vec![200, 120]);
}
