//! Immutable prepared effect metadata; DSP addresses never become widget IDs.
use sampler_core::{ParameterAddress, ParameterDescriptor, ParameterScope};
use sampler_ir as ir;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EffectKey {
    pub(crate) scope: ParameterScope,
    pub(crate) chain: usize,
    pub(crate) processor: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EffectSnapshot {
    pub(crate) key: EffectKey,
    pub(crate) descriptors: Arc<[ParameterDescriptor]>,
    pub(crate) processor: ir::Processor,
    pub(crate) sample_rate: u32,
    pub(crate) roles: Arc<[(ir::ProcessorParameter, ParameterAddress)]>,
}

pub(crate) fn snapshots(
    instrument: &ir::Instrument,
    descriptors: &[ParameterDescriptor],
    sample_rate: u32,
) -> Arc<[EffectSnapshot]> {
    instrument
        .chains
        .iter()
        .enumerate()
        .flat_map(|(chain, data)| {
            let scope = match data.scope {
                ir::Scope::Voice => ParameterScope::Voice,
                ir::Scope::Group(group) => ParameterScope::Group(group.0 as u32),
                ir::Scope::Bus(bus) => ParameterScope::Bus(bus.0 as u32),
                ir::Scope::Master => ParameterScope::Plan,
            };
            data.pre_amplitude
                .iter()
                .chain(&data.post_amplitude)
                .enumerate()
                .map(move |(processor, model)| {
                    let mut selected = Vec::new();
                    let mut roles = Vec::new();
                    for binding in instrument
                        .processor_controls
                        .iter()
                        .filter(|binding| binding.chain.0 == chain && binding.index == processor)
                    {
                        let Some(control) = instrument.controls.get(binding.control.0) else {
                            continue;
                        };
                        let owner = sampler_core::lower::ir_control_id(&control.key);
                        let Some(descriptor) = descriptors
                            .iter()
                            .find(|d| d.control == owner && d.address.scope == scope)
                        else {
                            continue;
                        };
                        if !selected
                            .iter()
                            .any(|d: &ParameterDescriptor| d.address == descriptor.address)
                        {
                            selected.push(descriptor.clone());
                        }
                        if !roles.contains(&(binding.parameter, descriptor.address)) {
                            roles.push((binding.parameter, descriptor.address));
                        }
                    }
                    EffectSnapshot {
                        key: EffectKey {
                            scope,
                            chain,
                            processor,
                        },
                        descriptors: selected.into(),
                        processor: *model,
                        sample_rate,
                        roles: roles.into(),
                    }
                })
        })
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sampler_core::{ParameterDisplay, ParameterLaw, ParameterUnit};

    pub(crate) fn fixture() -> (ir::Instrument, Vec<ParameterDescriptor>) {
        let mut instrument = ir::Instrument::default();
        let processor = ir::Processor::LadderLP4(ir::LadderLP4 {
            address: None,
            gain: 0.,
            cutoff: 0.5,
            resonance: 0.2,
            record_version: 1,
        });
        instrument.chains = vec![ir::Chain {
            scope: ir::Scope::Group(ir::GroupRef(3)),
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::UNITY)],
            post_amplitude: vec![processor],
        }];
        let descriptors = [
            ir::ProcessorParameter::Cutoff,
            ir::ProcessorParameter::Resonance,
            ir::ProcessorParameter::Gain,
        ]
        .into_iter()
        .enumerate()
        .map(|(n, role)| {
            let key = format!("fixture-owner-{n}");
            instrument.controls.push(ir::Control {
                key: key.clone(),
                label: "same label for every role".into(),
                value: ir::ControlValue::Continuous {
                    min: 0.,
                    max: 1.,
                    default: 0.5,
                    unit: ir::ControlUnit::None,
                },
                automation: Default::default(),
            });
            instrument.processor_controls.push(ir::ProcessorControl {
                control: ir::ControlRef(n),
                chain: ir::ChainRef(0),
                index: 1,
                parameter: role,
                ramp: ir::Time::ZERO,
            });
            ParameterDescriptor {
                address: ParameterAddress {
                    scope: ParameterScope::Group(3),
                    node: 901 + n as u32,
                    parameter: 17,
                },
                control: sampler_core::lower::ir_control_id(&key),
                name: "same label for every role".into(),
                unit: ParameterUnit::Normalized,
                range: [0., 1.],
                default: 0.5,
                law: ParameterLaw::Linear,
                display: ParameterDisplay {
                    group: "deliberately unrelated text".into(),
                    order: n as u32,
                },
            }
        })
        .collect();
        (instrument, descriptors)
    }

    #[test]
    fn selected_effect_keeps_exact_descriptors_and_roles_across_the_amplitude_split() {
        let (instrument, descriptors) = fixture();
        let effects = snapshots(&instrument, &descriptors, 48000);
        assert_eq!(
            effects.len(),
            2,
            "every prepared processor must publish immutable metadata"
        );
        let selected = &effects[1];
        assert_eq!(
            selected.key,
            EffectKey {
                scope: ParameterScope::Group(3),
                chain: 0,
                processor: 1
            }
        );
        assert_eq!(selected.processor, instrument.chains[0].post_amplitude[0]);
        assert_eq!(&*selected.descriptors, descriptors.as_slice());
        assert_eq!(selected.sample_rate, 48000);
        assert_eq!(
            &*selected.roles,
            &[
                (ir::ProcessorParameter::Cutoff, descriptors[0].address),
                (ir::ProcessorParameter::Resonance, descriptors[1].address),
                (ir::ProcessorParameter::Gain, descriptors[2].address),
            ]
        );
        assert!(effects[0].descriptors.is_empty());
        assert!(effects[0].roles.is_empty());
    }
}
