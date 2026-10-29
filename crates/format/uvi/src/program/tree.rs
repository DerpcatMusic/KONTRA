//! The program tree as the Lua module scripts read.

use std::collections::HashMap;

use roxmltree::{Node, NodeId};

use super::{Library, Translator, flag};

impl<L: Library> Translator<'_, '_, L> {
    /// The program tree as a Lua module for the script runtime: each element's
    /// attributes as `params`, its children by container (`layers`,
    /// `keygroups`, `inserts`, ...), each keygroup's engine group (0-based),
    /// the connections the engine carries (`links`, places in the program's
    /// `connections` of `{Source, Destination, Ratio, route key, amount per
    /// unit of Ratio, Bypass}`; the route is named `#key`; one it does not
    /// carry, ahead of one it does, holds its place with key −1),
    /// and the program-wide dB and pan every group adds to its layer's and
    /// keygroup's.
    pub(super) fn tree(&self) -> String {
        // One `{Source, Destination, Ratio, key, unit, Bypass}` per route and
        // source; a place holder per source, destination and ratio.
        let mut links = HashMap::new();
        let mut shared: Vec<String> = Vec::new();
        let mut index = HashMap::new();
        let lists = self
            .program
            .descendants()
            .filter(|n| n.has_tag_name("Connections"))
            .filter(|n| {
                n.parent()
                    .is_some_and(|o| !o.has_tag_name("SignalConnection"))
            });
        for list in lists {
            let list: Vec<_> = list.children().filter(Node::is_element).collect();
            let Some(last) = list.iter().rposition(|c| self.routed.contains_key(&c.id())) else {
                continue;
            };
            for c in list.into_iter().take(last + 1) {
                let key = self.routed.get(&c.id()).map_or(-1, |&k| k as i64);
                let source = c.attribute("Source").unwrap_or_default();
                let destination = c.attribute("Destination").unwrap_or_default();
                let ratio = c
                    .attribute("Ratio")
                    .and_then(|r| r.trim().parse::<f64>().ok())
                    .unwrap_or(0.0);
                let place = (key < 0).then(|| (destination, ratio.to_bits()));
                let n = *index.entry((key, source, place)).or_insert_with(|| {
                    shared.push(format!(
                        "{{{},{},{ratio},{key},{},{}}}",
                        lua_string(source),
                        lua_string(destination),
                        usize::try_from(key)
                            .ok()
                            .and_then(|k| self.units.get(&k))
                            .copied()
                            .unwrap_or(1.0),
                        flag(c, "Bypass"),
                    ));
                    shared.len()
                });
                links.insert(c.id(), n);
            }
        }
        let mut out = String::from("return ");
        self.lua_node(self.program, &links, &mut out);
        // Close the program's table with what only it carries.
        out.pop();
        out.push_str(&format!(
            ",db={},pan={},connections={{{}}}}}",
            self.base.volume_db,
            self.base.pan,
            shared.join(",")
        ));
        out
    }

    fn lua_node(&self, node: Node, links: &HashMap<NodeId, usize>, out: &mut String) {
        out.push_str("{params={");
        for a in node.attributes() {
            out.push_str(&format!(
                "[{}]={},",
                lua_string(a.name()),
                lua_string(a.value())
            ));
        }
        out.push('}');
        if let Some(group) = self.made.get(&node.id()) {
            out.push_str(&format!(",group={group}"));
        }
        if let Some(bus) = self.bus_of.get(&node.id()) {
            out.push_str(&format!(",bus={bus}"));
        }
        // The connections a route carries: their places in `connections`.
        let linked: Vec<String> = node
            .children()
            .filter(|c| c.has_tag_name("Connections"))
            .flat_map(|c| c.children())
            .filter_map(|c| links.get(&c.id()).map(usize::to_string))
            .collect();
        if !linked.is_empty() {
            out.push_str(&format!(",links={{{}}}", linked.join(",")));
        }
        for container in node.children().filter(Node::is_element) {
            let Some(key) = CONTAINERS
                .iter()
                .find(|(tag, _)| container.has_tag_name(*tag))
                .map(|(_, key)| key)
            else {
                continue;
            };
            out.push_str(&format!(",{key}={{"));
            for child in container.children().filter(Node::is_element) {
                self.lua_node(child, links, out);
                out.push(',');
            }
            out.push('}');
        }
        out.push('}');
    }
}

/// XML containers as the script API names their lists.
const CONTAINERS: [(&str, &str); 9] = [
    ("Layers", "layers"),
    ("Keygroups", "keygroups"),
    ("Inserts", "inserts"),
    ("Auxs", "auxs"),
    // An effect rack's chains are its auxs to scripts.
    ("Chains", "auxs"),
    ("Oscillators", "oscillators"),
    ("ControlSignalSources", "modulations"),
    ("EventProcessors", "eventProcessors"),
    ("BusRouters", "busRouters"),
];

/// `text` as a Lua string literal.
fn lua_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
