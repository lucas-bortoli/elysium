//! The `ely:sound` surface: playing and stopping voices, what reaches the
//! mixer, what happens when there's no device, and naming notes.

use super::*;
use crate::sound::{RampTarget, SoundLog, Waveform};

#[test]
fn playing_a_tone_returns_a_voice_id() {
    let (runtime, _log) = eval_with_audio(
        "import { playTone } from 'ely:sound'; \
         globalThis.isNumber = typeof playTone(440) === 'number';",
    );
    assert!(global::<bool>(&runtime, "isNumber"));
}

#[test]
fn playing_a_tone_sends_the_mixer_exactly_what_the_program_asked_for() {
    let (_runtime, log) = eval_with_audio(
        "import { playTone, Waveform } from 'ely:sound'; \
         playTone(440, { waveform: Waveform.Square, amplitude: 0.5, \
                         attack: 0.02, decay: 0.05, sustainLevel: 0.3, \
                         release: 0.3, duration: 0.4 });",
    );
    let played = log.played();
    assert_eq!(played.len(), 1);
    let tone = &played[0];
    assert_eq!(tone.waveform, Waveform::Square);
    assert_eq!(tone.frequency_hz, 440.0);
    assert_eq!(tone.amplitude, 0.5);
    assert_eq!(tone.envelope.attack_secs, 0.02);
    assert_eq!(tone.envelope.decay_secs, 0.05);
    assert_eq!(tone.envelope.sustain_level, 0.3);
    assert_eq!(tone.envelope.release_secs, 0.3);
    assert_eq!(tone.envelope.duration_secs, Some(0.4));
}

#[test]
fn an_unspecified_tone_takes_the_documented_defaults() {
    let (_runtime, log) = eval_with_audio("import { playTone } from 'ely:sound'; playTone(440);");
    let played = log.played();
    let tone = &played[0];
    assert_eq!(tone.waveform, Waveform::Triangle);
    assert_eq!(tone.amplitude, 0.6);
    assert_eq!(tone.envelope.attack_secs, 0.01);
    assert_eq!(tone.envelope.decay_secs, 0.0, "no decay by default");
    assert_eq!(
        tone.envelope.sustain_level, 1.0,
        "full sustain is a note that simply holds"
    );
    assert_eq!(tone.envelope.release_secs, 0.1);
    assert_eq!(tone.envelope.duration_secs, None, "no duration sustains");
}

#[test]
fn each_played_tone_gets_its_own_voice_id() {
    let (runtime, log) = eval_with_audio(
        "import { playTone } from 'ely:sound'; \
         const first = playTone(440); \
         const second = playTone(880); \
         globalThis.distinct = first !== second;",
    );
    assert!(global::<bool>(&runtime, "distinct"));
    assert_eq!(log.played().len(), 2);
}

#[test]
fn every_waveform_reaches_the_mixer_as_itself() {
    // The only thing keeping `ely:sound`'s constants and `Waveform::from_id`
    // honest about which number means which shape.
    let (_runtime, log) = eval_with_audio(
        "import { playTone, Waveform } from 'ely:sound'; \
         for (const waveform of [Waveform.Square, Waveform.Triangle, \
                                 Waveform.Sine, Waveform.Noise]) { \
             playTone(440, { waveform }); \
         }",
    );
    let shapes: Vec<Waveform> = log.played().into_iter().map(|t| t.waveform).collect();
    assert_eq!(
        shapes,
        vec![
            Waveform::Square,
            Waveform::Triangle,
            Waveform::Sine,
            Waveform::Noise
        ]
    );
}

#[test]
fn stopping_a_voice_reaches_the_mixer() {
    let (_runtime, log) = eval_with_audio(
        "import { playTone, stopVoice } from 'ely:sound'; \
         const id = playTone(440); \
         stopVoice(id);",
    );
    assert_eq!(log.stopped().len(), 1);
}

#[test]
fn stopping_a_voice_a_program_never_played_still_reaches_the_mixer() {
    // Deliberate: one speaker, shared, exactly like the one screen every
    // program draws to. There is no per-program ownership of a voice.
    let (_runtime, log) =
        eval_with_audio("import { stopVoice } from 'ely:sound'; stopVoice(9999);");
    assert_eq!(log.stopped(), vec![9999]);
}

