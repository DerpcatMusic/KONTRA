//! Kontakt machine source stages (S1200 Machine). Recovered from the Kontakt 8
//! decompile: the render `FUN_140cdf460` (vtable slot 27 of `BSP1200`, vtable
//! `0x144fd0600`) and the object constructor `FUN_140cd9220`. Constants are the
//! binary32 bit patterns read there, not tuned values.
//!
//! Chain per channel: the 7-tap / 6-pole anti-alias IIR, the filter-mode
//! biquads (modes 2..=5; mode 1 is a bypass), the mode gain, then the 26 kHz
//! sample-and-hold and output quantiser. Not recovered: filter modes 0, 6 and 7.
//! MP60 is not implemented.

use super::machine_tables::{S1200_MODE_BITS, S1200_NUMERATOR_BITS};

/// Feed-forward taps `b0..b6`, stored at object `+0x1f74..+0x1f8c`.
pub const S1200_AA_B: [f32; 7] = [
    f32::from_bits(0x3c94_0179), // b0 (+0x1f74)
    f32::from_bits(0x3d3f_06c5), // b1 (+0x1f78)
    f32::from_bits(0x3daa_6313), // b2 (+0x1f7c)
    f32::from_bits(0x3dc7_0042), // b3 (+0x1f80)
    f32::from_bits(0x3daa_6313), // b4 (+0x1f84)
    f32::from_bits(0x3d3f_06c5), // b5 (+0x1f88)
    f32::from_bits(0x3c94_0179), // b6 (+0x1f8c)
];

/// Feedback coefficients `c1..c6` at `+0x1f90..+0x1fa4`; the recurrence is
/// `y[n] = sum b_k x[n-k] - sum c_k y[n-k]`.
pub const S1200_AA_C: [f32; 6] = [
    f32::from_bits(0xc00c_ae5a), // c1 (+0x1f90)
    f32::from_bits(0x4051_bd8c), // c2 (+0x1f94)
    f32::from_bits(0xc03c_f080), // c3 (+0x1f98)
    f32::from_bits(0x3fea_87c2), // c4 (+0x1f9c)
    f32::from_bits(0xbf31_7924), // c5 (+0x1fa0)
    f32::from_bits(0x3e06_cb2b), // c6 (+0x1fa4)
];

/// Quantiser step of the S1200 output: `DAT_144fd0dc4` = 2^9.5 (binary32
/// `0x443504f3`), and its reciprocal `DAT_144fd0d50` (`0x3ab504f3`).
pub const S1200_QUANT_SCALE: f32 = f32::from_bits(0x4435_04f3);
pub const S1200_QUANT_INVERSE: f32 = f32::from_bits(0x3ab5_04f3);

/// Round to the nearest integer, ties to even: the render converts with
/// `cvtsd2si` under the default rounding mode (`ROUND` in the decompile).
pub fn s1200_round(x: f64) -> f64 {
    x.round_ties_even()
}

/// Output quantiser: `ROUND(x * 2^9.5) / 2^9.5`, the product taken in binary32.
pub fn s1200_quantize(x: f32) -> f32 {
    round_step(x, S1200_QUANT_SCALE, S1200_QUANT_INVERSE)
}

/// `ROUND(x * scale) * inverse` in binary32, ties to even (`cvtsd2si`). MP60's
/// stage-to-stage quantiser is the same form with scale 2^11.
fn round_step(x: f32, scale: f32, inverse: f32) -> f32 {
    s1200_round(f64::from(x * scale)) as f32 * inverse
}

/// The 7-tap / 6-pole anti-alias IIR of the S1200 render. History is stored
/// newest-first: `x[0] = x[n-1]`, `y[0] = y[n-1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct S1200AntiAlias {
    x: [f32; 6],
    y: [f32; 6],
}

impl S1200AntiAlias {
    pub fn process(&mut self, input: f32) -> f32 {
        let mut out = S1200_AA_B[0] * input;
        for k in 0..6 {
            out += S1200_AA_B[k + 1] * self.x[k];
            out -= S1200_AA_C[k] * self.y[k];
        }
        self.x.rotate_right(1);
        self.x[0] = input;
        self.y.rotate_right(1);
        self.y[0] = out;
        out
    }
}

