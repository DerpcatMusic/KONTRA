//! The white curve: what the bands ask of the reduction, as the reference
//! models it.
//!
//! It is a function of the bands alone -- no audio, no engine -- so it is on
//! screen with the transport stopped. Each enabled band contributes a linear
//! magnitude over the plot's points; the bands multiply. The shapes are the
//! reference's own, read from its code:
//!
//! * bell: the band's gain raised to a Gaussian in octaves, so the curve
//!   reaches the gain at the centre and the width follows the bandwidth;
//! * low shelf, high shelf and tilt: a logistic in octaves, half the gain at
//!   the centre, its sharpness the slope over six;
//! * low cut and high cut: a one-pole-like skirt that is -3 dB at the centre
//!   and falls at the slope's decibels per octave;
//! * bandpass and band reject: a high-pass and a low-pass a bandwidth apart,
//!   normalised to their value at the centre; the reject is one minus it.

use super::band::Band;
use super::axis::{BAND_DB_PER_UNIT, PLOT_BINS, plot_frequency};

/// The bell's width law, from the reference: its Gaussian is this many
/// octaves wide per octave of bandwidth.
const BELL_WIDTH: f64 = 0.601_591_280_067_049 * std::f64::consts::FRAC_1_SQRT_2;
/// `0.05 * log2(10)`: how a decibel-per-octave slope sets a skirt's exponent.
const SKIRT_EXPONENT: f64 = 0.166_096_4;
/// The skirts sit at -3 dB (`10^-0.15`) at their corner.
const SKIRT_CORNER: f64 = 0.412_537_54; // 10^0.15 - 1
/// The narrowest bandwidth a bandpass or reject skirt is cut for, in octaves.
const NARROWEST: f64 = 0.25;
/// The curve's floor, so a zero never reaches a logarithm.
const FLOOR: f64 = 2.0e-7;

/// A band's bandwidth in octaves from the host's Q.
fn bandwidth(q: f64) -> f64 {
    if q > 0.0 {
        (2.0 / std::f64::consts::LN_2) * (1.0 / (2.0 * q)).asinh()
    } else {
        0.0
    }
}

fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// A skirt's magnitude: `rising` for a high-pass, falling for a low-pass.
fn skirt(f: f64, corner: f64, slope: f64, rising: bool) -> f64 {
    let n = slope * SKIRT_EXPONENT;
    let r = (f / corner.max(1.0e-13)).max(1.0e-13);
    let power = if rising { r.powf(-n) } else { r.powf(n) };
    1.0 / (1.0 + SKIRT_CORNER * power)
}

/// One band's linear magnitude at `f`.
fn magnitude(band: &Band, f: f64) -> f64 {
    let fc = band.frequency.max(1.0e-7);
    let db = band.depth * BAND_DB_PER_UNIT;
    let gain = 10.0_f64.powf(db / 20.0);
    let octaves = (f / fc).log2();
    let sharpness = band.slope / 6.0;
    let bandpass = |width: f64, gain: f64| {
        let skirt_slope = 24.0 / width.min(NARROWEST);
        let half = 2.0_f64.powf(0.5 * width);
        let (lo, hi) = (fc / half, fc * half);
        let at = |f: f64| skirt(f, lo, skirt_slope, true) * skirt(f, hi, skirt_slope, false);
        gain * at(f) / at(fc).max(FLOOR)
    };
    match band.shape {
        0 => skirt(f, fc, band.slope, true),
        1 => 10.0_f64.powf(db * (1.0 - logistic(sharpness * octaves)) / 20.0),
        2 => {
            let width = BELL_WIDTH * bandwidth(band.q);
            let g = if width > 0.0 {
                (-0.5 * (octaves / width).powi(2)).exp()
            } else {
                f64::from(u8::from(octaves == 0.0))
            };
            gain.powf(g)
        }
        3 => bandpass(bandwidth(band.q), gain),
        4 => (1.0 - bandpass(std::f64::consts::FRAC_1_SQRT_2 * bandwidth(band.q), 1.0)).max(0.0),
        5 => 10.0_f64.powf(2.0 * db * (logistic(sharpness * octaves) - 0.5) / 20.0),
        6 => 10.0_f64.powf(db * logistic(sharpness * octaves) / 20.0),
        _ => skirt(f, fc, band.slope, false),
    }
}

/// One band's own curve over the plot's points, as linear magnitude.
pub fn band_curve(band: &Band) -> [f32; PLOT_BINS] {
    std::array::from_fn(|i| magnitude(band, f64::from(plot_frequency(i))).max(FLOOR) as f32)
}

