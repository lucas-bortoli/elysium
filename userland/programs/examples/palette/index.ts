// The whole palette. Every colour any program can draw with is one of these
// — there is no way to name a colour that isn't here, which is what keeps
// what's on screen consistent between programs.
//
// This example never reads Escape — that key belongs to the menu.

import {
  Color,
  addDrawHandler,
  clearScreen,
  drawText,
  fillRectangle,
  getWidth,
  strokeRectangle,
} from "ely:framebuffer";
import { getPointerPosition } from "ely:input";

// The 26 hue families, in the order their ids run. Each has eleven shades,
// from 50 to 950, and Black and White sit on the end on their own.
const FAMILIES = [
  "Red", "Orange", "Amber", "Yellow", "Lime", "Green", "Emerald", "Teal",
  "Cyan", "Sky", "Blue", "Indigo", "Violet", "Purple", "Fuchsia", "Pink",
  "Rose", "Slate", "Gray", "Zinc", "Neutral", "Stone", "Taupe", "Mauve",
  "Mist", "Olive",
] as const;
const SHADES = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;

const CELL_W = 23;
const CELL_H = 10;
const GRID_X = 62;
const GRID_Y = 46;

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  drawText(getWidth() / 2, 6, "Palette", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  const pointer = getPointerPosition();
  let hovered = "";

  for (const [family, name] of FAMILIES.entries()) {
    const y = GRID_Y + family * CELL_H;
    drawText(GRID_X - 6, y + 1, name, Color.Slate500, {
      align: "right",
    });

    for (const [shade, level] of SHADES.entries()) {
      const x = GRID_X + shade * CELL_W;
      // Ids run family by family, eleven shades each, in this same order —
      // so the id is just the position in the grid.
      const id = (family * SHADES.length + shade) as Color;
      fillRectangle(x, y, CELL_W - 1, CELL_H - 1, id);

      if (
        pointer.x >= x &&
        pointer.x < x + CELL_W - 1 &&
        pointer.y >= y &&
        pointer.y < y + CELL_H - 1
      ) {
        hovered = `Color.${name}${level}`;
        strokeRectangle(x - 1, y - 1, CELL_W + 1, CELL_H + 1, Color.White, 1);
      }
    }
  }

  // Black and White are the two entries that belong to no family.
  const tailY = GRID_Y + FAMILIES.length * CELL_H + 6;
  drawText(GRID_X - 6, tailY + 1, "and", Color.Slate500, { align: "right" });
  fillRectangle(GRID_X, tailY, CELL_W - 1, CELL_H - 1, Color.Black);
  strokeRectangle(GRID_X, tailY, CELL_W - 1, CELL_H - 1, Color.Slate700, 1);
  fillRectangle(GRID_X + CELL_W, tailY, CELL_W - 1, CELL_H - 1, Color.White);
  drawText(GRID_X + CELL_W * 2 + 8, tailY + 1, "Black and White", Color.Slate500);

  drawText(
    getWidth() - 20,
    tailY + 1,
    hovered === "" ? "point at a swatch to name it" : hovered,
    hovered === "" ? Color.Slate600 : Color.Amber300,
    { align: "right" },
  );
});
