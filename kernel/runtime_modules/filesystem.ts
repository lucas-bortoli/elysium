import { type Option, hasValue } from "ely:container";

/** Thrown when a userland filesystem path is given relative instead of
 * absolute. Every such path must start with `/` and is resolved against the
 * userland root.
 * `import.meta.directoryName`/`fileName` are how a module finds its own location
 * to build an absolute path from. */
export class RelativePathError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RelativePathError";
  }
}

export const sep = "/";

function toPosixPath(path: string): string {
  return path.replace(/\\/g, sep);
}

/** Resolves `.`/`..` segments and collapses redundant slashes. The result is
 * always rooted at `/`; a `..` that would go above the root is dropped
 * rather than escaping it. */
export function normalize(path: string): string {
  const parts = toPosixPath(path).split(sep);
  const stack: string[] = [];

  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (stack.length > 0) stack.pop();
    } else {
      stack.push(part);
    }
  }

  return sep + stack.join(sep);
}

/** Joins path segments into one and normalizes the result. */
export function join(...paths: string[]): string {
  return normalize(paths.join(sep));
}

/** The final segment of a path, with `ext` stripped from the end if
 * present. Trailing slashes are ignored. */
export function extractBaseName(path: string, ext: string = ""): string {
  path = toPosixPath(path);
  let end = path.length;
  while (end > 1 && path[end - 1] === sep) end--;
  path = path.slice(0, end);

  const base = path.substring(path.lastIndexOf(sep) + 1);
  if (ext && base.endsWith(ext)) {
    return base.slice(0, -ext.length);
  }
  return base;
}

/** The path up to, but not including, the final segment. Trailing slashes
 * are ignored, and the root's own parent is itself. */
export function extractDirectoryName(path: string): string {
  path = toPosixPath(path);
  let end = path.length;
  while (end > 1 && path[end - 1] === sep) end--;
  path = path.slice(0, end);

  if (path === sep) return sep;

  const idx = path.lastIndexOf(sep);
  if (idx === -1) return ".";
  if (idx === 0) return sep;
  return path.slice(0, idx);
}

/** The final segment's extension, including the leading `.`, or `""` if it
 * has none. A leading dot, as in `.bashrc`, is not itself an extension. */
export function extractExtension(path: string): string {
  const base = extractBaseName(path);
  const dotIndex = base.lastIndexOf(".");
  return dotIndex > 0 ? base.substring(dotIndex) : "";
}

/** Builds an absolute path by resolving segments right-to-left, stopping at
 * the first one that's already absolute. Equivalent to `join` when none of
 * the segments are absolute. */
export function resolve(...paths: string[]): string {
  let resolvedPath = "";
  let absolute = false;

  for (let i = paths.length - 1; i >= 0 && !absolute; i--) {
    const path = toPosixPath(paths[i] ?? "");
    if (!path) continue;

    resolvedPath = `${path}/${resolvedPath}`;
    absolute = path.startsWith(sep);
  }

  return normalize(resolvedPath);
}

/** Replaces characters outside `[a-zA-Z0-9_.-]` with `_` and truncates to
 * `maxLength`. `.` and `..` are replaced outright, since every one of their
 * characters is otherwise allowed and they would pass through unchanged and
 * remain usable for directory traversal. */
export function sanitizeName(
  filename: string,
  maxLength: number = 255,
): string {
  if (filename === "." || filename === "..") {
    return "_".repeat(filename.length);
  }

  const sanitized = filename.replace(/[^a-zA-Z0-9_\-.]/g, "_");
  return sanitized.slice(0, maxLength);
}

declare function __fs_read_file(path: string, offset: number, length: number): Uint8Array;
declare function __fs_write_file(
  path: string,
  data: Uint8Array,
  offset: number,
  truncate: boolean,
): void;
declare function __fs_read_text_file(path: string): string;
declare function __fs_write_text_file(path: string, text: string): void;
declare function __fs_remove(path: string): void;
declare function __fs_create_directory(path: string): void;
declare function __fs_list_directory(
  path: string,
): ({ kind: "File"; size: number; path: string } | { kind: "Directory"; path: string })[];
declare function __fs_stat(
  path: string,
): { kind: "File"; size: number; path: string } | { kind: "Directory"; path: string };

