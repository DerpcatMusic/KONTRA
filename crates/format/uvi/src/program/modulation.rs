//! Modulation routes of a program that the engine carries.

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, flag, matrix, num, source_node};

impl<L: Library> Translator<'_, '_, L> {
    /// A script's modulation (Script Event Modulation) on a gain or pitch:
    /// a route on each group under the element it modulates, driven by the
    /// value the script sends for the source's `EventId`. A gain scales
    /// linearly by the connection's ratio; a pitch moves by ratio semitones.
    /// A unipolar modulation is read through the connection's mapper.
    /// An oscillator's route goes on its keygroup's group once.
    /// ponytail: a bipolar one's mapper is not applied (VWinds' legato pitch
    /// curve, monotone over the same range); add when Script enters a
    /// table at `(x + 1)/2`.
    pub(super) fn script_routes(&mut self) {
        for connection in self.program.descendants() {
            let Some((owner, source)) = script_route(self.program, connection) else {
                continue;
            };
            let Some(id) = source.attribute("EventId").and_then(|i| i.parse().ok()) else {
                continue;
            };
            let ratio = num(connection, "Ratio", 1.0).unwrap_or(1.0) as f32;
            let pitch = connection.attribute("Destination") == Some("Pitch");
            let kind = |source| {
                if pitch {
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
                }
            };
            let mapped = matrix::transfer(self.program, connection, None)
                .filter(|t| t.source == ir::Source::Script(id) && !t.table.is_empty());
            let route = |kind| ir::Mod {
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
                let Some(g) = self.out.groups.get_mut(group) else {
                    continue;
                };
                let source = match &mapped {
                    Some(t) => ir::Source::Shaped(super::index_of(&mut g.shapers, t.clone())),
                    None => ir::Source::Script(id),
                };
                let route = route(kind(source));
                if !g.mods.contains(&route) {
                    g.mods.push(route);
                }
            }
        }
    }
}

/// A route: `unit × ratio` per unit of `source`, or of `timed` (then the
/// group's copy of it is the source). A script may set the ratio.
pub(super) struct Route {
    pub source: ir::Source,
    pub timed: Option<Timed>,
    pub unit: f32,
    pub ratio: f32,
}

/// A program or layer modulator a group carries a copy of.
pub(super) enum Timed {
    Lfo(ir::Lfo),
    Envelope(ir::Breakpoints),
    /// A saw LFO read through a table ([`steps`]).
    Steps(ir::Lfo, Vec<f32>),
    /// A source read through the connection's mapper ([`matrix::transfer`]).
    Mapped(ir::Transfer),
}

