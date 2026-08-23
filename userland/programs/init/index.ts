import { Color, addDrawHandler, clearScreen, fillRectangle } from "ely:framebuffer";
import { addUpdateTicker } from "ely:loop";

let x = 0;
const speed = 1; // pixels per second

addUpdateTicker((dt) => {
  x += speed * dt;
  if (x > 1280) x = -100;
});

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  fillRectangle(x, 300, 100, 100, Color.Amber400);
});
