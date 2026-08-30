//! The Audio device: a mixer that sums any number of simultaneously
//! sounding voices into the one stream the output device plays.
//!
//! `Audio` never leaks `cpal`'s device or stream types outside this module,
//! the same way `Framebuffer` never leaks `winit` types — see
//! `kernel/window.rs`, which establishes that pattern for the window's OS
//! resource. Here the OS resource is the default output stream instead of a
//! window, and unlike the window, nothing else in the kernel needs to reach
//! it: no other device shares state with it, so it's started independently
//! of `Devices` and `ProcessManager`.
//!
//! Mixing is [`Mixer::render`], which takes a buffer and a sample rate and
//! nothing else, so a mix can be verified against a plain `Vec<f32>` with no
//! real audio device involved — the same separation `framebuffer::rasterize`
//! uses to test rasterization without a window.
//!
//! This is the one place in the kernel that crosses an OS thread boundary.
//! The output callback runs on a thread the audio backend owns, invoked on
//! its own schedule, so the state it touches can't be `Rc` and the main
//! thread can't reach into it directly. The voices therefore live entirely
//! inside the callback, and the main thread only ever sends it commands
//! down an `mpsc` channel, drained at the top of each callback. A callback
//! that blocks produces a buffer underrun, heard as a click or a dropout,
//! and a `Mutex` shared with the main thread is exactly how you get one:
//! the main thread can be descheduled still holding the lock while the
//! higher-priority audio thread waits on it. `try_recv` never blocks
//! waiting for a message and never allocates on the receiving side, and
//! sends here are rare — a handful per frame at most — so contention is
//! effectively absent. That is a practical guarantee rather than a formal
//! one: `std::sync::mpsc` isn't documented as lock-free, and a receiver can
//! briefly spin when a sender has reserved a slot without publishing it
//! yet. A lock-free single-producer ring is where this design would go if
//! it ever needed to be rigorous.
//!
//! Unlike a missing window (load-bearing for everything else the kernel
//! does), a missing or unusable audio device is not fatal: a real machine
//! may simply have none. [`start`] returns `None` rather than panicking,
//! logging the specific reason, and `ely:sound`'s bindings degrade to silent
//! no-ops, so both the kernel and every program boot normally either way.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rquickjs::{Ctx, Result};

use crate::bindings::bind;

/// Hard ceiling on simultaneously sounding voices. A `play` past this is
/// rejected and logged. Mixing cost is linear in the voice count and every
/// voice steals headroom from the ones already sounding; this bounds both.
const MAX_VOICES: usize = 32;

/// Every voice is summed at full scale and the result clamped, so the mix is
/// attenuated to leave room for several at once. Clipping a summed signal
/// folds its peaks flat, which is heard as harsh distortion rather than as
/// loudness — the trade is a quieter mix that stays clean at any voice count
/// this device permits.
const MASTER_GAIN: f32 = 0.2;

/// The state a 15-bit LFSR powers up in; see [`Waveform::Noise`].
const LFSR_SEED: u16 = 0x7fff;

/// The highest frequency `ely:sound` will accept. Past roughly 20 kHz a tone
/// is inaudible, and past half the output rate it aliases back down into
/// something that is audible but isn't the note that was asked for.
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
/// holds at full, then falls back to silence across `release_secs`.
///
/// Its job is that a waveform switched on at full amplitude is a step
/// discontinuity in the signal, heard as a click at both ends of every note.
/// Even a ten-millisecond attack removes that entirely.
///
/// There is no decay stage and no sustain level: a note holds at full until
/// it releases. The fuller ADSR model adds a decay falling to a held level
/// below full, which is what gives a struck string its loud attack settling
/// into a quieter held tone — a shape this envelope can't express.
#[derive(Debug, Clone, Copy)]
pub struct Envelope {
    pub attack_secs: f32,
    pub release_secs: f32,
    /// How long the note holds at full amplitude before its release begins,
    /// so a voice sounds for `duration_secs + release_secs` in total.
    /// `None` holds until [`Audio::stop`].
    pub duration_secs: Option<f32>,
}

/// Everything [`Audio::play`] needs to start one voice.
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    pub waveform: Waveform,
    pub frequency_hz: f32,
    /// Scales this voice within the mix, before [`MASTER_GAIN`].
    pub amplitude: f32,
    pub envelope: Envelope,
}

