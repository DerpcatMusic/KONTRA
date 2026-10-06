//! UVI Falcon / Workstation programs (`.uvip` XML) to the semantic IR.
//!
//! Attribute names and units follow the v1 UVI reader on
//! `codex/uvi-latest-integration` (`src/uvi/program.rs`, `playback.rs`,
//! `modulation.rs`). Clear `.uvip` with loose samples load through [`load`].
//! With the `library-access` feature, installed UVI banks open through [`Bank`]
//! (ported from v1 `src/uvi/{access,crypto,ufs}.rs` and `src/library/uvi.rs`):
//! reader namespaces come from the user's own installed, hash-verified UVI
//! Workstation and a bank's content state lives only in v1's owner-only private
//! cache; neither is embedded, logged, printed or returned in an error. Every
//! module this translator does not model is listed in `Instrument::unsupported`.

#[cfg(feature = "library-access")]
mod access;
mod access_error;
mod audio;
#[cfg(feature = "library-access")]
mod bank;
#[cfg(feature = "library-access")]
mod crypto;
mod modulation;
#[cfg(not(feature = "library-access"))]
mod no_access;
pub mod script;
pub mod scripted;
mod stream;
#[cfg(feature = "library-access")]
mod ufs;

pub use access_error::AccessError;
#[cfg(feature = "library-access")]
pub use bank::Bank;
#[cfg(not(feature = "library-access"))]
pub use no_access::Bank;

use roxmltree::{Document, Node, ParsingOptions};
use sampler_ir as ir;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

const XML_LIMIT: u64 = 32 << 20;

/// Container nodes whose meaning is their children.
const STRUCTURAL: [&str; 18] = [
    "Mappers",
    "UVI4",
    "Program",
    "Layers",
    "Layer",
    "Keygroups",
    "Keygroup",
    "Oscillators",
    "SamplePlayer",
    "PlaybackOptions",
    "Connections",
    "SignalConnection",
    "ControlSignalSources",
    "EventProcessors",
    "Properties",
    "script",
    "ScriptData",
    "Loop",
];

#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    Xml {
        path: PathBuf,
        error: roxmltree::Error,
    },
    Invalid {
        path: PathBuf,
        reason: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Xml { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Invalid { path, reason } => write!(f, "{}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { error, .. } => Some(error),
            Self::Xml { error, .. } => Some(error),
            Self::Invalid { .. } => None,
        }
    }
}

/// A translated program and the file behind each of its assets.
pub struct Uvi {
    pub instrument: ir::Instrument,
    pub locations: Vec<PathBuf>,
}

/// Translate the program at `path`; sample paths resolve from its folder.
pub fn read(path: &Path) -> Result<Uvi, Error> {
    let text = read_text(path)?;
    translate(&text, path.parent().unwrap_or(Path::new("."))).map_err(|e| match e {
        Translate::Xml(error) => Error::Xml {
            path: path.into(),
            error,
        },
        Translate::Invalid(reason) => Error::Invalid {
            path: path.into(),
            reason,
        },
    })
}

/// Why translation failed, before the path is attached.
#[derive(Debug)]
pub enum Translate {
    Xml(roxmltree::Error),
    Invalid(String),
}

impl std::fmt::Display for Translate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Xml(error) => error.fmt(f),
            Self::Invalid(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for Translate {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Xml(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

/// Parse decoded program XML with the same bounds used by the loader and census.
/// Large installed Falcon programs exceed 200,000 nodes; retain a 32 MiB byte
/// bound and a one-million-node bound, and reject DTDs.
pub fn parse_program_xml(text: &str) -> Result<Document<'_>, Translate> {
    if text.len() as u64 > XML_LIMIT {
        return Err(Translate::Invalid("program exceeds 32 MiB".into()));
    }
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 1_000_000,
            ..Default::default()
        },
    )
    .map_err(Translate::Xml)
}

/// Translate program XML whose relative sample paths resolve from `folder`.
pub fn translate(text: &str, folder: &Path) -> Result<Uvi, Translate> {
    let (instrument, locations) = translate_with(text, Source::Disk(folder.into()))?;
    Ok(Uvi {
        instrument,
        locations: locations.into_iter().map(PathBuf::from).collect(),
    })
}

/// Translate decoded bank program XML; returns each asset's authored
/// bank-relative sample path, resolved by the loader against `program_path`.
fn translate_bank(text: &str) -> Result<(ir::Instrument, Vec<String>), Translate> {
    translate_with(text, Source::Bank)
}

fn translate_with(text: &str, source: Source) -> Result<(ir::Instrument, Vec<String>), Translate> {
    translate_full(text, source).map(|(instrument, locations, _)| (instrument, locations))
}

/// [`translate_with`], plus the IR group of each (layer, oscillator) of a
/// scripted program (empty when the program has no script).
fn translate_full(
    text: &str,
    source: Source,
) -> Result<(ir::Instrument, Vec<String>, Vec<OscGroup>), Translate> {
    let doc = parse_program_xml(text)?;
    let root = doc.root_element();
    let program = match root.tag_name().name() {
        "Program" => root,
        "UVI4" => {
            let mut programs = root.children().filter(|n| n.has_tag_name("Program"));
            match (programs.next(), programs.next()) {
                (Some(program), None) => program,
                _ => {
                    return Err(Translate::Invalid(
                        "expected exactly one UVI4/Program".into(),
                    ));
                }
            }
        }
        other => {
            return Err(Translate::Invalid(format!(
                "expected Program XML, found <{other}>"
            )));
        }
    };
    if program
        .attributes()
        .any(|a| a.name().to_ascii_lowercase().starts_with("password"))
    {
        return Err(Translate::Invalid(
            "protected program: only clear programs are read".into(),
        ));
    }
    let mut out = Translation {
        ir: ir::Instrument {
            name: program.attribute("Name").unwrap_or_default().into(),
            source: ir::SourceFormat::Uvi,
            ..Default::default()
        },
        source,
        assets: HashMap::new(),
        locations: Vec::new(),
        envelopes: HashMap::new(),
        modulator_index: HashMap::new(),
        route_index: HashMap::new(),
        shape_index: HashMap::new(),
        shared_sources: std::collections::HashSet::new(),
        used: Vec::new(),
        osc_groups: Vec::new(),
        split: None,
    };
    out.program(program).map_err(Translate::Invalid)?;
    // Whatever was neither structure nor consumed is reported once per node.
    // Modulation sources and mappers act only through connections, which
    // report what they could not translate.
    for node in program.descendants().filter(|n| n.is_element()) {
        let kind = node.tag_name().name();
        if !STRUCTURAL.contains(&kind)
            && !out.used.contains(&node.id())
            && !node.ancestors().skip(1).any(|a| out.used.contains(&a.id()))
            && !node.ancestors().any(|a| {
                a.parent().is_some_and(|p| {
                    matches!(p.tag_name().name(), "ControlSignalSources" | "Mappers")
                })
            })
        {
            let bypassed = node.attribute("Bypass") == Some("1");
            out.unsupported(
                &path(node),
                "module",
                format!("{kind}{}", if bypassed { " (bypassed)" } else { "" }),
            );
        }
    }
    out.ir
        .validate()
        .map_err(|e| Translate::Invalid(e.to_string()))?;
    Ok((out.ir, out.locations, out.osc_groups))
}

