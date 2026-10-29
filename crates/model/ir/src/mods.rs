//! What a group modulation moves and by how much.

use crate::{Curve, EffectParam, Source};

/// What a [`crate::Mod`] does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModKind {
    /// The group's [`Group::envelope`](crate::Group::envelope), shaping its level.
    Envelope,
    /// `source` scales the level along `curve`; `intensity` is −1..=1 and live.
    /// A bipolar source (an LFO, pitch bend) enters as `(x + 1)/2`: it swings the level
    /// down from unity and back. The route's gain follows as a one-pole with
    /// time constant `tau_ms` (0: at once), so a controller ride glides.
    Volume {
        source: Source,
        intensity: f32,
        curve: Curve,
        tau_ms: f32,
    },
    /// `source × depth × semitones` added to the pitch: `semitones` is the
    /// route's range, `depth` (−1..=1) its live intensity. The semitones
    /// follow as a one-pole with time constant `tau_ms` (0: at once).
    Pitch {
        source: Source,
        semitones: f32,
        depth: f32,
        tau_ms: f32,
    },
    /// `source × amount` added to the pan (−1..=1).
    Pan { source: Source, amount: f32 },
    /// Moves the cutoff of the [`Filter`](crate::Filter) in group insert slot `insert` by
    /// `source × depth × Filter::octaves` octaves; that filter runs per voice.
    Cutoff {
        source: Source,
        insert: u8,
        depth: f32,
    },
    /// Moves the resonance of the [`Filter`](crate::Filter) in group insert slot `insert` by
    /// `source × depth × Filter::q_octaves` octaves of Q; that filter runs per voice.
    /// On a [ladder](crate::Filter::ladder) it scales the resonance `1 − k/2` by
    /// [`Curve::Signed`](crate::Curve::Signed) at `depth` instead, a bipolar
    /// source read as `(x + 1)/2`.
    Resonance {
        source: Source,
        insert: u8,
        depth: f32,
    },
    /// Moves the gain of band `band` of the [`Eq`] in group insert slot
    /// `insert` by `source × depth_db` dB, clamped to `±limit_db`, per voice;
    /// `span_db` is the depth at the intensity knob's full travel.
    ///
    /// [`Eq`]: crate::EffectKind::Eq
    EqGain {
        source: Source,
        insert: u8,
        band: u8,
        depth_db: f32,
        limit_db: f32,
        span_db: f32,
    },
    /// `source × depth` of the zone's [`Zone::start_mod`](crate::Zone::start_mod) frames added to
    /// where its voices start, read at note-on.
    SampleStart { source: Source, depth: f32 },
    /// Scales the [`Group::envelope`](crate::Group::envelope) attack `t` at note-on by
    /// `m = 1 − (1 − x)·|intensity|` (`x` read at note-on, `1 − x` for a
    /// negative intensity): `t·m` when `floor_ms` is 0, else as a knob linear
    /// in `ln(t + floor_ms)`, `floor_ms·((t + floor_ms)/floor_ms)^(m³) − floor_ms`.
    Attack {
        source: Source,
        intensity: f32,
        floor_ms: f32,
    },
    /// As [`ModKind::Attack`], on the release time.
    Release {
        source: Source,
        intensity: f32,
        floor_ms: f32,
    },
    /// Adds `source × depth` cycles (`depth` −1..=1) to the start phase of
    /// the group's LFO `lfo`, `source` read at note-on.
    LfoPhase { source: Source, lfo: u8, depth: f32 },
    /// Moves the rate of the group's LFO `lfo` along its knob by
    /// `source × depth` (knob units, `source` read at note-on): the rate
    /// scales by `2^(source × depth × octaves)`, held within `range_hz`.
    LfoRate {
        source: Source,
        lfo: u8,
        depth: f32,
        octaves: f32,
        range_hz: [f32; 2],
    },
    /// Scales every route of the group's timed source `of` (an LFO or an
    /// envelope) by `1 − (1 − x)·depth`, `x` the live `source`, a bipolar one
    /// as `(x + 1)/2`.
    Intensity {
        source: Source,
        of: Source,
        depth: f32,
    },
    /// Moves the zone's loops by `source × depth` of the sample's frames,
    /// read at note-on: start and end together, or the end only (`length`).
    LoopShift {
        source: Source,
        length: bool,
        depth: f32,
    },
    /// Scales cell `row` (a sample channel), `side` (0 left, 1 right) of
    /// the [`Zone::mix`](crate::Zone::mix) of the group's zone `zone` (its
    /// index in [`Group::zones`](crate::Group::zones)) by the live `source`,
    /// 0..=1: a channel matrix that crossfades variants.
    Mix {
        source: Source,
        zone: u16,
        row: u8,
        side: u8,
    },
    /// Adds `source × depth` (knob × `span`) to `param` of the per-voice
    /// effect in group slot `insert`; `unipolar` reads an LFO as 0..=1.
    EffectParam {
        source: Source,
        insert: u8,
        param: EffectParam,
        depth: f32,
        span: f32,
        unipolar: bool,
    },
}

