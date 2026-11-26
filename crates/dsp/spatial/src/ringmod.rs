//! A diode ring modulator and frequency shifter: a one-pole high-pass, an
//! analytic pair from two polyphase allpass chains, each branch multiplied
//! by a quadrature carrier through a diode pair `√((k(c + x))² + 1) −
//! √((k(c − x))² + 1)` (first-order antiderivative antialiasing), the
//! quadrature branch weighted in, a silence gate and the dry mixed in.
//! Feedback takes the band-limited wet output back off the input, solved
//! implicitly (at most four fixed-point passes, as the native core does).
//! The carrier is a sine, the input's own analytic pair (sidechain), or a
//! sine the input is transmitted on and then received like an AM radio.
//! The structure is the Freak core's (ctor [ref], process
//! [ref]; laws read through v2 sampler-core dsp/color/freak.rs,
//! which matched vectors of the emulated core); the Hilbert pair here is
//! Olli Niemitalo's fixed design, a phase difference within 0.7° over
//! 0.002..0.998 of the band, not the native per-rate elliptic one (which
//! differs in phase only), and the receiver's filters are TPT state
//! variable ones at the native corners and dampings.

use std::f32::consts::PI;

use moose_dsp::svf::{Svf, SvfCoeffs};

/// The settings in signal terms (see `ir::RingMod`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingMod {
    pub hz: f32,
    pub spread: f32,
    pub bias: f32,
    pub quadrature: f32,
    pub drive: f32,
    pub highpass_hz: f32,
    pub gate: bool,
    pub feedback: f32,
    pub source: Source,
    pub wet: f32,
    pub dry: f32,
}

/// The carrier (see `ir::RingSource`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Oscillator,
    Sidechain {
        contour: f32,
        attack: f32,
        release: f32,
        bandpass_hz: Option<f32>,
    },
    Radio {
        gain: f32,
        tuned_hz: f32,
        damping: f32,
        makeup: f32,
        /// DEMOD: how far the received signal crossfades to the band times
        /// the tuned sine (unfiltered), and the envelope into the carrier.
        product: f32,
        noise: f32,
    },
    /// A continuous sine-to-triangle carrier whose rate is modulated by a
    /// parabolic sine or square LFO, followed by a carrier-driven stereo FM
    /// delay line.
    Modulated {
        /// 0 is parabolic sine, 1 is triangle.
        edge: f32,
        /// Peak stereo FM delay swing, ms.
        fm_ms: f32,
        /// LFO frequency, Hz.
        lfo_hz: f32,
        /// Peak upward carrier-rate modulation multiplier above base.
        lfo_depth: f32,
        /// `true` selects a square LFO; `false` selects a parabolic sine LFO.
        lfo_square: bool,
    },
}

/// The two branches' allpass coefficients `a`; each stage is
/// `(a² − z⁻²)/(1 − a²·z⁻²)`.
const BRANCHES: [[f32; 4]; 2] = [
    [0.692_387_8, 0.936_065_4, 0.988_229_5, 0.998_748_8],
    [0.402_192_1, 0.856_171_1, 0.972_291, 0.995_288_5],
];

/// The ADAA fallback's relative threshold.
const CLOSE: f32 = 3.452_7e-4;

/// The carrier's sine of a triangle phase −1..1 (`sin(π/2·t)`).
fn sine(t: f32) -> f32 {
    let t2 = t * t;
    (1.570_796_4 * t - 0.641_592_7 * t * t2) + 0.070_796_33 * t * (t2 * t2)
}

/// `−cos(2π·u)` for `u` in 0..1.
fn carrier(u: f32) -> f32 {
    let u = u - u.floor();
    sine(1.0 - (2.0 - 4.0 * u).abs())
}

/// The antiderivative of `√(q² + 1) − 1`.
fn antiderivative(q: f32) -> f32 {
    let r = (q * q + 1.0).sqrt();
    ((q + r).ln() + q * r) * 0.5 - q
}

/// The diode pair `√((k(m + a))² + 1) − √((k(m − a))² + 1)` without
/// antialiasing (the implicit solve's estimate).
fn diodes(k: f32, m: f32, a: f32) -> f32 {
    let (p, q) = (k * (m + a), k * (m - a));
    (p * p + 1.0).sqrt() - (q * q + 1.0).sqrt()
}

