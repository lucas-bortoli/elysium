//! The Audio device: a single hardcoded square-wave tone, generated and
//! played for the lifetime of the process.
//!
//! `Audio` never leaks `cpal`'s device or stream types outside this module,
//! the same way `Framebuffer` never leaks `winit` types — see
//! `kernel/window.rs`, which establishes that pattern for the window's OS
//! resource. Here the OS resource is the default output stream instead of a
//! window, and unlike the window, nothing else in the kernel needs to reach
//! it: no other device shares state with it, so it's started independently
//! of `Devices` and `ProcessManager`.
//!
//! Sample generation is a pure function, [`SquareWave::next_sample`], kept
//! separate from the `cpal` stream callback so the waveform can be verified
//! against a plain buffer with no real audio device involved — the same
//! separation `framebuffer::rasterize` uses to test rasterization without a
//! window.
//!
//! Unlike a missing window (load-bearing for everything else the kernel
//! does), a missing or unusable audio device is not fatal: nothing depends
//! on sound yet, and a real machine may simply have none. [`start`] returns
//! `None` rather than panicking, logging the specific reason, so the kernel
//! boots normally either way.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Phase-accumulator state for one fixed-frequency square wave.
pub struct SquareWave {
    frequency_hz: f32,
    /// `0.0..1.0`, wraps every cycle.
    phase: f32,
}

impl SquareWave {
    pub fn new(frequency_hz: f32) -> Self {
        Self {
            frequency_hz,
            phase: 0.0,
        }
    }

    /// Advances by one sample at `sample_rate_hz` and returns the next
    /// output value, always exactly `1.0` or `-1.0`. `sample_rate_hz` is
    /// taken per call rather than stored, so the pitch comes out correct
    /// regardless of what rate the output device negotiates.
    pub fn next_sample(&mut self, sample_rate_hz: f32) -> f32 {
        let value = if self.phase < 0.5 { 1.0 } else { -1.0 };
        self.phase += self.frequency_hz / sample_rate_hz;
        self.phase -= self.phase.floor();
        value
    }
}

/// Holds the live output stream open for as long as this is alive; dropping
/// it stops playback. Carries nothing else — Milestone 0 has no control
/// surface, just a tone that plays until the process exits.
pub struct Audio {
    _stream: cpal::Stream,
}

/// Opens the default output device and starts playing a fixed 440 Hz square
/// wave, or returns `None` and logs why if no device is available, no
/// `f32`-capable output config can be found, or the stream can't be built or
/// started. Never panics — see the module doc comment.
pub fn start() -> Option<Audio> {
    let Some(device) = cpal::default_host().default_output_device() else {
        eprintln!("[audio] no output device found, continuing without sound");
        return None;
    };

    // Milestone 0 supports f32 output only — the common modern default —
    // rather than the full per-SampleFormat dispatch a general engine would
    // need. Searched rather than taken from `default_output_config()`
    // alone: on some real ALSA setups the default isn't f32, and giving up
    // there would silently fail on a plausible real machine, not just an
    // audio-less sandbox.
    let config = device
        .supported_output_configs()
        .ok()
        .and_then(|mut configs| configs.find(|c| c.sample_format() == cpal::SampleFormat::F32))
        .map(|c| c.with_max_sample_rate());

    let Some(config) = config else {
        eprintln!("[audio] no f32-capable output config found, continuing without sound");
        return None;
    };

    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;
    let mut wave = SquareWave::new(440.0);

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(channels) {
                frame.fill(wave.next_sample(sample_rate));
            }
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

    eprintln!("[audio] playing test tone (440 Hz square wave)");
    Some(Audio { _stream: stream })
}

#[cfg(test)]
mod tests {
    use super::SquareWave;

    fn generate(frequency_hz: f32, sample_rate_hz: f32, count: usize) -> Vec<f32> {
        let mut wave = SquareWave::new(frequency_hz);
        (0..count)
            .map(|_| wave.next_sample(sample_rate_hz))
            .collect()
    }

    #[test]
    fn a_square_wave_only_ever_outputs_the_two_rail_values() {
        for sample in generate(440.0, 48_000.0, 1000) {
            assert!(sample == 1.0 || sample == -1.0, "{sample} is not a rail");
        }
    }

    #[test]
    fn a_square_wave_completes_one_period_in_sample_rate_over_frequency_samples() {
        // 100 Hz at 1000 Hz sample rate: one period is 10 samples, so the
        // sign should flip exactly at sample index 5, not before.
        let samples = generate(100.0, 1000.0, 10);
        assert!(
            samples[..5].iter().all(|&s| s == 1.0),
            "the first half-period should stay high"
        );
        assert!(
            samples[5..].iter().all(|&s| s == -1.0),
            "the second half-period should flip low"
        );
    }

    #[test]
    fn a_square_wave_repeats_identically_across_periods() {
        let period = 10;
        let samples = generate(100.0, 1000.0, period * 3);
        assert_eq!(samples[..period], samples[period..period * 2]);
        assert_eq!(samples[..period], samples[period * 2..period * 3]);
    }

    #[test]
    fn a_square_wave_is_silent_at_zero_frequency() {
        // The phase never advances, so every sample is the same fixed
        // starting value — pinned now, before a future milestone's dynamic
        // frequency makes this reachable from JS.
        for sample in generate(0.0, 48_000.0, 100) {
            assert_eq!(sample, 1.0);
        }
    }
}
