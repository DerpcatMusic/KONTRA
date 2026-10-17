//! Catalog fields bound to the controls the DSP reads. A scripted program's
//! translation gives every insert a fixed processor layout (see `lanes`), so a
//! script's `setParameter` lands on a control-driven stage.
use crate::{InsertNode, script::parameters};
use ir::ProcessorParameter as P;
use sampler_core::{EngineParameterAddress, EngineParameterBinding, EngineParameterLaw};
use sampler_ir as ir;

const PREFIX: &str = "uvi:insert:";

/// Whether `node` sits on a bus (program, layer or aux), not in a keygroup's
/// per-voice chain. Only bus chains get the control-driven stages.
pub(crate) fn bus_scope(node: roxmltree::Node) -> bool {
    !node.ancestors().any(|a| a.has_tag_name("Keygroup"))
}

/// Lane slots of a family element start here, clear of the catalog-position
/// slots below and of the Mix at slot -1.
const LANE_SLOTS: i32 = 1000;

/// Where the `k`-th processor of scripted bus element `node` listens.
pub(crate) fn lane_slot(node: usize, k: usize) -> ir::SlotAddress {
    ir::SlotAddress {
        group: -1,
        slot: LANE_SLOTS + i32::try_from(k).unwrap_or(0),
        generic: i32::try_from(node).unwrap_or(-1),
    }
}

/// Delay, modulation and reverb elements on a bus: they translate to family
/// units (`Processor::TimeMod`/`Space`) whose lanes take script writes.
pub(crate) fn lane_family(node: roxmltree::Node) -> bool {
    lane_kind(node.tag_name().name()) && bus_scope(node)
}

pub(crate) fn lane_kind(kind: &str) -> bool {
    matches!(
        kind,
        "FxDelay" | "FatDelay" | "SimpleDelay" | "StereoDelay" | "PingPongDelay" | "DualDelay" | "DiffuseDelay"
            | "DualDelayX" | "TapeEcho" | "DiffuseDelayNested" | "Buzz" | "Chorus" | "Flanger" | "AnalogChorus"
            | "AnalogFlanger" | "WhiteChorus" | "Ensemble" | "Phaser" | "CrossPhaser" | "Phasor" | "PhasorFilter"
            | "PlainReverb" | "GateReverb" | "FilteredReverb" | "DelayedReverb" | "SparkVerb" | "Bloom" | "Diffusion"
    )
}

