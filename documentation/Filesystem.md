# The filesystem: files and directories only

`ely:filesystem` gives a program read and write access to the userland tree every program lives under, the same root
`ely:image`'s `loadImage` already resolves pictures against ([1]).

```ts
import { listDirectory, extractBaseName } from "ely:filesystem";

function printTree(path: string, depth = 0) {
  for (const entry of listDirectory(path)) {
    print("  ".repeat(depth) + extractBaseName(entry.path));
    if (entry.kind === "Directory") printTree(entry.path, depth + 1);
  }
}

printTree("/");
```

## Working with paths

A handful of plain path-manipulation functions come along with this module,
independent of any actual disk access: `join` and `normalize` combine
segments and collapse `.`/`..` and redundant slashes; `resolve` builds an
absolute path out of several segments, stopping at the first one that's
already absolute; `extractBaseName`, `extractDirectoryName`, and
`extractExtension` pull the last segment, the parent, and the extension
back out of a path; and `sanitizeName` turns an arbitrary string into
something safe to use as a single path segment, replacing anything outside
`[a-zA-Z0-9_.-]` with `_` and turning a bare `.` or `..` into underscores of
the same length, since those two are otherwise made entirely of allowed
characters and would sail through unsanitized straight into a directory
traversal. None of these touch disk; they're the same kind of utility
Node's `path` module or a browser's `URL` provides, adapted to the
always-absolute, always-`/`-separated paths this module deals in.

## Reading and writing files

`readFile`/`writeFile` work in raw bytes, a `Uint8Array` in either
direction. Called with just a path, `readFile` reads the whole file and
`writeFile` truncates it and writes the whole thing anew. Both also accept
an optional range — `{ offset, length }` — that switches to partial I/O
instead: `readFile` seeks to `offset` and reads at most `length` bytes
rather than the whole file, while `writeFile` opens the file _without_
truncating it, seeks to `offset`, and writes only that many bytes, leaving
everything outside the written span untouched. This is what makes it
possible to patch a handful of bytes in the middle of an existing file
without reading and rewriting the whole thing. The optional range argument
is typed against `ely:container`'s `Option<T>` — modeled on Rust's own
`Option`, it's either the range object itself or absent (`undefined` or
`null`), which is exactly the distinction that decides whole-file
replacement versus an in-place patch.

`readTextFile`/`writeTextFile` cover the common case of a file that's just
UTF-8 text, without a program having to shuttle bytes through its own
encoder/decoder. They don't accept a range — unlike raw bytes, an arbitrary
byte offset into UTF-8 text can land in the middle of a multi-byte
character, so there's no sound way to define a partial read or write here.
Userland doesn't yet have `TextEncoder`/`TextDecoder` of its own (that's
expected on a separate branch); once it does, these two will likely become
thin wrappers over `readFile`/`writeFile` instead of doing their own text
handling.

## Directories

`createDirectory` makes a directory, creating any missing parent
directories along the way — the same as a shell's `mkdir -p`, not the
bare `mkdir` that fails if a parent is missing. `listDirectory` lists
what's directly inside a directory, and `stat` reports what a single path
is. Both describe an entry the same way, as an `EntryStat`: either
`{ kind: "File", size, path }` or `{ kind: "Directory", path }`, where
`path` is always that entry's own absolute path — for something
`listDirectory` returns, that's the directory being listed joined with the
entry's own name, not just the bare name by itself. Every entry
`listDirectory` reports is one or the other of these two kinds; nothing
else (and, per the invisibility described above, nothing symlinked) is
ever included.

There's a single function for removing something, `remove`, rather than
one for files and a separate one for directories — it looks at what's
actually there and does the right thing, unlinking a file or recursively
tearing down a directory and everything inside it.

## What goes wrong, specifically

Every one of these functions can fail in a handful of distinct, specific
ways, and each failure is its own error class.

- `NotFoundError` means a path, or something along it, doesn't exist.
- `IsADirectoryError` means a file operation ran into a directory instead.
- `NotADirectoryError` is the opposite: a directory operation blocked by an
  existing file somewhere along its path.
- `TextDecodeError` is specific to `readTextFile`, thrown when a file's bytes
  aren't valid UTF-8.
- `UnknownError` is the fallback for anything else — permission denied, running
  out of disk space, and so on — and still carries the full underlying error text,
  so it stays useful for tracking down a problem even though it isn't one of the
  named cases.

| Function          | `RelativePathError` | `NotFoundError` | `IsADirectoryError` | `NotADirectoryError` | `TextDecodeError` | `UnknownError` |
| ----------------- | :-----------------: | :-------------: | :-----------------: | :------------------: | :---------------: | :------------: |
| `readFile`        |          ✓          |        ✓        |          ✓          |                      |                   |       ✓        |
| `writeFile`       |          ✓          |        ✓        |          ✓          |                      |                   |       ✓        |
| `readTextFile`    |          ✓          |        ✓        |          ✓          |                      |         ✓         |       ✓        |
| `writeTextFile`   |          ✓          |        ✓        |          ✓          |                      |                   |       ✓        |
| `remove`          |          ✓          |        ✓        |                     |                      |                   |       ✓        |
| `createDirectory` |          ✓          |                 |                     |          ✓           |                   |       ✓        |
| `listDirectory`   |          ✓          |        ✓        |                     |          ✓           |                   |       ✓        |
| `stat`            |          ✓          |        ✓        |                     |                      |                   |       ✓        |

# References

[1] [Loading images](Image.md)
