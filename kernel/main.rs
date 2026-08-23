mod framebuffer;
mod runtime;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use framebuffer::Framebuffer;
use runtime::{ElysiumRuntime, GuardedError};
use window::ElysiumWindow;

pub mod transform;

fn main() {
    let path = "userland/programs/init/index.ts";
    let program =
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path}: {err}"));

    let draw_commands = Rc::new(RefCell::new(Vec::new()));
    let runtime =
        ElysiumRuntime::new(Rc::clone(&draw_commands)).expect("failed to initialize Elysium runtime");

    match runtime.eval_module(path, &program) {
        Ok(()) => {}
        Err(GuardedError::Timeout) => {
            eprintln!("program timed out during initialization");
            std::process::exit(1);
        }
        Err(GuardedError::Exception(err)) => {
            eprintln!("uncaught exception: {err}");
            std::process::exit(1);
        }
    }

    let mut framebuffer: Option<Framebuffer> = None;

    ElysiumWindow::new("Elysium", 1280, 720).run(|window, dt| {
        let framebuffer = framebuffer.get_or_insert_with(|| Framebuffer::new(window.clone()));

        let dt_seconds = dt.as_secs_f64();
        if let Err(err) = runtime.call_function("update", (dt_seconds,)) {
            report_frame_error("update", err);
        }
        if let Err(err) = runtime.call_function("draw", ()) {
            report_frame_error("draw", err);
        }

        framebuffer.render(&draw_commands.borrow());
        draw_commands.borrow_mut().clear();
    });
}

/// A callback timing out or throwing doesn't have a second program to fall
/// back to yet (Elysium runs one program per kernel today), so — per the
/// "VM is destroyed, but Elysium soldiers on" contract — this reports the
/// failure and exits, the same as a failed `eval_module` does, rather than
/// silently continuing to call into a VM the timeout has already poisoned.
fn report_frame_error(callback: &str, err: GuardedError) {
    match err {
        GuardedError::Timeout => {
            eprintln!("program timed out inside {callback}()");
        }
        GuardedError::Exception(err) => {
            eprintln!("uncaught exception in {callback}(): {err}");
        }
    }
    std::process::exit(1);
}
