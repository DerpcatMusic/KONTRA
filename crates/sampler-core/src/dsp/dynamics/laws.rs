//! Laws as configurations of the one engine: each law's knob laws (the
//! native setters, recovered from the binaries) and its stage chain.
//!
//! | law | input | detector | computer | ballistics / smoothing | gain map | delay | output |
//! |---|---|---|---|---|---|---|---|
//! | Generic | - | `Sidechain` (peak / RMS / true peak, HP, link, feedback tap) | `Ratio` | `SmoothBranch` (+ dB table) | - | look-ahead | `Mix` |
//! | Kontakt Classic | `Quantize` | `SignedMean` | `Trigger` | `HoldFollower` | `Reach` | - | `F32Apply` |
//! | Kontakt Enhanced | `Quantize` | `SignedMean` | `Segments` | `Velocity` | `Reciprocal` | - | `F32Apply` |
//! | Kontakt Pro | `Quantize` | `ChannelMax` + 33 `Windowed` | `ProKnee` | `ReleaseLowPass` (RBJ) | `Exp2Quadratic` | 32 | `ProMix` |
//! | Kontakt Limiter | `Scale` | `ChannelMax` | `SoftClip` | `PeakRelease`, 33 `Windowed`, `LowPass2` (RBJ) | - | 32 | `LimiterOut` |
//! | Kontakt Transparent Limiter | `Drive` | `Hilbert` (+ `L` window) | `Decibel`, `Ceiling` | `HilbertRelease`, `Hann` | - | `L - 1` | `CeilingOut` |
//! | Kontakt Feedback Comp | `Drive` | `HighPassed` | `FeedbackCurve` (in the loop) | `FeedbackLoop` (feedback tap) | - | - | `FeedbackOut` |
//! | Kontakt Transient Master | `Quantize` | `TenthRoot` | `TransientMix` | `SixEnvelopes` | - | - | `DcBlock` |
//! | Solid Bus Comp | `Quantize` | `BusTap` (feedback tap) | `BusRatio` | `BusEnvelope` (+ `2^-32e`) | - | - | `BusOut` |
//! | UVI Maximizer | `Quantize` | `MaxPeak` (peak / true peak) | `MaxKnee` | `SlewEnvelope` (+ `W` window), `MovingAverage2` | `NeperGain` | `W` (+6) | `ClipOut` |

use super::ballistics::*;
use super::computer::*;
use super::detector::*;
use super::output::*;
use super::pipeline::{Delay, Drive, Quantize, Scale, Shared, Window, high_pass_1, rbj_low_pass, run};
use super::{Planar, ProcessorState};
use crate::dsp::eq_family::solid::Spline;
use sampler_ir::fx::dynamics::{Detector, Direction, Generic, Law, Maximizer as MaxSaved, Topology};
use std::sync::LazyLock;