/// `1 − e^(ln 0.01 / (seconds · rate))`: a level follower's step.
fn follow(seconds: f32, rate: f32) -> f32 {
    if seconds > 0.0 {
        1.0 - (-std::f32::consts::LN_10 * 2.0 / (seconds * rate)).exp()
    } else {
        1.0
    }
}

fn track(level: &mut f32, x: f32, attack: f32, release: f32) {
    *level += (x - *level) * if x <= *level { release } else { attack };
}

/// A TPT one-pole's step `G/(1 + G)`, `G = tan(π·hz/rate)`.
fn pole(hz: f32, rate: f32) -> f32 {
    let g = (PI * hz.clamp(0.0, 0.49 * rate) / rate).tan();
    g / (1.0 + g)
}

/// A TPT one-pole low-pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct OnePole(f32);

impl OnePole {
    /// The low-pass of `x` were it fed now (the state unchanged).
    fn peek(self, x: f32, g: f32) -> f32 {
        g * x + (1.0 - g) * self.0
    }

    fn tick(&mut self, x: f32, g: f32) -> f32 {
        let v = (x - self.0) * g;
        let low = v + self.0;
        self.0 = low + v;
        low
    }
}

/// A TPT state-variable filter's coefficients at `hz`, held within
/// 1 Hz..=0.49 of the rate (damping `k = 1/Q`).
fn svf(hz: f32, k: f32, rate: f32) -> SvfCoeffs<f32> {
    SvfCoeffs::new(hz.clamp(1.0, 0.49 * rate), k, rate)
}

/// `√(q² + 1) − 1` antialiased against the previous input.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Diode {
    x: f32,
    f: f32,
}

impl Diode {
    fn tick(&mut self, x: f32) -> f32 {
        let f = antiderivative(x);
        let dx = x - self.x;
        let y = if dx.abs() < (x.abs() + self.x.abs() + 10.0) * CLOSE {
            let m = (x + self.x) * 0.5;
            (m * m + 1.0).sqrt() - 1.0
        } else {
            (f - self.f) / dx
        };
        (self.x, self.f) = (x, f);
        y
    }
}

/// `|x|` antialiased against the previous input (the radio's rectifier).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Rectifier {
    x: f32,
    f: f32,
}

impl Rectifier {
    fn tick(&mut self, x: f32) -> f32 {
        let f = 0.5 * x * x.abs();
        let dx = x - self.x;
        let y = if dx.abs() < (x.abs() + self.x.abs() + 0.2) * 1e-5 {
            ((x + self.x) * 0.5).abs()
        } else {
            (f - self.f) / dx
        };
        (self.x, self.f) = (x, f);
        y
    }
}

/// A high-pass and an analytic pair from two allpass chains.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Analytic {
    hp: OnePole,
    /// Per branch and stage `[x₁, x₂, y₁, y₂]`.
    allpass: [[[f32; 4]; 4]; 2],
    /// The first branch's last output (its one-frame delay).
    delayed: f32,
}

impl Analytic {
    /// The analytic pair of `x` high-passed with step `g`.
    fn tick(&mut self, x: f32, g: f32) -> [f32; 2] {
        let x = x - self.hp.tick(x, g);
        let mut out = [x; 2];
        for ((o, stages), coefs) in out.iter_mut().zip(&mut self.allpass).zip(BRANCHES) {
            for (s, a) in stages.iter_mut().zip(coefs) {
                let [x1, x2, y1, y2] = *s;
                let y = a * a * (*o + y2) - x2;
                *s = [*o, x1, y, y1];
                *o = y;
            }
        }
        let [re, im] = out;
        [std::mem::replace(&mut self.delayed, re), im]
    }
}

/// The radio receiver's state for one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Receiver {
    /// The noise generator's state; 0 (never a MINSTD value) until the
    /// first sample seeds it with 12345.
    seed: u32,
    noise: OnePole,
    band: Svf<f32>,
    rectifier: Rectifier,
    dc: OnePole,
    low: [Svf<f32>; 2],
}

