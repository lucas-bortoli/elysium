import {
  Color,
  addDrawHandler,
  fillRectangle,
  getWidth,
} from "ely:framebuffer";
import {
  Key,
  isPointerDown,
  wasKeyPressed,
  wasPointerPressed,
  wasPointerReleased,
} from "ely:input";
import { addPostInitHandler, addUpdateTicker, delay } from "ely:lifecycle";

let x = 0;
const speed = 1000; // pixels per second

addUpdateTicker((dt) => {
  x += speed * dt;
  if (x > getWidth()) x = -100;
});

addDrawHandler(() => {
  let color: Color = Color.White;

  if (wasPointerPressed()) {
    color = Color.Green400;
  } else if (wasPointerReleased()) {
    color = Color.Blue400;
  } else if (isPointerDown()) {
    color = Color.Red400;
  }

  if (wasKeyPressed(Key.KeyA)) {
    print("A");
  }

  fillRectangle(x, 100, 100, 100, color);
});

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  await delay(100);
});
