//! Modulation routes of a program that the engine carries.

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, flag, num, source_node};

impl<L: Library> Translator<'_, '_, L> {
    /// A script's modulation (Script Event Modulation) on a layer's,
    /// keygroup's or the program's gain: a volume route on each group under
    /// it, scaled by the value the script sends for the source's `EventId`.
    pub(super) fn script_routes(&mut self) {
        for connection in self.program.descendants() {
            let Some((owner, source)) = script_gain(self.program, connection) else {
                continue;
            };
            let Some(id) = source.attribute("EventId").and_then(|i| i.parse().ok()) else {
                continue;
            };
            let route = ir::Mod {
                name: source.attribute("Name").unwrap_or_default().into(),
                slot: id,
                target: 0,
                bypass: false,
                kind: ir::ModKind::Volume {
                    source: ir::Source::Script(id),
                    intensity: num(connection, "Ratio", 1.0).unwrap_or(1.0) as f32,
                    curve: ir::Curve::Depth { power: 1 },
                },
            };
            let groups: Vec<usize> = owner
                .descendants()
                .filter_map(|n| self.made.get(&n.id()).copied())
                .collect();
            for group in groups {
                if let Some(g) = self.out.groups.get_mut(group) {
                    g.mods.push(route.clone());
                }
            }
        }
    }
}

/// A live connection from a script modulation to the gain of a layer,
/// keygroup or the program: that element and the modulation.
pub(super) fn script_gain<'a, 'input>(
    program: Node<'a, 'input>,
    connection: Node<'a, 'input>,
) -> Option<(Node<'a, 'input>, Node<'a, 'input>)> {
    if !connection.has_tag_name("SignalConnection")
        || flag(connection, "Bypass")
        || flag(connection, "Inverted")
        || connection.attribute("Destination") != Some("Gain")
    {
        return None;
    }
    let owner = connection.parent()?.parent()?;
    let source = source_node(program, connection)?;
    (["Layer", "Keygroup", "Program"]
        .iter()
        .any(|tag| owner.has_tag_name(*tag))
        && source.has_tag_name("ScriptEventModulation"))
    .then_some((owner, source))
}
