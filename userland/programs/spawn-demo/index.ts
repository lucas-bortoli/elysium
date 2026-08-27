// A tiny demonstration of ely:process: spawn a worker, exchange a
// request/response, then ask it to shut down and shut down ourselves.

import {
  addMessageHandler,
  exit,
  postMessage,
  requestExit,
  spawn,
} from "ely:process";

const worker = spawn(`${import.meta.directoryName}/child.ts`, { label: "demo" });

addMessageHandler((envelope) => {
  if (envelope.kind === "pong") {
    print(`[spawn-demo] worker replied: ${envelope.data}`);
    requestExit(worker);
    exit();
  }
});

postMessage(worker, { kind: "ping", data: 41 });