/// One S1200 filter mode: the 5 denominator coefficients `A0..A4` and the
/// stored output gain (object `+0x1fb8`). Modes 2..=5 are tabled; mode 1 is a
/// bypass (the render skips the stage); other modes are not recovered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct S1200Mode {
    pub a: [f32; 5],
    pub gain: f32,
}

/// The tabled filter mode, or `None` for a bypass (mode 1) and for the modes
/// not recovered (0, 6, 7). Callers tell those apart by the stored id.
pub fn s1200_mode(filter_mode: u8) -> Option<S1200Mode> {
    S1200_MODE_BITS.iter().find(|(mode, _, _)| *mode == filter_mode).map(|&(_, a, gain)| S1200Mode {
        a: a.map(f32::from_bits),
        gain: f32::from_bits(gain),
    })
}

/// The S1200 filter as three cascaded lowpass sections (decompile: `+0x440`
/// state, numerator `T2` at `DAT_144fd0310`, denominators `T1` per mode):
/// a biquad `(1 + B0 z^-1 + B3 z^-2) / (1 - A0 z^-1 - A3 z^-2)`, a biquad
/// `(1 + B1 z^-1 + B4 z^-2) / (1 - A1 z^-1 - A4 z^-2)` on its output, and a
/// first-order `(1 + B2 z^-1) / (1 - A2 z^-1)`, then the mode gain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct S1200ModeFilter {
    x: [f32; 2],
    v1: [f32; 2],
    v2: [f32; 2],
    v3: f32,
}

impl S1200ModeFilter {
    pub fn process(&mut self, mode: &S1200Mode, input: f32) -> f32 {
        let b = S1200_NUMERATOR_BITS.map(f32::from_bits);
        let a = mode.a;
        let v1 = input + b[0] * self.x[0] + b[3] * self.x[1] + a[0] * self.v1[0] + a[3] * self.v1[1];
        let v2 = v1 + b[1] * self.v1[0] + b[4] * self.v1[1] + a[1] * self.v2[0] + a[4] * self.v2[1];
        let v3 = v2 + b[2] * self.v2[0] + a[2] * self.v3;
        self.x = [input, self.x[0]];
        self.v1 = [v1, self.v1[0]];
        self.v2 = [v2, self.v2[0]];
        self.v3 = v3;
        v3 * mode.gain
    }
}

/// S1200 output rate in Hz (object `+0x78`, written by the render as 26000.0).
pub const S1200_OUTPUT_RATE: f64 = 26000.0;

/// The 26 kHz capture schedule of the output hold: frame `n` is a capture when
/// `n >= next`, and each capture advances `next` by `host_rate / 26000`. The
/// render's copy path is always taken (`+0x74` is zeroed before every block).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct S1200Clock {
    interval: f64,
    next: f64,
}

impl S1200Clock {
    pub fn new(host_rate: u32) -> Self {
        Self { interval: f64::from(host_rate) / S1200_OUTPUT_RATE, next: 0.0 }
    }

    /// Whether frame `index` (counted from the voice start) captures.
    pub fn capture(&mut self, index: u64) -> bool {
        if index as f64 >= self.next {
            self.next += self.interval;
            true
        } else {
            false
        }
    }
}

/// Delay-storage words per stage: one `[f64; 2]` slot per word, holding the
/// left and right channel. Layout: anti-alias `x[0..6]`, `y[0..6]`, mode
/// `x[0..2]`, `v1[0..2]`, `v2[0..2]`, `v3`, then the held output sample.
pub const S1200_WORDS: usize = 20;

/// One S1200 channel's filter history, loaded from and stored to its slots.
#[derive(Clone, Copy, Debug, Default)]
struct S1200Channel {
    aa: S1200AntiAlias,
    mode: S1200ModeFilter,
    held: f32,
}

