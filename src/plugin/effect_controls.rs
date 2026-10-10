use super::*;
use crate::sound::effect_controls::{EffectKey, EffectSnapshot};
use sampler_core::ParameterAddress;

#[derive(Default)]
pub(super) struct EffectControls {
    generation: u64,
    values: Arc<[EffectCell]>,
}

struct EffectCell {
    address: ParameterAddress,
    value: AtomicU64,
}

impl PartView {
    pub(crate) fn effect_snapshot(&self, key: EffectKey) -> Option<&EffectSnapshot> {
        self.effects.iter().find(|effect| effect.key == key)
    }
}

impl PartShared {
    pub(super) fn publish_effect_controls(
        &self,
        generation: u64,
        part: &mut CorePart,
        instrument: Option<&sampler_ir::Instrument>,
    ) -> Arc<[EffectSnapshot]> {
        let descriptors = part.effect_descriptors();
        *self.effect_controls.lock_unpoisoned() = EffectControls {
            generation,
            values: descriptors
                .iter()
                .map(|d| EffectCell {
                    address: d.address,
                    value: AtomicU64::new(
                        part.effect_value(d.address)
                            .expect("registered parameter owner")
                            .to_bits(),
                    ),
                })
                .collect::<Vec<_>>()
                .into(),
        };
        if let Some(ingress) = part.ui_controls.as_mut() {
            ingress.parameter_generation = generation;
        }
        instrument.map_or_else(Arc::default, |instrument| {
            crate::sound::effect_controls::snapshots(
                instrument,
                &descriptors,
                part.effect_sample_rate(),
            )
        })
    }

    pub(crate) fn refresh_effect_controls(&self, value: impl Fn(ParameterAddress) -> Option<f64>) {
        if let Ok(controls) = self.effect_controls.try_lock() {
            if controls.generation != self.generation.load(Ordering::Acquire) {
                return;
            }
            let mut changed = false;
            for cell in controls.values.iter() {
                if let Some(value) = value(cell.address).filter(|value| value.is_finite()) {
                    let bits = value.to_bits();
                    if cell.value.load(Ordering::Relaxed) != bits {
                        cell.value.store(bits, Ordering::Relaxed);
                        changed = true;
                    }
                }
            }
            if changed {
                self.scalar_revision.fetch_add(1, Ordering::Release);
            }
        }
    }
}

impl Shared {
    pub(crate) fn effect_value_at(
        &self,
        slot: usize,
        generation: u64,
        address: ParameterAddress,
    ) -> Option<f64> {
        let part = self.part(slot)?;
        let controls = part.effect_controls.lock_unpoisoned();
        if controls.generation != generation
            || part.generation.load(Ordering::Acquire) != generation
        {
            return None;
        }
        controls
            .values
            .iter()
            .find(|cell| cell.address == address)
            .map(|cell| f64::from_bits(cell.value.load(Ordering::Relaxed)))
    }

