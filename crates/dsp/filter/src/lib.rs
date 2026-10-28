//! Filters: one state-variable shape per class (03-DSP.md §3). A fixed
//! state-variable stage is the bilinear transform of its analog prototype, so
//! it runs as a biquad; formats load their own cutoff, damping and gain laws.

pub use kontra_dsp_core::Biquad;

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
    let hz = f64::from(shape.hz).clamp(1.0, 0.49 * rate);
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
    fn a_band_peaks_at_one_over_its_damping_and_a_notch_removes_its_centre() {
        let band = Shape {
            damping: [0.5; 3],
            ..shape(Pass::Band, 1)
        };
        assert!((gain_db(&band, 1000.0) - 20.0 * 2f64.log10()).abs() < 0.05);
        assert!(gain_db(&shape(Pass::Notch, 1), 1000.0) < -40.0);
    }
}
