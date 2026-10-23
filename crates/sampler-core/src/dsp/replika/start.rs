//! Start state of the mode-1 chain (path 1, index 28 = 0.3) from the rate and the reset controls (piece 3c part 5a).
//!
//! The native object after ctor, init, the reset setters, prepare (`141aa1580`, and its per-head setup `141a9ac90` for
//! each of the three heads) and the index-28 setter. Every field is computed here, from the native formula, the design
//! functions of `design.rs` (checked against native vectors) or the selector [`Heads::select`] (checked against the
//! native selector vectors). The constants are the ctor values (`141a7f4d0`, init `141a9e300`), read from the exe
//! under kemu and checked against the start dump; the dump itself is only the oracle in `chain::tests`.
//!
//! Formulas (rate `fs`, all frames at the rate; the native constants are in `k-replika-gaps.md`, Piece 3c part 5a):
//! - delays: `lo` 0, `hi` = `fs x 5500 x 0.001` (5.5 s), `mode` = `fs x 50 x 0.001` (50 ms), base 1 (`141a9ac90`).
//! - ring: `next_pow2(max(hi + mode/2 + 4, next_pow2(fs)))` frames per tap, mask = ring - 1, ring positions 0 and ring.
//! - glide: `div` = `fs x 0.08` (f32 constant `0x3da3d70b`), `count` = `count_init` = round(div). Its `step` is `1 / (44100 x 0.08)`: the
//!   defaults setter ran at the ctor's 44.1 kHz, before prepare changed the divisor, so the step keeps that value.
//! - fade: coef = `0.05^(1 / (5 ms x fs))`, threshold 0.001, pending (settled by the selector).
//! - wrapper glides (`141ab7a90`): settled at 1 (target 1, step 0, count 0), threshold 0.001.
//! - sections `+0x350` and `+0x384`: `Section::new(tan_series(20000, fs), 1.4)` (the 20 kHz high cut default).
//! - taps: the ctor section constants, rate 0.5, rates 1, mask = ring - 1.
//! - post filters: `design::post(fs, 3)`, the envelope coefficient and the damping (the gain at `+0x54`).
//! - write lanes: `design::lane(fs, 3)`, the ctor constants and the dither seed (see [`NOISE_SEED`]).
//! - spread table: coefficient `exp(-35.322115776986905 / fs)` (`1479ab708`), thresholds 1e-4, the start values of
//!   width, skew, ratio and depth; the bounds `[0, 5500]` ms (`C+0xad8`, `C+0xad0`); flag 0; smoothing on.
//! - the selector (`Heads::select`) sets the head mode 1, the kernel flags, the rebase (`base_src` = fs x 0.25 x 0.5),
//!   the taps' rates and the fade settle.
//!
//! Not from the formulas: the spread targets (the P0 block at `C+0x80`, words `+0x2c`, `+0x30`, `+0x34`, `+0x38`) and
//! the start values of the table, which are the values the reset setters leave (their setter mapping is open, see the
//! gaps doc). The noise seed is the emulator's `std::random_device` value (`0x2545F491`, kemu stub `?_Random_device`).
//! The start is valid for the reset controls and rates up to 48 kHz (44.1 and 48 kHz are checked against the native
//! dumps). Above 48 kHz the native chain runs at an internal rate with an oversampler (not ported, see the gaps doc).

use super::chain::{Entry, Mode1, Wrap, WrapGlide};
use super::delay::DelayPos;
use super::design;
use super::head::{Fade, Glide, Head, Heads, path_for_index};
use super::mode1::{PostFilter, WriteFilter};
use super::read::Tap;
use super::{ReplikaCore, ReplikaState, index};
use crate::dsp::tpt_svf::{Section, State};

