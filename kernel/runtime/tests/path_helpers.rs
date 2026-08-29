//! `ely:filesystem`'s pure path helpers — `resolve`, `join`, the `extract*`
//! accessors and `sanitize`. These answer questions about a path as text and
//! never touch the disk, so unlike the operations in `filesystem.rs` they run
//! against the shared read-only fixtures root and need no scratch directory.

use super::*;

#[test]
fn filesystem_resolve_does_not_double_the_leading_slash() {
    let runtime = eval(
        "import { resolve } from 'ely:filesystem'; \
         globalThis.result = resolve('a', 'b');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "/a/b");
}

#[test]
fn filesystem_join_normalizes_dot_and_dot_dot_segments() {
    let runtime = eval(
        "import { join } from 'ely:filesystem'; \
         globalThis.result = join('/a', 'b/../c', './d');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "/a/c/d");
}

#[test]
fn filesystem_join_clamps_dot_dot_at_the_root() {
    let runtime = eval(
        "import { join } from 'ely:filesystem'; \
         globalThis.result = join('/a', '../../b');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "/b");
}

#[test]
fn filesystem_extract_directory_name_of_root_child_is_root() {
    let runtime = eval(
        "import { extractDirectoryName } from 'ely:filesystem'; \
         globalThis.result = extractDirectoryName('/a');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "/");
}

#[test]
fn filesystem_extract_directory_name_ignores_trailing_slashes() {
    let runtime = eval(
        "import { extractDirectoryName } from 'ely:filesystem'; \
         globalThis.result = extractDirectoryName('/a/b/');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "/a");
}

#[test]
fn filesystem_extract_base_name_ignores_trailing_slashes() {
    let runtime = eval(
        "import { extractBaseName } from 'ely:filesystem'; \
         globalThis.result = extractBaseName('/a/b/');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "b");
}

#[test]
fn filesystem_extract_extension_of_a_dotfile_is_empty() {
    let runtime = eval(
        "import { extractExtension } from 'ely:filesystem'; \
         globalThis.result = extractExtension('.bashrc');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "");
}

#[test]
fn filesystem_extract_extension_of_a_multi_dot_file_is_the_last_extension() {
    let runtime = eval(
        "import { extractExtension } from 'ely:filesystem'; \
         globalThis.result = extractExtension('archive.tar.gz');",
    );
    assert_eq!(global::<String>(&runtime, "result"), ".gz");
}

#[test]
fn filesystem_sanitize_name_replaces_traversal_segments() {
    let runtime = eval(
        "import { sanitizeName } from 'ely:filesystem'; \
         globalThis.dot = sanitizeName('.'); \
         globalThis.dotDot = sanitizeName('..');",
    );
    assert_eq!(global::<String>(&runtime, "dot"), "_");
    assert_eq!(global::<String>(&runtime, "dotDot"), "__");
}
