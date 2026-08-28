//! The `ely:input` surface: pointer and keyboard state derived from injected
//! window events.

use super::*;

#[test]
fn pointer_position_reflects_injected_cursor_moved_events() {
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, WindowEvent};

    let (runtime, input) = eval_with_input(
        "import { getPointerX, getPointerY } from 'ely:input'; \
         globalThis.x = getPointerX(); \
         globalThis.y = getPointerY();",
    );
    assert_eq!(global::<f64>(&runtime, "x"), 0.0);
    assert_eq!(global::<f64>(&runtime, "y"), 0.0);

    input.handle_window_event(&WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(144.0, 72.0),
    });
    runtime
        .eval_module(
            "test2.ts",
            "import { getPointerX, getPointerY } from 'ely:input'; \
             globalThis.x = getPointerX(); \
             globalThis.y = getPointerY();",
        )
        .unwrap();
    assert_eq!(global::<f64>(&runtime, "x"), 72.0);
    assert_eq!(global::<f64>(&runtime, "y"), 36.0);
}

#[test]
fn pointer_down_and_pressed_reflect_injected_mouse_input_events() {
    use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};

    let (runtime, input) = eval_with_input(
        "import { isPointerDown, isPointerUp, wasPointerPressed, wasPointerReleased } from 'ely:input'; \
         globalThis.readState = () => ({ \
             down: isPointerDown(), \
             up: isPointerUp(), \
             pressed: wasPointerPressed(), \
             released: wasPointerReleased(), \
         });",
    );

    input.handle_window_event(&WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Pressed,
        button: MouseButton::Left,
    });
    runtime
        .eval_module(
            "check1.ts",
            "const s = globalThis.readState(); \
             globalThis.down1 = s.down; \
             globalThis.up1 = s.up; \
             globalThis.pressed1 = s.pressed;",
        )
        .unwrap();
    assert!(global::<bool>(&runtime, "down1"));
    assert!(!global::<bool>(&runtime, "up1"));
    assert!(global::<bool>(&runtime, "pressed1"));

    input.end_frame();
    input.handle_window_event(&WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
    });
    runtime
        .eval_module(
            "check2.ts",
            "const s = globalThis.readState(); \
             globalThis.down2 = s.down; \
             globalThis.released2 = s.released;",
        )
        .unwrap();
    assert!(!global::<bool>(&runtime, "down2"));
    assert!(global::<bool>(&runtime, "released2"));
}

#[test]
fn key_down_and_pressed_reflect_injected_keyboard_events() {
    let (runtime, input) = eval_with_input(
        "import { Key, isKeyDown, isKeyUp, wasKeyPressed, wasKeyReleased } from 'ely:input'; \
         globalThis.readState = () => ({ \
             down: isKeyDown(Key.KeyW), \
             up: isKeyUp(Key.KeyW), \
             pressed: wasKeyPressed(Key.KeyW), \
             released: wasKeyReleased(Key.KeyW), \
         });",
    );

    input.handle_key_code(winit::keyboard::KeyCode::KeyW, true, false);
    runtime
        .eval_module(
            "check1.ts",
            "const s = globalThis.readState(); \
             globalThis.down1 = s.down; \
             globalThis.up1 = s.up; \
             globalThis.pressed1 = s.pressed;",
        )
        .unwrap();
    assert!(global::<bool>(&runtime, "down1"));
    assert!(!global::<bool>(&runtime, "up1"));
    assert!(global::<bool>(&runtime, "pressed1"));

    input.end_frame();
    input.handle_key_code(winit::keyboard::KeyCode::KeyW, false, false);
    runtime
        .eval_module(
            "check2.ts",
            "const s = globalThis.readState(); \
             globalThis.down2 = s.down; \
             globalThis.released2 = s.released;",
        )
        .unwrap();
    assert!(!global::<bool>(&runtime, "down2"));
    assert!(global::<bool>(&runtime, "released2"));
}
