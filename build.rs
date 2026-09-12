use std::path::PathBuf;

#[path = "build/fonts.rs"]
mod fonts;
#[path = "kernel/framebuffer/palette.rs"]
mod palette;

fn main() {
    println!("cargo:rerun-if-changed=build/fonts.rs");
    println!("cargo:rerun-if-changed=kernel/framebuffer/palette.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Parse the configured BDF fonts and emit `$OUT_DIR/fonts.rs`, which
    // `kernel/text.rs` includes.
    fonts::generate(&out_dir);
    // Emit `$OUT_DIR/palette.rs` (the `Color` enum and its `hex` match) from
    // the one palette table, which `kernel/framebuffer/colors.rs` includes.
    std::fs::write(out_dir.join("palette.rs"), palette::render_rust())
        .expect("failed to write the generated palette");

    // The userland tree is not copied or watched here: the kernel reads it
    // straight from the source checkout (see `kernel/main.rs`), so a `.ts`
    // edit is not an input to this build and never triggers a rebuild or
    // relink. A packaged binary instead finds `userland` beside the
    // executable, which the packaging step is responsible for placing there.
}
