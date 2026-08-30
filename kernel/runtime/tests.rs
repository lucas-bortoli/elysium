//! Integration tests for the `ely:` device surfaces this runtime wires into
//! each VM. Split by subject into the submodules below; the shared
//! VM-construction and inspection helpers live here.
//!
//! A test belongs here when it exercises a surface as a program sees it —
//! the JS is the input, and what it observes is the assertion. A test that
//! exercises a device's own internals, with no JS involved, belongs in a
//! `mod tests` inside that device's module instead: the clip and transform
//! algebra in `framebuffer/state.rs`, path geometry in `framebuffer/paths.rs`,
//! window-event folding in `input.rs`, glyph metrics in `text.rs`. The two
//! layers are complementary, and a mechanism is usually worth covering at
//! both: `ely:process`'s bindings are tested here against a detached runtime,
//! while the scheduling they feed — spawning, mailboxes, reaping, faults — is
//! tested against a real process table in `process_manager.rs`.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use rquickjs::FromJs;

use crate::framebuffer;
use crate::input::Input;
use crate::process::ProcessChannel;

pub(super) use super::{Devices, ElysiumRuntime, GuardedError, remap_deadlock_error};

use crate::sound::{Sound, SoundLog};

mod container;
mod filesystem;
mod graphics;
mod image;
mod input;
mod lifecycle;
mod modules;
mod path_helpers;
mod process;
mod sound;
mod text;
mod timers;

/// Builds an `ElysiumRuntime` against `root` with a fresh scale cell,
/// `Input`, and a detached `Sound`, apart from any process (id 0, a
/// throwaway channel, no arguments).
///
/// The returned [`SoundLog`] owns the receiving end of the audio command
/// channel, so it has to outlive the runtime — drop it first and every
/// later `playTone` reports the audio thread as gone. Helpers that discard
/// it are fine only because their tests never play anything.
fn build_runtime(root: PathBuf) -> (ElysiumRuntime, Rc<Input>, SoundLog) {
    let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
    let input = Rc::new(Input::new(Rc::clone(&scale)));
    let (audio, audio_log) = Sound::detached();
    let devices = Devices::new(
        Rc::new(RefCell::new(Vec::new())),
        Rc::clone(&input),
        scale,
        Some(Rc::new(audio)),
        root,
    );
    let runtime = ElysiumRuntime::new(&devices, 0, ProcessChannel::new(), None)
        .expect("failed to construct runtime");
    (runtime, input, audio_log)
}

/// A fresh VM with the entry module already evaluated from `source`. Test
/// programs report results by assigning onto `globalThis` — the simplest
/// way for a plain script body to leave something [`global`] can read back.
fn eval(source: &str) -> ElysiumRuntime {
    eval_with_input(source).0
}

/// Like [`eval`], but also hands back the `Input` device backing the VM, so
/// a test can feed it window events before or after evaluating.
fn eval_with_input(source: &str) -> (ElysiumRuntime, Rc<Input>) {
    eval_named_with_input("test.ts", source)
}

/// Like [`eval_with_input`], but lets a test pick the entry module's own
/// name — needed to exercise `import.meta.directoryName`/`fileName`, which
/// are only set when `name` canonicalizes to somewhere inside
/// [`test_userland_root`].
fn eval_named_with_input(name: &str, source: &str) -> (ElysiumRuntime, Rc<Input>) {
    let (runtime, input, _audio) = build_runtime(test_userland_root());
    runtime
        .eval_module(name, source)
        .expect("module failed to evaluate");
    (runtime, input)
}

/// Like [`eval`], but against `root` instead of [`test_userland_root`] —
/// needed by `ely:filesystem` tests that mutate the filesystem and so must
/// run against a private [`test_scratch_root`].
fn eval_with_root(root: PathBuf, source: &str) -> ElysiumRuntime {
    let (runtime, _input, _audio) = build_runtime(root);
    runtime
        .eval_module("test.ts", source)
        .expect("module failed to evaluate");
    runtime
}

/// The default `userland_root` for tests that don't need a writable one: a
/// fixed fixtures directory holding a small real PNG (`test.png`), a small
/// real module (`meta_module.ts`), and a relative-import target.
/// `kernel/framebuffer.rs`, two levels up, is a real file outside the
/// directory a path-traversal test can point at.
fn test_userland_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("kernel/image/fixtures")
}

/// A fresh, writable, uniquely-named directory under the OS temp dir, for
/// `ely:filesystem` tests that write or delete rather than only read.
/// [`test_userland_root`] is a single git-tracked directory shared by every
/// test — fine for read-only use, but `cargo test` runs tests concurrently
/// with no locking, so a mutating test needs its own root to avoid racing
/// another.
fn test_scratch_root() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "elysium-filesystem-test-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("failed to create scratch root");
    dir
}

/// Like [`eval`], but hands back the log of everything the VM asked the audio
/// device to do, for `ely:sound`'s tests. The log must outlive the runtime —
/// see [`build_runtime`].
fn eval_with_audio(source: &str) -> (ElysiumRuntime, SoundLog) {
    let (runtime, _input, audio_log) = build_runtime(test_userland_root());
    runtime
        .eval_module("test.ts", source)
        .expect("module failed to evaluate");
    (runtime, audio_log)
}

/// A VM built against a machine with no working output device, for the
/// bindings' silent-no-op path.
fn eval_without_audio(source: &str) -> ElysiumRuntime {
    let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
    let input = Rc::new(Input::new(Rc::clone(&scale)));
    let devices = Devices::new(
        Rc::new(RefCell::new(Vec::new())),
        input,
        scale,
        None,
        test_userland_root(),
    );
    let runtime = ElysiumRuntime::new(&devices, 0, ProcessChannel::new(), None)
        .expect("failed to construct runtime");
    runtime
        .eval_module("test.ts", source)
        .expect("module failed to evaluate");
    runtime
}

/// Reads `globalThis[name]` out of a VM and converts it to `T`.
fn global<T>(runtime: &ElysiumRuntime, name: &str) -> T
where
    T: for<'js> FromJs<'js>,
{
    runtime
        .context
        .with(|ctx| ctx.globals().get::<_, T>(name))
        .expect("failed to read global")
}
