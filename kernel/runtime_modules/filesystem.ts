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