/// What the bands ask of the reduction over the plot's points, as linear
/// magnitude: the enabled bands multiplied, a bandpass limiting the whole to
/// its own region.
pub fn curve(bands: &[Band]) -> [f32; PLOT_BINS] {
    let enabled = || bands.iter().filter(|b| b.active && !b.bypass);
    std::array::from_fn(|i| {
        let f = f64::from(plot_frequency(i));
        let product: f64 = enabled().filter(|b| b.shape != 3).map(|b| magnitude(b, f)).product();
        let passes: Vec<f64> = enabled().filter(|b| b.shape == 3).map(|b| magnitude(b, f)).collect();
        let limit = if passes.is_empty() { 1.0 } else { passes.iter().sum() };
        (product * limit).max(FLOOR) as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(shape: usize, frequency: f64, depth: f64) -> Band {
        Band { frequency, depth, shape, ..Band::at(frequency, depth, shape, true) }
    }

    fn db_at(curve: &[f32; PLOT_BINS], frequency: f64) -> f64 {
        let i = (0..PLOT_BINS)
            .min_by(|&a, &b| {
                let da = (f64::from(plot_frequency(a)) / frequency).ln().abs();
                let db = (f64::from(plot_frequency(b)) / frequency).ln().abs();
                da.total_cmp(&db)
            })
            .unwrap();
        20.0 * f64::from(curve[i]).log10()
    }

    #[test]
    fn a_bell_reaches_its_gain_at_its_centre_and_nothing_far_away() {
        let c = band_curve(&band(2, 1000.0, 5.0));
        let gain = 5.0 * BAND_DB_PER_UNIT;
        assert!((db_at(&c, 1000.0) - gain).abs() < 0.2, "{}", db_at(&c, 1000.0));
        assert!(db_at(&c, 20.0).abs() < 0.2 && db_at(&c, 20000.0).abs() < 0.2);
    }

    #[test]
    fn shelves_and_the_tilt_cross_half_their_gain_at_the_centre() {
        let gain = 4.0 * BAND_DB_PER_UNIT;
        for (shape, at_centre) in [(1, gain / 2.0), (6, gain / 2.0), (5, 0.0)] {
            let c = band_curve(&band(shape, 1000.0, 4.0));
            assert!((db_at(&c, 1000.0) - at_centre).abs() < 0.2, "shape {shape}");
        }
        let low = band_curve(&band(1, 1000.0, 4.0));
        assert!(db_at(&low, 20.0) > gain - 0.5 && db_at(&low, 20000.0).abs() < 0.5);
    }

    #[test]
    fn the_cuts_are_three_decibels_down_at_their_corner() {
        for shape in [0, 7] {
            let c = band_curve(&band(shape, 1000.0, 0.0));
            assert!((db_at(&c, 1000.0) + 3.0).abs() < 0.2, "shape {shape}");
        }
        // A skirt falls at its slope: 48 dB an octave is a wall a decade down.
        let steep = Band { slope: 48.0, ..band(0, 1000.0, 0.0) };
        let low_cut = band_curve(&steep);
        assert!(db_at(&low_cut, 100.0) < -60.0 && db_at(&low_cut, 10000.0).abs() < 1.0);
        // At the slope's own six decibels an octave it is gentler, and still a cut.
        let gentle = band_curve(&band(0, 1000.0, 0.0));
        assert!(db_at(&gentle, 100.0) < -10.0 && db_at(&gentle, 10000.0).abs() < 1.0);
    }

    #[test]
    fn a_reject_is_empty_at_its_centre_and_a_bandpass_is_full_there() {
        let reject = band_curve(&band(4, 1000.0, 0.0));
        assert!(db_at(&reject, 1000.0) < -40.0);
        assert!(db_at(&reject, 20.0).abs() < 0.5);
        let pass = band_curve(&band(3, 1000.0, 3.0));
        let gain = 3.0 * BAND_DB_PER_UNIT;
        assert!((db_at(&pass, 1000.0) - gain).abs() < 0.5);
    }

    #[test]
    fn flat_bands_make_a_flat_curve_and_bypassed_ones_vanish() {
        let flat = curve(&[band(2, 1000.0, 0.0)]);
        assert!(flat.iter().all(|v| (v - 1.0).abs() < 1.0e-6));
        let mut off = band(2, 1000.0, 8.0);
        off.bypass = true;
        assert!(curve(&[off]).iter().all(|v| (v - 1.0).abs() < 1.0e-6));
    }
}
