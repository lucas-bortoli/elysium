// A rendering benchmark. Runs a fixed sequence of drawing workloads, each
// for a fixed span of wall-clock time, and lets the kernel's own `[fps]`
// line (tick ms / render ms per frame, printed once a second to stderr)
// report the cost. It draws the same thing on every branch, so the numbers
// line up phase for phase.
//
// A phase boundary prints `[bench] phase: <name>` to stdout; merge the two
// streams (`2>&1`) when capturing so the markers sit among the `[fps]`
// samples they bracket.

import { Color, screen } from "ely:graphics";
import { addUpdateTicker } from "ely:lifecycle";
import { exit } from "ely:process";
import { loadImage } from "ely:image";

const WIDTH = 720;
const HEIGHT = 360;
const PHASE_SECONDS = 6;

const photo = loadImage(
  `${import.meta.directoryName}/../examples/image/photo.png`,
);

// A tiny deterministic PRNG so every branch lays the same scene out.
let seed = 0x2545f491;
function rnd(): number {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  seed >>>= 0;
  return seed / 0xffffffff;
}
function reseed(): void {
  seed = 0x2545f491;
}
function pick<T>(items: readonly T[]): T {
  return items[Math.floor(rnd() * items.length)] as T;
}

const PALETTE = [
  Color.Slate700,
  Color.Amber400,
  Color.Rose500,
  Color.Sky400,
  Color.Emerald400,
  Color.Teal300,
] as const;

let frame = 0;

function idle(): void {
  screen.clear(Color.Slate900);
}

function rects(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 3000; i++) {
    screen.fillRectangle(rnd() * WIDTH, rnd() * HEIGHT, 4 + rnd() * 40, 4 + rnd() * 40, pick(PALETTE));
  }
}

function circles(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 1000; i++) {
    screen.fillCircle(rnd() * WIDTH, rnd() * HEIGHT, 3 + rnd() * 20, pick(PALETTE));
  }
  for (let i = 0; i < 1000; i++) {
    screen.strokeCircle(rnd() * WIDTH, rnd() * HEIGHT, 3 + rnd() * 20, pick(PALETTE), 2);
  }
}

function rounded(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 800; i++) {
    screen.fillRoundedRectangle(rnd() * WIDTH, rnd() * HEIGHT, 20 + rnd() * 60, 16 + rnd() * 40, 6, pick(PALETTE));
  }
  for (let i = 0; i < 800; i++) {
    screen.strokeRoundedRectangle(rnd() * WIDTH, rnd() * HEIGHT, 20 + rnd() * 60, 16 + rnd() * 40, 6, pick(PALETTE), 2);
  }
}

function lines(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 3000; i++) {
    screen.drawLine(rnd() * WIDTH, rnd() * HEIGHT, rnd() * WIDTH, rnd() * HEIGHT, pick(PALETTE), 1 + rnd() * 3);
  }
}

function arcs(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 1200; i++) {
    const a = rnd() * Math.PI * 2;
    screen.drawArc(rnd() * WIDTH, rnd() * HEIGHT, 6 + rnd() * 24, a, a + 1 + rnd() * 4, pick(PALETTE), 2);
  }
}

const WORDS = "the quick brown fox jumps over a lazy dog while nine gray owls watch".split(" ");
function text(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 320; i++) {
    screen.drawText(rnd() * WIDTH, rnd() * HEIGHT, pick(WORDS), pick(PALETTE));
  }
  screen.drawText(20, 20, WORDS.join(" ") + "\n" + WORDS.join(" "), Color.Slate500, {
    maxWidth: 300,
  });
}

function spritesScaled(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 400; i++) {
    screen.drawImage(photo, rnd() * WIDTH - 32, rnd() * HEIGHT - 32, {
      sx: rnd() * (540 - 64),
      sy: rnd() * (360 - 64),
      sw: 64,
      sh: 64,
      scale: 2,
    });
  }
}

function spritesRotated(): void {
  screen.clear(Color.Slate900);
  reseed();
  const angle = frame * 0.05;
  for (let i = 0; i < 400; i++) {
    screen.drawImageRotated(photo, rnd() * WIDTH, rnd() * HEIGHT, angle + rnd() * 6, {
      sx: rnd() * (540 - 48),
      sy: rnd() * (360 - 48),
      sw: 48,
      sh: 48,
      originX: 24,
      originY: 24,
    });
  }
}

function transforms(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 900; i++) {
    screen.pushTransform({
      translate: { x: rnd() * WIDTH, y: rnd() * HEIGHT },
      rotate: frame * 0.02 + rnd() * 6,
      scale: 0.5 + rnd() * 2,
    });
    screen.fillRectangle(-10, -10, 20, 20, pick(PALETTE));
    screen.popTransform();
  }
}

function clipsRect(): void {
  screen.clear(Color.Slate900);
  reseed();
  // A panel with its content sitting inside it — the shape a viewport,
  // list or HUD clip actually takes.
  for (let i = 0; i < 300; i++) {
    const x = rnd() * (WIDTH - 140);
    const y = rnd() * (HEIGHT - 140);
    const w = 100 + rnd() * 40;
    const h = 100 + rnd() * 40;
    screen.pushClip(x, y, w, h);
    for (let j = 0; j < 6; j++) {
      screen.fillCircle(x + 20 + rnd() * (w - 40), y + 20 + rnd() * (h - 40), 6 + rnd() * 14, pick(PALETTE));
    }
    screen.popClip();
  }
}