/// One channel's state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Channel {
    input: Analytic,
    /// Sum and difference diodes per branch.
    diodes: [[Diode; 2]; 2],
    /// The sidechain's band-pass and analytic pair.
    side_band: Svf<f32>,
    side: Analytic,
    /// The feedback path: the last wet output and its 15 kHz low-pass and
    /// 20 Hz high-pass.
    wet: f32,
    smooth: OnePole,
    dc: OnePole,
    radio: Receiver,
}

/// The coefficients `set` derives.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Coefs {
    inc: f32,
    hg: f32,
    k: f32,
    norm: f32,
    attack: f32,
    release: f32,
    /// Feedback: the 15 kHz and 20 Hz steps and the output's makeup.
    smooth: f32,
    dc: f32,
    makeup: f32,
    /// Sidechain: the band-pass and the envelope's steps.
    side_band: SvfCoeffs<f32>,
    side_attack: f32,
    side_release: f32,
    /// Radio: the noise and 50 Hz steps, the band-pass, the low-pass pair,
    /// the demodulating sine's increment and the level control's steps.
    noise: f32,
    hum: f32,
    band: SvfCoeffs<f32>,
    low: [SvfCoeffs<f32>; 2],
    demod: f32,
    agc_attack: f32,
    agc_release: f32,
    /// Modulated carrier: phase increments (in [-1, 1) cycles), carrier shape
    /// weights, and peak FM delay in frames.
    mod_inc: f32,
    lfo_inc: f32,
    edge_sin: f32,
    edge_tri: f32,
    fm_frames: f32,
    mod_smooth: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    s: RingMod,
    c: Coefs,
    phase: f32,
    mod_phase: f32,
    lfo_phase: f32,
    /// Smoothed `[dry, wet, fm_frames, mod_inc]` for click-free live drags.
    mod_live: [f32; 4],
    mod_init: bool,
    envelope: f32,
    side: f32,
    demod: f32,
    agc: f32,
    channels: [Channel; 2],
    fm_ring: Box<[[f32; 2]]>,
    fm_write: usize,
}

impl Ring {
    #[must_use]
    pub fn new(s: RingMod, rate: u32) -> Self {
        let fm_ring = if matches!(s.source, Source::Modulated { .. }) {
            let cap = ((0.002 * rate.max(1) as f32).ceil() as usize)
                .next_power_of_two()
                .max(16);
            vec![[0.0; 2]; cap].into_boxed_slice()
        } else {
            Box::default()
        };
        let mut r = Self {
            s,
            c: Coefs::default(),
            phase: 0.0,
            mod_phase: 0.0,
            lfo_phase: 0.0,
            mod_live: [0.0; 4],
            mod_init: false,
            envelope: 0.0,
            side: 0.0,
            demod: 0.0,
            agc: 0.0,
            channels: [Channel::default(); 2],
            fm_ring,
            fm_write: 0,
        };
        r.set(s, rate);
        r
    }

