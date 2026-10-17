//! UVI Falcon / Workstation programs (`.uvip` XML) to the semantic IR.
//!
//! Attribute names and units follow the v1 UVI reader on
//! `codex/uvi-latest-integration` (`src/uvi/program.rs`, `playback.rs`,
//! `modulation.rs`). Clear `.uvip` with loose samples load through [`load`].
//! With the `library-access` feature, installed UVI banks open through [`Bank`]
//! using the native UFS and PasswordV2 implementation. Recovered bank access
//! state stays only in memory; it is never logged, printed or returned in an error. Every
//! module this translator does not model is listed in `Instrument::unsupported`.

#[cfg(feature = "library-access")]
mod access;
mod access_error;
pub mod arpeggiator;
mod audio;
mod sample_metadata;
mod residency_policy;
#[cfg(feature = "library-access")]
mod bank;
#[cfg(feature = "scan")]
mod coverage;
#[cfg(feature = "library-access")]
mod crypto;
mod engine_parameters;
pub mod meters;
mod fx;
mod inserts;
pub use inserts::InsertNode;
mod modulation;
#[cfg(not(feature = "library-access"))]
mod no_access;
pub mod parity;
mod resources;
pub mod script;
pub use parity::parity_reports;
pub use resources::{ResourceError, Resources};
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
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Increment when native protected-library decoding changes, for cache invalidation.
pub const LIBRARY_ACCESS_REVISION: u32 = 1;
/// Whether this build can read protected banks, including unified dependency features.
pub const LIBRARY_ACCESS_ENABLED: bool = cfg!(feature = "library-access");

const XML_LIMIT: u64 = 32 << 20;

/// Container nodes whose meaning is their children.
const STRUCTURAL: [&str; 22] = [
    "Auxs",
    "AuxEffect",
    "Inserts",
    "BusRouters",
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

#[cfg(all(test, feature = "library-access"))]
#[test]
fn bank_volume_sample_reaches_the_bank_resource_resolver() {
    let xml = r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators>
        <SamplePlayer SamplePath="$Authored.ufs/Samples/note.wav"/>
        </Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
    let (bank, locations) = translate_bank(xml).unwrap();
    assert_eq!(
        bank.zones.len(),
        1,
        "bank authority resolves its own volume"
    );
    assert_eq!(locations, ["$Authored.ufs/Samples/note.wav"]);
    let disk = translate(xml, Path::new(".")).unwrap();
    assert!(
        disk.instrument.zones.iter().all(|z| z.asset.is_none()) && disk.instrument.assets.is_empty(),
        "a loose file has no bank authority; its target remains unbound"
    );
}

fn translate_with(text: &str, source: Source) -> Result<(ir::Instrument, Vec<String>), Translate> {
    translate_full(text, source).map(|(instrument, locations, ..)| (instrument, locations))
}

type FullTranslation = (ir::Instrument, Vec<String>, Vec<OscGroup>, Vec<InsertNode>);

/// [`translate_with`], plus the IR group of each (layer, oscillator) of a
/// scripted program (empty when the program has no script).
fn translate_full(text: &str, source: Source) -> Result<FullTranslation, Translate> {
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
        used: HashSet::new(),
        #[cfg(feature = "scan")]
        dropped_connections: Default::default(),
        #[cfg(feature = "scan")]
        visited_connections: Default::default(),
        #[cfg(feature = "scan")]
        native_player_ids: program
            .descendants()
            .filter(|n| n.has_tag_name("SamplePlayer"))
            .enumerate()
            .map(|(i, n)| (n.id(), i))
            .collect(),
        osc_groups: Vec::new(),
        insert_nodes: Vec::new(),
        split: None,
    };
    out.program(program).map_err(Translate::Invalid)?;
    // Every authored user contributes, including a bypassed user sharing an
    // otherwise audible source. Do not instantiate unused/missing file assets.
    for player in program.descendants().filter(|n|n.has_tag_name("SamplePlayer")) {
        let required=residency_policy::sample(player).map_err(Translate::Invalid)?;
        let Some(path)=player.attribute("SamplePath") else {continue;};
        let relative=path.replace('\\',"/");
        let location=match &out.source {Source::Bank=>relative,Source::Disk(folder)=>folder.join(relative).to_string_lossy().into_owned()};
        if let Some(asset)=out.assets.get(&location) {out.ir.assets[asset.0].residency=out.ir.assets[asset.0].residency.merge(required);}
    }
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
    engine_parameters::register(&mut out.ir, &doc, &out.insert_nodes);
    out.ir.register_compressor_controls();
    #[cfg(feature = "scan")]
    {
        out.ir.dsp_slots = Some(coverage::slots(&out, program));
        out.ir.native_family = Some(coverage::native_family(program));
    }
    out.ir
        .validate()
        .map_err(|e| Translate::Invalid(e.to_string()))?;
    if !out.ir.behaviors.is_empty() {
        out.ir.uvi_script_sources = Some(ir::UviScriptSources {
            xml: text.into(),
            files: Default::default(),
        });
    }
    Ok((out.ir, out.locations, out.osc_groups, out.insert_nodes))
}

/// A layer's or keygroup's own `BusRouter`s.
fn routers<'a>(node: Node<'a, 'a>) -> impl Iterator<Item = Node<'a, 'a>> {
    node.children()
        .filter(|n| n.has_tag_name("BusRouters"))
        .flat_map(|r| r.children().filter(|c| c.has_tag_name("BusRouter")))
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

thread_local! {
    /// The (node, attribute) pairs the translator reads, while
    /// [`translate_initialized`] records them.
    static READS: std::cell::RefCell<Option<HashSet<(usize, String)>>> = const { std::cell::RefCell::new(None) };
}

fn record_read(node: Node, name: &str) {
    READS.with(|r| {
        if let Some(reads) = r.borrow_mut().as_mut() {
            reads.insert((node.id().get_usize(), name.to_owned()));
        }
    });
}