/// A node's own `SignalConnection`s.
fn connections<'a>(node: Node<'a, 'a>) -> impl Iterator<Item = Node<'a, 'a>> {
    node.children()
        .filter(|n| n.has_tag_name("Connections"))
        .flat_map(|c| c.children())
        .filter(|n| n.has_tag_name("SignalConnection"))
}

/// `Program/Layer "Name"/Keygroup "Name"/...`, for reports.
fn path(node: Node) -> String {
    let mut parts: Vec<String> = node
        .ancestors()
        .filter(|n| {
            n.is_element()
                && !matches!(
                    n.tag_name().name(),
                    "Layers" | "Keygroups" | "Oscillators" | "UVI4"
                )
        })
        .map(|n| match n.attribute("Name") {
            Some(name) => format!("{} {name:?}", n.tag_name().name()),
            None => n.tag_name().name().to_owned(),
        })
        .collect();
    parts.reverse();
    parts.join("/")
}

/// A UVI stage-curve value `c` in (-1, 1) as the native envelope curvature.
/// `k = 2·ln((1+c)/(1-c))` makes `expm1(k·t)/expm1(k)` equal v1's envelope law.
fn curve(c: f64) -> ir::Curve {
    let c = c.clamp(-0.9998, 0.9998);
    if c == 0.0 {
        ir::Curve::Linear
    } else {
        ir::Curve::Exponential(2.0 * ((1.0 + c) / (1.0 - c)).ln())
    }
}

fn number(node: Node, name: &str, default: f64) -> Result<f64, String> {
    match node.attribute(name) {
        None => Ok(default),
        Some(text) => text
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("{}: {name}={text:?}", path(node))),
    }
}

fn midi(node: Node, name: &str, default: u8) -> Result<u8, String> {
    let value = number(node, name, f64::from(default))?;
    if (0.0..=127.0).contains(&value) && value.fract() == 0.0 {
        Ok(value as u8)
    } else {
        Err(format!(
            "{}: {name}={value} is not a MIDI value",
            path(node)
        ))
    }
}

/// Where a program's samples live: loose on disk (resolved and existence-checked
/// against a folder) or inside the open bank (resolved later by the loader).
enum Source {
    Disk(PathBuf),
    Bank,
}

struct Translation {
    ir: ir::Instrument,
    source: Source,
    assets: HashMap<String, ir::AssetRef>,
    /// Per asset, its sample path as resolved for the loader: an absolute disk
    /// path for loose samples, or the authored bank-relative path for a bank.
    locations: Vec<String>,
    /// Envelope modulators by their source node, with their velocity law.
    envelopes: HashMap<roxmltree::NodeId, (ir::ModulatorRef, ir::VelocityResponse)>,
    /// Shared modulators, routes and shapes by identity (see `modulation`).
    modulator_index: HashMap<String, ir::ModulatorRef>,
    route_index: HashMap<String, ir::RouteRef>,
    shape_index: HashMap<String, ir::ShapeRef>,
    /// Program- and layer-level source nodes already given their shared-state
    /// report entry.
    shared_sources: std::collections::HashSet<roxmltree::NodeId>,
    /// Nodes whose meaning was carried into the IR.
    used: Vec<roxmltree::NodeId>,
    osc_groups: Vec<OscGroup>,
    /// The layer being translated, when a script may pick its oscillators.
    split: Option<(usize, ir::Group)>,
}

/// The IR group holding oscillator `osc` (1-based, counting bypassed ones) of
/// the keygroups of layer `layer` (1-based, as `Program.layers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OscGroup {
    pub layer: u32,
    pub osc: u32,
    pub group: u32,
}

impl Translation {
    fn unsupported(&mut self, location: &str, feature: &str, value: impl std::fmt::Display) {
        self.ir.unsupported.push(ir::Unsupported {
            location: location.into(),
            feature: feature.into(),
            value: value.to_string(),
            reason: ir::Reason::NotModeled,
        });
    }

    /// Program and layer connections act on the mixed signal of their scope.
    fn scope_connections(&mut self, scope: Node) -> Result<(), String> {
        for connection in connections(scope) {
            if number(connection, "Bypass", 0.0)? == 0.0 && number(connection, "Ratio", 1.0)? != 0.0
            {
                self.unsupported(
                    &path(connection),
                    "program or layer modulation",
                    format!(
                        "{} -> {}",
                        connection.attribute("Source").unwrap_or_default(),
                        connection.attribute("Destination").unwrap_or_default()
                    ),
                );
            }
        }
        Ok(())
    }

