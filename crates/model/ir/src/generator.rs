//! Unified voice generator descriptor and consolidated 7-family effect taxonomy
//! (IR vNext Phase 3).

use crate::{Effect, EffectId, EffectKind, Wave, Wavetable};

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

/// Consolidated 7-family effect taxonomy covering every DSP processor in KONTRA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectFamily {
    /// Gain, polarity/swap, stereo width/pan, channel matrix.
    Utility,
    /// State-variable/ladder filters, EQ, formant, cascade, clocked filter.
    FilterEq,
    /// Waveshaper, bitcrusher, overdrive, diode clipper, ring/freq mod.
    Drive,
    /// Compressor, channel strip, peak limiter, transient shaper.
    Dynamics,
    /// Delay, chorus, flanger, phaser, scanner, pitch shifter, comb banks.
    DelayMod,
    /// Algorithmic reverb and convolution.
    Reverb,
    /// Send taps and passthrough/placeholder routing slots.
    Routing,
}

impl EffectKind {
    /// Classifies this effect into one of the 7 consolidated [`EffectFamily`] groups.
    #[must_use]
    pub const fn family(&self) -> EffectFamily {
        match self {
            Self::Gain(_) | Self::Invert { .. } | Self::Stereo { .. } | Self::Matrix(_) => {
                EffectFamily::Utility
            }
            Self::Eq(_)
            | Self::Filter(_)
            | Self::Formant(_)
            | Self::Cascade(_)
            | Self::Clocked(_) => EffectFamily::FilterEq,
            Self::Shaper(_)
            | Self::Crush(_)
            | Self::Overdrive(_)
            | Self::Clipper(_)
            | Self::RingMod(_) => EffectFamily::Drive,
            Self::Compressor(_) | Self::Strip(_) | Self::Limiter(_) | Self::Transient(_) => {
                EffectFamily::Dynamics
            }
            Self::Delay(_)
            | Self::Scanner(_)
            | Self::Shifter(_)
            | Self::Phaser(_)
            | Self::Notches(_)
            | Self::Ensemble(_)
            | Self::CombBank(_)
            | Self::Comb(_) => EffectFamily::DelayMod,
            Self::Convolution { .. } | Self::Reverb(_) => EffectFamily::Reverb,
            Self::Sends(_) | Self::Other | Self::OtherWet => EffectFamily::Routing,
        }
    }
}

/// Indexed effect descriptor paired with its 4-byte [`EffectId`] and [`EffectFamily`].
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSpec {
    pub id: EffectId,
    pub family: EffectFamily,
    pub effect: Effect,
}

impl EffectSpec {
    #[must_use]
    pub const fn new(id: EffectId, effect: Effect) -> Self {
        let family = effect.kind.family();
        Self { id, family, effect }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_families_cover_all_seven_dsp_categories() {
        let cases = [
            (EffectKind::Gain(1.0), EffectFamily::Utility),
            (
                EffectKind::Eq([crate::Bell::FLAT; 4]),
                EffectFamily::FilterEq,
            ),
            (
                EffectKind::Crush(crate::Crush {
                    bits: 8.0,
                    rate: 12_000.0,
                    noise: 0.0,
                    noise_tone: 0.5,
                    converter: None,
                }),
                EffectFamily::Drive,
            ),
            (
                EffectKind::Limiter(crate::Limiter::PEAK),
                EffectFamily::Dynamics,
            ),
            (
                EffectKind::Comb(crate::Comb {
                    hz: 220.0,
                    feedback: 0.5,
                    forward: false,
                    octaves: 0.0,
                }),
                EffectFamily::DelayMod,
            ),
            (EffectKind::Convolution { impulse: 0 }, EffectFamily::Reverb),
            (EffectKind::Sends([0.0; 8]), EffectFamily::Routing),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.family(), expected);
        }
    }
}
