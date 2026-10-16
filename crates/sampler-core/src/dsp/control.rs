use crate::{ControlId, Error, Prepared};
use crate::control::ControlProjection;

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

#[cfg(test)]
std::thread_local! {
    pub(super) static PARAMETER_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static RAMP_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
pub(crate) enum PreparedParameter {
    Constant(f64),
    Control(usize),
    Expression {
        source: crate::ExpressionSource,
        low: f64,
        high: f64,
    },
}
impl PreparedParameter {
    pub(super) fn projected(self, parameters: &[ControlRamp]) -> bool {
        matches!(self, Self::Control(lane) if parameters[lane].modulation.is_some())
    }
    pub fn requires_expression(self) -> bool {
        matches!(self, Self::Expression { .. })
    }
    pub fn value(
        self,
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<&crate::Expression>,
    ) -> f64 {
        #[cfg(test)]
        PARAMETER_READS.with(|n| n.set(n.get() + 1));
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

fn normalized(value: ControlProjection) -> f64 {
    match value {
        ControlProjection::Integer { min, max, value } => {
            let span = i128::from(max) - i128::from(min);
            if span == 0 {
                0.
            } else {
                (i128::from(value) - i128::from(min)) as f64 / span as f64
            }
        }
        ControlProjection::Real { min, max, value } => {
            if min == max {
                0.
            } else if (max - min).is_finite() {
                (value - min) / (max - min)
            } else {
                (value * 0.5 - min * 0.5) / (max * 0.5 - min * 0.5)
            }
        }
        ControlProjection::Toggle(value) => f64::from(value),
    }
    .clamp(0., 1.)
}

/// One trajectory per prepared binding, shared by all voices in that generation.
/// Evaluation is a function of absolute sample time, never voice/render call count.
#[derive(Clone, Copy, Default)]
pub(crate) struct ControlRamp {
    from: f64,
    target: f64,
    start: u64,
    frames: u32,
    modulation: Option<(f64, [f64; 2])>,
}
impl ControlRamp {
    pub(super) fn held(self, at: u64, len: usize) -> Option<f64> {
        let first = self.value(at);
        if at.saturating_sub(self.start) >= u64::from(self.frames) || self.from == self.target {
            return Some(first);
        }
        // A linear ramp plus a held modulation offset and clamp is monotone.
        (first == self.value(at + len.saturating_sub(1) as u64)).then_some(first)
    }

    #[cfg(test)]
    pub(super) fn test_ramp(from: f64, target: f64, start: u64, frames: u32) -> Self {
        Self {
            from,
            target,
            start,
            frames,
            modulation: None,
        }
    }
    pub(crate) fn value(self, at: u64) -> f64 {
        #[cfg(test)]
        RAMP_READS.with(|n| n.set(n.get() + 1));
        let elapsed = at.saturating_sub(self.start);
        let value = if elapsed >= u64::from(self.frames) {
            self.target
        } else {
            (self.from + (self.target - self.from) * (elapsed as f64 / f64::from(self.frames)))
                .clamp(self.from.min(self.target), self.from.max(self.target))
        };
        match self.modulation {
            None => value,
            Some((delta, [low, high])) => (value + delta).clamp(low, high),
        }
    }
    pub(crate) fn add_modulation(&mut self, delta: f64, binding: ControlRange) {
        let sum = self.modulation.map_or(0., |(delta, _)| delta) + delta;
        self.modulation = Some((
            sum,
            [binding.low.min(binding.high), binding.low.max(binding.high)],
        ));
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
        self.voice_modulation
            .validate_dsp_targets(&self.filters, self.dsp_bindings.len())?;
        for binding in self.dsp_bindings.iter().chain(self.buses.parameters.iter()) {
            self.control_index(binding.control)?;
        }
        Ok(())
    }
}

pub(crate) fn initial_parameters(plan: &Prepared, bindings: &[ControlRange]) -> Box<[ControlRamp]> {
    bindings.iter().map(|binding| initial_parameter(plan, binding)).collect()
}
/// Same initialization law for one scoped graph parameter, without allocating
/// a temporary ramp array for controls belonging to excluded regions.
pub(crate) fn initial_parameter(plan: &Prepared, binding: &ControlRange) -> ControlRamp {
    let index = plan.control_index(binding.control).unwrap();
    let target = binding.target(normalized(plan.control_layout.initial(index)));
    ControlRamp { from: target, target, start: 0, frames: 0, modulation: None }
}

impl super::DspState {
    pub(crate) fn edit_control(
        &mut self,
        plan: &Prepared,
        id: ControlId,
        value: ControlProjection,
        at: u64,
    ) {
        edit_parameters(
            &mut self.parameters,
            &plan.dsp_bindings,
            &plan.dsp_controls,
            id,
            value,
            at,
        );
        edit_parameters(
            &mut self.buses.parameters,
            &plan.buses.parameters,
            &plan.buses.controls,
            id,
            value,
            at,
        );
    }
}

fn edit_parameters(
    parameters: &mut [ControlRamp],
    bindings: &[ControlRange],
    controls: &[(ControlId, usize)],
    control: ControlId,
    value: ControlProjection,
    at: u64,
) {
    let from = controls.partition_point(|(id, _)| *id < control);
    let until = controls.partition_point(|(id, _)| *id <= control);
    let value = normalized(value);
    for &(_, lane) in &controls[from..until] {
        let binding = bindings[lane];
        parameters[lane].set(at, binding.target(value), binding.ramp_frames);
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn typed_projection_retains_full_width_extreme_equal_and_signed_zero_laws() {
        for (projection,expected) in [
            (ControlProjection::Integer {min:i64::MIN,max:i64::MAX,value:i64::MIN},0.0f64),
            (ControlProjection::Integer {min:i64::MIN,max:i64::MAX,value:i64::MAX},1.),
            (ControlProjection::Integer {min:i64::MIN,max:i64::MAX,value:0},0.5),
            (ControlProjection::Integer {min:42,max:42,value:42},0.),
            (ControlProjection::Real {min:-f64::MAX,max:f64::MAX,value:0.},0.5),
            (ControlProjection::Real {min:-f64::MAX,max:f64::MAX,value:f64::MAX},1.),
            (ControlProjection::Real {min:0.,max:1.,value:-0.},-0.),
            (ControlProjection::Real {min:0.25,max:0.25,value:0.25},0.),
            (ControlProjection::Real {min:f64::from_bits(1),max:f64::from_bits(2),value:f64::from_bits(2)},1.),
            (ControlProjection::Toggle(false),0.),(ControlProjection::Toggle(true),1.),
        ] { assert_eq!(normalized(projection).to_bits(),expected.to_bits()); }
    }

    #[test]
    fn held_controls_keep_ramp_edges_and_clamped_plateaus() {
        let ramp = ControlRamp::test_ramp(0., 2., 10, 100);
        assert_eq!(ramp.held(0, 10), Some(0.));
        assert_eq!(ramp.held(10, 32), None);
        assert_eq!(ramp.held(100, 32), None);
        assert_eq!(ramp.held(110, 32), Some(2.));
        let projected = ControlRamp {
            modulation: Some((-1., [0., 2.])),
            ..ramp
        };
        assert_eq!(projected.held(10, 32), Some(0.));
        assert_eq!(projected.held(40, 32), None);
        assert_eq!(projected.held(110, 32), Some(1.));
    }

    #[test]
    fn projection_clamps_after_the_base_ramp_and_ordered_route_sum() {
        let base = ControlRamp {
            from: 0.,
            target: 2.,
            start: 0,
            frames: 100,
            modulation: None,
        };
        let binding = ControlRange {
            control: ControlId(1),
            low: 0.,
            high: 2.,
            ramp_frames: 0,
        };
        let mut projected = base;
        projected.add_modulation(-1., binding);
        assert_eq!(projected.value(25), 0.);
        assert_eq!(projected.value(75), 0.5);
        assert_eq!(base.value(75), 1.5);
        projected.add_modulation(1., binding);
        assert_eq!(projected.value(25), base.value(25));
        let mut projected = base;
        for delta in [1., 2f64.powi(-54), -1.] {
            projected.add_modulation(delta, binding);
        }
        assert_eq!(
            projected.value(0),
            0.,
            "saved route order cannot be reassociated"
        );
    }
}
