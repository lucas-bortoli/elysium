# Multitasking: many programs, one kernel

> A faulted process is destroyed, but Elysium soldiers on.

Elysium runs many programs at once, each in its own isolated JavaScript
process. The kernel keeps a table of them and, once per frame, gives every
process a turn: it delivers any messages waiting for that process, runs
whatever timers of that process are due, and then decides whether the
process still has anything left to do. A process that has run out of work is
reaped; a process that faults is dropped without disturbing the others; and
when the table is finally empty, the kernel exits. The first process, the
init program, is started by the kernel at boot and is otherwise no different
from any process a program spawns later.

## Bounding a single call into a process

Isolation between processes isn't a one-shot thing: the kernel calls back
into a running program repeatedly over its lifetime — a due timer, an update
ticker ([1]), a draw handler ([2]), a delivered message. Any one of those
calls has to be boundable, because if a program's script code enters an
infinite loop or a runaway computation, that can't be allowed to hang the
call — and by extension the process's turn, the frame, and the rest of
Elysium.

The JS engine supports installing a hook that it checks periodically while
running script code, roughly every 10,000 loop-iteration or function-call
steps. If that hook signals "stop," the engine immediately raises an
exception that unwinds the entire evaluation and cannot be caught by the
program's own `try`/`catch`. Elysium installs one such hook per process, for
the process's whole lifetime, but it doesn't fire on a fixed schedule — it
checks a deadline armed immediately before a guarded call begins and
disarmed immediately after, so between calls no deadline is armed and the
hook does nothing.

This is cooperative, not preemptive: it can only interrupt a process that's
actually still stepping through script code. Ordinary infinite loops
(`while (true) {}`, runaway recursion, and the like) are stepping
constantly, so they're reliably caught. What this doesn't protect against is
a process stuck inside a single long-running call out to the host itself — a
slow or blocking built-in — since control never returns to the
script-stepping loop for the hook to check in. That's a different problem
and isn't solved here. For the actual threat model — runaway script code —
cooperative interruption at this granularity is enough.

Every call into a process goes through one guarded entry point that arms the
deadline, runs the call, disarms it, and classifies the outcome. This is
deliberate: nobody calling into a program should have to remember to wrap
that call in a timeout, so budget enforcement lives inside the process's own
calling convention rather than at each call site. One-shot module evaluation
— running a program's top-level code — goes through it budgeted generously,
since initialization can legitimately take longer than a frame. Every
per-frame call goes through the same entry point with a much tighter budget.

Because module evaluation is one bounded, synchronous guarded call, a
program's top-level code can't `await` anything whose resolution depends on
a later guarded call — a timer, a ticker, a draw handler, a message — since
none of those run until evaluation has returned. Elysium rejects such
top-level awaits at compile time rather than let them hang. A program that
needs timer-dependent work as part of starting up registers a post-init
handler ([1]) instead, which runs once evaluation has finished and those
calls are live.

Recognizing a timeout takes its own bookkeeping, because the engine doesn't
distinguish an interrupt-triggered exception from an ordinary thrown one in
any reliably inspectable way. Instead the interrupt hook itself records, at
the moment it decides to interrupt, that it was the one that did it — a
direct record of cause rather than a guess reconstructed from timing or
exception text.

## A process's turn

Each frame, the manager walks its table in order and gives every process a
turn.

On a process's very first turn, before anything else, it starts: the
manager resolves its entry path, evaluates its module, and runs its
post-init handlers. Nothing about a process runs when it is spawned — a
`spawn` only allocates the VM and puts an un-started entry in the table, so
the frame that does the spawning pays nothing for the new program's code.
Starting failures — a missing entry file, a top-level throw, a post-init
throw — drop the process through the same path as any other fault (below).

Then it drains the process's mailbox. Messages are queued, not delivered
synchronously at send time, so a `postMessage` from one process lands in the
recipient's mailbox and is handed to its message handler here, on the
recipient's own turn, under the same guarded call a timer callback gets. A
process that hasn't yet registered a message handler (via
`addMessageHandler`) is left with its mailbox untouched: messages that
arrive before the first handler is added wait there and are delivered once
one is, rather than being dropped.

Next it runs the timers that process has due — its `setTimeout`s and
`setInterval`s, and the `requestAnimationFrame` callbacks that update
tickers and draw handlers ride on.

Finally it decides whether to reap the process. A process is reaped when it
has explicitly ended itself by calling `exit()`, or when it has genuinely
run out of work: no pending timers, no queued microtasks, no unsettled
Promise jobs, an empty mailbox, and no registered message handler. That last
condition matters — a process with a handler registered has said it wants to
keep receiving, so a purely message-driven server process is kept alive
rather than reaped the instant its mailbox empties. It ends by calling
`exit()`, by `removeMessageHandler`-ing its way back to zero handlers (and
having nothing else pending), or by being asked to leave (see below). A
consequence of the "no unsettled jobs" wording: a Promise that never
resolves leaves nothing pending, so a process blocked only on one is
considered idle and reaped.

