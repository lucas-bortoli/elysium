// Confining drawing to a region, and the rule that makes clips composable:
// nesting one inside another can only ever narrow what is already in
// effect, never widen it.
//
// This example never reads Escape — that key belongs to the menu.

import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";

let elapsed = 0;
addUpdateTicker((dt) => {
  elapsed += dt;
});

addUpdateTicker(() => {
  screen.clear(Color.Slate900);
  screen.drawText(screen.width / 2, 8, "Clipping", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  screen.drawText(40, 42, "one clip", Color.Slate400);
  screen.strokeRectangle(40, 58, 180, 110, Color.Slate700, 1);
  screen.pushClip(40, 58, 180, 110);
  for (let i = 0; i < 40; i++) {
    screen.fillCircle(40 + ((i * 37 + elapsed * 60) % 200), 58 + ((i * 53) % 110), 16, Color.Teal500);
  }
  screen.popClip();

  screen.drawText(260, 42, "a second clip narrows to the overlap", Color.Slate400);
  screen.strokeRectangle(260, 58, 180, 110, Color.Slate700, 1);
  screen.pushClip(260, 58, 180, 110);
  // Half as wide as the first, and offset — only where the two agree does
  // anything land.
  const slide = Math.sin(elapsed) * 60;
  screen.pushClip(300 + slide, 58, 90, 110);
  for (let i = 0; i < 40; i++) {
    screen.fillCircle(260 + ((i * 37 + elapsed * 60) % 200), 58 + ((i * 53) % 110), 16, Color.Amber500);
  }
  screen.popClip();
  // Back inside the outer clip only: this band proves the inner one is
  // gone, and that the outer one still holds.
  screen.fillRectangle(200, 150, 320, 12, Color.Rose500);
  screen.popClip();

  screen.drawText(480, 42, "a clip that isn't a box", Color.Slate400);
  screen.strokeRectangle(480, 58, 180, 110, Color.Slate700, 1);
  screen.pushClip(480, 58, 180, 110);
  // Any path can be a clip. This one is a star, traced in place.
  screen.beginPath();
  for (let i = 0; i < 5; i++) {
    const angle = (i * 4 * Math.PI) / 5 - Math.PI / 2 + elapsed * 0.5;
    const x = 570 + Math.cos(angle) * 52;
    const y = 113 + Math.sin(angle) * 52;
    if (i === 0) screen.moveTo(x, y);
    else screen.lineTo(x, y);
  }
  screen.closePath();
  screen.pushClipPath();
  for (let i = 0; i < 24; i++) {
    screen.fillRectangle(480, 58 + i * 5, 180, 3, i % 2 ? Color.Teal400 : Color.Teal700);
  }
  screen.popClip();
  screen.popClip();

  screen.drawText(40, 196, "text is clipped like everything else", Color.Slate400);
  screen.strokeRectangle(40, 212, 620, 40, Color.Slate700, 1);
  const window = 200 + Math.sin(elapsed * 0.8) * 160;
  screen.pushClip(window, 212, 180, 40);
  screen.drawText(
    350,
    222,
    "this sentence is only visible through a moving window",
    Color.Amber300,
    { align: "center" },
  );
  screen.popClip();

  screen.drawText(40, 268, "and a clip moves with the transform it was pushed under", Color.Slate400);
  screen.strokeRectangle(40, 284, 620, 56, Color.Slate700, 1);
  screen.pushClip(40, 284, 620, 56);
  for (let i = 0; i < 10; i++) {
    const x = 60 + i * 64;
    screen.pushClip(x, 290, 44, 44);
    screen.fillCircle(x + 22, 312, 26, Color.Rose400);
    screen.fillCircle(x + 22, 312, 14, Color.Slate900);
    screen.popClip();
  }
  screen.popClip();
});
