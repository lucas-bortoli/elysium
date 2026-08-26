//! Evaluating a realistic program end-to-end: `userland/programs/init/index.ts`,
//! run through construction, `eval_module`, and `run_post_init_handlers` —
//! the full cold-start path a real program pays, including its image load
//! and palette quantization.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use elysium_os::runtime::ElysiumRuntime;

fn init_program_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("userland/programs/init")
}

fn bench_cold_start(c: &mut Criterion) {
    let source = std::fs::read_to_string(init_program_dir().join("index.ts"))
        .expect("failed to read userland/programs/init/index.ts");

    c.bench_function("program_eval_cold_start", |b| {
        b.iter(|| {
            let (runtime, _input, draw_commands) = ElysiumRuntime::new_headless(init_program_dir())
                .expect("failed to construct runtime");
            runtime
                .eval_module("index.ts", &source)
                .expect("module failed to evaluate");
            runtime
                .run_post_init_handlers()
                .expect("post-init handler failed");
            black_box(&draw_commands);
        });
    });
}

criterion_group!(benches, bench_cold_start);
criterion_main!(benches);