/// Process `len` frames of `block` with `law`: the chain is built once per
/// block and runs monomorphised.
#[allow(clippy::too_many_arguments)]
pub(super) fn process(
    law: Law,
    shared: &Shared,
    knobs: [f64; 4],
    rate: u32,
    state: &mut ProcessorState,
    cells: &mut [[f64; 2]],
    block: &mut Planar,
    len: usize,
) {
    let linked = shared.linked();
    match law {
        Law::Generic(g) => {
            let extra = generic_extra_cells(&g);
            let (ring, history) = cells.split_at_mut(cells.len() - extra);
            let feedback = g.topology == Topology::Feedback;
            let ring = if feedback { &mut ring[..0] } else { ring };
            let true_peak = (g.detector == Detector::TruePeak).then(|| TruePeak::new(history, state, 6));
            let window = match g.detector {
                Detector::Rms { window } => window.seconds(),
                Detector::Peak | Detector::TruePeak => 0.,
            };
            let mut sc = Sidechain::new(state, true_peak);
            sc.feedback = feedback;
            sc.link = shared.link;
            sc.detector = g.detector;
            sc.rms = pole(window, rate);
            sc.highpass = if g.sidechain_highpass_hz > 0. {
                let fs = f64::from(rate);
                (-std::f64::consts::TAU * g.sidechain_highpass_hz.min(fs / 2.) / fs).exp()
            } else {
                0.
            };
            let ratio = generic_ratio(&g, knobs, if linked { 1 } else { 2 });
            let [attack, release] = [knobs[2], knobs[3]].map(|s| pole(s, rate));
            let ballistics = SmoothBranch::new(attack, release, linked, g.direction == Direction::Upward, state);
            let delay = Delay::new(ring, state);
            run((sc, ratio, ballistics, delay, Mix(*shared)), state, block, len);
        }
        Law::KontaktClassic(_) => {
            let c = Classic::new(knobs, rate);
            let follower = HoldFollower::new(c.attack, c.release, state);
            let (trigger, reach) = (Trigger(c.threshold), Reach(f64::from(1. - c.gain)));
            run((Quantize, SignedMean(linked), trigger, follower, reach, F32Apply(*shared)), state, block, len);
        }
        Law::KontaktEnhanced(_) => {
            let c = Enhanced::new(knobs, rate);
            let curve = Segments { threshold: c.threshold, excess_scale: c.excess_scale };
            let ballistics = Velocity::new(c.attack, c.release, state);
            run((Quantize, SignedMean(linked), curve, ballistics, Reciprocal, F32Apply(*shared)), state, block, len);
        }
        Law::KontaktPro(_) => {
            let c = Pro::new(knobs, rate);
            let (ring, window) = cells.split_at_mut(KONTAKT_LATENCY);
            let window = Windowed { window: Window::new(window, state, 8), floor: PRO_FLOOR };
            let delay = Delay::new(ring, state);
            let ballistics = ReleaseLowPass::new(c.release, (c.b0, c.b1, c.a1, c.a2), state);
            let gain = Exp2Quadratic { reduction: c.reduction, offset: c.offset };
            run(
                (Quantize, delay, ChannelMax, window, ProKnee(c.threshold_db), ballistics, gain, ProMix(*shared)),
                state,
                block,
                len,
            );
        }
        Law::KontaktLimiter(_) => {
            let c = Limiter::new(knobs, rate);
            let (ring, window) = cells.split_at_mut(KONTAKT_LATENCY);
            let window = Windowed { window: Window::new(window, state, 8), floor: 0. };
            let delay = Delay::new(ring, state);
            let follower = PeakRelease::new(c.release, state);
            let filter = LowPass2::new((c.b0, c.b1, c.a1, c.a2), state);
            run(
                (Scale(c.gain), delay, ChannelMax, follower, window, filter, SoftClip, LimiterOut(*shared)),
                state,
                block,
                len,
            );
        }
        Law::KontaktHilbert(_) => {
            let c = Hilbert::new(knobs, rate);
            let taps = hilbert_window(rate);
            let (ring, rest) = cells.split_at_mut(taps - 1);
            let (window, rest) = rest.split_at_mut(taps);
            let (allpass, hann) = rest.split_at_mut(HILBERT_CELLS);
            let delay = Delay::new(ring, state);
            let window = Window::new(window, state, 5);
            let detector = super::detector::Hilbert::new((c.allpass, c.delay, c.rise, c.fall), window, allpass, state);
            let ballistics = HilbertRelease::new(c.release, c.smooth, state);
            let hann = Hann::new(hann, c.cos, c.period, state);
            let out = CeilingOut { ceiling: c.ceiling, shared: *shared };
            let input = Drive { gain: c.drive, offset: HILBERT_EPS };
            run((input, delay, detector, Decibel, ballistics, Ceiling, hann, out), state, block, len);
        }
        Law::KontaktFeedback { high_quality, .. } => {
            let c = Feedback::new(knobs, high_quality, rate);
            let detector = HighPassed::new(c.hp, linked, state);
            let k = [c.attack, c.release, c.couple, c.floor];
            let ballistics = FeedbackLoop::new(c.curve, k, c.steps, linked, c.bypass, cells, state);
            let out = FeedbackOut::new(c.lp, c.hp, shared, state);
            run((Drive { gain: c.drive, offset: EPS_18 }, detector, ballistics, out), state, block, len);
        }
        Law::KontaktTransient { attack, sustain, gain, clip } => {
            let c = Transient::new([attack, sustain, gain].map(|v| v as f32), rate);
            let detector = TenthRoot::new(c.hp, state);
            let ballistics = SixEnvelopes::new(c.k, c.sub, state);
            let curve = TransientMix { attack: c.attack, sustain: c.sustain, gain: c.gain };
            run((Quantize, detector, ballistics, curve, DcBlock::new(c.hp, clip, state)), state, block, len);
        }
        Law::KontaktBusComp { threshold, makeup, attack, release, ratio, link } => {
            let c = Bus::new([threshold, makeup, attack, release, ratio], rate);
            let curve = BusRatio { threshold: c.threshold, slope: c.slope };
            let ballistics = BusEnvelope::new(c.attack, c.release, c.auto, state);
            let out = BusOut { link, makeup: c.makeup, wet: shared.mix.clamp(0., 1.) };
            run((Quantize, BusTap::new(state), curve, ballistics, out), state, block, len);
        }
        Law::UviMaximizer(m) => {
            let m = Maximizer::from(&m);
            let k = MaxCoefficients::new(&m, rate);
            let w = maximizer_window(m.lookahead, rate);
            let d = maximizer_latency(&m, rate);
            let (ring, rest) = cells.split_at_mut(d);
            let (window, rest) = rest.split_at_mut(w);
            let (average, history) = rest.split_at_mut(w / 2);
            let detector = MaxPeak(m.true_peak.then(|| TruePeak::new(history, state, 11)));
            let curve = MaxKnee { threshold: m.threshold, knee: m.knee };
            let offset = m.ceiling + m.threshold;
            let envelope = SlewEnvelope::new(
                [offset, k.slew, k.attack, k.fast, k.slow, m.blend],
                Window::new(window, state, 8),
                state,
            );
            let gain = NeperGain { offset, ceiling: m.ceiling };
            let out = ClipOut { ceiling: 10f32.powf(m.ceiling * 0.05), hard: m.mode == 2 };
            let average = MovingAverage2::new(average, state);
            run(
                (Quantize, detector, curve, envelope, gain, average, Delay::new(ring, state), out),
                state,
                block,
                len,
            );
        }
    }
}

