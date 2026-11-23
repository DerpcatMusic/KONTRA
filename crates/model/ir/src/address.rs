//! One modulation route: a source, the parameter it moves ([`ParamAddr`]),
//! how far ([`ModRoute::depth`], [`ModRoute::span`]) and along which law
//! ([`RouteLaw`]). Every translator emits these; plan and engine read them.
//! [`Scope`] with [`ParamAddr`] also addresses live parameters.

use crate::{Curve, EffectParam, Source, TableValue, TimeScale};

/// Where a live [`ParamAddr`] write lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Per-voice / per-note.
    Voice,
    /// Group-level shared state.
    Group(u16),
    /// Bus-level mixer channel.
    Bus(u16),
    /// Instrument-wide master/rack state.
    Instrument,
}

/// Envelope stage addressed by [`ParamAddr::EnvelopeStage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvStage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// LFO parameter addressed by [`ParamAddr::LfoParam`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LfoParam {
    /// Start phase, `source × depth` cycles (`depth` −1..=1), read at note-on.
    Phase,
    /// Rate along its knob by `source × depth` knob units: it scales by
    /// `2^(source × depth × span)` (`span` octaves), held within the
    /// route's [`RouteLaw::Range`] (Hz).
    Rate,
    /// `source × depth × (hi − lo)` Hz added to a free-running LFO's rate,
    /// held within the route's [`RouteLaw::Range`] `[lo, hi]`, live: the
    /// phase integrates the moving rate.
    Hz,
    /// A tempo-synced rate moved `source × depth × 29` places along
    /// [`NOTE_LADDER`](crate::NOTE_LADDER) (rounded, held on it), live.
    Division,
}

/// The parameter a [`ModRoute`] moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamAddr {
    /// The group's [`Group::envelope`](crate::Group::envelope) shaping its
    /// level: a structural route with no source, which scripts address by slot.
    AmpEnvelope,
    /// Level: `source` scales the gain along the route's [`RouteLaw::Gain`]
    /// curve, `depth` (−1..=1) its live intensity. A bipolar source (an LFO,
    /// pitch bend) enters as `(x + 1)/2`, swinging the level down from unity.
    Volume,
    /// `source × depth × span` semitones added to the pitch (`span` the
    /// route's range, `depth` −1..=1 its live intensity).
    Pitch,
    /// `source × depth` added to the pan (−1..=1).
    Pan,
    /// `source × depth` of the zone's [`Zone::start_mod`](crate::Zone::start_mod)
    /// frames added to where its voices start, read at note-on.
    SampleStart,
    /// The zone's loops moved by `source × depth` of the sample's frames,
    /// read at note-on: start and end together, or the end only (`length`).
    LoopShift { length: bool },
    /// A stage of the amplitude envelope (`envelope` none) or of breakpoint
    /// envelope `envelope` (attack: its first segment). A time scales by
    /// `m = 1 − (1 − x)·|depth|` (`1 − x` for a negative depth) under the
    /// route's [`RouteLaw::Time`], read at note-on (release: live at
    /// note-off); sustain scales by its [`RouteLaw::Gain`] at note-on.
    EnvelopeStage {
        envelope: Option<u8>,
        stage: EnvStage,
    },
    /// Parameter `param` of the group's LFO `lfo`.
    LfoParam { lfo: u8, param: LfoParam },
    /// The cutoff of the [`Filter`](crate::Filter) in group insert slot
    /// `insert`, per voice: `source × depth × Filter::octaves` octaves
    /// ([`RouteLaw::Linear`]), or an interpolated Hz offset across a
    /// [`RouteLaw::Range`], a bipolar source read on 0..=1.
    FilterCutoff { insert: u8 },
    /// The resonance of the filter in group insert slot `insert`, per voice,
    /// by `source × depth × Filter::q_octaves` octaves of Q. On a
    /// [ladder](crate::Filter::ladder) it scales the resonance `1 − k/2` by
    /// [`Curve::Signed`] at `depth` instead, a bipolar source read as `(x + 1)/2`.
    FilterResonance { insert: u8 },
    /// Band `band` of the [`Eq`](crate::EffectKind::Eq) in group insert slot
    /// `insert`, per voice, by `source × depth` dB held within the route's
    /// [`RouteLaw::Clamp`]; `span` is the depth at the knob's full travel.
    EqBandGain { insert: u8, band: u8 },
    /// `source × depth` (knob × `span`) added to `param` of the per-voice
    /// effect in group slot `insert` (on a bus: of its insert `insert`);
    /// [`RouteLaw::Unipolar`] reads an LFO as 0..=1.
    EffectParam { insert: u8, param: EffectParam },
    /// `source × depth` added to the voice's wavetable value (0..=1), live,
    /// held within 0..=1.
    Wavetable(TableValue),
    /// Cell `row` (a sample channel), `side` (0 left, 1 right) of the
    /// [`Zone::mix`](crate::Zone::mix) of the group's zone `zone` (its index
    /// in [`Group::zones`](crate::Group::zones)), scaled by the live `source`
    /// on 0..=1: a channel matrix that crossfades variants.
    ZoneMix { zone: u16, row: u8, side: u8 },
    /// Every route of the group's timed source `of` (an LFO or envelope)
    /// scaled by `1 − (1 − x)·depth`, `x` the live `source`, a bipolar one
    /// as `(x + 1)/2`.
    RouteIntensity { of: Source },
    /// The level of send `send` (its index in [`Bus::sends`](crate::Bus::sends))
    /// scaled by the live `source`; a bus route.
    BusSend { send: u8 },
}