    /// New settings; the carrier and filter state carry on.
    pub fn set(&mut self, s: RingMod, rate: u32) {
        let rate = rate.max(1) as f32;
        let x = f64::from(PI * s.highpass_hz.clamp(0.0, 0.45 * rate) / rate);
        // The native one-pole prewarp: a tan series as `P/(P + 1)`.
        let p = x * x * x * (1.0 / 3.0) + x + x * x * x * x * x * (2.0 / 15.0);
        let k = s.drive.max(1e-3);
        let fb = s.feedback.clamp(0.0, 0.99);
        let mut c = Coefs {
            inc: s.hz / rate,
            hg: (p / (p + 1.0)) as f32,
            k,
            norm: (k * k * 0.5 + 1.0).sqrt() * (0.5 / (k * k)),
            attack: follow(0.005, rate),
            release: follow(0.1, rate),
            smooth: pole(15_000.0, rate),
            dc: pole(20.0, rate),
            makeup: 1.0,
            ..Coefs::default()
        };
        match s.source {
            Source::Oscillator | Source::Sidechain { .. } => {
                // Feedback takes the shifted branch's level down by
                // `√(1 − fb²)`; the makeup follows the branch's share.
                let q = s.quadrature.clamp(0.0, 1.0);
                c.makeup = q * (1.0 - fb * fb).sqrt() + (1.0 - q);
            }
            Source::Radio { .. } | Source::Modulated { .. } => {}
        }
        if let Source::Modulated {
            edge,
            fm_ms,
            lfo_hz,
            ..
        } = s.source
        {
            let e = edge.clamp(0.0, 1.0);
            let e2 = e * e;
            c.mod_inc = (2.0 * s.hz.max(0.0) / rate).min(1.98);
            c.lfo_inc = (2.0 * lfo_hz.max(0.0) / rate).min(1.98);
            c.edge_sin = 2.0 * (1.0 - e2);
            c.edge_tri = e2;
            let max_delay = self.fm_ring.len().saturating_sub(2) as f32;
            c.fm_frames = (fm_ms.max(0.0) * 0.001 * rate).min(max_delay.max(0.0));
            c.mod_smooth = 1.0 - (-1.0 / (0.005 * rate)).exp();
            if !self.mod_init {
                self.mod_live = [s.dry, s.wet, c.fm_frames, c.mod_inc];
                self.mod_init = true;
            }
        }
        if let Source::Sidechain {
            attack,
            release,
            bandpass_hz,
            ..
        } = s.source
        {
            // 3.5 octaves: `1/Q = (o² − 1)/o`, `o = 2^1.75`.
            let o = 2f32.powf(1.75);
            c.side_band = svf(bandpass_hz.unwrap_or(1_000.0), (o * o - 1.0) / o, rate);
            (c.side_attack, c.side_release) = (follow(attack, rate), follow(release, rate));
        }
        if let Source::Radio {
            tuned_hz, damping, ..
        } = s.source
        {
            c.noise = pole(5_000.0, rate);
            c.hum = pole(50.0, rate);
            c.band = svf(tuned_hz, damping, rate);
            // A four-pole Butterworth: dampings 2·cos(3π/8) and 2·cos(π/8).
            c.low = [0.765_366_9, 1.847_759].map(|k| svf(0.5 * tuned_hz, k, rate));
            c.demod = tuned_hz / rate;
            (c.agc_attack, c.agc_release) = (follow(0.002, rate), follow(0.2, rate));
        }
        self.s = s;
        self.c = c;
    }

    pub fn reset(&mut self) {
        (
            self.phase,
            self.mod_phase,
            self.lfo_phase,
            self.envelope,
            self.side,
            self.demod,
            self.agc,
        ) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        self.channels = [Channel::default(); 2];
        self.fm_ring.fill([0.0; 2]);
        self.fm_write = 0;
        self.mod_live = [self.s.dry, self.s.wet, self.c.fm_frames, self.c.mod_inc];
    }

