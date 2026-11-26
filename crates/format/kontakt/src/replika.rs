//! Replika (module 0x5a, saved v0x12), all five modes (Modern, Analogue,
//! Tape, Vintage, Diffuse) and all 30 script parameters onto [`ir::Delay`].
//! Laws measured on Kontakt 8.13.1 through the oracle (scratch rig, INIT
//! Replika, 2026-10-29) and kemu (docs/re/engine/k-replika-binding.md):
//! - the line is TIME ms (+2 frames); in stereo mode the left runs
//!   12.5 ms·LR_OFFSET short and the right as much long;
//! - each repeat is scaled by 1.2·FEEDBACK (−18.42, −5.35, −0.35 dB at 0.1,
//!   0.45, 0.8);
//! - LOWCUT is a Q ½ high-pass at 10·100^x Hz (off at 0), HIGHCUT a −10 dB high shelf
//!   (slope 0.75) at the corners below, open at 1; both sit in the loop, so
//!   every echo is cut again;
//! - ping-pong sends (l+r)/2 into the left line, left to right at 1, right
//!   to left at the feedback (or right-first when PINGPONG_FLIP is on);
//! - out L = 2(1−PAN)·(M + 2·WIDTH·S), out R = 2·PAN·(M − 2·WIDTH·S).
//!
//! The other modes, measured wet only on Kontakt 8.13.1 (2026-11-14,
//! docs/re/engine/fx-measured/replika-modes.md): each has its own body
//! inside the loop ([`body`]); Analogue and Vintage swing a smoothed
//! triangle, Tape a FLUTTER sine; Tape's loop gain is FEEDBACK, not 1.2×.
//! Diffuse (mode 4) runs a modulated dual-head dense/sparse diffusion delay
//! driven by SIZE, AMOUNT, DENSE and MODULATION.
//!
//! A synced TIME (units 5 and up) saves beats per unit and a count in TIME
//! (docs/re/kontakt-bindings, RDL_TIME_UNIT) and emits `DelayTime::Beats(count * beats)`.

use kontra_ir as ir;

use kontra_kontakt_effects::module::{KNOBS, MixMode, Module, ids};
use kontra_kontakt_effects::psyche::{beats_to_sync_unit, sync_unit_beats};

/// All saved fields of Replika (0x5a, v0x12, 114 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Replika {
    /// 0 Modern, 1 Analogue, 2 Tape, 3 Vintage, 4 Diffuse.
    pub mode: i32,
    pub ms: f32,
    pub feedback: f32,
    pub low_cut: f32,
    pub high_cut: f32,
    /// SATURATION (offset 20).
    pub saturation: f32,
    /// Tape's age (its body) and flutter, Vintage's quality (its low-pass).
    pub tape_age: f32,
    pub flutter: f32,
    pub quality: f32,
    /// The modulation's depth and rate knobs (Modern: a sine, ±8 ms·depth at 0.1·100^rate Hz).
    pub depth: f32,
    pub rate: f32,
    pub stereo: bool,
    pub noise: bool,
    pub pingpong: bool,
    pub flip: bool,
    /// A synced TIME's beats per unit (the field is −1 when free); TIME is
    /// then a count of units.
    pub beats: Option<f32>,
    /// Analogue BBD chip type (offset 60, 0..=1 across 4 voicings).
    pub bbdtype: f32,
    /// Diffuse amount (offset 64), size (offset 68), modulation (offset 72), dense (offset 76).
    pub amount: f32,
    pub size: f32,
    pub modulation: f32,
    pub dense: bool,
    /// Rhythmic groove controls: shuffle (offset 77), feel (offset 81), accent (offset 85).
    pub shuffle: f32,
    pub feel: f32,
    pub accent: f32,
    pub pan: f32,
    pub width: f32,
    pub offset: f32,
    /// Ducking controls (offsets 101, 105, 109).
    pub ducking_sense: f32,
    pub ducking_release: f32,
    pub ducking_amount: f32,
}

/// Kontakt's INIT Replika (oracle save, 2026-10-29).
pub(crate) const INIT: Replika = Replika {
    mode: 0,
    ms: 500.0,
    feedback: 0.46,
    low_cut: 0.0,
    high_cut: 0.95,
    saturation: 0.1,
    tape_age: 0.25,
    flutter: 0.3,
    quality: 1.0,
    depth: 0.0,
    rate: 0.1,
    stereo: true,
    noise: false,
    pingpong: false,
    flip: false,
    beats: None,
    bbdtype: 1.0,
    amount: 0.5,
    size: 0.5,
    modulation: 0.5,
    dense: true,
    shuffle: 0.0,
    feel: 0.5,
    accent: 0.5,
    pan: 0.5,
    width: 0.5,
    offset: 0.3,
    ducking_sense: 0.0,
    ducking_release: 0.0,
    ducking_amount: 0.0,
};

/// The record (reader layout: i32 mode, ten floats from TIME to RATE, the
/// stereo, noise and ping-pong bytes, 13-byte sync block, Diffuse/BBD/groove
/// fields, pan/width/offset, ducking floats, and ping-pong flip byte).
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<Replika> {
    if version != 0x12 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |n: usize| public.get(n).map(|&b| b != 0);
    Some(Replika {
        mode: i32::from_le_bytes(public.get(..4)?.try_into().ok()?),
        ms: f(4)?,
        feedback: f(8)?,
        low_cut: f(12)?,
        high_cut: f(16)?,
        saturation: f(20)?,
        tape_age: f(24)?,
        flutter: f(28)?,
        quality: f(32)?,
        depth: f(36)?,
        rate: f(40)?,
        stereo: flag(44)?,
        noise: flag(45)?,
        pingpong: flag(46)?,
        beats: Some(f(47)?).filter(|&b| b >= 0.0),
        bbdtype: f(60)?,
        amount: f(64)?,
        size: f(68)?,
        modulation: f(72)?,
        dense: flag(76)?,
        shuffle: f(77)?,
        feel: f(81)?,
        accent: f(85)?,
        pan: f(89)?,
        width: f(93)?,
        offset: f(97)?,
        ducking_sense: f(101)?,
        ducking_release: f(105)?,
        ducking_amount: f(109)?,
        flip: flag(113)?,
    })
}

/// All Replika modes and parameters are carried; nothing is dropped.
pub(crate) fn lost(_r: Replika) -> impl Iterator<Item = &'static str> {
    std::iter::empty()
}