/// The lane mode the reset controls leave (`H+0x8c` = 3), and the post filter mode of the same design.
const LANE_MODE: u32 = 3;
/// Index-28 value of the mode-1 start (`floor(5 x 0.3)` = 1).
const PATH1_INDEX: f32 = 0.3;
/// Seed of the dither generator. The native ctor draws its seed from `std::random_device` (`141a7d920`); the emulator
/// returns this value, so the start is deterministic.
pub(super) const NOISE_SEED: u32 = 0x2545_F491;
/// `1479ab708`: the spread table coefficient exponent (coefficient = `exp(TABLE_EXP / fs)`).
const TABLE_EXP: f64 = -35.322_115_776_986_905;
/// Threshold of the spread table entries (1e-4, native `C+0x14220` and the entries after it).
const TABLE_THR: f64 = 1e-4;
/// Threshold of the head glides and the wrapper glides (0.001).
const GLIDE_THR: f32 = 0.001;
/// The delay range in ms (`C+0xad0` = 5500; `C+0xad8` = 0): the head positions run over `[0, 5500]` ms.
const BOUND_HI_MS: f64 = 5500.;
/// The ctor's rate: the defaults setter ran at 44.1 kHz, so the glide step is `1 / (44100 x 0.08)`.
const CTOR_RATE: f32 = 44_100.;
/// The glide divisor's factor (`DAT_147698800`, f32 0.08000000566, as the native multiplies by it).
const GLIDE_PER_FRAME: f32 = f32::from_bits(0x3da3_d70b);
/// Wrapper level (`H+0x5828` at rest) per head: head 1 is at 1, heads 2 and 3 at 0 (the level writer is not traced).
const WRAP_LEVEL: [f32; 3] = [1., 0., 0.];

/// The ring length of each mode-1 tap at rate `fs`: `next_pow2(max(hi + mode/2 + 4, next_pow2(fs)))`, with the delay
/// range `hi` = 5500 ms and `mode` = 50 ms in frames (`141a9ac90`, the head's ring size from the delay range).
pub(super) fn ring_len(fs: f32) -> usize {
    let hi = (f64::from(fs) * BOUND_HI_MS * 0.001) as i64;
    let mode = (f64::from(fs) * 50. * 0.001) as i64;
    // The native constant is -0.5 (f32), so the sum adds mode / 2.
    let half = (mode as f32 * -0.5) as i64;
    let base = (fs as u64).next_power_of_two() as i64;
    ((hi - half + 4).max(base) as u64).next_power_of_two() as usize
}

/// One head at the start, before the selector (its mode and taps are set by [`Heads::select`]).
fn head(fs: f32, ring: usize) -> Head {
    let f = f64::from(fs);
    let hi = (f * BOUND_HI_MS * 0.001) as i32;
    let mode = (f * 50. * 0.001) as i32;
    let ring_i = ring as i32;
    let tap = |mask: i32| Tap {
        // The ctor's kernel section (`+0x58..+0x7c`) and rate; the section is retuned by the read.
        sec: Section::from_native(
            f32::from_bits(0x3d2b_471a),
            f32::from_bits(0x3e31_2db6),
            f32::from_bits(0x3f37_485d),
            f32::from_bits(0x3fb3_3333),
        ),
        rate: 0.5,
        ..Tap::new(0., mask, 0., 0, 0., 0., 1.4, 0.5)
    };
    let lane = design::lane(fs, LANE_MODE).expect("lane mode 3 is designed");
    let post = design::post(fs, LANE_MODE).expect("post mode 3 is designed");
    let post_filter = |p: design::PostDesign| {
        let (c2, c3, c4, gain) = p.sec.parts();
        PostFilter {
            env: 0.,
            coef: p.env_coef,
            s0: 0.,
            s1: 0.,
            c2,
            c3,
            c4,
            gain,
        }
    };
    let sec = Section::new(design::tan_series(20_000., fs), 1.4);
    let div = fs * GLIDE_PER_FRAME;
    let n = div.round() as i32;
    Head {
        mode: 0,
        factor: 0.,
        s: 0.,
        base_src: 1.,
        delay: DelayPos {
            lo: 0,
            hi,
            mode,
            base: 1.,
            // The head positions after the selector's tail: `a` and `b` are zero, so `base / a` is infinite.
            r58: f64::INFINITY,
            r60: f64::INFINITY,
            a: 0.,
            b: 0.,
            flag: true,
        },
        taps: [tap(ring_i - 1), tap(ring_i - 1)],
        fade: Fade {
            coef: 0.05_f64.powf(1. / (0.005 * f)),
            level: 1.,
            target: 0.,
            thr: 0.001,
            pending: true,
            trigger: false,
        },
        glide: Glide {
            cur: 0.,
            target: 0.,
            step: 1. / (CTOR_RATE * GLIDE_PER_FRAME),
            thr: GLIDE_THR,
            count: n,
            pending: true,
            fresh: false,
            div,
            count_init: n,
        },
        post: [post_filter(post), post_filter(post)],
        sec: [sec, sec],
        rate: 0.5,
        time: [0.; 4],
        anchor: [f, 0.5, 0.125],
        lane: LANE_MODE as i32,
        mode_flags: [true, false, false],
        kernel: [0; 2],
        ring_pos: [0, ring_i],
        ring_wrap: [ring_i, 0],
        hold: false,
        fade_on: false,
        fade_amount: 0.,
        stored: [0.; 2],
        write_filter: [
            WriteFilter::start(&lane, NOISE_SEED),
            WriteFilter::start(&lane, NOISE_SEED),
        ],
        sec_state: [State::default(); 2],
        sec_out: [0.; 2],
    }
}

