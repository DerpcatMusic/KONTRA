//! Unified voice generator descriptor (IR vNext Phase 3).
use crate::{Wave, Wavetable};

/// Unified voice sound generator: PCM samples, wavetables, real-time synth
/// oscillators, or granular clouds.
#[derive(Clone, Debug, PartialEq)]
pub enum Generator {
    /// Sample playback from [`crate::Instrument::samples`].
    Pcm {
        sample: usize,
        start: u64,
        end: u64,
        reverse: bool,
    },
    /// Band-limited wavetable playback.
    Wavetable(Wavetable),
    /// Real-time oscillator (e.g. SFZ `*sine`/`*saw`/`*square`/`*tri` or native synths).
    Oscillator {
        wave: Wave,
        tune_semitones: f32,
        phase: f32,
    },
    /// Granular cloud generator reading from [`crate::Instrument::samples`].
    Granular {
        sample: usize,
        grain_ms: f32,
        density: f32,
        position: f32,
        pitch_semitones: f32,
        spread: f32,
    },
}
