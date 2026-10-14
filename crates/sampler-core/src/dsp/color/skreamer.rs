//! Kontakt Skreamer (`BEffectSkreamer`, module 0x21): the color chain's table shaper with its
//! own 4x cubic upsampling and 16-tap decimation.
//!
//! Audio 0x140aa5810 runs core 0x140aaf220 in four SSE lanes per channel; setter 0x140968150
//! (TONE, DRIVE, BASS, BRIGHT, MIX, identity-bound) and coefficient builder 0x140a938a0 give
//! the coefficients below. The curve is the 256-entry (value, slope) table at 0x144f3a100,
//! saved as `tables/skreamer.f32le`. Gains ramp from zero over `round(rate * 0.0003125)`
//! 32-frame blocks after reset (≈ 10 ms). Checked bit-exact against the native core under
//! kemu (model kcolor/skreamer.py).
// ponytail: knobs are constants (no script/modulation lanes); add when a library drives them.
use super::super::Planar;
use super::super::filter::ladder_kernel::exponential as table_db;

const TABLE_BYTES: &[u8; 2048] = include_bytes!("tables/skreamer.f32le");

fn curve(i: usize) -> (f32, f32) {
    let at = |k: usize| f32::from_le_bytes(TABLE_BYTES[k * 4..k * 4 + 4].try_into().unwrap());
    (at(2 * i), at(2 * i + 1))
}

/// Cubic (Catmull-Rom) interpolation rows, one per lane.
const R: [[f32; 4]; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.070_312_5, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.0625, 0.5625, 0.5625, -0.0625],
    [-0.023_437_5, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];
/// Decimating FIR, by delay (current, 1, 2, 3) and lane.
const C: [[f32; 4]; 4] = [
    [-0.005_859_375, -0.015_625, -0.017_578_125, 0.0],
    [0.056_640_625, 0.140_625, 0.216_796_875, 0.25],
    [0.216_796_875, 0.140_625, 0.056_640_625, 0.0],
    [-0.017_578_125, -0.015_625, -0.005_859_375, 0.0],
];

type Lanes = [f32; 4];

/// Prepared coefficients.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Skreamer {
    k: Lanes,
    e: Lanes,
    target: Lanes,
    blocks: u32,
    inverse: f32,
}

/// Per-channel lanes and the shared gain ramp.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    ch: [[Lanes; 8]; 2],
    gain: Lanes,
    step: Lanes,
    remaining: u32,
    started: bool,
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.ch.iter().flatten().flatten().chain(&self.gain).all(|v| v.is_finite())
    }
}

impl Skreamer {
    /// `[tone, drive, bass, bright, mix]`, normalized.
    pub(super) fn new([tone, drive, bass, bright, mix]: [f32; 5], rate: u32) -> Self {
        super::super::filter::ladder_kernel::prepare();
        let sr = rate as f32;
        let inv = 1.0 / sr;
        let w = inv * 6.283;
        let k = [
            (table_db((570.0 - bass * 400.0) + 1381.881_6) * w).min(1.0),
            (inv * 129.429_81).min(1.0),
            (inv * 28.901_8).min(1.0),
            0.0,
        ];
        let e = [
            (inv * 50.264).min(1.0),
            (table_db((((bright * 200.0) + 650.0) - drive * 200.0) + 1381.881_6) * w).min(1.0),
            (table_db((tone * tone) * 225.0 + 1716.881_6) * w).min(1.0),
            0.0,
        ];
        let target = [
            table_db(drive * 199.315_67 + 1550.795_7) * 4.0,
            table_db(mix * 398.631_35 + 1070.444_8) * 0.5,
            0.447,
            0.0,
        ];
        let blocks = ((sr * 0.000_312_5 + 0.5) as u32).max(1);
        Self { k, e, target, blocks, inverse: 1.0 / (blocks << 5) as f32 }
    }

    fn tick(&self, s: &mut State) {
        if !s.started {
            s.started = true;
            for i in 0..4 {
                let d = (self.target[i] - s.gain[i]) * self.inverse;
                if f64::from(d * d) < 1e-15 {
                    s.step[i] = 0.0;
                    s.gain[i] = self.target[i];
                } else {
                    s.step[i] = d;
                }
            }
            if s.step.iter().any(|&d| d != 0.0) {
                s.remaining = self.blocks;
            }
        } else if s.remaining != 0 {
            s.remaining -= 1;
            if s.remaining == 0 {
                s.gain = self.target;
                s.step = [0.0; 4];
            }
        }
    }