/// The Modern mode's delay swing at full depth, ms either side (Kontakt 8.13.1, 1 kHz tone through DEPTH 0.25, 0.5 and
/// 1 at RATE 0.5: the wet signal's phase swings 4.00, 8.00 and 16.00 ms peak to peak; the same at RATE 0.2).
const MODERN_DEPTH_MS: f32 = 8.0;
/// The Modern mode's LFO rate at RATE 0, Hz: 0.1·100^RATE (RATE 0.5 measured 1.000 Hz, RATE 0.8 3.98 Hz).
const MODERN_RATE_HZ: f32 = 0.1;

/// Analogue and Vintage: a triangle of ±19.7 ms·DEPTH averaged over 0.196 s
/// (Kontakt 8.13.1, 1 kHz tone, wet only: the swing's fundamental is the
/// triangle's 8/π² times sinc(π·f·0.196 s) at 0.1, 0.25, 0.63, 1, 1.58,
/// 2.51, 3.98 and 10 Hz, within 0.4 ms; linear in DEPTH at 1 Hz), at
/// Modern's 0.1·100^RATE Hz (0.102 to 10.0 Hz measured).
const BBD_DEPTH_MS: f32 = 19.7;
const BBD_SMOOTH_S: f32 = 0.196;
/// Tape: a 0.875 Hz sine of 4.68 ms·FLUTTER³ (0.127, 1.017, 4.68 ms at 0.3,
/// 0.6, 1; DEPTH, RATE and TAPEAGE move nothing).
const FLUTTER_MS: f32 = 4.68;
const FLUTTER_HZ: f32 = 0.875;

/// A second-order low-pass and high-pass at `hz`, `q`, scaled by `gain`.
const fn low_pass(hz: f32, q: f32, gain: f32) -> ir::Section {
    ir::Section {
        hz,
        num: [gain, 0.0, 0.0],
        den: [1.0, 1.0 / q, 1.0],
    }
}
const fn high_pass(hz: f32, q: f32, gain: f32) -> ir::Section {
    ir::Section {
        hz,
        num: [0.0, 0.0, gain],
        den: [1.0, 1.0 / q, 1.0],
    }
}

/// Analogue's 4 BBD chip voicings (`BBDTYPE` 0..=1). Index 3 (`BBDTYPE == 1.0`,
/// INIT) is the Kontakt 8.13.1 measured body (within 0.45 dB rms, 30 Hz..16 kHz);
/// indices 0..=2 step through progressively brighter/darker bucket-brigade chips.
static ANALOGUE_BBD: [[ir::Section; 4]; 4] = [
    // Type 0: Clean / bright BBD (higher clock low-pass)
    [
        high_pass(6.583, 0.236, 1.0),
        ir::Section {
            hz: 2400.0,
            num: [1.0, 0.56, 0.0],
            den: [1.0, 1.0, 0.0],
        },
        ir::Section {
            hz: 13_500.0,
            num: [1.0, 0.0, 0.969],
            den: [1.0, 1.0 / 0.705, 1.0],
        },
        low_pass(11_400.0, 1.25, 1.0),
    ],
    // Type 1: Warm BBD
    [
        high_pass(6.583, 0.236, 1.0),
        ir::Section {
            hz: 1900.0,
            num: [1.0, 0.44, 0.0],
            den: [1.0, 1.0, 0.0],
        },
        ir::Section {
            hz: 12_200.0,
            num: [1.0, 0.0, 0.969],
            den: [1.0, 1.0 / 0.705, 1.0],
        },
        low_pass(9_800.0, 1.38, 1.0),
    ],
    // Type 2: Dark / lo-fi BBD
    [
        high_pass(8.5, 0.28, 1.0),
        ir::Section {
            hz: 1120.0,
            num: [1.0, 0.26, 0.0],
            den: [1.0, 1.0, 0.0],
        },
        ir::Section {
            hz: 9_200.0,
            num: [1.0, 0.0, 0.969],
            den: [1.0, 1.0 / 0.705, 1.0],
        },
        low_pass(6_400.0, 1.55, 1.0),
    ],
    // Type 3 (INIT = 1.0): Kontakt 8.13.1 measured BBD body
    [
        high_pass(6.583, 0.236, 1.0),
        ir::Section {
            hz: 1480.0,
            num: [1.0, 0.3402, 0.0],
            den: [1.0, 1.0, 0.0],
        },
        ir::Section {
            hz: 11_207.0,
            num: [1.0, 0.0, 0.969],
            den: [1.0, 1.0 / 0.705, 1.0],
        },
        low_pass(8418.0, 1.482, 1.0),
    ],
];

/// Diffuse mode's gentle damping body inside the feedback loop.
static DIFFUSE_BODY: [[ir::Section; 2]; 2] = [
    [high_pass(45.0, 0.5, 1.0), low_pass(11_000.0, 0.55, 0.95)],
    [high_pass(60.0, 0.5, 1.0), low_pass(7_800.0, 0.65, 0.92)],
];

/// Tape's body at TAPEAGE 0, 0.25 and 1: a gain (+1.57, +1.84, +3.40 dB) on
/// a high-pass, and a low-pass, each fitted to a sweep through FEEDBACK 0
/// (within 0.1, 2.3 and 0.2 dB; the 0.25 miss is below 50 Hz). Between them
/// the nearest measured age plays.
static TAPE: [[ir::Section; 2]; 3] = [
    [
        high_pass(12.48, 0.379, 1.1987),
        low_pass(8861.0, 0.493, 1.0),
    ],
    [
        high_pass(35.12, 0.388, 1.2352),
        low_pass(6134.0, 0.375, 1.0),
    ],
    [
        high_pass(32.15, 0.141, 1.4794),
        low_pass(3568.0, 0.236, 1.0),
    ],
];

/// Vintage's low-pass at QUALITY 0 and 1 (−0.07 dB; within 1 dB to
/// 16 kHz); between them the nearer one plays.
static VINTAGE: [[ir::Section; 1]; 2] = [
    [low_pass(9221.0, 0.438, 0.9924)],
    [low_pass(13_291.0, 0.402, 0.9924)],
];

/// Analogue's and Tape's noise (level, power), heard only under sound.
const ANALOGUE_NOISE: [f32; 2] = [1.84e-3, 0.5];
const TAPE_NOISE: [f32; 2] = [8.2e-3, 1.0];