#[test]
fn a_sustaining_voice_is_stopped_when_the_program_ends() {
    // Teardown names the process rather than each voice: the mixer knows
    // which voices belong to it, so nothing here has to track them.
    let (runtime, log) = eval_with_audio("import { playTone } from 'ely:sound'; playTone(440);");
    assert!(
        log.released().is_empty(),
        "still sounding while the VM lives"
    );

    drop(runtime);
    assert_eq!(
        log.released().len(),
        1,
        "a voice with no duration is released when its program goes away"
    );
}

#[test]
fn a_timed_voice_outlives_the_program_that_started_it() {
    // A sound effect has to finish after the code that triggered it is gone.
    // Teardown does reach the sound device either way; that it spares the
    // timed voice is pinned in the mixer's own tests, which can see the
    // voices themselves.
    let (runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { duration: 0.4 });");
    drop(runtime);
    assert!(
        log.stopped().is_empty(),
        "a timed voice sees itself out rather than being cut short"
    );
}

#[test]
fn playing_a_tone_with_no_sound_device_reports_nothing_rather_than_throwing() {
    let runtime = eval_without_audio(
        "import { playTone, stopVoice } from 'ely:sound'; \
         globalThis.silent = playTone(440) === undefined; \
         globalThis.threw = false; \
         try { stopVoice(1); } catch { globalThis.threw = true; }",
    );
    assert!(global::<bool>(&runtime, "silent"));
    assert!(
        !global::<bool>(&runtime, "threw"),
        "stopping is still a no-op"
    );
}

/// The option a rejected `bendVoice`/`fadeVoice` blamed, or `""`.
fn rejected_bend(source: &str) -> String {
    let (runtime, _log) = eval_with_audio(&format!(
        "import {{ bendVoice, fadeVoice }} from 'ely:sound'; \
         globalThis.blamed = ''; \
         try {{ {source} }} catch (err) {{ globalThis.blamed = err.option ?? ''; }}"
    ));
    global::<String>(&runtime, "blamed")
}

/// Whether the one tone played carried no sweep.
fn played_sweep_is_absent(log: &SoundLog) -> bool {
    log.played()[0].sweep.is_none()
}

/// The option `ToneOptionError` blamed, or `""` if `source` didn't throw one.
fn rejected_option(source: &str) -> String {
    let (runtime, _log) = eval_with_audio(&format!(
        "import {{ playTone, ToneOptionError, Waveform }} from 'ely:sound'; \
         globalThis.option = ''; \
         try {{ {source} }} catch (err) {{ \
             if (err instanceof ToneOptionError) globalThis.option = err.option; \
         }}"
    ));
    global::<String>(&runtime, "option")
}

/// Evaluates `source` and reports whether it threw, and whether the thrown
/// value was of class `expected`.
fn throws(source: &str, expected: &str) -> (bool, bool) {
    let (runtime, _log) = eval_with_audio(&format!(
        "import {{ playTone, Waveform }} from 'ely:sound'; \
         globalThis.threw = false; globalThis.correct = false; \
         try {{ {source} }} catch (err) {{ \
             globalThis.threw = true; \
             globalThis.correct = err instanceof {expected}; \
         }}"
    ));
    (
        global::<bool>(&runtime, "threw"),
        global::<bool>(&runtime, "correct"),
    )
}

#[test]
fn an_out_of_range_amplitude_is_rejected() {
    for amplitude in ["-0.1", "1.1"] {
        assert_eq!(
            rejected_option(&format!("playTone(440, {{ amplitude: {amplitude} }});")),
            "amplitude",
            "amplitude {amplitude} should be rejected, and blamed on amplitude"
        );
    }
}

/// Every option, and the rule each one breaks. The point is the blame: a
/// test that only asserted `RangeError` would pass just as happily if the
/// wrong rule fired.
///
/// The pitch and the sweep's target are in here alongside the rest. They are
/// range-checked against what the sound device can actually sound rather
/// than against a fixed ceiling, but a program is told which option it got
/// wrong either way.
#[test]
fn every_rejected_option_names_itself() {
    let cases = [
        ("playTone(40000);", "frequency"),
        ("playTone(440, { amplitude: 2 });", "amplitude"),
        ("playTone(440, { attack: -1 });", "attack"),
        ("playTone(440, { decay: -1 });", "decay"),
        ("playTone(440, { sustainLevel: 2 });", "sustainLevel"),
        ("playTone(440, { release: -1 });", "release"),
        ("playTone(440, { sweepTo: 40000 });", "sweepTo"),
        (
            "playTone(440, { sweepTo: 200, sweepOver: -1 });",
            "sweepOver",
        ),
        ("playTone(440, { duration: 0 });", "duration"),
    ];
    for (source, blamed) in cases {
        assert_eq!(rejected_option(source), blamed, "{source}");
    }
}

