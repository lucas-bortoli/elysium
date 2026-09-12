//! The Framebuffer device's fixed color palette, and the single table every
//! copy of it is generated from.
//!
//! A palette entry has to be spelled out in three places: the kernel's
//! `Color` enum, the `Color` constant `ely:framebuffer` exports, and the
//! ambient declaration userland typechecks against. Keeping three hand-written
//! copies in step was never mechanized, so this table is the one place an
//! entry is written down and the other three are rendered from it — the Rust
//! side at build time (see `build.rs`), the two TypeScript sides by
//! [`render_typescript`]/[`render_declaration`], checked against what is on
//! disk by this module's tests.
//!
//! An entry's index in this table is the numeric id that crosses the
//! `ely:framebuffer` boundary, so entries may be appended but never reordered
//! or removed without breaking every program that names a color after them.
//!
//! The palette is 26 hue families x 11 shades (50-950, the familiar Tailwind
//! scale) plus pure black and white. Colors are given as sRGB (gamma-encoded)
//! `0xRRGGBB`, exactly the values the device was given.

/// Every palette entry, as `(name, 0xRRGGBB)`, in id order.
pub const PALETTE: &[(&str, u32)] = &[
    ("Red50", 0xfef2f2),
    ("Red100", 0xfee2e2),
    ("Red200", 0xfecaca),
    ("Red300", 0xfca5a5),
    ("Red400", 0xf87171),
    ("Red500", 0xef4444),
    ("Red600", 0xdc2626),
    ("Red700", 0xb91c1c),
    ("Red800", 0x991b1b),
    ("Red900", 0x7f1d1d),
    ("Red950", 0x450a0a),
    ("Orange50", 0xfff7ed),
    ("Orange100", 0xffedd5),
    ("Orange200", 0xfed7aa),
    ("Orange300", 0xfdba74),
    ("Orange400", 0xfb923c),
    ("Orange500", 0xf97316),
    ("Orange600", 0xea580c),
    ("Orange700", 0xc2410c),
    ("Orange800", 0x9a3412),
    ("Orange900", 0x7c2d12),
    ("Orange950", 0x431407),
    ("Amber50", 0xfffbeb),
    ("Amber100", 0xfef3c7),
    ("Amber200", 0xfde68a),
    ("Amber300", 0xfcd34d),
    ("Amber400", 0xfbbf24),
    ("Amber500", 0xf59e0b),
    ("Amber600", 0xd97706),
    ("Amber700", 0xb45309),
    ("Amber800", 0x92400e),
    ("Amber900", 0x78350f),
    ("Amber950", 0x451a03),
    ("Yellow50", 0xfefce8),
    ("Yellow100", 0xfef9c3),
    ("Yellow200", 0xfef08a),
    ("Yellow300", 0xfde047),
    ("Yellow400", 0xfacc15),
    ("Yellow500", 0xeab308),
    ("Yellow600", 0xca8a04),
    ("Yellow700", 0xa16207),
    ("Yellow800", 0x854d0e),
    ("Yellow900", 0x713f12),
    ("Yellow950", 0x422006),
    ("Lime50", 0xf7fee7),
    ("Lime100", 0xecfccb),
    ("Lime200", 0xd9f99d),
    ("Lime300", 0xbef264),
    ("Lime400", 0xa3e635),
    ("Lime500", 0x84cc16),
    ("Lime600", 0x65a30d),
    ("Lime700", 0x4d7c0f),
    ("Lime800", 0x3f6212),
    ("Lime900", 0x365314),
    ("Lime950", 0x1a2e05),
    ("Green50", 0xf0fdf4),
    ("Green100", 0xdcfce7),
    ("Green200", 0xbbf7d0),
    ("Green300", 0x86efac),
    ("Green400", 0x4ade80),
    ("Green500", 0x22c55e),
    ("Green600", 0x16a34a),
    ("Green700", 0x15803d),
    ("Green800", 0x166534),
    ("Green900", 0x14532d),
    ("Green950", 0x052e16),
    ("Emerald50", 0xecfdf5),
    ("Emerald100", 0xd1fae5),
    ("Emerald200", 0xa7f3d0),
    ("Emerald300", 0x6ee7b7),
    ("Emerald400", 0x34d399),
    ("Emerald500", 0x10b981),
    ("Emerald600", 0x059669),
    ("Emerald700", 0x047857),
    ("Emerald800", 0x065f46),
    ("Emerald900", 0x064e3b),
    ("Emerald950", 0x022c22),
    ("Teal50", 0xf0fdfa),
    ("Teal100", 0xccfbf1),
    ("Teal200", 0x99f6e4),
    ("Teal300", 0x5eead4),
    ("Teal400", 0x2dd4bf),
    ("Teal500", 0x14b8a6),
    ("Teal600", 0x0d9488),
    ("Teal700", 0x0f766e),
    ("Teal800", 0x115e59),
    ("Teal900", 0x134e4a),
    ("Teal950", 0x042f2e),
    ("Cyan50", 0xecfeff),
    ("Cyan100", 0xcffafe),
    ("Cyan200", 0xa5f3fc),
    ("Cyan300", 0x67e8f9),
    ("Cyan400", 0x22d3ee),
    ("Cyan500", 0x06b6d4),
    ("Cyan600", 0x0891b2),
    ("Cyan700", 0x0e7490),
    ("Cyan800", 0x155e75),
    ("Cyan900", 0x164e63),
    ("Cyan950", 0x083344),
    ("Sky50", 0xf0f9ff),
    ("Sky100", 0xe0f2fe),
    ("Sky200", 0xbae6fd),
    ("Sky300", 0x7dd3fc),
    ("Sky400", 0x38bdf8),
    ("Sky500", 0x0ea5e9),
    ("Sky600", 0x0284c7),
    ("Sky700", 0x0369a1),
    ("Sky800", 0x075985),
    ("Sky900", 0x0c4a6e),
    ("Sky950", 0x082f49),
    ("Blue50", 0xeff6ff),
    ("Blue100", 0xdbeafe),
    ("Blue200", 0xbfdbfe),
    ("Blue300", 0x93c5fd),
    ("Blue400", 0x60a5fa),
    ("Blue500", 0x3b82f6),
    ("Blue600", 0x2563eb),
    ("Blue700", 0x1d4ed8),
    ("Blue800", 0x1e40af),
    ("Blue900", 0x1e3a8a),
    ("Blue950", 0x172554),
    ("Indigo50", 0xeef2ff),
    ("Indigo100", 0xe0e7ff),
    ("Indigo200", 0xc7d2fe),
    ("Indigo300", 0xa5b4fc),
    ("Indigo400", 0x818cf8),
    ("Indigo500", 0x6366f1),
    ("Indigo600", 0x4f46e5),
    ("Indigo700", 0x4338ca),
    ("Indigo800", 0x3730a3),
    ("Indigo900", 0x312e81),
    ("Indigo950", 0x1e1b4b),
    ("Violet50", 0xf5f3ff),
    ("Violet100", 0xede9fe),
    ("Violet200", 0xddd6fe),
    ("Violet300", 0xc4b5fd),
    ("Violet400", 0xa78bfa),
    ("Violet500", 0x8b5cf6),
    ("Violet600", 0x7c3aed),
    ("Violet700", 0x6d28d9),
    ("Violet800", 0x5b21b6),
    ("Violet900", 0x4c1d95),
    ("Violet950", 0x2e1065),
    ("Purple50", 0xfaf5ff),
    ("Purple100", 0xf3e8ff),
    ("Purple200", 0xe9d5ff),
    ("Purple300", 0xd8b4fe),
    ("Purple400", 0xc084fc),
    ("Purple500", 0xa855f7),
    ("Purple600", 0x9333ea),
    ("Purple700", 0x7e22ce),
    ("Purple800", 0x6b21a8),
    ("Purple900", 0x581c87),
    ("Purple950", 0x3b0764),
    ("Fuchsia50", 0xfdf4ff),
    ("Fuchsia100", 0xfae8ff),
    ("Fuchsia200", 0xf5d0fe),
    ("Fuchsia300", 0xf0abfc),
    ("Fuchsia400", 0xe879f9),
    ("Fuchsia500", 0xd946ef),
    ("Fuchsia600", 0xc026d3),
    ("Fuchsia700", 0xa21caf),
    ("Fuchsia800", 0x86198f),
    ("Fuchsia900", 0x701a75),
    ("Fuchsia950", 0x4a044e),
    ("Pink50", 0xfdf2f8),
    ("Pink100", 0xfce7f3),
    ("Pink200", 0xfbcfe8),
    ("Pink300", 0xf9a8d4),
    ("Pink400", 0xf472b6),
    ("Pink500", 0xec4899),
    ("Pink600", 0xdb2777),
    ("Pink700", 0xbe185d),
    ("Pink800", 0x9d174d),
    ("Pink900", 0x831843),
    ("Pink950", 0x500724),
    ("Rose50", 0xfff1f2),
    ("Rose100", 0xffe4e6),
    ("Rose200", 0xfecdd3),
    ("Rose300", 0xfda4af),
    ("Rose400", 0xfb7185),
    ("Rose500", 0xf43f5e),
    ("Rose600", 0xe11d48),
    ("Rose700", 0xbe123c),
    ("Rose800", 0x9f1239),
    ("Rose900", 0x881337),
    ("Rose950", 0x4c0519),
    ("Slate50", 0xf8fafc),
    ("Slate100", 0xf1f5f9),
    ("Slate200", 0xe2e8f0),
    ("Slate300", 0xcbd5e1),
    ("Slate400", 0x94a3b8),
    ("Slate500", 0x64748b),
    ("Slate600", 0x475569),
    ("Slate700", 0x334155),
    ("Slate800", 0x1e293b),
    ("Slate900", 0x0f172a),
    ("Slate950", 0x020617),
    ("Gray50", 0xf9fafb),
    ("Gray100", 0xf3f4f6),
    ("Gray200", 0xe5e7eb),
    ("Gray300", 0xd1d5db),
    ("Gray400", 0x9ca3af),
    ("Gray500", 0x6b7280),
    ("Gray600", 0x4b5563),
    ("Gray700", 0x374151),
    ("Gray800", 0x1f2937),
    ("Gray900", 0x111827),
    ("Gray950", 0x030712),
    ("Zinc50", 0xfafafa),
    ("Zinc100", 0xf4f4f5),
    ("Zinc200", 0xe4e4e7),
    ("Zinc300", 0xd4d4d8),
    ("Zinc400", 0xa1a1aa),
    ("Zinc500", 0x71717a),
    ("Zinc600", 0x52525b),
    ("Zinc700", 0x3f3f46),
    ("Zinc800", 0x27272a),
    ("Zinc900", 0x18181b),
    ("Zinc950", 0x09090b),
    ("Neutral50", 0xfafafa),
    ("Neutral100", 0xf5f5f5),
    ("Neutral200", 0xe5e5e5),
    ("Neutral300", 0xd4d4d4),
    ("Neutral400", 0xa3a3a3),
    ("Neutral500", 0x737373),
    ("Neutral600", 0x525252),
    ("Neutral700", 0x404040),
    ("Neutral800", 0x262626),
    ("Neutral900", 0x171717),
    ("Neutral950", 0x0a0a0a),
    ("Stone50", 0xfafaf9),
    ("Stone100", 0xf5f5f4),
    ("Stone200", 0xe7e5e4),
    ("Stone300", 0xd6d3d1),
    ("Stone400", 0xa8a29e),
    ("Stone500", 0x78716c),
    ("Stone600", 0x57534e),
    ("Stone700", 0x44403c),
    ("Stone800", 0x292524),
    ("Stone900", 0x1c1917),
    ("Stone950", 0x0c0a09),
    ("Taupe50", 0xfbfaf9),
    ("Taupe100", 0xf3f1f1),
    ("Taupe200", 0xe8e4e3),
    ("Taupe300", 0xd8d2d0),
    ("Taupe400", 0xaba09c),
    ("Taupe500", 0x7c6d67),
    ("Taupe600", 0x5b4f4b),
    ("Taupe700", 0x473c39),
    ("Taupe800", 0x2b2422),
    ("Taupe900", 0x1d1816),
    ("Taupe950", 0x0c0a09),
    ("Mauve50", 0xfafafa),
    ("Mauve100", 0xf3f1f3),
    ("Mauve200", 0xe7e4e7),
    ("Mauve300", 0xd7d0d7),
    ("Mauve400", 0xa89ea9),
    ("Mauve500", 0x79697b),
    ("Mauve600", 0x594c5b),
    ("Mauve700", 0x463947),
    ("Mauve800", 0x2a212c),
    ("Mauve900", 0x1d161e),
    ("Mauve950", 0x0c090c),
    ("Mist50", 0xf9fbfb),
    ("Mist100", 0xf1f3f3),
    ("Mist200", 0xe3e7e8),
    ("Mist300", 0xd0d6d8),
    ("Mist400", 0x9ca8ab),
    ("Mist500", 0x67787c),
    ("Mist600", 0x4b585b),
    ("Mist700", 0x394447),
    ("Mist800", 0x22292b),
    ("Mist900", 0x161b1d),
    ("Mist950", 0x090b0c),
    ("Olive50", 0xfbfbf9),
    ("Olive100", 0xf4f4f0),
    ("Olive200", 0xe8e8e3),
    ("Olive300", 0xd8d8d0),
    ("Olive400", 0xabab9c),
    ("Olive500", 0x7c7c67),
    ("Olive600", 0x5b5b4b),
    ("Olive700", 0x474739),
    ("Olive800", 0x2b2b22),
    ("Olive900", 0x1d1d16),
    ("Olive950", 0x0c0c09),
    ("Black", 0x000000),
    ("White", 0xffffff),
];