/// The route a connection on `owner`'s `destination` makes, adding `Ratio`
/// per unit (an oscillator's `Pitch` in semitones, a keygroup's `Pan` on
/// −1..=1) of a pitch-bend (−1..=1), controller or velocity (0..=1) source,
/// or of a program or layer modulator ([`lfo`], [`ahd`], [`steps`]); an
/// inverted bend or LFO turns the other way (v1's native renders,
/// crates/sampler-uvi/src/modulation.rs). A connection with a mapper adds
/// `Ratio` per mapper output instead (UVIWorkstation 0x1411390c0: the table
/// read linearly, scaled by `Max`, or `-Min` below zero). `None` for
/// anything else: a nested or modulated connection, another source, an
/// inverted unipolar or mapped source (reported).
pub(super) fn route(
    program: Node,
    connection: Node,
    (owner, destination): (&str, &str),
) -> Option<Route> {
    if !connection.has_tag_name("SignalConnection")
        || !connection.parent()?.parent()?.has_tag_name(owner)
        || connection.attribute("Destination") != Some(destination)
        || flag(connection, "Bypass")
        || flag(connection, "ConnectionMode")
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
    if connection
        .attribute("Mapper")
        .is_some_and(|m| !m.is_empty())
    {
        let t = matrix::transfer(program, connection, None)?;
        // A script's modulation is [`script_route`]'s.
        let held = matches!(t.source, ir::Source::Constant | ir::Source::Script(_));
        return (sign > 0.0 && !held).then_some(Route {
            source: ir::Source::Constant,
            timed: Some(Timed::Mapped(t)),
            unit: 1.0,
            ratio,
        });
    }
    let fixed = |source| Route {
        source,
        timed: None,
        unit: 1.0,
        ratio,
    };
    Some(match connection.attribute("Source")? {
        "@PitchBend" => Route {
            unit: sign,
            ..fixed(ir::Source::PitchBend)
        },
        s if s.starts_with('$') => {
            let node = source_node(program, connection)?;
            if node.has_tag_name("AHD") || node.has_tag_name("StepEnvelope") {
                if sign < 0.0 {
                    return None;
                }
                let timed = if node.has_tag_name("AHD") {
                    Timed::Envelope(ahd(node)?)
                } else {
                    let (lfo, table) = steps(node)?;
                    Timed::Steps(lfo, table)
                };
                Route {
                    timed: Some(timed),
                    ..fixed(ir::Source::Constant)
                }
            } else {
                let (lfo, depth) = lfo(node)?;
                Route {
                    source: ir::Source::Lfo(0),
                    timed: Some(Timed::Lfo(lfo)),
                    unit: sign * depth,
                    ratio,
                }
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

/// An `AHD` as breakpoints from 0: to 1 over `AttackTime`, holding 1 for
/// `HoldTime`, to 0 over `DecayTime`, key-up or not. The registry
/// (UVIWorkstation 0x14113af30) builds these three `(time, level, curve)`
/// segments with defaults 0, 1 and 0.1 s. A segment's curve `c` passes
/// `(1 − c)/2` of its way at half its time on an exponential (setup
/// 0x1410f1220, coefficients 0x141350b70, clamped to 0.0001..=0.9999):
/// `(1 − q^2t)/(1 − q²)`, `q = (1 + c)/(1 − c)`, the IR's `k = 2·ln q`.
/// `None` when bypassed or velocity-scaled (not read).
pub(super) fn ahd(node: Node) -> Option<ir::Breakpoints> {
    if flag(node, "Bypass") || flag(node, "VelocityAmount") {
        return None;
    }
    let point = |time, default, level, name| -> Option<ir::Breakpoint> {
        Some(ir::Breakpoint {
            ms: (num(node, time, default).ok()?.clamp(0.0, 30.0) * 1000.0) as f32,
            level,
            curve: curve(node, name)?,
        })
    };
    Some(ir::Breakpoints {
        start: 0.0,
        points: vec![
            point("AttackTime", 0.0, 1.0, "AttackCurve")?,
            point("HoldTime", 1.0, 1.0, "")?,
            point("DecayTime", 0.1, 0.0, "DecayCurve")?,
        ],
        sustain: None,
        loop_from: None,
        stages: false,
    })
}

/// An envelope segment's curve attribute `name` as the IR's shape `k`
/// ([`ahd`]); a DAHDSR's segments run the same law (process
/// 0x1410f0570 calls setup 0x1410f1220).
pub(super) fn curve(node: Node, name: &str) -> Option<f32> {
    let c = num(node, name, 0.0).ok()?.clamp(-0.9998, 0.9998);
    Some((2.0 * ((1.0 + c) / (1.0 - c)).ln()) as f32)
}

/// Table entries per step: a step's edge ramps over 1/16 of it.
const ENTRIES_PER_STEP: usize = 16;

/// A `StepEnvelope` as a cycle of its `Levels`, each held for one step: a
/// saw LFO (`1 − 2p` at phase `p`, read through a table as `(x + 1)/2`, so
/// `1 − p`) and that table. UVIWorkstation's process (0x141153460) holds
/// level `⌊pos⌋ mod NumSteps`, `pos` advancing a step per `Freq` beats when
/// synced (`Freq` steps a second when not; 1/60 at 0x141e29edc), from the
/// song position when `Retrigger` is 0 or from the note when 1. `None`
/// for an interpolated envelope (`InterpolationMode` 1, a cubic) or an
/// unknown retrigger. ponytail: `Smooth`, a one-pole reaching 2/3 of a step
/// in `Smooth` s (`(1/3)^(1/(Smooth·rate))` a sample), is not applied; bake
/// it into the table at the host tempo if steps sound too hard.
pub(super) fn steps(node: Node) -> Option<(ir::Lfo, Vec<f32>)> {
    if flag(node, "Bypass") || flag(node, "InterpolationMode") {
        return None;
    }
    let levels: Vec<f32> = node
        .attribute("Levels")?
        .split_whitespace()
        .map(|l| {
            l.replace(',', ".")
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
        })
        .collect::<Option<_>>()?;
    let n = levels.len();
    let freq = num(node, "Freq", 0.25).ok()? as f32;
    if n == 0 || num(node, "NumSteps", 16.0).ok()? != n as f64 || freq <= 0.0 {
        return None;
    }
    let clock = match num(node, "Retrigger", 0.0).ok()? {
        0.0 => ir::Clock::Free,
        1.0 => ir::Clock::Voice,
        _ => return None,
    };
    let rate = if flag(node, "SyncToHost") {
        ir::Rate::Beats(freq * n as f32)
    } else {
        ir::Rate::Hz(freq / n as f32)
    };
    let last = ENTRIES_PER_STEP * n;
    let table = (0..=last)
        .map(|j| {
            let step = (n * (last - j)).div_euclid(last) % n;
            levels.get(step).copied().unwrap_or(0.0)
        })
        .collect();
    let lfo = ir::Lfo {
        wave: ir::Wave::Saw,
        rate,
        phase: 0.0,
        fade_ms: 0.0,
        clock,
    };
    Some((lfo, table))
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
