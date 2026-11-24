//! A plan's bus routing as an [`ir::AudioGraph`]: what the engine's rack
//! path does between the group sums and the instrument racks, as nodes.
//! Spec: docs/specs/effect-racks-on-audiograph.md.

use kontra_engine::Plan;
use kontra_engine::ir;

/// Lowers `plan`'s buses to a graph: one `InstrumentBus` node per plan bus
/// (node `b` is plan bus `b`, which runs its own racks, faders and sends),
/// each send to a later bus as an edge, and every bus that plays on main
/// into one `Output`. Voices and group chains sum into the buses as before;
/// the instrument's insert, send and main racks run after the graph.
#[must_use]
pub fn lower_graph(plan: &Plan) -> ir::AudioGraph {
    let mut graph = ir::AudioGraph::default();
    let buses: Vec<ir::NodeId> = (0..plan.buses.len())
        .map_while(|b| u16::try_from(b).ok())
        .map(|bus| graph.push_node(ir::GraphNodeKind::InstrumentBus { bus }))
        .collect();
    let out = graph.push_node(ir::GraphNodeKind::Output {
        bus_index: 0,
        layout: ir::ChannelLayout::Stereo,
    });
    for (b, bus) in plan.buses.iter().enumerate() {
        let Some(&from) = buses.get(b) else {
            continue;
        };
        // A send to an earlier bus would come a span late: the engine drops it.
        for send in bus.sends.iter().filter(|s| s.bus > b) {
            if let Some(&to) = buses.get(send.bus) {
                graph.connect(from, to, send.level, send.post_fader);
            }
        }
        if bus.to_main {
            graph.connect(from, out, 1.0, true);
        }
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buses_lower_in_order_with_forward_sends_only() {
        let mut plan = Plan::new(Vec::new(), 1);
        let main = plan.buses.first().cloned().expect("one bus");
        plan.buses = vec![main.clone(), main.clone(), main];
        let send = |bus| ir::BusSend {
            bus,
            level: 0.5,
            post_fader: true,
        };
        plan.buses[1].sends = vec![send(0), send(2)];
        plan.buses[1].to_main = false;
        let graph = lower_graph(&plan);
        assert!(graph.validate().is_empty(), "{:?}", graph.validate());
        let kinds: Vec<_> = graph.nodes.iter().map(|n| n.kind.clone()).collect();
        assert_eq!(kinds[1], ir::GraphNodeKind::InstrumentBus { bus: 1 });
        let edges: Vec<_> = graph.edges.iter().map(|e| (e.from.0, e.to.0)).collect();
        assert_eq!(
            edges,
            [(0, 3), (1, 2), (2, 3)],
            "bus 1 -> 0 dropped, 1 not on main"
        );
    }
}