#[test]
fn a_sweep_time_is_only_checked_when_there_is_a_sweep_to_use_it() {
    // `sweepOver` says how long a slide takes, so without a `sweepTo` there
    // is no slide for it to describe and its value never reaches anything.
    let (_runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { sweepOver: -1 });");
    assert_eq!(log.played().len(), 1, "the tone still sounds");
    assert!(played_sweep_is_absent(&log), "and it holds its pitch");
}

#[test]
fn a_frequency_outside_the_audible_range_is_rejected() {
    for frequency in ["-1", "NaN", "Infinity", "40000"] {
        let (threw, correct) = throws(&format!("playTone({frequency});"), "RangeError");
        assert!(threw, "frequency {frequency} should be rejected");
        assert!(correct, "and as a RangeError");
    }
}

#[test]
fn a_negative_attack_decay_or_release_is_rejected() {
    for option in ["attack: -1", "decay: -1", "release: -1"] {
        let (threw, correct) = throws(&format!("playTone(440, {{ {option} }});"), "RangeError");
        assert!(threw, "{option} should be rejected");
        assert!(correct, "and as a RangeError");
    }
}

#[test]
fn a_zero_or_negative_duration_is_rejected() {
    for duration in ["0", "-1"] {
        let (threw, correct) = throws(
            &format!("playTone(440, {{ duration: {duration} }});"),
            "RangeError",
        );
        assert!(threw, "duration {duration} should be rejected");
        assert!(correct, "and as a RangeError");
    }
}

#[test]
fn a_sweep_reaches_the_mixer() {
    let (_runtime, log) = eval_with_audio(
        "import { playTone } from 'ely:sound'; \
         playTone(150, { sweepTo: 50, sweepOver: 0.08 });",
    );
    let played = log.played();
    let sweep = played[0].sweep.expect("the tone should carry a sweep");
    assert_eq!(sweep.to_hz, 50.0);
    assert_eq!(sweep.over_secs, 0.08);
}

#[test]
fn a_tone_without_a_sweep_target_holds_its_pitch() {
    // `sweepOver` alone means nothing: without somewhere to slide to, the
    // note simply stays where it started.
    let (_runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { sweepOver: 2 });");
    assert!(played_sweep_is_absent(&log));
}

#[test]
fn a_sweep_takes_a_tenth_of_a_second_unless_told_otherwise() {
    let (_runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { sweepTo: 220 });");
    let played = log.played();
    let sweep = played[0].sweep.expect("the tone should carry a sweep");
    assert_eq!(sweep.over_secs, 0.1);
}

#[test]
fn a_sweep_target_outside_the_audible_range_is_rejected() {
    for target in ["-1", "40000", "NaN"] {
        let (threw, correct) = throws(
            &format!("playTone(440, {{ sweepTo: {target} }});"),
            "RangeError",
        );
        assert!(threw, "sweepTo {target} should be rejected");
        assert!(correct, "and as a RangeError");
    }
}

#[test]
fn a_negative_sweep_time_is_rejected() {
    let (threw, correct) = throws(
        "playTone(440, { sweepTo: 200, sweepOver: -1 });",
        "RangeError",
    );
    assert!(threw);
    assert!(correct);
}

#[test]
fn a_sustain_level_outside_zero_to_one_is_rejected() {
    // The one envelope field that is a level rather than a duration, so it
    // is bounded on both sides where the others are only bounded below.
    for sustain in ["-0.1", "1.1", "NaN"] {
        let (threw, correct) = throws(
            &format!("playTone(440, {{ sustainLevel: {sustain} }});"),
            "RangeError",
        );
        assert!(threw, "sustainLevel {sustain} should be rejected");
        assert!(correct, "and as a RangeError");
    }
}

#[test]
fn a_waveform_that_is_not_one_of_the_four_is_rejected() {
    let (threw, correct) = throws("playTone(440, { waveform: 9 });", "TypeError");
    assert!(threw);
    assert!(
        correct,
        "an unknown waveform is a TypeError, not a RangeError"
    );
}

#[test]
fn a_non_finite_argument_never_reaches_the_mixer() {
    // A NaN frequency would poison every sample of the whole mix for as long
    // as the voice sounded, so it has to be refused before the mixer sees it.
    let (_runtime, log) = eval_with_audio(
        "import { playTone } from 'ely:sound'; \
         try { playTone(NaN); } catch {}",
    );
    assert!(log.played().is_empty());
}

