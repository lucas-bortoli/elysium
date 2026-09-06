//! A fixed pool of worker threads for the one job in the kernel that is
//! embarrassingly parallel: turning a frame's drawing into pixels.
//!
//! This is the second place in the kernel that crosses an OS thread boundary,
//! and it exists for the opposite reason to the first. The sound callback
//! (see `kernel/sound.rs`) runs on a thread the audio backend owns because
//! the backend demands one; the state it touches is walled off there so the
//! main thread never has to synchronise with it. Here there is no external
//! schedule to obey — the pool exists purely because a 720x360 surface has
//! a quarter of a million independent pixels and a desktop has many cores.
//! The main (render) thread drives every batch and blocks until it finishes,
//! so nothing here runs concurrently with the rest of the kernel: the VMs
//! have already ticked and the command list is frozen before the first band
//! thread wakes.
//!
//! The pool is built on a plain mutex and two condition variables rather than
//! a channel per worker. A batch is: publish the work, wake the workers, let
//! every thread — the workers and the submitter alike — claim indices from a
//! shared counter until it is drained, then the submitter waits for the last
//! worker to drop out. Claiming under the lock and running the task with it
//! released keeps the critical section to a couple of integer ops, and the
//! submitter doing its share means a batch still completes even if the OS
//! never schedules a single worker.
//!
//! `run` accepts a task that borrows from the submitter's stack. That is
//! sound because `run` does not return while any worker can still call the
//! task: a worker only ever touches it after claiming an index under the
//! lock, and the submitter only stops waiting once every worker that claimed
//! one has finished. The borrow is erased to `'static` for the hop across
//! the thread boundary and restored on the other side; the lifetime is
//! upheld by that barrier, not by the type system.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;

/// The shared pool, created the first time [`Pool::global`] is called and
/// kept for the life of the process.
static POOL: OnceLock<Pool> = OnceLock::new();

pub struct Pool {
    shared: &'static Shared,
    workers: Vec<JoinHandle<()>>,
}

