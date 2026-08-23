// The Input device: the pointing device's current state. A single button —
// isPointerDown/isPointerUp — plus scroll. Position and delta are in the
// framebuffer's logical coordinate space (see ely:framebuffer), not the
// window's physical pixels.

import type { Vector2d } from "ely:math";

declare function __input_get_pointer_x(): number;
declare function __input_get_pointer_y(): number;
declare function __input_is_pointer_down(): boolean;
declare function __input_was_pointer_pressed(): boolean;
declare function __input_was_pointer_released(): boolean;
declare function __input_get_pointer_delta_x(): number;
declare function __input_get_pointer_delta_y(): number;
declare function __input_get_scroll_delta(): number;

/** The pointer's current x position. */
export function getPointerX(): number {
  return __input_get_pointer_x();
}

/** The pointer's current y position. */
export function getPointerY(): number {
  return __input_get_pointer_y();
}

/** The pointer's current position. */
export function getPointerPosition(): Vector2d {
  return { x: getPointerX(), y: getPointerY() };
}

/** Whether the pointer's button is currently held down. */
export function isPointerDown(): boolean {
  return __input_is_pointer_down();
}

/** Whether the pointer's button is currently not held down. */
export function isPointerUp(): boolean {
  return !isPointerDown();
}

/** Whether the pointer's button was pressed this frame. */
export function wasPointerPressed(): boolean {
  return __input_was_pointer_pressed();
}

/** Whether the pointer's button was released this frame. */
export function wasPointerReleased(): boolean {
  return __input_was_pointer_released();
}

/** How far the pointer moved since last frame. */
export function getPointerDelta(): Vector2d {
  return { x: __input_get_pointer_delta_x(), y: __input_get_pointer_delta_y() };
}

/** How far the scroll wheel moved since last frame. */
export function getScrollDelta(): number {
  return __input_get_scroll_delta();
}