#[test]
fn every_valid_combination_of_options_is_accepted() {
    let (runtime, _log) = eval_with_audio(
        "import { playTone, Waveform } from 'ely:sound'; \
         globalThis.error = 'none'; \
         try { \
             playTone(440); \
             playTone(1, { amplitude: 0 }); \
             playTone(20000, { amplitude: 1 }); \
             playTone(440, { attack: 0, release: 0 }); \
             playTone(440, { waveform: Waveform.Noise, duration: 10 }); \
             playTone(440, { decay: 0, sustain: 1 }); \
             playTone(440, { decay: 0.5, sustain: 0 }); \
             playTone(150, { sweepTo: 50, sweepOver: 0.08 }); \
             playTone(440, { sweepTo: 0 }); \
         } catch (err) { globalThis.error = String(err); }",
    );
    assert_eq!(global::<String>(&runtime, "error"), "none");
}

/// Reads a list of frequencies computed in JS back as numbers.
fn frequencies(expression: &str) -> Vec<f64> {
    let (runtime, _log) = eval_with_audio(&format!(
        "import {{ noteToFrequency }} from 'ely:sound'; globalThis.results = {expression};"
    ));
    global::<Vec<f64>>(&runtime, "results")
}

#[test]
fn a4_is_the_reference_pitch() {
    assert_eq!(frequencies("[noteToFrequency('A4')]"), vec![440.0]);
}

#[test]
fn an_octave_up_doubles_a_notes_frequency() {
    let found = frequencies("[noteToFrequency('A3'), noteToFrequency('A5')]");
    assert!((found[0] - 220.0).abs() < 1e-9);
    assert!((found[1] - 880.0).abs() < 1e-9);
}

#[test]
fn a_semitone_is_the_twelfth_root_of_two() {
    let found = frequencies("[noteToFrequency('A#4') / 440]");
    assert!((found[0] - 2f64.powf(1.0 / 12.0)).abs() < 1e-9);
}

#[test]
fn a_flat_and_the_sharp_below_it_are_the_same_note() {
    let found = frequencies("[noteToFrequency('Bb3'), noteToFrequency('A#3')]");
    assert_eq!(found[0], found[1]);
}

#[test]
fn a_flat_spelling_can_cross_an_octave_boundary() {
    // Cb4 is the note below C4, which is B3 — the arithmetic carries without
    // a special case.
    let found = frequencies("[noteToFrequency('Cb4'), noteToFrequency('B3')]");
    assert!((found[0] - found[1]).abs() < 1e-9);
}

#[test]
fn c4_sits_nine_semitones_below_a4() {
    // Where an off-by-one in the letter table would actually hide.
    let found = frequencies("[noteToFrequency('C4')]");
    assert!((found[0] - 261.6255).abs() < 1e-3, "got {}", found[0]);
}

#[test]
fn is_note_accepts_every_valid_spelling_and_rejects_the_rest() {
    let (runtime, _log) = eval_with_audio(
        "import { isNote } from 'ely:sound'; \
         globalThis.valid = ['A4', 'C#5', 'Eb3', 'G0', 'B8'].every(isNote); \
         globalThis.invalid = ['H4', 'A', '4A', '', 'A#b4', 'A9', 'a4'] \
             .some(isNote);",
    );
    assert!(global::<bool>(&runtime, "valid"));
    assert!(!global::<bool>(&runtime, "invalid"));
}

#[test]
fn a_name_that_is_not_a_note_throws_past_the_type() {
    let (runtime, _log) = eval_with_audio(
        "import { noteToFrequency, UnknownNoteError } from 'ely:sound'; \
         globalThis.correct = false; \
         try { noteToFrequency('H4'); } \
         catch (err) { globalThis.correct = err instanceof UnknownNoteError; }",
    );
    assert!(global::<bool>(&runtime, "correct"));
}

#[test]
fn a_note_name_can_be_played_directly() {
    let (_runtime, log) = eval_with_audio(
        "import { playTone, noteToFrequency } from 'ely:sound'; \
         playTone(noteToFrequency('A4'));",
    );
    assert_eq!(log.played()[0].frequency_hz, 440.0);
}

#[test]
fn a_tone_can_be_played_by_note_name_instead_of_by_frequency() {
    // The two spellings are one tone: the name is turned into its frequency
    // before anything else happens to it.
    let (_runtime, log) = eval_with_audio(
        "import { playTone, noteToFrequency } from 'ely:sound'; \
         playTone('A4'); playTone(noteToFrequency('A4')); playTone(440);",
    );
    let played = log.played();
    assert_eq!(played.len(), 3);
    assert_eq!(played[0].frequency_hz, played[1].frequency_hz);
    assert_eq!(played[1].frequency_hz, played[2].frequency_hz);
}

