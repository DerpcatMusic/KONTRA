//! Supercharger GT (module 0x60, saved v0x51) onto the engine's waveshaper,
//! from Kontakt 8.13.1 renders (2026-11-14,
//! docs/re/engine/fx-measured/distortion-family.md); there is no binary read.
//! Its saved record holds TRIM, SATURATION, COMPRESS, CHARACTER, MIX,
//! ATTACK and RELEASE as floats at 0..24 (bindings table `fx_other.tsv`);
//! where SAT_MODE, CHAR_MODE and HPF_MODE are saved is not known, so a saved
//! instrument plays Mild with the first character (scripts set them).
//!
//! - The saturator is `(C/T)·tanh(s·T·x/C)`: a ceiling `C` and small-signal
//!   slope `s` per SAT_MODE and SATURATION ([`SATURATION`], each fitted to a
//!   1 kHz ramp to full scale within 0.2 % to 2 % of full scale), TRIM a
//!   gain `T` of 58.4 dB·(TRIM − 0.5) in and out (TRIM 1 brings Moderate's
//!   0.107 ceiling to 0.0037).
//! - The tone, fitted to −60 dB sweeps: a fixed base ([`BASE`], within
//!   0.16 dB) and per CHAR_MODE a change at CHARACTER 1 ([`CHARACTER`],
//!   within 0.41, 1.16 and 0.94 dB) whose gains in dB scale with CHARACTER
//!   (CHARACTER 0.5 sits midway in dB, measured in two modes).
//!
//! Not carried: COMPRESS, ATTACK and RELEASE (the compressor), MIX, HPF_MODE,
//! channel link, and the hard clip at ±1 after the trim's make-up (heard at
//! TRIM near 0 with loud input).

use ir::Stage::{In, Out};
use kontra_ir as ir;

/// Ceiling and slope at SATURATION 0, 0.25, 0.5 and 1 for Mild, Moderate
/// and Hot. Moderate and Hot at 0 are Mild's (unmeasured).
const SATURATION: [[(f32, f32); 4]; 3] = [
    [
        (0.6521, 0.954),
        (0.6765, 1.219),
        (0.2486, 1.641),
        (0.1138, 1.822),
    ],
    [
        (0.6521, 0.954),
        (0.4151, 1.378),
        (0.1071, 2.095),
        (0.0515, 1.584),
    ],
    [
        (0.6521, 0.954),
        (0.1702, 1.060),
        (0.0649, 1.427),
        (0.0203, 0.650),
    ],
];
const AT: [f32; 4] = [0.0, 0.25, 0.5, 1.0];
/// TRIM's gain either way at 0 and 1, dB.
const TRIM_DB: f32 = 29.2;

/// A gain in dB as a factor.
fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

/// A peak at `hz`, `q`, `gain` dB, its numerator scaled by `scale`.
fn peak(hz: f32, q: f32, gain: f32, scale: f32) -> ir::Section {
    let a = db(gain / 2.0);
    ir::Section {
        hz,
        num: [scale, scale * a / q, scale],
        den: [1.0, 1.0 / (a * q), 1.0],
    }
}

/// A first-order low shelf (`gain` dB below `hz`).
fn low_shelf(hz: f32, gain: f32) -> ir::Section {
    ir::Section {
        hz,
        num: [db(gain), 1.0, 0.0],
        den: [1.0, 1.0, 0.0],
    }
}

/// A first-order high shelf (`gain` dB above `hz`).
fn high_shelf(hz: f32, gain: f32) -> ir::Section {
    ir::Section {
        hz,
        num: [1.0, db(gain), 0.0],
        den: [1.0, 1.0, 0.0],
    }
}

/// The base tone: +0.55 dB, a 0.78 dB bump at 67 Hz (Q 1.32), +5.0 dB above
/// 16.6 kHz. Static, so its sections are worked out by hand: the peak's
/// `10^(0.784/40)` = 1.0454 and the shelf's `10^(5.036/20)` = 1.7853, the
/// gain `10^(0.5475/20)` = 1.0651 on the peak's numerator.
static BASE: [(ir::Stage, ir::Section); 2] = [
    (
        In,
        ir::Section {
            hz: 67.32,
            num: [1.0651, 1.0651 * 1.0454 / 1.3239, 1.0651],
            den: [1.0, 1.0 / (1.0454 * 1.3239), 1.0],
        },
    ),
    (
        In,
        ir::Section {
            hz: 16_624.0,
            num: [1.0, 1.7853, 0.0],
            den: [1.0, 1.0, 0.0],
        },
    ),
];

/// One CHAR_MODE's tone at CHARACTER 1: gain dB, low shelf (Hz, dB), peak
/// (Hz, Q, dB), high shelf (Hz, dB).
type Tone = (f32, [f32; 2], [f32; 3], [f32; 2]);