/// One sounding voice: a waveform, where it currently is in its cycle, and
/// how far through its envelope it has travelled.
struct Voice {
    id: VoiceId,
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
    fn new(id: VoiceId, tone: Tone) -> Self {
        Self {
            id,
            tone,
            phase: 0.0,
            lfsr: LFSR_SEED,
            elapsed_secs: 0.0,
            releasing_since: None,
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
            return self.level_at_release_start() * (1.0 - through).clamp(0.0, 1.0);
        }

        let attack = self.tone.envelope.attack_secs;
        if attack > 0.0 && self.elapsed_secs < attack {
            return self.elapsed_secs / attack;
        }
        1.0
    }

    /// The envelope value the release started from. A voice stopped during
    /// its attack releases from however far it got, so cutting a note short
    /// still ramps down from where it actually was instead of jumping to
    /// full first.
    fn level_at_release_start(&self) -> f32 {
        let Some(since) = self.releasing_since else {
            return 1.0;
        };
        let attack = self.tone.envelope.attack_secs;
        if attack > 0.0 && since < attack {
            since / attack
        } else {
            1.0
        }
    }

    /// Advances the waveform and the envelope by one sample, entering the
    /// release once `duration_secs` has run out.
    fn advance(&mut self, dt_secs: f32) {
        self.phase += self.tone.frequency_hz * dt_secs;
        if self.phase >= 1.0 {
            // Clocked per cycle rather than per sample, so frequency
            // pitches the noise — see `advance_lfsr`.
            self.lfsr = advance_lfsr(self.lfsr);
            self.phase -= self.phase.floor();
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

/// What the main thread asks the audio thread to do.
enum Command {
    Play { id: VoiceId, tone: Tone },
    Stop(VoiceId),
}

/// The voices currently sounding, and the mix of them. Lives entirely on the
/// audio thread.
struct Mixer {
    voices: Vec<Voice>,
}

impl Mixer {
    fn new() -> Self {
        Self {
            // Allocated once, up front, and `add` refuses past the cap, so
            // `push` never reallocates — the audio callback never reaches
            // the allocator.
            voices: Vec::with_capacity(MAX_VOICES),
        }
    }

    /// Starts `voice` sounding, or returns `false` if every voice is already
    /// in use. The authoritative cap: [`Audio::play`]'s own check is against
    /// a count that can be one callback stale.
    fn add(&mut self, voice: Voice) -> bool {
        if self.voices.len() >= MAX_VOICES {
            return false;
        }
        self.voices.push(voice);
        true
    }

    /// Releases the voice `id` names. A voice that finished on its own a
    /// callback before its stop arrived is the ordinary race, not an error,
    /// so an unknown id is a no-op.
    fn stop(&mut self, id: VoiceId) {
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.id == id) {
            voice.release();
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
            let mixed: f32 = self
                .voices
                .iter()
                .map(|voice| {
                    voice.tone.waveform.sample(voice.phase, voice.lfsr)
                        * voice.tone.amplitude
                        * voice.amplitude_envelope()
                })
                .sum();
            frame.fill((mixed * MASTER_GAIN).clamp(-1.0, 1.0));

            for voice in &mut self.voices {
                voice.advance(dt);
            }
        }

        self.voices.retain(|voice| !voice.finished());
    }

    fn active(&self) -> usize {
        self.voices.len()
    }
}

/// The kernel's handle on the audio device: allocates voice ids, sends the
/// audio thread its commands, and holds the output stream open. Dropping it
/// stops playback.
pub struct Audio {
    commands: mpsc::Sender<Command>,
    /// How many voices were sounding as of the last callback. Approximate by
    /// construction — it lags by up to one buffer, and several `play` calls
    /// within one frame all read the same stale value — which is why
    /// [`Mixer::add`] enforces the cap again where the true count lives.
    active: Arc<AtomicUsize>,
    next_id: Cell<VoiceId>,
    _stream: Option<cpal::Stream>,
}

impl Audio {
    fn allocate_id(&self) -> VoiceId {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(1));
        id
    }

    /// Starts `tone` sounding and returns the id that stops it, or `None` if
    /// every voice is in use or the audio thread is gone.
    pub fn play(&self, tone: Tone) -> Option<VoiceId> {
        if self.active_voices() >= MAX_VOICES {
            eprintln!("[audio] play rejected: all voices are in use");
            return None;
        }

        let id = self.allocate_id();
        match self.commands.send(Command::Play { id, tone }) {
            Ok(()) => Some(id),
            Err(_) => {
                eprintln!("[audio] play failed: the audio thread is gone");
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

    /// How many voices were sounding as of the last callback; see
    /// [`Audio::active`] for why this is approximate.
    pub fn active_voices(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }
}

/// The sustaining voices one VM started — the ones played with no duration,
/// which would otherwise drone on forever if the program that started them
/// faulted or exited. [`ElysiumRuntime`]'s `Drop` stops them, the same way it
/// clears that VM's loaded images.
///
/// Timed voices are deliberately absent. A sound effect has to outlive the
/// code that triggered it — a program destroys whatever made the noise and
/// the noise still finishes — and a timed voice stops itself, so the table
/// stays bounded by [`MAX_VOICES`] without ever being pruned.
///
/// [`ElysiumRuntime`]: crate::runtime::ElysiumRuntime
pub struct VoiceTable {
    sustaining: RefCell<Vec<VoiceId>>,
}

impl VoiceTable {
    pub fn new() -> Self {
        Self {
            sustaining: RefCell::new(Vec::new()),
        }
    }

    fn track(&self, id: VoiceId) {
        self.sustaining.borrow_mut().push(id);
    }

    fn forget(&self, id: VoiceId) {
        self.sustaining.borrow_mut().retain(|held| *held != id);
    }

    /// Releases every sustaining voice this VM started. They ring out over
    /// their own release rather than being cut, so a faulted program's drone
    /// fades instead of clicking off.
    pub fn stop_all(&self, audio: &Audio) {
        for id in self.sustaining.borrow_mut().drain(..) {
            audio.stop(id);
        }
    }
}

/// Binds the hidden globals `ely:sound`'s embedded module wraps. A program
/// never names one of these: it calls the module's exported `playTone` and
/// `stopVoice`, which validate their arguments and call the matching global.
///
/// `audio` is `None` on a machine whose output device couldn't be opened, in
/// which case every binding here is a silent no-op — `playTone` reports that
/// nothing sounded and a program carries on, rather than the absence of a
/// sound card becoming an error every program has to handle.
pub fn bootstrap_audio_bindings(
    ctx: &Ctx<'_>,
    audio: Option<Rc<Audio>>,
    voices: Rc<VoiceTable>,
) -> Result<()> {
    {
        let audio = audio.clone();
        let voices = Rc::clone(&voices);
        bind(
            ctx,
            "__sound_play",
            move |ctx: Ctx<'_>,
                  waveform: u8,
                  frequency_hz: f32,
                  amplitude: f32,
                  attack_secs: f32,
                  release_secs: f32,
                  duration_secs: Option<f32>|
                  -> Result<Option<VoiceId>> {
                // Everything is checked before the mixer sees any of it. The
                // finiteness checks are the load-bearing ones: a NaN
                // frequency never advances past its phase wrap, so the
                // voice's every sample is NaN, and it poisons the whole
                // mix for as long as it sounds.
                let Some(waveform) = Waveform::from_id(waveform) else {
                    return Err(rquickjs::Exception::throw_type(
                        &ctx,
                        &format!("{waveform} is not a valid waveform"),
                    ));
                };
                if !frequency_hz.is_finite() || !(0.0..=MAX_FREQUENCY_HZ).contains(&frequency_hz) {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        &format!("frequency must be between 0 and {MAX_FREQUENCY_HZ} Hz"),
                    ));
                }
                if !amplitude.is_finite() || !(0.0..=1.0).contains(&amplitude) {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        "amplitude must be between 0 and 1",
                    ));
                }
                if !attack_secs.is_finite() || attack_secs < 0.0 {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        "attack must not be negative",
                    ));
                }
                if !release_secs.is_finite() || release_secs < 0.0 {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        "release must not be negative",
                    ));
                }
                if let Some(duration) = duration_secs
                    && (!duration.is_finite() || duration <= 0.0)
                {
                    return Err(rquickjs::Exception::throw_range(
                        &ctx,
                        "duration must be greater than zero",
                    ));
                }

                let Some(audio) = &audio else {
                    return Ok(None);
                };
                let id = audio.play(Tone {
                    waveform,
                    frequency_hz,
                    amplitude,
                    envelope: Envelope {
                        attack_secs,
                        release_secs,
                        duration_secs,
                    },
                });
                // Only a voice that holds until stopped needs releasing when
                // this VM goes away; a timed one sees itself out.
                if let Some(id) = id
                    && duration_secs.is_none()
                {
                    voices.track(id);
                }
                Ok(id)
            },
        )?;
    }

    bind(ctx, "__sound_stop", move |id: VoiceId| {
        voices.forget(id);
        if let Some(audio) = &audio {
            audio.stop(id);
        }
    })?;

    Ok(())
}