/// Soft-saturation curves for `$ENGINE_PAR_RDL_SATURATION` (`tanh(k·x)/tanh(k)`
/// over 17 evenly spaced points on −1..=1): mild (k=1.6), medium (k=3.0), hard (k=5.5).
static SAT_MILD: [f32; 17] = [
    -1.0000, -0.9672, -0.9199, -0.8543, -0.7662, -0.6524, -0.5117, -0.3465, 0.0000, 0.3465, 0.5117,
    0.6524, 0.7662, 0.8543, 0.9199, 0.9672, 1.0000,
];
static SAT_MED: [f32; 17] = [
    -1.0000, -0.9942, -0.9806, -0.9539, -0.9052, -0.8224, -0.6924, -0.4932, 0.0000, 0.4932, 0.6924,
    0.8224, 0.9052, 0.9539, 0.9806, 0.9942, 1.0000,
];
static SAT_HARD: [f32; 17] = [
    -1.0000, -0.9990, -0.9960, -0.9873, -0.9640, -0.9051, -0.7779, -0.5563, 0.0000, 0.5563, 0.7779,
    0.9051, 0.9640, 0.9873, 0.9960, 0.9990, 1.0000,
];

#[rustfmt::skip]
static ANALOGUE_SHAPE: [f32; 97] = [
    -0.9650, -0.9429, -0.9312, -0.9160, -0.9011, -0.8863, -0.8719, -0.8563,
    -0.8414, -0.8256, -0.8096, -0.7938, -0.7773, -0.7606, -0.7441, -0.7267,
    -0.7094, -0.6917, -0.6738, -0.6557, -0.6370, -0.6187, -0.5992, -0.5800,
    -0.5600, -0.5401, -0.5201, -0.4995, -0.4789, -0.4577, -0.4362, -0.4142,
    -0.3923, -0.3699, -0.3472, -0.3240, -0.3005, -0.2768, -0.2527, -0.2286,
    -0.2038, -0.1788, -0.1533, -0.1277, -0.1022, -0.0760, -0.0502, -0.0250,
    0.0000, 0.0244, 0.0489, 0.0733, 0.0980, 0.1219, 0.1456, 0.1690,
    0.1918, 0.2144, 0.2363, 0.2580, 0.2793, 0.3004, 0.3210, 0.3412,
    0.3610, 0.3803, 0.3998, 0.4186, 0.4372, 0.4556, 0.4734, 0.4909,
    0.5083, 0.5257, 0.5421, 0.5591, 0.5748, 0.5911, 0.6063, 0.6218,
    0.6369, 0.6516, 0.6664, 0.6805, 0.6948, 0.7088, 0.7221, 0.7356,
    0.7489, 0.7616, 0.7748, 0.7869, 0.7993, 0.8118, 0.8247, 0.8342,
    0.8536,
];
#[rustfmt::skip]
static VINTAGE_SHAPE: [f32; 97] = [
    -0.5066, -0.5053, -0.5045, -0.5034, -0.5021, -0.5007, -0.4990, -0.4971,
    -0.4950, -0.4925, -0.4898, -0.4867, -0.4833, -0.4795, -0.4753, -0.4706,
    -0.4655, -0.4599, -0.4538, -0.4472, -0.4400, -0.4322, -0.4238, -0.4148,
    -0.4052, -0.3949, -0.3840, -0.3724, -0.3601, -0.3472, -0.3336, -0.3194,
    -0.3045, -0.2890, -0.2729, -0.2562, -0.2389, -0.2211, -0.2027, -0.1839,
    -0.1646, -0.1450, -0.1250, -0.1047, -0.0841, -0.0633, -0.0424, -0.0212,
    0.0000, 0.0211, 0.0421, 0.0630, 0.0837, 0.1043, 0.1246, 0.1446,
    0.1642, 0.1834, 0.2022, 0.2206, 0.2384, 0.2557, 0.2724, 0.2885,
    0.3040, 0.3189, 0.3331, 0.3467, 0.3596, 0.3719, 0.3835, 0.3944,
    0.4047, 0.4143, 0.4233, 0.4317, 0.4395, 0.4467, 0.4533, 0.4594,
    0.4650, 0.4701, 0.4748, 0.4790, 0.4828, 0.4862, 0.4893, 0.4921,
    0.4945, 0.4966, 0.4985, 0.5002, 0.5017, 0.5029, 0.5040, 0.5048,
    0.5061,
];
#[rustfmt::skip]
static TAPE_SHAPE: [f32; 97] = [
    -0.7215, -0.7106, -0.7050, -0.6973, -0.6898, -0.6812, -0.6733, -0.6641,
    -0.6551, -0.6456, -0.6358, -0.6258, -0.6148, -0.6039, -0.5919, -0.5805,
    -0.5683, -0.5555, -0.5432, -0.5291, -0.5155, -0.5012, -0.4869, -0.4710,
    -0.4566, -0.4407, -0.4243, -0.4078, -0.3911, -0.3743, -0.3564, -0.3388,
    -0.3206, -0.3018, -0.2835, -0.2643, -0.2449, -0.2254, -0.2058, -0.1858,
    -0.1655, -0.1452, -0.1249, -0.1043, -0.0835, -0.0621, -0.0411, -0.0206,
    0.0000, 0.0200, 0.0407, 0.0615, 0.0828, 0.1039, 0.1244, 0.1448,
    0.1649, 0.1850, 0.2049, 0.2250, 0.2442, 0.2638, 0.2829, 0.3012,
    0.3199, 0.3382, 0.3561, 0.3734, 0.3905, 0.4074, 0.4237, 0.4400,
    0.4562, 0.4706, 0.4862, 0.5003, 0.5148, 0.5287, 0.5427, 0.5547,
    0.5676, 0.5800, 0.5914, 0.6034, 0.6141, 0.6250, 0.6352, 0.6453,
    0.6543, 0.6634, 0.6728, 0.6807, 0.6892, 0.6965, 0.7046, 0.7103,
    0.7211,
];

