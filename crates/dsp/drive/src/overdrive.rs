//! An overdrive pedal: one-pole splits around a clipping curve run at four
//! times the rate (cubic interpolation up, a 16-tap FIR down), its dry path
//! and the shaped one mixed back through a tone low-pass, with a feedback
//! of the shaped band into the next frame's input split. Ported from v2
//! sampler-core dsp/color/skreamer.rs, itself checked against the native
//! core under emulation; the format translators carry the knob laws and
//! the curve. It is its own class because the native core (FUN_140aaf220)
//! feeds the shaped band back into the input split, which `Shaper` cannot.

use std::f32::consts::TAU;

/// Four lanes: one per stage of the split, or one per upsampled phase.
type Lanes = [f32; 4];

/// Cubic interpolation rows, one per upsampled phase.
const UP: [Lanes; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.070_312_5, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.0625, 0.5625, 0.5625, -0.0625],
    [-0.023_437_5, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];

/// The decimating FIR, by delay (now, 1, 2, 3) and phase, in 512ths.
const DOWN: [Lanes; 4] = [
    [-3.0 / 512.0, -8.0 / 512.0, -9.0 / 512.0, 0.0],
    [29.0 / 512.0, 72.0 / 512.0, 111.0 / 512.0, 128.0 / 512.0],
    [111.0 / 512.0, 72.0 / 512.0, 29.0 / 512.0, 0.0],
    [-9.0 / 512.0, -8.0 / 512.0, -3.0 / 512.0, 0.0],
];

/// Frames per gain step: gains move in 32-frame blocks.
const BLOCK: u32 = 32;

/// How long the gains take to rise after `new` or `reset`: long enough not
/// to click.
const RAMP_SECONDS: f32 = 0.01;

/// The pedal's settings in physical terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voicing {
    /// The input split's three one-pole corners in Hz (bass, then two
    /// fixed high-passes).
    pub split_hz: [f32; 3],
    /// The output smoothers' corners in Hz: an envelope, the feedback band
    /// (bright) and the tone low-pass.
    pub smooth_hz: [f32; 3],
    /// Linear gain into the curve.
    pub drive: f32,
    /// Linear gain of the dry band.
    pub mix: f32,
    /// Linear output gain.
    pub level: f32,
    /// The clipping curve at 256 points over inputs −128..=127.
    pub curve: &'static [f32; 256],
}

/// A stereo overdrive and its state.
#[derive(Clone, Debug, PartialEq)]
pub struct Overdrive {
    split: Lanes,
    smooth: Lanes,
    target: Lanes,
    curve: &'static [f32; 256],
    gain: Lanes,
    step: Lanes,
    /// Gain blocks left on the ramp, frames left in this block, and the
    /// ramp's length in blocks.
    blocks: u32,
    frames: u32,
    ramp: u32,
    /// Per channel: input, split, dry delay, curve input history, smoothers,
    /// three FIR delays.
    ch: [[Lanes; 8]; 2],
}

fn coefficient(hz: f32, rate: u32) -> f32 {
    (TAU * hz / rate.max(1) as f32).clamp(0.0, 1.0)
}

impl Overdrive {
    /// At `v`, its gains rising from zero over about 10 ms.
    #[must_use]
    pub fn new(v: Voicing, rate: u32) -> Self {
        let ramp = ((rate as f32 * RAMP_SECONDS / BLOCK as f32 + 0.5) as u32).max(1);
        let mut o = Self {
            split: [0.0; 4],
            smooth: [0.0; 4],
            target: [0.0; 4],
            curve: v.curve,
            gain: [0.0; 4],
            step: [0.0; 4],
            blocks: 0,
            frames: 0,
            ramp,
            ch: [[[0.0; 4]; 8]; 2],
        };
        o.set(v, rate);
        o
    }

