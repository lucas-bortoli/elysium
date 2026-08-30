// The sound device: a keyboard you can play, four shapes to play it with,
// and an octave you can move it to.
//
// The distinction worth watching here is who ends a note. A tone started
// without a duration sustains until someone stops it, and the program that
// started it owns it — so holding a key here starts a voice and letting go
// stops one. What you hear on the way out is the release, which is why a
// note fades instead of clicking off.
//
// This example never reads Escape — that key belongs to the menu, which is
// still running while this draws. There is no input focus in Elysium: this
// program and the menu are seeing exactly the same key presses.

import {
  Color,
  addDrawHandler,
  clearScreen,
  drawLine,
  drawPolyline,
  drawText,
  fillRoundedRectangle,
  getHeight,
  getWidth,
  strokeRectangle,
} from "ely:framebuffer";
import type { Vector2d } from "ely:math";
import { Key, isKeyDown, wasKeyPressed } from "ely:input";
import { addUpdateTicker } from "ely:lifecycle";
import { hasValue } from "ely:container";
import { Waveform, isNote, noteToFrequency, playTone, stopVoice } from "ely:sound";
import type { Note, VoiceId } from "ely:sound";

/** One waveform, and the digit that selects it. */
const SHAPES: { waveform: Waveform; key: Key; label: string }[] = [
  { waveform: Waveform.Square, key: Key.Digit1, label: "1  square" },
  { waveform: Waveform.Triangle, key: Key.Digit2, label: "2  triangle" },
  { waveform: Waveform.Sine, key: Key.Digit3, label: "3  sine" },
  { waveform: Waveform.Noise, key: Key.Digit4, label: "4  noise" },
];

/** One playable key: which pitch it sounds, and which computer key plays it.
 * The octave isn't here — it moves, so a pad's note name is built when it's
 * needed rather than written out. */
interface Pad {
  key: Key;
  /** The note's letter and accidental, without an octave. */
  pitch: string;
  /** How many octaves above the base this sits. Only the C closing the row
   * is above it. */
  above: number;
  label: string;
}

const WHITE: Pad[] = [
  { key: Key.KeyA, pitch: "C", above: 0, label: "A" },
  { key: Key.KeyS, pitch: "D", above: 0, label: "S" },
  { key: Key.KeyD, pitch: "E", above: 0, label: "D" },
  { key: Key.KeyF, pitch: "F", above: 0, label: "F" },
  { key: Key.KeyG, pitch: "G", above: 0, label: "G" },
  { key: Key.KeyH, pitch: "A", above: 0, label: "H" },
  { key: Key.KeyJ, pitch: "B", above: 0, label: "J" },
  { key: Key.KeyK, pitch: "C", above: 1, label: "K" },
];

/** The sharps sit on the seams between white keys, and there is no seam
 * between E and F or between B and C — those two gaps are what make this
 * read as a piano rather than a second row of buttons. */
const BLACK: (Pad & { seam: number })[] = [
  { key: Key.KeyW, pitch: "C#", above: 0, label: "W", seam: 0 },
  { key: Key.KeyE, pitch: "D#", above: 0, label: "E", seam: 1 },
  { key: Key.KeyT, pitch: "F#", above: 0, label: "T", seam: 3 },
  { key: Key.KeyY, pitch: "G#", above: 0, label: "Y", seam: 4 },
  { key: Key.KeyU, pitch: "A#", above: 0, label: "U", seam: 5 },
];

const PADS: Pad[] = [...WHITE, ...BLACK];

/** The row covers the base octave and the C above it, so the base stops one
 * short of the highest octave a note name can name. */
const LOWEST_OCTAVE = 0;
const HIGHEST_OCTAVE = 7;

const PICKER_X = 96;
const PICKER_Y = 52;
const PICKER_W = 120;
const PICKER_H = 34;
const PICKER_PITCH = 136;

const SCOPE_X = 96;
const SCOPE_Y = 100;
const SCOPE_W = 528;
const SCOPE_H = 96;
const SCOPE_AMP = 38;
const SCOPE_CYCLES = 12;

