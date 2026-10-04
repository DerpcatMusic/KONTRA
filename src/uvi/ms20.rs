//! Original implicit-midpoint mathematics for the MS20 scalar signal leaf.
//!
//! Official identity/controls: https://lua.uvi.net/_elements.html#vcf-20 and
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf
//! Authored native helper comparisons on 2026-10-04 establish the nonlinear
//! two-integrator circuit, two internal steps and fixed Trim=0.5 behavior.
//! This leaf is deliberately not registered with Program playback: the host's
//! connected controls have not been measured. An explicit physical-point
//! dispatcher covers the measured outer 32-frame hold/bypass boundary.

use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{ensure, Result};

pub const FIDELITY_DIAGNOSTIC: &str = "MS20 scalar signal helper only: authored nonlinear cold/warm comparisons at 8/32/44.1/48/96/192 kHz and direct Freq/Q/Morph/Voltage field transitions at 32/48/96 kHz are float32-identical; Trim is fixed at 0.5, KeyTracking at zero, and Explicit physical Freq/Q points use measured 32-frame holds and bypass/resume behavior at 32/44.1/48/96 kHz. Native SIMD multiply dispatch was replaced with scalar float32 primitives in the isolated helper oracle. Control generation/smoothing, connected controls, other trim/tracking and whole-program audio remain unverified; Program playback does not admit this leaf";

#[derive(Clone, Copy, Default)]
struct State {
    integrators: [f64; 2],
    delta: [f64; 2],
    input: [f64; 2],
}

/// Helper-boundary DSP, without a claim about the surrounding native host.
pub struct Ms20Scalar {
    channels: usize,
    rate: f64,
    step: f64,
    feedback: f64,
    morph: f32,
    voltage: f64,
    state: [State; MAX_CHANNELS],
}

