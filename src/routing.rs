//! Output routing that runs itself: which DAW stereo pair each part (and
//! each of its mic buses) plays through, what each pair and host port is
//! called, and the one-click actions that replace Kontakt's batch operations.
//!
//! A part is the root of an output tree ([`crate::sound::tree`]). By default
//! every instrument gets a DAW pair of its own; "One per mic" also gives each
//! of its top-level source buses (mic positions) one. Assignments live in the
//! saved rack ([`Part::output`], [`NodeMix::output`]) and are kept wherever
//! they still hold: adding, removing or reordering parts moves no other part.
//! A route the player picks by hand ([`Part::output_manual`],
//! [`NodeMix::manual`]) is never moved, and automatic routes keep off its pair
//! and off pairs used as sends.

use crate::plugin::{Part, Selection};
use crate::sound::BUSES;
#[cfg(test)]
use crate::sound::tree::MixNode;
use crate::sound::tree::{MixTree, NodeKind, NodeMix, NodeOutput};
use std::time::{Duration, Instant};

/// The mixer's "Outputs" choice ([`Selection::outputs`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outputs {
    /// Each part on a pair and host port of its own, named after it.
    #[default]
    Instrument,
    /// Everything on outputs 1-2, as Kontakt starts: parts keep the pair
    /// they have, new ones start on 1-2 ([`to_stereo`] moves them all back).
    Stereo,
    /// As [`Outputs::Instrument`], and each top-level bus of a part's tree
    /// (a mic position) on one more: "Harp Close".
    Mic,
}

impl Outputs {
    pub const ALL: [Self; 3] = [Self::Instrument, Self::Stereo, Self::Mic];

    pub fn of(n: u8) -> Self {
        match n {
            1 => Self::Stereo,
            2 => Self::Mic,
            _ => Self::Instrument,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo mix",
            Self::Instrument => "One per instrument",
            Self::Mic => "One per mic",
        }
    }
}

/// Loaded slots, rack order first.
fn loaded(sel: &Selection) -> Vec<usize> {
    let used = |s: usize| sel.parts.get(s).is_some_and(|p| !p.path.is_empty());
    let mut slots = Vec::new();
    for s in sel.order.iter().map(|&s| s as usize).chain(0..sel.parts.len()) {
        if used(s) && !slots.contains(&s) {
            slots.push(s);
        }
    }
    slots
}

/// The nodes "One per mic" gives a pair of their own: the root's buses.
fn mic(tree: &MixTree, node: usize) -> bool {
    tree.nodes.get(node).is_some_and(|n| n.kind == NodeKind::Bus && n.parent == Some(0))
}

fn pair(mix: &NodeMix) -> Option<usize> {
    match mix.output {
        NodeOutput::Pair(p) => Some(usize::from(p).min(BUSES - 1)),
        NodeOutput::Parent => None,
    }
}

/// Route every part and node not routed by hand as [`Selection::outputs`]
/// says; `trees` are the slots' loaded output trees, by slot. Each part's
/// node settings are sized to its tree.
pub fn apply(sel: &mut Selection, trees: &[Option<std::sync::Arc<MixTree>>]) {
    let mode = Outputs::of(sel.outputs);
    let slots = loaded(sel);
    let tree = |s: usize| trees.get(s).and_then(Option::as_deref);
    let mut claimed = [false; BUSES];
    for &s in &slots {
        let p = &mut sel.parts[s];
        if let Some(t) = tree(s) {
            p.nodes.resize(t.nodes.len().saturating_sub(1), NodeMix::default());
        }
        if p.output_manual {
            claimed[usize::from(p.output).min(BUSES - 1)] = true;
        }
        if let Ok(aux) = usize::try_from(p.aux) {
            claimed[aux.min(BUSES - 1)] = true;
        }
        for n in p.nodes.iter().filter(|n| n.manual) {
            if let Some(b) = pair(n) {
                claimed[b] = true;
            }
        }
    }
    let mut pending = Vec::new();
    for &s in &slots {
        let p = &mut sel.parts[s];
        if !p.output_manual && mode != Outputs::Stereo {
            let o = usize::from(p.output).min(BUSES - 1);
            if claimed[o] {
                pending.push((s, None));
            } else {
                claimed[o] = true;
            }
        }
    }
    for &s in &slots {
        let p = &mut sel.parts[s];
        for (n, node) in p.nodes.iter_mut().enumerate().filter(|(_, n)| !n.manual) {
            if mode != Outputs::Mic || !tree(s).is_some_and(|t| mic(t, n + 1)) {
                node.output = NodeOutput::Parent;
                continue;
            }
            match pair(node) {
                Some(b) if !claimed[b] => claimed[b] = true,
                _ => pending.push((s, Some(n))),
            }
        }
    }
    // Instruments before mics: they claim in the order pending was built.
    for (s, node) in pending {
        let free = claimed.iter().position(|c| !c);
        if let Some(b) = free {
            claimed[b] = true;
        }
        let p = &mut sel.parts[s];
        match node {
            // ponytail: past 16 pairs a part shares the one it had.
            None => p.output = free.map_or(p.output, |b| b as u8),
            // Past 16, a mic plays with its instrument.
            Some(n) => p.nodes[n].output = free.map_or(NodeOutput::Parent, |b| NodeOutput::Pair(b as u8)),
        }
    }
}