    /// Moves to `v`; the gains glide there over the same ramp, the state
    /// carries on.
    pub fn set(&mut self, v: Voicing, rate: u32) {
        let [b, h1, h2] = v.split_hz.map(|hz| coefficient(hz, rate));
        let [e, bright, tone] = v.smooth_hz.map(|hz| coefficient(hz, rate));
        (self.split, self.smooth, self.curve) = ([b, h1, h2, 0.0], [e, bright, tone, 0.0], v.curve);
        let target = [v.drive, v.mix, v.level, 0.0].map(|g| if g.is_finite() { g } else { 0.0 });
        if target != self.target {
            self.target = target;
            let frames = (self.ramp * BLOCK) as f32;
            for ((s, t), g) in self.step.iter_mut().zip(target).zip(self.gain) {
                *s = (t - g) / frames;
            }
            (self.blocks, self.frames) = (self.ramp, 0);
        }
    }

    /// Drives `io` (frames of `[left, right]`) in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io {
            if self.frames == 0 {
                if self.blocks == 0 {
                    (self.gain, self.step) = (self.target, [0.0; 4]);
                } else {
                    self.blocks -= 1;
                }
                self.frames = BLOCK;
                // The state feeds back and there is no FTZ on the audio
                // thread: a decaying tail must not go subnormal.
                for v in self.ch.as_flattened_mut().as_flattened_mut() {
                    if v.abs() < f32::MIN_POSITIVE {
                        *v = 0.0;
                    }
                }
            }
            self.frames -= 1;
            for (g, s) in self.gain.iter_mut().zip(self.step) {
                *g += s;
            }
            let (gain, curve, split, smooth) = (self.gain, self.curve, self.split, self.smooth);
            for (x, lanes) in frame.iter_mut().zip(&mut self.ch) {
                *x = sample(lanes, *x, gain, curve, split, smooth);
            }
        }
    }

    /// Silence, the gains ramping in again.
    pub fn reset(&mut self) {
        self.ch = [[[0.0; 4]; 8]; 2];
        (self.gain, self.blocks, self.frames) = ([0.0; 4], self.ramp, 0);
        let frames = (self.ramp * BLOCK) as f32;
        self.step = self.target.map(|t| t / frames);
    }
}

fn map2(a: Lanes, b: Lanes, f: impl Fn(f32, f32) -> f32) -> Lanes {
    let mut out = [0.0; 4];
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        *o = f(x, y);
    }
    out
}

fn lookup(curve: &[f32; 256], m: f32) -> f32 {
    let m = if m.is_nan() {
        128.0
    } else {
        m.clamp(0.0, 255.0)
    };
    let i = m as usize;
    let a = curve.get(i).copied().unwrap_or(0.0);
    let b = curve.get(i + 1).copied().unwrap_or(a);
    a + (b - a) * (m - i as f32)
}

