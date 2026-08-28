//! The Framebuffer device's fixed color palette: 26 hue families x 11 shades
//! (50-950, the familiar Tailwind scale). The `Color` enum, `COUNT`, and
//! `Color::hex` below are generated from `build/palette.rs`'s `PALETTE`
//! table — the one source that also feeds the `ely:framebuffer` TS module's
//! `Color` constants and their `elysium.d.ts` types — so the numeric id a
//! program passes across the `ely:framebuffer` boundary can't drift from
//! this enum's discriminants. Program-supplied colors are always one of
//! these named shades, never raw, unconstrained RGBA channels.

include!(concat!(env!("OUT_DIR"), "/palette.rs"));

impl Color {
    /// Straight-line lookup: valid discriminants are exactly `0..COUNT`,
    /// contiguous and dense, so this can't fail for any id a correctly
    /// generated `ely:framebuffer` constant could send.
    pub fn from_id(id: u16) -> Option<Color> {
        if (id as usize) < COUNT {
            // SAFETY: `Color` is `#[repr(u16)]` with dense discriminants
            // `0..COUNT`, just checked `id` falls in that range.
            Some(unsafe { std::mem::transmute::<u16, Color>(id) })
        } else {
            None
        }
    }

    /// This shade's color as a `tiny_skia::Color` (sRGB, fully opaque) —
    /// no gamma re-encoding happens on the softbuffer path, so this is
    /// `hex`'s bytes, verbatim.
    pub fn to_skia(self) -> tiny_skia::Color {
        let hex = self.hex();
        let r = ((hex >> 16) & 0xff) as u8;
        let g = ((hex >> 8) & 0xff) as u8;
        let b = (hex & 0xff) as u8;
        tiny_skia::Color::from_rgba8(r, g, b, 255)
    }

    /// The palette entry whose sRGB color is closest to `(r, g, b)` by
    /// squared Euclidean distance — a linear scan over all 288 entries.
    /// Runs once per pixel at image-load time (`kernel/image.rs`'s palette
    /// quantization pass) and behind the `nearestColor` binding, never per
    /// frame. A few palette shades are exact duplicates of each other (e.g.
    /// `Zinc50`/`Neutral50`/`Mauve50` are all `0xfafafa`); on a tie, the
    /// lowest-id entry wins.
    pub fn nearest(r: u8, g: u8, b: u8) -> Color {
        let (r, g, b) = (r as i32, g as i32, b as i32);
        // A plain indexed loop rather than `min_by_key` — the iterator
        // adapters don't inline in a debug build, and this runs per pixel.
        let mut best = PALETTE_RGB[0];
        let mut best_dist = i32::MAX;
        for &(pr, pg, pb, color) in PALETTE_RGB.iter() {
            let (dr, dg, db) = (pr as i32 - r, pg as i32 - g, pb as i32 - b);
            let dist = dr * dr + dg * dg + db * db;
            if dist < best_dist {
                best_dist = dist;
                best = (pr, pg, pb, color);
            }
        }
        best.3
    }
}

/// Every palette entry's straight sRGB `(r, g, b)` alongside its `Color`,
/// in id order, computed once on first use. `Color::nearest` scans this
/// flat array rather than re-running `Color::hex`'s 288-arm match for
/// every entry on every call — which profiling showed dominating runtime
/// when several processes each quantize an image at load time.
static PALETTE_RGB: std::sync::LazyLock<[(u8, u8, u8, Color); COUNT]> =
    std::sync::LazyLock::new(|| {
        std::array::from_fn(|i| {
            let color = Color::from_id(i as u16).expect("id in 0..COUNT is always valid");
            let hex = color.hex();
            (
                ((hex >> 16) & 0xff) as u8,
                ((hex >> 8) & 0xff) as u8,
                (hex & 0xff) as u8,
                color,
            )
        })
    });

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry's own color must map back to *a* palette entry with the
    /// same hex value — not necessarily the same variant, since a few
    /// shades are exact duplicates (see `nearest`'s doc comment) and ties
    /// resolve to the lowest id.
    #[test]
    fn nearest_round_trips_every_palette_entry_hex() {
        for id in 0..COUNT as u16 {
            let color = Color::from_id(id).unwrap();
            let hex = color.hex();
            let r = ((hex >> 16) & 0xff) as u8;
            let g = ((hex >> 8) & 0xff) as u8;
            let b = (hex & 0xff) as u8;
            assert_eq!(
                Color::nearest(r, g, b).hex(),
                hex,
                "nearest({r}, {g}, {b}) didn't round-trip {color:?}'s own hex"
            );
        }
    }

    #[test]
    fn nearest_pure_black_and_white() {
        assert_eq!(Color::nearest(0, 0, 0), Color::Black);
        assert_eq!(Color::nearest(255, 255, 255), Color::White);
    }

    #[test]
    fn nearest_picks_the_right_hue_family() {
        assert!(format!("{:?}", Color::nearest(255, 0, 0)).starts_with("Red"));
        assert!(format!("{:?}", Color::nearest(0, 0, 255)).starts_with("Blue"));
        assert!(format!("{:?}", Color::nearest(0, 255, 0)).starts_with("Green"));
    }

    /// The precomputed-palette scan must agree with a naive scan that
    /// rebuilds each entry from its id — including lowest-id-wins on ties.
    #[test]
    fn nearest_matches_a_naive_reference_scan() {
        fn reference(r: u8, g: u8, b: u8) -> Color {
            (0..COUNT as u16)
                .map(|id| Color::from_id(id).unwrap())
                .min_by_key(|color| {
                    let hex = color.hex();
                    let dr = ((hex >> 16) & 0xff) as i32 - r as i32;
                    let dg = ((hex >> 8) & 0xff) as i32 - g as i32;
                    let db = (hex & 0xff) as i32 - b as i32;
                    dr * dr + dg * dg + db * db
                })
                .unwrap()
        }
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    assert_eq!(
                        Color::nearest(r, g, b),
                        reference(r, g, b),
                        "at ({r},{g},{b})"
                    );
                }
            }
        }
    }
}
