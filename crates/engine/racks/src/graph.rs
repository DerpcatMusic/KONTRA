//! Allocation-free real-time executor for [`kontra_ir::AudioGraph`] DAGs
//! (IR vNext Phase 3).
//!
//! All topological ordering, [`Rack`] processor allocation, and per-node audio
//! buffers are prepared off the audio thread in [`GraphRuntime::new`]. On the
//! audio thread, [`GraphRuntime::process`] runs strictly in topological order
//! with zero heap allocations, supporting parallel split racks, pre/post-fader
//! bus sends, and multi-output stem routing.

use kontra_effects::matrix as effects;
use kontra_ir as ir;
use kontra_plan::Response;

use crate::rack::Rack;

struct CompiledEdge {
    to: usize,
    gain: f32,
    post_fader: bool,
    sidechain: bool,
}

enum RuntimeNodeKind {
    GroupInput {
        group: u16,
    },
    Bus {
        rack: Rack,
        gain: f32,
        pan: f32,
    },
    Rack {
        rack: Rack,
    },
    Split {
        pre: Rack,
        post: Rack,
        amp_gain: f32,
        amp_pan: f32,
    },
    Mix {
        gain: f32,
        pan: f32,
    },
    Output {
        bus_index: u8,
    },
}

struct RuntimeNode {
    kind: RuntimeNodeKind,
    edges: Box<[CompiledEdge]>,
    buffer: Box<[[f32; 2]]>,
    /// Pre-fader scratch copy used only when a node has pre-fader outgoing sends.
    pre_fader: Option<Box<[[f32; 2]]>>,
    /// Sidechain detector buffer allocated only when a node has incoming sidechain edges.
    sidechain: Option<Box<[[f32; 2]]>>,
}

/// Real-time allocation-free audio DAG executor compiled from [`ir::AudioGraph`].
pub struct GraphRuntime {
    nodes: Box<[RuntimeNode]>,
    order: Box<[usize]>,
    max_frames: usize,
}

impl GraphRuntime {
    /// Compiles `graph` for blocks up to `max_frames` at `rate`.
    /// Returns `Err(node)` if the graph contains a cycle.
    pub fn new(
        graph: &ir::AudioGraph,
        rate: u32,
        max_frames: usize,
        impulses: &[Option<Response>],
    ) -> Result<Self, ir::NodeId> {
        let topo = graph.topological_order()?;
        let order: Box<[usize]> = topo.into_iter().map(|n| usize::from(n.0)).collect();
        let node_count = graph.nodes.len();

        let mut outgoing: Vec<Vec<CompiledEdge>> = (0..node_count).map(|_| Vec::new()).collect();
        let mut has_sidechain_in = vec![false; node_count];
        for e in &graph.edges {
            let from = usize::from(e.from.0);
            let to = usize::from(e.to.0);
            if from < node_count
                && to < node_count
                && let Some(list) = outgoing.get_mut(from)
            {
                if e.sidechain
                    && let Some(flag) = has_sidechain_in.get_mut(to)
                {
                    *flag = true;
                }
                list.push(CompiledEdge {
                    to,
                    gain: e.gain,
                    post_fader: e.post_fader,
                    sidechain: e.sidechain,
                });
            }
        }

        let build_rack = |ids: &[ir::EffectId]| -> Rack {
            let slots: Vec<Option<ir::Effect>> = ids
                .iter()
                .map(|id| graph.effects.get(id.0 as usize).cloned())
                .collect();
            Rack::new(&slots, rate, impulses)
        };

        let mut nodes = Vec::with_capacity(node_count);
        for (i, node) in graph.nodes.iter().enumerate() {
            let edges = outgoing
                .get_mut(i)
                .map(std::mem::take)
                .unwrap_or_default()
                .into_boxed_slice();
            let has_pre_fader = edges.iter().any(|e| !e.post_fader && !e.sidechain);
            let kind = match &node.kind {
                ir::GraphNodeKind::GroupInput { group } => {
                    RuntimeNodeKind::GroupInput { group: *group }
                }
                ir::GraphNodeKind::Bus {
                    effects,
                    gain_db,
                    pan,
                    ..
                } => RuntimeNodeKind::Bus {
                    rack: build_rack(effects),
                    gain: ir::common::DB_GAIN.eval_unclamped(*gain_db),
                    pan: *pan,
                },
                ir::GraphNodeKind::Rack { effects, .. } => RuntimeNodeKind::Rack {
                    rack: build_rack(effects),
                },
                ir::GraphNodeKind::Split {
                    pre_amp,
                    post_amp,
                    amp_gain,
                    amp_pan,
                } => RuntimeNodeKind::Split {
                    pre: build_rack(pre_amp),
                    post: build_rack(post_amp),
                    amp_gain: *amp_gain,
                    amp_pan: *amp_pan,
                },
                ir::GraphNodeKind::Mix { gain, pan } => RuntimeNodeKind::Mix {
                    gain: *gain,
                    pan: *pan,
                },
                ir::GraphNodeKind::Output { bus_index, .. } => RuntimeNodeKind::Output {
                    bus_index: *bus_index,
                },
            };
            let sidechain_in = has_sidechain_in.get(i).copied().unwrap_or(false);
            nodes.push(RuntimeNode {
                kind,
                edges,
                buffer: vec![[0.0; 2]; max_frames].into_boxed_slice(),
                pre_fader: has_pre_fader.then(|| vec![[0.0; 2]; max_frames].into_boxed_slice()),
                sidechain: sidechain_in.then(|| vec![[0.0; 2]; max_frames].into_boxed_slice()),
            });
        }

        Ok(Self {
            nodes: nodes.into_boxed_slice(),
            order,
            max_frames,
        })
    }

