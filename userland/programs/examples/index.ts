// The examples browser: a menu of example programs, each of which runs as a
// real process in its own VM.
//
// Picking one spawns it and hands the screen over — the menu drops its draw
// handler, so the example's drawing is all that's left. It keeps one update
// ticker running, and that's what keeps it alive and listening while
// invisible: drawing and ticking are two separate per-frame loops, so having
// no draw handler doesn't stop a program ticking.
//
// Escape belongs to the menu. There's no input focus in Elysium — one
// keyboard, and every running program sees the same key press in the same
// frame — so an example that read Escape would be fighting the menu for it.
// The menu only acts on Escape while an example is running, so a press is
// never ambiguous.

import {
  Color,
  addDrawHandler,
  clearScreen,
  drawLine,
  drawText,
  fillRoundedRectangle,
  getHeight,
  getWidth,
  measureText,
  popClip,
  pushClip,
  removeDrawHandler,
} from "ely:framebuffer";
import type { DrawTickerId } from "ely:framebuffer";
import {
  Key,
  getPointerDelta,
  getPointerPosition,
  getScrollDelta,
  wasKeyPressed,
  wasPointerPressed,
} from "ely:input";
import { addUpdateTicker } from "ely:lifecycle";
import { isLive, spawn, terminate } from "ely:process";
import type { ProcessHandle } from "ely:process";
import { join, listDirectory, readTextFile } from "ely:filesystem";

const EXAMPLES_ROOT = "/programs/examples";

/** One example, as the menu holds it. */
interface Example {
  /** The entry module to spawn. */
  path: string;
  title: string;
  description: string;
  /** Sorts the list. `listDirectory` has no order worth relying on. */
  order: number;
}

/** What an `example.json` is allowed to say. */
interface Manifest {
  title?: unknown;
  description?: unknown;
  order?: unknown;
}

/** Reads every example directory's manifest. A directory whose manifest is
 * missing, unparseable or untitled is skipped with a note rather than
 * bringing the menu down — one broken example shouldn't cost you the other
 * six. */
function loadExamples(): Example[] {
  const found: Example[] = [];
  let listing;
  try {
    listing = listDirectory(EXAMPLES_ROOT);
  } catch (err) {
    print(`[examples] cannot read ${EXAMPLES_ROOT}: ${err}`);
    return found;
  }

  for (const entry of listing) {
    if (entry.kind !== "Directory") continue;
    const manifestPath = join(entry.path, "example.json");
    try {
      const manifest = JSON.parse(readTextFile(manifestPath)) as Manifest;
      if (typeof manifest.title !== "string") {
        throw new Error("its manifest has no title");
      }
      found.push({
        path: join(entry.path, "index.ts"),
        title: manifest.title,
        description:
          typeof manifest.description === "string" ? manifest.description : "",
        order: typeof manifest.order === "number" ? manifest.order : Infinity,
      });
    } catch (err) {
      print(`[examples] skipping ${entry.path}: ${err}`);
    }
  }

  found.sort((a, b) =>
    a.order !== b.order ? a.order - b.order : a.title < b.title ? -1 : 1,
  );
  return found;
}

const examples = loadExamples();
print(`[examples] found ${examples.length} example(s)`);

const lineHeight = measureText("X").height;
const ROW_HEIGHT = lineHeight * 2 + 14;
const LIST_X = 48;
const LIST_Y = 74;
const LIST_WIDTH = getWidth() - LIST_X * 2;
const LIST_HEIGHT = getHeight() - LIST_Y - 40;

let selected = 0;
/** How far the list is scrolled, in pixels from the top of the first row. */
let scroll = 0;
/** The example currently running, or absent while the menu is showing. */
let child: ProcessHandle | undefined;
/** Absent exactly while an example is running — that's what hands the
 * screen over. */
let drawHandler: DrawTickerId | undefined = addDrawHandler(draw);

function maxScroll(): number {
  return Math.max(0, examples.length * ROW_HEIGHT - LIST_HEIGHT);
}

/** Scrolls the list far enough that the selected row is wholly visible. */
function revealSelected(): void {
  const top = selected * ROW_HEIGHT;
  if (top < scroll) scroll = top;
  else if (top + ROW_HEIGHT > scroll + LIST_HEIGHT) {
    scroll = top + ROW_HEIGHT - LIST_HEIGHT;
  }
  scroll = Math.min(maxScroll(), Math.max(0, scroll));
}

/** Hands the screen to `example`. Dropping the draw handler is the whole
 * mechanism: the menu keeps ticking, but stops putting anything on screen. */