const KEYS_X = 108;
const WHITE_Y = 228;
const WHITE_W = 56;
const WHITE_H = 88;
const WHITE_PITCH = 64;
const BLACK_W = 34;
const BLACK_H = 52;

/** The noise channel's first `SCOPE_CYCLES` values, taken from the same
 * 15-bit shift register the mixer uses, stepped once per cycle rather than
 * once per sample — which is why noise holds one value for a whole cycle and
 * has no pitch of its own. A voice that is actually sounding keeps advancing
 * its own register, so this is the sequence from the seed rather than a live
 * capture: the shape is real, the moment isn't. */
const NOISE: number[] = (() => {
  let lfsr = 0x7fff;
  const values: number[] = [];
  for (let cycle = 0; cycle < SCOPE_CYCLES; cycle++) {
    values.push((lfsr & 1) === 0 ? 1 : -1);
    const bit = (lfsr ^ (lfsr >> 1)) & 1;
    lfsr = (lfsr >> 1) | (bit << 14);
  }
  return values;
})();

/** The note `pad` sounds with the keyboard based at `octave`.
 *
 * The name is assembled here rather than written out, which makes it an
 * ordinary string as far as the checker is concerned — `isNote` is what turns
 * it back into a `Note` that `noteToFrequency` will accept. Clamping the base
 * means the guard never actually rejects anything, but it is the only honest
 * way to hand a built name to an API that takes a closed type. */
function noteFor(pad: Pad, octave: number): Note | undefined {
  const name = `${pad.pitch}${octave + pad.above}`;
  return isNote(name) ? name : undefined;
}

/** One cycle of `waveform` at `phase`, using the same arithmetic the mixer
 * does, so the trace is the shape being played rather than an impression of
 * it. */
function sample(waveform: Waveform, phase: number, cycle: number): number {
  if (waveform === Waveform.Square) return phase < 0.5 ? 1 : -1;
  if (waveform === Waveform.Triangle) return 4 * Math.abs(phase - 0.5) - 1;
  if (waveform === Waveform.Sine) return Math.sin(phase * 2 * Math.PI);
  return NOISE[cycle % SCOPE_CYCLES] ?? 1;
}

function trace(waveform: Waveform): Vector2d[] {
  const points: Vector2d[] = [];
  const middle = SCOPE_Y + SCOPE_H / 2;
  for (let column = 0; column <= SCOPE_W; column++) {
    const through = (column / SCOPE_W) * SCOPE_CYCLES;
    const value = sample(waveform, through % 1, Math.floor(through));
    points.push({ x: SCOPE_X + column, y: middle - value * SCOPE_AMP });
  }
  return points;
}

let selected: Waveform = Waveform.Triangle;
/** Which octave the row of white keys starts on. */
let octave = 4;
/** The selected shape's trace, rebuilt only when the shape changes — a frame
 * that redrew it would spend more time building points than on everything
 * else this program does put together. */
let traced: Vector2d[] = trace(Waveform.Triangle);
/** The sustaining voice each held key started, so releasing the key can stop
 * the right one. */
const voices = new Map<Key, VoiceId>();

/** Stops whatever voice `key` is holding, if it is holding one. */
function release(key: Key): void {
  const voice = voices.get(key);
  if (hasValue(voice)) stopVoice(voice);
  voices.delete(key);
}

