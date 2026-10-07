use crate::{ControlDomain, ControlId, ControlValue, Error, Prepared};

/// Map a control's declared range to destination units, with a sample-clock ramp.
/// Integer, real and toggle controls share the same value owner; constant domains
/// select `low`. The processor field defines the units and validates its range.
#[derive(Clone, Copy, Debug)]
pub struct ControlRange {
    pub control: ControlId,
    pub low: f64,
    pub high: f64,
    pub ramp_frames: u32,
}
impl ControlRange {
    pub(super) fn valid(self) -> bool {
        self.low.is_finite() && self.high.is_finite() && (self.high - self.low).is_finite()
    }

    pub(super) fn target(self, normalized: f64) -> f64 {
        interpolate(self.low, self.high, normalized)
    }
}

/// A processor parameter, in the units declared by its destination field.
#[derive(Clone, Copy, Debug)]
pub enum Parameter {
    Constant(f64),
    Control(ControlRange),
    /// Immediate event-rate mapping of the retained note's full-resolution value.
    /// Valid in voice scope; a summed bus has no single note-expression owner.
    Expression {
        source: crate::ExpressionSource,
        low: f64,
        high: f64,
    },
}

impl Parameter {
    pub(super) fn bounds(self) -> [f64; 2] {
        match self {
            Self::Constant(value) => [value; 2],
            Self::Control(binding) => [binding.low, binding.high],
            Self::Expression { low, high, .. } => [low, high],
        }
    }

    pub(super) fn valid(self) -> bool {
        match self {
            Self::Constant(value) => value.is_finite(),
            Self::Control(binding) => binding.valid(),
            Self::Expression { low, high, .. } => {
                low.is_finite() && high.is_finite() && (high - low).is_finite()
            }
        }
    }

    pub(super) fn compile(self, bindings: &mut Vec<ControlRange>) -> PreparedParameter {
        match self {
            Self::Constant(value) => PreparedParameter::Constant(value),
            Self::Expression { source, low, high } => {
                PreparedParameter::Expression { source, low, high }
            }
            Self::Control(binding) => {
                let lane = bindings.len();
                bindings.push(binding);
                PreparedParameter::Control(lane)
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum PreparedParameter {
    Constant(f64),
    Control(usize),
    Expression {
        source: crate::ExpressionSource,
        low: f64,
        high: f64,
    },
}
impl PreparedParameter {
    pub fn requires_expression(self) -> bool {
        matches!(self, Self::Expression { .. })
    }
    pub fn value(
        self,
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<&crate::Expression>,
    ) -> f64 {
        match self {
            Self::Constant(value) => value,
            Self::Control(lane) => parameters[lane].value(at),
            Self::Expression { source, low, high } => {
                let expression = expression.expect("prepared voice-scoped parameter");
                let value = match source {
                    crate::ExpressionSource::Pressure => expression.pressure,
                    crate::ExpressionSource::Timbre => expression.timbre,
                };
                interpolate(low, high, f64::from(value) / f64::from(u32::MAX))
            }
        }
    }
}

fn interpolate(low: f64, high: f64, normalized: f64) -> f64 {
    if normalized == 0. {
        low
    } else if normalized == 1. {
        high
    } else {
        (low + (high - low) * normalized).clamp(low.min(high), low.max(high))
    }
}

fn normalized(domain: ControlDomain, value: ControlValue) -> f64 {
    match (domain, value) {
        (ControlDomain::Integer { min, max }, ControlValue::Integer(value)) => {
            let span = i128::from(max) - i128::from(min);
            if span == 0 {
                0.
            } else {
                (i128::from(value) - i128::from(min)) as f64 / span as f64
            }
        }
        (ControlDomain::Real { min, max }, ControlValue::Real(value)) => {
            if min == max {
                0.
            } else if (max - min).is_finite() {
                (value - min) / (max - min)
            } else {
                (value * 0.5 - min * 0.5) / (max * 0.5 - min * 0.5)
            }
        }
        (ControlDomain::Toggle, ControlValue::Toggle(value)) => f64::from(value),
        _ => unreachable!("control values are validated before DSP projection"),
    }
    .clamp(0., 1.)
}

/// One trajectory per prepared binding, shared by all voices in that generation.
/// Evaluation is a function of absolute sample time, never voice/render call count.
#[derive(Clone, Copy)]
pub(crate) struct ControlRamp {
    from: f64,
    target: f64,
    start: u64,
    frames: u32,
}
impl ControlRamp {
    pub(super) fn value(self, at: u64) -> f64 {
        let elapsed = at.saturating_sub(self.start);
        if elapsed >= u64::from(self.frames) {
            self.target
        } else {
            (self.from + (self.target - self.from) * (elapsed as f64 / f64::from(self.frames)))
                .clamp(self.from.min(self.target), self.from.max(self.target))
        }
    }
    fn set(&mut self, at: u64, target: f64, frames: u32) {
        if target != self.target {
            self.from = self.value(at);
            self.target = target;
            self.start = at;
            self.frames = frames;
        }
    }
}

impl Prepared {
    pub(crate) fn validate_dsp_controls(&self) -> Result<(), Error> {
        for binding in self.dsp_bindings.iter().chain(self.buses.parameters.iter()) {
            self.control_index(binding.control)?;
        }
        Ok(())
    }
}

pub(crate) fn initial_parameters(plan: &Prepared, bindings: &[ControlRange]) -> Box<[ControlRamp]> {
    bindings
        .iter()
        .map(|binding| {
            let definition = plan.controls[plan.control_index(binding.control).unwrap()];
            let target = binding.target(normalized(definition.domain, definition.default));
            ControlRamp {
                from: target,
                target,
                start: 0,
                frames: 0,
            }
        })
        .collect()
}

impl super::DspState {
    pub(crate) fn edit_control(
        &mut self,
        plan: &Prepared,
        index: usize,
        value: ControlValue,
        at: u64,
    ) {
        let definition = plan.controls[index];
        edit_parameters(
            &mut self.parameters,
            &plan.dsp_bindings,
            &plan.dsp_controls,
            definition,
            value,
            at,
        );
        edit_parameters(
            &mut self.buses.parameters,
            &plan.buses.parameters,
            &plan.buses.controls,
            definition,
            value,
            at,
        );
    }
}

fn edit_parameters(
    parameters: &mut [ControlRamp],
    bindings: &[ControlRange],
    controls: &[(ControlId, usize)],
    definition: crate::ControlDefinition,
    value: ControlValue,
    at: u64,
) {
    let from = controls.partition_point(|(id, _)| *id < definition.id);
    let until = controls.partition_point(|(id, _)| *id <= definition.id);
    let value = normalized(definition.domain, value);
    for &(_, lane) in &controls[from..until] {
        let binding = bindings[lane];
        parameters[lane].set(at, binding.target(value), binding.ramp_frames);
    }
}
