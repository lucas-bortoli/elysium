//! `ely:graphics`'s text: measuring a string against a built-in font, and
//! drawing one onto a surface. The layout maths themselves are tested in
//! `kernel/text.rs`.

use super::*;

#[test]
fn measure_text_reports_one_line_as_the_fonts_own_line_height() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         const plain = screen.measureText('Hi'); \
         globalThis.width = plain.width; \
         globalThis.height = plain.height; \
         globalThis.scaledWidth = screen.measureText('Hi', { scale: 3 }).width; \
         globalThis.scaledHeight = screen.measureText('Hi', { scale: 3 }).height;",
    );
    let width = global::<u32>(&runtime, "width");
    let height = global::<u32>(&runtime, "height");
    assert!(width > 0 && height > 0);
    assert_eq!(global::<u32>(&runtime, "scaledWidth"), width * 3);
    assert_eq!(global::<u32>(&runtime, "scaledHeight"), height * 3);
}

#[test]
fn measure_text_counts_every_line_a_string_wraps_or_breaks_into() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         const one = screen.measureText('a').height; \
         globalThis.one = one; \
         globalThis.broken = screen.measureText('a\\nb\\nc').height; \
         const wide = screen.measureText('aaa aaa aaa').width; \
         globalThis.wrapped = screen.measureText('aaa aaa aaa', { maxWidth: wide / 2 }).height; \
         globalThis.wrappedFitsWidth = \
             screen.measureText('aaa aaa aaa', { maxWidth: wide / 2 }).width <= wide / 2;",
    );
    let one = global::<u32>(&runtime, "one");
    assert_eq!(global::<u32>(&runtime, "broken"), one * 3);
    assert!(
        global::<u32>(&runtime, "wrapped") > one,
        "should have wrapped"
    );
    assert!(global::<bool>(&runtime, "wrappedFitsWidth"));
}

#[test]
fn drawing_text_with_options_succeeds() {
    let runtime = eval(
        "import { screen, Color, Font } from 'ely:graphics'; \
         globalThis.error = ''; \
         try { \
             screen.drawText(10, 10, 'plain', Color.Amber400); \
             screen.drawText(10, 20, 'a font', Color.Amber400, Font.Cozette); \
             screen.drawText(60, 30, 'centred', Color.Amber400, { align: 'center' }); \
             screen.drawText(60, 40, 'right', Color.Amber400, { align: 'right' }); \
             screen.drawText(10, 50, 'big', Color.Amber400, { scale: 3 }); \
             screen.drawText(10, 70, 'wrap me around', Color.Amber400, \
                      { maxWidth: 40, lineSpacing: 1.5 }); \
             globalThis.error = 'none'; \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

#[test]
fn a_text_scale_that_is_not_a_whole_number_of_at_least_one_throws() {
    let runtime = eval(
        "import { screen } from 'ely:graphics'; \
         globalThis.threw = []; \
         for (const scale of [0, -1, 1.5]) { \
             try { screen.measureText('hi', { scale }); globalThis.threw.push(false); } \
             catch (err) { globalThis.threw.push(err instanceof RangeError); } \
         }",
    );
    assert_eq!(
        global::<Vec<bool>>(&runtime, "threw"),
        vec![true, true, true]
    );
}
