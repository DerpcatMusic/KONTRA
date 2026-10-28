//! A four-pole ladder in Huovilainen's explicit form: one saturator on the
//! input, four one-poles `y = (1 − g)·y₋₁ + g·(x + 0.3·x₋₁)/1.3` in series,
//! the last fed back a sample late, the output a weighted mix of the taps.
//!
//! The law is UVIWorkstation's XpanderFilter (core 0x1412c7f40, coefficients
//! 0x1412cbc30, tap table filled by 0x140842a10):
//! - `g = 0.9892ω − 0.4242ω² + 0.1381ω³ − 0.0202ω⁴` and the feedback gain
//!   `G = R·(1.0029 + 0.0526ω − 0.0926ω² + 0.0218ω³)`, `ω` the cutoff in
//!   radians per internal sample (at most π), `R` the resonance 0..=1;
//! - the input `u = x·(1 + 4G·fat) − 4G·y₄₋₁` through
//!   `T(u) = u(u² + 27)/(9u² + 27)`;
//! - the input gain `d` ahead of it and `1/T(d)` after it;
//! - twice the rate inside at 48 kHz and below, through a halfband pair
//!   (the native's is 0x14163cdc0 up, 0x14163cec0 down).

use kontra_dsp_core::{Halfband, flush};

/// Per mode (low 1..4 poles, high 1..4, band 2 and 4, notch 2 and 4): the
/// weights of the saturated input and the four poles.
#[rustfmt::skip]
const WEIGHTS: [[f64; 5]; 12] = [
    [0., 1., 0., 0., 0.], [0., 0., 1., 0., 0.], [0., 0., 0., 1., 0.], [0., 0., 0., 0., 1.],
    [1., -1., 0., 0., 0.], [1., -2., 1., 0., 0.], [1., -3., 3., -1., 0.], [1., -4., 6., -4., 1.],
    [0., 2., -2., 0., 0.], [0., 0., 4., -8., 4.], [1., -2., 2., 0., 0.], [1., -4., 8., -8., 4.],
];