/// How a route turns `source × depth` into a move of its target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum RouteLaw {
    /// Linear in the target's units.
    #[default]
    Linear,
    /// Linear, an LFO source read on 0..=1 instead of −1..=1.
    Unipolar,
    /// A gain along `Curve` (volume and sustain routes).
    Gain(Curve),
    /// An envelope stage time scaled under `TimeScale`.
    Time(TimeScale),
    /// Interpolated across, or held within, `[lo, hi]` Hz (a cutoff or an LFO rate).
    Range([f32; 2]),
    /// Held within `±limit` (dB, an EQ band).
    Clamp(f32),
}

/// One modulation route of a group ([`Mod`](crate::Mod)) or a bus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModRoute {
    pub source: Source,
    pub target: ParamAddr,
    /// Live intensity, in the units [`ParamAddr`] gives for its target.
    pub depth: f32,
    /// The target's range at full depth (semitones, dB, effect knob units,
    /// LFO octaves); 1 where the target has none.
    pub span: f32,
    pub law: RouteLaw,
    /// The route's value follows as a one-pole with this time constant
    /// (0: at once), so a controller ride glides.
    pub smooth_ms: f32,
}

impl ModRoute {
    /// `source` moving `target` by `depth`: linear, span 1, unsmoothed.
    #[must_use]
    pub const fn new(source: Source, target: ParamAddr, depth: f32) -> Self {
        Self {
            source,
            target,
            depth,
            span: 1.0,
            law: RouteLaw::Linear,
            smooth_ms: 0.0,
        }
    }

    /// The amplitude envelope's structural route.
    pub const AMP_ENVELOPE: Self = Self::new(Source::Constant, ParamAddr::AmpEnvelope, 1.0);

    #[must_use]
    pub const fn span(mut self, span: f32) -> Self {
        self.span = span;
        self
    }

    #[must_use]
    pub const fn law(mut self, law: RouteLaw) -> Self {
        self.law = law;
        self
    }

    #[must_use]
    pub const fn smooth(mut self, ms: f32) -> Self {
        self.smooth_ms = ms;
        self
    }

    /// What drives it; the amplitude envelope route has none.
    #[must_use]
    pub fn driver(&self) -> Option<Source> {
        (self.target != ParamAddr::AmpEnvelope).then_some(self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_route_stays_compact_and_copy() {
        assert!(
            std::mem::size_of::<ModRoute>() <= 48,
            "ModRoute grew to {} bytes",
            std::mem::size_of::<ModRoute>()
        );
    }

    #[test]
    fn the_amplitude_envelope_route_has_no_driver() {
        assert_eq!(ModRoute::AMP_ENVELOPE.driver(), None);
        let r = ModRoute::new(Source::Velocity, ParamAddr::Pitch, 0.5).span(12.0);
        assert_eq!(r.driver(), Some(Source::Velocity));
        assert_eq!((r.depth, r.span, r.law), (0.5, 12.0, RouteLaw::Linear));
    }
}