impl S1200Channel {
    fn load(slots: &[[f64; 2]], c: usize) -> Self {
        let w = |k: usize| slots[k][c] as f32;
        let mut aa = S1200AntiAlias::default();
        for k in 0..6 {
            aa.x[k] = w(k);
            aa.y[k] = w(6 + k);
        }
        Self {
            aa,
            mode: S1200ModeFilter { x: [w(12), w(13)], v1: [w(14), w(15)], v2: [w(16), w(17)], v3: w(18) },
            held: w(19),
        }
    }

    fn store(&self, slots: &mut [[f64; 2]], c: usize) {
        let mut put = |k: usize, v: f32| slots[k][c] = f64::from(v);
        for k in 0..6 {
            put(k, self.aa.x[k]);
            put(6 + k, self.aa.y[k]);
        }
        let mode = [
            self.mode.x[0], self.mode.x[1], self.mode.v1[0], self.mode.v1[1],
            self.mode.v2[0], self.mode.v2[1], self.mode.v3, self.held,
        ];
        for (k, v) in mode.into_iter().enumerate() {
            put(12 + k, v);
        }
    }
}

/// One S1200 machine source stage: anti-alias IIR, filter mode, 26 kHz hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct S1200Stage {
    /// `None` is the mode-1 bypass of the filter-mode biquads.
    pub mode: Option<S1200Mode>,
    /// Host frames per 26 kHz capture, used by [`S1200Clock`].
    interval: f64,
}

impl S1200Stage {
    pub fn new(mode: Option<S1200Mode>, host_rate: u32) -> Self {
        Self { mode, interval: S1200Clock::new(host_rate).interval }
    }

    pub fn valid(&self) -> bool {
        self.interval.is_finite() && self.interval > 0.0
    }

    /// Run `len` frames of both channels through the chain. `slots` holds the
    /// history words ([`S1200_WORDS`] long); `schedule` is `[frames run, next
    /// capture]`, zeroed for a fresh voice. Both channels share one schedule.
    pub fn process(&self, slots: &mut [[f64; 2]], schedule: &mut [f64; 2], block: [&mut [f64]; 2], len: usize) {
        let start = schedule[0] as u64;
        if start == 0 {
            slots.fill([0.; 2]);
        }
        let mut next = schedule[1];
        for (c, channel) in block.into_iter().enumerate() {
            let mut state = S1200Channel::load(slots, c);
            let mut clock = S1200Clock { interval: self.interval, next: schedule[1] };
            for (i, v) in channel[..len].iter_mut().enumerate() {
                let mut y = state.aa.process(*v as f32);
                if let Some(mode) = &self.mode {
                    y = state.mode.process(mode, y);
                }
                if clock.capture(start + i as u64) {
                    state.held = s1200_quantize(y);
                }
                *v = f64::from(state.held);
            }
            state.store(slots, c);
            if c == 0 {
                next = clock.next;
            }
        }
        schedule[0] = (start + len as u64) as f64;
        schedule[1] = next;
    }
}

/// One MP60 section: `y = x + ff[0] x[n-1] + ff[1] x[n-2] + fb[0] y[n-1] + fb[1] y[n-2]`.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // MP60 chain is unwired until its resampler's rate law is decoded.
pub struct Mp60Section {
    pub ff: [f32; 2],
    pub fb: [f32; 2],
}

/// One MP60 IIR stage: sections in render order, a trailing `tail * y[n-1]`
/// tap on the last section's output, then the stage gain.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // MP60 chain is unwired until its resampler's rate law is decoded.
pub struct Mp60Iir {
    pub sections: &'static [Mp60Section],
    pub tail: f32,
    pub gain: f32,
}

/// Stage 1 (object `+0xa60..+0xaf0`, render `FUN_140cddf50`): five sections.
#[allow(dead_code)] // MP60 chain is unwired until its resampler's rate law is decoded.
pub const MP60_IIR1: Mp60Iir = Mp60Iir {
    sections: &[
        Mp60Section { ff: [f32::from_bits(0xbe90_159c), f32::from_bits(0x3f4f_a03a)], fb: [f32::from_bits(0x3f3f_d3c9), f32::from_bits(0xbf30_08e5)] },
        Mp60Section { ff: [f32::from_bits(0x3db7_6fdf), f32::from_bits(0x3f0b_9ee0)], fb: [f32::from_bits(0x3f79_9a8f), f32::from_bits(0xbeac_f572)] },
        Mp60Section { ff: [f32::from_bits(0x4017_a773), 0.0], fb: [f32::from_bits(0x3f03_c688), f32::from_bits(0xbf6e_4121)] },
        Mp60Section { ff: [f32::from_bits(0x3feb_21b8), 0.0], fb: [f32::from_bits(0xbf1b_12e3), 0.0] },
        Mp60Section { ff: [f32::from_bits(0xbf55_1047), 0.0], fb: [f32::from_bits(0x3d34_fe36), 0.0] },
    ],
    tail: f32::from_bits(0x3f2c_b783),
    gain: f32::from_bits(0xbc6a_5c95),
};

