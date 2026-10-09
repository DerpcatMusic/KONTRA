use super::*;

impl Part {
    pub(crate) fn effect_descriptors(&self) -> Arc<[sampler_core::ParameterDescriptor]> {
        self.ui_controls
            .as_ref()
            .map_or_else(Arc::default, |ingress| ingress.parameters.clone())
    }

    pub(crate) fn effect_sample_rate(&self) -> u32 {
        self.runtime.sample_rate()
    }

    pub(crate) fn effect_value(&self, address: sampler_core::ParameterAddress) -> Option<f64> {
        let plan = self.runtime.active_plan();
        let registry = self.runtime.parameter_registry(plan).ok()?;
        let descriptor = registry.descriptor(registry.resolve(address)?)?;
        self.runtime
            .control_base_value(plan, descriptor.control)
            .ok()
            .map(number)
    }
}

impl ControlIngress {
    pub(crate) fn submit_effect(
        &mut self,
        address: sampler_core::ParameterAddress,
        value: f64,
    ) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(descriptor) = self.parameters.iter().find(|d| d.address == address) else {
            return false;
        };
        if !(descriptor.range[0]..=descriptor.range[1]).contains(&value) {
            return false;
        }
        let Some(definition) = self.definitions.iter().find(|d| d.id == descriptor.control) else {
            return false;
        };
        let value = match definition.domain {
            ControlDomain::Real { min, max } if (min..=max).contains(&value) => {
                ControlValue::Real(value)
            }
            ControlDomain::Integer { min, max }
                if value.fract() == 0.
                    && value >= i64::MIN as f64
                    && value < -(i64::MIN as f64) =>
            {
                let integer = value as i64;
                if !(min..=max).contains(&integer) {
                    return false;
                }
                ControlValue::Integer(integer)
            }
            ControlDomain::Toggle if value == 0. || value == 1. => {
                ControlValue::Toggle(value == 1.)
            }
            _ => return false,
        };
        self.client
            .submit(sampler_core::ControlRequest {
                plan: self.plan,
                expected_revision: None,
                operation: sampler_core::ControlOperation::Edit(
                    vec![ControlWrite {
                        id: descriptor.control,
                        value,
                    }]
                    .into_boxed_slice(),
                ),
            })
            .is_ok()
    }
}
