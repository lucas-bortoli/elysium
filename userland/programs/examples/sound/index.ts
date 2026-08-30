// The sound device: a keyboard you can play, and four shapes to play it
// with.
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
import { Key, isKeyDown, wasKeyPressed, wasKeyReleased } from "ely:input";
import { addUpdateTicker } from "ely:lifecycle";
import { hasValue } from "ely:container";
import { Waveform, noteToFrequency, playTone, stopVoice } from "ely:sound";
import type { Note, VoiceId } from "ely:sound";

/** One waveform, and the digit that selects it. */
const SHAPES: { waveform: Waveform; key: Key; label: string }[] = [
  { waveform: Waveform.Square, key: Key.Digit1, label: "1  square" },
  { waveform: Waveform.Triangle, key: Key.Digit2, label: "2  triangle" },
  { waveform: Waveform.Sine, key: Key.Digit3, label: "3  sine" },
  { waveform: Waveform.Noise, key: Key.Digit4, label: "4  noise" },
];

/** One playable note. The note names are checked as they're written — `Note`
 * is a closed type, so a typo here is a compile error rather than something
 * that throws when you press the key. */
interface Pad {
  key: Key;
  note: Note;
  label: string;
}

const WHITE: Pad[] = [
  { key: Key.KeyA, note: "C4", label: "A" },
  { key: Key.KeyS, note: "D4", label: "S" },
  { key: Key.KeyD, note: "E4", label: "D" },
  { key: Key.KeyF, note: "F4", label: "F" },
  { key: Key.KeyG, note: "G4", label: "G" },
  { key: Key.KeyH, note: "A4", label: "H" },
  { key: Key.KeyJ, note: "B4", label: "J" },
  { key: Key.KeyK, note: "C5", label: "K" },
];

/** The sharps sit on the seams between white keys, and there is no seam
 * between E and F or between B and C — those two gaps are what make this
 * read as a piano rather than a second row of buttons. */
const BLACK: (Pad & { seam: number })[] = [
  { key: Key.KeyW, note: "C#4", label: "W", seam: 0 },
  { key: Key.KeyE, note: "D#4", label: "E", seam: 1 },
  { key: Key.KeyT, note: "F#4", label: "T", seam: 3 },
  { key: Key.KeyY, note: "G#4", label: "Y", seam: 4 },
  { key: Key.KeyU, note: "A#4", label: "U", seam: 5 },
];

const PADS: Pad[] = [...WHITE, ...BLACK];

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
/** The selected shape's trace, rebuilt only when the shape changes — a frame
 * that redrew it would spend more time building points than on everything
 * else this program does put together. */
let traced: Vector2d[] = trace(Waveform.Triangle);
/** The sustaining voice each held key started, so releasing the key can stop
 * the right one. */
const voices = new Map<Key, VoiceId>();

addUpdateTicker(() => {
  for (const shape of SHAPES) {
    if (wasKeyPressed(shape.key)) {
      selected = shape.waveform;
      traced = trace(selected);
    }
  }

  for (const pad of PADS) {
    if (wasKeyPressed(pad.key)) {
      // No duration, so the voice holds until it's stopped below. A held
      // voice keeps the shape it started with; picking another only affects
      // the next key pressed.
      const voice = playTone(noteToFrequency(pad.note), { waveform: selected });
      // Absent means nothing sounded — no output device, or every voice
      // already in use. Neither is an error: the key simply has no voice to
      // stop later.
      if (hasValue(voice)) voices.set(pad.key, voice);
    }

    if (wasKeyReleased(pad.key)) {
      const voice = voices.get(pad.key);
      if (hasValue(voice)) stopVoice(voice);
      voices.delete(pad.key);
    }
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

  for (const [index, pad] of WHITE.entries()) {
    const x = KEYS_X + index * WHITE_PITCH;
    const down = isKeyDown(pad.key);
    fillRoundedRectangle(x, WHITE_Y, WHITE_W, WHITE_H, 4, down ? Color.Teal500 : Color.Slate200);
    drawText(x + WHITE_W / 2, WHITE_Y + WHITE_H - 34, pad.note, Color.Slate900, {
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
    drawText(x + BLACK_W / 2, WHITE_Y + 6, pad.note, down ? Color.Slate900 : Color.Slate400, {
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
