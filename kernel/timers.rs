use std::cell::Cell;
use std::time::{Duration, Instant};

use boa_engine::interop::JsRest;
use boa_engine::object::builtins::JsFunction;
use boa_engine::{Context, Finalize, IntoJsFunctionCopied, JsResult, JsValue, Trace, js_string};
use boa_gc::{Gc, GcRefCell};

/// The arguments a due timer's callback is invoked with. Most timers replay
/// whatever a program passed after the delay (WHATWG's `setTimeout(cb, ms,
/// ...args)`); `requestAnimationFrame` instead always calls back with a
/// single timestamp, computed fresh when the timer fires rather than
/// captured at scheduling time.
#[derive(Clone, Trace, Finalize)]
pub enum TimerArgs {
    User(Vec<JsValue>),
    AnimationFrameTimestamp,
}

#[derive(Trace, Finalize)]
struct Timer {
    id: u32,
    #[unsafe_ignore_trace]
    due: Instant,
    /// `Some(period)` for `setInterval` (rescheduled after every firing);
    /// `None` for everything else (`setTimeout`, `setImmediate`,
    /// `requestAnimationFrame`), which fire once and are dropped.
    #[unsafe_ignore_trace]
    interval: Option<Duration>,
    callback: JsFunction,
    args: TimerArgs,
}

/// The VM's set of pending `setTimeout`/`setInterval`/`setImmediate`/
/// `requestAnimationFrame` timers, checked once per frame by
/// [`crate::runtime::ElysiumRuntime::run_due_timers`] rather than on any
/// finer-grained clock. A plain `Vec`, scanned linearly for due timers, is
/// deliberately simple rather than a heap — the number of timers a program
/// keeps live at once is expected to be small. Stored both as a field on
/// `ElysiumRuntime` and (via [`bootstrap_timers`]) in the `Context`'s native
/// data map, so native bindings registered with
/// [`boa_engine::IntoJsFunctionCopied`] — which requires `Copy`, so can't
/// capture a `Gc` directly — can look the same instance up through
/// `Context::get_data` instead.
#[derive(Trace, Finalize)]
pub struct TimerQueue {
    #[unsafe_ignore_trace]
    next_id: Cell<u32>,
    timers: GcRefCell<Vec<Timer>>,
    #[unsafe_ignore_trace]
    start: Instant,
}

impl TimerQueue {
    pub fn new() -> Self {
        Self {
            next_id: Cell::new(1),
            timers: GcRefCell::default(),
            start: Instant::now(),
        }
    }

