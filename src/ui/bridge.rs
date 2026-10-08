//! The rack's parts as the v2 views' plain data: the nested mixer's
//! [`mt::Tree`] from each part's [`crate::sound::tree::MixTree`] and its
//! settings, and the load report from [`crate::sound::report::LoadReport`].

use super::{Cx, load_report as lr, mix_tree as mt};
use crate::plugin::{SamplerParams, Bus, Meters};
use crate::sound::BUSES;
use crate::sound::tree::NodeMix;
use std::sync::Arc;

/// Node `n` of part `slot`: the part itself is `slot + 1`.
fn id(slot: usize, n: usize) -> u64 {
    ((slot as u64 + 1) << 16) | n as u64
}

/// Every loaded part's output tree, in rack order.
pub fn tree(cx: &Cx) -> mt::Tree {
    let mut nodes = Vec::new();
    for &slot in &cx.selection.order {
        let slot = slot as usize;
        let Some(part) = cx.selection.parts.get(slot).filter(|p| !p.path.is_empty()) else { continue };
        let source = cx.view.parts.get(slot).and_then(|v| v.tree.clone());
        let base = nodes.len();
        let count = source.as_ref().map_or(1, |t| t.nodes.len().max(1));
        for n in 0..count {
            let src = source.as_ref().and_then(|t| t.nodes.get(n));
            let kind = src.map_or(mt::Kind::Instrument, |s| s.kind);
            let name = if n == 0 { super::rack::name(cx, slot) } else { src.map(|s| s.name.clone()).unwrap_or_default() };
            let mut node = mt::Node::new(id(slot, n), name, kind, src.and_then(|s| s.parent).map(|p| base + p));
            node.inserts = src.map(|s| s.inserts.clone()).unwrap_or_default();
            if n == 0 {
                (node.gain_db, node.pan, node.mute, node.solo) = (part.gain, part.pan, part.mute, part.solo);
                (node.output, node.output_set) = (mt::Output::Pair(part.output), part.output_manual);
                node.aux = Some((part.aux, part.aux_gain));
            } else {
                let m = part.nodes.get(n - 1).copied().unwrap_or_default();
                (node.gain_db, node.pan, node.mute, node.solo) = (m.gain, m.pan, m.mute, m.solo);
                (node.output, node.output_set) = (m.output, m.manual);
            }
            nodes.push(node);
        }
    }
    // Port v1's used/touched stereo bus strips beside v2's source tree.
    for n in 0..BUSES {
        let used = cx.selection.parts.iter().any(|p| !p.path.is_empty() && (usize::from(p.output) == n || p.aux == n as i16 || p.nodes.iter().any(|m| m.output == mt::Output::Pair(n as u8))));
        let bus = cx.selection.bus(n);
        if !used && bus == Bus::default() { continue; }
        let mut node = mt::Node::new(n as u64 + 1, crate::routing::label(&cx.selection, n), mt::Kind::Bus, None);
        (node.gain_db, node.pan, node.mute, node.solo) = (bus.gain, bus.pan, bus.mute, bus.solo);
        node.output = mt::Output::Pair(if bus.port < 0 { n as u8 } else { bus.port as u8 });
        node.output_set = bus.port >= 0;
        node.host = true;
        nodes.push(node);
    }
    mt::Tree { nodes }
}

