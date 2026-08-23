# Multitasking: keeping one program from hanging Elysium

> The VM is destroyed, but Elysium soldiers on.

Elysium runs one program per VM, and that isolation isn't a one-shot thing:
the kernel calls back into a running program repeatedly over its lifetime,
the way `love.update`/`love.draw` callbacks, message handlers, or timers
would. Any single one of those calls has to be boundable, because if a
program's script code enters an infinite loop or a runaway computation, that
can't be allowed to hang the call — and by extension the kernel, and by
extension the rest of Elysium.

The JS engine supports installing a hook that it checks periodically while
running script code, roughly every 10,000 loop-iteration or function-call
steps. If that hook signals "stop," the engine immediately raises an
exception that unwinds the entire evaluation and cannot be caught by the
program's own `try`/`catch`. Elysium installs one such hook per VM, for its
whole lifetime, but it doesn't fire on a fixed schedule — it checks a
deadline that gets armed immediately before a guarded call begins and
disarmed immediately after, so between calls no deadline is armed and the
hook does nothing.

This is cooperative, not preemptive: it can only interrupt a VM that's
actually still stepping through script code. Ordinary infinite loops
(`while (true) {}`, runaway recursion, and the like) are stepping
constantly, so they're reliably caught. What this doesn't protect against is
a VM stuck inside a single long-running call out to the host itself — a slow
or blocking built-in — since control never returns to the script-stepping
loop for the hook to check in. That's a different problem, a slow or
blocking host binding, and isn't solved here. For the actual threat model —
runaway script code — cooperative interruption at this granularity is
enough.

Every call into a VM goes through one guarded entry point that arms the
deadline, runs the call, disarms the deadline, and classifies the outcome.
This is deliberate: a kernel author calling into a program should never have
to remember to wrap that call in a timeout themselves, so budget enforcement
lives inside the VM's own calling convention rather than in each call site.
One-shot module evaluation — running a program's top-level code — goes
through it budgeted generously, since program initialization can
legitimately take longer than a single frame. Every per-frame call into a
program goes through the same entry point too, just with a much tighter
budget: whichever timer callback is due to run that frame, whether a
program's own `setTimeout`, an update ticker ([1]), or a draw handler ([2])
— no separate timeout logic was needed for any of them, just a new budget
and a different call inside.

The VM is destroyed, but Elysium soldiers on: a guarded call ends one of two
ways. It can time out, meaning the interrupt hook fired, in which case the
VM is considered poisoned from that point on and must be torn down, never
reused for another call — the caller, today Elysium's startup and
eventually the kernel, is expected to continue running everything else, so
one program hanging doesn't take the rest of the system down with it. Or it
can throw normally, an ordinary uncaught program error, which gets no
special handling beyond normal error reporting and doesn't poison the VM.
Tearing the VM down on timeout isn't automatic, since the guarded call
itself has no way to know whether it's being driven by a one-shot startup
run today or a long-lived per-frame kernel loop tomorrow — that decision is
left to whoever owns the VM.

Recognizing a timeout took its own bookkeeping, because the engine doesn't
distinguish an interrupt-triggered exception from an ordinary thrown one in
any way that's reliably inspectable afterward — both surface as the same
kind of exception, and the one the engine raises on interrupt could in
principle look just like something a program threw itself, so matching its
name or message after the fact would be unreliable. Instead the interrupt
hook itself records, at the exact moment it decides to interrupt, that it
was the one that did it — a direct record of cause rather than a guess
reconstructed afterward from timing or exception content.

This deliberately doesn't do a few things. It doesn't run each VM on its own
thread — everything here runs on a single thread, with no concurrency
primitives beyond the interrupt hook. It doesn't forcefully kill a VM either:
the interrupt hook is the only signal available, and there's no way to reach
into a running VM from outside and abort it instantly. If a VM ever gets
stuck somewhere the hook can't reach (the blocking-host-call case above),
the only recourse would be running that VM on its own thread and abandoning
it — not implemented, and not needed for the runaway-script-code threat
model this addresses. It also doesn't enforce a memory limit; a separate,
already-available mechanism covers that and isn't part of this document.
Running each VM on its own thread remains an option to reach for later if a
VM ever needs to be forcibly reclaimed rather than just cooperatively
interrupted — nothing here forecloses it, it just isn't required for the
failure mode this design targets.

# References

[1] [Per-frame ticking](Loop.md)
[2] [The Framebuffer](Framebuffer.md)