    /// One frame. `l = [p, s, q, a, e, h0, h1, h2]`.
    #[inline(always)]
    fn sample(&self, l: &mut [Lanes; 8], x: f32, g: Lanes) -> f32 {
        let [p, s, q, a, e, h0, h1, h2] = l;
        p[1] = x;
        let d: Lanes = std::array::from_fn(|i| p[i] - s[i]);
        *s = std::array::from_fn(|i| d[i] * self.k[i] + s[i]);
        let v: Lanes = std::array::from_fn(|i| d[i] * g[i]);
        let qn = [q[2], q[0], q[3], v[1]];
        *q = qn;
        a[0] = v[0];
        let y: Lanes = std::array::from_fn(|k| {
            let r = R[k];
            let m = ((a[1] * r[1]) + (a[3] * r[3])) + ((a[0] * r[0]) + (a[2] * r[2])) + 128.0;
            let i = m.clamp(0.0, 255.0) as usize;
            let (value, slope) = curve(i);
            (m - i as f32) * slope + value
        });
        let acc: Lanes =
            std::array::from_fn(|k| ((y[k] * C[0][k] + h0[k] * C[1][k]) + h1[k] * C[2][k]) + C[3][k] * h2[k]);
        (*h2, *h1, *h0) = (*h1, *h0, y);
        let f = (acc[1] + acc[3]) + (acc[0] + acc[2]);
        *a = [a[0], a[0], a[1], a[2]];
        let target = [v[0] * v[0], f, v[2], v[2]];
        *e = std::array::from_fn(|i| (target[i] - e[i]) * self.e[i] + e[i]);
        let qp1 = qn[1] + e[1];
        *p = [d[1], d[1], qp1, qp1];
        e[2]
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        let mut start = 0;
        while start < len {
            let end = (start + 32).min(len);
            self.tick(s);
            let mut last = s.gain;
            for (ch, lanes) in s.ch.iter_mut().enumerate() {
                let mut g = s.gain;
                for v in &mut block[ch][start..end] {
                    g = std::array::from_fn(|i| g[i] + s.step[i]);
                    *v = f64::from(self.sample(lanes, *v as f32, g));
                }
                last = g;
            }
            s.gain = last;
            start = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model of the native core (kcolor/skreamer.py, bit-exact to the emulated 0x140aa5810
    /// with error 0): a 0.5 sine at 200 Hz, 48 kHz, frames 960..976, at default knobs and at
    /// tone .2 drive 1 bass .8 bright .3 mix .9.
    #[test]
    fn sine_matches_the_native_core() {
        let cases: [([f32; 5], [f32; 16]); 2] = [
            ([0.5; 5], [0.232342914, 0.239326224, 0.246271864, 0.25317955, 0.260048747, 0.266879141, 0.273670435, 0.280421466, 0.287130535, 0.293795675, 0.30041483, 0.306985706, 0.31350559, 0.319971621, 0.326380789, 0.332729965]),
            ([0.2, 1.0, 0.8, 0.3, 0.9], [-0.399847537, -0.355867594, -0.312060535, -0.268421054, -0.224945724, -0.181632906, -0.13848266, -0.09549658, -0.0526777878, -0.0100307651, 0.0324386843, 0.0747235492, 0.116815671, 0.15870586, 0.200383902, 0.241838694]),
        ];
        const BLOCK: usize = super::super::super::BLOCK;
        for (knobs, native) in cases {
            let unit = Skreamer::new(knobs, 48_000);
            let mut state = State::default();
            let mut out = Vec::new();
            for start in (0..1024).step_by(BLOCK) {
                let mut block: Planar = [[0.0; BLOCK]; 2];
                for i in 0..BLOCK {
                    let t = (start + i) as f64;
                    let x = (0.5 * (std::f64::consts::TAU * 200.0 * t / 48_000.0).sin()) as f32;
                    block[0][i] = f64::from(x);
                    block[1][i] = f64::from(x);
                }
                unit.process(&mut state, &mut block, BLOCK);
                out.extend(block[0].iter().map(|&v| v as f32));
            }
            for (i, n) in native.iter().enumerate() {
                assert!((out[960 + i] - n).abs() < 1e-6, "{knobs:?} {i}: {} vs {n}", out[960 + i]);
            }
        }
    }
}
