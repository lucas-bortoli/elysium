//! Shared helper for the build-time table generators (`build/palette.rs`,
//! `build/keys.rs`): replace the span of a committed file between two marker
//! lines with freshly generated text, writing back only when it changed.

use std::path::Path;

/// In `path`, replace everything between the line holding
/// `// GENERATED:<tag>:start` and the line holding `// GENERATED:<tag>:end`
/// (both marker lines kept) with `body`, which must already carry the
/// surrounding file's indentation and end with a newline.
///
/// Panics if either marker is absent or out of order — a botched merge
/// should fail `cargo build` loudly, not silently skip codegen. The file is
/// rewritten only when the result differs, so a synced tree is left
/// untouched: no mtime churn, no rebuild loop.
pub fn rewrite_between_markers(path: &Path, tag: &str, body: &str) {
    let start_marker = format!("// GENERATED:{tag}:start");
    let end_marker = format!("// GENERATED:{tag}:end");

    let original = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("codegen: reading {}: {e}", path.display()));
    let lines: Vec<&str> = original.split_inclusive('\n').collect();

    let start = lines
        .iter()
        .position(|l| l.contains(&start_marker))
        .unwrap_or_else(|| panic!("codegen: {} has no `{start_marker}`", path.display()));
    let end = lines
        .iter()
        .position(|l| l.contains(&end_marker))
        .unwrap_or_else(|| panic!("codegen: {} has no `{end_marker}`", path.display()));
    assert!(
        start < end,
        "codegen: {} has `{end_marker}` before `{start_marker}`",
        path.display()
    );

    let mut out = String::new();
    out.extend(lines[..=start].iter().copied());
    out.push_str(body);
    out.extend(lines[end..].iter().copied());

    if out != original {
        std::fs::write(path, &out)
            .unwrap_or_else(|e| panic!("codegen: writing {}: {e}", path.display()));
    }
}
