// Tracing shapes by hand with the path calls, and the difference the fill
// rule makes where an outline crosses over itself.
//
// This example never reads Escape — that key belongs to the menu.

import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";

let elapsed = 0;
addUpdateTicker((dt) => {
  elapsed += dt;
});

/** Traces a five-pointed star as one closed contour. Stepping two fifths of
 * a turn at a time is what makes the outline cross itself, which is the
 * whole point here: the middle is enclosed twice over. */
function traceStar(cx: number, cy: number, radius: number) {
  screen.beginPath();
  for (let i = 0; i < 5; i++) {
    const angle = (i * 4 * Math.PI) / 5 - Math.PI / 2;
    const x = cx + Math.cos(angle) * radius;
    const y = cy + Math.sin(angle) * radius;
    if (i === 0) screen.moveTo(x, y);
    else screen.lineTo(x, y);
  }
  screen.closePath();
}

addUpdateTicker(() => {
  screen.clear(Color.Slate900);
  screen.drawText(screen.width / 2, 8, "Paths", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  screen.drawText(40, 44, "straight segments, a quadratic, and a cubic", Color.Slate400);

  // One contour using every kind of segment there is. The wobble is on the
  // control points only, so the curve moves while its ends stay put.
  const wobble = Math.sin(elapsed * 2) * 30;
  screen.beginPath();
  screen.moveTo(40, 130);
  screen.lineTo(100, 70);
  screen.quadraticTo(160, 70 + wobble, 220, 130);
  screen.cubicTo(260, 190 + wobble, 320, 70 - wobble, 380, 130);
  screen.lineTo(440, 70);
  screen.strokePath(Color.Teal300, 3, "round", "round");

  // The same path filled, to show that a fill closes what a stroke leaves
  // open: the ends are joined by a straight line that was never drawn.
  screen.beginPath();
  screen.moveTo(480, 130);
  screen.quadraticTo(540, 70 + wobble, 600, 130);
  screen.cubicTo(620, 150, 640, 90, 660, 130);
  screen.fillPath(Color.Teal900);
  screen.strokePath(Color.Teal300, 2, "round", "round");

  screen.drawText(40, 170, "the same star, filled two ways, then outlined", Color.Slate400);

  // Nonzero counts the middle as inside, because the outline winds around
  // it twice in the same direction. Evenodd alternates, so the second wind
  // takes it back out again and the middle is left hollow.
  traceStar(110, 250, 52);
  screen.fillPath(Color.Amber400);
  screen.drawText(110, 310, "nonzero", Color.Slate400, { align: "center" });

  traceStar(300, 250, 52);
  screen.fillPath(Color.Amber400, "evenodd");
  screen.drawText(300, 310, "evenodd", Color.Slate400, { align: "center" });

  // Filling leaves the path in place, so this one is stroked over its own
  // fill without being described again.
  traceStar(490, 250, 52);
  screen.fillPath(Color.Slate800);
  screen.strokePath(Color.Rose400, 2, "butt", "miter");
  screen.drawText(490, 310, "filled, then stroked", Color.Slate400, { align: "center" });
});