/// Writes the tree's edits back to the rack.
pub fn apply(cx: &mut Cx, tree: &mt::Tree) {
    for node in &tree.nodes {
        if node.id >> 16 == 0 {
            let Some(n) = (node.id as usize).checked_sub(1).filter(|&n| n < BUSES) else { continue };
            let default = if node.renamed { let bus = cx.selection.bus_mut(n); bus.name.clear(); crate::routing::label(&cx.selection, n) } else { String::new() };
            let bus = cx.selection.bus_mut(n);
            if node.renamed { bus.name = if node.name == default { String::new() } else { node.name.clone() }; }
            (bus.gain, bus.pan, bus.mute, bus.solo) = (node.gain_db, node.pan, node.mute, node.solo);
            bus.port = match node.output { mt::Output::Pair(p) if node.output_set && p as usize != n => i16::from(p), _ => -1 };
            continue;
        }
        let (slot, n) = ((node.id >> 16) as usize - 1, (node.id & 0xffff) as usize);
        let default = node.renamed.then(|| cx.instrument_name(slot).unwrap_or_else(|| super::header::stem(&cx.selection.parts[slot].path)));
        let Some(part) = cx.selection.parts.get_mut(slot) else { continue };
        if n == 0 {
            if let Some(default) = default { part.name = if node.name == default { String::new() } else { node.name.clone() }; }
            if let Some((aux, gain)) = node.aux { (part.aux, part.aux_gain) = (aux, gain); }
            (part.gain, part.pan, part.mute, part.solo) = (node.gain_db, node.pan, node.mute, node.solo);
            part.output_manual = node.output_set;
            if let mt::Output::Pair(p) = node.output {
                part.output = p;
            }
            continue;
        }
        if part.nodes.len() < n {
            part.nodes.resize(n, NodeMix::default());
        }
        let m = &mut part.nodes[n - 1];
        (m.gain, m.pan, m.mute, m.solo) = (node.gain_db, node.pan, node.mute, node.solo);
        (m.output, m.manual) = (node.output, node.output_set);
    }
}

/// Port from v1 0cb7a8a0:src/ui/mixer.rs; routing and names stay.
pub fn reset(cx: &mut Cx, id: u64) {
    if id >> 16 == 0 {
        if let Some(n) = (id as usize).checked_sub(1).filter(|&n| n < BUSES) {
            let bus = cx.selection.bus_mut(n);
            *bus = Bus { name: std::mem::take(&mut bus.name), port: bus.port, ..Bus::default() };
        }
    } else if id & 0xffff == 0 {
        if let Some(part) = cx.selection.parts.get_mut((id >> 16) as usize - 1) {
            part.gain = 0.; part.pan = 0.; part.mute = false; part.solo = false; part.aux = -1; part.aux_gain = 0.;
        }
    }
}

/// Post-fader levels of every strip: parts and their nodes.
pub fn levels(p: &Arc<SamplerParams>, tree: &mt::Tree) -> mt::Levels {
    let ids: Vec<u64> = tree.nodes.iter().map(|n| n.id).collect();
    let p = p.clone();
    Arc::new(move |n| {
        let Some(&id) = ids.get(n) else { return [0.; 2] };
        if id >> 16 == 0 { return (id as usize).checked_sub(1).and_then(|bus| p.shared.meters.buses.get(bus)).map(Meters::read).unwrap_or([0.; 2]); }
        p.shared.part((id >> 16) as usize - 1).map_or([0.; 2], |part| part.node_level((id & 0xffff) as usize))
    })
}