fn number(node: Node, name: &str, default: f64) -> Result<f64, String> {
    record_read(node, name);
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
#[derive(Clone)]
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
    used: HashSet<roxmltree::NodeId>,
    #[cfg(feature = "scan")]
    dropped_connections: std::collections::HashSet<roxmltree::NodeId>,
    /// Authored connections reached by translation in an instantiated scope.
    #[cfg(feature = "scan")]
    visited_connections: std::collections::HashSet<roxmltree::NodeId>,
    #[cfg(feature = "scan")]
    native_player_ids: HashMap<roxmltree::NodeId, usize>,
    osc_groups: Vec<OscGroup>,
    /// Where each insert element's processors sit, for script writes.
    insert_nodes: Vec<InsertNode>,
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
    pub keygroup: usize,
    pub oscillator: usize,
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
    /// Gain, pitch and pan modulation are linear in the mix, so each keygroup
    /// below the scope takes the connection itself (`Self::keygroup`).
    fn scope_connections<'a>(&mut self, scope: Node<'a, 'a>) -> Vec<Node<'a, 'a>> {
        connections(scope).collect()
    }

    fn program(&mut self, program: Node) -> Result<(), String> {
        let gain = number(program, "Gain", 1.0)?;
        for processor in program
            .descendants()
            .filter(|n| n.has_tag_name("ScriptProcessor"))
        {
            self.used.insert(processor.id());
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
        let program_scope = self.scope_connections(program);
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
            let bus = ir::BusRef(self.ir.buses.len());
            let live = number(aux, "Bypass", 0.0)? == 0.0;
            let (mut processors, mut placed) = if live || scripted {
                self.inserts(aux, false, None)?
            } else {
                (Vec::new(), Vec::new())
            };
            let mut gain = ir::Gain::Linear(number(aux, "Gain", 1.0)?);
            if scripted {
                // The aux's inserts sit in one Mix (its Bypass); its Gain is
                // the last stage, a control a script can move.
                if !processors.is_empty() {
                    processors.insert(
                        0,
                        ir::Processor::Mix {
                            count: processors.len() as u16,
                            address: ir::SlotAddress {
                                group: -1,
                                slot: -1,
                                generic: i32::try_from(aux.id().get_usize()).unwrap_or(-1),
                            },
                            dry: 0.0,
                            wet: 1.0,
                            bypass: !live,
                        },
                    );
                    for entry in &mut placed {
                        entry.1 += 1;
                    }
                }
                placed.push((aux.id().get_usize(), processors.len(), 1));
                processors.push(ir::Processor::Gain(std::mem::replace(
                    &mut gain,
                    ir::Gain::UNITY,
                )));
            }
            let chain = (!placed.is_empty()).then(|| {
                self.ir.chains.push(ir::Chain {
                    scope: ir::Scope::Bus(bus),
                    pre_amplitude: processors,
                    post_amplitude: Vec::new(),
                });
                let chain = ir::ChainRef(self.ir.chains.len() - 1);
                self.place(chain, placed);
                chain
            });
            self.ir.buses.push(ir::Bus {
                name: name.clone(),
                chain,
                sends: Vec::new(),
                output: ir::Output::Master,
                gain,
            });
            auxes.push((name, bus));
        }
        let program_output = self.insert_bus(program, ir::Output::Master)?;
        self.ir.input_bus = if let ir::Output::Bus(bus) = program_output {
            Some(bus)
        } else {
            None
        };
        for (_, bus) in &auxes {
            self.ir.buses[bus.0].output = program_output;
        }
        for (ordinal, layer) in program
            .descendants()
            .filter(|n| n.has_tag_name("Layer"))
            .enumerate()
        {
            if number(layer, "Mute", 0.0)? != 0.0 {
                continue;
            }
            let mut scoped = program_scope.clone();
            scoped.extend(self.scope_connections(layer));
            let output = self.insert_bus(layer, program_output)?;
            let pan = number(layer, "Pan", 0.0)?;
            let mut base = ir::Group {
                name: layer.attribute("Name").unwrap_or_default().into(),
                gain: ir::Gain::Linear(gain * number(layer, "Gain", 1.0)?),
                pan: ir::Pan {
                    position: pan.clamp(-1.0, 1.0),
                    law: ir::PanLaw::Balance,
                },
                output,
                ..Default::default()
            };
            // The layer's sends to aux buses, either side of its fader.
            for router in routers(layer) {
                if number(router, "Bypass", 0.0)? != 0.0 {
                    // A bypassed send carries nothing.
                    self.used.insert(router.id());
                    continue;
                }
                let Some(bus) = router
                    .attribute("Destination")
                    .and_then(|d| d.rsplit('/').next())
                    .and_then(|to| auxes.iter().find(|(n, _)| n == to))
                    .map(|a| a.1)
                else {
                    continue;
                };
                self.used.insert(router.id());
                base.sends.push(ir::GroupSend {
                    to: bus,
                    gain: ir::Gain::Linear(number(router, "Gain", 1.0)?),
                    pre_fader: number(router, "PreFader", 0.0)? != 0.0,
                });
            }
            // Scripts address individual keygroups/oscillators, including
            // single-oscillator keygroups; no two original nodes may alias.
            self.split = scripted.then(|| (ordinal + 1, base.clone()));
            if self.split.is_none() {
                self.ir.groups.push(base);
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
            // (oscIndex): a scripted layer is split per oscillator above, and
            // without one every oscillator plays, as in UVI.
            let keys = (midi(layer, "LowKey", 0)?, midi(layer, "HighKey", 127)?);
            for keygroup in layer.descendants().filter(|n| n.has_tag_name("Keygroup")) {
                self.keygroup(keygroup, group, keys, &scoped)?;
            }
        }
        Ok(())
    }

    fn keygroup(
        &mut self,
        keygroup: Node,
        group: ir::GroupRef,
        layer_keys: (u8, u8),
        scoped: &[Node],
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
        // The amplitude envelope: a plain Gain connection from a DAHDSR. Every other
        // connection is a route or a static factor. AnalogADSR is not modeled (below).
        let mut amplitude = None;
        let mut shared = modulation::Modulation::default();
        for connection in scoped {
            self.connect_scoped(*connection, &mut shared)?;
        }
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
            .filter(|n| n.tag_name().name() == "DAHDSR")
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
        // A keygroup's own aux sends: a bypassed or silent one carries nothing.
        for router in routers(keygroup) {
            if number(router, "Bypass", 0.0)? == 0.0 && number(router, "Gain", 1.0)? != 0.0 {
                // ponytail: an audible keygroup send taps the keygroup ahead of the
                // layer's fader and inserts; IR sends leave groups, after the layer gain.
                self.unsupported(&path(router), "keygroup aux send", number(router, "Gain", 1.0)?);
            }
            self.used.insert(router.id());
        }
        let gain = number(keygroup, "Gain", 1.0)?;
        let pan = number(keygroup, "Pan", 0.0)?;
        let (processors, placed) = self.inserts(
            keygroup,
            true,
            Some(((keys.0 as u16 + keys.1 as u16) / 2) as u8),
        )?;
        let chain = (!placed.is_empty()).then(|| {
            self.ir.chains.push(ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: processors,
                post_amplitude: Vec::new(),
            });
            let chain = ir::ChainRef(self.ir.chains.len() - 1);
            self.place(chain, placed);
            chain
        });
        if let Some(chain) = chain {
            for insert in keygroup
                .children()
                .filter(|n| n.has_tag_name("Inserts"))
                .flat_map(|n| n.descendants())
                .filter(|n| n.has_tag_name("OnePole"))
            {
                if let Some(placed) = self
                    .insert_nodes
                    .iter()
                    .find(|p| p.node == insert.id().get_usize() && p.count > 0)
                    .copied()
                {
                    for connection in connections(insert) {
                        self.connect_frequency(connection, chain, placed.first, &mut shared)?;
                    }
                }
            }
        }
        for (oscillator, player) in keygroup
            .descendants()
            .filter(|n| n.has_tag_name("SamplePlayer"))
            .enumerate()
        {
            let bypassed = number(player, "Bypass", 0.0)? != 0.0;
            if bypassed && self.split.is_none() {
                continue;
            }
            let group = self.oscillator_group(
                group,
                oscillator as u32 + 1,
                keygroup.id().get_usize(),
                player.id().get_usize(),
            );
            if bypassed {
                self.ir.groups[group.0].muted = true;
            }
            let at = path(player);
            let asset = player.attribute("SamplePath").filter(|p| !p.is_empty())
                .and_then(|sample| self.asset(&at, sample));
            let start = number(player, "SampleStart", 0.0)?;
            if start != 0.0 {
                self.unsupported(&at, "SampleStart", start);
            }
            let tracking = number(player, "NoteTracking", 1.0)?;
            let root = midi(player, "BaseNote", 60)?;
            // The runtime tracks the key at 100 cents per key or not at all, so
            // another scale is tracked normally and detuned by the scale's
            // difference at the middle of the zone's keys.
            let mut scaling_tune = 0.0;
            let pitch = match tracking {
                1.0 => ir::KeyTracking::Tracked { root },
                0.0 => ir::KeyTracking::Fixed,
                t => {
                    let middle = (f64::from(keys.0) + f64::from(keys.1)) / 2.0;
                    scaling_tune = (middle - f64::from(root)) * (t - 1.0);
                    self.unsupported(
                        &at,
                        "NoteTracking approximated by a detune at the middle of the zone",
                        t,
                    );
                    ir::KeyTracking::Tracked { root }
                }
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
            #[cfg(feature = "scan")]
            {
                self.ir
                    .source_indices
                    .zones
                    .resize(self.native_player_ids.len(), None);
                self.ir.source_indices.zones[self.native_player_ids[&player.id()]] =
                    Some(ir::ZoneRef(self.ir.zones.len()));
            }
            let zone = ir::ZoneRef(self.ir.zones.len());
            let node = u32::try_from(player.id().get_usize()).map_err(|_| "sample player identity exceeds supported range".to_string())?;
            self.ir.source_indices.sample_targets.push(ir::SourceSampleTarget { node, zones: vec![zone] });
            if !player.children().any(|n|n.has_tag_name("PlaybackOptions")) {
                self.ir.source_indices.file_playback_zones.push(zone);
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
                        + modulation.pitch
                        + scaling_tune,
                ),
                gain: ir::Gain::Linear(gain * number(player, "Gain", 1.0)? * modulation.gain),
                velocity,
                pan: ir::Pan {
                    position: (pan + player_pan + modulation.pan).clamp(-1.0, 1.0),
                    law: ir::PanLaw::Balance,
                },
                playback,
                chain,
                amplitude,
                routes: modulation.routes,
                ..asset.map_or_else(ir::Zone::dormant, ir::Zone::new)
            });
        }
        Ok(())
    }

    /// The group a zone of oscillator `osc` goes to: the layer's own, or in a
    /// scripted program one group per original oscillator so that scoped
    /// parameter writes and `playNote` selection share the same identity.
    fn oscillator_group(
        &mut self,
        layer_group: ir::GroupRef,
        osc: u32,
        keygroup: usize,
        oscillator: usize,
    ) -> ir::GroupRef {
        let Some((layer, base)) = &self.split else {
            return layer_group;
        };
        let layer = *layer as u32;
        if let Some(found) = self.osc_groups.iter().find(|g| g.oscillator == oscillator) {
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
            keygroup,
            oscillator,
        });
        ir::GroupRef(index)
    }

    fn envelope(&mut self, node: Node) -> Result<(ir::ModulatorRef, ir::VelocityResponse), String> {
        if let Some(&found) = self.envelopes.get(&node.id()) {
            return Ok(found);
        }
        self.used.insert(node.id());
        let at = path(node);
        let kind = node.tag_name().name();
        let seconds =
            |name, max| number(node, name, 0.0).map(|t| ir::Time::Seconds(t.clamp(0.0, max)));
        let ahd = kind == "AHD";
        let mut envelope = ir::Envelope {
            delay: if kind == "DAHDSR" {
                seconds("DelayTime", 10.0)?
            } else {
                ir::Time::ZERO
            },
            attack: seconds("AttackTime", 10.0)?,
            hold: if kind != "AnalogADSR" {
                seconds("HoldTime", 10.0)?
            } else {
                ir::Time::ZERO
            },
            decay: seconds("DecayTime", 30.0)?,
            // AHD has no sustain stage: it falls to zero after the decay.
            sustain: if ahd {
                0.0
            } else {
                number(node, "SustainLevel", 1.0)?.clamp(0.0, 1.0)
            },
            release: number(node, "ReleaseTime", 0.05)
                .map(|t| ir::Time::Seconds(t.clamp(0.0, 10.0)))?,
            ..Default::default()
        };
        if ahd {
            self.ahd_stages(node, &mut envelope, &at)?;
        }
        if kind != "AnalogADSR" {
            // The native envelope curve `expm1(k·t)/expm1(k)` is exactly v1's
            // `envelope_curve` (src/uvi/modulation.rs) at k = 2·ln((1+c)/(1-c)),
            // so a DAHDSR's per-stage curve translates without approximation.
            envelope.attack_shape = curve(number(node, "AttackCurve", 0.0)?);
            envelope.decay_shape = curve(number(node, "DecayCurve", 0.0)?);
            envelope.release_shape = curve(number(node, "ReleaseCurve", 0.0)?);
        }
        // AnalogADSR is never built here: native 0x14113f0a0 runs a stateful exponential
        // recurrence (attack toward a target, decay/release multiplying the level by
        // 1-coefficient, coefficient from pow helper 0x1419ef640; audit uvi-core-analog-law)
        // whose field units are unproved, so it is reported as unsupported, not linear.
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
        if matches!(&self.source, Source::Disk(_))
            && (relative.starts_with('$') || relative.contains(".ufs/"))
        {
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
            residency: Default::default(),
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
/// Protected programs use the native reader behind `library-access`.
pub fn load(path: &Path, rate: u32) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    assemble_translated(translate_path(path)?, rate)
}

/// A translated program whose samples are not decoded yet.
pub struct Translated {
    pub instrument: ir::Instrument,
    pub locations: Vec<String>,
    /// The bank and program path the samples come from; loose files have none.
    bank: Option<(std::sync::Arc<Bank>, String)>,
    /// The folder of a loose program, which its relative resources are read from.
    dir: Option<PathBuf>,
    /// Where each (layer, oscillator) of the program plays, for its scripts.
    pub groups: Vec<OscGroup>,
    /// Where each insert element sits in the IR chains, for parameter bindings.
    pub inserts: Vec<InsertNode>,
    /// The program's XML and the bank's Lua members, for its scripts.
    text: std::sync::Arc<str>,
    lua: script::Scripts,
}

/// A program's scripts loaded on a script thread for a host that owns the runtime.
pub struct AttachedScript {
    /// Every bank Lua occurrence and its admission result. Unused unavailable
    /// modules remain optional; these are collection outcomes, not executions.
    pub script_members: Vec<script::ScriptOccurrence>,
    /// Typed initial findings; runtime findings are published through the driver UI bridge.
    pub findings: Vec<script::Finding>,
    pub driver: scripted::Driver<scripted::ScriptThread>,
    /// The script's widgets.
    pub interface: sampler_ui_ir::Interface,
    /// Ancillary exact XML-element/bus ownership, never a native display law.
    pub meter_buses: std::sync::Arc<[meters::Binding]>,
}

impl Translated {
    /// Collection outcomes before script/source filtering. A failed unused
    /// member does not reject this program; required failures reach Lua.
    pub fn script_members(&self) -> &[script::ScriptOccurrence] {
        self.lua.occurrences()
    }

    /// Start the program's Lua scripts on their own thread and mark the
    /// instrument as scripted: what they replace is no longer reported, what
    /// they use that is not modeled is. `None` when the program has none.
    #[track_caller]
    pub fn attach_script(
        &mut self,
        rate: u32,
        config: script::Config,
    ) -> Result<Option<AttachedScript>, sampler_kontakt::LoadError> {
        self.attach_script_with_ui_state(rate, config, None)
    }

    pub fn attach_script_with_ui_state(
        &mut self,
        rate: u32,
        mut config: script::Config,
        state: Option<script::UiState>,
    ) -> Result<Option<AttachedScript>, sampler_kontakt::LoadError> {
        if self.instrument.behaviors.is_empty() {
            return Ok(None);
        }
        config.rate = f64::from(rate);
        let mut lua = self.lua.clone();
        #[cfg(feature = "library-access")]
        if let Some((bank, program)) = &self.bank {
            lua = bank.bind_script_images(program, lua);
        }
        if self.bank.is_none()
            && let Some(dir) = &self.dir
        {
            lua = bind_loose_script_images(lua, dir);
        }
        let sample_source:Option<script::SampleSource>=match (&self.bank,&self.dir) {
            (Some((bank,program)),_)=>{let(bank,program)=(std::sync::Arc::clone(bank),program.clone());
                Some(std::sync::Arc::new(move|path,explicit|bank.sample_resource(&program,path,explicit)))},
            (None,Some(dir))=>{let dir=dir.clone();Some(std::sync::Arc::new(move|path,explicit|stream::loose_sample_resource(&dir,path,explicit)))},
            _=>None,
        };
        if let Some(source)=sample_source {lua=lua.with_samples(source);}
        let decode: Option<Box<dyn Fn(&str) -> Option<sampler_kontakt::Decoded> + Send + Sync>> =
            match (&self.bank, &self.dir) {
                (Some((bank, program)), _) => {
                    let (bank, program) = (std::sync::Arc::clone(bank), program.clone());
                    Some(Box::new(move |path| {
                        bank.decode_resource(&program, path).ok()
                    }))
                }
                (None, Some(dir)) => {
                    let dir = dir.clone();
                    Some(Box::new(move |path| {
                        decode_sample(&dir.join(path.trim_start_matches('/'))).ok()
                    }))
                }
                (None, None) => None,
            };
        if let Some(decode) = decode {
            let source: script::ImpulseSource = std::sync::Arc::new(move |path: &str| {
                let d = decode(path)?;
                (!d.frames.is_empty() && d.frames.len() <= 1 << 24).then(|| ir::Impulse {
                    rate: d.rate,
                    left: d.frames.iter().map(|f| f[0]).collect(),
                    right: d.frames.iter().map(|f| f[1]).collect(),
                    asset: None,
                })
            });
            lua = lua.with_convolution(
                source,
                engine_parameters::convolver_slots(&self.instrument, &self.inserts),
            );
        }
        let script_members = lua.occurrences().to_vec();
        let (thread, loaded) = scripted::ScriptThread::spawn_shared_with_ui_state(
            self.text.clone(),
            lua,
            config,
            state,
        )
        .map_err(|reason| {
            sampler_kontakt::LoadError::Invalid {
                path: "script".into(),
                reason,
            }
            .at(sampler_kontakt::Stage::ScriptCompile)
        })?;
        if !loaded.folded_writes.is_empty() {
            // Initialization wrote fields with no live path: translate the
            // program as the scripts left it (node ids are unchanged).
            let source = match (&self.bank, &self.dir) {
                (None, Some(dir)) => Source::Disk(dir.clone()),
                _ => Source::Bank,
            };
            let (mut instrument, locations, groups, inserts) =
                translate_initialized(&self.text, source, &loaded.insert_overrides, &loaded.folded_writes)
                    .map_err(|error| sampler_kontakt::LoadError::Invalid {
                        path: "script initialization".into(),
                        reason: error.to_string(),
                    }.at(sampler_kontakt::Stage::Translate))?;
            // Commit only a complete translation. On failure the new owner is
            // dropped (stopped and joined) and the caller's previous IR stays intact.
            instrument.uvi_script_sources = self.instrument.uvi_script_sources.take();
            (self.instrument, self.locations, self.groups, self.inserts) = (instrument, locations, groups, inserts);
        }
        engine_parameters::initialize(&mut self.instrument, &loaded.insert_overrides);
        let unsupported = &mut self.instrument.unsupported;
        unsupported.retain(|u| !(u.feature == "script" && u.value.contains("no frontend")));
        for finding in &loaded.findings {
            unsupported.push(ir::Unsupported {
                location: "script".into(),
                feature: finding.feature.clone(),
                value: finding.value.clone(),
                reason: ir::Reason::NotModeled,
            });
        }
        let groups = self.groups.clone();
        let meter_buses = if loaded.interface.widgets.iter().any(|widget| widget.uvi_meter_request.is_some()) {
            meters::bindings(&self.text, &self.instrument, &self.inserts).into()
        } else { std::sync::Arc::default() };
        Ok(Some(AttachedScript {
            script_members,
            findings: loaded.findings,
            driver: scripted::Driver::new(thread, groups, rate),
            interface: loaded.interface,
            meter_buses,
        }))
    }
}

/// Translate what [`load`] accepts without decoding samples, so a host can
/// shape the instrument (mixer buses) before [`assemble_translated`].
#[track_caller]
pub fn translate_path(path: &Path) -> Result<Translated, Box<dyn std::error::Error>> {
    translate_untagged(path).map_err(|e| staged(e, path, sampler_kontakt::Stage::Translate))
}

/// Translate one program of an already opened `bank` to the IR, without its
/// scripts or samples: what a census needs, with no per-program bank open.
#[cfg(feature = "library-access")]
#[track_caller]
pub fn translate_program(
    bank: &Bank,
    program: &str,
) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    let translate = || -> Result<ir::Instrument, Box<dyn std::error::Error>> {
        let (text, _) = bank.program(program)?;
        translate_decoded_program_inner(bank, program, text)
    };
    translate().map_err(|e| staged(e, Path::new(program), sampler_kontakt::Stage::Translate))
}