    fn program(&mut self, program: Node) -> Result<(), String> {
        let gain = number(program, "Gain", 1.0)?;
        for processor in program
            .descendants()
            .filter(|n| n.has_tag_name("ScriptProcessor"))
        {
            self.used.push(processor.id());
            let source: String = processor
                .descendants()
                .filter(|n| n.has_tag_name("script"))
                .filter_map(|n| n.text())
                .collect();
            self.ir.behaviors.push(ir::Behavior {
                name: processor.attribute("Name").unwrap_or("script").into(),
                language: ir::Language::Lua,
                source,
                slot: None,
                state: Vec::new(),
                requires: Vec::new(),
            });
        }
        self.scope_connections(program)?;
        let scripted = !self.ir.behaviors.is_empty();
        // Program-level aux effects become buses (their own effects are
        // reported as modules); layers reach them through BusRouters.
        let mut auxes: Vec<(String, ir::BusRef)> = Vec::new();
        for aux in program
            .children()
            .filter(|n| n.has_tag_name("Auxs"))
            .flat_map(|a| a.children().filter(|c| c.has_tag_name("AuxEffect")))
        {
            let name = aux.attribute("Name").unwrap_or_default().to_owned();
            self.ir.buses.push(ir::Bus {
                name: name.clone(),
                chain: None,
                sends: Vec::new(),
                output: ir::Output::Master,
            });
            auxes.push((name, ir::BusRef(self.ir.buses.len() - 1)));
        }
        for (ordinal, layer) in program
            .descendants()
            .filter(|n| n.has_tag_name("Layer"))
            .enumerate()
        {
            if number(layer, "Mute", 0.0)? != 0.0 {
                continue;
            }
            self.scope_connections(layer)?;
            let pan = number(layer, "Pan", 0.0)?;
            let mut base = ir::Group {
                name: layer.attribute("Name").unwrap_or_default().into(),
                gain: ir::Gain::Linear(gain * number(layer, "Gain", 1.0)?),
                pan: ir::Pan {
                    position: pan.clamp(-1.0, 1.0),
                    law: ir::PanLaw::Balance,
                },
                ..Default::default()
            };
            // A closed fader with a pre-fader send: the sound is the send's.
            // (The IR has one output per group: the first such send is used.)
            if number(layer, "Gain", 1.0)? == 0.0 {
                let send = layer
                    .children()
                    .filter(|n| n.has_tag_name("BusRouters"))
                    .flat_map(|r| r.children().filter(|c| c.has_tag_name("BusRouter")))
                    .filter(|r| number(*r, "PreFader", 0.0).is_ok_and(|p| p != 0.0))
                    .filter(|r| number(*r, "Bypass", 0.0).is_ok_and(|b| b == 0.0))
                    .find_map(|r| {
                        let to = r.attribute("Destination")?.rsplit('/').next()?;
                        let bus = auxes.iter().find(|(n, _)| n == to)?.1;
                        Some((bus, number(r, "Gain", 1.0).ok()?))
                    });
                if let Some((bus, send_gain)) = send.filter(|s| s.1 > 0.0) {
                    base.gain = ir::Gain::Linear(gain * send_gain);
                    base.output = ir::Output::Bus(bus);
                    self.unsupported(
                        &path(layer),
                        "closed layer fader: its pre-fader send is played as the layer output",
                        send_gain,
                    );
                }
            }
            // Oscillators get groups of their own only where a keygroup stacks
            // several; otherwise the layer's group is oscillator 1.
            let stacked = layer
                .descendants()
                .filter(|n| n.has_tag_name("Keygroup"))
                .any(|k| k.descendants().filter(|n| n.has_tag_name("SamplePlayer")).count() > 1);
            self.split = (scripted && stacked).then(|| (ordinal + 1, base.clone()));
            if self.split.is_none() {
                self.ir.groups.push(base);
            }
            if scripted && !stacked {
                self.osc_groups.push(OscGroup {
                    layer: ordinal as u32 + 1,
                    osc: 1,
                    group: self.ir.groups.len() as u32 - 1,
                });
            }
            if pan != 0.0 {
                self.unsupported(
                    &path(layer),
                    "cos² balance pan law (linear balance used)",
                    pan,
                );
            }
            // (Split layers have no group of their own: every zone takes an
            // oscillator group, see `oscillator_group`.)
            let group = ir::GroupRef(self.ir.groups.len().saturating_sub(1));
            // A script's playNote can trigger one oscillator of a keygroup
            // (oscIndex); scripts do not run, so every oscillator plays.
            let stacked = layer
                .descendants()
                .filter(|n| n.has_tag_name("Keygroup"))
                .map(|k| {
                    k.descendants()
                        .filter(|n| n.has_tag_name("SamplePlayer"))
                        .filter(|n| number(*n, "Bypass", 0.0).is_ok_and(|b| b == 0.0))
                        .count()
                })
                .filter(|&n| n > 1);
            let (keygroups, most) = stacked.fold((0, 0), |(k, m), n| (k + 1, m.max(n)));
            if keygroups > 0 && !self.ir.behaviors.is_empty() {
                self.unsupported(
                    &path(layer),
                    "keygroup oscillators all play (the script may pick one per note)",
                    format!("{keygroups} keygroups, up to {most} oscillators"),
                );
            }
            let keys = (midi(layer, "LowKey", 0)?, midi(layer, "HighKey", 127)?);
            for keygroup in layer.descendants().filter(|n| n.has_tag_name("Keygroup")) {
                self.keygroup(keygroup, group, keys)?;
            }
        }
        Ok(())
    }

    fn keygroup(
        &mut self,
        keygroup: Node,
        group: ir::GroupRef,
        layer_keys: (u8, u8),
    ) -> Result<(), String> {
        let at = path(keygroup);
        let keys = (
            midi(keygroup, "LowKey", 0)?.max(layer_keys.0),
            midi(keygroup, "HighKey", 127)?.min(layer_keys.1),
        );
        let velocities = (
            midi(keygroup, "LowVelocity", 1)?.max(1),
            midi(keygroup, "HighVelocity", 127)?.max(1),
        );
        if keys.0 > keys.1 || velocities.0 > velocities.1 {
            return Ok(()); // Outside its layer's keys: never plays.
        }
        let fades = [
            "LowKeyFade",
            "HighKeyFade",
            "LowVelocityFade",
            "HighVelocityFade",
        ];
        for fade in fades {
            if number(keygroup, fade, 0.0)? != 0.0 {
                self.unsupported(&at, fade, number(keygroup, fade, 0.0)?);
            }
        }
        // The amplitude envelope: a plain Gain connection from a DAHDSR or
        // AnalogADSR. Every other connection is a route or a static factor.
        let mut amplitude = None;
        let mut shared = modulation::Modulation::default();
        for connection in connections(keygroup) {
            let plain = number(connection, "Ratio", 1.0)? == 1.0
                && number(connection, "Bypass", 0.0)? == 0.0
                && number(connection, "Inverted", 0.0)? == 0.0
                && number(connection, "ConnectionMode", 0.0)? == 0.0
                && connection
                    .attribute("Mapper")
                    .unwrap_or_default()
                    .is_empty()
                && connection.children().all(|c| !c.is_element());
            let envelope = modulation::source_node(
                keygroup,
                connection.attribute("Source").unwrap_or_default(),
            )
            .filter(|n| matches!(n.tag_name().name(), "DAHDSR" | "AnalogADSR"))
            .filter(|n| number(*n, "Bypass", 0.0).is_ok_and(|b| b == 0.0));
            match envelope {
                Some(envelope)
                    if connection.attribute("Destination") == Some("Gain")
                        && plain
                        && amplitude.is_none() =>
                {
                    amplitude = Some(self.envelope(envelope)?);
                }
                _ => self.connect(connection, &mut shared)?,
            }
        }
        let (amplitude, velocity) = match amplitude {
            Some((modulator, velocity)) => (Some(modulator), velocity),
            None => (None, ir::VelocityResponse::None),
        };
        let gain = number(keygroup, "Gain", 1.0)?;
        let pan = number(keygroup, "Pan", 0.0)?;
        for (oscillator, player) in keygroup
            .descendants()
            .filter(|n| n.has_tag_name("SamplePlayer"))
            .enumerate()
        {
            if number(player, "Bypass", 0.0)? != 0.0 {
                continue;
            }
            let group = self.oscillator_group(group, oscillator as u32 + 1);
            let at = path(player);
            let Some(sample) = player.attribute("SamplePath").filter(|p| !p.is_empty()) else {
                self.unsupported(&at, "sample player without a sample", "");
                continue;
            };
            let Some(asset) = self.asset(&at, sample) else {
                continue;
            };
            let start = number(player, "SampleStart", 0.0)?;
            if start != 0.0 {
                self.unsupported(&at, "SampleStart", start);
            }
            let tracking = number(player, "NoteTracking", 1.0)?;
            let root = midi(player, "BaseNote", 60)?;
            let pitch = match tracking {
                1.0 => ir::KeyTracking::Tracked { root },
                0.0 => ir::KeyTracking::Fixed,
                t => ir::KeyTracking::Scaled {
                    root,
                    cents_per_key: (t * 100.0).round() as i32,
                },
            };
            let player_pan = number(player, "Pan", 0.0)?;
            if player_pan != 0.0 && pan != 0.0 {
                self.unsupported(
                    &at,
                    "pan at both keygroup and player (summed)",
                    format!("{pan} + {player_pan}"),
                );
            }
            let playback = self.playback(player)?;
            let mut modulation = shared.clone();
            for connection in connections(player) {
                self.connect(connection, &mut modulation)?;
            }
            if amplitude.is_none()
                && modulation.routes.iter().any(|r| {
                    let route = &self.ir.routes[r.0];
                    route.target == ir::Target::Amplitude
                        && matches!(
                            self.ir.modulators[route.source.0].source,
                            ir::ModulationSource::Envelope(_)
                        )
                })
            {
                self.unsupported(
                    &at,
                    "envelope gain route without an amplitude envelope (voice ends at note-off)",
                    "",
                );
            }
            self.ir.zones.push(ir::Zone {
                group: Some(group),
                keys: ir::KeyRange {
                    low: keys.0,
                    high: keys.1,
                },
                velocities: ir::VelocityRange {
                    low: velocities.0,
                    high: velocities.1,
                },
                pitch,
                tune: ir::Pitch::Semitones(
                    // Pitch is the semitone base the Pitch connections add to.
                    number(player, "CoarseTune", 0.0)?
                        + number(player, "FineTune", 0.0)? / 100.0
                        + number(player, "Pitch", 0.0)?
                        + modulation.pitch,
                ),
                gain: ir::Gain::Linear(gain * number(player, "Gain", 1.0)? * modulation.gain),
                velocity,
                pan: ir::Pan {
                    position: (pan + player_pan + modulation.pan).clamp(-1.0, 1.0),
                    law: ir::PanLaw::Balance,
                },
                playback,
                amplitude,
                routes: modulation.routes,
                ..ir::Zone::new(asset)
            });
        }
        Ok(())
    }

