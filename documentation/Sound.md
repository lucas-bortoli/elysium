# Making sounds

A program describes a tone — a shape, a pitch, how loud, how long — and the
system sounds it. Several can sound at once: the kernel mixes every voice
currently playing into the one stream the speaker plays, the same way every
program's drawing lands on the one screen ([1]).

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

## What a tone is

Its **waveform** is its timbre — what it sounds like, as opposed to how high
it is. A square wave is hollow and buzzy, the classic chiptune lead. A
triangle is softer and rounder. A sine is a single pure frequency with no
harmonics at all, which is why it sounds thin and characterless. Noise has no
pitch of its own, only texture, and is what percussion is made of.

Its **frequency**, in hertz, is its pitch. Anything from 0 up to 20000 is
accepted; past that a tone is inaudible anyway.

Its **amplitude**, between 0 and 1, is how loud it is within the mix. Voices
add together rather than replacing one another, so several loud ones at once
share the room — the system leaves headroom for that, and a mix that would
overflow is held at the limit rather than distorting.

Its **envelope** is the shape of its loudness over time: it rises from silence
across the `attack`, falls from full to its `sustain` level across the
`decay`, holds there, then falls the rest of the way to silence across the
`release`.

```
1.0 |    /\
    |   /  \______________
    |  /   ^ sustain level \
    | /                     \
0.0 |/_______________________\___
      A   D       S           R
    note-on                note-off
```

That is a shape rather than a plot: the attack, decay and release are fixed
lengths of time, but the sustain stretches for however long the note is held,
so the horizontal axis isn't a clock.

The envelope exists for a concrete reason. A waveform switched on at full
volume is an abrupt step in the signal, and a step is heard as a click,
whatever note follows it. Even a hundredth of a second of attack removes that
entirely.

The decay and the sustain level are what make a plucked string sound plucked
— a loud attack settling immediately into a quieter tone that holds. Note
that `sustain` is a level between 0 and 1, while `attack`, `decay` and
`release` are all durations in seconds; it is the one setting here measured
in something other than time. A sustain of 1 with no decay is a note that
simply holds, which is the plainest shape and what you get by default. A
sustain of 0 is a note that rings out to silence on its own — and it is
**still a sounding voice** after it goes quiet, holding its place until it is
released or its duration ends. A program that held that note is still holding
it.

## Naming notes

`noteToFrequency` turns a note name into the frequency to play it at. Names
are a letter, an optional sharp or flat, and an octave from 0 to 8 — `"A4"`,
`"C#5"`, `"Eb3"`. They are checked as you write them, so a misspelled note is
caught before the program ever runs.

Notes are laid out in equal temperament: an octave doubles the frequency and
is divided into twelve equal steps, so every semitone multiplies by the same
amount. `A4` is the anchor, at 440 Hz. Because the steps are even, a flat and
the sharp below it name the same pitch — `"Bb3"` and `"A#3"` come out
identical, and so do `"Cb4"` and `"B3"` across an octave boundary.

A name assembled at runtime, out of data the program didn't write itself, is
just a string as far as the checker is concerned. `isNote` says whether one is
a real note name, and narrows it so it can be played.

## Timed and sustaining voices

This is the distinction worth internalising, because it decides who is
responsible for ending a sound.

A tone given a **duration** ends on its own, and **outlives the code that
started it**. Destroy whatever made the noise and the noise still finishes —
which is what a sound effect has to do. There is no way to end one early.

A tone given **no duration** holds until it is stopped, and the program that
started it owns it. If that program ends — cleanly, or by faulting — the
system releases it. Without that, a program that stopped running would leave a
note droning for as long as Elysium stayed up.

Stopping a voice fades it over its own release rather than cutting it, so a
stopped note doesn't click any more than a note that ended by itself.

## When a sound doesn't play

`playTone` reports nothing rather than failing, in two ordinary situations:
the machine has no working sound device, or every voice is already in use.
Neither is an error, and neither needs handling — a program that doesn't care
can ignore the result and carry on, silently, on a machine with no speakers.

## One speaker, shared

A voice id can be stopped by whoever holds it. There is no per-program
ownership of sound, deliberately — it is one speaker, shared by everything
running, exactly as it is one screen. The only thing tied to a program is the
release of its sustaining voices when it ends.

## Errors

`playTone` throws a `RangeError` for an amplitude outside 0 to 1, a frequency
that isn't a number between 0 and 20000, a negative attack, decay or release,
a sustain outside 0 to 1, or a duration that isn't greater than zero. It
throws a `TypeError` for a waveform that isn't one of the four.

`noteToFrequency` throws an `UnknownNoteError` for a name that isn't a note.
Written literally, such a name is rejected before the program runs, so this is
reachable only for a name built at runtime — check it with `isNote` first.

# References

- [1] [Drawing to the screen](Framebuffer.md)
- [2] [Program lifecycle](Lifecycle.md)
- [3] [Running several programs at once](Multitasking.md)
