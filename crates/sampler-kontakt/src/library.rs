//! Real Kontakt instruments (NKI, plain or encrypted) to the semantic IR,
//! using the vendored `ni-file` decoders the v1 importer is built on. The
//! translator states only what it decodes; everything else it finds is listed
//! in [`ir::Instrument::unsupported`] with its source location.

use crate::{LoadError, Samples};
use ni_file::kontakt::{
    StructuredObject,
    objects::{
        BParScript, ExternalModArray32, FNTableImpl, FileNameListPreK51, Group, GroupList,
        InternalModArray16, LoopArray, ModSource, Modulator, Program, VoiceGroups, Zone,
    },
};
use sampler_ir as ir;
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const PROGRAM: u16 = 0x28;
const GROUP_LIST: u16 = 0x33;
const ZONE_LIST: u16 = 0x34;
const LOOPS: u16 = 0x39;
const INTERNAL_MODS: u16 = 0x3b;
const EXTERNAL_MODS: u16 = 0x3c;
const FILE_TABLE: u16 = 0x4b;
const LEGACY_FILE_TABLE: u16 = 0x3d;
const SCRIPT: u16 = 6;

/// A translated instrument and the resolver of the samples its assets name.
pub struct Kontakt {
    pub instrument: ir::Instrument,
    /// Each asset's resolved location, in asset order.
    pub locations: Vec<PathBuf>,
    pub samples: Samples,
    pub(crate) initialized: Option<crate::load::ScriptInit>,
}

/// Translate the NKI at `path`. Zones whose sample is missing are left out
/// and reported; a malformed container or zone table is an error.
pub fn read(path: &Path) -> Result<Kontakt, LoadError> {
    read_overlaid(path, None)
}

/// [`read`] with a snapshot's saved native and script state applied.
pub fn read_with_snapshot(
    path: &Path,
    snapshot: &crate::SnapshotState,
) -> Result<Kontakt, LoadError> {
    let mut kontakt = read_overlaid(path, Some(snapshot))?;
    crate::apply_snapshot(&mut kontakt, snapshot).map_err(|e| LoadError::Invalid {
        path: path.into(),
        reason: format!("snapshot persistent values: {:?} at {}", e.kind, e.offset),
    })?;
    Ok(kontakt)
}

fn read_overlaid(
    path: &Path,
    snapshot: Option<&crate::SnapshotState>,
) -> Result<Kontakt, LoadError> {
    let path = path.canonicalize().map_err(|e| LoadError::io(path, e))?;
    let chunks = crate::read_chunks(&path).map_err(|e| e.at(crate::Stage::Container))?;
    let span = crate::audit::Span::new("ni_objects_parse");
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error).at(crate::Stage::Parse);
    let program = Program::try_from(
        chunks
            .find_first(PROGRAM)
            .ok_or_else(|| invalid("not a single-instrument preset"))?,
    )
    .map_err(|e| decode("program", e))?;
    let (table, others) = match chunks.find_first(FILE_TABLE) {
        Some(chunk) => {
            let t = FNTableImpl::try_from(chunk).map_err(|e| decode("sample file table", e))?;
            (t.sample_filetable, t.other_filetable)
        }
        None => {
            let chunk = chunks
                .find_first(LEGACY_FILE_TABLE)
                .ok_or_else(|| invalid("missing sample file table"))?;
            let t =
                FileNameListPreK51::try_from(chunk).map_err(|e| decode("legacy file table", e))?;
            (t.sample_filetable, t.other_filetable)
        }
    };
    drop(span);
    let _span = crate::audit::Span::new("translate_resolve_ir");
    translate(path, program, table, others, snapshot).map_err(|e| e.at(crate::Stage::Translate))
}

/// Translate program `index` (0-based, in slot order) of the multi at `path`;
/// the programs of a `.nkm` are the instruments of a rack.
pub fn read_program(path: &Path, index: usize) -> Result<Kontakt, LoadError> {
    use ni_file::kontakt::objects::Bank;
    let path = path.canonicalize().map_err(|e| LoadError::io(path, e))?;
    let chunks = crate::read_chunks(&path).map_err(|e| e.at(crate::Stage::Container))?;
    let span = crate::audit::Span::new("ni_objects_parse");
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error).at(crate::Stage::Parse);
    let bank = Bank::try_from(
        chunks
            .find_first(3)
            .ok_or_else(|| invalid("missing multi bank"))?,
    )
    .map_err(|e| decode("multi bank", e))?;
    let mut slots: Vec<_> = bank
        .slot_list()
        .map_err(|e| decode("multi slots", e))?
        .slots
        .into_iter()
        .collect();
    slots.sort_by_key(|(slot, _)| *slot);
    let mut programs = Vec::new();
    for (_, container) in slots {
        programs.extend(
            container
                .program_list()
                .map_err(|e| decode("multi programs", e))?
                .programs,
        );
    }
    let program = programs
        .into_iter()
        .nth(index)
        .ok_or_else(|| invalid("the multi has no such program"))?;
    let (table, others) = match chunks
        .filename_tables()
        .map_err(|e| decode("multi file table", e))?
    {
        Some(t) => (t.sample_filetable, t.other_filetable),
        None => (
            chunks
                .filename_table()
                .ok_or_else(|| invalid("missing multi sample table"))?
                .map_err(|e| decode("multi file table", e))?,
            Default::default(),
        ),
    };
    drop(span);
    let _span = crate::audit::Span::new("translate_resolve_ir");
    translate(path, program, table, others, None).map_err(|e| e.at(crate::Stage::Translate))
}

fn translate(
    path: PathBuf,
    mut program: Program,
    table: HashMap<u32, String>,
    others: HashMap<u32, String>,
    snapshot: Option<&crate::SnapshotState>,
) -> Result<Kontakt, LoadError> {
    if let Some(snapshot) = snapshot {
        // The snapshot's racks and buses are the program's own, in the same order.
        let mut saved = snapshot.effects.iter();
        for kind in [crate::effects::RACK, crate::effects::BUS] {
            let mut theirs = saved.clone().filter(|(id, _)| *id == kind);
            for child in program.0.children.iter_mut().filter(|c| c.id == kind) {
                if let Some((_, data)) = theirs.next() {
                    child.data = data.clone();
                }
            }
        }
        let _ = saved.next();
    }
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error).at(crate::Stage::Parse);
    let params = program
        .params()
        .map_err(|e| decode("program parameters", e))?;
    let mut out = Translation {
        ir: ir::Instrument {
            name: params.name.clone(),
            default_keyswitch: u8::try_from(params.default_key_switch)
                .ok()
                .filter(|&key| key <= 127),
            source: ir::SourceFormat::Kontakt {
                version: program.version(),
            },
            kontakt_objects: Some(Box::new(ir::kontakt::Objects {
                program: crate::objects::program(program.version(), &params),
                voice_groups: None,
                groups: Vec::new(),
                zones: Vec::new(),
            })),
            host_volume: Some(ir::HostVolume {
                controller: 7,
                saved: f64::from(params.volume),
            }),
            ..Default::default()
        },
        assets: HashMap::new(),
        locations: Vec::new(),
        start_criteria: Vec::new(),
        voice_groups: Vec::new(),
        snapshot_groups: snapshot.map(|s| s.groups.clone()).unwrap_or_default(),
        engine: Vec::new(),
        dynamic: false,
        eq_mod_slots: HashMap::new(),
        send_taps: Vec::new(),
        #[cfg(feature="scan")]
        target_outcomes: HashMap::new(),
    };
    match crate::program_automation(&program.0.private_data, program.version(), crate::Limits { bytes: 64 << 20, records: 65536 }) {
        Ok(records) => for record in records {
            let target = std::str::from_utf8(record.tag.data()).ok().and_then(|tag| {
                let rest = tag.strip_prefix("pts_script_slider_")?;
                let (slot, ordinal) = rest.split_once('_')?;
                Some((slot.parse::<u8>().ok()?, ordinal.parse::<u32>().ok()?))
            });
            let source = match record.mode {
                1 if record.address < 128 => Some(ir::ScriptAutomationSource::Controller(record.address as u8)),
                2 => Some(ir::ScriptAutomationSource::HostParameter(record.address)),
                _ => None,
            };
            if let (Some((source_slot, slider)), Some(source)) = (target, source)
                && (0.0..=1.0).contains(&record.low) && (0.0..=1.0).contains(&record.high)
            {
                out.ir.script_automation.push(ir::ScriptAutomation { source, source_slot, slider,
                    low: f64::from(record.low), high: f64::from(record.high), soft_takeover: record.soft_takeover });
            } else {
                out.unsupported("program private", "saved automation target", format!("record {} mode {} address {}", record.offset, record.mode, record.address), ir::Reason::NotModeled);
            }
        },
        Err(error) => out.unsupported("program private", "saved automation layout", error, ir::Reason::NotModeled),
    }
    if let Some(chunk) = program.0.find_first(VOICE_GROUPS) {
        out.voice_groups(&chunk.data)
            .map_err(|e| decode("voice groups", e))?;
    }
    let groups = GroupList::try_from(
        program
            .0
            .find_first(GROUP_LIST)
            .ok_or_else(|| invalid("missing group list"))?,
    )
    .map_err(|e| decode("group list", e))?;
    let mut resources = None;
    for (slot, chunk) in program
        .0
        .children
        .iter()
        .filter(|c| c.id == SCRIPT)
        .enumerate()
    {
        let mut script = BParScript::try_from(chunk)
            .and_then(|s| s.params())
            .map_err(|e| decode("script", e))?;
        if !script.bypass {
            if let Some(link) = script
                .textfile_name
                .as_deref()
                .filter(|n| !n.trim().is_empty())
            {
                if let Some(text) = resources
                    .get_or_insert_with(|| crate::Resources::of(&path))
                    .script(link)
                {
                    script.text = Some(text);
                }
            }
        }
        let location = format!("script slot {slot}");
        match script.text {
            _ if script.bypass => {}
            Some(text) if !text.trim().is_empty() => {
                let state = saved(&script.persistent).map_err(|e| {
                    invalid(&format!(
                        "script persistent values: {:?} at {}",
                        e.kind, e.offset
                    ))
                })?;
                out.ir.behaviors.push(ir::Behavior {
                    name: script
                        .description
                        .filter(|d| !d.is_empty())
                        .unwrap_or(location),
                    language: ir::Language::Ksp,
                    source: text,
                    slot: Some(slot.min(usize::from(u8::MAX)) as u8),
                    state,
                    requires: Vec::new(),
                });
            }
            // Neither text nor a linked file: an empty slot.
            _ => {
                if let Some(name) = script.textfile_name.filter(|name| !name.is_empty()) {
                    out.unsupported(
                        &location,
                        "linked script file",
                        name,
                        ir::Reason::NotModeled,
                    );
                }
            }
        }
    }
    // Lookup metadata precedes script init and never depends on DSP admission.
    for (index, group) in groups.groups.iter().enumerate() {
        out.source_modulators(index, group).map_err(|e| decode("source modulation names", e))?;
    }
    // Scripts first: what `on init` writes with set_engine_par (effect gains,
    // bypass) is the rack's state, so racks are translated after it.
    let group_names: Vec<String> = groups
        .groups
        .iter()
        .map(|g| g.params().map(|p| p.name))
        .collect::<Result<_,_>>()
        .map_err(|e| decode("source group names",e))?;
    if let Some(snapshot) = snapshot {
        for behavior in &mut out.ir.behaviors {
            if let Some(entries) = behavior
                .slot
                .and_then(|slot| snapshot.persistent.get(usize::from(slot)))
            {
                for (name, value) in saved(entries).map_err(|e| invalid(&format!("snapshot persistent values: {:?} at {}", e.kind, e.offset)))? {
                    match behavior.state.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = value,
                        None => behavior.state.push((name, value)),
                    }
                }
            }
        }
    }
    let span = crate::audit::Span::new("translate_ksp_init");
    let initialized = crate::load::initialize_scripts(&mut out.ir, Some(&path), group_names, &[]);
    out.engine = initialized
        .states
        .iter()
        .flatten()
        .filter_map(|s| s.as_ref().ok())
        .flat_map(|s| s.engine_pars())
        .collect();
    let dynamic = initialized
        .states
        .iter()
        .flatten()
        .filter_map(|s| s.as_ref().ok())
        .any(|s| s.writes_effect_slots());
    out.dynamic = dynamic;
    drop(span);
    let mut translated = Vec::new();
    for (index, group) in groups.groups.iter().enumerate() {
        let runtime = ir::GroupRef(out.ir.groups.len());
        translated.push(out.group(index, group).map_err(|e| decode("group", e))?);
        out.ir.source_indices.groups.push(Some(runtime));
    }
    for (runtime, behavior) in out.ir.behaviors.iter().enumerate() {
        if let Some(slot) = behavior.slot {
            out.ir.source_indices.slots.resize(
                out.ir.source_indices.slots.len().max(usize::from(slot) + 1),
                None,
            );
            out.ir.source_indices.slots[usize::from(slot)] = Some(runtime);
        }
    }
    let span = crate::audit::Span::new("translate_resource_ir_dsp");
    let parent = path
        .parent()
        .ok_or_else(|| invalid("instrument has no folder"))?;
    let root = path
        .ancestors()
        .find(|p| p.join("Samples").is_dir())
        .unwrap_or(parent);
    let mut samples = Samples::new(root);
    let mut rack_errors = Vec::new();
    let racks = crate::effects::program_racks(&program, &out.engine, |at, error| {
        rack_errors.push((at, error));
    });
    for (at, error) in rack_errors {
        out.unsupported(&at, "effect decoding", error, ir::Reason::Unknown);
    }
    let routes: Vec<_> = translated
        .iter()
        .flatten()
        .filter_map(|g| g.bus.map(|bus| (g.group, bus)))
        .collect();
    let mut buses = crate::effects::bus_plans(&program, &routes);
    for bus in &mut buses {
        if let Some(volume) = out.script_bus_volume(bus.index) {
            bus.volume = volume;
        }
    }
    {
        // Convolution impulse responses are named by the other-files table.
        let mut load = |index: i32| -> Result<crate::effects::Decoded, String> {
            let name = u32::try_from(index)
                .ok()
                .and_then(|i| others.get(&i))
                .ok_or_else(|| format!("index {index} is not in the file table"))?;
            let at = samples
                .resolve(parent, name)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("{name} was not found"))?;
            let decoded = samples.decode(&at).map_err(|e| e.to_string())?;
            Ok((decoded.rate, decoded.frames))
        };
        let (notes, send_buses) =
            crate::effects::instrument_buses(&mut out.ir, &racks, &buses, dynamic, &mut load);
        for (at, (slot, feature, value, reason)) in notes {
            out.unsupported(&format!("{at} slot {slot}"), &feature, value, reason);
        }
        out.resolve_send_taps(&send_buses);
    }
    drop(span);
    let span = crate::audit::Span::new("translate_zones_sample_resolve");
    // Racks of buses no group feeds do nothing, so they are not reported.
    let mut resolved = HashMap::new();
    let data = &program
        .0
        .find_first(ZONE_LIST)
        .ok_or_else(|| invalid("missing zone list"))?
        .data;
    let mut r = Cursor::new(data.as_slice());
    let count = u32le(&mut r).map_err(|e| LoadError::io(&path, e))? as usize;
    if count > data.len() / 8 {
        return Err(invalid("zone count exceeds the zone list"));
    }
    out.ir.source_indices.zones.resize(count, None);
    for index in 0..count {
        let zone =
            raw_zone(&mut r).map_err(|reason| invalid(&format!("zone {index}: {reason}")))?;
        if let Some(objects) = &mut out.ir.kontakt_objects {
            objects.zones.push(zone.source.clone());
        }
        let Some(group) = translated.get(zone.group).ok_or_else(|| {
            invalid(&format!(
                "zone {index} refers to missing group {}",
                zone.group
            ))
        })?
        else {
            continue; // Muted group: never sounds.
        };
        if !zone.sample_present { continue; } // Authored empty zone, no sample to resolve.
        let name = table.get(&(zone.file as u32)).ok_or_else(|| {
            invalid(&format!(
                "zone {index} sample is absent from the file table"
            ))
        })?;
        let location = match resolved.get(name) {
            Some(location) => location,
            None => resolved
                .entry(name.clone())
                .or_insert(samples.resolve(parent, name)?),
        };
        let Some(location) = location else {
            out.unsupported(
                &format!("zone {index}"),
                "missing sample",
                name,
                ir::Reason::InvalidValue,
            );
            continue;
        };
        // Kontakt stores the end relative to the end of the sample.
        let end = match zone.end {
            0 => None,
            end => Some(
                samples
                    .frames(location)?
                    .saturating_sub(u64::from(end.unsigned_abs())),
            ),
        };
        let before = out.ir.zones.len();
        out.zone(index, zone, end, group, &params, location.clone());
        if out.ir.zones.len() > before {
            out.ir.source_indices.zones[index] = Some(ir::ZoneRef(before));
        }
    }
    drop(span);
    let _span = crate::audit::Span::new("translate_keys_validate");
    crate::keyswitch::translate(&mut out.ir, &out.start_criteria);
    out.ir.unsupported.dedup();
    #[cfg(feature = "scan")]
    { out.ir.dsp_slots = crate::coverage::slots(&program, &out.ir, dynamic, &out.engine, &out.target_outcomes).ok(); out.ir.native_start_mod_groups = crate::coverage::start_mod_groups(&program).ok(); }
    out.ir.validate().map_err(|e| invalid(&e.to_string()))?;
    Ok(Kontakt {
        instrument: out.ir,
        locations: out.locations,
        samples,
        initialized: Some(initialized),
    })
}

