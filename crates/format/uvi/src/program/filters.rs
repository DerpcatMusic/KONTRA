//! A keygroup's filters. A `OnePole` (UVIWorkstation's factory names its modes
//! `lpf_1p` and `hpf_1p`, 0x14136aa50) is a one-pole at `Freq` Hz, 20 Hz to
//! 20 kHz. A connection on `Freq` moves it along that range's log scale:
//! `Freq × 1000^(Ratio × s)`, `s` the source (0..=1), `1 − s` when
//! `Inverted` (the parameter tables pair 20 and 20000, 0x14258b220; v1's
//! native renders, crates/sampler-uvi/src/modulation.rs).

use kontra_ir as ir;
use roxmltree::Node;

use super::{flag, num};

/// Octaves of the `Freq` range a connection's ratio of 1 spans: 20 Hz to 20 kHz.
const FREQ_OCTAVES: f32 = 9.965_784;

/// A keygroup insert this crate carries: an active `OnePole` with no key tracking.
pub(super) fn carried(node: Node) -> bool {
    node.has_tag_name("OnePole")
        && !flag(node, "Bypass")
        && !flag(node, "KeyTracking")
        && node
            .parent()
            .filter(|p| p.has_tag_name("Inserts"))
            .and_then(|p| p.parent())
            .is_some_and(|p| p.has_tag_name("Keygroup"))
}

/// The route a connection on a carried filter's `Freq` makes: its source,
/// its ratio, and whether it is inverted. `None` for any other connection.
pub(super) fn freq_route(connection: Node) -> Option<(ir::Source, f32, bool)> {
    let filter = connection.parent()?.parent()?;
    if !carried(filter)
        || connection.attribute("Destination") != Some("Freq")
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
    let source = match connection.attribute("Source")? {
        "@VoiceParam Velocity" => ir::Source::Velocity,
        s => ir::Source::Controller(s.strip_prefix("@MIDI CC ")?.trim().parse().ok()?),
    };
    let ratio = num(connection, "Ratio", 0.0).ok()? as f32;
    Some((source, ratio, flag(connection, "Inverted")))
}

/// A keygroup's inserts by slot (a slot not carried passes its input), and
/// the cutoff routes on them.
pub(super) fn keygroup(kg: Node) -> (Vec<Option<ir::Effect>>, Vec<ir::Mod>) {
    let mut inserts = Vec::new();
    let mut mods = Vec::new();
    let slots = kg
        .children()
        .filter(|n| n.has_tag_name("Inserts"))
        .flat_map(|n| n.children())
        .filter(Node::is_element);
    for (n, node) in slots.enumerate() {
        let Some(insert) = u8::try_from(n).ok().filter(|_| carried(node)) else {
            inserts.push(None);
            continue;
        };
        let mut hz = num(node, "Freq", 20_000.0).unwrap_or(20_000.0) as f32;
        let connections = node
            .children()
            .filter(|c| c.has_tag_name("Connections"))
            .flat_map(|c| c.children());
        for (source, ratio, inverted) in connections.filter_map(freq_route) {
            if ratio == 0.0 {
                continue;
            }
            // `1000^(r(1 − s))` is `1000^r` on the base and `−r` on `s`.
            let depth = if inverted {
                hz *= 1000f32.powf(ratio);
                -ratio
            } else {
                ratio
            };
            mods.push(ir::Mod {
                name: String::new(),
                slot: 0,
                target: 0,
                bypass: false,
                kind: ir::ModKind::Cutoff {
                    source,
                    insert,
                    depth,
                },
            });
        }
        let pass = if num(node, "Mode", 0.0).unwrap_or(0.0) == 1.0 {
            ir::Pass::High
        } else {
            ir::Pass::Low
        };
        inserts.push(Some(ir::Effect {
            kind: ir::EffectKind::Filter(ir::Filter {
                pass,
                hz: hz.clamp(20.0, 20_000.0),
                stages: 0,
                pole: false,
                damping: [2.0; 3],
                gain: 1.0,
                octaves: FREQ_OCTAVES,
                q_octaves: 0.0,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }));
    }
    (inserts, mods)
}
