//! The Input device: the pointing device's state, kept up to date from raw
//! window events and read by `ely:input`'s bindings. `Cell`-based rather
//! than `Arc`/`Mutex`, same reasoning as `TimerQueue`/`GuardState` in
//! `runtime.rs`/`timers.rs` — nothing here crosses an OS thread.

mod keys;

use std::cell::Cell;
use std::rc::Rc;

use crate::bindings::bind;
use rquickjs::{Ctx, Result};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};

use keys::Key;

pub struct Input {
    /// Shared with the Framebuffer device's own `scale` cell (see
    /// `framebuffer::Framebuffer`) — the same physical-pixels-per-
    /// logical-pixel ratio a program can change via `setScale`, read here
    /// so `handle_window_event` converts physical pointer coordinates
    /// against whatever scale is actually in effect, not a stale one.
    scale: Rc<Cell<u32>>,
    /// The pointer's position in the framebuffer's logical 720x360 space —
    /// winit reports physical window pixels, divided by `scale` and
    /// floored to a logical pixel on the way in so a program never has to
    /// think about the window's actual physical size, or deal with a
    /// fractional pixel position.
    position: Cell<(i32, i32)>,
    is_down: Cell<bool>,
    was_pressed: Cell<bool>,
    was_released: Cell<bool>,
    /// Logical-space movement accumulated since the last `end_frame`.
    delta: Cell<(f32, f32)>,
    /// Scroll accumulated since the last `end_frame`.
    scroll_delta: Cell<f32>,
    /// Whether each `Key` is currently held, indexed by its discriminant.
    key_down: [Cell<bool>; Key::COUNT],
    /// Whether each `Key` was pressed or released this frame — edge-
    /// triggered, reset in `end_frame` the same way `was_pressed`/
    /// `was_released` are for the pointer's button.
    key_pressed: [Cell<bool>; Key::COUNT],
    key_released: [Cell<bool>; Key::COUNT],
}

impl Input {
    pub fn new(scale: Rc<Cell<u32>>) -> Self {
        Self {
            scale,
            position: Cell::new((0, 0)),
            is_down: Cell::new(false),
            was_pressed: Cell::new(false),
            was_released: Cell::new(false),
            delta: Cell::new((0.0, 0.0)),
            scroll_delta: Cell::new(0.0),
            key_down: std::array::from_fn(|_| Cell::new(false)),
            key_pressed: std::array::from_fn(|_| Cell::new(false)),
            key_released: std::array::from_fn(|_| Cell::new(false)),
        }
    }

    /// Updates state from one raw window event; events this device doesn't
    /// care about are ignored.
    pub fn handle_window_event(&self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale.get() as f64;
                let new_x = (position.x / scale).floor() as i32;
                let new_y = (position.y / scale).floor() as i32;
                let (old_x, old_y) = self.position.get();
                let (dx, dy) = self.delta.get();
                self.delta
                    .set((dx + (new_x - old_x) as f32, dy + (new_y - old_y) as f32));
                self.position.set((new_x, new_y));
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = *state == ElementState::Pressed;
                self.is_down.set(pressed);
                if pressed {
                    self.was_pressed.set(true);
                } else {
                    self.was_released.set(true);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32,
                };
                self.scroll_delta.set(self.scroll_delta.get() + amount);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let winit::keyboard::PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                self.handle_key_code(code, event.state == ElementState::Pressed, event.repeat);
            }
            _ => {}
        }
    }

    /// The `KeyboardInput` half of `handle_window_event`, taking the fields
    /// it needs directly rather than a `winit::event::KeyEvent` — that type
    /// has a private field, so tests (including `runtime.rs`'s end-to-end
    /// ones) can't construct one to drive this through `handle_window_event`.
    pub(crate) fn handle_key_code(
        &self,
        code: winit::keyboard::KeyCode,
        pressed: bool,
        repeat: bool,
    ) {
        let Some(key) = Key::from_key_code(code) else {
            return;
        };
        let index = key as usize;
        self.key_down[index].set(pressed);
        if pressed {
            // Auto-repeat re-fires `Pressed` while a key is held;
            // `key_pressed` should only flip on the initial press, same as
            // the pointer's `was_pressed`.
            if !repeat {
                self.key_pressed[index].set(true);
            }
        } else {
            self.key_released[index].set(true);
        }
    }

    /// Resets everything that accumulates over a frame (the edge-triggered
    /// pressed/released flags, movement delta, and scroll delta) — called
    /// once per rendered frame, after that frame's callbacks have had a
    /// chance to observe them.
    pub fn end_frame(&self) {
        self.was_pressed.set(false);
        self.was_released.set(false);
        self.delta.set((0.0, 0.0));
        self.scroll_delta.set(0.0);
        for cell in &self.key_pressed {
            cell.set(false);
        }
        for cell in &self.key_released {
            cell.set(false);
        }
    }
}