/// Stage 2 (object `+0xb24..+0xbc4`): six sections.
#[allow(dead_code)] // MP60 chain is unwired until its resampler's rate law is decoded.
pub const MP60_IIR2: Mp60Iir = Mp60Iir {
    sections: &[
        Mp60Section { ff: [f32::from_bits(0x3fd2_4e50), f32::from_bits(0x3f49_7b8e)], fb: [f32::from_bits(0xbfd8_41bf), f32::from_bits(0xbf67_99cc)] },
        Mp60Section { ff: [f32::from_bits(0xbd64_413c), f32::from_bits(0x3ad2_0d5a)], fb: [f32::from_bits(0xbf31_0a86), f32::from_bits(0xbeac_80e6)] },
        Mp60Section { ff: [f32::from_bits(0x3df0_e93f), 0.0], fb: [f32::from_bits(0x3ece_02b3), f32::from_bits(0xbe5f_c54e)] },
        Mp60Section { ff: [f32::from_bits(0x4003_b432), 0.0], fb: [f32::from_bits(0x3d36_dd82), f32::from_bits(0xbab0_80e0)] },
        Mp60Section { ff: [f32::from_bits(0xbf10_2849), 0.0], fb: [f32::from_bits(0x3f21_a07b), 0.0] },
        Mp60Section { ff: [f32::from_bits(0x3f14_f53e), 0.0], fb: [f32::from_bits(0x3f14_d65c), 0.0] },
    ],
    tail: f32::from_bits(0x3e0f_acb8),
    gain: f32::from_bits(0x3f5d_1f83),
};

/// MP60 quantiser between the two IIR stages: `ROUND(x * 2^11) / 2^11`
/// (`DAT_144fd0dcc` = 2048.0, `DAT_144fd0d4c` = 2^-11).
pub const MP60_QUANT_SCALE: f32 = 2048.0;
pub const MP60_QUANT_INVERSE: f32 = 1.0 / 2048.0;

/// Per-channel history words of one stage: `[input, output]` per section,
/// each `[n-1, n-2]`. Six sections at most.
pub const MP60_SECTIONS: usize = 6;
pub const MP60_STAGE_WORDS: usize = MP60_SECTIONS * 4;

/// Runs one sample through an MP60 IIR stage, updating its histories.
#[allow(dead_code)] // MP60 chain is unwired until its resampler's rate law is decoded.
fn mp60_iir(stage: &Mp60Iir, history: &mut [[[f32; 2]; 2]; MP60_SECTIONS], x: f32) -> f32 {
    let mut input = x;
    for (section, h) in stage.sections.iter().zip(history.iter_mut()) {
        let y = input + section.ff[0] * h[0][0] + section.ff[1] * h[0][1] + section.fb[0] * h[1][0] + section.fb[1] * h[1][1];
        h[0] = [input, h[0][0]];
        h[1] = [y, h[1][0]];
        input = y;
    }
    // The shift above leaves y[n-1] of the last section in its second output word.
    let last = &history[stage.sections.len() - 1];
    (input + stage.tail * last[1][1]) * stage.gain
}

/// The MP60 IIR chain: stage 1, the 12-bit quantiser, stage 2. Resampling
/// around the chain is not implemented (its rate law is not decoded).
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)] // MP60 chain is unwired until its resampler's rate law is decoded.
pub struct Mp60Chain {
    stage1: [[[f32; 2]; 2]; MP60_SECTIONS],
    stage2: [[[f32; 2]; 2]; MP60_SECTIONS],
}

