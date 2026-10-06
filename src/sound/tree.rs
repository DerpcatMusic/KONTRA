//! An instrument's output tree, as plain data for the mixer view.
//!
//! Every rack part owns one [`MixTree`]. Node 0 is the instrument itself and
//! plays to a DAW stereo pair ([`super::mix::PartControls::output`]); every
//! other node is a source bus (Kontakt bus, UVI layer or mic bus) or a group,
//! nested to any depth, and plays to its parent unless its [`NodeMix::output`]
//! sends it to a DAW pair of its own. The core builds the tree when it
//! prepares a part; the shell keeps one [`NodeMix`] per node and hands them
//! back through [`super::mix::PartControls::nodes`].

use serde::{Deserialize, Serialize};

/// What a node stands for in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// The rack part's instrument: the root.
    Instrument,
    /// A bus the source declares (Kontakt bus, UVI layer or mic bus).
    Bus,
    /// A source group (a Kontakt group, a UVI keygroup layer).
    Group,
}

/// One mixer node as the source describes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixNode {
    pub name: String,
    pub kind: NodeKind,
    /// Index of the parent node; `None` only for the root.
    pub parent: Option<usize>,
    /// The source's insert effects on this node, by name, in order.
    pub inserts: Vec<String>,
    /// The source's sends from this node: target node and linear gain.
    pub sends: Vec<(usize, f32)>,
}

/// A part's nodes; node 0 is the root.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MixTree {
    pub nodes: Vec<MixNode>,
}

impl MixTree {
    /// Just the instrument, for sources without buses or groups.
    pub fn instrument(name: &str) -> Self {
        Self {
            nodes: vec![MixNode {
                name: name.into(),
                kind: NodeKind::Instrument,
                parent: None,
                inserts: Vec::new(),
                sends: Vec::new(),
            }],
        }
    }

    /// The children of `node`, in order.
    pub fn children(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        self.nodes
            .iter()
            .enumerate()
            .filter(move |(_, n)| n.parent == Some(node))
            .map(|(i, _)| i)
    }

    /// How deep `node` sits: 0 for the root.
    pub fn depth(&self, mut node: usize) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.nodes.get(node).and_then(|n| n.parent) {
            depth += 1;
            node = parent;
        }
        depth
    }
}

/// Where a node plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeOutput {
    /// Into its parent node.
    #[default]
    Parent,
    /// Straight to a DAW stereo pair, `0` being outputs 1-2.
    Pair(u8),
}

/// The user's settings for one non-root node; the root's are the part's own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeMix {
    /// Decibels.
    pub gain: f32,
    /// Balance, −1..=1.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub output: NodeOutput,
    /// The player picked `output`: automatic routing leaves it.
    pub manual: bool,
}

impl Default for NodeMix {
    fn default() -> Self {
        Self { gain: 0.0, pan: 0.0, mute: false, solo: false, output: NodeOutput::Parent, manual: false }
    }
}

/// Left/right gains of a node's fader, pan and mute; `audible` is the
/// part-wide solo verdict for it ([`audible`]).
pub fn stereo_gain(mix: &NodeMix, audible: bool) -> [f32; 2] {
    if mix.mute || !audible {
        return [0.0; 2];
    }
    super::mix::balance(super::mix::db_gain(mix.gain), mix.pan)
}

/// Which nodes sound under solo: with any node soloed, those soloed, their
/// ancestors (which carry them) and their descendants (which they carry).
pub fn audible(tree: &MixTree, mixes: &[NodeMix], out: &mut [bool]) {
    let soloed = |n: usize| n > 0 && mixes.get(n - 1).is_some_and(|m| m.solo);
    let any = (1..tree.nodes.len()).any(soloed);
    for (n, slot) in out.iter_mut().enumerate().take(tree.nodes.len()) {
        *slot = !any || {
            // A soloed node on `n`'s path to the root carries it down...
            let mut at = Some(n);
            let mut hit = false;
            while let Some(i) = at {
                hit |= soloed(i);
                at = tree.nodes[i].parent;
            }
            // ...and `n` carries any soloed node below it up.
            hit || carries_solo(tree, n, &soloed)
        };
    }
}

/// Whether some soloed node has `n` on its path to the root.
fn carries_solo(tree: &MixTree, n: usize, soloed: &dyn Fn(usize) -> bool) -> bool {
    (1..tree.nodes.len()).filter(|&s| soloed(s)).any(|mut s| loop {
        if s == n {
            break true;
        }
        match tree.nodes[s].parent {
            Some(p) => s = p,
            None => break false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, parent: usize) -> MixNode {
        MixNode { name: name.into(), kind: NodeKind::Bus, parent: Some(parent), inserts: vec![], sends: vec![] }
    }

    #[test]
    fn solo_keeps_the_soloed_path_and_its_children() {
        let mut tree = MixTree::instrument("piano");
        // 1 close, 2 room, 3 close/hammer
        tree.nodes.extend([node("close", 0), node("room", 0), node("hammer", 1)]);
        assert_eq!(tree.depth(3), 2);
        assert_eq!(tree.children(0).collect::<Vec<_>>(), [1, 2]);
        let mut mixes = vec![NodeMix::default(); 3];
        let mut out = [false; 4];
        audible(&tree, &mixes, &mut out);
        assert_eq!(out, [true; 4]);
        mixes[0].solo = true;
        audible(&tree, &mixes, &mut out);
        assert_eq!(out, [true, true, false, true]);
        mixes[0].solo = false;
        mixes[2].solo = true;
        audible(&tree, &mixes, &mut out);
        assert_eq!(out, [true, true, false, true]);
        assert_eq!(stereo_gain(&NodeMix { mute: true, ..NodeMix::default() }, true), [0.0; 2]);
    }
}
