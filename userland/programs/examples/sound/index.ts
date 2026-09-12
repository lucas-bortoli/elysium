// The sound device: a keyboard you can play, four shapes to play it with,
// four envelopes to shape those, and an octave to move the whole thing to.
//
// The distinction worth watching here is who ends a note. A tone started
// without a duration sustains until someone stops it, and the program that
// started it owns it — so holding a key here starts a voice and letting go
// stops one. What you hear on the way out is the release, which is why a
// note fades instead of clicking off.
//
// `bell` makes that ownership obvious by separating it from the sound: it
// sustains at nothing, so it rings out to silence while the key is still
// held. The voice is still there, still yours, still occupying one of the
// mixer's slots — it just has nothing left to say.
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
import {
  Waveform,
  bendVoice,
  currentTime,
  isNote,
  noiseSequence,
  noteToFrequency,
  playTone,
  stopVoice,
  waveformSample,
} from "ely:sound";
import type { ToneOptions } from "ely:sound";
import type { Note, VoiceId } from "ely:sound";

/** One waveform, and the digit that selects it. */
const SHAPES: { waveform: Waveform; key: Key; label: string }[] = [
  { waveform: Waveform.Square, key: Key.Digit1, label: "1  square" },
  { waveform: Waveform.Triangle, key: Key.Digit2, label: "2  triangle" },
  { waveform: Waveform.Sine, key: Key.Digit3, label: "3  sine" },
  { waveform: Waveform.Noise, key: Key.Digit4, label: "4  noise" },
];

/** Percussion, on its own four keys. These ignore the piano's note and
 * octave entirely — a drum isn't a pitch, it's a shape.
 *
 * `kick` is the one that needs a sweep: a tone falling from 150 Hz to 50 Hz
 * inside a tenth of a second reads as a thump, and no amount of shaping its
 * loudness gets there. `zap` is the same trick made obvious. The other two
 * are noise, where frequency sets how fast the shift register re-rolls —
 * fast is a hiss, slow is a rattle — which is how one noise generator covers
 * both a hi-hat and a snare.
 */
