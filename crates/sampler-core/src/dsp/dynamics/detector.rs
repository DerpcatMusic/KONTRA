//! Detector stages: from `audio` to `level` (per channel, or linked into
//! channel 0). Each is one law's measured detector; the true-peak
//! interpolator and the sliding window are shared.

use super::pipeline::{Frame, Stage, Window};
use super::ProcessorState;
use sampler_ir::fx::dynamics::{Detector, Link};

/// ITU-R BS.1770-4 Annex 2 true-peak phases, oldest tap first (UVI
/// Workstation `0x14201f4e0`).
pub(super) const PHASES: [[u32; 12]; 4] = [
    [
        0x3ae0_0000, 0x3c34_0000, 0xbca1_0000, 0x3d08_0000, 0xbd73_8000, 0x3e0c_a000, 0x3f78_e000,
        0xbdd1_8000, 0x3d43_0000, 0xbcda_0000, 0x3c74_0000, 0xbc08_0000,
    ],
    [
        0xbcef_0000, 0x3cf0_0000, 0xbd54_0000, 0x3db6_8000, 0xbe2a_8000, 0x3eee_2000, 0x3f47_a000,
        0xbe4d_2000, 0x3dd0_0000, 0xbd6e_8000, 0x3d07_8000, 0xbc9b_0000,
    ],
    [
        0xbc9b_0000, 0x3d07_8000, 0xbd6e_8000, 0x3dd0_0000, 0xbe4d_2000, 0x3f47_a000, 0x3eee_2000,
        0xbe2a_8000, 0x3db6_8000, 0xbd54_0000, 0x3cf0_0000, 0xbcef_0000,
    ],
    [
        0xbc08_0000, 0x3c74_0000, 0xbcda_0000, 0x3d43_0000, 0xbdd1_8000, 0x3f78_e000, 0x3e0c_a000,
        0xbd73_8000, 0x3d08_0000, 0xbca1_0000, 0x3c34_0000, 0x3ae0_0000,
    ],
];

/// One phase over the history `r` (`r(0)` oldest), in the native add tree
/// (three accumulators, `0x1416679c0..0x141667bc0`).
#[inline(always)]
fn interpolate(p: &[u32; 12], r: impl Fn(usize) -> f32) -> f32 {
    let t = |j: usize| f32::from_bits(p[j]);
    let head = (((t(2) * r(2) + (r(0) * t(0) + r(1) * t(1))) + t(3) * r(3)) + t(4) * r(4)) + (t(5) * r(5) + t(6) * r(6));
    let tail = ((t(7) * r(7) + t(8) * r(8)) + t(9) * r(9)) + (r(11) * t(11) + t(10) * r(10));
    head + tail
}

/// 4x true peak: a 12-frame history, cursor in `aux[slot]`.
pub(super) struct TruePeak<'a> {
    history: &'a mut [[f64; 2]],
    at: usize,
    slot: usize,
}

pub(super) const TRUE_PEAK_CELLS: usize = 12;

impl<'a> TruePeak<'a> {
    pub fn new(history: &'a mut [[f64; 2]], state: &ProcessorState, slot: usize) -> Self {
        Self { history, at: state.aux[slot] as usize, slot }
    }

    /// Largest interpolated magnitude per channel, `x` the newest frame.
    #[inline(always)]
    pub fn push(&mut self, x: [f64; 2]) -> [f32; 2] {
        self.history[self.at] = x;
        self.at = (self.at + 1) % TRUE_PEAK_CELLS;
        let (h, at) = (&*self.history, self.at);
        [0, 1].map(|ch| {
            let r = |j: usize| h[(at + j) % TRUE_PEAK_CELLS][ch] as f32;
            PHASES.iter().fold(0f32, |m, p| m.max(interpolate(p, r).abs()))
        })
    }

    pub fn store(&self, state: &mut ProcessorState) {
        state.aux[self.slot] = self.at as f64;
    }
}

