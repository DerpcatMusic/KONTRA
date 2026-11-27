//! Equalisers: one biquad band shape per class. Formats load their own laws
//! into these shapes; the shapes know no vendor.

pub use crate::{Biquad, Side, shelf};

/// A peaking band: `gain_db` at `hz`, unity far from it, width set by `q`
/// (Robert Bristow-Johnson's cookbook bell).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bell {
    pub hz: f32,
    pub q: f32,
    pub gain_db: f32,
}

/// The bell at `rate`; its centre is held below 0.49 of the rate.
#[must_use]
pub fn bell(bell: Bell, rate: u32) -> Biquad {
    crate::peak(bell.hz, bell.q, bell.gain_db, rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_db(mut f: Biquad, hz: f64) -> f64 {
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [(std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin() as f32; 2])
            .collect();
        f.process(&mut io);
        let peak = io[24_000..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
        20.0 * f64::from(peak).log10()
    }

    #[test]
    fn bell_and_shelves_hit_target_gains_and_pass_flat_at_unity() {
        let boost = Bell {
            hz: 1000.0,
            q: 1.0,
            gain_db: 12.0,
        };
        assert!((gain_db(bell(boost, 48_000), 1000.0) - 12.0).abs() < 0.05);
        assert!(gain_db(bell(boost, 48_000), 30.0).abs() < 0.1);
        let cut = Bell {
            gain_db: -12.0,
            ..boost
        };
        assert!((gain_db(bell(cut, 48_000), 1000.0) + 12.0).abs() < 0.05);

        let low = shelf(200.0, 6.0, 1.0, Side::Low, 48_000);
        assert!((gain_db(low, 30.0) - 6.0).abs() < 0.1);
        let high = shelf(2000.0, -6.0, 0.5, Side::High, 48_000);
        assert!((gain_db(high, 15_000.0) + 6.0).abs() < 0.2);

        let mut flat = bell(
            Bell {
                hz: 500.0,
                q: 0.7,
                gain_db: 0.0,
            },
            44_100,
        );
        let mut io = [[0.25, -0.5]; 64];
        flat.process(&mut io);
        assert!(
            io.iter()
                .all(|s| (s[0] - 0.25).abs() < 1e-6 && (s[1] + 0.5).abs() < 1e-6)
        );
    }
}
