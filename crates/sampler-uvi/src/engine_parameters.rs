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
                ("Q" | "Bandwidth", Ok(n @ 1..=16)) => vec![(n - 1, P::Quality)],
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
    if !matches!(kind, "Gain" | "OnePole") && !bus_scope(node) {
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
        "Convolver" | "SampledReverb" => bus_scope(node),
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
    let first = lanes(kind, name).first().map(|l| l.1)?;
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

fn key(node: usize, kind: &str, name: &str) -> String {
    format!("{PREFIX}{node}:{kind}:{name}")
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
    instrument
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
            binding(node, kind, name)
        })
        .collect()
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
            // Bandwidth is the same band parameter as Q: it writes through Q's control.
            if p.name.starts_with("Bandwidth") {
                continue;
            }
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
                default = match band_field(p.name) {
                    Some(("Q", band)) => band_q(read, band),
                    Some(("Gain", _)) => default * read("GainScale").unwrap_or(1.),
                    _ => default,
                };
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

/// A DigitalEq band's field name and number: `Gain3` is ("Gain", 3).
fn band_field(name: &str) -> Option<(&str, usize)> {
    let digits = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    Some((&name[..name.len() - digits.len()], digits.parse().ok()?))
}

/// Q for a band's width in octaves (RBJ), within the catalog's Q range.
fn q_for_octaves(octaves: f64) -> f64 {
    (0.5 / (std::f64::consts::LN_2 / 2. * octaves).sinh()).clamp(0.018, 28.284)
}

/// A DigitalEq band's Q. Q and Bandwidth are one parameter saved twice; a
/// program whose Q is still the default but whose Bandwidth was edited (the
/// English Horn formant EQs) was authored by bandwidth.
pub(crate) fn band_q(read: impl Fn(&str) -> Option<f64>, band: usize) -> f64 {
    let q = read(&format!("Q{band}")).unwrap_or(std::f64::consts::FRAC_1_SQRT_2);
    match read(&format!("Bandwidth{band}")) {
        Some(octaves) if (octaves - 1.).abs() > 1e-6 && (q - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6 => {
            q_for_octaves(octaves)
        }
        _ => q,
    }
}

/// The control name and value a script's write of `name` reaches the DSP as:
/// DigitalEq band gains are scaled by GainScale, and a Bandwidth is Q.
pub(crate) fn effective(
    kind: &str,
    name: &str,
    value: f64,
    read: impl Fn(&str) -> Option<f64>,
) -> (String, f64) {
    if kind == "DigitalEq" {
        match band_field(name) {
            Some(("Gain", _)) => return (name.into(), value * read("GainScale").unwrap_or(1.)),
            Some(("Bandwidth", band)) => return (format!("Q{band}"), q_for_octaves(value)),
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
    fn digital_eq_bandwidth_is_q_and_gain_follows_the_scale() {
        // One octave is Q 1.414; Q stays inside the catalog range.
        let (name, q) = effective("DigitalEq", "Bandwidth2", 1., |_| None);
        assert_eq!(name, "Q2");
        assert!((q - 1.4142).abs() < 1e-3, "{q}");
        assert!((0.018..0.04).contains(&q_for_octaves(10.)) && q_for_octaves(0.01) == 28.284);
        let (name, gain) = effective("DigitalEq", "Gain3", 6., |n| (n == "GainScale").then_some(0.5));
        assert_eq!((name.as_str(), gain), ("Gain3", 3.));
        // Q still at its default with an edited Bandwidth: authored by bandwidth.
        let read = |n: &str| match n {
            "Q1" => Some(std::f64::consts::FRAC_1_SQRT_2),
            "Bandwidth1" => Some(0.4),
            _ => None,
        };
        assert!((band_q(read, 1) - q_for_octaves(0.4)).abs() < 1e-9);
        assert!((band_q(|_| None, 1) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
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
            if matches!(processor, ir::Processor::Convolution { .. }) {
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