/// Per CHAR_MODE.
const CHARACTER: [Tone; 3] = [
    (
        -2.405,
        [93.73, 9.361],
        [130.9, 0.6576, -4.163],
        [10_338.0, 5.859],
    ),
    (
        -2.638,
        [5.882, 23.18],
        [11_660.0, 0.1994, -10.33],
        [1_548.0, 7.213],
    ),
    (
        -4.319,
        [205.1, 3.220],
        [2_426.0, 0.2349, 5.798],
        [8_997.0, 8.361],
    ),
];

/// The record's knobs and modes as the engine's waveshaper: TRIM,
/// SATURATION and CHARACTER 0..=1, SAT_MODE and CHAR_MODE 0..=2.
pub fn supergt(
    trim: f32,
    saturation: f32,
    character: f32,
    sat_mode: f32,
    char_mode: f32,
) -> ir::EffectKind {
    let [trim, saturation, character] = [trim, saturation, character].map(|x| x.clamp(0.0, 1.0));
    let pick = |m: f32| m.round().clamp(0.0, 2.0) as usize;
    let [mild, moderate, hot] = &SATURATION;
    let curve = match pick(sat_mode) {
        1 => moderate,
        2 => hot,
        _ => mild,
    };
    // The two measured settings either side, log-linear in the ceiling.
    let i = AT
        .iter()
        .rposition(|&a| a <= saturation)
        .unwrap_or(0)
        .min(2);
    let (lo, hi) = (curve.get(i).copied(), curve.get(i + 1).copied());
    let ((c0, s0), (c1, s1), (a0, a1)) = match (lo, hi, AT.get(i), AT.get(i + 1)) {
        (Some(lo), Some(hi), Some(&a0), Some(&a1)) => (lo, hi, (a0, a1)),
        _ => ((0.6521, 0.954), (0.6521, 0.954), (0.0, 1.0)),
    };
    let t = (saturation - a0) / (a1 - a0);
    let ceiling = c0 * (c1 / c0).powf(t);
    let slope = s0 + (s1 - s0) * t;
    let gain = db(2.0 * TRIM_DB * (trim - 0.5));
    let [first, second, third] = CHARACTER;
    let (out_db, [ls_hz, ls_db], [pk_hz, pk_q, pk_db], [hs_hz, hs_db]) = match pick(char_mode) {
        1 => second,
        2 => third,
        _ => first,
    };
    let k = character;
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Tanh(slope * gain / ceiling),
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections {
            fixed: &BASE,
            moving: [
                Some((
                    Out,
                    peak(pk_hz, pk_q, k * pk_db, db(k * out_db) * ceiling / gain),
                )),
                Some((Out, low_shelf(ls_hz, k * ls_db))),
                Some((Out, high_shelf(hs_hz, k * hs_db))),
            ],
        },
    })
}

/// SAT_MODE and CHAR_MODE as knobs 3 and 4 hold them before a script sets
/// them (not read from the record).
const MODES: [f32; 2] = [0.0, 0.0];

/// The saved record as a script moves it: TRIM, SATURATION and CHARACTER
/// from the record, the modes as [`MODES`].
pub fn model(public: &[u8]) -> Option<ir::KnobModel> {
    use ir::SlotKnob::Knob;
    let f = |n: usize| Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    let [sat_mode, char_mode] = MODES;
    Some(ir::KnobModel {
        knobs: (0..5).map(Knob).collect(),
        saved: [f(0)?, f(4)?, f(12)?, sat_mode, char_mode].into(),
        build: |p| {
            let [trim, saturation, character, sat_mode, char_mode] = *p else {
                return None;
            };
            Some(supergt(trim, saturation, character, sat_mode, char_mode))
        },
    })
}

/// The saved record's TRIM, SATURATION and CHARACTER, and MIX (not carried).
#[must_use]
pub fn saved(public: &[u8]) -> Option<[f32; 4]> {
    let f = |n: usize| Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    Some([f(0)?, f(4)?, f(12)?, f(16)?])
}

/// The KSP names of [`model`]'s knobs: the floats as set, the modes by their
/// index (a script sets SAT_MODE 1 as the raw value 1).
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 5] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let index = || ir::Law::Linear {
        lo: 0.0,
        hi: 1_000_000.0,
    };
    [
        ("$ENGINE_PAR_SUPERGT_TRIM", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_SUPERGT_SATURATION", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_SUPERGT_CHARACTER", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_SUPERGT_SAT_MODE", Slot(Knob(3)), index()),
        ("$ENGINE_PAR_SUPERGT_CHAR_MODE", Slot(Knob(4)), index()),
    ]
}
