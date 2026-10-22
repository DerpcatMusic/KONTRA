//! Kontakt Stereo Tune core (BEffectStereoTune). Native: prepare `0x140a9cae0` / `0x140ac0de0`,
//! apply `0x140abbce0`, driver `0x140aa7320` (countdown + smoother `0x140acb6c0`, see `ring_ramp`),
//! core `0x140aaf640`. The shared post stage `0x140ac21f0` (host mix) is not part of this module.
//!
//! One stereo pair. The input is written through a delay ring `line` (N frames) by two read taps
//! `tap[0]` and `tap[1]` that move at `1/ratio` and `ratio` frames per frame. Each tap is a 4-point
//! cubic read; the second read of each tap is the same cubic at `cross` frames further on, and the
//! two are crossfaded by the Hann window `window` indexed by the distance to the write head. The
//! two results go into output rings (`out`, R frames) that are read back with a fixed lag, mixed
//! with a one-pole and an all-pass pair, and crossed between the channels by `spread`.
//!
//! Lanes of `ramp` (native `+0x1e0`): 0 = weight of the stored lane (1.0), 1 = one-pole coefficient,
//! 2 = cross spread, 3 = wet mix. Verified against Kontakt 8 kemu vectors
//! (`stereo_tune_native_vectors.json`, `tools/re/stereotune_kemu_vectors.py`) at 1e-5 of peak.
//! The `+0x1c0` rate scale is the ctor constant 2*pi (`0x40c90fdb`, `FUN_140a86e50`); nothing later
//! writes it, so lowering passes `TAU`. Channel counts above two are not modelled (the native pair loop
//! runs once for stereo).
use super::ring_ramp::Ramp4;

/// Native denormal guard (0x144f385a8) added to each channel's one-pole input.
const GUARD: f32 = 9.999_999_960_041_972e-13;
/// Frames per control step (native driver countdown reload `0x20`).
const CHUNK: u32 = 32;

/// Cubic 4-point read at fraction `x` between `y0` and `y1` (native expression, same operation order).
fn cubic(ym1: f32, y0: f32, y1: f32, y2: f32, x: f32) -> f32 {
    let a = ((ym1 - y0 * 2.5) + y1 + y1) - y2 * 0.5;
    let b = (y0 - y1) * 1.5 + (y2 - ym1) * 0.5;
    ((a + b * x) * x + (y1 - ym1) * 0.5) * x + y0
}

/// Ring read of a cubic around `i` (points `i-1` .. `i+2`, wrapping at `n`).
fn read4(ring: &[f32], i: usize) -> (f32, f32, f32, f32) {
    let n = ring.len();
    (
        ring[(i + n - 1) % n],
        ring[i],
        ring[(i + 1) % n],
        ring[(i + 2) % n],
    )
}

pub(crate) struct StereoTune {
    ramp: Ramp4,
    rate: f32,
    /// Window over the main ring, `N` entries (native `+0x248`).
    window: Vec<f32>,
    /// Main delay ring, `N` frames (native `+0x2e8`).
    line: Vec<f32>,
    /// Output rings for the two channels, `R` frames (native `+0x250`, `+0x258`).
    out: [Vec<f32>; 2],
    /// Cross-tap offset in frames (native `+0x240`).
    cross: usize,
    /// Write head of the main ring, decremented per frame (native `+0x2d0`).
    head: usize,
    /// Read positions of the two taps (native `+0x2d4`, `+0x2dc`) and their steps: `1/ratio`, `ratio`
    /// (native `+0x2d8`, `+0x2e0`).
    tap: [f32; 2],
    step: [f32; 2],
    /// One-pole and all-pass states: `[x1, x1', y1, y1', x2, x2', y2, y2']` at `+0x304`..`+0x320`.
    st: [f32; 8],
    /// Read positions into the output rings, `[ring0 lag, ring1 lag, ring0 lag2, ring1 lag2]`
    /// (native `+0x2f0`, `+0x2f4`, `+0x2f8`, `+0x2fc`), and the output write position (`+0x300`).
    rd: [usize; 4],
    wr: usize,
    /// Countdown to the next control step (native `+0x1bc`).
    countdown: u32,
}

