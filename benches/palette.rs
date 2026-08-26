//! `Color::nearest`'s palette-quantization search in isolation — the
//! per-pixel cost `kernel/image.rs`'s `quantize_to_palette` pays once for
//! every pixel of every loaded image.

use criterion::{Criterion, criterion_group, criterion_main};
use elysium_os::framebuffer::Color;

fn bench_nearest(c: &mut Criterion) {
    let mut group = c.benchmark_group("palette_nearest");
    group.bench_function("single_pixel", |b| {
        b.iter(|| Color::nearest(123, 45, 210));
    });
    group.bench_function("one_image_worth_of_pixels", |b| {
        // Roughly a small icon's worth of pixels, each a different color, to
        // approximate `quantize_to_palette`'s real workload.
        let pixels: Vec<(u8, u8, u8)> = (0..64 * 64)
            .map(|i| {
                (
                    (i % 256) as u8,
                    ((i / 256) % 256) as u8,
                    ((i / 65536) % 256) as u8,
                )
            })
            .collect();
        b.iter(|| {
            for &(r, g, b) in &pixels {
                Color::nearest(r, g, b);
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_nearest);
criterion_main!(benches);