    /// Rebuilds every node's effect rack with updated convolution responses.
    /// Not real-time.
    pub fn rebuild(&mut self, rate: u32, responses: kontra_effects::processors::Responses<'_>) {
        for node in &mut self.nodes {
            match &mut node.kind {
                RuntimeNodeKind::Bus { rack, .. } | RuntimeNodeKind::Rack { rack } => {
                    rack.rebuild(rate, responses);
                }
                RuntimeNodeKind::Split { pre, post, .. } => {
                    pre.rebuild(rate, responses);
                    post.rebuild(rate, responses);
                }
                RuntimeNodeKind::GroupInput { .. }
                | RuntimeNodeKind::Mix { .. }
                | RuntimeNodeKind::Output { .. } => {}
            }
        }
    }

    /// Number of compiled nodes in the DAG.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of stereo output stems addressed by [`ir::GraphNodeKind::Output`] nodes (at least 1).
    #[must_use]
    pub fn output_count(&self) -> usize {
        self.nodes
            .iter()
            .filter_map(|n| match n.kind {
                RuntimeNodeKind::Output { bus_index } => Some(usize::from(bus_index) + 1),
                _ => None,
            })
            .max()
            .unwrap_or(1)
    }

    /// Clears all node audio and sidechain buffers before injecting group audio for a block.
    pub fn clear(&mut self, frames: usize) {
        let len = frames.min(self.max_frames);
        for node in &mut self.nodes {
            if let Some(buf) = node.buffer.get_mut(..len) {
                buf.fill([0.0; 2]);
            }
            if let Some(sc) = node.sidechain.as_mut().and_then(|b| b.get_mut(..len)) {
                sc.fill([0.0; 2]);
            }
        }
    }

    /// Injects summed stereo audio for `group` into every matching `GroupInput` node.
    pub fn add_group_audio(&mut self, group: u16, audio: &[[f32; 2]]) {
        let len = audio.len().min(self.max_frames);
        for node in &mut self.nodes {
            if let RuntimeNodeKind::GroupInput { group: g } = node.kind
                && g == group
                && let (Some(dst), Some(src)) = (node.buffer.get_mut(..len), audio.get(..len))
            {
                for (d, s) in dst.iter_mut().zip(src.iter()) {
                    d[0] += s[0];
                    d[1] += s[1];
                }
            }
        }
    }