impl StereoTune {
    /// Native prepare for a sample rate: ring sizes, window, reload and ramp step (`0x140ac0de0`).
    pub(crate) fn new(rate: f32) -> Self {
        let n = (rate * 0.011_609_977_1_f32) as usize;
        let r = (rate * 0.039_999_999_1_f32) as usize;
        let mut window = Vec::with_capacity(n);
        let step = std::f64::consts::TAU / n as f64;
        let mut x = 0.0_f64;
        for _ in 0..n {
            window.push((0.5 - x.cos() * 0.5) as f32);
            x += step;
        }
        Self {
            ramp: Ramp4::default(),
            rate,
            window,
            line: vec![0.0; n],
            out: [vec![0.0; r], vec![0.0; r]],
            cross: (n as f32 * 0.5) as usize,
            head: 0,
            tap: [0.0; 2],
            step: [1.0; 2],
            st: [0.0; 8],
            rd: [0; 4],
            wr: 0,
            countdown: 0,
        }
    }

    /// Prepared state for stored knobs `[split, drift, spread, mix]` at `rate` (ctor, prepare, apply).
    pub(crate) fn with_knobs(rate: u32, knobs: [f32; 4]) -> Self {
        let mut tune = Self::new(rate as f32);
        let [a, b, c, mix] = knobs;
        tune.apply(a, b, c, mix, std::f32::consts::TAU);
        tune
    }

    /// Clears the audio history and keeps the knobs and ramp (the owner's reset).
    pub(crate) fn clear(&mut self) {
        self.line.fill(0.);
        for ring in &mut self.out {
            ring.fill(0.);
        }
        self.head = 0;
        self.tap = [0.; 2];
        self.st = [0.; 8];
        self.rd = [0; 4];
        self.wr = 0;
        self.countdown = 0;
    }

    /// Native apply (`0x140abbce0`): `a` is the split (one-pole) knob, `b` the drift (tap pitch) knob,
    /// `c` the spread knob and `mix` the wet amount, stored at record `+0x8`, `+0xc`, `+0x10`, `+0x14`.
    /// `scale` is the `+0x1c0` rate scale (`TAU`, see the module note).
    pub(crate) fn apply(&mut self, a: f32, b: f32, c: f32, mix: f32, scale: f32) {
        let coeff = ((a * 900.0 + 100.0) * scale) / self.rate;
        let f7 = b * 0.85;
        let f5 = f7 * 0.4 + 0.06;
        // The native powf rounds a double pow (the UCRT form); a single-precision pow moves the taps by an ulp.
        let ratio = 1.059_463_14_f64.powf(f64::from(f5 * f5 * f5 * 4.0)) as f32;
        self.step = [1.0 / ratio, ratio];
        let f8 = c * 0.99;
        let spread = f8 + f8 + 0.02;
        // Native prepare fixes the ramp rate at 1/(reload * 32) and the reload at `reload` chunks.
        let reload = ((self.rate * 0.000_625_000_04_f32 + 0.5) as i32).max(1);
        let lanes = [1.0, coeff, spread, mix];
        self.ramp
            .set_target(lanes, 1.0 / ((reload as u32) << 5) as f32, reload as f32);
        self.ramp.snap(lanes);
    }

    /// Native driver `0x140aa7320` without its post stage: the countdown, the control step at each
    /// chunk end and the per-frame core. Writes the two output channels.
    pub(crate) fn process(
        &mut self,
        left: &[f32],
        right: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let total = left.len();
        let mut pos = 0;
        let mut remaining = total;
        while remaining > 0 {
            let c = self.countdown;
            let k = (c as usize).min(remaining);
            self.countdown = c - k as u32;
            if (c as usize) - k < 1 {
                self.ramp.control();
                self.countdown = CHUNK;
            }
            for t in pos..pos + k {
                (out_l[t], out_r[t]) = self.frame(left[t], right[t]);
            }
            pos += k;
            remaining -= k;
        }
    }

