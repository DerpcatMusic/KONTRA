//! Dynamic multi-channel audio routing DAG (IR vNext Phase 3).
//!
//! Replaces rigid fixed-rack chains with a flat topological [`AudioGraph`]
//! supporting pre/post-amp splits (Kontakt `fx_idx_amp_split_point`), parallel
//! wet/dry racks (UVI `EffectRack` / `FeedbackMachine`), sidechain taps, and
//! multi-channel stem outputs while keeping a stereo fast path.

use crate::{Diagnostic, Effect, Impact, Instrument};

/// 4-byte handle into [`AudioGraph::effects`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EffectId(pub u32);

const _: () = assert!(std::mem::size_of::<EffectId>() == 4);

/// 2-byte handle into [`AudioGraph::nodes`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NodeId(pub u16);

const _: () = assert!(std::mem::size_of::<NodeId>() == 2);

/// Channel topology of a graph node or output stem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChannelLayout {
    Mono,
    #[default]
    Stereo,
    /// Arbitrary `n`-channel stem layout (clamped to >= 1 when queried).
    Multi(u8),
    Surround51,
    Surround71,
    /// Ambisonics of order `order` (`(order + 1)^2` channels).
    Ambisonics {
        order: u8,
    },
}

impl ChannelLayout {
    /// Physical channel count for this layout.
    #[must_use]
    pub const fn channels(self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Multi(n) => {
                if n == 0 {
                    1
                } else {
                    n
                }
            }
            Self::Surround51 => 6,
            Self::Surround71 => 8,
            Self::Ambisonics { order } => {
                let side = order.saturating_add(1);
                side.saturating_mul(side)
            }
        }
    }

    /// True when the engine can run the stereo fast path (`[f32; 2]`).
    #[must_use]
    pub const fn is_stereo_fast_path(self) -> bool {
        matches!(self, Self::Stereo)
    }
}

/// What a node in an [`AudioGraph`] does.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphNodeKind {
    /// Voice sum for `group` entering the graph.
    GroupInput { group: u16 },
    /// Mixer bus with optional serial effects, fader gain (dB), and pan (`-1..=1`).
    Bus {
        name: String,
        effects: Vec<EffectId>,
        gain_db: f32,
        pan: f32,
        layout: ChannelLayout,
        to_main: bool,
    },
    /// Serial effect rack without fader/pan.
    Rack {
        effects: Vec<EffectId>,
        layout: ChannelLayout,
    },
    /// Pre-amp / post-amp split rack (Kontakt `fx_idx_amp_split_point` or UVI parallel chains).
    Split {
        pre_amp: Vec<EffectId>,
        post_amp: Vec<EffectId>,
        amp_gain: f32,
        amp_pan: f32,
    },
    /// Pure summing / pan / gain mixer node.
    Mix { gain: f32, pan: f32 },
    /// Terminal plugin output stem (`bus_index = 0` is Main Out).
    Output {
        bus_index: u8,
        layout: ChannelLayout,
    },
}

/// One node in an [`AudioGraph`].
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: GraphNodeKind,
}

/// Directed audio or sidechain edge between two [`GraphNode`]s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphEdge {
    pub from: NodeId,
    pub to: NodeId,
    /// Linear send gain.
    pub gain: f32,
    /// True when tapped after the source node's fader/gain stage.
    pub post_fader: bool,
    /// True when feeding the destination node's sidechain detector rather than audio input.
    pub sidechain: bool,
}

/// Flat audio routing DAG with interned [`Effect`] pool indexed by 4-byte [`EffectId`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub effects: Vec<Effect>,
}

impl AudioGraph {
    /// Interns an [`Effect`] into [`Self::effects`] and returns its 4-byte [`EffectId`].
    pub fn push_effect(&mut self, effect: Effect) -> EffectId {
        let id = EffectId(u32::try_from(self.effects.len()).unwrap_or(u32::MAX));
        self.effects.push(effect);
        id
    }

    /// Appends a [`GraphNode`] and returns its [`NodeId`].
    pub fn push_node(&mut self, kind: GraphNodeKind) -> NodeId {
        let id = NodeId(u16::try_from(self.nodes.len()).unwrap_or(u16::MAX));
        self.nodes.push(GraphNode { id, kind });
        id
    }

