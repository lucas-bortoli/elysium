mod esm_resolver;
mod framebuffer;
mod input;
mod runtime;
mod timers;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use framebuffer::Framebuffer;
use input::Input;
use runtime::{ElysiumRuntime, GuardedError};
use window::ElysiumWindow;

pub mod transform;

fn main() {
    let exe_dir = std::env::current_exe()
        .expect("failed to locate the running binary")
        .parent()
        .expect("binary path has no parent directory")
        .to_path_buf();
    let path = exe_dir.join("userland/programs/init/index.ts");
    let program = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let path = path.to_str().expect("binary path is not valid UTF-8");

    let draw_commands = Rc::new(RefCell::new(Vec::new()));
    let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
    let input = Rc::new(Input::new(Rc::clone(&scale)));
    let runtime = ElysiumRuntime::new(
        Rc::clone(&draw_commands),
        Rc::clone(&input),
        Rc::clone(&scale),
    )
    .expect("failed to initialize Elysium runtime");

    if let Err(err) = runtime.eval_module(path, &program) {
        report_frame_error("module evaluation", err);
    }
    if let Err(err) = runtime.run_post_init_handlers() {
        report_frame_error("addPostInitHandler callback", err);
    }

    let mut framebuffer: Option<Framebuffer> = None;

    // Reflects whatever `scale` was set to by the time evaluation/post-init
    // handlers finished, so a program that calls `setScale` during startup
    // gets a window created at the right size from the start, rather than
    // opening at DEFAULT_SCALE and immediately resizing.
    ElysiumWindow::new(
        "Elysium",
        framebuffer::FRAMEBUFFER_WIDTH * scale.get(),
        framebuffer::FRAMEBUFFER_HEIGHT * scale.get(),
    )
    .run(
        {
            let input = Rc::clone(&input);
            move |event| input.handle_window_event(event)
        },
        |window, _dt| {
            let framebuffer = framebuffer
                .get_or_insert_with(|| Framebuffer::new(window.clone(), Rc::clone(&scale)));

            if let Err(err) = runtime.run_due_timers() {
                report_frame_error("timer", err);
            }

            framebuffer.render(&draw_commands.borrow());
            draw_commands.borrow_mut().clear();
            input.end_frame();
        },
    );
}

/// A call into the VM timing out or throwing doesn't have a second program
/// to fall back to yet (Elysium runs one program per kernel today), so —
/// per the "VM is destroyed, but Elysium soldiers on" contract — this
/// reports the failure and exits, rather than silently continuing to call
/// into a VM a timeout may have already poisoned.
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