    /// One frame of the native core `0x140aaf640`.
    fn frame(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.ramp.advance();
        let [a0, a1, a2, a3] = self.ramp.lanes();
        let n = self.line.len();
        let rl = self.out[0].len();

        // First stage: one-pole and all-pass per channel, fed into the main ring.
        let st = &mut self.st;
        let v1 = (l + GUARD) - (st[0] + st[2]);
        let x1 = v1 * a1 + st[0];
        let y1 = x1 * a1 + st[2];
        let v2 = (r + GUARD) - (st[4] + st[6]);
        let x2 = v2 * a1 + st[4];
        let y2 = x2 * a1 + st[6];
        self.head = if self.head == 0 { n - 1 } else { self.head - 1 };
        self.line[self.head] = (st[5] + st[1] + v2 + v1) * a3;
        st[1] = x1;
        st[5] = x2;
        st[0] = x1;
        st[2] = y1;
        st[4] = x2;
        st[6] = y2;

        // Taps: cubic read at each tap, crossed with the cross-offset read, windowed by distance.
        self.tap[0] -= self.step[0];
        if self.tap[0] < 0.0 {
            self.tap[0] += n as f32;
        }
        let ta = self.tap[0];
        let ia = (ta.max(0.0) as usize).min(n - 1);
        let xa = ta - ia as f32;
        let i1 = (ia + 1) % n;
        let (p0, p1, p2, p3) = read4(&self.line, ia);
        let tap_a = cubic(p0, p1, p2, p3, xa);
        let ic = (self.cross + i1) % n;
        let (q0, q1, q2, q3) = read4(&self.line, ic);
        let cross_a = cubic(q0, q1, q2, q3, xa);
        let wa = (i1 + n - self.head) % n;
        let o0 = (tap_a - cross_a) * self.window[wa] + cross_a;

        self.tap[1] -= self.step[1];
        if self.tap[1] < 0.0 {
            self.tap[1] += n as f32;
        }
        let tb = self.tap[1];
        let ib = (tb.max(0.0) as usize).min(n - 1);
        let xb = tb - ib as f32;
        let j1 = (ib + 1) % n;
        let (p0, p1, p2, p3) = read4(&self.line, ib);
        let tap_b = cubic(p0, p1, p2, p3, xb);
        let jc = (self.cross + j1) % n;
        let (q0, q1, q2, q3) = read4(&self.line, jc);
        let cross_b = cubic(q0, q1, q2, q3, xb);
        let wb = (j1 + n - self.head) % n;
        let o1 = (tap_b - cross_b) * self.window[wb] + cross_b;

        // Output rings: the lagged reads come before this frame's writes.
        let [i0, i1r, i2, i3] = self.rd;
        let ring0_a = self.out[0][i0];
        let ring0_b = self.out[0][i2];
        let ring1_a = self.out[1][i1r];
        let ring1_b = self.out[1][i3];
        self.rd = [(i0 + 1) % rl, (i1r + 1) % rl, (i2 + 1) % rl, (i3 + 1) % rl];
        self.out[0][self.wr] = o0;
        self.out[1][self.wr] = o1;
        self.wr = (self.wr + 1) % rl;

        let s310 = self.st[3];
        let s320 = self.st[7];
        let fl = ((((ring0_b - ring0_a) * a0 + ring0_a + s310) * 0.5 - l) * a3 * 0.5) + l;
        self.st[3] = self.st[2];
        let w2 = 1.0 - a2;
        self.st[7] = self.st[6];
        let s2 = a2 + 1.0;
        let fr = ((((ring1_b - ring1_a) * a0 + ring1_a + s320) * 0.5 - r) * a3 * 0.5) + r;

        let out_l = (s2 * fl + w2 * fr) * 0.5 * a3 + (1.0 - a3) * l;
        let out_r = (s2 * fr + w2 * fl) * 0.5 * a3 + (1.0 - a3) * r;
        (out_l, out_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        knobs: [f32; 4],
        rate: f32,
        stim: [Vec<f32>; 2],
        #[serde(rename = "outL")]
        out_l: Vec<f32>,
        #[serde(rename = "outR")]
        out_r: Vec<f32>,
    }

    /// Kontakt 8 kemu: ctor, prepare, apply, then the driver countdown with the core per chunk.
    #[test]
    fn matches_native_kemu_vectors() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("stereo_tune_native_vectors.json")).unwrap();
        for case in cases {
            let mut st = StereoTune::new(case.rate);
            let [a, b, c, mix] = case.knobs;
            st.apply(a, b, c, mix, std::f32::consts::TAU);
            let n = case.stim[0].len();
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            st.process(&case.stim[0], &case.stim[1], &mut l, &mut r);
            for (name, got, want) in [("left", &l, &case.out_l), ("right", &r, &case.out_r)] {
                let peak = want.iter().fold(0f32, |m, v| m.max(v.abs()));
                let worst = got
                    .iter()
                    .zip(want)
                    .fold(0f32, |m, (g, w)| m.max((g - w).abs()));
                assert!(
                    worst <= 1e-5 * peak,
                    "{} {name}: worst {worst} vs peak {peak}",
                    case.name
                );
            }
        }
    }
}
