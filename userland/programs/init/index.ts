import { Color, addDrawHandler, beginPath, clearScreen } from "ely:framebuffer";
import { addPostInitHandler } from "ely:lifecycle";

addDrawHandler(() => {
  clearScreen(Color.Black);
});

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
});
