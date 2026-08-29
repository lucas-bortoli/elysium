// Moving the coordinate space rather than doing the arithmetic on every
// shape, and what nesting one transform inside another gets you: a camera
// on the outside, an object placing itself on the inside, neither knowing
// about the other.
//
// This example never reads Escape — that key belongs to the menu.

import {
  Color,
  addDrawHandler,
  clearScreen,
  drawText,
  fillRectangle,
  fillRoundedRectangle,
  getWidth,
  popTransform,
  pushTransform,
  strokeRectangle,
} from "ely:framebuffer";
import { addUpdateTicker } from "ely:lifecycle";

let elapsed = 0;
addUpdateTicker((dt) => {
  elapsed += dt;
});

/** A little windmill drawn at the origin, in its own coordinates. It has no
 * idea where on screen it ends up — that's the caller's transform's job. */
function windmill(color: Color) {
  for (let i = 0; i < 4; i++) {
    pushTransform({ rotate: (i * Math.PI) / 2 });
    fillRoundedRectangle(6, -4, 26, 8, 3, color);
    popTransform();
  }
  fillRectangle(-3, -3, 6, 6, Color.Slate200);
}

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  drawText(getWidth() / 2, 8, "Transforms", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  drawText(40, 42, "the same windmill, drawn at the origin every time", Color.Slate400);

  // Shifted only.
  pushTransform({ translate: { x: 90, y: 110 } });
  windmill(Color.Teal400);
  popTransform();
  drawText(90, 155, "translate", Color.Slate500, { align: "center" });

  // Shifted and turned.
  pushTransform({ translate: { x: 220, y: 110 }, rotate: elapsed });
  windmill(Color.Teal400);
  popTransform();
  drawText(220, 155, "+ rotate", Color.Slate500, { align: "center" });

  // Shifted, turned and scaled. Scale applies first, then the turn, then
  // the shift, which is why it grows in place instead of drifting.
  const pulse = 1 + Math.sin(elapsed * 2) * 0.4;
  pushTransform({
    translate: { x: 360, y: 110 },
    rotate: elapsed,
    scale: pulse,
  });
  windmill(Color.Teal400);
  popTransform();
  drawText(360, 155, "+ scale", Color.Slate500, { align: "center" });

  // Squashed on one axis only.
  pushTransform({
    translate: { x: 500, y: 110 },
    rotate: elapsed,
    scale: { x: 1.6, y: 0.6 },
  });
  windmill(Color.Teal400);
  popTransform();
  drawText(500, 155, "uneven scale", Color.Slate500, { align: "center" });

  drawText(40, 190, "a camera on the outside, objects placing themselves inside it", Color.Slate400);

  // The camera. Everything below is drawn in world coordinates and knows
  // nothing about the shake.
  const shakeX = Math.sin(elapsed * 9) * 6;
  const shakeY = Math.cos(elapsed * 7) * 4;
  pushTransform({
    translate: { x: 360 + shakeX, y: 275 + shakeY },
    rotate: Math.sin(elapsed * 0.6) * 0.12,
  });

  strokeRectangle(-300, -60, 600, 120, Color.Slate700, 2);
  for (let i = 0; i < 7; i++) {
    const x = -250 + i * 83;
    // Each windmill nests its own transform inside the camera's, so it
    // spins about itself while the camera moves the lot.
    pushTransform({ translate: { x, y: 0 }, rotate: elapsed * (i % 2 ? -1 : 1) });
    windmill(i % 2 ? Color.Amber400 : Color.Rose400);
    popTransform();
  }

  popTransform();
});
