//! The one LFO. Kontakt (sine/rectangle/triangle/saw/random/Multi) and UVI
//! waveforms are configurations of [`LfoShape`]; rate and amount are
//! modulatable through [`LfoInputs`] (the router writes them, W-mod).
//!
//! Evaluation is pure: no state beyond the caller's phase, no allocation.

pub use sampler_ir::NativeWave;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoShape {
    /// A native Kontakt waveform; see [`NativeWave`].
    Native(NativeWave),
    /// Saved Digital Multi sine; prepared native fade gains, separate from the linear `Lfo::fade`.
    DigitalSine {
        level: f64,
        fade: crate::KontaktLfoFade,
    },
    /// Signed bipolar peak, prepared in -1..=1.
    SineScaled(f64),
    /// Bipolar zero, including every phase, delay and fade state.
    Zero,
    Sine,
    Triangle,
    /// +1 for the first half cycle; exactly `Rectangle(0.5)`.
    Square,
    /// +1 for the first `width` of the cycle (0..=1), then -1.
    Rectangle(f64),
    SawUp,
    SawDown,
    SampleAndHold,
    /// A new random value each cycle, reached linearly by the cycle's end.
    Random,
    /// Weighted mix of sine, rectangle(`width`), triangle, saw-up and
    /// sample-and-hold. With `normalize` the sum is divided by
    /// max(1, sum(abs(weights))) (the Digital Multi law, native core
    /// 0x140b07290 as recorded in `sampler-kontakt` library.rs).
    Multi {
        weights: [f64; 5],
        width: f64,
        normalize: bool,
    },
}