impl Mp60Chain {
    pub fn process(&mut self, x: f32) -> f32 {
        let y1 = mp60_iir(&MP60_IIR1, &mut self.stage1, x);
        let q = round_step(y1, MP60_QUANT_SCALE, MP60_QUANT_INVERSE);
        mp60_iir(&MP60_IIR2, &mut self.stage2, q)
    }

    /// Load / store the histories of one channel from delay-storage slots.
    pub fn load(slots: &[[f64; 2]], c: usize) -> Self {
        let mut chain = Self::default();
        for k in 0..MP60_STAGE_WORDS * 2 {
            let word = slots[k][c] as f32;
            let (stage, rest) = if k < MP60_STAGE_WORDS { (&mut chain.stage1, k) } else { (&mut chain.stage2, k - MP60_STAGE_WORDS) };
            stage[rest / 4][rest % 4 / 2][rest % 2] = word;
        }
        chain
    }

    pub fn store(&self, slots: &mut [[f64; 2]], c: usize) {
        for (k, slot) in slots.iter_mut().enumerate().take(MP60_STAGE_WORDS * 2) {
            let (stage, rest) = if k < MP60_STAGE_WORDS { (&self.stage1, k) } else { (&self.stage2, k - MP60_STAGE_WORDS) };
            slot[c] = f64::from(stage[rest / 4][rest % 4 / 2][rest % 2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(v: &[f32]) -> f64 {
        v.iter().map(|&c| f64::from(c)).sum()
    }

    #[test]
    fn anti_alias_impulse_and_dc_match_the_recovered_taps() {
        let mut f = S1200AntiAlias::default();
        let y0 = f.process(1.0);
        let y1 = f.process(0.0);
        // y0 = b0; y1 = b1 - c1 * y0 (golden from the constants above).
        assert!((f64::from(y0) - f64::from(S1200_AA_B[0])).abs() < 1e-7);
        assert!((f64::from(y1) - (f64::from(S1200_AA_B[1]) - f64::from(S1200_AA_C[0]) * f64::from(y0))).abs() < 1e-6);
        // DC gain = sum(b) / (1 + sum(c)); the filter is near unity at DC.
        let mut f = S1200AntiAlias::default();
        let mut dc = 0.0;
        for _ in 0..4000 {
            dc = f64::from(f.process(1.0));
        }
        let expected = sum(&S1200_AA_B) / (1.0 + sum(&S1200_AA_C));
        assert!((dc - expected).abs() < 1e-4, "dc {dc} expected {expected}");
        assert!((expected - 0.9886).abs() < 1e-3);
    }

    #[test]
    fn quantiser_rounds_ties_to_even_like_cvtsd2si() {
        assert_eq!(s1200_round(0.5), 0.0);
        assert_eq!(s1200_round(1.5), 2.0);
        assert_eq!(s1200_round(2.5), 2.0);
        assert_eq!(s1200_round(-0.5), -0.0);
        // Inputs at k / 2^9.5 land on the grid point k (ties at k + 0.5 go to even).
        let step = S1200_QUANT_INVERSE;
        assert_eq!(s1200_quantize(1.0 / S1200_QUANT_SCALE), step);
        assert_eq!(s1200_quantize(0.5 / S1200_QUANT_SCALE), 0.0);
        assert_eq!(s1200_quantize(1.5 / S1200_QUANT_SCALE), 2.0 * step);
    }

    #[test]
    fn mode_filter_impulse_matches_the_golden_response_and_has_unity_dc_gain() {
        let mode = s1200_mode(2).unwrap();
        let mut f = S1200ModeFilter::default();
        let golden = [0.0005686027f32, 0.0048903979, 0.0201305618, 0.0535505752, 0.104831836, 0.162420813, 0.207708171, 0.223593811];
        for (n, want) in golden.iter().enumerate() {
            let got = f.process(&mode, if n == 0 { 1.0 } else { 0.0 });
            assert!((got - want).abs() < 1e-6 * want.abs().max(1.0), "n={n} got {got} want {want}");
        }
        for m in 2..=5u8 {
            let mode = s1200_mode(m).unwrap();
            let mut f = S1200ModeFilter::default();
            let mut dc = 0.0;
            for _ in 0..6000 {
                dc = f.process(&mode, 1.0);
            }
            assert!((dc - 1.0).abs() < 1e-4, "mode {m} dc {dc}");
        }
        assert!(s1200_mode(1).is_none() && s1200_mode(6).is_none());
    }

    /// The DC gain of a stage is the product of its section gains
    /// `(1 + ff0 + ff1) / (1 - fb0 - fb1)`, times `(1 + tail) * gain`. A
    /// constant input must settle to it, which also checks stability.
    #[test]
    fn mp60_stages_settle_to_their_analytic_dc_gain() {
        for stage in [MP60_IIR1, MP60_IIR2] {
            let mut history = [[[0.0f32; 2]; 2]; MP60_SECTIONS];
            let mut y = 0.0;
            for _ in 0..200_000 {
                y = mp60_iir(&stage, &mut history, 1.0);
            }
            let dc: f64 = stage.sections.iter().map(|s| {
                f64::from(1.0 + s.ff[0] + s.ff[1]) / f64::from(1.0 - s.fb[0] - s.fb[1])
            }).product::<f64>() * f64::from((1.0 + stage.tail) * stage.gain);
            assert!((f64::from(y) - dc).abs() <= 1e-4 * dc.abs().max(1e-3), "dc {y} expected {dc}");
        }
    }

    #[test]
    fn mp60_quantiser_steps_at_twelve_bits_and_ties_to_even() {
        assert_eq!(round_step(1.0 / 2048.0, MP60_QUANT_SCALE, MP60_QUANT_INVERSE), MP60_QUANT_INVERSE);
        assert_eq!(round_step(1.5 / 2048.0, MP60_QUANT_SCALE, MP60_QUANT_INVERSE), 2.0 * MP60_QUANT_INVERSE);
        assert_eq!(round_step(0.5 / 2048.0, MP60_QUANT_SCALE, MP60_QUANT_INVERSE), 0.0);
    }

    #[test]
    fn mp60_chain_state_round_trips_through_slots_across_blocks() {
        let signal: Vec<f32> = (0..600).map(|n| (n as f32 * 0.07).sin() * 0.5).collect();
        let mut whole = Mp60Chain::default();
        let want: Vec<f32> = signal.iter().map(|&x| whole.process(x)).collect();
        let mut slots = vec![[0.; 2]; MP60_STAGE_WORDS * 2];
        let mut got = Vec::new();
        for part in signal.chunks(37) {
            let mut chain = Mp60Chain::load(&slots, 0);
            got.extend(part.iter().map(|&x| chain.process(x)));
            chain.store(&mut slots, 0);
        }
        assert_eq!(got, want);
    }

    #[test]
    fn clock_captures_at_26k_and_the_stage_is_block_size_invariant() {
        // Captures land on ceil(k * 44100 / 26000); 44100 frames give 26000 captures.
        let mut clock = S1200Clock::new(44100);
        let captures: Vec<u64> = (0..44100u64).filter(|&n| clock.capture(n)).collect();
        assert_eq!(captures.len(), 26000);
        for (k, &n) in captures.iter().enumerate().take(12) {
            assert_eq!(n, (k as f64 * 44100.0 / 26000.0).ceil() as u64, "capture {k}");
        }
        // Splitting the same signal into blocks must not change the output.
        let stage = S1200Stage::new(s1200_mode(3), 44100);
        let signal: Vec<f64> = (0..1000).map(|n| (n as f64 * 0.05).sin()).collect();
        let run = |chunk: usize| {
            let mut slots = vec![[0.; 2]; S1200_WORDS];
            let mut schedule = [0.; 2];
            let mut out = Vec::new();
            for part in signal.chunks(chunk) {
                let mut l = part.to_vec();
                let mut r = part.to_vec();
                stage.process(&mut slots, &mut schedule, [&mut l, &mut r], part.len());
                assert_eq!(l, r);
                out.extend(l);
            }
            out
        };
        assert_eq!(run(1000), run(7));
    }
}
