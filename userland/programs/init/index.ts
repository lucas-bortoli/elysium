import { none } from "ely:container";
import { addPostInitHandler, delay } from "ely:lifecycle";
import { spawn } from "ely:process";
import { Waveform, playTone } from "ely:sound";
import type { Note } from "ely:sound";

// An A major triad, arpeggiated — the one audible sign the sound device came
// up. Each note is given a duration, so the chime rings out on its own after
// init has already been reaped.
const CHIME: Note[] = ["A4", "C#5", "E5"];

addPostInitHandler(async () => {
  print("Welcome to Elysium!");
  // Spawned before the chime, so the examples browser doesn't wait on it.
  spawn(`${import.meta.directoryName}/../examples/index.ts`, none());

  for (const note of CHIME) {
    await delay(100);
    playTone(note, {
      waveform: Waveform.Triangle,
      amplitude: 0.6,
      attack: 0.01,
      release: 0.3,
      duration: 0.4,
    });
  }
});
