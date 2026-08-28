//! The `ely:filesystem` surface: the pure path helpers and the
//! real-filesystem read/write/stat/list/remove operations.

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

#[test]
fn filesystem_write_file_and_read_file_round_trip_the_whole_file() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { writeFile, readFile } from 'ely:filesystem'; \
         const bytes = Uint8Array.from('hello', (c) => c.charCodeAt(0)); \
         writeFile('/greeting.bin', bytes); \
         const read = readFile('/greeting.bin'); \
         globalThis.result = Array.from(read, (b) => String.fromCharCode(b)).join('');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "hello");
}

#[test]
fn filesystem_write_file_with_a_range_patches_in_place() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { writeFile, readFile } from 'ely:filesystem'; \
         const initial = Uint8Array.from('0123456789', (c) => c.charCodeAt(0)); \
         writeFile('/patch.bin', initial); \
         const patch = Uint8Array.from('XYZ', (c) => c.charCodeAt(0)); \
         writeFile('/patch.bin', patch, { offset: 2, length: 3 }); \
         const read = readFile('/patch.bin'); \
         globalThis.result = Array.from(read, (b) => String.fromCharCode(b)).join('');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "01XYZ56789");
}

#[test]
fn filesystem_read_file_with_a_range_reads_a_slice() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { writeFile, readFile } from 'ely:filesystem'; \
         const bytes = Uint8Array.from('0123456789', (c) => c.charCodeAt(0)); \
         writeFile('/slice.bin', bytes); \
         const read = readFile('/slice.bin', { offset: 3, length: 4 }); \
         globalThis.result = Array.from(read, (b) => String.fromCharCode(b)).join('');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "3456");
}

#[test]
fn filesystem_write_text_file_and_read_text_file_round_trip() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { writeTextFile, readTextFile } from 'ely:filesystem'; \
         writeTextFile('/note.txt', 'hello world'); \
         globalThis.result = readTextFile('/note.txt');",
    );
    assert_eq!(global::<String>(&runtime, "result"), "hello world");
}

#[test]
fn filesystem_remove_deletes_a_file() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { writeTextFile, remove, stat } from 'ely:filesystem'; \
         writeTextFile('/gone.txt', 'bye'); \
         remove('/gone.txt'); \
         globalThis.threw = false; \
         try { \
             stat('/gone.txt'); \
         } catch (err) { \
             globalThis.threw = true; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn filesystem_remove_recursively_deletes_a_directory() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { createDirectory, writeTextFile, remove, stat } from 'ely:filesystem'; \
         createDirectory('/tree/nested'); \
         writeTextFile('/tree/nested/file.txt', 'x'); \
         remove('/tree'); \
         globalThis.threw = false; \
         try { \
             stat('/tree'); \
         } catch (err) { \
             globalThis.threw = true; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
}

#[test]
fn filesystem_create_directory_creates_nested_missing_parents() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { createDirectory, stat } from 'ely:filesystem'; \
         createDirectory('/a/b/c'); \
         globalThis.result = stat('/a/b/c').kind;",
    );
    assert_eq!(global::<String>(&runtime, "result"), "Directory");
}

#[test]
fn filesystem_list_directory_omits_symlinks_but_lists_regular_entries() {
    let root = test_scratch_root();
    std::fs::write(root.join("real.txt"), b"data").unwrap();
    std::fs::create_dir(root.join("realdir")).unwrap();
    std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt")).unwrap();

    let runtime = eval_with_root(
        root,
        "import { listDirectory } from 'ely:filesystem'; \
         const entries = listDirectory('/'); \
         globalThis.paths = entries.map((e) => e.path).sort().join(','); \
         globalThis.count = entries.length;",
    );
    assert_eq!(global::<String>(&runtime, "paths"), "/real.txt,/realdir");
    assert_eq!(global::<f64>(&runtime, "count"), 2.0);
}

#[test]
fn filesystem_stat_reports_file_and_directory_shapes() {
    let runtime = eval_with_root(
        test_scratch_root(),
        "import { writeTextFile, createDirectory, stat } from 'ely:filesystem'; \
         writeTextFile('/f.txt', 'abcde'); \
         createDirectory('/d'); \
         const file = stat('/f.txt'); \
         const dir = stat('/d'); \
         globalThis.fileKind = file.kind; \
         globalThis.fileSize = file.size; \
         globalThis.filePath = file.path; \
         globalThis.dirKind = dir.kind; \
         globalThis.dirPath = dir.path;",
    );
    assert_eq!(global::<String>(&runtime, "fileKind"), "File");
    assert_eq!(global::<f64>(&runtime, "fileSize"), 5.0);
    assert_eq!(global::<String>(&runtime, "filePath"), "/f.txt");
    assert_eq!(global::<String>(&runtime, "dirKind"), "Directory");
    assert_eq!(global::<String>(&runtime, "dirPath"), "/d");
}

#[test]
fn filesystem_read_file_through_a_symlink_component_is_not_found() {
    let root = test_scratch_root();
    std::fs::create_dir(root.join("real")).unwrap();
    std::fs::write(root.join("real/target.txt"), b"secret").unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();

    let runtime = eval_with_root(
        root,
        "import { readFile, NotFoundError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             readFile('/alias/target.txt'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof NotFoundError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn filesystem_read_file_on_a_directory_throws_is_a_directory_error() {
    let root = test_scratch_root();
    std::fs::create_dir(root.join("adir")).unwrap();

    let runtime = eval_with_root(
        root,
        "import { readFile, IsADirectoryError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             readFile('/adir'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof IsADirectoryError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn filesystem_write_file_on_a_directory_throws_is_a_directory_error() {
    let root = test_scratch_root();
    std::fs::create_dir(root.join("adir")).unwrap();

    let runtime = eval_with_root(
        root,
        "import { writeFile, IsADirectoryError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             writeFile('/adir', new Uint8Array([1])); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof IsADirectoryError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn filesystem_create_directory_blocked_by_a_file_throws_not_a_directory_error() {
    let root = test_scratch_root();
    std::fs::write(root.join("blocker"), b"x").unwrap();

    let runtime = eval_with_root(
        root,
        "import { createDirectory, NotADirectoryError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             createDirectory('/blocker/nested'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof NotADirectoryError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn filesystem_list_directory_on_a_file_throws_not_a_directory_error() {
    let root = test_scratch_root();
    std::fs::write(root.join("f.txt"), b"x").unwrap();

    let runtime = eval_with_root(
        root,
        "import { listDirectory, NotADirectoryError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             listDirectory('/f.txt'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof NotADirectoryError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}

#[test]
fn filesystem_read_text_file_on_invalid_utf8_throws_text_decode_error() {
    let root = test_scratch_root();
    std::fs::write(root.join("bad.txt"), [0xff, 0xfe, 0xfd]).unwrap();

    let runtime = eval_with_root(
        root,
        "import { readTextFile, TextDecodeError } from 'ely:filesystem'; \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             readTextFile('/bad.txt'); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof TextDecodeError; \
         }",
    );
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}
