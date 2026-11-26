//! A four-pole ladder in Huovilainen's explicit form: one saturator on the
//! input, four one-poles `y = (1 − g)·y₋₁ + g·(x + 0.3·x₋₁)/1.3` in series,
//! the last fed back a sample late, the output a weighted mix of the taps.
//!
//! The law is UVIWorkstation's XpanderFilter (core [ref], coefficients
//! [ref], tap table filled by [ref]):
//! - `g = 0.9892ω − 0.4242ω² + 0.1381ω³ − 0.0202ω⁴` and the feedback gain
//!   `G = R·(1.0029 + 0.0526ω − 0.0926ω² + 0.0218ω³)`, `ω` the cutoff in
//!   radians per internal sample (at most π), `R` the resonance 0..=1;
//! - the input `u = x·(1 + 4G·fat) − 4G·y₄₋₁` through
//!   `T(u) = u(u² + 27)/(9u² + 27)`;
//! - the input gain `d` ahead of it and `1/T(d)` after it;
//! - twice the rate inside at 48 kHz and below, through a halfband pair
//!   (the native's is [ref] up, [ref] down).

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
            weights: WEIGHTS
                .get(usize::from(taps.clamp(1, 12) - 1))
                .copied()
                .unwrap_or_default(),
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
        if io.is_empty() {
            return;
        }
        if self.state.iter().flatten().all(|&s| s == 0.0)
            && self.halfband.as_ref().is_none_or(Halfband::is_silent)
            && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
        {
            return;
        }
        let (g1, g2, h) = (self.g / 1.3, 0.3 * self.g / 1.3, 1.0 - self.g);
        let k = 4.0 * self.feedback;
        let lift_in = (1.0 + k * self.fat) * gain;
        let makeup = 1.0 / saturate(gain).max(1e-9);
        let [w0, w1, w2, w3, w4] = self.weights;
        let [s_l, s_r] = &mut self.state;
        let [mut l0, mut l1, mut l2, mut l3, mut l4] = *s_l;
        let [mut r0, mut r1, mut r2, mut r3, mut r4] = *s_r;
        let mut run = |frame: [f32; 2]| {
            let tl = saturate(f64::from(frame[0]) * lift_in - k * l4);
            let tr = saturate(f64::from(frame[1]) * lift_in - k * r4);

            let yl0 = tl * g1 + l0;
            let yr0 = tr * g1 + r0;
            l0 = h * yl0 + tl * g2;
            r0 = h * yr0 + tr * g2;

            let yl1 = yl0 * g1 + l1;
            let yr1 = yr0 * g1 + r1;
            l1 = h * yl1 + yl0 * g2;
            r1 = h * yr1 + yr0 * g2;

            let yl2 = yl1 * g1 + l2;
            let yr2 = yr1 * g1 + r2;
            l2 = h * yl2 + yl1 * g2;
            r2 = h * yr2 + yr1 * g2;

            let yl3 = yl2 * g1 + l3;
            let yr3 = yr2 * g1 + r3;
            l3 = h * yl3 + yl2 * g2;
            r3 = h * yr3 + yr2 * g2;

            l4 = yl3;
            r4 = yr3;

            let ml = w0 * tl + w1 * yl0 + w2 * yl1 + w3 * yl2 + w4 * yl3;
            let mr = w0 * tr + w1 * yr0 + w2 * yr1 + w3 * yr2 + w4 * yr3;
            [(ml * makeup) as f32, (mr * makeup) as f32]
        };
        match &mut self.halfband {
            Some(halfband) => halfband.process(io, &mut run),
            None => io.iter_mut().for_each(|frame| *frame = run(*frame)),
        }
        *s_l = [l0, l1, l2, l3, l4];
        *s_r = [r0, r1, r2, r3, r4];
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
