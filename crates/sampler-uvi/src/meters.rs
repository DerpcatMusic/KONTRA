//! Exact translator bus ownership, separate from native meter display laws.
use sampler_ir as ir;
use sampler_ui_ir::{UviMeterRequest, UviMeterScope};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Program,
    Layer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Availability {
    #[default]
    NotObserved = 0,
    RawStereoObserved = 1,
    NoPhysicalBus = 2,
    AmbiguousBus = 3,
    UnsupportedScope = 4,
    UnsupportedBus = 5,
    InputNotEnabled = 6,
    StalePlan = 7,
    InterestLimit = 8,
    OwnershipUnavailable = 9,
}

impl Availability {
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::RawStereoObserved,
            2 => Self::NoPhysicalBus,
            3 => Self::AmbiguousBus,
            4 => Self::UnsupportedScope,
            5 => Self::UnsupportedBus,
            6 => Self::InputNotEnabled,
            7 => Self::StalePlan,
            8 => Self::InterestLimit,
            9 => Self::OwnershipUnavailable,
            _ => Self::NotObserved,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observation {
    pub availability: Availability,
    pub physical_bus: Option<usize>,
    /// Kontra's raw stereo sample peaks; not a native mono or dB/pixel value.
    pub stereo_peak: Option<[f32; 2]>,
}

impl Observation {
    pub fn unavailable(availability: Availability) -> Self {
        Self {
            availability,
            physical_bus: None,
            stereo_peak: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub element_node: u32,
    pub scope: Scope,
    pub physical_bus: Option<usize>,
    pub unavailable: Availability,
}

/// This ancillary map is not serialized in Instrument caches. No physical
/// index is inferred from source/GUI order, layer ordinal, or parent output.
pub fn bindings(
    text: &str,
    instrument: &ir::Instrument,
    inserts: &[crate::InsertNode],
) -> Vec<Binding> {
    let Ok(doc) = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            nodes_limit: 1 << 20,
            ..Default::default()
        },
    ) else {
        return Vec::new();
    };
    let mut map = BTreeMap::new();
    for node in doc.descendants().filter(|node| node.is_element()) {
        let scope = match node.tag_name().name() {
            "Program" => Scope::Program,
            "Layer" => Scope::Layer,
            _ => continue,
        };
        let Ok(element_node) = u32::try_from(node.id().get_usize()) else {
            continue;
        };
        map.insert(
            element_node,
            Binding {
                element_node,
                scope,
                physical_bus: None,
                unavailable: Availability::NoPhysicalBus,
            },
        );
    }
    for placed in inserts {
        let Ok(node) = u32::try_from(placed.node) else {
            continue;
        };
        let Some(node) = doc.get_node(roxmltree::NodeId::new(node)) else {
            continue;
        };
        // A per-voice chain can never become a summed layer/program tap.
        let Some(ir::Chain {
            scope: ir::Scope::Bus(bus),
            ..
        }) = instrument.chains.get(placed.chain.0)
        else {
            continue;
        };
        if instrument
            .buses
            .get(bus.0)
            .is_none_or(|b| b.chain != Some(placed.chain))
        {
            continue;
        }
        let Some(owner) = node.ancestors().skip(1).find(|n| {
            matches!(
                n.tag_name().name(),
                "Program" | "Layer" | "Keygroup" | "SamplePlayer" | "AuxEffect"
            )
        }) else {
            continue;
        };
        let Ok(owner) = u32::try_from(owner.id().get_usize()) else {
            continue;
        };
        let Some(binding) = map.get_mut(&owner) else {
            continue;
        };
        match binding.physical_bus {
            Some(previous) if previous != bus.0 => {
                binding.physical_bus = None;
                binding.unavailable = Availability::AmbiguousBus;
            }
            None if binding.unavailable != Availability::AmbiguousBus => {
                binding.physical_bus = Some(bus.0);
                binding.unavailable = Availability::NotObserved;
            }
            _ => {}
        }
    }
    map.into_values().collect()
}

pub fn resolve(bindings: &[Binding], request: UviMeterRequest) -> Result<usize, Availability> {
    if request.bus_id != 0 {
        return Err(Availability::UnsupportedBus);
    }
    let scope = match request.scope {
        UviMeterScope::Program => Scope::Program,
        UviMeterScope::Layer { .. } => Scope::Layer,
        _ => return Err(Availability::UnsupportedScope),
    };
    let binding = bindings
        .binary_search_by_key(&request.element_node, |binding| binding.element_node)
        .ok()
        .and_then(|index| bindings.get(index))
        .filter(|binding| binding.scope == scope)
        .ok_or(Availability::OwnershipUnavailable)?;
    binding.physical_bus.ok_or(binding.unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_uses_exact_xml_chain_and_rejects_collapsed_voice_ambiguous_and_other_buses() {
        let xml = "<UVI4><Program><Inserts><Gain/></Inserts><Layers><Layer><Inserts><Gain/></Inserts><Keygroups><Keygroup><Inserts><Gain/></Inserts></Keygroup></Keygroups></Layer><Layer/></Layers></Program></UVI4>";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let owners: Vec<_> = doc
            .descendants()
            .filter(|n| matches!(n.tag_name().name(), "Program" | "Layer" | "Keygroup"))
            .map(|n| n.id().get_usize() as u32)
            .collect();
        let gains: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("Gain"))
            .map(|n| n.id().get_usize())
            .collect();
        let mut instrument = ir::Instrument::default();
        for index in 0..2 {
            instrument.chains.push(ir::Chain {
                scope: ir::Scope::Bus(ir::BusRef(index)),
                pre_amplitude: vec![],
                post_amplitude: vec![],
            });
            instrument.buses.push(ir::Bus {
                name: String::new(),
                chain: Some(ir::ChainRef(index)),
                sends: vec![],
                output: ir::Output::Master,
                gain: ir::Gain::UNITY,
            });
        }
        instrument.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![],
            post_amplitude: vec![],
        });
        let mut inserts: Vec<_> = gains
            .iter()
            .enumerate()
            .map(|(index, node)| crate::InsertNode {
                node: *node,
                chain: ir::ChainRef(index),
                first: 0,
                count: 1,
            })
            .collect();
        let map = bindings(xml, &instrument, &inserts);
        let req = |node, scope| UviMeterRequest {
            element_id: 999,
            element_node: node,
            scope,
            bus_id: 0,
            use_output: true,
            display_as_mono: false,
        };
        assert_eq!(resolve(&map, req(owners[0], UviMeterScope::Program)), Ok(0));
        assert_eq!(
            resolve(&map, req(owners[1], UviMeterScope::Layer { ordinal: 999 })),
            Ok(1)
        );
        assert_eq!(
            resolve(&map, req(owners[3], UviMeterScope::Layer { ordinal: 1 })),
            Err(Availability::NoPhysicalBus)
        );
        assert_eq!(
            resolve(&map, req(owners[2], UviMeterScope::Keygroup)),
            Err(Availability::UnsupportedScope)
        );
        let mut other = req(owners[0], UviMeterScope::Program);
        other.bus_id = 1;
        assert_eq!(resolve(&map, other), Err(Availability::UnsupportedBus));
        inserts.push(crate::InsertNode {
            node: gains[0],
            chain: ir::ChainRef(1),
            first: 0,
            count: 1,
        });
        let map = bindings(xml, &instrument, &inserts);
        assert_eq!(
            resolve(&map, req(owners[0], UviMeterScope::Program)),
            Err(Availability::AmbiguousBus)
        );
    }

    #[test]
    fn attached_script_carries_real_translation_ownership_without_an_audio_host() {
        let text = "<UVI4><Program><Inserts><OnePole Freq='1000'/></Inserts><EventProcessors><ScriptProcessor><script><![CDATA[AudioMeter('main',Program,true,0,true)]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let (instrument, locations, groups, inserts) =
            crate::translate_full(text, crate::Source::Bank).unwrap();
        let mut translated = crate::Translated {
            instrument,
            locations,
            groups,
            inserts,
            bank: None,
            dir: None,
            text: text.into(),
            lua: crate::script::Scripts::default(),
        };
        let attached = translated
            .attach_script(48000, crate::script::Config::default())
            .unwrap()
            .unwrap();
        let request = attached
            .interface
            .widgets
            .iter()
            .find_map(|widget| widget.uvi_meter_request)
            .unwrap();
        let bus = resolve(&attached.meter_buses, request).unwrap();
        assert_eq!(bus, 0);
        assert_eq!(
            translated.instrument.buses[bus].chain,
            Some(ir::ChainRef(0))
        );
        assert!(
            attached
                .interface
                .unsupported
                .iter()
                .any(|item| item.feature == "AudioMeter element"),
            "raw identity binding does not establish native display support"
        );
    }

    #[test]
    fn an_auxiliary_bus_never_becomes_the_program_default_tap() {
        let xml = "<UVI4><Program><Auxs><AuxEffect><Inserts><OnePole Freq='1000'/></Inserts></AuxEffect></Auxs></Program></UVI4>";
        let (instrument, _, _, inserts) = crate::translate_full(xml, crate::Source::Bank).unwrap();
        assert!(!instrument.buses.is_empty());
        let map = bindings(xml, &instrument, &inserts);
        assert_eq!(map.len(), 1);
        assert_eq!(map[0].physical_bus, None);
        assert_eq!(map[0].unavailable, Availability::NoPhysicalBus);
    }
}
