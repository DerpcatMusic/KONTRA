//! Stable normalized host slots. Capacity comes from Kontakt 8.13.1 standalone;
//! VST3's native published capacity has not been verified.
use moose::params::{
    FloatParam, ParamFlags, ParamInfo, ParamRange, ParamUnit, ParamValueKind, Params,
    SmoothingStyle,
};
use std::sync::LazyLock;

pub(crate) use super::automation_ids::HOST_AUTOMATION_BASE as BASE;
static INFOS: LazyLock<Vec<ParamInfo>> = LazyLock::new(|| {
    (0..crate::sound::HOST_AUTOMATION_SLOTS)
        .map(|address| ParamInfo {
            id: BASE + u32::from(address),
            name: Box::leak(format!("Automation {address}").into_boxed_str()),
            short_name: "Auto",
            group: "Instrument automation",
            range: ParamRange::Linear { min: 0., max: 1. },
            default_plain: 0.,
            flags: ParamFlags::AUTOMATABLE | ParamFlags::CHUNKED,
            unit: ParamUnit::None,
            kind: ParamValueKind::Float,
            midi_map: None,
            midi_channel: None,
        })
        .collect()
});

pub struct HostAutomation {
    values: Box<[FloatParam]>,
}
impl HostAutomation {
    pub fn new() -> Self {
        Self {
            values: INFOS
                .iter()
                .map(|info| FloatParam::new(info, SmoothingStyle::None))
                .collect(),
        }
    }
    pub fn offset_ids(&mut self, base: u32) {
        for value in &mut self.values {
            value.offset_id(base);
        }
    }
    pub(crate) fn address(id: u32) -> Option<u16> {
        let address = id.checked_sub(BASE)?;
        (address < u32::from(crate::sound::HOST_AUTOMATION_SLOTS)).then_some(address as u16)
    }
    fn value(&self, id: u32) -> Option<&FloatParam> {
        self.values.get(usize::from(Self::address(id)?))
    }
}
impl Default for HostAutomation {
    fn default() -> Self {
        Self::new()
    }
}
impl moose::params::__private::Sealed for HostAutomation {}
impl Params for HostAutomation {
    fn param_infos(&self) -> Vec<ParamInfo> {
        self.values.iter().map(FloatParam::info).collect()
    }
    fn param_infos_static() -> Vec<ParamInfo> {
        INFOS.clone()
    }
    fn count(&self) -> usize {
        self.values.len()
    }
    fn get_normalized(&self, id: u32) -> Option<f64> {
        self.get_plain(id)
    }
    fn set_normalized(&self, id: u32, value: f64) {
        self.set_plain(id, value);
    }
    fn get_plain(&self, id: u32) -> Option<f64> {
        self.value(id).map(FloatParam::raw_target)
    }
    fn set_plain(&self, id: u32, value: f64) {
        if let Some(param) = self.value(id) {
            param.set_value(value);
        }
    }
    fn format_value(&self, id: u32, value: f64) -> Option<String> {
        self.value(id).map(|_| format!("{value:.3}"))
    }
    fn parse_value(&self, id: u32, text: &str) -> Option<f64> {
        self.value(id)?;
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }
    fn snap_smoothers(&self) {}
    fn set_sample_rate(&self, _: f64) {}
    fn collect_values(&self) -> (Vec<u32>, Vec<f64>) {
        self.values.iter().map(|p| (p.id(), p.raw_target())).unzip()
    }
    fn restore_values(&self, values: &[(u32, f64)]) {
        for &(id, value) in values {
            self.set_plain(id, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_slots_have_stable_ids_and_include_native_address_2048() {
        let params = crate::plugin::SamplerParams::new();
        let infos = params.param_infos();
        assert_eq!(infos.len(), 2053); // Volume, Attack, Release, Tone plus 2049 slots.
        assert_eq!(
            infos.iter().map(|p| p.id).collect::<Vec<_>>(),
            crate::plugin::SamplerParams::param_infos_static()
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(HostAutomation::address(BASE - 1), None);
        assert_eq!(HostAutomation::address(BASE + 2048), Some(2048));
        assert_eq!(HostAutomation::address(BASE + 2049), None);
        assert_eq!(HostAutomation::address(params.volume.id()), None);
        params.set_plain(BASE + 2048, 0.75);
        params.set_plain(BASE + 2048, f64::NAN);
        assert_eq!(params.get_normalized(BASE + 2048), Some(0.75));
        assert_eq!(params.parse_value(BASE, "inf"), None);
    }

    #[test]
    fn v1_global_host_parameters_keep_ranges_defaults_and_stable_ids() {
        let params = crate::plugin::SamplerParams::new();
        let infos = params.param_infos();
        for (name, id, min, max, default) in [
            ("Attack", 0x95b8fd, 0.0001, 5., 0.002),
            ("Release", 0x36ae7e, 0.001, 10., 0.15),
            ("Tone", 0x62f120, 20., 20_000., 20_000.),
        ] {
            let info = infos
                .iter()
                .find(|p| p.name == name)
                .expect("v1 global host parameter");
            assert_eq!(info.id, id);
            assert!(
                matches!(info.range, ParamRange::Logarithmic { min: a, max: b } if a == min && b == max)
            );
            assert_eq!(info.default_plain, default);
            assert_eq!(HostAutomation::address(id), None);
            params.set_plain(id, min);
            let restored = crate::plugin::SamplerParams::new();
            let (ids, values) = params.collect_values();
            restored.restore_values(&ids.into_iter().zip(values).collect::<Vec<_>>());
            assert_eq!(restored.get_plain(id), Some(min));
        }
        assert_eq!(params.volume.id(), 0xe0698f);
        for address in 0..crate::sound::HOST_AUTOMATION_SLOTS {
            assert_eq!(
                infos
                    .iter()
                    .filter(|p| p.id == BASE + u32::from(address))
                    .count(),
                1
            );
        }
    }
}
