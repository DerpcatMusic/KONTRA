//! Wow / Flutter (Kontakt 0x6A) as a generic tape-transport pitch wobble.
//!
//! This is a generic law, not a reproduction of the Kontakt kernel: the decompiled
//! Wow / Flutter core is not yet extracted (docs/re/engine/k-lofi-tape.md). Two
//! sine modulators move a fractional read of a short ring around a fixed base
//! delay. A peak pitch deviation `p` at modulation rate `f` needs an excursion of
//! `p / (2π f)` seconds, so wow (0.5 %, 0.5..2 Hz) and flutter (0.2 %, 6..12 Hz) are
//! set by their depths. The right channel's phase trails the left by `stereo / 2`
//! cycles. The output is the line's wet signal; the mix is the wet gain.
//! Scrape, age, saturation and gate are not modelled (reported by the mapper).
// ponytail: generic laws, no Kontakt measurement yet. Upgrade with the decompiled
// core or Kontakt vectors and replace the constants below.
use super::{Planar, ProcessorState, TimeMod};
use sampler_ir::fx::timemod::WowFlutter;
use std::f64::consts::TAU;

/// Base delay in ms, and the longest excursion the ring must hold.
const BASE_MS: f64 = 8.0;
const MAX_MS: f64 = 12.0;
/// Peak pitch deviation at full wow / flutter depth.
const WOW_PEAK: f64 = 0.005;
const FLUTTER_PEAK: f64 = 0.002;
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
        let wow_hz = 0.5 + 1.5 * f64::from(w.speed);
        let flutter_hz = 6. + 6. * f64::from(w.speed);
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
                let delay = base
                    + wow_frames * (TAU * (pw + shift)).sin()
                    + flutter_frames * (TAU * (pf + shift)).sin();
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
}
