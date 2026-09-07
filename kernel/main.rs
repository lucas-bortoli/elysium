mod bindings;
mod esm_resolver;
mod filesystem;
mod graphics;
mod image;
mod input;
mod process;
mod process_manager;
mod runtime;
mod sound;
mod text;
mod timers;
mod window;

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use graphics::{Display, SCREEN_ID, SurfaceTable};
use input::Input;
use process_manager::{GRACE, ProcessManager};
use runtime::Devices;
use window::ElysiumWindow;

pub mod transform;

fn main() {
    let exe_dir = std::env::current_exe()
        .expect("failed to locate the running binary")
        .parent()
        .expect("binary path has no parent directory")
        .to_path_buf();
    // Prefer the userland tree in the source checkout this binary was built
    // from, so editing a program never feeds into `cargo build` and forces a
    // relink. When that path is gone — a binary packaged and shipped
    // elsewhere — fall back to `userland` sitting beside the executable.
    let userland_root = {
        let in_source = Path::new(env!("CARGO_MANIFEST_DIR")).join("userland");
        if in_source.is_dir() {
            in_source
        } else {
            exe_dir.join("userland")
        }
    };

    let sound = sound::start().map(Rc::new);

    // The one kernel-wide surface table, seeded with the screen (id 0).
    // Every VM's `ely:graphics` bindings draw through it by id; the frame
    // loop below presents the screen entry once per tick.
    let surfaces = Rc::new(SurfaceTable::with_screen(
        graphics::SCREEN_WIDTH,
        graphics::SCREEN_HEIGHT,
    ));
    let scale = Rc::new(Cell::new(graphics::DEFAULT_SCALE));
    let input = Rc::new(Input::new(Rc::clone(&scale)));

    let devices = Devices::new(
        Rc::clone(&surfaces),
        Rc::clone(&input),
        Rc::clone(&scale),
        sound,
        userland_root,
    );
    let mut manager = ProcessManager::new(devices);

    // The init process is spawned like any other — a fault in it drops it
    // and empties the table, no different from a fault in a child.
    if let Err(err) = manager.spawn_from_path("/init.ts", None) {
        eprintln!("failed to start the init process: {err:?}");
    }
    if manager.is_empty() {
        eprintln!("no processes running; kernel exiting");
        return;
    }

    let manager = Rc::new(RefCell::new(manager));
    let close_deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));

    let mut display: Option<Display> = None;

    // The window is created before any process runs (init evaluates on
    // frame 1), so it opens at DEFAULT_SCALE and the screen surface's
    // initial size. A process that calls `setScale` or resizes the screen
    // during startup resizes the window live on that first frame via
    // `Display::present` rather than changing the initial size.
    ElysiumWindow::new(
        "Elysium",
        graphics::SCREEN_WIDTH * scale.get(),
        graphics::SCREEN_HEIGHT * scale.get(),
    )
    .run(
        {
            let input = Rc::clone(&input);
            move |event| input.handle_window_event(event)
        },
        {
            let manager = Rc::clone(&manager);
            let surfaces = Rc::clone(&surfaces);
            let scale = Rc::clone(&scale);
            let input = Rc::clone(&input);
            let mut fps_frames: u32 = 0;
            let mut fps_since = Instant::now();
            // Summed across the same window `fps_frames`/`fps_since` cover,
            // so the per-frame averages reported alongside FPS are over the
            // same frames — not just a snapshot of the one frame that
            // happened to cross the one-second mark.
            let mut tick_time = std::time::Duration::ZERO;
            let mut render_time = std::time::Duration::ZERO;
            move |window, _dt| {
                let display = display.get_or_insert_with(|| {
                    let (w, h) = surfaces
                        .dimensions(SCREEN_ID)
                        .expect("the screen surface is always present");
                    Display::new(window.clone(), Rc::clone(&scale), (w, h))
                });

                let tick_start = Instant::now();
                manager.borrow_mut().tick(tick_start);
                tick_time += tick_start.elapsed();

                // The screen surface already holds this tick's drawing —
                // every process drew straight onto it. All that's left is
                // to get it in front of the viewer.
                let render_start = Instant::now();
                surfaces
                    .with(SCREEN_ID, |screen| display.present(screen))
                    .expect("the screen surface is always present");
                render_time += render_start.elapsed();

                input.end_frame();

                // Report the average frame rate over the last second, split
                // into how much of each frame went to ticking every
                // process's VM versus rasterizing and presenting the result
                // — the two halves of the frame loop a future decision to
                // move either onto its own thread would be based on.
                fps_frames += 1;
                let elapsed = fps_since.elapsed();
                if elapsed >= std::time::Duration::from_secs(1) {
                    eprintln!(
                        "[fps] {:.1} (tick {:.2}ms, render {:.2}ms avg/frame)",
                        fps_frames as f64 / elapsed.as_secs_f64(),
                        tick_time.as_secs_f64() * 1000.0 / fps_frames as f64,
                        render_time.as_secs_f64() * 1000.0 / fps_frames as f64,
                    );
                    fps_frames = 0;
                    fps_since = Instant::now();
                    tick_time = std::time::Duration::ZERO;
                    render_time = std::time::Duration::ZERO;
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
