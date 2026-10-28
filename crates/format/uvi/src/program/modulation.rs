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

/// A pitch route: `semitones` per unit of `source`, or of `lfo` (then the
/// group's copy of it is the source).
pub(super) struct PitchRoute {
    pub source: ir::Source,
    pub lfo: Option<ir::Lfo>,
    pub semitones: f32,
}

/// The pitch route a connection on an oscillator's `Pitch` makes: `Ratio`
/// semitones per unit of a pitch-bend (−1..=1), controller or velocity
/// (0..=1) source, or of a program or layer LFO ([`lfo`]); an inverted
/// bend or LFO turns the other way (v1's native renders,
/// crates/sampler-uvi/src/modulation.rs). `None` for anything else: a
/// mapper, a nested or modulated connection, another source, or an
/// inverted unipolar source (reported).
pub(super) fn pitch_route(program: Node, connection: Node) -> Option<PitchRoute> {
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
    let sign = if flag(connection, "Inverted") {
        -1.0
    } else {
        1.0
    };
    let ratio = num(connection, "Ratio", 0.0).ok()? as f32;
    let fixed = |source| PitchRoute {
        source,
        lfo: None,
        semitones: ratio,
    };
    Some(match connection.attribute("Source")? {
        "@PitchBend" => PitchRoute {
            semitones: sign * ratio,
            ..fixed(ir::Source::PitchBend)
        },
        s if s.starts_with('$') => {
            let (lfo, depth) = lfo(source_node(program, connection)?)?;
            PitchRoute {
                source: ir::Source::Lfo(0),
                lfo: Some(lfo),
                semitones: sign * ratio * depth,
            }
        }
        _ if sign < 0.0 => return None,
        "@VoiceParam Velocity" => fixed(ir::Source::Velocity),
        s => fixed(ir::Source::Controller(
            s.strip_prefix("@MIDI CC ")?.trim().parse().ok()?,
        )),
    })
}

/// A bipolar `LFO`, or a sine-and-noise `MultiLFO`, with no delay or rise, as an LFO
/// and its signed depth: `Depth·x`. The registry (UVIWorkstation 0x1410f5d00)
/// names `WaveFormType` Sinus, Square, Triangle, RampUp, RampDown, ... and
/// `Retrigger` No Retrigger, Retrigger, Legato, Song; a synchronized `Freq` is
/// the cycle's length in beats (v1). ponytail: `Smooth` is not applied (a
/// lag on the shape; AO's are under 25 ms on cycles of a beat or more).
pub(super) fn lfo(node: Node) -> Option<(ir::Lfo, f32)> {
    let unset = |name| !flag(node, name);
    if flag(node, "Bypass")
        || !(unset("DelayTime") && unset("RiseTime") && unset("ManualTrigger"))
        || num(node, "Bipolar", 1.0).ok()? != 1.0
    {
        return None;
    }
    let (wave, sign) = if node.has_tag_name("LFO") {
        match num(node, "WaveFormType", 0.0).ok()? {
            0.0 => (ir::Wave::Sine, 1.0),
            1.0 => (ir::Wave::Square { width: 0.5 }, 1.0),
            2.0 => (ir::Wave::Triangle, 1.0),
            3.0 => (ir::Wave::Saw, -1.0),
            4.0 => (ir::Wave::Saw, 1.0),
            _ => return None,
        }
    } else if node.has_tag_name("MultiLFO")
        && num(node, "SineDepth", 0.0).ok()? == 1.0
        && ["TriangleDepth", "SawDepth", "SquareDepth", "Invert"]
            .into_iter()
            .all(unset)
        && flag(node, "NormalizeOutput")
    {
        // Measured (v1, UVI_MULTILFO_ENDPOINT_EVIDENCE.md): `(sin + n·noise)/(1 + n)`,
        // a new uniform noise value each half cycle.
        let noise = num(node, "NoiseDepth", 0.0).ok()? as f32;
        if !(0.0..=1.0).contains(&noise) {
            return None;
        }
        let wave = if noise == 0.0 {
            ir::Wave::Sine
        } else {
            ir::Wave::Multi {
                weights: [-1.0, 0.0, 0.0, 0.0, noise],
                width: 0.5,
                normalize: true,
            }
        };
        (wave, 1.0)
    } else {
        return None;
    };
    let clock = match num(node, "Retrigger", 1.0).ok()? {
        0.0 => ir::Clock::Free,
        1.0 => ir::Clock::Voice,
        _ => return None,
    };
    let freq = num(node, "Freq", 0.5).ok()? as f32;
    if freq <= 0.0 || !freq.is_finite() {
        return None;
    }
    let rate = if flag(node, "SyncToHost") {
        ir::Rate::Beats(freq)
    } else {
        ir::Rate::Hz(freq)
    };
    let lfo = ir::Lfo {
        wave,
        rate,
        phase: num(node, "Phase", 0.0).ok()?.rem_euclid(1.0) as f32,
        fade_ms: 0.0,
        clock,
    };
    Some((
        lfo,
        sign * num(node, "Depth", 1.0).ok()?.clamp(0.0, 1.0) as f32,
    ))
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
