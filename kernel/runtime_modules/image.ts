// Loading pictures off disk. Every pixel a program loads gets snapped to
// the kernel's fixed color palette (see `ely:framebuffer`'s `Color`) the
// same way `fillRectangle`/`clearScreen` are already constrained to it —
// transparency is preserved, only color is quantized.

import { RelativePathError } from "ely:filesystem";

declare function __image_load(path: string): number;
declare function __image_width(id: number): number;
declare function __image_height(id: number): number;
declare function __image_unload(id: number): void;

/** Opaque handle to a loaded image, returned by `loadImage`. */
export type ImageId = number;

/** A loaded, palette-quantized picture, ready to be drawn with
 * `ely:framebuffer`'s `drawImage`. */
export interface Image {
  readonly id: ImageId;
  readonly width: number;
  readonly height: number;
}

export class ImageLoadError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ImageLoadError";
  }
}

/** Loads the PNG at `path`, an absolute path resolved against the userland
 * root — never the process's working directory, and never anywhere outside
 * that root. `path` must start with `/`; a module builds one relative to
 * its own location with `import.meta.directoryName`/`fileName`. Every pixel's
 * color is snapped to its nearest shade in the kernel's fixed palette;
 * alpha is left untouched. Throws `RelativePathError` if `path` doesn't
 * start with `/`, or `ImageLoadError` if it doesn't exist, escapes the
 * userland root, or isn't a decodable PNG. */
export function loadImage(path: string): Image {
  if (!path.startsWith("/")) {
    throw new RelativePathError(`${path} is not an absolute path`);
  }

  try {
    const id = __image_load(path);
    return { id, width: __image_width(id), height: __image_height(id) };
  } catch (err) {
    throw new ImageLoadError(err instanceof Error ? err.message : String(err));
  }
}

/** Frees a loaded image early. An image not explicitly unloaded stays
 * loaded for the lifetime of the program, not until nothing references it
 * anymore — dropping every reference to an `Image` does not free it. */
export function unloadImage(image: Image | ImageId): void {
  __image_unload(typeof image === "number" ? image : image.id);
}