    pub(crate) fn set_effect_value_at(
        &self,
        slot: usize,
        generation: u64,
        address: ParameterAddress,
        value: f64,
    ) -> bool {
        let Some(part) = self.part(slot) else {
            return false;
        };
        let mut ingress = part.ingress.lock_unpoisoned();
        if part.generation.load(Ordering::Acquire) != generation {
            return false;
        }
        ingress.as_mut().is_some_and(|ingress| {
            ingress.parameter_generation == generation && ingress.submit_effect(address, value)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::{
        ControlDefinition, ControlDomain, ControlValue, Limits, ParameterRegistry, Prepared,
        Runtime,
    };

    fn part() -> (
        CorePart,
        sampler_ir::Instrument,
        Vec<sampler_core::ParameterDescriptor>,
    ) {
        let (instrument, descriptors) = crate::sound::effect_controls::tests::fixture();
        let mut registry = ParameterRegistry::default();
        for descriptor in &descriptors {
            registry.register(descriptor.clone()).unwrap();
        }
        let plan = Prepared::new(
            48000,
            vec![sampler_core::Pcm::new(48000, vec![[1.; 2]; 512].into_boxed_slice()).unwrap()],
            vec![sampler_core::Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: Some(60),
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Default::default(),
                playback: Default::default(),
            }],
            1,
        )
        .unwrap()
        .with_controls(
            descriptors
                .iter()
                .map(|d| ControlDefinition {
                    id: d.control,
                    domain: ControlDomain::Real {
                        min: d.range[0],
                        max: d.range[1],
                    },
                    default: ControlValue::Real(d.default),
                })
                .collect(),
        )
        .unwrap()
        .with_parameter_registry(registry)
        .unwrap()
        .with_voice_chains(
            vec![
                sampler_core::VoiceChain::new(
                    vec![],
                    vec![sampler_core::Processor::ControlGain(
                        sampler_core::ControlRange {
                            control: descriptors[0].control,
                            low: 0.,
                            high: 1.,
                            ramp_frames: 0,
                        },
                    )],
                    0,
                )
                .unwrap(),
            ],
            vec![Some(0)],
        )
        .unwrap();
        let limits = Limits::for_plan(&plan, 8, 4);
        let runtime = Runtime::new(plan, limits).unwrap();
        (
            CorePart::new(runtime, MixTree::instrument("effect fixture")).unwrap(),
            instrument,
            descriptors,
        )
    }

    fn install(
        params: &SamplerParams,
        part: &mut CorePart,
        instrument: &sampler_ir::Instrument,
        generation: u64,
    ) {
        let atoms = params.shared.part(0).unwrap();
        atoms.generation.store(generation, Ordering::Release);
        let effects = atoms.publish_effect_controls(generation, part, Some(instrument));
        *atoms.ingress.lock_unpoisoned() = part.ui_controls.take();
        let mut view = params.shared.view.lock_unpoisoned();
        view.parts[0].generation = generation;
        view.parts[0].effects = effects;
        view.parts[0].attempted = Some((
            "effect fixture".into(),
            0,
            String::new(),
            48000f64.to_bits(),
            false,
            false,
            -1,
            Streaming::Auto,
        ));
    }

    #[test]
    fn loaded_part_publishes_an_immutable_selected_effect_snapshot() {
        let (mut part, instrument, descriptors) = part();
        let params = SamplerParams::new();
        install(&params, &mut part, &instrument, 7);
        let view = params.shared.view.lock_unpoisoned();
        let snapshot = view.parts[0]
            .effect_snapshot(EffectKey {
                scope: sampler_core::ParameterScope::Group(3),
                chain: 0,
                processor: 1,
            })
            .expect("loader must publish the selected processor's prepared metadata");
        assert_eq!(&*snapshot.descriptors, descriptors.as_slice());
        assert_eq!(snapshot.roles[0].1, descriptors[0].address);
        let held = view.parts[0].effects.clone();
        drop(view);
        params.shared.view.lock_unpoisoned().parts[0] = PartView::default();
        assert_eq!(
            &*held[1].descriptors,
            descriptors.as_slice(),
            "retired UI snapshots remain immutable"
        );
    }

    #[test]
    fn effect_addresses_admit_only_live_registered_finite_values() {
        let (mut part, instrument, descriptors) = part();
        let params = SamplerParams::new();
        install(&params, &mut part, &instrument, 7);
        let address = descriptors[0].address;
        assert_eq!(params.shared.effect_value_at(0, 7, address), Some(0.5));
        assert!(!params.shared.set_effect_value_at(0, 6, address, 0.75));
        assert!(!params.shared.set_effect_value_at(0, 7, address, f64::NAN));
        assert!(!params.shared.set_effect_value_at(0, 7, address, 1.01));
        assert!(!params.shared.set_effect_value_at(
            0,
            7,
            ParameterAddress {
                node: 999,
                ..address
            },
            0.75
        ));
        assert!(!params.shared.set_effect_value_at(
            0,
            7,
            ParameterAddress {
                scope: sampler_core::ParameterScope::Plan,
                ..address
            },
            0.75
        ));
        let mut core = V2Core::with_parts(1, 48000.);
        part.epoch = 7;
        core.install(0, Some(Box::new(part)));
        assert!(params.shared.set_effect_value_at(0, 7, address, 0.75));
        assert_eq!(
            params.shared.effect_value_at(0, 7, address),
            Some(0.5),
            "queue admission is not an audio acknowledgement"
        );
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                core.render(64);
            }),
            0
        );
        let atoms = params.shared.part(0).unwrap();
        atoms.refresh_effect_controls(|address| core.effect_value(0, address));
        assert_eq!(params.shared.effect_value_at(0, 7, address), Some(0.75));
        for _ in 0..256 {
            assert!(params.shared.set_effect_value_at(0, 7, address, 0.5));
        }
        assert!(
            !params.shared.set_effect_value_at(0, 7, address, 0.1),
            "full queue rejects without publishing a value"
        );
        atoms.generation.store(8, Ordering::Release);
        assert!(params.shared.effect_value_at(0, 7, address).is_none());
        assert!(
            params.shared.effect_value_at(0, 8, address).is_none(),
            "old ingress cannot serve a new generation"
        );
        assert!(!params.shared.set_effect_value_at(0, 8, address, 0.25));
    }

    #[test]
    fn effect_lane_edits_reach_the_real_processor_without_heap_work() {
        let (mut part, instrument, descriptors) = part();
        let params = SamplerParams::new();
        install(&params, &mut part, &instrument, 7);
        part.epoch = 7;
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(part)));
        core.event(
            0,
            crate::sound::event::Event::NoteOn {
                note: crate::sound::event::HostNote {
                    port: 0,
                    channel: 0,
                    key: 60,
                    id: 1,
                    clap: true,
                },
                velocity: 1.,
                tune: 0.,
            },
        );
        let before = core.render(64).buses[0][0][32].abs();
        assert!(
            before > 0.1,
            "the registered processor fixture must be audible"
        );
        assert!(
            params
                .shared
                .set_effect_value_at(0, 7, descriptors[0].address, 0.25)
        );
        let mut after = 0.;
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                after = core.render(64).buses[0][0][32].abs();
            }),
            0
        );
        assert!(
            (after / before - 0.5).abs() < 1e-5,
            "typed DSP edit must change the actual processor output"
        );
        assert!(
            params.shared.part(0).unwrap().control_values().is_empty(),
            "DSP addresses never enter the widget-value table"
        );
    }

    #[test]
    fn effect_lanes_survive_host_recall_without_a_script_or_widget_id() {
        let (mut original, instrument, descriptors) = part();
        original.prepare_persistence(&[], "").unwrap();
        let params = SamplerParams::new();
        params.selection.write().unwrap().parts = vec![Part {
            path: "effect fixture".into(),
            ..Default::default()
        }];
        install(&params, &mut original, &instrument, 7);
        original.epoch = 7;
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(original)));
        assert!(
            params
                .shared
                .set_effect_value_at(0, 7, descriptors[0].address, 0.875)
        );
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                core.render(64);
            }),
            0
        );
        params.capture_ui_controls();
        let host = moose::params::Params::serialize_persist(&params);
        let recalled = SamplerParams::new();
        moose::params::Params::load_persist(&recalled, &host);
        let saved = recalled.selection.read().unwrap().parts[0]
            .script_state
            .clone();
        assert!(
            !saved.is_empty(),
            "script-free DSP owners must enter host persistence"
        );
        let (mut reloaded, instrument, _) = part();
        reloaded.prepare_persistence(&[], &saved).unwrap();
        install(&recalled, &mut reloaded, &instrument, 8);
        assert_eq!(
            recalled
                .shared
                .effect_value_at(0, 8, descriptors[0].address),
            Some(0.875)
        );
    }
}