/// The marker that opens a generated region in a checked-in file. The
/// renderers below emit only what goes between it and [`GENERATED_END`];
/// everything outside is hand-written and left alone.
pub const GENERATED_BEGIN: &str = "generated from kernel/framebuffer/palette.rs";
pub const GENERATED_END: &str = "end generated";

// Written by the build script; the crate's own copy of this file never
// calls it, so that copy sees it as unused.
/// The `Color` enum, its `hex()` match, and `COUNT` — written to
/// `$OUT_DIR/palette.rs` by the build script and `include!`d by
/// `kernel/framebuffer/colors.rs`.
#[allow(dead_code)]
pub fn render_rust() -> String {
    let mut out = String::new();
    out.push_str("// Generated from kernel/framebuffer/palette.rs. Do not edit.\n\n");

    out.push_str("/// One shade from the palette. `#[repr(u16)]` with dense discriminants\n");
    out.push_str("/// because the numeric value *is* the wire format `ely:framebuffer`'s\n");
    out.push_str("/// functions receive from JS. Most variants are only ever constructed\n");
    out.push_str("/// from that numeric id via `Color::from_id`, never named directly in\n");
    out.push_str("/// Rust, so dead-code analysis can't see them as used.\n");
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n#[repr(u16)]\n#[allow(dead_code)]\npub enum Color {\n");
    for (id, (name, _)) in PALETTE.iter().enumerate() {
        out.push_str(&format!("    {name} = {id},\n"));
    }
    out.push_str("}\n\n");

    out.push_str("impl Color {\n");
    out.push_str("    /// This shade's color as its original sRGB (gamma-encoded) hex\n");
    out.push_str("    /// triplet `0xRRGGBB`, exactly the palette values the Framebuffer\n");
    out.push_str("    /// device was given.\n");
    out.push_str("    pub fn hex(self) -> u32 {\n        match self {\n");
    for (name, hex) in PALETTE {
        out.push_str(&format!("            Color::{name} => {hex:#08x},\n"));
    }
    out.push_str("        }\n    }\n}\n\n");

    out.push_str(&format!("pub const COUNT: usize = {};\n", PALETTE.len()));
    out
}