function clipsOverflow(): void {
  screen.clear(Color.Slate900);
  reseed();
  // The other case: shapes that spill past the clip edge, so the region
  // has to become a real coverage mask.
  for (let i = 0; i < 300; i++) {
    const x = rnd() * WIDTH;
    const y = rnd() * HEIGHT;
    screen.pushClip(x, y, 40 + rnd() * 80, 40 + rnd() * 80);
    for (let j = 0; j < 6; j++) {
      screen.fillCircle(x + rnd() * 100, y + rnd() * 100, 20 + rnd() * 30, pick(PALETTE));
    }
    screen.popClip();
  }
}

function clipsRotated(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 200; i++) {
    screen.pushTransform({
      translate: { x: rnd() * WIDTH, y: rnd() * HEIGHT },
      rotate: frame * 0.03 + rnd() * 6,
    });
    screen.pushClip(-40, -40, 80, 80);
    for (let j = 0; j < 8; j++) {
      screen.fillRectangle(-60 + rnd() * 120, -60 + rnd() * 120, 30, 30, pick(PALETTE));
    }
    screen.popClip();
    screen.popTransform();
  }
}

function star(cx: number, cy: number, r: number): void {
  screen.beginPath();
  for (let k = 0; k < 5; k++) {
    const a = (k * 4 * Math.PI) / 5 - Math.PI / 2;
    const x = cx + Math.cos(a) * r;
    const y = cy + Math.sin(a) * r;
    if (k === 0) screen.moveTo(x, y);
    else screen.lineTo(x, y);
  }
  screen.closePath();
}

function paths(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 220; i++) {
    star(rnd() * WIDTH, rnd() * HEIGHT, 12 + rnd() * 26);
    screen.fillPath(pick(PALETTE), "evenodd");
    screen.strokePath(Color.Slate950, 1.5);
  }
}

function polygons(): void {
  screen.clear(Color.Slate900);
  reseed();
  for (let i = 0; i < 400; i++) {
    const cx = rnd() * WIDTH;
    const cy = rnd() * HEIGHT;
    const r = 8 + rnd() * 24;
    const n = 3 + Math.floor(rnd() * 5);
    const pts = [];
    for (let k = 0; k < n; k++) {
      const a = (k / n) * Math.PI * 2;
      pts.push({ x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r });
    }
    screen.fillPolygon(pts, pick(PALETTE));
  }
}

function mixed(): void {
  screen.clear(Color.Slate900);
  reseed();
  // A tiled sprite background.
  for (let i = 0; i < 70; i++) {
    screen.drawImage(photo, rnd() * WIDTH - 24, rnd() * HEIGHT - 24, {
      sx: rnd() * (540 - 48),
      sy: rnd() * (360 - 48),
      sw: 48,
      sh: 48,
      scale: 2,
    });
  }
  // A few rotated actors.
  for (let i = 0; i < 24; i++) {
    screen.drawImageRotated(photo, rnd() * WIDTH, rnd() * HEIGHT, frame * 0.05 + rnd() * 6, {
      sx: 0,
      sy: 0,
      sw: 40,
      sh: 40,
      originX: 20,
      originY: 20,
    });
  }
  // HUD shapes under a panel clip.
  screen.pushClip(12, 12, 240, 90);
  screen.fillRoundedRectangle(12, 12, 240, 90, 8, Color.Slate800);
  for (let i = 0; i < 20; i++) {
    screen.fillCircle(24 + i * 11, 60 + Math.sin(frame * 0.1 + i) * 20, 5, pick(PALETTE));
  }
  screen.popClip();
  // Scattered labels and lines.
  for (let i = 0; i < 30; i++) {
    screen.drawText(rnd() * WIDTH, rnd() * HEIGHT, pick(WORDS), pick(PALETTE));
  }
  for (let i = 0; i < 40; i++) {
    screen.drawLine(rnd() * WIDTH, rnd() * HEIGHT, rnd() * WIDTH, rnd() * HEIGHT, pick(PALETTE), 1);
  }
  for (let i = 0; i < 12; i++) {
    star(rnd() * WIDTH, rnd() * HEIGHT, 10 + rnd() * 18);
    screen.fillPath(pick(PALETTE));
  }
}

const PHASES: readonly (readonly [string, () => void])[] = [
  ["idle", idle],
  ["rects", rects],
  ["circles", circles],
  ["rounded-rects", rounded],
  ["lines", lines],
  ["arcs", arcs],
  ["text", text],
  ["sprites-scaled", spritesScaled],
  ["sprites-rotated", spritesRotated],
  ["transforms", transforms],
  ["clips-rect", clipsRect],
  ["clips-overflow", clipsOverflow],
  ["clips-rotated", clipsRotated],
  ["paths", paths],
  ["polygons", polygons],
  ["mixed", mixed],
];

let elapsed = 0;
let phase = -1;

addUpdateTicker((dt) => {
  elapsed += dt;
  const next = Math.floor(elapsed / PHASE_SECONDS);
  if (next !== phase) {
    phase = next;
    const entry = PHASES[phase];
    if (!entry) {
      print("[bench] done");
      exit();
      return;
    }
    print(`[bench] phase: ${entry[0]}`);
  }
});

addUpdateTicker(() => {
  frame++;
  const entry = PHASES[phase];
  if (!entry) {
    screen.clear(Color.Slate900);
    return;
  }
  entry[1]();
});
