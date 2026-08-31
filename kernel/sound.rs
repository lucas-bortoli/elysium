//! The Sound device: a mixer that sums any number of simultaneously
//! sounding voices into the one stream the output device plays.
//!
//! `Sound` never leaks `cpal`'s device or stream types outside this module,
//! the same way `Framebuffer` never leaks `winit` types — see
//! `kernel/window.rs`, which establishes that pattern for the window's OS
//! resource. Here the OS resource is the default output stream instead of a
//! window, and unlike the window, nothing else in the kernel needs to reach
//! it: no other device shares state with it, so it's started independently
//! of `Devices` and `ProcessManager`.
//!
//! Mixing is [`Mixer::render`], which takes a buffer and a sample rate and
//! nothing else, so a mix can be verified against a plain `Vec<f32>` with no
//! real sound device involved — the same separation `framebuffer::rasterize`
//! uses to test rasterization without a window.
//!
//! This is the one place in the kernel that crosses an OS thread boundary.
//! The output callback runs on a thread the sound backend owns, invoked on
//! its own schedule, so the state it touches can't be `Rc` and the main
//! thread can't reach into it directly. The voices therefore live entirely
//! inside the callback, and the main thread only ever sends it commands
//! down an `mpsc` channel, drained at the top of each callback. A callback
//! that blocks produces a buffer underrun, heard as a click or a dropout,
//! and a `Mutex` shared with the main thread is exactly how you get one:
//! the main thread can be descheduled still holding the lock while the
//! higher-priority sound thread waits on it. `try_recv` never blocks
//! waiting for a message and never allocates on the receiving side, and
//! sends here are rare — a handful per frame at most — so contention is
//! effectively absent. That is a practical guarantee rather than a formal
//! one: `std::sync::mpsc` isn't documented as lock-free, and a receiver can
//! briefly spin when a sender has reserved a slot without publishing it
//! yet. A lock-free single-producer ring is where this design would go if
//! it ever needed to be rigorous.
//!
//! Unlike a missing window (load-bearing for everything else the kernel
//! does), a missing or unusable sound device is not fatal: a real machine
//! may simply have none. [`start`] returns `None` rather than panicking,
//! logging the specific reason, and `ely:sound`'s bindings degrade to silent
//! no-ops, so both the kernel and every program boot normally either way.

use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rquickjs::{Ctx, Result};

use crate::bindings::bind;
use crate::process::ProcessId;

/// Hard ceiling on simultaneously sounding voices. A `play` past this takes
/// the slot of the faintest voice already sounding — see [`Mixer::add`].
/// Mixing cost is linear in the voice count and every voice takes headroom
/// from the ones already sounding; this bounds both.
const MAX_VOICES: usize = 32;

/// Every voice is summed at full scale and the result clamped, so the mix is
/// attenuated to leave room for several at once. Clipping a summed signal
/// folds its peaks flat, which is heard as harsh distortion rather than as
/// loudness — the trade is a quieter mix that stays clean at any voice count
/// this device permits.
const MASTER_GAIN: f32 = 0.2;

/// The state a 15-bit LFSR powers up in; see [`Waveform::Noise`].
const LFSR_SEED: u16 = 0x7fff;

/// The highest frequency `ely:sound` will accept on a device fast enough for
/// it. Past roughly 20 kHz a tone is inaudible anyway.
///
/// Half the output rate is the other limit, and the lower of the two is what
/// actually applies — see [`Sound::max_frequency_hz`]. Past it a tone
/// aliases back down into something audible but not the note that was asked
/// for, which is worse than refusing it.
const MAX_FREQUENCY_HZ: f32 = 20_000.0;

/// The output rate to ask a device for, clamped into whatever range it
/// actually supports. Mixing work is linear in both the voice count and the
/// sample rate, and a device willing to run at 384 kHz will happily do so —
/// eight times the work per second for frequencies an ear can't hear, since
/// 48 kHz already covers the audible range twice over.
const PREFERRED_SAMPLE_RATE: u32 = 48_000;

/// How far ahead of now a tone may be scheduled. A voice booked for the
/// future holds one of [`MAX_VOICES`] slots before it makes any sound, and
/// voices are one pool shared by every program — so without a limit, a
/// program could reserve the whole device for an hour and silence the
/// machine for everyone. Two seconds is far past the hundred milliseconds a
/// lookahead scheduler actually queues, and bounds the worst case to
/// something that clears itself.
const MAX_SCHEDULE_AHEAD_SECS: f64 = 2.0;

/// Stands in for the mixer's clock on a machine with no sound device, so
/// that a program scheduling against `currentTime` behaves the same whether
/// or not anything can be heard.
static SILENT_CLOCK: OnceLock<Instant> = OnceLock::new();

pub type VoiceId = u32;

/// The shape of one voice's waveform, which is what decides its timbre —
/// what a note sounds like, as opposed to [`Envelope`], which decides how it
/// arrives and leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Square,
    Triangle,
    Sine,
    Noise,
}

impl Waveform {
    /// The waveform `id` names, or `None` if it names none of them. `ely:sound`
    /// exports these same numbers as its `Waveform` constants, so this match
    /// is the contract between the two rather than something inferred from
    /// the order the variants happen to be declared in.
    pub fn from_id(id: u8) -> Option<Waveform> {
        match id {
            0 => Some(Waveform::Square),
            1 => Some(Waveform::Triangle),
            2 => Some(Waveform::Sine),
            3 => Some(Waveform::Noise),
            _ => None,
        }
    }

    /// This waveform's value at `phase` (`0.0..1.0` through one cycle), in
    /// `-1.0..=1.0`. `lfsr` is only read by [`Waveform::Noise`]; the pitched
    /// waveforms ignore it.
    fn sample(self, phase: f32, lfsr: u16) -> f32 {
        match self {
            Waveform::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
            Waveform::Sine => (phase * std::f32::consts::TAU).sin(),
            Waveform::Noise => {
                if lfsr & 1 == 0 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

/// Advances a 15-bit Fibonacci LFSR one step, tapping bits 0 and 1 — the
/// polynomial `x^15 + x^14 + 1`, which is primitive, so the register visits
/// all 32767 of its non-zero states before repeating. This is the Game Boy's
/// noise channel, and it's clocked once per waveform cycle rather than once
/// per sample, which is what lets a voice's frequency pitch its noise.
/// Zero is unreachable from a non-zero seed, so the sequence can't collapse.
fn advance_lfsr(lfsr: u16) -> u16 {
    let bit = (lfsr ^ (lfsr >> 1)) & 1;
    (lfsr >> 1) | (bit << 14)
}

/// The shape of a note's amplitude over its life, multiplied against the
/// waveform sample by sample: it rises from silence across `attack_secs`,
/// falls from full to `sustain_level` across `decay_secs`, holds there, then
/// falls the rest of the way to silence across `release_secs`.
///
/// Its job is that a waveform switched on at full amplitude is a step
/// discontinuity in the signal, heard as a click at both ends of every note.
/// Even a ten-millisecond attack removes that entirely.
///
/// The decay and the sustain level are what make a struck or plucked note
/// read as struck: a loud attack settling into a quieter held tone. A
/// `sustain_level` of `1.0` with no decay is a note that simply holds, which
/// is the plainest shape and the one `ely:sound` defaults to. A level of
/// `0.0` is a note that rings out to silence on its own — still a sounding
/// voice, occupying a slot until it is released or its duration ends, but
/// inaudible long before then.
#[derive(Debug, Clone, Copy)]
pub struct Envelope {
    pub attack_secs: f32,
    pub decay_secs: f32,
    /// The fraction of full amplitude the note settles to once its decay is
    /// done — a level, unlike every other field here, which are durations.
    pub sustain_level: f32,
    pub release_secs: f32,
    /// How long the note holds at its sustain level before its release
    /// begins, so a voice sounds for `duration_secs + release_secs` in
    /// total. `None` holds until [`Sound::stop`].
    pub duration_secs: Option<f32>,
}

/// A slide from the note's own pitch to another one, over a fixed time.
///
/// This is what a kick drum is: a tone starting near 150 Hz and falling to
/// near 50 Hz inside a tenth of a second reads as a thump rather than as a
/// note, and no amount of shaping its loudness produces that. It is also the
/// falling "pew" of an arcade shot, which the Game Boy's first channel could
/// do in hardware for the same reason — it is cheap and it is unmistakable.
#[derive(Debug, Clone, Copy)]
pub struct Sweep {
    pub to_hz: f32,
    pub over_secs: f32,
}

/// How a ramp travels between its two settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Curve {
    /// Multiplies its way across, which is how pitch is heard — an octave is
    /// a doubling, so an even-sounding glide multiplies by a constant each
    /// moment rather than adding one. A linear ramp from 800 Hz to 200 Hz
    /// spends most of its time sounding low.
    Geometric,
    /// Adds its way across, for loudness, which is not heard that way.
    Linear,
}

/// Which of a voice's settings a ramp is moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampTarget {
    Frequency,
    Amplitude,
}

impl RampTarget {
    fn curve(self) -> Curve {
        match self {
            RampTarget::Frequency => Curve::Geometric,
            RampTarget::Amplitude => Curve::Linear,
        }
    }
}

/// One of a voice's settings moving to another over a stretch of samples.
///
/// A [`Sweep`] is this fixed at the note's start, and a bend is this arriving
/// later — one mechanism, so a voice never has two things deciding its pitch.
#[derive(Debug, Clone, Copy)]
struct Ramp {
    from: f32,
    to: f32,
    starts_at_sample: u64,
    /// Zero snaps straight to `to`.
    over_samples: u64,
    curve: Curve,
}

impl Ramp {
    /// Where the value sits at `sample`: `from` before the ramp begins, `to`
    /// once it is done.
    fn value_at(&self, sample: u64) -> f32 {
        if sample < self.starts_at_sample {
            return self.from;
        }
        // A ramp with no length arrives the moment it begins, rather than one
        // sample later.
        if self.over_samples == 0 {
            return self.to;
        }
        let through =
            ((sample - self.starts_at_sample) as f32 / self.over_samples as f32).clamp(0.0, 1.0);
        match self.curve {
            // Endpoints at or below zero can't be multiplied toward, so those
            // fall back to a straight line.
            Curve::Geometric if self.from > 0.0 && self.to > 0.0 => {
                self.from * (self.to / self.from).powf(through)
            }
            _ => self.from + (self.to - self.from) * through,
        }
    }
}

/// A setting wobbling either side of itself, over and over.
///
/// Periodic modulation has to be described rather than driven: a program
/// nudging a voice from its update ticker gets one step per frame, so a 6 Hz
/// wobble at 30 frames a second is five steps a cycle and sounds like stairs.
/// Named here, it is computed for every sample.
#[derive(Debug, Clone, Copy)]
pub struct Wobble {
    /// Semitones either side for a vibrato; a fraction of the level for a
    /// tremolo.
    pub depth: f32,
    pub rate_hz: f32,
}

impl Wobble {
    /// Where in the wobble `elapsed` seconds sits, in `-1.0..=1.0`. Measured
    /// from the voice's own start, so a wobble begins when its note does.
    fn at(&self, elapsed: f32) -> f32 {
        (elapsed * self.rate_hz * std::f32::consts::TAU).sin()
    }
}

/// Everything [`Sound::play`] needs to start one voice.
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    pub waveform: Waveform,
    pub frequency_hz: f32,
    /// Scales this voice within the mix, before [`MASTER_GAIN`].
    pub amplitude: f32,
    pub envelope: Envelope,
    /// Where the pitch slides to, if it slides at all.
    pub sweep: Option<Sweep>,
    /// Wobbles the pitch either side of itself, in semitones — the unit the
    /// effect is described in, and the one that sounds the same depth at
    /// every pitch.
    pub vibrato: Option<Wobble>,
    /// Wobbles the loudness either side of itself.
    pub tremolo: Option<Wobble>,
    /// When this voice should begin, on the same clock [`Sound::current_time`]
    /// reports. `None` starts it as soon as the mixer sees it.
    ///
    /// Absolute rather than a delay, which is what makes a schedule hold
    /// together: a delay would start counting when the command came off the
    /// channel, so every voice would absorb that latency separately and the
    /// error would pile up across a sequence. Naming the instant instead
    /// means a stale reading of the clock shifts everything once, by the same
    /// amount, and the gaps between voices stay exact.
    pub starts_at_secs: Option<f64>,
}

/// Why a set of tone parts was refused: which option was wrong, and what it
/// should have been.
///
/// The two halves stay separate all the way out to `ely:sound`, which
/// re-types them as a `ToneOptionError` carrying the option name as a field.
/// A caller — or a test — can then tell two rules apart without reading the
/// message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToneError {
    /// The option's name as a program spells it: `sweepTo`, not `to_hz`.
    pub option: &'static str,
    /// Completes the sentence the option name starts, so the message and the
    /// name can't drift apart.
    pub requirement: String,
}