/// Translate program XML already decoded from this bank member. The same
/// promotion and error staging as `translate_program` apply; the caller owns
/// the decoded text and can inspect it before passing it here.
#[cfg(feature = "library-access")]
#[track_caller]
pub fn translate_decoded_program(
    bank: &Bank,
    program: &str,
    text: String,
) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    translate_decoded_program_inner(bank, program, text)
        .map_err(|e| staged(e, Path::new(program), sampler_kontakt::Stage::Translate))
}

#[cfg(feature = "library-access")]
fn translate_decoded_program_inner(
    bank: &Bank,
    program: &str,
    text: String,
) -> Result<ir::Instrument, Box<dyn std::error::Error>> {
    let lua = bank.scripts();
    let text = promote(text, &lua);
    let (mut instrument, ..) =
        translate_full(&text, Source::Bank).map_err(|e| describe(Path::new(program), e))?;
    if let Some(sources) = &mut instrument.uvi_script_sources {
        sources.files = lua.sources();
    }
    Ok(instrument)
}

/// Tag `error` with the load `stage` and the caller's location, as the other
/// loaders do ([`sampler_kontakt::LoadError::at`]); reading failures are the
/// container's.
#[track_caller]
fn staged(
    error: Box<dyn std::error::Error>,
    path: &Path,
    stage: sampler_kontakt::Stage,
) -> Box<dyn std::error::Error> {
    use sampler_kontakt::{LoadError, Stage};
    let error=match error.downcast::<sampler_kontakt::LoadError>() {
        Ok(load)=>return Box::new(load.at(stage)),
        Err(other)=>other,
    };
    let (load, stage) = match error.downcast::<Error>() {
        Ok(e) => match *e {
            Error::Io { path, error } => (LoadError::Io { path, error }, Stage::Container),
            Error::Xml { path, error } => (
                LoadError::Invalid {
                    path,
                    reason: error.to_string(),
                },
                stage,
            ),
            Error::Invalid { path, reason } => (LoadError::Invalid { path, reason }, stage),
        },
        Err(other) => (
            LoadError::Invalid {
                path: path.into(),
                reason: other.to_string(),
            },
            stage,
        ),
    };
    Box::new(load.at(stage))
}

fn translate_untagged(path: &Path) -> Result<Translated, Box<dyn std::error::Error>> {
    if let Some(bank_path) = path
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
    {
        let span = sampler_kontakt::audit::Span::new("uvi_bank_open");
        let bank = Bank::open(bank_path)?;
        drop(span);
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
        let span = sampler_kontakt::audit::Span::new("uvi_program_read_decrypt");
        let (text, program_path) = bank.program(&member)?;
        let lua = bank.scripts();
        let text = promote(text, &lua);
        drop(span);
        let span = sampler_kontakt::audit::Span::new("uvi_xml_translate_ir");
        let (mut instrument, locations, groups, inserts) =
            translate_full(&text, Source::Bank).map_err(|e| describe(Path::new(&member), e))?;
        drop(span);
        let text = instrument
            .uvi_script_sources
            .as_ref()
            .map_or_else(|| std::sync::Arc::from(text), |s| s.xml.clone());
        if let Some(sources) = &mut instrument.uvi_script_sources {
            sources.files = lua.sources();
        }
        return Ok(Translated {
            instrument,
            locations,
            bank: Some((std::sync::Arc::new(bank), program_path)),
            dir: None,
            groups,
            inserts,
            text,
            lua,
        });
    }
    let text = promote(read_text(path)?, &Default::default());
    let (instrument, locations, groups, inserts) = translate_full(
        &text,
        Source::Disk(path.parent().unwrap_or(Path::new(".")).into()),
    )
    .map_err(|e| describe(path, e))?;
    let text = instrument
        .uvi_script_sources
        .as_ref()
        .map_or_else(|| std::sync::Arc::from(text), |s| s.xml.clone());
    Ok(Translated {
        instrument,
        locations,
        bank: None,
        dir: path.parent().map(Path::to_path_buf),
        groups,
        inserts,
        text,
        lua: Default::default(),
    })
}