    /// The group a zone of oscillator `osc` goes to: the layer's own, or in a
    /// scripted program one group per (layer, oscillator) so that `playNote`'s
    /// `oscIndex` can select it.
    fn oscillator_group(&mut self, layer_group: ir::GroupRef, osc: u32) -> ir::GroupRef {
        let Some((layer, base)) = &self.split else {
            return layer_group;
        };
        let layer = *layer as u32;
        if let Some(found) = self
            .osc_groups
            .iter()
            .find(|g| g.layer == layer && g.osc == osc)
        {
            return ir::GroupRef(found.group as usize);
        }
        let mut group = base.clone();
        group.name = format!("{} osc {osc}", base.name);
        self.ir.groups.push(group);
        let index = self.ir.groups.len() - 1;
        self.osc_groups.push(OscGroup {
            layer,
            osc,
            group: index as u32,
        });
        ir::GroupRef(index)
    }

    fn envelope(&mut self, node: Node) -> Result<(ir::ModulatorRef, ir::VelocityResponse), String> {
        if let Some(&found) = self.envelopes.get(&node.id()) {
            return Ok(found);
        }
        self.used.push(node.id());
        let at = path(node);
        let kind = node.tag_name().name();
        let seconds =
            |name, max| number(node, name, 0.0).map(|t| ir::Time::Seconds(t.clamp(0.0, max)));
        let mut envelope = ir::Envelope {
            delay: if kind == "DAHDSR" {
                seconds("DelayTime", 10.0)?
            } else {
                ir::Time::ZERO
            },
            attack: seconds("AttackTime", 10.0)?,
            hold: if kind == "DAHDSR" {
                seconds("HoldTime", 10.0)?
            } else {
                ir::Time::ZERO
            },
            decay: seconds("DecayTime", 30.0)?,
            sustain: number(node, "SustainLevel", 1.0)?.clamp(0.0, 1.0),
            release: number(node, "ReleaseTime", 0.05)
                .map(|t| ir::Time::Seconds(t.clamp(0.0, 10.0)))?,
            ..Default::default()
        };
        if kind == "DAHDSR" {
            // The native envelope curve `expm1(k·t)/expm1(k)` is exactly v1's
            // `envelope_curve` (src/uvi/modulation.rs) at k = 2·ln((1+c)/(1-c)),
            // so a DAHDSR's per-stage curve translates without approximation.
            envelope.attack_shape = curve(number(node, "AttackCurve", 0.0)?);
            envelope.decay_shape = curve(number(node, "DecayCurve", 0.0)?);
            envelope.release_shape = curve(number(node, "ReleaseCurve", 0.0)?);
        } else {
            // AnalogADSR integrates an RC stage law this envelope does not model.
            self.unsupported(&at, "analog ADSR stage law (linear stages used)", "");
        }
        // v1 measured law: level × (1 − a + a·velocity^(1 − log2(1 − sensitivity))),
        // a = VelocityAmount; sensitivity 1 gates at 127.
        let sensitivity = number(node, "VelocitySens", 0.75)?.clamp(-1.0, 1.0);
        let amount = number(node, "VelocityAmount", 0.0)?.clamp(0.0, 1.0);
        let velocity = if amount == 0.0 {
            ir::VelocityResponse::None
        } else if sensitivity < 1.0 {
            ir::VelocityResponse::Power(1.0 - (1.0 - sensitivity).log2())
        } else {
            self.unsupported(&at, "VelocitySens", sensitivity);
            ir::VelocityResponse::Linear
        };
        if amount != 0.0 && amount != 1.0 {
            self.unsupported(&at, "VelocityAmount (full amount used)", amount);
        }
        // v1 measured only per-voice envelopes released at note-off.
        for (name, expected) in [("Retrigger", 1.0), ("NoteOffRetrigger", 0.0)] {
            let value = number(node, name, expected)?;
            if value != expected {
                self.unsupported(&at, name, value);
            }
        }
        self.ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(envelope),
        });
        let found = (ir::ModulatorRef(self.ir.modulators.len() - 1), velocity);
        self.envelopes.insert(node.id(), found);
        Ok(found)
    }

    fn asset(&mut self, at: &str, sample: &str) -> Option<ir::AssetRef> {
        let relative = sample.replace('\\', "/");
        let encoding = match relative
            .rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("wav") => ir::Encoding::Wav,
            Some("flac") => ir::Encoding::Flac,
            Some("aif" | "aiff") => ir::Encoding::Aiff,
            Some("ogg") => ir::Encoding::Ogg,
            _ => ir::Encoding::Unknown,
        };
        if relative.starts_with('$') || relative.contains(".ufs/") {
            self.unsupported(at, "sample outside the program's bank", sample);
            return None;
        }
        if !matches!(
            encoding,
            ir::Encoding::Wav | ir::Encoding::Flac | ir::Encoding::Aiff
        ) {
            self.unsupported(
                at,
                "sample encoding (WAV, FLAC and AIFF are decoded)",
                sample,
            );
            return None;
        }
        // A bundle base keeps its trailing separator; otherwise collapse `.`/`..`.
        let resolved = match &self.source {
            Source::Bank => relative.clone(),
            Source::Disk(folder) => {
                let location = folder.join(&relative);
                if !location.is_file() {
                    self.unsupported(at, "missing sample", location.display());
                    return None;
                }
                location.to_string_lossy().into_owned()
            }
        };
        if let Some(&asset) = self.assets.get(&resolved) {
            return Some(asset);
        }
        self.ir.assets.push(ir::Asset {
            location: ir::AssetLocation::Path(resolved.clone()),
            encoding,
            root_key: None,
            loops: Vec::new(),
        });
        self.locations.push(resolved.clone());
        let asset = ir::AssetRef(self.ir.assets.len() - 1);
        self.assets.insert(resolved, asset);
        Some(asset)
    }

    fn playback(&mut self, player: Node) -> Result<ir::Playback, String> {
        let mut playback = ir::Playback {
            reverse: number(player, "Reverse", 0.0)? != 0.0,
            ..Default::default()
        };
        let Some(options) = player
            .children()
            .find(|n| n.has_tag_name("PlaybackOptions"))
        else {
            return Ok(playback);
        };
        let at = path(options);
        let frames = |node, name| number(node, name, 0.0).map(|v| v.max(0.0).round() as u64);
        playback.start = frames(options, "Start")?;
        playback.end = options
            .attribute("Stop")
            .map(|_| frames(options, "Stop"))
            .transpose()?;
        let direction = number(options, "PlayDirection", 0.0)?;
        if direction != 0.0 {
            self.unsupported(&at, "PlayDirection", direction);
        }
        let mut loops = options.children().filter(|n| n.has_tag_name("Loop"));
        if let Some(l) = loops.next() {
            let kind = number(l, "Type", 0.0)?;
            let range = ir::LoopRange {
                start: frames(l, "Start")?,
                end: frames(l, "End")?,
                crossfade: ir::Span::ZERO,
                alternating: false,
            };
            if kind == 0.0 && range.start < range.end {
                playback.looping = ir::Looping::Continuous(range);
            } else {
                self.unsupported(
                    &path(l),
                    "loop type or range",
                    format!("type {kind}, {}..{}", range.start, range.end),
                );
            }
        }
        if loops.next().is_some() {
            self.unsupported(&at, "additional loops", "");
        }
        Ok(playback)
    }
}

