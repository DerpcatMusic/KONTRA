//! Stable normalized host slots. Capacity comes from Kontakt 8.13.1 standalone;
//! VST3's native published capacity has not been verified.
use crate::support::MutexExt;
use moose::params::{
    FloatParam, ParamFlags, ParamInfo, ParamRange, ParamUnit, ParamValueKind, ParameterPresentation,
    Params, SmoothingStyle,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

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

/// What a bound host slot shows: its control's name and value text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Slot {
    pub name: String,
    pub group: String,
    /// The normalized window and control the host value is mapped through.
    pub low: f64,
    pub high: f64,
    pub domain: sampler_core::ControlDomain,
    /// Menu entries by value.
    pub labels: Vec<(i64, String)>,
    pub display: Option<(sampler_ui_ir::Display, sampler_ui_ir::Source)>,
}

impl Slot {
    /// The control's value for host value `v`, as the control reads it.
    fn text(&self, v: f64) -> String {
        use sampler_core::ControlDomain as D;
        let p = self.low + (self.high - self.low) * v.clamp(0., 1.);
        match self.domain {
            D::Toggle => if p >= 0.5 { "On" } else { "Off" }.into(),
            D::Integer { min, max } => {
                let n = (min as f64 + (max as f64 - min as f64) * p).round() as i64;
                match self.labels.iter().find(|(value, _)| *value == n) {
                    Some((_, label)) => label.clone(),
                    None => self.number(n as f64, true),
                }
            }
            D::Real { min, max } => self.number(min + (max - min) * p, false),
        }
    }

    fn number(&self, value: f64, integer: bool) -> String {
        match &self.display {
            Some((display, source)) => display.format_value(value, integer, *source),
            None if integer => format!("{value:.0}"),
            None => format!("{value:.2}"),
        }
    }

    /// The host value whose text is `text`.
    fn parse(&self, text: &str) -> Option<f64> {
        use sampler_core::ControlDomain as D;
        let text = text.trim();
        let span = |x: f64, min: f64, max: f64| if max != min { (x - min) / (max - min) } else { 0. };
        let p = match self.domain {
            D::Toggle => match text.to_ascii_lowercase().as_str() {
                "on" | "1" | "true" => 1.,
                "off" | "0" | "false" => 0.,
                _ => return None,
            },
            D::Integer { min, max } => {
                let n = self
                    .labels
                    .iter()
                    .find(|(_, label)| label.eq_ignore_ascii_case(text))
                    .map(|&(value, _)| value as f64)
                    .or_else(|| self.raw(text))?;
                span(n, min as f64, max as f64)
            }
            D::Real { min, max } => span(self.raw(text)?, min, max),
        };
        let v = span(p, self.low, self.high);
        v.is_finite().then(|| v.clamp(0., 1.))
    }

    /// A typed number in display units back to the control's raw units.
    // ponytail: KSP display ratio only; Falcon unit conversions (ms, kHz, dB gain) read as raw.
    fn raw(&self, text: &str) -> Option<f64> {
        let end = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.')))
            .unwrap_or(text.len());
        let shown: f64 = text[..end].parse().ok().filter(|x: &f64| x.is_finite())?;
        Some(match &self.display {
            Some((d, sampler_ui_ir::Source::Ksp { .. })) if d.ratio != 0. => shown * d.ratio,
            _ => shown,
        })
    }
}
pub(crate) type Slots = BTreeMap<u16, Slot>;

/// The bound slots across the rack, first part first: each part's plan
/// routes (saved assignments, script-fixed and NCKP slots) named by the
/// widget they drive.
pub(crate) fn slots(parts: &[super::PartView]) -> Slots {
    let mut slots = Slots::new();
    for (index, part) in parts.iter().enumerate() {
        for binding in part.host_bindings.iter() {
            if slots.contains_key(&binding.address) {
                continue; // ponytail: first part names a shared slot; join names if racks share slots often.
            }
            let found = part.interfaces.iter().find_map(|f| {
                let w = f.widgets.iter().find(|w| {
                    matches!(w.binding, sampler_ui_ir::Binding::Control(id)
                        if id == binding.widget || id == binding.control)
                })?;
                Some((f.source, w))
            });
            let widget = found.map(|(_, w)| w);
            let name = widget
                .and_then(|w| {
                    [w.automation.name.as_deref(), w.text.lines().next(), Some(&w.name)]
                        .into_iter()
                        .flatten()
                        .map(|n| n.trim().trim_start_matches(['$', '%', '~', '?', '@', '!']))
                        .find(|n| !n.is_empty())
                })
                .map_or_else(|| format!("Automation {}", binding.address), str::to_owned);
            let (labels, display) = match found.map(|(source, w)| (&w.kind, source)) {
                Some((sampler_ui_ir::Kind::Menu { items }, _)) => (
                    items
                        .iter()
                        .map(|i| (i64::from(i.value), i.text.clone()))
                        .collect(),
                    None,
                ),
                Some((
                    sampler_ui_ir::Kind::Knob { display, .. }
                    | sampler_ui_ir::Kind::ValueEdit { display, .. },
                    source,
                )) => (Vec::new(), Some((display.clone(), source))),
                _ => (Vec::new(), None),
            };
            let group = if part.active.is_empty() {
                format!("Part {}", index + 1)
            } else {
                part.active.clone()
            };
            slots.insert(
                binding.address,
                Slot {
                    name,
                    group,
                    low: binding.low,
                    high: binding.high,
                    domain: binding.domain,
                    labels,
                    display,
                },
            );
        }
    }
    slots
}

