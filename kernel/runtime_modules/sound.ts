// Making sounds. The kernel mixes a handful of simultaneously sounding
// voices into the one stream the speaker plays; a program describes a tone
// and the system sounds it.

import type { Option } from "ely:container";

declare function __sound_play(
  waveform: number,
  frequency: number,
  amplitude: number,
  envelope: number[],
  sweep: number[],
  duration: number | undefined,
): Option<number>;
declare function __sound_stop(id: number): void;

/** Opaque handle to a sounding voice, returned by `playTone`. */
export type VoiceId = number;

// The shape of a voice's waveform, which is what decides its timbre — what a
// note sounds like, as opposed to its envelope, which decides how it arrives
// and leaves. Kept in sync by hand with kernel/audio.rs's `Waveform`, whose
// `from_id` is the other half of this mapping.
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
 * spans everything audible. */
export type Note = `${Letter}${Accidental}${Octave}`;

export interface ToneOptions {
  /** Defaults to `Waveform.Triangle`. */
  waveform?: Waveform;
  /** How loud this voice is within the mix, `0` to `1`. Defaults to `0.6`. */
  amplitude?: number;
  /** Seconds spent rising from silence to full. Defaults to `0.01`. */
  attack?: number;
  /** Seconds spent falling from full to `sustain` once the attack finishes.
   * Defaults to `0` — no decay, so the note holds at full. */
  decay?: number;
  /** A level, from `0` to `1` — not a duration, unlike every other field
   * here. The fraction of full amplitude the note settles to once its decay
   * finishes, and holds at until it is released. Defaults to `1`, which
   * with the default decay of `0` is a note that simply holds. `0` is a
   * note that rings out to silence on its own while still sounding. */
  sustain?: number;
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
  sustain: number;
  release: number;
  sweepTo: number | undefined;
  sweepOver: number;
  duration: number | undefined;
}

/** Thrown when one of `playTone`'s options is out of range. `option` names
 * which one, so a caller — or a test — can tell two rules apart without
 * reading the message.
 *
 * It extends `RangeError`, so code that only cares that something was out of
 * range can keep catching that. Not every bad option throws this one:
 * `frequency` and `sweepTo` are range-checked by the kernel rather than
 * here, and arrive as a plain `RangeError`. */
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

/** Applies every default in one place, and rejects what the kernel would
 * reject, so a program gets the error from the call it made. */
function resolveToneOptions(options: ToneOptions | undefined): ResolvedToneOptions {
  const resolved: ResolvedToneOptions = {
    waveform: options?.waveform ?? Waveform.Triangle,
    amplitude: options?.amplitude ?? 0.6,
    attack: options?.attack ?? 0.01,
    decay: options?.decay ?? 0,
    sustain: options?.sustain ?? 1,
    release: options?.release ?? 0.1,
    sweepTo: options?.sweepTo,
    sweepOver: options?.sweepOver ?? 0.1,
    duration: options?.duration,
  };

  if (!(resolved.amplitude >= 0 && resolved.amplitude <= 1)) {
    throw new ToneOptionError("amplitude", "must be between 0 and 1");
  }
  if (!(resolved.attack >= 0)) {
    throw new ToneOptionError("attack", "must not be negative");
  }
  if (!(resolved.decay >= 0)) {
    throw new ToneOptionError("decay", "must not be negative");
  }
  if (!(resolved.sustain >= 0 && resolved.sustain <= 1)) {
    throw new ToneOptionError("sustain", "must be between 0 and 1");
  }
  if (!(resolved.release >= 0)) {
    throw new ToneOptionError("release", "must not be negative");
  }
  // `sweepTo` is a frequency, and frequencies are the kernel's to range-check
  // — `frequency` itself isn't checked here either. Checking it in both
  // places is how the two drift into disagreeing about the message.
  if (!(resolved.sweepOver >= 0)) {
    throw new ToneOptionError("sweepOver", "must not be negative");
  }
  if (resolved.duration !== undefined && !(resolved.duration > 0)) {
    throw new ToneOptionError("duration", "must be greater than zero");
  }
  return resolved;
}

/** Starts `frequency` sounding, and returns the voice's id so `stopVoice`
 * can release it later.
 *
 * Nothing sounds, and the result is absent, when the machine has no working
 * sound device or when every voice is already in use. Both are ordinary
 * conditions rather than errors: a program that doesn't care can ignore the
 * result entirely.
 *
 * A tone given a `duration` ends on its own, and outlives the code that
 * started it — destroy whatever made the noise and the noise still finishes.
 * A tone given none holds until `stopVoice`, and is released for you when
 * your program ends. */
export function playTone(
  frequency: number,
  options?: ToneOptions,
): Option<VoiceId> {
  const tone = resolveToneOptions(options);
  return __sound_play(
    tone.waveform,
    frequency,
    tone.amplitude,
    [tone.attack, tone.decay, tone.sustain, tone.release],
    tone.sweepTo === undefined ? [] : [tone.sweepTo, tone.sweepOver],
    tone.duration,
  );
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
  const [, letter, accidental, octave] = match;
  if (letter === undefined || accidental === undefined || octave === undefined) {
    throw new UnknownNoteError(note);
  }

  const base = LETTER_SEMITONES[letter];
  if (base === undefined) {
    throw new UnknownNoteError(note);
  }
  const shift = accidental === "#" ? 1 : accidental === "b" ? -1 : 0;
  // A4 is semitone 9 of octave 4, so 57 semitones above C0.
  const semitonesFromA4 = base + shift + 12 * Number(octave) - 57;
  return 440 * 2 ** (semitonesFromA4 / 12);
}
