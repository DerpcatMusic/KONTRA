//! Filters: one state-variable shape per class (03-DSP.md §3). A fixed
//! state-variable stage is the bilinear transform of its analog prototype, so
//! it runs as a biquad; formats load their own cutoff, damping and gain laws.
//! A cutoff that moves runs the same shape as [`Svf`], whose trapezoidal
//! integrators stay stable and click-free under per-span coefficient changes,
//! which a direct-form biquad's state does not guarantee (its state is not the
//! filter's energy). One shape, two runners: they agree to 1e-5 at a fixed cutoff.

mod formant;

pub use formant::{Formant, Vowels};
pub use kontra_dsp_core::Biquad;
use kontra_dsp_core::flush;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Low,
    High,
    Band,
    Notch,
}

/// `stages` two-pole stages in series (none: one pole) at `hz`, each with
/// its damping `k` (`1/Q`), then `gain`. A band stage passes `s/(s² + ks + 1)`,
/// the raw band tap: its peak is `1/k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub pass: Pass,
    pub hz: f32,
    pub stages: u8,
    pub damping: [f32; 3],
    pub gain: f32,
}

/// The shape's sections at `rate`; the cutoff is held below 0.49 of the rate.
#[must_use]
pub fn sections(shape: &Shape, rate: u32) -> Vec<Biquad> {
    let rate = f64::from(rate.max(1));
    let hz = f64::from(shape.hz).max(1.0).min(0.49 * rate);
    let g = (std::f64::consts::PI * hz / rate).tan();
    let mut out: Vec<Biquad> = if shape.stages == 0 {
        // 1/(s + 1) or s/(s + 1).
        let num = match shape.pass {
            Pass::High => [1.0, -1.0],
            _ => [g, g],
        };
        vec![Biquad::new([num[0], num[1], 0.0, 1.0 + g, g - 1.0, 0.0])]
    } else {
        shape
            .damping
            .iter()
            .take(usize::from(shape.stages))
            .map(|&k| stage(shape.pass, f64::from(k), g))
            .collect()
    };
    if let Some(first) = out.first_mut() {
        first.scale(f64::from(shape.gain));
    }
    out
}

/// `(b2 s² + b1 s + b0)/(s² + k s + 1)` through `s = (1/g)(1 − z⁻¹)/(1 + z⁻¹)`.
fn stage(pass: Pass, k: f64, g: f64) -> Biquad {
    let [b2, b1, b0] = match pass {
        Pass::Low => [0.0, 0.0, 1.0],
        Pass::High => [1.0, 0.0, 0.0],
        Pass::Band => [0.0, 1.0, 0.0],
        Pass::Notch => [1.0, 0.0, 1.0],
    };
    let gg = g * g;
    Biquad::new([
        b2 + b1 * g + b0 * gg,
        2.0 * (b0 * gg - b2),
        b2 - b1 * g + b0 * gg,
        1.0 + k * g + gg,
        2.0 * (gg - 1.0),
        1.0 - k * g + gg,
    ])
}

/// The shape run sample by sample as a zero-delay state-variable filter
/// (trapezoidal integrators), so its cutoff can move every span without
/// clicks. At a fixed cutoff it equals [`sections`].
#[derive(Clone, Debug, PartialEq)]
pub struct Svf {
    shape: Shape,
    hz: f32,
    g: f64,
    /// Per stage and channel, the two integrator states.
    state: [[[f64; 2]; 2]; 3],
}

impl Svf {
    #[must_use]
    pub fn new(shape: Shape) -> Self {
        Self {
            shape,
            hz: 0.0,
            g: 0.0,
            state: [[[0.0; 2]; 2]; 3],
        }
    }

    /// Moves the cutoff to `hz` at `rate`, held below 0.49 of the rate.
    pub fn set_cutoff(&mut self, hz: f32, rate: u32) {
        let rate = f64::from(rate.max(1));
        let hz = f64::from(hz).max(1.0).min(0.49 * rate);
        self.hz = hz as f32;
        self.g = (std::f64::consts::PI * hz / rate).tan();
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let (g, gain) = (self.g, f64::from(self.shape.gain));
        let stages = usize::from(self.shape.stages);
        // Per stage: k, then the TPT gains a1, a2, a3.
        let coefficients = self.shape.damping.map(|k| {
            let k = f64::from(k);
            let a1 = 1.0 / (1.0 + g * (g + k));
            [k, a1, g * a1, g * g * a1]
        });
        for frame in io {
            for (c, x) in frame.iter_mut().enumerate() {
                let mut v = f64::from(*x);
                if stages == 0
                    && let Some([s, _]) = self.state.first_mut().and_then(|st| st.get_mut(c))
                {
                    let low = (v - *s) * g / (1.0 + g) + *s;
                    *s = 2.0 * low - *s;
                    v = if self.shape.pass == Pass::High {
                        v - low
                    } else {
                        low
                    };
                }
                for (st, &[k, a1, a2, a3]) in self.state.iter_mut().zip(&coefficients).take(stages)
                {
                    let Some([ic1, ic2]) = st.get_mut(c) else {
                        continue;
                    };
                    let v3 = v - *ic2;
                    let band = a1 * *ic1 + a2 * v3;
                    let low = *ic2 + a2 * *ic1 + a3 * v3;
                    *ic1 = 2.0 * band - *ic1;
                    *ic2 = 2.0 * low - *ic2;
                    let high = v - k * band - low;
                    v = match self.shape.pass {
                        Pass::Low => low,
                        Pass::High => high,
                        Pass::Band => band,
                        Pass::Notch => low + high,
                    };
                }
                *x = (v * gain) as f32;
            }
        }
        flush(self.state.as_flattened_mut().as_flattened_mut());
    }

