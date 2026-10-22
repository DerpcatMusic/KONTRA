//! Ring modulator (sine carrier, linear dry/wet crossfade); laws in `sampler_ir::fx::ringmod`.
//!
//! The carrier phase of both channels lives in one frame of the stage's delay storage at
//! `offset`, so the stage needs no allocation and survives block boundaries. The carrier is
//! a single sine shared by both channels, so the output keeps the input's stereo image.
use super::{Planar, kernels};
use sampler_ir::fx::ringmod::Config;

/// Compiled ring modulator: carrier step per frame and the crossfade and wet gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RingMod {
    /// Carrier cycles per frame.
    step: f64,
    mix: f64,
    /// Wet gain times the crossfade's wet weight.
    wet: f64,
}

impl RingMod {
    /// Carrier cycles per frame, for traces.
    pub(crate) fn step(&self) -> f64 {
        self.step
    }

    pub(crate) fn mix(&self) -> f64 {
        self.mix
    }

    /// Frames of delay storage the phase occupies.
    pub(super) const FRAMES: usize = 1;

    pub(super) fn compile(config: Config, rate: u32) -> Self {
        Self {
            step: config.frequency / f64::from(rate),
            mix: config.mix,
            wet: config.mix * config.gain,
        }
    }

    /// `storage[0][0]` is the carrier phase in cycles, kept in [0, 1).
    pub(super) fn process(&self, storage: &mut [[f64; 2]], block: &mut Planar, len: usize) {
        let mut phase = storage[0][0];
        let dry = 1. - self.mix;
        for i in 0..len {
            let carrier = (std::f64::consts::TAU * phase).sin();
            for channel in block.iter_mut() {
                channel[i] = kernels::gain(channel[i], dry) + channel[i] * self.wet * carrier;
            }
            phase += self.step;
            phase -= phase.floor();
        }
        storage[0][0] = phase;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::BLOCK;

    /// Kontakt measurement: a 0.5 DC input at mix 1 gives a carrier whose peak is 1.6 times
    /// the dry level. At 80 Hz the carrier has 160 zero crossings per second.
    #[test]
    fn carrier_rate_and_wet_gain_follow_the_measured_laws() {
        let config = Config { frequency: 80., mix: 1., gain: Config::KONTAKT_GAIN };
        let stage = RingMod::compile(config, 48_000);
        let mut storage = [[0.; 2]; RingMod::FRAMES];
        let mut out = Vec::with_capacity(48_000);
        for _ in 0..48_000 / BLOCK {
            let mut block: Planar = [[0.5; BLOCK]; 2];
            stage.process(&mut storage, &mut block, BLOCK);
            out.extend_from_slice(&block[0]);
        }
        let peak = out.iter().fold(0f64, |m, v| m.max(v.abs()));
        assert!((peak - 0.8).abs() < 1e-3, "peak {peak}");
        let crossings = out.windows(2).filter(|w| (w[0] < 0.) != (w[1] < 0.)).count();
        assert!((159..=161).contains(&crossings), "crossings {crossings}");
    }

    /// Mix 0 is the dry signal exactly, and the phase is continuous across blocks.
    #[test]
    fn dry_mix_is_bit_exact_and_phase_persists() {
        let dry = RingMod::compile(Config { frequency: 80., mix: 0., gain: 1.6 }, 48_000);
        let mut storage = [[0.; 2]; RingMod::FRAMES];
        let mut block: Planar = [[0.25; BLOCK]; 2];
        dry.process(&mut storage, &mut block, BLOCK);
        assert!(block[0].iter().all(|v| *v == 0.25));
        let half = RingMod::compile(Config { frequency: 80., mix: 0.5, gain: 1.6 }, 48_000);
        let mut whole = [[0.; 2]; RingMod::FRAMES];
        let mut split = [[0.; 2]; RingMod::FRAMES];
        let mut a: Planar = [[0.5; BLOCK]; 2];
        half.process(&mut whole, &mut a, BLOCK);
        let mut b: Planar = [[0.5; BLOCK]; 2];
        let mut c: Planar = [[0.5; BLOCK]; 2];
        half.process(&mut split, &mut b, BLOCK / 2);
        half.process(&mut split, &mut c, BLOCK / 2);
        assert_eq!(&a[0][..BLOCK / 2], &b[0][..BLOCK / 2]);
        assert_eq!(&a[0][BLOCK / 2..], &c[0][..BLOCK / 2]);
        assert_eq!(whole, split);
    }
}
