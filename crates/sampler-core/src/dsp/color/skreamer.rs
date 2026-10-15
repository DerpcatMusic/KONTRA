//! Kontakt Skreamer (`BEffectSkreamer`, module 0x21): the color chain's table shaper with its
//! own 4x cubic upsampling and 16-tap decimation.
//!
//! Audio 0x140aa5810 runs core 0x140aaf220 in four SSE lanes per channel; setter 0x140968150
//! (TONE, DRIVE, BASS, BRIGHT, MIX, identity-bound) and coefficient builder 0x140a938a0 give
//! the coefficients below. The curve is the 256-entry (value, slope) table at 0x144f3a100,
//! saved as `tables/skreamer.f32le`. Gains ramp from zero over `round(rate * 0.0003125)`
//! 32-frame blocks after reset (≈ 10 ms). Checked bit-exact against the native core under
//! kemu (model kcolor/skreamer.py).
use super::super::Planar;
use super::super::ControlRamp;
use super::super::control::PreparedParameter;
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
    target: Option<Lanes>,
    /// Native wrapper 0x140aa5860 preserves its 32-frame residual across calls.
    quantum: u8,
    /// Live coefficients belong to this history, never another voice or slot.
    cached: Option<([f32; 5], Skreamer)>,
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.ch.iter().flatten().flatten().chain(&self.gain).chain(&self.step)
            .chain(self.target.iter().flatten()).all(|v| v.is_finite())
            && self.cached.as_ref().is_none_or(|(knobs, unit)| {
                knobs.iter().chain(&unit.k).chain(&unit.e).chain(&unit.target).all(|v| v.is_finite())
            })
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
        // Drive/Mix setters mark the target dirty (0x1409682b5 / 0x140968442).
        // The wrapper retargets from the current gain at the next control tick,
        // retaining all channel/filter histories (0x140aa58e6..0x140aa59f4).
        if s.target != Some(self.target) {
            s.target = Some(self.target);
            for i in 0..4 {
                let d = (self.target[i] - s.gain[i]) * self.inverse;
                if f64::from(d * d) < 1e-15 {
                    s.step[i] = 0.0;
                    s.gain[i] = self.target[i];
                } else {
                    s.step[i] = d;
                }
            }
            s.remaining = if s.step.iter().any(|&d| d != 0.0) { self.blocks } else { 0 };
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

    /// One span up to the next native control tick; splitting a host block does
    /// not advance the gain ramp early or discard its residual frame count.
    fn span(&self, s: &mut State, block: &mut Planar, start: usize, len: usize) -> usize {
        if s.quantum == 0 {
            self.tick(s);
            s.quantum = 32;
        }
        let end = (start + usize::from(s.quantum)).min(len);
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
        s.quantum -= u8::try_from(end - start).expect("span is at most 32 frames");
        end
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        let mut start = 0;
        while start < len {
            start = self.span(s, block, start, len);
        }
    }
}

/// Script writes use the same coefficient laws as saved fields. The native
/// wrapper samples modulation on Tone/Drive at 32-frame ticks; held controls
/// also take effect at a new processing call without resetting channel state.
pub(super) fn process_live(
    knobs: [PreparedParameter; 5], rate: u32, state: &mut State,
    parameters: &[ControlRamp], block: &mut Planar, len: usize, at: u64,
) {
    let mut start = 0;
    while start < len {
        let mut values = knobs.map(|p| p.value(parameters, at + start as u64, None) as f32);
        // A projected voice-modulation value is held until the next native
        // tick, even when the host splits the intervening audio into calls.
        // Ordinary script writes still update the coefficients at call entry.
        if state.quantum != 0 && let Some((saved, _)) = state.cached {
            for i in 0..5 {
                if knobs[i].projected(parameters) {
                    values[i] = saved[i];
                }
            }
        }
        if state.cached.is_none_or(|(saved, _)| saved != values) {
            state.cached = Some((values, Skreamer::new(values, rate)));
        }
        let unit = state.cached.expect("live knobs prepared above").1;
        start = unit.span(state, block, start, len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skreamer_projected_knob_waits_for_the_next_native_tick() {
        let knobs = std::array::from_fn(PreparedParameter::Control);
        let mut controls = [ControlRamp::test_ramp(0.5, 0.5, 0, 0); 5];
        let mut state = State::default();
        let mut block = [[0.2; 64]; 2];
        process_live(knobs, 48_000, &mut state, &controls, &mut block, 9, 0);
        controls[0].add_modulation(0.25, crate::ControlRange {
            control: crate::ControlId(1), low: 0., high: 1., ramp_frames: 0,
        });
        process_live(knobs, 48_000, &mut state, &controls, &mut block, 23, 9);
        assert_eq!(state.cached.unwrap().0[0], 0.5);
        process_live(knobs, 48_000, &mut state, &controls, &mut block, 1, 32);
        assert_eq!(state.cached.unwrap().0[0], 0.75);
    }

    #[test]
    fn skreamer_retarget_keeps_history_and_the_native_residual_clock() {
        let initial = Skreamer::new([0.5; 5], 48_000);
        let changed = Skreamer::new([0.2, 0.9, 0.8, 0.3, 0.1], 48_000);
        let mut state = State::default();
        let mut block = [[0.2; 64]; 2];
        for _ in 0..32 {
            initial.process(&mut state, &mut block, 64);
        }
        initial.process(&mut state, &mut block, 9);
        assert_eq!(state.quantum, 23);
        let current = state.gain;
        assert_eq!(current, initial.target);
        let history = state.ch;
        changed.process(&mut state, &mut block, 0);
        assert_eq!(state.ch, history, "parameter preparation must not reset the filter history");
        changed.process(&mut state, &mut block, 23);
        assert_eq!(state.gain, current, "the next native tick has not occurred");
        assert_eq!(state.quantum, 0);
        changed.process(&mut state, &mut block, 1);
        let step: Lanes = std::array::from_fn(|i| (changed.target[i] - current[i]) * changed.inverse);
        assert_eq!(state.step, step);
        assert_eq!(state.gain, std::array::from_fn(|i| current[i] + step[i]));
        assert_eq!(state.remaining, changed.blocks);
        assert!(state.finite());
    }

    #[test]
    fn skreamer_static_and_held_updates_ignore_host_block_partition() {
        let render = |partition: usize| {
            let mut state = State::default();
            let mut output = Vec::new();
            let mut start = 0;
            for (end, knobs) in [(777, [0.5; 5]), (934, [0.2, 0.9, 0.8, 0.3, 0.1]), (2048, [0.7; 5])] {
                let unit = Skreamer::new(knobs, 48_000);
                while start < end {
                    let len = partition.min(end - start);
                    let mut block = [[0.; 64]; 2];
                    for channel in &mut block {
                        for (i, v) in channel[..len].iter_mut().enumerate() {
                            *v = f64::from((0.5 * (std::f64::consts::TAU * 200. * (start + i) as f64 / 48_000.).sin()) as f32);
                        }
                    }
                    unit.process(&mut state, &mut block, len);
                    output.extend_from_slice(&block[0][..len]);
                    start += len;
                }
            }
            output
        };
        let expected = render(64);
        for partition in [1, 3, 7, 31, 32, 63] {
            assert_eq!(render(partition), expected, "partition {partition}");
        }
    }

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