// The classes below are a shared vocabulary reused across every function —
// mirroring `std::io::ErrorKind` being one shared set of variants for every
// Rust filesystem call, rather than a bespoke error type per call site. Each
// function's exported error type (`ReadFileError`, `WriteFileError`, ...) is
// just the union of whichever of these it can actually throw.

/** A path, or a component of it, doesn't exist. */
export class NotFoundError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NotFoundError";
  }
}

/** A file operation (read, write, or removing a file) found a directory
 * instead. */
export class IsADirectoryError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "IsADirectoryError";
  }
}

/** A directory operation (`createDirectory`, `listDirectory`) is blocked by
 * an existing non-directory somewhere along the path. */
export class NotADirectoryError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NotADirectoryError";
  }
}

/** `readTextFile`'s bytes aren't valid UTF-8. */
export class TextDecodeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TextDecodeError";
  }
}

/** Anything else the underlying operation failed with — permission denied,
 * disk full, and so on. `.message` carries the full underlying OS error
 * text plus the operation and path involved, so it stays useful for
 * debugging even though it isn't one of the specifically-typed cases
 * above. */
export class UnknownError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "UnknownError";
  }
}

/** Parses the `"<TAG>: "` prefix `kernel/filesystem.rs` attaches to every
 * native error message and throws the matching shared class above, with
 * everything after the prefix — operation, path, and full OS error text —
 * intact as `.message`. Falls back to `UnknownError` for an
 * unrecognized/missing tag. */
function throwTagged(err: unknown): never {
  const raw = err instanceof Error ? err.message : String(err);
  const separator = raw.indexOf(": ");
  const tag = separator === -1 ? "" : raw.slice(0, separator);
  const message = separator === -1 ? raw : raw.slice(separator + 2);

  switch (tag) {
    case "NOT_FOUND":
      throw new NotFoundError(message);
    case "IS_A_DIRECTORY":
      throw new IsADirectoryError(message);
    case "NOT_A_DIRECTORY":
      throw new NotADirectoryError(message);
    case "INVALID_UTF8":
      throw new TextDecodeError(message);
    default:
      throw new UnknownError(message);
  }
}

/** Every case `readFile` can actually throw natively (beyond
 * `RelativePathError`, thrown before any native call). */
export type ReadFileError = NotFoundError | IsADirectoryError | UnknownError;

/** Every case `readTextFile` can actually throw natively — like
 * `ReadFileError`, plus `TextDecodeError` for non-UTF-8 bytes. */
export type ReadTextFileError =
  | NotFoundError
  | IsADirectoryError
  | TextDecodeError
  | UnknownError;

/** Every case `writeFile` can actually throw natively. */
export type WriteFileError = NotFoundError | IsADirectoryError | UnknownError;

/** Every case `writeTextFile` can actually throw natively. */
export type WriteTextFileError = NotFoundError | IsADirectoryError | UnknownError;

/** Every case `remove` can actually throw natively. */
export type DeleteError = NotFoundError | UnknownError;

/** Every case `createDirectory` can actually throw natively. */
export type CreateDirectoryError = NotADirectoryError | UnknownError;

/** Every case `listDirectory` can actually throw natively. */
export type ListDirectoryError = NotFoundError | NotADirectoryError | UnknownError;

/** Every case `stat` can actually throw natively. */
export type StatError = NotFoundError | UnknownError;

/** A byte range within a file, used by `readFile`/`writeFile` to operate on
 * part of a file instead of the whole thing. Both fields are optional:
 * `offset` defaults to `0`, `length` defaults to "the rest of the file" for
 * `readFile` and "all of `data`" for `writeFile`. */
export interface ByteRange {
  offset?: number;
  length?: number;
}

/** The result of `stat`, and of each entry `listDirectory` returns.
 * `path` is the entry's own absolute, virtual path — for `listDirectory`,
 * that's the listed directory joined with the entry's name, not just the
 * name by itself. */
export type EntryStat =
  | { readonly kind: "File"; readonly size: number; readonly path: string }
  | { readonly kind: "Directory"; readonly path: string };

function assertAbsolute(path: string): void {
  if (!path.startsWith("/")) {
    throw new RelativePathError(`${path} is not an absolute path`);
  }
}

