# The Framebuffer

Elysium's kernel provides drawing machinery to programs as a module a program
imports explicitly. The Framebuffer is a drawing surface a program can put pictures on.

Elysium ships with one fixed, curated palette, and every color a program
can draw with is one of its named constants exported from `ely:framebuffer`.
Constraining every program to the same palette keeps what gets drawn visually
consistent across, the way a shared system theme would, instead of every program
inventing its own arbitrary colors.

A program doesn't open its own drawing loop to use the framebuffer. Instead
it exports up to two functions from its entry module, and the kernel calls
into them itself, every frame, for as long as the program is running:
`update(dt)` first, with `dt` the time in seconds since the previous frame,
then `draw()`, where the drawing calls belong. Both are optional — a
program that never exports either just never gets called back, and nothing
about the framebuffer is required for a program to run at all.

```ts
import {
  clearScreen,
  fillRectangle,
  SLATE_900,
  AMBER_400,
} from "ely:framebuffer";

let x = 0;

export function update(dt: number) {
  x += 200 * dt;
  if (x > 1280) x = -100;
}

export function draw() {
  clearScreen(SLATE_900);
  fillRectangle(x, 300, 100, 100, AMBER_400);
}
```

Draw calls are batched. What a program draws during `draw()` doesn't appear on screen call by call. That means later calls draw over earlier ones where they
overlap.