/// Generic: optional feedback tap and first-order side-chain high-pass, the
/// signed mean when `Link::Mean`, peak / true peak / RMS, the channel maximum
/// when `Link::Max`. State: `z[c][1]` mean square, `aux[0..4]` high-pass
/// (x1, y1 per channel), `aux[4..6]` the last gained frame, `aux[6]` the
/// true-peak cursor.
pub(super) struct Sidechain<'a> {
    pub feedback: bool,
    pub highpass: f64,
    pub link: Link,
    pub detector: Detector,
    pub rms: f64,
    pub true_peak: Option<TruePeak<'a>>,
    hp: [f64; 4],
    mean_square: [f64; 2],
    last: [f64; 2],
}

impl<'a> Sidechain<'a> {
    pub fn new(state: &ProcessorState, true_peak: Option<TruePeak<'a>>) -> Self {
        Self {
            feedback: false,
            highpass: 0.,
            link: Link::Off,
            detector: Detector::Peak,
            rms: 0.,
            true_peak,
            hp: [state.aux[0], state.aux[1], state.aux[2], state.aux[3]],
            mean_square: [state.z[0][1], state.z[1][1]],
            last: [state.aux[4], state.aux[5]],
        }
    }
}

impl Stage for Sidechain<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let mut sc = if self.feedback { self.last } else { f.audio };
        if self.highpass > 0. {
            for ch in 0..2 {
                let y = self.highpass * (self.hp[2 * ch + 1] + sc[ch] - self.hp[2 * ch]);
                (self.hp[2 * ch], self.hp[2 * ch + 1]) = (sc[ch], y);
                sc[ch] = y;
            }
        }
        if self.link == Link::Mean {
            sc = [(sc[0] + sc[1]) * 0.5; 2];
        }
        let mut level = sc.map(f64::abs);
        if let Some(tp) = &mut self.true_peak {
            level = tp.push(sc).map(f64::from);
        }
        if let Detector::Rms { .. } = self.detector {
            for ch in 0..2 {
                self.mean_square[ch] = sc[ch] * sc[ch] + self.rms * (self.mean_square[ch] - sc[ch] * sc[ch]);
                level[ch] = self.mean_square[ch].sqrt();
            }
        }
        if self.link == Link::Max {
            level = [level[0].max(level[1]); 2];
        }
        f.level = level;
    }

    #[inline(always)]
    fn after(&mut self, f: &Frame) {
        self.last = [f.audio[0] * f.gain[0], f.audio[1] * f.gain[1]];
    }

    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[..4].copy_from_slice(&self.hp);
        state.aux[4..6].copy_from_slice(&self.last);
        (state.z[0][1], state.z[1][1]) = (self.mean_square[0], self.mean_square[1]);
        if let Some(tp) = &self.true_peak {
            tp.store(state);
        }
    }
}

/// Kontakt Classic / Enhanced: `|x|` per channel, or of the signed mean
/// (each channel scaled by 1/2, as the cores) when linked. f32.
pub(super) struct SignedMean(pub bool);

impl Stage for SignedMean {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let x = f.audio.map(|v| v as f32);
        let mean = x[0] * 0.5 + x[1] * 0.5;
        f.level = [0, 1].map(|ch| f64::from(if self.0 { mean } else { x[ch] }.abs()));
    }
}

/// Channel maximum of `|x|` (Kontakt Limiter), f32.
pub(super) struct ChannelMax;

impl Stage for ChannelMax {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let m = f64::from((f.audio[0] as f32).abs().max((f.audio[1] as f32).abs()));
        f.level = [m; 2];
    }
}

/// The sliding maximum of `level[0]` over the window, at least `floor`, f32
/// (Kontakt Pro on the channel maximum, Kontakt Limiter on its follower).
pub(super) struct Windowed<'a> {
    pub window: Window<'a>,
    pub floor: f32,
}

impl Stage for Windowed<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let m = f64::from((self.window.push(f.level[0]) as f32).max(self.floor));
        f.level = [m; 2];
    }

    fn store(&mut self, state: &mut ProcessorState) {
        self.window.store(state);
    }
}