impl ToneError {
    fn new(option: &'static str, requirement: impl Into<String>) -> Self {
        Self {
            option,
            requirement: requirement.into(),
        }
    }
}

/// Checks one duration: finite and not negative.
fn checked_secs(option: &'static str, secs: f32) -> std::result::Result<f32, ToneError> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(ToneError::new(option, "must not be negative"));
    }
    Ok(secs)
}

/// Checks one `0.0..=1.0` level.
fn checked_level(option: &'static str, level: f32) -> std::result::Result<f32, ToneError> {
    if !level.is_finite() || !(0.0..=1.0).contains(&level) {
        return Err(ToneError::new(option, "must be between 0 and 1"));
    }
    Ok(level)
}

/// Checks one frequency against what this device can sound without aliasing.
fn checked_frequency(
    option: &'static str,
    frequency_hz: f32,
    max_frequency_hz: f32,
) -> std::result::Result<f32, ToneError> {
    if !frequency_hz.is_finite() || !(0.0..=max_frequency_hz).contains(&frequency_hz) {
        return Err(ToneError::new(
            option,
            format!("must be between 0 and {max_frequency_hz} Hz"),
        ));
    }
    Ok(frequency_hz)
}

/// The loose numbers a program passed, before any of them have been checked.
/// Every field is spelled the way `ely:sound` spells it, so a rejection can
/// name the option a program actually wrote.
pub struct ToneParts {
    pub waveform: u8,
    pub frequency_hz: f32,
    pub amplitude: f32,
    pub attack_secs: f32,
    pub decay_secs: f32,
    pub sustain_level: f32,
    pub release_secs: f32,
    pub sweep_to_hz: Option<f32>,
    pub sweep_over_secs: f32,
    pub duration_secs: Option<f32>,
    pub starts_at_secs: Option<f64>,
    /// Depth and rate, as a pair, or absent.
    pub vibrato: Option<[f32; 2]>,
    pub tremolo: Option<[f32; 2]>,
}

/// What the device this tone is bound for can actually do, and when it is.
/// Both are properties of the machine rather than of the tone, which is why
/// they arrive separately from [`ToneParts`].
pub struct ToneLimits {
    pub max_frequency_hz: f32,
    pub now_secs: f64,
}

/// The deepest a vibrato may wobble. An octave either side is already far
/// past anything musical.
const MAX_VIBRATO_SEMITONES: f32 = 12.0;

/// The fastest either wobble may run. Past this it stops being heard as a
/// wobble at all and becomes part of the timbre.
const MAX_WOBBLE_RATE_HZ: f32 = 50.0;

/// Checks one wobble, blaming the option as a whole and naming the part that
/// was wrong — so a nested option still reports itself the way a flat one
/// does.
fn checked_wobble(
    option: &'static str,
    wobble: Option<[f32; 2]>,
    max_depth: f32,
    depth_units: &str,
) -> std::result::Result<Option<Wobble>, ToneError> {
    let Some([depth, rate_hz]) = wobble else {
        return Ok(None);
    };
    if !depth.is_finite() || !(0.0..=max_depth).contains(&depth) {
        return Err(ToneError::new(
            option,
            format!("needs a depth between 0 and {max_depth} {depth_units}"),
        ));
    }
    if !rate_hz.is_finite() || !(0.0..=MAX_WOBBLE_RATE_HZ).contains(&rate_hz) {
        return Err(ToneError::new(
            option,
            format!("needs a rate between 0 and {MAX_WOBBLE_RATE_HZ} Hz"),
        ));
    }
    Ok(Some(Wobble { depth, rate_hz }))
}

/// Checks a bend's target pitch and the time it takes.
pub fn checked_bend(
    frequency_hz: f32,
    over_secs: f32,
    max_frequency_hz: f32,
) -> std::result::Result<(f32, f32), ToneError> {
    Ok((
        checked_frequency("frequency", frequency_hz, max_frequency_hz)?,
        checked_secs("overSeconds", over_secs)?,
    ))
}

/// Checks a fade's target level and the time it takes.
pub fn checked_fade(level: f32, over_secs: f32) -> std::result::Result<(f32, f32), ToneError> {
    Ok((
        checked_level("level", level)?,
        checked_secs("overSeconds", over_secs)?,
    ))
}

/// Assembles a [`Tone`] out of the loose numbers a program passed, refusing
/// anything the mixer shouldn't see.
///
/// This is the only place tone options are range-checked. `ely:sound` applies
/// defaults and does no checking of its own, so there is one statement of
/// each rule and one wording for each message.
///
/// The finiteness checks are the load-bearing ones: a NaN frequency never
/// advances past its phase wrap, so the voice's every sample is NaN, and it
/// poisons the whole mix for as long as it sounds.
pub fn tone_from_parts(
    parts: ToneParts,
    limits: ToneLimits,
) -> std::result::Result<Tone, ToneError> {
    let Some(waveform) = Waveform::from_id(parts.waveform) else {
        return Err(ToneError::new("waveform", "is not one of the four"));
    };

    let sweep = match parts.sweep_to_hz {
        Some(to_hz) => Some(Sweep {
            to_hz: checked_frequency("sweepTo", to_hz, limits.max_frequency_hz)?,
            over_secs: checked_secs("sweepOver", parts.sweep_over_secs)?,
        }),
        None => None,
    };

    if let Some(duration) = parts.duration_secs
        && (!duration.is_finite() || duration <= 0.0)
    {
        return Err(ToneError::new("duration", "must be greater than zero"));
    }

    // A time already past is not an error: it sounds at once. A program that
    // dropped a frame gets a late note rather than a silent gap, which is the
    // difference between a schedule that stumbles and one that loses a beat.
    if let Some(starts_at) = parts.starts_at_secs {
        if !starts_at.is_finite() {
            return Err(ToneError::new("startAt", "must be a time"));
        }
        if starts_at - limits.now_secs > MAX_SCHEDULE_AHEAD_SECS {
            return Err(ToneError::new(
                "startAt",
                format!("must be within {MAX_SCHEDULE_AHEAD_SECS} seconds of now"),
            ));
        }
    }

    Ok(Tone {
        waveform,
        frequency_hz: checked_frequency("frequency", parts.frequency_hz, limits.max_frequency_hz)?,
        amplitude: checked_level("amplitude", parts.amplitude)?,
        envelope: Envelope {
            attack_secs: checked_secs("attack", parts.attack_secs)?,
            decay_secs: checked_secs("decay", parts.decay_secs)?,
            sustain_level: checked_level("sustainLevel", parts.sustain_level)?,
            release_secs: checked_secs("release", parts.release_secs)?,
            duration_secs: parts.duration_secs,
        },
        sweep,
        vibrato: checked_wobble("vibrato", parts.vibrato, MAX_VIBRATO_SEMITONES, "semitones")?,
        tremolo: checked_wobble("tremolo", parts.tremolo, 1.0, "")?,
        starts_at_secs: parts.starts_at_secs,
    })
}

/// One sounding voice: a waveform, where it currently is in its cycle, and
/// how far through its envelope it has travelled.
struct Voice {
    id: VoiceId,
    /// The process whose `playTone` started this voice. A VM going away
    /// releases the sustaining voices it owns, and this is how the mixer
    /// knows which those are.
    owner: ProcessId,
    tone: Tone,
    /// The mixer sample this voice begins on. Until the mixer reaches it the
    /// voice is silent and does not age, so its envelope and its phase both
    /// start at the instant it was scheduled for rather than at the instant
    /// it was queued.
    starts_at_sample: u64,
    /// Where the pitch is going, if it is going anywhere. Built from the
    /// tone's `sweep` when the voice starts, and replaced outright by a bend.
    /// `None` holds the tone's own frequency.
    frequency_ramp: Option<Ramp>,
    /// Where the level is going. `None` holds the tone's own amplitude.
    amplitude_ramp: Option<Ramp>,
    /// `0.0..1.0`, wraps every cycle.
    phase: f32,
    lfsr: u16,
    elapsed_secs: f32,
    /// The `elapsed_secs` at which the release began, set either by
    /// `duration_secs` running out or by a `Stop` arriving.
    releasing_since: Option<f32>,
}

impl Voice {
    fn new(
        id: VoiceId,
        owner: ProcessId,
        tone: Tone,
        starts_at_sample: u64,
        sample_rate_hz: f32,
    ) -> Self {
        // A swept voice is born already travelling, anchored to its own
        // start rather than to the moment it was queued.
        let frequency_ramp = tone.sweep.map(|sweep| Ramp {
            from: tone.frequency_hz,
            to: sweep.to_hz,
            starts_at_sample,
            over_samples: (sweep.over_secs * sample_rate_hz) as u64,
            curve: Curve::Geometric,
        });
        Self {
            id,
            owner,
            tone,
            starts_at_sample,
            frequency_ramp,
            amplitude_ramp: None,
            phase: 0.0,
            lfsr: LFSR_SEED,
            elapsed_secs: 0.0,
            releasing_since: None,
        }
    }

    /// Whether this voice is still waiting for the sample it was scheduled
    /// to begin on.
    fn is_pending(&self, sample: u64) -> bool {
        sample < self.starts_at_sample
    }

    /// How loud this voice actually is right now: its envelope scaled by its
    /// own amplitude. What [`Mixer::add`] compares when it has to steal a
    /// slot, since neither number alone says which voice is faintest.
    fn audible_level(&self, sample: u64) -> f32 {
        self.amplitude_at(sample) * self.amplitude_envelope()
    }

    /// This voice's own level at `sample`, before the envelope shapes it.
    fn amplitude_at(&self, sample: u64) -> f32 {
        let level = self
            .amplitude_ramp
            .map_or(self.tone.amplitude, |ramp| ramp.value_at(sample));
        match self.tone.tremolo {
            Some(tremolo) => {
                (level * (1.0 + tremolo.depth * tremolo.at(self.elapsed_secs))).clamp(0.0, 1.0)
            }
            None => level,
        }
    }