/// Frames a law delays the audio by.
pub(super) fn latency(law: &Law, rate: u32) -> usize {
    match law {
        // ponytail: a feedback-topology generic reports its look-ahead but runs none (as before the
        // unification); report 0 if a format ever pairs the two.
        Law::Generic(g) => lookahead_frames(g.lookahead.seconds(), rate),
        Law::KontaktPro(_) | Law::KontaktLimiter(_) => KONTAKT_LATENCY,
        Law::KontaktHilbert(_) => hilbert_window(rate) - 1,
        Law::UviMaximizer(m) => maximizer_latency(&Maximizer::from(m), rate),
        _ => 0,
    }
}

/// Cells of the slab a law needs; `None` if it cannot run at this rate.
pub(super) fn cells(law: &Law, rate: u32) -> Option<usize> {
    Some(match law {
        Law::Generic(g) => lookahead_frames(g.lookahead.seconds(), rate) + generic_extra_cells(g),
        Law::KontaktPro(_) | Law::KontaktLimiter(_) => KONTAKT_LATENCY + KONTAKT_LATENCY + 1,
        Law::KontaktHilbert(_) => 3 * hilbert_window(rate) - 1 + HILBERT_CELLS + 2,
        Law::KontaktFeedback { .. } => FEEDBACK_CELLS,
        Law::UviMaximizer(m) => {
            let m = Maximizer::from(m);
            let w = maximizer_window(m.lookahead, rate);
            if w < 2 {
                return None;
            }
            maximizer_latency(&m, rate) + w + w / 2 + TRUE_PEAK_CELLS
        }
        Law::KontaktClassic(_) | Law::KontaktEnhanced(_) | Law::KontaktTransient { .. } | Law::KontaktBusComp { .. } => 0,
    })
}

pub(super) fn lookahead_frames(seconds: f64, rate: u32) -> usize {
    (seconds * f64::from(rate)).round() as usize
}

// --- Generic -----------------------------------------------------------------

fn generic_extra_cells(g: &Generic) -> usize {
    if g.detector == Detector::TruePeak { TRUE_PEAK_CELLS } else { 0 }
}

fn pole(seconds: f64, rate: u32) -> f64 {
    let frames = seconds * f64::from(rate);
    if frames <= 0. { 0. } else { (-1. / frames).exp() }
}

