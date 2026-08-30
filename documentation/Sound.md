# Making sounds

A program describes a tone — a shape, a pitch, how loud, how it begins and
ends — and the system sounds it. Several can sound at once: the kernel mixes
every voice currently playing into the one stream the speaker plays, the same
way every program's drawing lands on the one screen ([1]).

```ts
import { addUpdateTicker } from "ely:lifecycle";
import { Key, wasKeyPressed } from "ely:input";
import { Waveform, noteToFrequency, playTone } from "ely:sound";

addUpdateTicker(() => {
  if (wasKeyPressed(Key.Space)) {
    playTone(noteToFrequency("C5"), {
      waveform: Waveform.Square,
      duration: 0.15,
    });
  }
});
```

Everything below is one of four things: **what** is sounding (the waveform),
**how high** (frequency, and how it slides), **how loud over time** (the
envelope), and **who ends it** (duration, or you).

---

## The four waveforms

A waveform is a tone's *timbre* — what it sounds like, as distinct from how
high it is. The difference between them is which harmonics they contain:
overtones at whole-number multiples of the pitch, which is what your ear
reads as character.

| | shape over one cycle | harmonics | sounds like |
|---|---|---|---|
| `Square` | `▔▔▁▁▔▔▁▁` | odd only, falling slowly | hollow and buzzy — the classic chiptune lead |
| `Triangle` | `╱╲╱╲╱╲╱╲` | odd only, falling fast | softer and rounder than a square |
| `Sine` | `∿∿∿∿∿∿∿∿` | none at all | thin and characterless — one pure frequency |
| `Noise` | *(no repeat)* | none — no periodicity | no pitch of its own, only texture |

**Noise is the odd one.** It has no repeating shape, so it has no pitch — but
its `frequency` still does something useful. The generator re-rolls a new
random value once per cycle rather than once per sample, so frequency sets
*how fast* it churns:

| frequency | character | good for |
|---|---|---|
| ~8000 Hz | fine hiss | hi-hats, cymbals |
| ~2000 Hz | grainy rasp | snares |
| ~400 Hz | slow rattle | toms, rumble |

That one control is how a single noise generator covers a whole drum kit.

---

## Shaping a note: the envelope

The envelope is the shape of a note's **loudness over time**, in four stages:

```
1.0 |    /\
    |   /  \______________
    |  /   ^ sustain level \
    | /                     \
0.0 |/_______________________\___
      A   D       S           R
    note-on                note-off
```

> This is a shape, not a plot. The attack, decay and release are fixed
> lengths of time, but the sustain stretches for as long as the note is
> held — so the horizontal axis isn't a clock.

| stage | option | units | what it does |
|---|---|---|---|
| **A**ttack | `attack` | seconds | rises from silence to full |
| **D**ecay | `decay` | seconds | falls from full to the sustain level |
| **S**ustain | `sustain` | **a level, 0 to 1** | the level held until release |
| **R**elease | `release` | seconds | falls from wherever it is to silence |

⚠️ **`sustain` is a level, not a duration.** It's the one setting here
measured in something other than seconds, and mixing it up is the easiest
mistake to make with this API.

### Why an attack exists at all

A waveform switched on at full volume is an abrupt step in the signal, and a
step is heard as a **click**, whatever note follows it. Even a hundredth of a
second of attack removes that entirely. The same applies at the other end,
which is why stopping a voice fades it over its release rather than cutting
it.

### What decay and sustain buy you

They're what make a plucked string sound plucked: a loud attack settling
immediately into a quieter tone that holds.

| `decay` | `sustain` | result |
|---|---|---|
| `0` | `1` | a note that simply holds — the plainest shape, and the default |
| short | low | a pluck: snaps, then rings quietly |
| long | `0` | a bell: rings out to silence on its own |
| long | high | a pad: swells and settles |

A `sustain` of `0` is worth dwelling on. The note rings out to silence by
itself — but it is **still a sounding voice** afterwards, holding one of the
mixer's slots until it is released or its duration ends. A program that held
that note is still holding it, long after there's anything to hear.

---

## Sliding the pitch

A note doesn't have to hold the pitch it started on. `sweepTo` names a
frequency to slide to, and `sweepOver` how long the slide takes:

```ts
playTone(150, { sweepTo: 50, sweepOver: 0.08 });
```

That is a kick drum. A tone falling from 150 Hz to 50 Hz inside a tenth of a
second reads as a *thump* rather than as a note, and no amount of shaping its
loudness gets you there — the falling pitch **is** the sound. The same trick
rising or falling more slowly is the "pew" of an arcade shot.

**The slide is geometric, not linear in hertz.** Pitch is heard that way: an
octave is a *doubling*, so an even-sounding glide multiplies by a constant
each moment rather than subtracting one. Half way through a slide from 400 Hz
to 100 Hz you are at **200 Hz** — their geometric mean — not 250, their
average. A linear ramp would spend most of its time sounding low.

### A drum kit from four tones

None of these are samples; they're the primitives above, arranged.

| | waveform | frequency | sweep | envelope |
|---|---|---|---|---|
| kick | `Sine` | 150 | → 50 over 0.08 | attack 0.001, decay 0.25, sustain 0 |
| snare | `Noise` | 2000 | — | attack 0.001, decay 0.15, sustain 0 |
| hi-hat | `Noise` | 8000 | — | attack 0.001, decay 0.04, sustain 0 |
| zap | `Square` | 900 | → 120 over 0.18 | attack 0.001, decay 0.3, sustain 0 |

