//! Universal effect configurations: one superset per family (wave 4); formats fill these in.
pub mod dynamics;
pub mod filter;
pub mod eq;
pub mod timemod;
pub mod ringmod;
pub mod space;
pub mod color;
pub mod tables;

/// How a written `ENGINE_PAR_*` integer (0..=1e6) becomes the value a lane
/// holds: the measured raw -> stored laws of docs/re/kontakt-bindings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    /// `v / 1e6`.
    Identity,
    /// `a + b v / 1e6`.
    Linear { a: f64, b: f64 },
    /// `a r^(v / 1e6)`.
    Exp { a: f64, r: f64 },
    /// `a (v / 1e6)^p`. Kontakt's effect output gains are `a = 16, p = 3`
    /// (unity at 396851); the Gainer's GN_GAIN too (Conflux saves
    /// `$Footer__Volume` 397000 beside a stored gain of 1.00113).
    Pow { a: f64, p: f64 },
    /// Stored values at evenly spaced writes over 0..=1e6, geometric between points.
    Table(&'static [f64]),
    /// FREQn of the legacy EQ: `exp a=20 r=1000` Hz, then the native knob of
    /// `KontaktLegacyEqSettings::frequency_knob`, the domain the band holds.
    LegacyEqFrequency,
}

/// One script-addressable knob of an effect: `set_engine_par(name, ...)` at
/// the processor's slot drives `role` of the lowered stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    pub name: &'static str,
    pub role: crate::ProcessorParameter,
    /// The saved value in the law's (stored) domain: the lane's default.
    pub value: f64,
    pub law: Law,
    /// The stage's value at the law's two ends (write 0, write 1e6) when it is
    /// a linear rescale of the stored value (ms to seconds, -1..1 to 0..1).
    /// `None`: the stage takes the stored value itself.
    pub scale: Option<[f64; 2]>,
}

/// A lane of a UVI unit: the stored value is the unit's own field, over
/// `low..=high` (an exponential law for frequencies). UVI has no engine
/// parameter API; the name only gives the lane an address id, one per role.
/// `None` when the field lies outside the range (the unit keeps it fixed).
pub fn uvi_lane(role: crate::ProcessorParameter, value: f64, low: f64, high: f64) -> Option<Lane> {
    use crate::ProcessorParameter as P;
    let name = match role {
        P::Time => "ENGINE_PAR_DL_TIME",
        P::Feedback => "ENGINE_PAR_DL_FEEDBACK",
        P::Speed => "ENGINE_PAR_CH_SPEED",
        P::Depth => "ENGINE_PAR_CH_DEPTH",
        P::Cutoff => "ENGINE_PAR_LP_CUTOFF",
        P::Frequency => "ENGINE_PAR_HP_CUTOFF",
        P::Size => "ENGINE_PAR_RV_SIZE",
        P::Damping => "ENGINE_PAR_RV_DAMPING",
        P::Colour => "ENGINE_PAR_RV_COLOUR",
        P::Predelay => "ENGINE_PAR_RV_PREDELAY",
        P::Width => "ENGINE_PAR_RV_STEREO",
        P::Gain => "ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN",
        P::Response => "ENGINE_PAR_SEND_EFFECT_DRY_LEVEL",
        _ => return None,
    };
    let law = if matches!(role, P::Cutoff | P::Frequency) {
        Law::Exp { a: low, r: high / low }
    } else {
        Law::Linear { a: low, b: high - low }
    };
    (value.is_finite() && (low..=high).contains(&value)).then(|| Lane::new(name, role, value, law))
}

impl Lane {
    pub const fn new(name: &'static str, role: crate::ProcessorParameter, value: f64, law: Law) -> Self {
        Self { name, role, value, law, scale: None }
    }
    pub const fn scaled(self, ends: [f64; 2]) -> Self {
        Self { scale: Some(ends), ..self }
    }
}
