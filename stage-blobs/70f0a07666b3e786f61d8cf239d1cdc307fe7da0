//! Original implementation of the measured BitCrusher callback with Cutoff=0.
//! The hosted parameter manager and optional filter are not admitted by this leaf.
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "BitCrusher physical callback with Cutoff=0 and integer BitSize only; unchanged native callback comparisons cover quantization, rational Drive, Mix, callback-local sample-hold timing, static edits, native bypass and 1/2/6/12 channels. Nonzero Cutoff depends on unavailable native SIMD coefficient tables; hosted controls, cloning with an active filter and whole-program audio remain unverified. Program playback does not admit this leaf";

#[derive(Clone)]
pub struct BitCrusherCallback {
    channels: usize,
    rate: f32,
    effective_rate: f32,
    quantum: f32,
    drive: f32,
    mix: f32,
}

impl BitCrusherCallback {
    pub fn new(channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "BitCrusher needs 1..12 channels"
        );
        ensure!(
            [32_000., 44_100., 48_000., 96_000.].contains(&rate),
            "BitCrusher callback rate is unmeasured"
        );
        Ok(Self {
            channels,
            rate: rate as f32,
            effective_rate: 22050.,
            quantum: 65536.,
            drive: 0.,
            mix: 1.,
        })
    }

    /// Physical controls at a callback boundary, without hosted smoothing.
    pub fn configure(
        &mut self,
        effective_rate: f64,
        bits: f64,
        drive: f64,
        cutoff: f64,
        mix: f64,
    ) -> Result<()> {
        ensure!(
            effective_rate.is_finite() && (2000.0..=48000.0).contains(&effective_rate),
            "Invalid EffectiveSampleRate"
        );
        ensure!(
            bits.is_finite() && (1.0..=24.0).contains(&bits) && bits.fract() == 0.,
            "Only integer BitSize is measured"
        );
        ensure!(
            drive.is_finite() && (0.0..=1.0).contains(&drive),
            "Invalid Drive"
        );
        ensure!(
            cutoff == 0.,
            "Nonzero Cutoff requires unverified native SIMD filter tables"
        );
        ensure!(mix.is_finite() && (0.0..=1.0).contains(&mix), "Invalid Mix");
        let amount = (drive as f32) * 0.95;
        self.effective_rate = effective_rate as f32;
        self.quantum = (1u32 << (bits as u32)) as f32;
        self.drive = amount / (1. - amount);
        self.mix = mix as f32;
        Ok(())
    }

    /// Native timing restarts at frame zero for every callback. Its stored held
    /// sample is overwritten at that frame, so Cutoff=0 has no audible history.
    pub fn process(&self, frames: &mut [Frame], bypass: bool) -> Result<()> {
        ensure!(
            frames.len() <= 1_048_576,
            "BitCrusher callback exceeds bounded frame count"
        );
        ensure!(
            frames.iter().all(|f| f[..self.channels]
                .iter()
                .all(|x| x.is_finite() && x.abs() <= 4.)),
            "BitCrusher input exceeds finite quantizer range"
        );
        if bypass {
            return Ok(());
        }
        let step = self.rate / self.effective_rate;
        let inverse = 1. / self.quantum;
        for channel in 0..self.channels {
            let mut next = 0f32;
            let mut held = 0f32;
            for (index, frame) in frames.iter_mut().enumerate() {
                let dry = frame[channel];
                if index as f32 >= next {
                    held = dry;
                    next += step;
                }
                let scaled = held * self.quantum;
                // Native negative exact integers receive the same +1 adjustment.
                let quantized = (scaled.floor() + f32::from(scaled < 0.)) * inverse;
                let doubled = quantized + quantized;
                let wet = ((doubled / (doubled.abs() * self.drive + 1.)) * (self.drive + 1.)) * 0.5;
                frame[channel] = wet * self.mix + (1. - self.mix) * dry;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_negative_integer_and_callback_partition_contract() {
        let mut fx = BitCrusherCallback::new(1, 48_000.).unwrap();
        fx.configure(2000., 4., 0., 0., 1.).unwrap();
        let mut whole = [[0.; MAX_CHANNELS]; 2];
        whole[0][0] = -1.;
        whole[1][0] = -0.5;
        let mut split = whole;
        fx.process(&mut whole, false).unwrap();
        fx.process(&mut split[..1], false).unwrap();
        fx.process(&mut split[1..], false).unwrap();
        assert_eq!([whole[0][0], whole[1][0]], [-0.9375, -0.9375]);
        assert_eq!([split[0][0], split[1][0]], [-0.9375, -0.4375]);
    }

    #[test]
    fn dry_mix_cannot_waive_unmeasured_controls() {
        let mut fx = BitCrusherCallback::new(2, 44_100.).unwrap();
        assert!(
            fx.configure(4775.6899, 5., 0.783333, -0.413333, 0.)
                .is_err()
        );
        assert!(
            fx.configure(25360.979, 8.7607965, 0.31089061, 0., 0.)
                .is_err()
        );
        assert!(fx.configure(1999., 5., 0., 0., 1.).is_err());
    }

    #[test]
    fn invalid_input_fails_before_mutation_and_bypass_is_exact() {
        let fx = BitCrusherCallback::new(2, 96_000.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 2];
        frames[0][0] = -1.;
        frames[1][1] = 4.1;
        let before = frames;
        assert!(fx.process(&mut frames, false).is_err());
        assert_eq!(frames, before);
        frames[1][1] = 4.;
        let before = frames;
        fx.process(&mut frames, true).unwrap();
        assert_eq!(frames, before);
    }
}