    /// Moves the stages' damping `k` (`1/Q`) without touching the state.
    pub fn set_damping(&mut self, damping: [f32; 3]) {
        self.shape.damping = damping;
    }

    /// The cutoff as last set, in Hz.
    #[must_use]
    pub fn cutoff(&self) -> f32 {
        self.hz
    }

    pub fn reset(&mut self) {
        self.state = [[[0.0; 2]; 2]; 3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state gain of a sine at `hz` through the shape, in dB.
    fn gain_db(shape: &Shape, hz: f64) -> f64 {
        let mut f = sections(shape, 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| {
                let v = (std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin() as f32;
                [v, v]
            })
            .collect();
        for s in &mut f {
            s.process(&mut io);
        }
        let peak = io[24_000..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
        20.0 * f64::from(peak).log10()
    }

    fn shape(pass: Pass, stages: u8) -> Shape {
        let k = std::f32::consts::SQRT_2;
        Shape {
            pass,
            hz: 1000.0,
            stages,
            damping: [k; 3],
            gain: 1.0,
        }
    }

    #[test]
    fn cutoffs_sit_3_db_down_and_slopes_follow_the_order() {
        for (pass, far) in [(Pass::Low, 4000.0), (Pass::High, 250.0)] {
            assert!((gain_db(&shape(pass, 0), 1000.0) + 3.01).abs() < 0.05);
            assert!((gain_db(&shape(pass, 1), 1000.0) + 3.01).abs() < 0.05);
            // Two octaves out: about 12 and 24 dB down.
            let (one, two) = (gain_db(&shape(pass, 0), far), gain_db(&shape(pass, 1), far));
            assert!((one + 12.3).abs() < 1.0, "{pass:?} {one}");
            assert!((two + 24.1).abs() < 1.0, "{pass:?} {two}");
        }
        assert!(gain_db(&shape(Pass::Low, 1), 100.0).abs() < 0.05);
    }

    #[test]
    fn the_running_filter_matches_its_sections() {
        for (pass, stages) in [
            (Pass::Low, 0),
            (Pass::High, 0),
            (Pass::Band, 2),
            (Pass::Notch, 3),
        ] {
            let shape = Shape {
                damping: [0.7, 1.2, 0.3],
                gain: 0.8,
                ..shape(pass, stages)
            };
            let impulse = |n: usize| if n == 0 { [1.0, -0.5] } else { [0.0; 2] };
            let mut a: Vec<[f32; 2]> = (0..512).map(impulse).collect();
            let mut b = a.clone();
            for s in &mut sections(&shape, 48_000) {
                s.process(&mut a);
            }
            let mut svf = Svf::new(shape);
            svf.set_cutoff(shape.hz, 48_000);
            svf.process(&mut b);
            let worst = a
                .iter()
                .zip(&b)
                .map(|(x, y)| (x[0] - y[0]).abs().max((x[1] - y[1]).abs()));
            assert!(worst.fold(0.0, f32::max) < 1e-5, "{pass:?} {stages}");
        }
    }

    #[test]
    fn a_band_peaks_at_one_over_its_damping_and_a_notch_removes_its_centre() {
        let band = Shape {
            damping: [0.5; 3],
            ..shape(Pass::Band, 1)
        };
        assert!((gain_db(&band, 1000.0) - 20.0 * 2f64.log10()).abs() < 0.05);
        assert!(gain_db(&shape(Pass::Notch, 1), 1000.0) < -40.0);
    }

    #[test]
    fn the_running_filter_is_block_size_free_resets_clean_and_stays_finite() {
        let shape = Shape {
            damping: [0.7, 1.2, 0.3],
            ..shape(Pass::Low, 3)
        };
        let impulse = |n: usize| if n == 0 { [1.0, -0.5] } else { [0.0; 2] };
        let whole: Vec<[f32; 2]> = {
            let mut io: Vec<_> = (0..512).map(impulse).collect();
            let mut svf = Svf::new(shape);
            svf.set_cutoff(shape.hz, 48_000);
            svf.process(&mut io);
            io
        };
        let mut svf = Svf::new(shape);
        svf.set_cutoff(shape.hz, 48_000);
        for chunk in [1, 7, 512] {
            svf.reset();
            let mut io: Vec<_> = (0..512).map(impulse).collect();
            for part in io.chunks_mut(chunk) {
                svf.process(part);
            }
            assert_eq!(io, whole, "chunks of {chunk}");
        }
        for (hz, k, rate) in [(1e9, 0.0, 1), (0.0, 1e9, 384_000), (20.0, 1e-9, 8_000)] {
            let mut svf = Svf::new(Shape {
                damping: [k; 3],
                ..shape
            });
            svf.set_cutoff(hz, rate);
            let mut io: Vec<_> = (0..4096).map(|n| [((n % 7) as f32 - 3.0); 2]).collect();
            svf.process(&mut io);
            assert!(
                io.iter().flatten().all(|v| v.is_finite()),
                "{hz} {k} {rate}"
            );
        }
    }
}
