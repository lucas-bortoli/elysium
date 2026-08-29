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

Elysium boots into a gallery of example programs, one per feature described
here, which is the quickest way to see any of this working ([5]).

Draw calls are batched. What a program draws during a draw handler doesn't
appear on screen call by call. That means later calls draw over earlier
ones where they overlap.

## Shapes

Beyond `fillRectangle` there is a vocabulary of shapes, and every one of
them comes in the two forms a shape can take: filled, or drawn as an
outline. So there is `fillCircle` and `strokeCircle`, `fillEllipse` and
`strokeEllipse`, `fillRectangle` and `strokeRectangle`,
`fillRoundedRectangle` and `strokeRoundedRectangle`, `fillPolygon` and
`strokePolygon`. `fillTriangle` is the three-cornered polygon named for how
often it's wanted.

The shapes that aren't closed are outlines only, since there's nothing for
a fill to land in: `drawLine` between two points, `drawPolyline` through a
list of them with its two ends left loose, and `drawArc` for a piece of a
circle's rim — the shape a cooldown meter or a ring segment is made of. The
shapes taking a list of points take them as `Vector2d` values from
`ely:math`, the same `{ x, y }` shape the pointer is reported in.

An outline straddles the line it's drawn along, spreading half its
thickness to either side, which is why a stroked rectangle and a filled one
at the same coordinates don't land on quite the same pixels. Coordinates
([4]) explains how to place a thin outline so it lands on whole pixels.

For the smallest thing there is, `setPixel` sets the one pixel a coordinate
falls inside, and `drawPixels` sets a whole list of them at once.

## Paths

Underneath, every shape above is a path: a line traced through the surface,
which can then be filled or drawn along. A program that wants a shape
Elysium doesn't name can trace one itself. `beginPath` starts a fresh one,
`moveTo` lifts the pen to a point without drawing, `lineTo` draws a
straight segment, `quadraticTo` and `cubicTo` draw curves that bend toward
control points without passing through them, and `closePath` shuts the
current loop. `fillPath` then fills what the line encloses and `strokePath`
draws along it. Filling leaves the path in place, so a shape can be filled
and then outlined without being described twice.

There is one path under construction at a time, and the named shapes each
describe a whole path of their own, so calling one replaces a path in
progress rather than adding to it.

Where a path's line crosses over itself, what counts as inside is a
question with two reasonable answers, and `fillPath` takes which one to use:
`"nonzero"` treats a region as inside if the line winds around it at all,
while `"evenodd"` alternates, so a shape traced inside another punches a
hole through it. Stroking has its own choices — how a line finishes at a
loose end, and how it turns a corner.

## Transforms and clipping

Rather than doing the arithmetic on every shape it draws, a program can
move the coordinate space itself. `pushTransform` shifts, scales or turns
everything drawn after it until the matching `popTransform`, and
`pushClip` confines everything drawn after it to a region until the
matching `popClip` — `pushClipPath` does the same with the current path
instead of a rectangle, for a region that isn't a box.

Both nest, and nesting is what makes them useful: a second transform
applies inside the first, so a camera can hold the outer one while an
object places itself within it, and a second clip narrows to the overlap
with the first, so drawing can never escape a region an enclosing clip
already confined it to. Popping more than was pushed is a mistake rather
than a no-op, and throws. Each frame starts with both cleared, so a draw
handler that fails part way through can't leave the next frame confined to
a region it never asked for.

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

In place of a font, `drawText` takes a set of options covering the layout
work every program otherwise writes for itself. `align` decides which edge
of the text the given coordinate names, so centring a label means naming
the point to centre it on rather than measuring it and subtracting.
`maxWidth` wraps the string to a width, breaking between words; a single
word too wide to fit still gets a line to itself and overruns it, since
there is no hyphenation. Line breaks in the string are honoured whether or
not it wraps, and `lineSpacing` opens up the gap between the resulting
lines.

`scale` draws the text larger, and only in whole multiples. Enlarging a
bitmap font by a whole number is just drawing each of its pixels as a
square block, so bigger text stays exactly as crisp as the font itself and
every pixel of it is still one palette color — which a fractional size
couldn't promise.

Because the fonts are fixed but their sizes aren't something a program
should assume, `measureText(text)` reports the pixel box a string will
occupy — the width of its widest line and the height of the whole block —
as a `Size2d`. It takes the same options as `drawText` and lays the text
out the same way, so what it reports is exactly the box `drawText` fills.
Unlike the drawing calls, this is a plain query and can be called from
anywhere, so a program can size a background rectangle before it ever
enters a draw handler.

# References

[1] [Per-frame ticking](Lifecycle.md)

[2] [Input](Input.md)

[3] [Loading images](Image.md)

[4] [Coordinates](Coordinates.md)

[5] [The examples browser](Examples.md)
