//! The kernel's built-in bitmap fonts and the text-layout math the
//! Framebuffer device draws strings with.
//!
//! The font data itself — one `FontData` per built-in font, each carrying
//! its own metrics and a table of `Glyph`s with the BDF's own 1bpp row
//! packing — is generated at build time from BDF sources by `build/fonts.rs`
//! and `include!`d here. Nothing in this file assumes a particular font's
//! size: metrics travel with each `FontData`, so adding a second font is
//! just another entry in the generated `FONTS` slice.

include!(concat!(env!("OUT_DIR"), "/fonts.rs"));

/// A built-in font, identified by its index in the generated `FONTS` slice.
/// This is the value that crosses the `ely:framebuffer` boundary, mirroring
/// how a `Color` crosses it as a numeric id.
pub type FontId = u16;

/// The font used when a program doesn't name one. The `ely:framebuffer`
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
        assert_eq!(width, font.advance_of('H' as u32) + font.advance_of('i' as u32));
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
}