/** Reads the whole file at `path`, or — if `range` is given — only the
 * bytes from `range.offset` (default `0`) spanning at most `range.length`
 * bytes (default: to the end of the file). `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path` doesn't exist.
 * @throws {IsADirectoryError} if `path` names a directory.
 * @throws {UnknownError} on any other failure. */
export function readFile(path: string, range?: Option<ByteRange>): Uint8Array {
  assertAbsolute(path);
  const offset = hasValue(range) ? (range.offset ?? 0) : 0;
  const length = hasValue(range) && range.length !== undefined ? range.length : -1;

  try {
    return __fs_read_file(path, offset, length);
  } catch (err) {
    throwTagged(err);
  }
}

/** Writes `data` to `path`. With no `range`, the file is truncated and
 * replaced entirely. With `range`, the file is patched in place: writing
 * starts at `range.offset` (default `0`), and bytes outside the written
 * span are left untouched. `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path`'s parent directory doesn't exist.
 * @throws {IsADirectoryError} if `path` names a directory.
 * @throws {UnknownError} on any other failure. */
export function writeFile(path: string, data: Uint8Array, range?: Option<ByteRange>): void {
  assertAbsolute(path);
  const offset = hasValue(range) ? (range.offset ?? 0) : 0;
  const length = hasValue(range) && range.length !== undefined ? range.length : data.length;
  const truncate = !hasValue(range);
  const slice = length === data.length ? data : data.slice(0, length);

  try {
    __fs_write_file(path, slice, offset, truncate);
  } catch (err) {
    throwTagged(err);
  }
}

// Userland has no `TextEncoder`/`TextDecoder` yet (planned as a polyfill on
// a separate branch), so these two do their own UTF-8 conversion natively
// rather than layering on top of `readFile`/`writeFile`. Once that polyfill
// lands, these should likely become thin wrappers around them instead.

/** Reads the file at `path` as UTF-8 text. `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path` doesn't exist.
 * @throws {IsADirectoryError} if `path` names a directory.
 * @throws {TextDecodeError} if `path`'s bytes aren't valid UTF-8.
 * @throws {UnknownError} on any other failure. */
export function readTextFile(path: string): string {
  assertAbsolute(path);
  try {
    return __fs_read_text_file(path);
  } catch (err) {
    throwTagged(err);
  }
}

/** Writes `text` to `path`, truncating and replacing it entirely. `path`
 * must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path`'s parent directory doesn't exist.
 * @throws {IsADirectoryError} if `path` names a directory.
 * @throws {UnknownError} on any other failure. */
export function writeTextFile(path: string, text: string): void {
  assertAbsolute(path);
  try {
    __fs_write_text_file(path, text);
  } catch (err) {
    throwTagged(err);
  }
}

/** Removes the file or directory at `path`. A directory is removed
 * recursively, along with everything inside it. `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path` doesn't exist.
 * @throws {UnknownError} on any other failure. */
export function remove(path: string): void {
  assertAbsolute(path);
  try {
    __fs_remove(path);
  } catch (err) {
    throwTagged(err);
  }
}

/** Creates the directory at `path`, along with any missing parent
 * directories. `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotADirectoryError} if a non-directory already exists somewhere
 * along `path`.
 * @throws {UnknownError} on any other failure. */
export function createDirectory(path: string): void {
  assertAbsolute(path);
  try {
    __fs_create_directory(path);
  } catch (err) {
    throwTagged(err);
  }
}

/** Lists the entries directly inside the directory at `path`, each as an
 * `EntryStat` whose `path` is that entry's own absolute path (the listed
 * directory joined with its name). `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path` doesn't exist.
 * @throws {NotADirectoryError} if `path` names a file.
 * @throws {UnknownError} on any other failure. */
export function listDirectory(path: string): EntryStat[] {
  assertAbsolute(path);
  try {
    return __fs_list_directory(path);
  } catch (err) {
    throwTagged(err);
  }
}

/** Reports whether `path` is a file or a directory, its size in bytes if
 * it's a file, and its own absolute path. `path` must be absolute.
 * @throws {RelativePathError} if `path` isn't absolute.
 * @throws {NotFoundError} if `path` doesn't exist.
 * @throws {UnknownError} on any other failure. */
export function stat(path: string): EntryStat {
  assertAbsolute(path);
  try {
    return __fs_stat(path);
  } catch (err) {
    throwTagged(err);
  }
}