/// Group settings every zone of the group inherits.
struct GroupInfo {
    index: usize,
    group: ir::GroupRef,
    release: bool,
    tracking: bool,
    reverse: bool,
    envelope: Option<ir::ModulatorRef>,
    velocity: ir::VelocityResponse,
    /// Modulation routes every zone of the group carries.
    routes: Vec<ir::RouteRef>,
    /// Its insert rack as a voice chain.
    chain: Option<ir::ChainRef>,
    /// The instrument bus it is routed to.
    bus: Option<u8>,
}

struct Translation {
    ir: ir::Instrument,
    assets: HashMap<PathBuf, ir::AssetRef>,
    locations: Vec<PathBuf>,
    start_criteria: Vec<(
        String,
        ir::GroupRef,
        ni_file::kontakt::objects::StartCriteriaList,
    )>,
    /// Kontakt voice group index -> `ir.voice_limits` index.
    voice_groups: Vec<Option<usize>>,
    /// A snapshot's saved state per group, applied over the program's.
    snapshot_groups: Vec<crate::GroupState>,
    /// What the scripts' `on init` wrote with `set_engine_par`.
    engine: Vec<sampler_ksp::EnginePar>,
    /// A script writes effect slots while playing.
    dynamic: bool,
    eq_mod_slots: HashMap<usize, Vec<usize>>,
    send_taps: Vec<(ir::ChainRef, crate::effects::SendTap)>,
    #[cfg(feature="scan")]
    target_outcomes: crate::coverage::Targets,
}

fn eq_knob(name: &str) -> Option<(usize, ir::ProcessorParameter)> {
    for (prefix, parameter) in [("eqGain", ir::ProcessorParameter::Gain),
        ("eqFreq", ir::ProcessorParameter::Cutoff), ("eqBandwidth", ir::ProcessorParameter::Resonance)] {
        if let Some(rest) = name.strip_prefix(prefix) {
            let band = rest.parse::<u8>().ok().filter(|b| (1..=3).contains(b))?;
            return Some((usize::from(band - 1), parameter));
        }
    }
    None
}

const VOICE_GROUPS: u16 = 0x32;

/// Translate only established voice-limit semantics; retain signed saved values.
fn voice_limit(
    v: &ni_file::kontakt::objects::VoiceLimit,
) -> Result<ir::VoiceLimit, ni_file::Error> {
    let kill = match v.kill_mode {
        0 => ir::Kill::Any,
        1 => ir::Kill::Oldest,
        2 => ir::Kill::Newest,
        3 => ir::Kill::Highest,
        4 => ir::Kill::Lowest,
        other => return Err(ni_file::Error::Generic(format!("voice kill mode {other}"))),
    };
    Ok(ir::VoiceLimit {
        voices: v.max_num_voices.max(1) as u32,
        kill,
        prefer_released: v.prefer_released,
        fade: ir::Time::Milliseconds(f64::from(v.ms_fade_time.max(0))),
    })
}

/// Kontakt AHDSR stage laws as native `expm1(k·t)/expm1(k)` curves (decoded
/// from Kontakt's engine in v1, `src/engine/ahdsr.rs`, control rate rate/32):
/// decay and release fall geometrically to 3/43 of `1.075` above a `0.075`
/// floor, which is exactly k = ln(3/43); the attack runs a geometric segment
/// with base b = e^((1 − |c|)·ln 500000 − ln 20000) for authored curve c in
/// −1..1: k = ln(b / (1 + b)) for c > 0 (fast start), ln((1 + b) / b) else.
fn ahdsr_curves(curve: f32) -> (ir::Curve, ir::Curve) {
    let c = f64::from(curve.clamp(-1.0, 1.0));
    // The engine rounds the base to f32 before its power.
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let attack = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    (
        ir::Curve::Exponential(attack),
        ir::Curve::Exponential((3.0f64 / 43.0).ln()),
    )
}

impl Translation {
    fn resolve_send_taps(&mut self, buses: &[(usize, ir::BusRef)]) {
        for (chain, tap) in std::mem::take(&mut self.send_taps) {
            for (send, level) in tap.levels.into_iter().enumerate() {
                if !level.is_finite() || level < 0.0 {
                    self.unsupported(&format!("voice chain {} slot {}", chain.0, tap.slot),
                        "send level", send.to_string(), ir::Reason::InvalidValue);
                    continue;
                }
                let Some(&(_, bus)) = buses.iter().find(|(slot, _)| *slot == send) else {
                    if level != 0.0 && !tap.bypass {
                        self.unsupported(&format!("voice chain {} slot {}", chain.0, tap.slot),
                            "send return", send.to_string(), ir::Reason::NotModeled);
                    }
                    continue;
                };
                self.ir.voice_send_taps.push(ir::VoiceSendTap {
                    chain, position: tap.position, bus,
                    gain: ir::Gain::Linear(f64::from(level)), bypass: tap.bypass,
                    gain_control: None, bypass_control: None, ramp: ir::Time::Seconds(0.0),
                });
            }
        }
    }

    /// The `VoiceGroups` chunk: the instrument's voice limit, a 128-bit set of
    /// defined voice groups, then one voice limit per defined group.
    fn voice_groups(&mut self, data: &[u8]) -> Result<(), ni_file::Error> {
        let mut reader = Cursor::new(data);
        let groups = VoiceGroups::read(&mut reader)?;
        if reader.position() != data.len() as u64 {
            return Err(ni_file::Error::Static("Trailing VoiceGroups chunk data"));
        }
        self.ir.voice_limit = Some(voice_limit(&groups.voice_limit)?);
        if let Some(objects) = &mut self.ir.kontakt_objects {
            objects.voice_groups = Some(ir::kontakt::VoiceGroups {
                program: crate::objects::voice(&groups.voice_limit),
                groups: groups
                    .groups
                    .iter()
                    .map(|v| v.as_ref().map(|v| crate::objects::voice(&v.voice_limit)))
                    .collect(),
            });
        }
        self.voice_groups = vec![None; 128];
        for (g, group) in groups.groups.iter().enumerate() {
            if let Some(group) = group {
                let limit = voice_limit(&group.voice_limit)?;
                if group.voice_limit.exclusion_group >= 0 {
                    self.unsupported(
                        &format!("voice group {g}"),
                        "voice group exclusion group",
                        group.voice_limit.exclusion_group,
                        ir::Reason::NotModeled,
                    );
                }
                self.ir.voice_limits.push(limit);
                self.voice_groups[g] = Some(self.ir.voice_limits.len() - 1);
            }
        }
        Ok(())
    }
    fn unsupported(
        &mut self,
        location: &str,
        feature: &str,
        value: impl std::fmt::Display,
        reason: ir::Reason,
    ) {
        self.ir.unsupported.push(ir::Unsupported {
            location: location.into(),
            feature: feature.into(),
            value: value.to_string(),
            reason,
        });
    }