    /// This voice's envelope value right now, in `0.0..=1.0`.
    fn amplitude_envelope(&self) -> f32 {
        if let Some(since) = self.releasing_since {
            let release = self.tone.envelope.release_secs;
            if release <= 0.0 {
                return 0.0;
            }
            let through = (self.elapsed_secs - since) / release;
            return self.level_at(since) * (1.0 - through).clamp(0.0, 1.0);
        }

        self.level_at(self.elapsed_secs)
    }

    /// The envelope's value `elapsed` seconds in, before any release: rising
    /// through the attack, falling through the decay, then holding at the
    /// sustain level.
    ///
    /// This is also the level a release starts from, which is why it takes
    /// the instant rather than reading `elapsed_secs`. A voice cut short
    /// part-way up its attack or part-way down its decay rings out from
    /// where it actually was, instead of jumping to full first.
    fn level_at(&self, elapsed: f32) -> f32 {
        let envelope = &self.tone.envelope;

        if envelope.attack_secs > 0.0 && elapsed < envelope.attack_secs {
            return elapsed / envelope.attack_secs;
        }

        let after_attack = elapsed - envelope.attack_secs;
        if envelope.decay_secs > 0.0 && after_attack < envelope.decay_secs {
            let through = after_attack / envelope.decay_secs;
            return 1.0 + (envelope.sustain_level - 1.0) * through;
        }

        envelope.sustain_level
    }

    /// This voice's pitch at `sample`: whatever its ramp says, or its own
    /// frequency when nothing is moving it.
    fn frequency_at(&self, sample: u64) -> f32 {
        let hz = self
            .frequency_ramp
            .map_or(self.tone.frequency_hz, |ramp| ramp.value_at(sample));
        match self.tone.vibrato {
            // Semitones are multiplicative, so a depth sounds the same
            // whether it wobbles a low note or a high one.
            Some(vibrato) => hz * 2.0f32.powf(vibrato.depth * vibrato.at(self.elapsed_secs) / 12.0),
            None => hz,
        }
    }

    /// Advances the waveform and the envelope by one sample, entering the
    /// release once `duration_secs` has run out.
    fn advance(&mut self, dt_secs: f32, sample: u64) {
        self.phase += self.frequency_at(sample) * dt_secs;
        if self.phase >= 1.0 {
            // Clocked per cycle rather than per sample, so frequency
            // pitches the noise — see `advance_lfsr`. One step per whole
            // cycle crossed, since a high enough voice can cross several
            // inside one sample and the register has to keep up with the
            // pitch it is being asked for.
            let cycles = self.phase.floor();
            for _ in 0..cycles as u32 {
                self.lfsr = advance_lfsr(self.lfsr);
            }
            self.phase -= cycles;
        }

        self.elapsed_secs += dt_secs;

        if self.releasing_since.is_none()
            && let Some(duration) = self.tone.envelope.duration_secs
            && self.elapsed_secs >= duration
        {
            self.releasing_since = Some(self.elapsed_secs);
        }
    }

    /// Sends one of this voice's settings toward `to` over `over_samples`,
    /// starting now — or, on a voice still waiting for its moment, when the
    /// voice itself begins. A ramp that ran while the voice was silent would
    /// be over before anyone heard it.
    ///
    /// It starts from wherever the setting actually is, so nothing jumps: a
    /// pitch caught mid-sweep bends on from there rather than snapping back
    /// to the note's own frequency first.
    fn ramp_to(&mut self, target: RampTarget, to: f32, over_samples: u64, sample: u64) {
        let starts_at_sample = sample.max(self.starts_at_sample);
        let ramp = |from| Ramp {
            from,
            to,
            starts_at_sample,
            over_samples,
            curve: target.curve(),
        };
        match target {
            RampTarget::Frequency => {
                self.frequency_ramp = Some(ramp(self.frequency_at(starts_at_sample)));
            }
            RampTarget::Amplitude => {
                self.amplitude_ramp = Some(ramp(self.amplitude_at(starts_at_sample)));
            }
        }
    }

    /// Begins the release. Cutting a voice off outright would reintroduce
    /// exactly the click the envelope exists to remove, so a stop rings out
    /// over `release_secs` instead. Already-releasing voices keep the
    /// release they started.
    fn release(&mut self) {
        if self.releasing_since.is_none() {
            self.releasing_since = Some(self.elapsed_secs);
        }
    }

    fn finished(&self) -> bool {
        self.releasing_since
            .is_some_and(|since| self.elapsed_secs - since >= self.tone.envelope.release_secs)
    }
}

/// What the main thread asks the sound thread to do.
enum Command {
    Play {
        id: VoiceId,
        owner: ProcessId,
        tone: Tone,
    },
    Stop(VoiceId),
    /// Sends one setting of a sounding voice toward a new value.
    Ramp {
        id: VoiceId,
        target: RampTarget,
        to: f32,
        over_secs: f32,
    },
    /// Releases every sustaining voice `owner` started, sent when its VM
    /// goes away. Timed voices are left alone: a sound effect has to outlive
    /// the code that triggered it.
    ReleaseSustaining(ProcessId),
}

/// The voices currently sounding, and the mix of them. Lives entirely on the
/// sound thread.
struct Mixer {
    voices: Vec<Voice>,
    /// Samples rendered since the device started, which is the clock every
    /// scheduled voice is placed against and the one `ely:sound`'s
    /// `currentTime` reports. Counting what the device has actually consumed
    /// means a scheduled instant converts to a sample index by exact
    /// arithmetic, with no estimated mapping between two drifting clocks.
    clock_samples: u64,
}

impl Mixer {
    fn new() -> Self {
        Self {
            // Allocated once, up front, and `add` never grows past the cap,
            // so `push` never reallocates — the sound callback never reaches
            // the allocator.
            voices: Vec::with_capacity(MAX_VOICES),
            clock_samples: 0,
        }
    }

    /// The sample a voice scheduled for `starts_at_secs` begins on. A time
    /// already past lands at or before the clock, which starts the voice at
    /// once.
    fn sample_index_for(&self, starts_at_secs: Option<f64>, sample_rate_hz: f32) -> u64 {
        match starts_at_secs {
            Some(secs) if secs > 0.0 => (secs * f64::from(sample_rate_hz)) as u64,
            _ => self.clock_samples,
        }
    }

    /// Starts `voice` sounding, replacing the faintest voice already
    /// sounding if every slot is taken.
    ///
    /// Something has to give at the cap, and silently dropping the new voice
    /// makes a program's own sound disappear for reasons it can neither see
    /// nor predict. Stealing sacrifices the least audible thing instead: a
    /// voice part-way through its release, or one that has rung out to a low
    /// sustain, is by construction quieter than one that just started, so
    /// the replacement is the one a listener is least likely to notice. Ties
    /// go to the oldest, which is the one closest to being over.
    fn add(&mut self, voice: Voice) {
        if self.voices.len() < MAX_VOICES {
            self.voices.push(voice);
            return;
        }

        let clock = self.clock_samples;
        let quietest = self
            .voices
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                // A voice that has started gives up its slot before one that
                // has not. A scheduled voice is silent until its moment, so
                // by loudness alone it would look like the obvious thing to
                // discard — and a program queueing a sequence would have each
                // new voice eat the ones already waiting.
                a.is_pending(clock).cmp(&b.is_pending(clock)).then_with(|| {
                    if a.is_pending(clock) {
                        // Both waiting: the one furthest out is the least
                        // imminent, and the easiest for a scheduler to
                        // queue again.
                        b.starts_at_sample.cmp(&a.starts_at_sample)
                    } else {
                        a.audible_level(clock)
                            .total_cmp(&b.audible_level(clock))
                            .then(b.elapsed_secs.total_cmp(&a.elapsed_secs))
                    }
                })
            })
            .map(|(index, _)| index);
        if let Some(index) = quietest {
            self.voices[index] = voice;
        }
    }

    /// Releases the voice `id` names. A voice that finished on its own a
    /// callback before its stop arrived is the ordinary race, not an error,
    /// so an unknown id is a no-op.
    fn stop(&mut self, id: VoiceId) {
        let clock = self.clock_samples;
        let Some(index) = self.voices.iter().position(|voice| voice.id == id) else {
            return;
        };
        // A voice that never sounded has nothing to ring out, so it goes
        // rather than spending its release fading from silence to silence.
        if self.voices[index].is_pending(clock) {
            self.voices.remove(index);
        } else {
            self.voices[index].release();
        }
    }

    /// Sends `id`'s setting toward `to`. An id whose voice has already
    /// finished — or which lost its slot to a newer sound — is a no-op, the
    /// same as stopping one.
    fn ramp(&mut self, id: VoiceId, target: RampTarget, to: f32, over_samples: u64) {
        let clock = self.clock_samples;
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == id) {
            voice.ramp_to(target, to, over_samples, clock);
        }
    }

    /// Releases every sustaining voice `owner` started. A timed voice is
    /// left to finish on its own, so a sound effect still outlives the
    /// program that triggered it.
    fn release_sustaining(&mut self, owner: ProcessId) {
        let clock = self.clock_samples;
        let mine =
            |voice: &Voice| voice.owner == owner && voice.tone.envelope.duration_secs.is_none();
        self.voices
            .retain(|voice| !(mine(voice) && voice.is_pending(clock)));
        for voice in &mut self.voices {
            if mine(voice) {
                voice.release();
            }
        }
    }

    /// Writes one buffer's worth of mixed samples into `out` and advances
    /// every voice, dropping the ones whose release has finished.
    ///
    /// `out` arrives holding whatever the device left in it, so each frame
    /// is written rather than accumulated into. The mix is mono: every
    /// channel in a frame gets the same sample. A buffer whose length isn't
    /// a whole number of frames ends in a short chunk, which `fill` covers.
    fn render(&mut self, out: &mut [f32], channels: usize, sample_rate_hz: f32) {
        let dt = 1.0 / sample_rate_hz;

        let mut sample = self.clock_samples;
        for frame in out.chunks_mut(channels) {
            let mut mixed = 0.0;
            for voice in &mut self.voices {
                // Checked per sample rather than per buffer, so a voice
                // scheduled for the middle of this buffer begins in the
                // middle of it.
                if voice.is_pending(sample) {
                    continue;
                }
                mixed += voice.tone.waveform.sample(voice.phase, voice.lfsr)
                    * voice.amplitude_at(sample)
                    * voice.amplitude_envelope();
                voice.advance(dt, sample);
            }
            frame.fill((mixed * MASTER_GAIN).clamp(-1.0, 1.0));
            sample += 1;
        }
        self.clock_samples = sample;

        self.voices.retain(|voice| !voice.finished());
    }

    #[cfg(test)]
    fn active(&self) -> usize {
        self.voices.len()
    }
}

/// The kernel's handle on the sound device: allocates voice ids, sends the
/// sound thread its commands, and holds the output stream open. Dropping it
/// stops playback.
pub struct Sound {
    commands: mpsc::Sender<Command>,
    /// The highest frequency this device can sound without aliasing: the
    /// lower of [`MAX_FREQUENCY_HZ`] and half the rate the output stream
    /// actually negotiated.
    max_frequency_hz: f32,
    sample_rate_hz: f32,
    /// Samples the device has played, as of the last callback. Read to place
    /// scheduled voices; see [`Sound::current_time`].
    clock: Arc<AtomicU64>,
    next_id: Cell<VoiceId>,
    _stream: Option<cpal::Stream>,
}

