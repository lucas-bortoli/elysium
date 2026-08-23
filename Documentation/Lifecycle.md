# Per-frame ticking

A program that wants to run logic every frame, independent of drawing,
registers an update ticker through `ely:loop` rather than exporting
anything from its entry module: `addUpdateTicker(handler)` calls
`handler(dt)` once per frame, with `dt` the time in seconds since the
previous frame, for as long as the ticker stays registered, and returns an
id `removeUpdateTicker(id)` can later use to stop it. `getDeltaTime()`
returns that same `dt`, for code that isn't itself a ticker callback but
still wants to know how much time the last frame took.

```ts
import { addUpdateTicker, removeUpdateTicker } from "ely:loop";

let elapsed = 0;

const id = addUpdateTicker((dt) => {
  elapsed += dt;
  if (elapsed > 5) removeUpdateTicker(id);
});
```

Under the hood this is built entirely on `requestAnimationFrame` ([1]): a
ticker is really a callback that reschedules itself for the next frame
every time it runs, with `ely:loop` doing that rescheduling and the
delta-time bookkeeping so a program doesn't have to. Drawing has its own,
separate per-frame registration (`addDrawHandler`, from `ely:framebuffer`
([2])) built the same way, since drawing calls are only valid from inside
a draw handler and updating game state has no such restriction.

# References

[1] [Timers: scheduling future work](Timers.md)
[2] [The Framebuffer](Framebuffer.md)
