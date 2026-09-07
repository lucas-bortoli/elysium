//! The kernel's built-in bitmap fonts and the text-layout math the
//! drawing surface draws strings with.
//!
//! The font data itself — one `FontData` per built-in font, each carrying
//! its own metrics and a table of `Glyph`s with the BDF's own 1bpp row
//! packing — is generated at build time from BDF sources by `build/fonts.rs`
//! and `include!`d here. Nothing in this file assumes a particular font's
//! size: metrics travel with each `FontData`, so adding a second font is
//! just another entry in the generated `FONTS` slice.

include!(concat!(env!("OUT_DIR"), "/fonts.rs"));

/// A built-in font, identified by its index in the generated `FONTS` slice.
/// This is the value that crosses the `ely:graphics` boundary, mirroring
/// how a `Color` crosses it as a numeric id.
pub type FontId = u16;

/// The font used when a program doesn't name one. The `ely:graphics`
/// module always sends an explicit id, so this is only referenced by tests
/// and by anything that later needs a kernel-side default.
#[allow(dead_code)]
pub const DEFAULT_FONT: FontId = 0;

/// Resolves a font id to its data, or `None` if nothing is registered under
/// that id — mirrors `Color::from_id`.
pub fn font_from_id(id: FontId) -> Option<&'static FontData> {
    FONTS.get(id as usize)
}

impl FontData {
    fn glyph(&self, codepoint: u32) -> Option<&Glyph> {
        self.glyphs
            .binary_search_by_key(&codepoint, |g| g.codepoint)
            .ok()
            .map(|i| &self.glyphs[i])
    }

    fn advance_of(&self, codepoint: u32) -> u32 {
        self.glyph(codepoint)
            .map(|g| g.advance as u32)
            .unwrap_or(self.default_advance as u32)
    }
}

/// The pixel box `text` occupies when drawn with `font`: the summed advance
/// width of its characters, and the font's line height. A codepoint with no
/// glyph still advances the pen by the font's default advance.
pub fn measure(font: &FontData, text: &str) -> (u32, u32) {
    let width = text.chars().map(|c| font.advance_of(c as u32)).sum();
    (width, font.line_height)
}

/// Which edge of the text box a caller's `x` names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// How a string should be laid out — the kernel-side form of
/// `ely:graphics`'s `TextOptions`, with defaults already filled in and the
/// scale already checked to be at least 1.
pub struct TextLayout {
    pub font: FontId,
    pub scale: u32,
    pub align: TextAlign,
    /// Wrap width in surface pixels; `None` leaves lines unwrapped.
    pub max_width: Option<f32>,
    /// Multiplies the gap between lines.
    pub line_spacing: f32,
}

/// One laid-out line and where its top-left corner sits relative to the
/// `(x, y)` the caller passed.
pub struct PlacedLine {
    pub text: String,
    pub dx: f32,
    pub dy: f32,
}

/// The width one line occupies drawn with `layout`.
fn line_width(font: &FontData, layout: &TextLayout, line: &str) -> f32 {
    measure(font, line).0 as f32 * layout.scale as f32
}

/// Greedily packs as many words as fit within `max_width` onto each line. A
/// word wider than `max_width` on its own still gets its own line and
/// overruns it — there is no hyphenation or mid-word breaking.
fn wrap_line(font: &FontData, layout: &TextLayout, line: &str) -> Vec<String> {
    let Some(max_width) = layout.max_width else {
        return vec![line.to_string()];
    };
    let mut wrapped = Vec::new();
    let mut current = String::new();
    for word in line.split(' ') {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if !current.is_empty() && line_width(font, layout, &candidate) > max_width {
            wrapped.push(std::mem::take(&mut current));
            current = word.to_string();
        } else {
            current = candidate;
        }
    }
    wrapped.push(current);
    wrapped
}

/// Lays `text` out under `layout`: the lines it breaks into, each with its
/// offset from the anchor point, plus the whole block's width and height.
/// Shared by `drawText` and `measureText` so the two can't disagree.
pub fn lay_out(font: &FontData, layout: &TextLayout, text: &str) -> (Vec<PlacedLine>, f32, f32) {
    let lines: Vec<String> = text
        .split('\n')
        .flat_map(|line| wrap_line(font, layout, line))
        .collect();
    let widths: Vec<f32> = lines
        .iter()
        .map(|line| line_width(font, layout, line))
        .collect();

    let line_height = font.line_height as f32 * layout.scale as f32;
    let step = line_height * layout.line_spacing;
    let block_width = widths.iter().copied().fold(0.0_f32, f32::max);
    // The last line takes its full height rather than a spaced step, so a
    // single line measures exactly the font's line height whatever
    // `line_spacing` says.
    let block_height = (lines.len().saturating_sub(1)) as f32 * step + line_height;

    let placed = lines
        .into_iter()
        .zip(widths)
        .enumerate()
        .map(|(i, (text, width))| {
            let dx = match layout.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => -width / 2.0,
                TextAlign::Right => -width,
            };
            PlacedLine {
                text,
                dx,
                dy: step * i as f32,
            }
        })
        .collect();

    (placed, block_width, block_height)
}

