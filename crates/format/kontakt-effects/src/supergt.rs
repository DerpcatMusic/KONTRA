//! Supercharger GT (module 0x60, saved v0x51) onto the engine's waveshaper,
//! from Kontakt 8.13.1 renders (2026-11-14,
//! docs/re/engine/fx-measured/distortion-family.md); there is no binary read.
//! Its saved record (45 bytes) holds TRIM, SATURATION, COMPRESS, CHARACTER,
//! MIX, ATTACK and RELEASE as floats at 0..24, then HPF_MODE, SAT_MODE and
//! CHAR_MODE as i32 at 28, 32 and 36, each saved as `2 − mode` (oracle
//! saves of one instrument differing in one mode, byte-diffed).
//!
//! - The saturator is a measured curve per SAT_MODE at SATURATION 0, 0.25,
//!   0.5, 0.75 and 1 ([`CURVES`], `tools/kontakt-oracle/fx_curve.py` on a
//!   1 kHz ramp to full scale, rms error 0.0001 of full scale or less),
//!   blended linearly between them; TRIM a gain of 58.4 dB·(TRIM − 0.5) in
//!   and out.
//! - The tone, fitted to −60 dB sweeps: a fixed base ([`BASE`], within
//!   0.16 dB) and per CHAR_MODE a change at CHARACTER 1 ([`CHARACTER`],
//!   within 0.41, 1.16 and 0.94 dB) whose gains in dB scale with CHARACTER
//!   (CHARACTER 0.5 sits midway in dB, measured in two modes).
//! - MIX crossfades the dry input back in (`mix_dry`).
//!
//! - COMPRESS is the engine's compressor before all of it ([`SQUEEZE`],
//!   fitted to level steps), ATTACK and RELEASE its time constants (burst
//!   envelopes), HPF_MODE a one-pole high-pass on its detector and a fixed
//!   loss ([`DETECTOR`], stepped tones).
//!
//!   Both also move its steady gain ([`timing_db`]).
//!
//! Not carried: channel link (linked; no knob sets it), and the hard clip
//! at ±1 after the trim's make-up.

use ir::Stage::{In, Out};
use kontra_ir as ir;

/// Points per curve, evenly over inputs 0..=1 at the saturator (after
/// [`BASE`], whose 1 kHz gain of 1.0695 is divided out).
const POINTS: usize = 49;