impl Ms20Scalar {
    pub fn new(channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "MS20 needs 1..12 channels"
        );
        ensure!(
            [8_000., 32_000., 44_100., 48_000., 96_000., 192_000.].contains(&rate),
            "MS20 scalar helper rate has no authored native comparison"
        );
        let mut result = Self {
            channels,
            rate,
            step: 0.,
            feedback: 0.,
            morph: 1.,
            voltage: 1.,
            state: [State::default(); MAX_CHANNELS],
        };
        // The measured Workstation factory starts with the LP endpoint.
        result.configure(1_000., 0., 1., 1.)?;
        Ok(result)
    }

    /// Direct scalar signal fields; this is not a hosted parameter setter.
    /// Keep integrator/input history across edits, including voltage edits.
    pub fn configure(&mut self, freq: f64, q: f64, morph: f64, voltage: f64) -> Result<()> {
        for (name, value, low, high) in [
            ("Freq", freq, 20., 20_000.),
            ("Q", q, 0., 1.),
            ("Morph", morph, 0., 1.),
            ("ReferenceVoltage", voltage, 0.5, 5.),
        ] {
            ensure!(
                value.is_finite() && (low..=high).contains(&value),
                "Invalid MS20 {name}"
            );
        }
        let cutoff = (freq as f32).min(self.rate as f32 * 0.5);
        self.step = std::f64::consts::PI * f64::from(cutoff) / self.rate;
        self.feedback = f64::from(q as f32) * 10_000. / 18_200.;
        self.morph = morph as f32;
        self.voltage = f64::from(voltage as f32);
        Ok(())
    }

    /// Consume caller-prepared physical Freq/Q points, one per 32 frames.
    /// This does not generate or smooth controls. A bypassed buffer advances
    /// the selected control to its final interval while freezing audio state.
    pub fn process_control_points(
        &mut self,
        frames: &mut [Frame],
        freq: &[f32],
        q: &[f32],
        bypass: bool,
    ) -> Result<()> {
        ensure!(
            [32_000., 44_100., 48_000., 96_000.].contains(&self.rate),
            "MS20 control-point dispatcher rate has no authored native comparison"
        );
        let count = frames.len().div_ceil(32);
        ensure!(
            freq.len() == count && q.len() == count,
            "Invalid MS20 control-point count"
        );
        ensure!(
            freq.iter()
                .all(|v| v.is_finite() && (20.0..=20_000.0).contains(v)),
            "Invalid MS20 Freq points"
        );
        ensure!(
            q.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Invalid MS20 Q points"
        );
        if bypass {
            if let Some(index) = count.checked_sub(1) {
                self.configure(
                    f64::from(freq[index]),
                    f64::from(q[index]),
                    f64::from(self.morph),
                    self.voltage,
                )?;
            }
            return Ok(());
        }
        for ((chunk, freq), q) in frames.chunks_mut(32).zip(freq).zip(q) {
            self.configure(
                f64::from(*freq),
                f64::from(*q),
                f64::from(self.morph),
                self.voltage,
            )?;
            self.process(chunk)?;
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.state.fill(State::default());
    }

    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|f| f[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite MS20 input"
        );
        for frame in frames {
            for (sample, state) in frame[..self.channels].iter_mut().zip(&mut self.state) {
                let input = [
                    f64::from(*sample * self.morph) * self.voltage,
                    f64::from(*sample * (self.morph - 1.)) * self.voltage,
                ];
                let mut output = 0.;
                for _ in 0..2 {
                    let lp = (input[0] + state.input[0]) * 0.5;
                    let hp = (input[1] + state.input[1]) * 0.5;
                    let [mut dx, mut dy] = state.delta;
                    for _ in 0..20 {
                        let a = state.integrators[0] + dx * 0.5;
                        let b = state.integrators[1] + dy * 0.5;
                        let drive = (hp + b) * self.feedback;
                        // Authored scalar coefficient probes at Trim=0.5.
                        // This Padé saturation and its derivative are ordinary
                        // rational mathematics, not an exact tanh substitute.
                        let z = 1.749079 * drive;
                        let z2 = z * z;
                        let feedback = drive + 2.065561 * z * (27. + z2) / (27. + 9. * z2);
                        let slope = self.feedback
                            * (1.
                                + 2.065561 * 1.749079 * ((z2 - 18.) * z2 + 81.)
                                    / ((9. * z2 + 54.) * z2 + 81.));
                        let r1 = dx - self.step * (lp - feedback - a);
                        let r2 = dy - self.step * (-hp + feedback - b + a);
                        let j11 = 1. + self.step * 0.5;
                        let j12 = self.step * slope * 0.5;
                        let j21 = -self.step * 0.5;
                        let j22 = 1. + self.step * 0.5 - self.step * slope * 0.5;
                        let determinant = j11 * j22 - j12 * j21;
                        let cx = (j22 * r1 - j12 * r2) / determinant;
                        let cy = (j11 * r2 - j21 * r1) / determinant;
                        // Native tolerance stops before the final correction.
                        if cx.abs().max(cy.abs()) < 1e-6 {
                            break;
                        }
                        dx -= cx;
                        dy -= cy;
                    }
                    state.delta = [dx, dy];
                    output += hp + state.integrators[1] + dy * 0.5;
                    state.integrators[0] += dx;
                    state.integrators[1] += dy;
                    state.input = input;
                }
                *sample = (output / (2. * self.voltage)) as f32;
                ensure!(sample.is_finite(), "Nonfinite MS20 output");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_scalar_impulse_and_state_contract() {
        let mut filter = Ms20Scalar::new(2, 48_000.).unwrap();
        filter.configure(1_000., 0.5, 1., 1.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 16];
        frames[0][0] = 1.;
        filter.process(&mut frames[..7]).unwrap();
        filter.process(&mut frames[7..]).unwrap();
        // Native output on the original authored unit impulse; no bank audio.
        let expected: [f32; 16] = [
            0.0018150009,
            0.012381887,
            0.027077328,
            0.040236998,
            0.051537443,
            0.06096409,
            0.068534344,
            0.07429441,
            0.07831588,
            0.080692224,
            0.08153527,
            0.08097162,
            0.07913923,
            0.07618411,
            0.07225717,
            0.06751131,
        ];
        for (frame, expected) in frames.iter().zip(expected) {
            assert_eq!(frame[0].to_bits(), expected.to_bits());
            assert_eq!(frame[1], 0., "channel state must remain independent");
        }
        assert!(filter.configure(f64::NAN, 0.5, 1., 1.).is_err());
        assert!(Ms20Scalar::new(1, 64_000.).is_err());
        filter.reset();
        let mut reset = [[0.; MAX_CHANNELS]; 16];
        reset[0][0] = 1.;
        filter.process(&mut reset).unwrap();
        assert_eq!(frames, reset);
    }
}