impl Sound {
    fn allocate_id(&self) -> VoiceId {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(1));
        id
    }

    /// Starts `tone` sounding on `owner`'s behalf and returns the id that
    /// stops it, or `None` if the sound thread is gone.
    ///
    /// There is no cap to fail against here. The mixer holds the only
    /// truthful count of what is sounding, and it steals a slot rather than
    /// turning a voice away, so a play that reaches the thread always
    /// sounds.
    pub fn play(&self, owner: ProcessId, tone: Tone) -> Option<VoiceId> {
        let id = self.allocate_id();
        match self.commands.send(Command::Play { id, owner, tone }) {
            Ok(()) => Some(id),
            Err(_) => {
                eprintln!("[sound] play failed: the sound thread is gone");
                None
            }
        }
    }

    /// Sends one setting of the voice `id` names toward `to` across
    /// `over_secs`, rather than stepping it. A step in loudness is exactly
    /// the discontinuity an envelope's attack exists to remove, and a step in
    /// pitch is heard as a different note rather than the same one moving.
    pub fn ramp(&self, id: VoiceId, target: RampTarget, to: f32, over_secs: f32) {
        let _ = self.commands.send(Command::Ramp {
            id,
            target,
            to,
            over_secs,
        });
    }

    /// Releases the voice `id` names, ringing it out over its release rather
    /// than cutting it. Ignores an id that has already finished, and a
    /// stream that has already stopped — either way there's nothing left to
    /// silence.
    pub fn stop(&self, id: VoiceId) {
        let _ = self.commands.send(Command::Stop(id));
    }

    /// The highest frequency this device will accept.
    pub fn max_frequency_hz(&self) -> f32 {
        self.max_frequency_hz
    }

    /// Seconds of sound the device has actually played since it started —
    /// the clock a scheduled tone's `starts_at_secs` is measured against.
    ///
    /// The reading trails the device by up to one buffer, since it is stored
    /// once per callback. That costs a scheduler nothing but lookahead: a
    /// tone names an absolute instant the mixer then honours exactly, so a
    /// stale reading shifts a whole sequence by the same small amount
    /// instead of scattering its notes. What it must not do is stop
    /// advancing, which is why a machine with no device still gets a clock.
    pub fn current_time(&self) -> f64 {
        self.clock.load(Ordering::Relaxed) as f64 / f64::from(self.sample_rate_hz)
    }

    /// Releases every sustaining voice `owner` started, ringing each out
    /// over its own release. Called when a VM goes away, so a program that
    /// faulted or exited holding a note doesn't leave it droning for the
    /// life of the kernel.
    pub fn release_sustaining(&self, owner: ProcessId) {
        let _ = self.commands.send(Command::ReleaseSustaining(owner));
    }
}

/// Reads a `[depth, rate]` pair off the options object, or `None` when the
/// program didn't ask for one.
fn wobble_pair(
    ctx: &Ctx<'_>,
    options: &rquickjs::Object<'_>,
    name: &str,
) -> Result<Option<[f32; 2]>> {
    let Some(pair) = options.get::<_, Option<Vec<f32>>>(name)? else {
        return Ok(None);
    };
    <[f32; 2]>::try_from(pair.as_slice())
        .map(Some)
        .map_err(|_| {
            rquickjs::Exception::throw_type(ctx, &format!("{name} needs a depth and a rate"))
        })
}

/// Turns a refused option into the exception `ely:sound` re-types.
///
/// Tagged `option: requirement`, which the module splits back apart into a
/// `ToneOptionError`. A bad waveform names no point on any scale, so it is a
/// type error and stays untagged: the module re-types only what it can report
/// as an out-of-range option.
fn throw_tone_error(ctx: &Ctx<'_>, err: &ToneError) -> rquickjs::Error {
    if err.option == "waveform" {
        return rquickjs::Exception::throw_type(
            ctx,
            &format!("{} {}", err.option, err.requirement),
        );
    }
    rquickjs::Exception::throw_range(ctx, &format!("{}: {}", err.option, err.requirement))
}

/// Seconds of sound played so far, falling back to elapsed wall time on a
/// machine with no device. A scheduler that read a clock frozen at zero
/// would queue everything into the same instant, so the clock keeps running
/// whether or not anyone can hear it.
fn current_time(sound: Option<&Sound>) -> f64 {
    match sound {
        Some(sound) => sound.current_time(),
        None => SILENT_CLOCK
            .get_or_init(Instant::now)
            .elapsed()
            .as_secs_f64(),
    }
}

/// Binds the hidden globals `ely:sound`'s embedded module wraps. A program
/// never names one of these: it calls the module's exported `playTone` and
/// `stopVoice`, which validate their arguments and call the matching global.
///
/// `sound` is `None` on a machine whose output device couldn't be opened, in
/// which case every binding here is a silent no-op — `playTone` reports that
/// nothing sounded and a program carries on, rather than the absence of a
/// sound card becoming an error every program has to handle.
pub fn bootstrap_sound_bindings(
    ctx: &Ctx<'_>,
    sound: Option<Rc<Sound>>,
    owner: ProcessId,
) -> Result<()> {
    {
        let sound = sound.clone();
        bind(
            ctx,
            "__sound_play",
            move |ctx: Ctx<'_>,
                  waveform: u8,
                  frequency_hz: f32,
                  options: rquickjs::Object<'_>|
                  -> Result<Option<VoiceId>> {
                // Everything but the waveform and the pitch crosses as one
                // object, read by name. The alternative is a positional
                // argument per option, which this binding has already
                // outgrown twice — and named fields cost a handful of
                // property lookups on a call made a few times a frame.
                let parts = ToneParts {
                    waveform,
                    frequency_hz,
                    amplitude: options.get("amplitude")?,
                    attack_secs: options.get("attack")?,
                    decay_secs: options.get("decay")?,
                    sustain_level: options.get("sustainLevel")?,
                    release_secs: options.get("release")?,
                    sweep_to_hz: options.get("sweepTo")?,
                    sweep_over_secs: options.get("sweepOver")?,
                    duration_secs: options.get("duration")?,
                    starts_at_secs: options.get("startAt")?,
                    // A depth and a rate are one thing, so they cross as a
                    // pair the way the envelope's stages do.
                    vibrato: wobble_pair(&ctx, &options, "vibrato")?,
                    tremolo: wobble_pair(&ctx, &options, "tremolo")?,
                };

                // A machine with no device still range-checks every option,
                // so a program gets the same errors whether or not anything
                // can sound.
                let limits = ToneLimits {
                    max_frequency_hz: sound
                        .as_ref()
                        .map_or(MAX_FREQUENCY_HZ, |sound| sound.max_frequency_hz()),
                    now_secs: current_time(sound.as_deref()),
                };

                let tone =
                    tone_from_parts(parts, limits).map_err(|err| throw_tone_error(&ctx, &err))?;

                let Some(sound) = &sound else {
                    return Ok(None);
                };
                Ok(sound.play(owner, tone))
            },
        )?;
    }

    {
        let sound = sound.clone();
        bind(ctx, "__sound_current_time", move || {
            current_time(sound.as_deref())
        })?;
    }

    {
        let sound = sound.clone();
        bind(
            ctx,
            "__sound_bend",
            move |ctx: Ctx<'_>, id: VoiceId, frequency_hz: f32, over_secs: f32| -> Result<()> {
                let max_frequency_hz = sound
                    .as_ref()
                    .map_or(MAX_FREQUENCY_HZ, |sound| sound.max_frequency_hz());
                let (frequency_hz, over_secs) =
                    checked_bend(frequency_hz, over_secs, max_frequency_hz)
                        .map_err(|err| throw_tone_error(&ctx, &err))?;
                if let Some(sound) = &sound {
                    sound.ramp(id, RampTarget::Frequency, frequency_hz, over_secs);
                }
                Ok(())
            },
        )?;
    }

    {
        let sound = sound.clone();
        bind(
            ctx,
            "__sound_fade",
            move |ctx: Ctx<'_>, id: VoiceId, level: f32, over_secs: f32| -> Result<()> {
                let (level, over_secs) =
                    checked_fade(level, over_secs).map_err(|err| throw_tone_error(&ctx, &err))?;
                if let Some(sound) = &sound {
                    sound.ramp(id, RampTarget::Amplitude, level, over_secs);
                }
                Ok(())
            },
        )?;
    }

    bind(ctx, "__sound_stop", move |id: VoiceId| {
        if let Some(sound) = &sound {
            sound.stop(id);
        }
    })?;

    Ok(())
}

/// Opens the default output device and starts its stream, or returns `None`
/// and logs why if no device is available, no `f32`-capable output config can
/// be found, or the stream can't be built or started. Never panics — see the
/// module doc comment.
pub fn start() -> Option<Sound> {
    let Some(device) = cpal::default_host().default_output_device() else {
        eprintln!("[sound] no output device found, continuing without sound");
        return None;
    };

    // f32 output only — the common modern default — rather than the full
    // per-SampleFormat dispatch a general engine would need. Searched rather
    // than taken from `default_output_config()` alone: on some real ALSA
    // setups the default isn't f32, and giving up there would silently fail
    // on a plausible real machine, not just an sound-less sandbox.
    let config = device
        .supported_output_configs()
        .ok()
        .and_then(|mut configs| configs.find(|c| c.sample_format() == cpal::SampleFormat::F32))
        .map(|c| {
            // `with_sample_rate` panics outside the range this config
            // reports, so the preference is clamped into it rather than
            // passed through.
            let rate = PREFERRED_SAMPLE_RATE.clamp(c.min_sample_rate().0, c.max_sample_rate().0);
            c.with_sample_rate(cpal::SampleRate(rate))
        });

    let Some(config) = config else {
        eprintln!("[sound] no f32-capable output config found, continuing without sound");
        return None;
    };

    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;

    let (commands, incoming) = mpsc::channel();
    let clock = Arc::new(AtomicU64::new(0));
    let callback_clock = Arc::clone(&clock);
    let mut mixer = Mixer::new();

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // A disconnected channel means `Sound` was dropped; the mixer
            // rings its remaining voices out and then stays silent.
            while let Ok(command) = incoming.try_recv() {
                match command {
                    Command::Play { id, owner, tone } => {
                        let starts_at = mixer.sample_index_for(tone.starts_at_secs, sample_rate);
                        mixer.add(Voice::new(id, owner, tone, starts_at, sample_rate));
                    }
                    Command::Stop(id) => mixer.stop(id),
                    Command::Ramp {
                        id,
                        target,
                        to,
                        over_secs,
                    } => mixer.ramp(id, target, to, (over_secs * sample_rate) as u64),
                    Command::ReleaseSustaining(owner) => mixer.release_sustaining(owner),
                }
            }
            mixer.render(data, channels, sample_rate);
            callback_clock.store(mixer.clock_samples, Ordering::Relaxed);
        },
        |err| eprintln!("[sound] stream error: {err}"),
        None,
    );
    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("[sound] failed to build output stream: {err}");
            return None;
        }
    };

    if let Err(err) = stream.play() {
        eprintln!("[sound] failed to start output stream: {err}");
        return None;
    }

    eprintln!("[sound] output ready ({sample_rate} Hz, {channels} channels)");
    Some(Sound {
        commands,
        max_frequency_hz: MAX_FREQUENCY_HZ.min(sample_rate / 2.0),
        sample_rate_hz: sample_rate,
        clock,
        next_id: Cell::new(1),
        _stream: Some(stream),
    })
}

