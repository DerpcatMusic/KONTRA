//! Kolor shaper cases 2 and 5 of the dispatch `1419749f0`, decoded from `Kontakt8.exe`.
//!
//! Both are stateless. Case 2 (`141974bf1`) calls `14197adb0`; case 5 (`141974cad`)
//! calls `14197ae80`. Each returns its result in `xmm0`. `expf` is the CRT thunk
//! `1443d111a`. Constants: `0x1447148c0` = 0.05, `0x14769b2ec` = 0.0025,
//! `0x14470bad4` = 1, `0x14769b2f0` = 13.72612, `0x14769b2f4` = -0.75.

use super::KolorShaper;

const PRE_GAIN: f32 = 0.05;
const KNEE: f32 = 0.0025;
const DRIVE: f32 = 13.72612;
const LAG: f32 = -0.75;

/// Case 2: `(e^y - s) / (e^-y + e^y)` with `y = DRIVE * (PRE_GAIN*x) / sqrt(KNEE*x^2 + 1)`
/// and `s = e^(-(e^(LAG*sqrt|y|) + 1) * y)`.
pub struct Rational;

/// Case 5: the odd soft clip `-sign(x) * (1 - e^-|x|)`.
pub struct OddClip;

impl KolorShaper for Rational {
    fn process(&mut self, x: f32) -> f32 {
        let y = (x * PRE_GAIN) / (KNEE * x * x + 1.0).sqrt() * DRIVE;
        let ey = y.exp();
        let s = (-(((LAG * y.abs().sqrt()).exp() + 1.0) * y)).exp();
        (ey - s) / ((-y).exp() + ey)
    }
}

impl KolorShaper for OddClip {
    fn process(&mut self, x: f32) -> f32 {
        let e = (-x.abs()).exp();
        if x < 0.0 { 1.0 - e } else { e - 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_is_zero_at_zero_and_matches_f64_reference() {
        assert_eq!(Rational.process(0.0), 0.0);
        for i in -40..=40 {
            let x = i as f32 * 0.05;
            let y = (x as f64 * 0.05) / (0.0025 * (x as f64).powi(2) + 1.0).sqrt() * 13.72612;
            let s = (-((-0.75 * y.abs().sqrt()).exp() + 1.0) * y).exp();
            let reference = (y.exp() - s) / ((-y).exp() + y.exp());
            assert!((Rational.process(x) as f64 - reference).abs() < 1e-5, "x={x}");
        }
    }

    #[test]
    fn odd_clip_is_odd_bounded_and_signed() {
        assert_eq!(OddClip.process(0.0), 0.0);
        let x = 0.5;
        assert!((OddClip.process(x) + 1.0 - (-0.5f32).exp()).abs() < 1e-6);
        assert!((OddClip.process(-x) + OddClip.process(x)).abs() < 1e-7);
        for i in -100..=100 {
            let y = OddClip.process(i as f32 * 0.1);
            assert!(y.abs() < 1.0);
        }
    }
}
