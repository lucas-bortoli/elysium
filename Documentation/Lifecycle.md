# Program lifecycle

A program that wants to run logic every frame, independent of drawing,
registers an update ticker through `ely:lifecycle` rather than exporting
anything from its entry module: `addUpdateTicker(handler)` calls
`handler(dt)` once per frame, with `dt` the time in seconds since the
previous frame, for as long as the ticker stays registered, and returns an
id `removeUpdateTicker(id)` can later use to stop it. `getDeltaTime()`
returns that same `dt`, for code that isn't itself a ticker callback but
still wants to know how much time the last frame took.

```ts
import { addUpdateTicker, removeUpdateTicker } from "ely:lifecycle";

let elapsed = 0;

const id = addUpdateTicker((dt) => {
  elapsed += dt;
  if (elapsed > 5) removeUpdateTicker(id);
});
```

Under the hood this is built entirely on `requestAnimationFrame` ([1]): a
ticker is really a callback that reschedules itself for the next frame
every time it runs, with `ely:lifecycle` doing that rescheduling and the
delta-time bookkeeping so a program doesn't have to. Drawing has its own,
separate per-frame registration (`addDrawHandler`, from `ely:framebuffer`
([2])) built the same way, since drawing calls are only valid from inside
a draw handler and updating game state has no such restriction.

# Deferring work past initialization

A program's top-level code runs as one bounded, synchronous call, before
timers, tickers, or draw handlers exist ([3]) — so top-level `await` on
anything that only a later call could resolve (a `setTimeout`, in
particular) is rejected outright rather than left to hang. A program that
needs to run such work as part of starting up registers it with
`addPostInitHandler(handler)` instead: `handler` runs exactly once, right
after the program's top-level code has finished evaluating, at which point
timers are live and it's safe to `await` one.

`delay(ms)` is a small `Promise`-returning wrapper around `setTimeout`, for
convenience inside a post-init handler (or any other already-running
callback — a ticker, a draw handler, another timer). Like `setTimeout`
itself, it must not be awaited from top-level code, for the same reason.

```ts
import { addPostInitHandler, delay } from "ely:lifecycle";

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  await delay(100);
  print("ready");
});
```

# References

[1] [Timers: scheduling future work](Timers.md)
[2] [The Framebuffer](Framebuffer.md)
[3] [Multitasking: keeping one program from hanging Elysium](Multitasking.md)
