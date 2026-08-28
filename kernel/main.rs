mod esm_resolver;
mod filesystem;
mod framebuffer;
mod image;
mod input;
mod process;
mod process_manager;
mod runtime;
mod text;
mod timers;
mod window;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use framebuffer::Framebuffer;
use input::Input;
use process_manager::{GRACE, ProcessManager};
use window::ElysiumWindow;

pub mod transform;

fn main() {
    let exe_dir = std::env::current_exe()
        .expect("failed to locate the running binary")
        .parent()
        .expect("binary path has no parent directory")
        .to_path_buf();
    let userland_root = exe_dir.join("userland");

    let draw_commands = Rc::new(RefCell::new(Vec::new()));
    let scale = Rc::new(Cell::new(framebuffer::DEFAULT_SCALE));
    let input = Rc::new(Input::new(Rc::clone(&scale)));

    let mut manager = ProcessManager::new(
        Rc::clone(&draw_commands),
        Rc::clone(&input),
        Rc::clone(&scale),
        userland_root,
    );

    // The init process is spawned like any other — a fault in it drops it
    // and empties the table, no different from a fault in a child.
    if let Err(err) = manager.spawn_from_path("/programs/init/index.ts", None) {
        eprintln!("failed to start the init process: {err:?}");
    }
    if manager.is_empty() {
        eprintln!("no processes running; kernel exiting");
        return;
    }

    let manager = Rc::new(RefCell::new(manager));
    let close_deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let mut framebuffer: Option<Framebuffer> = None;

    // The window is created before any process runs (init evaluates on
    // frame 1), so it opens at DEFAULT_SCALE. A process that calls
    // `setScale` during startup resizes the window live on that first
    // frame via `Framebuffer::apply_scale` rather than changing the
    // initial size.
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
        {
            let manager = Rc::clone(&manager);
            let draw_commands = Rc::clone(&draw_commands);
            let scale = Rc::clone(&scale);
            let input = Rc::clone(&input);
            let mut fps_frames: u32 = 0;
            let mut fps_since = Instant::now();
            move |window, _dt| {
                let framebuffer = framebuffer
                    .get_or_insert_with(|| Framebuffer::new(window.clone(), Rc::clone(&scale)));

                manager.borrow_mut().tick(Instant::now());

                framebuffer.render(&draw_commands.borrow());
                draw_commands.borrow_mut().clear();
                input.end_frame();

                // Report the average frame rate over the last second.
                fps_frames += 1;
                let elapsed = fps_since.elapsed();
                if elapsed >= std::time::Duration::from_secs(1) {
                    eprintln!("[fps] {:.1}", fps_frames as f64 / elapsed.as_secs_f64());
                    fps_frames = 0;
                    fps_since = Instant::now();
                }
            }
        },
        {
            let manager = Rc::clone(&manager);
            let close_deadline = Rc::clone(&close_deadline);
            move || {
                if close_deadline.get().is_none() {
                    let now = Instant::now();
                    manager.borrow_mut().broadcast_exit(now);
                    close_deadline.set(Some(now + GRACE));
                }
            }
        },
        {
            let manager = Rc::clone(&manager);
            let close_deadline = Rc::clone(&close_deadline);
            move || {
                manager.borrow().is_empty()
                    || close_deadline
                        .get()
                        .is_some_and(|deadline| Instant::now() >= deadline)
            }
        },
    );
}
