# Graphics

Elysium's kernel provides drawing machinery to programs as a module a
program imports explicitly, `ely:graphics`. What a program draws onto is a
*surface*: a picture held in memory that drawing operations change in place
and that keeps whatever has been drawn on it until something draws over it.
The screen is one such surface. A program can make more.

Elysium ships with one fixed, curated palette, and every colour a program
can draw with is one of `Color`'s named entries, exported from
`ely:graphics`. Constraining every program to the same palette keeps what
gets drawn visually consistent across programs, the way a shared system
theme would, instead of every program inventing its own arbitrary colours.

## Surfaces

A surface is retained, not replayed. `surface.fillRectangle(...)` changes
that surface's pixels the moment it is called and they stay changed; there
is no list of pending commands and no point at which a frame is "finished".
Drawing the same rectangle twice paints it twice; a later call draws over
an earlier one where they overlap. A program that wants a clean surface
each frame clears it itself, with `surface.clear(colour)` — nothing does
that for it.

The screen is the surface the kernel puts in front of the viewer. It is
presented once per tick, after every running process has had its turn, so
whatever every process has drawn onto it by then is what the viewer sees.
`screen`, exported from `ely:graphics`, is that surface, ready to draw on:

```ts
import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";

let x = 0;

addUpdateTicker((dt) => {
  x += 200 * dt;
  if (x > screen.width) x = -100;

  screen.clear(Color.Slate900);
  screen.fillRectangle(x, 130, 100, 100, Color.Amber400);
});
```

There is no separate draw loop and no gate around drawing calls. A program
draws whenever it holds a surface — usually from an update ticker
([1]), which the system already calls once a frame, but a program is free
to draw from a timer, a message handler, or anywhere else. `screen.width`
and `screen.height` give the screen's logical size, and `screen.size`
gives it as one `{ width, height }` value — the `Size2d` shape from
`ely:math`, the module Elysium's geometry-returning APIs share their
point/size/rectangle types from — so a program never has to assume a
resolution.

Where a coordinate falls on a surface, which corner or centre a shape is
positioned by, and how angles are measured are one shared set of rules
every drawing call follows ([4]). Elysium boots into a gallery of example
programs, one per feature described here, which is the quickest way to see
any of this working ([5]).

## Making, sharing and destroying surfaces

`createSurface(width, height)` makes a new surface, fully transparent, and
returns a *handle* for it: a plain number. `useSurface(handle)` turns a
handle — one a program made itself, or one another process sent it — into
something it can draw on. `screen` is the one surface a program gets
already turned into a drawable object, over the handle the screen always
has.

A handle is an ordinary number, so it crosses between processes like any
other value a message carries ([6]). A window manager written as its own
process can make a surface for a window, send the handle to the client
process, and both then draw onto the same pixels. Neither process holds a
private copy; a surface is one picture reached by one number.

A surface stays alive as long as any live process holds it — the process
that created it, plus any process that has called `useSurface` on the
handle. `destroySurface(handle)` drops the calling process's hold. When a
process ends, every hold it had is dropped for it. A surface with no holders
left goes away and its handle stops naming anything; `useSurface` on a
handle like that throws. The screen is never reclaimed. There is a ceiling
on how much surface memory can be held at once across all of them, and
`createSurface` throws rather than exceed it.

## Drawing a surface onto another

A surface can be drawn onto another surface exactly as an image is:
`target.drawSurface(sourceHandle, x, y)` puts the whole of one surface at a
point on another, and `target.drawSurfaceRotated(...)` turns it about a
point as it goes. Both take the same options an image takes — a source
rectangle to crop to, a scale, a flip. This is how a compositor assembles
a screen from window surfaces, and how a program builds an expensive
picture once on an offscreen surface and then stamps it about cheaply. A
surface cannot be drawn onto itself; asking to throws.

## Resizing

