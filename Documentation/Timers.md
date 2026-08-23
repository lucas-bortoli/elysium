# Timers: scheduling future work

A program isn't limited to running once at startup and registering an
update ticker or draw handler for its ongoing per-frame work ([1]). It can
schedule its own future work with the same timer functions a browser or
Node program would recognize:
`setTimeout`, `setInterval`, `setImmediate`, and `requestAnimationFrame`,
each with a matching function to cancel it (`clearTimeout`,
`clearInterval`, `clearImmediate`, `cancelAnimationFrame`). `Promise`,
`async`/`await`, and `queueMicrotask` all work too, and interact with
timers the way they would anywhere else: a `.then()` callback, an awaited
value, or a microtask queued from inside a timer's callback all run before
the next timer gets a chance to fire.

```ts
setTimeout(() => print("one second later"), 1000);

const id = setInterval(() => print("tick"), 500);
setTimeout(() => clearInterval(id), 3000);

requestAnimationFrame(function frame(timestamp) {
  print("frame at", timestamp);
  requestAnimationFrame(frame);
});
```

These all rely on Elysium checking a program's pending timers once per
frame, the same cadence update tickers and draw handlers are already
called on, rather than running an independent clock of their own. That
means a timer's callback never runs any sooner than its delay allows, but
also never any more precisely than "the frame at or after that delay
elapsed" — a `setTimeout(fn, 500)` scheduled between frames doesn't fire
mid-frame, it fires on the next frame whose start is at or past that 500ms
mark. For a program ticking at a normal frame rate this difference is
imperceptible; it only matters if a program is relying on sub-frame timing
precision, which nothing in Elysium currently provides.

A timer callback that throws, or that runs long enough to hit its
per-frame budget, is treated exactly like an update ticker or draw handler
callback that does the same — Elysium's guarded-call machinery doesn't
distinguish between them ([2]).

# References

[1] [Per-frame ticking](Loop.md)
[2] [Multitasking: keeping one program from hanging Elysium](Multitasking.md)