    fn source_modulators(&mut self, index: usize, group: &Group) -> Result<(), ni_file::Error> {
        for (external, id) in [(false, INTERNAL_MODS), (true, EXTERNAL_MODS)] {
            if let Some(chunk) = group.0.find_first(id) {
                let names: Vec<_> = if external {
                    ExternalModArray32::try_from(chunk)?
                        .slots()?
                        .into_iter()
                        .map(|(slot, m)| m.params().map(|p| (slot, p.name, p.targets)))
                        .collect::<Result<_, _>>()?
                } else {
                    let mut names = Vec::new();
                    for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
                        let params = modulator.params()?;
                        if let Modulator::Ahdsr(envelope) = &params.modulator {
                            Self::source_envelope(&mut self.ir.source_indices, index, slot, envelope);
                        }
                        names.push((slot, params.name, params.targets));
                    }
                    names
                };
                for (slot, name, targets) in names {
                    for target in &targets {
                        if eq_knob(&target.param).is_some() && let Some(slot) = target.slot {
                            let live = self.eq_mod_slots.entry(index).or_default();
                            if !live.contains(&usize::from(slot)) { live.push(usize::from(slot)); }
                        }
                    }
                    self.source_modulator(index, slot, external, name, &targets);
                }
            }
        }
        let saved = self.snapshot_groups.get(index);
        let rack = match saved {
            Some(state) => Ok(ni_file::kontakt::objects::BParamArrayBParFX8 {
                version: state.fx.0,
                items: state.fx.1.iter().map(|slot| slot.as_ref().map(|(id, data)| ni_file::kontakt::Chunk { id: *id, data: data.clone() })).collect(),
            }),
            None => group.insert_fx(),
        };
        if let Ok(rack) = rack {
            let slots = crate::effects::rack(&rack, |_, _| {});
            self.source_eq_values(index, &slots);
        }
        Ok(())
    }

    fn source_eq_values(&mut self, group: usize, slots: &[crate::effects::Slot]) {
        for slot in slots {
            if let Some(crate::effects::Params::Eq { bands }) = slot.params() {
                for (band, [hz, width, db]) in bands.iter().enumerate() {
                    for (name, value, low, high) in [
                        ("GAIN", f64::from(*db), -18., 18.),
                        ("FREQ", (f64::from(*hz) / 20.).log10() / 3., 0., 1.),
                        ("BW", (f64::from(*width) - 0.3) / 2.7, 0., 1.),
                    ] {
                        if !value.is_finite() { continue; }
                        let parameter = sampler_core::engine_parameter_id(&format!("ENGINE_PAR_{name}{}", band + 1)).unwrap();
                        self.ir.source_indices.engine_values.push(ir::SourceEngineValue {
                            parameter, group: group as i32, slot: slot.slot as i32, generic: -1,
                            value: sampler_core::EngineParameterLaw::Linear { low, high }.encode(value),
                        });
                    }
                }
            }
        }
    }

    fn source_envelope(
        source: &mut ir::SourceIndices,
        group: usize,
        slot: usize,
        envelope: &ni_file::kontakt::objects::EnvelopeAhdsr,
    ) {
        use sampler_core::{EngineParameterLaw, EnvelopeStage};
        let (ir::Curve::Exponential(curve), _) = ahdsr_curves(envelope.attack_curve) else {
            unreachable!()
        };
        for (name, stage, native) in [
            (
                "ENGINE_PAR_ATTACK",
                EnvelopeStage::Attack,
                f64::from(envelope.attack_ms),
            ),
            (
                "ENGINE_PAR_HOLD",
                EnvelopeStage::Hold,
                f64::from(envelope.hold_ms),
            ),
            (
                "ENGINE_PAR_DECAY",
                EnvelopeStage::Decay,
                f64::from(envelope.decay_ms),
            ),
            (
                "ENGINE_PAR_SUSTAIN",
                EnvelopeStage::Sustain,
                f64::from(envelope.sustain),
            ),
            (
                "ENGINE_PAR_RELEASE",
                EnvelopeStage::Release,
                f64::from(envelope.release_ms),
            ),
            ("ENGINE_PAR_ATK_CURVE", EnvelopeStage::AttackCurve, curve),
        ] {
            source.engine_values.push(ir::SourceEngineValue {
                parameter: sampler_core::engine_parameter_id(name).unwrap(),
                group: group as i32,
                slot: slot as i32,
                generic: -1,
                value: EngineParameterLaw::envelope(stage, 1000).encode(native),
            });
        }
    }

    fn source_modulator(
        &mut self, group: usize, slot: usize, external: bool,
        name: String, targets: &[ni_file::kontakt::objects::ModTarget],
    ) {
        self.ir.source_indices.engine_lookups.push(ir::SourceEngineLookup {
            group: group as i32, owner: -1, target: false,
            name: name.clone(), index: slot as i32,
        });
        for (index, target) in targets.iter().enumerate() {
            self.ir.source_indices.engine_lookups.push(ir::SourceEngineLookup {
                group: group as i32, owner: slot as i32, target: true,
                name: target.name.clone(), index: index as i32,
            });
        }
        self.ir.source_indices.modulators.push(ir::SourceModulator {
            group, slot, external, name, runtime: None,
        });
    }

    /// A group's settings, or `None` for a muted group.
    fn group(&mut self, index: usize, group: &Group) -> Result<Option<GroupInfo>, ni_file::Error> {
        let mut v = group.params()?;
        if let Some(objects) = &mut self.ir.kontakt_objects {
            let source = crate::objects::group(group, &v);
            if let Some(error) = &source.source_error {
                self.ir.unsupported.push(ir::Unsupported {
                    location: format!("group {index}"),
                    feature: "source parameters".into(),
                    value: error.clone(),
                    reason: ir::Reason::Unknown,
                });
            }
            objects.groups.push(source);
        }
        let saved = self.snapshot_groups.get(index).cloned();
        if let Some(state) = &saved {
            v.volume = state.volume;
            v.pan = state.pan;
            v.tune = state.octaves.exp2();
            v.key_tracking = state.key_tracking;
            v.reverse = state.reverse;
        }
        let at = format!("group {index} {:?}", v.name);
        if v.muted {
            // Empty source groups retain their numeric address for KSP and DSP writes.
            self.ir.groups.push(ir::Group {
                name: v.name,
                ..Default::default()
            });
            return Ok(None);
        }
        let not_modeled = ir::Reason::NotModeled;
        // 1-based (0 = none): Una Corda's groups use 1 and 2 with voice
        // groups 0 and 1 defined, Afflatus 2 Horns KS 1..=8 with 0..=7.
        let voice_group = usize::try_from(v.voice_group_index - 1).ok();
        let voice_limit = voice_group.and_then(|g| {
            let limit = self.voice_groups.get(g).copied().flatten();
            if limit.is_none() {
                self.unsupported(&at, "undefined voice group", g, ir::Reason::InvalidValue);
            }
            limit
        });
        if v.midi_channel >= 0 {
            self.unsupported(&at, "MIDI channel filter", v.midi_channel, not_modeled);
        }
        // Start options become articulations or reports in `keyswitch::translate`.
        self.start_criteria.push((
            at.clone(),
            ir::GroupRef(self.ir.groups.len()),
            v.start_criteria.clone(),
        ));
        match group.source_identity() {
            // v1 plays every mode but wavetable (9) as a sampler; so does this.
            Ok(source) if source.mode == 9 => {
                self.unsupported(&at, "wavetable source", source.mode, not_modeled)
            }
            Ok(source) if source.mode != 0 => self.unsupported(
                &at,
                "source mode (played as a sampler)",
                source.mode,
                not_modeled,
            ),
            Ok(_) => {}
            Err(error) => self.unsupported(&at, "source module", error, ir::Reason::Unknown),
        }
        let mut chain = None;
        let mut filter_slots = Vec::new();
        let insert = match saved {
            Some(state) => Ok(ni_file::kontakt::objects::BParamArrayBParFX8 {
                version: state.fx.0,
                items: state
                    .fx
                    .1
                    .into_iter()
                    .map(|slot| slot.map(|(id, data)| ni_file::kontakt::Chunk { id, data }))
                    .collect(),
            }),
            None => group.insert_fx(),
        };
        match insert {
            Ok(array) => {
                let mut slots = crate::effects::rack(&array, |slot, error| {
                    self.unsupported(
                        &format!("{at} insert slot {slot}"),
                        "effect decoding",
                        error,
                        ir::Reason::Unknown,
                    )
                });
                crate::effects::apply_writes(&mut slots, &self.engine, index as i32, -1);
                let dynamic = self.dynamic.then_some((index as i32, -1));
                let live_eq = self.eq_mod_slots.get(&index).map(Vec::as_slice).unwrap_or(&[]);
                let (c, boundary) = crate::effects::voice_chain(
                    &slots, v.fx_idx_amp_split_point, dynamic, index as i32, live_eq,
                );
                let mut processors = c.processors;
                filter_slots = c.filter_slots;
                for (slot, feature, value, reason) in c.notes {
                    self.unsupported(&format!("{at} insert slot {slot}"), &feature, value, reason);
                }
                if !processors.is_empty() || !c.send_taps.is_empty() {
                    let post_amplitude = processors.split_off(boundary);
                    self.ir.chains.push(ir::Chain {
                        scope: ir::Scope::Voice,
                        pre_amplitude: processors,
                        post_amplitude,
                    });
                    chain = Some(ir::ChainRef(self.ir.chains.len() - 1));
                    self.eq_controls(index, chain.unwrap(), &filter_slots);
                    self.send_taps.extend(c.send_taps.into_iter().map(|tap| (chain.unwrap(), tap)));
                }
            }
            Err(error) => self.unsupported(
                &format!("{at} insert"),
                "effect decoding",
                error,
                ir::Reason::Unknown,
            ),
        }
        let mut envelope = None;
        let mut flex_release = None;
        let mut routes = Vec::new();
        if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
            for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
                let params = modulator.params()?;
                let at = format!("{at} modulator slot {slot}");
                // [router UI, bypass, retrigger, unknown]
                if params.unknown_flags[1] != 0 || params.targets.is_empty() {
                    continue;
                }
                let retrigger = params.unknown_flags[2] != 0;
                let volume = matches!(params.targets.as_slice(), [t]
                    if t.param == "volume" && t.signed_intensity() == 1.0 && !t.invert
                        && t.slot.is_none() && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled));
                let source = match params.modulator {
                    Modulator::Ahdsr(env) => {
                        let ms = |ms: f32| ir::Time::Milliseconds(f64::from(ms.max(0.0)));
                        let (attack_shape, fall) = ahdsr_curves(env.attack_curve);
                        ir::ModulationSource::Envelope(ir::Envelope {
                            attack: ms(env.attack_ms),
                            hold: ms(env.hold_ms),
                            decay: ms(env.decay_ms),
                            sustain: f64::from(env.sustain.clamp(0.0, 1.0)),
                            release: ms(env.release_ms),
                            attack_shape,
                            decay_shape: fall,
                            release_shape: fall,
                            // The AHD-only switch (v1: `ahd_only = flag != 0`).
                            one_shot: env.unknown_flag != 0,
                            ..Default::default()
                        })
                    }
                    Modulator::Lfo(lfo) => match self.lfo(&at, &lfo, retrigger) {
                        Some(lfo) => ir::ModulationSource::Lfo(lfo),
                        None => continue,
                    },
                    Modulator::Flex(flex) => {
                        // Point times are deltas from the previous point and
                        // levels linear gain (audits/MODULATION.md, medium).
                        // Segment curve s (0..1, 0.5 linear), c = s - 0.5:
                        // measured on Kontakt 8 flex envelopes, k = BOW_K * |c|
                        // with positive c starting fast and negative slowly
                        // (docs/architecture-v2/KONTAKT_REFERENCE.md s.10).
                        let sustain = flex.sustain as usize;
                        if volume {
                            // Held until the release segments end.
                            let after: f32 = flex
                                .points
                                .iter()
                                .skip(sustain + 1)
                                .map(|p| p.time_ms.max(0.0))
                                .sum();
                            flex_release = Some(flex_release.unwrap_or(0.0f32).max(after));
                        }
                        ir::ModulationSource::Breakpoints(ir::Breakpoints {
                            points: flex
                                .points
                                .iter()
                                .map(|p| ir::Breakpoint {
                                    time: ir::Time::Milliseconds(f64::from(p.time_ms.max(0.0))),
                                    level: f64::from(p.level.clamp(0.0, 1.0)),
                                    shape: match f64::from(p.curve) - 0.5 {
                                        c if c.abs() < 1e-4 => ir::Curve::Linear,
                                        c => ir::Curve::Exponential(-BOW_K * c),
                                    },
                                })
                                .collect(),
                            sustain: Some(sustain),
                        })
                    }
                    Modulator::Other { chunk_id } => {
                        self.unsupported(
                            &at,
                            "internal modulator chunk",
                            format!("{chunk_id:#x} {:?}", params.name),
                            ir::Reason::Unknown,
                        );
                        continue;
                    }
                };
                let envelope_source = matches!(source, ir::ModulationSource::Envelope(_));
                self.ir.modulators.push(ir::Modulator {
                    scope: ir::Scope::Voice,
                    source,
                });
                let modulator = ir::ModulatorRef(self.ir.modulators.len() - 1);
                if let Some(address) = self.ir.source_indices.modulators.iter_mut()
                    .find(|m| m.group == index && m.slot == usize::from(slot) && !m.external)
                { address.runtime = Some(modulator); }
                if envelope_source && volume && envelope.is_none() {
                    envelope = Some(modulator);
                    #[cfg(feature="scan")]
                    self.target_outcomes.insert((index,usize::from(slot),false,0),true);
                    continue;
                }
                for (_ordinal,target) in params.targets.iter().enumerate() {
                    let route=self.route(&at,modulator,envelope_source,target,chain.zip(Some(&filter_slots[..])));
                    #[cfg(feature="scan")]
                    self.target_outcomes.insert((index,usize::from(slot),false,_ordinal),route.is_some());
                    routes.extend(route);
                }
            }
        }
        if let (None, Some(release_ms)) = (envelope, flex_release) {
            // A flex volume envelope without an AHDSR plays against a gate
            // that holds through the flex release, then ends the voice.
            self.ir.modulators.push(ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Envelope(ir::Envelope {
                    release: ir::Time::Milliseconds(f64::from(release_ms)),
                    release_shape: ir::Curve::Step,
                    ..Default::default()
                }),
            });
            envelope = Some(ir::ModulatorRef(self.ir.modulators.len() - 1));
        }
        let mut velocity = ir::VelocityResponse::None;
        if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
            for (slot, modulation) in ExternalModArray32::try_from(chunk)?.slots()? {
                let mut params = modulation.params()?;
                if let Some(value) = self.script_intensity(index, &params.name) {
                    // `set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, ...)` on init and
                    // `on persistence_changed`: 0..=1000000 over 0..=1.
                    for target in &mut params.targets {
                        target.intensity = value;
                    }
                }
                let at = format!("{at} external modulation slot {slot}");
                let plain_volume = |t: &ni_file::kontakt::objects::ModTarget| {
                    t.param == "volume"
                        && t.slot.is_none()
                        && !t.invert
                        && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled)
                };
                if let (ModSource::Velocity, [t]) = (&params.source, params.targets.as_slice())
                    && plain_volume(t)
                    && t.signed_intensity() == 1.0
                    && velocity == ir::VelocityResponse::None
                {
                    // gain × velocity: the attenuate law at full intensity,
                    // kept on the voice so no per-voice modulation is needed.
                    velocity = ir::VelocityResponse::Linear;
                    #[cfg(feature="scan")]
                    self.target_outcomes.insert((index,usize::from(slot),true,0),true);
                    continue;
                }
                let source = match params.source {
                    ModSource::Velocity => ir::ModulationSource::Velocity,
                    ModSource::KeyPosition => ir::ModulationSource::Key,
                    ModSource::MidiCc(cc) if cc < 128 => ir::ModulationSource::Controller(cc),
                    ModSource::PitchBend => ir::ModulationSource::PitchBend,
                    ModSource::MonoAftertouch => ir::ModulationSource::ChannelPressure,
                    ModSource::PolyAftertouch => ir::ModulationSource::PolyPressure,
                    ModSource::Constant => ir::ModulationSource::Constant,
                    // Kontakt manual (Source module, T): counts down from T ms
                    // at note-on and holds its value at note-off.
                    ModSource::ReleaseTriggerCounter if v.rls_trig_counter > 0 => {
                        ir::ModulationSource::ReleaseCounter(ir::Time::Milliseconds(f64::from(
                            v.rls_trig_counter,
                        )))
                    }
                    // KSP set_event_par_arr($EVENT_PAR_MOD_VALUE_ID, v, id).
                    ModSource::Script(id) if id <= 1000 => ir::ModulationSource::Script(id as u16),
                    ModSource::RandomUnipolar => ir::ModulationSource::Random,
                    ModSource::Unassigned => continue,
                    other => {
                        let reason = match other {
                            ModSource::RandomBipolar => ir::Reason::UnknownLaw,
                            _ => not_modeled,
                        };
                        let targets: Vec<_> =
                            params.targets.iter().map(|t| t.param.as_str()).collect();
                        self.unsupported(
                            &at,
                            &format!("external modulation from {other:?}"),
                            format!("{:?} -> {targets:?}", params.name),
                            reason,
                        );
                        continue;
                    }
                };
                // Bend is bipolar: shapers read (bend + 1) / 2 (Pacific's PB
                // shapers sit at 0.81 at rest and 1 at full bend). Bend to
                // pitch stays the note's native expression bend in lowering.
                let bipolar = source.bipolar();
                self.ir.modulators.push(ir::Modulator {
                    scope: ir::Scope::Voice,
                    source,
                });
                let modulator = ir::ModulatorRef(self.ir.modulators.len() - 1);
                if let Some(address) = self
                    .ir
                    .source_indices
                    .modulators
                    .iter_mut()
                    .find(|m| m.group == index && m.slot == usize::from(slot) && m.external)
                {
                    address.runtime = Some(modulator);
                }
                for (_ordinal,target) in params.targets.iter().enumerate() {
                    let route=self.route(&at,modulator,!bipolar,target,chain.zip(Some(&filter_slots[..])));
                    #[cfg(feature="scan")]
                    self.target_outcomes.insert((index,usize::from(slot),true,_ordinal),route.is_some());
                    routes.extend(route);
                }
            }
        }
        if !v.volume.is_finite() || !v.pan.is_finite() || !(v.tune.is_finite() && v.tune > 0.0) {
            return Err(ni_file::Error::Generic(format!(
                "{at}: invalid gain, pan or tune"
            )));
        }
        self.ir.groups.push(ir::Group {
            name: v.name,
            gain: ir::Gain::Linear(f64::from(v.volume)),
            pan: ir::Pan {
                position: f64::from(v.pan.clamp(-1.0, 1.0)),
                law: ir::PanLaw::Balance,
            },
            tune: ir::Pitch::Ratio(f64::from(v.tune)),
            voice_limit,
            monophonic_release: v.release_trigger && v.release_trigger_note_monophonic,
            ..Default::default()
        });
        Ok(Some(GroupInfo {
            index,
            group: ir::GroupRef(self.ir.groups.len() - 1),
            release: v.release_trigger,
            tracking: v.key_tracking,
            reverse: v.reverse,
            envelope,
            velocity,
            routes,
            chain,
            bus: self.script_bus(index).or(group.bus_route()),
        }))
    }

    /// The `$ENGINE_PAR_*` value `on init` and `on persistence_changed` left for
    /// `(parameter, group, slot, generic)`.
    fn script_par(&self, parameter: &str, group: i32, slot: i32, generic: i32) -> Option<i32> {
        self.engine
            .iter()
            .rev()
            .find(|w| {
                w.parameter.trim_start_matches('$') == parameter
                    && (w.group, w.slot, w.generic) == (group, slot, generic)
            })
            .map(|w| w.value)
    }

    /// Port v1's EQ knob domains onto the physical band's real processor lanes.
    fn eq_controls(&mut self, group: usize, chain: ir::ChainRef, slots: &[(usize, usize)]) {
        let mut bands = HashMap::<usize, usize>::new();
        for &(slot, index) in slots {
            let Some(ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::Peak { gain },
                cutoff: ir::Frequency::Hertz(hz), resonance: ir::Resonance::Q(q) })) =
                self.ir.chains[chain.0].pre_amplitude.iter().chain(&self.ir.chains[chain.0].post_amplitude).nth(index)
            else { continue; };
            let band = bands.entry(slot).or_default();
            let saved = [20. * gain.linear().log10(), (hz / 20.).log10() / 3.,
                (2. * (0.5 / q).asinh() / std::f64::consts::LN_2 - 0.3) / 2.7];
            for (n, (native, name, label, parameter, min, max, unit)) in [
                ("GAIN", "gain", "Gain", ir::ProcessorParameter::Gain, -18., 18., ir::ControlUnit::Decibels),
                ("FREQ", "frequency", "Frequency", ir::ProcessorParameter::Cutoff, 0., 1., ir::ControlUnit::Percent),
                ("BW", "bandwidth", "Bandwidth", ir::ProcessorParameter::Resonance, 0., 1., ir::ControlUnit::Percent),
            ].into_iter().enumerate() {
                let native = format!("ENGINE_PAR_{native}{}", *band + 1);
                let default = self.script_par(&native, group as i32, slot as i32, -1)
                    .map_or(saved[n], |v| min + (max - min) * f64::from(v.clamp(0, 1_000_000)) / 1_000_000.)
                    .clamp(min, max);
                let control = ir::ControlRef(self.ir.controls.len());
                self.ir.controls.push(ir::Control {
                    key: format!("kontakt/group/{group}/slot/{slot}/eq/band/{}/{name}", *band + 1),
                    label: format!("EQ Band {} {label}", *band + 1),
                    value: ir::ControlValue::Continuous { min, max, default, unit },
                    automation: ir::Automation::None,
                });
                self.ir.processor_controls.push(ir::ProcessorControl { control, chain, index, parameter, ramp: ir::Time::ZERO });
                self.ir.source_indices.control_aliases.push(ir::SourceControlAlias { control, parameter: native,
                    address: ir::SlotAddress { group: group as i32, slot: slot as i32, generic: -1 } });
            }
            *band += 1;
        }
    }

    /// The instrument bus a script routed group `index` to
    /// (`$ENGINE_PAR_OUTPUT_CHANNEL` = `$NI_BUS_OFFSET` + n).
    fn script_bus(&self, index: usize) -> Option<u8> {
        let value = self.script_par("ENGINE_PAR_OUTPUT_CHANNEL", index as i32, -1, -1)?;
        u8::try_from(value.checked_sub(1000)?)
            .ok()
            .filter(|&n| n < 16)
    }

    /// An instrument bus fader a script set (linear gain). Law: dB = 18 log2(v)
    /// - 346.768, the one group volume uses (0 dB at 630957).
    fn script_bus_volume(&self, bus: usize) -> Option<f32> {
        let v = self.script_par("ENGINE_PAR_VOLUME", -1, -1, 1000 + bus as i32)?;
        let millibels = 18000.0 * f64::from(v.clamp(1, 1_000_000)).log2() - 346_768.234_247_835_1;
        Some(10f64.powf(millibels / 20_000.0) as f32)
    }

    /// The modulation intensity a script set for modulation `name` of group `index`.
    fn script_intensity(&self, index: usize, name: &str) -> Option<f32> {
        let slot = self.ir.source_indices.engine_lookups.iter()
            .find(|lookup| lookup.group == index as i32 && lookup.owner == -1 && !lookup.target
                && lookup.name.eq_ignore_ascii_case(name))?.index;
        let v = self.script_par("ENGINE_PAR_MOD_TARGET_INTENSITY", index as i32, slot, -1)?;
        Some(v.clamp(0, 1_000_000) as f32 / 1_000_000.0)
    }

    /// One Kontakt modulation target as an IR route, or a report entry.
    /// Laws (decoded from Kontakt's engine, control rate = rate / 32):
    /// volume factor 1 − i(1 − u); pitch 12·i semitones; playPos start + i·u
    /// of the zone's start-modulation range; lag reaches 99% in `lag_ms`.
    fn route(
        &mut self,
        at: &str,
        source: ir::ModulatorRef,
        _unipolar: bool,
        target: &ni_file::kontakt::objects::ModTarget,
        filters: Option<(ir::ChainRef, &[(usize, usize)])>,
    ) -> Option<ir::RouteRef> {
        let i = f64::from(target.signed_intensity());
        let report = |this: &mut Self, feature: &str, reason| {
            this.unsupported(
                at,
                feature,
                format!("{} ({}) intensity {i}", target.param, target.name),
                reason,
            );
            None
        };
        // Native Ladder/Daft use normalized knob addition (v1 filter.rs);
        // the generic filter fallback retains its octave law.
        let addressed = filters.and_then(|(chain, slots)| {
            let slot = target.slot?;
            let knob = eq_knob(&target.param);
            let (_, index) = slots.iter().filter(|(s, _)| *s == usize::from(slot)).nth(knob.map_or(0, |(band, _)| band))?;
            let processor = self.ir.chains.get(chain.0)?.pre_amplitude.iter()
                .chain(&self.ir.chains[chain.0].post_amplitude).nth(*index)?;
            if let Some((_, parameter)) = knob {
                return matches!(processor, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::Peak { .. }, .. }))
                    .then_some((ir::Target::Processor { chain, index: *index, parameter }, ir::Depth::Normalized(i)));
            }
            let native = matches!(processor, ir::Processor::LadderLP4(_) | ir::Processor::Daft(_));
            let parameter = match target.param.as_str() {
                "filterCutoff" if !matches!(processor, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::Peak { .. }, .. })) => ir::ProcessorParameter::Cutoff,
                "filterQ" if native => ir::ProcessorParameter::Resonance,
                "Gain" if native => ir::ProcessorParameter::Gain,
                _ => return None,
            };
            let depth = if native { ir::Depth::Normalized(i) }
                else { ir::Depth::Pitch(ir::Pitch::Semitones(120.0 * i)) };
            Some((ir::Target::Processor { chain, index: *index, parameter }, depth))
        });
        if target.slot.is_some() && addressed.is_none() {
            return report(self, "modulation of a module parameter", ir::Reason::NotModeled);
        }
        let (route_target, depth) = match target.param.as_str() {
            _ if addressed.is_some() => addressed.unwrap(),
            "volume" => (ir::Target::Amplitude, ir::Depth::Normalized(i)),
            "pitch" => (
                ir::Target::Pitch,
                ir::Depth::Pitch(ir::Pitch::Semitones(12.0 * i)),
            ),
            "playPos" => (ir::Target::SampleStart, ir::Depth::Normalized(i)),
            "pan" => (ir::Target::Pan, ir::Depth::Normalized(i)),
            _ => return report(self, "modulation target", ir::Reason::NotModeled),
        };
        // The invert flag does not act through an enabled shaper: Vista Full
        // Strings stores identical shaped crossfade copies (mic `cl`/`dc`,
        // `BALANCE_COMP`) that differ only in the flag and must play alike,
        // and no inversion order makes both play the same.
        let shaper = target.shaper.as_ref().filter(|s| s.enabled);
        let invert = target.invert && shaper.is_none();
        let shape = match shaper {
            None => None,
            Some(shaper) => {
                use ni_file::kontakt::objects::ShaperCurve;
                let points: Vec<(f64, f64)> = match &shaper.curve {
                    ShaperCurve::Table(table) if table.len() > 1 => {
                        let last = (table.len() - 1) as f64;
                        table
                            .iter()
                            .enumerate()
                            .map(|(n, y)| (n as f64 / last, f64::from(*y)))
                            .collect()
                    }
                    ShaperCurve::Breakpoints(points) if !points.is_empty() => curved_shaper(points),
                    _ => return report(self, "empty modulation shaper", ir::Reason::UnknownLaw),
                };
                self.ir.shapes.push(ir::Shape { points });
                Some(ir::ShapeRef(self.ir.shapes.len() - 1))
            }
        };
        self.ir.routes.push(ir::Route {
            source,
            target: route_target,
            depth,
            invert,
            shape,
            smoothing: ir::Time::Milliseconds(f64::from(target.lag_ms)),
            scale: None,
        });
        Some(ir::RouteRef(self.ir.routes.len() - 1))
    }

    /// Kontakt LFO: waveform ids sine 0, rectangle 1, triangle 2, sawtooth 3,
    /// random 4, Multi 5. Rate is Hz when the sync note value is −1, else a
    /// cycle of note value × count beats; initial values are fade-in ms,
    /// rate/count, pulse width and start phase in cycles.
    fn lfo(
        &mut self,
        at: &str,
        lfo: &ni_file::kontakt::objects::Lfo,
        retrigger: bool,
    ) -> Option<ir::Lfo> {
        let [fade_ms, rate, width, phase] = lfo.initial_values.map(f64::from);
        let shape = match (lfo.waveform, lfo.trailing_values) {
            // Native core 0x140b07290: the five weighted components sum to zero.
            // Verified original-byte waveform checks; the bipolar view is still 0.5.
            (5, Some(weights)) if weights == [0.; 5] && width > 0. && width < 1. => ir::LfoShape::Zero,
            (0, _) => ir::LfoShape::Sine,
            (1, _) if width == 0.5 => ir::LfoShape::Square,
            (2, _) => ir::LfoShape::Triangle,
            // Multi with exactly one wave and normalization on is that wave.
            (5, Some(weights))
                if lfo.records[0].flag
                    && weights.iter().filter(|w| **w != 0.0).count() == 1
                    && weights[3] == 0.0
                    && weights[4] == 0.0
                    && (weights[1] == 0.0 || width == 0.5)
                    && weights.iter().all(|w| *w >= 0.0) =>
            {
                match weights.iter().position(|w| *w != 0.0) {
                    Some(0) => ir::LfoShape::Sine,
                    Some(1) => ir::LfoShape::Square,
                    _ => ir::LfoShape::Triangle,
                }
            }
            (waveform, weights) => {
                self.unsupported(
                    at,
                    "LFO waveform (id, multi weights, pulse width)",
                    format!("{waveform} {weights:?} {width}"),
                    ir::Reason::UnknownLaw,
                );
                return None;
            }
        };
        let note = f64::from(lfo.records[0].values[0]);
        let rate = if note == -1.0 {
            ir::Frequency::Hertz(rate)
        } else {
            ir::Frequency::Beats(note * rate)
        };
        if !matches!(rate, ir::Frequency::Hertz(r) | ir::Frequency::Beats(r) if r.is_finite() && r > 0.0)
            || !fade_ms.is_finite()
            || !phase.is_finite()
        {
            self.unsupported(
                at,
                "LFO rate",
                format!("{rate:?}"),
                ir::Reason::InvalidValue,
            );
            return None;
        }
        Some(ir::Lfo {
            shape,
            rate,
            delay: ir::Time::ZERO,
            fade_in: ir::Time::Milliseconds(fade_ms.max(0.0)),
            phase: phase.rem_euclid(1.0),
            retrigger,
        })
    }

    fn zone(
        &mut self,
        index: usize,
        z: RawZone,
        end: Option<u64>,
        group: &GroupInfo,
        program: &ni_file::kontakt::objects::ProgramPublicParams,
        location: PathBuf,
    ) {
        let at = format!("zone {index} (group {})", group.index);
        let asset = match self.assets.get(&location) {
            Some(&asset) => asset,
            None => {
                let encoding = match location
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .as_deref()
                {
                    Some("ncw") => ir::Encoding::Ncw,
                    Some("wav") => ir::Encoding::Wav,
                    Some("aif" | "aiff") => ir::Encoding::Aiff,
                    _ => ir::Encoding::Unknown,
                };
                self.ir.assets.push(ir::Asset {
                    location: ir::AssetLocation::Path(location.to_string_lossy().into_owned()),
                    encoding,
                    root_key: None,
                    loops: Vec::new(),
                });
                self.locations.push(location.clone());
                let asset = ir::AssetRef(self.ir.assets.len() - 1);
                self.assets.insert(location, asset);
                asset
            }
        };
        if end.is_some_and(|end| end <= z.start) {
            self.unsupported(
                &at,
                "empty sample range (start, end)",
                format!("{}, {end:?}", z.start),
                ir::Reason::InvalidValue,
            );
            return;
        }
        let mut slots = [None; 8];
        for (slot, l) in z.loop_slots.iter().zip(&z.loops).filter(|(_, l)| l.mode != 0) {
            if l.loop_count < 0
                || !l.loop_tuning.is_finite()
                || l.loop_tuning <= 0.0
                || l.loop_start < 0
                || l.loop_length <= 0
                || l.x_fade_length < 0
            {
                self.unsupported(
                    &at,
                    "invalid loop",
                    format!("slot {slot}: {l:?}"),
                    ir::Reason::InvalidValue,
                );
                continue;
            }
            if !matches!(l.mode, 1 | 2) {
                self.unsupported(&at, "loop mode", l.mode, ir::Reason::Unknown);
                continue;
            }
            if l.alternating_loop && l.x_fade_length > 0 {
                self.unsupported(&at, "alternating loop crossfade law (metadata retained)", format!("slot {slot}, fade {}", l.x_fade_length), ir::Reason::UnknownLaw);
            }
            slots[usize::from(*slot)] = Some(ir::LoopSlot {
                range: ir::LoopRange {
                    start: l.loop_start as u64,
                    end: l.loop_start as u64 + l.loop_length as u64,
                    crossfade: ir::Span::Frames(l.x_fade_length as u64),
                    alternating: l.alternating_loop,
                },
                count: l.loop_count as u32,
                tuning: f64::from(l.loop_tuning),
                until_release: l.mode == 2,
            });
        }
        let looping = if slots.iter().any(Option::is_some) {
            ir::Looping::Slots(slots)
        } else {
            ir::Looping::None
        };
        let semitones =
            12.0 * f64::from(z.tune * program.tune).log2() + f64::from(program.transpose);
        self.ir.zones.push(ir::Zone {
            group: Some(group.group),
            keys: ir::KeyRange {
                low: z.keys[0],
                high: z.keys[1],
            },
            velocities: ir::VelocityRange {
                low: z.velocities[0].max(1),
                high: z.velocities[1].max(1),
            },
            // Kontakt's system pedal script holds the note-off under sustain,
            // so release-trigger groups fire when the note actually releases:
            // at pedal-up for a sustained key (at key-up with
            // NO_SYS_SCRIPT_PEDAL, which sampler-ksp maps to script sustain).
            trigger: if group.release {
                ir::Trigger::GateRelease
            } else {
                ir::Trigger::Attack
            },
            pitch: if group.tracking {
                ir::KeyTracking::Tracked { root: z.root }
            } else {
                ir::KeyTracking::Fixed
            },
            tune: ir::Pitch::Semitones(semitones),
            gain: ir::Gain::Linear(f64::from(z.gain * program.volume)),
            velocity: group.velocity,
            // Mapping Editor crossfades: widths in key and velocity steps
            // inside the zone; the gain law is measured (Kontakt 8,
            // KONTAKT_REFERENCE.md s.11). Field order low velocity, high
            // velocity, low key, high key (v1 importer); the velocity pair is
            // confirmed by ANALOG STRINGS' zone 1..40 fading out (f1 = 21) and
            // Afflatus zones fading in (f0), no library here uses the key pair.
            fades: {
                let step = |w: i16| w.clamp(0, 127) as u8;
                ir::Fades {
                    velocity_in: step(z.fades[0]),
                    velocity_out: step(z.fades[1]),
                    key_in: step(z.fades[2]),
                    key_out: step(z.fades[3]),
                }
            },
            pan: ir::Pan {
                position: f64::from((z.pan + program.pan).clamp(-1.0, 1.0)),
                law: ir::PanLaw::Balance,
            },
            playback: ir::Playback {
                start: z.start,
                end,
                reverse: group.reverse,
                looping,
                start_range: z.start_mod,
            },
            amplitude: group.envelope,
            routes: group.routes.clone(),
            chain: group.chain,
            ..ir::Zone::new(asset)
        });
    }
}

