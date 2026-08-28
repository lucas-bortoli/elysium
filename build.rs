use std::path::{Path, PathBuf};

#[path = "build/fonts.rs"]
mod fonts;

fn main() {
    println!("cargo:rerun-if-changed=userland");
    println!("cargo:rerun-if-changed=build/fonts.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Parse the configured BDF fonts and emit `$OUT_DIR/fonts.rs`, which
    // `kernel/text.rs` includes.
    fonts::generate(&out_dir);
    // OUT_DIR is target/<profile>/build/<pkg>-<hash>/out; the binary lands
    // three levels up, in target/<profile>.
    let profile_dir = out_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("OUT_DIR has an unexpected shape");

    let src = Path::new("userland");
    let dst = profile_dir.join("userland");
    copy_dir(src, &dst);
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}
