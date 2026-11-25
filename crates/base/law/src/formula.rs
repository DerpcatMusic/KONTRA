//! Closed forms that no point list reproduces exactly.

use crate::real::Real;

/// Names a closed form. Add a variant, its arm in [`FormulaId::eval`] and a test when a law
/// needs one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaId {
    /// A bipolar attack-curve knob (−1 concave ..= 1 convex) to an exponential segment shape.
    /// Its base is rounded to f32 before the log, as the native envelope does
    /// (`docs/re/engine/k-mod-envelopes.md`).
    AttackShape,
    /// A flex-envelope curve knob (0.5 linear, 0 read as linear) to a segment shape: the log of
    /// the level ratio `(1/f − 1)²`, `f` the knob's quadratic ease-in/out; knobs below 0.001
    /// read `f = 2·10⁻⁶` and above 0.999 `f ≈ 0.999998` (Kontakt 0x140ae5a10).
    FlexShape,
    /// A feedback compressor's ratio `r` to its knee factor
    /// `3.6018/r + 0.8903 − 1.4406/r^1.5` (Kontakt ratio law 0x141a91d10).
    FeedbackKnee,
    /// A bipolar curve knob `c` to `2·ln((1 + c)/(1 − c))` with `c` clamped to ±0.9998,
    /// computed in f64 (UVI envelope setup 0x1410f1220).
    LogRatio,
    /// Raum's predelay knob to ms (`Kontakt8.exe` `0x141982050`): `80·(2x)²` up to
    /// 0.5, then `1920·(2x − 1)² + 80` (0..80..2000 ms).
    RaumPredelayMs,
    /// Raum's high-cut knob to Hz (`Kontakt8.exe` `0x141981200`): `1000 + 23324.1·x²`
    /// up to 0.95, then `22050 + 1764000·(x − 0.95)`.
    RaumHighCutHz,
    /// Bass Pro's gain knob to its readout in dB (`Kontakt8.exe` `0x140a9de30`): the exp
    /// table at `279.44·x + 920.56`, less its value at 0, scaled to 1 at 1; `-inf` below 1e-5.
    BassProGainDb,
    /// Bass Pro's master knob in dB (`0x140a9e430`): as the gain, from `312.43·x + 887.57`,
    /// plus 17.
    BassProMasterDb,
    /// Bass Pro's mid knob in dB (`0x140a9df60`), a cubic in f32 operation order.
    BassProMidDb,
    /// A Bass Pro graphic-EQ band in dB (`0x140a9dc60`), a cubic in f32 operation order.
    BassProGeqDb,
    /// Bass Pro's graphic-EQ volume in dB (`0x140a9dd50`): `(22 − 4x)·x − 10`.
    BassProGeqVolumeDb,
    /// Psyche Delay's detune in cents (`0x140a9db60`): `101·x − 50`, rounded half away
    /// from zero below 0 and truncated above, clamped to ±50.
    PsycheDetuneCents,
    /// Psyche Delay's pitch in semitones (`0x140a9e110`): `25·x − 12`, rounded as the
    /// detune, clamped to ±12.
    PsychePitchSemitones,
    /// Raum's decay knob in ms (`0x1419862e0`): `199.75·x/((1 − x)·75 + 1) + 0.25` s.
    RaumDecayMs,
    /// Raum's modulation rate in Hz (`0x14198bfd0`): `x/((1 − x)·23 + 1)·34.99 + 0.01`.
    RaumRateHz,
    /// Choral's rate in Hz (`0x1407b7d00`): `7.4·x³ + 0.25`.
    ChoralRateHz,
    /// Twin Delay's time in ms (`0x140a9e390`): `2002·x² − 3·x + 1`.
    TwinDelayTimeMs,
    /// Freak's carrier in Hz (`0x140a9da30`): the exp table at `200·x + 1881.88`.
    FreakCarrierHz,
    /// Freak's band-pass frequency in Hz (`0x140a9e290`): the exp table at `455·x + 1551.88`.
    FreakBandPassHz,
    /// Bite's sample rate in Hz (`0x140a9dd90`): the exp table at `527.08·x + 1598.63`.
    BiteFrequencyHz,
    /// Bite's bit depth (`0x140a9d970`): `2 − trunc(−15·x)`, clamped to 2..=16.
    BiteBits,
    /// Flair's rate in Hz (`0x140adfbe0`): `440·2^(11.5·x − 16.25)` by a quartic exp2.
    FlairRateHz,
}

