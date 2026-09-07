// Every shape the framebuffer can draw, filled on the left of each pair and
// outlined on the right, so the difference between the two is visible.
//
// Like every example, this one never reads Escape — that key belongs to the
// menu, which is still running and listening while this draws.

import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";

let elapsed = 0;
addUpdateTicker((dt) => {
  elapsed += dt;
});

/** A five-pointed star, so the polygons have an outline that crosses itself
 * and a fill rule worth arguing about. */
function star(cx: number, cy: number, radius: number) {
  const points = [];
  for (let i = 0; i < 5; i++) {
    // Two fifths of a turn per step is what makes the outline cross over.
    const angle = (i * 4 * Math.PI) / 5 - Math.PI / 2;
    points.push({
      x: cx + Math.cos(angle) * radius,
      y: cy + Math.sin(angle) * radius,
    });
  }
  return points;
}

function label(x: number, y: number, text: string) {
  screen.drawText(x, y, text, Color.Slate400);
}

addUpdateTicker(() => {
  screen.clear(Color.Slate900);
  screen.drawText(screen.width / 2, 8, "Shapes", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  const fill = Color.Teal400;
  const line = Color.Amber400;

  label(40, 44, "rectangle");
  screen.fillRectangle(40, 58, 54, 40, fill);
  screen.strokeRectangle(104, 58, 54, 40, line, 2);

  label(180, 44, "rounded");
  screen.fillRoundedRectangle(180, 58, 54, 40, 10, fill);
  screen.strokeRoundedRectangle(244, 58, 54, 40, 10, line, 2);

  label(320, 44, "circle");
  screen.fillCircle(347, 78, 20, fill);
  screen.strokeCircle(411, 78, 20, line, 2);

  label(460, 44, "ellipse");
  screen.fillEllipse(487, 78, 27, 20, fill);
  screen.strokeEllipse(551, 78, 27, 20, line, 2);

  label(600, 44, "triangle");
  screen.fillTriangle(
    { x: 600, y: 98 },
    { x: 627, y: 58 },
    { x: 654, y: 98 },
    fill,
  );

  label(40, 120, "polygon (nonzero / evenodd / outline)");
  screen.fillPolygon(star(67, 168, 32), fill);
  screen.fillPolygon(star(157, 168, 32), fill, "evenodd");
  screen.strokePolygon(star(247, 168, 32), line, 2);

  label(320, 120, "line and polyline");
  screen.drawLine(320, 140, 460, 200, line, 3);
  // Half-pixel offsets put a one-thick line down the middle of a pixel
  // column instead of straddling two, which is what keeps it crisp.
  screen.drawLine(320.5, 205.5, 460.5, 205.5, Color.Slate500, 1);
  const wave = [];
  for (let i = 0; i <= 28; i++) {
    wave.push({
      x: 480 + i * 7,
      y: 170 + Math.sin(i / 3 + elapsed * 3) * 26,
    });
  }
  screen.drawPolyline(wave, Color.Rose400, 2);

  label(40, 224, "arc — swept either way round");
  const sweep = (Math.sin(elapsed) * 0.5 + 0.5) * Math.PI * 2;
  screen.drawArc(90, 288, 34, 0, sweep, Color.Amber400, 8);
  screen.drawArc(200, 288, 34, 0, -sweep, Color.Teal400, 8);
  // A ring the sweeping arcs are measured against.
  screen.strokeCircle(310, 288, 34, Color.Slate700, 8);
  screen.drawArc(310, 288, 34, -Math.PI / 2, -Math.PI / 2 + sweep, Color.Rose400, 8);
});
