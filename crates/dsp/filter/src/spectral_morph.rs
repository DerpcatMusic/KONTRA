//! Dynamic spectral/formant morphing filter.
//!
//! Interpolates a 4-band resonant spectral envelope across acoustic formant
//! profiles controlled by `morph` (`0..=1`) and depth `amount` (`0..=1`).
//! Pre-allocated, zero heap allocations on the audio thread, denormal-safe.

use std::f64::consts::TAU;

use kontra_dsp_core::Biquad;

/// Physical settings for [`SpectralMorph`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectralMorphSettings {
    /// Morph position across spectral profiles (`0.0..=1.0`).
    pub morph: f32,
    /// Wet morph depth (`0.0..=1.0`).
    pub amount: f32,
    /// Spectral profile bank (`0..=3`).
    pub bank: u8,
    /// Optional stereo phase/formant offset tracking.
    pub tracking: bool,
}

/// Four resonant formant bands `[hz, q, gain_db]` per anchor profile;
/// four banks × four morph anchors.
const BANKS: [[[[f32; 3]; 4]; 4]; 4] = [
    // Bank 0: vocal / brass continuum (dark -> open -> bright -> nasal)
    [
        [
            [280.0, 2.2, 7.0],
            [720.0, 2.8, 5.0],
            [2100.0, 3.0, -5.0],
            [4200.0, 2.0, -8.0],
        ],
        [
            [460.0, 2.4, 5.5],
            [1150.0, 3.2, 6.5],
            [2600.0, 3.2, 3.0],
            [4800.0, 2.4, -2.0],
        ],
        [
            [680.0, 2.5, 4.0],
            [1650.0, 3.5, 7.5],
            [3100.0, 3.4, 6.0],
            [5600.0, 2.2, 3.0],
        ],
        [
            [340.0, 3.0, -4.0],
            [1950.0, 4.2, 9.0],
            [3400.0, 3.8, 5.0],
            [6200.0, 2.5, -3.0],
        ],
    ],
    // Bank 1: woodwind / string body continuum
    [
        [
            [220.0, 2.0, 6.5],
            [560.0, 2.5, 4.5],
            [1400.0, 2.8, -4.0],
            [3600.0, 2.0, -7.0],
        ],
        [
            [350.0, 2.2, 4.0],
            [920.0, 3.0, 6.0],
            [2200.0, 3.0, 4.5],
            [4500.0, 2.2, 1.0],
        ],
        [
            [500.0, 2.4, 2.0],
            [1350.0, 3.2, 5.5],
            [2900.0, 3.5, 7.0],
            [5500.0, 2.5, 4.5],
        ],
        [
            [750.0, 2.6, -2.0],
            [1800.0, 3.8, 6.5],
            [3800.0, 3.6, 8.0],
            [7200.0, 2.2, 5.0],
        ],
    ],
    // Bank 2: percussive / metallic resonant continuum
    [
        [
            [180.0, 2.8, 8.0],
            [640.0, 3.5, -5.0],
            [1600.0, 3.8, 6.0],
            [4000.0, 3.0, -4.0],
        ],
        [
            [320.0, 3.0, 5.0],
            [1100.0, 4.0, 7.0],
            [2400.0, 4.2, -3.0],
            [5200.0, 3.2, 6.0],
        ],
        [
            [520.0, 3.2, -3.0],
            [1550.0, 4.2, 6.0],
            [3300.0, 4.5, 8.0],
            [6800.0, 3.0, 4.0],
        ],
        [
            [880.0, 3.5, 6.0],
            [2200.0, 4.5, -4.0],
            [4400.0, 4.8, 8.5],
            [8400.0, 3.0, 6.0],
        ],
    ],
    // Bank 3: warm spectral tilt continuum
    [
        [
            [160.0, 1.5, 7.0],
            [600.0, 1.8, 4.0],
            [2000.0, 1.8, -5.0],
            [6000.0, 1.5, -9.0],
        ],
        [
            [260.0, 1.6, 4.0],
            [950.0, 2.0, 3.0],
            [2500.0, 2.0, 1.0],
            [6200.0, 1.6, -3.0],
        ],
        [
            [380.0, 1.6, -2.0],
            [1300.0, 2.2, 4.0],
            [3200.0, 2.2, 5.0],
            [6800.0, 1.6, 4.0],
        ],
        [
            [520.0, 1.8, -6.0],
            [1700.0, 2.5, 5.0],
            [4200.0, 2.5, 7.5],
            [8000.0, 1.8, 8.0],
        ],
    ],
];

