// Making sounds. The kernel mixes a handful of simultaneously sounding
// voices into the one stream the speaker plays; a program describes a tone
// and the system sounds it.

import type { Option } from "ely:container";

declare function __sound_play(
  waveform: number,
  frequency: number,
  amplitude: number,
  envelope: number[],
  sweep: number[] | undefined,
  duration: number | undefined,
): Option<number>;
declare function __sound_stop(id: number): void;

/** Opaque handle to a sounding voice, returned by `playTone`. */
export type VoiceId = number;

// The shape of a voice's waveform, which is what decides its timbre — what a
// note sounds like, as opposed to its envelope, which decides how it arrives
// and leaves.
//
// Named constants rather than a string union, unlike `Note` below: the set is
// small and the names are arbitrary labels, so there is nothing a string
// spelling would carry that a constant doesn't. The numbers are the contract
// with the kernel, whose `from_id` is the other half of this mapping; the
// mixer's `every_waveform_reaches_the_mixer_as_itself` is what holds the two
// sides together.
export const Waveform = {
  /** Hollow and buzzy — the classic chiptune lead. */
  Square: 0,
  /** Softer and rounder than a square, with the same odd harmonics. */
  Triangle: 1,
  /** A single pure frequency, with no harmonics at all. */
  Sine: 2,
  /** No pitch of its own, only texture — percussion. */
  Noise: 3,
} as const;

/** One of `Waveform`'s named entries (e.g. `Waveform.Triangle`). */
export type Waveform = (typeof Waveform)[keyof typeof Waveform];

type Letter = "A" | "B" | "C" | "D" | "E" | "F" | "G";
type Accidental = "" | "#" | "b";
type Octave = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;

/** Every note name the system understands: a letter, an optional sharp or
 * flat, and an octave — `"A4"`, `"C#5"`, `"Eb3"`. Octaves run 0 to 8, which
 * spans everything audible.
 *
 * A string union rather than named constants, unlike `Waveform` above: the
 * set has 189 members and the string *is* the notation a musician already
 * writes, so `"C#5"` says everything `Note.CSharp5` would and can be built
 * from parsed data besides. `isNote` is what turns a built one back into a
 * `Note`. */
export type Note = `${Letter}${Accidental}${Octave}`;

export interface ToneOptions {
  /** Defaults to `Waveform.Triangle`. */
  waveform?: Waveform;
  /** How loud this voice is within the mix, `0` to `1`. Defaults to `0.6`. */
  amplitude?: number;
  /** Seconds spent rising from silence to full. Defaults to `0.01`. */
  attack?: number;
  /** Seconds spent falling from full to `sustainLevel` once the attack
   * finishes. Defaults to `0` — no decay, so the note holds at full. */
  decay?: number;
  /** The fraction of full amplitude the note settles to once its decay
   * finishes, `0` to `1`, and holds at until it is released. Defaults to `1`,
   * which with the default decay of `0` is a note that simply holds. `0` is a
   * note that rings out to silence on its own while still sounding. */
  sustainLevel?: number;
  /** Seconds spent falling the rest of the way to silence once released.
   * Defaults to `0.1`. */
  release?: number;
  /** The frequency, in hertz, the pitch slides to over `sweepOver` seconds.
   * Omitted, the note holds the pitch it started on.
   *
   * A slide is what a drum is: start near 150 and fall to near 50 inside a
   * tenth of a second and it reads as a thump rather than as a note. It is
   * also the falling "pew" of an arcade shot. The slide is geometric, since
   * that is how pitch is heard — an even-sounding glide multiplies rather
   * than subtracts. */
  sweepTo?: number;
  /** How long the slide to `sweepTo` takes, after which the pitch holds
   * there. Defaults to `0.1`. Ignored without a `sweepTo`. */
  sweepOver?: number;
  /** Seconds to hold at the sustain level before the release begins, so the
   * voice sounds for `duration + release` in total. Omitted, the voice holds
   * until `stopVoice`. */
  duration?: number;
}

