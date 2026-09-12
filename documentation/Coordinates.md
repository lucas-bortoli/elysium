# Coordinates

Everything Elysium draws, and every position it reports back, lives in one
coordinate space. Programs never work in the window's real pixels.

The drawing surface is 720 logical pixels wide and 360 tall. `(0, 0)` is its
top-left corner; `x` grows to the right and `y` grows *downward*, so `(0, 359)`
is the bottom-left. A program shouldn't hard-code that size — `getWidth()`,
`getHeight()` and `getSize2d()` report it, and it may change.

That surface is what a program draws on, but not what the screen shows. Elysium
presents it enlarged by a whole-number factor, every logical pixel becoming a
square block of real ones, so a picture stays sharp and blocky instead of being
smoothed. `setScale` changes that factor. Nothing a program draws or measures
changes with it: a rectangle 10 wide is 10 logical pixels wide at every scale,
and the pointer reports where it is on the logical surface, not on the physical
window. Scale is a presentation detail, and the only place it becomes visible
is the size of the window on the desktop.

## Where a coordinate falls

Coordinates name the *corners* of the pixel grid, not the pixels themselves.
`(0, 0)` is the outer corner of the very first pixel, and that first pixel
occupies the square between `(0, 0)` and `(1, 1)` — its middle is at
`(0.5, 0.5)`. So a rectangle at `(10, 10)` that is 5 by 5 covers exactly the
five-by-five block of pixels starting at the eleventh column and row, with no
part of it bleeding into a sixth.

Coordinates are not required to be whole numbers, but the surface only has
whole pixels to fill, and Elysium never softens an edge to suggest otherwise —
there is no anti-aliasing anywhere. A pixel is painted when its middle lies
inside the shape being drawn, and left alone when it doesn't. Edges are always
hard, and a shape given fractional coordinates simply lands on whichever pixels
its outline happens to cover.

This has one consequence worth knowing before it surprises you. A thin line
straddles the path it's drawn along, spreading half its thickness to either
side. A line of thickness 1 drawn straight down `x = 10` therefore covers the
half-pixel on each side of that grid line, and each pixel it touches is
half-covered — right at the boundary of the rule above. To get one crisp
column of pixels, draw down the middle of a pixel instead: `x = 10.5`. The same
goes for the outline of a rectangle, which is why a stroked rectangle at whole
coordinates and a filled one at the same coordinates don't line up pixel for
pixel. Even thicknesses have the opposite habit and sit cleanly on whole
coordinates.

## What a shape's position means

A shape that has a natural corner is placed by its top-left one: rectangles,
rounded rectangles, images, and the box a line of text occupies. A shape built
around a middle is placed by that middle: circles, ellipses, and arcs.

Angles are in radians. Zero points along `+x`, to the right, and an angle
increases toward `+y` — downward — so on screen an increasing angle turns
clockwise. An arc sweeps from its start angle to its end angle in that same
direction. This is a consequence of `y` growing downward and is worth
remembering when porting an equation written for a `y`-up space, where the same
angles turn the other way.

Nothing has to fit on the surface. A shape may sit partly or entirely outside
it, at negative coordinates or past the far edge; whatever falls outside is
discarded and the rest is drawn. Drawing off-surface is a normal thing to do,
not an error, and it's how a program scrolls a world larger than the screen
past a fixed window.

## Moving the space

A program can move the coordinate space itself rather than doing the arithmetic
on every shape it draws. Pushing a transform — a shift, a scale, a rotation, or
a combination — changes how the coordinates handed to subsequent drawing calls
map onto the surface, until it's popped again. Transforms nest: pushing a
second one applies on top of the first, so a program can hold a camera in the
outer transform and position one object within it in the inner one.

Clipping works the same way and stacks the same way. Pushing a clip region
confines everything drawn afterwards to it, and pushing a second one confines
drawing to the overlap of the two — a clip can only ever narrow what's already
in effect, never widen it. Because a clip region is itself given in the
coordinate space current when it's pushed, a clip pushed inside a transform
moves with it.

Text is the one thing that doesn't follow a transform completely. Where a line
of text is placed does move with the current transform, and text is clipped
like everything else, but the glyphs themselves are never rotated or scaled:
they're drawn from fixed bitmaps and stay upright and pixel-crisp. Text under a
rotating camera moves around the screen without tipping over.

# References

[1] [The Framebuffer](Framebuffer.md)

[2] [Loading images](Image.md)

[3] [Input](Input.md)