/// A sum of live sources, as a macro knob and what moves it:
/// `base + Σ amount × input × Π factors`. [`Source::Blend`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Blend {
    pub base: f32,
    pub terms: Vec<Term>,
}

/// One input of a [`Blend`]: its source through a table, scaled.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub input: Transfer,
    pub amount: f32,
    pub factors: Vec<Factor>,
}

/// A live multiplier: `1 − depth + depth × t` (`1 + depth × t` for a
/// negative depth), `t` its source through its table.
#[derive(Clone, Debug, PartialEq)]
pub struct Factor {
    pub input: Transfer,
    pub depth: f32,
}

impl Factor {
    /// The multiplier for the table's output `t`.
    #[must_use]
    pub fn at(&self, t: f32) -> f32 {
        let d = self.depth;
        if d < 0.0 {
            1.0 + d * t
        } else {
            1.0 - d + d * t
        }
    }
}

/// A source through a transfer table: `table` holds the outputs for evenly
/// spaced inputs 0..=1, read with linear interpolation; an LFO enters as
/// `(x + 1)/2`. Its source is never itself shaped.
#[derive(Clone, Debug, PartialEq)]
pub struct Transfer {
    pub source: Source,
    pub table: Vec<f32>,
}

impl Transfer {
    /// The table's output for `x`, clamped to 0..=1; an empty table passes
    /// `x` through.
    #[must_use]
    pub fn apply(&self, x: f32) -> f32 {
        let Some(last) = self.table.len().checked_sub(1) else {
            return x;
        };
        let at = x.clamp(0.0, 1.0) * last as f32;
        let i = (at as usize).min(last);
        let (a, b) = (
            self.table.get(i).copied().unwrap_or(x),
            self.table
                .get(i + 1)
                .or(self.table.get(i))
                .copied()
                .unwrap_or(x),
        );
        a + (b - a) * (at - i as f32)
    }
}

/// A power of two as a sampler computes it: exactly, or as
/// `2^n·(1 + f − c·(f − f²))` for `x = n + f` (`f` in 0..1), a quadratic
/// exact at whole octaves and up to 4.6 cents flat between them for
/// `c = 0.33971`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exp2 {
    Exact,
    Quadratic { c: f32 },
}

impl Exp2 {
    #[must_use]
    pub fn of(self, x: f64) -> f64 {
        match self {
            Self::Exact => x.exp2(),
            Self::Quadratic { c } => {
                let n = x.floor();
                let f = x - n;
                n.exp2() * (1.0 + f - f64::from(c) * (f - f * f))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Exp2;

    #[test]
    fn the_quadratic_power_of_two_plays_a_two_semitone_bend_flat() {
        let law = Exp2::Quadratic { c: 0.33971 };
        let semitones = |st: f64| 12.0 * law.of(st / 12.0).log2();
        // Kontakt on a ±2 st route: full bend up and down, a quarter each way.
        for (st, heard) in [(2.0, 1.954), (-2.0, -1.958), (0.5, 0.480), (-0.5, -0.485)] {
            assert!(
                (semitones(st) - heard).abs() < 0.001,
                "{st}: {}",
                semitones(st)
            );
        }
        assert_eq!(law.of(1.0), 2.0);
        assert_eq!(Exp2::Exact.of(-1.0), 0.5);
    }
}