/// A `Sound` with no output device, and the record of everything asked of
/// it. Lets a test assert on the exact tones a program played without a
/// sound card anywhere in the picture — the same reason [`Mixer::render`]
/// takes a plain buffer.
#[cfg(test)]
pub(crate) struct SoundLog {
    incoming: mpsc::Receiver<Command>,
    /// The same clock the detached `Sound` reads, so a test can move time
    /// forward without an output callback to move it.
    clock: Arc<AtomicU64>,
    /// Everything drained so far. The channel can only be read once, so what
    /// comes out of it is kept here and every accessor filters this instead
    /// — otherwise asking what played would throw away what stopped.
    seen: RefCell<Vec<Command>>,
}

#[cfg(test)]
impl SoundLog {
    /// Moves everything pending out of the channel and into `seen`.
    fn drain(&self) {
        let mut seen = self.seen.borrow_mut();
        while let Ok(command) = self.incoming.try_recv() {
            seen.push(command);
        }
    }

    /// The tones played so far, in order.
    pub(crate) fn played(&self) -> Vec<Tone> {
        self.drain();
        self.seen
            .borrow()
            .iter()
            .filter_map(|command| match command {
                Command::Play { tone, .. } => Some(*tone),
                _ => None,
            })
            .collect()
    }

    /// The ramps asked for so far, as `(id, target, to, over_secs)`.
    pub(crate) fn ramped(&self) -> Vec<(VoiceId, RampTarget, f32, f32)> {
        self.drain();
        self.seen
            .borrow()
            .iter()
            .filter_map(|command| match command {
                Command::Ramp {
                    id,
                    target,
                    to,
                    over_secs,
                } => Some((*id, *target, *to, *over_secs)),
                _ => None,
            })
            .collect()
    }

    /// The voices explicitly stopped so far, in order.
    pub(crate) fn stopped(&self) -> Vec<VoiceId> {
        self.drain();
        self.seen
            .borrow()
            .iter()
            .filter_map(|command| match command {
                Command::Stop(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// Moves the clock on as if the device had played `seconds` of sound.
    /// Nothing advances it without a real output callback, so without this a
    /// test can't reach anything that depends on time passing.
    pub(crate) fn play_out(&self, seconds: f64) {
        let samples = (seconds * f64::from(PREFERRED_SAMPLE_RATE)) as u64;
        self.clock.fetch_add(samples, Ordering::Relaxed);
    }

    /// The processes whose sustaining voices were released, in order — one
    /// entry per VM teardown that reached the sound thread.
    pub(crate) fn released(&self) -> Vec<ProcessId> {
        self.drain();
        self.seen
            .borrow()
            .iter()
            .filter_map(|command| match command {
                Command::ReleaseSustaining(owner) => Some(*owner),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
impl Sound {
    /// A `Sound` wired to nothing, paired with the log of what it was asked
    /// to do. The log holds the receiving end of the command channel, so it
    /// has to outlive the `Sound`: drop it first and every later `play`
    /// reports the sound thread as gone.
    pub(crate) fn detached() -> (Sound, SoundLog) {
        let (commands, incoming) = mpsc::channel();
        let clock = Arc::new(AtomicU64::new(0));
        let sound = Sound {
            commands,
            max_frequency_hz: MAX_FREQUENCY_HZ,
            sample_rate_hz: PREFERRED_SAMPLE_RATE as f32,
            clock: Arc::clone(&clock),
            next_id: Cell::new(1),
            _stream: None,
        };
        (
            sound,
            SoundLog {
                incoming,
                clock,
                seen: RefCell::new(Vec::new()),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Envelope, LFSR_SEED, MASTER_GAIN, MAX_FREQUENCY_HZ, MAX_VOICES, Mixer, Sound, Sweep, Tone,
        Voice, VoiceId, Waveform, advance_lfsr,
    };

    /// Every test here is about the mix rather than about ownership, so
    /// they all play as the same process.
    const OWNER: crate::process::ProcessId = 1;

    /// The rate every test here renders at: a round 1000 Hz, so a period
    /// lands on a whole number of samples.
    const RATE: f32 = 1000.0;

    /// A voice starting at once, owned by `OWNER`. Keeps every test that
    /// doesn't care about ownership or scheduling out of the way of those
    /// that do.
    fn voice(id: VoiceId, tone: Tone) -> Voice {
        Voice::new(id, OWNER, tone, 0, RATE)
    }

    /// A tone that holds until stopped, with no attack or release ramp, so a
    /// test sees the waveform itself rather than an envelope shaping it.
    fn tone(waveform: Waveform, frequency_hz: f32) -> Tone {
        Tone {
            waveform,
            frequency_hz,
            amplitude: 1.0,
            envelope: Envelope {
                attack_secs: 0.0,
                decay_secs: 0.0,
                sustain_level: 1.0,
                release_secs: 0.0,
                duration_secs: None,
            },
            sweep: None,
            vibrato: None,
            tremolo: None,
            starts_at_secs: None,
        }
    }

    /// Renders `count` mono samples of one voice. The sample rate is a round
    /// 1000 Hz so that a period lands on a whole number of samples.
    fn render_one(tone: Tone, count: usize) -> Vec<f32> {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone));
        let mut out = vec![0.0; count];
        mixer.render(&mut out, 1, 1000.0);
        out
    }

    /// Undoes [`MASTER_GAIN`] so a test can assert against the waveform's
    /// own rails rather than the attenuated mix.
    fn unscaled(samples: Vec<f32>) -> Vec<f32> {
        samples.into_iter().map(|s| s / MASTER_GAIN).collect()
    }

    #[test]
    fn a_square_wave_only_ever_outputs_the_two_rail_values() {
        for sample in unscaled(render_one(tone(Waveform::Square, 100.0), 1000)) {
            assert!(
                (sample.abs() - 1.0).abs() < 1e-5,
                "{sample} is not a rail value"
            );
        }
    }

    #[test]
    fn a_square_wave_completes_one_period_in_sample_rate_over_frequency_samples() {
        // 100 Hz at 1000 Hz: one period is 10 samples, so the sign flips
        // exactly at sample index 5, not before.
        let samples = unscaled(render_one(tone(Waveform::Square, 100.0), 10));
        assert!(
            samples[..5].iter().all(|&s| s > 0.0),
            "the first half-period should stay high"
        );
        assert!(
            samples[5..].iter().all(|&s| s < 0.0),
            "the second half-period should flip low"
        );
    }

    #[test]
    fn a_square_wave_repeats_identically_across_periods() {
        let period = 10;
        let samples = render_one(tone(Waveform::Square, 100.0), period * 3);
        assert_eq!(samples[..period], samples[period..period * 2]);
        assert_eq!(samples[..period], samples[period * 2..period * 3]);
    }

    #[test]
    fn a_square_wave_is_silent_at_zero_frequency() {
        // The phase never advances, so every sample holds the value the
        // waveform starts at.
        let samples = unscaled(render_one(tone(Waveform::Square, 0.0), 100));
        for sample in samples {
            assert!((sample - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn a_triangle_wave_peaks_at_the_start_of_its_period_and_troughs_at_the_middle() {
        let samples = unscaled(render_one(tone(Waveform::Triangle, 100.0), 10));
        assert!((samples[0] - 1.0).abs() < 1e-5, "starts at its peak");
        assert!(
            (samples[5] + 1.0).abs() < 1e-5,
            "troughs at the half-period"
        );
    }

    #[test]
    fn a_triangle_wave_moves_between_its_rails_in_equal_steps() {
        // Constant successive differences are what make it a triangle rather
        // than a saw or a curve. The sign flips at the turn, so compare
        // magnitudes.
        let samples = unscaled(render_one(tone(Waveform::Triangle, 100.0), 10));
        let steps: Vec<f32> = samples.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        let first = steps[0];
        for step in steps {
            assert!((step - first).abs() < 1e-5, "{step} differs from {first}");
        }
    }

    #[test]
    fn a_sine_wave_stays_within_its_rails_and_crosses_zero_twice_per_period() {
        let samples = unscaled(render_one(tone(Waveform::Sine, 100.0), 10));
        for &sample in &samples {
            assert!(sample.abs() <= 1.0 + 1e-5, "{sample} left the rails");
        }
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
            .count();
        assert_eq!(crossings, 2, "a full period crosses zero twice");
    }

    #[test]
    fn noise_holds_one_value_for_a_whole_phase_period() {
        // The LFSR is clocked once per cycle, not once per sample, which is
        // what pitches the noise: at 100 Hz and 1000 Hz there are ten
        // samples per cycle, so a value can only change every ten.
        let samples = render_one(tone(Waveform::Noise, 100.0), 30);
        for chunk in samples.chunks(10) {
            let first = chunk[0];
            assert!(
                chunk.iter().all(|&s| s == first),
                "noise changed inside one cycle"
            );
        }
    }

    #[test]
    fn noise_repeats_only_after_the_full_lfsr_cycle() {
        let seed = super::LFSR_SEED;
        let mut lfsr = seed;
        for step in 1..32767 {
            lfsr = advance_lfsr(lfsr);
            assert_ne!(lfsr, seed, "the register returned to its seed at {step}");
        }
        assert_eq!(
            advance_lfsr(lfsr),
            seed,
            "the register should return to its seed after a full cycle"
        );
    }

    #[test]
    fn noise_from_the_same_seed_is_identical_every_time() {
        let first = render_one(tone(Waveform::Noise, 100.0), 200);
        let second = render_one(tone(Waveform::Noise, 100.0), 200);
        assert_eq!(first, second);
    }

    #[test]
    fn a_voice_rises_from_silence_across_its_attack() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.attack_secs = 0.01; // ten samples at 1000 Hz
        let samples = unscaled(render_one(shaped, 10));
        assert!(samples[0].abs() < 1e-5, "starts silent");
        for window in samples.windows(2) {
            assert!(window[1] > window[0], "the attack should rise throughout");
        }
    }

    #[test]
    fn a_voice_with_no_decay_holds_full_amplitude_until_its_duration() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.attack_secs = 0.005;
        shaped.envelope.duration_secs = Some(0.02);
        let samples = unscaled(render_one(shaped, 20));
        for &sample in &samples[6..19] {
            assert!(
                (sample - 1.0).abs() < 1e-5,
                "{sample} is not full amplitude"
            );
        }
    }

    #[test]
    fn a_voice_fades_to_silence_across_its_release() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.duration_secs = Some(0.005);
        shaped.envelope.release_secs = 0.01;
        let samples = unscaled(render_one(shaped, 20));
        // Falling from the moment the release begins, reaching silence by
        // the time it ends.
        for window in samples[6..15].windows(2) {
            assert!(window[1] < window[0], "the release should fall throughout");
        }
        assert!(samples[15].abs() < 1e-5, "silent once released");
    }

    #[test]
    fn a_voice_with_no_duration_sounds_until_it_is_stopped() {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone(Waveform::Square, 0.0)));
        let mut out = vec![0.0; 1000];
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.active(), 1, "still sounding a whole second later");
        assert!(out.iter().all(|&s| s != 0.0));
    }

    #[test]
    fn stopping_a_voice_releases_it_rather_than_cutting_it_off() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.release_secs = 0.01;
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));

        let mut before = vec![0.0; 5];
        mixer.render(&mut before, 1, 1000.0);
        mixer.stop(1);
        let mut after = vec![0.0; 5];
        mixer.render(&mut after, 1, 1000.0);

        // No step: the first sample after the stop is close to the last one
        // before it, which is the click the envelope exists to prevent.
        assert!(
            (after[0] - before[4]).abs() < 0.05,
            "stopping stepped from {} to {}",
            before[4],
            after[0]
        );
        assert!(after[4].abs() < before[4].abs(), "and it decays");
    }

    #[test]
    fn a_voice_is_removed_once_its_release_has_finished() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.duration_secs = Some(0.005);
        shaped.envelope.release_secs = 0.005;
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));

        let mut out = vec![0.0; 20];
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.active(), 0, "a finished voice is dropped");

