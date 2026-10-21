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
    let scaled = x * S1200_QUANT_SCALE;
    s1200_round(f64::from(scaled)) as f32 * S1200_QUANT_INVERSE
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
