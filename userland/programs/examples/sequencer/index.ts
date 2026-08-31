// A step sequencer: sixteen steps of melody and drums, looping, editable while
// it runs.
//
// The piano example next door plays a note when you press a key, which is all
// a frame can do. This one plays music, and music needs instants a frame can't
// land on. A step here is 125ms at 120 BPM; frames arrive every 16 to 33. So
// nothing here is played *when the ticker runs* — every note names the moment
// it should sound, and the ticker's only job is to stay far enough ahead that
// the queue never runs dry. Frames stop needing to be punctual and only need
// to be frequent.
//
// The playhead is the other half of that idea, and the reason this example
// draws at all. Its position comes from the same clock the notes were placed
// against, not from counting frames — so it sits exactly on the note you are
// hearing, and it cannot drift away from the sound no matter what the frame
// rate does. Nothing is synchronising the two; they are the same number read
// twice.
//
// Escape belongs to the menu, which is still running behind this.

import {
  Color,
  addDrawHandler,
  beginPath,
  clearScreen,
  cubicTo,
  drawLine,
  drawPolyline,
  drawText,
  fillCircle,
  fillRectangle,
  fillRoundedRectangle,
  moveTo,
  strokePath,
  strokeRectangle,
} from "ely:framebuffer";
import type { Vector2d } from "ely:math";
import { Key, getPointerPosition, wasKeyPressed, wasPointerPressed } from "ely:input";
import { addUpdateTicker } from "ely:lifecycle";
import { hasValue } from "ely:container";
import type { Option } from "ely:container";
import {
  Waveform,
  bendVoice,
  currentTime,
  fadeVoice,
  noteToFrequency,
  playTone,
} from "ely:sound";
import type { Note, ToneOptions, VoiceId } from "ely:sound";

const STEPS = 16;
/** Four steps to the beat, so a step is a sixteenth note. */
const STEPS_PER_BEAT = 4;

/** How far ahead to queue. Comfortably longer than a frame, so a slow one
 * can't leave a gap, and comfortably inside the two seconds the system
 * allows. */
const LOOKAHEAD = 0.25;

/** The melody's pitches, low row last — a C minor pentatonic, so anything you
 * draw on the grid lands somewhere musical. */
const PITCHES: Note[] = ["C5", "A4", "G4", "E4", "D4", "C4", "A3", "G3"];

/** What a melody step does to the note before it. */
type Effect = "none" | "vibrato" | "tremolo";
const EFFECTS: Effect[] = ["none", "vibrato", "tremolo"];

/** One drum, and the tone that is it. These carry a duration, so each one
 * sees itself out and there is nothing to track. */
const DRUMS: { label: string; frequency: number; options: ToneOptions }[] = [
  {
    label: "kick",
    frequency: 150,
    options: {
      waveform: Waveform.Sine,
      amplitude: 0.7,
      sweepTo: 50,
      sweepOver: 0.08,
      attack: 0.001,
      decay: 0.25,
      sustainLevel: 0,
      duration: 0.3,
    },
  },
  {
    label: "snare",
    frequency: 2000,
    options: {
      waveform: Waveform.Noise,
      amplitude: 0.4,
      attack: 0.001,
      decay: 0.15,
      sustainLevel: 0,
      duration: 0.18,
    },
  },
  {
    label: "hat",
    frequency: 8000,
    options: {
      waveform: Waveform.Noise,
      amplitude: 0.25,
      attack: 0.001,
      decay: 0.04,
      sustainLevel: 0,
      duration: 0.05,
    },
  },
];

// ── the pattern ─────────────────────────────────────────────────────────────

/** Which pitch each step plays, or `undefined` for a rest. At most one per
 * step: the melody is a single line, which is what lets a step slide — a slide
 * has to have exactly one voice to bend. */
const melody: (number | undefined)[] = [
  5, undefined, 3, undefined, 2, undefined, 3, undefined,
  1, undefined, 2, 2, 0, undefined, 3, undefined,
];
/** A step that slides carries the note before it to a new pitch instead of
 * starting one of its own. */
const slides: boolean[] = Array.from({ length: STEPS }, () => false);
const effects: Effect[] = Array.from({ length: STEPS }, () => "none");
const drums: boolean[][] = [
  [true, false, false, false, true, false, false, false, true, false, false, true, true, false, false, false],
  [false, false, false, false, true, false, false, false, false, false, false, false, true, false, false, false],
  [true, false, true, true, false, false, true, false, true, false, true, true, false, false, true, false],
];

