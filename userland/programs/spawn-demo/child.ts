// The worker half of the spawn-demo: echo a bumped number back to the
// sender, and wind down cleanly when the kernel asks.

import {
  addMessageHandler,
  currentArguments,
  exit,
  postMessage,
} from "ely:process";

const args = currentArguments() as { label?: string } | undefined;
print(`[spawn-demo/child] started with label=${args?.label ?? "(none)"}`);

addMessageHandler((envelope) => {
  if (envelope.kind === "ping") {
    postMessage(envelope.from, {
      kind: "pong",
      data: (envelope.data as number) + 1,
    });
  } else if (envelope.kind === "ely:exit") {
    // The cooperative response to a shutdown request: stop now. A process
    // that ignores this is force-reaped once the grace period elapses.
    exit();
  }
});
