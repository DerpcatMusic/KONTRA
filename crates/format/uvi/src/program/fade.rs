//! Keygroup crossfades: the native edge law at 0x140f28cba..0x140f28dc8.

use super::{index_of, midi_or, num};
use kontra_ir as ir;
use roxmltree::Node;

pub(super) fn apply(kg: Node, group: &mut ir::Group) -> Result<(), String> {
    let curve = num(kg, "FadeCurve", 2.0)?.clamp(1.0, 2.0) as f32;
    let fade = kontra_law::uvi::fade_edge(curve);
    for (source, low, high, low_fade, high_fade) in [
        (
            ir::Source::Key,
            midi_or(kg, "LowKey", 0),
            midi_or(kg, "HighKey", 127),
            midi_or(kg, "LowKeyFade", 0),
            midi_or(kg, "HighKeyFade", 0),
        ),
        (
            ir::Source::Velocity,
            midi_or(kg, "LowVelocity", 1),
            midi_or(kg, "HighVelocity", 127),
            midi_or(kg, "LowVelocityFade", 0),
            midi_or(kg, "HighVelocityFade", 0),
        ),
    ] {
        if low_fade == 0 && high_fade == 0 {
            continue;
        }
        let edge = |distance: f32, width: u8| {
            if width == 0 {
                1.0
            } else {
                fade.eval(distance / f32::from(width))
            }
        };
        let table = (0..=127)
            .map(|n| {
                edge(n as f32 - f32::from(low), low_fade)
                    * edge(f32::from(high) - n as f32, high_fade)
            })
            .collect();
        super::provenance::record(
            "uvi.key_velocity_fade.v1",
            kg,
            &(source, curve, low, high, low_fade, high_fade),
            &table,
        );
        let shaped = index_of(&mut group.shapers, ir::Transfer { source, table });
        group.mods.push(ir::Mod {
            name: String::new(),
            slot: u16::MAX,
            target: 0,
            bypass: false,
            kind: ir::ModKind::Volume {
                source: ir::Source::Shaped(shaped),
                intensity: 1.0,
                curve: ir::Curve::Depth { power: 1 },
                tau_ms: 0.0,
            },
        });
    }
    Ok(())
}