## Faults drop one process, not the kernel

A guarded call ends one of three ways. It can time out, meaning the
interrupt hook fired. It can throw an ordinary uncaught error. Or it can
fail an allocation against the process's 16 MB heap cap, which surfaces as
an ordinary exception. In every case the manager drops that one process —
logging a line saying which process and why — and moves on to the next. The
kernel and every other process keep running. A failure while a process is
starting up — its entry file missing, its module throwing at top level, a
post-init handler throwing — is the same: the process is already in the
table, so it goes through this exact drop path rather than a separate one.
This applies to the init process too: a fault in init drops init, which may
leave the table empty, no differently from a fault in anything else.

A timed-out process must never be called into again, but since the manager
removes it from the table in the same pass, nothing ever gets the chance.

## Ending a process

There is no `exit()` that one process can call on another — forcibly
stopping a process at an arbitrary point is the same hazard as
`pthread_cancel`, leaving half-updated state behind. Instead there are three
ways a process ends:

It ends **itself** by calling `exit()`. The manager reaps it at the end of
its current turn, after the call stack has unwound — cooperative, never
mid-instruction. `finally` blocks that haven't run yet don't run.

It is **asked to leave** via `requestExit(target)`. The kernel delivers an
`{ kind: "ely:exit" }` message to the target and starts a grace period. A
cooperative program handles that message — winds down, calls `exit()` — and
is reaped normally. One that ignores it is force-reaped once the grace
period elapses. Closing the window does this to every process at once, then
hard-stops after the grace period regardless.

It is **terminated** via `terminate(target)`. The kernel drops it at the end
of the frame, when no process is executing — no grace period, no `finally`
blocks. This is safe for the same reason `exit()` is: the process is removed
between turns, never while it holds the stack. It is the escalation when a
`requestExit` goes unanswered and you don't want to wait out the grace
period.

`requestExit` and `terminate` reach any live process by id; `postMessage`
does too. A process id is just a number — there is no handle object.

## Messages

A message is an envelope: `{ kind, from, to, data }`. `kind` is a label the
receiver switches on; `from` and `to` are process ids; `data` is the payload
(an `ely:container` `Option` — anything JSON round-trippable, or absent).
Kernel-originated messages use a `kind` under the `ely:` namespace (today
only `ely:exit`), and userland `postMessage` refuses to send an
`ely:`-prefixed `kind`, so a program can trust that an `ely:`-kinded message
genuinely came from the kernel. Sending to an id that isn't a live process
throws rather than silently vanishing.

## Spawning

`spawn(path, args)` allocates a process from a userland-virtual entry path
and returns its id. That is all it does synchronously — the id is usable
immediately (you can `postMessage` it right away), but the child's entry
path isn't resolved and its module isn't evaluated until its first turn on
the next frame. The `args` value is not sent as a message; it's stashed for
the child to read with `currentArguments()` during that first-turn
evaluation. A message sent to a just-spawned child before it has run simply
waits in its mailbox and is delivered on that first turn, right after the
child registers its handler. Because `spawn` runs no program code, a
runaway `spawn` chain can add at most one process per frame.

The process table is capped at 128 live processes. A `spawn` past the cap
is rejected and logged; the returned id is dead on arrival. The cap bounds
both a fork chain and total worst-case memory (128 processes × 16 MB).

## The shared screen and input

There is one framebuffer and one input device, shared by every process.
Draw commands from all processes go into a single buffer in the order their
draw handlers ran that frame, painter's algorithm, with the last
`clearScreen` winning; there is no compositor and no per-process layer.
Every process sees the same pointer and keyboard state — input is
broadcast, with no notion of focus. `setScale` writes the one shared
window's scale, last writer wins.

## Frame pacing

Elysium still targets 30 fps. A frame runs every process's turn back to
back; the event loop then sleeps for whatever is left of the frame's time
budget after that work. If the processes together overrun the budget, the
frame simply runs long — there is no accumulated debt and no catch-up burst
of short frames afterward.

## What this deliberately doesn't do

It doesn't run each process on its own thread — everything here is one
thread, with no concurrency primitives beyond the interrupt hook. It doesn't
forcibly abort a process mid-execution: `exit()`, the `requestExit` grace
period, and the between-turns `terminate` are all cooperative or deferred,
never a preemptive kill of running script code. A process wedged somewhere
the interrupt hook can't reach (the blocking-host-call case above) still has
no recourse; running processes on their own threads remains the option to
reach for if one ever needs to be forcibly reclaimed, and nothing here
forecloses it.

# References

[1] [Per-frame ticking](Lifecycle.md)
[2] [The Framebuffer](Framebuffer.md)
[3] Rich Harris, [Top-level `await` is a footgun](https://gist.github.com/Rich-Harris/0b6f317657f5167663b493c722647221) (2016; later edited to note TC39's revised design addressed the original concern for JS engines, which have a mitigation Elysium doesn't)
