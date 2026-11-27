//! Console pass-filter bank: 3rd-order (18 dB/oct Butterworth) high-pass
//! cascaded with an analog-matched 2nd-order (12 dB/oct Butterworth) low-pass.
//! Built directly on [`kontra_dsp_core::Biquad`] with zero heap allocation.

use std::f64::consts::FRAC_1_SQRT_2;

use kontra_dsp_core::{Biquad, first_order_hp, second_order_hp, vicanek_lp};

/// Settings for [`PassBank`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassBankSettings {
    pub hp_on: bool,
    pub hp_hz: f32,
    pub lp_on: bool,
    pub lp_hz: f32,
}

impl Default for PassBankSettings {
    fn default() -> Self {
        Self {
            hp_on: false,
            hp_hz: 20.0,
            lp_on: false,
            lp_hz: 20_000.0,
        }
    }
}

/// 18 dB/oct Butterworth HP (1st + 2nd order) and 12 dB/oct Vicanek LP.
#[derive(Clone, Debug, PartialEq)]
pub struct PassBank {
    rate: u32,
    hp_on: bool,
    lp_on: bool,
    hp1: Biquad,
    hp2: Biquad,
    lp: Biquad,
}

impl PassBank {
    #[must_use]
    pub fn new(settings: PassBankSettings, rate: u32) -> Self {
        let mut bank = Self {
            rate: rate.max(1),
            hp_on: false,
            lp_on: false,
            hp1: Biquad::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            hp2: Biquad::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            lp: Biquad::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        };
        bank.set(settings);
        bank
    }

    pub fn set(&mut self, s: PassBankSettings) {
        self.hp_on = s.hp_on;
        self.lp_on = s.lp_on;
        if s.hp_on && s.hp_hz.is_finite() {
            // 3rd-order Butterworth HP = 1st-order + 2nd-order (Q = 1.0).
            self.hp1.retune(&first_order_hp(s.hp_hz, self.rate));
            self.hp2.retune(&second_order_hp(s.hp_hz, 1.0, self.rate));
        }
        if s.lp_on && s.lp_hz.is_finite() {
            // 2nd-order Butterworth LP (Q = 1 / sqrt(2)), analog-matched at Nyquist.
            self.lp
                .retune(&vicanek_lp(s.lp_hz, FRAC_1_SQRT_2, self.rate));
        }
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if self.hp_on {
            self.hp1.process(io);
            self.hp2.process(io);
        }
        if self.lp_on {
            self.lp.process(io);
        }
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        (!self.hp_on || (self.hp1.is_silent() && self.hp2.is_silent()))
            && (!self.lp_on || self.lp.is_silent())
    }

    pub fn reset(&mut self) {
        self.hp1.reset();
        self.hp2.reset();
        self.lp.reset();
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_1_SQRT_2, PI};

    use super::*;

    fn sine_gain_db(filter: &mut PassBank, hz: f64, rate: u32) -> f64 {
        filter.reset();
        let n = 4096usize;
        let settle = 2048usize;
        let mut buf: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let s = (2.0 * PI * hz * i as f64 / f64::from(rate)).sin() as f32;
                [s, 0.0]
            })
            .collect();
        filter.process(&mut buf);
        assert!(buf.iter().all(|f| f[1] == 0.0));
        let rms_out = (buf
            .iter()
            .skip(settle)
            .map(|f| f64::from(f[0]) * f64::from(f[0]))
            .sum::<f64>()
            / (n - settle) as f64)
            .sqrt();
        20.0 * (rms_out / FRAC_1_SQRT_2).max(1e-9).log10()
    }

    #[test]
    fn pass_bank_matches_18db_hp_and_12db_lp_slopes() {
        let rate = 48_000;
        let mut hp = PassBank::new(
            PassBankSettings {
                hp_on: true,
                hp_hz: 286.0,
                lp_on: false,
                lp_hz: 20_000.0,
            },
            rate,
        );
        let db60 = sine_gain_db(&mut hp, 60.0, rate);
        let db120 = sine_gain_db(&mut hp, 120.0, rate);
        let db200 = sine_gain_db(&mut hp, 200.0, rate);
        let db1000 = sine_gain_db(&mut hp, 1000.0, rate);
        assert!((db120 - db60 - 18.0).abs() < 1.0);
        assert!((db200 - (-10.07)).abs() < 1.0);
        assert!(db1000.abs() < 0.25);

        let mut lp = PassBank::new(
            PassBankSettings {
                hp_on: false,
                hp_hz: 20.0,
                lp_on: true,
                lp_hz: 3_270.0,
            },
            rate,
        );
        let db200_lp = sine_gain_db(&mut lp, 200.0, rate);
        let db2k = sine_gain_db(&mut lp, 2_000.0, rate);
        let db5k = sine_gain_db(&mut lp, 5_000.0, rate);
        let db10k = sine_gain_db(&mut lp, 10_000.0, rate);
        let db15k = sine_gain_db(&mut lp, 15_000.0, rate);
        assert!(db200_lp.abs() < 0.1);
        assert!((db2k - (-0.55)).abs() < 0.4);
        assert!((db5k - (-8.40)).abs() < 0.4);
        assert!((db10k - (-19.56)).abs() < 0.4);
        assert!((db15k - (-26.78)).abs() < 0.4);
    }
}