/// Load a loose program, a virtual `bank.ufs/member.uvip` path, or the first
/// program in a UFS bank. [`load_program`] selects a specific bank member.
/// Protected programs use the installed reader behind `library-access`.
pub fn load(path: &Path, rate: u32) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    assemble_translated(translate_path(path)?, rate)
}

/// A translated program whose samples are not decoded yet.
pub struct Translated {
    pub instrument: ir::Instrument,
    pub locations: Vec<String>,
    /// The bank and program path the samples come from; loose files have none.
    bank: Option<(Bank, String)>,
    /// Where each (layer, oscillator) of the program plays, for its scripts.
    pub groups: Vec<OscGroup>,
    /// The program's XML and the bank's Lua members, for its scripts.
    text: String,
    lua: script::Scripts,
}

/// A program's scripts loaded on a script thread for a host that owns the runtime.
pub struct AttachedScript {
    pub driver: scripted::Driver<scripted::ScriptThread>,
    /// The script's widgets.
    pub interface: sampler_ui_ir::Interface,
}

impl Translated {
    /// Start the program's Lua scripts on their own thread and mark the
    /// instrument as scripted: what they replace is no longer reported, what
    /// they use that is not modeled is. `None` when the program has none.
    pub fn attach_script(
        &mut self,
        rate: u32,
        config: script::Config,
    ) -> Result<Option<AttachedScript>, String> {
        if self.instrument.behaviors.is_empty() {
            return Ok(None);
        }
        let (thread, loaded) =
            scripted::ScriptThread::spawn(self.text.clone(), self.lua.clone(), config)?;
        let unsupported = &mut self.instrument.unsupported;
        if scripted::Script::handles_notes(&thread) {
            unsupported.retain(|u| !u.feature.starts_with("keygroup oscillators all play"));
        }
        unsupported.retain(|u| !(u.feature == "script" && u.value.contains("no frontend")));
        for finding in loaded.findings {
            unsupported.push(ir::Unsupported {
                location: "script".into(),
                feature: finding.feature,
                value: format!("{} (x{})", finding.value, finding.count),
                reason: ir::Reason::NotModeled,
            });
        }
        let groups = self.groups.clone();
        Ok(Some(AttachedScript {
            driver: scripted::Driver::new(thread, groups, rate),
            interface: loaded.interface,
        }))
    }
}

/// Translate what [`load`] accepts without decoding samples, so a host can
/// shape the instrument (mixer buses) before [`assemble_translated`].
pub fn translate_path(path: &Path) -> Result<Translated, Box<dyn std::error::Error>> {
    if let Some(bank_path) = path
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
    {
        let bank = Bank::open(bank_path)?;
        let member = path
            .strip_prefix(bank_path)?
            .to_string_lossy()
            .replace('\\', "/");
        let member = if member.is_empty() {
            bank.programs()
                .into_iter()
                .next()
                .ok_or_else(|| Error::Invalid {
                    path: bank_path.into(),
                    reason: "bank has no programs".into(),
                })?
        } else {
            member
        };
        let (text, program_path) = bank.program(&member)?;
        let (instrument, locations, groups) = translate_full(&text, Source::Bank)
            .map_err(|e| describe(Path::new(&member), e))?;
        let lua = bank.scripts();
        return Ok(Translated {
            instrument,
            locations,
            bank: Some((bank, program_path)),
            groups,
            text,
            lua,
        });
    }
    let text = read_text(path)?;
    let (instrument, locations, groups) =
        translate_full(&text, Source::Disk(path.parent().unwrap_or(Path::new(".")).into()))
            .map_err(|e| describe(path, e))?;
    Ok(Translated { instrument, locations, bank: None, groups, text, lua: Default::default() })
}