slides[11] = true;
effects[8] = "vibrato";
effects[12] = "tremolo";

// ── transport ───────────────────────────────────────────────────────────────

let playing = true;
let beatsPerMinute = 120;
let stepSeconds = 60 / beatsPerMinute / STEPS_PER_BEAT;

/** The clock time of absolute step zero. Every instant this program cares
 * about is derived from this one number by arithmetic, which is what keeps a
 * loop from drifting: nothing is ever added to a previous reading, so nothing
 * accumulates. */
let sequenceStart = currentTime() + 0.1;
/** The next absolute step to queue — an ever-growing count, not a position in
 * the pattern. `index % STEPS` is where in the pattern it lands.
 *
 * Absolute is the part to be careful with: anything that re-anchors
 * `sequenceStart` has to do it against this number and not against the
 * position within the pattern, or the two disagree by however many whole
 * loops have gone by and nothing lands where it should. */
let nextIndex = 0;

/** Where the playhead stopped, as an absolute fractional step, so resuming can
 * pick it up from exactly there. */
let pausedAt = 0;

const timeOf = (index: number): number => sequenceStart + index * stepSeconds;

/** The melody voice currently sounding, so a slide has something to bend.
 * Absent when nothing has played yet, or when the machine has no speakers. */
let melodyVoice: Option<VoiceId>;
let melodyLevel = 0.5;

/** Bends waiting for their moment.
 *
 * Note starts are placed exactly, because the ear is exact about when a note
 * begins. Bends are not schedulable, because the ear is vague about when a
 * *gesture* begins — so they are applied whenever the ticker next runs, up to
 * a frame late, inside a glide much longer than that. The note underneath is
 * still starting on the sample it named. */
const pending: { at: number; voice: VoiceId; to: number }[] = [];

/** How long the note starting at `index` should hold: until the next step that
 * starts a note of its own. A step that slides doesn't start one, so it
 * extends the note before it rather than replacing it.
 *
 * Working the length out here is what keeps the line monophonic without ever
 * calling `stopVoice`. The voice ends itself exactly as the next one begins,
 * which is both simpler and better placed than stopping it from a frame. */
function heldSteps(index: number): number {
  for (let ahead = 1; ahead <= STEPS; ahead++) {
    const step = (index + ahead) % STEPS;
    if (melody[step] !== undefined && !slides[step]) return ahead;
  }
  return STEPS;
}

/** Queues everything step `index` should do, at time `at`. */
function scheduleStep(index: number, at: number): void {
  const step = index % STEPS;

  for (const [row, drum] of DRUMS.entries()) {
    if (drums[row]?.[step]) playTone(drum.frequency, { ...drum.options, startAt: at });
  }

  const row = melody[step];
  if (row === undefined) return;
  const frequency = noteToFrequency(PITCHES[row]!);

  // A slide bends what is already sounding rather than starting anything, so
  // the note carries on through it — no retrigger, no click, no new envelope.
  if (slides[step] && hasValue(melodyVoice)) {
    pending.push({ at, voice: melodyVoice, to: frequency });
    return;
  }

  const effect = effects[step];
  const wobble = { depth: effect === "vibrato" ? 0.4 : 0.5, rate: 6 };
  melodyVoice = playTone(frequency, {
    waveform: Waveform.Triangle,
    amplitude: melodyLevel,
    attack: 0.01,
    decay: 0.08,
    sustainLevel: 0.7,
    release: 0.12,
    duration: heldSteps(index) * stepSeconds * 0.98,
    startAt: at,
    ...(effect === "vibrato" ? { vibrato: wobble } : {}),
    ...(effect === "tremolo" ? { tremolo: wobble } : {}),
  });
}

/** Where the playhead is, as a fractional step count from the start. */
function playhead(): number {
  return (currentTime() - sequenceStart) / stepSeconds;
}

/** Restarts the loop from now. */
function rewind(): void {
  sequenceStart = currentTime() + 0.05;
  nextIndex = 0;
  pending.length = 0;
}

/** Moves the tempo without moving the playhead, so the grid doesn't jump.
 *
 * Steps already queued keep the spacing they were queued with — up to a
 * lookahead of the old tempo is already committed, and the mixer is right to
 * honour what it was told rather than what this program has since changed its
 * mind about. */
