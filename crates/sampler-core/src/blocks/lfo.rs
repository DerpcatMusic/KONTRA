//! The one LFO. Kontakt (sine/rectangle/triangle/saw/random/Multi) and UVI
//! waveforms are configurations of [`LfoShape`]; rate and amount are
//! modulatable through [`LfoInputs`] (the router writes them, W-mod).
//!
//! Evaluation is pure: no state beyond the caller's phase, no allocation.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoShape {
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

/// Waveform value at cycle position `phase` (any real; the integer part counts
/// cycles for the random shapes).
pub(crate) fn wave(shape: LfoShape, phase: f64, seed: u64) -> f64 {
    let cycle = phase.floor();
    let t = phase - cycle;
    match shape {
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
