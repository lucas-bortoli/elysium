//! The `ely:sound` surface: playing and stopping voices, what reaches the
//! mixer, what happens when there's no device, and naming notes.

use super::*;
use crate::audio::{AudioLog, Waveform};

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
                         attack: 0.02, decay: 0.05, sustain: 0.3, \
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
    let (runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; globalThis.id = playTone(440);");
    let id = global::<f64>(&runtime, "id") as u32;
    assert!(
        log.stopped().is_empty(),
        "still sounding while the VM lives"
    );

    drop(runtime);
    assert_eq!(
        log.stopped(),
        vec![id],
        "a voice with no duration is released when its program goes away"
    );
}

#[test]
fn a_timed_voice_outlives_the_program_that_started_it() {
    // A sound effect has to finish after the code that triggered it is gone.
    let (runtime, log) =
        eval_with_audio("import { playTone } from 'ely:sound'; playTone(440, { duration: 0.4 });");
    drop(runtime);
    assert!(
        log.stopped().is_empty(),
        "a timed voice sees itself out rather than being cut short"
    );
}

#[test]
fn playing_a_tone_with_no_audio_device_reports_nothing_rather_than_throwing() {
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

#[test]
fn playing_a_tone_while_every_voice_is_in_use_reports_nothing() {
    let (runtime, _input, log) = build_runtime(test_userland_root());
    log.saturate();
    runtime
        .eval_module(
            "test.ts",
            "import { playTone } from 'ely:sound'; \
             globalThis.silent = playTone(440) === undefined;",
        )
        .expect("module failed to evaluate");
    assert!(global::<bool>(&runtime, "silent"));
}

/// Whether the one tone played carried no sweep.
fn played_sweep_is_absent(log: &AudioLog) -> bool {
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

/// Every option this module checks itself, and the rule each one breaks. The
/// point is the blame: a test that only asserted `RangeError` would pass
/// just as happily if the wrong rule fired.
#[test]
fn a_rejected_option_names_itself() {
    let cases = [
        ("amplitude: 2", "amplitude"),
        ("attack: -1", "attack"),
        ("decay: -1", "decay"),
        ("sustain: 2", "sustain"),
        ("release: -1", "release"),
        ("sweepOver: -1", "sweepOver"),
        ("duration: 0", "duration"),
    ];
    for (option, blamed) in cases {
        assert_eq!(
            rejected_option(&format!("playTone(440, {{ {option} }});")),
            blamed
        );
    }
}

/// The two the kernel range-checks rather than the module, so they arrive as
/// a plain `RangeError` with nothing to blame.
#[test]
fn a_kernel_checked_option_is_a_plain_range_error() {
    for source in ["playTone(40000);", "playTone(440, { sweepTo: 40000 });"] {
        let (threw, correct) = throws(source, "RangeError");
        assert!(threw, "{source} should be rejected");
        assert!(correct, "as a RangeError");
        assert_eq!(rejected_option(source), "", "but not as a ToneOptionError");
    }
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
    let (threw, correct) = throws("playTone(440, { sweepOver: -1 });", "RangeError");
    assert!(threw);
    assert!(correct);
}

#[test]
fn a_sustain_outside_zero_to_one_is_rejected() {
    // The one envelope field that is a level rather than a duration, so it
    // is bounded on both sides where the others are only bounded below.
    for sustain in ["-0.1", "1.1", "NaN"] {
        let (threw, correct) = throws(
            &format!("playTone(440, {{ sustain: {sustain} }});"),
            "RangeError",
        );
        assert!(threw, "sustain {sustain} should be rejected");
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

