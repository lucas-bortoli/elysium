import type { TickerId } from "ely:lifecycle";

declare function __add_post_init_handler(handler: () => void): void;

/** Registers `handler` to run once, right after the program's top-level
 * code finishes evaluating, once timers, tickers, and draw handlers are
 * live. */
export function addPostInitHandler(handler: () => void): void {
  __add_post_init_handler(handler);
}

/** Resolves after `ms` milliseconds — `setTimeout` as an awaitable.
 * @warn Awaiting this from module top level deadlocks. See
 * Documentation/Multitasking.md. */
export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

let nextTickerId = 1;
const tickers = new Map<TickerId, (dt: number) => void>();
let lastTimestamp: number | null = null;
let currentDeltaTime = 0;
let frameScheduled = false;

function frame(timestamp: number) {
  frameScheduled = false;
  const dt = lastTimestamp === null ? 0 : timestamp - lastTimestamp;
  lastTimestamp = timestamp;
  currentDeltaTime = dt;
  for (const handler of [...tickers.values()]) handler(dt);
  if (tickers.size > 0) scheduleFrame();
}

function scheduleFrame() {
  if (!frameScheduled) {
    frameScheduled = true;
    requestAnimationFrame(frame);
  }
}

export function addUpdateTicker(handler: (dt: number) => void): TickerId {
  const id = nextTickerId++;
  tickers.set(id, handler);
  scheduleFrame();
  return id;
}

export function removeUpdateTicker(id: TickerId): void {
  tickers.delete(id);
}

export function getDeltaTime(): number {
  return currentDeltaTime;
}
