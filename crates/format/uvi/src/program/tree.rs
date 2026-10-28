//! The program tree as the Lua module scripts read.

use roxmltree::Node;

use super::{Library, Translator};

impl<L: Library> Translator<'_, '_, L> {
    /// The program tree as a Lua module for the script runtime: each element's
    /// attributes as `params`, its children by container (`layers`,
    /// `keygroups`, `inserts`, ...), each keygroup's engine group (0-based),
    /// and the program-wide dB and pan every group adds to its layer's and
    /// keygroup's.
    pub(super) fn tree(&self) -> String {
        let mut out = String::from("return ");
        self.lua_node(self.program, &mut out);
        // Close the program's table with what only it carries.
        out.pop();
        out.push_str(&format!(
            ",db={},pan={}}}",
            self.base.volume_db, self.base.pan
        ));
        out
    }

    fn lua_node(&self, node: Node, out: &mut String) {
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
                self.lua_node(child, out);
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