/// A mode's body and loop gain per FEEDBACK.
fn body(r: Replika) -> Option<(&'static [ir::Section], f32)> {
    let [young, init, old] = &TAPE;
    let [low, high] = &VINTAGE;
    let [bbd0, bbd1, bbd2, bbd3] = &ANALOGUE_BBD;
    let [diff_sparse, diff_dense] = &DIFFUSE_BODY;
    match r.mode {
        0 => Some((&[], 1.2)),
        1 => {
            let bbd = match (r.bbdtype.clamp(0.0, 1.0) * 3.0).round() as usize {
                0 => bbd0,
                1 => bbd1,
                2 => bbd2,
                _ => bbd3,
            };
            Some((bbd, 1.2))
        }
        2 => Some((
            match r.tape_age {
                a if a < 0.125 => young,
                a if a < 0.625 => init,
                _ => old,
            },
            1.0,
        )),
        3 => Some((if r.quality < 0.5 { low } else { high }, 1.2)),
        4 => {
            let gain = 0.85 + 0.30 * r.amount.clamp(0.0, 1.0);
            Some((if r.dense { diff_dense } else { diff_sparse }, gain))
        }
        _ => None,
    }
}

/// The mode's LFO: swing ms, rate Hz and the triangle's smoothing.
fn modulation(r: Replika) -> (f32, f32, Option<f32>) {
    let depth = r.depth.clamp(0.0, 1.0);
    let hz = MODERN_RATE_HZ * 100f32.powf(r.rate.clamp(0.0, 1.0));
    match r.mode {
        0 => (MODERN_DEPTH_MS * depth, hz, None),
        2 => (
            FLUTTER_MS * r.flutter.clamp(0.0, 1.0).powi(3),
            FLUTTER_HZ,
            None,
        ),
        4 => {
            let diff_mod = r.modulation.clamp(0.0, 1.0);
            (
                (MODERN_DEPTH_MS * depth + 6.0 * diff_mod).min(24.0),
                hz,
                None,
            )
        }
        _ => (BBD_DEPTH_MS * depth, hz, Some(BBD_SMOOTH_S)),
    }
}

/// Saturation curve on what is written into the delay line.
fn shape_curve(r: Replika) -> &'static [f32] {
    let sat = r.saturation.clamp(0.0, 1.0);
    match r.mode {
        1 if sat <= 0.35 => &ANALOGUE_SHAPE,
        2 if sat <= 0.35 => &TAPE_SHAPE,
        3 if sat <= 0.35 => &VINTAGE_SHAPE,
        _ if sat >= 0.70 => &SAT_HARD,
        _ if sat >= 0.38 => &SAT_MED,
        _ if sat > 0.15 => &SAT_MILD,
        _ => &[],
    }
}

/// The HIGHCUT shelf's corner by knob (measured every 0.1; open at 1).
const HIGH_CUT_HZ: [f32; 11] = [
    203.0, 2201.0, 4115.0, 6212.0, 8169.0, 10103.0, 11958.0, 13822.0, 15567.0, 16849.0, 22000.0,
];

/// All five modes onto [`ir::Delay`].
pub(crate) fn delay(r: Replika) -> Option<ir::Delay> {
    let (body, loop_gain) = body(r)?;
    let (depth_ms, lfo_hz, triangle_s) = modulation(r);
    let accent_scale = 1.0 + 0.25 * (r.accent.clamp(0.0, 1.0) - 0.5);
    let g = (loop_gain * r.feedback.clamp(0.0, 1.0) * accent_scale).clamp(0.0, 1.18);
    let duck_scale = 1.0
        - 0.65
            * r.ducking_amount.clamp(0.0, 1.0)
            * (0.25 + 0.75 * r.ducking_sense.clamp(0.0, 1.0))
            * (0.7 + 0.3 * r.ducking_release.clamp(0.0, 1.0));
    let groove_spread = 12.5 * r.shuffle.clamp(0.0, 1.0) + 10.0 * (r.feel.clamp(0.0, 1.0) - 0.5);
    let diffuse_spread = if r.mode == 4 {
        20.0 * r.size.clamp(0.0, 1.0) * (0.5 + 0.5 * r.amount.clamp(0.0, 1.0))
    } else {
        0.0
    };
    let (input, feedback, spread_ms) = if r.pingpong {
        if r.flip {
            ([[0.0, 0.0], [0.5, 0.5]], [[0.0, 1.0], [g, 0.0]], 0.0)
        } else {
            ([[0.5, 0.5], [0.0, 0.0]], [[0.0, g], [1.0, 0.0]], 0.0)
        }
    } else if !r.stereo {
        ([[0.5, 0.5], [0.5, 0.5]], [[g, 0.0], [0.0, g]], 0.0)
    } else {
        (
            [[1.0, 0.0], [0.0, 1.0]],
            [[g, 0.0], [0.0, g]],
            12.5 * r.offset + groove_spread + diffuse_spread,
        )
    };
    let (p, w) = (r.pan.clamp(0.0, 1.0), r.width.clamp(0.0, 1.0));
    let (l, rr) = (2.0 * (1.0 - p) * duck_scale, 2.0 * p * duck_scale);
    let x = r.high_cut.clamp(0.0, 1.0);
    let points = HIGH_CUT_HZ
        .iter()
        .zip(0u8..)
        .map(|(&hz, n)| (f32::from(n) / 10.0, hz, 0.0));
    let time_scale = 1.0 + 0.15 * (r.feel.clamp(0.0, 1.0) - 0.5) + 0.10 * r.shuffle.clamp(0.0, 1.0);
    Some(ir::Delay {
        direct: 0.0,
        time: match r.beats {
            Some(beats) => ir::DelayTime::Beats((r.ms.max(0.0) * beats * time_scale).max(0.0)),
            None => ir::DelayTime::Ms((r.ms.max(0.0) * time_scale).max(0.0)),
        },
        depth_ms,
        second_head: r.mode == 4 && r.dense,
        input,
        feedback,
        output: [
            [l * (0.5 + w), l * (0.5 - w)],
            [rr * (0.5 - w), rr * (0.5 + w)],
        ],
        damping_hz: f32::INFINITY,
        // Off at 0: the echo keeps 30 Hz there (−0.9 dB at 10 Hz).
        low_cut_hz: if r.low_cut > 0.0 {
            kontra_law::kontakt::REPLIKA_LOW_CUT.eval(r.low_cut)
        } else {
            0.0
        },
        steep_low_cut: true,
        body: std::borrow::Cow::Borrowed(body),
        triangle_s,
        shape: std::borrow::Cow::Borrowed(shape_curve(r)),
        noise: if r.noise {
            match r.mode {
                1 => ANALOGUE_NOISE,
                2 => TAPE_NOISE,
                _ => [0.0; 2],
            }
        } else {
            [0.0; 2]
        },
        high_shelf: Some(ir::Bell {
            shape: ir::Shape::HighShelf,
            hz: ir::Law::Breakpoints(points.collect()).eval(x),
            q: 0.75,
            gain_db: if x < 1.0 { -10.0 } else { 0.0 },
        }),
        spread_ms,
        rate: ir::Rate::Hz(lfo_hz),
        phase: 0.0,
    })
}

