//! DStortion (module 0x56, `BEffectDStortion`) onto the engine's waveshaper.
//! Its saved record (reader [ref]) holds volume, tone, drive, bass, mid
//! and treble as floats at 0..20 and mono as a byte at 24. The structure is
//! the native kernel's ([ref], its SR setup [ref] and apply
//! [ref], read through v2's kemu-checked port), every filter a
//! one-pole `y += c·(x − y)` with `c = w·hz/rate` capped, `w` =
//! `f32::from_bits(0x40c90e56)` and `hz` an index into
//! [`PITCH_INDEX`] (`2^(i/60 − 20)`):
//!
//! 1. Twice the input through a low shelf: `(1 + k)·x − k·lp(x)` read as
//!    the high-pass `x − lp(x)`, `k = 2^(1170.1/60 − 20) − 1` (−3 dB above
//!    the corner); MID moves the corner, `0.854·2^((1581.88 + 400·mid)/60 − 20)`
//!    Hz (82 Hz to 8.4 kHz).
//! 2. TREBLE's low-pass (`2^((1881.88 + 200·treble)/60 − 20)` Hz, 2.6 kHz;
//!    open from 0.46), a 26 Hz high-pass, the treble low-pass again, then
//!    BASS's high-pass (`2^((1951.88 − 400·bass)/60 − 20)` Hz: 5.9 kHz to 58 Hz).
//! 3. DRIVE's gain `4·2^((1760.08 + 262.1·drive²)/60 − 20)` (2580 to 53000)
//!    into a 256-point table over −127..=128 (built by [ref]), run at
//!    four times the rate.
//! 4. Tone: `0.398·volume·(0.881·tone·(h − 0.859·lp₁₂₂₄(h)) + (1 − tone)·lp₃₁₁(h))`.
//!
//! Each one-pole is a first-order digital section; [`section`] carries it
//! to the engine as the analog section the engine's bilinear transform turns
//! back into it at 48 kHz (exactly; at other rates within the transform's
//! warp). Not carried: MONO (both channels summed first; reported), the
//! table's 0.002 even part, the knobs' ramps, and the native 4x upsampler
//! (the engine runs the curve at 2x; the resampler's roll-off is a fixed low-pass).

use ir::Stage::{In, Out, Pre};
use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

/// The native coefficient constant (it multiplies where the engine has 2π).
const W: f32 = f32::from_bits(0x40c9_0e56);
/// The rate the native coefficients are worked out for here.
const RATE: f32 = 48_000.0;
/// The corner every section is prewarped at: any works, the mapping is exact.
const AT_HZ: f32 = 1_000.0;

fn index_hz(i: f32) -> f32 {
    PITCH_INDEX.eval_unclamped(i)
}

/// A one-pole's coefficient for `hz`, capped at `cap`.
fn coefficient(hz: f32, cap: f32) -> f32 {
    (W * hz / RATE).min(cap)
}

/// `(b0 + b1·z⁻¹ + b2·z⁻²) / (a0 + a1·z⁻¹ + a2·z⁻²)` as the analog section
/// whose bilinear transform at 48 kHz, prewarped at [`AT_HZ`], it is:
/// `z⁻¹ = (1 − t·s')/(1 + t·s')`, `t = tan(π·AT_HZ/48000)`.
fn section(b: [f32; 3], a: [f32; 3]) -> ir::Section {
    let t = (std::f32::consts::PI * AT_HZ / RATE).tan();
    let analog =
        |[c0, c1, c2]: [f32; 3]| [c0 + c1 + c2, 2.0 * t * (c0 - c2), t * t * (c0 - c1 + c2)];
    ir::Section {
        hz: AT_HZ,
        num: analog(b),
        den: analog(a),
    }
}

/// Products of first-order `(b0 + b1·z⁻¹)` polynomials.
fn mul(p: [f32; 2], q: [f32; 2]) -> [f32; 3] {
    [p[0] * q[0], p[0] * q[1] + p[1] * q[0], p[1] * q[1]]
}

/// `y += c·(x − y)`: `c / (1 − (1 − c)·z⁻¹)`.
fn low(c: f32) -> ([f32; 2], [f32; 2]) {
    ([c, 0.0], [1.0, c - 1.0])
}

