//! The v2 core's plain data ([`crate::sound::tree`], [`crate::sound::report`])
//! as the mixer tree and load report views take it, and the views' edits
//! back into the rack's part settings.

use super::{Cx, load_report as lr, mix_tree as mt};
use crate::plugin::SamplerParams;
use crate::sound::report::{LoadReport, RuntimeProblems};
use crate::sound::tree::{NodeKind, NodeMix, NodeOutput};
use std::sync::Arc;

/// A node's stable id: its part's slot and its index in the part's tree.
fn id(slot: usize, node: usize) -> u64 {
    ((slot as u64 + 1) << 16) | node as u64
}

/// Every loaded part's output tree, in rack order.
pub fn tree(cx: &Cx) -> mt::Tree {
    let mut nodes = Vec::new();
    for &slot in &cx.selection.order {
        let slot = slot as usize;
        let Some(part) = cx.selection.parts.get(slot).filter(|p| !p.path.is_empty()) else { continue };
        let source = cx.view.parts.get(slot).and_then(|v| v.tree.clone());
        let root = nodes.len();
        let mut n = mt::Node::new(id(slot, 0), super::rack::name(cx, slot), mt::Kind::Instrument, None);
        (n.gain_db, n.pan, n.mute, n.solo) = (part.gain, part.pan, part.mute, part.solo);
        (n.output, n.output_set) = (mt::Output::Host(part.output), part.output_manual);
        n.inserts = source.as_ref().map(|t| t.nodes[0].inserts.clone()).unwrap_or_default();
        nodes.push(n);
        let Some(source) = source else { continue };
        for (k, node) in source.nodes.iter().enumerate().skip(1) {
            let kind = match node.kind {
                NodeKind::Group => mt::Kind::Group,
                NodeKind::Mic => mt::Kind::Mic,
                NodeKind::Instrument | NodeKind::Bus => mt::Kind::Bus,
            };
            let mut n = mt::Node::new(id(slot, k), node.name.clone(), kind, node.parent.map(|p| root + p));
            let mix = part.nodes.get(k - 1).copied().unwrap_or_default();
            (n.gain_db, n.pan, n.mute, n.solo) = (mix.gain, mix.pan, mix.mute, mix.solo);
            (n.output, n.output_set) = match mix.output {
                NodeOutput::Parent => (mt::Output::Parent, mix.manual),
                NodeOutput::Pair(p) => (mt::Output::Host(p), mix.manual),
            };
            n.inserts = node.inserts.clone();
            nodes.push(n);
        }
    }
    mt::Tree { nodes }
}

/// Writes the tree's edits back to the rack's parts.
pub fn apply(cx: &mut Cx, tree: &mt::Tree) {
    for n in &tree.nodes {
        let (slot, k) = ((n.id >> 16) as usize - 1, (n.id & 0xffff) as usize);
        let Some(part) = cx.selection.parts.get_mut(slot) else { continue };
        if k == 0 {
            (part.gain, part.pan, part.mute, part.solo) = (n.gain_db, n.pan, n.mute, n.solo);
            part.output_manual = n.output_set;
            if let mt::Output::Host(p) = n.output {
                part.output = p;
            }
            continue;
        }
        if part.nodes.len() < k {
            part.nodes.resize(k, NodeMix::default());
        }
        let output = match n.output {
            mt::Output::Parent => NodeOutput::Parent,
            mt::Output::Host(p) => NodeOutput::Pair(p),
        };
        let mix = NodeMix { gain: n.gain_db, pan: n.pan, mute: n.mute, solo: n.solo, output, manual: n.output_set };
        if part.nodes[k - 1] != mix {
            part.nodes[k - 1] = mix;
        }
    }
}

/// Each node's post-fader level: the part meter for an instrument, the
/// node meter below it.
pub fn levels(p: &Arc<SamplerParams>, tree: &mt::Tree) -> mt::Levels {
    let at: Vec<(usize, usize)> = tree.nodes.iter().map(|n| ((n.id >> 16) as usize - 1, (n.id & 0xffff) as usize)).collect();
    let p = p.clone();
    Arc::new(move |n| {
        let Some(&(slot, k)) = at.get(n) else { return [0.; 2] };
        p.shared.part(slot).map_or([0.; 2], |part| part.level(k))
    })
}