/// One serialized zone: its group, mapping and sample reference.
struct RawZone {
    sample_present: bool,
    group: usize,
    start: u64,
    end: i32,
    /// Frames past `start` that full-depth playPos modulation reaches.
    start_mod: u64,
    velocities: [u8; 2],
    keys: [u8; 2],
    fades: [i16; 4],
    root: u8,
    gain: f32,
    pan: f32,
    tune: f32,
    file: i32,
    loops: Vec<ni_file::kontakt::objects::Loop>,
    loop_slots: Vec<u8>,
    source: ir::kontakt::Zone,
}

#[cfg(test)]
mod empty_zone_tests {
    #[test]
    fn tester_sampleless_zone_preserves_native_mapping_without_a_file_reference() {
        use std::io::Cursor;
        let mut public = vec![0;48];
        public[16..18].copy_from_slice(&36i16.to_le_bytes());
        public[18..20].copy_from_slice(&81i16.to_le_bytes());
        let mut record = 2u32.to_le_bytes().to_vec();
        record.extend([1,0x9a,0]); record.extend(0u32.to_le_bytes());
        record.extend(48u32.to_le_bytes()); record.extend(public);
        record.extend(0u32.to_le_bytes());
        let mut cursor = Cursor::new(record.as_slice());
        let zone = super::raw_zone(&mut cursor).unwrap();
        assert!(!zone.sample_present);
        assert_eq!((zone.group,zone.file),(2,-1));
        assert_eq!((zone.source.low_key,zone.source.high_key),(36,81));
        assert_eq!(zone.source.zone_tune,0.);
        assert_eq!(cursor.position(),record.len() as u64);
    }
}