    /// The feedback taken off the input for a wet output `w`.
    fn returned(ch: &Channel, c: &Coefs, w: f32) -> f32 {
        let low = ch.smooth.peek(w, c.smooth);
        (1.0 - c.dc) * (low - ch.dc.0)
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let (s, c) = (self.s, self.c);
        if let Source::Modulated {
            lfo_depth,
            lfo_square,
            ..
        } = s.source
        {
            let depth = lfo_depth.max(0.0);
            let mask = self.fm_ring.len().saturating_sub(1);
            let max_idx = mask.saturating_sub(1) as f32;
            let targets = [s.dry, s.wet, c.fm_frames, c.mod_inc];
            let alpha = c.mod_smooth;
            for frame in io {
                for (cur, tgt) in self.mod_live.iter_mut().zip(targets) {
                    *cur += alpha * (tgt - *cur);
                }
                let [dry_g, wet_g, fm_frames, base_inc] = self.mod_live;
                self.lfo_phase += c.lfo_inc;
                if self.lfo_phase >= 1.0 {
                    self.lfo_phase -= 2.0;
                }
                let lfo_abs = self.lfo_phase.abs();
                let lfo_val = if lfo_square {
                    if self.lfo_phase >= 0.0 { 0.5 } else { 0.0 }
                } else if lfo_abs >= 0.5 {
                    let u = lfo_abs - 1.0;
                    0.5 - u * u
                } else {
                    lfo_abs * lfo_abs
                };
                let inst_inc = (base_inc * (1.0 + lfo_val * depth)).min(1.98);
                self.mod_phase += inst_inc;
                if self.mod_phase >= 1.0 {
                    self.mod_phase -= 2.0;
                }
                let abs_p = self.mod_phase.abs();
                let parabolic = if abs_p >= 0.5 {
                    let u = abs_p - 1.0;
                    0.5 - u * u
                } else {
                    abs_p * abs_p
                };
                let carrier_val = c.edge_sin * parabolic + c.edge_tri * abs_p;
                let gain = dry_g + wet_g * carrier_val;
                let mixed = [frame[0] * gain, frame[1] * gain];
                if !self.fm_ring.is_empty() {
                    self.fm_write = self.fm_write.wrapping_sub(1) & mask;
                    if let Some(slot) = self.fm_ring.get_mut(self.fm_write) {
                        *slot = mixed;
                    }
                    let delay = (fm_frames * carrier_val).clamp(0.0, max_idx);
                    let i0 = delay as usize;
                    let frac = delay - i0 as f32;
                    let a = self
                        .fm_ring
                        .get((self.fm_write + i0) & mask)
                        .copied()
                        .unwrap_or([0.0; 2]);
                    let b = self
                        .fm_ring
                        .get((self.fm_write + i0 + 1) & mask)
                        .copied()
                        .unwrap_or([0.0; 2]);
                    *frame = [a[0] + frac * (b[0] - a[0]), a[1] + frac * (b[1] - a[1])];
                } else {
                    *frame = mixed;
                }
            }
            return;
        }
        let fb = s.feedback.clamp(0.0, 0.99);
        let (weight, offset, scale) = match s.source {
            Source::Oscillator => (s.quadrature, s.bias, 1.0 - s.bias * 0.5),
            Source::Sidechain { .. } => (-s.quadrature, 0.0, 1.0 - s.bias * 0.5),
            Source::Radio { .. } | Source::Modulated { .. } => (s.quadrature, 0.0, 1.0),
        };
        for frame in io {
            let input = *frame;
            let level = input[0].abs().max(input[1].abs());
            track(&mut self.envelope, level, c.attack, c.release);
            let open = if s.gate && self.envelope < 1e-5 {
                self.envelope * 1e5
            } else {
                1.0
            };
            // Each channel's carrier pair, before the bias.
            let lead = s.spread * 0.5;
            let mut carriers = [lead, -lead].map(|o| {
                let u = self.phase + o;
                [carrier(u + 0.5), carrier(u + 0.25)]
            });
            let mut bias = offset;
            if let Source::Sidechain {
                contour,
                bandpass_hz,
                ..
            } = s.source
            {
                let mut pairs = [[0.0f32; 2]; 2];
                for ((pair, ch), mut x) in pairs.iter_mut().zip(&mut self.channels).zip(input) {
                    if bandpass_hz.is_some() {
                        x = ch.side_band.tick(x, &c.side_band).band_norm();
                    }
                    *pair = ch.side.tick(x, 0.0);
                }
                let [[re_l, im_l], [re_r, im_r]] = pairs;
                let (re, im) = (re_l + re_r, im_l + im_r);
                let amp = ((re * re + im * im) * 0.25).sqrt();
                track(&mut self.side, amp, c.side_attack, c.side_release);
                let e = self.side;
                carriers = pairs.map(|p| p.map(|v| v + (e - v) * contour));
                bias = e * s.bias;
            }
            let mut out = [0.0f32; 2];
            for ((ch, cs), (o, x)) in self
                .channels
                .iter_mut()
                .zip(carriers)
                .zip(out.iter_mut().zip(input))
            {
                let pre = match s.source {
                    Source::Radio { gain, product, .. } => {
                        let carried = ((self.envelope - 1.0) * product + 1.0) * s.bias;
                        (gain * x + carried) * (1.0 - s.bias * 0.5)
                    }
                    _ => x,
                };
                let m = cs.map(|v| (bias + v) * scale);
                let mut y = pre;
                if fb > 0.0 {
                    y = pre - fb * Self::returned(ch, &c, ch.wet);
                    for _ in 0..4 {
                        let a = { ch.input }.tick(y, c.hg);
                        let w =
                            (diodes(c.k, m[0], a[0]) + weight * diodes(c.k, m[1], a[1])) * c.norm;
                        let next = pre - fb * Self::returned(ch, &c, w);
                        let settled = (next - y).abs() < 1e-4;
                        y = next;
                        if settled {
                            break;
                        }
                    }
                }
                let a = ch.input.tick(y, c.hg);
                let mut wet = 0.0;
                for (((a, m), [sum, difference]), weight) in
                    a.into_iter().zip(m).zip(&mut ch.diodes).zip([1.0, weight])
                {
                    wet += (sum.tick(c.k * (m + a)) - difference.tick(c.k * (m - a)))
                        * c.norm
                        * weight;
                }
                ch.wet = wet;
                ch.dc.tick(ch.smooth.tick(wet, c.smooth), c.dc);
                *o = wet;
            }
            if let Source::Radio {
                gain,
                makeup,
                product,
                noise,
                ..
            } = s.source
            {
                let lfo = carrier(self.demod);
                self.demod += c.demod;
                self.demod -= self.demod.floor();
                let scale = makeup * 2.0 / gain.max(1e-3).sqrt();
                for (o, ch) in out.iter_mut().zip(&mut self.channels) {
                    *o = ch.radio.receive(*o, &c, noise, lfo, product) * scale;
                }
                track(
                    &mut self.agc,
                    out[0].abs().max(out[1].abs()),
                    c.agc_attack,
                    c.agc_release,
                );
                let level = std::f32::consts::FRAC_1_SQRT_2 / (self.agc + 0.4);
                out = out.map(|o| o * level);
            }
            for ((x, dry), wet) in frame.iter_mut().zip(input).zip(out) {
                *x = s.dry * dry + s.wet * open * c.makeup * wet;
            }
            self.phase += c.inc;
            self.phase -= self.phase.floor();
        }
    }
}

