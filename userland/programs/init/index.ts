import { Color, addDrawHandler, clearScreen } from "ely:framebuffer";
import { addPostInitHandler } from "ely:lifecycle";
import { spawn } from "ely:process";

// The session's floor. The framebuffer is only ever cleared when a program
// asks, so clearing here means a program that never clears still opens on a
// clean frame instead of whatever the last one left behind. The last clear
// of a frame is the one that takes effect, so a program that does clear
// simply overrides this.
addDrawHandler(() => {
  clearScreen(Color.Black);
});

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  spawn("/programs/examples/index.ts", undefined);
});
