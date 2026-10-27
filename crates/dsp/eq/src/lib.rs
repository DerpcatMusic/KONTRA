//! Equalisers: one biquad band shape per class (03-DSP.md §3). Formats load
//! their own laws into these shapes; the shapes know no vendor.

/// A peaking band: `gain_db` at `hz`, unity far from it, width set by `q`
/// (Robert Bristow-Johnson's cookbook bell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bell {
    pub hz: f32,
    pub q: f32,
    pub gain_db: f32,
}

/// One stereo biquad section, transposed direct form II, f64 state so
/// low bands stay clean at high rates.
#[derive(Clone, Debug, PartialEq)]
pub struct Biquad {
    /// b0, b1, b2, a1, a2 with a0 = 1.
    c: [f64; 5],
    state: [[f64; 2]; 2],
}

impl Biquad {
    /// The bell at `rate`; its centre is held below 0.49 of the rate.
    #[must_use]
    pub fn bell(bell: Bell, rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        let hz = f64::from(bell.hz).clamp(1.0, 0.49 * rate);
        let w = std::f64::consts::TAU * hz / rate;
        let a = 10f64.powf(f64::from(bell.gain_db) / 40.0);
        let alpha = w.sin() / (2.0 * f64::from(bell.q).max(1e-3));
        let a0 = 1.0 + alpha / a;
        let c = [
            (1.0 + alpha * a) / a0,
            -2.0 * w.cos() / a0,
            (1.0 - alpha * a) / a0,
            -2.0 * w.cos() / a0,
            (1.0 - alpha / a) / a0,
        ];
        Self {
            c,
            state: [[0.0; 2]; 2],
        }
    }

    /// Filters both channels of `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let [b0, b1, b2, a1, a2] = self.c;
        for frame in io {
            for (x, s) in frame.iter_mut().zip(&mut self.state) {
                let input = f64::from(*x);
                let y = b0 * input + s[0];
                s[0] = b1 * input - a1 * y + s[1];
                s[1] = b2 * input - a2 * y;
                *x = y as f32;
            }
        }
    }

    /// Whether the state has rung out (below −200 dB).
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.state.iter().flatten().all(|s| s.abs() < 1e-10)
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 2]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state gain of a sine at `hz` through the band, in dB.
    fn gain_db(bell: Bell, hz: f64) -> f64 {
        let rate = 48_000;
        let mut f = Biquad::bell(bell, rate);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| {
                let v = (std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin() as f32;
                [v, 0.0]
            })
            .collect();
        f.process(&mut io);
        let peak = io[24_000..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
        20.0 * f64::from(peak).log10()
    }

    #[test]
    fn a_bell_has_its_gain_at_the_centre_and_unity_far_away() {
        let bell = Bell {
            hz: 1000.0,
            q: 1.0,
            gain_db: 12.0,
        };
        assert!((gain_db(bell, 1000.0) - 12.0).abs() < 0.05);
        assert!(gain_db(bell, 30.0).abs() < 0.1);
        let cut = Bell {
            gain_db: -12.0,
            ..bell
        };
        assert!((gain_db(cut, 1000.0) + 12.0).abs() < 0.05);
    }

    #[test]
    fn a_flat_bell_passes_audio_unchanged() {
        let mut f = Biquad::bell(
            Bell {
                hz: 500.0,
                q: 0.7,
                gain_db: 0.0,
            },
            44_100,
        );
        let mut io = [[0.25, -0.5]; 64];
        f.process(&mut io);
        assert!(
            io.iter()
                .all(|s| (s[0] - 0.25).abs() < 1e-6 && (s[1] + 0.5).abs() < 1e-6)
        );
    }
}
