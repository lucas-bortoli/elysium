import { none } from "ely:container";
import { addPostInitHandler } from "ely:lifecycle";
import { spawn } from "ely:process";

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  spawn(`${import.meta.directoryName}/../examples/index.ts`, none());
});
