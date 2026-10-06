use crate::{ControlDomain, ControlId, ControlValue, Error, Prepared};

/// Map a control's declared range linearly to amplitude, with a sample-clock ramp.
/// Integer, real and toggle controls share the same value owner; constant domains
/// select `low`. Signed gain supports polarity inversion.
#[derive(Clone, Copy, Debug)]
pub struct GainControl {
    pub control: ControlId,
    pub low: f64,
    pub high: f64,
    pub ramp_frames: u32,
}
impl GainControl {
    pub(super) fn valid(self) -> bool {
        self.low.is_finite() && self.high.is_finite() && (self.high - self.low).is_finite()
    }

    pub(super) fn target(self, normalized: f64) -> f64 {
        if normalized == 0. {
            self.low
        } else if normalized == 1. {
            self.high
        } else {
            (self.low + (self.high - self.low) * normalized)
                .clamp(self.low.min(self.high), self.low.max(self.high))
        }
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
pub(crate) struct GainRamp {
    from: f64,
    target: f64,
    start: u64,
    frames: u32,
}
impl GainRamp {
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
    pub(crate) fn validate_gain_controls(&self) -> Result<(), Error> {
        for binding in self.gain_bindings.iter().chain(self.buses.gains.iter()) {
            self.control_index(binding.control)?;
        }
        Ok(())
    }
}

pub(crate) fn initial_gains(plan: &Prepared, bindings: &[GainControl]) -> Box<[GainRamp]> {
    bindings
        .iter()
        .map(|binding| {
            let definition = plan.controls[plan.control_index(binding.control).unwrap()];
            let target = binding.target(normalized(definition.domain, definition.default));
            GainRamp {
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
        edit_gains(
            &mut self.gains,
            &plan.gain_bindings,
            &plan.gain_controls,
            definition,
            value,
            at,
        );
        edit_gains(
            &mut self.buses.gains,
            &plan.buses.gains,
            &plan.buses.controls,
            definition,
            value,
            at,
        );
    }
}

fn edit_gains(
    gains: &mut [GainRamp],
    bindings: &[GainControl],
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
        gains[lane].set(at, binding.target(value), binding.ramp_frames);
    }
}
