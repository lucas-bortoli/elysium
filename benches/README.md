# Benchmarks

Run all of them with `cargo bench`, or a single file with `cargo bench --bench <name>`.
Criterion writes an HTML report per benchmark under `target/criterion/`.

- `vm_lifecycle` — cost of `ElysiumRuntime::new_headless` alone, and of
  construction plus evaluating a trivial script, isolating VM startup from
  module evaluation.
- `program_eval` — evaluates `userland/programs/init/index.ts` end to end
  (construction, `eval_module`, `run_post_init_handlers`), including its
  image load and palette quantization — the full cold-start cost a real
  program pays.
- `bouncing_objects` — the case that actually matters for a game loop: N
  objects with `ely:math`-driven motion, bouncing off the framebuffer edges,
  each drawn with `drawImage` every frame. Each iteration runs
  `run_due_timers` to advance JS state and collect draw commands, then
  `framebuffer::rasterize` to paint them onto a bare `Pixmap` — so the
  reported time is what one frame actually costs, parameterized over N
  (10/100/1,000/10,000) to see where it crosses the 16ms `FRAME_BUDGET`
  (`kernel/runtime.rs`).
- `rasterize` — `framebuffer::rasterize` alone, no VM: N pre-built
  `FillRectangle`/`DrawImage` commands rasterized onto a bare `Pixmap`,
  isolating raw tiny-skia drawing cost from JS overhead.
- `frame_tick` — `run_due_timers` with N `setInterval` timers registered
  and nothing else, isolating timer-dispatch overhead on its own.
- `palette` — `Color::nearest`'s palette search in isolation, the per-pixel
  cost every loaded image's quantization pass pays.

`benches/fixtures/heartshapedobject.png` is the texture `bouncing_objects`
and `rasterize`'s `draw_image` case draw; it plays the same role
`kernel/image/fixtures/test.png` plays for the unit tests in
`kernel/runtime.rs`.