function setTempo(next: number): void {
  const position = playhead();
  beatsPerMinute = Math.max(50, Math.min(220, next));
  stepSeconds = 60 / beatsPerMinute / STEPS_PER_BEAT;
  sequenceStart = currentTime() - position * stepSeconds;
}

// ── editing ─────────────────────────────────────────────────────────────────

/** Rows 0-7 are the melody's pitches; 8-10 are the drums. */
let cursorRow = 5;
let cursorStep = 0;

function toggleCell(row: number, step: number): void {
  if (row < PITCHES.length) {
    // One note to a step, so setting a pitch replaces whatever was there and
    // choosing the same one again clears it.
    melody[step] = melody[step] === row ? undefined : row;
  } else {
    const drum = drums[row - PITCHES.length];
    if (drum !== undefined) drum[step] = !drum[step];
  }
}

// ── layout ──────────────────────────────────────────────────────────────────

const GUTTER = 8;
const GRID_X = 58;
const CELL_W = 38;
const GRID_W = STEPS * CELL_W;
const GRID_RIGHT = GRID_X + GRID_W;

const RULER_Y = 46;
const MELODY_Y = 58;
const MELODY_H = 20;
const DRUMS_Y = MELODY_Y + PITCHES.length * MELODY_H + 10;
const DRUM_H = 18;
const GRID_BOTTOM = DRUMS_Y + DRUMS.length * DRUM_H;

const stepX = (step: number): number => GRID_X + step * CELL_W;
const rowY = (row: number): number =>
  row < PITCHES.length
    ? MELODY_Y + row * MELODY_H
    : DRUMS_Y + (row - PITCHES.length) * DRUM_H;
const rowH = (row: number): number => (row < PITCHES.length ? MELODY_H : DRUM_H);
const ROWS = PITCHES.length + DRUMS.length;

/** The row and step under the pointer, or nothing if it is off the grid. */
function cellUnderPointer(): { row: number; step: number } | undefined {
  const { x, y } = getPointerPosition();
  if (x < GRID_X || x >= GRID_RIGHT) return undefined;
  const step = Math.floor((x - GRID_X) / CELL_W);
  for (let row = 0; row < ROWS; row++) {
    if (y >= rowY(row) && y < rowY(row) + rowH(row)) return { row, step };
  }
  return undefined;
}

// ── the loop ────────────────────────────────────────────────────────────────

