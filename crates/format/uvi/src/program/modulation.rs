//! Modulation routes of a program that the engine carries.

use std::cell::RefCell;
use std::collections::HashMap;

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, blend, flag, index_of, matrix, num, source_node};

impl<L: Library> Translator<'_, '_, L> {
    /// The connections on the `Depth` of the LFO `connection` reads, as
    /// [`ir::ModKind::Intensity`] routes on its group copy `of`: the
    /// connection law (UVIWorkstation 0x141429a80) scales the depth by
    /// `1 − r(1 − x)`, or `1 + r·x` below zero, where Intensity reads `1 − x`
    /// at `−r` (unit −1, so a script's ratio keeps its sign). ponytail: a
    /// script turning a depth ratio's sign keeps its inversion.
    pub(super) fn depth_scales(&mut self, group: &mut ir::Group, connection: Node, of: ir::Source) {
        // An LFO read through a table (a gain's `factor`) scales the same way.
        let of = match of {
            ir::Source::Shaped(n) => group.shapers.get(usize::from(n)).map_or(of, |t| t.source),
            of => of,
        };
        if !matches!(of, ir::Source::Lfo(_)) {
            return;
        }
        let Some(lfo) = source_node(self.program, connection) else {
            return;
        };
        // ponytail: an Intensity scales the wave around 0, a unipolar LFO's
        // depth its `m`; carried when a library routes one (none does).
        if lfo.has_tag_name("LFO") && !matrix::bipolar(lfo) {
            self.notes
                .add(ir::Impact::Audible, "unipolar LFO depth route", "");
            return;
        }
        let owner = lfo.tag_name().name();
        let depths = lfo
            .children()
            .filter(|c| c.has_tag_name("Connections"))
            .flat_map(|c| c.children())
            .filter_map(|c| Some((c, depth_route(self.program, c, owner)?)))
            .collect::<Vec<_>>();
        for (c, r) in depths {
            self.add_route(group, c, r, |source, depth| ir::ModKind::Intensity {
                source,
                of,
                depth,
            });
        }
    }