/// Opens the default output device and starts its stream, or returns `None`
/// and logs why if no device is available, no `f32`-capable output config can
/// be found, or the stream can't be built or started. Never panics — see the
/// module doc comment.
pub fn start() -> Option<Audio> {
    let Some(device) = cpal::default_host().default_output_device() else {
        eprintln!("[audio] no output device found, continuing without sound");
        return None;
    };

    // f32 output only — the common modern default — rather than the full
    // per-SampleFormat dispatch a general engine would need. Searched rather
    // than taken from `default_output_config()` alone: on some real ALSA
    // setups the default isn't f32, and giving up there would silently fail
    // on a plausible real machine, not just an audio-less sandbox.
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
        eprintln!("[audio] no f32-capable output config found, continuing without sound");
        return None;
    };

    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;

    let (commands, incoming) = mpsc::channel();
    let active = Arc::new(AtomicUsize::new(0));
    let callback_active = Arc::clone(&active);
    let mut mixer = Mixer::new();

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // A disconnected channel means `Audio` was dropped; the mixer
            // rings its remaining voices out and then stays silent.
            while let Ok(command) = incoming.try_recv() {
                match command {
                    Command::Play { id, tone } => {
                        mixer.add(Voice::new(id, tone));
                    }
                    Command::Stop(id) => mixer.stop(id),
                }
            }
            mixer.render(data, channels, sample_rate);
            callback_active.store(mixer.active(), Ordering::Relaxed);
        },
        |err| eprintln!("[audio] stream error: {err}"),
        None,
    );
    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("[audio] failed to build output stream: {err}");
            return None;
        }
    };

    if let Err(err) = stream.play() {
        eprintln!("[audio] failed to start output stream: {err}");
        return None;
    }

    eprintln!("[audio] output ready ({sample_rate} Hz, {channels} channels)");
    Some(Audio {
        commands,
        active,
        next_id: Cell::new(1),
        _stream: Some(stream),
    })
}

