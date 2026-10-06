//! Feeds the v2 views from the current rack until the v2 core exposes its
//! mixer tree and load reports directly. Everything here goes when the
//! seam lands; the views themselves only see plain data.

use super::{Cx, load_report as lr, mix_tree as mt};
use crate::plugin::SamplerParams;
use std::sync::Arc;

/// The rack as a mixer tree: each part, and under it the outputs its
/// library's mic mixer names. Mic levels are the library's own (its script
/// owns them), so their strips show routing only.
pub fn tree(cx: &Cx) -> mt::Tree {
    let mut nodes = Vec::new();
    for &slot in &cx.selection.order {
        let slot = slot as usize;
        let Some(part) = cx.selection.parts.get(slot).filter(|p| !p.path.is_empty()) else { continue };
        let root = nodes.len();
        let mut n = mt::Node::new(slot as u64 + 1, super::rack::name(cx, slot), mt::Kind::Instrument, None);
        (n.gain_db, n.pan, n.mute, n.solo) = (part.gain, part.pan, part.mute, part.solo);
        (n.output, n.output_set) = (mt::Output::Host(part.output), part.output_manual);
        n.inserts = super::instrument::instrument_of(cx, slot)
            .map(|i| i.fx.insert.slots.iter().map(|e| e.kind.name()).collect())
            .unwrap_or_default();
        nodes.push(n);
        for (k, name) in part.mic_names.iter().enumerate() {
            let mut m = mt::Node::new(((slot as u64 + 1) << 16) | k as u64, name.clone(), mt::Kind::Mic, Some(root));
            m.adjustable = false;
            match part.mic_buses.get(k).copied().unwrap_or(-1) {
                b if b >= 0 => (m.output, m.output_set) = (mt::Output::Host(b as u8), true),
                _ => m.output = mt::Output::Parent,
            }
            nodes.push(m);
        }
    }
    mt::Tree { nodes }
}

/// Writes the tree's edits back to the rack.
pub fn apply(cx: &mut Cx, tree: &mt::Tree) {
    for n in &tree.nodes {
        let (slot, mic) = if n.id >> 16 == 0 { (n.id as usize - 1, None) } else { ((n.id >> 16) as usize - 1, Some((n.id & 0xffff) as usize)) };
        let Some(part) = cx.selection.parts.get_mut(slot) else { continue };
        match mic {
            None => {
                (part.gain, part.pan, part.mute, part.solo) = (n.gain_db, n.pan, n.mute, n.solo);
                part.output_manual = n.output_set;
                if let mt::Output::Host(p) = n.output {
                    part.output = p;
                }
            }
            Some(k) => {
                if let Some(b) = part.mic_buses.get_mut(k) {
                    *b = match n.output {
                        mt::Output::Host(p) => i16::from(p),
                        mt::Output::Parent => -1,
                    };
                }
            }
        }
    }
}

/// Post-fader part levels for the tree's instrument strips.
pub fn levels(p: &Arc<SamplerParams>, tree: &mt::Tree) -> mt::Levels {
    let slots: Vec<Option<usize>> = tree.nodes.iter().map(|n| (n.id >> 16 == 0).then(|| n.id as usize - 1)).collect();
    let p = p.clone();
    Arc::new(move |n| {
        slots
            .get(n)
            .copied()
            .flatten()
            .and_then(|s| p.shared.part(s))
            .map_or([0.; 2], |part| crate::plugin::Meters::read(&part.meter))
    })
}

/// What the current loader recorded about `slot`'s load, as a report.
pub fn report(cx: &Cx, slot: usize) -> lr::Report {
    let mut r = lr::Report { instrument: super::rack::name(cx, slot), ..lr::Report::default() };
    let v = &cx.view.parts[slot];
    if let Some(i) = super::instrument::instrument_of(cx, slot) {
        r.loaded.push(lr::Loaded { area: lr::Area::Mapping, summary: format!("{} zones · {} groups", i.zones.len(), i.groups.len()) });
        if !i.scripts.is_empty() {
            r.loaded.push(lr::Loaded { area: lr::Area::Scripts, summary: format!("{} scripts", i.scripts.len()) });
        }
        r.missing.extend(i.missing_samples.iter().map(|path| lr::Missing::Sample { path: path.clone() }));
        for w in &i.warnings {
            let (feature, value) = w.split_once(": ").unwrap_or((w.as_str(), ""));
            r.missing.push(lr::Missing::Other { location: String::new(), feature: feature.to_owned(), value: value.to_owned() });
        }
    }
    if let Some(u) = &v.interface {
        r.loaded.push(lr::Loaded { area: lr::Area::Interface, summary: format!("{} controls", u.controls.len()) });
        for d in &u.diagnostics {
            r.missing.push(lr::Missing::Other { location: "Script".into(), feature: "Script".into(), value: d.clone() });
        }
    }
    r
}
