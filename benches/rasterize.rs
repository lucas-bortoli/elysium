//! Raw drawing throughput, no VM involved: `framebuffer::rasterize`
//! applied to a pre-built list of N `DrawCommand`s onto a bare `Pixmap`,
//! isolating tiny-skia's rectangle-fill and image-compositing cost from
//! any JS/VM overhead.

use std::rc::Rc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use elysium_os::framebuffer::{self, Color, DrawCommand, FRAMEBUFFER_HEIGHT, FRAMEBUFFER_WIDTH};

fn heart_pixmap() -> Rc<tiny_skia::Pixmap> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches/fixtures/heartshapedobject.png");
    let bytes = std::fs::read(&path).expect("failed to read heartshapedobject.png fixture");
    Rc::new(tiny_skia::Pixmap::decode_png(&bytes).expect("failed to decode heartshapedobject.png"))
}

fn fill_rectangle_commands(count: u32) -> Vec<DrawCommand> {
    (0..count)
        .map(|i| DrawCommand::FillRectangle {
            x: (i % FRAMEBUFFER_WIDTH) as f32,
            y: (i % FRAMEBUFFER_HEIGHT) as f32,
            w: 20.0,
            h: 20.0,
            color: Color::Red500,
        })
        .collect()
}

fn draw_image_commands(count: u32) -> Vec<DrawCommand> {
    let pixmap = heart_pixmap();
    (0..count)
        .map(|i| DrawCommand::DrawImage {
            pixmap: Rc::clone(&pixmap),
            x: (i % FRAMEBUFFER_WIDTH) as f32,
            y: (i % FRAMEBUFFER_HEIGHT) as f32,
        })
        .collect()
}

fn bench_rasterize(c: &mut Criterion) {
    let mut group = c.benchmark_group("rasterize");
    for count in [10u32, 100, 1_000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("fill_rectangle", count),
            &count,
            |b, &count| {
                let commands = fill_rectangle_commands(count);
                let mut pixmap = tiny_skia::Pixmap::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT)
                    .expect("failed to allocate pixmap");
                b.iter(|| framebuffer::rasterize(&mut pixmap, &commands));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("draw_image", count),
            &count,
            |b, &count| {
                let commands = draw_image_commands(count);
                let mut pixmap = tiny_skia::Pixmap::new(FRAMEBUFFER_WIDTH, FRAMEBUFFER_HEIGHT)
                    .expect("failed to allocate pixmap");
                b.iter(|| framebuffer::rasterize(&mut pixmap, &commands));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_rasterize);
criterion_main!(benches);