#[test]
fn a_note_name_that_is_not_a_note_is_still_rejected_when_played_directly() {
    // Unreachable from TypeScript, which checks a literal name as it is
    // written — but a name can still arrive from parsed data or a cast.
    let runtime = eval_without_audio(
        "import { playTone } from 'ely:sound'; \
         globalThis.thrown = ''; \
         try { playTone('H9' as never); } catch (err) { globalThis.thrown = err.name; }",
    );
    assert_eq!(global::<String>(&runtime, "thrown"), "UnknownNoteError");
}

#[test]
fn the_noise_sequence_matches_the_one_the_mixer_generates() {
    // The example draws the noise channel using this, so a drift here would
    // put a shape on screen that isn't the one being played.
    let expected = {
        let mut register: u16 = 0x7fff;
        let mut values = Vec::new();
        for _ in 0..16 {
            values.push(if register & 1 == 0 { 1.0 } else { -1.0 });
            let bit = (register ^ (register >> 1)) & 1;
            register = (register >> 1) | (bit << 14);
        }
        values
    };
    let runtime = eval_without_audio(
        "import { noiseSequence } from 'ely:sound'; \
         globalThis.values = noiseSequence(16).join(',');",
    );
    let actual: Vec<f64> = global::<String>(&runtime, "values")
        .split(',')
        .map(|value| value.parse().expect("a number"))
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn every_waveform_samples_the_shape_the_mixer_would_sound() {
    // Exported so a program drawing a waveform draws what will actually
    // sound; these are the values the mixer's own tests pin down.
    let runtime = eval_without_audio(
        "import { Waveform, waveformSample } from 'ely:sound'; \
         globalThis.square = waveformSample(Waveform.Square, 0.25); \
         globalThis.squareLate = waveformSample(Waveform.Square, 0.75); \
         globalThis.triangle = waveformSample(Waveform.Triangle, 0.5); \
         globalThis.sine = waveformSample(Waveform.Sine, 0.25);",
    );
    assert_eq!(global::<f64>(&runtime, "square"), 1.0);
    assert_eq!(global::<f64>(&runtime, "squareLate"), -1.0);
    assert_eq!(global::<f64>(&runtime, "triangle"), -1.0);
    assert!((global::<f64>(&runtime, "sine") - 1.0).abs() < 1e-9);
}

#[test]
fn a_tone_reaches_the_mixer_with_the_start_time_it_was_given() {
    let (_runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { startAt: 0.75 });");
    assert_eq!(log.played()[0].starts_at_secs, Some(0.75));
}

#[test]
fn a_tone_with_no_start_time_sounds_as_soon_as_the_mixer_sees_it() {
    let (_runtime, log) = eval_with_audio("import { playTone } from 'ely:sound'; playTone(440);");
    assert_eq!(log.played()[0].starts_at_secs, None);
}

#[test]
fn the_clock_reports_the_sound_played_so_far() {
    let (runtime, log) = eval_with_audio(
        "import { currentTime } from 'ely:sound'; globalThis.before = currentTime();",
    );
    assert_eq!(global::<f64>(&runtime, "before"), 0.0);

    log.play_out(1.5);
    runtime
        .eval_module(
            "later.ts",
            "import { currentTime } from 'ely:sound'; globalThis.after = currentTime();",
        )
        .expect("module failed to evaluate");
    assert!(
        (global::<f64>(&runtime, "after") - 1.5).abs() < 1e-6,
        "the clock followed the device"
    );
}

#[test]
fn the_clock_still_advances_without_a_sound_device() {
    // A scheduler reading a clock frozen at zero would queue everything into
    // the same instant, so a machine with no speakers still gets time.
    let runtime = eval_without_audio(
        "import { currentTime } from 'ely:sound'; \
         globalThis.first = currentTime(); \
         for (let i = 0; i < 200000; i++) {} \
         globalThis.second = currentTime();",
    );
    let first = global::<f64>(&runtime, "first");
    let second = global::<f64>(&runtime, "second");
    assert!(second >= first, "it only ever moves forward");
    assert!(second > 0.0, "and it is running");
}

#[test]
fn scheduling_further_ahead_than_the_horizon_names_start_at() {
    // A voice queued for the far future holds one of the system's slots
    // without ever sounding, and the slots belong to every program at once.
    assert_eq!(
        rejected_option("playTone(440, { startAt: 60 });"),
        "startAt"
    );
}

#[test]
fn scheduling_within_the_horizon_is_accepted() {
    let (_runtime, log) = eval_with_audio(
        "import { currentTime, playTone } from 'ely:sound'; \
         playTone(440, { startAt: currentTime() + 0.1 });",
    );
    assert_eq!(log.played().len(), 1);
}

#[test]
fn a_tone_scheduled_in_the_past_is_accepted_rather_than_refused() {
    // A program that dropped a frame should get a late note, not an
    // exception part-way through a sequence.
    let (_runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { startAt: -5 });");
    assert_eq!(log.played().len(), 1);
}

#[test]
fn a_sequence_derived_from_one_clock_reading_keeps_its_gaps_exact() {
    // The pattern the whole design exists for: read the clock once, derive
    // every instant from it. Whatever the reading was, the spacing is exact.
    let (_runtime, log) = eval_with_audio(
        "import { currentTime, playTone } from 'ely:sound'; \
         const start = currentTime() + 0.1; \
         for (let beat = 0; beat < 4; beat++) { \
             playTone(440, { startAt: start + beat * 0.12 }); \
         }",
    );
    let starts: Vec<f64> = log
        .played()
        .iter()
        .map(|tone| tone.starts_at_secs.expect("scheduled"))
        .collect();
    assert_eq!(starts.len(), 4);
    for pair in starts.windows(2) {
        assert!(
            (pair[1] - pair[0] - 0.12).abs() < 1e-9,
            "gap was {}",
            pair[1] - pair[0]
        );
    }
}

#[test]
fn bending_a_voice_reaches_the_mixer() {
    let (_runtime, log) = eval_with_audio(
        "import { bendVoice, playTone } from 'ely:sound'; \
         const id = playTone(440); bendVoice(id, 880, { overSeconds: 0.2 });",
    );
    let ramped = log.ramped();
    assert_eq!(ramped.len(), 1);
    let (id, target, to, over) = ramped[0];
    assert_eq!(id, 1);
    assert_eq!(target, RampTarget::Frequency);
    assert_eq!(to, 880.0);
    assert_eq!(over, 0.2);
}

#[test]
fn a_voice_can_be_bent_to_a_note_name() {
    // The same spellings playTone accepts, so a melody doesn't have to switch
    // units half way through.
    let (_runtime, log) = eval_with_audio(
        "import { bendVoice, playTone } from 'ely:sound'; \
         const id = playTone('A4'); bendVoice(id, 'A5');",
    );
    assert_eq!(log.ramped()[0].2, 880.0);
}

#[test]
fn a_bend_takes_a_hundredth_of_a_second_unless_told_otherwise() {
    // Long enough not to click, which is the same reason the default attack
    // is what it is.
    let (_runtime, log) = eval_with_audio(
        "import { bendVoice, playTone } from 'ely:sound'; \
         bendVoice(playTone(440), 880);",
    );
    assert_eq!(log.ramped()[0].3, 0.01);
}

#[test]
fn fading_a_voice_reaches_the_mixer() {
    let (_runtime, log) = eval_with_audio(
        "import { fadeVoice, playTone } from 'ely:sound'; \
         fadeVoice(playTone(440), 0.25, { overSeconds: 0.5 });",
    );
    let (_, target, to, over) = log.ramped()[0];
    assert_eq!(target, RampTarget::Amplitude);
    assert_eq!(to, 0.25);
    assert_eq!(over, 0.5);
}

#[test]
fn a_rejected_bend_or_fade_names_the_option_it_blames() {
    let cases = [
        ("bendVoice(1, 40000);", "frequency"),
        ("bendVoice(1, 880, { overSeconds: -1 });", "overSeconds"),
        ("fadeVoice(1, 2);", "level"),
        ("fadeVoice(1, 0.5, { overSeconds: -1 });", "overSeconds"),
    ];
    for (source, blamed) in cases {
        assert_eq!(rejected_bend(source), blamed, "{source}");
    }
}

#[test]
fn bending_a_voice_that_was_never_played_still_reaches_the_mixer() {
    // Whether the id names anything is the mixer's to know — it holds the
    // only truthful account of what is sounding.
    let (_runtime, log) =
        eval_with_audio("import { bendVoice } from 'ely:sound'; bendVoice(9999, 440);");
    assert_eq!(log.ramped()[0].0, 9999);
}
