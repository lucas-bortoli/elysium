# Loading images

A program can load a picture off disk and put it on the Framebuffer ([1])
alongside whatever it draws with `clearScreen`/`fillRectangle`. `ely:image`
loads a PNG file into an `Image`; `ely:framebuffer`'s `drawImage` puts it on
screen.

```ts
import { addDrawHandler, drawImage } from "ely:framebuffer";
import { loadImage } from "ely:image";

const sprite = loadImage(`${import.meta.directoryName}/sprite.png`);

addDrawHandler(() => {
  drawImage(sprite, 100, 80);
});
```

An image is still bound by the same fixed, curated palette everything else
on the Framebuffer is drawn with. Loading a PNG doesn't hand its colors to
the screen unchanged: every pixel's color is snapped, once, to whichever
palette shade is closest to it, the moment `loadImage` reads the file —
never again after that, so drawing the same image every frame costs nothing
extra for this. Transparency is snapped at the same moment, to all or
nothing: a pixel more than half transparent disappears entirely, and one
less than half transparent becomes solid. So a picture never half-shows
whatever sits behind it, and every pixel it does put on screen is an exact
palette shade rather than a blend of one — the same hard-edged rule the
rest of the Framebuffer draws by. The practical effect is that a picture
brought in from outside Elysium ends up looking like it was always drawn
from the same palette every other program on the system uses, rather than
introducing its own arbitrary colors.

A PNG that relies on soft, feathered edges will therefore come out with
those edges made crisp. A picture meant for Elysium is best authored with
hard-edged transparency to begin with, so what a program sees on screen is
what it drew.

`drawImage(image, x, y)` places `image`'s top-left corner at `(x, y)`, in
the same logical coordinate space `fillRectangle` and the pointer both use
([2]), at the image's natural pixel size. Like `clearScreen`/`fillRectangle`,
it only takes effect from inside a currently running draw handler.

Given options it can draw less than the whole image, and place it more
freely. Naming a rectangle within the image draws only that part, which is
how a program keeps every frame of an animation, or every tile of a
tileset, in one file and picks out the one it wants. It can also be drawn
at a multiple of its natural size, mirrored left to right or top to bottom,
and — with `drawImageRotated` — turned about a point within it, which is
usually its middle.

However an image is turned or resized, it's sampled without smoothing:
every pixel drawn is one whole pixel of the original, never a blend of
neighbouring ones, so a resized image is still made only of palette colors.
Whole-number sizes keep it looking like the picture it started as; a
fractional one lands the original's pixels unevenly, some drawn wider than
others.

`loadImage` reads an absolute path against the root of the whole userland
tree — the same tree every program lives under — never the process's
actual working directory, and a path can't be made to point anywhere
outside that tree. This means `path` must start with `/`; `loadImage`
throws a `RelativePathError` if it doesn't. A program builds a path
relative to its own location using `import.meta.directoryName` (its own
directory) or `import.meta.fileName` (its own file), both already
expressed as absolute userland paths — `${import.meta.directoryName}/sprite.png`
reaches a picture checked in alongside the program's own source, the same
way a relative import would, just spelled out explicitly. `loadImage`
throws an `ImageLoadError` if the file doesn't exist, escapes the userland
tree, or isn't a PNG file Elysium can decode.

An image loaded this way stays loaded for as long as the program keeps
running, whether or not the program still holds a reference to it — this
is unlike a value that gets garbage collected once nothing points to it
anymore. A program that's done with an image can free it early by calling
`unloadImage`; anything it never explicitly unloads is freed automatically
once the program itself exits.

# References

[1] [The Framebuffer](Framebuffer.md)

[2] [Coordinates](Coordinates.md)