/// `x − y` before `y += c·(x − y)`: `(1 − z⁻¹) / (1 − (1 − c)·z⁻¹)`.
fn high(c: f32) -> ([f32; 2], [f32; 2]) {
    ([1.0, -1.0], [1.0, c - 1.0])
}

/// The 26 Hz high-pass between the treble low-passes (index 1481.88), and
/// the native 4x resampler's roll-off: a 22 kHz, Q 0.28 low-pass, fitted to
/// Kontakt 8.13.1 sweeps (the same at every knob setting; within 0.5 dB to
/// 12.8 kHz, 1.1 dB at 16 and 20 kHz; fx-measured/distortion-family.md).
static FIXED: [(ir::Stage, ir::Section); 2] = [
    (
        Pre,
        ir::Section {
            hz: 25.98,
            num: [0.0, 1.0, 0.0],
            den: [1.0, 1.0, 0.0],
        },
    ),
    (
        Out,
        ir::Section {
            hz: 22_000.0,
            num: [1.0, 0.0, 0.0],
            den: [1.0, 1.0 / 0.28, 1.0],
        },
    ),
];

/// The record's knobs, each 0..=1, as the engine's waveshaper.
pub fn dstortion(
    volume: f32,
    tone: f32,
    drive: f32,
    bass: f32,
    mid: f32,
    treble: f32,
) -> ir::EffectKind {
    let [volume, tone, drive, bass, mid, treble] =
        [volume, tone, drive, bass, mid, treble].map(|x| x.clamp(0.0, 1.0));
    // 1. The shelf: 2·((1 + k)·x − k·(x − hp(x))) = 2·(x + k·hp(x)).
    let k = index_hz(1_170.102_7) - 1.0;
    let c = (index_hz(400.0 * mid + 1_581.881_6) * (k + 2.0) * 0.5 * W / RATE)
        .min((2.0 * (k + 1.0)).min(1.5));
    let (hb, ha) = high(c);
    let shelf = [2.0 * (ha[0] + k * hb[0]), 2.0 * (ha[1] + k * hb[1])];
    let (bb, ba) = high(coefficient(index_hz(1_951.881_6 - 400.0 * bass), 1.0));
    let (tb, ta) = low(coefficient(index_hz(200.0 * treble + 1_881.881_6), 1.0));
    // 3. The curve's input in table steps, and its slope at 0 (made up after).
    let gain = 4.0 * index_hz(262.100_13 * drive * drive + 1_760.077);
    let slope = POINTS[1] - POINTS[0];
    // 4. The tone network over (1 − q₁·z⁻¹)(1 − q₂·z⁻¹).
    let level = 0.398 * volume;
    let (bright, dark) = (0.881 * tone * level, (1.0 - tone) * level);
    let shelf_low = index_hz(1_030.581_7);
    let c1 = coefficient(index_hz(1_766.881_6) / ((shelf_low + 1.0) * 0.5), 1.0);
    let c2 = coefficient(index_hz(1_696.881_6), 1.0);
    let (d1, d2) = ([1.0, c1 - 1.0], [1.0, c2 - 1.0]);
    // bright·h + bright·(shelf_low − 1)·lp₁(h) + dark·lp₂(h), over d1·d2.
    let both = mul(d1, d2);
    let lp1 = bright * (shelf_low - 1.0) * c1;
    let lp2 = dark * c2;
    let num = [
        bright * both[0] + lp1 * d2[0] + lp2 * d1[0],
        bright * both[1] + lp1 * d2[1] + lp2 * d1[1],
        bright * both[2],
    ]
    .map(|v| v * gain * slope);
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Table {
            drive: 2.0 * gain / 127.0,
            points: std::borrow::Cow::Borrowed(&POINTS),
        },
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::new(
            &FIXED,
            [
                Some((In, section(mul(tb, tb), mul(ta, ta)))),
                Some((In, section(mul(shelf, bb), mul(ha, ba)))),
                Some((Out, section(num, mul(d1, d2)))),
            ],
        ),
    })
}

/// The saved record as a script moves it: every knob move rebuilds the
/// shaper through [`dstortion`].
/// Single-encoding module descriptor for Distortion (`0x56`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::DSTORTION,
    module_id: 0x56,
    versions: (0, u32::MAX),
    name: "Distortion",
    script_type: 36,
    mix: crate::module::MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(6).0,
    build: |p| {
        let [volume, tone, drive, bass, mid, treble] = *p else {
            return None;
        };
        Some(dstortion(volume, tone, drive, bass, mid, treble))
    },
};

