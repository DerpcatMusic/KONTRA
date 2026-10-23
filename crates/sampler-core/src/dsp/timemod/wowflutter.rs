//! Wow / Flutter (Kontakt 0x6A) as a generic tape-transport pitch wobble.
//!
//! Generic pitch law under the resolved Kontakt controls. The controls arrive already
//! mapped (`wow_flutter_laws` in sampler-kontakt; docs/re/engine/k-wowflutter-kernel.md
//! pass 4): `wow` and `flutter` are depth multipliers, `speed` is the rate multiplier,
//! `stereo` the right-channel phase offset (squared control). Two sine modulators move
//! a fractional read of a short ring around a fixed base delay. A peak pitch deviation
//! `p` at modulation rate `f` needs an excursion of `p / (2π f)` seconds. The output is
//! the line's wet signal; the mix is the wet gain.
//! Scrape, age, saturation and gate are not modelled (reported by the mapper).
// ponytail: the pitch scale (peak constants, base rates, phase mapping) is still generic:
// the LFO entry init (+0x1c/+0x20/+0x68/+0x98) is unresolved. Excursions clamp to the
// ring, so a slow deep wow flattens instead of wrapping. Upgrade when the base rates
// are measured.
use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::WowFlutter;
use std::f64::consts::TAU;

/// Base delay in ms, and the longest excursion the ring must hold.
const BASE_MS: f64 = 8.0;
const MAX_MS: f64 = 12.0;
/// Peak pitch deviation at unit depth, and base modulation rates at unit speed (unconfirmed).
const WOW_PEAK: f64 = 0.005;
const FLUTTER_PEAK: f64 = 0.002;
const WOW_HZ: f64 = 1.0;
const FLUTTER_HZ: f64 = 8.0;
/// Samples kept beyond the longest read for the interpolation neighbour.
const GUARD: usize = 4;

/// Ring frames.
pub(super) fn ring(rate: f64) -> usize {
    (MAX_MS * 1e-3 * rate).ceil() as usize + GUARD
}

pub(super) fn capacity(rate: f64) -> usize {
    ring(rate)
}