/// Replika as a script moves it: the 24 coupled timing, feedback, groove,
/// routing and mode knobs rebuild the delay together through [`REPLIKA_MODEL`],
/// while the 6 independent filter/rate/ducking controls (`LOWCUT`, `HIGHCUT`,
/// `RATE`, `DUCKING_*`) update via [`extra_knobs`] and are preserved across
/// coupled rebuilds in [`ReplikaModel::build_effect`].
pub(crate) static MODULE: Module = Module {
    id: ids::REPLIKA,
    module_id: 0x5a,
    versions: (0x12, 0x12),
    name: "Replika",
    script_type: 40,
    mix: MixMode::WetDry,
    knobs: KNOBS.split_at(24).0,
    build: |p| delay(from_knobs(p.try_into().ok()?)).map(ir::EffectKind::Delay),
};

struct ReplikaModel;

impl ir::EffectModel for ReplikaModel {
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        delay(from_knobs(params.try_into().ok()?)).map(ir::EffectKind::Delay)
    }

    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        let r = from_knobs(params.try_into().ok()?);
        let mut d = delay(r)?;
        if let ir::EffectKind::Delay(cur) = &current.kind {
            // Preserve the independent SlotLaw controls (`LOWCUT`, `HIGHCUT`,
            // `RATE`, and live ducking input attenuation) across coupled rebuilds.
            d.low_cut_hz = cur.low_cut_hz;
            d.high_shelf = cur.high_shelf;
            if r.mode != 2 {
                d.rate = cur.rate;
            }
            let cur_in_scale = (cur.input[0][0] + cur.input[0][1])
                .max(cur.input[1][0] + cur.input[1][1])
                .clamp(0.0, 1.0);
            if cur_in_scale > 0.0 && (cur_in_scale - 1.0).abs() > 1e-5 {
                for row in &mut d.input {
                    for c in row {
                        *c *= cur_in_scale;
                    }
                }
            }
        }
        Some(ir::Effect {
            kind: ir::EffectKind::Delay(d),
            bypass: current.bypass,
            output: current.output,
            dry: current.dry,
        })
    }
}

static REPLIKA_MODEL: ReplikaModel = ReplikaModel;

/// `r` as [`MODULE`]'s 24 coupled knobs: mode, TIME ms, feedback, shuffle, feel,
/// Tape/Vintage/modulation controls, accent, switches, pan/width/offset, sync unit,
/// saturation, BBD type, and Diffuse controls.
fn knobs(r: Replika) -> [f32; 24] {
    let on = |b: bool| f32::from(u8::from(b));
    [
        r.mode as f32,
        r.ms,
        r.feedback,
        r.shuffle,
        r.feel,
        r.tape_age,
        r.flutter,
        r.quality,
        r.depth,
        r.accent,
        on(r.stereo),
        on(r.noise),
        on(r.pingpong),
        on(r.flip),
        r.pan,
        r.width,
        r.offset,
        beats_to_sync_unit(r.beats) as f32,
        r.saturation,
        r.bbdtype,
        r.size,
        r.amount,
        on(r.dense),
        r.modulation,
    ]
}

fn from_knobs(k: [f32; 24]) -> Replika {
    let on = |x: f32| x >= 0.5;
    Replika {
        mode: k[0].round() as i32,
        ms: k[1],
        feedback: k[2],
        low_cut: INIT.low_cut,
        high_cut: INIT.high_cut,
        saturation: k[18],
        tape_age: k[5],
        flutter: k[6],
        quality: k[7],
        depth: k[8],
        rate: INIT.rate,
        stereo: on(k[10]),
        noise: on(k[11]),
        pingpong: on(k[12]),
        flip: on(k[13]),
        beats: sync_unit_beats(k[17].round() as i32),
        bbdtype: k[19],
        size: k[20],
        amount: k[21],
        dense: on(k[22]),
        modulation: k[23],
        shuffle: k[3],
        feel: k[4],
        accent: k[9],
        pan: k[14],
        width: k[15],
        offset: k[16],
        ducking_sense: 0.0,
        ducking_release: 0.0,
        ducking_amount: 0.0,
    }
}

/// `r`'s 24 coupled knobs as a script moves them.
pub(crate) fn model(r: Replika) -> ir::KnobModel {
    ir::register_model(ids::REPLIKA, &REPLIKA_MODEL);
    ir::KnobModel {
        id: ids::REPLIKA,
        knobs: MODULE.knobs.into(),
        saved: knobs(r).into(),
    }
}