    /// Connects `from -> to` with a linear `gain` edge.
    pub fn connect(&mut self, from: NodeId, to: NodeId, gain: f32, post_fader: bool) {
        self.edges.push(GraphEdge {
            from,
            to,
            gain,
            post_fader,
            sidechain: false,
        });
    }

    /// Connects `from -> to` as a sidechain detector feed.
    pub fn connect_sidechain(&mut self, from: NodeId, to: NodeId, gain: f32) {
        self.edges.push(GraphEdge {
            from,
            to,
            gain,
            post_fader: true,
            sidechain: true,
        });
    }

    /// Compiles an existing [`Instrument`] (groups, buses, bus sends, insert/send/main
    /// racks) into a flat topological [`AudioGraph`].
    #[must_use]
    pub fn from_instrument(instrument: &Instrument) -> Self {
        let mut graph = Self::default();
        let intern_rack = |g: &mut Self, slots: &[Option<Effect>]| -> Vec<EffectId> {
            slots
                .iter()
                .flatten()
                .cloned()
                .map(|eff| g.push_effect(eff))
                .collect()
        };

        // 1. Parallel send effect nodes (connected to `main_node` only when tapped).
        let send_nodes: Vec<Option<NodeId>> = instrument
            .sends
            .iter()
            .map(|slot| {
                slot.as_ref().map(|eff| {
                    let eid = graph.push_effect(eff.clone());
                    graph.push_node(GraphNodeKind::Rack {
                        effects: vec![eid],
                        layout: ChannelLayout::Stereo,
                    })
                })
            })
            .collect();
        let mut send_tapped = vec![false; send_nodes.len()];

        // Splits a slot list at any unbypassed `EffectKind::Sends` tap so each send
        // slot receives the signal at that point in the chain scaled by its tap level.
        let lower_chain_with_sends = |g: &mut Self,
                                      slots: &[Option<Effect>],
                                      upstream: NodeId,
                                      send_tapped: &mut [bool]|
         -> NodeId {
            let mut current = upstream;
            let mut pending = Vec::new();
            for eff in slots.iter().flatten() {
                if let crate::EffectKind::Sends(levels) = eff.kind {
                    if !pending.is_empty() {
                        let next = g.push_node(GraphNodeKind::Rack {
                            effects: std::mem::take(&mut pending),
                            layout: ChannelLayout::Stereo,
                        });
                        g.connect(current, next, 1.0, true);
                        current = next;
                    }
                    if !eff.bypass {
                        for (si, &lvl) in levels.iter().enumerate() {
                            if lvl != 0.0
                                && let Some(Some(snode)) = send_nodes.get(si)
                            {
                                g.connect(current, *snode, lvl, true);
                                if let Some(tapped) = send_tapped.get_mut(si) {
                                    *tapped = true;
                                }
                            }
                        }
                    }
                } else {
                    pending.push(g.push_effect(eff.clone()));
                }
            }
            if !pending.is_empty() {
                let next = g.push_node(GraphNodeKind::Rack {
                    effects: pending,
                    layout: ChannelLayout::Stereo,
                });
                g.connect(current, next, 1.0, true);
                current = next;
            }
            current
        };

        // 2. Instrument buses (or a single default main bus when empty).
        // Track `(entry_node, exit_node)` so bus-level `EffectKind::Sends` taps
        // run before the bus fader while groups & incoming sends feed `entry_node`.
        let mut bus_nodes: Vec<(NodeId, NodeId)> = Vec::new();
        if instrument.buses.is_empty() {
            let id = graph.push_node(GraphNodeKind::Bus {
                name: "Main".into(),
                effects: Vec::new(),
                gain_db: 0.0,
                pan: 0.0,
                layout: ChannelLayout::Stereo,
                to_main: true,
            });
            bus_nodes.push((id, id));
        } else {
            for bus in &instrument.buses {
                let has_sends = bus
                    .inserts
                    .iter()
                    .flatten()
                    .any(|e| matches!(e.kind, crate::EffectKind::Sends(_)));
                if has_sends {
                    let bus_in = graph.push_node(GraphNodeKind::Mix {
                        gain: 1.0,
                        pan: 0.0,
                    });
                    let post_fx =
                        lower_chain_with_sends(&mut graph, &bus.inserts, bus_in, &mut send_tapped);
                    let id = graph.push_node(GraphNodeKind::Bus {
                        name: bus.name.clone(),
                        effects: Vec::new(),
                        gain_db: bus.volume_db,
                        pan: bus.pan,
                        layout: ChannelLayout::Stereo,
                        to_main: bus.to_main,
                    });
                    graph.connect(post_fx, id, 1.0, true);
                    bus_nodes.push((bus_in, id));
                } else {
                    let effects = intern_rack(&mut graph, &bus.inserts);
                    let id = graph.push_node(GraphNodeKind::Bus {
                        name: bus.name.clone(),
                        effects,
                        gain_db: bus.volume_db,
                        pan: bus.pan,
                        layout: ChannelLayout::Stereo,
                        to_main: bus.to_main,
                    });
                    bus_nodes.push((id, id));
                }
            }
        }

        // 3. Group input nodes routed to their target bus entry.
        for (gi, group) in instrument.groups.iter().enumerate() {
            let gid = graph.push_node(GraphNodeKind::GroupInput {
                group: u16::try_from(gi).unwrap_or(u16::MAX),
            });
            if let Some(&(bus_in, _)) = bus_nodes.get(group.bus).or_else(|| bus_nodes.first()) {
                graph.connect(gid, bus_in, 1.0, false);
            }
        }

        // 4. Bus-to-bus sends.
        for (bi, bus) in instrument.buses.iter().enumerate() {
            let Some(&(_, from_out)) = bus_nodes.get(bi) else {
                continue;
            };
            for send in &bus.sends {
                if let Some(&(to_in, _)) = bus_nodes.get(send.bus) {
                    graph.connect(from_out, to_in, send.level, send.post_fader);
                }
            }
        }

        // 5. Instrument insert rack (with `EffectKind::Sends` taps), main rack, and main output.
        let insert_in = graph.push_node(GraphNodeKind::Mix {
            gain: 1.0,
            pan: 0.0,
        });
        for (bi, &(_, bnode_out)) in bus_nodes.iter().enumerate() {
            let to_main = instrument.buses.get(bi).is_none_or(|b| b.to_main);
            if to_main {
                graph.connect(bnode_out, insert_in, 1.0, true);
            }
        }
        let insert_out =
            lower_chain_with_sends(&mut graph, &instrument.inserts, insert_in, &mut send_tapped);

        let main_fx = intern_rack(&mut graph, &instrument.main);
        let main_node = graph.push_node(GraphNodeKind::Rack {
            effects: main_fx,
            layout: ChannelLayout::Stereo,
        });
        graph.connect(insert_out, main_node, 1.0, true);

        for (snode, tapped) in send_nodes.iter().zip(send_tapped.iter()) {
            if *tapped && let Some(snode) = *snode {
                graph.connect(snode, main_node, 1.0, true);
            }
        }

        let out_node = graph.push_node(GraphNodeKind::Output {
            bus_index: 0,
            layout: ChannelLayout::Stereo,
        });
        graph.connect(main_node, out_node, 1.0, true);

        graph
    }