/// Choose "Stereo mix": every part and node not routed by hand back on 1-2.
pub fn to_stereo(sel: &mut Selection) {
    sel.outputs = Outputs::Stereo as u8;
    for p in sel.parts.iter_mut() {
        if !p.output_manual {
            p.output = 0;
        }
        for n in p.nodes.iter_mut().filter(|n| !n.manual) {
            n.output = NodeOutput::Parent;
        }
    }
}

/// A part's name: the player's, else its preset file's.
pub fn part_name(p: &Part) -> String {
    if !p.name.is_empty() {
        return p.name.clone();
    }
    std::path::Path::new(&p.path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// "Harp Close", or just "Harp Close" when the mic is already named so.
fn mic_label(part: &str, mic: &str) -> String {
    if mic.to_lowercase().starts_with(&part.to_lowercase()) {
        mic.to_owned()
    } else {
        format!("{part} {mic}")
    }
}

/// One name for several: the words they all start with ("Violins 1",
/// "Violins 2": "Violins"), else the first and how many more.
fn common(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => {
            let words: Vec<&str> = first.split_whitespace().collect();
            let shared = (0..words.len())
                .take_while(|&i| rest.iter().all(|n| n.split_whitespace().nth(i) == Some(words[i])))
                .count();
            if shared > 0 {
                words[..shared].join(" ")
            } else {
                format!("{first} +{}", rest.len())
            }
        }
    }
}

/// What each pair plays, by name: empty when nothing is routed to it.
/// `trees` name the parts' nodes; without one a node is "Out N".
pub fn auto_names_with(sel: &Selection, trees: &[Option<std::sync::Arc<MixTree>>]) -> [String; BUSES] {
    let mut sources: [Vec<String>; BUSES] = Default::default();
    for s in loaded(sel) {
        let p = &sel.parts[s];
        let name = part_name(p);
        sources[usize::from(p.output).min(BUSES - 1)].push(name.clone());
        for (n, node) in p.nodes.iter().enumerate() {
            if let Some(b) = pair(node) {
                let tree = trees.get(s).and_then(Option::as_deref);
                let mic = tree.and_then(|t| t.nodes.get(n + 1)).map_or(format!("Out {}", n + 2), |m| m.name.clone());
                sources[b].push(mic_label(&name, &mic));
            }
        }
    }
    sources.map(|names| common(&names))
}

pub fn auto_names(sel: &Selection) -> [String; BUSES] {
    auto_names_with(sel, &[])
}

/// Bus `n`'s name: the player's, else what plays through it, else "st.N".
pub fn label(sel: &Selection, n: usize) -> String {
    label_with(sel, &auto_names(sel), n)
}

fn label_with(sel: &Selection, auto: &[String; BUSES], n: usize) -> String {
    let bus = sel.bus(n);
    match auto.get(n) {
        _ if !bus.name.is_empty() => bus.name,
        Some(name) if !name.is_empty() => name.clone(),
        _ => bus.label(n),
    }
}

/// What the host lists each stereo output port as: the pairs playing
/// through it, named; "st.N" when none does.
pub fn port_names(sel: &Selection, trees: &[Option<std::sync::Arc<MixTree>>]) -> [String; BUSES] {
    let auto = auto_names_with(sel, trees);
    std::array::from_fn(|port| {
        let names: Vec<String> = (0..BUSES)
            .filter(|&n| {
                let b = sel.bus(n);
                let to = usize::try_from(b.port).ok().filter(|&p| p < BUSES).unwrap_or(n);
                to == port && (!b.name.is_empty() || !auto[n].is_empty())
            })
            .map(|n| label_with(sel, &auto, n))
            .collect();
        match &names[..] {
            [] => format!("st.{}", port + 1),
            names => common(names),
        }
    })
}

/// Parts listen on A1, A2, … in rack order (B1… past 16).
pub fn own_channels(sel: &mut Selection) {
    for (n, s) in loaded(sel).into_iter().enumerate() {
        let p = &mut sel.parts[s];
        (p.port, p.channel) = ((n / 16).min(3) as u8, (n % 16) as i16);
    }
}

/// Every part hears everything on port A.
pub fn all_omni(sel: &mut Selection) {
    for s in loaded(sel) {
        let p = &mut sel.parts[s];
        (p.port, p.channel) = (0, -1);
    }
}

/// Keep what plays through each bus as its name.
pub fn name_outputs(sel: &mut Selection) {
    let auto = auto_names(sel);
    for (n, name) in auto.into_iter().enumerate() {
        if !name.is_empty() && sel.bus(n).name.is_empty() {
            sel.bus_mut(n).name = name;
        }
    }
}

/// Forget the hand-picked routes: every part and bus routed automatically
/// again (on st.1 in "Stereo mix"), every bus on its own port. Levels,
/// names and sends stay.
pub fn reset(sel: &mut Selection) {
    for p in &mut sel.parts {
        p.output_manual = false;
        for n in &mut p.nodes {
            n.manual = false;
        }
    }
    if Outputs::of(sel.outputs) == Outputs::Stereo {
        to_stereo(sel);
    }
    for n in 0..sel.buses.len() {
        sel.bus_mut(n).port = -1;
    }
}

/// How long new port names must hold before the host hears of them: a
/// host rescans for each change, and parts load one at a time.
pub const SETTLE: Duration = Duration::from_millis(500);

/// Host port names as last published, and a change waiting to settle.
pub struct PortNames {
    pub published: [String; BUSES],
    pending: Option<([String; BUSES], Instant)>,
}

impl Default for PortNames {
    fn default() -> Self {
        Self {
            published: std::array::from_fn(|n| format!("st.{}", n + 1)),
            pending: None,
        }
    }
}

impl PortNames {
    /// Offer the names as of `now`; true when they are published: they
    /// differ from the last published and have held for [`SETTLE`].
    pub fn offer(&mut self, names: [String; BUSES], now: Instant) -> bool {
        if names == self.published {
            self.pending = None;
            return false;
        }
        match &self.pending {
            Some((pending, since)) if *pending == names => {
                if now.duration_since(*since) < SETTLE {
                    return false;
                }
            }
            _ => {
                self.pending = Some((names, now));
                return false;
            }
        }
        self.published = names;
        self.pending = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rack(names: &[&str]) -> Selection {
        let mut sel = Selection::default();
        for n in names {
            sel.parts.push(Part {
                path: format!("/lib/{n}.nki"),
                ..Default::default()
            });
        }
        sel.order = (0..names.len() as u32).collect();
        sel
    }

    fn outputs(sel: &Selection) -> Vec<u8> {
        sel.parts.iter().map(|p| p.output).collect()
    }

    fn apply_now(sel: &mut Selection) {
        apply(sel, &[]);
    }

    #[test]
    fn one_per_instrument_is_the_default_and_stable() {
        let mut sel = rack(&["Harp", "Celli", "Horns"]);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 1, 2], "each instrument on its own pair by default");
        // Remove the middle one: the others keep their pairs.
        sel.parts[1] = Part::default();
        sel.order.retain(|&s| s != 1);
        apply_now(&mut sel);
        assert_eq!((sel.parts[0].output, sel.parts[2].output), (0, 2));
        // Reorder: nothing moves.
        sel.order = vec![2, 0];
        apply_now(&mut sel);
        assert_eq!((sel.parts[0].output, sel.parts[2].output), (0, 2));
        // A new part takes the free pair, a duplicate in rack order gets its own.
        sel.parts[1] = Part { path: "/lib/Flute.nki".into(), ..Default::default() };
        sel.order.push(1);
        let dup = Part { output: 2, ..sel.parts[2].clone() };
        sel.parts.push(dup);
        sel.order.push(3);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 1, 2, 3]);
        // Saved and reloaded: the same routes.
        let again = sel.clone();
        apply_now(&mut sel);
        assert!(sel == again);
        let names = port_names(&sel, &[]);
        assert_eq!(&names[..5], ["Harp", "Flute", "Horns", "Horns", "st.5"]);
        to_stereo(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 0, 0], "choosing stereo: everything on 1-2");
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 0, 0], "and it stays there");
    }

    #[test]
    fn manual_routes_stick() {
        let mut sel = rack(&["Harp", "Celli", "Horns"]);
        sel.parts[2].output = 1;
        sel.parts[2].output_manual = true;
        sel.parts[0].aux = 0;
        apply_now(&mut sel);
        // Horns keep 3-4; Harp leaves 1-2, a send; Celli take the next free.
        assert_eq!(outputs(&sel), [2, 3, 1]);
        to_stereo(&mut sel);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 1], "stereo mix keeps a hand-picked route");
        reset(&mut sel);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 0]);
    }

    #[test]
    fn mic_buses_get_pairs_and_names_groups_stay_inside() {
        let node = |name: &str, kind, parent| MixNode { name: name.into(), kind, parent: Some(parent), inserts: vec![], sends: vec![] };
        let mut harp = MixTree::instrument("Harp");
        harp.nodes.extend([node("Close", NodeKind::Bus, 0), node("Tree", NodeKind::Bus, 0), node("Pluck", NodeKind::Group, 1)]);
        let trees = [Some(std::sync::Arc::new(harp)), None];
        let mut sel = rack(&["Harp", "Celli"]);
        sel.outputs = Outputs::Mic as u8;
        apply(&mut sel, &trees);
        assert_eq!(outputs(&sel), [0, 1]);
        let pairs: Vec<_> = sel.parts[0].nodes.iter().map(|n| n.output).collect();
        assert_eq!(pairs, [NodeOutput::Pair(2), NodeOutput::Pair(3), NodeOutput::Parent]);
        let names = port_names(&sel, &trees);
        assert_eq!(&names[..5], ["Harp", "Celli", "Harp Close", "Harp Tree", "st.5"]);
        // A node routed by hand keeps its pair; the rest go home with one per instrument.
        sel.parts[0].nodes[2] = NodeMix { output: NodeOutput::Pair(9), manual: true, ..NodeMix::default() };
        sel.outputs = Outputs::Instrument as u8;
        apply(&mut sel, &trees);
        let pairs: Vec<_> = sel.parts[0].nodes.iter().map(|n| n.output).collect();
        assert_eq!(pairs, [NodeOutput::Parent, NodeOutput::Parent, NodeOutput::Pair(9)]);
    }

    #[test]
    fn names_and_channels() {
        assert_eq!(common(&["Violins 1".into(), "Violins 2".into()]), "Violins");
        assert_eq!(common(&["Harp".into(), "Celli".into(), "Flute".into()]), "Harp +2");
        let mut sel = rack(&["Violins 1", "Violins 2", "Harp"]);
        to_stereo(&mut sel);
        assert_eq!(port_names(&sel, &[])[0], "Violins 1 +2");
        sel.parts[2].output = 1;
        sel.parts[2].output_manual = true;
        apply_now(&mut sel);
        assert_eq!(port_names(&sel, &[])[..3], ["Violins", "Harp", "st.3"]);
        // Two buses on one port share it; a player's name wins.
        sel.bus_mut(1).port = 0;
        assert_eq!(port_names(&sel, &[])[..2], ["Violins +1", "st.2"]);
        sel.bus_mut(0).name = "Strings".into();
        assert_eq!(label(&sel, 0), "Strings");
        name_outputs(&mut sel);
        assert_eq!(sel.bus(1).name, "Harp");
        own_channels(&mut sel);
        assert_eq!(sel.parts.iter().map(|p| p.channel).collect::<Vec<_>>(), [0, 1, 2]);
        all_omni(&mut sel);
        assert!(sel.parts.iter().all(|p| p.channel == -1 && p.port == 0));
    }

    #[test]
    fn port_names_settle_before_publishing() {
        let mut names = PortNames::default();
        let t = Instant::now();
        let mut harp = names.published.clone();
        harp[0] = "Harp".into();
        assert!(!names.offer(harp.clone(), t), "not at once");
        assert!(!names.offer(harp.clone(), t + SETTLE / 2));
        let mut celli = harp.clone();
        celli[1] = "Celli".into();
        assert!(!names.offer(celli.clone(), t + SETTLE), "a change starts the wait again");
        assert!(!names.offer(celli.clone(), t + SETTLE + SETTLE / 2));
        assert!(names.offer(celli.clone(), t + SETTLE * 2));
        assert_eq!(names.published[1], "Celli");
        assert!(!names.offer(celli, t + SETTLE * 3), "unchanged: nothing to tell");
    }
}