impl FormulaId {
    /// Evaluates the closed form at `x` in its own source units.
    #[must_use]
    pub fn eval(self, x: f32) -> f32 {
        self.at(x)
    }

    pub(crate) fn at<T: Real>(self, x: T) -> T {
        match self {
            Self::AttackShape => T::of(attack_shape(x.to_f64())),
            Self::FlexShape => T::of(flex_ratio(x.to_f32()).ln()),
            Self::FeedbackKnee => T::of(feedback_knee(x.to_f32())),
            Self::LogRatio => T::of(log_ratio(x.to_f64())),
            Self::RaumPredelayMs => T::of(raum_predelay_ms(x.to_f32())),
            Self::RaumHighCutHz => T::of(raum_high_cut_hz(x.to_f32())),
            Self::BassProGainDb => T::of(bass_pro_db(x.to_f32(), GAIN_TABLE)),
            Self::BassProMasterDb => T::of(bass_pro_db(x.to_f32(), MASTER_TABLE)),
            Self::BassProMidDb => T::of(bass_pro_mid_db(x.to_f32())),
            Self::BassProGeqDb => T::of(bass_pro_geq_db(x.to_f32())),
            Self::BassProGeqVolumeDb => {
                let x = x.to_f32();
                T::of((22.0 - x * 4.0) * x - 10.0)
            }
            Self::PsycheDetuneCents => T::of(stepped(x.to_f32(), 101.0, 50)),
            Self::PsychePitchSemitones => T::of(stepped(x.to_f32(), 25.0, 12)),
            Self::RaumDecayMs => {
                let x = x.to_f32();
                T::of((x * 199.75 / ((1.0 - x) * 75.0 + 1.0) + 0.25) * 1000.0)
            }
            Self::RaumRateHz => {
                let x = x.to_f32();
                T::of(x / ((1.0 - x) * 23.0 + 1.0) * 34.99 + 0.01)
            }
            Self::ChoralRateHz => {
                let x = x.to_f32();
                T::of(x * x * x * 7.4 + 0.25)
            }
            Self::FreakCarrierHz => T::of(exp_table(x.to_f32() * 200.0 + 1881.881_6)),
            Self::FreakBandPassHz => T::of(exp_table(x.to_f32() * 455.0 + 1551.881_6)),
            Self::BiteFrequencyHz => T::of(exp_table(x.to_f32() * 527.078_1 + 1598.631_3)),
            Self::BiteBits => {
                T::of((2 - (x.to_f32().clamp(0.0, 1.0) * -15.0) as i32).clamp(2, 16) as f32)
            }
            Self::FlairRateHz => T::of(fast_exp2(x.to_f32() * 11.5 - 16.25) * 440.0),
            Self::TwinDelayTimeMs => {
                let x = x.to_f32();
                T::of(x * 2002.0 * x - x * 3.0 + 1.0)
            }
        }
    }

    /// The input this closed form maps to `y`, where an inverse is known.
    pub(crate) fn inverse(self, y: f64) -> Option<f64> {
        match self {
            Self::AttackShape => Some(attack_curve(y)),
            Self::FlexShape
            | Self::FeedbackKnee
            | Self::LogRatio
            | Self::RaumPredelayMs
            | Self::RaumHighCutHz
            | Self::BassProGainDb
            | Self::BassProMasterDb
            | Self::BassProMidDb
            | Self::BassProGeqDb
            | Self::BassProGeqVolumeDb
            | Self::PsycheDetuneCents
            | Self::PsychePitchSemitones
            | Self::RaumDecayMs
            | Self::RaumRateHz
            | Self::ChoralRateHz
            | Self::TwinDelayTimeMs
            | Self::FreakCarrierHz
            | Self::FreakBandPassHz
            | Self::BiteFrequencyHz
            | Self::BiteBits
            | Self::FlairRateHz => None,
        }
    }
}