/// Decode a [`translate_path`] result's samples and lower it.
pub fn assemble_translated(
    t: Translated,
    rate: u32,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    let decoded = t
        .locations
        .iter()
        .map(|location| match &t.bank {
            Some((bank, program_path)) => bank
                .resource(program_path, location)
                .map_err(|e| e.to_string())
                .and_then(|parts| audio::decode(&parts).map(|(d, _)| d)),
            None => decode_sample(Path::new(location)).map_err(|e| e.to_string()),
        })
        .collect();
    assemble(
        t.instrument,
        t.locations,
        decoded,
        &sampler_kontakt::Options {
            rate,
            ..Default::default()
        },
    )
}

/// [`assemble_translated`], streamed: only the frames where zones start and a
/// page pool are resident; the rest is read from the bank or file on demand.
pub fn assemble_translated_streamed(
    t: Translated,
    rate: u32,
    policy: &sampler_kontakt::StreamPolicy,
) -> Result<sampler_kontakt::Streamed, Box<dyn std::error::Error>> {
    let sources = t
        .locations
        .iter()
        .map(|location| match &t.bank {
            #[cfg(feature = "library-access")]
            Some((bank, program_path)) => bank.stream_source(program_path, location),
            #[cfg(not(feature = "library-access"))]
            Some(_) => Err("bank samples need the library-access feature".to_string()),
            None => stream::source(vec![stream::Origin::File(location.into())]),
        })
        .collect();
    assemble_streamed(t.instrument, t.locations, sources, rate, policy)
}

/// Decode one loose WAV, AIFF or FLAC sample to in-memory stereo frames.
/// Both the encoded input and decoded audio are bounded to 512 MiB.
pub fn decode_sample(path: &Path) -> Result<sampler_kontakt::Decoded, Error> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take((512 << 20) + 1).read_to_end(&mut bytes))
        .map_err(|error| Error::Io {
            path: path.into(),
            error,
        })?;
    audio::decode(&[bytes])
        .map(|(audio, _)| audio)
        .map_err(|reason| Error::Invalid {
            path: path.into(),
            reason,
        })
}

/// Load a clear `.uvip` program streamed: only the frames where zones start
/// and a page pool are resident (see [`sampler_kontakt::stream_instrument`]).
pub fn load_streamed(
    path: &Path,
    rate: u32,
    policy: &sampler_kontakt::StreamPolicy,
) -> Result<sampler_kontakt::Streamed, Box<dyn std::error::Error>> {
    let folder = path.parent().unwrap_or(Path::new("."));
    let (instrument, locations) =
        translate_with(&read_text(path)?, Source::Disk(folder.into())).map_err(|e| describe(path, e))?;
    let sources = locations
        .iter()
        .map(|location| stream::source(vec![stream::Origin::File(location.into())]))
        .collect();
    assemble_streamed(instrument, locations, sources, rate, policy)
}

/// [`load_program`], streamed.
#[cfg(feature = "library-access")]
pub fn load_program_streamed(
    bank: &Bank,
    program: &str,
    rate: u32,
    policy: &sampler_kontakt::StreamPolicy,
) -> Result<sampler_kontakt::Streamed, Box<dyn std::error::Error>> {
    let (text, program_path) = bank.program(program)?;
    let (instrument, locations) =
        translate_bank(&text).map_err(|e| describe(Path::new(program), e))?;
    let sources = locations
        .iter()
        .map(|authored| bank.stream_source(&program_path, authored))
        .collect();
    assemble_streamed(instrument, locations, sources, rate, policy)
}

/// Compare streamed reads of a bank resource with a full decode at assorted
/// ranges (start, middle, end, backward). Returns its frame count.
#[doc(hidden)]
#[cfg(feature = "library-access")]
pub fn check_stream(bank: &Bank, program_path: &str, path: &str) -> Result<usize, String> {
    use sampler_kontakt::AssetSource;
    let full = audio::decode(&bank.resource(program_path, path).map_err(|e| e.to_string())?)?.0.frames;
    let mut reader = bank.stream_source(program_path, path)?.open().map_err(|e| e.to_string())?;
    if reader.frames() != full.len() {
        return Err(format!("{} streamed frames, {} decoded", reader.frames(), full.len()));
    }
    let n = full.len();
    for range in [0..n.min(5000), n / 2..(n / 2 + 9000).min(n), n.saturating_sub(7000)..n, 100.min(n)..300.min(n), 0..n.min(30000)] {
        let mut out = vec![[0.0; 2]; range.len()];
        reader.read(range.start, &mut out).map_err(|e| e.to_string())?;
        if out != full[range.clone()] {
            return Err(format!("frames {range:?} differ"));
        }
    }
    Ok(n)
}

type AssetSources = Vec<Result<std::sync::Arc<dyn sampler_kontakt::AssetSource>, String>>;

/// [`assemble`] for streamed samples: zones whose sample cannot be opened are dropped (reported).
fn assemble_streamed(
    mut instrument: ir::Instrument,
    locations: Vec<String>,
    sources: AssetSources,
    rate: u32,
    policy: &sampler_kontakt::StreamPolicy,
) -> Result<sampler_kontakt::Streamed, Box<dyn std::error::Error>> {
    for (location, result) in locations.iter().zip(&sources) {
        if let Err(reason) = result {
            instrument.unsupported.push(ir::Unsupported {
                location: location.clone(),
                feature: "unreadable sample, zone dropped".into(),
                value: reason.clone(),
                reason: ir::Reason::InvalidValue,
            });
        }
    }
    let usable: Vec<bool> = sources.iter().map(Result::is_ok).collect();
    let kept = instrument.retain_zones(|z| usable[z.asset.0]);
    let mut sources: Vec<_> = sources.into_iter().map(Result::ok).collect();
    let sources = kept.iter().map(|&a| sources[a].take().expect("usable")).collect();
    let labels = kept.iter().map(|&a| locations[a].clone()).collect();
    let options = sampler_kontakt::Options {
        rate,
        scripts: true,
        ..Default::default()
    };
    Ok(sampler_kontakt::stream_instrument(instrument, sources, labels, &options, policy)?)
}

/// Load a program inside an installed UVI bank. `bank` is an open [`Bank`]; `program`
/// is a member path from [`Bank::programs`]. Samples are read from the bank.
pub fn load_program(
    bank: &Bank,
    program: &str,
    rate: u32,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    load_program_with_options(
        bank,
        program,
        &sampler_kontakt::Options {
            rate,
            ..Default::default()
        },
    )
}

