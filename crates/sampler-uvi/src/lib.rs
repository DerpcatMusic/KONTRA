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
#[cfg(not(feature = "library-access"))]
mod no_access;
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
const STRUCTURAL: [&str; 17] = [
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
        nodes_limit: 200_000,
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
        used: Vec::new(),
    };
    out.program(program).map_err(Translate::Invalid)?;
    // Whatever was neither structure nor consumed is reported once per node.
    for node in program.descendants().filter(|n| n.is_element()) {
        let kind = node.tag_name().name();
        if !STRUCTURAL.contains(&kind)
            && !out.used.contains(&node.id())
            && !node.ancestors().skip(1).any(|a| out.used.contains(&a.id()))
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
        for layer in program.descendants().filter(|n| n.has_tag_name("Layer")) {
            if number(layer, "Mute", 0.0)? != 0.0 {
                continue;
            }
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
        // The amplitude envelope: a Gain connection from a DAHDSR or AnalogADSR.
        let mut amplitude = None;
        let connections = keygroup
            .children()
            .filter(|n| n.has_tag_name("Connections"))
            .flat_map(|c| c.children());
        for connection in connections.filter(|n| n.has_tag_name("SignalConnection")) {
            let (source, destination) = (
                connection.attribute("Source").unwrap_or_default(),
                connection.attribute("Destination").unwrap_or_default(),
            );
            let plain = number(connection, "Ratio", 1.0)? == 1.0
                && number(connection, "Bypass", 0.0)? == 0.0
                && number(connection, "Inverted", 0.0)? == 0.0
                && connection
                    .attribute("Mapper")
                    .unwrap_or_default()
                    .is_empty()
                && connection.children().all(|c| !c.is_element());
            match self.envelope_source(keygroup, source) {
                Some(envelope) if destination == "Gain" && plain && amplitude.is_none() => {
                    amplitude = Some(self.envelope(envelope)?);
                    self.used.push(connection.id());
                }
                _ => self.unsupported(
                    &path(connection),
                    "modulation",
                    format!("{source} -> {destination}"),
                ),
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
            for (name, default) in [("Pitch", 0.0), ("SampleStart", 0.0)] {
                let value = number(player, name, default)?;
                if value != default {
                    self.unsupported(&at, name, value);
                }
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
                    number(player, "CoarseTune", 0.0)? + number(player, "FineTune", 0.0)? / 100.0,
                ),
                gain: ir::Gain::Linear(gain * number(player, "Gain", 1.0)?),
                velocity,
                pan: ir::Pan {
                    position: (pan + player_pan).clamp(-1.0, 1.0),
                    law: ir::PanLaw::Balance,
                },
                playback,
                amplitude,
                ..ir::Zone::new(asset)
            });
        }
        Ok(())
    }

    /// The DAHDSR or AnalogADSR node a connection source names: `$Program/X`
    /// and `$Layer/X` address those scopes, a bare name the nearest scope.
    fn envelope_source<'a>(&self, keygroup: Node<'a, 'a>, source: &str) -> Option<Node<'a, 'a>> {
        let (scopes, name): (Vec<Node>, &str) = match source.split_once('/') {
            Some(("$Program", name)) => (
                keygroup
                    .ancestors()
                    .filter(|n| n.has_tag_name("Program"))
                    .collect(),
                name,
            ),
            Some(("$Layer", name)) => (
                keygroup
                    .ancestors()
                    .filter(|n| n.has_tag_name("Layer"))
                    .collect(),
                name,
            ),
            Some(_) => return None,
            None => (
                keygroup.ancestors().filter(|n| n.is_element()).collect(),
                source,
            ),
        };
        scopes.into_iter().find_map(|scope| {
            scope
                .children()
                .filter(|n| n.has_tag_name("ControlSignalSources"))
                .flat_map(|s| s.children())
                .find(|n| {
                    matches!(n.tag_name().name(), "DAHDSR" | "AnalogADSR")
                        && n.attribute("Name") == Some(name)
                })
        })
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
            release: seconds("ReleaseTime", 10.0)?,
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
        // v1 measured law: velocity^(1 - log2(1 - sensitivity)); 1 gates at 127.
        let sensitivity = number(node, "VelocitySens", 0.75)?.clamp(-1.0, 1.0);
        let velocity = if sensitivity < 1.0 {
            ir::VelocityResponse::Power(1.0 - (1.0 - sensitivity).log2())
        } else {
            self.unsupported(&at, "VelocitySens", sensitivity);
            ir::VelocityResponse::Linear
        };
        let amount = number(node, "VelocityAmount", 0.0)?;
        if amount != 0.0 {
            self.unsupported(&at, "VelocityAmount", amount);
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
        return load_program(&bank, &member, rate);
    }
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
                .map_err(|e| e.to_string())
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