interface ResolvedToneOptions {
  waveform: Waveform;
  amplitude: number;
  attack: number;
  decay: number;
  sustainLevel: number;
  release: number;
  sweepTo: number | undefined;
  sweepOver: number;
  duration: number | undefined;
}

/** Thrown when one of `playTone`'s options is out of range. `option` names
 * which one, so a caller — or a test — can tell two rules apart without
 * reading the message. It extends `RangeError`, so code that only cares that
 * something was out of range can keep catching that. */
export class ToneOptionError extends RangeError {
  readonly option: string;

  /** `requirement` completes the sentence the option name starts, so the
   * message and the `option` field can't drift apart. */
  constructor(option: string, requirement: string) {
    super(`${option} ${requirement}`);
    this.name = "ToneOptionError";
    this.option = option;
  }
}

/** Applies every default in one place. Nothing is range-checked here: the
 * kernel checks every option, and it tags what it throws with the option's
 * name so `playTone` can re-type it. Checking in both places is how the two
 * drift into disagreeing about the message. */
function withDefaults(options: ToneOptions | undefined): ResolvedToneOptions {
  return {
    waveform: options?.waveform ?? Waveform.Triangle,
    amplitude: options?.amplitude ?? 0.6,
    attack: options?.attack ?? 0.01,
    decay: options?.decay ?? 0,
    sustainLevel: options?.sustainLevel ?? 1,
    release: options?.release ?? 0.1,
    sweepTo: options?.sweepTo,
    sweepOver: options?.sweepOver ?? 0.1,
    duration: options?.duration,
  };
}

/** Every option name the kernel can tag a rejection with. A message whose
 * tag isn't one of these isn't a rejected option, so it travels on unchanged
 * — a bad `waveform` among them, which stays the `TypeError` it is. */
const TONE_OPTIONS = [
  "frequency",
  "amplitude",
  "attack",
  "decay",
  "sustainLevel",
  "release",
  "sweepTo",
  "sweepOver",
  "duration",
];

/** Re-throws the kernel's `"<option>: <requirement>"` rejections as a
 * `ToneOptionError` naming the option, and anything else untouched. */
function rethrowToneError(err: unknown): never {
  const raw = err instanceof Error ? err.message : String(err);
  const separator = raw.indexOf(": ");
  const option = separator === -1 ? "" : raw.slice(0, separator);
  if (TONE_OPTIONS.includes(option)) {
    throw new ToneOptionError(option, raw.slice(separator + 2));
  }
  throw err;
}

/** Starts `pitch` sounding, and returns the voice's id so `stopVoice` can
 * release it later. `pitch` is a note name or a frequency in hertz —
 * `playTone("C5")` and `playTone(523.25)` are the same tone.
 *
 * Nothing sounds, and the result is absent, when the machine has no working
 * sound device. That is an ordinary condition rather than an error: a program
 * that doesn't care can ignore the result entirely.
 *
 * A tone given a `duration` ends on its own, and outlives the code that
 * started it — destroy whatever made the noise and the noise still finishes.
 * A tone given none holds until `stopVoice`, and is released for you when
 * your program ends.
 *
 * TODO: a sounding voice can be started and stopped but not changed. Bending
 * a held note's pitch or fading its amplitude wants `setVoiceFrequency` and
 * `setVoiceAmplitude`; until those exist, everything about a voice is fixed
 * at the moment it starts. */
export function playTone(
  pitch: Note | number,
  options?: ToneOptions,
): Option<VoiceId> {
  const tone = withDefaults(options);
  const frequency = typeof pitch === "number" ? pitch : noteToFrequency(pitch);
  try {
    return __sound_play(
      tone.waveform,
      frequency,
      tone.amplitude,
      [tone.attack, tone.decay, tone.sustainLevel, tone.release],
      tone.sweepTo === undefined ? undefined : [tone.sweepTo, tone.sweepOver],
      tone.duration,
    );
  } catch (err) {
    rethrowToneError(err);
  }
}

