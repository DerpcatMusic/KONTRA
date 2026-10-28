//! Modulation routes of a program that the engine carries.

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, flag, num, source_node};

impl<L: Library> Translator<'_, '_, L> {
    /// A script's modulation (Script Event Modulation) on a gain or pitch:
    /// a route on each group under the element it modulates, driven by the
    /// value the script sends for the source's `EventId`. A gain scales
    /// linearly by the connection's ratio; a pitch moves by ratio semitones.
    /// An oscillator's route goes on its keygroup's group once.
    pub(super) fn script_routes(&mut self) {
        for connection in self.program.descendants() {
            let Some((owner, source)) = script_route(self.program, connection) else {
                continue;
            };
            let Some(id) = source.attribute("EventId").and_then(|i| i.parse().ok()) else {
                continue;
            };
            let ratio = num(connection, "Ratio", 1.0).unwrap_or(1.0) as f32;
            let source = ir::Source::Script(id);
            let kind = if connection.attribute("Destination") == Some("Pitch") {
                ir::ModKind::Pitch {
                    source,
                    semitones: ratio,
                    depth: 1.0,
                }
            } else {
                // A negative ratio turns the gain down as the value rises.
                ir::ModKind::Volume {
                    source,
                    intensity: ratio.abs(),
                    curve: if ratio < 0.0 {
                        ir::Curve::Falling
                    } else {
                        ir::Curve::Depth { power: 1 }
                    },
                }
            };
            let route = ir::Mod {
                name: connection
                    .attribute("Source")
                    .and_then(|s| s.rsplit('/').next())
                    .unwrap_or_default()
                    .into(),
                slot: id,
                target: 0,
                bypass: false,
                kind,
            };
            let scope = if owner.has_tag_name("SamplePlayer") {
                owner.ancestors().find(|n| n.has_tag_name("Keygroup"))
            } else {
                Some(owner)
            };
            let groups: Vec<usize> = scope
                .iter()
                .flat_map(|n| n.descendants())
                .filter_map(|n| self.made.get(&n.id()).copied())
                .collect();
            for group in groups {
                if let Some(g) = self.out.groups.get_mut(group)
                    && !g.mods.contains(&route)
                {
                    g.mods.push(route.clone());
                }
            }
        }
    }
}

/// The pitch route a connection on an oscillator's `Pitch` makes: `Ratio`
/// semitones per unit of a pitch-bend (−1..=1), controller or velocity
/// (0..=1) source, an inverted bend turning the other way (v1's native
/// renders, crates/sampler-uvi/src/modulation.rs). `None` for anything
/// else: a mapper, a nested or modulated connection, another source, or an
/// inverted unipolar source (reported).
pub(super) fn pitch_route(connection: Node) -> Option<ir::ModKind> {
    let owner = connection.parent()?.parent()?;
    if !connection.has_tag_name("SignalConnection")
        || !owner.has_tag_name("SamplePlayer")
        || connection.attribute("Destination") != Some("Pitch")
        || flag(connection, "Bypass")
        || flag(connection, "ConnectionMode")
        || !connection
            .attribute("Mapper")
            .unwrap_or_default()
            .is_empty()
        || connection.children().any(|n| n.is_element())
    {
        return None;
    }
    let inverted = flag(connection, "Inverted");
    let ratio = num(connection, "Ratio", 0.0).ok()? as f32;
    let (source, semitones) = match connection.attribute("Source")? {
        "@PitchBend" => (ir::Source::PitchBend, if inverted { -ratio } else { ratio }),
        _ if inverted => return None,
        "@VoiceParam Velocity" => (ir::Source::Velocity, ratio),
        s => (
            ir::Source::Controller(s.strip_prefix("@MIDI CC ")?.trim().parse().ok()?),
            ratio,
        ),
    };
    Some(ir::ModKind::Pitch {
        source,
        semitones,
        depth: 1.0,
    })
}

/// A live connection from a script modulation to the gain of a layer,
/// keygroup or the program, or to the pitch of those or an oscillator: that
/// element and the modulation.
pub(super) fn script_route<'a, 'input>(
    program: Node<'a, 'input>,
    connection: Node<'a, 'input>,
) -> Option<(Node<'a, 'input>, Node<'a, 'input>)> {
    if !connection.has_tag_name("SignalConnection")
        || flag(connection, "Bypass")
        || flag(connection, "Inverted")
    {
        return None;
    }
    let owners: &[&str] = match connection.attribute("Destination")? {
        "Gain" => &["Layer", "Keygroup", "Program"],
        "Pitch" => &["Layer", "Keygroup", "Program", "SamplePlayer"],
        _ => return None,
    };
    let owner = connection.parent()?.parent()?;
    let source = source_node(program, connection)?;
    (owners.iter().any(|tag| owner.has_tag_name(*tag))
        && source.has_tag_name("ScriptEventModulation"))
    .then_some((owner, source))
}