/// Binds the *hidden* globals `ely:input`'s embedded module wraps
/// (`__input_get_pointer_x`, etc.) — never called by a program directly,
/// only through `ely:input`'s exported functions.
pub fn bootstrap_input_bindings(ctx: &Ctx<'_>, input: Rc<Input>) -> Result<()> {
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_get_pointer_x", move || input.position.get().0)?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_get_pointer_y", move || input.position.get().1)?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_is_pointer_down", move || input.is_down.get())?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_was_pointer_pressed", move || {
            input.was_pressed.get()
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_was_pointer_released", move || {
            input.was_released.get()
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_get_pointer_delta_x", move || {
            input.delta.get().0
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_get_pointer_delta_y", move || {
            input.delta.get().1
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_get_scroll_delta", move || {
            input.scroll_delta.get()
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_is_key_down", move |id: u16| {
            Key::from_id(id).is_some_and(|key| input.key_down[key as usize].get())
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_was_key_pressed", move |id: u16| {
            Key::from_id(id).is_some_and(|key| input.key_pressed[key as usize].get())
        })?;
    }
    {
        let input = Rc::clone(&input);
        bind(ctx, "__input_was_key_released", move |id: u16| {
            Key::from_id(id).is_some_and(|key| input.key_released[key as usize].get())
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    use winit::event::DeviceId;

    use super::*;

    /// An `Input` backed by its own scale cell, fixed at
    /// `framebuffer::DEFAULT_SCALE` — none of these tests exercise a live
    /// `setScale` change, so a dedicated cell per test (rather than one
    /// shared across the whole module) keeps each test isolated.
    fn test_input() -> Input {
        Input::new(Rc::new(Cell::new(crate::framebuffer::DEFAULT_SCALE)))
    }

    fn cursor_moved(x: f64, y: f64) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(x, y),
        }
    }

    fn mouse_input(pressed: bool) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: if pressed {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
            button: MouseButton::Left,
        }
    }

    #[test]
    fn cursor_moved_updates_position_in_logical_space() {
        let input = test_input();
        input.handle_window_event(&cursor_moved(144.0, 72.0));
        assert_eq!(input.position.get(), (72, 36));
    }

    #[test]
    fn cursor_moved_floors_to_a_logical_pixel() {
        let input = test_input();
        input.handle_window_event(&cursor_moved(145.0, 71.0));
        assert_eq!(input.position.get(), (72, 35));
    }

    #[test]
    fn cursor_moved_accumulates_delta_in_logical_space() {
        let input = test_input();
        input.handle_window_event(&cursor_moved(100.0, 100.0));
        input.delta.set((0.0, 0.0)); // isolate movement after the initial jump
        input.handle_window_event(&cursor_moved(120.0, 90.0));
        assert_eq!(input.delta.get(), (10.0, -5.0));
    }

    #[test]
    fn press_sets_down_and_pressed() {
        let input = test_input();
        input.handle_window_event(&mouse_input(true));
        assert!(input.is_down.get());
        assert!(input.was_pressed.get());
        assert!(!input.was_released.get());
    }

    #[test]
    fn release_clears_down_and_sets_released() {
        let input = test_input();
        input.handle_window_event(&mouse_input(true));
        input.handle_window_event(&mouse_input(false));
        assert!(!input.is_down.get());
        assert!(input.was_released.get());
    }

    #[test]
    fn end_frame_clears_edge_state_and_deltas_but_not_down_or_position() {
        let input = test_input();
        input.handle_window_event(&cursor_moved(144.0, 72.0));
        input.handle_window_event(&mouse_input(true));

        input.end_frame();

        assert!(!input.was_pressed.get());
        assert!(!input.was_released.get());
        assert_eq!(input.delta.get(), (0.0, 0.0));
        assert_eq!(input.scroll_delta.get(), 0.0);
        assert!(input.is_down.get(), "is_down survives end_frame");
        assert_eq!(
            input.position.get(),
            (72, 36),
            "position survives end_frame"
        );
    }

    #[test]
    fn mouse_wheel_line_delta_accumulates() {
        let input = test_input();
        input.handle_window_event(&WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 1.5),
            phase: winit::event::TouchPhase::Moved,
        });
        input.handle_window_event(&WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 2.0),
            phase: winit::event::TouchPhase::Moved,
        });
        assert_eq!(input.scroll_delta.get(), 3.5);
    }

    #[test]
    fn mouse_wheel_pixel_delta_accumulates() {
        let input = test_input();
        input.handle_window_event(&WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 12.0)),
            phase: winit::event::TouchPhase::Moved,
        });
        assert_eq!(input.scroll_delta.get(), 12.0);
    }

    #[test]
    fn unrelated_events_are_ignored() {
        let input = test_input();
        input.handle_window_event(&WindowEvent::Resized(PhysicalSize::new(1440, 720)));
        assert_eq!(input.position.get(), (0, 0));
        assert!(!input.is_down.get());
    }

    #[test]
    fn key_press_sets_down_and_pressed() {
        let input = test_input();
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);
        let index = Key::KeyW as usize;
        assert!(input.key_down[index].get());
        assert!(input.key_pressed[index].get());
        assert!(!input.key_released[index].get());
    }

    #[test]
    fn key_release_clears_down_and_sets_released() {
        let input = test_input();
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, false, false);
        let index = Key::KeyW as usize;
        assert!(!input.key_down[index].get());
        assert!(input.key_released[index].get());
    }

    #[test]
    fn key_repeat_does_not_resend_pressed() {
        let input = test_input();
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);
        input.key_pressed[Key::KeyW as usize].set(false); // isolate the repeat
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, true);
        assert!(!input.key_pressed[Key::KeyW as usize].get());
        assert!(input.key_down[Key::KeyW as usize].get());
    }

    #[test]
    fn other_keys_are_unaffected() {
        let input = test_input();
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);
        assert!(!input.key_down[Key::KeyA as usize].get());
    }

    /// Edges say *that* a transition happened this frame, not how many. A
    /// key released and pressed again before the next frame reports both,
    /// and `key_down` is whatever the last event left — so a program that
    /// starts something on the press has to reconcile against `key_down`
    /// rather than assume one press pairs with one release.
    #[test]
    fn a_release_and_a_press_in_one_frame_both_report() {
        let input = test_input();
        let index = Key::KeyA as usize;

        input.handle_key_code(winit::keyboard::KeyCode::KeyA, true, false);
        input.end_frame();

        input.handle_key_code(winit::keyboard::KeyCode::KeyA, false, false);
        input.handle_key_code(winit::keyboard::KeyCode::KeyA, true, false);
        input.handle_key_code(winit::keyboard::KeyCode::KeyA, false, false);

        assert!(input.key_pressed[index].get(), "the press is reported");
        assert!(input.key_released[index].get(), "and so is the release");
        assert!(
            !input.key_down[index].get(),
            "and the key is up, since that is where the last event left it"
        );
    }

    #[test]
    fn end_frame_clears_key_edges_but_not_key_down() {
        let input = test_input();
        input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);

        input.end_frame();

        let index = Key::KeyW as usize;
        assert!(!input.key_pressed[index].get());
        assert!(!input.key_released[index].get());
        assert!(input.key_down[index].get(), "key_down survives end_frame");
    }
}