fn peak(hz: f32, q: f32, gain_db: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.45 * r);
    let w = TAU * f / r;
    let a = 10.0f64.powf(f64::from(gain_db) / 40.0);
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.15));
    let cos_w = w.cos();
    Biquad::new([
        1.0 + alpha * a,
        -2.0 * cos_w,
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * cos_w,
        1.0 - alpha / a,
    ])
}

#[inline]
fn flush32(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

/// Dynamic spectral/formant morphing filter.
#[derive(Debug)]
pub struct SpectralMorph {
    s: SpectralMorphSettings,
    rate: u32,
    bands: [Biquad; 4],
    amount_cur: f32,
    smooth: f32,
}

impl SpectralMorph {
    #[must_use]
    pub fn new(s: SpectralMorphSettings, rate: u32) -> Self {
        let rate = rate.max(8_000);
        let smooth = 1.0 - (-1.0 / (0.010 * rate as f32)).exp();
        let mut m = Self {
            s,
            rate,
            bands: std::array::from_fn(|_| peak(1000.0, 2.0, 0.0, rate)),
            amount_cur: s.amount.clamp(0.0, 1.0),
            smooth,
        };
        m.retune(s);
        m
    }

    pub fn set(&mut self, s: SpectralMorphSettings) {
        self.s = s;
        self.retune(s);
    }

    fn retune(&mut self, s: SpectralMorphSettings) {
        let bank = BANKS
            .get(usize::from(s.bank) % BANKS.len())
            .unwrap_or(&BANKS[0]);
        let pos = s.morph.clamp(0.0, 1.0) * 3.0;
        let lo = (pos.floor() as usize).min(2);
        let hi = (lo + 1).min(3);
        let frac = pos - lo as f32;
        let track_shift = if s.tracking { 1.06 } else { 1.0 };

        let row_a = bank.get(lo).unwrap_or(&bank[0]);
        let row_c = bank.get(hi).unwrap_or(&bank[0]);
        for ((b, &a), &c) in self.bands.iter_mut().zip(row_a.iter()).zip(row_c.iter()) {
            // Log-frequency interpolation for smooth formant sweeps.
            let hz = a[0] * (c[0] / a[0].max(1.0)).powf(frac) * track_shift;
            let q = a[1] + (c[1] - a[1]) * frac;
            let db = a[2] + (c[2] - a[2]) * frac;
            b.retune(&peak(hz, q, db, self.rate));
        }
    }

    /// Processes stereo audio in place with zero allocations.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        let amt_tgt = self.s.amount.clamp(0.0, 1.0);
        let smooth = self.smooth;

        for chunk in io.chunks_mut(64) {
            let mut dry = [[0.0f32; 2]; 64];
            let n = chunk.len();
            for (dst, &src) in dry.iter_mut().zip(chunk.iter()) {
                *dst = src;
            }

            for band in &mut self.bands {
                band.process(chunk);
            }

            for (frame, d) in chunk.iter_mut().zip(dry.iter().take(n)) {
                self.amount_cur += (amt_tgt - self.amount_cur) * smooth;
                let w = self.amount_cur;
                let inv = 1.0 - w;
                frame[0] = flush32(inv * d[0] + w * frame[0]);
                frame[1] = flush32(inv * d[1] + w * frame[1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_sweeps_spectral_response_smoothly() {
        let make = |morph: f32| {
            let mut f = SpectralMorph::new(
                SpectralMorphSettings {
                    morph,
                    amount: 1.0,
                    bank: 0,
                    tracking: false,
                },
                48_000,
            );
            let mut io: Vec<[f32; 2]> = (0..1024)
                .map(|n| [(std::f32::consts::TAU * 1650.0 * n as f32 / 48_000.0).sin() * 0.4; 2])
                .collect();
            f.process(&mut io);
            let e: f32 = io[256..].iter().map(|[l, _]| l * l).sum();
            (e / 768.0).sqrt()
        };
        let dark = make(0.0);
        let bright = make(0.67);
        assert!(
            bright > dark * 1.4,
            "expected 1650 Hz boost at bright morph: dark={dark} bright={bright}"
        );
    }
}
