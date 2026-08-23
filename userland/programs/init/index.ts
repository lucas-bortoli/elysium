import {
  Color,
  addDrawHandler,
  clearScreen,
  fillRectangle,
  getWidth,
} from "ely:framebuffer";
import { addPostInitHandler, addUpdateTicker, delay } from "ely:lifecycle";

let x = 0;
const speed = 1000; // pixels per second

addUpdateTicker((dt) => {
  x += speed * dt;
  if (x > getWidth()) x = -100;
});

addDrawHandler(() => {
  fillRectangle(x, 130, 100, 100, Color.Amber400);
});

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  await delay(100);
});
