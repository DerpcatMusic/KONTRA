use super::*;
use crate::sound::effect_controls::{EffectKey, EffectSnapshot};
use sampler_core::ParameterAddress;

impl PartView {
    pub(crate) fn effect_snapshot(&self, key: EffectKey) -> Option<&EffectSnapshot> {
        self.effects.iter().find(|effect| effect.key == key)
    }
}

impl PartShared {
    fn publish_effect_controls(&self, _generation: u64, _part: &mut CorePart, _instrument: Option<&sampler_ir::Instrument>) -> Arc<[EffectSnapshot]> {
        Arc::default()
    }
    pub(crate) fn refresh_effect_controls(&self, _value: impl Fn(ParameterAddress) -> Option<f64>) {}
}

impl Shared {
    pub(crate) fn effect_value_at(&self, _slot: usize, _generation: u64, _address: ParameterAddress) -> Option<f64> {
        None
    }

    pub(crate) fn set_effect_value_at(&self, _slot: usize, _generation: u64, _address: ParameterAddress, _value: f64) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::{ControlDefinition, ControlDomain, ControlValue, Limits, ParameterRegistry, Prepared, Runtime};

    fn part() -> (CorePart, sampler_ir::Instrument, Vec<sampler_core::ParameterDescriptor>) {
        let (instrument, descriptors) = crate::sound::effect_controls::tests::fixture();
        let mut registry = ParameterRegistry::default();
        for descriptor in &descriptors { registry.register(descriptor.clone()).unwrap(); }
        let plan = Prepared::new(48000, vec![], vec![], 0).unwrap()
            .with_controls(descriptors.iter().map(|d| ControlDefinition {
                id: d.control, domain: ControlDomain::Real { min: d.range[0], max: d.range[1] },
                default: ControlValue::Real(d.default),
            }).collect()).unwrap()
            .with_parameter_registry(registry).unwrap();
        let limits = Limits::for_plan(&plan, 8, 0);
        let runtime = Runtime::new(plan, limits).unwrap();
        (CorePart::new(runtime, MixTree::instrument("effect fixture")).unwrap(), instrument, descriptors)
    }

    fn install(params: &SamplerParams, part: &mut CorePart, instrument: &sampler_ir::Instrument, generation: u64) {
        let atoms = params.shared.part(0).unwrap();
        atoms.generation.store(generation, Ordering::Release);
        let effects = atoms.publish_effect_controls(generation, part, Some(instrument));
        *atoms.ingress.lock().unwrap() = part.ui_controls.take();
        let mut view = params.shared.view.lock().unwrap();
        view.parts[0].generation = generation;
        view.parts[0].effects = effects;
        view.parts[0].attempted = Some(("effect fixture".into(), 0, String::new(), 48000f64.to_bits(), false, false, -1, Streaming::Auto));
    }

    #[test]
    fn loaded_part_publishes_an_immutable_selected_effect_snapshot() {
        let (mut part, instrument, descriptors) = part();
        let params = SamplerParams::new();
        install(&params, &mut part, &instrument, 7);
        let view = params.shared.view.lock().unwrap();
        let snapshot = view.parts[0].effect_snapshot(EffectKey { scope: sampler_core::ParameterScope::Group(3), chain: 0, processor: 1 })
            .expect("loader must publish the selected processor's prepared metadata");
        assert_eq!(&*snapshot.descriptors, descriptors.as_slice());
        assert_eq!(snapshot.roles[0].1, descriptors[0].address);
        let held = view.parts[0].effects.clone();
        drop(view);
        params.shared.view.lock().unwrap().parts[0] = PartView::default();
        assert_eq!(&*held[1].descriptors, descriptors.as_slice(), "retired UI snapshots remain immutable");
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
        assert!(!params.shared.set_effect_value_at(0, 7, ParameterAddress { node: 999, ..address }, 0.75));
        let mut core = V2Core::with_parts(1, 48000.);
        part.epoch = 7;
        core.install(0, Some(Box::new(part)));
        assert!(params.shared.set_effect_value_at(0, 7, address, 0.75));
        assert_eq!(params.shared.effect_value_at(0, 7, address), Some(0.5), "queue admission is not an audio acknowledgement");
        assert_eq!(crate::plugin::tests::allocations(|| { core.render(64); }), 0);
        let atoms = params.shared.part(0).unwrap();
        atoms.refresh_effect_controls(|address| core.effect_value(0, address));
        assert_eq!(params.shared.effect_value_at(0, 7, address), Some(0.75));
        for _ in 0..256 { assert!(params.shared.set_effect_value_at(0, 7, address, 0.5)); }
        assert!(!params.shared.set_effect_value_at(0, 7, address, 0.1), "full queue rejects without publishing a value");
        atoms.generation.store(8, Ordering::Release);
        assert!(params.shared.effect_value_at(0, 7, address).is_none());
        assert!(params.shared.effect_value_at(0, 8, address).is_none(), "old ingress cannot serve a new generation");
        assert!(!params.shared.set_effect_value_at(0, 8, address, 0.25));
    }

    #[test]
    fn effect_lanes_survive_host_recall_without_a_script_or_widget_id() {
        let (mut original, instrument, descriptors) = part();
        original.prepare_persistence(&[], "").unwrap();
        let params = SamplerParams::new();
        params.selection.write().unwrap().parts = vec![Part { path: "effect fixture".into(), ..Default::default() }];
        install(&params, &mut original, &instrument, 7);
        original.epoch = 7;
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(original)));
        assert!(params.shared.set_effect_value_at(0, 7, descriptors[0].address, 0.875));
        assert_eq!(crate::plugin::tests::allocations(|| { core.render(64); }), 0);
        params.capture_ui_controls();
        let host = moose::params::Params::serialize_persist(&params);
        let recalled = SamplerParams::new();
        moose::params::Params::load_persist(&recalled, &host);
        let saved = recalled.selection.read().unwrap().parts[0].script_state.clone();
        assert!(!saved.is_empty(), "script-free DSP owners must enter host persistence");
        let (mut reloaded, instrument, _) = part();
        reloaded.prepare_persistence(&[], &saved).unwrap();
        install(&recalled, &mut reloaded, &instrument, 8);
        assert_eq!(recalled.shared.effect_value_at(0, 8, descriptors[0].address), Some(0.875));
    }
}