function launch(example: Example): void {
  try {
    child = spawn(example.path, undefined);
  } catch (err) {
    print(`[examples] could not start ${example.path}: ${err}`);
    return;
  }
  print(`[examples] running ${example.title} as process ${child}`);
  if (drawHandler !== undefined) {
    removeDrawHandler(drawHandler);
    drawHandler = undefined;
  }
}

/** Takes the screen back. Terminating is immediate — asking an example to
 * exit would leave it drawing over the menu for the whole grace period. An
 * example that already ended on its own needs no terminating. */
function closeChild(): void {
  if (child !== undefined && isLive(child)) terminate(child);
  child = undefined;
  if (drawHandler === undefined) drawHandler = addDrawHandler(draw);
}

/** The row under the pointer, or `-1` if it isn't over one. */
function rowUnderPointer(): number {
  const pointer = getPointerPosition();
  if (
    pointer.x < LIST_X ||
    pointer.x >= LIST_X + LIST_WIDTH ||
    pointer.y < LIST_Y ||
    pointer.y >= LIST_Y + LIST_HEIGHT
  ) {
    return -1;
  }
  const row = Math.floor((pointer.y - LIST_Y + scroll) / ROW_HEIGHT);
  return row >= 0 && row < examples.length ? row : -1;
}

addUpdateTicker(() => {
  // While an example is running the menu does one thing only: watch for the
  // way back. Its own navigation keys belong to the example now.
  if (child !== undefined) {
    if (wasKeyPressed(Key.Escape) || !isLive(child)) closeChild();
    return;
  }

  if (examples.length === 0) return;

  if (wasKeyPressed(Key.ArrowUp) || wasKeyPressed(Key.KeyW)) {
    selected = (selected - 1 + examples.length) % examples.length;
    revealSelected();
  }
  if (wasKeyPressed(Key.ArrowDown) || wasKeyPressed(Key.KeyS)) {
    selected = (selected + 1) % examples.length;
    revealSelected();
  }

  const wheel = getScrollDelta();
  if (wheel !== 0) {
    scroll = Math.min(maxScroll(), Math.max(0, scroll - wheel * ROW_HEIGHT));
  }

  // The pointer only takes over the selection when it actually moves, so
  // resting it over a row doesn't fight the arrow keys.
  const row = rowUnderPointer();
  const delta = getPointerDelta();
  if (row !== -1 && (delta.x !== 0 || delta.y !== 0)) selected = row;

  if (wasKeyPressed(Key.Enter)) launch(examples[selected]);
  else if (row !== -1 && wasPointerPressed()) {
    selected = row;
    launch(examples[row]);
  }
});

function draw(): void {
  clearScreen(Color.Slate900);

  const middle = getWidth() / 2;
  drawText(middle, 12, "Welcome to Examples!", Color.Amber300, {
    align: "center",
    scale: 2,
  });
  drawText(middle, 12 + lineHeight * 2 + 6, "Please select an example program.", Color.Slate400, {
    align: "center",
  });

  if (examples.length === 0) {
    drawText(middle, LIST_Y + 40, `No examples found in ${EXAMPLES_ROOT}.`, Color.Rose400, {
      align: "center",
    });
    return;
  }

  // Every row is drawn against the same clip, so a row scrolled half out of
  // the viewport is cut off cleanly instead of spilling over the heading.
  pushClip(LIST_X, LIST_Y, LIST_WIDTH, LIST_HEIGHT);
  for (let i = 0; i < examples.length; i++) {
    const top = LIST_Y + i * ROW_HEIGHT - scroll;
    if (top + ROW_HEIGHT < LIST_Y || top > LIST_Y + LIST_HEIGHT) continue;

    const chosen = i === selected;
    if (chosen) {
      fillRoundedRectangle(LIST_X, top, LIST_WIDTH, ROW_HEIGHT - 6, 4, Color.Slate700);
    }
    drawText(
      LIST_X + 10,
      top + 5,
      examples[i].title,
      chosen ? Color.Amber300 : Color.Slate200,
    );
    drawText(
      LIST_X + 10,
      top + 7 + lineHeight,
      examples[i].description,
      chosen ? Color.Slate300 : Color.Slate500,
      { maxWidth: LIST_WIDTH - 20 },
    );
  }
  popClip();

  const footer = getHeight() - 26;
  drawLine(LIST_X, footer, LIST_X + LIST_WIDTH, footer, Color.Slate700);
  drawText(
    middle,
    footer + 7,
    "Up/Down or mouse to choose - Enter or click to run - Esc to come back",
    Color.Slate500,
    { align: "center" },
  );
}