pub fn model(public: &[u8]) -> Option<ir::KnobModel> {
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    Some(MODULE.knob_model(&[f(0)?, f(4)?, f(8)?, f(12)?, f(16)?, f(20)?]))
}

/// Whether the record's mono switch is on (not carried).
#[must_use]
pub fn mono(public: &[u8]) -> bool {
    public.get(24).is_some_and(|&b| b != 0)
}

/// The KSP names of [`model`]'s knobs; each is the record's field as set
/// (bindings table `fx_other.tsv`).
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        ("$ENGINE_PAR_DS_VOLUME", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_DS_TONE", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_DS_DRIVE", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_DS_BASS", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_DS_MID", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_DS_TREBLE", Slot(Knob(5)), unit()),
    ]
}

/// The odd part of the native table's 256 points over −127..=128, from 0 to
/// 127 (its even part is at most 0.002; table built by [ref], dumped
/// under kemu into v2's `amp/dstortion.f32le`).
#[rustfmt::skip]
static POINTS: [f32; 128] = [
    0.000000, 0.018738, 0.037460, 0.056213, 0.074997, 0.093796, 0.112610, 0.131470,
    0.150360, 0.169250, 0.188171, 0.207123, 0.226044, 0.244949, 0.263794, 0.282578,
    0.301270, 0.319809, 0.338181, 0.356339, 0.374237, 0.391846, 0.409119, 0.425980,
    0.442429, 0.458389, 0.473846, 0.488754, 0.503082, 0.516785, 0.529861, 0.542282,
    0.554047, 0.565140, 0.575577, 0.585342, 0.594482, 0.602997, 0.610916, 0.618256,
    0.625061, 0.631363, 0.637207, 0.642624, 0.647644, 0.652328, 0.656677, 0.660767,
    0.664627, 0.668259, 0.671722, 0.675049, 0.678238, 0.681305, 0.684296, 0.687195,
    0.690033, 0.692810, 0.695511, 0.698151, 0.700760, 0.703278, 0.705719, 0.708115,
    0.710419, 0.712646, 0.714798, 0.716843, 0.718811, 0.720673, 0.722473, 0.724197,
    0.725815, 0.727402, 0.728912, 0.730377, 0.731796, 0.733200, 0.734604, 0.735977,
    0.737335, 0.738724, 0.740097, 0.741486, 0.742874, 0.744247, 0.745621, 0.746979,
    0.748306, 0.749603, 0.750885, 0.752106, 0.753311, 0.754425, 0.755524, 0.756577,
    0.757584, 0.758575, 0.759537, 0.760483, 0.761414, 0.762344, 0.763306, 0.764267,
    0.765228, 0.766205, 0.767197, 0.768173, 0.769165, 0.770126, 0.771057, 0.771942,
    0.772812, 0.773651, 0.774429, 0.775192, 0.775940, 0.776703, 0.777481, 0.778259,
    0.779053, 0.779877, 0.780685, 0.781479, 0.782242, 0.782959, 0.783661, 0.783981,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The digital response of `s` at 48 kHz, `hz`, by the bilinear map back.
    fn gain(s: ir::Section, hz: f32) -> f32 {
        let t = (std::f32::consts::PI * s.hz / RATE).tan();
        let w = 2.0 * std::f32::consts::PI * hz / RATE;
        // s' at z = e^{jw}: (1/t)·j·tan(w/2).
        let x = (w / 2.0).tan() / t;
        let eval = |c: [f32; 3]| {
            let (re, im) = (c[0] - c[2] * x * x, c[1] * x);
            (re * re + im * im).sqrt()
        };
        eval(s.num) / eval(s.den)
    }

    #[test]
    fn the_sections_are_the_native_one_poles() {
        // y += c·(x − y) at c = 0.3: |c / (1 − 0.7·e^{−jw})| at 4 kHz.
        let (b, a) = low(0.3);
        let s = section(mul(b, [1.0, 0.0]), mul(a, [1.0, 0.0]));
        let w = 2.0 * std::f32::consts::PI * 4_000.0 / RATE;
        let native = 0.3 / ((1.0 - 0.7 * w.cos()).powi(2) + (0.7 * w.sin()).powi(2)).sqrt();
        assert!((gain(s, 4_000.0) / native - 1.0).abs() < 1e-4);
    }
}
