//! Prepared, note-scoped event-rate modulation. These routes are evaluated at
//! exact expression event boundaries, never implicitly demoted to host block rate.
use crate::{Error, Expression, PlanId, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpressionSource {
    Pressure,
    Timbre,
}

/// Endpoint values carry destination-specific units and combination rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Destination {
    /// Multiply linear gain by the mapped factor; endpoints must be in 0..=1.
    LinearGain { zero: f64, one: f64 },
    /// Add stereo balance in -1..=1; the final sum saturates to that domain.
    StereoBalance { zero: f64, one: f64 },
    /// Add semitones. Source admission and live changes enforce supported rates.
    PitchSemitones { zero: f64, one: f64 },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    pub source: ExpressionSource,
    pub destination: Destination,
}

/// One shared note-expression program per prepared instrument. No smoothing is
/// implied: mappings are immediate at event time. Audio-rate sources and per-voice
/// programs require their own explicit scope/rate bindings, not a block-rate shim.
#[derive(Default)]
pub struct Modulation {
    routes: Box<[Route]>,
}
impl Modulation {
    /// Compile off audio with an explicit authored-route work budget. Identity
    /// mappings are removed; remaining routes execute in authored order.
    pub fn new(mut routes: Vec<Route>, max_routes: usize) -> Result<Self, Error> {
        if routes.len() > max_routes {
            return Err(Error::Capacity);
        }
        for route in &routes {
            let (zero, one, bounds) = match route.destination {
                Destination::LinearGain { zero, one } => (zero, one, Some(0.0..=1.0)),
                Destination::StereoBalance { zero, one } => (zero, one, Some(-1.0..=1.0)),
                Destination::PitchSemitones { zero, one } => (zero, one, None),
            };
            if !zero.is_finite()
                || !one.is_finite()
                || bounds.is_some_and(|bounds| !bounds.contains(&zero) || !bounds.contains(&one))
            {
                return Err(Error::InvalidInput);
            }
        }
        routes.retain(|route| {
            !matches!(
                route.destination,
                Destination::LinearGain {
                    zero: 1.0,
                    one: 1.0
                } | Destination::StereoBalance {
                    zero: 0.0,
                    one: 0.0
                } | Destination::PitchSemitones {
                    zero: 0.0,
                    one: 0.0
                }
            )
        });
        Ok(Self {
            routes: routes.into_boxed_slice(),
        })
    }

    fn project(&self, input: Expression) -> Result<Expression, Error> {
        // Canonical inputs were checked at admission/mutation. This private
        // evaluator validates the computed destination values below.
        debug_assert!(input.valid());
        let mut output = input;
        let pressure = f64::from(input.pressure) / f64::from(u32::MAX);
        let timbre = f64::from(input.timbre) / f64::from(u32::MAX);
        for route in &self.routes {
            let source = match route.source {
                ExpressionSource::Pressure => pressure,
                ExpressionSource::Timbre => timbre,
            };
            let interpolate = |zero: f64, one: f64| {
                if zero == one {
                    zero
                } else {
                    (1.0 - source) * zero + source * one
                }
            };
            match route.destination {
                Destination::LinearGain { zero, one } => output.gain *= interpolate(zero, one),
                Destination::StereoBalance { zero, one } => output.pan += interpolate(zero, one),
                Destination::PitchSemitones { zero, one } => {
                    output.pitch_semitones += interpolate(zero, one)
                }
            }
        }
        output.pan = output.pan.clamp(-1.0, 1.0);
        if !output.valid() {
            return Err(Error::InvalidInput);
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RenderedExpression {
    pub gains: [f32; 2],
    pub pitch: f64,
    pub ratio: f64,
}
impl Runtime {
    pub(super) fn modulation_plan(&self, plan: PlanId) -> Option<PlanId> {
        (!self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .modulation
            .routes
            .is_empty())
        .then_some(plan)
    }

    // Mutation/admission boundaries validate raw input. Empty programs need no
    // plan lookup or controller normalization; unchanged destinations reuse cache.
    pub(super) fn project_expression(
        &self,
        program: Option<PlanId>,
        value: Expression,
        previous: Option<&crate::ownership::ExpressionOwner>,
    ) -> Result<RenderedExpression, Error> {
        let projected = match program {
            Some(plan) => self
                .plans
                .get(plan.0)
                .unwrap()
                .prepared
                .modulation
                .project(value)?,
            None => value,
        };
        Ok(RenderedExpression {
            gains: match previous {
                Some(previous)
                    if program.is_none()
                        && value.gain == previous.value.gain
                        && value.pan == previous.value.pan =>
                {
                    previous.rendered.gains
                }
                _ => projected.gains(),
            },
            pitch: projected.pitch_semitones,
            ratio: match previous {
                Some(previous) if previous.rendered.pitch == projected.pitch_semitones => {
                    previous.rendered.ratio
                }
                _ => crate::pitch::ratio(projected.pitch_semitones),
            },
        })
    }
}
