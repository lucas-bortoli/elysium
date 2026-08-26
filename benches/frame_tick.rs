//! Timer-dispatch overhead in isolation: with N `setInterval` timers
//! already registered and no drawing involved, how long one
//! `run_due_timers` sweep takes — the steady-state per-frame cost the
//! `run_guarded`/`FRAME_BUDGET` machinery in `kernel/runtime.rs` has to
//! stay under.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use elysium_os::runtime::ElysiumRuntime;

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/fixtures")
}

fn runtime_with_timers(count: u32) -> ElysiumRuntime {
    let (runtime, _input, _draw_commands) =
        ElysiumRuntime::new_headless(fixtures_dir()).expect("failed to construct runtime");
    let source = format!("for (let i = 0; i < {count}; i++) {{ setInterval(() => {{}}, 0); }}");
    runtime
        .eval_module("bench.ts", &source)
        .expect("module failed to evaluate");
    runtime
}

fn bench_run_due_timers(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_tick_run_due_timers");
    for count in [10u32, 100, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let runtime = runtime_with_timers(count);
            b.iter(|| runtime.run_due_timers().expect("timer callback failed"));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_run_due_timers);
criterion_main!(benches);
