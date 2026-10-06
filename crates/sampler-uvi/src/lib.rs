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
mod audio;
#[cfg(feature = "library-access")]
mod bank;
#[cfg(feature = "library-access")]
mod crypto;
mod modulation;
#[cfg(feature = "library-access")]
mod ufs;

#[cfg(feature = "library-access")]
pub use bank::Bank;

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
    let io = |error| Error::Io {
        path: path.into(),
        error,
    };
    let size = std::fs::metadata(path).map_err(io)?.len();
    if size > XML_LIMIT {
        return Err(Error::Invalid {
            path: path.into(),
            reason: "program exceeds 32 MiB".into(),
        });
    }
    let text = std::fs::read_to_string(path).map_err(io)?;
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
    let options = ParsingOptions {
        allow_dtd: false,
        // Augmented Orchestra programs reach ~97k elements (~200k nodes with
        // their whitespace); XML_LIMIT bounds the text either way.
        nodes_limit: 1_000_000,
        ..Default::default()
    };
    let doc = Document::parse_with_options(text, options).map_err(Translate::Xml)?;
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
        used: Vec::new(),
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
    Ok((out.ir, out.locations))
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
    /// Nodes whose meaning was carried into the IR.
    used: Vec<roxmltree::NodeId>,
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
                state: Vec::new(),
                requires: Vec::new(),
            });
        }
        self.scope_connections(program)?;
        for layer in program.descendants().filter(|n| n.has_tag_name("Layer")) {
            if number(layer, "Mute", 0.0)? != 0.0 {
                continue;
            }
            self.scope_connections(layer)?;
            let pan = number(layer, "Pan", 0.0)?;
            self.ir.groups.push(ir::Group {
                name: layer.attribute("Name").unwrap_or_default().into(),
                gain: ir::Gain::Linear(gain * number(layer, "Gain", 1.0)?),
                pan: ir::Pan {
                    position: pan.clamp(-1.0, 1.0),
                    law: ir::PanLaw::Balance,
                },
                ..Default::default()
            });
            if pan != 0.0 {
                self.unsupported(
                    &path(layer),
                    "cos² balance pan law (linear balance used)",
                    pan,
                );
            }
            let group = ir::GroupRef(self.ir.groups.len() - 1);
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
        for player in keygroup
            .descendants()
            .filter(|n| n.has_tag_name("SamplePlayer"))
        {
            if number(player, "Bypass", 0.0)? != 0.0 {
                continue;
            }
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

/// Load a clear `.uvip` program (loose WAV/FLAC/AIFF samples) as a plan at
/// `rate`. For an encrypted bank program use [`load_program`]. Lua scripts have
/// no frontend yet and are reported in the instrument, not run.
pub fn load(path: &Path, rate: u32) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    let (instrument, locations) = translate_with(
        &read_text(path)?,
        Source::Disk(path.parent().unwrap_or(Path::new(".")).into()),
    )
    .map_err(|e| describe(path, e))?;
    let decoded = locations
        .iter()
        .map(|location| {
            std::fs::read(location)
                .map_err(|e| e.to_string())
                .and_then(|bytes| audio::decode(&[bytes]).map(|(d, _)| d))
        })
        .collect();
    assemble(instrument, locations, decoded, rate)
}

/// Load a program inside an installed UVI bank. `bank` is an open [`Bank`]; `program`
/// is a member path from [`Bank::programs`]. Samples are read from the bank.
#[cfg(feature = "library-access")]
pub fn load_program(
    bank: &Bank,
    program: &str,
    rate: u32,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    let (text, program_path) = bank.program(program)?;
    let (instrument, locations) =
        translate_bank(&text).map_err(|e| describe(Path::new(program), e))?;
    let decoded = locations
        .iter()
        .map(|authored| {
            bank.resource(&program_path, authored)
                .and_then(|parts| audio::decode(&parts).map(|(d, _)| d))
        })
        .collect();
    assemble(instrument, locations, decoded, rate)
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
    rate: u32,
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
    for &asset in &kept {
        let d = decoded[asset].as_ref().unwrap();
        pcm.push(sampler_core::Pcm::new(
            d.rate,
            d.frames.clone().into_boxed_slice(),
        )?);
    }
    let labels: Vec<String> = kept.iter().map(|&a| locations[a].clone()).collect();
    let options = sampler_kontakt::Options {
        rate,
        scripts: true,
        ..Default::default()
    };
    Ok(sampler_kontakt::finish(instrument, pcm, labels, &options)?)
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