/// `knobs` = [threshold dB, ratio, attack s, release s].
pub(crate) fn generic_ratio(g: &Generic, [threshold_db, ratio, _, _]: [f64; 4], channels: usize) -> Ratio {
    Ratio {
        direction: g.direction,
        threshold_db,
        threshold: 10f64.powf(threshold_db / 20.),
        slope: match g.direction {
            Direction::Expand => ratio - 1.,
            _ => 1. - 1. / ratio,
        },
        knee_db: g.knee_db,
        knee_floor: 10f64.powf((threshold_db - g.knee_db / 2.) / 20.),
        range: if g.range_db > 0. { g.range_db } else { f64::INFINITY },
        channels,
    }
}

// --- Kontakt Compressor, Classic (mode 0): core 0x140ab2250, setter 0x140ab9220 ---

pub(super) struct Classic {
    /// `2^(dB/6)`, f32.
    pub threshold: f32,
    /// Gain at full follower `thr^(1-ratio)`, f32.
    pub gain: f32,
    /// Follower step `0.625^(1/n)` and hold decay `(1/3)^(1/n)`.
    pub attack: f64,
    pub release: f64,
}

impl Classic {
    /// `knobs` = [threshold dB, saved ratio, attack s, release s].
    pub fn new([db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let threshold = (db as f32 * f32::from_bits(0x3e2a_aaab)).exp2();
        // Setter: `pow(2, (L*c1 - L*c1*ratio) * c2)` with L = ln(threshold),
        // c1 = -8.65617 (`0x144fc4d28`), c2 = -1/6 (`0x144fc4c58`); f32 up to the pow.
        let scaled = threshold.ln() * -8.656_169_89_f32;
        let exponent = (scaled - scaled * ratio as f32) * f32::from_bits(0xbe2a_aaab);
        let gain = 2_f64.powf(f64::from(exponent)) as f32;
        // trunc(ms * 0.001 * rate + 0.5) in f32, at least one frame.
        let frames = |seconds: f64| {
            let n = (seconds as f32 * 1000. * 0.001_f32 * rate as f32 + 0.5) as u64;
            n.max(1) as f64
        };
        Self {
            threshold,
            gain,
            attack: 0.625_f64.powf(1. / frames(attack)),
            release: (1. / 3_f64).powf(1. / frames(release)),
        }
    }
}

// --- Kontakt Compressor, Enhanced (mode 1): Ghidra FUN_140ab9220 / FUN_140ab2c80 ---

pub(super) struct Enhanced {
    threshold: f32,
    excess_scale: f32,
    attack: f32,
    release: f32,
}

impl Enhanced {
    /// `knobs` = [threshold dB, saved ratio, attack s, release s].
    fn new([db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let [db, ratio, attack_ms, release_ms] =
            [db as f32, ratio as f32, (attack * 1000.) as f32, (release * 1000.) as f32];
        let threshold = (db * f32::from_bits(0x3e2aaaab)).exp2();
        let slope = if ratio < 0.04 { 5. } else { 1. / (ratio + 0.13) - 0.884 };
        let attack_factor = (f64::from(attack_ms).ln() * 0.2895294507215381 + 1.) as f32;
        Self {
            threshold,
            excess_scale: (slope / threshold.max(0.0004)) * attack_factor,
            attack: 2513.2 / (rate as f32 * attack_ms.max(1.)),
            release: if release_ms < 50. { 1. - 35.566 / rate as f32 } else { 1. - 1778.3 / (rate as f32 * release_ms) },
        }
    }
}

// --- Kontakt Compressor, Pro (mode 2): core 0x140ab35d0, setter 0x140ab9220 ---
// --- Kontakt Limiter (SerType 0x1c): process 0x140904890, setter 0x140960140 ---

/// The Pro and Limiter look-ahead (and window less one), frames.
pub(super) const KONTAKT_LATENCY: usize = 32;
const PRO_FLOOR: f32 = f32::from_bits(0x2b8c_bccc); // 0x144f385a8
const TAU: f32 = f32::from_bits(0x40c9_0fe4); // 0x144f38be0

pub(super) struct Pro {
    threshold_db: f32,
    /// `-(1 - ratio)`.
    reduction: f32,
    /// `(1 - ratio) * threshold_db * -0.666`: the threshold-tracking make-up.
    offset: f32,
    /// Release: fraction of a falling step taken per frame.
    pub release: f32,
    /// Biquad: b0 (= b2), b1, -a1, -a2.
    pub b0: f32,
    pub b1: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Pro {
    /// `knobs` = [threshold dB, saved ratio, attack s, release s].
    pub fn new([db, ratio, attack, release]: [f64; 4], rate: u32) -> Self {
        let rate = rate as f32;
        // Threshold: powf(2, dB/6) then back through ln in the setter.
        let threshold = 2_f32.powf(db as f32 * f32::from_bits(0x3e2a_aaab));
        let threshold_db = (f64::from(threshold.ln()) * 8.656_170_245_333_781) as f32;
        let remaining = 1. - ratio as f32;
        let offset = remaining * threshold_db * f32::from_bits(0xbf2a_7efa);
        // Release 10..1000 ms, at least one frame.
        let release_ms = (release as f32 * 1000.).clamp(10., 1000.);
        let release = f32::from_bits(0x3f31_7215) / (release_ms * 0.001 * rate).max(1.);
        // Attack 0.1..10 ms sets the detector low-pass: 1000/ms Hz, clamped.
        let attack_ms = (attack as f32 * 1000.).clamp(0.1, 10.);
        let wanted = f32::from_bits(0x4479_ffff) / attack_ms;
        let (low, high) = (rate * f32::from_bits(0x382a_aaab), rate * f32::from_bits(0x3ef0_f0f1));
        let hertz = if wanted <= high { low.max(wanted) } else { high };
        let (b0, b1, a1, a2) = rbj_low_pass(hertz * (TAU / rate));
        Self { threshold_db, reduction: -remaining, offset, release, b0, b1, a1, a2 }
    }
}

pub(super) struct Limiter {
    /// Input gain `2^(x1/6)`.
    pub gain: f32,
    /// Fraction of a falling step the follower takes per frame.
    pub release: f32,
    b0: f32,
    b1: f32,
    a1: f32,
    a2: f32,
}

impl Limiter {
    /// `knobs` = [input gain x1, unused, unused, release s].
    pub fn new([x1, _, _, release]: [f64; 4], rate: u32) -> Self {
        let rate = rate as f32;
        let frames = (release as f32 * 1000. * 0.001 * rate).max(1.);
        // Cut-off at a fixed 1/32 cycle per frame: (rate/32) * (tau / rate).
        let (b0, b1, a1, a2) = rbj_low_pass((rate * 0.031_25) * (TAU / rate));
        Self {
            gain: 2_f32.powf(x1 as f32 * f32::from_bits(0x3e2a_aaab)),
            release: f32::from_bits(0x3f31_7215) / frames,
            b0,
            b1,
            a1,
            a2,
        }
    }
}

// --- Kontakt Transparent Limiter (SerType 0x5f): core NI::DSP::HilbertLimiter,
// process 0x141ae3400, setter 0x141aea680, prepare 0x141ae23a0, Hilbert pair
// 0x141aa4400 (designed by 0x141a81a90), gain smoother 0x141aa42a0 ---

const HILBERT_EPS: f32 = f32::from_bits(0x2b8c_bccc); // 0x144f385a8, 1e-12
/// Prototype allpass coefficients, lanes [A, B, A, B], 0x1479ab7f0..
const PROTOTYPE: [[u32; 2]; 6] = [
    [0x3ea8_8690, 0x3f18_24e4],
    [0x3f46_065d, 0x3f61_b493],
    [0x3f71_c293, 0x3f7b_d4e2],
    [0xbea8_8690, 0xbf18_24e4],
    [0xbf46_065d, 0xbf61_b493],
    [0xbf71_c293, 0xbf7b_d4e2],
];
/// Warp of the pair, `-0.89` (reset `0x141ae15b0` stores 0.89, negated).
const WARP: f32 = -f32::from_bits(0x3f63_d70a);

/// `max(1, int(rate / 44100))`, the core's `rate * 2.2675737e-5`.
fn hilbert_multiple(rate: u32) -> usize {
    ((rate as f32 * f32::from_bits(0x37be_37c6)) as i32).max(1) as usize
}

/// Look-ahead window `L = 128 * max(1, int(rate / 44100))`.
pub(super) fn hilbert_window(rate: u32) -> usize {
    hilbert_multiple(rate) * 128
}

pub(super) struct Hilbert {
    drive: f32,
    ceiling: f32,
    release: f32,
    rise: f32,
    fall: f32,
    smooth: f32,
    /// Hilbert stage coefficients per lane.
    pub allpass: [[f32; 4]; 7],
    pub delay: usize,
    cos: f64,
    period: usize,
}

impl Hilbert {
    /// `knobs` = [drive x0, ceiling x2, unused, release x1 s], all 0..=1:
    /// drive `(1 - x) * 40` dB, ceiling `-0.35 - (1 - x) * 39.7` dB, release
    /// `max(x, 0.001)` s.
    pub fn new([drive, ceiling, _, release]: [f64; 4], rate: u32) -> Self {
        let fs = rate as f32;
        let unit = |x: f64| (x as f32).clamp(0., 1.);
        let db = |d: f32| 10f32.powf(d * f32::from_bits(0x3d4c_cccd));
        let drive_db = (1. - unit(drive)) * 40.;
        let ceiling_db = f32::from_bits(0xbeb3_3333) - (1. - unit(ceiling)) * f32::from_bits(0x421e_cccd);
        let release = unit(release).max(f32::from_bits(0x3a83_126f));
        let mut allpass = [[WARP; 4]; 7];
        for (stage, [a, b]) in allpass.iter_mut().zip(PROTOTYPE) {
            let warp = |c: u32| {
                let c = f32::from_bits(c);
                (c + WARP) / (c * WARP + 1.)
            };
            *stage = [warp(a), warp(b), warp(a), warp(b)];
        }
        let taps = hilbert_window(rate);
        Self {
            drive: db(drive_db),
            ceiling: db(ceiling_db),
            release: 1. - 1. / (release * fs),
            rise: 1. - f32::from_bits(0x428e_db6d) / fs,
            fall: 1. - f32::from_bits(0x43a6_aaab) / fs,
            smooth: 1. - f32::from_bits(0x44d0_5555) / fs,
            allpass,
            delay: hilbert_multiple(rate).min(8),
            cos: (1. / taps as f64 * std::f64::consts::TAU).cos(),
            period: (rate as usize).max(taps),
        }
    }
}

// --- Kontakt Feedback Compressor (SerType 0x4c): core NI::DSP::FBComp, process
// 0x141aa80d0, setter 0x141ac56d0, ratio law 0x141a91d10, prepare 0x141aa1280,
// time constants 0x141a92370 / 0x141ac3250 ---

pub(super) struct Feedback {
    drive: f32,
    attack: f32,
    release: f32,
    couple: f32,
    steps: usize,
    pub curve: FeedbackCurve,
    floor: f32,
    bypass: bool,
    hp: [f32; 3],
    lp: [f32; 3],
}

impl Feedback {
    /// `knobs` = [drive (linear), ratio (1..=20), attack s, release s].
    pub fn new([drive, ratio, attack, release]: [f64; 4], high_quality: bool, rate: u32) -> Self {
        let fs = rate as f32;
        let steps = if high_quality { 4 } else { 2 };
        let sub = 1. / steps as f32;
        let one_pole = |t: f32| 1. - table_exp(-((sub / (t * fs)) * TABLE_PER_NEPER));
        // Ratio law, 0x141a91d10.
        let r = ratio as f32;
        let r1 = r - 1.;
        let r1d = f64::from(r1);
        let slope = ((r1d * r1d) * (r1d * r1d) * f64::from_bits(0x3f10_ecd0_0000_0000)
            + f64::from(r1 * f32::from_bits(0x4019_702b))
            + 1.) as f32;
        let threshold = r.max(2.).powf(f32::from_bits(0x3e4c_cccd)) * f32::from_bits(0x3f34_359a)
            - f32::from_bits(0x3f2c_2971);
        let knee = ((f32::from_bits(0x4066_83fb) / r + f32::from_bits(0x3f57_1cdc))
            - f32::from_bits(0x3fb8_6405) / (r.sqrt() * r))
            + f32::from_bits(0x3d4c_cccd);
        let l = knee.ln();
        let om = 1. - slope;
        let g0 = table_exp(om * l * f32::from_bits(0x41ad_1f97));
        let ld = f64::from(l);
        let quartic = ((f64::from(om) / (ld * ld * ld)) * 0.25) as f32;
        let root = (f64::from_bits(0xbf87_9415_0000_0000) / f64::from(quartic).min(-0.1)).powf(0.25);
        let floor = table_exp(root as f32 * TABLE_PER_NEPER) * threshold;
        // Gain low-pass.
        let w = (10_000. / fs).atan().tan() * f32::from_bits(0x4875_6fda);
        let inv = 1. / (w + 20_000.);
        Self {
            drive: drive as f32,
            attack: one_pole(attack as f32),
            release: one_pole(release as f32),
            couple: 1. - table_exp(-((f32::from_bits(0x4332_d8b3) / fs) * sub * TABLE_PER_NEPER)),
            steps,
            curve: FeedbackCurve { threshold, knee_top: threshold * knee, slope, g0, quartic },
            floor,
            bypass: slope == 1.,
            // Detector high-pass and output DC blocker, 10 Hz.
            hp: high_pass_1(rate, f32::from_bits(0x41fb_53d1)),
            lp: [w * inv, w * inv, (w - 20_000.) * inv],
        }
    }
}

// --- Kontakt Transient Master (SerType 0x43): setter 0x141ac87b0, prepare
// 0x141aa27a0, state 0x141a83550 / 0x141aa0b80, process variants 0x141a4a9d0
// and (output clip) 0x141a59d40 ---

struct Transient {
    attack: f32,
    sustain: f32,
    gain: f32,
    hp: [f32; 3],
    sub: [f32; 3],
    k: [f32; 9],
}

impl Transient {
    /// Normalized saved attack, sustain and gain (0.5 is neutral).
    fn new([attack, sustain, gain]: [f32; 3], rate: u32) -> Self {
        let per = |bits: u32| f32::from_bits(bits) / rate as f32;
        let a = (attack - 0.5) * 120.;
        let s = (sustain - 0.5) * 115.;
        Self {
            attack: a + a,
            sustain: s + s,
            gain: (gain - 0.5) * if gain >= 0.5 { 12. } else { 48. },
            hp: high_pass_1(rate, f32::from_bits(0x41fb_53d1)),
            sub: high_pass_1(rate, f32::from_bits(0x4149_0fdb)),
            // e1 up/down, e2 up/down, e3 down, e4 down, e5 up/down, e6 up/down.
            k: [
                per(0x4765_7546),
                per(0x4128_091f),
                per(0x42cc_c52a),
                per(0x4128_091f),
                per(0x42ac_9e2b),
                per(0x415d_ce9a),
                per(0x44ad_4968),
                per(0x45b4_81ce),
                per(0x3ed0_c272),
            ],
        }
    }
}

// --- Solid Bus Comp (SerType 0x46): core NI::DSP::CSL::G_BusComp, process
// 0x141aaa1e0, detector 0x141a95cb0, setter 0x141ac69e0, time coefficient
// 0x141acc420, level map 0x141a9d400 ---

/// Core descriptor tables 0 and 1 (threshold and makeup dB), `0x1479a7d60` /
/// `0x1479a7dc0`.
pub(super) static BUS_THRESHOLD: LazyLock<Spline> = LazyLock::new(|| {
    Spline::new([-18.6, -14.88, -11.16, -7.44, -3.72, 0., 3.72, 7.44, 11.16, 14.88, 18.6])
});
static BUS_MAKEUP: LazyLock<Spline> =
    LazyLock::new(|| Spline::new([-5.25, -2.625, 0., 2.625, 5.25, 7.875, 10.5, 13.125, 15.75, 18.375, 21.]));
const BUS_RATIO: [f64; 6] = [1.5, 2., 3., 4., 5., 10.]; // 0x1479a7e18
const BUS_THRESHOLD_OFFSET: [f64; 6] = [-12., -9., -5., -4., -3.3, -0.2]; // 0x1479a7e48
const BUS_ATTACK_MS: [f64; 6] = [0.1, 0.3, 1., 3., 10., 30.]; // 0x1479a7e78
const BUS_RELEASE: [f64; 6] = [0.2, 0.3, 0.6, 0.8, 1.6, 1.2]; // 0x1479a7ea8, x 62.5 ms
const LN_08: f64 = f64::from_bits(0xbfcc_8ff7_c79a_9a20); // 0x1479ab6a8

pub(super) struct Bus {
    threshold: f64,
    slope: f64,
    pub makeup: f64,
    pub attack: f64,
    pub release: f64,
    auto: Option<[f64; 2]>,
}

impl Bus {
    /// Normalized core controls 0..=4: threshold, makeup, attack, release, ratio.
    pub fn new([threshold, makeup, attack, release, ratio]: [f64; 5], rate: u32) -> Self {
        // `floor(5x + .5)` on a 0..5 descriptor.
        let index = |x: f64| (x * 5. + 0.5).floor().clamp(0., 5.) as usize;
        let fs = f64::from(rate);
        let coefficient = |ms: f64| if ms == 0. { 0. } else { (LN_08 / (ms * 0.001 * fs)).exp().min(1.) };
        let r = index(ratio);
        let inverse = 1. / BUS_RATIO[r];
        let db = (BUS_THRESHOLD.eval(threshold) - 20.) + BUS_THRESHOLD_OFFSET[r];
        let (a, rl) = (index(attack), index(release));
        let attack_ms = BUS_ATTACK_MS[a];
        Self {
            threshold: bus_level(10f64.powf(db * 0.05) * 0.0078125),
            slope: 1. - 1. / inverse,
            makeup: 10f64.powf(BUS_MAKEUP.eval(makeup) * 0.05),
            attack: coefficient(attack_ms),
            release: coefficient(BUS_RELEASE[rl] * 62.5),
            auto: (rl == 5).then(|| [coefficient(attack_ms + 90.), coefficient(150.)]),
        }
    }
}

// --- UVI Maximizer (Workstation 4.0.9): registration 0x141279360, module
// 0x141278ac0, core init 0x141665430, prepare 0x141666580, process 0x141665620.
// The vector primitives are Intel IPP (`log10`, `exp`) called through an
// encrypted dispatch table: f32 libm stands in for them. ---

#[derive(Clone, Copy)]
pub(super) struct Maximizer {
    pub threshold: f32,
    pub attack: f32,
    pub lookahead: f32,
    pub release: f32,
    pub ceiling: f32,
    pub knee: f32,
    pub blend: f32,
    pub slew: f32,
    /// 0 brickwall, 1 soft, 2 hard.
    pub mode: u8,
    pub true_peak: bool,
}

impl From<&MaxSaved> for Maximizer {
    fn from(m: &MaxSaved) -> Self {
        Self {
            threshold: m.threshold_db as f32,
            attack: m.attack_ms as f32,
            lookahead: m.lookahead_ms as f32,
            release: m.release_ms as f32,
            ceiling: m.ceiling_db as f32,
            knee: m.knee_db as f32,
            blend: m.release_blend as f32,
            slew: m.slew_ms as f32,
            mode: m.mode,
            true_peak: m.true_peak,
        }
    }
}

/// Look-ahead window `W` in frames: `int(ms * fs * .0005) * 2`.
pub(super) fn maximizer_window(lookahead: f32, rate: u32) -> usize {
    ((lookahead * rate as f32 * 0.0005) as i32).max(0) as usize * 2
}

/// Delay in frames: `W`, plus 6 with true peak.
pub(super) fn maximizer_latency(m: &Maximizer, rate: u32) -> usize {
    maximizer_window(m.lookahead, rate) + if m.true_peak { 6 } else { 0 }
}

struct MaxCoefficients {
    attack: f32,
    fast: f32,
    slow: f32,
    slew: f32,
}

impl MaxCoefficients {
    fn new(c: &Maximizer, rate: u32) -> Self {
        let fs = rate as f32;
        let ln3 = f64::from_bits(0xbff1_93ea_7aad_030b); // -ln 3, 0x141e64248
        let attack = if c.mode == 0 { 0. } else { c.attack * f32::from_bits(0x3a83_126f) * fs };
        let release = (f64::from(c.release) * 0.001 * f64::from(fs)) as f32;
        let pole = |k: f64, t: f32| if t > 0. { (k / f64::from(t)).exp() as f32 } else { 0. };
        Self {
            attack: pole(ln3, attack),
            fast: pole(ln3, release),
            slow: pole(f64::from_bits(0xbf86_7fe4_7410_a7bc), release), // 0x14201f5b0
            slew: if c.slew > 0. { (1e4 / (f64::from(fs) * f64::from(c.slew))) as f32 } else { f32::MAX },
        }
    }
}
