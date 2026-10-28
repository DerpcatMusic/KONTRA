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

pub use kontra_dsp_core::Biquad;

/// The bell at `rate`; its centre is held below 0.49 of the rate.
#[must_use]
pub fn bell(bell: Bell, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let hz = f64::from(bell.hz).max(1.0).min(0.49 * rate);
    let w = std::f64::consts::TAU * hz / rate;
    let a = 10f64.powf(f64::from(bell.gain_db) / 40.0);
    let alpha = w.sin() / (2.0 * f64::from(bell.q).max(1e-3));
    Biquad::new([
        1.0 + alpha * a,
        -2.0 * w.cos(),
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * w.cos(),
        1.0 - alpha / a,
    ])
}

/// A shelf: `gain_db` below (`high` false) or above `hz`, its knee set by
/// `slope` (the cookbook's S, held to 0.01..=1 so the knee never peaks).
#[must_use]
pub fn shelf(hz: f32, gain_db: f32, slope: f32, high: bool, rate: u32) -> Biquad {
    let rate = f64::from(rate.max(1));
    let hz = f64::from(hz).max(1.0).min(0.49 * rate);
    let w = std::f64::consts::TAU * hz / rate;
    let a = 10f64.powf(f64::from(gain_db) / 40.0);
    let s = f64::from(slope).clamp(0.01, 1.0);
    let alpha = w.sin() / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
    let (cos, root) = (w.cos(), 2.0 * a.sqrt() * alpha);
    // The high shelf is the low one with cos negated and b1, a1 flipped.
    let (c, sign) = if high { (-cos, -1.0) } else { (cos, 1.0) };
    Biquad::new([
        a * ((a + 1.0) - (a - 1.0) * c + root),
        sign * 2.0 * a * ((a - 1.0) - (a + 1.0) * c),
        a * ((a + 1.0) - (a - 1.0) * c - root),
        (a + 1.0) + (a - 1.0) * c + root,
        sign * -2.0 * ((a - 1.0) + (a + 1.0) * c),
        (a + 1.0) + (a - 1.0) * c - root,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state gain of a sine at `hz` through the band, in dB.
    fn gain_db(band: Bell, hz: f64) -> f64 {
        let rate = 48_000;
        let mut f = bell(band, rate);
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

    /// Steady-state gain of a sine at `hz` through `f`, in dB.
    fn through(mut f: Biquad, hz: f64) -> f64 {
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [(std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin() as f32; 2])
            .collect();
        f.process(&mut io);
        let peak = io[24_000..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
        20.0 * f64::from(peak).log10()
    }

    #[test]
    fn shelves_lift_their_side_and_leave_the_other() {
        let low = || shelf(200.0, 6.0, 1.0, false, 48_000);
        assert!((through(low(), 30.0) - 6.0).abs() < 0.1);
        assert!(through(low(), 7919.0).abs() < 0.1);
        let high = || shelf(2000.0, -6.0, 0.5, true, 48_000);
        assert!((through(high(), 15_000.0) + 6.0).abs() < 0.2);
        assert!(through(high(), 50.0).abs() < 0.1);
    }

    #[test]
    fn a_flat_bell_passes_audio_unchanged() {
        let mut f = bell(
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