    /// Computes a deterministic Kahn topological execution order for the DAG.
    /// Returns `Err(node)` naming a node participating in a cycle if the graph is cyclic.
    pub fn topological_order(&self) -> Result<Vec<NodeId>, NodeId> {
        let n = self.nodes.len();
        let mut indegree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

        for edge in &self.edges {
            let from = usize::from(edge.from.0);
            let to = usize::from(edge.to.0);
            if from < n && to < n {
                if let Some(list) = adj.get_mut(from) {
                    list.push(to);
                }
                if let Some(deg) = indegree.get_mut(to) {
                    *deg += 1;
                }
            }
        }

        let mut queue = std::collections::VecDeque::new();
        for (i, &deg) in indegree.iter().enumerate() {
            if deg == 0 {
                queue.push_back(i);
            }
        }

        let mut order = Vec::with_capacity(n);
        while let Some(u) = queue.pop_front() {
            order.push(NodeId(u16::try_from(u).unwrap_or(u16::MAX)));
            if let Some(neighbors) = adj.get(u) {
                for &v in neighbors {
                    if let Some(deg) = indegree.get_mut(v) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(v);
                        }
                    }
                }
            }
        }

        if order.len() != n {
            let cyclic = indegree.iter().position(|&d| d > 0).unwrap_or(0);
            return Err(NodeId(u16::try_from(cyclic).unwrap_or(u16::MAX)));
        }
        Ok(order)
    }

    /// Validates all node, edge, and effect indices plus DAG acyclicity.
    #[must_use]
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let node_count = self.nodes.len();
        let fx_count = self.effects.len();

        for (ni, node) in self.nodes.iter().enumerate() {
            if usize::from(node.id.0) != ni {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.graph.node_id_mismatch".into(),
                    detail: format!("node at index {ni} has mismatched id {}", node.id.0),
                });
            }
            let fx_ids: &[EffectId] = match &node.kind {
                GraphNodeKind::Bus { effects, .. } | GraphNodeKind::Rack { effects, .. } => {
                    effects.as_slice()
                }
                GraphNodeKind::Split {
                    pre_amp, post_amp, ..
                } => {
                    for &eid in pre_amp.iter().chain(post_amp.iter()) {
                        if eid.0 as usize >= fx_count {
                            out.push(Diagnostic {
                                impact: Impact::Broken,
                                feature: "ir.graph.effect_out_of_bounds".into(),
                                detail: format!(
                                    "node {ni} references effect {} (effects: {fx_count})",
                                    eid.0
                                ),
                            });
                        }
                    }
                    &[]
                }
                GraphNodeKind::GroupInput { .. }
                | GraphNodeKind::Mix { .. }
                | GraphNodeKind::Output { .. } => &[],
            };
            for &eid in fx_ids {
                if eid.0 as usize >= fx_count {
                    out.push(Diagnostic {
                        impact: Impact::Broken,
                        feature: "ir.graph.effect_out_of_bounds".into(),
                        detail: format!(
                            "node {ni} references effect {} (effects: {fx_count})",
                            eid.0
                        ),
                    });
                }
            }
        }

        for (ei, edge) in self.edges.iter().enumerate() {
            if usize::from(edge.from.0) >= node_count || usize::from(edge.to.0) >= node_count {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.graph.edge_out_of_bounds".into(),
                    detail: format!(
                        "edge {ei} connects {} -> {} (nodes: {node_count})",
                        edge.from.0, edge.to.0
                    ),
                });
            }
            if !edge.gain.is_finite() {
                out.push(Diagnostic {
                    impact: Impact::Broken,
                    feature: "ir.graph.non_finite_gain".into(),
                    detail: format!("edge {ei} has non-finite gain {}", edge.gain),
                });
            }
            if edge.sidechain && edge.gain != 0.0 {
                out.push(Diagnostic {
                    impact: Impact::Audible,
                    feature: "ir.graph.sidechain_unsupported".into(),
                    detail: format!(
                        "edge {ei} ({} -> {}) sidechain feed is not carried by the runtime",
                        edge.from.0, edge.to.0
                    ),
                });
            }
        }

        if let Err(cyclic) = self.topological_order() {
            out.push(Diagnostic {
                impact: Impact::Broken,
                feature: "ir.graph.cycle".into(),
                detail: format!("audio graph contains a cycle involving node {}", cyclic.0),
            });
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bus, BusSend, EffectKind, Group};

    #[test]
    fn channel_layouts_report_exact_widths_and_stereo_fast_path() {
        assert_eq!(ChannelLayout::Mono.channels(), 1);
        assert_eq!(ChannelLayout::Stereo.channels(), 2);
        assert!(ChannelLayout::Stereo.is_stereo_fast_path());
        assert!(!ChannelLayout::Surround51.is_stereo_fast_path());
        assert_eq!(ChannelLayout::Surround51.channels(), 6);
        assert_eq!(ChannelLayout::Surround71.channels(), 8);
        assert_eq!(ChannelLayout::Multi(16).channels(), 16);
        assert_eq!(ChannelLayout::Ambisonics { order: 1 }.channels(), 4);
        assert_eq!(ChannelLayout::Ambisonics { order: 3 }.channels(), 16);
    }

    #[test]
    fn instrument_compiles_to_valid_acyclic_audio_graph() {
        let mut bus0 = Bus::new("Close");
        bus0.sends.push(BusSend {
            bus: 1,
            level: 0.5,
            post_fader: true,
        });
        let bus1 = Bus::new("Hall");
        let inst = Instrument {
            groups: vec![Group {
                bus: 0,
                ..Group::DEFAULT
            }],
            buses: vec![bus0, bus1],
            inserts: vec![Some(Effect {
                kind: EffectKind::Gain(0.9),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            })],
            ..Instrument::DEFAULT
        };
        let graph = AudioGraph::from_instrument(&inst);
        assert!(graph.validate().is_empty(), "{:?}", graph.validate());
        assert_eq!(graph.effects.len(), 1);
        let order = graph.topological_order();
        assert!(order.is_ok());
    }

    #[test]
    fn cyclic_audio_graph_is_detected_by_validation() {
        let mut graph = AudioGraph::default();
        let a = graph.push_node(GraphNodeKind::Mix {
            gain: 1.0,
            pan: 0.0,
        });
        let b = graph.push_node(GraphNodeKind::Mix {
            gain: 1.0,
            pan: 0.0,
        });
        graph.connect(a, b, 1.0, true);
        graph.connect(b, a, 0.5, true);
        let diags = graph.validate();
        assert!(
            diags.iter().any(|d| d.feature == "ir.graph.cycle"),
            "{diags:?}"
        );
    }

    #[test]
    fn from_instrument_wires_send_tap_levels_and_omits_untapped_sends() {
        let mut levels = [0.0f32; 8];
        levels[0] = 0.25;
        let inst = Instrument {
            groups: vec![Group::DEFAULT],
            inserts: vec![Some(Effect {
                kind: EffectKind::Sends(levels),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            })],
            sends: vec![
                Some(Effect {
                    kind: EffectKind::Gain(1.0),
                    bypass: false,
                    output: 1.0,
                    dry: 0.0,
                }),
                Some(Effect {
                    kind: EffectKind::Gain(1.0),
                    bypass: false,
                    output: 1.0,
                    dry: 0.0,
                }),
            ],
            ..Instrument::DEFAULT
        };
        let graph = AudioGraph::from_instrument(&inst);
        assert!(graph.validate().is_empty(), "{:?}", graph.validate());
        // Send slot 0 is node 0 (tapped at 0.25 and routed to main); send slot 1 is node 1 (untapped).
        let to_s0: Vec<_> = graph.edges.iter().filter(|e| e.to == NodeId(0)).collect();
        let from_s0: Vec<_> = graph.edges.iter().filter(|e| e.from == NodeId(0)).collect();
        let to_s1: Vec<_> = graph.edges.iter().filter(|e| e.to == NodeId(1)).collect();
        let from_s1: Vec<_> = graph.edges.iter().filter(|e| e.from == NodeId(1)).collect();
        assert_eq!(to_s0.len(), 1);
        assert!((to_s0[0].gain - 0.25).abs() < 1e-6);
        assert_eq!(from_s0.len(), 1);
        assert!(to_s1.is_empty());
        assert!(from_s1.is_empty());
    }

    #[test]
    fn active_sidechain_edge_is_reported_by_validation() {
        let mut graph = AudioGraph::default();
        let a = graph.push_node(GraphNodeKind::Mix {
            gain: 1.0,
            pan: 0.0,
        });
        let b = graph.push_node(GraphNodeKind::Mix {
            gain: 1.0,
            pan: 0.0,
        });
        graph.connect_sidechain(a, b, 1.0);
        let diags = graph.validate();
        assert!(
            diags
                .iter()
                .any(|d| d.feature == "ir.graph.sidechain_unsupported"),
            "{diags:?}"
        );
    }
}