/** Releases the voice `id` names. It fades over its own release rather than
 * cutting off, so a stopped note doesn't click. An id whose voice has
 * already finished is ignored. */
export function stopVoice(id: VoiceId): void {
  __sound_stop(id);
}

export class UnknownNoteError extends Error {
  constructor(note: string) {
    super(`${note} is not a note name`);
    this.name = "UnknownNoteError";
  }
}

// Semitones above the C below each note. Sharps and flats shift by one, and
// the arithmetic carries across an octave on its own, so `Cb4` lands on B3
// without a special case.
const LETTER_SEMITONES: Record<string, number> = {
  C: 0,
  D: 2,
  E: 4,
  F: 5,
  G: 7,
  A: 9,
  B: 11,
};

const NOTE_PATTERN = /^([A-G])([#b]?)([0-8])$/;

/** Whether `value` names a note, narrowing it to `Note` when it does. Note
 * names written literally are checked as you type them; this is for one
 * built at runtime, out of parsed data a program didn't write itself. */
export function isNote(value: string): value is Note {
  return NOTE_PATTERN.test(value);
}

/** The frequency, in hertz, of the note `note` names.
 *
 * Notes are laid out in equal temperament: an octave doubles the frequency
 * and is divided into twelve equal steps, so each semitone multiplies by the
 * twelfth root of two. `A4` is the anchor, at 440 Hz. A flat and the sharp
 * below it name the same pitch, so `Bb3` and `A#3` come out identical.
 *
 * Throws `UnknownNoteError` for a name that isn't one — unreachable from
 * TypeScript, which checks names against `Note` as they're written, but a
 * name can still arrive from parsed data or a cast. */
export function noteToFrequency(note: Note): number {
  const match = NOTE_PATTERN.exec(note);
  if (match === null) {
    throw new UnknownNoteError(note);
  }
  // The pattern is what guarantees these: a match has all three groups, and
  // its first is one of the seven letters `LETTER_SEMITONES` is keyed by.
  const [, letter, accidental, octave] = match as unknown as [string, string, string, string];
  const base = LETTER_SEMITONES[letter]!;
  const shift = accidental === "#" ? 1 : accidental === "b" ? -1 : 0;
  // A4 is semitone 9 of octave 4, so 57 semitones above C0.
  const semitonesFromA4 = base + shift + 12 * Number(octave) - 57;
  return 440 * 2 ** (semitonesFromA4 / 12);
}

/** One cycle of `waveform` sampled at `phase` (`0` to `1` through the cycle),
 * in `-1` to `1`. `noiseStep` picks which value of the noise generator's
 * sequence to use, and is ignored by the three pitched waveforms.
 *
 * This is the arithmetic the mixer itself uses, exported so a program drawing
 * a waveform draws the shape that will actually sound rather than an
 * impression of it. */
export function waveformSample(
  waveform: Waveform,
  phase: number,
  noiseStep = 0,
): number {
  switch (waveform) {
    case Waveform.Square:
      return phase < 0.5 ? 1 : -1;
    case Waveform.Triangle:
      return 4 * Math.abs(phase - 0.5) - 1;
    case Waveform.Sine:
      return Math.sin(phase * 2 * Math.PI);
    case Waveform.Noise:
      return noiseSequence(noiseStep + 1)[noiseStep]!;
  }
}

/** The first `length` values of the noise generator's sequence, each `1` or
 * `-1`.
 *
 * The generator is a 15-bit shift register clocked once per cycle rather than
 * once per sample, which is why noise holds one value for a whole cycle and
 * why a voice's frequency sets how fast it churns instead of how high it
 * sounds. The sequence always starts from the same state, so this is what a
 * voice sounds on the way in; one already sounding has advanced past it. */
export function noiseSequence(length: number): number[] {
  let register = 0x7fff;
  const values: number[] = [];
  for (let step = 0; step < length; step++) {
    values.push((register & 1) === 0 ? 1 : -1);
    const bit = (register ^ (register >> 1)) & 1;
    register = (register >> 1) | (bit << 14);
  }
  return values;
}