/// Calls `f(x, y)` once for every lit pixel of `text` drawn with `font`,
/// with `(origin_x, origin_y)` the top-left of the text box. Keeps the
/// baseline placement and bit-walking in one place so the renderer only has
/// to plot points.
pub fn for_each_lit_pixel(
    font: &FontData,
    text: &str,
    origin_x: i32,
    origin_y: i32,
    mut f: impl FnMut(i32, i32),
) {
    let mut pen = origin_x;
    for ch in text.chars() {
        let codepoint = ch as u32;
        let Some(glyph) = font.glyph(codepoint) else {
            pen += font.default_advance as i32;
            continue;
        };
        let row_bytes = (glyph.w as usize).div_ceil(8);
        // BDF's BBX y offset is measured from the baseline to the bottom of
        // the cell; the cell's top sits `ascent - (y_off + h)` pixels below
        // the text box's top.
        let top = origin_y + font.ascent - (glyph.y_off as i32 + glyph.h as i32);
        for row in 0..glyph.h as usize {
            for col in 0..glyph.w as usize {
                let byte = glyph.bits[row * row_bytes + col / 8];
                // Hex rows are MSB-first: bit 7 is the leftmost pixel.
                if byte & (0x80 >> (col % 8)) != 0 {
                    f(pen + glyph.x_off as i32 + col as i32, top + row as i32);
                }
            }
        }
        pen += glyph.advance as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cozette() -> &'static FontData {
        font_from_id(DEFAULT_FONT).expect("the default font is always registered")
    }

    #[test]
    fn measure_sums_advances_and_reports_line_height() {
        let font = cozette();
        let (width, height) = measure(font, "Hi");
        assert_eq!(
            width,
            font.advance_of('H' as u32) + font.advance_of('i' as u32)
        );
        assert_eq!(height, font.line_height);
    }

    #[test]
    fn missing_glyph_still_advances_by_the_default() {
        let font = cozette();
        let (width, _) = measure(font, "\u{1F600}");
        assert_eq!(width, font.default_advance as u32);
    }

    #[test]
    fn lit_pixels_match_a_known_glyph() {
        // Cozette's 'A' is 70 88 88 88 F8 88 88 88 -> 3+2+2+2+5+2+2+2 lit.
        let mut count = 0;
        for_each_lit_pixel(cozette(), "A", 0, 0, |_, _| count += 1);
        assert_eq!(count, 20);
    }

    #[test]
    fn a_descender_reaches_below_the_baseline() {
        // 'g' has a BBX y offset of -3, so its lowest pixels land past the
        // baseline (ascent) and within the full line height.
        let font = cozette();
        let mut max_y = i32::MIN;
        for_each_lit_pixel(font, "g", 0, 0, |_, y| max_y = max_y.max(y));
        assert!(max_y >= font.ascent);
        assert!(max_y < font.line_height as i32);
    }

    fn layout(align: TextAlign, max_width: Option<f32>, line_spacing: f32) -> TextLayout {
        TextLayout {
            font: DEFAULT_FONT,
            scale: 1,
            align,
            max_width,
            line_spacing,
        }
    }

    #[test]
    fn one_line_measures_exactly_the_font_line_height() {
        let font = cozette();
        let (_, _, height) = lay_out(font, &layout(TextAlign::Left, None, 2.0), "Hi");
        assert_eq!(height, font.line_height as f32);
    }

    #[test]
    fn an_explicit_break_makes_two_lines_spaced_by_line_spacing() {
        let font = cozette();
        let (lines, _, height) = lay_out(font, &layout(TextAlign::Left, None, 1.5), "a\nb");
        assert_eq!(lines.len(), 2);
        let step = font.line_height as f32 * 1.5;
        assert_eq!(lines[1].dy, step);
        assert_eq!(height, step + font.line_height as f32);
    }

    #[test]
    fn wrapping_breaks_between_words_at_the_width() {
        let font = cozette();
        let one_word = measure(font, "hello").0 as f32;
        // Room for one word but not two.
        let (lines, _, _) = lay_out(
            font,
            &layout(TextAlign::Left, Some(one_word + 2.0), 1.0),
            "hello hello hello",
        );
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "hello");
    }

    #[test]
    fn alignment_offsets_each_line_by_its_own_width() {
        let font = cozette();
        let (left, block_w, _) = lay_out(font, &layout(TextAlign::Left, None, 1.0), "wide\nx");
        assert_eq!(left[0].dx, 0.0);
        assert_eq!(left[1].dx, 0.0);

        let (centre, _, _) = lay_out(font, &layout(TextAlign::Center, None, 1.0), "wide\nx");
        let wide_w = measure(font, "wide").0 as f32;
        let x_w = measure(font, "x").0 as f32;
        assert_eq!(centre[0].dx, -wide_w / 2.0);
        assert_eq!(centre[1].dx, -x_w / 2.0);

        let (right, _, _) = lay_out(font, &layout(TextAlign::Right, None, 1.0), "wide");
        assert_eq!(right[0].dx, -wide_w);
        assert_eq!(block_w, wide_w);
    }
}