/// `slot`'s load report in the panel's terms.
pub fn report(cx: &Cx, slot: usize) -> lr::Report {
    let v = &cx.view.parts[slot];
    let mut r = lr::Report { instrument: super::rack::name(cx, slot), ..lr::Report::default() };
    if let Some(reason) = v.status.strip_prefix("Load failed: ") {
        r.missing.push(lr::Missing::Access { what: "the instrument".into(), reason: reason.to_owned() });
    }
    let Some(l) = &v.report else { return r };
    let d = &l.decoded;
    let loaded = |area, summary: String| lr::Loaded { area, summary };
    r.loaded.push(loaded(lr::Area::Mapping, format!("{} · {} zones · {} groups", d.format, d.zones, d.groups)));
    let part = cx.p.shared.part(slot);
    let held = part.as_ref().map_or(0, |p| p.resident_bytes.load(std::sync::atomic::Ordering::Relaxed));
    let samples = match d.full_bytes {
        0 => format!("{} samples", d.samples),
        full => format!("{} samples · {} in memory of {}", d.samples, size(held), size(full)),
    };
    r.loaded.push(loaded(lr::Area::Samples, samples));
    if d.scripts > 0 {
        r.loaded.push(loaded(lr::Area::Scripts, format!("{} scripts · {} controls", d.scripts, d.controls)));
    }
    if !v.interfaces.is_empty() {
        let widgets: usize = v.interfaces.iter().map(|i| i.widgets.len()).sum();
        r.loaded.push(loaded(lr::Area::Interface, format!("{} views · {widgets} controls", v.interfaces.len())));
    }
    if !d.mpe.is_empty() {
        r.loaded.push(loaded(lr::Area::Modulation, format!("MPE: {}", d.mpe)));
    }
    r.missing.extend(l.missing.iter().map(missing));
    r.why_silent = l.why_silent.clone();
    r.faults = l.faults.clone();
    let p = part.map(|s| s.problems()).unwrap_or(l.runtime);
    let counts = [
        (p.script_overruns, lr::Runtime::ScriptBudget { overruns: p.script_overruns }),
        (p.capacity_drops, lr::Runtime::VoicesDropped { count: p.capacity_drops }),
        (p.underruns, lr::Runtime::StreamUnderruns { count: p.underruns }),
        (p.nonfinite, lr::Runtime::NonFinite { count: p.nonfinite }),
        (p.narrowed_input, lr::Runtime::InputNarrowed { count: p.narrowed_input }),
        (p.ignored_input, lr::Runtime::InputIgnored { count: p.ignored_input }),
        (p.stolen_voices, lr::Runtime::VoicesStolen { count: p.stolen_voices }),
    ];
    r.runtime.extend(counts.into_iter().filter(|(n, _)| *n > 0).map(|(_, r)| r));
    r
}

/// `bytes` as "12 KB", "3.4 MB" or "2.1 GB".
fn size(bytes: u64) -> String {
    match bytes {
        b if b < 1 << 20 => format!("{} KB", b.div_ceil(1 << 10)),
        b if b < 1 << 30 => format!("{:.1} MB", b as f64 / f64::from(1 << 20)),
        b => format!("{:.1} GB", b as f64 / f64::from(1 << 30)),
    }
}

/// A translator entry as a report row, recognizing the script ones
/// (`sampler_kontakt`: "script" with "line:col: message", or "script Kind: builtin"
/// at "Name line N").
fn missing(m: &crate::sound::report::Missing) -> lr::Missing {
    let num = |s: &str| s.trim().parse::<u32>().unwrap_or(0);
    if m.feature == "script" {
        let mut it = m.value.splitn(3, ':');
        if let (Some(line), Some(column), Some(message)) = (it.next(), it.next(), it.next()) {
            return lr::Missing::ScriptError { script: m.location.clone(), line: num(line), column: num(column), message: message.trim().to_owned() };
        }
    }
    if let Some((_, name)) = m.feature.strip_prefix("script ").and_then(|f| f.split_once(": "))
        && let Some((script, line)) = m.location.rsplit_once(" line ")
    {
        return lr::Missing::ScriptBuiltin { script: script.to_owned(), name: name.to_owned(), line: num(line), column: 0 };
    }
    lr::Missing::Other { location: m.location.clone(), feature: m.feature.clone(), value: m.value.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::report::{Missing, MissingReason};

    #[test]
    fn script_entries_become_script_rows() {
        let m = |location: &str, feature: &str, value: &str| missing(&Missing { location: location.into(), feature: feature.into(), value: value.into(), reason: MissingReason::NotModeled });
        assert_eq!(
            m("Pyramid", "script", "1:1: source byte budget exceeded"),
            lr::Missing::ScriptError { script: "Pyramid".into(), line: 1, column: 1, message: "source byte budget exceeded".into() }
        );
        assert_eq!(
            m("Main line 40", "script Unsupported: set_snapshot_type", ""),
            lr::Missing::ScriptBuiltin { script: "Main".into(), name: "set_snapshot_type".into(), line: 40, column: 0 }
        );
        assert!(matches!(m("zone 3", "loop mode", "4"), lr::Missing::Other { .. }));
    }
}