/// Load only sample zones overlapping `options.keys`, as Kontakt's loader does.
/// This bounds offline renders to the played range without changing translation.
pub fn load_program_with_options(
    bank: &Bank,
    program: &str,
    options: &sampler_kontakt::Options,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    let (text, program_path) = bank.program(program)?;
    let (mut instrument, locations) =
        translate_bank(&text).map_err(|e| describe(Path::new(program), e))?;
    let kept = instrument.retain_zones(|zone| {
        zone.keys.high >= *options.keys.start() && zone.keys.low <= *options.keys.end()
    });
    let locations: Vec<_> = kept.iter().map(|&asset| locations[asset].clone()).collect();
    let decoded = locations
        .iter()
        .map(|authored| {
            bank.resource(&program_path, authored)
                .map_err(|e| e.to_string())
                .and_then(|parts| audio::decode(&parts).map(|(d, _)| d))
        })
        .collect();
    assemble(instrument, locations, decoded, options)
}

/// [`load_program`] with the program's Lua scripts run: the returned
/// [`scripted::Program`] plays through [`scripted::Player`]. What the scripts
/// use that is not modeled is added to `instrument.unsupported`.
#[cfg(feature = "library-access")]
pub fn load_program_scripted(
    bank: &Bank,
    program: &str,
    rate: u32,
) -> Result<scripted::Program, Box<dyn std::error::Error>> {
    let options = sampler_kontakt::Options { rate, ..Default::default() };
    load_program_scripted_with_options(bank, program, &options)
}

/// [`load_program_scripted`] keeping only the zones overlapping `options.keys`.
#[cfg(feature = "library-access")]
pub fn load_program_scripted_with_options(
    bank: &Bank,
    program: &str,
    options: &sampler_kontakt::Options,
) -> Result<scripted::Program, Box<dyn std::error::Error>> {
    let (text, program_path) = bank.program(program)?;
    let (mut instrument, locations, groups) = translate_full(&text, Source::Bank)
        .map_err(|e| describe(Path::new(program), e))?;
    let kept = instrument.retain_zones(|zone| {
        zone.keys.high >= *options.keys.start() && zone.keys.low <= *options.keys.end()
    });
    let locations: Vec<_> = kept.iter().map(|&asset| locations[asset].clone()).collect();
    let decoded = locations
        .iter()
        .map(|authored| {
            bank.resource(&program_path, authored)
                .map_err(|e| e.to_string())
                .and_then(|parts| audio::decode(&parts).map(|(d, _)| d))
        })
        .collect();
    let loaded = assemble(instrument, locations, decoded, options)?;
    let host = script::ScriptHost::new(&text, bank.scripts(), script::Config::default())?;
    let mut instrument = loaded.instrument;
    if host.handles_notes() {
        // The script picks the oscillators now.
        instrument
            .unsupported
            .retain(|u| !u.feature.starts_with("keygroup oscillators all play"));
    }
    // The scripts run; what they use that is not modeled is listed below.
    instrument
        .unsupported
        .retain(|u| !(u.feature == "script" && u.value.contains("no frontend")));
    for finding in host.findings() {
        instrument.unsupported.push(ir::Unsupported {
            location: "script".into(),
            feature: finding.feature,
            value: format!("{} (x{})", finding.value, finding.count),
            reason: ir::Reason::NotModeled,
        });
    }
    Ok(scripted::Program {
        instrument,
        plan: loaded.plan,
        host,
        groups,
    })
}

fn read_text(path: &Path) -> Result<String, Error> {
    let io = |error| Error::Io {
        path: path.into(),
        error,
    };
    if std::fs::metadata(path).map_err(io)?.len() > XML_LIMIT {
        return Err(Error::Invalid {
            path: path.into(),
            reason: "program exceeds 32 MiB".into(),
        });
    }
    #[cfg(feature = "library-access")]
    {
        let bytes = std::fs::read(path).map_err(io)?;
        bank::program_text(&bytes).map_err(|e| Error::Invalid {
            path: path.into(),
            reason: e.to_string(),
        })
    }
    #[cfg(not(feature = "library-access"))]
    std::fs::read_to_string(path).map_err(io)
}

fn describe(path: &Path, e: Translate) -> Error {
    match e {
        Translate::Xml(error) => Error::Xml {
            path: path.into(),
            error,
        },
        Translate::Invalid(reason) => Error::Invalid {
            path: path.into(),
            reason,
        },
    }
}

/// Drop zones whose sample could not be read (reported), then fit the rest to
/// their decoded audio and lower. `decoded[i]` belongs to asset `i`.
fn assemble(
    mut instrument: ir::Instrument,
    locations: Vec<String>,
    decoded: Vec<Result<sampler_kontakt::Decoded, String>>,
    options: &sampler_kontakt::Options,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    for (location, result) in locations.iter().zip(&decoded) {
        if let Err(reason) = result {
            instrument.unsupported.push(ir::Unsupported {
                location: location.clone(),
                feature: "unreadable sample, zone dropped".into(),
                value: reason.clone(),
                reason: ir::Reason::InvalidValue,
            });
        }
    }
    let kept = instrument.retain_zones(|z| decoded[z.asset.0].is_ok());
    let mut pcm = Vec::with_capacity(kept.len());
    let mut decoded: Vec<_> = decoded.into_iter().map(Some).collect();
    for &asset in &kept {
        let d = decoded[asset].take().unwrap().unwrap();
        pcm.push(sampler_core::Pcm::new(d.rate, d.frames.into_boxed_slice())?);
    }
    let labels: Vec<String> = kept.iter().map(|&a| locations[a].clone()).collect();
    Ok(sampler_kontakt::finish(instrument, pcm, labels, options)?)
}

#[cfg(all(test, feature = "library-access"))]
mod survey {
    use sampler_ir as ir;
    use std::collections::BTreeMap;

