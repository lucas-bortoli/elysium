// What drawText's options do: which edge the coordinate names, wrapping to
// a width, the gap between lines, and drawing larger.
//
// This example never reads Escape — that key belongs to the menu.

import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";

const PROSE =
  "Text is placed by its box rather than its baseline, so a program never has to know where a font's baseline sits to line something up against it.";

addUpdateTicker(() => {
  screen.clear(Color.Slate900);
  screen.drawText(screen.width / 2, 8, "Text", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  // A rule down the middle, to show that alignment is about which edge of
  // the text the given coordinate names.
  const middle = screen.width / 2;
  screen.drawLine(middle + 0.5, 44, middle + 0.5, 108, Color.Slate700, 1);

  screen.drawText(middle, 46, "align: left starts here", Color.Teal300);
  screen.drawText(middle, 46 + 20, "align: center is centred here", Color.Teal300, {
    align: "center",
  });
  screen.drawText(middle, 46 + 40, "align: right ends here", Color.Teal300, {
    align: "right",
  });

  screen.drawText(40, 120, "wrapping, and what measureText reports for it", Color.Slate400);

  // measureText lays the text out exactly as drawText will, so the box it
  // reports is the box the text fills — which is what makes it safe to
  // draw a background behind it.
  const wrapped = { maxWidth: 260, lineSpacing: 1.4 } as const;
  const box = screen.measureText(PROSE, wrapped);
  screen.fillRectangle(40, 138, box.width, box.height, Color.Slate800);
  screen.strokeRectangle(40, 138, box.width, box.height, Color.Slate600, 1);
  screen.drawText(40, 138, PROSE, Color.Slate200, wrapped);

  screen.drawText(330, 120, "line breaks are kept, spacing opens them up", Color.Slate400);
  const broken = "one\ntwo\nthree";
  screen.drawText(330, 138, broken, Color.Slate200);
  screen.drawText(400, 138, broken, Color.Slate200, { lineSpacing: 1.6 });
  screen.drawText(480, 138, broken, Color.Slate200, { lineSpacing: 2.2 });
  screen.drawText(330, 138 + 90, "1.0", Color.Slate500);
  screen.drawText(400, 138 + 90, "1.6", Color.Slate500);
  screen.drawText(480, 138 + 90, "2.2", Color.Slate500);

  screen.drawText(40, 258, "sizes are whole numbers, so bigger text stays as crisp as the font", Color.Slate400);

  // Each of these is the same bitmap with bigger pixels — nothing is
  // smoothed, and every pixel is still one palette color.
  let x = 40;
  for (const scale of [1, 2, 3, 4] as const) {
    screen.drawText(x, 278, "Aa", Color.Amber300, { scale });
    x += screen.measureText("Aa", { scale }).width + 16;
  }

  screen.drawText(300, 278, "centred and scaled together", Color.Rose400, {
    align: "center",
    scale: 2,
  });
});