/// Kontakt Transparent Limiter: two 7-stage first-order allpass chains per
/// channel (lanes `[L a, L b, R a, R b]`) whose magnitude each one-pole
/// follows (rise / fall), maxed with the raw magnitudes into `level[0]`; the
/// window maximum of the driven peaks into `peak`. State: `aux[0]` frame
/// counter (mod 8 matters), `aux[1..3]` the envelopes, window from `aux[5]`;
/// cells: 7 stages x 8 slots x 4 lanes.
pub(super) struct Hilbert<'a> {
    pub allpass: [[f32; 4]; 7],
    pub delay: usize,
    pub rise: f32,
    pub fall: f32,
    pub window: Window<'a>,
    cells: &'a mut [[f64; 2]],
    n: usize,
    env: [f32; 2],
}

pub(super) const HILBERT_CELLS: usize = 7 * 16;

impl<'a> Hilbert<'a> {
    pub fn new(
        coefficients: ([[f32; 4]; 7], usize, f32, f32),
        window: Window<'a>,
        cells: &'a mut [[f64; 2]],
        state: &ProcessorState,
    ) -> Self {
        let (allpass, delay, rise, fall) = coefficients;
        let env = [state.aux[1] as f32, state.aux[2] as f32];
        Self { allpass, delay, rise, fall, window, cells, n: state.aux[0] as usize, env }
    }
}

impl Stage for Hilbert<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let driven = f.audio.map(|v| v as f32);
        f.peak = self.window.push(f64::from(driven[0].abs().max(driven[1].abs())));
        let mut lanes = [driven[0], driven[0], driven[1], driven[1]];
        let mut last_input = lanes;
        let n = self.n;
        let (write, read) = ((n + 1) & 7, (n + 1).wrapping_sub(self.delay) & 7);
        for (s, coef) in self.allpass.iter().enumerate() {
            last_input = lanes;
            let slot = |k: usize, j: usize| s * 16 + k * 2 + j / 2;
            for (k, lane) in lanes.iter_mut().enumerate() {
                let r = self.cells[slot(read, k)][k & 1] as f32;
                let v = (*lane - r) * coef[k];
                self.cells[slot(write, k)][k & 1] = f64::from(v + *lane);
                *lane = v + r;
            }
        }
        // Lanes a skip the last (warp) stage.
        let magnitude = |re: f32, im: f32| (re * re + im * im).sqrt();
        let follow = |env: f32, m: f32| {
            let k = if m <= env { self.fall } else { self.rise };
            (env - m) * k + m
        };
        self.env[0] = follow(self.env[0], magnitude(last_input[0], lanes[1]));
        self.env[1] = follow(self.env[1], magnitude(last_input[2], lanes[3]));
        let level = self.env[0].max(self.env[1]).max(driven[0].abs()).max(driven[1].abs());
        f.level = [f64::from(level); 2];
        self.n = n.wrapping_add(1);
    }

    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[0] = (self.n % 8) as f64;
        state.aux[1] = f64::from(self.env[0]);
        state.aux[2] = f64::from(self.env[1]);
        self.window.store(state);
    }
}

/// Kontakt Transient Master: the channel maximum of a 10 Hz high-passed
/// input, raised to the 0.1 power (native table), times 0.7. State
/// `aux[0..4]` (x1, y1 per channel), f32.
pub(super) struct TenthRoot {
    pub hp: [f32; 3],
    f: [f32; 4],
}

impl TenthRoot {
    pub fn new(hp: [f32; 3], state: &ProcessorState) -> Self {
        Self { hp, f: std::array::from_fn(|i| state.aux[i] as f32) }
    }
}

/// `(i / 10000)^0.1`, `0x1478fb240`.
fn root_table() -> &'static [f32; 30_002] {
    static T: std::sync::OnceLock<Box<[f32; 30_002]>> = std::sync::OnceLock::new();
    T.get_or_init(|| Box::new(std::array::from_fn(|i| (i as f64 / 10_000.).powf(0.1) as f32)))
}

pub(super) fn tenth_root(x: f32) -> f32 {
    if !(f32::from_bits(0x3a03_126f)..=3.).contains(&x) {
        return x.powf(f32::from_bits(0x3dcc_cccd));
    }
    let t = root_table();
    let p = x * 10_000.;
    let i = p as i32 as usize;
    (t[i + 1] - t[i]) * (p - i as f32) + t[i]
}