    /// The amplitude envelope: the DAHDSR a keygroup connects to its gain.
    pub(super) fn envelope(&mut self, kg: Node) -> Result<Option<ir::Envelope>, String> {
        let Some(connection) = kg.descendants().find(|c| {
            c.has_tag_name("SignalConnection")
                && c.attribute("Destination") == Some("Gain")
                && num(*c, "Ratio", 0.0).is_ok_and(|r| r == 1.0)
                && !flag(*c, "Bypass")
                && source_node(self.program, *c).is_some_and(|n| n.has_tag_name("DAHDSR"))
        }) else {
            return Ok(None);
        };
        let Some(env) = source_node(self.program, connection) else {
            return Ok(None);
        };
        let source = connection.attribute("Source").unwrap_or_default();
        let seconds = |name| num(env, name, 0.0).map(|t| t.clamp(0.0, 30.0) as f32);
        if num(env, "DelayTime", 0.0)? != 0.0 {
            self.notes.add(ir::Impact::Audible, "envelope delay", "");
        }
        // One shape serves decay and release: the release's, and the decay's
        // when it is what moves (AO's two are equal).
        let sustain = num(env, "SustainLevel", 1.0)?.clamp(0.0, 1.0) as f32;
        let shape = |name| curve(env, name).unwrap_or(0.0);
        let decays = sustain < 1.0 && seconds("DecayTime")? > 0.0;
        let fall = shape(if decays && seconds("ReleaseTime")? == 0.0 {
            "DecayCurve"
        } else {
            "ReleaseCurve"
        });
        if decays && shape("DecayCurve") != fall {
            self.notes
                .add(ir::Impact::Audible, "envelope decay curve", source);
        }
        Ok(Some(ir::Envelope {
            attack: seconds("AttackTime")?,
            hold: seconds("HoldTime")?,
            decay: seconds("DecayTime")?,
            sustain,
            release: seconds("ReleaseTime")?,
            attack_shape: shape("AttackCurve"),
            fall_shape: fall,
        }))
    }

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
                        tau_ms: 0.0,
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
                        tau_ms: 0.0,
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
    /// An LFO its routes reach through a delay and rise ([`rising`]), read
    /// through a table unless it is empty.
    Rising(ir::Lfo, ir::Breakpoints, Vec<f32>),
    /// A source read as a blend ([`blend`]).
    Blend(blend::Blend),
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
pub(super) fn route(program: Node, connection: Node, to: (&str, &str)) -> Option<Route> {
    if flag(connection, "Bypass") {
        return None;
    }
    reach(program, connection, to)
}

/// [`route`] whether or not the connection is bypassed (a script may turn
/// it on).
pub(super) fn reach(
    program: Node,
    connection: Node,
    (owner, destination): (&str, &str),
) -> Option<Route> {
    if !connection.has_tag_name("SignalConnection")
        || !connection.parent()?.parent()?.has_tag_name(owner)
        || connection.attribute("Destination") != Some(destination)
        || flag(connection, "ConnectionMode")
    {
        return None;
    }
    let sign = if flag(connection, "Inverted") {
        -1.0
    } else {
        1.0
    };
    let ratio = matrix::unkeyed_ratio(program, connection)?;
    let keyed = |r| blend::keyed(program, connection, r);
    if connection
        .attribute("Mapper")
        .is_some_and(|m| !m.is_empty())
    {
        let t = matrix::transfer(program, connection, None)?;
        // A script's modulation is [`script_route`]'s.
        let held = matches!(t.source, ir::Source::Constant | ir::Source::Script(_));
        let route = Route {
            source: ir::Source::Constant,
            timed: Some(Timed::Mapped(t)),
            unit: 1.0,
            ratio,
        };
        return (sign > 0.0 && !held).then(|| keyed(route)).flatten();
    }
    let fixed = |source| Route {
        source,
        timed: None,
        unit: 1.0,
        ratio,
    };
    keyed(match connection.attribute("Source")? {
        "@PitchBend" => Route {
            unit: sign,
            ..fixed(ir::Source::PitchBend)
        },
        s if s.starts_with('$') => {
            let node = source_node(program, connection)?;
            if ["AHD", "DAHDSR", "StepEnvelope"].contains(&node.tag_name().name()) {
                if sign < 0.0 {
                    return None;
                }
                let timed = if node.has_tag_name("AHD") {
                    Timed::Envelope(ahd(node)?)
                } else if node.has_tag_name("DAHDSR") {
                    Timed::Envelope(dahdsr(node)?)
                } else {
                    let (lfo, table) = steps(node)?;
                    Timed::Steps(lfo, table)
                };
                Route {
                    timed: Some(timed),
                    ..fixed(ir::Source::Constant)
                }
            } else if let Some(steps) = blend::table(program, node, sign < 0.0) {
                Route {
                    timed: Some(steps?),
                    ..fixed(ir::Source::Constant)
                }
            } else {
                let (lfo, depth, rise) = rising(node)?;
                Route {
                    source: ir::Source::Lfo(0),
                    timed: Some(match rise {
                        Some(rise) => Timed::Rising(lfo, rise, Vec::new()),
                        None => Timed::Lfo(lfo),
                    }),
                    unit: sign * depth,
                    ratio,
                }
            }
        }
        s => {
            let source = match s {
                "@VoiceParam Velocity" => ir::Source::Velocity,
                s => ir::Source::Controller(s.strip_prefix("@MIDI CC ")?.trim().parse().ok()?),
            };
            if sign > 0.0 {
                fixed(source)
            } else {
                // Inverted, a unipolar source reads `1 − s`.
                Route {
                    timed: Some(Timed::Mapped(ir::Transfer {
                        source,
                        table: vec![1.0, 0.0],
                    })),
                    ..fixed(ir::Source::Constant)
                }
            }
        }
    })
}

/// A connection on a keygroup's `Gain`, which scales it by `1 − r + r·x`,
/// or `1 + r·x` below zero ([`ir::Curve::Signed`], `r` clamped to ±1;
/// UVIWorkstation 0x141429a80, its constants −1 at 0x141cfac24 and 0.5 at
/// 0x141cfaa20). A bipolar source enters at `x = (m + 1)/2` (0x141429b19 on,
/// per sample at 0x141429e30), a unipolar one at `x = m` (0x14142a3e8). An
/// LFO's own depth `d` is inside `m` (0x1410f9b11), so a bipolar LFO enters
/// through `[(1 − d)/2, (1 + d)/2]` and rests at `1 − |r|/2` (AO Meadows:
/// −6 dB with the wheel down). Not the amplitude DAHDSR ([`Translator::envelope`]).
pub(super) fn gain_route(program: Node, connection: Node) -> Option<Route> {
    if source_node(program, connection).is_some_and(|n| n.has_tag_name("DAHDSR")) {
        return None;
    }
    factor(
        route(program, connection, ("Keygroup", "Gain"))?,
        connection,
    )
}

/// `r` for a destination the connection law scales by `1 − r + r·x`, or
/// `1 + r·x` below zero ([`ir::Curve::Signed`], `r` clamped to ±1): an LFO's
/// own depth `d` is inside the source, so it enters through `[(1 − d)/2,
/// (1 + d)/2]`; a rise scales the LFO under that table, so the source is
/// `(1 + ρ·d·w)/2` (0x1410f9767). A mapped source enters as
/// [`matrix::centred`].
pub(super) fn factor(mut r: Route, connection: Node) -> Option<Route> {
    r.ratio = r.ratio.clamp(-1.0, 1.0);
    let d = r.unit;
    let table = vec![(1.0 - d) / 2.0, (1.0 + d) / 2.0];
    match r.timed.take() {
        Some(Timed::Rising(lfo, rise, t)) if t.is_empty() => {
            r.timed = Some(Timed::Rising(lfo, rise, table));
            r.unit = 1.0;
        }
        Some(Timed::Lfo(lfo)) => {
            r.timed = Some(Timed::Steps(lfo, table));
            r.unit = 1.0;
        }
        Some(Timed::Blend(b)) => r.timed = Some(Timed::Blend(blend::gain(b, r.ratio))),
        Some(Timed::Mapped(mut t)) => {
            t.table = matrix::centred(connection, t.table);
            r.timed = Some(Timed::Mapped(t));
        }
        timed => r.timed = timed,
    }
    Some(r)
}

/// A connection on an LFO's `Depth` ([`Translator::depth_scales`]): its
/// source on 0..=1, read `1 − x` for a negative ratio, clamped to ±1. An LFO
/// source enters at `(d·x + 1)/2`, its own depth `d` inside. Not an
/// envelope or a rising LFO read backwards.
fn depth_route(program: Node, connection: Node, owner: &str) -> Option<Route> {
    let mut r = route(program, connection, (owner, "Depth"))?;
    r.ratio = r.ratio.clamp(-1.0, 1.0);
    let falling = r.ratio < 0.0;
    let flip = |t: Vec<f32>| -> Vec<f32> {
        if falling {
            t.into_iter().map(|v| 1.0 - v).collect()
        } else {
            t
        }
    };
    r.timed = match r.timed.take() {
        Some(Timed::Lfo(lfo)) => {
            let d = r.unit;
            Some(Timed::Steps(
                lfo,
                flip(vec![(1.0 - d) / 2.0, (1.0 + d) / 2.0]),
            ))
        }
        Some(Timed::Steps(lfo, table)) => Some(Timed::Steps(lfo, flip(table))),
        Some(Timed::Mapped(mut t)) => {
            if t.table.is_empty() {
                t.table = vec![0.0, 1.0];
            }
            t.table = flip(t.table);
            Some(Timed::Mapped(t))
        }
        None if falling => Some(Timed::Mapped(ir::Transfer {
            source: r.source,
            table: vec![1.0, 0.0],
        })),
        None => None,
        Some(Timed::Envelope(_) | Timed::Rising(..) | Timed::Blend(_)) if falling => return None,
        timed => timed,
    };
    if r.timed.is_some() {
        r.source = ir::Source::Constant;
    }
    r.unit = if falling { -1.0 } else { 1.0 };
    Some(r)
}

/// The source `timed` (or `source`) is in `group`, its modulator copied
/// onto it: a rising LFO is the LFO under an [`ir::ModKind::Intensity`]
/// from its rise.
pub(super) fn group_source(
    group: &mut ir::Group,
    source: ir::Source,
    timed: Option<Timed>,
) -> ir::Source {
    match timed {
        None => source,
        Some(Timed::Lfo(lfo)) => ir::Source::Lfo(index_of(&mut group.lfos, lfo)),
        Some(Timed::Rising(lfo, rise, table)) => {
            let of = ir::Source::Lfo(index_of(&mut group.lfos, lfo));
            let source = ir::Source::Envelope(index_of(&mut group.envelopes, rise));
            let kind = ir::ModKind::Intensity {
                source,
                of,
                depth: 1.0,
            };
            if !group.mods.iter().any(|m| m.kind == kind) {
                group.mods.push(ir::Mod {
                    name: String::new(),
                    slot: 0,
                    target: 0,
                    bypass: false,
                    kind,
                });
            }
            if table.is_empty() {
                of
            } else {
                let t = ir::Transfer { source: of, table };
                ir::Source::Shaped(index_of(&mut group.shapers, t))
            }
        }
        Some(Timed::Envelope(e)) => ir::Source::Envelope(index_of(&mut group.envelopes, e)),
        Some(Timed::Steps(lfo, table)) => {
            let source = ir::Source::Lfo(index_of(&mut group.lfos, lfo));
            ir::Source::Shaped(index_of(&mut group.shapers, ir::Transfer { source, table }))
        }
        Some(Timed::Mapped(t)) => ir::Source::Shaped(index_of(&mut group.shapers, t)),
        Some(Timed::Blend(b)) => blend::lower(group, b),
    }
}

/// A bipolar `LFO` or `MultiLFO` ([`multi`]), with no delay or rise, as an LFO
/// and its signed depth: `Depth·x`. The registry (UVIWorkstation 0x1410f5d00)
/// names `WaveFormType` Sinus, Square, Triangle, RampUp, RampDown, ... and
/// `Retrigger` No Retrigger, Retrigger, Legato, Song; a synchronized `Freq` is
/// the cycle's length in beats (v1). ponytail: `Smooth` is not applied (a
/// lag on the shape; AO's are under 25 ms on cycles of a beat or more).
pub(super) fn lfo(node: Node) -> Option<(ir::Lfo, f32)> {
    let still = !flag(node, "DelayTime") && !flag(node, "RiseTime");
    (still && matrix::bipolar(node))
        .then(|| shape(node, false))
        .flatten()
}

/// [`lfo`], and for one with a `DelayTime` or `RiseTime` the gain its routes
/// reach it through: 0 for the delay, then linearly to 1 over the rise
/// (process 0x1410f8ca0, in 32-sample blocks; the wave starts at its
/// `Phase` once the delay is over). A retriggered LFO's phase is moved back
/// by the delay; one synced to the host is not carried with a delay (its
/// shift depends on the tempo). ponytail: an LFO without a delay alike in
/// every other way shares the delayed one's copy and its rise.
fn rising(node: Node) -> Option<(ir::Lfo, f32, Option<ir::Breakpoints>)> {
    matrix::bipolar(node).then_some(())?;
    let (mut lfo, depth) = shape(node, false)?;
    let seconds = |name| num(node, name, 0.0).ok().map(|t| t.clamp(0.0, 30.0) as f32);
    let (delay, rise) = (seconds("DelayTime")?, seconds("RiseTime")?);
    if delay == 0.0 && rise == 0.0 {
        return Some((lfo, depth, None));
    }
    if delay > 0.0 && lfo.clock == ir::Clock::Voice {
        let ir::Rate::Hz(hz) = lfo.rate else {
            return None;
        };
        lfo.phase = (lfo.phase - hz * delay).rem_euclid(1.0);
    }
    let point = |s: f32, level| ir::Breakpoint {
        ms: s * 1000.0,
        level,
        curve: 0.0,
    };
    let rise = ir::Breakpoints {
        start: 0.0,
        points: vec![point(delay, 0.0), point(rise, 1.0)],
        sustain: None,
        loop_from: None,
        stages: false,
    };
    Some((lfo, depth, Some(rise)))
}

/// [`lfo`] whatever its delay and rise.
pub(super) fn shape(node: Node, user: bool) -> Option<(ir::Lfo, f32)> {
    let unset = |name| !flag(node, name);
    if flag(node, "Bypass") || !unset("ManualTrigger") {
        return None;
    }
    let (wave, sign) = if node.has_tag_name("LFO") {
        match num(node, "WaveFormType", 0.0).ok()? {
            0.0 => (ir::Wave::Sine, 1.0),
            1.0 => (ir::Wave::Square { width: 0.5 }, 1.0),
            2.0 => (ir::Wave::Triangle, 1.0),
            3.0 => (ir::Wave::Saw, -1.0),
            4.0 => (ir::Wave::Saw, 1.0),
            // A user table, read through it ([`blend::table`]).
            9.0 if user => (ir::Wave::Saw, 1.0),
            _ => return None,
        }
    } else if node.has_tag_name("MultiLFO") {
        (multi(node)?, 1.0)
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

/// A `MultiLFO`'s mix at phase `p` (process UVIWorkstation 0x1411497f0):
/// `s·sin 2πp + t·tri + w·(1 − 2p) + q·pulse + n·noise`, the triangle from
/// +1 at 0 and the pulse high below `PulseWidth`, the noise a new uniform
/// draw each half cycle, all negated by `Invert`. `NormalizeOutput` divides
/// by the mix's peak when it passes 1 (0x14114a7a0 evaluates the waves at
/// their extrema and adds `|n|`); here that is folded into the weights.
fn multi(node: Node) -> Option<ir::Wave> {
    let depth = |name| num(node, name, 0.0).ok().map(|d| d as f32);
    let (s, t, w, q, n) = (
        depth("SineDepth")?,
        depth("TriangleDepth")?,
        depth("SawDepth")?,
        depth("SquareDepth")?,
        depth("NoiseDepth")?,
    );
    let width = num(node, "PulseWidth", 0.5).ok()?.clamp(0.0, 1.0) as f32;
    let at = |p: f32| {
        let tri = if p < 0.5 {
            1.0 - 4.0 * p
        } else {
            4.0 * p - 3.0
        };
        let pulse = if p < width { 1.0 } else { -1.0 };
        s * (std::f32::consts::TAU * p).sin() + t * tri + w * (1.0 - 2.0 * p) + q * pulse
    };
    // Dense phases and both sides of the pulse's edges find the peak.
    const STEPS: u16 = 4096;
    let edges = [0.0, width - 1e-6, width, 1.0 - 1e-6];
    // ponytail: one peak per mix for the thread's life (a bank has a few
    // dozen); every route through the LFO asks again.
    thread_local! {
        static PEAKS: RefCell<HashMap<[u32; 5], f32>> = RefCell::new(HashMap::new());
    }
    let key = [s, t, w, q, width].map(f32::to_bits);
    let wave = PEAKS.with_borrow_mut(|peaks| {
        *peaks.entry(key).or_insert_with(|| {
            (0..STEPS)
                .map(|i| f32::from(i) / f32::from(STEPS))
                .chain(edges)
                .map(|p| at(p).abs())
                .fold(0.0, f32::max)
        })
    });
    let peak = wave + n.abs();
    if !peak.is_finite() {
        return None;
    }
    let scale = if flag(node, "NormalizeOutput") {
        peak.max(1.0)
    } else {
        1.0
    };
    let k = if flag(node, "Invert") { -1.0 } else { 1.0 } / scale;
    Some(if (s, t, w, q, n, k) == (1.0, 0.0, 0.0, 0.0, 0.0, 1.0) {
        ir::Wave::Sine
    } else {
        ir::Wave::Multi {
            weights: [-s * k, 0.0, -t * k, -q * k, n * k, w * k],
            width,
            normalize: false,
        }
    })
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

/// A `DAHDSR` as a modulation source: [`ahd`]'s segments after a `DelayTime`
/// at 0, the decay falling to `SustainLevel` and held there, then
/// `ReleaseTime` to 0 (process 0x1410f0570, the same multi-point state as the
/// AHD's). `None` when velocity scales it (`VelocityAmount`, `VelocitySens`).
fn dahdsr(node: Node) -> Option<ir::Breakpoints> {
    if flag(node, "Bypass") || flag(node, "VelocityAmount") || flag(node, "VelocitySens") {
        return None;
    }
    let sustain = num(node, "SustainLevel", 1.0).ok()?.clamp(0.0, 1.0) as f32;
    let point = |time, level, name: &str| {
        Some(ir::Breakpoint {
            ms: (num(node, time, 0.0).ok()?.clamp(0.0, 30.0) * 1000.0) as f32,
            level,
            curve: if name.is_empty() {
                0.0
            } else {
                curve(node, name)?
            },
        })
    };
    Some(ir::Breakpoints {
        start: 0.0,
        points: vec![
            point("DelayTime", 0.0, "")?,
            point("AttackTime", 1.0, "AttackCurve")?,
            point("HoldTime", 1.0, "")?,
            point("DecayTime", sustain, "DecayCurve")?,
            point("ReleaseTime", 0.0, "ReleaseCurve")?,
        ],
        sustain: Some(3),
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
    // ponytail: as `multi`'s peaks, one table per step pattern.
    thread_local! {
        static TABLES: RefCell<HashMap<Vec<u32>, Vec<f32>>> = RefCell::new(HashMap::new());
    }
    let key = levels.iter().map(|l| l.to_bits()).collect();
    let table = TABLES.with_borrow_mut(|tables| {
        tables
            .entry(key)
            .or_insert_with(|| {
                (0..=last)
                    .map(|j| {
                        let step = (n * (last - j)).div_euclid(last) % n;
                        levels.get(step).copied().unwrap_or(0.0)
                    })
                    .collect()
            })
            .clone()
    });
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