addUpdateTicker(() => {
  for (const shape of SHAPES) {
    if (wasKeyPressed(shape.key)) {
      selected = shape.waveform;
      traced = trace(selected);
    }
  }

  // Held voices keep the pitch they started on, the same way they keep their
  // shape — a voice's frequency is fixed when it starts.
  if (wasKeyPressed(Key.ArrowLeft)) octave = Math.max(LOWEST_OCTAVE, octave - 1);
  if (wasKeyPressed(Key.ArrowRight)) octave = Math.min(HIGHEST_OCTAVE, octave + 1);

  for (const pad of PADS) {
    if (wasKeyPressed(pad.key)) {
      // Releasing first is what keeps a fast retrigger from stranding a
      // voice: an edge says a press happened this frame, not that only one
      // did, so this key may already be holding one.
      release(pad.key);

      // No duration, so the voice holds until it's released. A held voice
      // keeps the shape and pitch it started with; changing either only
      // affects the next key pressed.
      const note = noteFor(pad, octave);
      if (note !== undefined) {
        const voice = playTone(noteToFrequency(note), { waveform: selected });
        // Absent means nothing sounded — no output device, or every voice
        // already in use. Neither is an error: the key simply has no voice to
        // release later.
        if (hasValue(voice)) voices.set(pad.key, voice);
      }
    }

    // Whether the key is down decides whether it should still be sounding,
    // rather than whether a release was seen. A key pressed and let go
    // inside one frame reports both edges at once, and one released without
    // the window watching reports neither.
    if (!isKeyDown(pad.key)) release(pad.key);
  }
});

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  drawText(getWidth() / 2, 8, "Sound", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  drawText(PICKER_X, 34, "waveform — press 1 to 4", Color.Slate400);
  for (const [index, shape] of SHAPES.entries()) {
    const x = PICKER_X + index * PICKER_PITCH;
    const active = shape.waveform === selected;
    fillRoundedRectangle(
      x,
      PICKER_Y,
      PICKER_W,
      PICKER_H,
      6,
      active ? Color.Teal500 : Color.Slate800,
    );
    drawText(x + PICKER_W / 2, PICKER_Y + 13, shape.label, active ? Color.Slate900 : Color.Slate300, {
      align: "center",
    });
  }

  // The shape itself, twelve cycles of it, drawn whether or not anything is
  // sounding — it is a legend for the picker above, not a meter.
  strokeRectangle(SCOPE_X, SCOPE_Y, SCOPE_W, SCOPE_H, Color.Slate700, 1);
  const middle = SCOPE_Y + SCOPE_H / 2;
  drawLine(SCOPE_X + 1, middle + 0.5, SCOPE_X + SCOPE_W - 1, middle + 0.5, Color.Slate800, 1);
  drawPolyline(traced, Color.Rose400, 2);

  drawText(PICKER_X, 206, "hold a key to sustain — let go to hear the release", Color.Slate400);
  drawText(SCOPE_X + SCOPE_W, 206, `← octave ${octave} →`, Color.Slate300, {
    align: "right",
  });

  for (const [index, pad] of WHITE.entries()) {
    const x = KEYS_X + index * WHITE_PITCH;
    const down = isKeyDown(pad.key);
    fillRoundedRectangle(x, WHITE_Y, WHITE_W, WHITE_H, 4, down ? Color.Teal500 : Color.Slate200);
    drawText(x + WHITE_W / 2, WHITE_Y + WHITE_H - 34, noteFor(pad, octave) ?? "", Color.Slate900, {
      align: "center",
    });
    drawText(x + WHITE_W / 2, WHITE_Y + WHITE_H - 18, pad.label, Color.Slate600, {
      align: "center",
    });
  }

  for (const pad of BLACK) {
    const x = KEYS_X + (pad.seam + 1) * WHITE_PITCH - 4 - BLACK_W / 2;
    const down = isKeyDown(pad.key);
    fillRoundedRectangle(x, WHITE_Y, BLACK_W, BLACK_H, 3, down ? Color.Teal300 : Color.Slate800);
    drawText(x + BLACK_W / 2, WHITE_Y + 6, noteFor(pad, octave) ?? "", down ? Color.Slate900 : Color.Slate400, {
      align: "center",
    });
    drawText(x + BLACK_W / 2, WHITE_Y + 22, pad.label, down ? Color.Slate900 : Color.Slate600, {
      align: "center",
    });
  }

  drawText(
    getWidth() - 40,
    getHeight() - 24,
    "one speaker, shared — the menu hears these keys too",
    Color.Slate600,
    { align: "right" },
  );
});