Every one has a near-instant attack and a sustain of `0` — percussion *is*
that shape. Only the kick and the zap need a sweep; the snare and hi-hat are
the same generator at two churn rates.

---

## Naming notes

`noteToFrequency` turns a note name into the frequency to play it at:

```ts
playTone(noteToFrequency("C#5"));
```

Names are a letter, an optional sharp or flat, and an octave from `0` to `8`.
They're a **closed set** — checked as you write them, so a misspelled note is
a compile error rather than something that throws at runtime.

Notes are laid out in **equal temperament**: an octave doubles the frequency
and is divided into twelve equal steps, so every semitone multiplies by the
same amount (the twelfth root of two, about 1.0595). `A4` is the anchor, at
440 Hz.

| note | semitones from `A4` | frequency |
|---|---|---|
| `A3` | −12 | 220 Hz |
| `C4` | −9 | 261.63 Hz |
| `A4` | 0 | 440 Hz |
| `C#5` | +4 | 554.37 Hz |
| `A5` | +12 | 880 Hz |

Because the steps are even, **a flat and the sharp below it are the same
pitch**: `"Bb3"` and `"A#3"` come out identical, and so do `"Cb4"` and `"B3"`
across an octave boundary.

A name assembled at runtime — out of parsed data a program didn't write
itself — is just a string as far as the checker is concerned. `isNote` says
whether one is a real note name, and narrows it so it can be played:

```ts
if (isNote(raw)) playTone(noteToFrequency(raw));
```

---

## Timed and sustaining voices

**This is the distinction worth internalising**, because it decides who is
responsible for ending a sound.

| | `duration` given | `duration` omitted |
|---|---|---|
| ends when | its duration runs out | you call `stopVoice` |
| outlives its program? | **yes** | no — released when the program ends |
| can you stop it early? | no | yes, that's the point |
| use for | sound effects | held notes, drones |

A tone given a **duration** outlives the code that started it. Destroy
whatever made the noise and the noise still finishes — which is exactly what
a sound effect has to do.

A tone given **no duration** is yours until you release it. If your program
ends, cleanly or by faulting, the system releases it for you. Without that, a
program that stopped running would leave a note droning for as long as
Elysium stayed up.

---

## When a sound doesn't play

`playTone` reports nothing rather than failing, in two ordinary situations:

- the machine has **no working sound device**, or
- **every voice is already in use**.

Neither is an error and neither needs handling. A program that doesn't care
can ignore the result entirely and carry on, silently, on a machine with no
speakers.

## One speaker, shared

A voice id can be stopped by whoever holds it. There is no per-program
ownership of sound, deliberately — it is one speaker, shared by everything
running, exactly as it is one screen. The only thing tied to a program is the
release of its sustaining voices when it ends.

---

## Every option at a glance

| option | default | range | |
|---|---|---|---|
| `waveform` | `Triangle` | one of the four | timbre |
| `amplitude` | `0.6` | 0 to 1 | loudness within the mix |
| `attack` | `0.01` | ≥ 0 seconds | silence → full |
| `decay` | `0` | ≥ 0 seconds | full → sustain |
| `sustain` | `1` | 0 to 1 **(a level)** | held level |
| `release` | `0.1` | ≥ 0 seconds | → silence, once released |
| `sweepTo` | *none* | 0 to 20000 Hz | pitch slides here |
| `sweepOver` | `0.1` | ≥ 0 seconds | how long the slide takes |
| `duration` | *none* | > 0 seconds | omitted, the voice is yours to stop |

Voices **add together** rather than replacing one another, so several loud
ones at once share the room. The system leaves headroom for that, and a mix
that would overflow is held at the limit rather than distorting.

## Errors

Most bad options throw a **`ToneOptionError`**, which extends `RangeError`
and carries an `option` field naming which one you got wrong:

```ts
try {
  playTone(440, { sustain: 5 });
} catch (err) {
  if (err instanceof ToneOptionError) {
    print(`bad ${err.option}`); // "bad sustain"
  }
}
```

| thrown | when |
|---|---|
| `ToneOptionError` | `amplitude` or `sustain` outside 0 to 1 |
| `ToneOptionError` | a negative `attack`, `decay`, `release` or `sweepOver` |
| `ToneOptionError` | a `duration` that isn't greater than zero |
| `RangeError` | `frequency` or `sweepTo` outside 0 to 20000 Hz |
| `TypeError` | a `waveform` that isn't one of the four |
| `UnknownNoteError` | `noteToFrequency` given a name that isn't a note |

`frequency` and `sweepTo` are the exception: they're range-checked by the
kernel rather than by the module, so they arrive as a plain `RangeError` with
no `option` to read. Checking them in both places is how the two would drift
into disagreeing about the message.

`UnknownNoteError` is reachable only for a name built at runtime — one
written literally is rejected before the program runs. Check it with `isNote`
first.

# References

- [1] [Drawing to the screen](Framebuffer.md)
- [2] [Program lifecycle](Lifecycle.md)
- [3] [Running several programs at once](Multitasking.md)