/// The positive half of each odd curve per SAT_MODE (Mild, Moderate, Hot) at
/// SATURATION 0, 0.25, 0.5, 0.75 and 1.
#[rustfmt::skip]
static CURVES: [[[f32; POINTS]; 5]; 3] = [
    // Mild
    [
        [
            0.0000, 0.0201, 0.0400, 0.0598, 0.0795, 0.0992, 0.1187, 0.1380,
            0.1568, 0.1751, 0.1929, 0.2102, 0.2271, 0.2435, 0.2595, 0.2750,
            0.2902, 0.3048, 0.3191, 0.3330, 0.3466, 0.3597, 0.3725, 0.3850,
            0.3972, 0.4090, 0.4205, 0.4317, 0.4426, 0.4532, 0.4635, 0.4735,
            0.4833, 0.4928, 0.5021, 0.5111, 0.5199, 0.5285, 0.5368, 0.5450,
            0.5529, 0.5607, 0.5682, 0.5756, 0.5829, 0.5899, 0.5968, 0.6035,
            0.6100,
        ],
        [
            0.0000, 0.0240, 0.0483, 0.0729, 0.0976, 0.1221, 0.1465, 0.1703,
            0.1934, 0.2157, 0.2373, 0.2582, 0.2783, 0.2977, 0.3164, 0.3344,
            0.3518, 0.3684, 0.3845, 0.3999, 0.4148, 0.4292, 0.4430, 0.4563,
            0.4691, 0.4814, 0.4932, 0.5047, 0.5156, 0.5262, 0.5364, 0.5462,
            0.5557, 0.5648, 0.5736, 0.5819, 0.5903, 0.5988, 0.6065, 0.6137,
            0.6208, 0.6276, 0.6342, 0.6406, 0.6468, 0.6527, 0.6585, 0.6642,
            0.6695,
        ],
        [
            0.0000, 0.0318, 0.0623, 0.0904, 0.1162, 0.1393, 0.1593, 0.1761,
            0.1901, 0.2014, 0.2107, 0.2181, 0.2242, 0.2291, 0.2330, 0.2362,
            0.2388, 0.2409, 0.2426, 0.2440, 0.2452, 0.2461, 0.2469, 0.2476,
            0.2482, 0.2487, 0.2491, 0.2495, 0.2498, 0.2500, 0.2503, 0.2505,
            0.2507, 0.2508, 0.2510, 0.2511, 0.2512, 0.2513, 0.2514, 0.2515,
            0.2516, 0.2517, 0.2518, 0.2518, 0.2519, 0.2519, 0.2520, 0.2520,
            0.2521,
        ],
        [
            0.0000, 0.0336, 0.0643, 0.0898, 0.1108, 0.1270, 0.1391, 0.1477,
            0.1538, 0.1580, 0.1610, 0.1630, 0.1645, 0.1655, 0.1663, 0.1668,
            0.1672, 0.1675, 0.1678, 0.1679, 0.1681, 0.1683, 0.1684, 0.1685,
            0.1685, 0.1686, 0.1687, 0.1687, 0.1688, 0.1688, 0.1689, 0.1689,
            0.1690, 0.1690, 0.1691, 0.1691, 0.1691, 0.1692, 0.1692, 0.1692,
            0.1692, 0.1693, 0.1693, 0.1693, 0.1693, 0.1693, 0.1693, 0.1694,
            0.1694,
        ],
        [
            0.0000, 0.0346, 0.0638, 0.0827, 0.0953, 0.1028, 0.1071, 0.1094,
            0.1108, 0.1115, 0.1120, 0.1122, 0.1124, 0.1126, 0.1127, 0.1128,
            0.1129, 0.1129, 0.1130, 0.1130, 0.1130, 0.1131, 0.1131, 0.1131,
            0.1131, 0.1131, 0.1132, 0.1132, 0.1132, 0.1132, 0.1132, 0.1133,
            0.1134, 0.1135, 0.1135, 0.1136, 0.1137, 0.1137, 0.1138, 0.1138,
            0.1139, 0.1139, 0.1140, 0.1140, 0.1140, 0.1141, 0.1141, 0.1141,
            0.1142,
        ],
    ],
    // Moderate
    [
        [
            0.0000, 0.0200, 0.0398, 0.0595, 0.0791, 0.0988, 0.1183, 0.1374,
            0.1561, 0.1743, 0.1919, 0.2091, 0.2258, 0.2420, 0.2578, 0.2731,
            0.2879, 0.3024, 0.3164, 0.3300, 0.3432, 0.3560, 0.3685, 0.3806,
            0.3924, 0.4038, 0.4150, 0.4258, 0.4363, 0.4465, 0.4564, 0.4660,
            0.4753, 0.4844, 0.4932, 0.5018, 0.5102, 0.5183, 0.5262, 0.5339,
            0.5414, 0.5487, 0.5558, 0.5627, 0.5695, 0.5761, 0.5825, 0.5888,
            0.5948,
        ],
        [
            0.0000, 0.0268, 0.0532, 0.0791, 0.1046, 0.1296, 0.1538, 0.1770,
            0.1989, 0.2194, 0.2385, 0.2563, 0.2728, 0.2881, 0.3020, 0.3148,
            0.3264, 0.3369, 0.3464, 0.3549, 0.3626, 0.3694, 0.3754, 0.3807,
            0.3854, 0.3895, 0.3930, 0.3960, 0.3986, 0.4008, 0.4026, 0.4041,
            0.4053, 0.4063, 0.4070, 0.4076, 0.4080, 0.4083, 0.4085, 0.4086,
            0.4087, 0.4087, 0.4088, 0.4088, 0.4088, 0.4088, 0.4088, 0.4088,
            0.4088,
        ],
        [
            0.0000, 0.0357, 0.0664, 0.0876, 0.1005, 0.1051, 0.1057, 0.1057,
            0.1057, 0.1057, 0.1057, 0.1057, 0.1058, 0.1057, 0.1057, 0.1058,
            0.1060, 0.1060, 0.1060, 0.1059, 0.1058, 0.1058, 0.1058, 0.1058,
            0.1059, 0.1061, 0.1063, 0.1065, 0.1066, 0.1067, 0.1067, 0.1068,
            0.1068, 0.1068, 0.1068, 0.1068, 0.1068, 0.1067, 0.1067, 0.1067,
            0.1067, 0.1068, 0.1068, 0.1068, 0.1069, 0.1069, 0.1069, 0.1069,
            0.1070,
        ],
        [
            0.0000, 0.0387, 0.0658, 0.0704, 0.0706, 0.0706, 0.0707, 0.0707,
            0.0707, 0.0708, 0.0708, 0.0707, 0.0707, 0.0711, 0.0713, 0.0714,
            0.0714, 0.0713, 0.0713, 0.0714, 0.0715, 0.0715, 0.0716, 0.0716,
            0.0716, 0.0717, 0.0717, 0.0718, 0.0718, 0.0720, 0.0725, 0.0728,
            0.0731, 0.0734, 0.0737, 0.0740, 0.0742, 0.0744, 0.0746, 0.0748,
            0.0750, 0.0752, 0.0754, 0.0755, 0.0756, 0.0758, 0.0759, 0.0760,
            0.0761,
        ],
        [
            0.0000, 0.0265, 0.0447, 0.0472, 0.0472, 0.0473, 0.0475, 0.0474,
            0.0477, 0.0482, 0.0486, 0.0489, 0.0495, 0.0502, 0.0508, 0.0512,
            0.0516, 0.0519, 0.0522, 0.0524, 0.0526, 0.0528, 0.0530, 0.0531,
            0.0532, 0.0533, 0.0534, 0.0535, 0.0536, 0.0537, 0.0537, 0.0538,
            0.0539, 0.0539, 0.0540, 0.0540, 0.0540, 0.0541, 0.0541, 0.0541,
            0.0542, 0.0542, 0.0542, 0.0542, 0.0543, 0.0543, 0.0543, 0.0543,
            0.0543,
        ],
    ],
    // Hot
    [
        [
            0.0000, 0.0207, 0.0410, 0.0607, 0.0789, 0.0955, 0.1107, 0.1246,
            0.1374, 0.1491, 0.1598, 0.1697, 0.1789, 0.1875, 0.1954, 0.2029,
            0.2098, 0.2163, 0.2225, 0.2283, 0.2337, 0.2389, 0.2438, 0.2484,
            0.2528, 0.2569, 0.2609, 0.2647, 0.2683, 0.2717, 0.2750, 0.2781,
            0.2811, 0.2840, 0.2867, 0.2893, 0.2918, 0.2942, 0.2966, 0.2988,
            0.3009, 0.3030, 0.3050, 0.3069, 0.3088, 0.3106, 0.3123, 0.3140,
            0.3156,
        ],
        [
            0.0000, 0.0330, 0.0607, 0.0779, 0.0911, 0.1019, 0.1107, 0.1180,
            0.1243, 0.1296, 0.1343, 0.1383, 0.1419, 0.1451, 0.1480, 0.1505,
            0.1528, 0.1549, 0.1568, 0.1586, 0.1602, 0.1617, 0.1630, 0.1643,
            0.1655, 0.1666, 0.1676, 0.1685, 0.1694, 0.1703, 0.1711, 0.1718,
            0.1725, 0.1731, 0.1737, 0.1743, 0.1749, 0.1754, 0.1759, 0.1763,
            0.1768, 0.1772, 0.1776, 0.1780, 0.1784, 0.1787, 0.1790, 0.1794,
            0.1797,
        ],
        [
            0.0000, 0.0271, 0.0472, 0.0538, 0.0572, 0.0595, 0.0611, 0.0622,
            0.0630, 0.0636, 0.0640, 0.0643, 0.0645, 0.0646, 0.0647, 0.0647,
            0.0648, 0.0648, 0.0648, 0.0649, 0.0649, 0.0649, 0.0649, 0.0650,
            0.0650, 0.0650, 0.0650, 0.0650, 0.0650, 0.0650, 0.0651, 0.0651,
            0.0651, 0.0651, 0.0651, 0.0651, 0.0651, 0.0651, 0.0651, 0.0652,
            0.0652, 0.0652, 0.0652, 0.0652, 0.0652, 0.0652, 0.0652, 0.0652,
            0.0652,
        ],
        [
            0.0000, 0.0188, 0.0318, 0.0336, 0.0337, 0.0338, 0.0339, 0.0339,
            0.0340, 0.0341, 0.0341, 0.0342, 0.0342, 0.0343, 0.0346, 0.0350,
            0.0353, 0.0356, 0.0358, 0.0360, 0.0361, 0.0363, 0.0364, 0.0365,
            0.0366, 0.0367, 0.0368, 0.0369, 0.0369, 0.0370, 0.0371, 0.0371,
            0.0372, 0.0372, 0.0373, 0.0373, 0.0373, 0.0374, 0.0374, 0.0374,
            0.0375, 0.0375, 0.0375, 0.0375, 0.0375, 0.0376, 0.0376, 0.0376,
            0.0376,
        ],
        [
            0.0000, 0.0107, 0.0182, 0.0194, 0.0196, 0.0197, 0.0198, 0.0199,
            0.0199, 0.0200, 0.0200, 0.0201, 0.0201, 0.0201, 0.0201, 0.0202,
            0.0202, 0.0202, 0.0202, 0.0202, 0.0202, 0.0203, 0.0203, 0.0203,
            0.0203, 0.0203, 0.0203, 0.0203, 0.0203, 0.0203, 0.0203, 0.0203,
            0.0203, 0.0203, 0.0204, 0.0204, 0.0204, 0.0204, 0.0204, 0.0204,
            0.0204, 0.0204, 0.0204, 0.0204, 0.0204, 0.0204, 0.0204, 0.0204,
            0.0204,
        ],
    ],
];

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
/// 16.6 kHz, then before the curve an 8 Hz one-pole high-pass: what rings
/// for 100 ms or more after a loud input stops (a tail at DC falling 4 to
/// 5 dB per 10 ms from 44 dB under the tone, which the saturator lifts).
/// Static, so its sections are worked out by hand: the peak's
/// `10^(0.784/40)` = 1.0454 and the shelf's `10^(5.036/20)` = 1.7853, the
/// gain `10^(0.5475/20)` = 1.0651 on the peak's numerator.
static BASE: [(ir::Stage, ir::Section); 3] = [
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
    (
        ir::Stage::Pre,
        ir::Section {
            hz: 8.0,
            num: [0.0, 1.0, 0.0],
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

/// The record's settings: each 0..=1 but the modes, 0..=2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    pub trim: f32,
    pub saturation: f32,
    pub compress: f32,
    pub character: f32,
    pub mix: f32,
    pub attack: f32,
    pub release: f32,
    pub hpf_mode: f32,
    pub sat_mode: f32,
    pub char_mode: f32,
}

/// The INIT record.
pub const INIT: Knobs = Knobs {
    trim: 0.5,
    saturation: 0.0,
    compress: 0.0,
    character: 0.0,
    mix: 1.0,
    attack: 0.3667,
    release: 0.4444,
    hpf_mode: 0.0,
    sat_mode: 0.0,
    char_mode: 0.0,
};

/// The compressor's static curve at COMPRESS 0, 0.25, 0.5, 0.75 and 1:
/// threshold dB, ratio, knee dB and make-up dB, fitted to 1 kHz tones from
/// −48 to −8 dB peak, at 1 down to −58 dB too (a burst's quiet tail; rms
/// error 0.06, 0.10, 0.19 and 0.24 dB); linear between. At 0 it passes.
const SQUEEZE: [[f32; 4]; 5] = [
    [-26.5, 1.0, 19.0, 0.0],
    [-26.5, 2.41, 19.0, 3.48],
    [-33.5, 3.85, 24.0, 8.0],
    [-42.0, 4.71, 25.0, 15.96],
    [-68.0, 5.76, 27.0, 37.54],
];

/// HPF_MODE 1 and 2: the detector's high-pass (Hz) and the output's loss
/// (dB), from stepped tones at COMPRESS 1 and level steps at COMPRESS 0.
const DETECTOR: [(f32, f32); 2] = [(100.0, -1.5), (300.0, -3.0)];

/// The record as the engine's compressor into its waveshaper, the
/// compressor built at every COMPRESS (passing at 0), so a script or knob
/// raising it later is heard; a group rack runs both on each voice. MIX scales the wet path (the slot's dry level carries the rest,
/// [`mix_dry`]).
pub fn supergt(k: Knobs) -> ir::EffectKind {
    let [trim, saturation, character, mix, compress, attack, release] = [
        k.trim,
        k.saturation,
        k.character,
        k.mix,
        k.compress,
        k.attack,
        k.release,
    ]
    .map(|x| x.clamp(0.0, 1.0));
    let pick = |m: f32| m.round().clamp(0.0, 2.0) as usize;
    let [mild, moderate, hot] = &CURVES;
    let curves = match pick(k.sat_mode) {
        1 => moderate,
        2 => hot,
        _ => mild,
    };
    let at = saturation * 4.0;
    let i = (at as usize).min(3);
    // Rows i and i + 1, adjacent in the table.
    let rows = curves.get(i..i + 2).map_or(&[][..], <[_]>::as_flattened);
    let trim_db = 2.0 * TRIM_DB * (trim - 0.5);
    let [first, second, third] = CHARACTER;
    let (out_db, [ls_hz, ls_db], [pk_hz, pk_q, pk_db], [hs_hz, hs_db]) = match pick(k.char_mode) {
        1 => second,
        2 => third,
        _ => first,
    };
    let (sidechain_hz, loss_db) = match pick(k.hpf_mode) {
        0 => (None, 0.0),
        m => DETECTOR
            .get(m - 1)
            .map_or((None, 0.0), |&(hz, db)| (Some(hz), db)),
    };
    let c = character;
    let shaper = ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Blend {
            drive: db(trim_db),
            rows,
            t: at - i as f32,
        },
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections {
            fixed: &BASE,
            moving: [
                Some((
                    Out,
                    peak(pk_hz, pk_q, c * pk_db, db(c * out_db + loss_db) * mix),
                )),
                Some((Out, low_shelf(ls_hz, c * ls_db))),
                Some((Out, high_shelf(hs_hz, c * hs_db))),
            ],
        },
    };
    let at = compress * 4.0;
    let j = (at as usize).min(3);
    let row = |n: usize| SQUEEZE.get(n).copied().unwrap_or([0.0, 1.0, 0.0, 0.0]);
    let [mut threshold, mut ratio, mut knee, mut makeup] = row(j);
    let t = at - j as f32;
    for (a, b) in [&mut threshold, &mut ratio, &mut knee, &mut makeup]
        .into_iter()
        .zip(row(j + 1))
    {
        *a += (b - *a) * t;
    }
    ir::EffectKind::Strip(ir::Strip {
        compressor: ir::Compressor {
            // The trim is the gain into it: the same reduction at a
            // threshold that much lower on the untrimmed input.
            threshold_db: threshold - trim_db,
            ratio,
            knee_db: knee,
            // 1 ms at ATTACK 0, 1.6 ms at the INIT 0.3667, 30 ms at 1 (gain
            // envelopes of −12 dB bursts); 63, 288 and 770 ms at RELEASE 0,
            // 0.5 and 1 (the recovery after them).
            attack_ms: 30f32.powf(attack * attack),
            release_ms: (4.143 + 3.57 * release - 1.06 * release * release).exp(),
            // The timing shifts were measured at COMPRESS 1; they fade in
            // by 0.25 so COMPRESS 0 passes at unity.
            makeup_db: makeup + timing_db(attack, release) * (4.0 * compress).min(1.0),
            makeup_slope: 0.0,
            link: true,
            mix: 1.0,
            detector: ir::Detector::Input,
            sidechain_hz,
        },
        shaper,
    })
}

/// The steady gain ATTACK and RELEASE add at COMPRESS 1, loud and quiet
/// alike (−12 and −50 dB bursts, against the INIT 0.3667 and 0.4444 the
/// curve was fitted at): +0.8 dB at ATTACK 0, none from the INIT to 0.5,
/// −9 dB at 1; 3.3 dB per RELEASE unit.
fn timing_db(attack: f32, release: f32) -> f32 {
    let attack_db = if attack < INIT.attack {
        0.8 * (1.0 - attack / INIT.attack)
    } else {
        -18.0 * (attack - 0.5).max(0.0)
    };
    attack_db + 3.3 * (release - INIT.release)
}

/// The saved record.
#[must_use]
pub fn saved(public: &[u8]) -> Option<Knobs> {
    let f = |n: usize| Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    let mode = |n: usize| {
        let saved = i32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?);
        Some((2 - saved).clamp(0, 2) as f32)
    };
    Some(Knobs {
        trim: f(0)?,
        saturation: f(4)?,
        compress: f(8)?,
        character: f(12)?,
        mix: f(16)?,
        attack: f(20)?,
        release: f(24)?,
        hpf_mode: mode(28)?,
        sat_mode: mode(32)?,
        char_mode: mode(36)?,
    })
}

