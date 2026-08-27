import {
  Color,
  addDrawHandler,
  clearScreen,
  drawImage,
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
import { loadImage } from "ely:image";
import * as process from "ely:process";
import * as fs from "ely:filesystem";
import { none } from "ely:container";

let x = 0;
const speed = 1000; // pixels per second

const bg = loadImage(
  `${import.meta.directoryName}/pexels-elizabeth-ferreira-1040803688-33035533.png`,
);

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

  clearScreen(Color.Black);
  drawImage(bg, getWidth() / 2 - bg.width / 2, 0);
  fillRectangle(x, 100 + 2, 100 + 2, 100, Color.Neutral600);
  fillRectangle(x, 100, 100, 100, color);
});

addPostInitHandler(async () => {
  function recurse(path: string, depth: number = 0) {
    for (const entry of fs.listDirectory(path)) {
      const icon = entry.kind === "Directory" ? "📁" : "📄";
      print("   ".repeat(depth) + icon + " " + fs.extractBaseName(entry.path));
      if (entry.kind === "Directory") {
        recurse(entry.path, depth + 1);
      }
    }
  }

  print("Welcome to Elysium!");
  print("--- userland directory information ---");
  recurse("/", 1);
  print("--- userland directory information ---");

  // Show off multitasking: a sibling process that runs, talks, and exits
  // on its own while init keeps drawing.
  const child = process.spawn("/programs/spawn-demo/index.ts", undefined);
  process.postMessage(child, { kind: "hello", data: none() });
  await delay(100);
});