pub struct HostAutomation {
    values: Box<[FloatParam]>,
    /// Bound slots as last told to the host, settling like port names.
    slots: Mutex<crate::routing::Settled<Slots>>,
    revision: AtomicU64,
}
impl HostAutomation {
    pub fn new() -> Self {
        Self {
            values: INFOS
                .iter()
                .map(|info| FloatParam::new(info, SmoothingStyle::None))
                .collect(),
            slots: Mutex::new(crate::routing::Settled::new(Slots::new())),
            revision: AtomicU64::new(0),
        }
    }
    /// Main thread: offer the rack's bound slots; the host hears once they settle.
    pub(crate) fn offer(&self, slots: Slots, now: Instant) {
        if self.slots.lock_unpoisoned().offer(slots, now) {
            self.revision.fetch_add(1, Ordering::Release);
        }
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    /// A bound slot shows its control's name; unbound slots are hidden.
    pub(crate) fn presentation(&self, id: u32) -> Option<ParameterPresentation> {
        let address = Self::address(id)?;
        let slots = self.slots.lock_unpoisoned();
        Some(match slots.published.get(&address) {
            Some(slot) => ParameterPresentation {
                name: slot.name.clone(),
                group: slot.group.clone(),
                ..Default::default()
            },
            None => ParameterPresentation {
                name: format!("Automation {address}"),
                group: "Instrument automation".into(),
                hidden: true,
                available: true,
            },
        })
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
        self.value(id)?;
        let slots = self.slots.lock_unpoisoned();
        Some(match slots.published.get(&Self::address(id)?) {
            Some(slot) => slot.text(value),
            None => format!("{value:.3}"),
        })
    }
    fn parse_value(&self, id: u32, text: &str) -> Option<f64> {
        self.value(id)?;
        if let Some(slot) = self.slots.lock_unpoisoned().published.get(&Self::address(id)?) {
            return slot.parse(text);
        }
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
    fn bound_slots_name_and_read_their_control() {
        use sampler_core::ControlDomain as D;
        let host = HostAutomation::new();
        let slot = |domain, labels: &[(i64, &str)], display| Slot {
            name: "Cutoff".into(),
            group: "Synth".into(),
            low: 0.,
            high: 1.,
            domain,
            labels: labels.iter().map(|&(v, t)| (v, t.into())).collect(),
            display,
        };
        let ksp = sampler_ui_ir::Source::Ksp { slot: 0 };
        let percent = sampler_ui_ir::Display { ratio: 10., unit: "%".into() };
        let slots = Slots::from([
            (3, slot(D::Integer { min: 0, max: 1000 }, &[], Some((percent, ksp)))),
            (4, slot(D::Integer { min: 0, max: 2 }, &[(0, "Saw"), (1, "Square"), (2, "Sine")], None)),
            (5, slot(D::Toggle, &[], None)),
        ]);
        let t = Instant::now();
        host.offer(slots.clone(), t);
        assert_eq!(host.revision(), 0, "settles first");
        host.offer(slots, t + crate::routing::SETTLE);
        assert_eq!(host.revision(), 1);
        let p = host.presentation(BASE + 3).unwrap();
        assert_eq!((p.name.as_str(), p.hidden), ("Cutoff", false));
        assert!(host.presentation(BASE + 6).unwrap().hidden);
        assert_eq!(host.format_value(BASE + 3, 0.5).unwrap(), "50 %");
        assert_eq!(host.parse_value(BASE + 3, "25 %"), Some(0.25));
        assert_eq!(host.format_value(BASE + 4, 1.).unwrap(), "Sine");
        assert_eq!(host.parse_value(BASE + 4, "square"), Some(0.5));
        assert_eq!(host.format_value(BASE + 5, 0.7).unwrap(), "On");
        assert_eq!(host.parse_value(BASE + 5, "off"), Some(0.));
        assert_eq!(host.format_value(BASE + 6, 0.5).unwrap(), "0.500");
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