/// Layout from the v1 importer: the owning group, then a structured zone whose
/// public data is start, end, start-modulation range, velocity and key ranges,
/// four crossfade widths, root, gain, pan, tune and (v0x9a+) six unknown bytes
/// before the file ID.
fn raw_zone(r: &mut Cursor<&[u8]>) -> Result<RawZone, String> {
    let group = u32le(r).map_err(|e| e.to_string())? as usize;
    let so = StructuredObject::read(&mut *r).map_err(|e| e.to_string())?;
    let source_zone = Zone(so);
    let params = source_zone.params().map_err(|e| e.to_string())?;
    let (start, end, start_mod) = (
        params.sample_start,
        params.sample_end,
        params.sample_start_mod_range,
    );
    let ranges = [
        params.low_velocity,
        params.high_velocity,
        params.low_key,
        params.high_key,
        params.fade_low_velocity,
        params.fade_high_velocity,
        params.fade_low_key,
        params.fade_high_key,
        params.root_key,
    ];
    let (gain, pan, tune, file) = (
        params.zone_volume,
        params.zone_pan,
        params.zone_tune,
        params.filename_id,
    );
    let [lv, hv, lk, hk, f0, f1, f2, f3, root] = ranges;
    let midi = |value: i16, what| {
        if !params.sample_present { return Ok(0); }
        u8::try_from(value)
            .ok()
            .filter(|v| *v < 128)
            .ok_or(format!("{what} {value} is outside 0..=127"))
    };
    let (lk, hk, lv, hv) = (
        midi(lk, "low key")?,
        midi(hk, "high key")?,
        midi(lv, "low velocity")?,
        midi(hv, "high velocity")?,
    );
    if lk > hk || lv > hv {
        return Err(format!(
            "inverted range: keys {lk}..={hk}, velocities {lv}..={hv}"
        ));
    }
    if params.sample_present && (start < 0
        || end > 0
        || !gain.is_finite()
        || !pan.is_finite()
        || !(tune.is_finite() && tune > 0.0))
    {
        return Err(format!(
            "start {start}, end {end}, gain {gain}, pan {pan}, tune {tune}"
        ));
    }
    let loops = match source_zone.0.find_first(LOOPS) {
        Some(chunk) => LoopArray::try_from(chunk).map_err(|e| e.to_string())?,
        None => LoopArray {
            mask: 0,
            items: Vec::new(),
            slots: Vec::new(),
        },
    };
    let sample_present = params.sample_present;
    let source = crate::objects::zone(source_zone.0.version, group as u32, params, &loops);
    Ok(RawZone {
        sample_present,
        group,
        start: start as u64,
        end,
        start_mod: start_mod.max(0) as u64,
        velocities: [lv, hv],
        keys: [lk, hk],
        fades: [f0, f1, f2, f3],
        root: midi(root, "root")?,
        gain,
        pan,
        tune,
        file,
        source,
        loop_slots: loops.slots,
        loops: loops.items,
    })
}