/// A script's write of `name` on a family element (`lane_family`), as the
/// lane writes that move its units from the old configuration to the new
/// one. The element is translated alone from its retained fields, before and
/// after the write. `None` when anything but lanes would change: a field
/// with no lane, a value outside its lane, a new report or another layout.
pub(crate) fn retune(
    node: usize,
    kind: &str,
    params: &[(String, String)],
    name: &str,
    value: f64,
) -> Option<Vec<(EngineParameterAddress, i32)>> {
    let alone = |written: Option<f64>| -> Option<(Vec<ir::Processor>, Vec<String>)> {
        let escape = |v: &str| v.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
        let mut xml = format!("<UVI4><Program><Inserts><{kind}");
        // Bypass is the element's Mix, outside its units.
        for (k, v) in params.iter().filter(|(k, _)| k != "Bypass" && (written.is_none() || k != name)) {
            xml += &format!(" {k}=\"{}\"", escape(v));
        }
        if let Some(v) = written {
            xml += &format!(" {name}=\"{v}\"");
        }
        xml += "/></Inserts></Program></UVI4>";
        let (ir, _, _, inserts) = crate::translate_full(&xml, crate::Source::Bank).ok()?;
        let at = inserts.iter().find(|i| i.count > 0)?;
        let units = ir.chains[at.chain.0].pre_amplitude.get(at.first..at.first + at.count)?.to_vec();
        Some((units, ir.unsupported.into_iter().map(|u| u.feature).collect()))
    };
    let (old, old_reports) = alone(None)?;
    let (new, new_reports) = alone(Some(value))?;
    if old.len() != new.len() || new_reports.iter().any(|r| !old_reports.contains(r)) {
        return None;
    }
    let mut out = Vec::new();
    for (k, (old, new)) in old.into_iter().zip(new).enumerate() {
        let slot = lane_slot(node, k);
        let (old, new) = (old.at(slot), new.at(slot));
        if old == new {
            continue;
        }
        let (Some((_, before)), Some((_, after))) = (old.lanes(), new.lanes()) else {
            return None;
        };
        if before.iter().map(|l| l.role).ne(after.iter().map(|l| l.role)) {
            return None;
        }
        // The new configuration must be the old one with the new lane values.
        let mut moved = old;
        for lane in &after {
            match &mut moved {
                ir::Processor::TimeMod(c) => c.set(lane.role, lane.value),
                ir::Processor::Space(c) => c.set(lane.role, lane.value),
                _ => return None,
            }
        }
        if moved != new {
            return None;
        }
        for (b, a) in before.iter().zip(&after).filter(|(b, a)| b.value != a.value) {
            let _ = b;
            let law = match a.law {
                ir::fx::Law::Identity => EngineParameterLaw::Linear { low: 0., high: 1. },
                ir::fx::Law::Linear { a: low, b } => EngineParameterLaw::Linear { low, high: low + b },
                ir::fx::Law::Exp { a: low, r } => EngineParameterLaw::Exponential { low, high: low * r },
                _ => return None,
            };
            let address = EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id(a.name)?,
                group: slot.group,
                slot: slot.slot,
                generic: slot.generic,
            };
            out.push((address, law.normalized_value(a.value).ok()?));
        }
    }
    Some(out)
}