/// `slot`'s load report as the report view shows it.
pub fn report(cx: &Cx, slot: usize) -> lr::Report {
    let mut r = lr::Report { instrument: super::rack::name(cx, slot), ..lr::Report::default() };
    let Some(report) = cx.view.parts.get(slot).and_then(|v| v.report.clone()) else { return r };
    convert(&report, cx.view.parts[slot].interfaces.len(), &mut r);
    r
}

fn convert(report: &LoadReport, interfaces: usize, r: &mut lr::Report) {
    let d = &report.decoded;
    let loaded = [
        (lr::Area::Mapping, d.zones > 0, format!("{} zones · {} groups", d.zones, d.groups)),
        (lr::Area::Samples, d.samples > 0, format!("{} samples", d.samples)),
        (lr::Area::Scripts, d.scripts > 0, format!("{} scripts", d.scripts)),
        (lr::Area::Interface, interfaces > 0, format!("{interfaces} script panels")),
    ];
    r.loaded.extend(loaded.into_iter().filter(|(_, any, _)| *any).map(|(area, _, summary)| lr::Loaded { area, summary }));
    r.missing.extend(report.missing.iter().map(|m| missing(&m.location, &m.feature, &m.value)));
    let RuntimeProblems { capacity_drops, underruns, nonfinite, .. } = report.runtime;
    if capacity_drops > 0 {
        r.runtime.push(lr::Runtime::VoicesDropped { count: capacity_drops });
    }
    if underruns > 0 {
        r.runtime.push(lr::Runtime::StreamUnderruns { count: underruns });
    }
    if nonfinite > 0 {
        r.runtime.push(lr::Runtime::NonFinite { location: report.name.clone(), count: nonfinite });
    }
}

/// One untranslated item, in the view's terms. Script diagnostics come as
/// `script` (`line:column: message`) and `script <Kind>[: builtin]`
/// ([`sampler_kontakt::prepare`]).
fn missing(location: &str, feature: &str, value: &str) -> lr::Missing {
    let script = location.rsplit_once(" line ").map_or(location, |(s, _)| s).to_owned();
    if feature == "script" {
        let mut at = value.splitn(3, ':');
        let (line, column, message) = (at.next(), at.next(), at.next());
        if let (Some(Ok(line)), Some(Ok(column)), Some(message)) =
            (line.map(str::parse), column.map(str::parse), message)
        {
            return lr::Missing::ScriptError { script, line, column, message: message.trim().to_owned() };
        }
        return lr::Missing::ScriptError { script, line: 0, column: 0, message: value.to_owned() };
    }
    if let Some(name) = feature.strip_prefix("script Unsupported: ") {
        let line = location.rsplit_once(" line ").and_then(|(_, l)| l.parse().ok()).unwrap_or(0);
        return lr::Missing::ScriptBuiltin { script, name: name.to_owned(), line, column: 0 };
    }
    if feature == "missing sample" {
        return lr::Missing::Sample { path: value.to_owned() };
    }
    lr::Missing::Other { location: location.to_owned(), feature: feature.to_owned(), value: value.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_diagnostics_become_their_own_rows() {
        assert_eq!(
            missing("panel", "script", "12:4: unknown variable $x"),
            lr::Missing::ScriptError { script: "panel".into(), line: 12, column: 4, message: "unknown variable $x".into() }
        );
        assert_eq!(
            missing("panel line 9", "script Unsupported: set_skin_offset", "ignored"),
            lr::Missing::ScriptBuiltin { script: "panel".into(), name: "set_skin_offset".into(), line: 9, column: 0 }
        );
        assert!(matches!(missing("zone 3", "loop", "dropped"), lr::Missing::Other { .. }));
    }
}