impl Receiver {
    /// The transmitted `x` received: noise added, band-passed, rectified
    /// its hum and highs taken off, then crossfaded by `product` to the band
    /// times `lfo` (the native core skips the stages there too).
    fn receive(&mut self, x: f32, c: &Coefs, noise: f32, lfo: f32, product: f32) -> f32 {
        // MINSTD (48271, 2³¹ − 1), uniform in −1..1.
        let seed = if self.seed == 0 { 12_345 } else { self.seed };
        self.seed = ((u64::from(seed) * 48_271) % 2_147_483_647) as u32;
        let white = 2.0 * (self.seed as f32 - 1.0) * 4.656_613e-10 - 1.0;
        let x = x + noise * self.noise.tick(white, c.noise);
        let band = self.band.tick(x, &c.band).band_norm();
        let rectified = self.rectifier.tick(band);
        let mut y = rectified - self.dc.tick(rectified, c.hum);
        for (state, svf) in self.low.iter_mut().zip(&c.low) {
            y = state.tick(y, svf).low;
        }
        y + (lfo * band - y) * product
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(bias: f32, quadrature: f32) -> RingMod {
        RingMod {
            hz: 500.0,
            spread: 0.0,
            bias,
            quadrature,
            drive: 0.5,
            highpass_hz: 8.0,
            gate: false,
            feedback: 0.0,
            source: Source::Oscillator,
            wet: 1.0,
            dry: 0.0,
        }
    }

    /// Level of `tone` Hz in the settled output of a 2 kHz sine.
    fn level(s: RingMod, tone: f32) -> f32 {
        let mut ring = Ring::new(s, 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [0.1 * (std::f32::consts::TAU * 2_000.0 * n as f32 / 48_000.0).sin(); 2])
            .collect();
        ring.process(&mut io);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, f) in io.iter().enumerate().skip(24_000) {
            let w = f64::from(std::f32::consts::TAU * tone) * n as f64 / 48_000.0;
            re += f64::from(f[0]) * w.cos();
            im += f64::from(f[0]) * w.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / 24_000.0) as f32
    }

    #[test]
    fn a_ring_makes_both_sidebands_and_a_shift_one() {
        let ring = settings(0.0, 0.0);
        let (up, down) = (level(ring, 2_500.0), level(ring, 1_500.0));
        assert!(
            (up / 0.05 - 1.0).abs() < 0.1 && (down / 0.05 - 1.0).abs() < 0.1,
            "{up} {down}"
        );
        let shift = settings(0.0, 1.0);
        let (up, down) = (level(shift, 2_500.0), level(shift, 1_500.0));
        assert!((up / 0.1 - 1.0).abs() < 0.1 && down < 0.003, "{up} {down}");
        let am = settings(1.0, 0.0);
        let carrier_less = level(am, 2_000.0);
        assert!((carrier_less / 0.05 - 1.0).abs() < 0.1, "{carrier_less}");
    }