A surface is not fixed at the size it was made. `surface.resize(width,
height)` reallocates it, keeping whatever was drawn anchored at the
top-left corner and leaving any newly exposed area transparent, so a resize
partway through a frame doesn't flash. It empties the surface's transform
and clip stacks: a clip is a region in the surface's own coordinates, and
there is no honest way to carry one over to different bounds. Resizing to
the size it already is does nothing.

Resizing the screen is how the window's logical resolution changes — the
`720` by `360` a program starts with is a starting size, not a fixed one.

## Shapes

Beyond `fillRectangle` there is a vocabulary of shapes on every surface,
and each comes in the two forms a shape can take: filled, or drawn as an
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

Underneath, every shape above is a path: a line traced through a surface,
which can then be filled or drawn along. A program that wants a shape
Elysium doesn't name can trace one itself. `beginPath` starts a fresh one,
`moveTo` lifts the pen to a point without drawing, `lineTo` draws a
straight segment, `quadraticTo` and `cubicTo` draw curves that bend toward
control points without passing through them, and `closePath` shuts the
current loop. `fillPath` then fills what the line encloses and `strokePath`
draws along it. Filling leaves the path in place, so a shape can be filled
and then outlined without being described twice.

There is one path under construction per program, not one per surface — a
program describes a path, then fills, strokes or clips one surface with it.
The named shapes each describe a whole path of their own, so calling one
replaces a path in progress rather than adding to it.

Where a path's line crosses over itself, what counts as inside is a
question with two reasonable answers, and `fillPath` takes which one to
use: `"nonzero"` treats a region as inside if the line winds around it at
all, while `"evenodd"` alternates, so a shape traced inside another punches
a hole through it. Stroking has its own choices — how a line finishes at a
loose end, and how it turns a corner.

## Transforms and clipping

Rather than doing the arithmetic on every shape it draws, a program can
move a surface's coordinate space itself. `pushTransform` shifts, scales or
turns everything drawn after it until the matching `popTransform`, and
`pushClip` confines everything drawn after it to a region until the
matching `popClip` — `pushClipPath` does the same with the current path
instead of a rectangle, for a region that isn't a box.

Both nest, and nesting is what makes them useful: a second transform
applies inside the first, so a camera can hold the outer one while an
object places itself within it, and a second clip narrows to the overlap
with the first, so drawing can never escape a region an enclosing clip
already confined it to.

The transform and clip stacks belong to the surface and persist between
calls — they are not reset each frame. A program that clears and redraws a
surface every frame should balance every push with a pop within that pass,
or the leftover state carries into the next one. Popping more than was
pushed leaves the surface in its default, unshifted, unclipped state rather
than failing. When two processes share a surface, they share its stacks
too, and keeping them balanced is theirs to arrange.

## Text

Elysium draws text with bitmap fonts that belong to the system, not to the
program. Just as every colour is a named palette entry, every font is one
of a small fixed set the kernel carries — `Font`, exported from
`ely:graphics`, with `Font.Cozette` as the default a program gets when it
names none. A program can't load or embed a font of its own; more built-in
fonts may be added over time, and a program selects one the same way it
selects a colour.

`surface.drawText(x, y, text, colour)` draws a string with its top-left
corner at `(x, y)` in a palette colour. Text is placed by its box, not its
baseline: the kernel positions each glyph from the chosen font's own
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
lines. `scale` draws the text larger, and only in whole multiples:
enlarging a bitmap font by a whole number is just drawing each of its
pixels as a square block, so bigger text stays exactly as crisp as the
font itself and every pixel of it is still one palette colour — which a
fractional size couldn't promise.

`surface.measureText(text)` reports the pixel box a string will occupy —
the width of its widest line and the height of the whole block — as a
`Size2d`. It takes the same options as `drawText` and lays the text out the
same way, so what it reports is exactly the box `drawText` fills, which is
what makes it safe to size a background rectangle from.

# References

[1] [Per-frame ticking](Lifecycle.md)

[2] [Input](Input.md)

[3] [Loading images](Image.md)

[4] [Coordinates](Coordinates.md)

[5] [The examples browser](Examples.md)

[6] [Message passing between processes](Multitasking.md)
