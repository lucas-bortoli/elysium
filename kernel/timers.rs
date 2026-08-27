use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

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
    let global = ctx.globals();

    {
        let timers = Rc::clone(&timers);
        global.set(
            "setTimeout",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>,
                      callback: Function<'js>,
                      delay: Opt<f64>,
                      extra: Rest<Value<'js>>|
                      -> Result<u32> {
                    timers.schedule(&ctx, callback, delay.0.unwrap_or(0.0), extra.0, None)
                },
            )?,
        )?;
    }

    {
        let timers = Rc::clone(&timers);
        global.set(
            "setInterval",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>,
                      callback: Function<'js>,
                      delay: Opt<f64>,
                      extra: Rest<Value<'js>>|
                      -> Result<u32> {
                    let delay_ms = delay.0.unwrap_or(0.0);
                    let period = Duration::from_secs_f64(delay_ms.max(0.0) / 1000.0);
                    timers.schedule(&ctx, callback, delay_ms, extra.0, Some(period))
                },
            )?,
        )?;
    }

    {
        let timers = Rc::clone(&timers);
        global.set(
            "setImmediate",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>,
                      callback: Function<'js>,
                      extra: Rest<Value<'js>>|
                      -> Result<u32> {
                    timers.schedule(&ctx, callback, 0.0, extra.0, None)
                },
            )?,
        )?;
    }

    {
        let timers = Rc::clone(&timers);
        global.set(
            "requestAnimationFrame",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, callback: Function<'js>| -> Result<u32> {
                    timers.schedule_animation_frame(&ctx, callback)
                },
            )?,
        )?;
    }

    for name in [
        "clearTimeout",
        "clearInterval",
        "clearImmediate",
        "cancelAnimationFrame",
    ] {
        let timers = Rc::clone(&timers);
        global.set(
            name,
            Function::new(ctx.clone(), move |id: Opt<u32>| {
                if let Some(id) = id.0 {
                    timers.clear(id);
                }
            })?,
        )?;
    }

    {
        let microtasks = Rc::clone(&microtasks);
        global.set(
            "queueMicrotask",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, callback: Function<'js>| {
                    microtasks
                        .borrow_mut()
                        .push(Persistent::save(&ctx, callback));
                },
            )?,
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
}