addUpdateTicker(() => {
  if (wasKeyPressed(Key.Space)) {
    playing = !playing;
    if (playing) {
      // Re-anchor so that the step the playhead stopped on is the one that
      // sounds now. Both numbers move together: the anchor is set from the
      // absolute position, and the next step to queue is the first one at or
      // after it.
      //
      // Anything already queued when the pause began still sounds — up to a
      // lookahead of it. There is no unsaying a scheduled note, and pretending
      // otherwise would mean not scheduling ahead in the first place.
      sequenceStart = currentTime() - pausedAt * stepSeconds;
      nextIndex = Math.ceil(pausedAt);
    } else {
      pausedAt = playhead();
    }
  }
  if (wasKeyPressed(Key.KeyR)) rewind();
  if (wasKeyPressed(Key.Minus)) setTempo(beatsPerMinute - 4);
  if (wasKeyPressed(Key.Equal)) setTempo(beatsPerMinute + 4);

  if (wasKeyPressed(Key.ArrowLeft)) cursorStep = (cursorStep - 1 + STEPS) % STEPS;
  if (wasKeyPressed(Key.ArrowRight)) cursorStep = (cursorStep + 1) % STEPS;
  if (wasKeyPressed(Key.ArrowUp)) cursorRow = (cursorRow - 1 + ROWS) % ROWS;
  if (wasKeyPressed(Key.ArrowDown)) cursorRow = (cursorRow + 1) % ROWS;
  if (wasKeyPressed(Key.Enter)) toggleCell(cursorRow, cursorStep);

  if (wasKeyPressed(Key.KeyS)) slides[cursorStep] = !slides[cursorStep];
  if (wasKeyPressed(Key.KeyV)) {
    const at = EFFECTS.indexOf(effects[cursorStep] ?? "none");
    effects[cursorStep] = EFFECTS[(at + 1) % EFFECTS.length]!;
  }
  if (wasKeyPressed(Key.Backspace)) {
    for (let step = 0; step < STEPS; step++) {
      melody[step] = undefined;
      slides[step] = false;
      effects[step] = "none";
      for (const drum of drums) drum[step] = false;
    }
  }

  // The level reaches a voice two different ways depending on whether it has
  // started yet: the one already sounding is faded to it, and the ones still
  // to come are simply played at it.
  const levelStep = (wasKeyPressed(Key.BracketRight) ? 0.1 : 0) - (wasKeyPressed(Key.BracketLeft) ? 0.1 : 0);
  if (levelStep !== 0) {
    melodyLevel = Math.max(0, Math.min(1, Math.round((melodyLevel + levelStep) * 10) / 10));
    if (hasValue(melodyVoice)) fadeVoice(melodyVoice, melodyLevel, { overSeconds: 0.05 });
  }

  const pointerCell = cellUnderPointer();
  if (wasPointerPressed() && pointerCell !== undefined) {
    cursorRow = pointerCell.row;
    cursorStep = pointerCell.step;
    toggleCell(pointerCell.row, pointerCell.step);
  }

  if (!playing) return;

  const now = currentTime();

  // Anything whose moment has already gone is skipped rather than queued. A
  // program that stalled — a slow frame, a window dragged — would otherwise
  // come back and dump every step it missed into the same instant, since a
  // time already past sounds at once.
  if (timeOf(nextIndex) < now) {
    nextIndex = Math.ceil((now - sequenceStart) / stepSeconds);
  }

  while (timeOf(nextIndex) < now + LOOKAHEAD) {
    scheduleStep(nextIndex, timeOf(nextIndex));
    nextIndex++;
  }

  // Bends land when the ticker reaches them, which is the difference between
  // an onset and a gesture.
  while (pending.length > 0 && pending[0]!.at <= now) {
    const bend = pending.shift()!;
    bendVoice(bend.voice, bend.to, { overSeconds: Math.min(0.12, stepSeconds) });
  }
});

// ── drawing ─────────────────────────────────────────────────────────────────

const ROW_COLORS: Color[] = [
  Color.Rose400,
  Color.Orange400,
  Color.Amber400,
  Color.Lime400,
  Color.Emerald400,
  Color.Cyan300,
  Color.Sky400,
  Color.Indigo400,
];

/** A short sine drawn inside a cell — the marker for a vibrato, and the shape
 * of what it does. */
function squiggle(x: number, y: number, width: number, color: Color): void {
  const points: Vector2d[] = [];
  for (let i = 0; i <= 10; i++) {
    points.push({ x: x + (width * i) / 10, y: y - Math.sin((i / 10) * Math.PI * 2) * 2.5 });
  }
  drawPolyline(points, color, 1);
}