fn u32le(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
#[cfg(test)]
mod survey {
    use super::*;
    #[test]
    #[ignore]
    fn survey_modulation() {
        let root = std::env::var("KONTRA_KONTAKT_LIBRARIES").unwrap();
        let mut stack = vec![PathBuf::from(root)];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) {
                    files.push(p)
                }
            }
        }
        files.sort();
        let mut seen = std::collections::BTreeMap::<String, (usize, String)>::new();
        for f in &files {
            let Ok(chunks) = crate::read_chunks(f) else {
                continue;
            };
            let Some(program) = chunks.find_first(PROGRAM) else {
                continue;
            };
            let Ok(program) = Program::try_from(program) else {
                continue;
            };
            let Some(gl) = program.0.find_first(GROUP_LIST) else {
                continue;
            };
            let Ok(groups) = GroupList::try_from(gl) else {
                continue;
            };
            for g in &groups.groups {
                if let Some(chunk) = g.0.find_first(INTERNAL_MODS) {
                    let Ok(arr) = InternalModArray16::try_from(chunk) else {
                        continue;
                    };
                    let Ok(slots) = arr.slots() else { continue };
                    for (slot, m) in slots {
                        let Ok(p) = m.params() else { continue };
                        let t: Vec<_> = p
                            .targets
                            .iter()
                            .map(|t| {
                                format!(
                                    "{}@{:?} i={} inv={} lag={} fl={:#x} sh={}",
                                    t.param,
                                    t.slot,
                                    t.intensity,
                                    t.invert,
                                    t.lag_ms,
                                    t.unknown_flags,
                                    t.shaper.as_ref().is_some_and(|s| s.enabled)
                                )
                            })
                            .collect();
                        let src = match &p.modulator {
                            Modulator::Lfo(l) => format!(
                                "LFO v{:#x} wf={} init={:?} r0={:?} r1={:?} tf={} tv={:?} add={:?}",
                                l.version,
                                l.waveform,
                                l.initial_values,
                                l.records[0],
                                l.records[1],
                                l.trailing_flag,
                                l.trailing_values,
                                l.additional_flag
                            ),
                            Modulator::Ahdsr(e) => format!(
                                "AHDSR a={} h={} d={} s={} r={} c={} f={}",
                                e.attack_ms,
                                e.hold_ms,
                                e.decay_ms,
                                e.sustain,
                                e.release_ms,
                                e.attack_curve,
                                e.unknown_flag
                            ),
                            Modulator::Flex(e) => format!("FLEX {:?} sus={}", e.points, e.sustain),
                            Modulator::Other { chunk_id } => format!("OTHER {chunk_id:#x}"),
                        };
                        let key =
                            format!("INT {} flags={:?} {src} -> {t:?}", p.name, p.unknown_flags);
                        let e = seen
                            .entry(key)
                            .or_insert((0, format!("{} slot {slot}", f.display())));
                        e.0 += 1;
                    }
                }
                if let Some(chunk) = g.0.find_first(EXTERNAL_MODS) {
                    let Ok(arr) = ExternalModArray32::try_from(chunk) else {
                        continue;
                    };
                    let Ok(slots) = arr.slots() else { continue };
                    for (_slot, m) in slots {
                        let Ok(p) = m.params() else { continue };
                        let t: Vec<_> = p
                            .targets
                            .iter()
                            .map(|t| {
                                format!(
                                    "{}@{:?} i={} inv={} lag={} fl={:#x} sh={}",
                                    t.param,
                                    t.slot,
                                    t.intensity,
                                    t.invert,
                                    t.lag_ms,
                                    t.unknown_flags,
                                    t.shaper.as_ref().is_some_and(|s| s.enabled)
                                )
                            })
                            .collect();
                        let key = format!("EXT {} {:?} -> {t:?}", p.name, p.source);
                        let e = seen.entry(key).or_insert((0, f.display().to_string()));
                        e.0 += 1;
                    }
                }
            }
        }
        for (k, (n, f)) in &seen {
            println!("{n:6} {k}\n         e.g. {f}");
        }
        println!("{} files", files.len());
    }
}

/// Strict saved-entry reader. Numeric repeated tails are expanded by the KSP
/// declaration, while LF string arrays preserve whitespace and empty cells.
pub(crate) fn saved(entries: &[String]) -> Result<Vec<(String, ir::Saved)>, crate::Error> {
    use crate::{SavedEntry, SavedValue};
    entries
        .iter()
        .map(|raw| {
            let entry = SavedEntry::parse(
                raw.as_bytes(),
                None,
                crate::Limits {
                    bytes: 16 * 1024 * 1024,
                    records: 1_000_000,
                },
            )?;
            let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
            let value = match entry.value {
                SavedValue::Int(v) | SavedValue::MenuIndex(v) => ir::Saved::Int(v.into()),
                SavedValue::Real(v) => ir::Saved::Real(v),
                SavedValue::Text(v) => ir::Saved::Text(text(v)),
                SavedValue::Ints { values, .. } => {
                    ir::Saved::Ints(values.iter().map(i64::from).collect())
                }
                SavedValue::Reals { values, .. } => ir::Saved::Reals(values.iter().collect()),
                SavedValue::Texts(values) => ir::Saved::Texts(values.iter().map(text).collect()),
            };
            Ok((entry.name.to_owned(), value))
        })
        .collect()
}

#[cfg(test)]
mod saved_tests {
    use super::*;
    #[test]
    fn authored_envelope_init_values_keep_physical_slots_and_native_laws() {
        let mut indices = ir::SourceIndices::default();
        let envelope = ni_file::kontakt::objects::EnvelopeAhdsr {
            attack_ms: 80.,
            hold_ms: 2.,
            decay_ms: 25000.,
            sustain: 0.4,
            release_ms: 120.,
            attack_curve: 0.75,
            unknown_flag: 1,
            unknown_tail: vec![0; 52],
        };
        Translation::source_envelope(&mut indices, 7, 12, &envelope);
        let values = &indices.engine_values;
        assert_eq!(values.len(), 6);
        assert_eq!(
            values
                .iter()
                .find(|value| sampler_core::engine_parameter_name(value.parameter)
                    == Some("$ENGINE_PAR_SUSTAIN"))
                .unwrap()
                .value,
            400000
        );
        assert!(
            values
                .iter()
                .all(|value| value.group == 7 && value.slot == 12 && value.generic == -1)
        );
        let source = values
            .iter()
            .map(|value| {
                format!(
                    "set_engine_par({},get_engine_par({},7,12,-1),7,12,-1)\n",
                    sampler_core::engine_parameter_name(value.parameter).unwrap(),
                    sampler_core::engine_parameter_name(value.parameter).unwrap()
                )
            })
            .collect::<String>();
        let behavior = ir::Behavior {
            name: "authored getter feedback".into(),
            language: ir::Language::Ksp,
            source: format!("on init\n{source}end on"),
            slot: Some(0),
            state: vec![],
            requires: vec![],
        };
        let environment = crate::load::script_environment(
            &behavior,
            0,
            vec![String::new(); 8],
            &indices,
            Default::default(),
        );
        let writes =
            sampler_ksp::initialize(&behavior.source, sampler_ksp::Limits::LIBRARY, &environment)
                .unwrap()
                .engine_pars();
        assert_eq!(writes.len(), 6);
        for value in values {
            assert!(
                writes
                    .iter()
                    .any(|write| sampler_core::engine_parameter_id(&write.parameter)
                        == Some(value.parameter)
                        && write.group == 7
                        && write.slot == 12
                        && write.generic == -1
                        && write.value == value.value)
            );
        }
        for (stage, native) in [
            (sampler_core::EnvelopeStage::Attack, 80.),
            (sampler_core::EnvelopeStage::Decay, 25000.),
        ] {
            let ms = sampler_core::EngineParameterLaw::envelope(stage, 1000).encode(native);
            let frames = sampler_core::EngineParameterLaw::envelope(stage, 48000).encode(native * 48.);
            assert_eq!(
                ms, frames,
                "authored init and DSP binding must use the same law"
            );
        }
    }

    #[test]
    fn dolce_sustain_getter_feedback_is_observable_without_exporting_source() {
        let path = std::path::Path::new(
            "/mnt/MAIN_STORAGE/Libraries/Kontakt/Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 03 7 1st Violins - Sustained Con Sordino.nki",
        );
        if !path.is_file() {
            return;
        }
        let kontakt = super::read(path)
            .unwrap_or_else(|_| panic!("instrument read failed; authored diagnostics omitted"));
        let instrument = &kontakt.instrument;
        let parameter = sampler_core::engine_parameter_id("ENGINE_PAR_SUSTAIN").unwrap();
        let mut changed = 0;
        for (index, behavior) in instrument.behaviors.iter().enumerate() {
            let mut environment = crate::load::script_environment(
                behavior,
                index,
                instrument
                    .groups
                    .iter()
                    .map(|group| group.name.clone())
                    .collect(),
                &instrument.source_indices,
                Default::default(),
            );
            environment.engine_values.clear();
            let cold =
                sampler_ksp::initialize(&behavior.source, sampler_ksp::Limits::LIBRARY, &environment)
                    .unwrap_or_else(|_| panic!("init failed; authored diagnostics omitted"))
                    .engine_pars();
            for group in 0..instrument.groups.len() {
                environment
                    .engine_values
                    .insert([i32::from(parameter), group as i32, 1, -1], 1_000_000);
            }
            let authored =
                sampler_ksp::initialize(&behavior.source, sampler_ksp::Limits::LIBRARY, &environment)
                    .unwrap_or_else(|_| panic!("init failed; authored diagnostics omitted"))
                    .engine_pars();
            for write in authored
                .iter()
                .filter(|write| write.parameter == "$ENGINE_PAR_SUSTAIN" && write.slot == 1)
            {
                if cold.iter().any(|prior| {
                    prior.parameter == write.parameter
                        && prior.group == write.group
                        && prior.slot == write.slot
                        && prior.generic == write.generic
                        && prior.value == 0
                }) && write.value == 1_000_000
                {
                    changed += 1;
                }
            }
        }
        assert_eq!(
            changed, 36,
            "changing only authored getter input must reach physical sustain setters"
        );
        println!("DOLCE_INIT_FEEDBACK sustain_setters_changed={changed}");
    }

