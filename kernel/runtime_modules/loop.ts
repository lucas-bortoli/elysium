export type TickerId = number;

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
