// The input device: where the pointer is, what it's doing, how far the
// wheel has turned, and which keys are down.
//
// The distinction worth watching here is held versus pressed. "Held" is
// true for as long as you hold the button; "pressed" is true for the single
// frame it goes down. One is a state, the other an event, and mixing them
// up is where input bugs come from.
//
// This example never reads Escape — that key belongs to the menu, which is
// still running while this draws. There is no input focus in Elysium: this
// program and the menu are seeing exactly the same key presses.

import {
  Color,
  addDrawHandler,
  clearScreen,
  drawLine,
  drawPolyline,
  drawText,
  fillCircle,
  fillRectangle,
  fillRoundedRectangle,
  getHeight,
  getWidth,
  strokeCircle,
} from "ely:framebuffer";
import type { Vector2d } from "ely:math";
import {
  Key,
  getPointerDelta,
  getPointerPosition,
  getScrollDelta,
  isKeyDown,
  isPointerDown,
  wasKeyPressed,
  wasPointerPressed,
  wasPointerReleased,
} from "ely:input";
import { addUpdateTicker } from "ely:lifecycle";

/** The keys drawn as a little keyboard, so there is something to hold down
 * and something to tap. Escape is deliberately not among them. */
const WATCHED = [
  { key: Key.KeyW, label: "W", x: 1, y: 0 },
  { key: Key.KeyA, label: "A", x: 0, y: 1 },
  { key: Key.KeyS, label: "S", x: 1, y: 1 },
  { key: Key.KeyD, label: "D", x: 2, y: 1 },
  { key: Key.Space, label: "Space", x: 3.4, y: 1, wide: true },
] as const;

const trail: Vector2d[] = [];
let scrolled = 0;
/** Counts the frame a press happens on, to show an edge is a single frame
 * however long you hold the button. */
let clicks = 0;
let releases = 0;
const taps = new Map<number, number>();

addUpdateTicker(() => {
  trail.push(getPointerPosition());
  if (trail.length > 60) trail.shift();

  scrolled += getScrollDelta();
  if (wasPointerPressed()) clicks++;
  if (wasPointerReleased()) releases++;
  for (const watched of WATCHED) {
    if (wasKeyPressed(watched.key)) {
      taps.set(watched.key, (taps.get(watched.key) ?? 0) + 1);
    }
  }
});

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  drawText(getWidth() / 2, 8, "Input", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  const pointer = getPointerPosition();
  const delta = getPointerDelta();

  // Where the pointer has been, then where it is. The ring grows while the
  // button is held, which is the state; the count below it only moves on
  // the frame the button goes down, which is the event.
  if (trail.length > 1) drawPolyline(trail, Color.Slate700, 1);
  drawLine(0, pointer.y + 0.5, getWidth(), pointer.y + 0.5, Color.Slate800, 1);
  drawLine(pointer.x + 0.5, 0, pointer.x + 0.5, getHeight(), Color.Slate800, 1);
  strokeCircle(pointer.x, pointer.y, isPointerDown() ? 18 : 10, Color.Amber400, 2);
  fillCircle(pointer.x, pointer.y, 3, Color.Amber300);

  drawText(40, 46, "pointer", Color.Slate400);
  drawText(40, 62, `position  ${Math.round(pointer.x)}, ${Math.round(pointer.y)}`, Color.Slate200);
  drawText(40, 78, `moved     ${Math.round(delta.x)}, ${Math.round(delta.y)}`, Color.Slate200);
  drawText(
    40,
    94,
    `button    ${isPointerDown() ? "held" : "up"}`,
    isPointerDown() ? Color.Teal300 : Color.Slate500,
  );
  drawText(40, 110, `pressed   ${clicks} times`, Color.Slate200);
  drawText(40, 126, `released  ${releases} times`, Color.Slate200);

  // The wheel reports how far it turned this frame, so a program that wants
  // a running total keeps one itself, the way this does.
  drawText(300, 46, "wheel", Color.Slate400);
  drawText(300, 62, `total  ${scrolled.toFixed(1)}`, Color.Slate200);
  const barY = 82;
  fillRectangle(300, barY, 200, 10, Color.Slate800);
  const knob = Math.max(0, Math.min(190, 95 + scrolled * 8));
  fillRectangle(300 + knob, barY, 10, 10, Color.Rose400);

  drawText(300, 110, "this frame", Color.Slate500);
  drawText(300, 126, getScrollDelta().toFixed(2), Color.Slate200);

  drawText(40, 160, "keys — filled while held, counted on each press", Color.Slate400);
  for (const watched of WATCHED) {
    const w = watched.wide ? 92 : 40;
    const x = 40 + watched.x * 48;
    const y = 182 + watched.y * 48;
    const down = isKeyDown(watched.key);
    fillRoundedRectangle(x, y, w, 40, 6, down ? Color.Teal500 : Color.Slate800);
    drawText(x + w / 2, y + 8, watched.label, down ? Color.Slate900 : Color.Slate300, {
      align: "center",
    });
    drawText(
      x + w / 2,
      y + 24,
      `${taps.get(watched.key) ?? 0}`,
      down ? Color.Slate900 : Color.Slate500,
      { align: "center" },
    );
  }

  drawText(
    getWidth() - 40,
    getHeight() - 24,
    "the menu is still running, and sees these same keys",
    Color.Slate600,
    { align: "right" },
  );
});
