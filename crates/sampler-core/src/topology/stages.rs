//! Width-preserving authored core-law stages. Native sampler laws are separate.
use crate::dsp::control::{ControlRamp, PreparedParameter};
use crate::dsp::svf::{core_one_pole_coefficient, core_one_pole_history, one_pole_sample};
use crate::{ControlDomain, ControlId, ControlRange, Error, Prepared};
use sampler_ir as ir;

pub(crate) enum Stage {
    Gain {
        gain: PreparedParameter,
    },
    OnePole {
        cutoff: PreparedParameter,
        high: bool,
        history: usize,
        channels: usize,
    },
}
fn parameter(
    plan: &Prepared,
    definition: &ir::SignalParameter,
    parameters: &mut Vec<ControlRange>,
    domains: &mut Vec<(ControlId, ControlDomain)>,
    valid: impl Fn(f64) -> bool,
) -> Result<PreparedParameter, Error> {
    if !valid(definition.constant) {
        return Err(Error::InvalidInput);
    }
    let Some(key) = &definition.control else {
        return Ok(PreparedParameter::Constant(definition.constant));
    };
    let id = crate::lower::ir_control_id(key);
    let domain = plan.controls[plan.control_index(id)?].domain;
    let ControlDomain::Real { min, max } = domain else {
        return Err(Error::InvalidInput);
    };
    if !valid(min) || !valid(max) {
        return Err(Error::InvalidInput);
    }
    plan.real_control(id)?;
    let lane = parameters.len();
    parameters.push(ControlRange {
        control: id,
        low: min,
        high: max,
        ramp_frames: 0,
    });
    domains.push((id, domain));
    Ok(PreparedParameter::Control(lane))
}
impl Stage {
    pub(crate) fn compile(
        plan: &Prepared,
        definition: &ir::ProcessingSignalNode,
        channels: usize,
        histories: &mut usize,
        parameters: &mut Vec<ControlRange>,
        domains: &mut Vec<(ControlId, ControlDomain)>,
    ) -> Result<(Self, u32), Error> {
        let finite = |value: f64| value.is_finite() && value.abs() <= f64::from(f32::MAX);
        match definition {
            ir::ProcessingSignalNode::Gain(gain) => {
                match gain.arithmetic {
                    ir::ChannelGainArithmetic::CoreGainF64ThenF32 => {}
                }
                Ok((
                    Self::Gain {
                        gain: parameter(plan, &gain.gain, parameters, domains, finite)?,
                    },
                    0,
                ))
            }
            ir::ProcessingSignalNode::OnePole(filter) => {
                match filter.arithmetic {
                    ir::ChannelOnePoleArithmetic::CoreOnePoleF64 => {}
                }
                let valid =
                    |value: f64| finite(value) && value > 0. && value < f64::from(plan.rate) * 0.5;
                let cutoff = parameter(plan, &filter.cutoff_hz, parameters, domains, valid)?;
                let history = *histories;
                *histories = history.checked_add(channels).ok_or(Error::Capacity)?;
                Ok((
                    Self::OnePole {
                        cutoff,
                        high: filter.mode == ir::ChannelOnePoleMode::HighPass,
                        history,
                        channels,
                    },
                    filter.tail_frames,
                ))
            }
            ir::ProcessingSignalNode::Matrix(_) => Err(Error::InvalidInput),
        }
    }
    pub(crate) fn render(
        &self,
        block: &mut [f32],
        channels: usize,
        frames: usize,
        stride: usize,
        histories: &mut [f64],
        parameters: &[ControlRamp],
        rate: u32,
        at: u64,
    ) -> Result<(), Error> {
        let end = channels
            .checked_sub(1)
            .and_then(|c| c.checked_mul(stride))
            .and_then(|n| n.checked_add(frames))
            .ok_or(Error::Capacity)?;
        if frames > stride || block.len() < end {
            return Err(Error::InvalidInput);
        }
        at.checked_add(frames.saturating_sub(1) as u64)
            .ok_or(Error::ClockOverflow)?;
        match *self {
            Self::Gain { gain } => {
                for frame in 0..frames {
                    let gain = gain.value(parameters, at + frame as u64, None);
                    for channel in 0..channels {
                        let value = (f64::from(block[channel * stride + frame]) * gain) as f32;
                        if !value.is_finite() {
                            return Err(Error::InvalidInput);
                        }
                        block[channel * stride + frame] = value;
                    }
                }
            }
            Self::OnePole {
                cutoff,
                high,
                history,
                channels: expected,
            } => {
                if channels != expected {
                    return Err(Error::InvalidInput);
                }
                let end = history.checked_add(channels).ok_or(Error::Capacity)?;
                let state = histories.get_mut(history..end).ok_or(Error::InvalidInput)?;
                let mut previous = None;
                let mut b = 0.;
                for frame in 0..frames {
                    let hz = cutoff.value(parameters, at + frame as u64, None);
                    if previous != Some(hz) {
                        b = core_one_pole_coefficient(f64::from(rate), hz);
                        previous = Some(hz);
                    }
                    for channel in 0..channels {
                        let input = f64::from(block[channel * stride + frame]);
                        let (output, next) = one_pole_sample(input, state[channel], b, high);
                        let output = output as f32;
                        if !next.is_finite() || !output.is_finite() {
                            return Err(Error::InvalidInput);
                        }
                        state[channel] = next;
                        block[channel * stride + frame] = output;
                    }
                }
                if frames != 0 {
                    for value in state {
                        *value = core_one_pole_history(*value);
                    }
                }
            }
        }
        Ok(())
    }
}
