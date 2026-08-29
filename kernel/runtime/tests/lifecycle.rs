//! The `ely:lifecycle` surface: update tickers, and the post-init handlers a
//! program defers work to when it needs timers already running.

use super::*;

#[test]
fn update_ticker_fires_once_per_frame_with_a_delta_time() {
    let runtime = eval(
        "import { addUpdateTicker } from 'ely:lifecycle'; \
         globalThis.calls = 0; \
         globalThis.lastDt = -1; \
         addUpdateTicker((dt) => { \
             globalThis.calls += 1; \
             globalThis.lastDt = dt; \
         });",
    );
    for expected_calls in 1..=3 {
        runtime.run_due_timers().unwrap();
        assert_eq!(global::<f64>(&runtime, "calls"), expected_calls as f64);
        assert!(global::<f64>(&runtime, "lastDt") >= 0.0);
    }
}

#[test]
fn remove_update_ticker_stops_further_calls() {
    let runtime = eval(
        "import { addUpdateTicker, removeUpdateTicker } from 'ely:lifecycle'; \
         globalThis.calls = 0; \
         const id = addUpdateTicker(() => { \
             globalThis.calls += 1; \
             removeUpdateTicker(id); \
         });",
    );
    for _ in 0..3 {
        runtime.run_due_timers().unwrap();
    }
    assert_eq!(global::<f64>(&runtime, "calls"), 1.0);
}

#[test]
fn post_init_handler_runs_once_after_eval_and_sees_working_timers() {
    let runtime = eval(
        "import { addPostInitHandler, delay } from 'ely:lifecycle'; \
         globalThis.ran = false; \
         globalThis.timerFired = false; \
         addPostInitHandler(async () => { \
             globalThis.ran = true; \
             await delay(0); \
             globalThis.timerFired = true; \
         });",
    );
    assert!(
        !global::<bool>(&runtime, "ran"),
        "must not run during eval_module itself"
    );

    runtime.run_post_init_handlers().unwrap();
    assert!(global::<bool>(&runtime, "ran"));
    assert!(
        !global::<bool>(&runtime, "timerFired"),
        "the delay(0) timer hasn't been serviced yet"
    );

    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "timerFired"));

    // A second call must not re-run anything: the handler list was
    // drained, not just iterated.
    runtime.run_post_init_handlers().unwrap();
}