    fn allocate_id(&self) -> u32 {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(1));
        id
    }

    /// Backs `setTimeout`/`setInterval`/`setImmediate`. `delay_ms` is
    /// clamped to be non-negative, per spec (a missing, negative, or `NaN`
    /// delay behaves as `0`).
    pub fn schedule(
        &self,
        callback: JsFunction,
        delay_ms: f64,
        args: Vec<JsValue>,
        interval: Option<Duration>,
    ) -> u32 {
        let delay = Duration::from_secs_f64(delay_ms.max(0.0) / 1000.0);
        let id = self.allocate_id();

        self.timers.borrow_mut().push(Timer {
            id,
            due: Instant::now() + delay,
            interval,
            callback,
            args: TimerArgs::User(args),
        });

        id
    }

    /// Backs `requestAnimationFrame`: always fires on the next tick (never
    /// this one, since `run_due_timers` has already taken its snapshot of
    /// due ids by the time script code can call this), and always calls
    /// back with a timestamp rather than replaying caller-supplied args.
    pub fn schedule_animation_frame(&self, callback: JsFunction) -> u32 {
        let id = self.allocate_id();
        self.timers.borrow_mut().push(Timer {
            id,
            due: Instant::now(),
            interval: None,
            callback,
            args: TimerArgs::AnimationFrameTimestamp,
        });
        id
    }

    /// Backs `clearTimeout`/`clearInterval`/`clearImmediate`/
    /// `cancelAnimationFrame` — all four share one id space and can cancel
    /// any kind of timer, matching how browsers let any of them cross-clear.
    pub fn clear(&self, id: u32) {
        self.timers.borrow_mut().retain(|timer| timer.id != id);
    }

    /// Snapshots the ids due at `now`, taken once at the start of a tick so
    /// a timer scheduled by a callback running *during* this tick isn't
    /// picked up until the next one.
    pub fn due_ids(&self, now: Instant) -> Vec<u32> {
        self.timers
            .borrow()
            .iter()
            .filter(|timer| timer.due <= now)
            .map(|timer| timer.id)
            .collect()
    }

    /// Prepares to run `id`'s callback: a one-shot timer (`setTimeout`,
    /// `setImmediate`, `requestAnimationFrame`) is removed outright, since
    /// nothing needs it after this firing; a `setInterval` timer is left in
    /// place (its payload cloned out) so that after the callback runs,
    /// [`Self::reschedule_if_still_active`] can tell whether the callback
    /// cleared itself. Returns `None` if `id` was already cleared by an
    /// earlier callback in this same tick's batch. The borrow this takes is
    /// released before the caller invokes the callback — no borrow is ever
    /// held across a call into JS, which is what makes `clearInterval`
    /// called reentrantly from within a firing callback safe.
    pub fn prepare_run(&self, id: u32) -> Option<(JsFunction, TimerArgs, Option<Duration>)> {
        let mut timers = self.timers.borrow_mut();
        let index = timers.iter().position(|timer| timer.id == id)?;

        if timers[index].interval.is_none() {
            let timer = timers.remove(index);
            Some((timer.callback.clone(), timer.args.clone(), None))
        } else {
            let timer = &timers[index];
            Some((timer.callback.clone(), timer.args.clone(), timer.interval))
        }
    }

    /// After a `setInterval` callback has run, advances its `due` to
    /// `next_due` — unless the callback cleared it (via `clearInterval`),
    /// in which case `id` is no longer present and this is a no-op.
    pub fn reschedule_if_still_active(&self, id: u32, next_due: Instant) {
        if let Some(timer) = self
            .timers
            .borrow_mut()
            .iter_mut()
            .find(|timer| timer.id == id)
        {
            timer.due = next_due;
        }
    }

    pub fn elapsed_seconds(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

/// Wraps the `queueMicrotask` queue so it has a distinct type from any other
/// `Gc<GcRefCell<Vec<JsFunction>>>` stored in the same `Context`'s native
/// data map (see [`TimerQueue`]'s doc comment) — `ely:lifecycle`'s
/// `addPostInitHandler` list, bootstrapped separately in `runtime.rs`, is
/// shaped identically but must not collide with this one.
#[derive(Clone, Trace, Finalize, boa_engine::JsData)]
pub struct Microtasks(pub Gc<GcRefCell<Vec<JsFunction>>>);

fn timer_queue(context: &Context) -> Gc<TimerQueue> {
    context
        .get_data::<Gc<TimerQueue>>()
        .expect("bootstrap_timers must run before any timer global is reachable")
        .clone()
}

fn microtasks(context: &Context) -> Gc<GcRefCell<Vec<JsFunction>>> {
    context
        .get_data::<Microtasks>()
        .expect("bootstrap_timers must run before queueMicrotask is reachable")
        .0
        .clone()
}

fn set_timeout(
    callback: JsFunction,
    delay: Option<f64>,
    extra: JsRest<'_>,
    context: &mut Context,
) -> u32 {
    timer_queue(context).schedule(callback, delay.unwrap_or(0.0), extra.0.to_vec(), None)
}

fn set_interval(
    callback: JsFunction,
    delay: Option<f64>,
    extra: JsRest<'_>,
    context: &mut Context,
) -> u32 {
    let delay_ms = delay.unwrap_or(0.0);
    let period = Duration::from_secs_f64(delay_ms.max(0.0) / 1000.0);
    timer_queue(context).schedule(callback, delay_ms, extra.0.to_vec(), Some(period))
}

fn set_immediate(callback: JsFunction, extra: JsRest<'_>, context: &mut Context) -> u32 {
    timer_queue(context).schedule(callback, 0.0, extra.0.to_vec(), None)
}

fn request_animation_frame(callback: JsFunction, context: &mut Context) -> u32 {
    timer_queue(context).schedule_animation_frame(callback)
}

fn clear_timer(id: Option<u32>, context: &mut Context) {
    if let Some(id) = id {
        timer_queue(context).clear(id);
    }
}

fn queue_microtask(callback: JsFunction, context: &mut Context) {
    microtasks(context).borrow_mut().push(callback);
}

/// Registers `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`,
/// `setImmediate`, `clearImmediate`, `requestAnimationFrame`,
/// `cancelAnimationFrame`, and `queueMicrotask` as globals, and stores
/// `timers`/`microtasks` in the `Context`'s native data map so those
/// globals' native bindings — plain `fn`s registered via
/// [`boa_engine::IntoJsFunctionCopied`], which requires `Copy` and so can't
/// capture a `Gc` in a closure — can find them again on every call.
pub fn bootstrap_timers(
    context: &mut Context,
    timers: Gc<TimerQueue>,
    microtasks: Gc<GcRefCell<Vec<JsFunction>>>,
) -> JsResult<()> {
    context.insert_data(timers);
    context.insert_data(Microtasks(microtasks));

    let f = set_timeout.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("setTimeout"), 2, f)?;

    let f = set_interval.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("setInterval"), 2, f)?;

    let f = set_immediate.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("setImmediate"), 1, f)?;

    let f = request_animation_frame.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("requestAnimationFrame"), 1, f)?;

    for name in [
        "clearTimeout",
        "clearInterval",
        "clearImmediate",
        "cancelAnimationFrame",
    ] {
        let f = clear_timer.into_js_function_copied(context);
        context.register_global_builtin_callable(js_string!(name), 1, f)?;
    }

    let f = queue_microtask.into_js_function_copied(context);
    context.register_global_builtin_callable(js_string!("queueMicrotask"), 1, f)?;

    Ok(())
}