addDrawHandler(() => {
  clearScreen(Color.Slate950);

  // Paused, the playhead holds where it was heard to stop — not where
  // scheduling had got to, which is a lookahead further on.
  const position = playing ? playhead() : pausedAt;
  const sounding = ((Math.floor(position) % STEPS) + STEPS) % STEPS;
  const through = position - Math.floor(position);

  drawText(GUTTER, 8, "Sequencer", Color.Amber300, { scale: 2 });
  drawText(
    GRID_RIGHT,
    10,
    `${playing ? "playing" : "paused"}   ${beatsPerMinute} BPM   level ${melodyLevel.toFixed(1)}`,
    playing ? Color.Teal300 : Color.Slate500,
    { align: "right" },
  );

  // Bar shading, so the metre reads without counting.
  for (let step = 0; step < STEPS; step += STEPS_PER_BEAT * 2) {
    const width = CELL_W * STEPS_PER_BEAT;
    fillRectangle(stepX(step), RULER_Y, width, GRID_BOTTOM - RULER_Y, Color.Slate900);
  }

  // The column the ear is in, and the exact place inside it. Both come from
  // the clock the notes were scheduled against, so neither can drift from
  // what you are hearing.
  if (playing) {
    fillRectangle(stepX(sounding), RULER_Y, CELL_W, GRID_BOTTOM - RULER_Y, Color.Slate800);
    const wrapped = (((position % STEPS) + STEPS) % STEPS);
    const head = GRID_X + wrapped * CELL_W;
    drawLine(head, RULER_Y, head, GRID_BOTTOM, Color.Amber300, 1);
    fillCircle(head, RULER_Y - 4, 3, Color.Amber300);
  }

  for (let step = 0; step < STEPS; step++) {
    drawText(
      stepX(step) + CELL_W / 2,
      RULER_Y - 12,
      step % STEPS_PER_BEAT === 0 ? `${step / STEPS_PER_BEAT + 1}` : "·",
      step % STEPS_PER_BEAT === 0 ? Color.Slate400 : Color.Slate700,
      { align: "center" },
    );
  }

  // Slides, drawn as the curve the pitch actually travels, from the note
  // before to the note it is carried to.
  for (let step = 0; step < STEPS; step++) {
    const row = melody[step];
    if (!slides[step] || row === undefined) continue;
    let from = (step - 1 + STEPS) % STEPS;
    while (melody[from] === undefined && from !== step) from = (from - 1 + STEPS) % STEPS;
    const fromRow = melody[from];
    if (fromRow === undefined || from > step) continue;

    const x1 = stepX(from) + CELL_W - 4;
    const y1 = rowY(fromRow) + MELODY_H / 2;
    const x2 = stepX(step) + 4;
    const y2 = rowY(row) + MELODY_H / 2;
    beginPath();
    moveTo(x1, y1);
    cubicTo(x1 + CELL_W * 0.5, y1, x2 - CELL_W * 0.5, y2, x2, y2);
    strokePath(Color.Fuchsia400, 2, "round", "round");
  }

  for (let row = 0; row < ROWS; row++) {
    const melodic = row < PITCHES.length;
    const y = rowY(row);
    const h = rowH(row);
    drawText(
      GRID_X - 6,
      y + h / 2 - 4,
      melodic ? PITCHES[row]! : DRUMS[row - PITCHES.length]!.label,
      melodic ? ROW_COLORS[row]! : Color.Slate400,
      { align: "right" },
    );

    for (let step = 0; step < STEPS; step++) {
      const x = stepX(step);
      const on = melodic ? melody[step] === row : (drums[row - PITCHES.length]?.[step] ?? false);
      const cursor = row === cursorRow && step === cursorStep;

      if (on) {
        // A note blooms as the playhead crosses it and dims across the step.
        // Nothing here remembers that it sounded: the playhead's fractional
        // position already says how far through the step we are, and it came
        // from the audio clock, so the flash is exactly on the beat.
        const lit = playing && step === sounding;
        const base = melodic ? ROW_COLORS[row]! : Color.Slate300;
        fillRoundedRectangle(x + 2, y + 2, CELL_W - 4, h - 4, 3, base);
        if (lit) {
          const bloom = Math.max(0, 1 - through * 2);
          fillRoundedRectangle(
            x + 2,
            y + 2,
            CELL_W - 4,
            h - 4,
            3,
            bloom > 0.4 ? Color.Slate100 : base,
          );
        }
      } else {
        strokeRectangle(x + 2, y + 2, CELL_W - 4, h - 4, Color.Slate800, 1);
      }

      if (cursor) strokeRectangle(x, y, CELL_W, h, Color.Teal300, 1);
    }
  }

  // Per-step markers under the melody: what each step does beyond its pitch.
  for (let step = 0; step < STEPS; step++) {
    const x = stepX(step);
    const y = MELODY_Y + PITCHES.length * MELODY_H + 2;
    if (slides[step]) drawText(x + 8, y, "S", Color.Fuchsia400);
    if (effects[step] === "vibrato") squiggle(x + 18, y + 4, 14, Color.Teal300);
    if (effects[step] === "tremolo") {
      fillCircle(x + 22, y + 4, 2 + Math.sin(currentTime() * 8) * 1.2, Color.Teal300);
    }
  }

  drawText(
    GUTTER,
    GRID_BOTTOM + 14,
    "click or Enter  note      S  slide      V  vibrato/tremolo      space  play",
    Color.Slate500,
  );
  drawText(
    GUTTER,
    GRID_BOTTOM + 28,
    "arrows  cursor      - =  tempo      [ ]  level      R  rewind      backspace  clear",
    Color.Slate500,
  );
  drawText(
    GUTTER,
    GRID_BOTTOM + 46,
    "the notes and the playhead read the same clock, so neither can drift from the other",
    Color.Slate700,
  );
});
