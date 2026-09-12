//! `ely:framebuffer`'s drawing surface: the draw-handler gate every drawing
//! call sits behind, paths and shapes, the transform and clip stacks, and
//! individual pixels. Text lives in `text.rs`, and what a frame's commands
//! actually rasterize to is tested against a bare pixmap in
//! `kernel/framebuffer.rs`.

use super::*;

#[test]
fn draw_calls_outside_a_handler_throw_draw_outside_handler_error() {
    let runtime = eval(
        "import { clearScreen, DrawOutsideHandlerError } from 'ely:framebuffer'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             clearScreen(0); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof DrawOutsideHandlerError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn draw_calls_inside_a_registered_handler_succeed() {
    let runtime = eval(
        "import { clearScreen, addDrawHandler, Color } from 'ely:framebuffer'; \
         globalThis.drawn = false; \
         addDrawHandler(() => { \
             clearScreen(Color.Slate900); \
             globalThis.drawn = true; \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "drawn"));
}

#[test]
fn path_calls_outside_a_handler_throw_draw_outside_handler_error() {
    let runtime = eval(
        "import { beginPath, moveTo, fillPath, pushClip, popClip, pushTransform, \
                   DrawOutsideHandlerError } from 'ely:framebuffer'; \
         globalThis.threw = []; \
         for (const call of [ \
             () => beginPath(), \
             () => moveTo(0, 0), \
             () => fillPath(0), \
             () => pushClip(0, 0, 1, 1), \
             () => popClip(), \
             () => pushTransform({}), \
         ]) { \
             try { call(); globalThis.threw.push(false); } \
             catch (err) { globalThis.threw.push(err instanceof DrawOutsideHandlerError); } \
         }",
    );
    let threw = global::<Vec<bool>>(&runtime, "threw");
    assert_eq!(threw.len(), 6);
    assert!(
        threw.iter().all(|&t| t),
        "every path call should have thrown"
    );
}

#[test]
fn describing_filling_and_stroking_a_path_inside_a_handler_succeeds() {
    let runtime = eval(
        "import { addDrawHandler, beginPath, moveTo, lineTo, quadraticTo, cubicTo, \
                   closePath, fillPath, strokePath, Color } from 'ely:framebuffer'; \
         globalThis.drawn = false; \
         addDrawHandler(() => { \
             beginPath(); \
             moveTo(10, 10); \
             lineTo(50, 10); \
             quadraticTo(60, 20, 50, 30); \
             cubicTo(40, 40, 20, 40, 10, 30); \
             closePath(); \
             fillPath(Color.Amber400, 'evenodd'); \
             strokePath(Color.Teal300, 2, 'round', 'bevel'); \
             globalThis.drawn = true; \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "drawn"));
}

#[test]
fn an_unknown_fill_rule_throws() {
    let runtime = eval(
        "import { addDrawHandler, beginPath, moveTo, lineTo, fillPath } from 'ely:framebuffer'; \
         globalThis.threw = false; \
         addDrawHandler(() => { \
             beginPath(); moveTo(0, 0); lineTo(10, 10); \
             try { fillPath(0, 'inside-ish'); } \
             catch (err) { globalThis.threw = err instanceof TypeError; } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn an_unknown_line_cap_or_join_throws() {
    let runtime = eval(
        "import { addDrawHandler, beginPath, moveTo, lineTo, strokePath } from 'ely:framebuffer'; \
         globalThis.threw = []; \
         addDrawHandler(() => { \
             beginPath(); moveTo(0, 0); lineTo(10, 10); \
             for (const [cap, join] of [['flat', 'miter'], ['butt', 'rounded']]) { \
                 try { strokePath(0, 1, cap, join); globalThis.threw.push(false); } \
                 catch (err) { globalThis.threw.push(err instanceof TypeError); } \
             } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<Vec<bool>>(&runtime, "threw"), vec![true, true]);
}

#[test]
fn a_stroke_thickness_of_zero_or_less_throws() {
    let runtime = eval(
        "import { addDrawHandler, beginPath, moveTo, lineTo, strokePath } from 'ely:framebuffer'; \
         globalThis.threw = []; \
         addDrawHandler(() => { \
             beginPath(); moveTo(0, 0); lineTo(10, 10); \
             for (const thickness of [0, -1]) { \
                 try { strokePath(0, thickness); globalThis.threw.push(false); } \
                 catch (err) { globalThis.threw.push(err instanceof RangeError); } \
             } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<Vec<bool>>(&runtime, "threw"), vec![true, true]);
}

#[test]
fn pushing_and_popping_transforms_and_clips_inside_a_handler_succeeds() {
    let runtime = eval(
        "import { addDrawHandler, pushTransform, popTransform, pushClip, popClip, \
                   pushClipPath, beginPath, moveTo, lineTo, fillRectangle, Color } \
             from 'ely:framebuffer'; \
         globalThis.drawn = false; \
         addDrawHandler(() => { \
             pushTransform({ translate: { x: 10, y: 5 }, scale: 2, rotate: 0.5 }); \
             pushClip(0, 0, 100, 100); \
             beginPath(); moveTo(0, 0); lineTo(10, 0); lineTo(10, 10); \
             pushClipPath('evenodd'); \
             fillRectangle(0, 0, 20, 20, Color.Amber400); \
             popClip(); \
             popClip(); \
             popTransform(); \
             globalThis.drawn = true; \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "drawn"));
}

#[test]
fn popping_more_than_was_pushed_throws_unbalanced_stack_error() {
    let runtime = eval(
        "import { addDrawHandler, pushClip, popClip, popTransform, \
                   UnbalancedStackError } from 'ely:framebuffer'; \
         globalThis.threw = []; \
         addDrawHandler(() => { \
             pushClip(0, 0, 10, 10); \
             popClip(); \
             for (const call of [() => popClip(), () => popTransform()]) { \
                 try { call(); globalThis.threw.push(false); } \
                 catch (err) { globalThis.threw.push(err instanceof UnbalancedStackError); } \
             } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<Vec<bool>>(&runtime, "threw"), vec![true, true]);
}

#[test]
fn a_handler_that_leaves_its_stacks_unbalanced_starts_the_next_frame_clean() {
    // A draw handler that throws part way through never reaches its pops,
    // and the frame after it must not inherit that debt.
    let runtime = eval(
        "import { addDrawHandler, pushClip, popClip, UnbalancedStackError } \
             from 'ely:framebuffer'; \
         globalThis.frames = 0; \
         globalThis.threw = false; \
         addDrawHandler(() => { \
             globalThis.frames++; \
             if (globalThis.frames === 1) { pushClip(0, 0, 10, 10); return; } \
             try { popClip(); } \
             catch (err) { globalThis.threw = err instanceof UnbalancedStackError; } \
         });",
    );
    runtime.run_due_timers().unwrap();
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<u32>(&runtime, "frames"), 2);
    assert!(
        global::<bool>(&runtime, "threw"),
        "the second frame's clip stack should have started empty"
    );
}

#[test]
fn every_shape_call_draws_inside_a_handler() {
    // Each of these describes a whole path and hands it to the same fill or
    // stroke the raw path calls use, so a wrong argument reaches the kernel
    // as a bad fill rule, cap or join and throws.
    let runtime = eval(
        "import { addDrawHandler, Color, strokeRectangle, fillRoundedRectangle, \
                   strokeRoundedRectangle, drawLine, drawPolyline, fillCircle, \
                   strokeCircle, fillEllipse, strokeEllipse, drawArc, fillTriangle, \
                   fillPolygon, strokePolygon } from 'ely:framebuffer'; \
         globalThis.error = ''; \
         const square = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }]; \
         addDrawHandler(() => { \
             try { \
                 strokeRectangle(1, 1, 20, 10, Color.Amber400); \
                 fillRoundedRectangle(1, 1, 20, 10, 3, Color.Amber400); \
                 strokeRoundedRectangle(1, 1, 20, 10, 3, Color.Amber400, 2); \
                 drawLine(0, 0, 30, 30, Color.Teal300, 2); \
                 drawPolyline(square, Color.Teal300, 2); \
                 fillCircle(20, 20, 8, Color.Rose500); \
                 strokeCircle(20, 20, 8, Color.Rose500, 2); \
                 fillEllipse(20, 20, 10, 5, Color.Rose500); \
                 strokeEllipse(20, 20, 10, 5, Color.Rose500, 2); \
                 drawArc(20, 20, 10, 0, 1.5, Color.Amber400, 3); \
                 fillTriangle(square[0], square[1], square[2], Color.Teal300); \
                 fillPolygon(square, Color.Teal300, 'evenodd'); \
                 strokePolygon(square, Color.Teal300, 2); \
                 globalThis.error = 'none'; \
             } catch (err) { globalThis.error = String(err); } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn a_polygon_or_polyline_too_short_to_enclose_anything_draws_nothing() {
    let runtime = eval(
        "import { addDrawHandler, Color, drawPolyline, fillPolygon, strokePolygon } \
             from 'ely:framebuffer'; \
         globalThis.error = ''; \
         addDrawHandler(() => { \
             try { \
                 drawPolyline([], Color.Amber400); \
                 drawPolyline([{ x: 1, y: 1 }], Color.Amber400); \
                 fillPolygon([{ x: 1, y: 1 }, { x: 2, y: 2 }], Color.Amber400); \
                 strokePolygon([], Color.Amber400); \
                 globalThis.error = 'none'; \
             } catch (err) { globalThis.error = String(err); } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn shape_calls_outside_a_handler_throw_draw_outside_handler_error() {
    let runtime = eval(
        "import { fillCircle, strokeRectangle, drawArc, fillPolygon, \
                   DrawOutsideHandlerError } from 'ely:framebuffer'; \
         globalThis.threw = []; \
         for (const call of [ \
             () => fillCircle(0, 0, 5, 0), \
             () => strokeRectangle(0, 0, 5, 5, 0), \
             () => drawArc(0, 0, 5, 0, 1, 0), \
             () => fillPolygon([{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }], 0), \
         ]) { \
             try { call(); globalThis.threw.push(false); } \
             catch (err) { globalThis.threw.push(err instanceof DrawOutsideHandlerError); } \
         }",
    );
    let threw = global::<Vec<bool>>(&runtime, "threw");
    assert_eq!(threw.len(), 4);
    assert!(threw.iter().all(|&t| t));
}

#[test]
fn setting_pixels_inside_a_handler_succeeds() {
    let runtime = eval(
        "import { addDrawHandler, setPixel, drawPixels, Color } from 'ely:framebuffer'; \
         globalThis.error = ''; \
         addDrawHandler(() => { \
             try { \
                 setPixel(3, 4, Color.Amber400); \
                 drawPixels([{ x: 1, y: 1 }, { x: 2, y: 2 }], Color.Teal300); \
                 drawPixels([], Color.Teal300); \
                 globalThis.error = 'none'; \
             } catch (err) { globalThis.error = String(err); } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn setting_a_pixel_to_an_unknown_color_throws() {
    let runtime = eval(
        "import { addDrawHandler, setPixel } from 'ely:framebuffer'; \
         globalThis.threw = false; \
         addDrawHandler(() => { \
             try { setPixel(1, 1, 60000); } \
             catch (err) { globalThis.threw = err instanceof TypeError; } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn a_handler_that_leaves_a_clip_pushed_does_not_confine_the_next_one() {
    // Every running program's handlers draw into one frame in turn, so a
    // clip one of them leaves open would go on confining drawing that isn't
    // its own.
    let runtime = eval(
        "import { addDrawHandler, pushClip, popClip, UnbalancedStackError } \
             from 'ely:framebuffer'; \
         globalThis.leakedIn = false; \
         addDrawHandler(() => { pushClip(0, 0, 10, 10); }); \
         addDrawHandler(() => { \
             try { popClip(); } \
             catch (err) { globalThis.leakedIn = !(err instanceof UnbalancedStackError); } \
         });",
    );
    runtime.run_due_timers().unwrap();
    assert!(
        !global::<bool>(&runtime, "leakedIn"),
        "the second handler should have started with an empty clip stack"
    );
}