/// An `Audio` with no output device, and the record of everything asked of
/// it. Lets a test assert on the exact tones a program played without a
/// sound card anywhere in the picture — the same reason [`Mixer::render`]
/// takes a plain buffer.
#[cfg(test)]
pub(crate) struct AudioLog {
    incoming: mpsc::Receiver<Command>,
    active: Arc<AtomicUsize>,
}

#[cfg(test)]
impl AudioLog {
    fn drain(&self) -> Vec<Command> {
        std::iter::from_fn(|| self.incoming.try_recv().ok()).collect()
    }

    /// The tones played since the last drain, in order. Draining empties the
    /// channel, so call one of these once per stretch of interest.
    pub(crate) fn played(&self) -> Vec<Tone> {
        self.drain()
            .into_iter()
            .filter_map(|command| match command {
                Command::Play { tone, .. } => Some(tone),
                Command::Stop(_) => None,
            })
            .collect()
    }

    /// The voices stopped since the last drain, in order.
    pub(crate) fn stopped(&self) -> Vec<VoiceId> {
        self.drain()
            .into_iter()
            .filter_map(|command| match command {
                Command::Stop(id) => Some(id),
                Command::Play { .. } => None,
            })
            .collect()
    }

    /// Makes the next `play` find every voice in use. Nothing stores to the
    /// live count without a real output callback, so without this the
    /// all-voices-busy path can't be reached from a test.
    pub(crate) fn saturate(&self) {
        self.active.store(MAX_VOICES, Ordering::Relaxed);
    }
}

#[cfg(test)]
impl Audio {
    /// An `Audio` wired to nothing, paired with the log of what it was asked
    /// to do. The log holds the receiving end of the command channel, so it
    /// has to outlive the `Audio`: drop it first and every later `play`
    /// reports the audio thread as gone.
    pub(crate) fn detached() -> (Audio, AudioLog) {
        let (commands, incoming) = mpsc::channel();
        let active = Arc::new(AtomicUsize::new(0));
        let audio = Audio {
            commands,
            active: Arc::clone(&active),
            next_id: Cell::new(1),
            _stream: None,
        };
        (audio, AudioLog { incoming, active })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Audio, Envelope, MASTER_GAIN, MAX_VOICES, Mixer, Tone, Voice, Waveform, advance_lfsr,
    };