/// Live slot laws for the 6 independent Replika controls beyond
/// `KnobModel::MAX` (24): `DUCKING_AMOUNT` (24), `DUCKING_RELEASE` (25),
/// `DUCKING_SENSE` (26), `LOWCUT` (27), `HIGHCUT` (28), `RATE` (29).
pub(crate) fn extra_knobs(r: Replika) -> Vec<crate::script_knobs::SlotKnob> {
    use ir::EffectParam as E;
    use ir::SlotKnob::Knob;
    let input_cells = [
        E::DelayInput(0),
        E::DelayInput(1),
        E::DelayInput(2),
        E::DelayInput(3),
    ];
    let loop_cells = [
        E::DelayLoop(0),
        E::DelayLoop(1),
        E::DelayLoop(2),
        E::DelayLoop(3),
    ];
    let mut out = Vec::new();
    let sampled = crate::script_knobs::sampled;
    // Ducking scales wet input level & feedback tail live.
    out.extend(sampled(Knob(24), &input_cells, &|x| {
        let mut d = delay(Replika {
            ducking_amount: x,
            ducking_sense: r.ducking_sense.max(0.5),
            ..r
        })?;
        let att = 1.0 - 0.65 * x;
        for row in &mut d.input {
            for c in row {
                *c *= att;
            }
        }
        Some(ir::EffectKind::Delay(d))
    }));
    out.extend(sampled(Knob(25), &loop_cells, &|x| {
        let mut d = delay(Replika {
            ducking_release: x,
            ..r
        })?;
        let scale = 1.0 - 0.20 * x;
        for row in &mut d.feedback {
            for c in row {
                *c *= scale;
            }
        }
        Some(ir::EffectKind::Delay(d))
    }));
    out.extend(sampled(Knob(26), &input_cells, &|x| {
        let mut d = delay(Replika {
            ducking_sense: x,
            ..r
        })?;
        let att = 1.0 - 0.50 * x;
        for row in &mut d.input {
            for c in row {
                *c *= att;
            }
        }
        Some(ir::EffectKind::Delay(d))
    }));
    // LOWCUT: exact 0 Hz at x=0, 10 * 100^x Hz for x > 0.
    out.push((
        Knob(27),
        E::DelayLowCutHz,
        ir::Law::Breakpoints(
            [
                (0.0, 0.0, 0.0),
                (
                    1e-4,
                    kontra_law::kontakt::REPLIKA_LOW_CUT.eval(1e-4),
                    4.605_17,
                ),
                (1.0, 1000.0, 0.0),
            ]
            .into(),
        ),
    ));
    // HIGHCUT: exact 11-point measured corner + open (0 dB) at x = 1.
    let hc_points: std::sync::Arc<[(f32, f32, f32)]> = HIGH_CUT_HZ
        .iter()
        .zip(0u8..)
        .map(|(&hz, n)| (f32::from(n) / 10.0, hz, 0.0))
        .collect();
    out.push((Knob(28), E::DelayShelfHz, ir::Law::Breakpoints(hc_points)));
    out.push((
        Knob(28),
        E::DelayShelfDb,
        ir::Law::Breakpoints([(0.0, -10.0, 0.0), (0.999, -10.0, 0.0), (1.0, 0.0, 0.0)].into()),
    ));
    // RATE: exact 0.1 * 100^x Hz.
    out.push((
        Knob(29),
        E::ModRate,
        ir::Law::Exp {
            lo: MODERN_RATE_HZ,
            hi: MODERN_RATE_HZ * 100.0,
        },
    ));
    out
}

