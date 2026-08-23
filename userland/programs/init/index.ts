import {
  AMBER_400,
  clearScreen,
  fillRectangle,
  SLATE_900,
} from "ely:framebuffer";

let x = 0;
const speed = 1; // pixels per second

export function update(dt: number) {
  x += speed * dt;
  if (x > 1280) x = -100;
}

export function draw() {
  clearScreen(SLATE_900);
  fillRectangle(x, 300, 100, 100, AMBER_400);
}