    /// Injects redirected stereo audio directly into the `bus_index`-th `Bus`
    /// node, or falls back to any `Output { bus_index }` node when the graph has
    /// no bus at that index. Returns `true` if a destination node accepted it.
    pub fn add_bus_audio(&mut self, bus_index: usize, audio: &[[f32; 2]]) -> bool {
        let len = audio.len().min(self.max_frames);
        let Some(src) = audio.get(..len) else {
            return false;
        };
        let mut seen_bus = 0usize;
        for node in &mut self.nodes {
            if matches!(node.kind, RuntimeNodeKind::Bus { .. }) {
                if seen_bus == bus_index {
                    if let Some(dst) = node.buffer.get_mut(..len) {
                        for (d, s) in dst.iter_mut().zip(src.iter()) {
                            d[0] += s[0];
                            d[1] += s[1];
                        }
                    }
                    return true;
                }
                seen_bus += 1;
            }
        }
        let mut delivered = false;
        for node in &mut self.nodes {
            if let RuntimeNodeKind::Output { bus_index: idx } = node.kind
                && usize::from(idx) == bus_index
                && let Some(dst) = node.buffer.get_mut(..len)
            {
                for (d, s) in dst.iter_mut().zip(src.iter()) {
                    d[0] += s[0];
                    d[1] += s[1];
                }
                delivered = true;
            }
        }
        delivered
    }

    /// Processes the DAG in topological order with zero heap allocations and
    /// sums terminal [`ir::GraphNodeKind::Output`] nodes into `outputs[bus_index]`.
    pub fn process(&mut self, frames: usize, tempo: f32, outputs: &mut [&mut [[f32; 2]]]) {
        let len = frames.min(self.max_frames);
        for order_pos in 0..self.order.len() {
            let Some(&idx) = self.order.get(order_pos) else {
                continue;
            };
            let Some(node) = self.nodes.get_mut(idx) else {
                continue;
            };
            let Some(buf) = node.buffer.get_mut(..len) else {
                continue;
            };

            let sidechain_slice = node.sidechain.as_deref().and_then(|s| s.get(..len));

            match &mut node.kind {
                RuntimeNodeKind::GroupInput { .. } => {
                    if let Some(pre) = node.pre_fader.as_mut().and_then(|b| b.get_mut(..len)) {
                        pre.copy_from_slice(buf);
                    }
                }
                RuntimeNodeKind::Bus { rack, gain, pan } => {
                    rack.process_with_sidechain(buf, sidechain_slice, &mut [], tempo);
                    if let Some(pre) = node.pre_fader.as_mut().and_then(|b| b.get_mut(..len)) {
                        pre.copy_from_slice(buf);
                    }
                    apply_gain_pan(buf, *gain, *pan);
                }
                RuntimeNodeKind::Rack { rack } => {
                    rack.process_with_sidechain(buf, sidechain_slice, &mut [], tempo);
                    if let Some(pre) = node.pre_fader.as_mut().and_then(|b| b.get_mut(..len)) {
                        pre.copy_from_slice(buf);
                    }
                }
                RuntimeNodeKind::Split {
                    pre,
                    post,
                    amp_gain,
                    amp_pan,
                } => {
                    pre.process_with_sidechain(buf, sidechain_slice, &mut [], tempo);
                    if let Some(pre_buf) = node.pre_fader.as_mut().and_then(|b| b.get_mut(..len)) {
                        pre_buf.copy_from_slice(buf);
                    }
                    apply_gain_pan(buf, *amp_gain, *amp_pan);
                    post.process_with_sidechain(buf, sidechain_slice, &mut [], tempo);
                }
                RuntimeNodeKind::Mix { gain, pan } => {
                    if let Some(pre) = node.pre_fader.as_mut().and_then(|b| b.get_mut(..len)) {
                        pre.copy_from_slice(buf);
                    }
                    apply_gain_pan(buf, *gain, *pan);
                }
                RuntimeNodeKind::Output { bus_index } => {
                    if let Some(out) = outputs
                        .get_mut(usize::from(*bus_index))
                        .and_then(|slice| slice.get_mut(..len))
                    {
                        for (o, s) in out.iter_mut().zip(buf.iter()) {
                            o[0] += s[0];
                            o[1] += s[1];
                        }
                    }
                }
            }

            // Route outgoing edges to downstream nodes (guaranteed `to != idx` in a DAG).
            let edge_count = node.edges.len();
            for ei in 0..edge_count {
                let (to, gain, post_fader, sidechain) =
                    match self.nodes.get(idx).and_then(|n| n.edges.get(ei)) {
                        Some(e) => (e.to, e.gain, e.post_fader, e.sidechain),
                        None => continue,
                    };
                if to == idx {
                    continue;
                }
                // Split `self.nodes` cleanly without unsafe code.
                let (src_node, dst_node) = if idx < to {
                    let (left, right) = self.nodes.split_at_mut(to);
                    match (left.get(idx), right.first_mut()) {
                        (Some(s), Some(d)) => (s, d),
                        _ => continue,
                    }
                } else {
                    let (left, right) = self.nodes.split_at_mut(idx);
                    match (right.first(), left.get_mut(to)) {
                        (Some(s), Some(d)) => (s, d),
                        _ => continue,
                    }
                };
                let src_slice = if !post_fader && let Some(pre) = src_node.pre_fader.as_deref() {
                    pre.get(..len)
                } else {
                    src_node.buffer.get(..len)
                };
                let dst_slice = if sidechain {
                    dst_node
                        .sidechain
                        .as_deref_mut()
                        .and_then(|b| b.get_mut(..len))
                } else {
                    dst_node.buffer.get_mut(..len)
                };
                if let (Some(src), Some(dst)) = (src_slice, dst_slice) {
                    for (d, s) in dst.iter_mut().zip(src.iter()) {
                        d[0] += s[0] * gain;
                        d[1] += s[1] * gain;
                    }
                }
            }
        }
    }
}

