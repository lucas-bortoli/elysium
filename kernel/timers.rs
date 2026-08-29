use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::bindings::bind;
use rquickjs::function::{Opt, Rest};
use rquickjs::{Ctx, Function, Persistent, Result, Value};

/// The arguments a due timer's callback is invoked with. Most timers replay
/// whatever a program passed after the delay (WHATWG's `setTimeout(cb, ms,
/// ...args)`); `requestAnimationFrame` instead always calls back with a
/// single timestamp, computed fresh when the timer fires rather than
/// captured at scheduling time.
#[derive(Clone)]
pub enum TimerArgs {
    User(Vec<Persistent<Value<'static>>>),
    AnimationFrameTimestamp,
}

struct Timer {
    id: u32,
    due: Instant,
    /// `Some(period)` for `setInterval` (rescheduled after every firing);
    /// `None` for everything else (`setTimeout`, `setImmediate`,
    /// `requestAnimationFrame`), which fire once and are dropped.
    interval: Option<Duration>,
    callback: Persistent<Function<'static>>,
    args: TimerArgs,
}

/// The VM's set of pending `setTimeout`/`setInterval`/`setImmediate`/
/// `requestAnimationFrame` timers, checked once per frame by
/// [`crate::runtime::ElysiumRuntime::run_due_timers`] rather than on any
/// finer-grained clock. `Rc`/`Cell`/`RefCell`, not `Arc`/`Mutex`, for the
/// same reason as `GuardState` in `runtime.rs`: nothing here crosses an OS
/// thread. A plain `Vec`, scanned linearly for due timers, is deliberately
/// simple rather than a heap — the number of timers a program keeps live at
/// once is expected to be small.
pub struct TimerQueue {
    next_id: Cell<u32>,
    timers: RefCell<Vec<Timer>>,
    start: Instant,
}

impl TimerQueue {
    pub fn new() -> Self {
        Self {
            next_id: Cell::new(1),
            timers: RefCell::new(Vec::new()),
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
    pub fn schedule<'js>(
        &self,
        ctx: &Ctx<'js>,
        callback: Function<'js>,
        delay_ms: f64,
        args: Vec<Value<'js>>,
        interval: Option<Duration>,
    ) -> Result<u32> {
        let delay = Duration::from_secs_f64(delay_ms.max(0.0) / 1000.0);
        let id = self.allocate_id();
        let args = args.into_iter().map(|v| Persistent::save(ctx, v)).collect();

        self.timers.borrow_mut().push(Timer {
            id,
            due: Instant::now() + delay,
            interval,
            callback: Persistent::save(ctx, callback),
            args: TimerArgs::User(args),
        });

        Ok(id)
    }

    /// Backs `requestAnimationFrame`: always fires on the next tick (never
    /// this one, since `run_due_timers` has already taken its snapshot of
    /// due ids by the time script code can call this), and always calls
    /// back with a timestamp rather than replaying caller-supplied args.
    pub fn schedule_animation_frame<'js>(
        &self,
        ctx: &Ctx<'js>,
        callback: Function<'js>,
    ) -> Result<u32> {
        let id = self.allocate_id();
        self.timers.borrow_mut().push(Timer {
            id,
            due: Instant::now(),
            interval: None,
            callback: Persistent::save(ctx, callback),
            args: TimerArgs::AnimationFrameTimestamp,
        });
        Ok(id)
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
    /// [`Self::is_still_scheduled`] can tell whether the callback cleared
    /// itself. Returns `None` if `id` was already cleared by an earlier
    /// callback in this same tick's batch. The borrow this takes is
    /// released before the caller invokes the callback — no borrow is ever
    /// held across a call into JS, which is what makes `clearInterval`
    /// called reentrantly from within a firing callback safe.
    pub fn prepare_run(
        &self,
        id: u32,
    ) -> Option<(Persistent<Function<'static>>, TimerArgs, Option<Duration>)> {
        let mut timers = self.timers.borrow_mut();
        let index = timers.iter().position(|timer| timer.id == id)?;

        if timers[index].interval.is_none() {
            let timer = timers.remove(index);
            Some((timer.callback, timer.args, None))
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

    /// Whether any timer is still pending. Part of the signal
    /// [`crate::process_manager::ProcessManager`] uses to decide a process
    /// has run out of work and can be reaped.
    pub fn is_empty(&self) -> bool {
        self.timers.borrow().is_empty()
    }

    /// Drops every pending timer, releasing the `Persistent` callbacks (and
    /// any `Persistent` args) they hold. Used by `ElysiumRuntime`'s `Drop`
    /// impl to release these deterministically, through an explicit call
    /// while the VM is still fully valid, rather than leaving it to
    /// whichever native-closure finalizer QuickJS happens to run them
    /// through during its own teardown.
    pub fn clear_all(&self) {
        self.timers.borrow_mut().clear();
    }
}

/// Registers `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`,
/// `setImmediate`, `clearImmediate`, `requestAnimationFrame`,
/// `cancelAnimationFrame`, and `queueMicrotask` as globals.
pub fn bootstrap_timers<'js>(
    ctx: &Ctx<'js>,
    timers: Rc<TimerQueue>,
    microtasks: Rc<RefCell<Vec<Persistent<Function<'static>>>>>,
) -> Result<()> {
    {
        let timers = Rc::clone(&timers);
        bind(
            ctx,
            "setTimeout",
            move |ctx: Ctx<'js>,
                  callback: Function<'js>,
                  delay: Opt<f64>,
                  extra: Rest<Value<'js>>|
                  -> Result<u32> {
                timers.schedule(&ctx, callback, delay.0.unwrap_or(0.0), extra.0, None)
            },
        )?;
    }

    {
        let timers = Rc::clone(&timers);
        bind(
            ctx,
            "setInterval",
            move |ctx: Ctx<'js>,
                  callback: Function<'js>,
                  delay: Opt<f64>,
                  extra: Rest<Value<'js>>|
                  -> Result<u32> {
                let delay_ms = delay.0.unwrap_or(0.0);
                let period = Duration::from_secs_f64(delay_ms.max(0.0) / 1000.0);
                timers.schedule(&ctx, callback, delay_ms, extra.0, Some(period))
            },
        )?;
    }

    {
        let timers = Rc::clone(&timers);
        bind(
            ctx,
            "setImmediate",
            move |ctx: Ctx<'js>, callback: Function<'js>, extra: Rest<Value<'js>>| -> Result<u32> {
                timers.schedule(&ctx, callback, 0.0, extra.0, None)
            },
        )?;
    }

    {
        let timers = Rc::clone(&timers);
        bind(
            ctx,
            "requestAnimationFrame",
            move |ctx: Ctx<'js>, callback: Function<'js>| -> Result<u32> {
                timers.schedule_animation_frame(&ctx, callback)
            },
        )?;
    }

    for name in [
        "clearTimeout",
        "clearInterval",
        "clearImmediate",
        "cancelAnimationFrame",
    ] {
        let timers = Rc::clone(&timers);
        bind(ctx, name, move |id: Opt<u32>| {
            if let Some(id) = id.0 {
                timers.clear(id);
            }
        })?;
    }

    {
        let microtasks = Rc::clone(&microtasks);
        bind(
            ctx,
            "queueMicrotask",
            move |ctx: Ctx<'js>, callback: Function<'js>| {
                microtasks
                    .borrow_mut()
                    .push(Persistent::save(&ctx, callback));
            },
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    use super::*;

    #[test]
    fn is_empty_tracks_pending_timers() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        let queue = TimerQueue::new();

        assert!(queue.is_empty(), "fresh queue");

        let id = context.with(|ctx| {
            let noop = Function::new(ctx.clone(), || {}).unwrap();
            queue.schedule(&ctx, noop, 0.0, Vec::new(), None).unwrap()
        });
        assert!(!queue.is_empty(), "one pending timer");

        queue.clear(id);
        assert!(queue.is_empty(), "cleared");
    }

    /// A queue plus the JS context its callbacks have to be saved against.
    /// Every test below schedules through this, since a `Persistent` can
    /// only be made from inside a live context.
    fn queue_with<R>(body: impl FnOnce(&TimerQueue, &Ctx<'_>) -> R) -> R {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        let queue = TimerQueue::new();
        context.with(|ctx| body(&queue, &ctx))
    }

    fn schedule_at(queue: &TimerQueue, ctx: &Ctx<'_>, delay_ms: f64) -> u32 {
        let noop = Function::new(ctx.clone(), || {}).unwrap();
        queue
            .schedule(ctx, noop, delay_ms, Vec::new(), None)
            .unwrap()
    }

    #[test]
    fn due_ids_reports_only_timers_whose_delay_has_elapsed() {
        queue_with(|queue, ctx| {
            let soon = schedule_at(queue, ctx, 0.0);
            let later = schedule_at(queue, ctx, 10_000.0);

            let due = queue.due_ids(Instant::now());
            assert_eq!(due, vec![soon], "only the elapsed timer is due");

            let due = queue.due_ids(Instant::now() + Duration::from_secs(20));
            assert_eq!(due, vec![soon, later], "far enough ahead, both are");
        });
    }

    #[test]
    fn a_negative_or_absent_delay_is_due_immediately() {
        queue_with(|queue, ctx| {
            let negative = schedule_at(queue, ctx, -5_000.0);
            assert_eq!(queue.due_ids(Instant::now()), vec![negative]);
        });
    }

    #[test]
    fn preparing_a_one_shot_timer_takes_it_out_of_the_queue() {
        queue_with(|queue, ctx| {
            let id = schedule_at(queue, ctx, 0.0);
            assert!(queue.prepare_run(id).is_some(), "first run");
            assert!(queue.is_empty(), "a one-shot timer is spent once prepared");
            assert!(
                queue.prepare_run(id).is_none(),
                "and cannot be prepared twice"
            );
        });
    }

    #[test]
    fn preparing_an_interval_leaves_it_in_the_queue_to_be_rescheduled() {
        queue_with(|queue, ctx| {
            let noop = Function::new(ctx.clone(), || {}).unwrap();
            let id = queue
                .schedule(ctx, noop, 0.0, Vec::new(), Some(Duration::from_secs(1)))
                .unwrap();

            let (_callback, _args, interval) = queue.prepare_run(id).expect("interval prepares");
            assert_eq!(interval, Some(Duration::from_secs(1)));
            assert!(!queue.is_empty(), "an interval survives its own firing");
        });
    }

    /// The case a callback that clears its own interval exercises: by the
    /// time the manager reschedules, the id is gone and must stay gone.
    #[test]
    fn rescheduling_a_timer_its_callback_cleared_does_not_revive_it() {
        queue_with(|queue, ctx| {
            let noop = Function::new(ctx.clone(), || {}).unwrap();
            let id = queue
                .schedule(ctx, noop, 0.0, Vec::new(), Some(Duration::from_secs(1)))
                .unwrap();

            queue.prepare_run(id).expect("interval prepares");
            queue.clear(id); // what `clearInterval` inside the callback does
            queue.reschedule_if_still_active(id, Instant::now() + Duration::from_secs(1));

            assert!(queue.is_empty(), "a cleared interval stays cleared");
            assert!(queue.due_ids(Instant::now()).is_empty());
        });
    }

    #[test]
    fn rescheduling_a_live_interval_moves_it_past_the_current_instant() {
        queue_with(|queue, ctx| {
            let noop = Function::new(ctx.clone(), || {}).unwrap();
            let id = queue
                .schedule(ctx, noop, 0.0, Vec::new(), Some(Duration::from_secs(1)))
                .unwrap();

            let now = Instant::now();
            assert_eq!(queue.due_ids(now), vec![id], "due before rescheduling");
            queue.prepare_run(id).expect("interval prepares");
            queue.reschedule_if_still_active(id, now + Duration::from_secs(1));

            assert!(queue.due_ids(now).is_empty(), "not due again yet");
            assert_eq!(
                queue.due_ids(now + Duration::from_secs(2)),
                vec![id],
                "due again once its period has passed"
            );
        });
    }

    /// All four clear functions share one id space, so clearing one timer
    /// must leave its neighbours alone.
    #[test]
    fn clearing_one_timer_leaves_the_others_scheduled() {
        queue_with(|queue, ctx| {
            let first = schedule_at(queue, ctx, 0.0);
            let second = schedule_at(queue, ctx, 0.0);

            queue.clear(first);
            assert_eq!(queue.due_ids(Instant::now()), vec![second]);
            queue.clear(9_999); // an id that was never handed out
            assert_eq!(queue.due_ids(Instant::now()), vec![second]);
        });
    }

    #[test]
    fn an_animation_frame_callback_is_handed_a_timestamp_instead_of_args() {
        queue_with(|queue, ctx| {
            let noop = Function::new(ctx.clone(), || {}).unwrap();
            let id = queue.schedule_animation_frame(ctx, noop).unwrap();
            let (_callback, args, interval) = queue.prepare_run(id).expect("prepares");
            assert!(matches!(args, TimerArgs::AnimationFrameTimestamp));
            assert_eq!(interval, None, "an animation frame fires once");
        });
    }
}
