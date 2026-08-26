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
export function sanitizeName(filename: string, maxLength: number = 255): string {
  if (filename === "." || filename === "..") {
    return "_".repeat(filename.length);
  }

  const sanitized = filename.replace(/[^a-zA-Z0-9_\-.]/g, "_");
  return sanitized.slice(0, maxLength);
}
