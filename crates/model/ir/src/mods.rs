//! What a group modulation moves and by how much.

use crate::{Curve, EffectParam, Source};

/// What a [`crate::Mod`] does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModKind {
    /// The group's [`Group::envelope`](crate::Group::envelope), shaping its level.
    Envelope,
    /// `source` scales the level along `curve`; `intensity` is −1..=1 and live.
    /// A bipolar source (an LFO, pitch bend) enters as `(x + 1)/2`: it swings the level
    /// down from unity and back.
    Volume {
        source: Source,
        intensity: f32,
        curve: Curve,
    },
    /// `source × depth × semitones` added to the pitch: `semitones` is the
    /// route's range, `depth` (−1..=1) its live intensity.
    Pitch {
        source: Source,
        semitones: f32,
        depth: f32,
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
    /// Scales the [`Group::envelope`](crate::Group::envelope) attack `t` at note-on, as a knob
    /// linear in `ln(t + floor_ms)`: with `m = 1 − (1 − x)·|intensity|` (`x`
    /// read at note-on, `1 − x` for a negative intensity), the attack becomes
    /// `floor_ms·((t + floor_ms)/floor_ms)^m − floor_ms`.
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