const DRUMS: {
  key: Key;
  label: string;
  frequency: number;
  options: ToneOptions;
}[] = [
  {
    key: Key.KeyZ,
    label: "Z kick",
    frequency: 150,
    options: {
      waveform: Waveform.Sine,
      sweepTo: 50,
      sweepOver: 0.08,
      attack: 0.001,
      decay: 0.25,
      sustainLevel: 0,
      duration: 0.3,
    },
  },
  {
    key: Key.KeyX,
    label: "X snare",
    frequency: 2000,
    options: {
      waveform: Waveform.Noise,
      attack: 0.001,
      decay: 0.15,
      sustainLevel: 0,
      duration: 0.2,
    },
  },
  {
    key: Key.KeyC,
    label: "C hat",
    frequency: 8000,
    options: {
      waveform: Waveform.Noise,
      attack: 0.001,
      decay: 0.04,
      sustainLevel: 0,
      duration: 0.06,
    },
  },
  {
    key: Key.KeyV,
    label: "V zap",
    frequency: 900,
    options: {
      waveform: Waveform.Square,
      sweepTo: 120,
      sweepOver: 0.18,
      attack: 0.001,
      decay: 0.3,
      sustainLevel: 0,
      duration: 0.25,
    },
  },
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

/** The right edge of the content column, which every row shares. */
const CONTENT_RIGHT = 624;

const PICKER_X = 96;
const SHAPE_PICKER_Y = 46;
const ENVELOPE_PICKER_Y = 80;
const PICKER_W = 120;
const PICKER_H = 28;
const PICKER_PITCH = 136;

const SCOPE_X = 96;
const SCOPE_Y = 122;
const SCOPE_W = 380;
const SCOPE_H = 76;
const SCOPE_AMP = 30;
const SCOPE_CYCLES = 8;

const GRAPH_X = 492;
const GRAPH_W = 132;
const GRAPH_INSET = 8;

const KEYS_X = 108;
const WHITE_Y = 228;
const WHITE_W = 56;
const WHITE_H = 88;
const WHITE_PITCH = 64;
const BLACK_W = 34;
const BLACK_H = 52;

/** A column beside the piano — the drums aren't notes, so they don't belong
 * on it. */
const DRUMS_X = 624;
const DRUMS_W = 80;
const DRUMS_H = 20;
const DRUMS_PITCH = 23;

/** The noise channel's first `SCOPE_CYCLES` values, from the same generator
 * the mixer uses. A voice that is actually sounding keeps advancing its own
 * register, so this is the sequence from the start rather than a live
 * capture: the shape is real, the moment isn't. */
const NOISE: number[] = noiseSequence(SCOPE_CYCLES);

/** The note `pad` sounds with the keyboard based at `octave`.
 *
 * The name is assembled here rather than written out, which makes it an
 * ordinary string as far as the checker is concerned — `isNote` is what turns
 * it back into a `Note` that `playTone` will accept. Clamping the base means
 * the guard never actually rejects anything, but it is the only honest way to
 * hand a built name to an API that takes a closed type. */
function noteFor(pad: Pad, octave: number): Note | undefined {
  const name = `${pad.pitch}${octave + pad.above}`;
  return isNote(name) ? name : undefined;
}

/** The five corners of an envelope's shape, for the panel beside the scope.
 *
 * The horizontal axis is not time. A held note has no fixed length, so the
 * sustain gets a fixed slice of the width and the three real durations share
 * what's left in proportion to each other — enough to compare two envelopes
 * at a glance, not to measure one. */
function envelopeShape(envelope: {
  attack: number;
  decay: number;
  sustainLevel: number;
  release: number;
}): Vector2d[] {
  const left = GRAPH_X + GRAPH_INSET;
  const width = GRAPH_W - GRAPH_INSET * 2;
  const baseline = SCOPE_Y + SCOPE_H - GRAPH_INSET;
  const peak = SCOPE_Y + GRAPH_INSET;

  const hold = width * 0.28;
  const timed = envelope.attack + envelope.decay + envelope.release;
  const share = (seconds: number) =>
    timed > 0 ? ((width - hold) * seconds) / timed : (width - hold) / 3;

  const attackW = share(envelope.attack);
  const decayW = share(envelope.decay);
  const sustainY = baseline - envelope.sustainLevel * (baseline - peak);

  return [
    { x: left, y: baseline },
    { x: left + attackW, y: peak },
    { x: left + attackW + decayW, y: sustainY },
    { x: left + attackW + decayW + hold, y: sustainY },
    { x: left + width, y: baseline },
  ];
}

/** One envelope, and the digit that selects it. Four shapes chosen to sound
 * as unlike each other as the same waveform can: a note that just holds, one
 * that snaps and settles, one that rings out on its own, and one that swells.
 * `bell` sustains at nothing, so it fades to silence while the key is still
 * down — the voice is still yours to release, it just has nothing left to
 * say. */
interface Preset {
  key: Key;
  label: string;
  attack: number;
  decay: number;
  sustainLevel: number;
  release: number;
  /** The shape drawn in the panel, built once — it depends on nothing but
   * the four numbers above. */
  points: Vector2d[];
}

const PRESETS: Preset[] = [
  { key: Key.Digit5, label: "5  organ", attack: 0.02, decay: 0, sustainLevel: 1, release: 0.08 },
  {
    key: Key.Digit6,
    label: "6  pluck",
    attack: 0.005,
    decay: 0.12,
    sustainLevel: 0.15,
    release: 0.15,
  },
  { key: Key.Digit7, label: "7  bell", attack: 0.002, decay: 0.9, sustainLevel: 0, release: 0.4 },
  { key: Key.Digit8, label: "8  pad", attack: 0.25, decay: 0.3, sustainLevel: 0.7, release: 0.6 },
].map((preset) => ({ ...preset, points: envelopeShape(preset) }));

/** The selected shape, traced across the scope. `waveformSample` is the
 * mixer's own arithmetic, so this is the shape that will actually sound
 * rather than an impression of it. */
function trace(waveform: Waveform): Vector2d[] {
  const points: Vector2d[] = [];
  const middle = SCOPE_Y + SCOPE_H / 2;
  for (let column = 0; column <= SCOPE_W; column++) {
    const through = (column / SCOPE_W) * SCOPE_CYCLES;
    const value = waveformSample(waveform, through % 1, Math.floor(through) % SCOPE_CYCLES);
    points.push({ x: SCOPE_X + column, y: middle - value * SCOPE_AMP });
  }
  return points;
}

/** The metronome, which is the one thing here that couldn't be built out of
 * key presses alone.
 *
 * A frame lands every 16 to 33 milliseconds and a beat almost never does, so
 * a click played from an update ticker is a click played at the wrong moment
 * — audibly uneven, and no amount of care in this file would fix it. Instead
 * each click names the instant it should sound, and the ticker only has to
 * stay far enough ahead to keep the queue fed.
 *
 * That's the whole pattern: read the clock once, derive every beat from it by
 * arithmetic, and queue anything falling inside the next `LOOKAHEAD`. The
 * reading trails the speaker a little, which moves the first beat by an
 * inaudible amount and leaves every gap after it exact. */
const BEATS_PER_BAR = 4;
const BEAT_SECONDS = 0.5;
/** How far ahead to queue. Comfortably more than a frame, so a slow frame
 * can't leave a gap, and comfortably inside the two seconds the system
 * allows. */
const LOOKAHEAD = 0.25;

let metronome = false;
/** The instant of the next click to queue, on `currentTime`'s clock. Advances
 * by exactly `BEAT_SECONDS` each time, so nothing accumulates. */
let nextBeat = 0;
/** Counts through the bar, so the downbeat can be the one that stands out. */
let beatInBar = 0;

/** Whether new notes are given a vibrato. Held voices keep whatever they
 * started with, like everything else about them. */
let vibrato = false;
/** How far the held voices have been bent from the notes they started on, in
 * semitones. Bending doesn't retrigger, so a chord slides as one. */
let bentBy = 0;
const BEND_LIMIT = 12;
const BEND_SECONDS = 0.15;

let selected: Waveform = Waveform.Triangle;
/** The envelope every new voice is given. Index 0 is `organ`, which is a
 * plain hold — so the example starts sounding as it did before envelopes
 * were selectable. */
let preset: Preset = PRESETS[0]!;
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

/** Queues every click falling inside the lookahead window. Called each frame,
 * but what it produces doesn't depend on when it runs — only that it runs
 * often enough. */
function queueClicks(): void {
  while (nextBeat < currentTime() + LOOKAHEAD) {
    const downbeat = beatInBar === 0;
    playTone(downbeat ? 1600 : 1000, {
      waveform: Waveform.Noise,
      amplitude: downbeat ? 0.7 : 0.35,
      attack: 0.001,
      decay: 0.03,
      sustainLevel: 0,
      duration: 0.04,
      startAt: nextBeat,
    });
    nextBeat += BEAT_SECONDS;
    beatInBar = (beatInBar + 1) % BEATS_PER_BAR;
  }
}

addUpdateTicker(() => {
  if (wasKeyPressed(Key.Space)) {
    metronome = !metronome;
    if (metronome) {
      // A moment's head start, so the first click isn't already late.
      nextBeat = currentTime() + 0.1;
      beatInBar = 0;
    }
  }
  if (metronome) queueClicks();

  for (const shape of SHAPES) {
    if (wasKeyPressed(shape.key)) {
      selected = shape.waveform;
      traced = trace(selected);
    }
  }

  for (const candidate of PRESETS) {
    if (wasKeyPressed(candidate.key)) preset = candidate;
  }

  // Held voices keep the envelope and shape they started on — those are
  // fixed when a voice starts. The octave only moves the next note played.
  if (wasKeyPressed(Key.ArrowLeft)) octave = Math.max(LOWEST_OCTAVE, octave - 1);
  if (wasKeyPressed(Key.ArrowRight)) octave = Math.min(HIGHEST_OCTAVE, octave + 1);

  if (wasKeyPressed(Key.KeyB)) vibrato = !vibrato;

  // Pitch, unlike the rest, can move on a note already sounding. Up and Down
  // bend every held voice a whole tone without retriggering any of them, so a
  // held chord glides as one — try holding three keys and pressing Up.
  const bend = (wasKeyPressed(Key.ArrowUp) ? 2 : 0) - (wasKeyPressed(Key.ArrowDown) ? 2 : 0);
  if (bend !== 0) {
    bentBy = Math.max(-BEND_LIMIT, Math.min(BEND_LIMIT, bentBy + bend));
    for (const [key, voice] of voices) {
      const pad = PADS.find((candidate) => candidate.key === key);
      const note = pad === undefined ? undefined : noteFor(pad, octave);
      if (note !== undefined) {
        bendVoice(voice, noteToFrequency(note) * 2 ** (bentBy / 12), {
          overSeconds: BEND_SECONDS,
        });
      }
    }
  }

  for (const drum of DRUMS) {
    // Every drum carries a duration, so it sees itself out — there is
    // nothing here to track and nothing to release.
    if (wasKeyPressed(drum.key)) {
      playTone(drum.frequency, drum.options);
    }
  }

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
        const voice = playTone(noteToFrequency(note) * 2 ** (bentBy / 12), {
          waveform: selected,
          attack: preset.attack,
          decay: preset.decay,
          sustainLevel: preset.sustainLevel,
          release: preset.release,
          ...(vibrato ? { vibrato: { depth: 0.3, rate: 6 } } : {}),
        });
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

/** One picker cell, filled when it is the current selection. */
function pickerCell(index: number, y: number, label: string, active: boolean): void {
  const x = PICKER_X + index * PICKER_PITCH;
  fillRoundedRectangle(x, y, PICKER_W, PICKER_H, 6, active ? Color.Teal500 : Color.Slate800);
  drawText(x + PICKER_W / 2, y + 10, label, active ? Color.Slate900 : Color.Slate300, {
    align: "center",
  });
}

addDrawHandler(() => {
  clearScreen(Color.Slate900);
  drawText(getWidth() / 2, 8, "Sound", Color.Amber300, {
    align: "center",
    scale: 2,
  });

  drawText(PICKER_X, 34, "waveform — press 1 to 4", Color.Slate400);
  drawText(CONTENT_RIGHT, 34, "envelope — press 5 to 8", Color.Slate400, {
    align: "right",
  });

  for (const [index, shape] of SHAPES.entries()) {
    pickerCell(index, SHAPE_PICKER_Y, shape.label, shape.waveform === selected);
  }
  for (const [index, candidate] of PRESETS.entries()) {
    pickerCell(index, ENVELOPE_PICKER_Y, candidate.label, candidate === preset);
  }

  // The shape itself, twelve cycles of it, drawn whether or not anything is
  // sounding — it is a legend for the picker above, not a meter.
  strokeRectangle(SCOPE_X, SCOPE_Y, SCOPE_W, SCOPE_H, Color.Slate700, 1);
  const middle = SCOPE_Y + SCOPE_H / 2;
  drawLine(SCOPE_X + 1, middle + 0.5, SCOPE_X + SCOPE_W - 1, middle + 0.5, Color.Slate800, 1);
  drawPolyline(traced, Color.Rose400, 2);

  // The envelope beside the shape: what the note's loudness does, next to
  // what one cycle of it looks like. A different accent so two panels side
  // by side don't read as one graph.
  strokeRectangle(GRAPH_X, SCOPE_Y, GRAPH_W, SCOPE_H, Color.Slate700, 1);
  drawPolyline(preset.points, Color.Amber400, 2);

  drawText(PICKER_X, 206, "hold a key to sustain — let go to hear the release", Color.Slate400);
  drawText(
    PICKER_X,
    getHeight() - 24,
    metronome ? "space  metronome on" : "space  metronome off",
    metronome ? Color.Amber300 : Color.Slate600,
  );
  drawText(CONTENT_RIGHT, 206, `← octave ${octave} →`, Color.Slate300, {
    align: "right",
  });
  drawText(
    CONTENT_RIGHT,
    222,
    `↑ bend ${bentBy > 0 ? "+" : ""}${bentBy} ↓    B vibrato ${vibrato ? "on" : "off"}`,
    bentBy === 0 && !vibrato ? Color.Slate600 : Color.Amber300,
    { align: "right" },
  );

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

  for (const [index, drum] of DRUMS.entries()) {
    const y = WHITE_Y + index * DRUMS_PITCH;
    const down = isKeyDown(drum.key);
    fillRoundedRectangle(DRUMS_X, y, DRUMS_W, DRUMS_H, 4, down ? Color.Rose400 : Color.Slate800);
    drawText(DRUMS_X + DRUMS_W / 2, y + 6, drum.label, down ? Color.Slate900 : Color.Slate400, {
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