/// Processors of the insert (offset from its first) that follow `name`. Pure
/// in the kind and name; the translator emits the same fixed layouts:
/// ThreeBandShelves is [level, low shelf, high shelf] with the shelves
/// relative to the level, DigitalEq is 16 bands then its overall level, a
/// GainMatrix and an AuxEffect gain are one processor.
pub(crate) fn lanes(kind: &str, name: &str) -> Vec<(usize, P)> {
    match (kind, name) {
        ("Gain", "Volume") | ("AuxEffect", "Gain") => vec![(0, P::Gain)],
        ("OnePole", "Freq") => vec![(0, P::Cutoff)],
        ("ThreeBandShelves", "GainMid") => vec![
            (0, P::GainDecibels),
            (1, P::ReferenceDecibels),
            (2, P::ReferenceDecibels),
        ],
        ("ThreeBandShelves", "GainLow") => vec![(1, P::GainDecibels)],
        ("ThreeBandShelves", "GainHigh") => vec![(2, P::GainDecibels)],
        ("ThreeBandShelves", "FreqLowMid") => vec![(1, P::Frequency)],
        ("ThreeBandShelves", "FreqMidHigh") => vec![(2, P::Frequency)],
        ("GainMatrix", "Gain_1_1") => vec![(0, P::CellLL)],
        ("GainMatrix", "Gain_2_1") => vec![(0, P::CellLR)],
        ("GainMatrix", "Gain_1_2") => vec![(0, P::CellRL)],
        ("GainMatrix", "Gain_2_2") => vec![(0, P::CellRR)],
        ("DigitalEq", "OverallGain") => vec![(16, P::GainDecibels)],
        ("DigitalEq", band) => {
            let digits = band.trim_start_matches(|c: char| !c.is_ascii_digit());
            let field = &band[..band.len() - digits.len()];
            match (field, digits.parse::<usize>()) {
                ("Enabled", Ok(n @ 1..=16)) => vec![(n - 1, P::Enabled)],
                ("Freq", Ok(n @ 1..=16)) => vec![(n - 1, P::Frequency)],
                ("Q", Ok(n @ 1..=16)) => vec![(n - 1, P::Quality)],
                ("Bandwidth", Ok(n @ 1..=16)) => vec![(n - 1, P::Bandwidth)],
                ("Gain", Ok(n @ 1..=16)) => vec![(n - 1, P::GainDecibels)],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// `lanes` for a concrete element. Kinds with a fixed layout only on a bus
/// are left static in a keygroup. A OnePole that tracks keys keeps the
/// translator's folded cutoff.
fn node_lanes(node: roxmltree::Node, name: &str) -> Vec<(usize, P)> {
    let kind = node.tag_name().name();
    if !matches!(kind, "Gain" | "OnePole" | "GainMatrix") && !bus_scope(node) {
        return Vec::new();
    }
    let number = |n: &str, d: f64| node.attribute(n).and_then(|v| v.parse().ok()).unwrap_or(d);
    if kind == "OnePole" && number("KeyTracking", 0.) != 0. {
        return Vec::new();
    }
    lanes(kind, name)
}

/// Whether the translator wraps this element in a Mix of its own, so that
/// `Bypass` reaches the DSP. Elements it does not model have nothing to bypass.
pub(crate) fn bypassable(node: roxmltree::Node) -> bool {
    let number = |n: &str, d: f64| node.attribute(n).and_then(|v| v.parse().ok()).unwrap_or(d);
    match node.tag_name().name() {
        "Gain" | "GainMatrix" | "DigitalEq" | "ThreeBandShelves" | "CompExp" | "OnePole"
        | "TrackDelay" | "EffectRack" => true,
        "WaveShaper" => matches!(number("Mode", 0.) as i64, 6 | 7),
        "Convolver" | "SampledReverb" | "PlainReverb" | "GateReverb" | "FilteredReverb" | "DelayedReverb"
        | "SparkVerb" | "Bloom" | "Diffusion" => bus_scope(node),
        // A program aux wraps its inserts; a rack chain's bypass is its gain.
        "AuxEffect" => node.parent().is_some_and(|p| p.has_tag_name("Auxs")),
        _ => false,
    }
}

/// The catalog fields of `node` that reach the DSP in a scripted program.
/// "BypassGain" marks a rack chain, whose bypass is folded into its gain.
pub(crate) fn live_names(node: roxmltree::Node) -> Vec<&'static str> {
    let kind = node.tag_name().name();
    if kind == "ConstantModulation" {
        // Marked by `promote` when a script can write it.
        return if node.attribute("ScriptLive").is_some() { vec!["Value", "Style"] } else { vec![] };
    }
    let mut out: Vec<&'static str> = parameters::definitions(kind)
        .iter()
        .filter(|p| !node_lanes(node, p.name).is_empty())
        .map(|p| p.name)
        .collect();
    if lane_family(node) {
        // Any field: `retune` decides whether its write is lanes only.
        for name in parameters::definitions(kind).iter().map(|p| p.name) {
            if name != "Bypass" && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    if bypassable(node) {
        out.push("Bypass");
    } else if kind == "AuxEffect"
        && node.parent().is_some_and(|p| p.has_tag_name("Chains"))
        && bus_scope(node)
    {
        out.push("BypassGain");
    }
    out
}

/// The engine parameter id and address slot a field is written through. The
/// older Gain and OnePole fields keep slot -1; the rest are told apart by
/// their catalog position. The engine name only has to be a valid id.
fn address(kind: &str, name: &str) -> Option<(&'static str, i32)> {
    let first = if kind=="GainMatrix" && matrix_cell(name).is_some() {P::CellLL}
        else {lanes(kind,name).first().map(|lane|lane.1)?};
    let engine = if matches!(first, P::Cutoff | P::Frequency) {
        "ENGINE_PAR_CUTOFF"
    } else {
        "ENGINE_PAR_VOLUME"
    };
    let slot = if matches!((kind, name), ("Gain", "Volume") | ("OnePole", "Freq")) {
        -1
    } else {
        parameters::definitions(kind)
            .iter()
            .position(|p| p.name == name)
            .and_then(|i| i32::try_from(i).ok())?
    };
    Some((engine, slot))
}

pub(crate) fn key(node: usize, kind: &str, name: &str) -> String {
    format!("{PREFIX}{node}:{kind}:{name}")
}

fn matrix_cell(name:&str)->Option<(usize,usize)> {
    let mut parts=name.strip_prefix("Gain_")?.split('_');
    let input=parts.next()?.parse::<usize>().ok()?.checked_sub(1)?;
    let output=parts.next()?.parse::<usize>().ok()?.checked_sub(1)?;
    (parts.next().is_none() && input<12 && output<12).then_some((input,output))
}

pub(crate) fn binding(node: usize, kind: &str, name: &str) -> Option<EngineParameterBinding> {
    let (engine, slot) = address(kind, name)?;
    let p = parameters::definitions(kind)
        .iter()
        .find(|p| p.name == name)?;
    Some(EngineParameterBinding {
        address: EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id(engine)?,
            group: -1,
            slot,
            generic: i32::try_from(node).ok()?,
        },
        control: sampler_core::lower::ir_control_id(&key(node, kind, name)),
        // This is service normalization, not a second DSP law. DSP lanes hold
        // native Hz or linear amplitude directly.
        law: if p.unit == "Hz" {
            EngineParameterLaw::Exponential {
                low: p.min,
                high: p.max,
            }
        } else {
            EngineParameterLaw::Linear {
                low: p.min,
                high: p.max,
            }
        },
    })
}

pub(crate) fn bindings(instrument: &ir::Instrument) -> Vec<EngineParameterBinding> {
    let mut bindings:Vec<_>=instrument
        .controls
        .iter()
        .filter_map(|c| {
            let mut parts = c.key.strip_prefix(PREFIX)?.split(':');
            let node = parts.next()?.parse().ok()?;
            let kind = parts.next()?;
            let name = parts.next()?;
            if parts.next().is_some() {
                return None;
            }
            if kind=="GainMatrix" && let Some(matrix)=instrument.source_indices.processing_matrices.iter().find(|matrix|matrix.node as usize==node) {
                let (input,output)=matrix_cell(name)?;
                let compiled=matrix.compiled.as_ref()?;
                if !compiled.cells.iter().any(|cell|cell.input==input && cell.output==output
                    && instrument.controls.get(cell.control.0).is_some_and(|control|control.key==c.key)) {return None;}
            }
            binding(node, kind, name)
        })
        .collect();
    for control in &instrument.controls {
        let Some(node)=control.key.strip_prefix("uvi:scope-pan:").and_then(|node|node.parse::<usize>().ok()) else {continue;};
        let Some(parameter)=sampler_core::engine_parameter_id("ENGINE_PAR_PAN") else {continue;};
        if let Ok(generic)=i32::try_from(node) {
            bindings.push(EngineParameterBinding{address:EngineParameterAddress{parameter,group:-1,slot:-1,generic},
                control:sampler_core::lower::ir_control_id(&control.key),law:EngineParameterLaw::Linear{low:-1.,high:1.}});
        }
    }
    bindings
}

pub(crate) fn register(
    instrument: &mut ir::Instrument,
    doc: &roxmltree::Document,
    inserts: &[InsertNode],
) {
    let mut made = std::collections::HashMap::new();
    for insert in inserts.iter().filter(|i| i.count > 0) {
        let Some(node) = u32::try_from(insert.node)
            .ok()
            .and_then(|id| doc.get_node(roxmltree::NodeId::new(id)))
        else {
            continue;
        };
        let kind = node.tag_name().name();
        for p in parameters::definitions(kind) {
            let lanes: Vec<_> = node_lanes(node, p.name)
                .into_iter()
                .filter(|(offset, _)| *offset < insert.count)
                .collect();
            if lanes.is_empty() {
                continue;
            }
            let read = |n: &str| node.attribute(n).and_then(|v| v.parse::<f64>().ok());
            let mut default = read(p.name).unwrap_or(p.default);
            if kind == "DigitalEq" {
                if p.name == "OverallGain" || matches!(band_field(p.name), Some(("Gain", _))) {
                    default *= read("GainScale").unwrap_or(1.);
                }
            }
            // A bypassed rack chain starts silent: its gain is what bypass is.
            if kind == "AuxEffect"
                && p.name == "Gain"
                && node.attribute("Bypass").is_some_and(|v| v.parse::<f64>().unwrap_or(0.) != 0.)
                && live_names(node).contains(&"BypassGain")
            {
                default = 0.;
            }
            if !default.is_finite() || default < p.min || default > p.max {
                continue;
            }
            // A node placed on several chains (one per keygroup, say) is one control.
            let key = key(insert.node, kind, p.name);
            let control = *made.entry(key.clone()).or_insert_with(|| {
                let control = ir::ControlRef(instrument.controls.len());
                instrument.controls.push(ir::Control {
                    key,
                    label: p.name.into(),
                    value: ir::ControlValue::Continuous {
                        min: p.min,
                        max: p.max,
                        default,
                        unit: if p.unit == "Hz" {
                            ir::ControlUnit::Hertz
                        } else {
                            ir::ControlUnit::None
                        },
                    },
                    automation: ir::Automation::None,
                });
                control
            });
            for (offset, parameter) in lanes {
                instrument.processor_controls.push(ir::ProcessorControl {
                    control,
                    chain: insert.chain,
                    index: insert.first + offset,
                    parameter,
                    ramp: ir::Time::Seconds(0.),
                });
            }
        }
    }
}

/// Retain full immutable cell schemas, not 144 mutable runtime controls. The
/// normal materializer resolves only cells in the actual decoded working port.
pub(crate) fn register_source_matrices(instrument:&mut ir::Instrument) {
    for matrix in &mut instrument.source_indices.processing_matrices {
        for output in 0..matrix.matrix.output.channels {
            for input in 0..matrix.matrix.input.channels {
                let name=format!("Gain_{}_{}",input+1,output+1);
                let Some(parameter)=parameters::definitions("GainMatrix").iter().find(|p|p.name==name) else {continue;};
                let key=key(matrix.node as usize,"GainMatrix",&name);
                let value=matrix.matrix.cells[output*matrix.matrix.input.channels+input];
                // Static authored coefficients remain exact even when no live
                // catalog domain can represent that value; never clamp them.
                if value<parameter.min || value>parameter.max {continue;}
                let definition=ir::Control{key:key.clone(),label:name,value:ir::ControlValue::Continuous{
                    min:parameter.min,max:parameter.max,default:value,unit:ir::ControlUnit::None},automation:ir::Automation::None};
                if let Some(index)=instrument.controls.iter().position(|control|control.key==key) {
                    matrix.controls.push(ir::SourceProcessingCell{input,output,control:ir::ControlRef(index)});
                }
                matrix.control_definitions.push(ir::SourceProcessingCellDefinition{input,output,control:definition});
            }
        }
    }
}

/// A DigitalEq band's field name and number: `Gain3` is ("Gain", 3).
fn band_field(name: &str) -> Option<(&str, usize)> {
    let digits = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    Some((&name[..name.len() - digits.len()], digits.parse().ok()?))
}

/// The control name and value a script's write of `name` reaches the DSP as:
/// DigitalEq band gains are scaled by GainScale (Q and Bandwidth are separate native fields).
pub(crate) fn effective(
    kind: &str,
    name: &str,
    value: f64,
    read: impl Fn(&str) -> Option<f64>,
) -> (String, f64) {
    if kind == "DigitalEq" {
        match band_field(name) {
            Some(("Gain", _)) => return (name.into(), value * read("GainScale").unwrap_or(1.)),
            _ if name == "OverallGain" => return (name.into(), value * read("GainScale").unwrap_or(1.)),
            _ => {}
        }
    }
    (name.into(), value)
}

/// Start the program as the scripts left it: control defaults, and a Bypass
/// written while loading (a Mix's bypass, or a rack chain's gain at zero).
pub(crate) fn initialize(instrument: &mut ir::Instrument, overrides: &[(usize, String, String)]) {
    for (node, name, value) in overrides {
        if name == "Bypass" {
            let off = value.parse::<f64>().is_ok_and(|v| v != 0.);
            let mut mixed = false;
            for p in instrument.chains.iter_mut().flat_map(|c| {
                c.pre_amplitude.iter_mut().chain(&mut c.post_amplitude)
            }) {
                if let ir::Processor::Mix {
                    address, bypass, ..
                } = p
                    && usize::try_from(address.generic).ok() == Some(*node)
                {
                    *bypass = off;
                    mixed = true;
                }
            }
            if !mixed && off {
                // A rack chain: bypass is gain zero.
                for c in &mut instrument.controls {
                    if c.key == key(*node, "AuxEffect", "Gain")
                        && let ir::ControlValue::Continuous { default, .. } = &mut c.value
                    {
                        *default = 0.;
                    }
                }
            }
            continue;
        }
        for c in &mut instrument.controls {
            let Some(mut parts) = c.key.strip_prefix(PREFIX).map(|s| s.split(':')) else {
                continue;
            };
            if parts.next().and_then(|n| n.parse().ok()) != Some(*node) {
                continue;
            }
            parts.next();
            if parts.next() != Some(name.as_str()) {
                continue;
            }
            if let (
                ir::ControlValue::Continuous {
                    min, max, default, ..
                },
                Ok(v),
            ) = (&mut c.value, value.parse::<f64>())
            {
                if v.is_finite() && v >= *min && v <= *max {
                    *default = v;
                }
            }
        }
    }
}

/// The ConstantModulation sources (XML node -> Script Event Modulation id,
/// counted down from 1000: the core keeps script values below USER_EVENT_PAR)
/// that a program's scripts can write: those they name by a string literal
/// (or by a literal prefix before `..`), or all of them when a script indexes
/// `modulations` by anything but a literal. Only these leave the static fold
/// and become live sources, so a constant nobody writes costs nothing.
pub(crate) fn promoted_constants(
    xml: &str,
    scripts: &crate::script::Scripts,
) -> std::collections::HashMap<usize, u16> {
    let Ok(doc) = crate::parse_program_xml(xml) else {
        return Default::default();
    };
    let roots: String = doc
        .descendants()
        .filter(|n| n.has_tag_name("script"))
        .filter_map(|n| n.text())
        .collect();
    let text = scripts.reachable(&roots);
    if !text.contains("modulations") {
        return Default::default();
    }
    // Lowercased by `reachable`; names are compared the same way.
    let mut literals = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != quote && bytes[end] != b'\n' {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == quote {
                let after = text[end + 1..].trim_start();
                literals.push((&text[start..end], after.starts_with("..")));
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    let dynamic = text.match_indices("modulations").any(|(at, word)| {
        let rest = text[at + word.len()..].trim_start();
        rest.strip_prefix('[')
            .is_some_and(|r| !r.trim_start().starts_with(['"', '\'']))
    });
    let mut out = std::collections::HashMap::new();
    let sources = doc.descendants().filter(|n| n.has_tag_name("ConstantModulation"));
    for (rank, node) in sources.enumerate() {
        let name = node.attribute("Name").unwrap_or_default().to_lowercase();
        let named = literals
            .iter()
            .any(|(l, open)| !l.is_empty() && (*l == name || (*open && name.starts_with(l))));
        if let Some(id) = 1000u16.checked_sub(rank as u16)
            && (dynamic || named)
        {
            out.insert(node.id().get_usize(), id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digital_eq_gain_follows_the_scale_and_bandwidth_is_its_own_lane() {
        let (name, gain) = effective("DigitalEq", "Gain3", 6., |n| (n == "GainScale").then_some(0.5));
        assert_eq!((name.as_str(), gain), ("Gain3", 3.));
        assert_eq!(effective("DigitalEq", "Bandwidth2", 0.4, |_| None), ("Bandwidth2".into(), 0.4));
        assert_eq!(lanes("DigitalEq", "Bandwidth2"), vec![(1, P::Bandwidth)]);
    }

    #[test]
    fn a_node_placed_on_several_chains_is_one_control() {
        let doc = roxmltree::Document::parse(r#"<Gain Volume="0.5"/>"#).unwrap();
        let node = doc.root_element().id().get_usize();
        let placed = |chain| InsertNode { node, chain: ir::ChainRef(chain), first: 0, count: 1 };
        let mut ir = ir::Instrument::default();
        register(&mut ir, &doc, &[placed(0), placed(1)]);
        assert_eq!((ir.controls.len(), ir.processor_controls.len()), (1, 2));
    }
}

/// The convolution slot (bus, then processor order, as lowering numbers them)
/// of each bus insert element that holds a convolver.
pub(crate) fn convolver_slots(
    instrument: &ir::Instrument,
    inserts: &[InsertNode],
) -> std::collections::HashMap<usize, usize> {
    let mut slots = std::collections::HashMap::new();
    let mut next = 0;
    for chain in instrument.buses.iter().filter_map(|bus| bus.chain) {
        let processors = &instrument.chains[chain.0];
        let all = processors.pre_amplitude.iter().chain(&processors.post_amplitude);
        for (index, processor) in all.enumerate() {
            if matches!(processor, ir::Processor::Space(c) if c.impulse.is_some()) {
                if let Some(insert) = inserts
                    .iter()
                    .find(|n| n.chain == chain && (n.first..n.first + n.count).contains(&index))
                {
                    slots.insert(insert.node, next);
                }
                next += 1;
            }
        }
    }
    slots
}

/// Source-owner identity remains the actual XML node id. Key generation here
/// is off audio; the callback equivalent below uses only a fixed stack buffer.
pub(crate) fn scope_gain_key(node: usize) -> String {
    format!("uvi:scope-gain:{node}")
}
pub(crate) fn scope_pan_key(node:usize)->String {format!("uvi:scope-pan:{node}")}
pub(crate) fn scope_pan_control(node:usize)->Result<sampler_core::ControlId,sampler_core::Error> {
    scope_control(node,b"uvi:scope-pan:")
}
pub(crate) fn scope_gain_control(node: usize) -> Result<sampler_core::ControlId, sampler_core::Error> {
    scope_control(node,b"uvi:scope-gain:")
}
fn scope_control(node:usize,prefix:&[u8])->Result<sampler_core::ControlId,sampler_core::Error> {
    let mut bytes = [0u8; 64];
    bytes[..prefix.len()].copy_from_slice(prefix);
    let mut digits = [0u8; 20];
    let mut n = node;
    let mut count = 0;
    loop {
        if count == digits.len() { return Err(sampler_core::Error::InvalidInput); }
        digits[count] = b'0' + (n % 10) as u8;
        count += 1;
        n /= 10;
        if n == 0 { break; }
    }
    for (index, byte) in digits[..count].iter().rev().enumerate() {
        bytes[prefix.len() + index] = *byte;
    }
    let key = std::str::from_utf8(&bytes[..prefix.len() + count])
        .map_err(|_| sampler_core::Error::InvalidInput)?;
    Ok(sampler_core::lower::ir_control_id(key))
}
