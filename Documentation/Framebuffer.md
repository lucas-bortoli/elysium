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
import { Color, addDrawHandler, clearScreen, fillRectangle, getWidth } from "ely:framebuffer";
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

Draw calls are batched. What a program draws during a draw handler doesn't
appear on screen call by call. That means later calls draw over earlier
ones where they overlap.

# References

[1] [Per-frame ticking](Lifecycle.md)