/// The wrapper glide of `141ab7a90` at its settled start (level and gain 1).
fn settled_glide() -> WrapGlide {
    WrapGlide {
        cur: 1.,
        target: 1.,
        step: 0.,
        thr: GLIDE_THR,
        count: 0,
        flag: false,
    }
}

impl Mode1 {
    /// The start state of path 1 at rate `fs` (see the module doc). Built from the formulas, then the selector runs for
    /// index 28 = 0.3, as the native setter does.
    pub(super) fn start(fs: f32) -> Self {
        let ring = ring_len(fs);
        let mut heads = Heads {
            path: i32::MAX,
            heads: std::array::from_fn(|_| head(fs, ring)),
        };
        let mut st = ReplikaState::new(fs);
        heads
            .select(&mut st, PATH1_INDEX)
            .expect("path 1 is not blocked");
        let thr = TABLE_THR;
        let coef = (TABLE_EXP / f64::from(fs)).exp();
        let entry = |value: f64| Entry {
            value,
            target: value,
            thr,
            flag: false,
            coef,
        };
        Self {
            heads,
            rings: std::array::from_fn(|_| [vec![0.; ring], vec![0.; ring]]),
            // Width, skew, ratio, depth (the table order); the start values of the table are 0, 0.5, 0, 0.5.
            table: [entry(0.), entry(0.5), entry(0.), entry(0.5)],
            // The spread targets at the reset controls, in the order `[+0x2c, +0x34, +0x30, +0x38]` (P0 words 40, 0.5, 0, 0.3).
            targets: [40., 0.5, 0., f64::from(f32::from_bits(0x3e99_999a))],
            bounds: [0., BOUND_HI_MS],
            flag: 0,
            smooth: true,
            seen: 0,
            wrap: std::array::from_fn(|h| Wrap {
                level: WrapGlide {
                    cur: WRAP_LEVEL[h],
                    target: WRAP_LEVEL[h],
                    ..settled_glide()
                },
                gain: settled_glide(),
                own: 1.,
                cross: 0.,
            }),
        }
    }
}

impl ReplikaCore {
    /// The mode-1 start for the core's index-28 selection. `None` when index 28 selects another path, and above 48 kHz:
    /// the native prepare runs the chain at an internal 48 kHz there (oversampled, flag `0x608`, not ported), so the
    /// 1x start does not apply. The controls are the reset values the start is derived for (see the module doc).
    pub(super) fn mode1_start(&self) -> Option<Mode1> {
        let on_path = path_for_index(self.raw[index::SELECT]) == 1;
        (on_path && self.rate <= 48_000.).then(|| Mode1::start(self.rate))
    }
}