impl TimeMod {
    pub(super) fn wow_flutter(
        &self,
        state: &mut ProcessorState,
        slab: &mut [[f64; 2]],
        block: &mut Planar,
        len: usize,
        w: WowFlutter,
    ) -> bool {
        let rate = self.rate;
        let n = ring(rate);
        let wow_hz = WOW_HZ * f64::from(w.speed);
        let flutter_hz = FLUTTER_HZ * f64::from(w.speed);
        // Excursions in frames: peak deviation / (2π f) seconds, times the rate.
        let wow_frames = f64::from(w.wow) * WOW_PEAK / (TAU * wow_hz) * rate;
        let flutter_frames = f64::from(w.flutter) * FLUTTER_PEAK / (TAU * flutter_hz) * rate;
        let base = BASE_MS * 1e-3 * rate;
        let stereo = f64::from(w.stereo) * 0.5;
        // aux: 0 wow phase, 1 flutter phase (cycles), 2 write position.
        let (mut pw, mut pf, mut write) = (state.aux[0], state.aux[1], state.aux[2] as usize);
        let mut finite = true;
        let [left, right] = block;
        for f in 0..len {
            if write == n {
                write = 0;
            }
            let x = [left[f], right[f]];
            slab[write] = x;
            let mut y = [0f64; 2];
            for (c, out) in y.iter_mut().enumerate() {
                let shift = c as f64 * stereo;
                // Clamped to the ring: a slow, deep excursion flattens rather than wrapping.
                let delay = (base
                    + wow_frames * (TAU * (pw + shift)).sin()
                    + flutter_frames * (TAU * (pf + shift)).sin())
                .clamp(0., (n - GUARD) as f64);
                let p = (write as f64 - delay).rem_euclid(n as f64);
                // The clamp guards a rounded `rem_euclid` that lands exactly on `n`.
                let i = (p as usize).min(n - 1);
                let j = if i + 1 == n { 0 } else { i + 1 };
                let frac = p - i as f64;
                *out = slab[i][c] + (slab[j][c] - slab[i][c]) * frac;
            }
            write += 1;
            pw = (pw + wow_hz / rate).fract();
            pf = (pf + flutter_hz / rate).fract();
            left[f] = y[0] * self.cfg.wet + self.cfg.dry * x[0];
            right[f] = y[1] * self.cfg.wet + self.cfg.dry * x[1];
            finite &= y[0].is_finite() && y[1].is_finite();
        }
        (state.aux[0], state.aux[1], state.aux[2]) = (pw, pf, write as f64);
        !finite
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::timemod::{Config, DelayTime, Lfo, LfoShape, Line, Native, Order, Rate};

    /// Zero depth is a pure delay of the base time (8 ms = 384 frames at 48 kHz).
    #[test]
    fn zero_depth_is_the_base_delay() {
        let mut line = Line::single(DelayTime::Ms(0.), 2000.);
        line.native = Native::WowFlutter(WowFlutter {
            wow: 0.,
            flutter: 0.,
            speed: 0.5,
            stereo: 0.,
        });
        let cfg = Config {
            address: None,
            line: Some(line),
            allpass: None,
            order: Order::LineThenAllpass,
            lfo: Lfo {
                shape: LfoShape::Sine,
                rate: Rate::Hz(0.),
                phase: 0.,
            },
            dry: 0.,
            wet: 1.,
        };
        let mut x = vec![[0.0; 2]; 400];
        x[0] = [1.0, -0.5];
        let (y, fault) = super::super::tests::run(cfg, 48_000, 120., &x, 64);
        assert!(!fault);
        assert!(
            (y[384][0] - 1.).abs() < 1e-6 && (y[384][1] + 0.5).abs() < 1e-6,
            "{:?}",
            y[384]
        );
        assert!(y[383][0].abs() < 1e-6 && y[385][0].abs() < 1e-6);
    }

    /// The law has no gain of its own: the output is `mix * wet_read + (1 - mix) * dry`
    /// and each read is a linear interpolation of earlier inputs, so no depth or mix
    /// can push a sine's peak above its input's. A +23 dB insert gain is not here.
    #[test]
    fn wow_flutter_adds_no_gain_at_any_mix() {
        const RATE: u32 = 48_000;
        let x: Vec<[f64; 2]> = (0..RATE as usize)
            .map(|n| {
                let s = 0.5 * (TAU * 440. * n as f64 / f64::from(RATE)).sin();
                [s, s]
            })
            .collect();
        for mix in [0.0, 0.5, 1.0] {
            let mut line = Line::single(DelayTime::Ms(0.), 2000.);
            line.native = Native::WowFlutter(WowFlutter {
                wow: 1.,
                flutter: 1.,
                speed: 1.,
                stereo: 1.,
            });
            let cfg = Config {
                address: None,
                line: Some(line),
                allpass: None,
                order: Order::LineThenAllpass,
                lfo: Lfo {
                    shape: LfoShape::Sine,
                    rate: Rate::Hz(0.),
                    phase: 0.,
                },
                dry: 1. - mix,
                wet: mix,
            };
            let (y, fault) = super::super::tests::run(cfg, RATE, 120., &x, 64);
            assert!(!fault);
            let peak = y.iter().flatten().fold(0f64, |p, v| p.max(v.abs()));
            assert!(
                peak <= 0.5 + 1e-9,
                "mix {mix}: peak {peak} exceeds the input peak 0.5"
            );
            if mix == 1. {
                // A delayed sine keeps its level; linear interpolation at 440 Hz costs ~nothing.
                let tail = &y[RATE as usize / 4..];
                let rms =
                    (tail.iter().map(|f| f[0] * f[0]).sum::<f64>() / tail.len() as f64).sqrt();
                let input_rms = 0.5 / 2f64.sqrt();
                assert!(
                    (rms / input_rms - 1.).abs() < 0.02,
                    "wet-only rms {rms} vs input {input_rms}"
                );
            }
        }
    }
}
