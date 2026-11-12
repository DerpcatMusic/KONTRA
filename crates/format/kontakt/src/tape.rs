//! Tape Saturator (module 0x42, `NI::KONTAKTFX::Tape`) onto the engine's
//! waveshaper. Its saved record holds gain, warmth, roll-off and quality at
//! 0, 4, 8, 12 (bindings table `fx_dynamics_dist_tape.tsv`). The structure
//! is read from Kontakt 8.13's binary; no constant is fitted:
//!
//! - Knobs (setter FUN_140ae1e10): gain → drive `300·10^(48·gain/20)`
//!   (FUN_140ae1ee0); warmth → `10^((6·warmth − 3)/20)`; roll-off moves two
//!   low-passes (FUN_140adff50).
//! - Process (FUN_140ae1190), coefficients in FUN_140ae0200, with Quality
//!   off at 48 kHz running twice the rate (FUN_140ae0fc0):
//!   1. Base rate: a Butterworth high-pass designed at 25 Hz for the doubled
//!      rate but run at the base rate, so at 12.5 Hz; a 17 Hz bump.
//!   2. Doubled rate: a 26 kHz low-pass (Q 1.8), a first-order pre-emphasis
//!      set at 20 kHz (its zero ~8.2 kHz, pole ~40 kHz), the same 26 kHz
//!      low-pass again.
//!   3. The curve: the 4097-point table at 0x144fc5510 read at `drive·|x|`
//!      (clamped to 4096), times `1395.19/drive` (its slope at 0, so small
//!      signals pass at unity).
//!   4. Doubled rate: low-passes at `10·(1 + roll-off)` kHz (Q 0.7) and
//!      `12 + 8·roll-off` kHz (Q 0.8); a 1 kHz differentiator into a 3.2 kHz
//!      low-pass (Q 0.4), together one band-pass.
//!   5. Base rate: a +1 dB first-order low shelf at 80 Hz; a first-order
//!      20 Hz high-pass at −3 dB; then the output plus warmth times 27.18
//!      times its 55 Hz one-pole low-pass: a first-order low shelf.
//!
//! Each stage is its analog prototype ([`ir::Section`]); the engine's own
//! bilinear transform at twice its rate gives Kontakt's coefficients at
//! 48 kHz. The pre-emphasis and the band-pass carry ratios of prewarped
//! corners, evaluated at 96 kHz.
//!
//! Carried as is: the curve, as 65 of the table's points (every 64th; at
//! most 0.01 dB from the full table). Not carried: Quality (twice the rate
//! again, 25/4 Hz for the first high-pass) and the 30 ms control ramps;
//! Kontakt at 96 kHz runs Tape without oversampling, the engine still at
//! twice the rate.

use ir::Stage::{In, Out, Post, Pre};
use kontra_ir as ir;

/// `num / den` in `s / (2π·hz)`.
const fn section(hz: f32, num: [f32; 3], den: [f32; 3]) -> ir::Section {
    ir::Section { hz, num, den }
}

/// A second-order low-pass at `hz`, `1/Q` as Kontakt stores it.
const fn low_pass(hz: f32, inverse_q: f32) -> ir::Section {
    section(hz, [1.0, 0.0, 0.0], [1.0, inverse_q, 1.0])
}

/// The stages no knob moves.
static FIXED: [(ir::Stage, ir::Section); 8] = [
    // 1/Q 0x144fcdc14.
    (In, section(12.5, [0.0, 0.0, 1.0], [1.0, 1.414_427_2, 1.0])),
    // Numerator term 0x144fcdc1c over two coincident poles.
    (In, section(17.0, [1.0, 3.556_558_8, 1.0], [1.0, 2.0, 1.0])),
    // 1/Q 0x144fcdc0c.
    (Pre, low_pass(26_000.0, 0.555_555_6)),
    // Zero term 0x144fcdc24 (2.981): `[1, k·(k + 2.981)] / [1, k²]`,
    // `k = tan(π·20/96)`.
    (
        Pre,
        section(20_000.0, [1.0, 2.876_247_7, 0.0], [1.0, 0.588_790_7, 0.0]),
    ),
    (Pre, low_pass(26_000.0, 0.555_555_6)),
    // `(2π·3200 / 2π·1000)` prewarped, `tan(π·3.2/96) / tan(π/96)`; 1/Q
    // 0x144fc40e0.
    (
        Post,
        section(3_200.0, [0.0, 3.210_602_3, 0.0], [1.0, 2.5, 1.0]),
    ),
    // +1 dB: 0x144fcdc08 (0.122).
    (Out, section(80.0, [1.122_018_5, 1.0, 0.0], [1.0, 1.0, 0.0])),
    // −3 dB: 0x144fcdc10.
    (Out, section(20.0, [0.0, 0.707_945_8, 0.0], [1.0, 1.0, 0.0])),
];