/// The input saturator, a Padé approximant of tanh.
fn saturate(u: f64) -> f64 {
    let u2 = u * u;
    u * (u2 + 27.0) / (9.0 * u2 + 27.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Huovilainen {
    weights: [f64; 5],
    fat: f64,
    g: f64,
    feedback: f64,
    /// Twice the rate inside, at 48 kHz and below.
    halfband: Option<Halfband>,
    /// Per channel: the four one-poles' states, then the last output.
    state: [[f64; 5]; 2],
}

impl Huovilainen {
    /// Mode `taps` 1..=12, held in range; `fat` scales the input by the
    /// feedback so the passband holds as resonance rises (1 holds it).
    #[must_use]
    pub fn new(taps: u8, fat: f32) -> Self {
        Self {
            weights: WEIGHTS[usize::from(taps.clamp(1, 12) - 1)],
            fat: f64::from(fat),
            g: 0.0,
            feedback: 0.0,
            halfband: None,
            state: [[0.0; 5]; 2],
        }
    }

    /// Moves the cutoff to `hz` at `rate` and the resonance to `r` (0..=1).
    pub fn tune(&mut self, hz: f64, rate: f64, r: f64) {
        let rate = rate.max(1.0);
        if rate > 48_000.0 {
            self.halfband = None;
        } else if self.halfband.is_none() {
            self.halfband = Some(Halfband::new(rate as u32));
        }
        let inner = if self.halfband.is_some() {
            2.0 * rate
        } else {
            rate
        };
        let w = std::f64::consts::TAU * (hz / inner).min(0.5);
        let (w2, w3) = (w * w, w * w * w);
        self.g = 0.9892 * w - 0.4242 * w2 + 0.1381 * w3 - 0.0202 * w2 * w2;
        self.feedback = r.clamp(0.0, 1.0) * (1.0029 + 0.0526 * w - 0.0926 * w2 + 0.0218 * w3);
    }

    /// Filters `io` in place after the input `gain`.
    pub fn process(&mut self, io: &mut [[f32; 2]], gain: f64) {
        let (g1, g2, h) = (self.g / 1.3, 0.3 * self.g / 1.3, 1.0 - self.g);
        let k = 4.0 * self.feedback;
        let lift = 1.0 + k * self.fat;
        let w = self.weights;
        let state = &mut self.state;
        let mut run = |frame: [f32; 2]| {
            let mut out = [0.0; 2];
            for ((o, x), s) in out.iter_mut().zip(frame).zip(state.iter_mut()) {
                let t = saturate(f64::from(x) * lift - k * s[4]);
                let (mut mix, mut v) = (w[0] * t, t);
                for (si, wi) in s[..4].iter_mut().zip(&w[1..]) {
                    let y = v * g1 + *si;
                    *si = h * y + v * g2;
                    v = y;
                    mix += wi * y;
                }
                s[4] = v;
                *o = mix as f32;
            }
            out
        };
        let input = gain as f32;
        for frame in io.iter_mut() {
            *frame = frame.map(|x| x * input);
        }
        match &mut self.halfband {
            Some(halfband) => halfband.process(io, run),
            None => io.iter_mut().for_each(|frame| *frame = run(*frame)),
        }
        let makeup = (1.0 / saturate(gain).max(1e-9)) as f32;
        for frame in io.iter_mut() {
            *frame = frame.map(|x| x * makeup);
        }
        flush(self.state.as_flattened_mut());
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.state.iter().flatten().all(|s| s.abs() < 1e-10)
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 5]; 2];
        if let Some(halfband) = &mut self.halfband {
            halfband.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The level of a `hz` sine through `f` once settled, against the input.
    fn gain_at(f: &mut Huovilainen, hz: f64, rate: f64) -> f64 {
        let n = (rate as usize) / 2;
        let mut peak = 0.0f64;
        for i in 0..n {
            let x = (std::f64::consts::TAU * hz * i as f64 / rate).sin() * 0.01;
            let mut io = [[x as f32; 2]];
            f.process(&mut io, 1.0);
            if i > n / 2 {
                peak = peak.max(f64::from(io[0][0]).abs());
            }
        }
        peak / 0.01
    }

    #[test]
    fn a_low_pass_four_passes_the_lows_and_falls_24_db_an_octave() {
        let rate = 48_000.0;
        let mut f = Huovilainen::new(4, 1.0);
        f.tune(1_000.0, rate, 0.0);
        // Small signals: the saturator is linear, so the makeup's 1/T(1) shows.
        let low = gain_at(&mut f, 30.0, rate);
        assert!((low - 36.0 / 28.0).abs() < 0.02, "{low}");
        f.reset();
        let (a, b) = (gain_at(&mut f, 4_000.0, rate), {
            f.reset();
            gain_at(&mut f, 8_000.0, rate)
        });
        let slope = 20.0 * (a / b).log10();
        assert!((slope - 24.0).abs() < 3.0, "{slope} dB/oct");
    }

    #[test]
    fn resonance_peaks_at_the_cutoff_and_fat_holds_the_passband() {
        let rate = 48_000.0;
        let mut thin = Huovilainen::new(4, 0.0);
        thin.tune(1_000.0, rate, 0.8);
        let (pass, peak) = (gain_at(&mut thin, 100.0, rate), {
            thin.reset();
            gain_at(&mut thin, 1_000.0, rate)
        });
        assert!(peak > 2.0 * pass, "{pass} {peak}");
        let mut fat = Huovilainen::new(4, 1.0);
        fat.tune(1_000.0, rate, 0.8);
        let held = gain_at(&mut fat, 100.0, rate);
        assert!((held - 36.0 / 28.0).abs() < 0.05, "{held}");
    }
}
