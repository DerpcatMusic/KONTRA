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
    /// A microphone position's bus.
    Mic,
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
        Self {
            gain: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: NodeOutput::Parent,
            manual: false,
        }
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
    for slot in out.iter_mut().take(tree.nodes.len()) {
        *slot = !any;
    }
    if !any {
        return;
    }
    // Each explicitly soloed node carries its ancestors. Mark those paths
    // once, rather than searching every possible solo for every tree node.
    for n in (1..tree.nodes.len()).filter(|&n| soloed(n)) {
        let mut at = Some(n);
        while let Some(i) = at {
            if let Some(slot) = out.get_mut(i) {
                *slot = true;
            }
            at = tree.nodes[i].parent;
        }
    }
    // Descendants sound only if an explicitly soloed node is on their own
    // path. A marked ancestor may merely carry a sibling's solo and must not
    // turn that sibling's whole branch on. Node order need not be topological.
    for (n, slot) in out.iter_mut().enumerate().take(tree.nodes.len()) {
        if *slot {
            continue;
        }
        let mut at = Some(n);
        while let Some(i) = at {
            if soloed(i) {
                *slot = true;
                break;
            }
            at = tree.nodes[i].parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, parent: usize) -> MixNode {
        MixNode {
            name: name.into(),
            kind: NodeKind::Bus,
            parent: Some(parent),
            inserts: vec![],
            sends: vec![],
        }
    }

    #[test]
    fn solo_keeps_the_soloed_path_and_its_children() {
        let mut tree = MixTree::instrument("piano");
        // 1 close, 2 room, 3 close/hammer
        tree.nodes
            .extend([node("close", 0), node("room", 0), node("hammer", 1)]);
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
        assert_eq!(
            stereo_gain(
                &NodeMix {
                    mute: true,
                    ..NodeMix::default()
                },
                true
            ),
            [0.0; 2]
        );
    }

    #[test]
    fn every_authored_solo_and_mute_combination_keeps_exact_paths() {
        let mut tree = MixTree::instrument("branches");
        // Include parents after children: source bus order is not required to
        // be tree traversal order. Two leaves share an ancestor, not a solo.
        tree.nodes.extend([
            node("left child", 4),
            node("right child", 5),
            node("left sibling", 4),
            node("left", 0),
            node("right", 0),
            node("right grandchild", 2),
        ]);
        let ancestor = |candidate: usize, mut n: usize| loop {
            if candidate == n {
                break true;
            }
            match tree.nodes[n].parent {
                Some(parent) => n = parent,
                None => break false,
            }
        };
        let mut mixes = vec![NodeMix::default(); 6];
        // Four states per node: open, solo, mute, muted solo. Muting remains a
        // separate fader decision and never removes that node's solo request.
        for combination in 0..4usize.pow(6) {
            for (n, mix) in mixes.iter_mut().enumerate() {
                let state = combination >> (n * 2) & 3;
                mix.solo = state & 1 != 0;
                mix.mute = state & 2 != 0;
            }
            let any = mixes.iter().any(|m| m.solo);
            let expected: Vec<_> = (0..tree.nodes.len())
                .map(|n| {
                    !any || mixes
                        .iter()
                        .enumerate()
                        .any(|(s, m)| m.solo && (ancestor(n, s + 1) || ancestor(s + 1, n)))
                })
                .collect();
            let mut actual = vec![false; tree.nodes.len() + 1];
            actual[tree.nodes.len()] = true;
            audible(&tree, &mixes, &mut actual);
            assert_eq!(
                &actual[..tree.nodes.len()],
                expected,
                "combination {combination}"
            );
            assert!(
                actual[tree.nodes.len()],
                "unowned output slots stay unchanged"
            );
            let mut short = [false; 3];
            audible(&tree, &mixes, &mut short);
            assert_eq!(short, expected[..3]);
            for (n, mix) in mixes.iter().enumerate() {
                assert_eq!(
                    stereo_gain(mix, actual[n + 1]),
                    if mix.mute || !expected[n + 1] {
                        [0.; 2]
                    } else {
                        [1.; 2]
                    }
                );
            }
        }
    }

    #[test]
    fn large_solo_tree_preserves_sparse_paths_without_allocating() {
        let mut tree = MixTree::instrument("large");
        tree.nodes.extend((0..10_000).map(|_| node("source", 0)));
        let mut mixes = vec![NodeMix::default(); 10_000];
        mixes[7_321].solo = true;
        let mut out = vec![false; tree.nodes.len()];
        let started = std::time::Instant::now();
        #[cfg(feature = "plugin")]
        let allocations = crate::plugin::tests::allocations(|| audible(&tree, &mixes, &mut out));
        #[cfg(not(feature = "plugin"))]
        audible(&tree, &mixes, &mut out);
        assert!(out[0] && out[7_322]);
        assert_eq!(out.iter().filter(|&&on| on).count(), 2);
        #[cfg(feature = "plugin")]
        assert_eq!(allocations, 0, "solo mask is an audio-thread operation");
        eprintln!(
            "SOLO_MASK nodes={} elapsed_us={}",
            tree.nodes.len(),
            started.elapsed().as_micros()
        );
    }
}