/// One frame of one channel.
fn sample(
    l: &mut [Lanes; 8],
    x: f32,
    g: Lanes,
    curve: &[f32; 256],
    split: Lanes,
    smooth: Lanes,
) -> f32 {
    let [p, s, q, a, e, h0, h1, h2] = l;
    p[1] = x;
    let d = map2(*p, *s, |p, s| p - s);
    *s = map2(map2(d, split, |d, k| d * k), *s, |a, s| a + s);
    let v = map2(d, g, |d, g| d * g);
    let qn = [q[2], q[0], q[3], v[1]];
    *q = qn;
    a[0] = v[0];
    let y: Lanes = UP.map(|r| {
        let m = ((a[1] * r[1]) + (a[3] * r[3])) + ((a[0] * r[0]) + (a[2] * r[2])) + 128.0;
        lookup(curve, m)
    });
    let [c0, c1, c2, c3] = DOWN;
    let acc = map2(
        map2(
            map2(y, c0, |y, c| y * c),
            map2(*h0, c1, |h, c| h * c),
            |a, b| a + b,
        ),
        map2(*h1, c2, |h, c| h * c),
        |a, b| a + b,
    );
    let acc = map2(acc, map2(c3, *h2, |c, h| c * h), |a, b| a + b);
    (*h2, *h1, *h0) = (*h1, *h0, y);
    let f = (acc[1] + acc[3]) + (acc[0] + acc[2]);
    *a = [a[0], a[0], a[1], a[2]];
    let target = [v[0] * v[0], f, v[2], v[2]];
    *e = map2(
        map2(map2(target, *e, |t, e| t - e), smooth, |d, k| d * k),
        *e,
        |a, e| a + e,
    );
    let fed = qn[1] + e[1];
    *p = [d[1], d[1], fed, fed];
    e[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_never_goes_subnormal() {
        static CURVE: std::sync::LazyLock<[f32; 256]> =
            std::sync::LazyLock::new(|| std::array::from_fn(|i| (i as f32 - 128.0) / 128.0));
        let v = Voicing {
            split_hz: [20.0; 3],
            smooth_hz: [20.0; 3],
            drive: 0.5,
            mix: 0.5,
            level: 0.5,
            curve: &CURVE,
        };
        let mut o = Overdrive::new(v, 48_000);
        let mut burst = [[0.5f32; 2]; 4800];
        o.process(&mut burst);
        let mut silence = [[0.0f32; 2]; 48_000];
        for _ in 0..2 {
            o.process(&mut silence);
        }
        let flat = o.ch.as_flattened().as_flattened();
        assert!(flat.iter().all(|v| *v == 0.0 || v.is_normal()));
    }

    /// A 0.5 sine at 200 Hz through the pedal at 48 kHz, frames 960..976,
    /// against the native core under emulation (v2 sampler-core
    /// dsp/color/skreamer.rs, whose curve file this reads), at all knobs
    /// 0.5 and at tone 0.2, drive 1, bass 0.8, bright 0.3, mix 0.9.
    #[test]
    fn a_sine_matches_the_native_core() {
        let bytes = include_bytes!("../../../sampler-core/src/dsp/color/tables/skreamer.f32le");
        let mut curve = [0.0; 256];
        for (c, b) in curve.iter_mut().zip(bytes.as_chunks::<8>().0) {
            *c = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        let curve: &'static [f32; 256] = Box::leak(Box::new(curve));
        let table = |i: f32| (i / 60.0 - 20.0).exp2();
        let hz = |c: f32| c * f32::from_bits(0x40c9_0e56) / TAU;
        let cases: [([f32; 5], [f32; 16]); 2] = [
            (
                [0.5; 5],
                [
                    0.232_342_91,
                    0.239_326_22,
                    0.246_271_86,
                    0.253_179_55,
                    0.260_048_75,
                    0.266_879_14,
                    0.273_670_44,
                    0.280_421_47,
                    0.287_130_54,
                    0.293_795_68,
                    0.300_414_83,
                    0.306_985_7,
                    0.313_505_6,
                    0.319_971_62,
                    0.326_380_8,
                    0.332_729_97,
                ],
            ),
            (
                [0.2, 1.0, 0.8, 0.3, 0.9],
                [
                    -0.399_847_54,
                    -0.355_867_6,
                    -0.312_060_54,
                    -0.268_421_05,
                    -0.224_945_72,
                    -0.181_632_9,
                    -0.138_482_66,
                    -0.095_496_58,
                    -0.052_677_79,
                    -0.010_030_765,
                    0.032_438_684,
                    0.074_723_55,
                    0.116_815_67,
                    0.158_705_86,
                    0.200_383_9,
                    0.241_838_7,
                ],
            ),
        ];
        for ([tone, drive, bass, bright, mix], native) in cases {
            let v = Voicing {
                split_hz: [
                    hz(table(570.0 - 400.0 * bass + 1_381.881_6)),
                    129.429_81 / TAU,
                    28.901_8 / TAU,
                ],
                smooth_hz: [
                    50.264 / TAU,
                    hz(table(200.0 * bright + 650.0 - 200.0 * drive + 1_381.881_6)),
                    hz(table(tone * tone * 225.0 + 1_716.881_6)),
                ],
                drive: table(199.315_67 * drive + 1_550.795_7) * 4.0,
                mix: table(398.631_35 * mix + 1_070.444_8) * 0.5,
                level: 0.447,
                curve,
            };
            let mut o = Overdrive::new(v, 48_000);
            let mut io: Vec<[f32; 2]> = (0..976)
                .map(|t| {
                    let x = (0.5 * (std::f64::consts::TAU * 200.0 * f64::from(t) / 48_000.0).sin())
                        as f32;
                    [x, x]
                })
                .collect();
            o.process(&mut io);
            for (got, want) in io[960..].iter().zip(native) {
                assert!(
                    (got[0] - want).abs() < 1e-5 && got[0] == got[1],
                    "{got:?} vs {want}"
                );
            }
        }
    }
}