// Rendered for the checked-in TypeScript copies, which the build script
// doesn't touch — reached only from this module's tests, so its copy of
// this file sees these as unused.
/// The body of `ely:framebuffer`'s exported `Color` constant, for the
/// generated region of `kernel/runtime_modules/framebuffer.ts`.
#[allow(dead_code)]
pub fn render_typescript() -> String {
    let mut out = String::from("export const Color = {\n");
    for (id, (name, _)) in PALETTE.iter().enumerate() {
        out.push_str(&format!("  {name}: {id},\n"));
    }
    out.push_str("} as const;\n");
    out
}

/// The body of the ambient `Color` declaration, for the generated region of
/// `userland/elysium.d.ts`. Each entry is declared at its literal type so a
/// program passing `Color.Red500` is checked against the exact id.
#[allow(dead_code)]
pub fn render_declaration() -> String {
    let mut out = String::from("  export const Color: {\n");
    for (id, (name, _)) in PALETTE.iter().enumerate() {
        out.push_str(&format!("    readonly {name}: {id};\n"));
    }
    out.push_str("  };\n");
    out
}

/// Replaces the text between the `GENERATED_BEGIN` and `GENERATED_END`
/// markers in `source` with `body`, keeping the marker lines themselves.
/// `None` if `source` doesn't carry both markers.
#[allow(dead_code)]
pub fn splice_generated_region(source: &str, body: &str) -> Option<String> {
    let begin = source.find(GENERATED_BEGIN)?;
    let after_begin = begin + source[begin..].find('\n')? + 1;
    let end = source[after_begin..].find(GENERATED_END)? + after_begin;
    let before_end = source[..end].rfind('\n')? + 1;
    Some(format!(
        "{}{}{}",
        &source[..after_begin],
        body,
        &source[before_end..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repository_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Checks that the generated region of `relative_path` holds exactly what
    /// `rendered` produces. Setting `ELYSIUM_BLESS=1` rewrites the file
    /// instead of failing, which is how the two TypeScript copies are brought
    /// back in step after the table changes.
    fn assert_generated_region(relative_path: &str, rendered: String) {
        let path = repository_root().join(relative_path);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        let expected = splice_generated_region(&source, &rendered).unwrap_or_else(|| {
            panic!("{relative_path} has no `<{GENERATED_BEGIN}>` / `<{GENERATED_END}>` markers")
        });

        if source == expected {
            return;
        }
        if std::env::var_os("ELYSIUM_BLESS").is_some() {
            std::fs::write(&path, expected).expect("failed to write the regenerated region");
            return;
        }
        panic!(
            "{relative_path} is out of step with kernel/framebuffer/palette.rs.\n\
             Re-run with ELYSIUM_BLESS=1 to regenerate it."
        );
    }

    #[test]
    fn the_typescript_palette_matches_the_table() {
        assert_generated_region("kernel/runtime_modules/framebuffer.ts", render_typescript());
    }

    #[test]
    fn the_declared_palette_matches_the_table() {
        assert_generated_region("userland/elysium.d.ts", render_declaration());
    }

    /// An entry's index is the wire id, so a duplicate name would give two
    /// shades the same constant on the TypeScript side and silently shadow
    /// one of them.
    #[test]
    fn every_palette_entry_has_a_distinct_name() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in PALETTE {
            assert!(seen.insert(*name), "{name} appears in the palette twice");
        }
        assert_eq!(seen.len(), PALETTE.len());
    }

    /// Nothing outside `0xRRGGBB` can round-trip through `hex`.
    #[test]
    fn every_palette_color_fits_in_three_channels() {
        for (name, hex) in PALETTE {
            assert!(*hex <= 0xffffff, "{name} is not a 24-bit color");
        }
    }

    #[test]
    fn splice_replaces_only_what_lies_between_the_markers() {
        let source = format!(
            "keep me\n// <{GENERATED_BEGIN}>\nold\nlines\n// <{GENERATED_END}>\nkeep me too\n"
        );
        let spliced = splice_generated_region(&source, "new\n").expect("markers are present");
        assert_eq!(
            spliced,
            format!("keep me\n// <{GENERATED_BEGIN}>\nnew\n// <{GENERATED_END}>\nkeep me too\n")
        );
    }

    #[test]
    fn splice_reports_a_file_with_no_markers() {
        assert!(splice_generated_region("nothing to see here\n", "new\n").is_none());
    }
}