/// The slot's dry level for the saved MIX at `output`: the input crossfaded
/// back in.
#[must_use]
pub fn mix_dry(public: &[u8], output: f32) -> f32 {
    saved(public).map_or(0.0, |k| (1.0 - k.mix.clamp(0.0, 1.0)) * output)
}

/// The saved record as a script moves it: every knob and mode but MIX
/// (as saved), in [`rows`]' order.
pub fn model(public: &[u8]) -> Option<ir::KnobModel> {
    use ir::SlotKnob::Knob;
    let k = saved(public)?;
    Some(ir::KnobModel {
        knobs: (0..10).map(Knob).collect(),
        saved: [
            k.trim,
            k.saturation,
            k.character,
            k.sat_mode,
            k.char_mode,
            k.compress,
            k.attack,
            k.release,
            k.hpf_mode,
            k.mix,
        ]
        .into(),
        build: |p| {
            let [
                trim,
                saturation,
                character,
                sat_mode,
                char_mode,
                compress,
                attack,
                release,
                hpf_mode,
                mix,
            ] = *p
            else {
                return None;
            };
            Some(supergt(Knobs {
                trim,
                saturation,
                compress,
                character,
                mix,
                attack,
                release,
                hpf_mode,
                sat_mode,
                char_mode,
            }))
        },
    })
}

/// The KSP names of [`model`]'s knobs: the floats as set, the modes by their
/// index (a script sets SAT_MODE 1 as the raw value 1).
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 9] {
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
        ("$ENGINE_PAR_SUPERGT_COMPRESS", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_SUPERGT_ATTACK", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_SUPERGT_RELEASE", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_SUPERGT_HPF_MODE", Slot(Knob(8)), index()),
    ]
}