fn apply_gain_pan(buf: &mut [[f32; 2]], gain: f32, pan: f32) {
    let [[l, _], [_, r]] = effects::balance(pan);
    let (gl, gr) = (gain * l, gain * r);
    for frame in buf {
        frame[0] *= gl;
        frame[1] *= gr;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn lowered_sends_use_slot_levels_at_the_insert_tap() {
        let fx = |kind| {
            Some(ir::Effect {
                kind,
                bypass: false,
                output: 1.0,
                dry: 0.0,
            })
        };
        let inst = ir::Instrument {
            groups: vec![ir::Group::DEFAULT],
            inserts: vec![
                fx(ir::EffectKind::Sends([
                    0.5, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0,
                ])),
                fx(ir::EffectKind::Gain(0.0)),
            ],
            sends: vec![
                fx(ir::EffectKind::Gain(1.0)),
                None,
                fx(ir::EffectKind::Gain(2.0)),
                fx(ir::EffectKind::Gain(8.0)),
            ],
            ..ir::Instrument::DEFAULT
        };
        let graph = ir::AudioGraph::from_instrument(&inst);
        let mut rt = GraphRuntime::new(&graph, 48000, 32, &[]).unwrap();
        rt.add_group_audio(0, &[[1.0; 2]; 32]);
        let mut out = [[0.0; 2]; 32];
        rt.process(32, 120.0, &mut [&mut out]);
        assert!((out[31][0] - 1.0).abs() < 1e-4, "{out:?}");
    }

    #[test]
    fn graph_runtime_runs_split_rack_pre_fader_send_and_multi_stem_outputs() {
        let mut graph = ir::AudioGraph::default();
        let pre_fx = graph.push_effect(ir::Effect {
            kind: ir::EffectKind::Gain(0.5),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        let post_fx = graph.push_effect(ir::Effect {
            kind: ir::EffectKind::Gain(0.5),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });

        let g0 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 0 });
        // Split: input 1.0 -> pre (x0.5 = 0.5) -> pre-fader send gets 0.5 -> amp (x0.5 = 0.25) -> post (x0.5 = 0.125)
        let split = graph.push_node(ir::GraphNodeKind::Split {
            pre_amp: vec![pre_fx],
            post_amp: vec![post_fx],
            amp_gain: 0.5,
            amp_pan: 0.0,
        });
        let out_main = graph.push_node(ir::GraphNodeKind::Output {
            bus_index: 0,
            layout: ir::ChannelLayout::Stereo,
        });
        let out_aux = graph.push_node(ir::GraphNodeKind::Output {
            bus_index: 1,
            layout: ir::ChannelLayout::Stereo,
        });

        graph.connect(g0, split, 1.0, true);
        graph.connect(split, out_main, 1.0, true);
        graph.connect(split, out_aux, 1.0, false);

        let mut rt = GraphRuntime::new(&graph, 48_000, 64, &[]).unwrap();
        let input = [[1.0, 1.0]; 32];
        rt.clear(32);
        rt.add_group_audio(0, &input);

        let mut main_buf = [[0.0; 2]; 32];
        let mut aux_buf = [[0.0; 2]; 32];
        let mut outs: [&mut [[f32; 2]]; 2] = [&mut main_buf, &mut aux_buf];
        rt.process(32, 120.0, &mut outs);

        // Main output sees full chain: 1.0 * 0.5 (pre) * 0.5 (amp) * 0.5 (post) = 0.125
        assert!(
            (main_buf[31][0] - 0.125).abs() < 1e-4,
            "main {}",
            main_buf[31][0]
        );
        // Aux stem tapped pre-fader after pre-amp FX: 1.0 * 0.5 = 0.5
        assert!(
            (aux_buf[31][0] - 0.5).abs() < 1e-4,
            "aux {}",
            aux_buf[31][0]
        );
    }

    #[test]
    fn graph_runtime_sidechain_key_ducks_program_audio() {
        let mut graph = ir::AudioGraph::default();
        let comp = graph.push_effect(ir::Effect {
            kind: ir::EffectKind::Compressor(ir::Compressor {
                threshold_db: -24.0,
                ratio: 10.0,
                knee_db: 0.0,
                attack_ms: 0.5,
                release_ms: 50.0,
                makeup_db: 0.0,
                makeup_slope: 0.0,
                link: true,
                mix: 1.0,
                detector: ir::Detector::Input,
                sidechain_hz: None,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        let prog = graph.push_node(ir::GraphNodeKind::GroupInput { group: 0 });
        let key = graph.push_node(ir::GraphNodeKind::GroupInput { group: 1 });
        let comp_node = graph.push_node(ir::GraphNodeKind::Rack {
            effects: vec![comp],
            layout: ir::ChannelLayout::Stereo,
        });
        let out_node = graph.push_node(ir::GraphNodeKind::Output {
            bus_index: 0,
            layout: ir::ChannelLayout::Stereo,
        });
        graph.connect(prog, comp_node, 1.0, true);
        graph.connect_sidechain(key, comp_node, 1.0);
        graph.connect(comp_node, out_node, 1.0, true);

        // 1. Program only (no sidechain key): passes uncompressed at ~0.5.
        let mut rt = GraphRuntime::new(&graph, 48_000, 256, &[]).unwrap();
        let mut prog_only = [[0.0; 2]; 256];
        for _ in 0..4 {
            prog_only.fill([0.0; 2]);
            rt.clear(256);
            rt.add_group_audio(0, &[[0.5; 2]; 256]);
            rt.process(256, 120.0, &mut [&mut prog_only]);
        }

        // 2. Key only (no program): output is completely silent.
        let mut rt_key = GraphRuntime::new(&graph, 48_000, 256, &[]).unwrap();
        let mut key_only = [[0.0; 2]; 256];
        rt_key.clear(256);
        rt_key.add_group_audio(1, &[[0.9; 2]; 256]);
        rt_key.process(256, 120.0, &mut [&mut key_only]);

        // 3. Program + loud key: program is heavily ducked!
        let mut rt_duck = GraphRuntime::new(&graph, 48_000, 256, &[]).unwrap();
        let mut ducked = [[0.0; 2]; 256];
        for _ in 0..4 {
            ducked.fill([0.0; 2]);
            rt_duck.clear(256);
            rt_duck.add_group_audio(0, &[[0.5; 2]; 256]);
            rt_duck.add_group_audio(1, &[[0.9; 2]; 256]);
            rt_duck.process(256, 120.0, &mut [&mut ducked]);
        }

        assert!(
            prog_only[255][0] > 0.45,
            "program-only should pass uncompressed: {}",
            prog_only[255][0]
        );
        assert!(
            key_only
                .iter()
                .all(|f| f[0].abs() < 1e-6 && f[1].abs() < 1e-6),
            "key-only must not bleed into Main Out"
        );
        assert!(
            ducked[255][0] < prog_only[255][0] * 0.25,
            "sidechain key must duck program: prog_only={}, ducked={}",
            prog_only[255][0],
            ducked[255][0]
        );
    }
}