    #[test]
    fn translated_script_prefers_the_link_then_falls_back_to_saved_source() {
        use ni_file::kontakt::{Chunk, StructuredObject, objects::Program};
        let root =
            std::env::temp_dir().join(format!("kontakt-linked-translation-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Resources/scripts")).unwrap();
        let linked = "on init\nmessage(\"linked\")\nend on";
        let saved = "on init\nmessage(\"saved\")\nend on";
        let file = root.join("Resources/scripts/Source.txt");
        std::fs::write(&file, linked).unwrap();
        for expected in [linked, saved] {
            let mut public = (saved.len() as u32).to_le_bytes().to_vec();
            public.extend(saved.as_bytes());
            public.extend([0; 3]);
            public.extend(0u32.to_le_bytes());
            public.extend(u32::MAX.to_le_bytes());
            let name = r"C:\old\source.TXT";
            public.extend((name.len() as u32).to_le_bytes());
            public.extend(name.as_bytes());
            let mut script = vec![0, 0x50, 0];
            script.extend(public);
            let program = Program(StructuredObject {
                version: 0xaf,
                public_data: vec![0; 70],
                private_data: Vec::new(),
                children: vec![
                    Chunk {
                        id: 0x33,
                        data: vec![0; 4],
                    },
                    Chunk {
                        id: 0x34,
                        data: vec![0; 4],
                    },
                    Chunk {
                        id: 6,
                        data: script,
                    },
                ],
            });
            let translated = super::translate(
                root.join("Piano.nki"),
                program,
                Default::default(),
                Default::default(),
                None,
            )
            .unwrap();
            assert_eq!(translated.instrument.behaviors[0].source, expected);
            if expected == linked {
                std::fs::remove_file(&file).unwrap();
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_saved_entry_is_a_fault() {
        assert!(super::saved(&["$bad x".into()]).is_err());
    }
    #[test]
    fn saved_values_keep_their_types_and_arrays() {
        let entries = [
            "$level 17",
            "~mix 0.5",
            "@label two words",
            "%table 1 2 3",
            "!strings first line\n\nthird line\n",
        ]
        .map(String::from);
        let saved = super::saved(&entries).unwrap();
        assert_eq!(
            saved,
            [
                ("$level".to_owned(), sampler_ir::Saved::Int(17)),
                ("~mix".to_owned(), sampler_ir::Saved::Real(0.5)),
                (
                    "@label".to_owned(),
                    sampler_ir::Saved::Text("two words".into())
                ),
                ("%table".to_owned(), sampler_ir::Saved::Ints(vec![1, 2, 3])),
                (
                    "!strings".to_owned(),
                    sampler_ir::Saved::Texts(vec![
                        "first line".into(),
                        "".into(),
                        "third line".into()
                    ])
                ),
            ]
        );
    }
}

#[cfg(test)]
mod modulation {
    use super::*;
    use ni_file::kontakt::objects::{Lfo, LfoRecord, ModTarget};

    #[test]
    fn ahdsr_stage_laws_are_native_exponential_curves() {
        let curve = |k: ir::Curve, t: f64| match k {
            ir::Curve::Exponential(k) => (k * t).exp_m1() / k.exp_m1(),
            ir::Curve::Linear | ir::Curve::Step => t,
        };
        let (attack, fall) = ahdsr_curves(0.5);
        // Decay: 1.075·(3/43)^t − 0.075 falls from 1 to 0.
        for t in [0.25, 0.5, 0.9] {
            let kontakt = 1.075 * (3.0f64 / 43.0).powf(t) - 0.075;
            assert!((1.0 - curve(fall, t) - kontakt).abs() < 2e-3, "{t}");
        }
        // Positive curve: geometric from 1 + b down to b, read as start − state.
        let b = f64::from(((0.5 * 500_000f64.ln() - 20_000f64.ln()).exp()) as f32);
        for t in [0.1, 0.5] {
            let kontakt = (1.0 + b) * (1.0 - (b / (1.0 + b)).powf(t));
            assert!((curve(attack, t) - kontakt).abs() < 1e-9, "{t}");
        }
        // Negative curves start slowly.
        let (attack, _) = ahdsr_curves(-0.5);
        assert!(curve(attack, 0.5) < 0.5);
    }

    fn translation() -> Translation {
        Translation {
            ir: ir::Instrument::default(),
            assets: HashMap::new(),
            locations: Vec::new(),
            start_criteria: Vec::new(),
            voice_groups: Vec::new(),
            snapshot_groups: Vec::new(),
            engine: Vec::new(),
            dynamic: false,
            eq_mod_slots: HashMap::new(),
            send_taps: Vec::new(),
        #[cfg(feature="scan")]
        target_outcomes: HashMap::new(),
        }
    }

    #[test]
    fn group_send_taps_keep_physical_return_identity_and_amplifier_side() {
        let mut out = translation();
        out.ir.chains.push(ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: Vec::new(), post_amplitude: Vec::new() });
        out.send_taps.push((ir::ChainRef(0), crate::effects::SendTap {
            slot: 5, position: ir::VoiceSendPosition::AfterAmplitude(0),
            levels: vec![0.0, 0.0, 0.0, 0.5], bypass: false,
        }));
        out.resolve_send_taps(&[(3, ir::BusRef(1))]);
        assert_eq!(out.ir.voice_send_taps, vec![ir::VoiceSendTap {
            chain: ir::ChainRef(0), position: ir::VoiceSendPosition::AfterAmplitude(0),
            bus: ir::BusRef(1), gain: ir::Gain::Linear(0.5), bypass: false,
            gain_control: None, bypass_control: None, ramp: ir::Time::Seconds(0.0),
        }]);
        assert!(out.ir.unsupported.is_empty());
    }

    #[test]
    fn source_mod_and_target_lookups_keep_holes_and_unmodeled_targets() {
        let mut out = translation();
        let mut unnamed = target("not-modeled", 1.);
        unnamed.name.clear();
        let mut named = target("not-modeled", 1.);
        named.name = "Cutoff".into();
        out.source_modulator(7, 31, true, "Controller".into(), &[unnamed, named]);
        out.source_modulator(7, 12, false, "Envelope".into(), &[]);
        let lookups = &out.ir.source_indices.engine_lookups;
        assert_eq!(lookups.len(), 4);
        assert_eq!((lookups[0].group, lookups[0].owner, lookups[0].target, lookups[0].index), (7,-1,false,31));
        assert_eq!((lookups[2].group, lookups[2].owner, lookups[2].target, lookups[2].index), (7,31,true,1));
        assert_eq!(lookups[2].name,"Cutoff");
        assert_eq!(lookups[3].index,12);
        assert_eq!(out.ir.source_indices.modulators[0].slot,31);
        assert!(out.ir.source_indices.modulators[0].external);
        assert_eq!(out.ir.source_indices.modulators[0].runtime,None);
    }

    #[test]
    fn authored_init_intensity_uses_the_same_physical_modulator_slot() {
        let mut out=translation();
        out.source_modulator(7,31,true,"Controller".into(),&[]);
        out.engine.push(sampler_ksp::EnginePar { parameter:"$ENGINE_PAR_MOD_TARGET_INTENSITY".into(), group:7, slot:31, generic:-1, value:500000 });
        assert_eq!(out.script_intensity(7,"controller"),Some(0.5));
    }

    #[test]
    fn fx_decode_group_insert_errors_reach_the_ir_report() {
        let mut public = 0u32.to_le_bytes().to_vec();
        for value in [1f32, 0.0, 1.0] {
            public.extend(value.to_le_bytes());
        }
        public.extend([1, 0, 0, 0]);
        public.extend(0i32.to_le_bytes());
        public.extend((-1i16).to_le_bytes());
        public.extend([0; 10]); // Voice group, amplifier split, mute/solo.
        public.extend(0i32.to_le_bytes());
        let mut group = Group(ni_file::kontakt::StructuredObject {
            version: 0x95,
            private_data: vec![],
            public_data: public,
            children: vec![ni_file::kontakt::Chunk {
                id: 0x38,
                data: vec![0],
            }],
        });
        let mut out = translation();
        assert!(out.group(0, &group).unwrap().is_some());
        let notes: Vec<_> = out
            .ir
            .unsupported
            .iter()
            .filter(|n| n.feature == "effect decoding")
            .collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].location, "group 0 \"\" insert");
        assert_eq!(notes[0].reason, ir::Reason::Unknown);
        assert!(notes[0].value.contains("Unrecognized group private data"));
        for _ in 0..136 {
            group.0.private_data.extend(8u32.to_le_bytes());
            group.0.private_data.extend([0; 8]);
        }
        group.0.private_data.extend([0; 24]);
        group.0.private_data.extend([0, 0xff, 0xff]);
        let raw = group.0.private_data.clone();
        let mut out = translation();
        assert!(out.group(0, &group).unwrap().is_some());
        assert!(
            out.ir
                .unsupported
                .iter()
                .any(|n| n.feature == "effect decoding"
                    && n.location == "group 0 \"\" insert"
                    && n.value.contains("ffff"))
        );
        assert_eq!(group.0.private_data, raw);
        group.0.public_data[34] = 1; // Muted groups do not enter FX translation.
        let mut muted = translation();
        assert!(muted.group(0, &group).unwrap().is_none());
        assert!(muted.ir.unsupported.is_empty());
    }

    fn target(param: &str, intensity: f32) -> ModTarget {
        ModTarget {
            param: param.into(),
            intensity,
            lag_ms: 0,
            name: String::new(),
            slot: None,
            invert: false,
            shaper: None,
            unknown_i16: -1,
            unknown_flags: 0x10,
        }
    }

    /// KONTAKT_REFERENCE.md section 17: with a 2 s decay to sustain 0.25 the
    /// envelope minus sustain falls 0.51, 0.23, 0.085, 0.02 at 0.25 s steps
    /// from the 2 s peak (1 - 0.25 = 0.75 of travel, so ratios .67/.30/.11/.026).
    #[test]
    fn ahdsr_decay_is_exponential_like_the_measurement() {
        let (attack, ir::Curve::Exponential(k)) = ahdsr_curves(0.0) else {
            panic!("exponential decay")
        };
        let remaining = |t: f64| 1.0 - ((k * t).exp() - 1.0) / (k.exp() - 1.0);
        for (t, measured) in [(0.125, 0.67), (0.375, 0.30), (0.625, 0.11), (0.875, 0.026)] {
            assert!(
                (remaining(t) - measured).abs() < 0.04,
                "t {t}: {} vs {measured}",
                remaining(t)
            );
        }
        // Section 22: a 500 ms decay to sustain -24 dB reaches -3/-6/-10/-20 dB
        // at 0.072/0.130/0.216/0.427 s (within 0.5 dB), and sustain at 0.5 s.
        let level = |t: f64| 1.0 - (1.0 - 0.063_1) * (1.0 - remaining(t / 0.5));
        for (t, db) in [(0.072, -3.0), (0.130, -6.0), (0.216, -10.0), (0.427, -20.0)] {
            let got = 20.0 * level(t).log10();
            assert!((got - db).abs() < 0.5, "t {t}: {got} dB vs {db}");
        }
        assert!((level(0.5) - 0.063_1).abs() < 1e-9);
        // Curve 0 attack is near-linear: env 0.12/0.35/0.59/0.84 at 1/8, 3/8, 5/8, 7/8.
        let ir::Curve::Exponential(a) = attack else {
            panic!("attack curve")
        };
        let rise = |t: f64| ((a * t).exp() - 1.0) / (a.exp() - 1.0);
        for (t, measured) in [(0.125, 0.12), (0.375, 0.35), (0.625, 0.59), (0.875, 0.84)] {
            assert!((rise(t) - measured).abs() < 0.04, "t {t}: {}", rise(t));
        }
    }

