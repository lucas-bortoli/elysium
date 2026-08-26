//! Cost of constructing a VM, isolated from module evaluation: how long
//! `ElysiumRuntime::new_headless` alone takes, versus construction plus
//! evaluating a trivial script — the fixed overhead every program pays
//! before its own code starts running.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use elysium_os::runtime::ElysiumRuntime;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/fixtures")
}

fn bench_construction(c: &mut Criterion) {
    c.bench_function("vm_construction", |b| {
        b.iter(|| {
            let (runtime, _input, _draw_commands) =
                ElysiumRuntime::new_headless(fixtures_dir()).expect("failed to construct runtime");
            black_box(runtime);
        });
    });
}

fn bench_construction_and_trivial_eval(c: &mut Criterion) {
    c.bench_function("vm_construction_and_trivial_eval", |b| {
        b.iter(|| {
            let (runtime, _input, _draw_commands) =
                ElysiumRuntime::new_headless(fixtures_dir()).expect("failed to construct runtime");
            runtime
                .eval_module("bench.ts", "globalThis.x = 1 + 1;")
                .expect("module failed to evaluate");
            black_box(runtime);
        });
    });
}

criterion_group!(
    benches,
    bench_construction,
    bench_construction_and_trivial_eval
);
criterion_main!(benches);