        let mut silence = vec![1.0; 10];
        mixer.render(&mut silence, 1, 1000.0);
        assert!(silence.iter().all(|&s| s == 0.0), "and leaves silence");
    }

    #[test]
    fn stopping_a_voice_that_has_already_finished_does_nothing() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.duration_secs = Some(0.001);
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));
        let mut out = vec![0.0; 10];
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.active(), 0);

        mixer.stop(1);
        assert_eq!(mixer.active(), 0);
    }

    #[test]
    fn stopping_an_id_that_was_never_played_does_nothing() {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone(Waveform::Square, 100.0)));
        mixer.stop(9999);
        assert_eq!(mixer.active(), 1, "the sounding voice is untouched");
    }

    /// A mixer holding `MAX_VOICES` identical voices, ready for a steal.
    fn full_mixer() -> Mixer {
        let mut mixer = Mixer::new();
        for id in 0..MAX_VOICES as u32 {
            mixer.add(voice(id, tone(Waveform::Square, 100.0)));
        }
        mixer
    }

    #[test]
    fn stealing_keeps_the_mixer_at_its_cap() {
        let mut mixer = full_mixer();
        mixer.add(voice(9999, tone(Waveform::Square, 100.0)));
        assert_eq!(mixer.active(), MAX_VOICES, "the cap still holds");
        assert!(
            mixer.voices.iter().any(|voice| voice.id == 9999),
            "and the new voice is the one sounding"
        );
    }

    #[test]
    fn the_quietest_voice_is_the_one_stolen() {
        let mut mixer = Mixer::new();
        // One voice is audibly quieter than the rest, so it is the one with
        // the least to lose.
        mixer.add(voice(
            1,
            Tone {
                amplitude: 0.05,
                ..tone(Waveform::Square, 100.0)
            },
        ));
        for id in 2..=MAX_VOICES as u32 {
            mixer.add(voice(id, tone(Waveform::Square, 100.0)));
        }

        mixer.add(voice(9999, tone(Waveform::Square, 100.0)));

        assert!(
            !mixer.voices.iter().any(|voice| voice.id == 1),
            "the faintest voice gave up its slot"
        );
        assert_eq!(
            mixer.voices.iter().filter(|voice| voice.id != 9999).count(),
            MAX_VOICES - 1,
            "and nothing else was disturbed"
        );
    }

    #[test]
    fn a_releasing_voice_is_stolen_before_a_sounding_one() {
        let mut shaped = tone(Waveform::Square, 100.0);
        shaped.envelope.release_secs = 1.0;
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));
        for id in 2..=MAX_VOICES as u32 {
            mixer.add(voice(id, tone(Waveform::Square, 100.0)));
        }

        // Part-way through a long release, so it is still sounding but on
        // its way out — the one a listener misses least.
        mixer.stop(1);
        let mut out = vec![0.0; 500];
        mixer.render(&mut out, 1, 1000.0);
        mixer.add(voice(9999, tone(Waveform::Square, 100.0)));

        assert!(
            !mixer.voices.iter().any(|voice| voice.id == 1),
            "the releasing voice gave up its slot"
        );
    }

    #[test]
    fn a_stolen_voice_leaves_the_others_sounding_as_they_were() {
        let mut untouched = full_mixer();
        let mut stolen_from = full_mixer();
        stolen_from.add(voice(9999, tone(Waveform::Square, 100.0)));

        let mut before = vec![0.0; 50];
        untouched.render(&mut before, 1, 1000.0);
        let mut after = vec![0.0; 50];
        stolen_from.render(&mut after, 1, 1000.0);

        // Every voice here is identical, so replacing one changes nothing a
        // listener could point at.
        assert_eq!(before, after);
    }

    #[test]
    fn a_vm_going_away_releases_only_the_voices_it_was_holding() {
        let mut held = tone(Waveform::Square, 0.0);
        held.envelope.release_secs = 0.01;
        let mut timed = held;
        timed.envelope.duration_secs = Some(10.0);

        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, 7, held, 0, RATE));
        mixer.add(Voice::new(2, 7, timed, 0, RATE));
        mixer.add(Voice::new(3, 8, held, 0, RATE));

        mixer.release_sustaining(7);

        let releasing = |mixer: &Mixer, id: VoiceId| {
            mixer
                .voices
                .iter()
                .find(|voice| voice.id == id)
                .expect("still sounding")
                .releasing_since
                .is_some()
        };
        assert!(releasing(&mixer, 1), "the VM's held note is released");
        assert!(
            !releasing(&mixer, 2),
            "its timed voice sees itself out instead"
        );
        assert!(!releasing(&mixer, 3), "another VM's note is untouched");
    }

    #[test]
    fn rendering_overwrites_whatever_was_in_the_buffer() {
        // The device hands back a buffer holding the last frame's samples,
        // so a mix has to write rather than accumulate.
        let mut mixer = Mixer::new();
        let mut out = vec![1.0; 16];
        mixer.render(&mut out, 1, 1000.0);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_silent_mixer_writes_zeros() {
        let mut mixer = Mixer::new();
        let mut out = vec![0.5; 16];
        mixer.render(&mut out, 2, 1000.0);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn every_channel_in_a_frame_gets_the_same_sample() {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone(Waveform::Sine, 100.0)));
        let mut out = vec![0.0; 20];
        mixer.render(&mut out, 2, 1000.0);
        for frame in out.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn a_partial_final_frame_is_still_filled() {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone(Waveform::Square, 0.0)));
        // Seven samples across two channels leaves a one-sample final frame.
        let mut out = vec![0.0; 7];
        mixer.render(&mut out, 2, 1000.0);
        assert!(
            out.iter().all(|&s| s != 0.0),
            "no sample was left unwritten"
        );
    }

    #[test]
    fn the_mix_never_leaves_the_output_range() {
        let mut mixer = Mixer::new();
        for id in 0..MAX_VOICES as u32 {
            mixer.add(voice(id, tone(Waveform::Square, 100.0)));
        }
        let mut out = vec![0.0; 100];
        mixer.render(&mut out, 1, 1000.0);
        for sample in out {
            assert!((-1.0..=1.0).contains(&sample), "{sample} left the range");
        }
    }

    #[test]
    fn a_voice_falls_from_full_to_its_sustain_level_across_its_decay() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.decay_secs = 0.01; // ten samples at 1000 Hz
        shaped.envelope.sustain_level = 0.4;
        let samples = unscaled(render_one(shaped, 10));

        assert!((samples[0] - 1.0).abs() < 1e-5, "starts at full");
        for window in samples.windows(2) {
            assert!(window[1] < window[0], "the decay should fall throughout");
        }
        assert!(
            (samples[9] - 0.46).abs() < 0.02,
            "and arrives at the sustain level, got {}",
            samples[9]
        );
    }

    #[test]
    fn a_voice_holds_its_sustain_level_once_its_decay_is_done() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.decay_secs = 0.005;
        shaped.envelope.sustain_level = 0.4;
        let samples = unscaled(render_one(shaped, 30));

        for &sample in &samples[6..] {
            assert!(
                (sample - 0.4).abs() < 1e-5,
                "{sample} is not the sustain level"
            );
        }
    }

    #[test]
    fn a_release_from_sustain_starts_at_the_sustain_level_rather_than_full() {
        // A note that has already settled quieter releases from where it
        // actually is, not from full.
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.decay_secs = 0.002;
        shaped.envelope.sustain_level = 0.25;
        shaped.envelope.duration_secs = Some(0.01);
        shaped.envelope.release_secs = 0.01;
        let samples = unscaled(render_one(shaped, 12));

        assert!(
            (samples[10] - 0.25).abs() < 0.03,
            "the release begins at the sustain level, got {}",
            samples[10]
        );
    }

    #[test]
    fn a_voice_stopped_mid_decay_releases_from_where_it_actually_was() {
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.decay_secs = 0.02;
        shaped.envelope.sustain_level = 0.0;
        shaped.envelope.release_secs = 0.01;

        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));
        let mut before = vec![0.0; 10];
        mixer.render(&mut before, 1, 1000.0);
        mixer.stop(1);
        let mut after = vec![0.0; 10];
        mixer.render(&mut after, 1, 1000.0);

        // Half way down the decay, so neither full nor silent — and no step
        // across the stop, which is the click the envelope exists to avoid.
        assert!(
            (after[0] - before[9]).abs() < 0.05,
            "stopping stepped from {} to {}",
            before[9],
            after[0]
        );
    }

    #[test]
    fn a_voice_with_no_sustain_goes_silent_while_it_is_still_sounding() {
        // A bell that has rung out is still a voice: it holds its slot until
        // something releases it, long after there is anything to hear.
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.decay_secs = 0.005;
        shaped.envelope.sustain_level = 0.0;
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));

        let mut out = vec![0.0; 40];
        mixer.render(&mut out, 1, 1000.0);

        assert!(
            out[20..].iter().all(|&s| s.abs() < 1e-5),
            "it should have rung out"
        );
        assert_eq!(mixer.active(), 1, "but it is still a sounding voice");
    }

    /// Counts complete cycles in a rendered square wave, which is a
    /// stand-in for how high it sounded: more zero crossings, higher pitch.
    fn cycles_in(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| w[0] > 0.0 && w[1] < 0.0)
            .count()
    }

    #[test]
    fn a_sweep_slides_the_pitch_toward_its_target() {
        let mut swept = tone(Waveform::Square, 400.0);
        swept.sweep = Some(Sweep {
            to_hz: 100.0,
            over_secs: 0.05,
        });
        let samples = render_one(swept, 100);

        // The first half sweeps down through the high frequencies; by the
        // second half it has arrived and holds.
        let early = cycles_in(&samples[..25]);
        let late = cycles_in(&samples[75..]);
        assert!(
            early > late,
            "the pitch should fall: {early} cycles early, {late} late"
        );
    }

    #[test]
    fn a_sweep_holds_its_target_once_it_arrives() {
        let mut swept = tone(Waveform::Square, 400.0);
        swept.sweep = Some(Sweep {
            to_hz: 100.0,
            over_secs: 0.02,
        });
        let samples = render_one(swept, 120);
        // 100 Hz at 1000 Hz is one cycle every ten samples, so forty
        // samples well past the sweep hold four.
        assert_eq!(cycles_in(&samples[60..100]), 4);
    }

    #[test]
    fn a_tone_with_no_sweep_holds_the_pitch_it_started_on() {
        let steady = tone(Waveform::Square, 100.0);
        let samples = render_one(steady, 100);
        assert_eq!(cycles_in(&samples[..50]), cycles_in(&samples[50..]));
    }

    #[test]
    fn a_sweep_between_two_pitches_passes_through_their_geometric_middle() {
        // Pitch is heard geometrically, so half way through a slide from 400
        // to 100 is 200 — their geometric mean — not 250, their average.
        let mut swept = tone(Waveform::Square, 400.0);
        swept.sweep = Some(Sweep {
            to_hz: 100.0,
            over_secs: 1.0,
        });
        let mut mixer = Mixer::new();
        mixer.add(voice(1, swept));
        let mut out = vec![0.0; 1];
        mixer.render(&mut out, 1, 1000.0);

        // Half a second into a one-second sweep, which at 1000 Hz is sample
        // 500.
        let sounding = &mixer.voices[0];
        let middle = sounding.frequency_at(500);
        assert!(
            (middle - 200.0).abs() < 1.0,
            "half way should be 200 Hz, got {middle}"
        );
    }

    /// Valid parts, for a test that wants to break exactly one of them.
    fn parts() -> super::ToneParts {
        super::ToneParts {
            waveform: 1,
            frequency_hz: 440.0,
            amplitude: 0.6,
            attack_secs: 0.01,
            decay_secs: 0.0,
            sustain_level: 1.0,
            release_secs: 0.1,
            sweep_to_hz: None,
            sweep_over_secs: 0.1,
            duration_secs: None,
            starts_at_secs: None,
            vibrato: None,
            tremolo: None,
        }
    }

    /// A device that can sound anything, at time zero.
    fn limits() -> super::ToneLimits {
        super::ToneLimits {
            max_frequency_hz: MAX_FREQUENCY_HZ,
            now_secs: 0.0,
        }
    }

    /// The option `tone_from_parts` blamed, or `""` if it accepted them.
    fn blamed(parts: super::ToneParts) -> &'static str {
        super::tone_from_parts(parts, limits())
            .err()
            .map_or("", |err| err.option)
    }

    #[test]
    fn valid_parts_become_a_tone() {
        let tone = super::tone_from_parts(parts(), limits()).expect("valid parts");
        assert_eq!(tone.waveform, Waveform::Triangle);
        assert_eq!(tone.frequency_hz, 440.0);
        assert_eq!(tone.envelope.sustain_level, 1.0);
        assert_eq!(tone.starts_at_secs, None);
    }

    #[test]
    fn each_bad_part_is_blamed_on_the_option_a_program_named() {
        let broken = |change: fn(&mut super::ToneParts)| {
            let mut parts = parts();
            change(&mut parts);
            blamed(parts)
        };

        assert_eq!(broken(|p| p.waveform = 9), "waveform");
        assert_eq!(broken(|p| p.frequency_hz = 40_000.0), "frequency");
        assert_eq!(broken(|p| p.amplitude = 2.0), "amplitude");
        assert_eq!(broken(|p| p.attack_secs = -1.0), "attack");
        assert_eq!(broken(|p| p.decay_secs = -1.0), "decay");
        assert_eq!(broken(|p| p.sustain_level = 2.0), "sustainLevel");
        assert_eq!(broken(|p| p.release_secs = -1.0), "release");
        assert_eq!(broken(|p| p.sweep_to_hz = Some(40_000.0)), "sweepTo");
        assert_eq!(
            broken(|p| {
                p.sweep_to_hz = Some(200.0);
                p.sweep_over_secs = -1.0;
            }),
            "sweepOver"
        );
        assert_eq!(broken(|p| p.duration_secs = Some(0.0)), "duration");
        assert_eq!(broken(|p| p.starts_at_secs = Some(60.0)), "startAt");
    }

    #[test]
    fn nothing_that_is_not_a_number_gets_through() {
        // A NaN anywhere would poison every sample of the whole mix for as
        // long as the voice sounded.
        let broken = |change: fn(&mut super::ToneParts)| {
            let mut parts = parts();
            change(&mut parts);
            blamed(parts)
        };
        assert_eq!(broken(|p| p.frequency_hz = f32::NAN), "frequency");
        assert_eq!(broken(|p| p.amplitude = f32::NAN), "amplitude");
        assert_eq!(broken(|p| p.attack_secs = f32::NAN), "attack");
        assert_eq!(broken(|p| p.starts_at_secs = Some(f64::NAN)), "startAt");
    }

    #[test]
    fn a_frequency_is_checked_against_what_the_device_can_sound() {
        // A device running at 16 kHz can only carry 8 kHz without aliasing,
        // whatever the fixed ceiling says.
        let mut parts = parts();
        parts.frequency_hz = 12_000.0;
        assert_eq!(
            super::tone_from_parts(
                super::ToneParts { ..parts },
                super::ToneLimits {
                    max_frequency_hz: 8_000.0,
                    now_secs: 0.0,
                },
            )
            .err()
            .map_or("", |err| err.option),
            "frequency",
            "past half the output rate it would alias back down"
        );
        assert_eq!(
            blamed(parts),
            "",
            "and the same tone is fine on a faster device"
        );
    }

    #[test]
    fn a_tone_may_be_scheduled_up_to_the_horizon_but_no_further() {
        let at = |starts_at, now| {
            let mut parts = parts();
            parts.starts_at_secs = Some(starts_at);
            super::tone_from_parts(
                parts,
                super::ToneLimits {
                    max_frequency_hz: MAX_FREQUENCY_HZ,
                    now_secs: now,
                },
            )
            .err()
            .map_or("", |err| err.option)
        };

        assert_eq!(at(10.5, 10.0), "", "half a second out is fine");
        assert_eq!(at(12.0, 10.0), "", "and the horizon itself is fine");
        assert_eq!(at(12.1, 10.0), "startAt", "past it is not");
        // A voice parked in the far future holds a slot without ever
        // sounding, and the slots belong to every program at once.
        assert_eq!(at(3600.0, 10.0), "startAt");
    }

    #[test]
    fn a_tone_scheduled_in_the_past_is_accepted_rather_than_refused() {
        // A program that dropped a frame should get a late note, not an
        // exception in the middle of a sequence.
        let mut parts = parts();
        parts.starts_at_secs = Some(1.0);
        assert_eq!(
            super::tone_from_parts(
                parts,
                super::ToneLimits {
                    max_frequency_hz: MAX_FREQUENCY_HZ,
                    now_secs: 10.0,
                },
            )
            .err()
            .map_or("", |err| err.option),
            ""
        );
    }

    #[test]
    fn noise_churns_once_per_cycle_even_when_a_sample_spans_several() {
        // Three cycles per sample: the register has to step three times, or
        // the noise stops churning at the pitch it was asked for.
        let mut sounding = voice(1, tone(Waveform::Noise, 3000.0));
        sounding.advance(1.0 / 1000.0, 0);
        let mut expected = LFSR_SEED;
        for _ in 0..3 {
            expected = advance_lfsr(expected);
        }
        assert_eq!(sounding.lfsr, expected);
    }

    /// A tone scheduled for `secs`, with a flat envelope so a test sees the
    /// waveform itself.
    fn scheduled(secs: f64) -> Tone {
        Tone {
            starts_at_secs: Some(secs),
            ..tone(Waveform::Square, 0.0)
        }
    }

    #[test]
    fn the_clock_advances_by_one_sample_for_every_frame_rendered() {
        let mut mixer = Mixer::new();
        let mut out = vec![0.0; 40];
        mixer.render(&mut out, 2, 1000.0);
        assert_eq!(mixer.clock_samples, 20, "twenty frames of two channels");
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.clock_samples, 60);
    }

    #[test]
    fn a_scheduled_voice_is_silent_until_the_sample_it_named() {
        // Ten samples in, at a round 1000 Hz.
        let mut mixer = Mixer::new();
        let starts_at = mixer.sample_index_for(Some(0.01), 1000.0);
        assert_eq!(starts_at, 10);
        mixer.add(Voice::new(1, OWNER, scheduled(0.01), starts_at, RATE));

        let mut out = vec![0.0; 20];
        mixer.render(&mut out, 1, 1000.0);

        assert!(
            out[..10].iter().all(|&s| s == 0.0),
            "nothing before its moment"
        );
        assert!(
            out[10..].iter().all(|&s| s != 0.0),
            "and sounding from it onward"
        );
    }

    #[test]
    fn a_scheduled_voice_starts_mid_buffer_rather_than_on_a_buffer_boundary() {
        // The whole point of scheduling: a device hands over whole buffers,
        // and a voice placed inside one has to begin inside it rather than
        // waiting for the edge. Sixteen-sample buffers, a voice due at 21.
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, scheduled(0.021), 21, RATE));

        let mut first = vec![0.0; 16];
        mixer.render(&mut first, 1, 1000.0);
        assert!(first.iter().all(|&s| s == 0.0), "not in the first buffer");

        let mut second = vec![0.0; 16];
        mixer.render(&mut second, 1, 1000.0);
        assert!(
            second[..5].iter().all(|&s| s == 0.0),
            "silent for the first five of the second buffer"
        );
        assert!(
            second[5..].iter().all(|&s| s != 0.0),
            "and sounding from the sixth, which is sample 21"
        );
    }

    #[test]
    fn a_scheduled_voice_does_not_age_while_it_waits() {
        // Its envelope has to begin when the voice does. A voice that aged
        // while pending would arrive part-way through its own attack, or
        // already over.
        let mut shaped = scheduled(0.01);
        shaped.envelope.attack_secs = 0.01;
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, shaped, 10, RATE));

        let mut out = vec![0.0; 20];
        mixer.render(&mut out, 1, 1000.0);

        let sounding = unscaled(out[10..].to_vec());
        assert!(
            sounding[0].abs() < 1e-5,
            "starts from silence, not part-way"
        );
        for window in sounding.windows(2) {
            assert!(window[1] > window[0], "and rises through its whole attack");
        }
    }

    #[test]
    fn a_voice_scheduled_in_the_past_sounds_immediately() {
        // A program that dropped a frame gets a late note rather than a hole.
        let mut mixer = Mixer::new();
        let mut out = vec![0.0; 50];
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.clock_samples, 50);

        let starts_at = mixer.sample_index_for(Some(0.01), 1000.0);
        mixer.add(Voice::new(1, OWNER, scheduled(0.01), starts_at, RATE));
        let mut after = vec![0.0; 4];
        mixer.render(&mut after, 1, 1000.0);
        assert!(after.iter().all(|&s| s != 0.0), "sounding at once");
    }

    #[test]
    fn a_pending_voice_is_stolen_only_after_every_sounding_one() {
        // A scheduled voice is silent, so by loudness alone it would be the
        // obvious thing to discard — and a program queueing a sequence would
        // watch each new voice eat the ones already waiting.
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, scheduled(1.0), 1000, RATE));
        for id in 2..=MAX_VOICES as u32 {
            mixer.add(voice(id, tone(Waveform::Square, 100.0)));
        }

        mixer.add(voice(9999, tone(Waveform::Square, 100.0)));

        assert!(
            mixer.voices.iter().any(|voice| voice.id == 1),
            "the waiting voice keeps its slot"
        );
    }

    #[test]
    fn the_latest_starting_pending_voice_is_the_one_stolen() {
        let mut mixer = Mixer::new();
        for (id, at) in (1..=MAX_VOICES as u32).zip((1..).map(|n| n * 100)) {
            mixer.add(Voice::new(id, OWNER, scheduled(1.0), at, RATE));
        }

        mixer.add(voice(9999, tone(Waveform::Square, 100.0)));

        assert!(
            !mixer
                .voices
                .iter()
                .any(|voice| voice.id == MAX_VOICES as u32),
            "the one furthest out gives way first"
        );
        assert!(
            mixer.voices.iter().any(|voice| voice.id == 1),
            "and the most imminent is untouched"
        );
    }

    #[test]
    fn stopping_a_voice_that_has_not_sounded_removes_it() {
        // Nothing to ring out, so it goes rather than spending its release
        // fading from silence to silence.
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, scheduled(1.0), 1000, RATE));
        mixer.stop(1);
        assert_eq!(mixer.active(), 0);

        let mut out = vec![0.0; 2000];
        mixer.render(&mut out, 1, 1000.0);
        assert!(out.iter().all(|&s| s == 0.0), "and never sounds");
    }

    #[test]
    fn a_vm_going_away_takes_its_scheduled_voices_with_it() {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, 7, scheduled(1.0), 1000, RATE));
        mixer.add(Voice::new(2, 8, scheduled(1.0), 1000, RATE));

        mixer.release_sustaining(7);

        assert_eq!(mixer.active(), 1, "its queued voice is dropped");
        assert_eq!(mixer.voices[0].id, 2, "another VM's is not");
    }

    #[test]
    fn a_bend_moves_the_pitch_across_its_ramp_rather_than_jumping() {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone(Waveform::Square, 400.0)));
        // Half a ramp of 100 samples, so half way geometrically between 400
        // and 100 — which is 200, their geometric mean, not 250.
        mixer.ramp(1, super::RampTarget::Frequency, 100.0, 100);

        let sounding = &mixer.voices[0];
        assert!(
            (sounding.frequency_at(0) - 400.0).abs() < 1.0,
            "starts where it was"
        );
        assert!(
            (sounding.frequency_at(50) - 200.0).abs() < 1.0,
            "half way is the geometric middle, got {}",
            sounding.frequency_at(50)
        );
        assert!(
            (sounding.frequency_at(100) - 100.0).abs() < 1.0,
            "and arrives"
        );
    }

    #[test]
    fn a_bend_starts_from_the_pitch_the_voice_actually_had() {
        // Caught mid-sweep, it carries on from where it was rather than
        // snapping back to the note's own frequency first.
        let mut swept = tone(Waveform::Square, 400.0);
        swept.sweep = Some(Sweep {
            to_hz: 100.0,
            over_secs: 1.0,
        });
        let mut mixer = Mixer::new();
        mixer.add(voice(1, swept));

        let mut out = vec![0.0; 500];
        mixer.render(&mut out, 1, RATE);
        let mid_sweep = mixer.voices[0].frequency_at(500);
        assert!((mid_sweep - 200.0).abs() < 1.0, "half way down the sweep");

        mixer.ramp(1, super::RampTarget::Frequency, 800.0, 100);
        assert!(
            (mixer.voices[0].frequency_at(500) - mid_sweep).abs() < 1.0,
            "the bend begins from there, with no step"
        );
    }

    #[test]
    fn a_bend_replaces_a_sweep_still_in_progress() {
        let mut swept = tone(Waveform::Square, 400.0);
        swept.sweep = Some(Sweep {
            to_hz: 100.0,
            over_secs: 1.0,
        });
        let mut mixer = Mixer::new();
        mixer.add(voice(1, swept));
        mixer.ramp(1, super::RampTarget::Frequency, 800.0, 100);

        // The sweep was heading for 100; one thing decides a voice's pitch,
        // and the newer instruction is it.
        assert!(
            (mixer.voices[0].frequency_at(100) - 800.0).abs() < 1.0,
            "the bend's target wins"
        );
    }

    #[test]
    fn a_bend_on_a_scheduled_voice_begins_when_the_voice_does() {
        // A ramp that ran while the voice was still silent would be over
        // before anyone heard it.
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, scheduled(1.0), 1000, RATE));
        mixer.ramp(1, super::RampTarget::Frequency, 100.0, 100);

        let sounding = &mixer.voices[0];
        assert!(
            (sounding.frequency_at(1000) - sounding.tone.frequency_hz).abs() < 1e-3,
            "still at its own pitch when it starts"
        );
        assert!(
            (sounding.frequency_at(1100) - 100.0).abs() < 1.0,
            "and arrives a ramp after that"
        );
    }

    #[test]
    fn a_fade_moves_the_level_without_a_step() {
        let mut mixer = Mixer::new();
        mixer.add(voice(1, tone(Waveform::Square, 100.0)));
        mixer.ramp(1, super::RampTarget::Amplitude, 0.0, 100);

        let sounding = &mixer.voices[0];
        assert!(
            (sounding.amplitude_at(0) - 1.0).abs() < 1e-5,
            "starts where it was"
        );
        assert!(
            (sounding.amplitude_at(50) - 0.5).abs() < 1e-5,
            "linear, unlike pitch"
        );
        assert!(sounding.amplitude_at(100).abs() < 1e-5, "and arrives");
    }

    #[test]
    fn a_fade_still_leaves_the_envelope_shaping_the_note() {
        // Fading scales the voice's own level; the envelope multiplies on
        // top rather than being overridden by it.
        let mut shaped = tone(Waveform::Square, 0.0);
        shaped.envelope.sustain_level = 0.5;
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));
        mixer.ramp(1, super::RampTarget::Amplitude, 0.4, 0);

        let mut out = vec![0.0; 4];
        mixer.render(&mut out, 1, RATE);
        assert!(
            (unscaled(out)[0] - 0.2).abs() < 1e-5,
            "half the envelope times four tenths of the level"
        );
    }

    #[test]
    fn a_faded_out_voice_is_the_first_one_stolen() {
        // It is genuinely inaudible, so it is what a listener misses least.
        let mut mixer = full_mixer();
        mixer.ramp(1, super::RampTarget::Amplitude, 0.0, 0);
        mixer.add(voice(9999, tone(Waveform::Square, 100.0)));

        assert!(
            !mixer.voices.iter().any(|voice| voice.id == 1),
            "the faded voice gave up its slot"
        );
    }

    #[test]
    fn bending_a_voice_that_has_finished_does_nothing() {
        let mut shaped = tone(Waveform::Square, 100.0);
        shaped.envelope.duration_secs = Some(0.001);
        let mut mixer = Mixer::new();
        mixer.add(voice(1, shaped));
        let mut out = vec![0.0; 10];
        mixer.render(&mut out, 1, RATE);
        assert_eq!(mixer.active(), 0);

        mixer.ramp(1, super::RampTarget::Frequency, 200.0, 10);
        assert_eq!(mixer.active(), 0, "and nothing came back");
    }

    #[test]
    fn vibrato_carries_the_pitch_either_side_of_the_note() {
        // A quarter of the way through a cycle is the top of the wobble, and
        // three quarters is the bottom.
        let mut wobbled = tone(Waveform::Square, 440.0);
        wobbled.vibrato = Some(super::Wobble {
            depth: 12.0,
            rate_hz: 1.0,
        });
        let mut sounding = voice(1, wobbled);

        assert!(
            (sounding.frequency_at(0) - 440.0).abs() < 1.0,
            "starts on the note"
        );
        sounding.elapsed_secs = 0.25;
        assert!(
            (sounding.frequency_at(0) - 880.0).abs() < 1.0,
            "an octave up at the peak, since depth is in semitones"
        );
        sounding.elapsed_secs = 0.75;
        assert!(
            (sounding.frequency_at(0) - 220.0).abs() < 1.0,
            "and an octave down at the trough"
        );
    }

    #[test]
    fn a_vibratos_depth_sounds_the_same_at_every_pitch() {
        // Semitones are multiplicative, which is the reason for the unit: two
        // notes an octave apart wobble by the same musical interval rather
        // than the same number of hertz.
        let wobble = super::Wobble {
            depth: 12.0,
            rate_hz: 1.0,
        };
        let peak = |hz| {
            let mut wobbled = tone(Waveform::Square, hz);
            wobbled.vibrato = Some(wobble);
            let mut sounding = voice(1, wobbled);
            sounding.elapsed_secs = 0.25;
            sounding.frequency_at(0) / hz
        };
        assert!((peak(220.0) - peak(880.0)).abs() < 1e-3);
    }

    #[test]
    fn tremolo_carries_the_level_either_side_of_its_amplitude() {
        let mut wobbled = tone(Waveform::Square, 100.0);
        wobbled.amplitude = 0.5;
        wobbled.tremolo = Some(super::Wobble {
            depth: 1.0,
            rate_hz: 1.0,
        });
        let mut sounding = voice(1, wobbled);

        assert!(
            (sounding.amplitude_at(0) - 0.5).abs() < 1e-5,
            "starts at its level"
        );
        sounding.elapsed_secs = 0.25;
        assert!(
            (sounding.amplitude_at(0) - 1.0).abs() < 1e-5,
            "up at the peak"
        );
        sounding.elapsed_secs = 0.75;
        assert!(
            sounding.amplitude_at(0).abs() < 1e-5,
            "and down at the trough"
        );
    }

    #[test]
    fn a_wobble_starts_when_its_voice_does_rather_than_when_it_was_queued() {
        // Measured from the voice's own elapsed time, which doesn't move
        // while it waits for its moment.
        let mut wobbled = scheduled(1.0);
        wobbled.vibrato = Some(super::Wobble {
            depth: 12.0,
            rate_hz: 1.0,
        });
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, wobbled, 1000, RATE));

        let mut out = vec![0.0; 1000];
        mixer.render(&mut out, 1, RATE);
        assert_eq!(
            mixer.voices[0].elapsed_secs, 0.0,
            "it hasn't aged, so its wobble hasn't started"
        );
    }

    #[test]
    fn a_wobble_that_is_out_of_range_blames_itself() {
        let broken = |change: fn(&mut super::ToneParts)| {
            let mut parts = parts();
            change(&mut parts);
            blamed(parts)
        };
        assert_eq!(broken(|p| p.vibrato = Some([24.0, 6.0])), "vibrato");
        assert_eq!(broken(|p| p.vibrato = Some([1.0, 200.0])), "vibrato");
        assert_eq!(broken(|p| p.tremolo = Some([2.0, 6.0])), "tremolo");
        assert_eq!(broken(|p| p.tremolo = Some([0.5, -1.0])), "tremolo");
        assert_eq!(
            broken(|p| p.vibrato = Some([0.3, 6.0])),
            "",
            "a real one is fine"
        );
    }

    #[test]
    fn every_waveform_id_maps_to_its_own_waveform() {
        // `ely:sound` exports these same numbers, so this mapping is a
        // contract with userland rather than an internal detail.
        assert_eq!(Waveform::from_id(0), Some(Waveform::Square));
        assert_eq!(Waveform::from_id(1), Some(Waveform::Triangle));
        assert_eq!(Waveform::from_id(2), Some(Waveform::Sine));
        assert_eq!(Waveform::from_id(3), Some(Waveform::Noise));
        assert_eq!(Waveform::from_id(4), None);
    }

    #[test]
    fn a_detached_audio_records_what_it_was_asked_to_play() {
        let (sound, log) = Sound::detached();
        let id = sound
            .play(OWNER, tone(Waveform::Sine, 220.0))
            .expect("plays");
        sound.stop(id);

        let played = log.played();
        assert_eq!(played.len(), 1);
        assert_eq!(played[0].waveform, Waveform::Sine);
    }

    #[test]
    fn two_voices_sum_to_more_than_either_alone() {
        // Mixing is additive rather than last-writer-wins, which is the one
        // place the analogy with drawing onto a framebuffer breaks down.
        let quiet = Tone {
            amplitude: 0.25,
            ..tone(Waveform::Square, 0.0)
        };
        let one = render_one(quiet, 4);

        let mut mixer = Mixer::new();
        mixer.add(voice(1, quiet));
        mixer.add(voice(2, quiet));
        let mut two = vec![0.0; 4];
        mixer.render(&mut two, 1, 1000.0);

        assert!(two[0] > one[0], "{} is not louder than {}", two[0], one[0]);
        assert!((two[0] - one[0] * 2.0).abs() < 1e-5, "and it is a sum");
    }
}