impl Stage for TenthRoot {
    #[inline(always)]
    fn tick(&mut self, fr: &mut Frame) {
        let (c, f) = (self.hp, &mut self.f);
        let mut det = 0f32;
        for ch in 0..2 {
            let x = fr.audio[ch] as f32;
            let y = (x * c[0] + c[1] * f[2 * ch]) - c[2] * f[2 * ch + 1];
            (f[2 * ch], f[2 * ch + 1]) = (x, y);
            det = det.max(y.abs());
        }
        fr.level = [f64::from(tenth_root(det) * f32::from_bits(0x3f33_3333)); 2];
    }

    fn store(&mut self, state: &mut ProcessorState) {
        for (i, v) in self.f.iter().enumerate() {
            state.aux[i] = f64::from(*v);
        }
    }
}

/// Kontakt Feedback Compressor: `|x|` (the channel maximum when linked)
/// through the 10 Hz high-pass, with the core's 1e-18 offsets. State
/// `aux[0..4]` (x1, y1 per detector), f32.
pub(super) struct HighPassed {
    pub hp: [f32; 3],
    pub linked: bool,
    f: [f32; 4],
}

pub(super) const EPS_18: f32 = f32::from_bits(0x2193_92ef); // 1e-18

impl HighPassed {
    pub fn new(hp: [f32; 3], linked: bool, state: &ProcessorState) -> Self {
        Self { hp, linked, f: std::array::from_fn(|i| state.aux[i] as f32) }
    }
}

impl Stage for HighPassed {
    #[inline(always)]
    fn tick(&mut self, fr: &mut Frame) {
        let s = fr.audio.map(|v| v as f32);
        let raw = if self.linked { [s[0].abs().max(s[1].abs()); 2] } else { s.map(f32::abs) };
        let (c, f) = (self.hp, &mut self.f);
        for k in 0..if self.linked { 1 } else { 2 } {
            let (x1, y1) = (f[2 * k], f[2 * k + 1]);
            let y = (raw[k] * c[0] + x1 * c[1]) - y1 * c[2];
            (f[2 * k], f[2 * k + 1]) = (raw[k] + EPS_18, y + EPS_18);
            fr.level[k] = f64::from(y);
        }
    }

    fn store(&mut self, state: &mut ProcessorState) {
        for (i, v) in self.f.iter().enumerate() {
            state.aux[i] = f64::from(*v);
        }
    }
}

/// Solid Bus Comp: each channel's own previous output level `g * x / 128`,
/// as `log2 / 32` (floor -24). State `aux[2..4]`, f64.
pub(super) struct BusTap([f64; 2]);

impl BusTap {
    pub fn new(state: &ProcessorState) -> Self {
        Self([state.aux[2], state.aux[3]])
    }
}

/// `log2(v) / 32`, with `-24` below `2^-24`.
pub(super) fn bus_level(v: f64) -> f64 {
    (if v >= (-24f64).exp2() { v.log2() } else { -24. }) * 0.03125
}

impl Stage for BusTap {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        f.level = self.0.map(|v| bus_level(v.abs().min(1.)));
    }

    #[inline(always)]
    fn after(&mut self, f: &Frame) {
        self.0 = [0, 1].map(|ch| f.gain[ch] * f.audio[ch] * 0.0078125);
    }

    fn store(&mut self, state: &mut ProcessorState) {
        state.aux[2..4].copy_from_slice(&self.0);
    }
}

/// UVI Maximizer: the channel maximum of `|x|` or of the true-peak phases,
/// floored at 1e-10, f32.
pub(super) struct MaxPeak<'a>(pub Option<TruePeak<'a>>);

const MAX_FLOOR: f32 = f32::from_bits(0x2edb_e6ff); // 0x141e6e4f0

impl Stage for MaxPeak<'_> {
    #[inline(always)]
    fn tick(&mut self, f: &mut Frame) {
        let x = f.audio.map(|v| v as f32);
        let level = match &mut self.0 {
            Some(tp) => {
                let [l, r] = tp.push(f.audio);
                MAX_FLOOR.max(l).max(r)
            }
            None => MAX_FLOOR.max(x[0].abs()).max(x[1].abs()),
        };
        f.level = [f64::from(level); 2];
    }

    fn store(&mut self, state: &mut ProcessorState) {
        if let Some(tp) = &self.0 {
            tp.store(state);
        }
    }
}
