# The Framebuffer

Elysium's kernel provides drawing machinery to programs as a module a program
imports explicitly. The Framebuffer is a drawing surface a program can put pictures on.

Elysium ships with one fixed, curated palette, and every color a program
can draw with is one of `Color`'s named entries, exported from
`ely:framebuffer`. Constraining every program to the same palette keeps
what gets drawn visually consistent across, the way a shared system theme
would, instead of every program inventing its own arbitrary colors.

A program doesn't open its own drawing loop to use the framebuffer. Instead
it registers a draw handler, a function the kernel calls once every frame
for as long as it stays registered: `addDrawHandler` returns an id, and
`removeDrawHandler(id)` unregisters it. Drawing calls only take effect from
inside a currently running draw handler — calling `clearScreen` or
`fillRectangle` from anywhere else (module top level, a timer, a ticker
registered through `ely:lifecycle` ([1])) throws a `DrawOutsideHandlerError`
rather than silently doing nothing.

```ts
import {
  Color,
  addDrawHandler,
  clearScreen,
  fillRectangle,
  getWidth,
} from "ely:framebuffer";
import { addUpdateTicker } from "ely:lifecycle";

let x = 0;

addUpdateTicker((dt) => {
  x += 200 * dt;
  if (x > getWidth()) x = -100;
});

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  fillRectangle(x, 130, 100, 100, Color.Amber400);
});
```

A program that needs to know the framebuffer's logical size, rather than
assuming a resolution, can ask for it: `getWidth()` and `getHeight()` return
it as separate numbers, and `getSize2d()` returns it as one `Size2d` — a
plain `{ width, height }` shape exported from `ely:math`, the module
Elysium's geometry-returning APIs share their point/size/rectangle types
from.

Where a coordinate falls on that surface, which corner or centre a shape is
positioned by, and how angles are measured are all one shared set of rules
every drawing call follows ([4]).

Draw calls are batched. What a program draws during a draw handler doesn't
appear on screen call by call. That means later calls draw over earlier
ones where they overlap.

## Text

Elysium draws text with bitmap fonts that belong to the system, not to the
program. Just as every color is a named palette entry, every font is one of
a small fixed set the kernel carries — `Font`, exported from
`ely:framebuffer`, with `Font.Cozette` as the default a program gets
when it names none. A program can't load or embed a font of its own; more
built-in fonts may be added over time, and a program selects one the same
way it selects a color.

`drawText(x, y, text, color)` draws a string with its top-left corner at
`(x, y)` in a palette color, and like the other drawing calls it only takes
effect from inside a running draw handler. Text is placed by its box, not
its baseline: the kernel positions each glyph from the chosen font's own
metrics, so a program never has to know a font's height or where its
baseline sits. A character the font has no glyph for still advances the
cursor by the font's default width, leaving a blank cell rather than
collapsing the layout.

Because the fonts are fixed but their sizes aren't something a program
should assume, `measureText(text)` reports the pixel box a string will
occupy — its total width and the font's line height — as a `Size2d`. Unlike
the drawing calls, this is a plain query and can be called from anywhere, so
a program can lay out a line, right-align it, or size a background rectangle
before it ever enters a draw handler.

# References

[1] [Per-frame ticking](Lifecycle.md)

[2] [Input](Input.md)

[3] [Loading images](Image.md)

[4] [Coordinates](Coordinates.md)
