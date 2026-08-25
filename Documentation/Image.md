# Loading images

A program can load a picture off disk and put it on the Framebuffer ([1])
alongside whatever it draws with `clearScreen`/`fillRectangle`. `ely:image`
loads a PNG file into an `Image`; `ely:framebuffer`'s `drawImage` puts it on
screen.

```ts
import { addDrawHandler, drawImage } from "ely:framebuffer";
import { loadImage } from "ely:image";

const sprite = loadImage("sprite.png");

addDrawHandler(() => {
  drawImage(sprite, 100, 80);
});
```

Loading is synchronous, the same way importing another module is — a
program never has to `await` an image before drawing it, or handle it
still being "in flight" partway through startup.

An image is still bound by the same fixed, curated palette everything else
on the Framebuffer is drawn with. Loading a PNG doesn't hand its colors to
the screen unchanged: every pixel's color is snapped, once, to whichever
palette shade is closest to it, the moment `loadImage` reads the file —
never again after that, so drawing the same image every frame costs nothing
extra for this. Transparency is untouched by this — only color is
quantized, so a PNG's alpha channel comes through exactly as it was drawn.
The practical effect is that a picture brought in from outside Elysium
ends up looking like it was always drawn from the same palette every other
program on the system uses, rather than introducing its own arbitrary
colors.

`drawImage(image, x, y)` places `image`'s top-left corner at `(x, y)`, in
the same logical coordinate space `fillRectangle` and the pointer both use,
at the image's natural pixel size — there's no scaling or rotation. Like
`clearScreen`/`fillRectangle`, it only takes effect from inside a currently
running draw handler.

`loadImage` reads a path relative to the program's own directory, never
the process's actual working directory, and a path can't be made to point
anywhere outside that directory — a program's isolation extends to what it
can read off disk through this call, the same as everything else about how
programs run in their own VM. `loadImage` throws an `ImageLoadError` if the
file doesn't exist, escapes the program's directory, or isn't a PNG file
Elysium can decode.

An image loaded this way stays loaded for as long as the program keeps
running, whether or not the program still holds a reference to it — this
is unlike a value that gets garbage collected once nothing points to it
anymore. A program that's done with an image can free it early by calling
`unloadImage`; anything it never explicitly unloads is freed automatically
once the program itself exits.

# References

[1] [The Framebuffer](Framebuffer.md)