    /// The level at `tone` Hz of `s` run over a 0.1 sine at `hz`.
    fn level_of(s: RingMod, hz: f32, tone: f32) -> f32 {
        let mut ring = Ring::new(s, 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| [0.1 * (std::f32::consts::TAU * hz * n as f32 / 48_000.0).sin(); 2])
            .collect();
        ring.process(&mut io);
        assert!(io.iter().all(|f| f[0].is_finite()));
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, f) in io.iter().enumerate().skip(24_000) {
            let w = f64::from(std::f32::consts::TAU * tone) * n as f64 / 48_000.0;
            re += f64::from(f[0]) * w.cos();
            im += f64::from(f[0]) * w.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / 24_000.0) as f32
    }

    #[test]
    fn a_sidechain_shift_doubles_its_input_and_a_radio_plays_it_back() {
        let side = RingMod {
            highpass_hz: 0.0,
            source: Source::Sidechain {
                contour: 0.0,
                attack: 0.005,
                release: 0.01,
                bandpass_hz: None,
            },
            ..settings(0.0, 1.0)
        };
        let (octave, unison) = (level_of(side, 500.0, 1_000.0), level_of(side, 500.0, 500.0));
        assert!(octave > 10.0 * unison, "{octave} {unison}");
        // AM on a 10 kHz carrier, tuned there, envelope-demodulated.
        let radio = RingMod {
            hz: 10_000.0,
            highpass_hz: 0.0,
            source: Source::Radio {
                gain: 1.0,
                tuned_hz: 10_000.0,
                damping: 0.7,
                makeup: 1.0,
                product: 0.0,
                noise: 0.001,
            },
            feedback: 0.5,
            ..settings(1.0, 0.0)
        };
        let (played, other) = (level_of(radio, 500.0, 500.0), level_of(radio, 500.0, 730.0));
        assert!(played > 0.02 && played > 10.0 * other, "{played} {other}");
    }

    #[test]
    fn modulated_ring_makes_sidebands_fm_and_lfo_and_is_block_invariant() {
        let s = RingMod {
            hz: 500.0,
            spread: 0.0,
            bias: 0.0,
            quadrature: 0.0,
            drive: 1.0,
            highpass_hz: 0.0,
            gate: false,
            feedback: 0.0,
            source: Source::Modulated {
                edge: 0.0,
                fm_ms: 0.0,
                lfo_hz: 1.0,
                lfo_depth: 0.0,
                lfo_square: false,
            },
            wet: 3.2,
            dry: -1.6,
        };
        let (up, down, carrier_leak) = (
            level_of(s, 2_000.0, 2_500.0),
            level_of(s, 2_000.0, 1_500.0),
            level_of(s, 2_000.0, 2_000.0),
        );
        // Pure ring modulation (ring=1.0): strong ±500 Hz sidebands (~0.08), zero carrier leak.
        assert!(up > 0.06 && down > 0.06, "sidebands {up} {down}");
        assert!(carrier_leak < 0.005, "carrier leak {carrier_leak}");

        // FM + LFO + triangle edge: block-size invariant and finite after reset.
        let s_fm = RingMod {
            source: Source::Modulated {
                edge: 0.8,
                fm_ms: 1.0,
                lfo_hz: 5.0,
                lfo_depth: 1.5,
                lfo_square: true,
            },
            ..s
        };
        let input: Vec<[f32; 2]> = (0..2048)
            .map(|n| [(std::f32::consts::TAU * 440.0 * n as f32 / 48_000.0).sin() * 0.4; 2])
            .collect();
        let mut r = Ring::new(s_fm, 48_000);
        let mut whole = input.clone();
        r.process(&mut whole);
        assert!(whole.iter().flatten().all(|v| v.is_finite()));
        r.reset();
        let mut chunked = input;
        for c in chunked.chunks_mut(19) {
            r.process(c);
        }
        assert_eq!(whole, chunked, "block-size invariance after reset");
    }
}
