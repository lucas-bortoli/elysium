//! Timers, microtasks, and promises: `setTimeout`/`setInterval`/`setImmediate`/
//! `requestAnimationFrame`, `queueMicrotask`, and async-function resumption.

use super::*;

#[test]
fn set_timeout_fires_once_due() {
    let runtime =
        eval("globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 0);");
    assert!(!global::<bool>(&runtime, "fired"), "fired before due");
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "fired"), "fired after due");
}

#[test]
fn set_timeout_does_not_fire_before_its_delay() {
    let runtime =
        eval("globalThis.fired = false; setTimeout(() => { globalThis.fired = true; }, 60_000);");
    runtime.run_due_timers().unwrap();
    assert!(!global::<bool>(&runtime, "fired"));
}

#[test]
fn clear_timeout_prevents_firing() {
    let runtime = eval(
        "globalThis.fired = false; \
         const id = setTimeout(() => { globalThis.fired = true; }, 0); \
         clearTimeout(id);",
    );
    runtime.run_due_timers().unwrap();
    assert!(!global::<bool>(&runtime, "fired"));
}

#[test]
fn set_interval_reschedules_until_cleared() {
    let runtime = eval(
        "globalThis.count = 0; \
         const id = setInterval(() => { \
             globalThis.count += 1; \
             if (globalThis.count >= 3) clearInterval(id); \
         }, 0);",
    );
    for _ in 0..5 {
        runtime.run_due_timers().unwrap();
    }
    assert_eq!(global::<f64>(&runtime, "count"), 3.0);
}

#[test]
fn set_immediate_fires_on_next_tick() {
    let runtime =
        eval("globalThis.fired = false; setImmediate(() => { globalThis.fired = true; });");
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "fired"));
}

#[test]
fn request_animation_frame_receives_a_timestamp() {
    let runtime = eval(
        "globalThis.timestamp = -1; \
         requestAnimationFrame((t) => { globalThis.timestamp = t; });",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<f64>(&runtime, "timestamp") >= 0.0);
}

#[test]
fn cancel_animation_frame_prevents_firing() {
    let runtime = eval(
        "globalThis.fired = false; \
         const id = requestAnimationFrame(() => { globalThis.fired = true; }); \
         cancelAnimationFrame(id);",
    );
    runtime.run_due_timers().unwrap();
    assert!(!global::<bool>(&runtime, "fired"));
}

#[test]
fn queue_microtask_runs_in_order_including_self_queued_work() {
    let runtime = eval(
        "globalThis.order = ''; \
         queueMicrotask(() => { \
             globalThis.order += '1'; \
             queueMicrotask(() => { globalThis.order += '2'; }); \
         }); \
         queueMicrotask(() => { globalThis.order += 'a'; });",
    );
    runtime.drain_microtasks();
    assert_eq!(global::<String>(&runtime, "order"), "1a2");
}

#[test]
fn promise_then_resolves_after_draining_microtasks() {
    let runtime = eval(
        "globalThis.resolved = false; \
         Promise.resolve().then(() => { globalThis.resolved = true; });",
    );
    runtime.drain_microtasks();
    assert!(global::<bool>(&runtime, "resolved"));
}

#[test]
fn timer_callback_can_use_a_promise() {
    let runtime = eval(
        "globalThis.resolved = false; \
         setTimeout(() => { \
             Promise.resolve().then(() => { globalThis.resolved = true; }); \
         }, 0);",
    );
    runtime.run_due_timers().unwrap();
    assert!(global::<bool>(&runtime, "resolved"));
}

#[test]
fn async_function_resumes_after_an_awaited_timer_resolves() {
    let runtime = eval(
        "globalThis.done = false; \
         async function run() { \
             await new Promise((resolve) => setTimeout(resolve, 0)); \
             globalThis.done = true; \
         } \
         run();",
    );
    assert!(!global::<bool>(&runtime, "done"), "shouldn't resume yet");
    runtime.run_due_timers().unwrap();
    assert!(
        global::<bool>(&runtime, "done"),
        "should resume once the awaited timer fires"
    );
}

#[test]
fn async_function_return_value_is_observable_via_then() {
    let runtime = eval(
        "globalThis.result = ''; \
         async function greeting() { \
             await Promise.resolve(); \
             return 'hi'; \
         } \
         greeting().then((value) => { globalThis.result = value; });",
    );
    runtime.drain_microtasks();
    assert_eq!(global::<String>(&runtime, "result"), "hi");
}
