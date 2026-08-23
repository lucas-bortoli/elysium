import {
  Color,
  addDrawHandler,
  clearScreen,
  fillRectangle,
} from "ely:framebuffer";
import { addPostInitHandler, addUpdateTicker, delay } from "ely:lifecycle";

let x = 0;
const speed = 1000; // pixels per second

addUpdateTicker((dt) => {
  x += speed * dt;
  if (x > 1280) x = -100;
});

addDrawHandler(() => {
  fillRectangle(x, 300, 100, 100, Color.Amber400);
});

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  await delay(100);
});