    #[test]
    fn authored_pan_target_reaches_the_shared_voice_pan_route() {
        for unipolar in [false, true] {
            let mut t = translation();
            let pan = ModTarget { lag_ms: 15, invert: true, ..target("pan", 0.5) };
            t.route("g", ir::ModulatorRef(0), unipolar, &pan, None)
                .expect("authored pan must execute");
            assert_eq!(t.ir.routes[0].target, ir::Target::Pan);
            assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(0.5));
            assert!(t.ir.routes[0].invert);
            assert_eq!(t.ir.routes[0].smoothing, ir::Time::Milliseconds(15.));
        }
    }

    #[test]
    fn zero_wave_multi_lfo_retains_its_bipolar_source_instead_of_disappearing() {
        let mut t = translation();
        let lfo = Lfo { structured: false, version: 0x73, waveform: 5,
            initial_values: [0., 1., 0.5, 0.],
            records: [LfoRecord { flag: true, values: [-1., 0., 1.] },
                LfoRecord { flag: false, values: [-1., 0., 1.] }],
            trailing_flag: false, trailing_values: Some([0.; 5]), additional_flag: Some(true) };
        let source = t.lfo("g", &lfo, true).expect("authored zero-wave Multi still has a bipolar output");
        assert_eq!(source.shape, ir::LfoShape::Zero);
    }

    #[test]
    fn filter_cutoff_modulation_is_ten_octaves_per_full_amount() {
        let mut t = translation();
        let source = ir::ModulatorRef(0);
        let cutoff = ModTarget {
            slot: Some(2),
            ..target("filterCutoff", 0.444)
        };
        let chain = ir::ChainRef(3);
        let filter = ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::LowPass { poles: 2 },
            cutoff: ir::Frequency::Hertz(1000.), resonance: ir::Resonance::Q(1.) });
        t.ir.chains = vec![ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![filter; 2], post_amplitude: vec![] }; 4];
        t.route("g", source, true, &cutoff, Some((chain, &[(2, 1)])))
            .unwrap();
        assert_eq!(
            t.ir.routes[0].target,
            ir::Target::Processor {
                chain,
                index: 1,
                parameter: ir::ProcessorParameter::Cutoff
            }
        );
        let ir::Depth::Pitch(p) = t.ir.routes[0].depth else {
            panic!("pitch depth")
        };
        assert!((p.semitones() / 12.0 - 4.44 * 1.0).abs() < 1e-6);
        // A slot with no translated filter stays reported.
        assert!(
            t.route("g", source, true, &cutoff, Some((chain, &[])))
                .is_none()
        );
    }

    #[test]
    fn eq_frequency_and_width_routes_keep_the_physical_band() {
        let mut t = translation();
        let peak = ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::Peak { gain: ir::Gain::Decibels(6.) },
            cutoff: ir::Frequency::Hertz(1000.), resonance: ir::Resonance::Q(1.) });
        t.ir.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![peak; 3], post_amplitude: vec![] });
        t.engine.push(sampler_ksp::EnginePar { parameter: "$ENGINE_PAR_FREQ2".into(), group: 7, slot: 5, generic: -1, value: 700000 });
        t.engine.push(sampler_ksp::EnginePar { parameter: "$ENGINE_PAR_BW3".into(), group: 7, slot: 5, generic: -1, value: 400000 });
        t.eq_controls(7, ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)]);
        for (control, default, name) in [(4, 0.7, "ENGINE_PAR_FREQ2"), (8, 0.4, "ENGINE_PAR_BW3")] {
            assert!(matches!(t.ir.controls[control].value, ir::ControlValue::Continuous { min: 0., max: 1., default: v, unit: ir::ControlUnit::Percent } if v == default));
            assert_eq!(t.ir.source_indices.control_aliases[control].parameter, name);
        }
        for (name, index, parameter) in [("eqFreq2", 1, ir::ProcessorParameter::Cutoff), ("eqBandwidth3", 2, ir::ProcessorParameter::Resonance)] {
            let target = ModTarget { slot: Some(5), ..target(name, -0.25) };
            let route = t.route("g", ir::ModulatorRef(0), true, &target, Some((ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)])))
                .expect("native EQ knob route reaches its physical band");
            assert_eq!(t.ir.routes[route.0].target, ir::Target::Processor { chain: ir::ChainRef(0), index, parameter });
            assert_eq!(t.ir.routes[route.0].depth, ir::Depth::Normalized(-0.25));
        }
    }

    #[test]
    fn eq_live_owner_clamps_saved_gain_like_v1_knobs() {
        let mut t = translation();
        t.ir.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![ir::Processor::Filter(ir::Filter {
            kind: ir::FilterKind::Peak { gain: ir::Gain::Decibels(24.) },
            cutoff: ir::Frequency::Hertz(1000.), resonance: ir::Resonance::Q(1.),
        })], post_amplitude: vec![] });
        t.eq_controls(7, ir::ChainRef(0), &[(5, 0)]);
        assert!(matches!(t.ir.controls[0].value, ir::ControlValue::Continuous { default: 18., .. }),
            "v1 clamps normalized EQ knobs before conversion; a saved outlier must not invalidate the instrument");
    }

    #[test]
    fn eq_saved_gain_metadata_precedes_script_init() {
        let mut public = Vec::new();
        for kind in [24i32, 24] { public.extend(kind.to_le_bytes()); }
        for values in [[300f32, 1., 0.], [1000., 1., 9.], [6000., 1., -18.]] {
            for value in values { public.extend(value.to_le_bytes()); }
        }
        let slot = crate::effects::Slot { slot: 5, module: 0x18, version: 0x92,
            bypass: false, output_gain: 1., dry_level: 0., output_set: false, public };
        let mut t = translation();
        t.source_eq_values(7, &[slot]);
        let values = &t.ir.source_indices.engine_values;
        assert_eq!(values.len(), 9);
        for (band, expected) in [500000, 750000, 0].into_iter().enumerate() {
            assert_eq!(values[band * 3].value, expected);
            assert_eq!((values[band * 3].group, values[band * 3].slot, values[band * 3].generic), (7, 5, -1));
            assert_eq!(sampler_core::engine_parameter_name(values[band * 3].parameter), Some(["$ENGINE_PAR_GAIN1", "$ENGINE_PAR_GAIN2", "$ENGINE_PAR_GAIN3"][band]));
        }
    }

    #[test]
    fn eq_gain_routes_keep_band_and_physical_slot_identity() {
        for (name, index) in [("eqGain1", 0), ("eqGain2", 1), ("eqGain3", 2)] {
            let mut t = translation();
            let peak = ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::Peak { gain: ir::Gain::Decibels(0.) },
                cutoff: ir::Frequency::Hertz(1000.), resonance: ir::Resonance::Q(1.) });
            t.ir.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![peak; 3], post_amplitude: vec![] });
            t.engine.push(sampler_ksp::EnginePar { parameter: "$ENGINE_PAR_GAIN2".into(), group: 7, slot: 5, generic: -1, value: 750000 });
            t.eq_controls(7, ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)]);
            assert_eq!(t.ir.processor_controls.len(), 9);
            assert_eq!(t.ir.source_indices.control_aliases[3].parameter, "ENGINE_PAR_GAIN2");
            assert!(matches!(t.ir.controls[3].value, ir::ControlValue::Continuous { default: 9., unit: ir::ControlUnit::Decibels, .. }));
            let target = ModTarget { slot: Some(5), ..target(name, -0.25) };
            t.route("g", ir::ModulatorRef(0), true, &target, Some((ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)])))
                .expect("authored EQ gain must reach its physical band");
            assert_eq!(t.ir.routes[0].target, ir::Target::Processor {
                chain: ir::ChainRef(0), index, parameter: ir::ProcessorParameter::Gain });
            assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(-0.25));
        }
    }

    #[test]
    fn native_filter_q_and_gain_routes_add_normalized_depth() {
        for processor in [ir::Processor::LadderLP4(ir::LadderLP4 { address: None, gain: -0.25,
            cutoff: 0.5, resonance: 0., record_version: 0x92 }),
            ir::Processor::Daft(ir::Daft { gain: 0., cutoff: 0.5, resonance: 0., highpass: false })] {
            for (name, parameter) in [("filterQ", ir::ProcessorParameter::Resonance), ("Gain", ir::ProcessorParameter::Gain)] {
                let mut t = translation();
                t.ir.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![processor], post_amplitude: vec![] });
                let target = ModTarget { slot: Some(5), ..target(name, -0.25) };
                t.route("g", ir::ModulatorRef(0), true, &target, Some((ir::ChainRef(0), &[(5, 0)])))
                    .expect("v1 executes native normalized Q/Gain routes");
                assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(-0.25));
                assert_eq!(t.ir.routes[0].target, ir::Target::Processor { chain: ir::ChainRef(0), index: 0, parameter });
            }
        }
    }

    #[test]
    fn native_filter_cutoff_modulation_adds_normalized_depth() {
        for processor in [ir::Processor::LadderLP4(ir::LadderLP4 { address: None, gain: 0.,
            cutoff: 0.5, resonance: 0., record_version: 0x92 }),
            ir::Processor::Daft(ir::Daft { gain: 0., cutoff: 0.5, resonance: 0., highpass: false })] {
            let mut t = translation();
            t.ir.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![processor], post_amplitude: vec![] });
            let target = ModTarget { slot: Some(5), ..target("filterCutoff", -0.25) };
            t.route("g", ir::ModulatorRef(0), true, &target, Some((ir::ChainRef(0), &[(5, 0)]))).unwrap();
            assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(-0.25));
            assert_eq!(t.ir.routes[0].target, ir::Target::Processor { chain: ir::ChainRef(0), index: 0, parameter: ir::ProcessorParameter::Cutoff });
        }
    }

    #[test]
    fn targets_translate_with_kontakt_laws_or_are_reported() {
        let mut t = translation();
        let source = ir::ModulatorRef(0);
        let lagged = ModTarget {
            lag_ms: 40,
            invert: true,
            ..target("pitch", 0.5)
        };
        t.route("g", source, true, &lagged, None).unwrap();
        assert_eq!(
            t.ir.routes[0],
            ir::Route {
                source,
                target: ir::Target::Pitch,
                depth: ir::Depth::Pitch(ir::Pitch::Semitones(6.0)),
                invert: true,
                shape: None,
                smoothing: ir::Time::Milliseconds(40.0),
                scale: None,
            }
        );
        t.route("g", source, true, &target("volume", 0.25), None)
            .unwrap();
        assert_eq!(t.ir.routes[1].depth, ir::Depth::Normalized(0.25));
        t.route("g", source, true, &target("playPos", 1.0), None)
            .unwrap();
        assert_eq!(t.ir.routes[2].target, ir::Target::SampleStart);
        t.route("g", source, true, &target("pan", 1.0), None).unwrap();
        assert_eq!(t.ir.routes[3].target, ir::Target::Pan);
        for unknown in [
            target("cutoff", 1.0),
            ModTarget {
                slot: Some(0),
                ..target("cutoff", 1.0)
            },
        ] {
            assert!(t.route("g", source, true, &unknown, None).is_none());
        }
        assert_eq!(t.ir.routes.len(), 4);
        let reasons: Vec<_> = t.ir.unsupported.iter().map(|u| u.reason).collect();
        assert_eq!(
            reasons,
            [
                ir::Reason::NotModeled,
                ir::Reason::NotModeled
            ]
        );
    }

    #[test]
    fn saved_depth_sign_is_independent_of_invert_and_source_polarity() {
        for unipolar in [false, true] {
            for invert in [false, true] {
                let mut t = translation();
                let signed = ModTarget { unknown_flags: 0x12, invert, ..target("pitch", 0.25) };
                t.route("g", ir::ModulatorRef(0), unipolar, &signed, None).expect("signed pitch route");
                assert_eq!(t.ir.routes[0].depth, ir::Depth::Pitch(ir::Pitch::Semitones(-3.0)));
                assert_eq!(t.ir.routes[0].invert, invert);
            }
        }
    }

    #[test]
    fn lfos_translate_known_waves_and_report_the_rest() {
        let lfo = |waveform, weights| Lfo {
            structured: false,
            version: 0x72,
            waveform,
            initial_values: [10.0, 4.0, 0.5, 0.25],
            records: [
                LfoRecord {
                    flag: true,
                    values: [-1.0, 0.0, 0.0],
                },
                LfoRecord {
                    flag: false,
                    values: [0.0; 3],
                },
            ],
            trailing_flag: false,
            trailing_values: weights,
            additional_flag: None,
        };
        let mut t = translation();
        let sine = t.lfo("g", &lfo(0, None), true).unwrap();
        assert_eq!(sine.shape, ir::LfoShape::Sine);
        assert_eq!(sine.rate, ir::Frequency::Hertz(4.0));
        assert_eq!(sine.fade_in, ir::Time::Milliseconds(10.0));
        assert_eq!(sine.phase, 0.25);
        let multi = t
            .lfo("g", &lfo(5, Some([0.0, 0.0, 0.7, 0.0, 0.0])), false)
            .unwrap();
        assert_eq!(multi.shape, ir::LfoShape::Triangle);
        assert!(!multi.retrigger);
        let mut synced = lfo(1, None);
        synced.records[0].values[0] = 0.25;
        assert_eq!(
            t.lfo("g", &synced, true).unwrap().rate,
            ir::Frequency::Beats(1.0)
        );
        assert!(t.lfo("g", &lfo(3, None), true).is_none());
        assert!(
            t.lfo("g", &lfo(5, Some([0.5, 0.0, 0.5, 0.0, 0.0])), true)
                .is_none()
        );
        assert_eq!(t.ir.unsupported.len(), 2);
    }
}

#[cfg(test)]
mod multi_tests {
    #[test]
    fn a_multi_program_translates() {
        let Ok(root) = std::env::var("KONTRA_KONTAKT_LIBRARIES") else {
            return;
        };
        let path = std::path::Path::new(&root)
            .join("Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm");
        if !path.exists() {
            return;
        }
        let k = super::read_program(&path, 0).expect("first program");
        assert!(!k.instrument.groups.is_empty());
        assert!(super::read_program(&path, 999).is_err());
    }
}

/// Exponential bow constant for a shaper or flex-envelope segment:
/// `k = BOW_K * |curvature|`.
/// Measured, not in the manual: fitted to Kontakt 8 renders of Vista 3 Cellos
/// group 37 swept over CC100 (0..96), 0.08 dB RMS residual with K = 18; the
/// Una Corda Cotton "Depth" shaper GUI reading (curvature 0.118, mid value
/// 0.29) implies K about 15. docs/architecture-v2/KONTAKT_REFERENCE.md.
const BOW_K: f64 = 17.0;
const BOW_STEPS: usize = 16;

/// A breakpoint shaper as plain points: each segment's stored curvature
/// (positive bows below the chord, negative above) expands into `BOW_STEPS`
/// linear pieces of `y0 + (y1 - y0) f(t)`, with `e(t) = (e^kt - 1)/(e^k - 1)`
/// and `f = e(t)` when the curvature's sign matches the segment's rise (a
/// rising positive or falling negative segment), else `1 - e(1 - t)`.
fn curved_shaper(points: &[ni_file::kontakt::objects::Breakpoint]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (n, p) in points.iter().enumerate() {
        let (x0, y0, c) = (f64::from(p.x), f64::from(p.y), f64::from(p.curve));
        out.push((x0, y0));
        let Some(next) = points.get(n + 1) else {
            continue;
        };
        let (x1, y1) = (f64::from(next.x), f64::from(next.y));
        let k = BOW_K * c.abs();
        if k < 1e-6 || x1 <= x0 {
            continue;
        }
        let e = |t: f64| ((k * t).exp() - 1.0) / (k.exp() - 1.0);
        let same = (c > 0.0) == (y1 > y0);
        for i in 1..BOW_STEPS {
            let t = i as f64 / BOW_STEPS as f64;
            let f = if same { e(t) } else { 1.0 - e(1.0 - t) };
            out.push((x0 + (x1 - x0) * t, y0 + (y1 - y0) * f));
        }
    }
    out
}

#[cfg(test)]
mod curved_shaper_tests {
    use super::curved_shaper;
    use ni_file::kontakt::objects::Breakpoint;

    fn at(points: &[(f64, f64)], x: f64) -> f64 {
        let i = points.iter().position(|p| p.0 >= x).unwrap();
        if i == 0 {
            return points[0].1;
        }
        let (a, b) = (points[i - 1], points[i]);
        a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
    }

    fn seg(y0: f32, y1: f32, c: f32) -> Vec<(f64, f64)> {
        curved_shaper(&[
            Breakpoint {
                x: 0.0,
                y: y0,
                curve: c,
            },
            Breakpoint {
                x: 1.0,
                y: y1,
                curve: 0.0,
            },
        ])
    }

    #[test]
    fn a_positive_rising_segment_bows_below_the_chord_like_unas_depth_shaper() {
        // Una Corda Cotton "Depth": curvature 0.118, GUI mid value 0.29-0.30.
        let p = seg(0.0, 1.0, 0.11824325);
        assert!((at(&p, 0.5) - 0.28).abs() < 0.03, "{}", at(&p, 0.5));
    }

    #[test]
    fn curvature_sign_is_geometric_not_directional() {
        // Vista CC100 shaper: a falling segment with negative curvature
        // bows above the chord, and mirrors the rising positive one.
        let up = seg(0.0, 1.0, 0.15);
        let down = seg(1.0, 0.0, -0.15);
        assert!(at(&down, 0.5) > 0.5);
        assert!((at(&up, 0.5) + at(&down, 0.5) - 1.0).abs() < 1e-9);
        let flat = seg(0.0, 1.0, 0.0);
        assert_eq!(flat.len(), 2);
    }
}

#[cfg(test)]
mod census {
    #[test]
    #[ignore]
    fn mic_census() {
        let root = std::env::var("KONTRA_KONTAKT_LIBRARIES").unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) {
                    files.push(p)
                }
            }
        }
        files.sort();
        for f in &files {
            let Ok(k) = super::read(f) else { continue };
            let i = &k.instrument;
            let mut outs = std::collections::BTreeSet::new();
            for g in &i.groups {
                outs.insert(format!("{:?}", g.output));
            }
            let names: std::collections::BTreeSet<_> =
                i.groups.iter().map(|g| g.name.as_str()).collect();
            let buses: Vec<_> = i.buses.iter().map(|b| b.name.as_str()).collect();
            println!(
                "CENSUS\t{}\tgroups={}\tdistinct_names={}\touts={}\tbuses={:?}\tnames={:?}",
                f.display(),
                i.groups.len(),
                names.len(),
                outs.len(),
                buses,
                names.iter().take(12).collect::<Vec<_>>()
            );
        }
    }
}