impl LfoShape {
    /// Parameters a prepared program may carry: finite, widths in 0..=1.
    pub fn valid(&self) -> bool {
        match *self {
            Self::Native(NativeWave::Rectangle(w)) => w > 0.0 && w < 1.0,
            Self::Rectangle(w) => (0.0..=1.0).contains(&w),
            Self::Multi { weights, width, .. } => {
                (0.0..=1.0).contains(&width) && weights.iter().all(|w| w.is_finite())
            }
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoRate {
    Hertz(f64),
    /// Cycle length in quarter-note beats at [`crate::Runtime::set_tempo`].
    Beats(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: LfoShape,
    pub rate: LfoRate,
    /// Cycle position at the start, 0..1.
    pub phase: f64,
    /// Silent frames after the voice starts.
    pub delay: u32,
    /// Frames of linear depth ramp after the delay.
    pub fade: u32,
    /// Per-voice cycle from `phase`; otherwise one cycle on the runtime clock.
    pub retrigger: bool,
    /// With `retrigger`: one instance shared by every voice, restarted at
    /// `phase` (and its delay and fade) whenever any voice starts.
    pub shared: bool,
}

/// Modulatable inputs: `rate` multiplies the frequency, `amount` the output.
/// Unity leaves every bit unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoInputs {
    pub rate: f64,
    pub amount: f64,
}

impl LfoInputs {
    pub const UNITY: Self = Self {
        rate: 1.0,
        amount: 1.0,
    };
}

impl Lfo {
    /// Cycles per second at `tempo` BPM.
    pub fn hertz(&self, tempo: f64, inputs: LfoInputs) -> f64 {
        let hz = match self.rate {
            LfoRate::Hertz(hz) => hz,
            LfoRate::Beats(beats) => tempo / (60.0 * beats),
        };
        hz * inputs.rate
    }

    /// Whether the start state fits the shape: native held noise counts its
    /// half-cycle updates from a start phase below one half.
    pub fn valid(&self) -> bool {
        self.shape.valid() && (self.shape != LfoShape::Native(NativeWave::Held) || self.phase < 0.5)
    }

    /// Delay then linear fade-in, evaluated at `age` frames since the start.
    pub fn depth(&self, age: u32) -> f64 {
        if age < self.delay {
            0.0
        } else if self.fade == 0 {
            1.0
        } else {
            (f64::from(age - self.delay) / f64::from(self.fade)).min(1.0)
        }
    }

    /// Output at cycle position `phase` and `age` frames since the start.
    pub fn value(&self, phase: f64, seed: u64, age: u32, inputs: LfoInputs) -> f64 {
        self.depth(age) * wave(self.shape, phase, seed) * inputs.amount
    }
}

/// splitmix64 of a seed, source and cycle: deterministic per voice and cycle,
/// and identical across voices for free-running sources (seed 0).
pub(crate) fn hash(seed: u64, source: u64, cycle: u64) -> u64 {
    let mut z = seed
        .wrapping_add(source.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(cycle.wrapping_mul(0xbf58_476d_1ce4_e5b9));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

pub(crate) fn uniform(bits: u64) -> f64 {
    (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

fn rectangle(t: f64, width: f64) -> f64 {
    if t < width { 1.0 } else { -1.0 }
}

fn triangle(t: f64) -> f64 {
    if t < 0.25 {
        4.0 * t
    } else if t < 0.75 {
        2.0 - 4.0 * t
    } else {
        4.0 * t - 4.0
    }
}

fn held(seed: u64, cycle: f64) -> f64 {
    2.0 * uniform(hash(seed, 0, cycle as i64 as u64)) - 1.0
}

const SINE: &[u8; 16384] = include_bytes!("tables/lfo-sine.f32le");
const SAW: &[u8; 16384] = include_bytes!("tables/lfo-saw.f32le");
const TRIANGLE: &[u8; 16384] = include_bytes!("tables/lfo-triangle.f32le");
const PULSE: &[u8; 16384] = include_bytes!("tables/lfo-pulse.f32le");

fn entry(table: &[u8; 16384], i: i64) -> f32 {
    let i = (i & 4095) as usize;
    f32::from_le_bytes([table[4 * i], table[4 * i + 1], table[4 * i + 2], table[4 * i + 3]])
}

/// Four-point interpolation at table position `v` (entries; wraps), in the
/// native binary32 operation order of the table helpers (0x140b09070,
/// 0x140b08f90, 0x140b09150; modulators.md "Exact table interpolation").
fn table(table: &[u8; 16384], v: f64) -> f32 {
    let i = v.trunc() as i64;
    let t = (v - i as f64) as f32;
    let (a, b) = (t + 1.0, t - 1.0);
    let c = b - 1.0;
    let big_a = (a * entry(table, i + 2) * b * t) - (t * entry(table, i - 1) * c * b);
    let big_b = (a * entry(table, i) * b) - (a * entry(table, i + 1) * t);
    big_a * 0.166_666_67 + big_b * c * 0.5
}

/// Native rectangle (0x140b08c40): outside +-0.01 of one half, a difference of
/// two shifted saw lookups plus the binary64 offset 1-2w; inside it, the
/// near-half pulse table. The inline saw interpolation's exact operation order
/// is not recovered, so the shared order stands in (differs in the last f32 bits).
fn rectangle_native(p: f64, w: f32) -> f64 {
    if (w - 0.5).abs() >= 0.009_999_999_8 {
        let w = f64::from(w);
        let first = table(SAW, p * 4096.0);
        let second = table(SAW, (1.0 - w) * 4096.0 + p * 4096.0);
        f64::from(first - second) - (1.0 - (w + w))
    } else {
        f64::from(table(PULSE, p * 4096.0))
    }
}

const LCG: (u32, u32) = (0x0bb3_8435, 0x3619_636b);

/// `seed` advanced `n` steps of the native noise recurrence (affine jump-ahead).
fn lcg_jump(mut seed: u32, mut n: u64) -> u32 {
    let (mut a, mut c) = LCG;
    while n > 0 {
        if n & 1 == 1 {
            seed = seed.wrapping_mul(a).wrapping_add(c);
        }
        c = c.wrapping_mul(a).wrapping_add(c);
        a = a.wrapping_mul(a);
        n >>= 1;
    }
    seed
}

/// Held noise (0x140b06f50): the seed recurrence advances at each half cycle;
/// `seed` is the reset seed (native resets from the tick count).
fn held_native(phase: f64, seed: u64) -> f64 {
    let reset = ((seed ^ (seed >> 32)) as u32).wrapping_mul(LCG.0).wrapping_add(LCG.1);
    let bits = lcg_jump(reset, (2.0 * phase).floor().max(0.0) as u64);
    f64::from(f32::from_bits((bits & 0x7f_ffff) | 0x4000_0000) - 3.0)
}

fn native(w: NativeWave, p: f64, seed: u64, phase: f64) -> f64 {
    match w {
        NativeWave::Sine => f64::from(table(SINE, p * 4096.0)),
        NativeWave::Saw => f64::from(table(SAW, p * 4096.0)),
        NativeWave::Triangle => f64::from(table(TRIANGLE, p * 4096.0)),
        NativeWave::Rectangle(w) => rectangle_native(p, w),
        NativeWave::Held => held_native(phase, seed),
    }
}

/// Native normalized-frequency lane (BLfo* and BArp32Step slot 6, e.g.
/// 0x140b088e7..0x140b088eb): input `u` clamped to 0..1, an approximate
/// exponential built by reinterpreting float bits, scaled by `reciprocal_rate`
/// (the object's `F(1/rate)`), returned as the binary64 per-output phase step.
/// What rate the caller supplies is unproved (not assumed to be audio/32).
pub fn native_phase_step(u: f32, reciprocal_rate: f32) -> f64 {
    let u = if u.is_nan() { 0.0 } else { u.clamp(0.0, 1.0) };
    let z = u * 14.316_152 - 6.584_316;
    let r = z - z.floor();
    let bits = ((z + 127.0) - (r - r * r) * 0.339_71) * 8_388_608.0;
    f64::from(f32::from_bits(bits as i32 as u32) * reciprocal_rate)
}

/// Waveform value at cycle position `phase` (any real; the integer part counts
/// cycles for the random shapes).
pub(crate) fn wave(shape: LfoShape, phase: f64, seed: u64) -> f64 {
    let cycle = phase.floor();
    let t = phase - cycle;
    match shape {
        LfoShape::Native(w) => native(w, t, seed, phase),
        LfoShape::DigitalSine { level, .. } | LfoShape::SineScaled(level) => {
            f64::from((t * std::f64::consts::TAU).sin() as f32 * level as f32)
        }
        LfoShape::Zero => 0.,
        LfoShape::Sine => (t * std::f64::consts::TAU).sin(),
        LfoShape::Triangle => triangle(t),
        LfoShape::Square => rectangle(t, 0.5),
        LfoShape::Rectangle(width) => rectangle(t, width),
        LfoShape::SawUp => 2.0 * t - 1.0,
        LfoShape::SawDown => 1.0 - 2.0 * t,
        LfoShape::SampleAndHold => held(seed, cycle),
        LfoShape::Random => {
            let from = held(seed, cycle);
            let to = 2.0 * uniform(hash(seed, 0, (cycle as i64 + 1) as u64)) - 1.0;
            from + (to - from) * t
        }
        LfoShape::Multi {
            weights: w,
            width,
            normalize,
        } => {
            let sum = w[0] * (t * std::f64::consts::TAU).sin()
                + w[1] * rectangle(t, width)
                + w[2] * triangle(t)
                + w[3] * (2.0 * t - 1.0)
                + w[4] * held(seed, cycle);
            if normalize {
                sum / w.iter().map(|w| w.abs()).sum::<f64>().max(1.0)
            } else {
                sum
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lfo(shape: LfoShape) -> Lfo {
        Lfo {
            shape,
            rate: LfoRate::Hertz(2.0),
            phase: 0.0,
            delay: 0,
            fade: 0,
            retrigger: true,
            shared: false,
        }
    }

    #[test]
    fn rectangle_width_sets_the_high_fraction_and_half_is_square() {
        let high = |w| {
            (0..1000)
                .filter(|i| wave(LfoShape::Rectangle(w), f64::from(*i) / 1000., 0) > 0.)
                .count()
        };
        assert_eq!(high(0.25), 250);
        assert_eq!(high(0.8), 800);
        for i in 0..64 {
            let p = f64::from(i) / 64.;
            assert_eq!(
                wave(LfoShape::Rectangle(0.5), p, 0).to_bits(),
                wave(LfoShape::Square, p, 0).to_bits()
            );
        }
    }

    #[test]
    fn multi_mixes_components_and_normalizes_by_total_weight() {
        let multi = |weights, normalize| LfoShape::Multi {
            weights,
            width: 0.5,
            normalize,
        };
        // A single unit weight is that component.
        for (k, solo) in [
            LfoShape::Sine,
            LfoShape::Square,
            LfoShape::Triangle,
            LfoShape::SawUp,
            LfoShape::SampleAndHold,
        ]
        .into_iter()
        .enumerate()
        {
            let mut w = [0.; 5];
            w[k] = 1.;
            for i in 0..16 {
                let p = 3.0 + f64::from(i) / 16. + 0.01;
                assert!((wave(multi(w, true), p, 9) - wave(solo, p, 9)).abs() < 1e-12);
            }
        }
        // Weights summing past one are scaled back; below one they are not.
        let p = 0.125;
        let sum = wave(LfoShape::Sine, p, 0) + wave(LfoShape::Triangle, p, 0);
        assert!((wave(multi([1., 0., 1., 0., 0.], true), p, 0) - sum / 2.).abs() < 1e-12);
        assert!((wave(multi([0.5, 0., 0., 0., 0.], true), p, 0) - 0.5 * wave(LfoShape::Sine, p, 0)).abs() < 1e-12);
        assert!((wave(multi([1., 0., 1., 0., 0.], false), p, 0) - sum).abs() < 1e-12);
    }

    const SINE: &[u8; 16384] = include_bytes!("tables/lfo-sine.f32le");
    fn entry(t: &[u8; 16384], i: usize) -> f32 {
        f32::from_le_bytes(t[4 * i..4 * i + 4].try_into().unwrap())
    }

    /// modulators.md "Exact table interpolation": landmarks of each table helper.
    #[test]
    fn native_tables_hit_the_recovered_landmarks() {
        let n = |w, p| wave(LfoShape::Native(w), p, 0);
        for (w, quarter) in [
            (NativeWave::Sine, 1.0),
            (NativeWave::Saw, 0.4998449981212616),
            (NativeWave::Triangle, 0.999800980091095),
        ] {
            assert_eq!(n(w, 0.0), 0.0);
            assert!((n(w, 0.25) - quarter).abs() < 1e-6, "{w:?}");
            assert!(n(w, 0.5).abs() < 1e-6);
            assert!((n(w, 0.75) + quarter).abs() < 1e-6);
        }
        // Whole-entry positions return the table entry exactly; between them the
        // cubic stays close to the ideal sine.
        for i in [1usize, 77, 1024, 4095] {
            assert_eq!(
                n(NativeWave::Sine, i as f64 / 4096.0).to_bits(),
                f64::from(entry(SINE, i)).to_bits()
            );
        }
        let mid = n(NativeWave::Sine, 100.5 / 4096.0);
        assert!((mid - (100.5 / 4096.0 * std::f64::consts::TAU).sin()).abs() < 1e-6);
    }

    #[test]
    fn native_saw_rings_past_unity() {
        let peak = (0..4096)
            .map(|i| wave(LfoShape::Native(NativeWave::Saw), f64::from(i) / 4096.0, 0).abs())
            .fold(0.0, f64::max);
        assert!((peak - 1.1784900426864624).abs() < 1e-6, "{peak}");
    }

    #[test]
    fn native_rectangle_width_branch_and_near_half_table() {
        let r = |w, p| wave(LfoShape::Native(NativeWave::Rectangle(w)), p, 0);
        // Narrow pulse: high for the first quarter of the cycle, low after, within ringing.
        assert!((r(0.25, 0.1) - 1.0).abs() < 0.25, "{}", r(0.25, 0.1));
        assert!((r(0.25, 0.6) + 1.0).abs() < 0.25, "{}", r(0.25, 0.6));
        // Within 0.01 of one half the near-half pulse table answers (not an ideal square).
        assert!((r(0.5, 0.25) - 0.999688982963562).abs() < 1e-6);
        assert_eq!(r(0.505, 0.25), r(0.5, 0.25));
        assert!((r(0.5, 0.75) + 0.999688982963562).abs() < 1e-6);
    }

    #[test]
    fn native_held_noise_is_the_lcg_grid_updated_twice_per_cycle() {
        let held = |p| wave(LfoShape::Native(NativeWave::Held), p, 0x1234);
        // Constant inside a half cycle, new value at each half.
        assert_eq!(held(0.1), held(0.4));
        assert_ne!(held(0.4), held(0.6));
        assert_eq!(held(0.6), held(0.9));
        assert_ne!(held(0.9), held(1.1));
        // Jump-ahead equals iterating the recurrence.
        let (a, c) = (0x0bb3_8435u32, 0x3619_636bu32);
        let mut s = 0x1234u32.wrapping_mul(a).wrapping_add(c);
        let mut grid = Vec::new();
        for _ in 0..40 {
            grid.push(f32::from_bits((s & 0x7f_ffff) | 0x4000_0000) - 3.0);
            s = s.wrapping_mul(a).wrapping_add(c);
        }
        for (k, want) in grid.iter().enumerate() {
            assert_eq!(held((k as f64 + 0.25) / 2.0), f64::from(*want), "update {k}");
            assert!((-1.0..1.0).contains(want));
        }
    }

    #[test]
    fn native_rate_lane_approximates_a_base_two_exponential() {
        for k in 0..=20 {
            let u = k as f32 / 20.0;
            let want = 2f64.powf(f64::from(u) * 14.316_152 - 6.584_316);
            let got = native_phase_step(u, 1.0);
            assert!((got / want - 1.0).abs() < 0.01, "u={u} {got} vs {want}");
        }
        assert_eq!(native_phase_step(2.0, 0.5), native_phase_step(1.0, 0.5), "clamped");
        assert_eq!(native_phase_step(f32::NAN, 1.0), native_phase_step(0.0, 1.0));
        assert_eq!(native_phase_step(0.5, 0.25) * 4.0, native_phase_step(0.5, 1.0));
    }

    #[test]
    fn invalid_shapes_are_rejected() {
        let held = Lfo { phase: 0.5, ..lfo(LfoShape::Native(NativeWave::Held)) };
        assert!(!held.valid() && Lfo { phase: 0.25, ..held }.valid());
        assert!(!LfoShape::Native(NativeWave::Rectangle(0.0)).valid());
        assert!(LfoShape::Rectangle(0.0).valid() && LfoShape::Rectangle(1.0).valid());
        assert!(!LfoShape::Rectangle(1.5).valid() && !LfoShape::Rectangle(f64::NAN).valid());
        let multi = |weights, width| LfoShape::Multi { weights, width, normalize: true };
        assert!(multi([0.; 5], 0.5).valid());
        assert!(!multi([f64::INFINITY, 0., 0., 0., 0.], 0.5).valid());
        assert!(!multi([0.; 5], -0.1).valid());
    }

    #[test]
    fn delay_fade_and_modulated_inputs() {
        let mut l = lfo(LfoShape::SawUp);
        l.delay = 10;
        l.fade = 20;
        let at = |age| l.value(0.75, 0, age, LfoInputs::UNITY);
        assert_eq!(at(9), 0.0);
        assert_eq!(at(10), 0.0);
        assert!((at(20) - 0.5 * 0.5).abs() < 1e-12);
        assert!((at(30) - 0.5).abs() < 1e-12);
        let half = LfoInputs {
            rate: 2.0,
            amount: 0.5,
        };
        assert!((l.value(0.75, 0, 99, half) - 0.25).abs() < 1e-12);
        assert_eq!(l.hertz(120., half), 4.0);
        l.rate = LfoRate::Beats(1.0);
        assert_eq!(l.hertz(120., LfoInputs::UNITY), 2.0);
    }
}