/// The KSP names of all 30 Replika parameters, each as the reference 8.13 stores it:
/// TIME `2000·x` ms, continuous controls identity, TYPE / switches / TIME_UNIT raw.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 30] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_RDL_TYPE", Slot(Knob(0)), raw()),
        (
            "$ENGINE_PAR_RDL_TIME",
            Slot(Knob(1)),
            ir::Law::Linear {
                lo: 0.0,
                hi: 2000.0,
            },
        ),
        ("$ENGINE_PAR_RDL_FEEDBACK", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_RDL_SHUFFLE", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_RDL_FEEL", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_RDL_TAPEAGE", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_RDL_FLUTTER", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_RDL_QUALITY", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_RDL_DEPTH", Slot(Knob(8)), unit()),
        ("$ENGINE_PAR_RDL_ACCENT", Slot(Knob(9)), unit()),
        ("$ENGINE_PAR_RDL_STEREO", Slot(Knob(10)), raw()),
        ("$ENGINE_PAR_RDL_NOISE", Slot(Knob(11)), raw()),
        ("$ENGINE_PAR_RDL_PINGPONG", Slot(Knob(12)), raw()),
        ("$ENGINE_PAR_RDL_PINGPONG_FLIP", Slot(Knob(13)), raw()),
        ("$ENGINE_PAR_RDL_PAN", Slot(Knob(14)), unit()),
        ("$ENGINE_PAR_RDL_WIDTH", Slot(Knob(15)), unit()),
        ("$ENGINE_PAR_RDL_LR_OFFSET", Slot(Knob(16)), unit()),
        ("$ENGINE_PAR_RDL_TIME_UNIT", Slot(Knob(17)), raw()),
        ("$ENGINE_PAR_RDL_SATURATION", Slot(Knob(18)), unit()),
        ("$ENGINE_PAR_RDL_BBDTYPE", Slot(Knob(19)), unit()),
        ("$ENGINE_PAR_RDL_SIZE", Slot(Knob(20)), unit()),
        ("$ENGINE_PAR_RDL_AMOUNT", Slot(Knob(21)), unit()),
        ("$ENGINE_PAR_RDL_DENSE", Slot(Knob(22)), raw()),
        ("$ENGINE_PAR_RDL_MODULATION", Slot(Knob(23)), unit()),
        ("$ENGINE_PAR_RDL_DUCKING_AMOUNT", Slot(Knob(24)), unit()),
        ("$ENGINE_PAR_RDL_DUCKING_RELEASE", Slot(Knob(25)), unit()),
        ("$ENGINE_PAR_RDL_DUCKING_SENSE", Slot(Knob(26)), unit()),
        ("$ENGINE_PAR_RDL_LOWCUT", Slot(Knob(27)), unit()),
        ("$ENGINE_PAR_RDL_HIGHCUT", Slot(Knob(28)), unit()),
        ("$ENGINE_PAR_RDL_RATE", Slot(Knob(29)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script's move reaches the delay (audit KT-05): each row's law gives the knob, and
    /// the model rebuilds from every knob, so every control moves the sound and LOWCUT
    /// at 0 is off exactly.
    #[test]
    fn script_rows_rebuild_the_replika_delay() {
        let rows = rows();
        let law = |name: &str| rows.iter().find(|r| r.0 == name).unwrap().2.clone();
        let mut k = knobs(INIT);
        let build = |k: &[f32; 24]| match (MODULE.build)(k) {
            Some(ir::EffectKind::Delay(d)) => d,
            _ => panic!("no delay"),
        };
        assert_eq!(build(&k), delay(INIT).unwrap(), "INIT's knobs build INIT");
        k[8] = law("$ENGINE_PAR_RDL_DEPTH").eval(0.5);
        assert_eq!(build(&k).depth_ms, MODERN_DEPTH_MS * 0.5);
        k[1] = law("$ENGINE_PAR_RDL_TIME").eval(0.125);
        assert_eq!(build(&k).time, ir::DelayTime::Ms(250.0));
        // LOWCUT at 0 via extra_knobs is off (0 Hz) exactly; 0.5 is 100 Hz.
        let lc_law = extra_knobs(INIT)
            .into_iter()
            .find(|(knob, field, _)| {
                *knob == ir::SlotKnob::Knob(27) && *field == ir::EffectParam::DelayLowCutHz
            })
            .expect("LOWCUT extra knob")
            .2;
        assert_eq!(lc_law.eval(0.0), 0.0);
        assert!((lc_law.eval(0.5) - 100.0).abs() < 1.0);
        // Saturation drives a non-empty waveshaper curve.
        k[18] = law("$ENGINE_PAR_RDL_SATURATION").eval(0.8);
        assert!(!build(&k).shape.is_empty());
        // TIME_UNIT switches between ms and synced beats live.
        k[17] = law("$ENGINE_PAR_RDL_TIME_UNIT").eval(12e-6); // 1/8 = 0.5 beat
        assert!(matches!(build(&k).time, ir::DelayTime::Beats(b) if b > 0.0));
        k[17] = 0.0;
        // TYPE and the switches take raw values: Tape, ping-pong, ping-pong flip.
        k[0] = law("$ENGINE_PAR_RDL_TYPE").eval(2e-6);
        k[12] = law("$ENGINE_PAR_RDL_PINGPONG").eval(1e-6);
        let d = build(&k);
        assert_eq!(d.input, [[0.5, 0.5], [0.0, 0.0]]);
        assert_eq!(
            d.feedback[0][1], INIT.feedback,
            "Tape's loop gain is FEEDBACK"
        );
        k[13] = law("$ENGINE_PAR_RDL_PINGPONG_FLIP").eval(1e-6);
        let flipped = build(&k);
        assert_eq!(flipped.input, [[0.0, 0.0], [0.5, 0.5]]);
        // Diffuse mode (type 4) builds a valid dense modulated delay.
        k[0] = law("$ENGINE_PAR_RDL_TYPE").eval(4e-6);
        k[12] = 0.0;
        k[22] = law("$ENGINE_PAR_RDL_DENSE").eval(1e-6);
        let diffuse = build(&k);
        assert!(diffuse.second_head);
        assert!(!diffuse.body.is_empty());
    }

    /// The INIT record as Kontakt 8.13.1 saves it.
    const INIT_RECORD: &str = "000000000000fa431f85eb3e000000003333733fcdcccc3d0000803e9a99993e0000803f00000000cdcccc3d010000000080bf000000000000803f000000803f0000003f0000003f0000003f01000000000000003f0000003f0000003f0000003f9a99993e00000000000000000000000000";

    fn bytes(hex: &str) -> Vec<u8> {
        let hex: String = hex.split_whitespace().collect();
        (0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
            .collect()
    }

    #[test]
    fn the_init_record_reads_as_kontakts_defaults() {
        assert_eq!(saved(0x12, &bytes(INIT_RECORD)), Some(INIT));
    }

    #[test]
    fn modern_lands_on_the_measured_laws() {
        let d = delay(INIT).expect("modern");
        assert_eq!(d.time, ir::DelayTime::Ms(500.0));
        // 0.46 feedback: 1.2x a repeat; LR offset 0.3: 3.75 ms either side.
        assert!((d.feedback[0][0] - 0.552).abs() < 1e-6 && d.feedback[0][1] == 0.0);
        assert!((d.spread_ms - 3.75).abs() < 1e-6);
        assert_eq!(d.output, [[1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(d.low_cut_hz, 0.0);
        let shelf = d.high_shelf.expect("shelf");
        assert!((shelf.hz - 19424.5).abs() < 1.0 && shelf.gain_db == -10.0);
        let open = delay(Replika {
            high_cut: 1.0,
            low_cut: 0.5,
            ..INIT
        })
        .expect("modern");
        assert_eq!(open.high_shelf.map(|b| b.gain_db), Some(0.0));
        assert!((open.low_cut_hz - 100.0).abs() < 1e-3);
        let pp = delay(Replika {
            pingpong: true,
            ..INIT
        })
        .expect("modern");
        assert_eq!(
            (pp.input[0], pp.feedback[1], pp.spread_ms),
            ([0.5, 0.5], [1.0, 0.0], 0.0)
        );
        // Diffuse (mode 4) now builds a diffuse delay rather than returning None.
        let diff = delay(Replika { mode: 4, ..INIT }).expect("diffuse");
        assert!(diff.second_head && !diff.body.is_empty());
    }

    #[test]
    fn modern_modulation_is_a_sine_of_8_ms_at_a_log_rate() {
        let d = |depth, rate| {
            delay(Replika {
                depth,
                rate,
                ..INIT
            })
            .expect("modern")
        };
        assert_eq!(d(0.5, 0.5).depth_ms, 4.0);
        assert_eq!(d(1.0, 0.5).depth_ms, 8.0);
        assert!(matches!(d(1.0, 0.5).rate, ir::Rate::Hz(hz) if (hz - 1.0).abs() < 1e-5));
        assert!(matches!(d(1.0, 0.8).rate, ir::Rate::Hz(hz) if (hz - 3.981).abs() < 1e-3));
        assert_eq!(d(0.0, 0.5).depth_ms, 0.0);
    }

    #[test]
    fn the_other_modes_carry_their_measured_laws() {
        let d = |mode, r: Replika| {
            delay(Replika {
                mode,
                depth: 1.0,
                rate: 0.5,
                ..r
            })
            .expect("mode")
        };
        for mode in [1, 3] {
            let v = d(mode, INIT);
            assert_eq!((v.depth_ms, v.triangle_s), (19.7, Some(0.196)));
            assert!(matches!(v.rate, ir::Rate::Hz(hz) if (hz - 1.0).abs() < 1e-5));
            assert!((v.feedback[0][0] - 0.552).abs() < 1e-6);
        }
        let tape = d(
            2,
            Replika {
                flutter: 0.6,
                ..INIT
            },
        );
        assert!((tape.depth_ms - 4.68 * 0.216).abs() < 1e-5 && tape.rate == ir::Rate::Hz(0.875));
        assert!((tape.feedback[0][0] - 0.46).abs() < 1e-6 && tape.triangle_s.is_none());
        let old = d(
            2,
            Replika {
                tape_age: 1.0,
                ..INIT
            },
        );
        assert!(matches!(&*old.body, [_, s] if (s.hz - 3568.0).abs() < 0.5));
        // BBD type switches Analogue body voicing.
        let bbd0 = delay(Replika {
            mode: 1,
            bbdtype: 0.0,
            ..INIT
        })
        .expect("bbd0");
        let bbd3 = delay(Replika {
            mode: 1,
            bbdtype: 1.0,
            ..INIT
        })
        .expect("bbd3");
        assert_ne!(bbd0.body, bbd3.body);
    }

    #[test]
    fn analogue_and_tape_noise_obey_saved_noise_switch() {
        let off = delay(Replika {
            mode: 1,
            noise: false,
            ..INIT
        })
        .expect("analogue");
        assert_eq!(off.noise, [0.0; 2]);
        let on = delay(Replika {
            mode: 1,
            noise: true,
            ..INIT
        })
        .expect("analogue");
        assert_eq!(on.noise, ANALOGUE_NOISE);
        let tape_on = delay(Replika {
            mode: 2,
            noise: true,
            ..INIT
        })
        .expect("tape");
        assert_eq!(tape_on.noise, TAPE_NOISE);
    }

    #[test]
    fn synced_ducked_and_extra_knobs_all_act_with_zero_lost() {
        let synced = Replika {
            ms: 2.0,
            beats: Some(0.5),
            saturation: 0.5,
            ducking_sense: 0.8,
            ducking_release: 0.4,
            ducking_amount: 0.5,
            ..INIT
        };
        let d = delay(synced).unwrap();
        assert_eq!(d.time, ir::DelayTime::Beats(1.0));
        assert!(!d.shape.is_empty());
        assert!(d.output[0][0] < 1.0, "ducking attenuates wet output");
        assert!(lost(synced).next().is_none());

        let extra = extra_knobs(INIT);
        for k in 24..30u8 {
            assert!(
                extra
                    .iter()
                    .any(|(knob, _, _)| *knob == ir::SlotKnob::Knob(k)),
                "missing live SlotLaw for Replika Knob({k})"
            );
        }
    }

    #[test]
    fn saved_shuffle_and_live_shuffle_plus_feedback_preserve_54_108ms_echo_timing() {
        use kontra_effects::processors::{Dsp, Responses, processor};

        // 1. Saved shuffle = 0.8 on a 50 ms mono delay must import with 54 ms timing
        //    (echoes at 54 ms and 108 ms, not 50 ms and 100 ms).
        let mut raw = bytes(INIT_RECORD);
        // ms @ offset 4 = 50.0, feedback @ 8 = 0.5, stereo @ 44 = 0, shuffle @ 77 = 0.8, feel @ 81 = 0.5.
        raw[4..8].copy_from_slice(&50.0_f32.to_le_bytes());
        raw[8..12].copy_from_slice(&0.5_f32.to_le_bytes());
        raw[44] = 0; // mono so L/R have no spread_ms offset
        raw[77..81].copy_from_slice(&0.8_f32.to_le_bytes());
        raw[81..85].copy_from_slice(&0.5_f32.to_le_bytes());
        let parsed = saved(0x12, &raw).expect("parse saved replika");
        assert!((parsed.shuffle - 0.8).abs() < 1e-6);

        let m = model(parsed);
        let built = m.build(&m.saved).expect("model build preserves shuffle");
        let ir::EffectKind::Delay(d) = &built else {
            panic!("expected Delay");
        };
        assert!(
            matches!(d.time, ir::DelayTime::Ms(ms) if (ms - 54.0).abs() < 1e-4),
            "expected Ms(54.0), got {:?}",
            d.time
        );

        // Render an impulse at 48 kHz and confirm echoes land at 54 ms (~2592) and 108 ms (~5184).
        let render_peaks = |kind: ir::EffectKind| -> (usize, usize) {
            let slot = ir::Effect {
                kind,
                bypass: false,
                output: 1.0,
                dry: 0.0,
            };
            let Dsp::Wet(mut unit, _) = processor(Some(&slot), 48_000, &Responses(&[], &[])) else {
                panic!("replika processor");
            };
            let mut io = vec![[0.0_f32; 2]; 8000];
            io[0] = [1.0, 1.0];
            for chunk in io.chunks_mut(256) {
                unit.process(chunk);
            }
            let peak_in = |lo: usize, hi: usize| -> usize {
                (lo..hi)
                    .max_by(|&a, &b| io[a][0].abs().total_cmp(&io[b][0].abs()))
                    .unwrap()
            };
            (peak_in(2000, 3200), peak_in(4200, 6200))
        };
        let (e1, e2) = render_peaks(built);
        assert!(
            (e1 as isize - 2592).abs() <= 4 && (e2 as isize - 5184).abs() <= 8,
            "saved shuffle=0.8 echoes expected near 2592/5184 (54/108 ms), got {e1}/{e2}"
        );

        // 2. Live KSP sequence: start unshuffled (ms=50, shuffle=0), write shuffle=0.8
        //    (Knob(3)), write LOWCUT=0.5 (extra Knob(27)), then write feedback=0.6 (Knob(2)).
        //    Shuffle timing (54/108 ms) AND lowcut (100 Hz) must both survive the feedback write!
        let base = Replika {
            ms: 50.0,
            feedback: 0.5,
            stereo: false,
            shuffle: 0.0,
            feel: 0.5,
            ..INIT
        };
        let km = model(base);
        let mut live: Vec<f32> = km.saved.to_vec();
        let mut eff = ir::Effect {
            kind: ir::EffectKind::Delay(delay(base).unwrap()),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        // Apply shuffle = 0.8 via Knob(3).
        live[3] = 0.8;
        eff = km.build_effect(&live, &eff).expect("shuffle update");
        // Apply live LOWCUT = 0.5 via extra_knobs.
        let lc = extra_knobs(base)
            .into_iter()
            .find(|(k, f, _)| *k == ir::SlotKnob::Knob(27) && *f == ir::EffectParam::DelayLowCutHz)
            .unwrap()
            .2
            .eval(0.5);
        ir::EffectParam::DelayLowCutHz.set(&mut eff, lc);
        // Now write feedback = 0.6 via Knob(2) — must NOT reset shuffle or lowcut!
        live[2] = 0.6;
        eff = km.build_effect(&live, &eff).expect("feedback update");
        let ir::EffectKind::Delay(after) = &eff.kind else {
            panic!("expected Delay");
        };
        assert!(
            matches!(after.time, ir::DelayTime::Ms(ms) if (ms - 54.0).abs() < 1e-4),
            "expected Ms(54.0), got {:?}",
            after.time
        );
        assert!((after.low_cut_hz - 100.0).abs() < 1.0);
        assert!((after.feedback[0][0] - 0.72).abs() < 1e-5);
        let (l1, l2) = render_peaks(eff.kind);
        assert!(
            (l1 as isize - 2592).abs() <= 4 && (l2 as isize - 5184).abs() <= 8,
            "live shuffle+feedback echoes expected near 2592/5184, got {l1}/{l2}"
        );
    }
}
