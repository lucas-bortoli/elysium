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
use std::sync::mpsc;

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
#[allow(clippy::too_many_arguments)]
pub fn tone_from_parts(
    waveform: u8,
    frequency_hz: f32,
    amplitude: f32,
    envelope: [f32; 4],
    sweep: Option<[f32; 2]>,
    duration_secs: Option<f32>,
    max_frequency_hz: f32,
) -> std::result::Result<Tone, ToneError> {
    let Some(waveform) = Waveform::from_id(waveform) else {
        return Err(ToneError::new("waveform", "is not one of the four"));
    };
    let [attack_secs, decay_secs, sustain_level, release_secs] = envelope;

    let sweep = match sweep {
        Some([to_hz, over_secs]) => Some(Sweep {
            to_hz: checked_frequency("sweepTo", to_hz, max_frequency_hz)?,
            over_secs: checked_secs("sweepOver", over_secs)?,
        }),
        None => None,
    };

    if let Some(duration) = duration_secs
        && (!duration.is_finite() || duration <= 0.0)
    {
        return Err(ToneError::new("duration", "must be greater than zero"));
    }

    Ok(Tone {
        waveform,
        frequency_hz: checked_frequency("frequency", frequency_hz, max_frequency_hz)?,
        amplitude: checked_level("amplitude", amplitude)?,
        envelope: Envelope {
            attack_secs: checked_secs("attack", attack_secs)?,
            decay_secs: checked_secs("decay", decay_secs)?,
            sustain_level: checked_level("sustainLevel", sustain_level)?,
            release_secs: checked_secs("release", release_secs)?,
            duration_secs,
        },
        sweep,
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
    /// `0.0..1.0`, wraps every cycle.
    phase: f32,
    lfsr: u16,
    elapsed_secs: f32,
    /// The `elapsed_secs` at which the release began, set either by
    /// `duration_secs` running out or by a `Stop` arriving.
    releasing_since: Option<f32>,
}

impl Voice {
    fn new(id: VoiceId, owner: ProcessId, tone: Tone) -> Self {
        Self {
            id,
            owner,
            tone,
            phase: 0.0,
            lfsr: LFSR_SEED,
            elapsed_secs: 0.0,
            releasing_since: None,
        }
    }

    /// How loud this voice actually is right now: its envelope scaled by its
    /// own amplitude. What [`Mixer::add`] compares when it has to steal a
    /// slot, since neither number alone says which voice is faintest.
    fn audible_level(&self) -> f32 {
        self.tone.amplitude * self.amplitude_envelope()
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

    /// This voice's pitch `elapsed` seconds in: its own frequency, sliding
    /// toward the sweep's target across the sweep's time and holding there
    /// afterwards.
    ///
    /// The slide is geometric rather than linear in hertz, because pitch is
    /// heard that way — an octave is a doubling, so an even-sounding glide
    /// is one that multiplies by a constant each moment rather than adding
    /// one. A linear ramp from 800 Hz to 200 Hz spends most of its time
    /// sounding low. Endpoints at or below zero can't be multiplied toward,
    /// so those fall back to a straight ramp.
    fn frequency_at(&self, elapsed: f32) -> f32 {
        let Some(sweep) = self.tone.sweep else {
            return self.tone.frequency_hz;
        };
        if sweep.over_secs <= 0.0 {
            return sweep.to_hz;
        }

        let from = self.tone.frequency_hz;
        let through = (elapsed / sweep.over_secs).clamp(0.0, 1.0);
        if from > 0.0 && sweep.to_hz > 0.0 {
            from * (sweep.to_hz / from).powf(through)
        } else {
            from + (sweep.to_hz - from) * through
        }
    }

    /// Advances the waveform and the envelope by one sample, entering the
    /// release once `duration_secs` has run out.
    fn advance(&mut self, dt_secs: f32) {
        self.phase += self.frequency_at(self.elapsed_secs) * dt_secs;
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
    /// Releases every sustaining voice `owner` started, sent when its VM
    /// goes away. Timed voices are left alone: a sound effect has to outlive
    /// the code that triggered it.
    ReleaseSustaining(ProcessId),
}

/// The voices currently sounding, and the mix of them. Lives entirely on the
/// sound thread.
struct Mixer {
    voices: Vec<Voice>,
}

impl Mixer {
    fn new() -> Self {
        Self {
            // Allocated once, up front, and `add` never grows past the cap,
            // so `push` never reallocates — the sound callback never reaches
            // the allocator.
            voices: Vec::with_capacity(MAX_VOICES),
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

        let quietest = self
            .voices
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.audible_level()
                    .total_cmp(&b.audible_level())
                    .then(b.elapsed_secs.total_cmp(&a.elapsed_secs))
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
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == id) {
            voice.release();
        }
    }

    /// Releases every sustaining voice `owner` started. A timed voice is
    /// left to finish on its own, so a sound effect still outlives the
    /// program that triggered it.
    fn release_sustaining(&mut self, owner: ProcessId) {
        for voice in &mut self.voices {
            if voice.owner == owner && voice.tone.envelope.duration_secs.is_none() {
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

        for frame in out.chunks_mut(channels) {
            let mut mixed = 0.0;
            for voice in &mut self.voices {
                mixed += voice.tone.waveform.sample(voice.phase, voice.lfsr)
                    * voice.tone.amplitude
                    * voice.amplitude_envelope();
                voice.advance(dt);
            }
            frame.fill((mixed * MASTER_GAIN).clamp(-1.0, 1.0));
        }

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

    /// Releases every sustaining voice `owner` started, ringing each out
    /// over its own release. Called when a VM goes away, so a program that
    /// faulted or exited holding a note doesn't leave it droning for the
    /// life of the kernel.
    pub fn release_sustaining(&self, owner: ProcessId) {
        let _ = self.commands.send(Command::ReleaseSustaining(owner));
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
                  amplitude: f32,
                  envelope: Vec<f32>,
                  sweep: Option<Vec<f32>>,
                  duration_secs: Option<f32>|
                  -> Result<Option<VoiceId>> {
                // The four stages arrive as one envelope rather than four
                // loose floats — not because the arguments wouldn't fit, but
                // because they are one thing, the same way a source rect
                // crosses as one array in `__framebuffer_draw_image_transformed`.
                let envelope = <[f32; 4]>::try_from(envelope.as_slice()).map_err(|_| {
                    rquickjs::Exception::throw_type(&ctx, "an envelope needs four numbers")
                })?;
                // Absent when the note holds its pitch, which is the common
                // case. A sweep's target and its time are one thing, like the
                // envelope's four stages, so they cross together or not at
                // all.
                let sweep = sweep
                    .map(|sweep| {
                        <[f32; 2]>::try_from(sweep.as_slice()).map_err(|_| {
                            rquickjs::Exception::throw_type(
                                &ctx,
                                "a sweep needs a frequency and a time",
                            )
                        })
                    })
                    .transpose()?;

                // A machine with no device still range-checks every option,
                // so a program gets the same errors whether or not anything
                // can sound. Only the ceiling differs, and 20 kHz is the one
                // no real device lowers by much.
                let max_frequency_hz = sound
                    .as_ref()
                    .map_or(MAX_FREQUENCY_HZ, |sound| sound.max_frequency_hz());

                let tone = tone_from_parts(
                    waveform,
                    frequency_hz,
                    amplitude,
                    envelope,
                    sweep,
                    duration_secs,
                    max_frequency_hz,
                )
                .map_err(|err| {
                    // A bad waveform names no point on any scale, so it is a
                    // type error and stays untagged: `ely:sound` re-types
                    // only what it can report as an out-of-range option.
                    if err.option == "waveform" {
                        return rquickjs::Exception::throw_type(
                            &ctx,
                            &format!("{} {}", err.option, err.requirement),
                        );
                    }
                    // Tagged `option: requirement`, which `ely:sound` splits
                    // back apart into a `ToneOptionError`.
                    rquickjs::Exception::throw_range(
                        &ctx,
                        &format!("{}: {}", err.option, err.requirement),
                    )
                })?;

                let Some(sound) = &sound else {
                    return Ok(None);
                };
                Ok(sound.play(owner, tone))
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
    let mut mixer = Mixer::new();

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // A disconnected channel means `Sound` was dropped; the mixer
            // rings its remaining voices out and then stays silent.
            while let Ok(command) = incoming.try_recv() {
                match command {
                    Command::Play { id, owner, tone } => {
                        mixer.add(Voice::new(id, owner, tone));
                    }
                    Command::Stop(id) => mixer.stop(id),
                    Command::ReleaseSustaining(owner) => mixer.release_sustaining(owner),
                }
            }
            mixer.render(data, channels, sample_rate);
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
        let sound = Sound {
            commands,
            max_frequency_hz: MAX_FREQUENCY_HZ,
            next_id: Cell::new(1),
            _stream: None,
        };
        (
            sound,
            SoundLog {
                incoming,
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
        }
    }

    /// Renders `count` mono samples of one voice. The sample rate is a round
    /// 1000 Hz so that a period lands on a whole number of samples.
    fn render_one(tone: Tone, count: usize) -> Vec<f32> {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, tone));
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
        mixer.add(Voice::new(1, OWNER, tone(Waveform::Square, 0.0)));
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
        mixer.add(Voice::new(1, OWNER, shaped));

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
        mixer.add(Voice::new(1, OWNER, shaped));

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
        mixer.add(Voice::new(1, OWNER, shaped));
        let mut out = vec![0.0; 10];
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.active(), 0);

        mixer.stop(1);
        assert_eq!(mixer.active(), 0);
    }

    #[test]
    fn stopping_an_id_that_was_never_played_does_nothing() {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, tone(Waveform::Square, 100.0)));
        mixer.stop(9999);
        assert_eq!(mixer.active(), 1, "the sounding voice is untouched");
    }

    /// A mixer holding `MAX_VOICES` identical voices, ready for a steal.
    fn full_mixer() -> Mixer {
        let mut mixer = Mixer::new();
        for id in 0..MAX_VOICES as u32 {
            mixer.add(Voice::new(id, OWNER, tone(Waveform::Square, 100.0)));
        }
        mixer
    }

    #[test]
    fn stealing_keeps_the_mixer_at_its_cap() {
        let mut mixer = full_mixer();
        mixer.add(Voice::new(9999, OWNER, tone(Waveform::Square, 100.0)));
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
        mixer.add(Voice::new(
            1,
            OWNER,
            Tone {
                amplitude: 0.05,
                ..tone(Waveform::Square, 100.0)
            },
        ));
        for id in 2..=MAX_VOICES as u32 {
            mixer.add(Voice::new(id, OWNER, tone(Waveform::Square, 100.0)));
        }

        mixer.add(Voice::new(9999, OWNER, tone(Waveform::Square, 100.0)));

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
        mixer.add(Voice::new(1, OWNER, shaped));
        for id in 2..=MAX_VOICES as u32 {
            mixer.add(Voice::new(id, OWNER, tone(Waveform::Square, 100.0)));
        }

        // Part-way through a long release, so it is still sounding but on
        // its way out — the one a listener misses least.
        mixer.stop(1);
        let mut out = vec![0.0; 500];
        mixer.render(&mut out, 1, 1000.0);
        mixer.add(Voice::new(9999, OWNER, tone(Waveform::Square, 100.0)));

        assert!(
            !mixer.voices.iter().any(|voice| voice.id == 1),
            "the releasing voice gave up its slot"
        );
    }

    #[test]
    fn a_stolen_voice_leaves_the_others_sounding_as_they_were() {
        let mut untouched = full_mixer();
        let mut stolen_from = full_mixer();
        stolen_from.add(Voice::new(9999, OWNER, tone(Waveform::Square, 100.0)));

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
        mixer.add(Voice::new(1, 7, held));
        mixer.add(Voice::new(2, 7, timed));
        mixer.add(Voice::new(3, 8, held));

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
        mixer.add(Voice::new(1, OWNER, tone(Waveform::Sine, 100.0)));
        let mut out = vec![0.0; 20];
        mixer.render(&mut out, 2, 1000.0);
        for frame in out.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn a_partial_final_frame_is_still_filled() {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, OWNER, tone(Waveform::Square, 0.0)));
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
            mixer.add(Voice::new(id, OWNER, tone(Waveform::Square, 100.0)));
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
        mixer.add(Voice::new(1, OWNER, shaped));
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
        mixer.add(Voice::new(1, OWNER, shaped));

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
        mixer.add(Voice::new(1, OWNER, swept));
        let mut out = vec![0.0; 1];
        mixer.render(&mut out, 1, 1000.0);

        let voice = &mixer.voices[0];
        let middle = voice.frequency_at(0.5);
        assert!(
            (middle - 200.0).abs() < 1.0,
            "half way should be 200 Hz, got {middle}"
        );
    }

    /// The loose numbers a program passes, in the order `tone_from_parts`
    /// takes them: waveform, frequency, amplitude, envelope, sweep, duration.
    type Parts = (u8, f32, f32, [f32; 4], Option<[f32; 2]>, Option<f32>);

    /// Valid parts, for a test that wants to break exactly one of them.
    fn parts() -> Parts {
        (1, 440.0, 0.6, [0.01, 0.0, 1.0, 0.1], None, None)
    }

    /// The option `tone_from_parts` blamed, or `""` if it accepted them.
    fn blamed(tone: std::result::Result<Tone, super::ToneError>) -> &'static str {
        tone.err().map_or("", |err| err.option)
    }

    #[test]
    fn valid_parts_become_a_tone() {
        let (waveform, frequency, amplitude, envelope, sweep, duration) = parts();
        let tone = super::tone_from_parts(
            waveform,
            frequency,
            amplitude,
            envelope,
            sweep,
            duration,
            MAX_FREQUENCY_HZ,
        )
        .expect("valid parts");
        assert_eq!(tone.waveform, Waveform::Triangle);
        assert_eq!(tone.frequency_hz, 440.0);
        assert_eq!(tone.envelope.sustain_level, 1.0);
    }

    #[test]
    fn each_bad_part_is_blamed_on_the_option_a_program_named() {
        let (w, f, a, e, s, d) = parts();
        let check = |waveform, frequency, amplitude, envelope, sweep, duration| {
            blamed(super::tone_from_parts(
                waveform,
                frequency,
                amplitude,
                envelope,
                sweep,
                duration,
                MAX_FREQUENCY_HZ,
            ))
        };

        assert_eq!(check(9, f, a, e, s, d), "waveform");
        assert_eq!(check(w, 40_000.0, a, e, s, d), "frequency");
        assert_eq!(check(w, f, 2.0, e, s, d), "amplitude");
        assert_eq!(check(w, f, a, [-1.0, 0.0, 1.0, 0.1], s, d), "attack");
        assert_eq!(check(w, f, a, [0.0, -1.0, 1.0, 0.1], s, d), "decay");
        assert_eq!(check(w, f, a, [0.0, 0.0, 2.0, 0.1], s, d), "sustainLevel");
        assert_eq!(check(w, f, a, [0.0, 0.0, 1.0, -1.0], s, d), "release");
        assert_eq!(check(w, f, a, e, Some([40_000.0, 0.1]), d), "sweepTo");
        assert_eq!(check(w, f, a, e, Some([200.0, -1.0]), d), "sweepOver");
        assert_eq!(check(w, f, a, e, s, Some(0.0)), "duration");
    }

    #[test]
    fn nothing_that_is_not_a_number_gets_through() {
        // A NaN anywhere would poison every sample of the whole mix for as
        // long as the voice sounded.
        let (w, f, a, e, s, d) = parts();
        let nan = f32::NAN;
        assert_eq!(
            blamed(super::tone_from_parts(w, nan, a, e, s, d, MAX_FREQUENCY_HZ)),
            "frequency"
        );
        assert_eq!(
            blamed(super::tone_from_parts(w, f, nan, e, s, d, MAX_FREQUENCY_HZ)),
            "amplitude"
        );
        assert_eq!(
            blamed(super::tone_from_parts(
                w,
                f,
                a,
                [nan, 0.0, 1.0, 0.1],
                s,
                d,
                MAX_FREQUENCY_HZ
            )),
            "attack"
        );
    }

    #[test]
    fn a_frequency_is_checked_against_what_the_device_can_sound() {
        let (w, _, a, e, s, d) = parts();
        // A device running at 16 kHz can only carry 8 kHz without aliasing,
        // whatever the fixed ceiling says.
        assert_eq!(
            blamed(super::tone_from_parts(w, 12_000.0, a, e, s, d, 8_000.0)),
            "frequency",
            "past half the output rate it would alias back down"
        );
        assert_eq!(
            blamed(super::tone_from_parts(
                w,
                12_000.0,
                a,
                e,
                s,
                d,
                MAX_FREQUENCY_HZ
            )),
            "",
            "and the same tone is fine on a faster device"
        );
    }

    #[test]
    fn noise_churns_once_per_cycle_even_when_a_sample_spans_several() {
        // Three cycles per sample: the register has to step three times, or
        // the noise stops churning at the pitch it was asked for.
        let mut voice = Voice::new(1, OWNER, tone(Waveform::Noise, 3000.0));
        voice.advance(1.0 / 1000.0);
        let mut expected = LFSR_SEED;
        for _ in 0..3 {
            expected = advance_lfsr(expected);
        }
        assert_eq!(voice.lfsr, expected);
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
        mixer.add(Voice::new(1, OWNER, quiet));
        mixer.add(Voice::new(2, OWNER, quiet));
        let mut two = vec![0.0; 4];
        mixer.render(&mut two, 1, 1000.0);

        assert!(two[0] > one[0], "{} is not louder than {}", two[0], one[0]);
        assert!((two[0] - one[0] * 2.0).abs() < 1e-5, "and it is a sum");
    }
}
