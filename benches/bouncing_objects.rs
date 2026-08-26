//! The "N objects moving every frame" case end-to-end: a program that
//! loads a texture, keeps N objects' positions/velocities in a plain
//! array, integrates motion and bounces off the framebuffer edges each
//! tick via `ely:math`'s vector helpers, and draws each object with
//! `drawImage`. Each iteration drives one real frame — `run_due_timers`
//! advances the JS state and collects `DrawCommand`s, then
//! `framebuffer::rasterize` paints them onto a bare `Pixmap` — so the
//! reported time is what one frame of a bouncing-objects scene actually
//! costs, measured against the 16ms `FRAME_BUDGET` in `kernel/runtime.rs`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use elysium_os::framebuffer::{self, FRAMEBUFFER_HEIGHT, FRAMEBUFFER_WIDTH};
use elysium_os::runtime::ElysiumRuntime;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/fixtures")
}

fn bouncing_objects_source(count: u32) -> String {
    format!(
        "import {{ loadImage }} from 'ely:image';
         import {{ addDrawHandler, drawImage, getWidth, getHeight }} from 'ely:framebuffer';
         import {{ addUpdateTicker }} from 'ely:lifecycle';
         import {{ vector2Add, vector2Scale }} from 'ely:math';

         const heart = loadImage('heartshapedobject.png');
         const objects = [];
         for (let i = 0; i < {count}; i++) {{
             objects.push({{
                 position: {{ x: (i * 37) % getWidth(), y: (i * 53) % getHeight() }},
                 velocity: {{ x: 50 + (i % 10), y: 30 + (i % 7) }},
             }});
         }}

         addUpdateTicker((dt) => {{
             const dtSeconds = dt / 1000;
             for (const obj of objects) {{
                 obj.position = vector2Add(obj.position, vector2Scale(obj.velocity, dtSeconds));
                 if (obj.position.x < 0 || obj.position.x > getWidth()) obj.velocity.x *= -1;
                 if (obj.position.y < 0 || obj.position.y > getHeight()) obj.velocity.y *= -1;
             }}
         }});

         addDrawHandler(() => {{
             for (const obj of objects) {{
                 drawImage(heart, obj.position.x, obj.position.y);
             }}
         }});"
    )
}

fn bench_bouncing_objects(c: &mut Criterion) {
    let mut group = c.benchmark_group("bouncing_objects_per_frame");
    for count in [10u32, 100, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let (runtime, _input, draw_commands) =
                ElysiumRuntime::new_headless(fixtures_dir()).expect("failed to construct runtime");
            runtime
                .eval_module("bouncing_objects.ts", &bouncing_objects_source(count))
                .expect("module failed to evaluate");
            let mut pixmap = tiny_skia::Pixmap::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT)
                .expect("failed to allocate pixmap");

            b.iter(|| {
                runtime.run_due_timers().expect("frame callback failed");
                framebuffer::rasterize(&mut pixmap, &draw_commands.borrow());
                draw_commands.borrow_mut().clear();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_bouncing_objects);
criterion_main!(benches);