    /// A tone that holds until stopped, with no attack or release ramp, so a
    /// test sees the waveform itself rather than an envelope shaping it.
    fn tone(waveform: Waveform, frequency_hz: f32) -> Tone {
        Tone {
            waveform,
            frequency_hz,
            amplitude: 1.0,
            envelope: Envelope {
                attack_secs: 0.0,
                release_secs: 0.0,
                duration_secs: None,
            },
        }
    }

    /// Renders `count` mono samples of one voice. The sample rate is a round
    /// 1000 Hz so that a period lands on a whole number of samples.
    fn render_one(tone: Tone, count: usize) -> Vec<f32> {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, tone));
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
    fn a_voice_holds_full_amplitude_between_its_attack_and_its_duration() {
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
    fn a_voice_decays_to_silence_across_its_release() {
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
        mixer.add(Voice::new(1, tone(Waveform::Square, 0.0)));
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
        mixer.add(Voice::new(1, shaped));

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
        mixer.add(Voice::new(1, shaped));

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
        mixer.add(Voice::new(1, shaped));
        let mut out = vec![0.0; 10];
        mixer.render(&mut out, 1, 1000.0);
        assert_eq!(mixer.active(), 0);

        mixer.stop(1);
        assert_eq!(mixer.active(), 0);
    }

    #[test]
    fn stopping_an_id_that_was_never_played_does_nothing() {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, tone(Waveform::Square, 100.0)));
        mixer.stop(9999);
        assert_eq!(mixer.active(), 1, "the sounding voice is untouched");
    }

    #[test]
    fn the_mixer_refuses_voices_past_its_cap() {
        let mut mixer = Mixer::new();
        for id in 0..MAX_VOICES as u32 {
            assert!(mixer.add(Voice::new(id, tone(Waveform::Square, 100.0))));
        }
        assert!(
            !mixer.add(Voice::new(9999, tone(Waveform::Square, 100.0))),
            "the voice past the cap is refused"
        );
        assert_eq!(mixer.active(), MAX_VOICES);
    }

    #[test]
    fn a_refused_voice_does_not_disturb_the_ones_already_sounding() {
        let full = |also_refuse: bool| {
            let mut mixer = Mixer::new();
            for id in 0..MAX_VOICES as u32 {
                mixer.add(Voice::new(id, tone(Waveform::Square, 100.0)));
            }
            if also_refuse {
                mixer.add(Voice::new(9999, tone(Waveform::Sine, 250.0)));
            }
            let mut out = vec![0.0; 50];
            mixer.render(&mut out, 1, 1000.0);
            out
        };
        assert_eq!(full(false), full(true));
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
        mixer.add(Voice::new(1, tone(Waveform::Sine, 100.0)));
        let mut out = vec![0.0; 20];
        mixer.render(&mut out, 2, 1000.0);
        for frame in out.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn a_partial_final_frame_is_still_filled() {
        let mut mixer = Mixer::new();
        mixer.add(Voice::new(1, tone(Waveform::Square, 0.0)));
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
            mixer.add(Voice::new(id, tone(Waveform::Square, 100.0)));
        }
        let mut out = vec![0.0; 100];
        mixer.render(&mut out, 1, 1000.0);
        for sample in out {
            assert!((-1.0..=1.0).contains(&sample), "{sample} left the range");
        }
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
        let (audio, log) = Audio::detached();
        let id = audio.play(tone(Waveform::Sine, 220.0)).expect("plays");
        audio.stop(id);

        let played = log.played();
        assert_eq!(played.len(), 1);
        assert_eq!(played[0].waveform, Waveform::Sine);
    }

    #[test]
    fn a_saturated_detached_audio_refuses_to_play() {
        let (audio, log) = Audio::detached();
        log.saturate();
        assert!(audio.play(tone(Waveform::Square, 440.0)).is_none());
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
        mixer.add(Voice::new(1, quiet));
        mixer.add(Voice::new(2, quiet));
        let mut two = vec![0.0; 4];
        mixer.render(&mut two, 1, 1000.0);

        assert!(two[0] > one[0], "{} is not louder than {}", two[0], one[0]);
        assert!((two[0] - one[0] * 2.0).abs() < 1e-5, "and it is a sum");
    }
}