    /// Every program under `KONTRA_UVI_LIBRARIES`: `.ufs` banks, or clear
    /// `.uvip` files (e.g. decoded copies), as `(name, xml)`.
    pub(crate) fn programs(mut each: impl FnMut(&str, &str)) {
        let root = std::env::var("KONTRA_UVI_LIBRARIES").unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|e| e == "ufs" || e == "uvip") {
                    files.push(p)
                }
            }
        }
        files.sort();
        for f in files {
            if f.extension().is_some_and(|e| e == "uvip") {
                each(
                    &f.display().to_string(),
                    &std::fs::read_to_string(&f).unwrap(),
                );
                continue;
            }
            let Ok(bank) = crate::Bank::open(&f) else {
                continue;
            };
            for program in bank.programs() {
                if let Ok((text, _)) = bank.program(&program) {
                    each(&format!("{} {program}", f.display()), &text);
                }
            }
        }
    }

    /// Plays every program at a key its zones cover through its Lua script and
    /// prints one `UC` line per program: sounds, silent or failed, with counts
    /// only. Shard with `KONTRA_SHARD=i/n`.
    #[test]
    #[ignore]
    #[cfg(feature = "library-access")]
    fn census_scripted_render() {
        let (shard, shards): (usize, usize) = std::env::var("KONTRA_SHARD")
            .ok()
            .and_then(|v| {
                let (a, b) = v.split_once('/')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .unwrap_or((0, 1));
        let root = std::env::var("KONTRA_UVI_LIBRARIES").unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|e| e == "ufs") {
                    files.push(p)
                }
            }
        }
        files.sort();
        let mut index = 0;
        for f in files {
            let Ok(bank) = crate::Bank::open(&f) else { continue };
            for program in bank.programs() {
                index += 1;
                if index % shards != shard {
                    continue;
                }
                let name = format!("{}::{program}", f.display());
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    census_one(&bank, &program)
                }));
                match outcome {
                    Ok(line) => println!("UC {line} {name}"),
                    Err(_) => println!("UC panic - {name}"),
                }
            }
        }
    }

    #[cfg(feature = "library-access")]
    fn census_one(bank: &crate::Bank, program: &str) -> String {
        use sampler_core::Limits;
        let Ok((text, _)) = bank.program(program) else { return "open-fail -".into() };
        let Ok((ir, _)) = crate::translate_bank(&text) else { return "translate-fail -".into() };
        if ir.zones.is_empty() {
            return "no-zones -".into();
        }
        // Keys a played note could mean: the median zone, middle C inside the
        // covered range, and the widest zone's middle.
        let mut mids: Vec<u8> = ir
            .zones
            .iter()
            .map(|z| ((u16::from(z.keys.low) + u16::from(z.keys.high)) / 2) as u8)
            .collect();
        mids.sort_unstable();
        let (lo, hi) = (
            ir.zones.iter().map(|z| z.keys.low).min().unwrap(),
            ir.zones.iter().map(|z| z.keys.high).max().unwrap(),
        );
        let widest = ir.zones.iter().max_by_key(|z| z.keys.high - z.keys.low).unwrap();
        let mut candidates = vec![
            mids[mids.len() / 2],
            60.clamp(lo, hi),
            ((u16::from(widest.keys.low) + u16::from(widest.keys.high)) / 2) as u8,
        ];
        candidates.dedup();
        let mut last = String::new();
        for key in candidates {
            last = census_play(bank, program, key);
            if last.starts_with("sounds") {
                break;
            }
        }
        last
    }

    #[cfg(feature = "library-access")]
    fn census_play(bank: &crate::Bank, program: &str, key: u8) -> String {
        use sampler_core::Limits;
        let options = sampler_kontakt::Options {
            keys: key.saturating_sub(12)..=key.saturating_add(12).min(127),
            ..Default::default()
        };
        let program = match crate::load_program_scripted_with_options(bank, program, &options) {
            Ok(p) => p,
            Err(e) => return format!("load-fail {}", e.to_string().chars().take(60).collect::<String>().replace(' ', "_")),
        };
        let errors = program
            .instrument
            .unsupported
            .iter()
            .filter(|u| u.feature == "lua error")
            .count();
        let first = program
            .instrument
            .unsupported
            .iter()
            .find(|u| u.feature == "lua error")
            .map(|u| u.value.chars().take(70).collect::<String>().replace(' ', "_"))
            .unwrap_or_else(|| "-".into());
        let scripted = program.host.handles_notes();
        let limits = Limits {
            notes: 64, channels: 16, performances: 1, expressions: 64, families: 64,
            decisions: 256, voices: 512, commands: 256, behaviors: 16,
            behavior_fuel: 1 << 20, behavior_cells: 0, note_cells: 0,
        };
        let Ok(mut player) = crate::scripted::Player::new(program, limits, 48_000) else {
            return "player-fail -".into();
        };
        let mut peak = 0.0f32;
        let mut buf = [[0.0f32; 2]; 256];
        let mut run = |player: &mut crate::scripted::Player, blocks: usize| {
            for _ in 0..blocks {
                player.render(&mut buf)?;
                peak = buf.iter().flatten().fold(peak, |p, x| p.max(x.abs()));
            }
            Ok::<(), sampler_core::Error>(())
        };
        let result = player
            .note_on(key, 100.0 / 127.0)
            .and_then(|_| run(&mut player, 188))
            .and_then(|_| player.note_off(key))
            .and_then(|_| run(&mut player, 188));
        let (ok, failure) = (result.is_ok(), result.err());
        let state = match (ok, peak > 1e-4) {
            (false, _) => "render-err",
            (true, true) => "sounds",
            (true, false) => "silent",
        };
        let first = match failure {
            Some(e) => e.to_string().chars().take(60).collect::<String>().replace(' ', "_"),
            None => first,
        };
        format!("{state} peak={peak:.3} key={key} lua={} errs={errors} {first}", u8::from(scripted))
    }

    /// Which modulation translates and what stays reported, across the
    /// installed banks: `KONTRA_UVI_LIBRARIES=... cargo test -p sampler-uvi
    /// --lib survey_modulation -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn survey_modulation() {
        let (mut total, mut routed) = (0, 0);
        let mut routes = BTreeMap::<String, (usize, usize, String)>::new();
        let mut gaps = BTreeMap::<String, (usize, usize)>::new();
        programs(|name, text| {
            total += 1;
            let ir = match crate::translate_bank(text) {
                Ok((ir, _)) => ir,
                Err(e) => return println!("{name}: {e:?}"),
            };
            routed += usize::from(ir.zones.iter().any(|z| !z.routes.is_empty()));
            let mut seen = std::collections::HashSet::new();
            for zone in &ir.zones {
                for r in &zone.routes {
                    let route = &ir.routes[r.0];
                    let source = match &ir.modulators[route.source.0].source {
                        ir::ModulationSource::Lfo(l) => format!("Lfo {:?}", l.shape),
                        ir::ModulationSource::Envelope(_) => "Envelope".into(),
                        other => format!("{other:?}"),
                    };
                    let key = format!("{source} -> {:?}", route.target);
                    let entry = routes
                        .entry(key.clone())
                        .or_insert_with(|| (0, 0, name.to_owned()));
                    entry.1 += 1;
                    if seen.insert(key) {
                        entry.0 += 1;
                    }
                }
            }
            let mut seen = std::collections::HashSet::new();
            for u in &ir.unsupported {
                let value = u.value.split_once(": ").map_or(u.value.as_str(), |v| v.1);
                let value = if u.feature == "module"
                    || u.feature.contains("modulat")
                    || u.feature.starts_with("LFO")
                {
                    value
                } else {
                    ""
                };
                let key = format!("{:?} {} {}", u.reason, u.feature, value);
                let entry = gaps.entry(key.clone()).or_default();
                entry.1 += 1;
                if seen.insert(key) {
                    entry.0 += 1;
                }
            }
        });
        println!(
            "{routed}/{total} programs have modulation routes\n\nroutes (programs, zone bindings):"
        );
        for (k, (p, n, example)) in &routes {
            println!("{p:5} {n:9} {k}  e.g. {example}");
        }
        println!("\nreported (programs, entries):");
        for (k, (p, n)) in &gaps {
            println!("{p:5} {n:9} {k}");
        }
    }
}
