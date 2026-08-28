//! Build-time color-palette codegen, invoked by `build.rs`.
//!
//! `PALETTE` is the single source of truth for the Framebuffer device's
//! fixed palette: `(variant name, 0xRRGGBB)` in id order, an entry's index
//! being the `u16` that crosses the `ely:framebuffer` boundary. From it this
//! emits `$OUT_DIR/palette.rs` — the `Color` enum, `COUNT`, and
//! `Color::hex`, `include!`d by `kernel/framebuffer/colors.rs` — and
//! rewrites the generated `Color` blocks in
//! `kernel/runtime_modules/framebuffer.ts` and `userland/elysium.d.ts`.
//!
//! A malformed entry here fails `cargo build` when the generated Rust
//! doesn't compile, so this stands in for a unit test of the emitter the
//! same way `build/fonts.rs`'s asserts do.

use std::fmt::Write as _;
use std::path::Path;

use crate::splice::rewrite_between_markers;

/// `(variant name, straight sRGB hex)` in wire-id order: 26 hue families x
/// 11 shades (Tailwind's 50-950), then `Black` and `White`.
const PALETTE: &[(&str, u32)] = &[
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

pub fn generate(out_dir: &Path) {
    generate_rust(out_dir);
    generate_ts();
    generate_dts();
}

fn generate_rust(out_dir: &Path) {
    let mut s =
        String::from("// @generated by build/palette.rs from its PALETTE table — do not edit.\n\n");
    s.push_str(
        "/// One shade from the Framebuffer device's fixed palette. `#[repr(u16)]`\n\
         /// with explicit discriminants because the value *is* the wire format that\n\
         /// crosses the `ely:framebuffer` boundary; variants are built from that id\n\
         /// via `Color::from_id`, seldom named in Rust, hence `#[allow(dead_code)]`.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n\
         #[repr(u16)]\n\
         #[allow(dead_code)]\n\
         pub enum Color {\n",
    );
    for (id, (name, _)) in PALETTE.iter().enumerate() {
        writeln!(s, "    {name} = {id},").unwrap();
    }
    s.push_str("}\n\n");
    s.push_str("/// Number of palette entries — valid discriminants are exactly `0..COUNT`.\n");
    writeln!(s, "const COUNT: usize = {};\n", PALETTE.len()).unwrap();
    s.push_str(
        "impl Color {\n\
         \x20   /// This shade's straight sRGB (gamma-encoded) hex triplet `0xRRGGBB`.\n\
         \x20   pub fn hex(self) -> u32 {\n\
         \x20       match self {\n",
    );
    for (name, hex) in PALETTE {
        writeln!(s, "            Color::{name} => {hex:#08x},").unwrap();
    }
    s.push_str("        }\n    }\n}\n");

    std::fs::write(out_dir.join("palette.rs"), s).unwrap();
}

fn generate_ts() {
    let mut body = String::from("export const Color = {\n");
    for (id, (name, _)) in PALETTE.iter().enumerate() {
        writeln!(body, "  {name}: {id},").unwrap();
    }
    body.push_str("} as const;\n");
    rewrite_between_markers(
        Path::new("kernel/runtime_modules/framebuffer.ts"),
        "palette",
        &body,
    );
}

fn generate_dts() {
    let mut body = String::from("  export const Color: {\n");
    for (id, (name, _)) in PALETTE.iter().enumerate() {
        writeln!(body, "    readonly {name}: {id};").unwrap();
    }
    body.push_str("  };\n");
    rewrite_between_markers(Path::new("userland/elysium.d.ts"), "palette", &body);
}