/// A type-erased `&(dyn Fn(usize) + Sync)` whose lifetime has been erased to
/// `'static`. Only ever dereferenced by a thread that has claimed an index
/// for the current batch, and only while [`Pool::run`] is parked waiting for
/// that batch to drain — so the true referent always outlives the call.
#[derive(Clone, Copy)]
struct TaskPtr(*const (dyn Fn(usize) + Sync + 'static));

// SAFETY: the referent is `Sync`, and the barrier in `run` guarantees it is
// alive and unmoved for every dereference. The pointer is only published
// under `Shared::state`'s lock.
unsafe impl Send for TaskPtr {}
unsafe impl Sync for TaskPtr {}

struct BatchState {
    /// The current batch's task, or `None` between batches.
    task: Option<TaskPtr>,
    /// Next index no thread has claimed yet.
    next: usize,
    /// One past the last index in this batch.
    count: usize,
    /// Bumped once per batch so a worker can tell a new one has been
    /// published from one it has already drained.
    epoch: u64,
    /// Workers that have joined the current batch and not yet dropped out.
    /// The submitter waits for this to fall back to zero.
    active: usize,
}

struct Shared {
    state: Mutex<BatchState>,
    /// Woken when a new batch is published, or when shutting down.
    wake: Condvar,
    /// Woken when the last active worker leaves a batch.
    drained: Condvar,
    shutdown: AtomicBool,
}

impl Pool {
    /// The process-wide pool, sized to the machine's parallelism. The first
    /// call spawns the worker threads; every later call is a cheap lookup.
    pub fn global() -> &'static Pool {
        POOL.get_or_init(|| {
            let threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            Pool::with_threads(threads)
        })
    }

    /// A standalone pool of `threads` total participants (the caller plus
    /// `threads - 1` spawned workers). Used by [`Pool::global`]; also handy
    /// for tests that want a fixed band count independent of the machine.
    pub(crate) fn with_threads(threads: usize) -> Pool {
        let shared: &'static Shared = Box::leak(Box::new(Shared {
            state: Mutex::new(BatchState {
                task: None,
                next: 0,
                count: 0,
                epoch: 0,
                active: 0,
            }),
            wake: Condvar::new(),
            drained: Condvar::new(),
            shutdown: AtomicBool::new(false),
        }));

        // The submitter runs one share of every batch itself, so it counts
        // as one worker; the pool only needs `threads - 1` of its own.
        let extra = threads.saturating_sub(1);
        let workers = (0..extra)
            .map(|i| {
                std::thread::Builder::new()
                    .name(format!("elysium-raster-{i}"))
                    .spawn(move || worker_loop(shared))
                    .expect("failed to spawn a rasterizer worker thread")
            })
            .collect();

        Pool { shared, workers }
    }

    /// Number of threads that share a batch, the calling thread included.
    /// A caller uses this to pick how many pieces to cut the work into.
    pub fn thread_count(&self) -> usize {
        self.workers.len() + 1
    }

    /// Runs `task(i)` once for every `i` in `0..count`, spreading the calls
    /// across the pool and the calling thread, and returns only once all of
    /// them have finished. `task` may borrow from the caller's stack.
    ///
    /// Calls to `run` do not nest and come from one thread at a time — the
    /// render thread. `count` should be at most [`Pool::thread_count`] for
    /// the pieces to run truly in parallel, though any value is correct.
    pub fn run<F>(&self, count: usize, task: F)
    where
        F: Fn(usize) + Sync,
    {
        if count == 0 {
            return;
        }
        // Fast path: nothing to hand off if we are the only thread, or the
        // batch is a single piece.
        if count == 1 || self.workers.is_empty() {
            for i in 0..count {
                task(i);
            }
            return;
        }

        let task_ref: &(dyn Fn(usize) + Sync) = &task;
        // SAFETY: erased only for the thread hop. `run` blocks below until
        // `active` hits zero, i.e. until no worker can call the task again,
        // and `task` outlives that wait. Workers dereference it solely after
        // claiming an index under the lock.
        let erased: *const (dyn Fn(usize) + Sync + 'static) =
            unsafe { std::mem::transmute(task_ref as *const (dyn Fn(usize) + Sync)) };

        {
            let mut state = self.shared.state.lock().unwrap();
            state.task = Some(TaskPtr(erased));
            state.next = 0;
            state.count = count;
            state.active = 0;
            state.epoch = state.epoch.wrapping_add(1);
        }
        self.shared.wake.notify_all();

        // The submitter takes its own share.
        loop {
            let i = {
                let mut state = self.shared.state.lock().unwrap();
                if state.next >= state.count {
                    break;
                }
                let i = state.next;
                state.next += 1;
                i
            };
            task(i);
        }

        // Wait for any worker that claimed an index to finish with the task.
        let mut state = self.shared.state.lock().unwrap();
        while state.active != 0 {
            state = self.shared.drained.wait(state).unwrap();
        }
        state.task = None;
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.wake.notify_all();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

fn worker_loop(shared: &'static Shared) {
    let mut seen_epoch = 0u64;
    let mut state = shared.state.lock().unwrap();
    loop {
        while !shared.shutdown.load(Ordering::Acquire) && state.epoch == seen_epoch {
            state = shared.wake.wait(state).unwrap();
        }
        if shared.shutdown.load(Ordering::Acquire) {
            return;
        }
        seen_epoch = state.epoch;

        // Join the batch, drain a share of it, then drop out. `task` is only
        // read for an index this thread actually claimed, so a batch the
        // submitter finished on its own is a no-op here.
        state.active += 1;
        let task = state.task;
        loop {
            if state.next >= state.count {
                break;
            }
            let i = state.next;
            state.next += 1;
            drop(state);
            // SAFETY: `i` was claimed under the lock and is `< count`, so the
            // batch is live: `run` published a task before waking us and has
            // not yet stopped waiting on `active`, so the referent is alive.
            let task = task.expect("a claimed index implies a published task");
            unsafe { (*task.0)(i) };
            state = shared.state.lock().unwrap();
        }
        state.active -= 1;
        if state.active == 0 {
            shared.drained.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Pool;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn every_index_runs_exactly_once() {
        let pool = Pool::with_threads(4);
        let hits: Vec<AtomicUsize> = (0..64).map(|_| AtomicUsize::new(0)).collect();
        pool.run(hits.len(), |i| {
            hits[i].fetch_add(1, Ordering::Relaxed);
        });
        assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
    }

    #[test]
    fn a_batch_larger_than_the_pool_still_covers_every_index() {
        let pool = Pool::with_threads(3);
        let total = AtomicUsize::new(0);
        pool.run(1000, |_| {
            total.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(total.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn back_to_back_batches_do_not_lose_or_repeat_work() {
        let pool = Pool::with_threads(4);
        for round in 1..=50 {
            let sum = AtomicUsize::new(0);
            pool.run(round, |i| {
                sum.fetch_add(i, Ordering::Relaxed);
            });
            assert_eq!(sum.load(Ordering::Relaxed), round * (round - 1) / 2);
        }
    }

    #[test]
    fn a_single_thread_pool_runs_the_whole_batch_on_the_caller() {
        let pool = Pool::with_threads(1);
        let seen: Vec<AtomicUsize> = (0..10).map(|_| AtomicUsize::new(0)).collect();
        pool.run(seen.len(), |i| {
            seen[i].fetch_add(1, Ordering::Relaxed);
        });
        assert!(seen.iter().all(|h| h.load(Ordering::Relaxed) == 1));
    }
}