/// The curve's table (0x144fc5510), every 64th of its 4097 points.
static CURVE: [f32; 65] = [
    0.0,
    0.045_825_79,
    0.091_393_106,
    0.136_465_46,
    0.180_832_42,
    0.224_318_39,
    0.266_787_26,
    0.308_142_84,
    0.348_325_43,
    0.387_305_4,
    0.425_074_76,
    0.461_638_42,
    0.497_006,
    0.531_185_5,
    0.564_179_4,
    0.595_983_5,
    0.626_588_35,
    0.655_982,
    0.684_154_03,
    0.711_098_9,
    0.736_818_55,
    0.761_322_8,
    0.784_628_15,
    0.806_754_4,
    0.827_720_17,
    0.847_537_6,
    0.866_207_24,
    0.883_714_7,
    0.900_028_35,
    0.915_098_55,
    0.928_859_5,
    0.941_232_44,
    0.952_132_17,
    0.961_477_9,
    0.969_210_5,
    0.975_412,
    0.980_365_9,
    0.984_323_14,
    0.987_484_3,
    0.990_009_5,
    0.992_026_8,
    0.993_638_16,
    0.994_925_4,
    0.995_953_7,
    0.996_775,
    0.997_431_2,
    0.997_955_4,
    0.998_374,
    0.998_708_5,
    0.998_975_7,
    0.999_189_1,
    0.999_359_67,
    0.999_495_9,
    0.999_604_6,
    0.999_691_5,
    0.999_761,
    0.999_816_4,
    0.999_860_7,
    0.999_896_1,
    0.999_924_4,
    0.999_947,
    0.999_965,
    0.999_979_4,
    0.999_991,
    1.0,
];

/// The record's knobs, each 0..=1, as the engine's waveshaper.
pub(crate) fn tape(gain: f32, warmth: f32, rolloff: f32) -> ir::EffectKind {
    let [gain, warmth, rolloff] = [gain, warmth, rolloff].map(|x| x.clamp(0.0, 1.0));
    let warmth = 10f32.powf((6.0 * warmth - 3.0) / 20.0);
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        // The table spans 4096 of the drive's units (0x149e6c5b0).
        curve: ir::DriveCurve::Table {
            drive: 300.0 * 10f32.powf(48.0 * gain / 20.0) / 4096.0,
            points: &CURVE,
        },
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections {
            fixed: &FIXED,
            moving: [
                // 0x144fcdc7c, 0x144fcdc18.
                Some((Post, low_pass(10_000.0 * (1.0 + rolloff), 1.428_571_4))),
                // 0x144fcdc78, 0x144fcdc80, 0x144714940.
                Some((Post, low_pass(8_000.0 * rolloff + 12_000.0, 1.25))),
                // 0x144fcdc70 (27.18) at 55 Hz.
                Some((
                    Out,
                    section(55.0, [1.0 + 27.183_832 * warmth, 1.0, 0.0], [1.0, 1.0, 0.0]),
                )),
            ],
        },
    })
}

/// The saved knobs (gain, warmth, roll-off, quality) as a script moves
/// them: every move rebuilds the shaper through [`tape`].
pub(crate) fn model(public: &[u8]) -> Option<ir::KnobModel> {
    use ir::SlotKnob::Knob;
    let f = |n: usize| Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    let quality = f32::from(*public.get(12)?);
    Some(ir::KnobModel {
        knobs: [Knob(0), Knob(1), Knob(2), Knob(3)].into(),
        saved: [f(0)?, f(4)?, f(8)?, quality].into(),
        build: |p| {
            let [gain, warmth, rolloff, _quality] = *p else {
                return None;
            };
            Some(tape(gain, warmth, rolloff))
        },
    })
}

/// The KSP names of [`model`]'s knobs; each is the record's field as set
/// (bindings table; Quality stores 1 at any value ≥ 1).
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 4] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        ("$ENGINE_PAR_TP_GAIN", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_TP_WARMTH", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_TP_HF_ROLLOFF", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_TP_QUALITY", Slot(Knob(3)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants written from prewarped ratios are those ratios.
    #[test]
    fn the_ratios_are_the_prewarped_corners() {
        let t = |hz: f64| (std::f64::consts::PI * hz / 96_000.0).tan();
        let k = t(20_000.0);
        let close = |a: f32, b: f64| (f64::from(a) - b).abs() < 1e-6;
        let [_, _, _, (_, emphasis), _, (_, band), ..] = FIXED;
        assert!(close(emphasis.num[1], k * (k + 2.981_071_949)));
        assert!(close(emphasis.den[1], k * k));
        assert!(close(band.num[1], t(3_200.0) / t(1_000.0)));
    }

    /// Small signals pass every curve setting at unity, and Kontakt's
    /// ceiling holds: at full gain the curve tops out at `1395.19 / drive`.
    #[test]
    fn the_curve_is_unity_small_and_ceilinged_at_full_gain() {
        let ir::EffectKind::Shaper(s) = tape(1.0, 0.5, 0.5) else {
            panic!("a shaper");
        };
        let ir::DriveCurve::Table { drive, points } = s.curve else {
            panic!("a table");
        };
        let slope = (points[1] - points[0]) * 64.0;
        let ceiling = 1.0 / (drive * slope);
        let g = 300.0 * 10f32.powf(2.4);
        assert!((ceiling / (1395.19 / g) - 1.0).abs() < 2e-3, "{ceiling}");
    }
}