/// The attack base's span and floor: `b = e^((1 − |c|)·ln ATTACK_SPAN − ln ATTACK_FLOOR)`, so
/// `b` runs from 1/20 000 at a full bend to 25 when straight
/// (`docs/re/engine/k-mod-envelopes.md`). [`attack_curve`] inverts with the same values.
const ATTACK_SPAN: f64 = 500_000.0;
const ATTACK_FLOOR: f64 = 20_000.0;

fn attack_shape(curve: f64) -> f32 {
    let c = curve.clamp(-1.0, 1.0);
    let b = f64::from(((1.0 - c.abs()) * ATTACK_SPAN.ln() - ATTACK_FLOOR.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

/// The bipolar knob whose [`attack_shape`] is `shape`.
fn attack_curve(shape: f64) -> f64 {
    if shape == 0.0 {
        return 0.0;
    }
    let b = 1.0 / shape.abs().exp_m1();
    let c = (1.0 - (b.ln() + ATTACK_FLOOR.ln()) / ATTACK_SPAN.ln()).clamp(0.0, 1.0);
    -shape.signum() * c
}

fn raum_predelay_ms(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.5 {
        let t = 2.0 * x;
        80.0 * t * t
    } else {
        let t = 2.0 * x - 1.0;
        1920.0 * t * t + 80.0
    }
}

fn raum_high_cut_hz(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.95 {
        1000.0 + 23_324.1 * x * x
    } else {
        22_050.0 + 1_764_000.0 * (x - 0.95)
    }
}

/// A Bass Pro level's table index (`scale·x + offset`), the table's value at 0 and the
/// factor that makes its value at 1 read 1, and the dB added.
struct LevelTable {
    scale: f32,
    offset: f32,
    floor: f32,
    norm: f32,
    db: f32,
}

const GAIN_TABLE: LevelTable = LevelTable {
    scale: 279.440_58,
    offset: 920.559_45,
    floor: 0.039_627_8,
    norm: 1.041_263,
    db: 0.0,
};

const MASTER_TABLE: LevelTable = LevelTable {
    scale: 312.427_34,
    offset: 887.572_63,
    floor: 0.027_070_733,
    norm: 1.027_823_9,
    db: 17.0,
};

/// Kontakt's exp table, `2^((i − 1200)/60)` per entry, read with linear interpolation.
fn exp_table(i: f32) -> f32 {
    let at = |j: i32| (f64::from(j - 1200) / 60.0).exp2() as f32;
    let j = i as i32;
    let frac = i - j as f32;
    frac * (at(j + 1) - at(j)) + at(j)
}

/// `2^y` as the reference's quartic: `2^round(y)` times a polynomial in the remainder.
fn fast_exp2(y: f32) -> f32 {
    let n = y.round_ties_even();
    let r = y - n;
    let p = (r * 0.056_570_42 + 0.240_424_28) * (r * r) + r * 0.692_987_6 + 1.0;
    let p = p + r * r * (r * r) * 0.009_428_403;
    p * f32::from_bits(((n as i32 + 127) as u32) << 23)
}

fn bass_pro_db(x: f32, t: LevelTable) -> f32 {
    let v = (exp_table(x * t.scale + t.offset) - t.floor) * t.norm;
    if f64::from(v) < 1e-5 {
        return f32::NEG_INFINITY;
    }
    v.ln() * 8.685_889 + t.db
}

fn bass_pro_mid_db(x: f32) -> f32 {
    let ax = x * 5.3;
    let b = x * 26.7 - 13.35;
    b - ((x + x - 1.0) * ax - (ax - 2.65))
}

fn bass_pro_geq_db(x: f32) -> f32 {
    let ax = x * 19.2;
    let c = x * x * 38.4 - ax;
    (x + x - 1.0) * c - (c - (ax - 9.6)) + (x * 4.8 - 2.4)
}

/// `k·x − n` as a whole number: rounded half away from zero below 0, truncated above,
/// clamped to ±n.
fn stepped(x: f32, k: f32, n: i32) -> f32 {
    let v = x.clamp(0.0, 1.0) * k - n as f32;
    let v = if v < 0.0 { v.round() } else { v };
    (v as i32).clamp(-n, n) as f32
}

fn log_ratio(c: f64) -> f32 {
    let c = c.clamp(-0.9998, 0.9998);
    (2.0 * ((1.0 + c) / (1.0 - c)).ln()) as f32
}

fn feedback_knee(r: f32) -> f32 {
    3.601_805_4 / r + 0.840_284_1 - 1.440_552_4 / (r.sqrt() * r) + 0.05
}

fn flex_ratio(c: f32) -> f32 {
    let f = if c == 0.0 {
        0.5
    } else if c < 0.001 {
        0.001 * 0.002
    } else if c > 0.999 {
        (4.0 - 1.998) * 0.999 - 1.0
    } else if c <= 0.5 {
        2.0 * c * c
    } else {
        (4.0 - 2.0 * c) * c - 1.0
    };
    (1.0 / f - 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::FormulaId;

    #[test]
    fn display_laws_round_and_clamp_as_the_reference_prints_them() {
        use FormulaId::{BassProGainDb, BassProMasterDb, BiteBits, PsycheDetuneCents};
        // Below 0 the detune rounds half away from zero; above it truncates (0.505 → 1.005).
        assert_eq!(PsycheDetuneCents.eval(0.005), -49.0);
        assert_eq!(PsycheDetuneCents.eval(0.505), 1.0);
        assert_eq!(PsycheDetuneCents.eval(1.0), 50.0);
        assert_eq!(BassProGainDb.eval(0.0), f32::NEG_INFINITY);
        assert!(BassProGainDb.eval(1.0).abs() < 1e-3);
        assert!((BassProMasterDb.eval(1.0) - 17.0).abs() < 1e-3);
        assert_eq!(BiteBits.eval(0.065), 2.0);
        assert_eq!(BiteBits.eval(0.07), 3.0);
        assert_eq!(BiteBits.eval(1.0), 16.0);
    }

    #[test]
    fn the_flex_knob_is_linear_at_half_and_bows_either_side() {
        let shape = |c| FormulaId::FlexShape.eval(c);
        assert!(shape(0.5).abs() < 1e-6);
        assert!(shape(0.0).abs() < 1e-6, "zero reads as linear");
        assert!(shape(0.25) > 0.0);
        assert!(shape(0.75) < 0.0);
        // f(0.25) = 0.125 and f(0.75) = 0.875: ratios 49 and 1/49.
        assert!((shape(0.25) - 49f32.ln()).abs() < 1e-5, "{}", shape(0.25));
        assert!((shape(0.75) + 49f32.ln()).abs() < 1e-5, "{}", shape(0.75));
        // The end branches: ±2·ln(1/(2·10⁻⁶)) ≈ ±26.24 (f32 rounding of 1/f − 1 near 1 shifts
        // the upper end slightly).
        assert!((shape(0.0005) - 26.24).abs() < 0.01, "{}", shape(0.0005));
        assert!((shape(0.9995) + 26.24).abs() < 0.2, "{}", shape(0.9995));
    }

    #[test]
    fn the_attack_knob_is_odd_and_bends_hardest_at_its_ends() {
        let shape = |c| FormulaId::AttackShape.eval(c);
        assert!((shape(-0.4) + shape(0.4)).abs() < 1e-6);
        assert!(shape(1.0) < shape(0.5) && shape(0.5) < 0.0);
        // b = √(5·10⁵) / 2·10⁴ at 0.5 and 1 / 2·10⁴ at 1: ln(b / (1 + b)).
        assert!((shape(0.5) + 3.377_05).abs() < 1e-4, "{}", shape(0.5));
        assert!((shape(1.0) + 9.903_54).abs() < 1e-4, "{}", shape(1.0));
    }
}