/// Decode a [`translate_path`] result's samples and lower it.
#[track_caller]
pub fn assemble_translated(
    t: Translated,
    rate: u32,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    let named = t.locations.first().map(PathBuf::from).unwrap_or_default();
    let inherit=initial_playback_assets(&t.instrument,t.locations.len())?;
    let decoded = t
        .locations
        .iter()
        .enumerate().map(|(asset,location)| match &t.bank {
            Some((bank, program_path)) => initial_bank_decoded(bank,program_path,location,inherit[asset]),
            None => decode_initial_file(Path::new(location),inherit[asset]).map_err(|e| e.to_string()),
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
    .map_err(|e| staged(e, &named, sampler_kontakt::Stage::Prepare))
}

/// [`assemble_translated`], streamed: only the frames where zones start and a
/// page pool are resident; the rest is read from the bank or file on demand.
#[track_caller]
pub fn assemble_translated_streamed(
    t: Translated,rate:u32,policy:&sampler_kontakt::StreamPolicy,
)->Result<sampler_kontakt::Streamed,Box<dyn std::error::Error>> {
    assemble_translated_streamed_cancelable(t,rate,policy,&||false)
}
/// As assemble_translated_streamed, preserving caller cancellation through
/// source resolution, bounded decode and final publication. No canceled part
/// is returned; the error remains LoadError::Canceled rather than Invalid.
#[track_caller]
pub fn assemble_translated_streamed_cancelable(
    t:Translated,rate:u32,policy:&sampler_kontakt::StreamPolicy,canceled:&(dyn Fn()->bool+Sync),
)->Result<sampler_kontakt::Streamed,Box<dyn std::error::Error>> {
    if canceled(){return Err(Box::new(sampler_kontakt::LoadError::Canceled));}
    let named=t.locations.first().map(PathBuf::from).unwrap_or_default();
    let inherit=initial_playback_assets(&t.instrument,t.locations.len())?;
    let mut sources=Vec::with_capacity(t.locations.len());
    for (asset,location) in t.locations.iter().enumerate() {
        if canceled(){return Err(Box::new(sampler_kontakt::LoadError::Canceled));}
        sources.push(match &t.bank {
            #[cfg(feature="library-access")]
            Some((bank,program_path))=>bank.initial_stream_source(program_path,location,inherit[asset]),
            #[cfg(not(feature="library-access"))]
            Some(_)=>Err("bank samples need the library-access feature".to_string()),
            None=>stream::initial_source(vec![stream::Origin::File(location.into())],inherit[asset]),
        });
    }
    assemble_streamed_cancelable(t.instrument,t.locations,sources,rate,policy,canceled)
        .map_err(|e|staged(e,&named,sampler_kontakt::Stage::Prepare))
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

/// Decode one loose initial sample through the normal UVI resident reader,
/// retaining all original channel ordinals. This does not infer a signal
/// layout, install a sampler graph or silently project a wide sample to stereo.
pub fn decode_source(path:&Path)->Result<sampler_core::SourceStorage,Error> {
    decode_initial_file(path,false).map(|decoded|decoded.source)
}

/// Load a clear `.uvip` program streamed: only the frames where zones start
/// and a page pool are resident (see [`sampler_kontakt::stream_instrument`]).
pub fn load_streamed(
    path: &Path,
    rate: u32,
    policy: &sampler_kontakt::StreamPolicy,
) -> Result<sampler_kontakt::Streamed, Box<dyn std::error::Error>> {
    let folder = path.parent().unwrap_or(Path::new("."));
    let (instrument, locations) = translate_with(&read_text(path)?, Source::Disk(folder.into()))
        .map_err(|e| describe(path, e))?;
    let inherit=initial_playback_assets(&instrument,locations.len())?;
    let sources = locations
        .iter().enumerate()
        .map(|(asset,location)| stream::initial_source(vec![stream::Origin::File(location.into())],inherit[asset]))
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
    let (mut instrument, locations) =
        translate_bank(&text).map_err(|e| describe(Path::new(program), e))?;
    if let Some(sources) = &mut instrument.uvi_script_sources {
        sources.files = bank.scripts().sources();
    }
    let inherit=initial_playback_assets(&instrument,locations.len())?;
    let sources = locations
        .iter().enumerate()
        .map(|(asset,authored)| bank.initial_stream_source(&program_path, authored,inherit[asset]))
        .collect();
    assemble_streamed(instrument, locations, sources, rate, policy)
}

/// Compare streamed reads of a bank resource with a full decode at assorted
/// ranges (start, middle, end, backward). Returns its frame count.
#[doc(hidden)]
#[cfg(feature = "library-access")]
pub fn check_stream(bank: &Bank, program_path: &str, path: &str) -> Result<usize, String> {
    let full = audio::decode(
        &bank
            .resource(program_path, path)
            .map_err(|e| e.to_string())?,
    )?
    .0
    .frames;
    let mut reader = bank
        .stream_source(program_path, path)?
        .open()
        .map_err(|e| e.to_string())?;
    if reader.frames() != full.len() {
        return Err(format!(
            "{} streamed frames, {} decoded",
            reader.frames(),
            full.len()
        ));
    }
    let n = full.len();
    for range in [
        0..n.min(5000),
        n / 2..(n / 2 + 9000).min(n),
        n.saturating_sub(7000)..n,
        100.min(n)..300.min(n),
        0..n.min(30000),
    ] {
        let mut out = vec![[0.0; 2]; range.len()];
        reader
            .read(range.start, &mut out)
            .map_err(|e| e.to_string())?;
        if out != full[range.clone()] {
            return Err(format!("frames {range:?} differ"));
        }
    }
    Ok(n)
}

type AssetSources = Vec<Result<stream::InitialSource, String>>;

fn initial_playback_error(reason:impl Into<String>)->sampler_kontakt::LoadError {
    sampler_kontakt::LoadError::Invalid {path:"initial file playback".into(),reason:reason.into()}
        .at(sampler_kontakt::Stage::Prepare)
}
fn initial_playback_assets(instrument:&ir::Instrument,source_count:usize)->Result<Vec<bool>,sampler_kontakt::LoadError> {
    if source_count!=instrument.assets.len() {return Err(initial_playback_error("asset/source count mismatch"));}
    for zone in &instrument.zones {
        if zone.asset.is_some_and(|a|a.0>=source_count) {return Err(initial_playback_error("dangling initial source asset"));}
    }
    let mut needed=vec![false;source_count];let mut seen=std::collections::HashSet::new();
    for id in &instrument.source_indices.file_playback_zones {
        if !seen.insert(id.0) {return Err(initial_playback_error("duplicate file playback marker"));}
        let zone=instrument.zones.get(id.0).ok_or_else(||initial_playback_error("dangling file playback marker"))?;
        if let Some(asset)=zone.asset {
            *needed.get_mut(asset.0).ok_or_else(||initial_playback_error("dangling file playback asset"))?=true;
        }
    }
    Ok(needed)
}
fn apply_initial_playbacks(instrument:&mut ir::Instrument,playbacks:&[Result<Option<ir::Playback>,String>],usable:&[bool])->Result<(),sampler_kontakt::LoadError> {
    initial_playback_assets(instrument,playbacks.len())?;
    if usable.len()!=playbacks.len() {return Err(initial_playback_error("playback/source state count mismatch"));}
    for id in &instrument.source_indices.file_playback_zones {
        let zone=instrument.zones.get_mut(id.0).ok_or_else(||initial_playback_error("dangling file playback marker"))?;
        if let Some(asset)=zone.asset {
            let available=*usable.get(asset.0).ok_or_else(||initial_playback_error("dangling file playback asset"))?;
            let playback=playbacks.get(asset.0).ok_or_else(||initial_playback_error("missing file playback state"))?;
            if available {
                match playback {
                    Ok(Some(playback))=>{
                        zone.playback.start=playback.start;zone.playback.end=playback.end;zone.playback.looping=playback.looping;
                        zone.playback.loop_control_crossfade=playback.loop_control_crossfade;
                    },
                    Ok(None)=>return Err(initial_playback_error("successful inherited source has no playback metadata")),
                    Err(reason)=>{
                        // Only this marked player's recipe is disabled. Other
                        // players and impulse readers still own the valid PCM.
                        zone.asset=None;
                        instrument.unsupported.push(ir::Unsupported {location:format!("file playback zone {}",id.0),
                            feature:"unreadable initial sample".into(),value:reason.clone(),reason:ir::Reason::InvalidValue});
                    },
                }
            }
            // Failed sources keep explicit unreadable-source reports/unbound
            // recipes. No replacement playback is fabricated.
        }
    }
    Ok(())
}

fn initial_bank_decoded(bank:&Bank,program_path:&str,location:&str,inherit:bool)->Result<audio::InitialDecoded,String> {
    #[cfg(feature="library-access")]
    {bank.initial_resident_sample(program_path,location,inherit)}
    #[cfg(not(feature="library-access"))]
    {let _=(bank,program_path,location,inherit);Err("bank samples need the library-access feature".into())}
}
fn read_initial_snapshot(path:&Path,identity:&sampler_kontakt::FileIdentity)->Result<Vec<u8>,Error> {
    use std::io::Read;
    let mut file=std::fs::File::open(path).map_err(|error|Error::Io{path:path.into(),error})?;
    identity.check(&file).map_err(|error|Error::Io{path:path.into(),error})?;
    let mut bytes=Vec::new();file.by_ref().take((512<<20)+1).read_to_end(&mut bytes).map_err(|error|Error::Io{path:path.into(),error})?;
    identity.check(&file).map_err(|error|Error::Io{path:path.into(),error})?;
    Ok(bytes)
}
fn decode_initial_file(path:&Path,inherit:bool)->Result<audio::InitialDecoded,Error> {
    let file=std::fs::File::open(path).map_err(|error|Error::Io{path:path.into(),error})?;
    let metadata=file.metadata().map_err(|error|Error::Io{path:path.into(),error})?;
    let identity=sampler_kontakt::FileIdentity::capture(&metadata).map_err(|error|Error::Io{path:path.into(),error})?;
    let bytes=read_initial_snapshot(path,&identity)?;
    let decoded=audio::decode_initial(&[bytes],inherit).map_err(|reason|Error::Invalid{path:path.into(),reason})?;
    let path=path.to_owned();let error_path=path.clone();
    sampler_kontakt::install_resident_source_reload(&decoded.source,move || {
        let bytes=read_initial_snapshot(&path,&identity).map_err(|_|sampler_core::Error::NotReady)?;
        audio::decode_initial(&[bytes],false).map(|d|d.source).map_err(|_|sampler_core::Error::NotReady)
    }).map_err(|e|Error::Invalid{path:error_path,reason:e.to_string()})?;
    Ok(decoded)
}

/// A failed initial file must not erase an authored dynamic target's mapping.
/// The loader still reports the failed file; a later sample transaction can
/// activate this same compiled region without replacing the whole plan.
fn retain_failed_sample_targets(instrument: &mut ir::Instrument, usable: &[bool]) {
    for target in &instrument.source_indices.sample_targets {
        for zone in &target.zones {
            let zone = &mut instrument.zones[zone.0];
            if zone.asset.is_some_and(|a| !usable[a.0]) {
                zone.asset = None;
            }
        }
    }
}

/// [`assemble`] for streamed samples. Failed initial samples are reported;
/// script-loadable players retain an unbound recipe, while other zones are removed.
fn assemble_streamed(
    instrument: ir::Instrument,locations:Vec<String>,sources:AssetSources,rate:u32,policy:&sampler_kontakt::StreamPolicy,
)->Result<sampler_kontakt::Streamed,Box<dyn std::error::Error>> {
    assemble_streamed_cancelable(instrument,locations,sources,rate,policy,&||false)
}
fn assemble_streamed_cancelable(
    mut instrument: ir::Instrument,
    locations: Vec<String>, sources:AssetSources,rate:u32,policy:&sampler_kontakt::StreamPolicy,
    canceled:&(dyn Fn()->bool+Sync),
)->Result<sampler_kontakt::Streamed,Box<dyn std::error::Error>> {
    if canceled(){return Err(Box::new(sampler_kontakt::LoadError::Canceled));}
    initial_playback_assets(&instrument,locations.len())?;
    if sources.len()!=locations.len() {return Err(initial_playback_error("stream/source count mismatch").into());}
    let playbacks:Vec<_>=sources.iter().map(|s|match s {Ok(s)=>s.playback.clone(),Err(_)=>Ok(None)}).collect();
    apply_initial_playbacks(&mut instrument,&playbacks,&sources.iter().map(Result::is_ok).collect::<Vec<_>>())?;
    for (location, result) in locations.iter().zip(&sources) {
        if let Err(reason) = result {
            instrument.unsupported.push(ir::Unsupported {
                location: location.clone(),
                feature: "unreadable initial sample".into(),
                value: reason.clone(),
                reason: ir::Reason::InvalidValue,
            });
        }
    }
    inserts::fill_impulses(&mut instrument, &locations, |a| {
        let source = sources[a].as_ref().map_err(Clone::clone)?;
        let mut reader = source.source.open().map_err(|e| e.to_string())?;
        if canceled(){return Err("load cancelled".into());}
        let mut frames = vec![[0f32; 2]; reader.frames()];
        for (index,page) in frames.chunks_mut(sampler_core::PAGE_FRAMES).enumerate() {
            if canceled(){return Err("load cancelled".into());}
            reader.read(index*sampler_core::PAGE_FRAMES,page).map_err(|e|e.to_string())?;
        }
        Ok((reader.rate(), frames))
    });
    if canceled(){return Err(Box::new(sampler_kontakt::LoadError::Canceled));}
    let usable: Vec<bool> = sources.iter().map(Result::is_ok).collect();
    retain_failed_sample_targets(&mut instrument, &usable);
    let kept = instrument.retain_zones(|z| z.asset.is_none_or(|a| usable[a.0]));
    let mut sources: Vec<_> = sources.into_iter().map(|s|s.ok().map(|s|s.source)).collect();
    let sources = kept
        .iter()
        .map(|&a| sources[a].take().expect("usable"))
        .collect();
    let labels = kept.iter().map(|&a| locations[a].clone()).collect();
    let options = sampler_kontakt::Options {
        rate,
        scripts: true,
        ..Default::default()
    };
    let bindings = engine_parameters::bindings(&instrument);
    let mut streamed =
        sampler_kontakt::stream_instrument_cancelable(instrument, sources, labels, &options, policy,canceled)?;
    streamed.loaded.plan = streamed
        .loaded
        .plan
        .with_engine_parameters(bindings, Vec::new())?;
    Ok(streamed)
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
    if let Some(sources) = &mut instrument.uvi_script_sources {
        sources.files = bank.scripts().sources();
    }
    let (locations, decoded) =
        decode_selected_bank_assets(&mut instrument, locations, options, |authored,inherit| {
            initial_bank_decoded(bank,&program_path,authored,inherit)
        })?;
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
    let options = sampler_kontakt::Options {
        rate,
        ..Default::default()
    };
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
    let lua = bank.bind_script_samples(&program_path, bank.scripts());
    let ((mut instrument, locations, groups, inserts), host) =
        initialized_translation(text, lua, Source::Bank, Path::new(program), false)
            .map_err(ScriptedTranslationFailure::into_error)?;
    let (locations, decoded) =
        decode_selected_bank_assets(&mut instrument, locations, options, |authored,inherit| {
            initial_bank_decoded(bank,&program_path,authored,inherit)
        })?;
    let loaded = assemble(instrument, locations, decoded, options)?;
    let mut instrument = loaded.instrument;
    note_script(&host, &mut instrument);
    Ok(scripted::Program {
        instrument,
        plan: loaded.plan,
        host,
        groups,
        inserts,
        stream: None,
    })
}

/// Share the loose-program image service with playback and offline initialization.
fn bind_loose_script_images(files: script::Scripts, dir: &Path) -> script::Scripts {
    let resources = resources::Resources::of(&dir.join("ui.uvip"));
    let dir=dir.to_owned();
    files.with_images(move |path| resources.read(path)).with_samples(std::sync::Arc::new(move |path,explicit|stream::loose_sample_resource(&dir,path,explicit)))
}

/// Which source authority to use when inspecting decoded XML after onLoad.
#[cfg(feature = "library-access")]
pub enum ScriptedOrigin<'a> {
    /// A decoded member without a retained resource authority. Image services
    /// are unavailable; callers that own the bank should use BankWithResources.
    Bank(&'a str),
    BankWithResources(&'a Bank, &'a str),
    Loose(&'a Path),
}

/// The phase that rejected a scripted translation. Error text may contain
/// authored values and must be handled as private by callers.
#[cfg(feature = "library-access")]
#[derive(Debug)]
pub enum ScriptedTranslationFailure {
    Initialize(Box<dyn std::error::Error>),
    Translate(Box<dyn std::error::Error>),
}

#[cfg(feature = "library-access")]
impl ScriptedTranslationFailure {
    fn into_error(self) -> Box<dyn std::error::Error> {
        match self {
            Self::Initialize(error) | Self::Translate(error) => error,
        }
    }
}

/// Run the same promotion, one initializer, override application and XML
/// translation as the scripted bank loader, without reading or decoding audio.
#[cfg(feature = "library-access")]
pub fn inspect_decoded_scripted_program(
    text: String,
    files: script::Scripts,
    origin: ScriptedOrigin<'_>,
) -> Result<(ir::Instrument, script::ScriptHost), ScriptedTranslationFailure> {
    inspect_decoded_scripted_program_with_options(text, files, origin, true, false)
}

/// Same source/resource authority, with explicit coverage and profiling choices.
/// UI preview uses the ordinary non-coverage constructor; source audits retain
/// line coverage. Profiling changes observations, never the initializer count.
#[cfg(feature = "library-access")]
pub fn inspect_decoded_scripted_program_with_options(
    text: String,
    files: script::Scripts,
    origin: ScriptedOrigin<'_>,
    coverage: bool,
    profile: bool,
) -> Result<(ir::Instrument, script::ScriptHost), ScriptedTranslationFailure> {
    inspect_decoded_scripted_program_with_config(
        text, files, origin, coverage, profile, script::Config::default(),
    )
}

/// Explicit inspection configuration, including scanner-only deterministic RNG.
/// Ordinary production and legacy inspection entry points retain their defaults.
#[cfg(feature = "library-access")]
pub fn inspect_decoded_scripted_program_with_config(
    text: String,
    files: script::Scripts,
    origin: ScriptedOrigin<'_>,
    coverage: bool,
    profile: bool,
    config: script::Config,
) -> Result<(ir::Instrument, script::ScriptHost), ScriptedTranslationFailure> {
    let (source, path, files) = match origin {
        ScriptedOrigin::Bank(program) => (Source::Bank, Path::new(program), files),
        ScriptedOrigin::BankWithResources(bank, program) => (
            Source::Bank, Path::new(program), bank.bind_script_images(program, files),
        ),
        ScriptedOrigin::Loose(path) => {
            let dir = path.parent().unwrap_or(Path::new("."));
            (Source::Disk(dir.into()), path, bind_loose_script_images(files, dir))
        }
    };
    let ((mut instrument, ..), host) =
        initialized_translation_configured(text, files, source, path, coverage, profile, config)?;
    note_script(&host, &mut instrument);
    Ok((instrument, host))
}

#[cfg(feature = "library-access")]
fn initialized_translation(
    text: String,
    files: script::Scripts,
    source: Source,
    path: &Path,
    inspected: bool,
) -> Result<(FullTranslation, script::ScriptHost), ScriptedTranslationFailure> {
    initialized_translation_profiled(text, files, source, path, inspected, false)
}

#[cfg(feature = "library-access")]
fn initialized_translation_profiled(
    text: String,
    files: script::Scripts,
    source: Source,
    path: &Path,
    inspected: bool,
    profile: bool,
) -> Result<(FullTranslation, script::ScriptHost), ScriptedTranslationFailure> {
    initialized_translation_configured(
        text, files, source, path, inspected, profile, script::Config::default(),
    )
}

#[cfg(feature = "library-access")]
fn initialized_translation_configured(
    text: String,
    files: script::Scripts,
    source: Source,
    path: &Path,
    inspected: bool,
    profile: bool,
    config: script::Config,
) -> Result<(FullTranslation, script::ScriptHost), ScriptedTranslationFailure> {
    let promote_started = std::time::Instant::now();
    let text = promote(text, &files);
    let promote_ms = promote_started.elapsed().as_secs_f64() * 1000.;
    let host = if profile {
        script::ScriptHost::new_profiled(&text, files.clone(), config, inspected)
    } else if inspected {
        script::ScriptHost::new_inspected(&text, files.clone(), config)
    } else {
        script::ScriptHost::new(&text, files.clone(), config)
    }
    .map_err(|error| ScriptedTranslationFailure::Initialize(error.into()))?;
    if profile {
        host.profile_record("script_promotion", promote_ms);
    }
    let _translation = host.profile_span("post_init_overlays_and_ir_translation");
    let overlays = host.profile_span("parameter_overlay_export");
    let insert_overrides = host.insert_overrides();
    let folded_writes = host.folded_writes();
    drop(overlays);
    let (mut instrument, locations, groups, inserts) =
        translate_initialized(&text, source, &insert_overrides, &folded_writes).map_err(
            |error| ScriptedTranslationFailure::Translate(Box::new(describe(path, error))),
        )?;
    if let Some(sources) = &mut instrument.uvi_script_sources {
        sources.files = files.sources();
    }
    drop(_translation);
    Ok(((instrument, locations, groups, inserts), host))
}

#[cfg(all(test, feature = "library-access"))]
mod scripted_translation_tests {
    use super::*;

    fn translated_fixture(xml: &str) -> Translated {
        let (instrument, locations, groups, inserts) = translate_full(xml, Source::Bank).unwrap();
        Translated {
            instrument, locations, groups, inserts, bank: None, dir: None,
            text: std::sync::Arc::from(xml), lua: Default::default(),
        }
    }

    #[test]
    fn failed_final_initialization_stops_the_new_owner_and_preserves_previous_translation() {
        // Fault injection after the initial graph was translated: a fractional
        // MIDI root is valid XML but invalid for translation. Both final and
        // fallback translations must reject it; the old implementation silently
        // kept the prior graph. No vendor occurrence of this failure is claimed.
        let xml = r#"<Program Name="Before" Streaming="1"><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers>
          <EventProcessors><ScriptProcessor><script>
            Program:setParameter('Streaming', false)
        </script></ScriptProcessor></EventProcessors></Program>"#;
        let mut translated = translated_fixture(xml);
        translated.text = std::sync::Arc::from(xml.replace("BaseNote=\"60\"", "BaseNote=\"60.5\""));
        let before = translated.instrument.clone();
        let locations = translated.locations.clone();
        let retained = std::sync::Arc::new(());
        let owner_ref = retained.clone();
        translated.lua = translated.lua.with_images(move |_| { let _ = &owner_ref; None });
        for _ in 0..2 {
            let error = translated.attach_script(48000, Default::default()).err()
                .expect("untranslatable initialized state must reject attachment");
            assert_eq!(error.stage(), Some(sampler_kontakt::Stage::Translate));
            assert_eq!(translated.instrument, before);
            assert_eq!(translated.locations, locations);
            assert_eq!(std::sync::Arc::strong_count(&retained), 2,
                "failed attachment must stop and join its owner before returning");
        }
        assert!(matches!(inspect_decoded_scripted_program(
            translated.text.to_string(), Default::default(), ScriptedOrigin::Bank("Authored.uvip")),
            Err(ScriptedTranslationFailure::Translate(_))));
        translated.text = std::sync::Arc::from(xml);
        let attached = translated.attach_script(48000, Default::default()).unwrap().unwrap();
        assert_eq!(translated.instrument.name, "Before");
        drop(attached);
        assert_eq!(std::sync::Arc::strong_count(&retained), 2);
    }

    #[test]
    fn rejected_folded_value_keeps_the_documented_explicit_fallback() {
        let xml = r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
        let doc = Document::parse(xml).unwrap();
        let node = doc.descendants().find(|n| n.has_tag_name("SamplePlayer")).unwrap().id().get_usize();
        let (before, ..) = translate_full(xml, Source::Bank).unwrap();
        let (after, ..) = translate_initialized(xml, Source::Bank,
            &[(node, "BaseNote".into(), "60.5".into())],
            &[(node, "SamplePlayer".into(), "BaseNote".into())]).unwrap();
        assert_eq!(after.zones, before.zones);
        assert!(after.unsupported.iter().any(|u|
            u.feature.contains("BaseNote") && u.value.contains("translation rejects")));
    }

    #[test]
    fn bank_product_and_inspector_use_the_same_post_onload_gain() {
        let xml = r#"<Program><Inserts><Gain Volume="1"/></Inserts><EventProcessors><ScriptProcessor><script><![CDATA[local gain=Program.inserts[1]
function onInit() gain:setParameter('Volume',0.25) end]]></script></ScriptProcessor></EventProcessors></Program>"#;
        let files = script::Scripts::default();
        let (static_ir, ..) = translate_full(xml, Source::Bank).unwrap();
        let ((product_ir, ..), _) = initialized_translation(
            xml.to_owned(),
            files.clone(),
            Source::Bank,
            Path::new("Authored.uvip"),
            false,
        )
        .unwrap();
        let (inspected_ir, host) = inspect_decoded_scripted_program(
            xml.to_owned(),
            files,
            ScriptedOrigin::Bank("Authored.uvip"),
        )
        .unwrap();
        let gain = |ir: &ir::Instrument| {
            ir.chains
                .iter()
                .flat_map(|chain| {
                    chain
                        .pre_amplitude
                        .iter()
                        .chain(&chain.post_amplitude)
                })
                .find_map(|processor| match processor {
                    ir::Processor::Gain(ir::Gain::Linear(value)) => Some(*value),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(gain(&static_ir), 1.0);
        assert_eq!(gain(&product_ir), 0.25);
        assert_eq!(gain(&inspected_ir), gain(&product_ir));
        assert!(host.fault_counts().init.values().all(|count| *count == 0));
    }

    #[test]
    fn loose_inspection_uses_playbacks_image_authority_for_intrinsic_widget_size() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
        }
        let directory = Fixture(std::env::temp_dir().join(format!(
            "kontra-inspected-images-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        )));
        std::fs::create_dir_all(&directory.0).unwrap();
        let path = directory.0.join("Authored.uvip");
        let mut header = vec![0; 33];
        header[..16].copy_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
        header[16..20].copy_from_slice(&720u32.to_be_bytes());
        header[20..24].copy_from_slice(&18u32.to_be_bytes());
        std::fs::write(directory.0.join("overlay.png"), header).unwrap();
        let xml = r#"<Program><EventProcessors><ScriptProcessor><script>image=Image('overlay.png')</script></ScriptProcessor></EventProcessors></Program>"#;
        let product = script::ScriptHost::new(xml,
            bind_loose_script_images(script::Scripts::default(), &directory.0), Default::default()).unwrap();
        let (_, inspected) = inspect_decoded_scripted_program(xml.into(), script::Scripts::default(),
            ScriptedOrigin::Loose(&path)).unwrap();
        for host in [&product, &inspected] {
            let image = &host.interface().widgets[0];
            assert_eq!((image.rect.width, image.rect.height), (720., 18.));
            assert!(host.fault_counts().init.is_empty());
        }
        assert_eq!(product.interface(), inspected.interface());
    }
}

/// Impulse assets use the translated program's original asset indices. Resolve
/// them before narrowing note zones, which renumbers only zone assets.
fn decode_selected_bank_assets(
    instrument: &mut ir::Instrument,
    locations: Vec<String>,
    options: &sampler_kontakt::Options,
    mut decode: impl FnMut(&str,bool) -> Result<audio::InitialDecoded, String>,
) -> Result<(Vec<String>, Vec<Result<audio::InitialDecoded, String>>),sampler_kontakt::LoadError> {
    initial_playback_assets(instrument,locations.len())?;
    inserts::fill_impulses(instrument, &locations, |asset| {
        decode(&locations[asset],false).and_then(|decoded|decoded.stereo_audio().map(|audio|(audio.rate,audio.frames)))
    });
    let kept = instrument.retain_zones(|zone| {
        zone.keys.high >= *options.keys.start() && zone.keys.low <= *options.keys.end()
    });
    let locations: Vec<_> = kept.iter().map(|&asset| locations[asset].clone()).collect();
    let inherit=initial_playback_assets(instrument,locations.len())?;
    let decoded = locations.iter().enumerate().map(|(asset,location)| decode(location,inherit[asset])).collect();
    Ok((locations, decoded))
}

#[cfg(test)]
mod narrowed_impulse_tests {
    use super::*;

    #[test]
    fn retained_note_range_still_decodes_impulse_at_original_asset_index() {
        let locations: Vec<String> = ["low.wav", "high.wav", "spare.wav", "room.wav"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut instrument = ir::Instrument {
            assets: locations
                .iter()
                .map(|location| ir::Asset {
                    location: ir::AssetLocation::Path(location.clone()),
                    residency: Default::default(),
                    encoding: ir::Encoding::Wav,
                    root_key: None,
                    loops: Vec::new(),
                })
                .collect(),
            zones: [
                (0, ir::KeyRange { low: 60, high: 60 }),
                (1, ir::KeyRange { low: 61, high: 61 }),
            ]
            .into_iter()
            .map(|(asset, keys)| {
                let mut zone = ir::Zone::new(ir::AssetRef(asset));
                zone.keys = keys;
                zone
            })
            .collect(),
            impulses: vec![ir::Impulse {
                rate: 48000,
                left: vec![1.0],
                right: vec![1.0],
                asset: Some(ir::AssetRef(3)),
            }],
            ..Default::default()
        };
        let mut reads = Vec::new();
        let (selected, decoded) = decode_selected_bank_assets(
            &mut instrument,
            locations,
            &sampler_kontakt::Options {
                rate: 48000,
                keys: 60..=60,
                ..Default::default()
            },
            |location,_| {
                reads.push(location.to_owned());
                Ok(audio::InitialDecoded {source:sampler_core::Pcm::new(48000,vec![[0.5,-0.5]].into_boxed_slice()).unwrap().into(),playback:Ok(None)})
            },
        ).unwrap();
        assert_eq!(reads, ["room.wav", "low.wav"]);
        assert_eq!(selected, ["low.wav"]);
        assert_eq!(decoded.len(), 1);
        assert_eq!(instrument.zones[0].asset, Some(ir::AssetRef(0)));
        assert_eq!(instrument.impulses[0].asset, None);
        assert_eq!(instrument.impulses[0].left, [0.5]);
        assert_eq!(instrument.impulses[0].right, [-0.5]);
    }
}

/// Mark (`ScriptLive`) the ConstantModulation nodes the program's scripts can
/// write, so the translator makes them live and the host drives them.
fn promote(text: String, scripts: &script::Scripts) -> String {
    let promoted = engine_parameters::promoted_constants(&text, scripts);
    let marks: Vec<_> = promoted
        .into_iter()
        .map(|(node, id)| (node, "ScriptLive".to_owned(), id.to_string()))
        .collect();
    if marks.is_empty() {
        text
    } else {
        apply_overrides(&text, &marks)
    }
}

/// Write initialized script values into XML before translating the program.
/// Translate `text` as initialization left it: `overrides` patch its XML. A
/// folded write (node, kind, field) the translator never reads has no effect
/// and is reported. When a written value makes the program untranslatable, the
/// authored program is translated and the folded writes are reported instead.
fn translate_initialized(
    text: &str,
    source: Source,
    overrides: &[(usize, String, String)],
    folded: &[(usize, String, String)],
) -> Result<FullTranslation, Translate> {
    if folded.is_empty() {
        return translate_full(&apply_overrides(text, overrides), source);
    }
    let patched = apply_overrides(text, overrides);
    READS.with(|r| *r.borrow_mut() = Some(HashSet::new()));
    let result = translate_full(&patched, source.clone());
    let reads = READS.with(|r| r.borrow_mut().take()).unwrap_or_default();
    let (mut full, why) = match result {
        Ok(full) => (full, "a write the translation does not read"),
        Err(_) => {
            let unfolded: Vec<_> = overrides
                .iter()
                .filter(|(node, name, _)| !folded.iter().any(|(n, _, f)| n == node && f == name))
                .cloned()
                .collect();
            (translate_full(&apply_overrides(text, &unfolded), source)?, "a written value the translation rejects")
        }
    };
    // An element left bypassed (or inside a bypassed one) is silent, so its
    // fields are rightly never read: such writes have no effect.
    let doc = roxmltree::Document::parse(&patched).ok();
    let silent = |node: usize| {
        let Some(doc) = &doc else { return false };
        let Some(node) = u32::try_from(node).ok().and_then(|n| doc.get_node(roxmltree::NodeId::new(n))) else {
            return false;
        };
        node.ancestors().any(|a| a.attribute("Bypass").and_then(|b| b.parse::<f64>().ok()).is_some_and(|b| b != 0.))
    };
    let mut reported = HashSet::new();
    for (node, kind, name) in folded {
        let read = why.starts_with("a write") && (reads.contains(&(*node, name.clone())) || silent(*node));
        if !read && reported.insert((kind.as_str(), name.as_str())) {
            full.0.unsupported.push(ir::Unsupported {
                location: "script".into(),
                feature: format!("lua setParameter {kind}.{name}"),
                value: why.into(),
                reason: ir::Reason::NotModeled,
            });
        }
    }
    Ok(full)
}

fn apply_overrides(text: &str, overrides: &[(usize, String, String)]) -> String {
    let Ok(doc) = roxmltree::Document::parse(text) else {
        return text.to_owned();
    };
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        for (_, name, value) in overrides
            .iter()
            .filter(|(id, ..)| *id == node.id().get_usize())
        {
            match node.attribute_node(name.as_str()) {
                Some(a) => {
                    let r = a.range_value();
                    edits.push((r.start, r.end, value.clone()));
                }
                None => {
                    let at = node.range().start + 1 + node.tag_name().name().len();
                    edits.push((at, at, format!(" {name}=\"{value}\"")));
                }
            }
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = text.to_owned();
    for (start, end, value) in edits {
        out.replace_range(start..end, &value);
    }
    out
}

/// What the scripts replace is no longer reported; what they use that is not
/// modeled is.
#[cfg(feature = "library-access")]
fn note_script(host: &script::ScriptHost, instrument: &mut ir::Instrument) {
    instrument
        .unsupported
        .retain(|u| !(u.feature == "script" && u.value.contains("no frontend")));
    for finding in host.findings() {
        instrument.unsupported.push(ir::Unsupported {
            location: "script".into(),
            feature: finding.feature,
            value: finding.value,
            reason: ir::Reason::NotModeled,
        });
    }
}

/// [`load_program_scripted`] with the samples streamed from the bank: every zone
/// is present for the scripts, only their starts are resident.
#[cfg(feature = "library-access")]
pub fn load_program_scripted_streamed(
    bank: &Bank,
    program: &str,
    rate: u32,
    policy: &sampler_kontakt::StreamPolicy,
) -> Result<scripted::Program, Box<dyn std::error::Error>> {
    let (text, program_path) = bank.program(program)?;
    let lua = bank.bind_script_samples(&program_path, bank.scripts());
    let ((instrument, locations, groups, inserts), host) =
        initialized_translation(text, lua, Source::Bank, Path::new(program), false)
            .map_err(ScriptedTranslationFailure::into_error)?;
    let inherit=initial_playback_assets(&instrument,locations.len())?;
    let sources = locations
        .iter().enumerate()
        .map(|(asset,authored)| bank.initial_stream_source(&program_path, authored,inherit[asset]))
        .collect();
    let streamed = assemble_streamed(instrument, locations, sources, rate, policy)?;
    let sampler_kontakt::Streamed {
        loaded,
        assets,
        cache,
        streamer,
        report,
    } = streamed;
    let mut instrument = loaded.instrument;
    note_script(&host, &mut instrument);
    Ok(scripted::Program {
        instrument,
        plan: loaded.plan,
        host,
        groups,
        inserts,
        stream: Some(scripted::Stream {
            cache: Some(cache),
            horizon: report.head_frames,
            _keep: (streamer, assets),
        }),
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

/// Report unreadable initial samples, retain unbound script-loadable recipes,
/// then fit decoded zones to their audio and lower. `decoded[i]` belongs to asset `i`.
fn assemble(
    mut instrument: ir::Instrument,
    locations: Vec<String>,
    decoded: Vec<Result<audio::InitialDecoded, String>>,
    options: &sampler_kontakt::Options,
) -> Result<sampler_kontakt::Loaded, Box<dyn std::error::Error>> {
    initial_playback_assets(&instrument,locations.len())?;
    if decoded.len()!=locations.len() {return Err(initial_playback_error("decoded/source count mismatch").into());}
    let playbacks:Vec<_>=decoded.iter().map(|d|match d {Ok(d)=>d.playback.clone(),Err(_)=>Ok(None)}).collect();
    apply_initial_playbacks(&mut instrument,&playbacks,&decoded.iter().map(Result::is_ok).collect::<Vec<_>>())?;
    for (location, result) in locations.iter().zip(&decoded) {
        if let Err(reason) = result {
            instrument.unsupported.push(ir::Unsupported {
                location: location.clone(),
                feature: "unreadable initial sample".into(),
                value: reason.clone(),
                reason: ir::Reason::InvalidValue,
            });
        }
    }
    inserts::fill_impulses(&mut instrument, &locations, |a| match &decoded[a] {
        Ok(d) => d.stereo_audio().map(|audio|(audio.rate,audio.frames)),
        Err(e) => Err(e.clone()),
    });
    let usable: Vec<bool> = decoded.iter().map(Result::is_ok).collect();
    retain_failed_sample_targets(&mut instrument, &usable);
    let kept = instrument.retain_zones(|z| z.asset.is_none_or(|a| usable[a.0]));
    let mut sources = Vec::with_capacity(kept.len());
    let mut decoded: Vec<_> = decoded.into_iter().map(Some).collect();
    for &asset in &kept {
        let d = decoded[asset].take().unwrap().unwrap();
        sources.push(d.source);
    }
    let labels: Vec<String> = kept.iter().map(|&a| locations[a].clone()).collect();
    let bindings = engine_parameters::bindings(&instrument);
    let mut loaded = sampler_kontakt::finish_sources(instrument, sources, labels, options)?;
    loaded.plan = loaded.plan.with_engine_parameters(bindings, Vec::new())?;
    Ok(loaded)
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

    /// What each program is made of and what the translation drops: `UE`
    /// lines (element tag, attribute names, one per element) and `UM` lines
    /// (feature, value). Names only. Shard with `KONTRA_SHARD=i/n`.
    #[test]
    #[ignore]
    #[cfg(feature = "library-access")]
    fn census_modules() {
        let (shard, shards): (usize, usize) = std::env::var("KONTRA_SHARD")
            .ok()
            .and_then(|v| {
                let (a, b) = v.split_once('/')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .unwrap_or((0, 1));
        let mut index = 0;
        // (tag|parent|attrs) -> (elements, programs); (feature|value) -> programs
        let mut elements: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        let mut dropped: std::collections::BTreeMap<String, usize> = Default::default();
        // per library and insert module: (active, bypassed), and per numeric
        // attribute its (min, max) over the active instances
        let mut inserts: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        let mut ranges: std::collections::BTreeMap<String, (f64, f64)> = Default::default();
        let mut files = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(
            std::env::var("KONTRA_UVI_LIBRARIES").unwrap(),
        )];
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
        for f in files {
            let Ok(bank) = crate::Bank::open(&f) else {
                continue;
            };
            for program in bank.programs() {
                index += 1;
                if index % shards != shard {
                    continue;
                }
                let Ok((text, _)) = bank.program(&program) else {
                    continue;
                };
                let Ok(doc) = crate::parse_program_xml(&text) else {
                    continue;
                };
                let lib = f
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().replace(' ', "_"))
                    .unwrap_or_default();
                let lib = lib.split("_-_").last().unwrap_or("").to_owned();
                let mut seen = std::collections::BTreeSet::new();
                for node in doc.descendants().filter(|n| n.is_element()) {
                    if node.parent().is_some_and(|p| p.has_tag_name("Inserts")) {
                        let tag = node.tag_name().name();
                        let bypassed = node.attribute("Bypass") == Some("1");
                        let e = inserts.entry(format!("{lib}|{tag}")).or_default();
                        if bypassed {
                            e.1 += 1;
                        } else {
                            e.0 += 1;
                            for a in node.attributes() {
                                if let Ok(v) = a.value().parse::<f64>() {
                                    let r = ranges
                                        .entry(format!("{lib}|{tag}|{}", a.name()))
                                        .or_insert((v, v));
                                    r.0 = r.0.min(v);
                                    r.1 = r.1.max(v);
                                }
                            }
                        }
                    }
                }
                for node in doc.descendants().filter(|n| n.is_element()) {
                    let mut attrs: Vec<&str> = node.attributes().map(|a| a.name()).collect();
                    attrs.sort();
                    let parent = node.parent().map(|p| p.tag_name().name()).unwrap_or("");
                    let key = format!("{}|{}|{}", node.tag_name().name(), parent, attrs.join(","));
                    let e = elements.entry(key.clone()).or_default();
                    e.0 += 1;
                    if seen.insert(key) {
                        e.1 += 1;
                    }
                }
                let mut once = std::collections::BTreeSet::new();
                match crate::translate_bank(&text) {
                    Ok((ir, _)) => {
                        for u in &ir.unsupported {
                            let key = format!(
                                "{}|{}",
                                u.feature,
                                u.value
                                    .chars()
                                    .take(60)
                                    .collect::<String>()
                                    .replace(' ', "_")
                            );
                            if once.insert(key.clone()) {
                                *dropped.entry(key).or_default() += 1;
                            }
                        }
                    }
                    Err(_) => *dropped.entry("translate-fail|".into()).or_default() += 1,
                }
            }
        }
        for (k, (n, p)) in &elements {
            println!("UE {n} {p} {k}");
        }
        for (k, p) in &dropped {
            println!("UM {p} {k}");
        }
        for (k, (a, y)) in &inserts {
            println!("UB {a} {y} {k}");
        }
        for (k, (lo, hi)) in &ranges {
            println!("UR {lo} {hi} {k}");
        }
    }

    /// How many programs' scripts (and the modules they require) use each API
    /// name: `US name programs occurrences` for the names in the file
    /// `KONTRA_SYMS`, and `UK name programs` for names that are called but
    /// that no script defines. Names and counts only. Shard like the others.
    #[test]
    #[ignore]
    #[cfg(feature = "library-access")]
    fn census_symbols() {
        use std::collections::{BTreeMap, BTreeSet};
        let wanted: BTreeSet<String> = std::env::var("KONTRA_SYMS")
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let mut files = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(
            std::env::var("KONTRA_UVI_LIBRARIES").unwrap(),
        )];
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
        let mut used: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut called: BTreeMap<String, usize> = BTreeMap::new();
        let mut defined_anywhere: BTreeSet<String> = BTreeSet::new();
        let mut programs = 0;
        let words = |src: &str| -> Vec<(String, bool, bool)> {
            // (word, called, defined)
            let b = src.as_bytes();
            let mut out = Vec::new();
            let mut i = 0;
            let mut prev = String::new();
            while i < b.len() {
                if b[i] == b'-' && b.get(i + 1) == Some(&b'-') {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
                    let st = i;
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    let w = src[st..i].to_owned();
                    let mut j = i;
                    while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                        j += 1;
                    }
                    let called =
                        matches!(b.get(j), Some(b'(') | Some(b'{') | Some(b'"') | Some(b'\''));
                    let assigned = b.get(j) == Some(&b'=') && b.get(j + 1) != Some(&b'=');
                    let defined =
                        assigned || matches!(prev.as_str(), "function" | "local" | "class");
                    prev = w.clone();
                    out.push((w, called, defined));
                } else {
                    if !b[i].is_ascii_whitespace() && b[i] != b'.' && b[i] != b':' {
                        prev.clear();
                    }
                    i += 1;
                }
            }
            out
        };
        for f in files {
            let Ok(bank) = crate::Bank::open(&f) else {
                continue;
            };
            let lua = bank.scripts();
            for program in bank.programs() {
                let Ok((text, _)) = bank.program(&program) else {
                    continue;
                };
                let Ok(doc) = crate::parse_program_xml(&text) else {
                    continue;
                };
                let mut sources: Vec<String> = doc
                    .descendants()
                    .filter(|n| n.has_tag_name("script"))
                    .filter_map(|n| n.text().map(str::to_owned))
                    .collect();
                if sources.is_empty() {
                    continue;
                }
                programs += 1;
                use crate::script::Files;
                let mut seen_modules = BTreeSet::new();
                let mut k = 0;
                while k < sources.len() {
                    let src = sources[k].clone();
                    k += 1;
                    let mut rest = src.as_str();
                    while let Some(at) = rest.find("require") {
                        rest = &rest[at + 7..];
                        let t = rest.trim_start_matches([' ', '(']);
                        if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'')
                            && let Some(end) = t[1..].find(q)
                        {
                            let m = &t[1..1 + end];
                            if seen_modules.insert(m.to_owned())
                                && let Some(source) = lua.script(m)
                            {
                                sources.push(source);
                            }
                        }
                    }
                }
                let mut here: BTreeMap<String, usize> = BTreeMap::new();
                let mut here_called = BTreeSet::new();
                for src in &sources {
                    for (w, c, d) in words(src) {
                        if d {
                            defined_anywhere.insert(w.clone());
                        }
                        if c {
                            here_called.insert(w.clone());
                        }
                        if wanted.contains(&w) {
                            *here.entry(w).or_default() += 1;
                        }
                    }
                }
                for (w, n) in here {
                    let e = used.entry(w).or_default();
                    e.0 += 1;
                    e.1 += n;
                }
                for w in here_called {
                    *called.entry(w).or_default() += 1;
                }
            }
        }
        println!("UN programs {programs}");
        for (w, (p, n)) in &used {
            println!("US {w} {p} {n}");
        }
        for (w, p) in &called {
            if !defined_anywhere.contains(w) {
                println!("UK {w} {p}");
            }
        }
    }

    /// What the corpus scripts use that the host leaves inert, and whether their
    /// widgets export: prints `UA` lines (feature, first value; counts only) and
    /// one `UI` line per program. Shard with `KONTRA_SHARD=i/n`.
    /// Survey aid: scripts with trace markers when KONTRA_TRACE is set.
    struct Marked(crate::script::Scripts);
    impl crate::script::Files for Marked {
        fn script(&self, module: &str) -> Option<String> {
            let text = self.0.script(module)?;
            if let Some(dir) = std::env::var_os("KONTRA_DUMP") {
                let _ = std::fs::write(
                    std::path::Path::new(&dir).join(module.replace('/', "_")),
                    &text,
                );
            }
            if std::env::var_os("KONTRA_TRACE").is_none() {
                return Some(text);
            }
            let mut out = String::new();
            for line in text.lines() {
                let t = line.trim_start();
                let tag = if t.starts_with("function theOnNote") {
                    "A"
                } else if t.starts_with("if enote >= minNote") {
                    "B"
                } else if t.starts_with("local isLegato = false") {
                    "D"
                } else if t.starts_with("if ccVel > 0 then") {
                    "E"
                } else if t.starts_with("function startNote(") {
                    "S"
                } else if t.starts_with("function onNote") {
                    "N"
                } else if t.starts_with("ids[enote] = playNote") {
                    "P"
                } else {
                    ""
                };
                if tag.is_empty() {
                    out.push_str(line);
                    out.push('\n');
                    continue;
                }
                if t.starts_with("function") {
                    out.push_str(line);
                    out.push_str(&format!(" t_{tag} = (t_{tag} or 0) + 1\n"));
                } else {
                    out.push_str(&format!("t_{tag} = (t_{tag} or 0) + 1\n{line}\n"));
                }
            }
            Some(out)
        }
    }

    #[test]
    #[ignore]
    #[cfg(feature = "library-access")]
    fn census_script_api() {
        let (shard, shards): (usize, usize) = std::env::var("KONTRA_SHARD")
            .ok()
            .and_then(|v| {
                let (a, b) = v.split_once('/')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            })
            .unwrap_or((0, 1));
        let mut index = 0;
        let mut files = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(
            std::env::var("KONTRA_UVI_LIBRARIES").unwrap(),
        )];
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
        for f in files {
            if std::env::var("KONTRA_ONLY").is_ok_and(|o| {
                o.split_once("::")
                    .is_some_and(|(file, _)| !f.to_string_lossy().contains(file))
            }) {
                continue;
            }
            let Ok(bank) = crate::Bank::open(&f) else {
                continue;
            };
            let scripts = bank.scripts();
            for program in bank.programs() {
                index += 1;
                if index % shards != shard {
                    continue;
                }
                if std::env::var("KONTRA_ONLY").is_ok_and(|o| {
                    let (file, member) = o.split_once("::").unwrap_or(("", &o));
                    !(f.to_string_lossy().contains(file)
                        && format!("{}::{program}", f.display()).contains(member))
                }) {
                    continue;
                }
                let Ok((text, _)) = bank.program(&program) else {
                    continue;
                };
                let name = format!("{}::{program}", f.display());
                let mut host = match crate::script::ScriptHost::new(
                    &text,
                    Marked(scripts.clone()),
                    crate::script::Config::default(),
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        println!(
                            "UL compile-fail {} {name}",
                            e.chars().take(90).collect::<String>().replace(' ', "_")
                        );
                        continue;
                    }
                };
                host.note_on(1, 60, 100, 0);
                host.advance(1000.0);
                host.note_off(1, 60, 64, 0);
                host.advance(2000.0);
                for f in host.findings() {
                    println!(
                        "UA {}|{}|{}",
                        f.feature,
                        f.value
                            .chars()
                            .take(200)
                            .collect::<String>()
                            .replace(' ', "_"),
                        name
                    );
                }
                if std::env::var_os("KONTRA_TRACE").is_some() {
                    host.take_commands();
                    host.controller(1, 100, 0);
                    host.advance(500.0);
                    let mut clock = 500.0;
                    for key in [24u8, 36, 48, 59, 60, 72, 84] {
                        host.note_on(
                            1000 + u64::from(key),
                            key,
                            std::env::var("KONTRA_VEL")
                                .ok()
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(100),
                            0,
                        );
                        clock += 2000.0;
                        host.advance(clock);
                        let c = host.take_commands();
                        let plays = c
                            .iter()
                            .filter(|c| matches!(c, crate::script::Command::Play(_)))
                            .count();
                        let globals = [
                            "t_P",
                            "t_A",
                            "t_B",
                            "t_D",
                            "t_E",
                            "t_S",
                            "t_N",
                            "latestNoteIdIncr",
                            "lastNote",
                            "lastKeyboardNote",
                            "tuneOutAttackValueNote",
                            "lastVelocityAnyNote",
                            "MIDItransposeValue",
                            "KEYSWtransposeValue",
                            "hornModel",
                            "ccVel",
                            "minNote",
                            "maxNote",
                            "windCCValue",
                            "isNoteOn",
                        ]
                        .iter()
                        .map(|g| format!("{g}={}", host.global_text(g)))
                        .collect::<Vec<_>>()
                        .join(" ");
                        println!(
                            "UT key {key}: {} commands, {plays} plays; {globals}",
                            c.len()
                        );
                        for command in c
                            .iter()
                            .filter(|c| {
                                matches!(
                                    c,
                                    crate::script::Command::Play(_)
                                        | crate::script::Command::Parameter { .. }
                                )
                            })
                            .take(8)
                        {
                            println!(
                                "UT   {}",
                                format!("{command:?}").chars().take(160).collect::<String>()
                            );
                        }
                        host.note_off(1000 + u64::from(key), key, 64, 0);
                        clock += 4000.0;
                        host.advance(clock);
                        host.take_commands();
                    }
                    for f in host.findings() {
                        println!(
                            "UF {}|{}",
                            f.feature,
                            f.value
                                .chars()
                                .take(300)
                                .collect::<String>()
                                .replace(' ', "_")
                        );
                    }
                }
                if std::env::var_os("KONTRA_INSERTS").is_some() {
                    // survey aid: the inserts of the program with their main values
                    let doc = roxmltree::Document::parse(&text).unwrap();
                    for n in doc
                        .descendants()
                        .filter(|n| n.parent().is_some_and(|p| p.has_tag_name("Inserts")))
                    {
                        let keep = [
                            "Name",
                            "Bypass",
                            "SamplePath",
                            "Dry",
                            "Wet",
                            "Gain_1_1",
                            "Gain_1_2",
                            "Gain_2_1",
                            "Gain_2_2",
                            "Time",
                            "Freq",
                            "Mode",
                            "Volume",
                            "OverallGain",
                            "Gain",
                        ];
                        let attrs: Vec<String> = n
                            .attributes()
                            .filter(|a| keep.contains(&a.name()))
                            .map(|a| format!("{}={}", a.name(), a.value()))
                            .collect();
                        println!("UN {} {}", n.tag_name().name(), attrs.join(" "));
                    }
                }
                if let Ok(specs) = std::env::var("KONTRA_LINE") {
                    // transient debugging aid, prints to the terminal only
                    for spec in specs.split(',') {
                        if spec == "ls" {
                            for n in scripts.names() {
                                println!("UX ls {n}");
                            }
                            continue;
                        }
                        let Some((m, l)) = spec.split_once(':') else {
                            continue;
                        };
                        use crate::script::Files;
                        let src = if m == "main" {
                            text.clone()
                        } else {
                            scripts.script(m).unwrap_or_default()
                        };
                        if let Some(pattern) = l.strip_prefix('~') {
                            for (i, line) in src.lines().enumerate() {
                                if line.contains(pattern) {
                                    println!("UX {m}:{}: {}", i + 1, line);
                                }
                            }
                            continue;
                        }
                        let (l, to) = match l.split_once('-') {
                            Some((a, b)) => {
                                (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap())
                            }
                            None => (l.parse::<usize>().unwrap(), l.parse::<usize>().unwrap() - 2),
                        };
                        for (i, line) in src.lines().enumerate() {
                            if (to < l && i + 3 >= l && i < l + 1)
                                || (to >= l && i + 1 >= l && i < to)
                            {
                                println!("UX {m}:{}: {}", i + 1, line);
                            }
                        }
                    }
                }
                let ui = host.interface();
                println!("UI {} {} {}", ui.widgets.len(), ui.unsupported.len(), name);
                for u in &ui.unsupported {
                    println!(
                        "UU {}|{}",
                        u.feature,
                        u.value
                            .chars()
                            .take(40)
                            .collect::<String>()
                            .replace(' ', "_")
                    );
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
            let Ok(bank) = crate::Bank::open(&f) else {
                continue;
            };
            for program in bank.programs() {
                index += 1;
                if index % shards != shard {
                    continue;
                }
                if std::env::var("KONTRA_ONLY").is_ok_and(|o| {
                    let (file, member) = o.split_once("::").unwrap_or(("", &o));
                    !(f.to_string_lossy().contains(file)
                        && format!("{}::{program}", f.display()).contains(member))
                }) {
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
        let Ok((text, _)) = bank.program(program) else {
            return "open-fail -".into();
        };
        let Ok((ir, _)) = crate::translate_bank(&text) else {
            return "translate-fail -".into();
        };
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
        let widest = ir
            .zones
            .iter()
            .max_by_key(|z| z.keys.high - z.keys.low)
            .unwrap();
        let mut candidates = vec![
            mids[mids.len() / 2],
            60.clamp(lo, hi),
            ((u16::from(widest.keys.low) + u16::from(widest.keys.high)) / 2) as u8,
        ];
        candidates.dedup();
        let mut last = String::new();
        for key in candidates {
            last = census_play(bank, program, key, false);
            if last.starts_with("silent") {
                // Expressive instruments wait for the mod wheel.
                last = census_play(bank, program, key, true);
            }
            if last.starts_with("sounds") {
                break;
            }
        }
        last
    }

    #[cfg(feature = "library-access")]
    fn census_play(bank: &crate::Bank, program: &str, key: u8, cc1: bool) -> String {
        use sampler_core::Limits;
        let program =
            match crate::load_program_scripted_streamed(bank, program, 48_000, &Default::default())
            {
                Ok(p) => p,
                Err(e) => {
                    return format!(
                        "load-fail {}",
                        e.to_string()
                            .chars()
                            .take(160)
                            .collect::<String>()
                            .replace(' ', "_")
                    );
                }
            };
        if std::env::var_os("KONTRA_CHAINS").is_some() {
            // survey aid: the chain of the zone covering `key`, and the buses' and groups' chains
            let ins = &program.instrument;
            let show = |label: &str, c: Option<sampler_ir::ChainRef>| {
                if let Some(c) = c {
                    let ch = &ins.chains[c.0];
                    println!(
                        "UK {label} {:?} pre={} post={}",
                        ch.scope,
                        format!("{:?}", ch.pre_amplitude)
                            .chars()
                            .take(400)
                            .collect::<String>(),
                        format!("{:?}", ch.post_amplitude)
                            .chars()
                            .take(400)
                            .collect::<String>()
                    );
                }
            };
            for z in ins
                .zones
                .iter()
                .filter(|z| z.keys.low <= key && key <= z.keys.high)
                .take(2)
            {
                show("zone", z.chain);
            }
            for g in &ins.groups {
                show(&format!("group {}", g.name), g.chain);
            }
            for b in &ins.buses {
                show(&format!("bus {}", b.name), b.chain);
            }
            println!(
                "UK zones={} chains={} impulses={:?}",
                ins.zones.len(),
                ins.chains.len(),
                ins.impulses
                    .iter()
                    .map(|i| (i.rate, i.left.len()))
                    .collect::<Vec<_>>()
            );
        }
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
            .map(|u| {
                u.value
                    .chars()
                    .take(70)
                    .collect::<String>()
                    .replace(' ', "_")
            })
            .unwrap_or_else(|| "-".into());
        let scripted = program.host.handles_notes();
        let limits = Limits {
            notes: 64,
            channels: 16,
            performances: 1,
            expressions: 64,
            families: 64,
            decisions: 256,
            voices: 512,
            commands: 256,
            behaviors: 16,
            behavior_fuel: 1 << 20,
            behavior_cells: 0,
            note_cells: 0,
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
        if cc1 {
            player.input(crate::scripted::HostInput::Controller {
                cc: 1,
                value: 100,
                channel: 0,
            });
        }
        let result = Ok::<(), sampler_core::Error>(())
            .and_then(|()| if cc1 { run(&mut player, 20) } else { Ok(()) })
            .and_then(|_| player.note_on(key, 100.0 / 127.0))
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
            Some(e) => e
                .to_string()
                .chars()
                .take(60)
                .collect::<String>()
                .replace(' ', "_"),
            None => first,
        };
        let state = if cc1 && state == "sounds" {
            "sounds-with-cc1"
        } else {
            state
        };
        format!(
            "{state} peak={peak:.3} key={key} lua={} errs={errors} {first}",
            u8::from(scripted)
        )
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

#[cfg(test)]
mod initial_playback_admission_tests {
    use super::*;
    fn instrument()->ir::Instrument {
        ir::Instrument {assets:vec![ir::Asset {location:ir::AssetLocation::Path("not-opened.wav".into()),
            residency:Default::default(),encoding:ir::Encoding::Wav,root_key:None,loops:vec![]}],
            zones:vec![ir::Zone::new(ir::AssetRef(0))],..Default::default()}
    }
    #[test]
    fn invalid_markers_assets_and_dimensions_reject_before_effect_preparation() {
        let mut value=instrument();value.source_indices.file_playback_zones=vec![ir::ZoneRef(99)];
        assert!(initial_playback_assets(&value,1).is_err());
        value.source_indices.file_playback_zones=vec![ir::ZoneRef(0)];value.zones[0].asset=Some(ir::AssetRef(99));
        assert!(initial_playback_assets(&value,1).is_err());
        value.zones[0].asset=Some(ir::AssetRef(0));assert!(initial_playback_assets(&value,0).is_err());
        value.source_indices.file_playback_zones.push(ir::ZoneRef(0));assert!(initial_playback_assets(&value,1).is_err());
        value.source_indices.file_playback_zones=vec![ir::ZoneRef(0)];
        assert!(apply_initial_playbacks(&mut value,&[Ok(None)],&[true]).is_err());
    }
}
