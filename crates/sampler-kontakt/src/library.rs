//! Real Kontakt instruments (NKI, plain or encrypted) to the semantic IR,
//! using the vendored `ni-file` decoders the v1 importer is built on. The
//! translator states only what it decodes; everything else it finds is listed
//! in [`ir::Instrument::unsupported`] with its source location.

use crate::{LoadError, Samples, time_stretch_writes};
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
const ZONE_SLICES: u16 = 0x35;
const INTERNAL_MODS: u16 = 0x3b;
const EXTERNAL_MODS: u16 = 0x3c;
const FILE_TABLE: u16 = 0x4b;
const LEGACY_FILE_TABLE: u16 = 0x3d;
const SCRIPT: u16 = 6;

mod impulse_namespace;
mod zone_routing;
use impulse_namespace::EffectFileOrigins;

/// Kontakt's release-trigger-counter source (source type 10). The producer
/// at decompile 0x14084f380 returns `(T - held) / T`, clamped at 0, only for a
/// release-trigger group with `T > 0` (`T` in ms, the group's `rls_trig_counter`);
/// every other group holds 1.
fn release_counter_source(release_trigger: bool, counter_ms: i32) -> ir::ModulationSource {
    if release_trigger && counter_ms > 0 {
        ir::ModulationSource::ReleaseCounter(ir::Time::Milliseconds(f64::from(counter_ms)))
    } else {
        ir::ModulationSource::Constant
    }
}

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
    read_with_controls(path, &[])
}

/// [`read`] with host-saved scalar values applied before script initialization.
pub fn read_with_controls(
    path: &Path,
    control_values: &[(sampler_core::ControlId, i32)],
) -> Result<Kontakt, LoadError> {
    read_overlaid(path, None, control_values)
}

/// [`read`] with a snapshot's saved native and script state applied.
pub fn read_with_snapshot(
    path: &Path,
    snapshot: &crate::SnapshotState,
) -> Result<Kontakt, LoadError> {
    read_with_snapshot_and_controls(path, snapshot, &[])
}

/// Snapshot state with host overrides applied before script initialization.
pub fn read_with_snapshot_and_controls(
    path: &Path,
    snapshot: &crate::SnapshotState,
    control_values: &[(sampler_core::ControlId, i32)],
) -> Result<Kontakt, LoadError> {
    let mut kontakt = read_overlaid(path, Some(snapshot), control_values)?;
    crate::apply_snapshot(&mut kontakt, snapshot).map_err(|e| LoadError::Invalid {
        path: path.into(),
        reason: format!("snapshot persistent values: {:?} at {}", e.kind, e.offset),
    })?;
    Ok(kontakt)
}

fn read_overlaid(
    path: &Path,
    snapshot: Option<&crate::SnapshotState>,
    control_values: &[(sampler_core::ControlId, i32)],
) -> Result<Kontakt, LoadError> {
    let path = path.canonicalize().map_err(|e| LoadError::io(path, e))?;
    if snapshot.is_none() {
        if let Some(cached) = crate::cache::load(&path, 0, control_values) {
            return Ok(cached);
        }
    }
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
    let (table, others, containers) = match chunks.find_first(FILE_TABLE) {
        Some(chunk) => {
            let t = FNTableImpl::try_from(chunk).map_err(|e| decode("sample file table", e))?;
            (
                t.sample_filetable,
                t.other_filetable,
                t.special_filetable
                    .into_values()
                    .filter(|name| name.to_ascii_lowercase().ends_with(".nkr"))
                    .collect(),
            )
        }
        None => {
            let chunk = chunks
                .find_first(LEGACY_FILE_TABLE)
                .ok_or_else(|| invalid("missing sample file table"))?;
            let t =
                FileNameListPreK51::try_from(chunk).map_err(|e| decode("legacy file table", e))?;
            (
                t.sample_filetable,
                t.other_filetable,
                t.special_filetable
                    .into_values()
                    .filter(|name| name.to_ascii_lowercase().ends_with(".nkr"))
                    .collect(),
            )
        }
    };
    drop(span);
    let _span = crate::audit::Span::new("translate_resolve_ir");
    translate(
        path,
        program,
        table,
        others,
        containers,
        snapshot,
        control_values,
        snapshot.is_none().then_some(0),
    )
    .map_err(|e| e.at(crate::Stage::Translate))
}

/// Translate program `index` (0-based, in slot order) of the multi at `path`;
/// the programs of a `.nkm` are the instruments of a rack.
pub fn read_program(path: &Path, index: usize) -> Result<Kontakt, LoadError> {
    read_program_with_controls(path, index, &[])
}

/// [`read_program`] with host overrides applied before script initialization.
pub fn read_program_with_controls(
    path: &Path,
    index: usize,
    control_values: &[(sampler_core::ControlId, i32)],
) -> Result<Kontakt, LoadError> {
    use ni_file::kontakt::objects::Bank;
    let path = path.canonicalize().map_err(|e| LoadError::io(path, e))?;
    if let Ok(index) = u32::try_from(index) {
        if let Some(cached) = crate::cache::load(&path, index, control_values) {
            return Ok(cached);
        }
    }
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
    let (table, others, containers) = match chunks
        .filename_tables()
        .map_err(|e| decode("multi file table", e))?
    {
        Some(t) => (
            t.sample_filetable,
            t.other_filetable,
            t.special_filetable
                .into_values()
                .filter(|name| name.to_ascii_lowercase().ends_with(".nkr"))
                .collect(),
        ),
        None => {
            // filename_table() exposes only samples; retain the legacy IR and NKR tables too.
            let chunk = chunks
                .find_first(LEGACY_FILE_TABLE)
                .ok_or_else(|| invalid("missing multi sample table"))?;
            let t = FileNameListPreK51::try_from(chunk)
                .map_err(|e| decode("multi legacy file table", e))?;
            (
                t.sample_filetable,
                t.other_filetable,
                t.special_filetable
                    .into_values()
                    .filter(|name| name.to_ascii_lowercase().ends_with(".nkr"))
                    .collect(),
            )
        }
    };
    drop(span);
    let _span = crate::audit::Span::new("translate_resolve_ir");
    translate(
        path,
        program,
        table,
        others,
        containers,
        None,
        control_values,
        u32::try_from(index).ok(),
    )
    .map_err(|e| e.at(crate::Stage::Translate))
}

#[allow(clippy::too_many_arguments)]
fn translate(
    path: PathBuf,
    mut program: Program,
    table: HashMap<u32, String>,
    others: HashMap<u32, String>,
    containers: Vec<String>,
    snapshot: Option<&crate::SnapshotState>,
    control_values: &[(sampler_core::ControlId, i32)],
    cache_program: Option<u32>,
) -> Result<Kontakt, LoadError> {
    let effect_files = EffectFileOrigins::overlay(&mut program, snapshot);
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
        retained_snapshot: Default::default(),
        engine: Vec::new(),
        dynamic: false,
        eq_mod_slots: HashMap::new(),
        cutoff_octaves: HashMap::new(),
        route_intensity: HashMap::new(),
        alias_claims: HashMap::new(),
        intensity_scales: Vec::new(),
        lane_slots: Vec::new(),
        intensity_slots: HashMap::new(),
        engine_by_group: Default::default(),
        dormant_modulators: Default::default(),
        amplitude_envelopes: Vec::new(),
        send_taps: Vec::new(),
        #[cfg(feature = "scan")]
        target_outcomes: HashMap::new(),
    };
    // Kontakt stores the preload in bytes (`dfdChannelPreloadSize`, Kontakt 8 `0x21b70`).
    // The global `overrideInstPreloadSize` replaces it only when the user preference
    // `overrideInstDFD` is on (`FUN_14082a530`), which a library load never sets, so the
    // authored value is in effect and 0 is a zero window, not a policy selector. 0xf000 is
    // the init default (`FUN_140940000`). The stream head does not use the window yet.
    if !matches!(params.dfd_channel_preload_size, 0 | 0xf000) {
        out.unsupported(
            "instrument",
            "DFD preload window not applied to stream head",
            params.dfd_channel_preload_size,
            ir::Reason::NotModeled,
        );
    }
    match crate::program_automation(
        &program.0.private_data,
        program.version(),
        crate::Limits {
            bytes: 64 << 20,
            records: 65536,
        },
    ) {
        Ok(records) => {
            for record in records {
                let target = std::str::from_utf8(record.tag.data())
                    .ok()
                    .and_then(automation_target);
                let source = match record.mode {
                    1 if record.address < 128 => {
                        Some(ir::ScriptAutomationSource::Controller(record.address as u8))
                    }
                    2 => Some(ir::ScriptAutomationSource::HostParameter(record.address)),
                    _ => None,
                };
                if let (Some((source_slot, slider, switch)), Some(source)) = (target, source)
                    && (0.0..=1.0).contains(&record.low)
                    && (0.0..=1.0).contains(&record.high)
                {
                    out.ir.script_automation.push(ir::ScriptAutomation {
                        source,
                        source_slot,
                        switch,
                        slider,
                        low: f64::from(record.low),
                        high: f64::from(record.high),
                        soft_takeover: record.soft_takeover,
                    });
                } else {
                    out.unsupported(
                        "program private",
                        "saved automation target",
                        format!(
                            "{} record {} mode {} address {} id {} {:?}",
                            String::from_utf8_lossy(record.tag.data()),
                            record.offset,
                            record.mode,
                            record.address,
                            record.id,
                            record.secondary_id
                        ),
                        ir::Reason::NotModeled,
                    );
                }
            }
        }
        Err(error) => out.unsupported(
            "program private",
            "saved automation layout",
            error,
            ir::Reason::NotModeled,
        ),
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
    let mut source_event_processors = Vec::new();
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
        // Preserve the complete native SCRIPT chunk order, including empty and
        // saved-bypass slots. Runtime Behavior ordinals are not source IDs.
        let source_slot = u32::try_from(slot).map_err(|_| invalid("script source ordinal exceeds source identity domain"))?;
        source_event_processors.push(ir::SourceEventProcessor {
            id: ir::SourceEventProcessorId::KontaktScriptSlot(source_slot),
            owner: ir::SourceOwnerId::KontaktInstrument,
            ordinal: source_slot,
            kind: if script.text.as_ref().is_some_and(|text| !text.trim().is_empty()) {
                "KspScript"
            } else if script.textfile_name.as_ref().is_some_and(|name| !name.is_empty()) {
                "UnresolvedKspScript"
            } else { "EmptyKspSlot" }.into(),
            bypass: ir::SourceSwitch { saved: script.bypass, control: None },
        });
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
        out.source_modulators(index, group)
            .map_err(|e| decode("source modulation names", e))?;
    }
    // Scripts first: what `on init` writes with set_engine_par (effect gains,
    // bypass) is the rack's state, so racks are translated after it.
    let group_names: Vec<String> = groups
        .groups
        .iter()
        .map(|g| g.params().map(|p| p.name))
        .collect::<Result<_, _>>()
        .map_err(|e| decode("source group names", e))?;
    let zone_data = &program.0.find_first(ZONE_LIST)
        .ok_or_else(|| invalid("missing zone list"))?.data;
    // Initializers may query/attach physical zones before effect/group DSP
    // admission. Read only bounded serialized metadata here, never samples.
    let span = crate::audit::Span::new("translate_zone_metadata_prepass");
    let (catalog, identity_layouts) = source_zone_catalog_with_layouts(zone_data,group_names.len())
        .map_err(|reason|invalid(&reason))?;
    out.ir.source_indices.zone_catalog = catalog;
    let zone_metadata = zone_catalog_homes(&out.ir.source_indices.zone_catalog);
    let unavailable=out.ir.source_indices.zone_catalog.iter().filter(|z|z.id.is_none()).count();
    if unavailable!=0 {
        out.unsupported("zone catalog","native source zone identity",
            format!("{unavailable} identities unavailable; no ordinal alias was created; {identity_layouts}"),ir::Reason::NotModeled);
    }
    drop(span);
    if let Some(snapshot) = snapshot {
        out.ir.snapshot_base_state = out.ir.behaviors.iter().map(|b| b.state.clone()).collect();
        for behavior in &mut out.ir.behaviors {
            if let Some(entries) = behavior
                .slot
                .and_then(|slot| snapshot.persistent.get(usize::from(slot)))
            {
                for (name, value) in saved(entries).map_err(|e| {
                    invalid(&format!(
                        "snapshot persistent values: {:?} at {}",
                        e.kind, e.offset
                    ))
                })? {
                    match behavior.state.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = value,
                        None => behavior.state.push((name, value)),
                    }
                }
            }
        }
    }
    let span = crate::audit::Span::new("translate_ksp_init");
    let mut initialized = crate::load::initialize_scripts_with_zones(
        &mut out.ir, Some(&path), group_names, control_values, &zone_metadata,
    );

    let zone_routes = zone_routing::Routes::new(&initialized,&zone_metadata,groups.groups.len())
        .map_err(|reason|invalid(&reason))?;
    out.ir.zone_routing_mode=zone_routes.mode;
    for entry in &mut out.ir.source_indices.zone_catalog {
        if let Some(home)=entry.id.and_then(|id|zone_routes.home(id)) {
            entry.home=ir::SourceZoneHome {group:home[0] as u32,low_key:home[1] as u8,high_key:home[2] as u8};
        }
    }
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
    out.ir.source_parameters.extend(
        crate::source_parameters::program(&program)
            .map_err(|e| decode("authored program FX", e))?,
    );
    drop(span);
    let mut translated = Vec::new();
    for (index, group) in groups.groups.iter().enumerate() {
        translated.push(out.group(index, group).map_err(|e| decode("group", e))?);
    }
    out.flush_retained_snapshot_state();
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
    let mut samples = Samples::instrument(root, &path)?;
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
    let mut impulse_sources = HashMap::new();
    let mut impulse_params = Vec::new();
    {
        // Convolution impulse responses are named by the other-files table.
        let mut load = |request: crate::effects::ImpulseRequest| -> Result<crate::effects::Decoded, String> {
            let name = effect_files.name(request, &others, snapshot)?;
            let at = samples
                .resolve_impulse(parent, name, &containers)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("{name} was not found"))?;
            impulse_sources.insert(request, at.clone());
            let decoded = samples.decode(&at).map_err(|e| e.to_string())?;
            Ok((decoded.rate, decoded.frames))
        };
        let (notes, send_buses) = crate::effects::instrument_buses_with_recipes(
            &mut out.ir,
            &racks,
            &buses,
            dynamic,
            &mut load,
            Some(&mut impulse_params),
        );
        for (at, (slot, feature, value, reason)) in notes {
            out.unsupported(&format!("{at} slot {slot}"), &feature, value, reason);
        }
        out.resolve_send_taps(&send_buses);
    }
    drop(span);
    let span = crate::audit::Span::new("translate_zones_sample_resolve");
    // Racks of buses no group feeds do nothing, so they are not reported.
    let mut resolved = HashMap::new();
    let data = zone_data;
    let mut r = Cursor::new(data.as_slice());
    let count = u32le(&mut r).map_err(|e| LoadError::io(&path, e))? as usize;
    if count > data.len() / 8 {
        return Err(invalid("zone count exceeds the zone list"));
    }
    out.ir.source_indices.zones.resize(count, None);
    let addressed = addressed(&out.ir.source_indices.zone_catalog);
    for index in 0..count {
        let mut zone =
            raw_zone(&mut r).map_err(|reason| invalid(&format!("zone {index}: {reason}")))?;
        zone.source_id = out.ir.source_indices.zone_catalog.get(index).and_then(|z| z.id)
            .filter(|_| addressed.get(index).copied().unwrap_or(false));
        if !zone_metadata.is_empty() {
            if let Some(id)=zone.source_id {
                zone_routes.apply_initial(id,&mut zone).map_err(|reason|invalid(&reason))?;
            }
        }
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
        if out.ir.groups[group.group.0].beat_slices.is_some() {
            // Beat Machine zones carry their own grid; a zone without one plays as a sampler.
            let at = format!("zone {index} (group {})", group.index);
            let grid = ir::BeatGrid::from_markers(&zone.slice_markers, zone.slice_tempo);
            zone.beat_grid = grid.unwrap_or_else(|reason| {
                out.unsupported(
                    &at,
                    "beat machine slice markers",
                    reason,
                    ir::Reason::NotModeled,
                );
                None
            });
            if zone.beat_grid.is_none() {
                out.unsupported(
                    &at,
                    "beat machine zone without slice markers (played as a sampler)",
                    zone.slice_markers.len(),
                    ir::Reason::NotModeled,
                );
            }
        }
        if !zone.sample_present {
            continue;
        } // Authored empty zone, no sample to resolve.
        if let Some(range) = &zone.unreachable {
            out.unsupported(&format!("zone {index}"), "unreachable zone", range, ir::Reason::InvalidValue);
            continue; // No MIDI note can trigger it.
        }
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
        let template=zone.clone();
        let home=[zone.group as u32,u32::from(zone.keys[0]),u32::from(zone.keys[1])];
        out.zone(index, zone, end, group, &params, location.clone());
        if out.ir.zones.len() > before {
            out.ir.source_indices.zones[index] = Some(ir::ZoneRef(before));
            if zone_routes.enabled && let Some(source_zone)=template.source_id {
                let mut variants=vec![ir::ZoneRouteVariant {group:home[0],zone:Some(ir::ZoneRef(before))}];
                for destination in zone_routes.destinations(home[0] as usize) {
                    let at=out.ir.zones.len();
                    if let Some(Some(group))=translated.get(destination) {
                        let mut variant=template.clone();
                        variant.group=destination;
                        out.zone(index,variant,end,group,&params,location.clone());
                    }
                    variants.push(ir::ZoneRouteVariant {group:destination as u32,
                        zone:(out.ir.zones.len()>at).then_some(ir::ZoneRef(at))});
                }
                out.ir.zone_routing.push(ir::ZoneRouting {source_zone,
                    group:home[0],low_key:home[1] as u8,high_key:home[2] as u8,variants});
            }
        }
    }
    drop(span);
    let _span = crate::audit::Span::new("translate_keys_validate");
    out.apply_intensity_scales();
    crate::keyswitch::translate(&mut out.ir, &out.start_criteria);
    out.ir.register_compressor_controls();
    retain_kontakt_source_owners(&mut out.ir, source_event_processors)
        .map_err(|reason| invalid(&reason))?;
    out.ir.unsupported.dedup();
    #[cfg(feature = "scan")]
    {
        out.ir.dsp_slots = crate::coverage::slots(
            &program,
            &out.ir,
            dynamic,
            &out.engine,
            &out.target_outcomes,
        )
        .ok();
        out.ir.native_start_mod_groups = crate::coverage::start_mod_groups(&program).ok();
    }
    out.ir.validate().map_err(|e| invalid(&e.to_string()))?;
    // Keep validated zone storage sized to admitted content. This runs before
    // publication/cache storage; physical zone indices and holes stay unchanged.
    out.ir.zones.shrink_to_fit();
    if let Some(objects) = &mut out.ir.kontakt_objects {
        objects.zones.shrink_to_fit();
    }
    drop(_span);
    crate::load::admit_initialized_zone_sources(&mut out.ir, &mut initialized);
    let mut kontakt = Kontakt {
        instrument: out.ir,
        locations: out.locations,
        samples,
        initialized: Some(initialized),
    };
    let recipes = impulse_params
        .into_iter()
        .map(|(request, params)| {
            Some(crate::cache::ImpulseRecipe {
                source: impulse_sources.get(&request)?.clone(),
                params,
            })
        })
        .collect::<Option<Vec<_>>>();
    if let (Some(program), Some(recipes)) = (cache_program, recipes) {
        crate::cache::store(&path, program, &mut kontakt, &recipes);
    }
    Ok(kontakt)
}

/// Retain actual native document addresses independently of DSP admission.
/// Kontakt Group effects already have their native per-voice ownership below;
/// this common hierarchy does not turn them into a UVI shared keygroup sum.
fn retain_kontakt_source_owners(
    instrument: &mut ir::Instrument,
    event_processors: Vec<ir::SourceEventProcessor>,
) -> Result<(), String> {
    use ir::{SourceOwner, SourceOwnerId as Id, SourceOwnerKind as Kind,
        SourceInputPredicate as Predicate};
    let group_count = instrument.source_indices.groups.len();
    let catalog = &instrument.source_indices.zone_catalog;
    if catalog.len() != instrument.source_indices.zones.len()
        || group_count > u32::MAX as usize || catalog.len() > u32::MAX as usize {
        return Err("Kontakt source hierarchy coordinate range is invalid".into());
    }
    let mut owners = Vec::with_capacity(1 + group_count + catalog.len());
    owners.push(SourceOwner { id: Id::KontaktInstrument, parent: None,
        kind: Kind::Instrument,
        children: (0..group_count).map(|group| Id::KontaktGroup(group as u32)).collect(),
        groups: vec![], zones: vec![], predicates: vec![], bypass: None, mute: None,
        event_processors });
    for group in 0..group_count {
        let runtime = instrument.source_indices.groups[group];
        let mut predicates = Vec::new();
        if runtime.is_none() {
            predicates.push(Predicate::Unsupported { field: "kontakt-group-not-compiled".into() });
        } else if runtime.is_some_and(|runtime| !instrument.groups[runtime.0].start.is_empty()) {
            // The actual criterion/RR program remains the existing native
            // group owner. A copied keys-only predicate must not bypass it.
            predicates.push(Predicate::Unsupported { field: "kontakt-group-start-criteria".into() });
        }
        owners.push(SourceOwner { id: Id::KontaktGroup(group as u32),
            parent: Some(Id::KontaktInstrument), kind: Kind::KontaktGroup,
            children: catalog.iter().enumerate().filter(|(_, zone)| zone.home.group as usize == group)
                .map(|(zone, _)| Id::KontaktZone(zone as u32)).collect(),
            groups: runtime.into_iter().collect(), zones: vec![], predicates,
            // Native group mute lives in the existing GroupParams owner, not a
            // new cached boolean or an invented ControlId in this document row.
            bypass: None, mute: None, event_processors: vec![] });
    }
    for (index, source) in catalog.iter().enumerate() {
        if source.home.group as usize >= group_count {
            return Err("Kontakt source zone has an absent native home group".into());
        }
        let primary = instrument.source_indices.zones[index];
        let mut zones: Vec<_> = primary.into_iter().collect();
        if let Some(id) = source.id {
            for routing in instrument.zone_routing.iter().filter(|routing| routing.source_zone == id) {
                for zone in routing.variants.iter().filter_map(|variant| variant.zone) {
                    if !zones.contains(&zone) { zones.push(zone); }
                }
            }
        }
        let mut groups: Vec<_> = instrument.source_indices.groups[source.home.group as usize].into_iter().collect();
        for zone in &zones {
            if let Some(group) = instrument.zones[zone.0].group && !groups.contains(&group) {
                groups.push(group);
            }
        }
        let mut predicates = vec![Predicate::Keys(ir::KeyRange {
            low: source.home.low_key, high: source.home.high_key })];
        if let Some(zone) = primary {
            predicates.push(Predicate::Velocities(instrument.zones[zone.0].velocities));
        } else {
            predicates.push(Predicate::Unsupported { field: "kontakt-zone-not-compiled".into() });
        }
        if source.id.is_some_and(|id| instrument.zone_routing.iter().any(|routing| routing.source_zone == id)) {
            // home is initialized source provenance. Later routing is resolved
            // by the real zone-routing registry, not this immutable parent edge.
            predicates.push(Predicate::Unsupported { field: "kontakt-dynamic-zone-routing".into() });
        }
        owners.push(SourceOwner { id: Id::KontaktZone(index as u32),
            parent: Some(Id::KontaktGroup(source.home.group)), kind: Kind::KontaktZone,
            children: vec![], groups, zones, predicates, bypass: None, mute: None,
            event_processors: vec![] });
    }
    instrument.source_indices.owners = owners;
    Ok(())
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
    /// Retained-but-unproved snapshot group bytes -> the groups carrying them,
    /// reported once per distinct value (see `flush_retained_snapshot_state`).
    retained_snapshot: std::collections::BTreeMap<String, Vec<usize>>,
    /// What the scripts' `on init` wrote with `set_engine_par`.
    engine: Vec<sampler_ksp::EnginePar>,
    /// A script writes effect slots while playing.
    dynamic: bool,
    eq_mod_slots: HashMap<usize, Vec<usize>>,
    cutoff_octaves: HashMap<(usize, usize), f64>,
    send_taps: Vec<(ir::ChainRef, crate::effects::SendTap)>,
    /// Each translated route's saved signed intensity and its depth value per
    /// unit intensity (for the ID25 intensity law's `unit`).
    route_intensity: HashMap<usize, (f64, f64)>,
    /// `claim_alias`'s index of contested (parameter, address) script aliases.
    alias_claims: HashMap<(String, ir::SlotAddress), usize>,
    /// Assignments whose destination is another modulator's `intensity`; applied
    /// to that modulator's routes once every route exists.
    intensity_scales: Vec<IntensityScale>,
    /// Module slot -> processor index of the current group's script-addressed
    /// effect modules (distortion, saturation, formant, lo-fi).
    lane_slots: Vec<(usize, usize)>,
    /// First modulator slot per (group, lower-case name) in `engine_lookups`,
    /// so `script_intensity` is a map probe instead of a scan of every lookup.
    intensity_slots: HashMap<(i32, String), i32>,
    /// `engine` positions per group, built on first read (writes come first).
    engine_by_group: std::cell::OnceCell<HashMap<i32, Vec<usize>>>,
    /// (group, internal slot) of saved-bypassed modulators nothing can wake.
    dormant_modulators: std::collections::HashSet<(usize, usize)>,
    /// Each group's own amplitude envelope, once its internal modulators are
    /// indexed. A route can address a stage time only of that envelope.
    amplitude_envelopes: Vec<Option<ir::ModulatorRef>>,
    #[cfg(feature = "scan")]
    target_outcomes: crate::coverage::Targets,
}

/// The internal modulator's single route is the primary amplitude at full depth.
fn primary_volume(params: &ni_file::kontakt::objects::InternalModParams) -> bool {
    matches!(params.targets.as_slice(), [t]
        if t.param == "volume" && t.signed_intensity() == 1.0 && !t.invert
            && t.slot.is_none() && t.lag_ms == 0
            && !t.shaper.as_ref().is_some_and(|s| s.enabled))
}

/// A saved-bypassed internal modulator that nothing can wake is not
/// instantiated, so routes into it (phase, rate) are inert, as natively.
/// The primary amplitude/Flex admission is unchanged; only connected outgoing
/// AHDSR/LFO routes can carry a live bypass.
fn dormant(params: &ni_file::kontakt::objects::InternalModParams) -> bool {
    params.unknown_flags[1] != 0 && !wakeable(params)
}

/// Whether the modulator's outgoing routes carry a live bypass lane.
fn wakeable(params: &ni_file::kontakt::objects::InternalModParams) -> bool {
    use ni_file::kontakt::objects::Modulator;
    let retrigger = params.unknown_flags[2] != 0;
    (matches!(&params.modulator, Modulator::Lfo(_))
        || matches!(&params.modulator, Modulator::Ahdsr(_)) && retrigger && !primary_volume(params))
        && params.targets.iter().all(|t| {
            // Module targets (filter, effect and formant knobs) carry the
            // same live depth lane as volume/pitch/pan; a route whose depth
            // cannot be bound is dropped and reported below. Rate targets use
            // that lane after their physical destination is resolved, but only
            // for already-active sources: a newly added bypassed rate route
            // changes the saved rate through the lane's inverse/forward pair.
            // Phase, intensity and sample-position have separate admission.
            !matches!(
                t.param.as_str(),
                "startPhase" | "intensity" | "playPos" | "loopStart" | "loopLength"
            ) && (t.param != "frequency" || params.unknown_flags[1] == 0)
                && t.unknown_flags & 2 == 0
                && !t.shaper.as_ref().is_some_and(|s| s.enabled)
                && (0. ..=1.).contains(&t.intensity)
        })
}

fn eq_knob(name: &str) -> Option<(usize, ir::ProcessorParameter)> {
    for (prefix, parameter) in [
        ("eqGain", ir::ProcessorParameter::Gain),
        ("eqFreq", ir::ProcessorParameter::Cutoff),
        ("eqBandwidth", ir::ProcessorParameter::Resonance),
        // The registry name (FUN_140981410 id 41/44/47) spells it `eqBandWidth`.
        ("eqBandWidth", ir::ProcessorParameter::Resonance),
    ] {
        if let Some(rest) = name.strip_prefix(prefix) {
            let band = rest.parse::<u8>().ok().filter(|b| (1..=3).contains(b))?;
            return Some((usize::from(band - 1), parameter));
        }
    }
    None
}

/// A group-level engine write as the corpus spells it (`set_engine_par(p, v, group, -1, -1)`),
/// falling back to the generic-0 spelling, which Kontakt also honours for these parameters
/// (measured with tm_sweep.py). The last matching write wins.
fn group_engine_write(writes: &[sampler_ksp::EnginePar], parameter: &str, group: i32) -> Option<i32> {
    let find = |generic: i32| {
        writes
            .iter()
            .rev()
            .find(|w| {
                w.parameter.trim_start_matches('$') == parameter
                    && (w.group, w.slot, w.generic) == (group, -1, generic)
            })
            .map(|w| w.value)
    };
    find(-1).or_else(|| find(0))
}

/// Zone velocity response for a velocity-driven plain volume route, or `None` when the route stays on
/// the generic modulation path. Full intensity is gain x velocity. Kontakt's default intensity
/// (50 of 127, the Beat Machine route, oracle session B) follows `VolumeTable`. Other intensities
/// are not measured and keep the generic path.
fn velocity_volume_law(intensity: f64) -> Option<ir::VelocityResponse> {
    const DEFAULT_INTENSITY: f64 = 50.0 / 127.0;
    if intensity == 1.0 {
        Some(ir::VelocityResponse::Linear)
    } else if (intensity - DEFAULT_INTENSITY).abs() < 1e-6 {
        Some(ir::VelocityResponse::VolumeTable)
    } else {
        None
    }
}

/// The Beat Machine controls of a stored mode-4 record: `slice_float_1` is Slice Atk and
/// `slice_float_2` Slice Rel (both milliseconds, in control order), `slice_flag_1` the flag.
fn beat_slices_from_record(
    params: &ni_file::kontakt::objects::BParSrcMode,
) -> Option<ir::BeatSlices> {
    use ni_file::kontakt::objects::SourceValue as V;
    let value = |name: &str| {
        params
            .fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value)
    };
    match (
        value("slice_float_1")?,
        value("slice_float_2")?,
        value("slice_flag_1")?,
    ) {
        (V::Float(attack_ms), V::Float(release_ms), V::Flag(flag)) => Some(ir::BeatSlices {
            attack_ms,
            release_ms,
            flag,
        }),
        _ => None,
    }
}

/// Stored (serialized) group source modes, in the order of Kontakt 8.13.1's own
/// enum-name table (strings at file offset 0x7555700 of Kontakt8.exe: Sampler, ToneMachine, TimeMachine,
/// DirectFromDisk, BeatMachine, TimeMachine2, S1200Machine, MP60Machine,
/// TimeMachinePro; Wavetable stored last as 9). Field shapes agree: only TM2 (5)
/// and Pro (8) carry the extra fields their controls need, DFD (3) none.
/// (docs/architecture-v2/KONTAKT_OBJECTS.md's table naming 3 as Time Machine 2 and
/// 5 as DFD is wrong; Conflux's 90 mode-3 groups are DFD.)
const SOURCE_MODE_TONE: u32 = 1;
const SOURCE_MODE_TM1: u32 = 2;
const SOURCE_MODE_TM2: u32 = 5;
const SOURCE_MODE_TM_PRO: u32 = 8;

/// Stored (serialized) group source mode of DFD. The menu lists Sampler, DFD,
/// Wavetable, Tone Machine, Time Machine 1/2/Pro, Beat Machine, S1200 and MP60;
/// the stored order is Sampler 0, Tone/Time Machine 1-2, DFD 3, Beat Machine 4
/// (slice fields), Time Machine 2 5, S1200/MP60 6-7, Time Machine Pro 8 (pro
/// fields), Wavetable 9. Mode 3 has no fields beyond the common record, is the
/// only non-sampler mode in the 3,906 source groups of the survey's streaming
/// libraries, and mode 5 (which DFD's streaming fields would occupy) never occurs.
const SOURCE_MODE_DFD: u32 = 3;

/// The machine kind of a stored source mode (4 Beat Machine, 6 S1200, 7 MP60).
fn machine_kind(mode: u32) -> Option<ir::Machine> {
    match mode {
        4 => Some(ir::Machine::BeatMachine),
        6 => Some(ir::Machine::S1200 { filter_mode: None }),
        7 => Some(ir::Machine::Mp60),
        _ => None,
    }
}

fn source_mode_name(mode: u32) -> &'static str {
    match mode {
        2 => "Time Machine",
        4 => "Beat Machine",
        5 => "Time Machine 2",
        6 => "S1200 Machine",
        7 => "MP60 Machine",
        8 => "Time Machine Pro",
        _ => "unknown",
    }
}

type Note = (&'static str, String, ir::Reason);

/// Map a Time Machine family record (stored modes 2, 5, 8) to the IR, pushing
/// what the core does not reproduce natively onto `notes`.
///
/// Spec s.3: speed is the common F at wire +7 (default 1.0) passed through the
/// note-value helper, which returns it unchanged unless the timing F at +17 is
/// above 1e-9 (tempo sync; the sync law is incomplete, so reported). TM2 and
/// Pro floor the base rate at 0.01 (0x140ce6c00); TM1 does not.
fn stretch_from_record(
    params: &ni_file::kontakt::objects::BParSrcMode,
    notes: &mut Vec<Note>,
) -> Option<ir::TimeStretch> {
    use ni_file::kontakt::objects::SourceValue as V;
    let f = |name: &str| {
        params.fields.iter().find_map(|f| match (f.name == name, f.value) {
            (true, V::Float(v)) => Some(v),
            _ => None,
        })
    };
    let b = |name: &str| {
        params.fields.iter().find_map(|f| match (f.name == name, f.value) {
            (true, V::Flag(v)) => Some(v),
            _ => None,
        })
    };
    let i = |name: &str| {
        params.fields.iter().find_map(|f| match (f.name == name, f.value) {
            (true, V::Integer(v)) => Some(v),
            _ => None,
        })
    };
    let kind = match params.mode {
        SOURCE_MODE_TONE => ir::StretchKind::ToneMachine,
        SOURCE_MODE_TM1 => ir::StretchKind::TimeMachine1,
        SOURCE_MODE_TM2 => ir::StretchKind::TimeMachine2,
        SOURCE_MODE_TM_PRO => ir::StretchKind::TimeMachinePro,
        _ => return None,
    };
    let mut speed = f("common_float_7")?;
    if f("timing_value").is_some_and(|v| v > 1e-9) {
        notes.push(("time machine tempo-synced speed", format!("{speed}"), ir::Reason::UnknownLaw));
    }
    if !matches!(kind, ir::StretchKind::TimeMachine1 | ir::StretchKind::ToneMachine) {
        speed = speed.max(0.01);
    }
    if !(speed.is_finite() && (0.0..=64.0).contains(&speed)) {
        notes.push(("time machine speed", format!("{speed}"), ir::Reason::InvalidValue));
        speed = 1.0;
    }
    let mut t = ir::TimeStretch::new(kind, speed);
    match kind {
        ir::StretchKind::ToneMachine => {
            // Tone Machine shares the Time Machine 1 machine-record fields.
            t.smooth = f("machine_float_1")?;
            t.grain_ms = f("machine_float_2")?;
            t.high_quality = b("machine_flag")?;
            notes.push((
                "tone machine backend (stand-in granular engine, unmeasured)",
                "tone".into(),
                ir::Reason::NotModeled,
            ));
        }
        ir::StretchKind::TimeMachine1 => {
            t.smooth = f("machine_float_1")?;
            t.grain_ms = f("machine_float_2")?;
            t.high_quality = b("machine_flag")?;
            if t.high_quality {
                notes.push((
                    "time machine 1 HQ grain analysis (non-HQ scheduler used)",
                    "hq".into(),
                    ir::Reason::NotModeled,
                ));
            }
        }
        ir::StretchKind::TimeMachine2 => {
            t.transient_copy = b("dfd_flag")?;
            t.transient_size = i("dfd_integer_1")?;
            t.grain_profile = i("dfd_integer_2")?;
            notes.push((
                "time machine 2 PSOLA mark analysis and transient scheduling (stand-in granular engine)",
                format!("grain profile {}", t.grain_profile),
                ir::Reason::NotModeled,
            ));
        }
        ir::StretchKind::TimeMachinePro => {
            if params.version == 0x100 {
                notes.push(("time machine pro v0x100 record", "legacy order".into(), ir::Reason::NotModeled));
            } else {
                t.high_quality = b("pro_flag_1")?;
                t.keep_formants = b("pro_flag_2")?;
                t.formant_shift = f("pro_float_1")?;
                t.env_order = f("pro_float_2")?;
            }
            notes.push((
                "time machine pro Elastique backend (stand-in granular engine)",
                "pro".into(),
                ir::Reason::NotModeled,
            ));
        }
    }
    Some(t)
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
        exclusive_set: u32::try_from(v.exclusion_group).ok().filter(|&set| set < 16),
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
    let (attack, fall) = ahdsr_coefficients(curve);
    (ir::Curve::Exponential(attack), ir::Curve::Exponential(fall))
}

/// Stored AHDSR times are already milliseconds: the 15 s / 25 s laws
/// (`fconv_u2n_env_a` / `_d`) apply to script and automation values in 0..1, not
/// to these fields. Running them here clamped every stored time of 1 ms or more
/// to the law's maximum (Dolce staccato: 80 ms attack became 15 s, silent).
/// Corpus evidence: stored attacks are 80/125/200 ms, releases 120/351/1050 ms.
fn ahdsr_attack_ms(ms: f32) -> f32 {
    ms.max(0.0)
}

fn ahdsr_decay_ms(ms: f32) -> f32 {
    ms.max(0.0)
}

fn ahdsr_sustain(knob: f32) -> f32 {
    let x = knob.clamp(0.0, 1.0);
    x * x * x
}

fn ahdsr_coefficients(curve: f32) -> (f64, f64) {
    let c = f64::from(curve.clamp(-1.0, 1.0));
    // The engine rounds the base to f32 before its power.
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let attack = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    (attack, (3.0f64 / 43.0).ln())
}

impl Translation {
    fn resolve_send_taps(&mut self, buses: &[(usize, ir::BusRef)]) {
        for (chain, tap) in std::mem::take(&mut self.send_taps) {
            for (send, level) in tap.levels.into_iter().enumerate() {
                if !level.is_finite() || level < 0.0 {
                    self.unsupported(
                        &format!("voice chain {} slot {}", chain.0, tap.slot),
                        "send level",
                        send.to_string(),
                        ir::Reason::InvalidValue,
                    );
                    continue;
                }
                let Some(&(_, bus)) = buses.iter().find(|(slot, _)| *slot == send) else {
                    if level != 0.0 && !tap.bypass {
                        self.unsupported(
                            &format!("voice chain {} slot {}", chain.0, tap.slot),
                            "send return",
                            send.to_string(),
                            ir::Reason::NotModeled,
                        );
                    }
                    continue;
                };
                self.ir.voice_send_taps.push(ir::VoiceSendTap {
                    chain,
                    position: tap.position,
                    bus,
                    gain: ir::Gain::Linear(f64::from(level)),
                    bypass: tap.bypass,
                    gain_control: None,
                    bypass_control: None,
                    ramp: ir::Time::Seconds(0.0),
                });
            }
        }
    }

    /// The `VoiceGroups` chunk: the instrument's voice limit, a 128-bit set of
    /// saved overrides, then one voice limit per override.
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
                if group.voice_limit.exclusion_group >= 16 {
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
                        .map(|(slot, m)| {
                            m.params().map(|p| {
                                let settings = crate::modulation_objects::external(m.0.version, &p);
                                (slot, p.name, p.targets, settings)
                            })
                        })
                        .collect::<Result<_, _>>()?
                } else {
                    let mut names = Vec::new();
                    let mut amplitude_seen = false;
                    for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
                        let params = modulator.params()?;
                        if let Modulator::Ahdsr(envelope) = &params.modulator {
                            Self::source_envelope(
                                &mut self.ir.source_indices,
                                index,
                                slot,
                                envelope,
                            );
                            if params.targets.iter().any(|t| t.param == "volume") && !amplitude_seen
                            {
                                amplitude_seen = true;
                                self.ir
                                    .source_indices
                                    .ahdsrs
                                    .last_mut()
                                    .unwrap()
                                    .native_amplitude =
                                    Self::native_primary_ahdsr(&modulator, &params, envelope);
                            }
                        }
                        let settings = crate::modulation_objects::internal(modulator.0.version, &params);
                        if dormant(&params) {
                            self.dormant_modulators.insert((index, slot));
                        }
                        names.push((slot, params.name, params.targets, settings));
                    }
                    names
                };
                // Preserve original array framing and nested source bytes before
                // snapshot/init writes or DSP admission. Descriptors are not a codec.
                self.ir
                    .source_parameters
                    .push(crate::modulation_objects::wire(index, external, chunk)?);
                for (slot, name, targets, settings) in names {
                    for target in &targets {
                        if eq_knob(&target.param).is_some()
                            && let Some(slot) = target.slot
                        {
                            let live = self.eq_mod_slots.entry(index).or_default();
                            if !live.contains(&usize::from(slot)) {
                                live.push(usize::from(slot));
                            }
                        }
                    }
                    self.source_modulator(index, slot, external, name, &targets, Some(settings));
                }
            }
        }
        let saved = self.snapshot_groups.get(index);
        let rack = match saved {
            Some(state) => Ok(ni_file::kontakt::objects::BParamArrayBParFX8 {
                version: state.fx.0,
                items: state
                    .fx
                    .1
                    .iter()
                    .map(|slot| {
                        slot.as_ref().map(|(id, data)| ni_file::kontakt::Chunk {
                            id: *id,
                            data: data.clone(),
                        })
                    })
                    .collect(),
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
                        ("FREQ", f64::from(sampler_core::KontaktLegacyEqSettings::frequency_knob(*hz)), 0., 1.),
                        (
                            "BW",
                            f64::from(sampler_core::KontaktLegacyEqSettings::bandwidth_knob(*width)),
                            0.,
                            1.,
                        ),
                    ] {
                        if !value.is_finite() {
                            continue;
                        }
                        let parameter = sampler_core::engine_parameter_id(&format!(
                            "ENGINE_PAR_{name}{}",
                            band + 1
                        ))
                        .unwrap();
                        self.ir
                            .source_indices
                            .engine_values
                            .push(ir::SourceEngineValue {
                                parameter,
                                group: group as i32,
                                slot: slot.slot as i32,
                                generic: -1,
                                value: sampler_core::EngineParameterLaw::Linear { low, high }
                                    .encode(value),
                            });
                    }
                }
            }
        }
    }

    // Port from v1 0cb7a8a0:src/modulation.rs primary AHDSR admission.
    fn native_primary_ahdsr(
        modulator: &ni_file::kontakt::objects::InternalMod,
        params: &ni_file::kontakt::objects::InternalModParams,
        env: &ni_file::kontakt::objects::EnvelopeAhdsr,
    ) -> bool {
        let native_wrapper = modulator.0.private_data.ends_with(&2u32.to_le_bytes())
            && modulator
                .0
                .find_first(7)
                .and_then(|c| ni_file::kontakt::StructuredObject::try_from(c).ok())
                .is_some_and(|w| {
                    w.version == 0x90
                        && w.private_data.is_empty()
                        && w.public_data == [0; 4]
                        && w.children.len() == 1
                        && w.children[0].id == 0x3f
                });
        let default_time = env.unknown_tail.len() == 52
            && env
                .unknown_tail
                .chunks_exact(13)
                .all(|r| r == [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0]);
        native_wrapper
            && params.targets.len() == 1
            && params.targets.iter().all(|t| {
                t.param == "volume"
                    && t.slot.is_none()
                    && t.intensity == 1.
                    && t.unknown_i16 == -1
                    && t.unknown_flags == 0x10
                    && !t.invert
                    && t.lag_ms == 0
                    && !t.shaper.as_ref().is_some_and(|s| s.enabled)
            })
            && params.unknown_flags[0] <= 1
            && params.unknown_flags[1..] == [0, 1, 0]
            && env.unknown_flag <= 1
            && default_time
    }

    fn source_envelope(
        source: &mut ir::SourceIndices,
        group: usize,
        slot: usize,
        envelope: &ni_file::kontakt::objects::EnvelopeAhdsr,
    ) {
        source.ahdsrs.push(ir::SourceAhdsr {
            group,
            slot,
            attack_ms: ahdsr_attack_ms(envelope.attack_ms),
            attack_curve: envelope.attack_curve,
            hold_ms: ahdsr_attack_ms(envelope.hold_ms),
            decay_ms: ahdsr_decay_ms(envelope.decay_ms),
            sustain: ahdsr_sustain(envelope.sustain),
            release_ms: ahdsr_decay_ms(envelope.release_ms),
            ahd_only: envelope.unknown_flag != 0,
            native_amplitude: false,
        });
        use sampler_core::{EngineParameterLaw, EnvelopeStage};
        let (curve, _) = ahdsr_coefficients(envelope.attack_curve);
        for (name, stage, native) in [
            (
                "ENGINE_PAR_ATTACK",
                EnvelopeStage::Attack,
                f64::from(ahdsr_attack_ms(envelope.attack_ms)),
            ),
            (
                "ENGINE_PAR_HOLD",
                EnvelopeStage::Hold,
                f64::from(ahdsr_attack_ms(envelope.hold_ms)),
            ),
            (
                "ENGINE_PAR_DECAY",
                EnvelopeStage::Decay,
                f64::from(ahdsr_decay_ms(envelope.decay_ms)),
            ),
            (
                "ENGINE_PAR_SUSTAIN",
                EnvelopeStage::Sustain,
                f64::from(ahdsr_sustain(envelope.sustain)),
            ),
            (
                "ENGINE_PAR_RELEASE",
                EnvelopeStage::Release,
                f64::from(ahdsr_decay_ms(envelope.release_ms)),
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
        &mut self,
        group: usize,
        slot: usize,
        external: bool,
        name: String,
        targets: &[ni_file::kontakt::objects::ModTarget],
        settings: Option<ir::kontakt::Modulation>,
    ) {
        self.intensity_slots
            .entry((group as i32, name.to_ascii_lowercase()))
            .or_insert(slot as i32);
        self.ir
            .source_indices
            .engine_lookups
            .push(ir::SourceEngineLookup {
                group: group as i32,
                owner: -1,
                target: false,
                name: name.clone(),
                index: slot as i32,
            });
        for (index, target) in targets.iter().enumerate() {
            self.ir
                .source_indices
                .engine_lookups
                .push(ir::SourceEngineLookup {
                    group: group as i32,
                    owner: slot as i32,
                    target: true,
                    name: target.name.clone(),
                    index: index as i32,
                });
        }
        self.ir.source_indices.modulators.push(ir::SourceModulator {
            group,
            slot,
            external,
            name,
            runtime: None,
            settings,
        });
    }

    /// One entry per distinct retained value with its group count, instead of
    /// one per group (thousands on large instruments).
    fn flush_retained_snapshot_state(&mut self) {
        for (value, groups) in std::mem::take(&mut self.retained_snapshot) {
            self.ir.unsupported.push(ir::Unsupported {
                location: format!("snapshot groups ({} of them, first {})", groups.len(), groups[0]),
                feature: "saved release/MIDI/voice state and v4 selection words".into(),
                value,
                reason: ir::Reason::NotModeled,
            });
        }
    }

    /// A group's complete settings, including dormant saved-mute content.
    fn group(&mut self, index: usize, group: &Group) -> Result<Option<GroupInfo>, ni_file::Error> {
        let v = group.params()?;
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
        let overlaid;
        let group = if let Some(state) = &saved {
            overlaid = crate::snapshot::overlay_group(group, state)?;
            self.retained_snapshot
                .entry(format!(
                    "compact bytes 14..24 {:02x?}, selection {}, v4 words {:02x?}",
                    state.compact_state, state.selection, state.v4_words
                ))
                .or_default()
                .push(index);
            &overlaid
        } else {
            group
        };
        let mut v = group.params()?;
        if let Some(state) = &saved {
            v.volume = state.volume;
            v.pan = state.pan;
            v.tune = state.octaves.exp2();
            v.key_tracking = state.key_tracking;
            v.reverse = state.reverse;
        }
        let at = format!("group {index} {:?}", v.name);
        let insert = match &saved {
            Some(state) => Ok(ni_file::kontakt::objects::BParamArrayBParFX8 {
                version: state.fx.0,
                items: state
                    .fx
                    .1
                    .iter()
                    .map(|slot| {
                        slot.as_ref().map(|(id, data)| ni_file::kontakt::Chunk {
                            id: *id,
                            data: data.clone(),
                        })
                    })
                    .collect(),
            }),
            None => group.insert_fx(),
        };
        match &insert {
            Ok(array) => self
                .ir
                .source_parameters
                .extend(crate::source_parameters::rack(
                    &format!("group {index} insert"),
                    array,
                )?),
            Err(error) => self.unsupported(&at, "group insert rack", error, ir::Reason::Unknown),
        }
        let not_modeled = ir::Reason::NotModeled;
        // 1-based (0 = none): Una Corda's groups use 1 and 2 with voice
        // groups 0 and 1 defined, Afflatus 2 Horns KS 1..=8 with 0..=7.
        let voice_group = usize::try_from(v.voice_group_index - 1).ok();
        let voice_limit = voice_group.and_then(|g| {
            match self.voice_groups.get(g).copied() {
                Some(Some(limit)) => Some(limit),
                Some(None) => {
                    // Kontakt manual, Classic view / Voice Groups: all 128
                    // groups are predefined. Native writer 0x140d14978..9ab
                    // omits these defaults; None is an absent override, not
                    // an undefined group (KONTAKT_VOICE_GROUP_DEFAULTS.md).
                    let limit = self.ir.voice_limits.len();
                    self.ir.voice_limits.push(ir::VoiceLimit {
                        voices: 1,
                        kill: ir::Kill::Oldest,
                        prefer_released: true,
                        fade: ir::Time::Milliseconds(10.0),
                        exclusive_set: None,
                    });
                    self.voice_groups[g] = Some(limit);
                    Some(limit)
                }
                None => {
                    self.unsupported(&at, "undefined voice group", g, ir::Reason::InvalidValue);
                    None
                }
            }
        });
        if v.midi_channel >= 0 {
            self.unsupported(&at, "MIDI channel filter", v.midi_channel, not_modeled);
        }
        // Stored and retained, but no shared resampler selects a quality per group yet.
        if v.interp_quality != 0 {
            self.unsupported(&at, "interpolation quality", v.interp_quality, not_modeled);
        }
        // Start options become articulations or reports in `keyswitch::translate`.
        self.start_criteria.push((
            at.clone(),
            ir::GroupRef(self.ir.groups.len()),
            v.start_criteria.clone(),
        ));
        let mut wavetable = None;
        let mut time_stretch = None;
        let mut machine = None;
        let mut beat_slices = None;
        match group.source_identity() {
            Ok(source) if source.mode == 9 => {
                wavetable = group
                    .wavetable_source()?
                    .as_ref()
                    .and_then(|source| crate::wavetable::admitted(source, v.key_tracking));
                if let Some(wave) = &mut wavetable {
                    for (name, target) in [
                        ("ENGINE_PAR_WT_POSITION", &mut wave.position),
                        ("ENGINE_PAR_WT_PHASE", &mut wave.phase),
                        ("ENGINE_PAR_WT_FORM", &mut wave.form1),
                        ("ENGINE_PAR_WT_FORM2", &mut wave.form2),
                    ] {
                        if let Some(value) = self.script_par(name, index as i32, -1, -1) {
                            *target = sampler_core::EngineParameterLaw::Linear { low: 0., high: 1. }
                                .decode(value) as f32;
                        }
                    }
                    for (name, target) in [
                        ("ENGINE_PAR_WT_FORM_MODE", &mut wave.form1_type),
                        ("ENGINE_PAR_WT_FORM2_MODE", &mut wave.form2_type),
                    ] {
                        if let Some(value) = self.script_par(name, index as i32, -1, -1) {
                            if u8::try_from(value).is_ok_and(ir::Wavetable::supports_form) {
                                *target = value as u8;
                            } else {
                                self.unsupported(
                                    &at,
                                    "wavetable phase form write",
                                    value,
                                    ir::Reason::NotModeled,
                                );
                            }
                        }
                    }
                } else {
                    self.unsupported(&at, "wavetable source", source.mode, not_modeled);
                }
            }
            // DFD streams the same sampled signal as Sampler; streaming is the
            // loader's concern, so nothing is lost and nothing is reported.
            Ok(source) if matches!(source.mode, 0 | SOURCE_MODE_DFD) => {}
            Ok(source)
                if matches!(
                    source.mode,
                    SOURCE_MODE_TONE | SOURCE_MODE_TM1 | SOURCE_MODE_TM2 | SOURCE_MODE_TM_PRO
                ) =>
            {
                time_stretch = self.time_stretch(group, source.mode, &at, index);
            }
            Ok(source) if machine_kind(source.mode).is_some() => {
                machine = self.machine(source.mode, &at, index);
                if matches!(machine, Some(ir::Machine::BeatMachine)) {
                    beat_slices = match group.source_params() {
                        Ok((params, _)) => beat_slices_from_record(&params),
                        Err(error) => {
                            self.unsupported(
                                &at,
                                "beat machine source record",
                                error,
                                ir::Reason::Unknown,
                            );
                            None
                        }
                    };
                    if beat_slices.is_none() {
                        self.unsupported(
                            &at,
                            "beat machine slice controls missing",
                            source.mode,
                            not_modeled,
                        );
                    }
                }
            }
            // The remaining engines are played as a sampler, named in the report.
            Ok(source) => self.unsupported(
                &at,
                &format!("{} source (played as a sampler)", source_mode_name(source.mode)),
                source.mode,
                not_modeled,
            ),
            Err(error) => self.unsupported(&at, "source module", error, ir::Reason::Unknown),
        }
        // Publish the actual source and its physical-to-runtime identity before
        // translating routes. Wavetable destinations inspect this owner; doing
        // this after modulation silently rejected supported position/warp routes.
        let runtime_group = ir::GroupRef(self.ir.groups.len());
        if !v.volume.is_finite() || !v.pan.is_finite() || !(v.tune.is_finite() && v.tune > 0.0) {
            return Err(ni_file::Error::Generic(format!(
                "{at}: invalid gain, pan or tune"
            )));
        }
        self.ir.groups.push(ir::Group {
            muted: v.muted,
            wavetable,
            time_stretch,
            machine,
            beat_slices,
            name: v.name.clone(),
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
        if self.ir.source_indices.groups.len() <= index {
            self.ir.source_indices.groups.resize(index + 1, None);
        }
        self.ir.source_indices.groups[index] = Some(runtime_group);
        let mut chain = None;
        let mut filter_slots = Vec::new();
        self.lane_slots.clear();
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
                let live_eq = self
                    .eq_mod_slots
                    .get(&index)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let (c, boundary) = crate::effects::voice_chain(
                    &slots,
                    v.fx_idx_amp_split_point,
                    dynamic,
                    index as i32,
                    live_eq,
                );
                let mut processors = c.processors;
                filter_slots = c.filter_slots;
                self.lane_slots = c.lane_slots;
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
                    self.send_taps
                        .extend(c.send_taps.into_iter().map(|tap| (chain.unwrap(), tap)));
                }
            }
            Err(error) => self.unsupported(
                &format!("{at} insert"),
                "effect decoding",
                error,
                ir::Reason::Unknown,
            ),
        }
        // BParGroupDynamics (0x4A): 64 raw floats on the group (BGroup +0x680). No
        // processing class, detector or gain law is recovered, so any nonzero
        // value is reported; all-zero (every local preset) changes nothing.
        if let Some(chunk) = group.0.find_first(0x4A)
            && let Ok(dynamics) = ni_file::kontakt::objects::GroupDynamics::try_from(chunk)
            && dynamics.values.iter().any(|&v| v != 0.0)
        {
            self.unsupported(
                &at,
                "group dynamics (0x4A)",
                format!("{} of 64 values set", dynamics.values.iter().filter(|&&v| v != 0.0).count()),
                ir::Reason::UnknownLaw,
            );
        }
        let mut envelope = None;
        let mut flex_release = None;
        let mut routes = Vec::new();
        let mut phase_routes = Vec::new();
        let mut internal_bypasses = [None; 16];
        if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
            let modulators = InternalModArray16::try_from(chunk)?.slots()?.into_iter()
                .map(|(slot, modulator)| Ok((slot, modulator.params()?, modulator)))
                .collect::<Result<Vec<_>, ni_file::Error>>()?;
            let free_lfos = modulators.iter().fold(0u16, |mask, (slot, params, _)| {
                if matches!(params.modulator, Modulator::Lfo(_)) && params.unknown_flags[2] == 0 {
                    mask | (1u16 << slot)
                } else { mask }
            });
            for (slot, params, modulator) in modulators {
                let at = format!("{at} modulator slot {slot}");
                self.ir
                    .source_parameters
                    .extend(crate::source_parameters::internal(
                        &format!("group {index} internal slot {slot}"),
                        &modulator,
                        &params,
                    )?);
                // [router UI, bypass, retrigger, unknown]
                if params.targets.is_empty() {
                    continue;
                }
                let retrigger = params.unknown_flags[2] != 0;
                let volume = primary_volume(&params);
                if params.unknown_flags[1] != 0 {
                    for target in params.targets.iter().filter(|target| target.param == "frequency") {
                        self.unsupported(&at, "LFO rate live controls",
                            format!("frequency ({}) destination slot {:?}: saved-bypassed base-rate law", target.name, target.slot),
                            ir::Reason::UnknownLaw);
                    }
                }
                let mut live = wakeable(&params);
                if live {
                    for target in &params.targets {
                        if target.param == "frequency" && target.slot.is_some_and(|slot|
                            free_lfos & 1u16.checked_shl(u32::from(slot)).unwrap_or(0) != 0)
                        {
                            // A shared source bypass cannot cover only its other targets.
                            live = false;
                            self.unsupported(&at, "LFO rate live controls",
                                format!("frequency ({}) destination slot {:?}: free-running destination clock", target.name, target.slot),
                                ir::Reason::UnknownLaw);
                        }
                    }
                }
                if params.unknown_flags[1] != 0 && !live {
                    continue;
                }
                let source = match params.modulator {
                    Modulator::Ahdsr(env) => {
                        let ms = |ms: f32| ir::Time::Milliseconds(f64::from(ms.max(0.0)));
                        let (attack, hold, decay, release) = (
                            ahdsr_attack_ms(env.attack_ms),
                            ahdsr_attack_ms(env.hold_ms),
                            ahdsr_decay_ms(env.decay_ms),
                            ahdsr_decay_ms(env.release_ms),
                        );
                        let (attack_shape, fall) = ahdsr_curves(env.attack_curve);
                        ir::ModulationSource::Envelope(ir::Envelope {
                            attack: ms(attack),
                            hold: ms(hold),
                            decay: ms(decay),
                            sustain: f64::from(ahdsr_sustain(env.sustain)),
                            release: ms(release),
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
                        let sustain = flex.sustain as usize;
                        // Levels reach the universal envelope as 0..1; the setter's
                        // behaviour outside that range (0x140ae5a10 clamps only a
                        // tiny neighbourhood) is not carried, so report, never clamp.
                        if let Some(p) = flex.points.iter().find(|p| !(0.0..=1.0).contains(&p.level)) {
                            self.unsupported(
                                &at,
                                "flex level outside 0..1",
                                p.level,
                                ir::Reason::NotModeled,
                            );
                            continue;
                        }
                        if volume {
                            // Held until the release segments end (an upper bound: the
                            // native release scaling only shortens them).
                            let after: f32 = flex
                                .points
                                .iter()
                                .skip(sustain + 1)
                                .map(|p| p.time_ms.max(1.0))
                                .sum();
                            flex_release = Some(flex_release.unwrap_or(0.0f32).max(after));
                        }
                        // v0x12 tail bytes +0x194 (loop jump at the sustain row) and
                        // +0x195 (release adjustment bypass), runtime +0x321/+0x322.
                        // The loop jump is executed (the envelope returns to the
                        // point before the sustain row); the release adjustment
                        // bypass has no proved effect on the held envelope.
                        if let Some(word) = flex.unknown_version_word() {
                            if word >> 8 != 0 {
                                self.unsupported(
                                    &at,
                                    "flex release adjustment bypass (+0x195)",
                                    word >> 8,
                                    ir::Reason::UnknownLaw,
                                );
                            }
                        }
                        let breakpoints = flex_breakpoints(&flex);
                        if word_loop_jump(&flex) && breakpoints.loop_range.is_none() {
                            self.unsupported(
                                &at,
                                "flex loop jump at the sustain row (+0x194)",
                                flex.unknown_index,
                                ir::Reason::NotModeled,
                            );
                        }
                        ir::ModulationSource::Breakpoints(breakpoints)
                    }
                    // Decoded, not executable: the five DBD floats, the glide
                    // fields and the follower coefficients have no proven
                    // stored-to-runtime constructor (modulators.md, 0x140ae5990,
                    // 0x140ceb090, 0x140ae3d80), so they stay diagnosed.
                    // The runtime laws are blocks (`Breakpoints::dbd`, `GlideRamp`,
                    // `AdaptiveFollower`, `StepTable`); what is not proved is the step
                    // from the stored fields to their parameters:
                    //  - DBD: the saved durations pass a resolver (helper slot +0x28,
                    //    0x1409651b0) whose unit law, and the v0x10/0x11 legacy tail, are open.
                    //  - Glide: executable (`ModSource::Glide`); the setup law is
                    //    FUN_140cebba0, the interval is the previous note of the plan.
                    //  - Follower: executable (`ModSource::Follower`), stored floats map to
                    //    the builder roles; it samples the group audio of the
                    //    previous span (working timing choice, see k-mod-external.md).
                    Modulator::Follower(f) => ir::ModulationSource::Follower(ir::Follower {
                        attack: f.values[0],
                        decay: f.values[1],
                        gain: f.values[2],
                        adaption: f.values[3],
                    }),
                    Modulator::Glide(g) => ir::ModulationSource::Glide(ir::Glide {
                        mode: g.mode,
                        time_ms: g.values[0],
                    }),
                    Modulator::Dbd(ref dbd) => match dbd_breakpoints(dbd) {
                        Some(breakpoints) => ir::ModulationSource::Breakpoints(breakpoints),
                        None => {
                            self.unsupported(
                                &at,
                                "DBD envelope (0x41): v0x10/0x11 record or refused duration shape",
                                format!("{:?} {:?}", params.name, params.modulator),
                                ir::Reason::UnknownLaw,
                            );
                            continue;
                        }
                    },
                    Modulator::Other { chunk_id } => {
                        // 0x09 is BParArp, the note-blind 32-step table modulator
                        // (BArp32Step): its stored-to-table/length/rate/mode setup is unproved.
                        let (feature, reason) = if chunk_id == 0x09 {
                            ("32-step table modulator (0x09): stored fields to table, length, rate and mode", ir::Reason::UnknownLaw)
                        } else {
                            ("internal modulator chunk", ir::Reason::Unknown)
                        };
                        self.unsupported(
                            &at,
                            feature,
                            format!("{chunk_id:#x} {:?}", params.name),
                            reason,
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
                if let Some(address) = self
                    .ir
                    .source_indices
                    .modulators
                    .iter_mut()
                    .find(|m| m.group == index && m.slot == usize::from(slot) && !m.external)
                {
                    address.runtime = Some(modulator);
                }
                if envelope_source && volume && envelope.is_none() {
                    envelope = Some(modulator);
                    #[cfg(feature = "scan")]
                    self.target_outcomes
                        .insert((index, usize::from(slot), false, 0), true);
                    continue;
                }
                let mut bypass = None;
                for (_ordinal, target) in params.targets.iter().enumerate() {
                    if matches!(target.param.as_str(), "startPhase" | "frequency") && target.slot.is_some() {
                        phase_routes.push((
                            at.clone(),
                            modulator,
                            usize::from(slot),
                            false,
                            _ordinal,
                            target.clone(),
                            Some((live, params.unknown_flags[1] != 0)),
                        ));
                        continue;
                    }
                    let mut route = self.route(
                        &at,
                        modulator,
                        envelope_source,
                        target,
                        chain.zip(Some(&filter_slots[..])),
                    );
                    if live && let Some(bound) = route {
                        route = self.finalize_internal_route(
                            &at,
                            [index, usize::from(slot), _ordinal],
                            bound,
                            target,
                            params.unknown_flags[1] != 0,
                            &mut bypass,
                        );
                    }
                    #[cfg(feature = "scan")]
                    self.target_outcomes
                        .insert((index, usize::from(slot), false, _ordinal), route.is_some());
                    routes.extend(route);
                }
                internal_bypasses[slot] = bypass;
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
        // The group's own amplitude envelope is known once its internal
        // modulators are indexed; a route may address its stage times.
        if self.amplitude_envelopes.len() <= index {
            self.amplitude_envelopes.resize(index + 1, None);
        }
        self.amplitude_envelopes[index] = envelope;
        if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
            for (slot, modulation) in ExternalModArray32::try_from(chunk)?.slots()? {
                let mut params = modulation.params()?;
                if let Some(value) = self.script_intensity(index, &params.name) {
                    // `set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, ...)` on init and
                    // `on persistence_changed`: 0..=1000000 over 0..=1.
                    for target in &mut params.targets {
                        if !matches!(target.param.as_str(), "loopStart" | "loopLength") {
                            target.intensity = value;
                        }
                    }
                }
                let at = format!("{at} external modulation slot {slot}");
                self.ir
                    .source_parameters
                    .push(crate::source_parameters::external(
                        &format!("group {index} external slot {slot}"),
                        &modulation,
                        &params,
                    ));
                let plain_volume = |t: &ni_file::kontakt::objects::ModTarget| {
                    t.param == "volume"
                        && t.slot.is_none()
                        && t.unknown_flags & 2 == 0
                        && !t.invert
                        && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled)
                };
                if let (ModSource::Velocity, [t]) = (&params.source, params.targets.as_slice())
                    && plain_volume(t)
                    && velocity == ir::VelocityResponse::None
                    && let Some(law) = velocity_volume_law(f64::from(t.signed_intensity()))
                {
                    // gain × velocity at full intensity, or Kontakt's default-intensity table:
                    // kept on the voice so no per-voice modulation is needed.
                    velocity = law;
                    #[cfg(feature = "scan")]
                    self.target_outcomes
                        .insert((index, usize::from(slot), true, 0), true);
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
                    ModSource::ReleaseTriggerCounter => {
                        release_counter_source(v.release_trigger, v.rls_trig_counter)
                    }
                    // KSP set_event_par_arr($EVENT_PAR_MOD_VALUE_ID, v, id).
                    ModSource::Script(id) if id <= 1000 => ir::ModulationSource::Script(id as u16),
                    ModSource::RandomUnipolar => ir::ModulationSource::Random,
                    ModSource::Unassigned => continue,
                    other => {
                        let reason = match other {
                            ModSource::RandomBipolar => ir::Reason::UnknownLawOmitted,
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
                for (_ordinal, target) in params.targets.iter().enumerate() {
                    if matches!(target.param.as_str(), "startPhase" | "frequency") && target.slot.is_some() {
                        phase_routes.push((
                            at.clone(),
                            modulator,
                            usize::from(slot),
                            true,
                            _ordinal,
                            target.clone(),
                            None,
                        ));
                        continue;
                    }
                    if matches!(target.param.as_str(), "loopStart" | "loopLength")
                        && group.source_identity()?.mode != 0
                    {
                        self.unsupported(
                            &at,
                            "modulation target",
                            format!("{}: loop consumer requires Source mode 0", target.param),
                            ir::Reason::UnknownLaw,
                        );
                        continue;
                    }
                    let route = self.route(
                        &at,
                        modulator,
                        !bipolar,
                        target,
                        chain.zip(Some(&filter_slots[..])),
                    );
                    #[cfg(feature = "scan")]
                    self.target_outcomes
                        .insert((index, usize::from(slot), true, _ordinal), route.is_some());
                    if let Some(route) = route {
                        // Native identity (name parser FUN_1409250d0 type values).
                        let source_type = match params.source {
                            ModSource::PitchBend => Some(1),
                            ModSource::PolyAftertouch => Some(2),
                            ModSource::MonoAftertouch => Some(3),
                            ModSource::MidiCc(_) => Some(4),
                            ModSource::KeyPosition => Some(7),
                            ModSource::Velocity => Some(8),
                            ModSource::ReleaseVelocity => Some(9),
                            ModSource::ReleaseTriggerCounter => Some(10),
                            ModSource::Constant => Some(11),
                            ModSource::RandomUnipolar => Some(12),
                            ModSource::RandomBipolar => Some(13),
                            _ => None,
                        };
                        if let (Some(source_type), Some(target_id)) = (
                            source_type,
                            sampler_core::native_mod_laws::target_id(&target.param),
                        ) {
                            self.ir.source_indices.native_routes.push((
                                route,
                                ir::NativeRoute {
                                    target: target_id,
                                    flags: target.unknown_flags,
                                    source_type,
                                },
                            ));
                        }
                        let live = self.script_depth_target(route);
                        if live {
                            self.loop_route_controls(index, usize::from(slot), _ordinal, route);
                        }
                    }
                    routes.extend(route);
                }
            }
        }
        // Resolve source targets only after every physical internal slot has an owner.
        for (at, source, _slot, _external, _ordinal, target, internal) in phase_routes {
            let mut route = self.route(&at, source, true, &target, None);
            if let Some((true, saved_bypass)) = internal
                && let Some(bound) = route
            {
                route = self.finalize_internal_route(
                    &at, [index, _slot, _ordinal], bound, &target, saved_bypass,
                    &mut internal_bypasses[_slot],
                );
            }
            #[cfg(feature = "scan")]
            self.target_outcomes
                .insert((index, _slot, _external, _ordinal), route.is_some());
            if let Some(route) = route
                && _external
                && self.script_depth_target(route)
            {
                self.loop_route_controls(index, _slot, _ordinal, route);
            }
            routes.extend(route);
        }
        Ok(Some(GroupInfo {
            index,
            group: runtime_group,
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
    /// A Time Machine family group (stored modes 2, 5, 8): fields per
    /// specs/time-machine.md s.3-4 (BParSrcMode reader 0x140d03aa0, parameter
    /// transfers 0x140cdad90 / 0x140cdac40 / 0x140cdacd0). What the engine does
    /// not reproduce natively is reported, never hidden.
    fn time_stretch(
        &mut self,
        group: &ni_file::kontakt::objects::Group,
        mode: u32,
        at: &str,
        index: usize,
    ) -> Option<ir::TimeStretch> {
        let (params, _) = match group.source_params() {
            Ok(p) => p,
            Err(error) => {
                self.unsupported(at, "time machine source record", error, ir::Reason::Unknown);
                return None;
            }
        };
        let mut notes = Vec::new();
        let mut stretch = stretch_from_record(&params, &mut notes)?;
        for (feature, value, reason) in notes {
            self.unsupported(at, feature, value, reason);
        }
        // Measured writes (time_stretch_writes) change the stored source record; the
        // rest have no law yet, and writes with no field for this kind are reported.
        let unapplied = time_stretch_writes::apply(&mut stretch, |name| {
            group_engine_write(&self.engine, name, index as i32)
        });
        for name in unapplied {
            if let Some(value) = group_engine_write(&self.engine, name, index as i32) {
                self.unsupported(at, &format!("{name} write"), value, ir::Reason::NotModeled);
            }
        }
        let _ = mode;
        Some(stretch)
    }

    /// A machine source group (stored modes 4, 6, 7). Its own controls are
    /// recorded in the IR; the DSP that would consume them is not modelled, so
    /// the zones play as a sampler and the report says so.
    fn machine(&mut self, mode: u32, at: &str, index: usize) -> Option<ir::Machine> {
        let kind = machine_kind(mode)?;
        let (value, name) = match kind {
            ir::Machine::S1200 { .. } => {
                let raw = self.script_par("ENGINE_PAR_S1200_FILTER_MODE", index as i32, -1, -1);
                let filter_mode = match raw {
                    Some(v) if (0..=i32::from(ir::S1200_FILTER_MODE_MAX)).contains(&v) => Some(v as u8),
                    Some(v) => {
                        self.unsupported(at, "S1200 filter mode out of range", v, ir::Reason::InvalidValue);
                        None
                    }
                    None => None,
                };
                (Some(ir::Machine::S1200 { filter_mode }), "S1200 Machine")
            }
            ir::Machine::Mp60 => (Some(ir::Machine::Mp60), "MP60 Machine"),
            ir::Machine::BeatMachine => (Some(ir::Machine::BeatMachine), "Beat Machine"),
        };
        // Filter modes 1..=5 of the S1200 and the MP60 chain are modelled; the rest play as a sampler.
        // A Beat Machine group is modelled through its zones: a zone without a grid is reported there.
        let modelled = matches!(
            value,
            Some(
                ir::Machine::S1200 { filter_mode: Some(1..=ir::S1200_FILTER_MODE_MAX) }
                    | ir::Machine::Mp60
                    | ir::Machine::BeatMachine
            )
        );
        if !modelled {
            self.unsupported(
                at,
                &format!("{name} source (played as a sampler; its DSP is not modelled)"),
                mode,
                ir::Reason::NotModeled,
            );
        }
        value
    }

    fn script_par(&self, parameter: &str, group: i32, slot: i32, generic: i32) -> Option<i32> {
        let by_group = self.engine_by_group.get_or_init(|| {
            let mut map: HashMap<i32, Vec<usize>> = HashMap::new();
            for (at, write) in self.engine.iter().enumerate() {
                map.entry(write.group).or_default().push(at);
            }
            map
        });
        by_group
            .get(&group)?
            .iter()
            .rev()
            .map(|&at| &self.engine[at])
            .find(|w| {
                w.parameter.trim_start_matches('$') == parameter
                    && (w.slot, w.generic) == (slot, generic)
            })
            .map(|w| w.value)
    }

    /// Port v1's EQ knob domains onto the physical band's real processor lanes.
    fn eq_controls(&mut self, group: usize, chain: ir::ChainRef, slots: &[(usize, usize)]) {
        let mut bands = HashMap::<usize, usize>::new();
        for &(slot, index) in slots {
            let processor = self.ir.chains[chain.0]
                .pre_amplitude
                .iter()
                .chain(&self.ir.chains[chain.0].post_amplitude)
                .nth(index);
            let saved = match processor {
                Some(ir::Processor::Eq(eq)) if eq.count == 1 && eq.bands[0].law == ir::fx::eq::Law::KontaktLegacy => {
                    let b = eq.bands[0];
                    [f64::from(b.gain_db), f64::from(b.frequency), f64::from(b.q)]
                }
                _ => continue,
            };
            let band = bands.entry(slot).or_default();
            for (n, (native, name, label, parameter, min, max, unit)) in [
                (
                    "GAIN",
                    "gain",
                    "Gain",
                    ir::ProcessorParameter::Gain,
                    -18.,
                    18.,
                    ir::ControlUnit::Decibels,
                ),
                (
                    "FREQ",
                    "frequency",
                    "Frequency",
                    ir::ProcessorParameter::Cutoff,
                    0.,
                    1.,
                    ir::ControlUnit::Percent,
                ),
                (
                    "BW",
                    "bandwidth",
                    "Bandwidth",
                    ir::ProcessorParameter::Resonance,
                    0.,
                    1.,
                    ir::ControlUnit::Percent,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let native = format!("ENGINE_PAR_{native}{}", *band + 1);
                let default = self
                    .script_par(&native, group as i32, slot as i32, -1)
                    .map_or(saved[n], |v| {
                        min + (max - min) * f64::from(v.clamp(0, 1_000_000)) / 1_000_000.
                    })
                    .clamp(min, max);
                let control = ir::ControlRef(self.ir.controls.len());
                self.ir.controls.push(ir::Control {
                    key: format!(
                        "kontakt/group/{group}/slot/{slot}/eq/band/{}/{name}",
                        *band + 1
                    ),
                    label: format!("EQ Band {} {label}", *band + 1),
                    value: ir::ControlValue::Continuous {
                        min,
                        max,
                        default,
                        unit,
                    },
                    automation: ir::Automation::None,
                });
                self.ir.processor_controls.push(ir::ProcessorControl {
                    control,
                    chain,
                    index,
                    parameter,
                    ramp: ir::Time::ZERO,
                });
                self.claim_alias(control, &native, [group as i32, slot as i32, -1], false);
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

    /// Apply the same internal control admission before or after resolving a destination.
    fn finalize_internal_route(
        &mut self,
        at: &str,
        address: [usize; 3],
        route: ir::RouteRef,
        target: &ni_file::kontakt::objects::ModTarget,
        saved_bypass: bool,
        bypass: &mut Option<ir::ControlRef>,
    ) -> Option<ir::RouteRef> {
        debug_assert_eq!(route.0 + 1, self.ir.routes.len(), "finalize only the just-added route");
        let clock_supported = match self.ir.routes[route.0].target {
            ir::Target::SourceRate(destination) => matches!(self.ir.modulators[destination.0].source,
                ir::ModulationSource::Lfo(ir::Lfo { retrigger: true, .. })),
            _ => true,
        };
        if !clock_supported {
            self.unsupported(at, "LFO rate live controls",
                format!("{} ({}) destination slot {:?}: free-running destination clock", target.param, target.name, target.slot),
                ir::Reason::UnknownLaw);
        }
        let bound = clock_supported && self.internal_route_controls(address, route, target, saved_bypass, bypass);
        if !bound && saved_bypass {
            // An unbound saved-bypassed route must not run unconditionally.
            self.ir.routes.truncate(route.0);
            self.route_intensity.remove(&route.0);
            self.ir.source_indices.unipolar_routes.retain(|r| r.0 < route.0);
            if clock_supported {
                self.unsupported(at, "bypassed modulator route without live depth",
                    format!("{} ({})", target.param, target.name), ir::Reason::NotModeled);
            }
            None
        } else {
            Some(route)
        }
    }

    // Only positive, simple outgoing targets use the documented linear knob here.
    // Primary amplitude ownership, saved signed depth, phase and sample-start stay unbound.
    fn internal_route_controls(
        &mut self,
        address: [usize; 3],
        route: ir::RouteRef,
        target: &ni_file::kontakt::objects::ModTarget,
        saved_bypass: bool,
        bypass: &mut Option<ir::ControlRef>,
    ) -> bool {
        let [group, slot, ordinal] = address;
        let unit = match self.ir.routes[route.0].target {
            ir::Target::Amplitude | ir::Target::Pan | ir::Target::SourceRate(_)
                | ir::Target::Wavetable(_) => 1.,
            ir::Target::Pitch => 12.,
            // Module targets: depth units per unit of intensity, as the route's own law.
            ir::Target::Processor { .. } | ir::Target::Control(_) => {
                match self.route_intensity.get(&route.0) {
                    Some(&(_, unit)) if unit.is_finite() && unit != 0. => unit,
                    _ => return false,
                }
            }
            _ => return false,
        };
        if target.unknown_flags & 2 != 0
            || !target.intensity.is_finite()
            || !(0. ..=1.).contains(&target.intensity)
        {
            return false;
        }
        let mut lane = |name: &str, generic: i32, max: f64, saved: f64| {
            let bypass = name == "ENGINE_PAR_INTMOD_BYPASS";
            let script = |name| self.script_par(name, group as i32, slot as i32, generic);
            // The legacy and MP intensities write the same depth on the signed
            // cubic law (v1: INTMOD 1e6 at the target's generic is depth 1).
            let cubic = ["ENGINE_PAR_INTMOD_INTENSITY", "ENGINE_PAR_MOD_TARGET_MP_INTENSITY"]
                .into_iter()
                .find_map(|name| script(name))
                .filter(|_| !bypass)
                .map(|v| max * sampler_core::EngineParameterLaw::SignedCubic.decode(v));
            let default = cubic.or_else(|| {
                script(name).map(|v| {
                    if bypass {
                        f64::from(v != 0)
                    } else {
                        max * f64::from(v.clamp(0, 1_000_000)) / 1_000_000.
                    }
                })
            });
            let control = ir::ControlRef(self.ir.controls.len());
            self.ir.controls.push(ir::Control {
                key: format!("kontakt/group/{group}/mod/{slot}/{generic}/{name}"),
                label: name.into(),
                value: ir::ControlValue::Continuous {
                    // A depth may be negative (inverted) through the cubic writers.
                    min: if bypass { 0. } else { -max },
                    max,
                    default: default.unwrap_or(saved),
                    unit: ir::ControlUnit::Percent,
                },
                automation: ir::Automation::None,
            });
            let names: &[&str] = if bypass {
                &["ENGINE_PAR_INTMOD_BYPASS"]
            } else {
                &["ENGINE_PAR_MOD_TARGET_INTENSITY", "ENGINE_PAR_INTMOD_INTENSITY", "ENGINE_PAR_MOD_TARGET_MP_INTENSITY"]
            };
            for name in names {
                self.claim_alias(control, name, [group as i32, slot as i32, generic], false);
            }
            control
        };
        let bypass = *bypass.get_or_insert_with(|| {
            lane("ENGINE_PAR_INTMOD_BYPASS", -1, 1., f64::from(saved_bypass))
        });
        let depth = lane(
            "ENGINE_PAR_MOD_TARGET_INTENSITY",
            ordinal as i32,
            unit,
            f64::from(target.intensity) * unit,
        );
        // Generic -1 is the modulator's own intensity (ANALOG STRINGS writes its
        // LFO amounts there); as for external modulators it reaches the first target.
        // ponytail: -1 aliases target 0 only; multi-target -1 semantics unmeasured.
        if ordinal == 0 {
            for parameter in ["ENGINE_PAR_INTMOD_INTENSITY", "ENGINE_PAR_MOD_TARGET_MP_INTENSITY"] {
                self.claim_alias(depth, parameter, [group as i32, slot as i32, -1], false);
            }
        }
        self.ir
            .source_indices
            .route_controls
            .push((route, depth, bypass));
        true
    }

    /// An instrument bus fader a script set (linear gain). Law: dB = 18 log2(v)
    /// - 346.768, the one group volume uses (0 dB at 630957). Kontakt's dB is
    /// 6 dB per doubling (gain = 2^(dB/6)), measured with the native oracle
    /// (tools/kontakt-oracle/probes/volume_law.lua), so this fader is cubic in v;
    /// a runtime write decodes with the same law.
    fn script_bus_volume(&self, bus: usize) -> Option<f32> {
        let v = self.script_par("ENGINE_PAR_VOLUME", -1, -1, 1000 + bus as i32)?;
        Some(sampler_core::BUS_VOLUME_LAW.decode(v) as f32)
    }

    /// Alias a script address to `control`. Internal and external modulators
    /// number their slots apart, so both can claim one (group, slot, generic):
    /// the measured loop/module depth (`evict`) wins over an internal route's.
    // ponytail: KSP's find_mod index spans both lists; resolve it to drop this tie-break.
    fn claim_alias(&mut self, control: ir::ControlRef, parameter: &str, [group, slot, generic]: [i32; 3], evict: bool) {
        let address = ir::SlotAddress { group, slot, generic };
        let aliases = &mut self.ir.source_indices.control_aliases;
        let alias = ir::SourceControlAlias { control, parameter: parameter.into(), address };
        match self.alias_claims.entry((parameter.into(), address)) {
            std::collections::hash_map::Entry::Occupied(at) => {
                if evict {
                    aliases[*at.get()] = alias;
                }
            }
            std::collections::hash_map::Entry::Vacant(at) => {
                at.insert(aliases.len());
                aliases.push(alias);
            }
        }
    }

    /// Port v1's Constant loop target aliases and signed cubic depth domain.
    fn loop_route_controls(
        &mut self,
        group: usize,
        slot: usize,
        ordinal: usize,
        route: ir::RouteRef,
    ) {
        let ir::Depth::Normalized(saved) = self.ir.routes[route.0].depth else {
            return;
        };
        let raw = [
            "ENGINE_PAR_INTMOD_INTENSITY",
            "ENGINE_PAR_MOD_TARGET_MP_INTENSITY",
        ]
        .into_iter()
        .find_map(|name| {
            self.script_par(name, group as i32, slot as i32, ordinal as i32).or_else(|| {
                (ordinal == 0).then(|| self.script_par(name, group as i32, slot as i32, -1))?
            })
        });
        let saved = raw.map_or(saved, |v| {
            sampler_core::EngineParameterLaw::SignedCubic.decode(v)
        });
        let depth = ir::ControlRef(self.ir.controls.len());
        self.ir.controls.push(ir::Control {
            key: format!("kontakt/group/{group}/external/{slot}/target/{ordinal}/{}", if matches!(self.ir.routes[route.0].target, ir::Target::LoopStart | ir::Target::LoopLength) { "loop-depth" } else { "module-depth" }),
            label: "Loop depth".into(),
            value: ir::ControlValue::Continuous {
                min: -1.,
                max: 1.,
                default: saved,
                unit: ir::ControlUnit::Percent,
            },
            automation: ir::Automation::None,
        });
        // Generic -1 is the modulator's own intensity: scripts address a
        // one-target modulator that way (ANALOG STRINGS Phase_P1/P2, Loop_Start).
        // ponytail: -1 aliases the first target only; multi-target -1 semantics unmeasured.
        let generics = [Some(ordinal as i32), (ordinal == 0).then_some(-1)];
        for parameter in [
            "ENGINE_PAR_INTMOD_INTENSITY",
            "ENGINE_PAR_MOD_TARGET_MP_INTENSITY",
        ] {
            for generic in generics.into_iter().flatten() {
                self.claim_alias(depth, parameter, [group as i32, slot as i32, generic], true);
            }
        }
        let bypass = ir::ControlRef(self.ir.controls.len());
        self.ir.controls.push(ir::Control {
            key: format!("kontakt/group/{group}/external/{slot}/target/{ordinal}/enabled"),
            label: "Loop route bypass".into(),
            value: ir::ControlValue::Continuous {
                min: 0.,
                max: 1.,
                default: 0.,
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::None,
        });
        self.ir
            .source_indices
            .route_controls
            .push((route, depth, bypass));
    }

    /// The modulation intensity a script set for modulation `name` of group `index`.
    fn script_intensity(&self, index: usize, name: &str) -> Option<f32> {
        let slot = *self
            .intensity_slots
            .get(&(index as i32, name.to_ascii_lowercase()))?;
        let v = self.script_par("ENGINE_PAR_MOD_TARGET_INTENSITY", index as i32, slot, -1)?;
        Some(v.clamp(0, 1_000_000) as f32 / 1_000_000.0)
    }

    /// Whether a script can set this external route's intensity live: loop
    /// ranges, LFO rates and script-addressed effect knobs (not filters).
    fn script_depth_target(&self, route: ir::RouteRef) -> bool {
        match self.ir.routes[route.0].target {
            ir::Target::LoopStart
            | ir::Target::LoopLength
            | ir::Target::SourceRate(_)
            | ir::Target::SourcePhase(_)
            | ir::Target::Wavetable(_) => true,
            ir::Target::Processor { parameter, .. } => !matches!(
                parameter,
                ir::ProcessorParameter::Gain
                    | ir::ProcessorParameter::Cutoff
                    | ir::ProcessorParameter::Resonance
                    | ir::ProcessorParameter::Response
            ),
            _ => false,
        }
    }

    /// An enabled modulation shaper as an IR transfer curve.
    fn shape_of(&mut self, shaper: &ni_file::kontakt::objects::ModShaper) -> Option<ir::ShapeRef> {
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
            _ => return None,
        };
        self.ir.shapes.push(ir::Shape { points });
        Some(ir::ShapeRef(self.ir.shapes.len() - 1))
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
            // A zero-intensity route to a destination we do not model has no depth,
            // so it cannot change the sound: nothing to report. Routes with a
            // live intensity (or other features) still report.
            if i == 0. && matches!(feature, "modulation target" | "modulation of a module parameter") {
                return None;
            }
            let destination = target.slot.map_or_else(String::new, |slot| format!(" destination slot {slot}"));
            this.unsupported(
                at,
                feature,
                format!("{} ({}){destination} intensity {i}", target.param, target.name),
                reason,
            );
            None
        };
        // The native lag is signed (FUN_140873040 route field +0xe); only the
        // non-negative law is proved, so a negative lag is reported, not smoothed.
        if target.lag_ms < 0 {
            return report(self, "negative modulation lag", ir::Reason::NotModeled);
        }
        // Modulating another modulator's intensity scales that modulator's own
        // routes (Kontakt ID25 law); it is not a route of its own.
        if target.param == "intensity" {
            let shaper = target.shaper.as_ref().filter(|s| s.enabled);
            if let Some(slot) = target.slot {
                // The destination resolves after the group's modulators are indexed.
                let shape = match shaper {
                    Some(shaper) => match self.shape_of(shaper) {
                        Some(shape) => Some(shape),
                        None => {
                            report(self, "empty modulation shaper", ir::Reason::UnknownLaw);
                            return None;
                        }
                    },
                    None => None,
                };
                self.intensity_scales.push(IntensityScale {
                    at: at.to_string(),
                    slot: usize::from(slot),
                    source,
                    depth: i,
                    flags: target.unknown_flags,
                    // Invert does not act through an enabled shaper (see below).
                    inverted: target.invert && shaper.is_none(),
                    lag_ms: target.lag_ms,
                    shape,
                });
                return None;
            }
        }
        let phase_target = (target.param == "startPhase" && target.lag_ms == 0)
            .then(|| {
                let slot = usize::from(target.slot?);
                let group = self
                    .ir
                    .source_indices
                    .modulators
                    .iter()
                    .find(|m| m.runtime == Some(source))?
                    .group;
                let destination = self
                    .ir
                    .source_indices
                    .modulators
                    .iter()
                    .find(|m| m.group == group && m.slot == slot && !m.external)?
                    .runtime?;
                matches!(
                    self.ir.modulators[destination.0].source,
                    ir::ModulationSource::Lfo(_)
                )
                .then_some((
                    ir::Target::SourcePhase(destination),
                    ir::Depth::Normalized(i),
                ))
            })
            .flatten();
        // LFO `frequency`: the module's normalized frequency parameter, additive
        // (ID26 is in FUN_140873040's set D: `$in + E * $intensity`). Its rate
        // codec is parameter 713 / lane 0x140b07bd9, and every saved Hz in the
        // corpus (legacy and Digital kinds alike) is that codec of the saved point.
        // A beat-synced cycle has no frequency lane and stays reported.
        let rate_target = (target.param == "frequency")
            .then(|| {
                let destination = self.slot_modulator(source, usize::from(target.slot?))?;
                // A shared (instrument-scope) cycle has one phase for every voice.
                (self.ir.modulators[destination.0].scope == ir::Scope::Voice).then_some(())?;
                matches!(
                    self.ir.modulators[destination.0].source,
                    ir::ModulationSource::Lfo(ir::Lfo {
                        rate: ir::Frequency::Hertz(hz),
                        ..
                    }) if sampler_core::v1_voice_controls::LfoFrequency::lane(hz).is_some()
                )
                .then_some((ir::Target::SourceRate(destination), ir::Depth::Normalized(i)))
            })
            .flatten();
        // Script-addressed effect knobs: normalized, additive lane (set D/F of the
        // law compiler: shaper 98, bitdepth 101, downsample 102, formant 36..38).
        // `distortionIntensity` (64) is outside both sets, so its source is read
        // through the unipolar view (`need_uni`).
        let lane_target = filters.and_then(|(chain, _)| {
            use ir::ProcessorParameter as P;
            let slot = usize::from(target.slot?);
            let &(_, index) = self.lane_slots.iter().find(|(s, _)| *s == slot)?;
            let chain_data = self.ir.chains.get(chain.0)?;
            let processor = chain_data.pre_amplitude.iter().chain(&chain_data.post_amplitude).nth(index)?;
            let parameter = match (processor, target.param.as_str()) {
                (ir::Processor::LiveFormant { .. }, "formantTalk") => P::FormantTalk,
                (ir::Processor::LiveFormant { .. }, "formantSharp") => P::FormantSharp,
                (ir::Processor::LiveFormant { .. }, "formantSize") => P::FormantSize,
                (ir::Processor::LivePhaser { .. }, "filterCutoff") => P::Cutoff,
                (ir::Processor::LivePhaser { .. }, "filterQ") => P::Resonance,
                (ir::Processor::TimeMod(config), "filterCutoff" | "filterQ") => {
                    let role = if target.param == "filterQ" { P::Resonance } else { P::Cutoff };
                    config.lanes().iter().any(|lane| lane.role == role).then_some(role)?
                }
                (ir::Processor::Color(config), name) => {
                    let role = match name {
                        "shaper" => P::Shape,
                        "distortionIntensity" => P::Drive,
                        "bitdepth" => P::LoFiBits,
                        "downsample" => P::LoFiFrequency,
                        _ => return None,
                    };
                    config.lanes().iter().any(|lane| lane.role == role).then_some(role)?
                }
                _ => return None,
            };
            Some((ir::Target::Processor { chain, index, parameter }, ir::Depth::Normalized(i)))
        });
        let phase_target = phase_target.or(rate_target).or(lane_target);
        // Native Ladder/Daft use normalized knob addition (v1 filter.rs);
        // the generic filter fallback retains its octave law.
        let addressed = phase_target.or_else(|| {
            filters.and_then(|(chain, slots)| {
                if target.param == "startPhase" {
                    return None;
                }
                let slot = target.slot?;
                let knob = eq_knob(&target.param);
                let (_, index) = slots
                    .iter()
                    .filter(|(s, _)| *s == usize::from(slot))
                    .nth(knob.map_or(0, |(band, _)| band))?;
                let processor = self
                    .ir
                    .chains
                    .get(chain.0)?
                    .pre_amplitude
                    .iter()
                    .chain(&self.ir.chains[chain.0].post_amplitude)
                    .nth(*index)?;
                if let Some((_, parameter)) = knob {
                    return matches!(processor, ir::Processor::Eq(_)).then_some((
                        ir::Target::Processor {
                            chain,
                            index: *index,
                            parameter,
                        },
                        ir::Depth::Normalized(i),
                    ));
                }
                let native = matches!(
                    processor,
                    ir::Processor::LadderLP4(_) | ir::Processor::Daft(_) | ir::Processor::Ar(_)
                        | ir::Processor::FilterBank(sampler_ir::fx::filter::Config {
                            topology: sampler_ir::fx::filter::Topology::Cascade,
                            ..
                        })
                );
                let parameter = match target.param.as_str() {
                    "filterCutoff"
                        if !matches!(processor, ir::Processor::Eq(_)) =>
                    {
                        ir::ProcessorParameter::Cutoff
                    }
                    "filterQ" if native => ir::ProcessorParameter::Resonance,
                    "Gain" if native => ir::ProcessorParameter::Gain,
                    _ => return None,
                };
                let depth = if native {
                    ir::Depth::Normalized(i)
                } else {
                    ir::Depth::Pitch(ir::Pitch::Semitones(
                        12. * self
                            .cutoff_octaves
                            .get(&(chain.0, *index))
                            .copied()
                            .unwrap_or(10.)
                            * i,
                    ))
                };
                Some((
                    ir::Target::Processor {
                        chain,
                        index: *index,
                        parameter,
                    },
                    depth,
                ))
            })
        });
        // An AHDSR stage time addressed as a modulation target (ids 184..187).
        // The destination is the group's envelope modulator; the compiler
        // multiplies the stage's normalized knob by its family H product.
        let envelope_target = (matches!(
            target.param.as_str(),
            "ahdsr_attack" | "ahdsr_hold" | "ahdsr_decay" | "ahdsr_release"
        ))
        .then(|| {
            let destination = self.slot_modulator(source, usize::from(target.slot?))?;
            // Only the group's own amplitude envelope carries the stage times
            // a route can address; another AHDSR is a modulator source.
            let group = self
                .ir
                .source_indices
                .modulators
                .iter()
                .find(|m| m.runtime == Some(source))?
                .group;
            if self.amplitude_envelope(group) != Some(destination) {
                return None;
            }
            let stage = match target.param.as_str() {
                "ahdsr_attack" => ir::EnvelopeStage::Attack,
                "ahdsr_hold" => ir::EnvelopeStage::Hold,
                "ahdsr_decay" => ir::EnvelopeStage::Decay,
                "ahdsr_release" => ir::EnvelopeStage::Release,
                _ => return None,
            };
            // Only an AHDSR envelope carries stage times.
            matches!(
                self.ir.modulators[destination.0].source,
                ir::ModulationSource::Envelope(_)
            )
            .then_some((
                ir::Target::EnvelopeStage {
                    source: destination,
                    stage,
                },
                ir::Depth::Normalized(i),
            ))
        })
        .flatten();
        let addressed = addressed.or(envelope_target);
        let addressed = addressed.or_else(|| {
            let slot = i32::from(target.slot?);
            let group = self
                .ir
                .source_indices
                .modulators
                .iter()
                .find(|m| m.runtime == Some(source))?
                .group as i32;
            let native = match target.param.as_str() {
                "filterCutoff" => "ENGINE_PAR_CUTOFF",
                "filterQ" => "ENGINE_PAR_RESONANCE",
                "bitdepth" => "ENGINE_PAR_BITS",
                "downsample" => "ENGINE_PAR_FREQUENCY",
                "distortionIntensity" => "ENGINE_PAR_DRIVE",
                "shaper" => "ENGINE_PAR_SHAPE",
                "formantTalk" => "ENGINE_PAR_FORMANT_TALK",
                "formantSize" => "ENGINE_PAR_FORMANT_SIZE",
                _ => return None,
            };
            let control = self
                .ir
                .source_indices
                .control_aliases
                .iter()
                .find(|a| {
                    a.address.group == group
                        && a.address.slot == slot
                        && a.address.generic == -1
                        && a.parameter.trim_start_matches('$') == native
                })?
                .control;
            // The adapter must publish the native normalized knob; physical-unit
            // controls alone do not establish the destination's modulation law.
            if !matches!(
                self.ir.controls.get(control.0)?.value,
                ir::ControlValue::Continuous {
                    min: 0.,
                    max: 1.,
                    ..
                }
            ) {
                return None;
            }
            let (chain, _) = filters?;
            self.ir
                .processor_controls
                .iter()
                .any(|b| b.control == control && b.chain == chain)
                .then_some((ir::Target::Control(control), ir::Depth::Normalized(i)))
        });
        // Phase and rate of a dormant (saved-bypassed, unwakeable) modulator: inert natively.
        if addressed.is_none()
            && matches!(target.param.as_str(), "startPhase" | "frequency")
            && let Some(slot) = target.slot
            && let Some(group) = self.ir.source_indices.modulators.iter().find(|m| m.runtime == Some(source)).map(|m| m.group)
            && self.dormant_modulators.contains(&(group, usize::from(slot)))
        {
            return None;
        }
        if target.slot.is_some() && addressed.is_none() {
            return report(
                self,
                "modulation of a module parameter",
                ir::Reason::NotModeled,
            );
        }
        let (route_target, depth) = match target.param.as_str() {
            _ if addressed.is_some() => addressed.unwrap(),
            "volume" => (ir::Target::Amplitude, ir::Depth::Normalized(i)),
            "pitch" => (
                ir::Target::Pitch,
                ir::Depth::Pitch(ir::Pitch::Semitones(12.0 * i)),
            ),
            "playPos" => (ir::Target::SampleStart, ir::Depth::Normalized(i)),
            "loopStart" | "loopLength"
                if self
                    .ir
                    .modulators
                    .get(source.0)
                    .is_some_and(|m| m.source == ir::ModulationSource::Constant)
                    && target.lag_ms == 0
                    && !target.invert
                    && !target.shaper.as_ref().is_some_and(|s| s.enabled) =>
            {
                (
                    if target.param == "loopStart" {
                        ir::Target::LoopStart
                    } else {
                        ir::Target::LoopLength
                    },
                    ir::Depth::Normalized(i),
                )
            }
            "pan" => (ir::Target::Pan, ir::Depth::Normalized(i)),
            "wavetablePosition" | "warpFactor" | "warpFactor2"
                if self.ir.source_indices.modulators.iter().find(|m| m.runtime == Some(source))
                    .and_then(|m| self.ir.source_indices.groups.get(m.group).copied().flatten()
                        .or_else(|| self.ir.source_indices.groups.is_empty().then_some(ir::GroupRef(m.group))))
                    .and_then(|group| self.ir.groups.get(group.0))
                    .is_some_and(|group| group.wavetable.is_some()) =>
            {
                (ir::Target::Wavetable(match target.param.as_str() {
                    "wavetablePosition" => ir::WavetableParameter::Position,
                    "warpFactor" => ir::WavetableParameter::Form1,
                    _ => ir::WavetableParameter::Form2,
                }), ir::Depth::Normalized(i))
            }
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
            Some(shaper) => match self.shape_of(shaper) {
                Some(shape) => Some(shape),
                None => return report(self, "empty modulation shaper", ir::Reason::UnknownLaw),
            },
        };
        let per_unit = match depth {
            ir::Depth::Normalized(d) => d,
            ir::Depth::Pitch(p) => p.semitones(),
            ir::Depth::Gain(g) => 20. * g.linear().log10(),
        };
        // Depth units per unit of saved intensity: the law's own factor, so a
        // route saved at zero (set live by a script) keeps its unit.
        let unit = if i != 0. {
            per_unit / i
        } else {
            match (depth, route_target) {
                (ir::Depth::Normalized(_), _) => 1.,
                (ir::Depth::Pitch(_), ir::Target::Pitch) => 12.,
                (ir::Depth::Pitch(_), ir::Target::Processor { chain, index, .. }) => {
                    12. * self.cutoff_octaves.get(&(chain.0, index)).copied().unwrap_or(10.)
                }
                _ => f64::NAN,
            }
        };
        self.route_intensity.insert(self.ir.routes.len(), (i, unit));
        if target.param == "distortionIntensity" {
            let route = ir::RouteRef(self.ir.routes.len());
            self.ir.source_indices.unipolar_routes.push(route);
        }
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
        // The saved Digital fade clock: retriggered, unsynchronized, free of the trailing
        // delay flag and the alternate record flag. Both Digital entries use it; the shared
        // post stage 0x140b06c90 runs after the Digital mixer 0x140b078d0 as after the sine.
        let digital_clock = retrigger
            && !lfo.trailing_flag
            && !lfo.records[1].flag
            && lfo.records[0].values[0] == -1.
            && (0. ..=1.).contains(&phase);
        let shape = match (lfo.waveform, lfo.trailing_values) {
            // NI LFO controls / Waveform Mixer; native core 0x140b07290 sums five weights.
            // Admit zero before sine-only Multi; its unipolar view remains 0.5.
            (5, Some(weights)) if weights == [0.; 5] && width > 0. && width < 1. => {
                ir::LfoShape::Zero
            }
            // Digital Multi's falling sine retains subunit weights; normalization
            // divides by max(1, sum(abs(weights))), not by the sole weight.
            // Other components, synchronized fades and the alternate mode remain unverified.
            (6, Some([sine, 0., 0., 0., 0.]))
                if sine.is_finite()
                    && sine.abs() <= 1.
                    && (0. ..=5000.).contains(&fade_ms)
                    && (fade_ms == 0. || digital_clock)
                    && lfo.records[1].values[0] == -1.
                    && lfo.additional_flag == Some(false) =>
            {
                if fade_ms == 0. {
                    ir::LfoShape::SineScaled(-f64::from(sine))
                } else {
                    ir::LfoShape::DigitalSine {
                        level: -f64::from(sine),
                        fade_ms: lfo.initial_values[0],
                    }
                }
            }
            // Original legacy and Digital entries agree on weighted falling sine.
            (5, Some([sine, 0., 0., 0., 0.]))
                if sine.is_finite()
                    && sine.abs() <= 1.
                    && fade_ms == 0.
                    && lfo.records[1].values[0] == -1. =>
            {
                ir::LfoShape::SineScaled(-f64::from(sine))
            }
            // BLfoMulti (5) and BLfoMultiDigital (6), 0x140b07290 / 0x140b078d0
            // (modulators-runtime.md): the stored weights (sine, pulse, triangle,
            // doubled saw, noise) reach the runtime in order sine, saw, triangle,
            // pulse, noise (builder copy 0x140977e57..0x140977e9f). The stored
            // normalization byte is records[0].flag. No fade (the exponential Multi
            // fade is only wired for the sine-only entry above), the pulse width
            // inside the builder's 0.05..0.95 clamp, and held noise needs a start
            // phase below one half.
            // A legacy Multi's saved fade (unsynchronized, records[1] -1: v1's admitted
            // fade clock) runs the native fade law over the mix; Digital fades keep the
            // verified sine-only entry above.
            // ponytail: free-running LFOs fade per voice from note start, as the fade
            // lives in the per-voice modulator; not reference-rendered.
            (5 | 6, Some([sine, pulse, triangle, saw, noise]))
                if (fade_ms == 0.
                    || (lfo.waveform == 5 && (0. ..=5000.).contains(&fade_ms) && lfo.records[1].values[0] == -1.)
                    || (lfo.waveform == 6
                        && (0. ..=5000.).contains(&fade_ms)
                        && lfo.records[1].values[0] == -1.
                        && lfo.additional_flag == Some(false)
                        && digital_clock))
                    && [sine, pulse, triangle, saw, noise].iter().all(|w| w.is_finite())
                    && (0.05..=0.95).contains(&width)
                    && (noise == 0. || (0. ..0.5).contains(&phase)) =>
            {
                let wave = ir::NativeWave::Multi {
                    weights: [sine, saw, triangle, pulse, noise],
                    width: width as f32,
                    normalize: lfo.records[0].flag,
                    digital: lfo.waveform == 6,
                };
                if fade_ms == 0. {
                    ir::LfoShape::Native(wave)
                } else {
                    ir::LfoShape::NativeFaded { wave, fade_ms: lfo.initial_values[0] }
                }
            }
            // Native BLfo* processors (modulators.md): recovered tables, any pulse
            // width in 0..1, and held noise whose half-cycle grid counts from a
            // start phase below one half.
            (0, _) => ir::LfoShape::Native(ir::NativeWave::Sine),
            (1, _) if width > 0. && width < 1. => ir::LfoShape::Native(ir::NativeWave::Rectangle(width as f32)),
            (2, _) => ir::LfoShape::Native(ir::NativeWave::Triangle),
            (3, _) => ir::LfoShape::Native(ir::NativeWave::Saw),
            (4, _) if (0. ..0.5).contains(&phase) => ir::LfoShape::Native(ir::NativeWave::Held),
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
            fade_in: if matches!(shape, ir::LfoShape::DigitalSine { .. } | ir::LfoShape::NativeFaded { .. }) {
                ir::Time::ZERO
            } else {
                ir::Time::Milliseconds(fade_ms.max(0.0))
            },
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
                    Some("aif" | "aiff" | "aifc") => ir::Encoding::Aiff,
                    _ => ir::Encoding::Unknown,
                };
                self.ir.assets.push(ir::Asset {
                    location: ir::AssetLocation::Path(location.to_string_lossy().into_owned()),
                    residency: Default::default(),
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
        for (slot, l) in z
            .loop_slots
            .iter()
            .zip(&z.loops)
            .filter(|(_, l)| l.mode != 0)
        {
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
                self.unsupported(
                    &at,
                    "alternating loop crossfade law (metadata retained)",
                    format!("slot {slot}, fade {}", l.x_fade_length),
                    ir::Reason::UnknownLaw,
                );
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
                beat_grid: z.beat_grid,
                start: z.start,
                end,
                reverse: group.reverse,
                looping,
                start_range: z.start_mod,
                loop_control_crossfade: if group.routes.iter().any(|r| {
                    matches!(
                        self.ir.routes[r.0].target,
                        ir::Target::LoopStart | ir::Target::LoopLength
                    )
                }) && slots.iter().flatten().count() == 1
                {
                    slots
                        .iter()
                        .flatten()
                        .next()
                        .map(|slot| slot.range.crossfade)
                } else {
                    None
                },
            },
            amplitude: group.envelope,
            routes: group.routes.clone(),
            chain: group.chain,
            ..ir::Zone::new(asset)
        });
    }
}

/// One serialized zone: its group, mapping and sample reference.
#[derive(Clone)]
struct RawZone {
    /// Zone slice boundaries in sample frames (Beat Machine grid); empty without a grid.
    slice_markers: Vec<u32>,
    /// Stored zone playback rate of the grid (private float at byte 12).
    slice_tempo: f32,
    /// Set by the loader for zones of a Beat Machine group.
    beat_grid: Option<ir::BeatGrid>,
    source_id: Option<u32>,
    identity_layout: (u16, usize, Option<u16>),
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
    /// Set when no MIDI note can trigger the zone: its whole stored velocity
    /// or key range lies outside 0..=127. Describes the range for diagnostics.
    unreachable: Option<String>,
}

/// Complete physical order, independent of source/sample/playback admission.
/// Unsupported/absent private identity is None, never ordinal+1 or region0.
/// An identity stored on several zones names the first of them: Kontakt 8.8.0's
/// zone lookup (`FUN_14088b7b0`) scans the zone list and returns the first match,
/// so later zones with that identity cannot be addressed and are None here.
#[cfg(test)]
fn source_zone_catalog(data:&[u8],groups:usize)->Result<Vec<ir::SourceZone>,String> {
    source_zone_catalog_with_layouts(data, groups).map(|(catalog, _)| catalog)
}
fn source_zone_catalog_with_layouts(data:&[u8],groups:usize)->Result<(Vec<ir::SourceZone>, String),String> {
    let mut layouts = std::collections::BTreeMap::new();
    let mut reader=Cursor::new(data);
    let count=u32le(&mut reader).map_err(|e|e.to_string())?;
    if count as usize>data.len()/8 {return Err("zone count exceeds the zone list".into());}
    let mut catalog=Vec::with_capacity(count as usize);
    for index in 0..count {
        let zone=raw_zone(&mut reader).map_err(|e|format!("zone {index}: {e}"))?;
        if zone.group>=groups {return Err(format!("zone {index} refers to missing group {}",zone.group));}
        if zone.source_id.is_none() {
            *layouts.entry(zone.identity_layout).or_insert(0usize) += 1;
        }
        let id=zone.source_id;
        if !(0..=127).contains(&zone.source.low_key) || !(0..=127).contains(&zone.source.high_key)
            || zone.source.low_key>zone.source.high_key {return Err(format!("zone {index} has invalid source key bounds"));}
        catalog.push(ir::SourceZone {id,home:ir::SourceZoneHome {
            group:zone.group as u32,low_key:u8::try_from(zone.source.low_key).map_err(|_|"invalid source low key")?,
            high_key:u8::try_from(zone.source.high_key).map_err(|_|"invalid source high key")?,
        }});
    }
    if reader.position()!=data.len() as u64 {return Err("trailing zone-list bytes".into());}
    let summary = layouts.iter().take(8).map(|(&(version, private_len, word56), count)|
        format!("{count} zones: version=0x{version:x} private_bytes={private_len} private_u16_at_56={word56:?}"))
        .collect::<Vec<_>>().join("; ");
    let omitted = layouts.len().saturating_sub(8);
    Ok((catalog, if omitted > 0 { format!("{summary}; {omitted} more layouts omitted") } else { summary }))
}
fn zone_catalog_homes(catalog:&[ir::SourceZone])->std::collections::BTreeMap<u32,[i32;3]> {
    crate::load::first_homes(catalog.iter().map(|z|(z,true)))
}
/// Whether each catalog zone is the first with its identity, the one lookups by identity find.
fn addressed(catalog:&[ir::SourceZone])->Vec<bool> {
    let mut seen=std::collections::BTreeSet::new();
    catalog.iter().map(|z|z.id.is_some_and(|id|seen.insert(id))).collect()
}
#[cfg(test)]
fn source_zone_metadata(data:&[u8],groups:usize)->Result<std::collections::BTreeMap<u32,[i32;3]>,String> {
    source_zone_catalog(data,groups).map(|c|zone_catalog_homes(&c))
}

/// A saved automation tag's script slot, widget ordinal and whether it names a `ui_switch`.
fn automation_target(tag: &str) -> Option<(u8, u32, bool)> {
    let (switch, rest) = match tag.strip_prefix("pts_script_slider_") {
        Some(rest) => (false, rest),
        None => (true, tag.strip_prefix("pts_script_switch_")?),
    };
    let (slot, ordinal) = rest.split_once('_')?;
    Some((slot.parse().ok()?, ordinal.parse().ok()?, switch))
}

#[cfg(test)]
mod empty_zone_tests {
    fn zone_record_layout(version: u8, private_len: usize, id: i32, word56: u16, group: u32, low: i16, high: i16) -> Vec<u8> {
        let mut public = vec![0; 48];
        public[16..18].copy_from_slice(&low.to_le_bytes());
        public[18..20].copy_from_slice(&high.to_le_bytes());
        let mut record = group.to_le_bytes().to_vec();
        record.extend([1, version, 0]);
        record.extend(u32::try_from(private_len).unwrap().to_le_bytes());
        let mut private=vec![0;private_len];
        private[56..58].copy_from_slice(&word56.to_le_bytes());
        private[private_len-4..].copy_from_slice(&id.to_le_bytes());
        record.extend(private);
        record.extend(48u32.to_le_bytes());
        record.extend(public);
        record.extend(0u32.to_le_bytes());
        record
    }
    fn zone_record(id: i32, group: u32, low: i16, high: i16) -> Vec<u8> {
        zone_record_layout(0x9a, 87, id, 0, group, low, high)
    }

    #[test]
    fn initializer_zone_prepass_uses_physical_order_and_needs_no_sample_resolver() {
        let mut data = 3u32.to_le_bytes().to_vec();
        data.extend(zone_record(0,2,60,61));
        data.extend(zone_record(7,0,36,48));
        data.extend(zone_record(2018,1,80,90));
        assert_eq!(super::source_zone_metadata(&data,3).unwrap(),
            [(0,[2,60,61]),(7,[0,36,48]),(2018,[1,80,90])].into());
        assert!(super::source_zone_metadata(&data,2).is_err());
        assert!(super::source_zone_metadata(&data[..data.len()-1],3).is_err());
        data[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(super::source_zone_metadata(&data,3).is_err());
    }

    #[test]
    fn missing_identity_diagnostics_retain_layout_counts_without_private_payloads() {
        let mut data = 3u32.to_le_bytes().to_vec();
        for id in [0, -1, -1] { data.extend(zone_record(id, 0, 60, 61)); }
        let (catalog, detail) = super::source_zone_catalog_with_layouts(&data, 1).unwrap();
        assert_eq!(catalog.iter().map(|zone| zone.id).collect::<Vec<_>>(), [Some(0), None, None]);
        assert_eq!(detail, "2 zones: version=0x9a private_bytes=87 private_u16_at_56=Some(0)");
    }

    #[test]
    fn identity_shared_by_several_zones_names_the_first() {
        let mut data = 3u32.to_le_bytes().to_vec();
        data.extend(zone_record(0,2,60,61));
        data.extend(zone_record(0,0,36,48));
        data.extend(zone_record(7,1,80,90));
        let ids:Vec<_>=super::source_zone_catalog(&data,3).unwrap().iter().map(|z|z.id).collect();
        // get_zone_id reads each zone's own identity; lookups by identity find the first.
        assert_eq!(ids,[Some(0),Some(0),Some(7)]);
        assert_eq!(super::source_zone_metadata(&data,3).unwrap(),[(0,[2,60,61]),(7,[1,80,90])].into());
    }

    #[test]
    fn every_native_private_layout_ends_with_its_identity() {
        let mut data = 4u32.to_le_bytes().to_vec();
        data.extend(zone_record_layout(0x9a, 87, 11, 0, 0, 60, 61));
        data.extend(zone_record_layout(0x9a, 140, 12, 1, 0, 60, 61));
        data.extend(zone_record_layout(0x9c, 152, 13, 1, 0, 60, 61));
        data.extend(zone_record_layout(0x9c, 176, 14, 1, 0, 60, 61));
        let (catalog, detail) = super::source_zone_catalog_with_layouts(&data, 1).unwrap();
        assert_eq!(catalog.iter().map(|zone| zone.id).collect::<Vec<_>>(), [Some(11), Some(12), Some(13), Some(14)]);
        assert!(detail.is_empty(), "{detail}");
    }

    #[test]
    fn automation_tags_name_sliders_and_switches() {
        assert_eq!(super::automation_target("pts_script_slider_3_2"), Some((3, 2, false)));
        assert_eq!(super::automation_target("pts_script_switch_2_81"), Some((2, 81, true)));
        assert_eq!(super::automation_target("pts_script_knob_2_1"), None);
    }

    #[test]
    fn tester_sampleless_zone_preserves_native_mapping_without_a_file_reference() {
        use std::io::Cursor;
        let mut public = vec![0; 48];
        public[16..18].copy_from_slice(&36i16.to_le_bytes());
        public[18..20].copy_from_slice(&81i16.to_le_bytes());
        let mut record = 2u32.to_le_bytes().to_vec();
        record.extend([1, 0x9a, 0]);
        record.extend(0u32.to_le_bytes());
        record.extend(48u32.to_le_bytes());
        record.extend(public);
        record.extend(0u32.to_le_bytes());
        let mut cursor = Cursor::new(record.as_slice());
        let zone = super::raw_zone(&mut cursor).unwrap();
        assert!(!zone.sample_present);
        assert_eq!((zone.group, zone.file), (2, -1));
        assert_eq!((zone.source.low_key, zone.source.high_key), (36, 81));
        assert_eq!(zone.source.zone_tune, 0.);
        assert_eq!(cursor.position(), record.len() as u64);
    }
}

#[cfg(test)]
mod zone_range_tests {
    use std::io::Cursor;

    /// A v0x9a sampled zone record in group 3 with file 0, gain 1 and tune 1.
    pub(super) fn sampled(velocities: [i16; 2], keys: [i16; 2], root: i16) -> Vec<u8> {
        let mut public = vec![0u8; 82];
        public[12..14].copy_from_slice(&velocities[0].to_le_bytes());
        public[14..16].copy_from_slice(&velocities[1].to_le_bytes());
        public[16..18].copy_from_slice(&keys[0].to_le_bytes());
        public[18..20].copy_from_slice(&keys[1].to_le_bytes());
        public[28..30].copy_from_slice(&root.to_le_bytes());
        public[30..34].copy_from_slice(&1f32.to_le_bytes());
        public[38..42].copy_from_slice(&1f32.to_le_bytes());
        public[43] = 1; // v0x9a prefix byte 1: a sample is present
        let mut record = 3u32.to_le_bytes().to_vec(); // owning group
        record.extend([1, 0x9a, 0]); // structured object, version 0x009a
        record.extend(0u32.to_le_bytes()); // private data length
        record.extend(u32::try_from(public.len()).unwrap().to_le_bytes());
        record.extend(public);
        record.extend(0u32.to_le_bytes()); // children length
        record
    }

    fn read(record: &[u8]) -> Result<super::RawZone, String> {
        super::raw_zone(&mut Cursor::new(record))
    }

    #[test]
    fn in_range_zone_is_unchanged() {
        let zone = read(&sampled([1, 127], [36, 81], 60)).unwrap();
        assert!(zone.sample_present);
        assert_eq!(zone.unreachable, None);
        assert_eq!((zone.keys, zone.velocities, zone.root), ([36, 81], [1, 127], 60));
    }

    #[test]
    fn zone_above_the_velocity_range_is_unreachable_not_an_error() {
        let zone = read(&sampled([139, 139], [40, 40], 40)).unwrap();
        assert_eq!(zone.unreachable.as_deref(), Some("velocity 139..=139; keys 40..=40; group 3"));
    }

    #[test]
    fn zone_below_the_key_range_is_unreachable() {
        let zone = read(&sampled([1, 127], [-20, -1], 60)).unwrap();
        assert_eq!(zone.unreachable.as_deref(), Some("key -20..=-1; velocities 1..=127; group 3"));
    }

    #[test]
    fn zone_partly_outside_still_fails_until_checked_against_kontakt() {
        let error = read(&sampled([100, 200], [36, 81], 60)).err().unwrap();
        assert_eq!(error, "high velocity 200 is outside 0..=127");
    }

    /// The zone parameter block starts `sample_start`, `sample_end`, then
    /// `sample_start_mod_range`, all i32 in source frames (public offsets 0, 4
    /// and 8; the record's public block begins 15 bytes in). Kontakt's start
    /// modulation adds an amount in frames and bounds it by this range in the
    /// same unit (decompile 0x14084f380 and the zone getter at 0x140884db0),
    /// so the range passes through unscaled. A -1 range is kept as no range
    /// (`max(0)`), as the oracle diff compares it.
    #[test]
    fn start_modulation_range_is_source_frames_and_minus_one_is_no_range() {
        let mut record = sampled([1, 127], [36, 81], 60);
        record[23..27].copy_from_slice(&1234i32.to_le_bytes());
        assert_eq!(read(&record).unwrap().start_mod, 1234);
        record[23..27].copy_from_slice(&(-1i32).to_le_bytes());
        assert_eq!(read(&record).unwrap().start_mod, 0);
    }

    #[test]
    fn range_boundaries_at_the_edge_of_midi() {
        let unreachable = |v: [i16; 2]| read(&sampled(v, [36, 81], 60)).unwrap().unreachable.is_some();
        assert!(!unreachable([0, 0]));
        assert!(!unreachable([127, 127]));
        assert!(unreachable([128, 128]));
        assert!(unreachable([-1, -1]));
        assert!(unreachable([i16::MIN, -1]));
        assert!(unreachable([128, i16::MAX]));
        // Straddling an edge is partly reachable: still an error, not a skip.
        assert!(read(&sampled([-1, 0], [36, 81], 60)).is_err());
        assert!(read(&sampled([127, 128], [36, 81], 60)).is_err());
        // Inverted and outside: not reported as unreachable.
        assert!(read(&sampled([200, 100], [36, 81], 60)).is_err());
    }

    #[test]
    fn inverted_range_is_still_an_error() {
        let error = read(&sampled([100, 50], [36, 81], 60)).err().unwrap();
        assert!(error.starts_with("inverted range"), "{error}");
    }

    #[test]
    fn out_of_range_root_is_still_an_error() {
        let error = read(&sampled([1, 127], [36, 81], 200)).err().unwrap();
        assert_eq!(error, "root 200 is outside 0..=127");
    }

    #[test]
    #[ignore = "needs an installed instrument: KONTRA_TEST_UNREACHABLE_NKI=/path/to/instrument.nki"]
    fn installed_instrument_with_unreachable_zones_loads() {
        let path = std::env::var_os("KONTRA_TEST_UNREACHABLE_NKI").expect("set KONTRA_TEST_UNREACHABLE_NKI");
        let source = crate::read(std::path::Path::new(&path)).unwrap();
        let unreachable: Vec<_> = source
            .instrument
            .unsupported
            .iter()
            .filter(|u| u.feature == "unreachable zone")
            .collect();
        for u in &unreachable {
            eprintln!("{}: {}", u.location, u.value);
        }
        assert!(!unreachable.is_empty(), "expected at least one unreachable-zone diagnostic");
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
    // Unknown private variants retain no invented command identity. The native
    // private reader ends every version branch with the identity read, so the
    // gate is the decoder, not a fixed length: an undecodable private stays
    // `None` and the catalog reports the layout instead of aliasing an ordinal.
    let source_id = source_zone.source_id().ok().flatten();
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
    let sample_present = params.sample_present;
    let midi = |value: i16, what| {
        if !sample_present {
            return Ok(0);
        }
        u8::try_from(value)
            .ok()
            .filter(|v| *v < 128)
            .ok_or(format!("{what} {value} is outside 0..=127"))
    };
    // A sampled zone whose whole stored velocity or key range lies outside
    // 0..=127 cannot be triggered by any MIDI note, so it never sounds. It is
    // kept for diagnostics instead of failing the instrument (spec §3.5;
    // checked against Kontakt 8 in docs/kontra-notes/a1-kontakt-reference.md).
    // A range only partly outside still fails below until Kontakt's
    // behaviour for it has been observed.
    let outside = |lo: i16, hi: i16| lo <= hi && (lo > 127 || hi < 0);
    let unreachable = if !sample_present {
        None
    } else if outside(lv, hv) {
        Some(format!("velocity {lv}..={hv}; keys {lk}..={hk}; group {group}"))
    } else if outside(lk, hk) {
        Some(format!("key {lk}..={hk}; velocities {lv}..={hv}; group {group}"))
    } else {
        None
    };
    let (lk, hk, lv, hv) = if unreachable.is_some() {
        (0, 0, 0, 0)
    } else {
        (
            midi(lk, "low key")?,
            midi(hk, "high key")?,
            midi(lv, "low velocity")?,
            midi(hv, "high velocity")?,
        )
    };
    if lk > hk || lv > hv {
        return Err(format!(
            "inverted range: keys {lk}..={hk}, velocities {lv}..={hv}"
        ));
    }
    if params.sample_present
        && (start < 0
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
    let slice_markers: Vec<u32> = match source_zone.0.find_first(ZONE_SLICES) {
        Some(chunk) => ni_file::kontakt::objects::ZoneSlices::try_from(chunk)
            .map_err(|e| e.to_string())?
            .markers
            .iter()
            .map(|marker| marker.frame)
            .collect(),
        None => Vec::new(),
    };
    let slice_tempo = match source_zone.0.private_data.get(12..16) {
        Some(bytes) if !slice_markers.is_empty() => {
            let tempo = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !(tempo.is_finite() && tempo > 0.0) {
                return Err(format!("zone grid playback rate {tempo}"));
            }
            tempo
        }
        _ => 1.0,
    };
    let source = crate::objects::zone(source_zone.0.version, group as u32, params, &loops);
    Ok(RawZone {
        slice_markers,
        slice_tempo,
        beat_grid: None,
        source_id,
        identity_layout: (source_zone.0.version, source_zone.0.private_data.len(),
            source_zone.0.private_data.get(56..58).map(|b| u16::from_le_bytes([b[0],b[1]]))),
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
        unreachable,
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
                            Modulator::Dbd(e) => format!("DBD {e:?}"),
                            Modulator::Glide(e) => format!("GLIDE {e:?}"),
                            Modulator::Follower(e) => format!("FOLLOWER {e:?}"),
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
#[path = "voice_group_default_tests.rs"]
mod voice_group_default_tests;

#[cfg(test)]
mod saved_tests {
    use super::*;
    #[test]
    fn native_primary_ahdsr_requires_exact_v1_wrapper_and_unity_target() {
        use ni_file::kontakt::{
            Chunk, StructuredObject,
            objects::{EnvelopeAhdsr, InternalMod, InternalModParams},
        };
        let target = |param: &str, intensity: f32| ni_file::kontakt::objects::ModTarget {
            param: param.into(),
            intensity,
            lag_ms: 0,
            name: String::new(),
            slot: None,
            invert: false,
            shaper: None,
            unknown_i16: -1,
            unknown_flags: 0x10,
        };
        let env = EnvelopeAhdsr {
            attack_ms: 125.012924,
            attack_curve: 0.75,
            hold_ms: 0.,
            decay_ms: 0.,
            sustain: 1.,
            release_ms: 25000.043,
            unknown_flag: 1,
            unknown_tail: [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0].repeat(4),
        };
        let mut child = Vec::new();
        env.to_chunk().unwrap().write(&mut child).unwrap();
        let mut data = vec![1];
        data.extend(0x90u16.to_le_bytes());
        for bytes in [&[][..], &[0; 4][..], &child] {
            data.extend((bytes.len() as u32).to_le_bytes());
            data.extend(bytes);
        }
        let raw = InternalMod(StructuredObject {
            version: 0x80,
            private_data: 2u32.to_le_bytes().to_vec(),
            public_data: vec![],
            children: vec![Chunk { id: 7, data }],
        });
        let mut t = target("volume", 1.);
        t.unknown_flags = 0x10;
        let params = InternalModParams {
            name: "amp".into(),
            targets: vec![t],
            modulator: Modulator::Ahdsr(env.clone()),
            unknown_flags: [0, 0, 1, 0],
            unknown_id: 0,
        };
        assert!(Translation::native_primary_ahdsr(&raw, &params, &env));
        let mut open = params.clone();
        open.unknown_flags[0] = 1;
        assert!(
            Translation::native_primary_ahdsr(&raw, &open, &env),
            "router UI is independent"
        );
        for change in 0..6 {
            let mut other = params.clone();
            match change {
                0 => other.targets[0].intensity = 0.5,
                1 => other.targets[0].lag_ms = 1,
                2 => other.targets[0].invert = true,
                3 => other.unknown_flags[1] = 1,
                4 => other.targets.push(target("filterCutoff", 1.)),
                _ => other.targets[0].unknown_flags = 0x12,
            }
            assert!(!Translation::native_primary_ahdsr(&raw, &other, &env));
        }
        let mut opaque = env.clone();
        opaque.unknown_tail[0] = 1;
        assert!(!Translation::native_primary_ahdsr(&raw, &params, &opaque));
        let mut wrapper = raw;
        wrapper.0.children[0].data[1] = 0x91;
        assert!(!Translation::native_primary_ahdsr(&wrapper, &params, &env));
    }

    #[test]
    fn ahdsr_preparation_lowers_stored_knobs_before_curve_normalization() {
        let mut indices = ir::SourceIndices::default();
        let envelope = ni_file::kontakt::objects::EnvelopeAhdsr {
            attack_ms: 125.012924,
            attack_curve: 0.75,
            hold_ms: 0.012345,
            decay_ms: 25000.043,
            sustain: 0.4,
            release_ms: 1234.567,
            unknown_flag: 1,
            unknown_tail: vec![0; 52],
        };
        Translation::source_envelope(&mut indices, 7, 12, &envelope);
        let source = indices
            .ahdsrs
            .first()
            .expect("raw AHDSR descriptor lost before preparation");
        assert_eq!((source.group, source.slot), (7, 12));
        assert_eq!(
            [
                source.attack_ms,
                source.attack_curve,
                source.hold_ms,
                source.decay_ms,
                source.sustain,
                source.release_ms
            ]
            .map(f32::to_bits),
            [
                ahdsr_attack_ms(envelope.attack_ms),
                envelope.attack_curve,
                ahdsr_attack_ms(envelope.hold_ms),
                ahdsr_decay_ms(envelope.decay_ms),
                ahdsr_sustain(envelope.sustain),
                ahdsr_decay_ms(envelope.release_ms)
            ]
            .map(f32::to_bits)
        );
        assert!(source.ahd_only);
        assert!(
            !source.native_amplitude,
            "standalone scalars cannot prove native primary geometry"
        );
    }

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
            // Stored sustain knob 0.4 is cubed: level 0.064 (x³, not linear).
            64000
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
            &[],
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
            let frames =
                sampler_core::EngineParameterLaw::envelope(stage, 48000).encode(native * 48.);
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
                &instrument.zones,
                Default::default(),
            );
            environment.engine_values.clear();
            let cold = sampler_ksp::initialize(
                &behavior.source,
                sampler_ksp::Limits::LIBRARY,
                &environment,
            )
            .unwrap_or_else(|_| panic!("init failed; authored diagnostics omitted"))
            .engine_pars();
            for group in 0..instrument.groups.len() {
                environment
                    .engine_values
                    .insert([i32::from(parameter), group as i32, 1, -1], 1_000_000);
            }
            let authored = sampler_ksp::initialize(
                &behavior.source,
                sampler_ksp::Limits::LIBRARY,
                &environment,
            )
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
        std::fs::write(root.join("Piano.nki"), [0; 16]).unwrap();
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
            public.extend(0u32.to_le_bytes()); // persistent-table count
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
                Default::default(),
                None,
                &[],
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

    fn menu_program() -> ni_file::kontakt::objects::Program {
        use ni_file::kontakt::{Chunk, StructuredObject, objects::Program};
        let source = r#"on init
            make_perfview
            declare ui_menu $mode
            add_menu_item($mode, "Soft", 4)
            add_menu_item($mode, "Linear", -3)
            add_menu_item($mode, "Hard", 0)
            make_persistent($mode)
            read_persistent_var($mode)
            declare ui_knob $echo(-100,100,1)
        end on
        on persistence_changed
            $echo := $mode
        end on"#;
        let mut script = vec![0, 0x50, 0];
        script.extend((source.len() as u32).to_le_bytes());
        script.extend(source.as_bytes());
        script.extend([0; 3]);
        script.extend(0u32.to_le_bytes());
        script.extend(u32::MAX.to_le_bytes());
        script.extend(0u32.to_le_bytes());
        script.extend(0u32.to_le_bytes()); // persistent-table count
        Program(StructuredObject {
            version: 0xaf,
            public_data: vec![0; 70],
            private_data: vec![],
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
        })
    }

    #[test]
    fn translated_empty_numeric_script_arrays_do_not_fail_library_open() {
        let path =
            std::env::temp_dir().join(format!("empty-script-arrays-{}.nki", std::process::id()));
        std::fs::write(&path, [0; 16]).unwrap();
        let mut program = menu_program();
        let script = program.0.children.iter_mut().find(|c| c.id == 6).unwrap();
        script.data.truncate(script.data.len() - 4); // the fixture's empty persistent table
        script.data.extend(2u32.to_le_bytes());
        for entry in [b"%steps ".as_slice(), b"?xy "] {
            script.data.extend((entry.len() as u32).to_le_bytes());
            script.data.extend(entry);
        }
        let translated = super::translate(
            path.clone(),
            program,
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            &[],
            None,
        )
        .unwrap();
        assert_eq!(
            translated.instrument.behaviors[0].state,
            [
                ("%steps".into(), sampler_ir::Saved::Ints(vec![])),
                ("?xy".into(), sampler_ir::Saved::Reals(vec![])),
            ]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn restored_translation_initializes_once_with_host_menu_values() {
        let path = std::env::temp_dir().join(format!("restored-menu-{}.nki", std::process::id()));
        std::fs::write(&path, [0; 16]).unwrap();
        let mode = sampler_ksp::derived_control_id(0, "$mode");
        let echo = sampler_ksp::derived_control_id(0, "$echo");
        for saved in [4, -3, 0] {
            crate::load::take_script_init_runs();
            let controls = vec![(mode, saved)];
            let translated = super::translate(
                path.clone(),
                menu_program(),
                Default::default(),
                Default::default(),
                Default::default(),
                None,
                &controls,
                None,
            )
            .unwrap();
            let loaded = crate::load_read(
                translated,
                &crate::Options {
                    control_values: controls,
                    ..Default::default()
                },
                |_| {},
                || false,
            )
            .unwrap();
            assert_eq!(
                crate::load::take_script_init_runs(),
                1,
                "restored translation must initialize once"
            );
            for id in [mode, echo] {
                assert_eq!(
                    loaded
                        .plan
                        .controls()
                        .iter()
                        .find(|c| c.id == id)
                        .unwrap()
                        .default,
                    sampler_core::ControlValue::Integer(i64::from(saved)),
                    "host menu item values reach persistence_changed"
                );
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn translated_ui_consumes_init_once_and_reinitializes_for_late_controls() {
        let path = std::env::temp_dir().join(format!("ui-init-{}.nki", std::process::id()));
        std::fs::write(&path, [0; 16]).unwrap();
        let mode = sampler_ksp::derived_control_id(0, "$mode");
        let echo = sampler_ksp::derived_control_id(0, "$echo");
        let defaults = |scripts: &[sampler_ksp::Script]| {
            [mode, echo].map(|id| {
                scripts[0]
                    .controls()
                    .iter()
                    .find(|c| c.definition.id == id)
                    .unwrap()
                    .definition
                    .default
            })
        };

        crate::load::take_script_init_runs();
        let mut translated = super::translate(
            path.clone(),
            menu_program(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            &[(mode, 4)],
            None,
        )
        .unwrap();
        assert_eq!(crate::load::take_script_init_runs(), 1);
        let (scripts, faces, _) = translated.compile_ui(&crate::Options {
            library: Some(path.clone()),
            control_values: vec![(mode, 4)],
            ..Default::default()
        });
        assert_eq!(crate::load::take_script_init_runs(), 0);
        assert_eq!(scripts.len(), 1);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].widgets.len(), 2);
        assert!(faces[0]
            .widgets
            .iter()
            .all(|widget| widget.value == Some(sampler_ui_ir::Value::Integer(4))));
        assert_eq!(
            defaults(&scripts),
            [sampler_core::ControlValue::Integer(4); 2]
        );

        let mut translated = super::translate(
            path.clone(),
            menu_program(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            &[],
            None,
        )
        .unwrap();
        assert_eq!(crate::load::take_script_init_runs(), 1);
        let (scripts, faces_changed, _) = translated.compile_ui(&crate::Options {
            library: Some(path.clone()),
            control_values: vec![(mode, -3)],
            ..Default::default()
        });
        assert_eq!(crate::load::take_script_init_runs(), 1);
        assert!(faces_changed[0]
            .widgets
            .iter()
            .all(|widget| widget.value == Some(sampler_ui_ir::Value::Integer(-3))));
        assert_eq!(
            faces_changed[0]
                .widgets
                .iter()
                .map(|widget| widget.name.as_str())
                .collect::<Vec<_>>(),
            faces[0]
                .widgets
                .iter()
                .map(|widget| widget.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            defaults(&scripts),
            [sampler_core::ControlValue::Integer(-3); 2]
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn late_host_override_reinitializes_instead_of_reusing_wrong_state() {
        let path = std::env::temp_dir().join(format!("late-menu-{}.nki", std::process::id()));
        std::fs::write(&path, [0; 16]).unwrap();
        let mode = sampler_ksp::derived_control_id(0, "$mode");
        crate::load::take_script_init_runs();
        let translated = super::translate(
            path.clone(),
            menu_program(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            &[],
            None,
        )
        .unwrap();
        let loaded = crate::load_read(
            translated,
            &crate::Options {
                control_values: vec![(mode, -3)],
                ..Default::default()
            },
            |_| {},
            || false,
        )
        .unwrap();
        assert_eq!(crate::load::take_script_init_runs(), 2);
        assert_eq!(
            loaded
                .plan
                .controls()
                .iter()
                .find(|c| c.id == mode)
                .unwrap()
                .default,
            sampler_core::ControlValue::Integer(-3)
        );
        std::fs::remove_file(path).unwrap();
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
    include!("modulation_descriptor_tests.rs");
    include!("voice_allocation_tests.rs");
    use super::*;
    use ni_file::kontakt::objects::{Lfo, LfoRecord, ModTarget};

    #[test]
    /// DBD v0x12 stored record: knobs 0.5 are 221.6 ms each, level 0.25 raw,
    /// start 1/3; the flag zeroes the envelope; refused shapes stay diagnosed.
    #[test]
    fn dbd_v12_lowers_to_the_native_three_point_ramp() {
        use ni_file::kontakt::objects::EnvelopeDbd;
        let dbd = |version, values, flag| EnvelopeDbd {
            version,
            values,
            extra: None,
            records: None,
            flag: Some(flag),
        };
        let b = dbd_breakpoints(&dbd(0x12, [0.5, 0.25, 0.5], 0)).unwrap();
        assert_eq!(b.points.len(), 3);
        assert_eq!(b.points[1].level, 0.25);
        let ir::Time::Milliseconds(ms) = b.points[1].time else { panic!("ms") };
        assert!((ms - 221.615_87).abs() < 1e-3, "{ms}");
        assert_eq!(b.points[2].level, 0.0);
        let zero = dbd_breakpoints(&dbd(0x12, [0.5, 0.25, 0.5], 1)).unwrap();
        assert_eq!(zero.points.len(), 1);
        assert_eq!(zero.points[0].level, 0.0);
        assert!(dbd_breakpoints(&dbd(0x12, [0.5, 0.5, 0.0], 0)).is_none());
        assert!(dbd_breakpoints(&dbd(0x11, [0.5, 0.5, 0.5], 0)).is_none());
    }

    /// Stored AHDSR times are milliseconds (Dolce staccato stores 80 ms attack;
    /// the corpus stores 80/125/200 ms attacks and 120/351/1050 ms releases). The
    /// 15 s / 25 s laws apply to normalized control values, not to these fields.
    /// Sustain stays the cubed knob.
    #[test]
    fn ahdsr_stored_times_are_native_milliseconds_and_sustain_is_a_knob() {
        assert_eq!(ahdsr_attack_ms(80.0077), 80.0077);
        assert_eq!(ahdsr_decay_ms(351.0), 351.0);
        // Raw stored values from the native-file survey (2026-10-23): Dolce
        // legato/staccato (v174), ANALOG STRINGS 1.05 ms (v171), Una Corda
        // 25000.04 ms release at the law maximum (v168), Dolce 1050 ms release.
        assert_eq!(ahdsr_attack_ms(139.9995574951172), 139.9995574951172);
        assert_eq!(ahdsr_attack_ms(0.00010704994201660156), 0.00010704994201660156);
        assert_eq!(ahdsr_attack_ms(1.05), 1.05);
        assert_eq!(ahdsr_decay_ms(213.49972534179688), 213.49972534179688);
        assert_eq!(ahdsr_decay_ms(1050.0009765625), 1050.0009765625);
        assert_eq!(ahdsr_decay_ms(25000.04), 25000.04);
        assert_eq!(ahdsr_attack_ms(-1.0), 0.0);
        assert_eq!(ahdsr_sustain(0.5), 0.125);
        assert_eq!(ahdsr_sustain(2.0), 1.0);
    }

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

    #[test]
    fn modulator_intensity_assignment_scales_the_target_modulators_routes() {
        // Conflux "MOD ENV -> LFO 1 AMT": an assignment whose destination is
        // another modulator's intensity becomes an ID25 scale on that
        // modulator's routes, never a route of its own. Outside the proved
        // envelope (here negative depth, flags 0x12) it is reported instead.
        let mut t = translation();
        t.ir.modulators = vec![
            ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Velocity },
            ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Key },
        ];
        for (slot, runtime) in [(1, 0), (2, 1)] {
            t.ir.source_indices.modulators.push(ir::SourceModulator {
                group: 0,
                slot,
                external: false,
                name: String::new(),
                runtime: Some(ir::ModulatorRef(runtime)),
                settings: None,
            });
        }
        // The destination modulator (slot 2) drives volume at intensity 0.8.
        t.route_intensity.insert(0, (0.8, 1.0));
        t.ir.routes.push(ir::Route::new(ir::ModulatorRef(1), ir::Target::Amplitude, ir::Depth::Normalized(0.8)));
        let assignment = |flags| IntensityScale {
            at: "group 0 modulator slot 1".into(),
            slot: 2,
            source: ir::ModulatorRef(0),
            depth: 0.5,
            flags,
            inverted: false,
            lag_ms: 0,
            shape: None,
        };
        t.intensity_scales.push(assignment(0x10));
        t.apply_intensity_scales();
        assert_eq!(
            t.ir.routes[0].scale,
            Some(ir::RouteScale {
                source: ir::ModulatorRef(0),
                shape: None,
                law: ir::ScaleLaw::KontaktIntensity { depth: 0.5, flags: 0x10, unit: 1.0 },
                lag_ms: 0.,
            })
        );
        t.ir.routes[0].scale = None;
        t.intensity_scales.push(assignment(0x11));
        t.apply_intensity_scales();
        assert_eq!(t.ir.routes[0].scale, None);
        assert!(t.ir.unsupported.iter().any(|u| u.feature == "modulation of a modulator's intensity"));
    }

    /// A zero-intensity route to a destination that is not modeled is inert: no
    /// route, no report. A non-zero one still reports.
    #[test]
    fn zero_intensity_unmodeled_routes_are_silent() {
        let mut out = translation();
        out.ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Constant,
        });
        let source = ir::ModulatorRef(out.ir.modulators.len() - 1);
        let silent = ModTarget {
            slot: Some(3),
            ..target("frequency", 0.0)
        };
        assert!(out.route("g", source, true, &silent, None).is_none());
        assert!(out.ir.unsupported.is_empty());
        let live = ModTarget {
            slot: Some(3),
            ..target("frequency", 0.5)
        };
        assert!(out.route("g", source, true, &live, None).is_none());
        assert_eq!(out.ir.unsupported.len(), 1);
        assert_eq!(out.ir.unsupported[0].feature, "modulation of a module parameter");
    }

    #[test]
    fn one_script_address_has_one_alias_and_the_measured_lane_wins() {
        let mut t = translation();
        let at = [0, 5, -1];
        t.claim_alias(ir::ControlRef(1), "ENGINE_PAR_INTMOD_INTENSITY", at, false);
        t.claim_alias(ir::ControlRef(2), "ENGINE_PAR_INTMOD_INTENSITY", at, false);
        t.claim_alias(ir::ControlRef(3), "ENGINE_PAR_MOD_TARGET_MP_INTENSITY", at, false);
        let aliases = &t.ir.source_indices.control_aliases;
        assert_eq!(aliases.iter().map(|a| a.control.0).collect::<Vec<_>>(), [1, 3]);
        t.claim_alias(ir::ControlRef(4), "ENGINE_PAR_INTMOD_INTENSITY", at, true);
        let aliases = &t.ir.source_indices.control_aliases;
        assert_eq!(aliases.iter().map(|a| a.control.0).collect::<Vec<_>>(), [4, 3]);
    }

    fn translation() -> Translation {
        Translation {
            ir: ir::Instrument::default(),
            assets: HashMap::new(),
            locations: Vec::new(),
            start_criteria: Vec::new(),
            voice_groups: Vec::new(),
            snapshot_groups: Vec::new(),
            retained_snapshot: Default::default(),
            engine: Vec::new(),
            dynamic: false,
            eq_mod_slots: HashMap::new(),
            cutoff_octaves: HashMap::new(),
            route_intensity: HashMap::new(),
            alias_claims: HashMap::new(),
            intensity_scales: Vec::new(),
            lane_slots: Vec::new(),
            intensity_slots: HashMap::new(),
            engine_by_group: Default::default(),
        dormant_modulators: Default::default(),
            amplitude_envelopes: Vec::new(),
            send_taps: Vec::new(),
            #[cfg(feature = "scan")]
            target_outcomes: HashMap::new(),
        }
    }

    /// A group with no routes, envelope or insert chain: only what a zone inherits.
    fn plain_group(reverse: bool) -> GroupInfo {
        GroupInfo {
            index: 0,
            group: ir::GroupRef(0),
            release: false,
            tracking: true,
            reverse,
            envelope: None,
            velocity: ir::VelocityResponse::None,
            routes: Vec::new(),
            chain: None,
            bus: None,
        }
    }

    /// BASIC_BEHAVIORS row 21: a zone tune ratio of 2.0 is +12 semitones.
    #[test]
    fn basic_behavior_zone_tune_ratio_two_is_plus_twelve_semitones() {
        let mut out = translation();
        // The sampled record from zone_range_tests (tune 1, public offset 38);
        // the public block starts after the group id, header and two lengths.
        let mut record = super::zone_range_tests::sampled([0, 127], [0, 127], 60);
        record[4 + 3 + 4 + 4 + 38..][..4].copy_from_slice(&2f32.to_le_bytes());
        let zone = super::raw_zone(&mut std::io::Cursor::new(record.as_slice())).unwrap();
        let program = ni_file::kontakt::objects::ProgramPublicParams {
            tune: 1.0,
            ..Default::default()
        };
        out.zone(
            0,
            zone,
            None,
            &plain_group(false),
            &program,
            "tone.wav".into(),
        );
        let [made] = out.ir.zones.as_slice() else {
            panic!("one zone")
        };
        let ir::Pitch::Semitones(semitones) = made.tune else {
            panic!("semitone tune")
        };
        assert!((semitones - 12.).abs() < 1e-9, "{semitones}");
    }

    /// BASIC_BEHAVIORS row 20: a zone inherits reverse from its reversed group.
    #[test]
    fn basic_behavior_reversed_group_sets_reverse_on_its_zones() {
        let mut out = translation();
        let record = super::zone_range_tests::sampled([0, 127], [0, 127], 60);
        let zone = super::raw_zone(&mut std::io::Cursor::new(record.as_slice())).unwrap();
        let program = ni_file::kontakt::objects::ProgramPublicParams {
            tune: 1.0,
            ..Default::default()
        };
        out.zone(
            0,
            zone,
            None,
            &plain_group(true),
            &program,
            "ramp.wav".into(),
        );
        let [made] = out.ir.zones.as_slice() else {
            panic!("one zone")
        };
        assert!(made.playback.reverse);
    }

    #[test]
    fn group_send_taps_keep_physical_return_identity_and_amplifier_side() {
        let mut out = translation();
        out.ir.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: Vec::new(),
            post_amplitude: Vec::new(),
        });
        out.send_taps.push((
            ir::ChainRef(0),
            crate::effects::SendTap {
                slot: 5,
                position: ir::VoiceSendPosition::AfterAmplitude(0),
                levels: vec![0.0, 0.0, 0.0, 0.5],
                bypass: false,
            },
        ));
        out.resolve_send_taps(&[(3, ir::BusRef(1))]);
        assert_eq!(
            out.ir.voice_send_taps,
            vec![ir::VoiceSendTap {
                chain: ir::ChainRef(0),
                position: ir::VoiceSendPosition::AfterAmplitude(0),
                bus: ir::BusRef(1),
                gain: ir::Gain::Linear(0.5),
                bypass: false,
                gain_control: None,
                bypass_control: None,
                ramp: ir::Time::Seconds(0.0),
            }]
        );
        assert!(out.ir.unsupported.is_empty());
    }

    #[test]
    fn source_mod_and_target_lookups_keep_holes_and_unmodeled_targets() {
        let mut out = translation();
        let mut unnamed = target("not-modeled", 1.);
        unnamed.name.clear();
        let mut named = target("not-modeled", 1.);
        named.name = "Cutoff".into();
        out.source_modulator(7, 31, true, "Controller".into(), &[unnamed, named], None);
        out.source_modulator(7, 12, false, "Envelope".into(), &[], None);
        let lookups = &out.ir.source_indices.engine_lookups;
        assert_eq!(lookups.len(), 4);
        assert_eq!(
            (
                lookups[0].group,
                lookups[0].owner,
                lookups[0].target,
                lookups[0].index
            ),
            (7, -1, false, 31)
        );
        assert_eq!(
            (
                lookups[2].group,
                lookups[2].owner,
                lookups[2].target,
                lookups[2].index
            ),
            (7, 31, true, 1)
        );
        assert_eq!(lookups[2].name, "Cutoff");
        assert_eq!(lookups[3].index, 12);
        assert_eq!(out.ir.source_indices.modulators[0].slot, 31);
        assert!(out.ir.source_indices.modulators[0].external);
        assert_eq!(out.ir.source_indices.modulators[0].runtime, None);
    }

    #[test]
    fn authored_init_intensity_uses_the_same_physical_modulator_slot() {
        let mut out = translation();
        out.source_modulator(7, 31, true, "Controller".into(), &[], None);
        out.engine.push(sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_MOD_TARGET_INTENSITY".into(),
            group: 7,
            slot: 31,
            generic: -1,
            value: 500000,
        });
        assert_eq!(out.script_intensity(7, "controller"), Some(0.5));
    }

    /// The indexed lookups keep the scan's answers: the first modulator named
    /// case-insensitively within the group wins, and the last engine write wins.
    #[test]
    fn script_intensity_index_matches_first_lookup_and_last_write() {
        let mut out = translation();
        out.source_modulator(3, 1, true, "Foo".into(), &[], None);
        out.source_modulator(3, 2, true, "foo".into(), &[], None);
        out.source_modulator(4, 5, true, "FOO".into(), &[], None);
        let write = |group, slot, value| sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_MOD_TARGET_INTENSITY".into(),
            group,
            slot,
            generic: -1,
            value,
        };
        out.engine.push(write(3, 1, 100_000));
        out.engine.push(write(3, 1, 250_000));
        out.engine.push(write(4, 5, 750_000));
        assert_eq!(out.script_intensity(3, "FOO"), Some(0.25));
        assert_eq!(out.script_intensity(4, "foo"), Some(0.75));
        assert_eq!(out.script_intensity(3, "bar"), None);
        assert_eq!(out.script_intensity(5, "foo"), None);
        assert_eq!(out.script_par("ENGINE_PAR_MOD_TARGET_INTENSITY", 3, 2, -1), None);
    }

    fn group_public(interp_quality: i32) -> Vec<u8> {
        let mut public = 0u32.to_le_bytes().to_vec();
        for value in [1f32, 0.0, 1.0] {
            public.extend(value.to_le_bytes());
        }
        public.extend([1, 0, 0, 0]);
        public.extend(0i32.to_le_bytes());
        public.extend((-1i16).to_le_bytes());
        public.extend([0; 10]); // Voice group, amplifier split, mute/solo.
        public.extend(interp_quality.to_le_bytes());
        public
    }

    #[test]
    fn unmodeled_group_interpolation_is_reported_not_dropped() {
        let mut group = Group(ni_file::kontakt::StructuredObject {
            version: 0x95,
            private_data: vec![],
            public_data: group_public(2),
            children: vec![ni_file::kontakt::Chunk {
                id: 0x38,
                data: vec![0],
            }],
        });
        let mut out = translation();
        assert!(out.group(0, &group).unwrap().is_some());
        assert!(
            out.ir
                .unsupported
                .iter()
                .any(|n| n.feature == "interpolation quality" && n.value == "2")
        );
        group.0.public_data = group_public(0);
        let mut default = translation();
        assert!(default.group(0, &group).unwrap().is_some());
        assert!(
            !default
                .ir
                .unsupported
                .iter()
                .any(|n| n.feature == "interpolation quality")
        );
    }

    #[test]
    fn fx_decode_group_insert_errors_reach_the_ir_report() {
        let mut group = Group(ni_file::kontakt::StructuredObject {
            version: 0x95,
            private_data: vec![],
            public_data: group_public(0),
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
        group.0.public_data[34] = 1; // Muted definitions retain FX decoding and diagnostics.
        let mut muted = translation();
        assert!(muted.group(0, &group).unwrap().is_some());
        assert!(muted.ir.groups[0].muted);
        assert!(
            muted
                .ir
                .unsupported
                .iter()
                .any(|n| n.feature == "effect decoding")
        );
    }

    #[test]
    fn legacy_sine_multi_retains_subunit_signed_weights_and_phase() {
        let mut out = translation();
        for weight in [0.3, -0.7, 1.] {
            for normalize in [false, true] {
                let raw = Lfo {
                    structured: false,
                    version: 0x71,
                    waveform: 5,
                    initial_values: [0., 4., 0.25, 0.125],
                    records: [
                        LfoRecord {
                            flag: normalize,
                            values: [-1., 0., 0.],
                        },
                        LfoRecord {
                            flag: false,
                            values: [-1., 0., 0.],
                        },
                    ],
                    trailing_flag: false,
                    trailing_values: Some([weight, 0., 0., 0., 0.]),
                    additional_flag: None,
                };
                let source = out.lfo("g", &raw, true).unwrap();
                assert_eq!(source.shape, ir::LfoShape::SineScaled(-f64::from(weight)));
                assert_eq!(source.phase, 0.125);
            }
        }
    }

    #[test]
    fn constant_loop_targets_retain_signed_depth_and_unmeasured_routes_stay_reported() {
        let mut out = translation();
        out.ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Constant,
        });
        let source = ir::ModulatorRef(0);
        for (name, expected) in [
            ("loopStart", ir::Target::LoopStart),
            ("loopLength", ir::Target::LoopLength),
        ] {
            let mut raw = target(name, 0.1266360729932785);
            raw.unknown_flags |= 2;
            let route = out.route("g", source, true, &raw, None).unwrap();
            assert_eq!(out.ir.routes[route.0].target, expected);
            assert_eq!(
                out.ir.routes[route.0].depth,
                ir::Depth::Normalized(-f64::from(raw.intensity))
            );
            raw.lag_ms = 1;
            assert!(out.route("g", source, true, &raw, None).is_none());
            assert_eq!(
                out.ir.unsupported.last().unwrap().feature,
                "modulation target"
            );
        }
    }

    #[test]
    fn source_phase_uses_the_internal_slot_namespace() {
        let mut out = translation();
        out.ir.modulators = vec![
            ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Constant,
            },
            ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Lfo(ir::Lfo {
                    shape: ir::LfoShape::Sine,
                    rate: ir::Frequency::Hertz(1.),
                    phase: 0.,
                    delay: ir::Time::ZERO,
                    fade_in: ir::Time::ZERO,
                    retrigger: true,
                }),
            },
        ];
        out.source_modulator(7, 31, true, String::new(), &[], None);
        out.source_modulator(7, 12, false, String::new(), &[], None);
        out.ir.source_indices.modulators[0].runtime = Some(ir::ModulatorRef(0));
        out.ir.source_indices.modulators[1].runtime = Some(ir::ModulatorRef(1));
        let phase = ModTarget {
            slot: Some(12),
            ..target("startPhase", 0.25)
        };
        let route = out
            .route("g", ir::ModulatorRef(0), true, &phase, None)
            .unwrap();
        assert_eq!(
            out.ir.routes[route.0].target,
            ir::Target::SourcePhase(ir::ModulatorRef(1))
        );
        assert_eq!(out.ir.routes[route.0].depth, ir::Depth::Normalized(0.25));
        assert!(out.ir.unsupported.is_empty());
        out.ir.source_indices.modulators[1].runtime = None;
        assert!(
            out.route("g", ir::ModulatorRef(0), true, &phase, None)
                .is_none()
        );
        assert_eq!(
            out.ir.unsupported.last().unwrap().feature,
            "modulation of a module parameter"
        );
    }

    #[test]
    fn wavetable_targets_bind_to_the_real_source_group_and_keep_normalized_depth() {
        let mut out = translation();
        out.ir.groups.push(ir::Group { wavetable: Some(ir::Wavetable::default()), ..Default::default() });
        out.ir.source_indices.groups = vec![None; 8];
        out.ir.source_indices.groups[7] = Some(ir::GroupRef(0));
        out.ir.modulators.push(ir::Modulator { scope: ir::Scope::Voice, source: ir::ModulationSource::Velocity });
        out.source_modulator(7, 31, true, String::new(), &[], None);
        out.ir.source_indices.modulators[0].runtime = Some(ir::ModulatorRef(0));
        for (name, parameter) in [("wavetablePosition", ir::WavetableParameter::Position),
            ("warpFactor", ir::WavetableParameter::Form1), ("warpFactor2", ir::WavetableParameter::Form2)] {
            let route = out.route("source", ir::ModulatorRef(0), true, &target(name, 0.25), None).unwrap();
            assert_eq!(out.ir.routes[route.0].target, ir::Target::Wavetable(parameter));
            assert_eq!(out.ir.routes[route.0].depth, ir::Depth::Normalized(0.25));
        }
        out.ir.groups[0].wavetable = None;
        assert!(out.route("sample", ir::ModulatorRef(0), true, &target("wavetablePosition", 0.25), None).is_none());
    }

    /// Exercise the real source/group publication order, including a sparse
    /// physical group index and an already translated non-wavetable sibling.
    #[test]
    fn group_translation_keeps_wavetable_routes_before_source_map_was_published() {
        use ni_file::kontakt::objects::WavetableSource;
        for admitted in [true, false] {
            let mut out = translation();
            out.group(0, &deferred_rate_group(false, false)).unwrap();
            let targets = ["wavetablePosition", "warpFactor", "warpFactor2"]
                .map(|name| target(name, 0.25)).to_vec();
            let mut group = deferred_source_group(true, targets, false, true);
            let mut private = Vec::new();
            for _ in 0..136 {
                private.extend(8u32.to_le_bytes());
                private.extend([0; 8]);
            }
            private.extend([0; 24]);
            private.extend([0, 0x13, 0]);
            private.extend(8u32.to_le_bytes());
            private.extend([0; 8]);
            private.push(1);
            let mut common = [0; 30];
            common[..7].copy_from_slice(&[0, 6, 1, 9, 0, 0, 0]);
            WavetableSource {
                common, position: 0.25, phase: 0., phase_random: 0.,
                form1: 0.5, form2: 0., form_type: if admitted { 17 } else { 20 },
                form2_type: 1, quality: 1, inharmonic_enabled: false,
                inharmonic: 0., mod_wave: 0, mod_type: 0, mod_amount: 0.,
                mod_tune: 0., unknown_tail: [0; 16],
            }.write(&mut private).unwrap();
            private.extend([0xff; 22]);
            group.0.private_data = private;
            out.source_modulators(7, &group).unwrap();
            let info = out.group(7, &group).unwrap().unwrap();
            assert_eq!(info.group, ir::GroupRef(1));
            assert_eq!(out.ir.source_indices.groups.len(), 8);
            assert_eq!(out.ir.source_indices.groups[0], Some(ir::GroupRef(0)));
            assert_eq!(out.ir.source_indices.groups[7], Some(info.group));
            let routes: Vec<_> = info.routes.iter().filter(|r|
                matches!(out.ir.routes[r.0].target, ir::Target::Wavetable(_))).collect();
            assert_eq!(routes.len(), if admitted { 3 } else { 0 });
            for route in routes {
                assert_eq!(out.ir.routes[route.0].depth, ir::Depth::Normalized(0.25));
                assert!(out.ir.source_indices.route_controls.iter().any(|(r, _, _)| r == route));
            }
            assert_eq!(out.ir.groups[info.group.0].wavetable.is_some(), admitted);
        }
    }

    #[test]
    fn published_processor_controls_accept_native_modulation_by_physical_slot() {
        let mut out = translation();
        out.source_modulator(7, 31, true, String::new(), &[], None);
        out.ir.source_indices.modulators[0].runtime = Some(ir::ModulatorRef(0));
        out.ir.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::Gain(ir::Gain::Linear(1.))],
            post_amplitude: vec![],
        });
        let control = ir::ControlRef(0);
        out.ir.controls.push(ir::Control {
            key: "native knob".into(),
            label: "Drive".into(),
            value: ir::ControlValue::Continuous {
                min: 0.,
                max: 1.,
                default: 0.5,
                unit: ir::ControlUnit::Percent,
            },
            automation: ir::Automation::None,
        });
        out.ir.processor_controls.push(ir::ProcessorControl {
            control,
            chain: ir::ChainRef(0),
            index: 0,
            parameter: ir::ProcessorParameter::GainDecibels,
            ramp: ir::Time::ZERO,
        });
        out.ir
            .source_indices
            .control_aliases
            .push(ir::SourceControlAlias {
                control,
                parameter: "ENGINE_PAR_DRIVE".into(),
                address: ir::SlotAddress {
                    group: 7,
                    slot: 5,
                    generic: -1,
                },
            });
        let drive = ModTarget {
            slot: Some(5),
            ..target("distortionIntensity", 0.25)
        };
        let route = out
            .route(
                "g",
                ir::ModulatorRef(0),
                true,
                &drive,
                Some((ir::ChainRef(0), &[(5, 0)])),
            )
            .unwrap();
        assert_eq!(out.ir.routes[route.0].target, ir::Target::Control(control));
        assert_eq!(out.ir.routes[route.0].depth, ir::Depth::Normalized(0.25));
        assert!(out.ir.unsupported.is_empty());
        let sibling = ModTarget {
            slot: Some(4),
            ..drive
        };
        assert!(
            out.route(
                "g",
                ir::ModulatorRef(0),
                true,
                &sibling,
                Some((ir::ChainRef(0), &[(5, 0)]))
            )
            .is_none()
        );
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

    /// The release counter counts `T` ms only in a release-trigger group with
    /// `T > 0` (decompile 0x14084f380, source type 10). Otherwise its value is
    /// the constant 1, not a dropped route.
    #[test]
    fn release_counter_is_a_counter_only_in_a_release_group_with_a_time() {
        assert_eq!(
            release_counter_source(true, 1000),
            ir::ModulationSource::ReleaseCounter(ir::Time::Milliseconds(1000.))
        );
        assert_eq!(
            release_counter_source(true, 0),
            ir::ModulationSource::Constant
        );
        assert_eq!(
            release_counter_source(false, 1000),
            ir::ModulationSource::Constant
        );
    }

    /// An AHDSR stage time addressed as a modulation target binds to the
    /// group's own amplitude envelope. Another envelope in the group is a
    /// modulator source, so the route stays reported.
    #[test]
    fn an_envelope_stage_route_binds_to_the_groups_amplitude_envelope() {
        let mut out = translation();
        // The group's amplitude envelope: an internal AHDSR at slot 0.
        out.ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(ir::Envelope::default()),
        });
        let envelope = ir::ModulatorRef(out.ir.modulators.len() - 1);
        out.ir.source_indices.modulators.push(ir::SourceModulator {
            group: 7,
            slot: 0,
            external: false,
            name: String::new(),
            runtime: Some(envelope),
            settings: None,
        });
        // An external source in the same group.
        out.ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::ReleaseCounter(ir::Time::Milliseconds(1000.)),
        });
        let external = ir::ModulatorRef(out.ir.modulators.len() - 1);
        out.ir.source_indices.modulators.push(ir::SourceModulator {
            group: 7,
            slot: 5,
            external: true,
            name: String::new(),
            runtime: Some(external),
            settings: None,
        });
        out.amplitude_envelopes = vec![None; 8];
        out.amplitude_envelopes[7] = Some(envelope);
        let attack = ModTarget {
            slot: Some(0),
            ..target("ahdsr_attack", 1.0)
        };
        let route = out.route("g", external, true, &attack, None).unwrap();
        assert_eq!(
            out.ir.routes[route.0].target,
            ir::Target::EnvelopeStage {
                source: envelope,
                stage: ir::EnvelopeStage::Attack
            }
        );
        assert_eq!(out.ir.routes[route.0].depth, ir::Depth::Normalized(1.0));
        assert!(out.ir.unsupported.is_empty());
        // Another envelope is not the voice's envelope: the route stays reported.
        out.amplitude_envelopes[7] = None;
        assert!(out.route("g", external, true, &attack, None).is_none());
        assert_eq!(
            out.ir.unsupported.last().unwrap().feature,
            "modulation of a module parameter"
        );
    }

    /// A saved-bypassed LFO with module targets keeps a live depth lane (unit =
    /// the route's own depth per intensity); a route with no known unit is not bound.
    #[test]
    fn internal_module_routes_bind_live_depth_in_route_units() {
        let mut out = translation();
        out.ir.routes.push(ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::Processor {
                chain: ir::ChainRef(0),
                index: 0,
                parameter: ir::ProcessorParameter::Resonance,
            },
            depth: ir::Depth::Normalized(0.),
            invert: false,
            shape: None,
            smoothing: ir::Time::ZERO,
            scale: None,
        });
        let mut bypass = None;
        let t = target("filterQ", 0.);
        assert!(!out.internal_route_controls([0, 3, 1], ir::RouteRef(0), &t, true, &mut bypass));
        out.route_intensity.insert(0, (0., 1.));
        assert!(out.internal_route_controls([0, 3, 1], ir::RouteRef(0), &t, true, &mut bypass));
        assert_eq!(out.ir.source_indices.route_controls.len(), 1);
    }

    /// Sparse authored source slots: slot 2 modulates slot 9's rate and pan.
    /// No native binary, library sample, or script payload is used.
    fn deferred_rate_group(bypass: bool, rate_first: bool) -> Group {
        let rate = ModTarget { slot: Some(9), ..target("frequency", 0.25) };
        let pan = target("pan", 0.);
        let targets = if rate_first { vec![rate, pan] } else { vec![pan, rate] };
        deferred_source_group(bypass, targets, false, true)
    }

    fn deferred_source_group(bypass: bool, targets: Vec<ModTarget>, synchronized_receiver: bool, receiver_retrigger: bool) -> Group {
        let source = |rate: f32, phase: f32| Lfo {
            structured: false, version: 0x72, waveform: 0,
            initial_values: [0., rate, 0.5, phase],
            records: std::array::from_fn(|_| LfoRecord { flag: false, values: [-1., 0., 0.] }),
            trailing_flag: false, trailing_values: None, additional_flag: None,
        };
        let internal = |name: &str, bypass: bool, retrigger: bool, lfo: Lfo, targets: Vec<ModTarget>| {
            let mut private = (targets.len() as u32).to_le_bytes().to_vec();
            for target in &targets {
                descriptor_name(&mut private, &target.param);
                private.extend(target.intensity.to_le_bytes());
                private.extend(target.unknown_i16.to_le_bytes());
                private.push(target.unknown_flags);
                private.extend(target.lag_ms.to_le_bytes());
                descriptor_name(&mut private, &target.name);
                if let Some(slot) = target.slot { private.push(slot); }
                private.push(u8::from(target.invert));
            }
            private.extend(std::iter::repeat_n(0, targets.len())); // No shapers.
            private.extend([0, u8::from(bypass), u8::from(retrigger), 0]);
            private.extend(0u32.to_le_bytes());
            descriptor_name(&mut private, name);
            private.extend(1u32.to_le_bytes());
            descriptor_object(0x0d, 0x81, &private, &[], &[lfo.to_chunk().unwrap()])
        };
        let mut receiver = source(2., 0.);
        if synchronized_receiver { receiver.records[0].values[0] = 1.; }
        let mut public = 0u32.to_le_bytes().to_vec();
        for value in [1f32, 0., 1.] { public.extend(value.to_le_bytes()); }
        public.extend([1, 0, 0, 0]);
        public.extend(0i32.to_le_bytes());
        public.extend((-1i16).to_le_bytes());
        public.extend([0; 10]);
        public.extend(0i32.to_le_bytes());
        Group(ni_file::kontakt::StructuredObject {
            version: 0x95, private_data: vec![], public_data: public,
            children: vec![
                ni_file::kontakt::Chunk { id: 0x38, data: 0u32.to_le_bytes().to_vec() },
                descriptor_slots(INTERNAL_MODS, 16, &[
                    (2, internal("rate-driver", bypass, true, source(3., 0.25), targets)),
                    (9, internal("rate-receiver", false, receiver_retrigger, receiver, vec![target("volume", 1.)])),
                ]),
            ],
        })
    }

    fn deferred_rate_translation(bypass: bool, rate_first: bool) -> (Translation, GroupInfo) {
        let mut out = translation();
        let group = deferred_rate_group(bypass, rate_first);
        out.source_modulators(7, &group).unwrap();
        let info = out.group(7, &group).unwrap().unwrap();
        out.ir.source_indices.groups = vec![None; 8];
        out.ir.source_indices.groups[7] = Some(info.group);
        (out, info)
    }

    #[test]
    fn deferred_rate_routes_keep_internal_depth_and_one_shared_bypass() {
        for saved_bypass in [false, true] {
            for rate_first in [false, true] {
                let (out, _) = deferred_rate_translation(saved_bypass, rate_first);
                let owner = out.ir.source_indices.modulators.iter()
                    .find(|m| m.group == 7 && m.slot == 2 && !m.external).unwrap();
                if saved_bypass {
                    assert!(owner.runtime.is_none(), "never wake a rate source before its bypassed base-rate law is proved");
                    assert!(!out.ir.source_indices.control_aliases.iter().any(|a| a.address.group == 7 && a.address.slot == 2));
                    assert!(out.ir.unsupported.iter().any(|u| u.feature == "LFO rate live controls"
                        && u.reason == ir::Reason::UnknownLaw && u.value.contains("saved-bypassed")));
                    continue;
                }
                let source = owner.runtime.expect("a supported active internal rate source has a live owner");
                let receiver = out.ir.source_indices.modulators.iter()
                    .find(|m| m.group == 7 && m.slot == 9 && !m.external).unwrap().runtime.unwrap();
                let routes: Vec<_> = out.ir.routes.iter().enumerate().filter(|(_, r)| r.source == source).collect();
                assert_eq!(routes.len(), 2);
                let rate = routes.iter().find(|(_, r)| r.target == ir::Target::SourceRate(receiver)).unwrap().0;
                let controls: Vec<_> = out.ir.source_indices.route_controls.iter()
                    .filter(|(r, _, _)| routes.iter().any(|(index, _)| *index == r.0)).collect();
                assert_eq!(controls.len(), 2, "ordinary and deferred targets need live controls");
                assert_eq!(controls[0].2, controls[1].2, "one source owns one bypass");
                assert!(matches!(out.ir.controls[controls[0].2.0].value,
                    ir::ControlValue::Continuous { default, .. } if default == f64::from(saved_bypass)));
                let depth = controls.iter().find(|(r, _, _)| r.0 == rate).unwrap().1;
                let ordinal = if rate_first { 0 } else { 1 };
                for name in ["ENGINE_PAR_MOD_TARGET_INTENSITY", "ENGINE_PAR_INTMOD_INTENSITY", "ENGINE_PAR_MOD_TARGET_MP_INTENSITY"] {
                    assert!(out.ir.source_indices.control_aliases.iter().any(|a|
                        a.control == depth && a.parameter == name
                            && a.address == ir::SlotAddress { group: 7, slot: 2, generic: ordinal }));
                }
                if rate_first {
                    assert!(out.ir.source_indices.control_aliases.iter().any(|a|
                        a.control == depth && a.parameter == "ENGINE_PAR_INTMOD_INTENSITY"
                            && a.address == ir::SlotAddress { group: 7, slot: 2, generic: -1 }));
                }
            }
        }
    }

    #[test]
    fn deferred_rate_refusals_are_reported_for_active_and_saved_bypassed_sources() {
        for bypass in [false, true] {
            for mixed in [false, true] {
                for (slot, synchronized, lag) in [(8, false, 0), (9, true, 0), (9, false, -1)] {
                    let mut targets = vec![ModTarget { slot: Some(slot), lag_ms: lag, ..target("frequency", 0.25) }];
                    if mixed { targets.push(target("pan", 0.)); }
                    let group = deferred_source_group(bypass, targets, synchronized, true);
                    let mut out = translation();
                    out.source_modulators(7, &group).unwrap();
                    out.group(7, &group).unwrap().unwrap();
                    let source = out.ir.source_indices.modulators.iter()
                        .find(|m| m.group == 7 && m.slot == 2 && !m.external).unwrap().runtime;
                    assert_eq!(source.is_none(), bypass, "a saved-bypassed rate source must stay inactive");
                    assert!(!out.ir.routes.iter().any(|r| Some(r.source) == source && matches!(r.target, ir::Target::SourceRate(_))));
                    assert!(out.ir.unsupported.iter().any(|u| u.value.contains("frequency") && if bypass {
                        u.feature == "LFO rate live controls" && u.reason == ir::Reason::UnknownLaw
                    } else {
                        matches!(u.feature.as_str(), "modulation of a module parameter" | "negative modulation lag")
                    }), "bypass {bypass}, mixed {mixed}, slot {slot}, synchronized {synchronized}, lag {lag}: retain the refusal");
                    if bypass {
                        assert!(!out.ir.source_indices.control_aliases.iter().any(|a| a.address.group == 7 && a.address.slot == 2),
                            "never publish a partial sibling bypass");
                    }
                }
            }
        }
    }

    #[test]
    fn deferred_rate_does_not_widen_a_mixed_phase_sources_bypass_admission() {
        for bypass in [false, true] {
            let group = deferred_source_group(bypass, vec![
                ModTarget { slot: Some(9), ..target("frequency", 0.25) },
                ModTarget { slot: Some(9), ..target("startPhase", 0.25) },
            ], false, true);
            let mut out = translation();
            out.source_modulators(7, &group).unwrap();
            out.group(7, &group).unwrap().unwrap();
            let owner = out.ir.source_indices.modulators.iter()
                .find(|m| m.group == 7 && m.slot == 2 && !m.external).unwrap();
            assert_eq!(owner.runtime.is_none(), bypass);
            assert_eq!(owner.settings.as_ref().unwrap().targets.len(), 2);
            assert!(!out.ir.source_indices.control_aliases.iter().any(|a| a.address.group == 7 && a.address.slot == 2));
        }
    }

    #[test]
    fn deferred_rate_native_writes_read_back_and_match_direct_control_pcm() {
        use sampler_core::{ControlValue, ControlWrite, EngineParameterAddress, Input, Protocol, Runtime};
        let bypass = false;
        for rate_first in [false, true] {
            for partition in [1, 7, 64, 127] {
                let (mut out, info) = deferred_rate_translation(bypass, rate_first);
                let ordinal = if rate_first { 0 } else { 1 };
                let address = |name, generic| EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id(name).unwrap(), group: 7, slot: 2, generic,
                };
                let control = |name, generic| {
                    let alias = out.ir.source_indices.control_aliases.iter().find(|a|
                        a.parameter == name && a.address == ir::SlotAddress { group: 7, slot: 2, generic }).unwrap();
                    sampler_core::lower::ir_control_id(&out.ir.controls[alias.control.0].key)
                };
                let depth = control("ENGINE_PAR_MOD_TARGET_INTENSITY", ordinal);
                let bypass_id = control("ENGINE_PAR_INTMOD_BYPASS", -1);
                out.ir.assets.push(ir::Asset { location: ir::AssetLocation::Path("authored-rate".into()),
                    residency: Default::default(), encoding: ir::Encoding::Wav, root_key: None, loops: vec![] });
                let mut zone = ir::Zone::new(ir::AssetRef(0));
                zone.group = Some(info.group);
                zone.routes = info.routes;
                zone.pitch = ir::KeyTracking::Fixed;
                zone.velocity = ir::VelocityResponse::None;
                out.ir.zones.push(zone);
                let driver = out.ir.source_indices.modulators.iter()
                    .find(|m| m.group == 7 && m.slot == 2 && !m.external).unwrap().runtime.unwrap();
                let make = |bind_controls: bool| {
                    let mut instrument = out.ir.clone();
                    if !bind_controls {
                        // The previous importer kept this owner's routes static.
                        let routes = &instrument.routes;
                        let removed: Vec<_> = instrument.source_indices.route_controls.iter()
                            .filter(|(r, _, _)| routes[r.0].source == driver)
                            .flat_map(|(_, depth, bypass)| [*depth, *bypass]).collect();
                        instrument.source_indices.route_controls.retain(|(r, _, _)| routes[r.0].source != driver);
                        instrument.source_indices.control_aliases.retain(|alias| !removed.contains(&alias.control));
                    }
                    let pcm = sampler_core::Pcm::new(48000, vec![[0.5, 0.25]; 16384].into_boxed_slice()).unwrap();
                    let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
                    let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
                    Runtime::new(plan, limits).unwrap()
                };
                let (mut actual, mut reference, mut unchanged) = (make(true), make(true), make(false));
                let input = |id| Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
                let mut a = [[0.; 2]; 384];
                let mut b = [[0.; 2]; 384];
                let mut baseline = [[0.; 2]; 384];
                for id in [1, 2] {
                    actual.trigger(input(id), 60, 1.).unwrap();
                    reference.trigger(input(id), 60, 1.).unwrap();
                    unchanged.trigger(input(id), 60, 1.).unwrap();
                    if id == 1 {
                        assert_eq!(actual.engine_parameter(address("ENGINE_PAR_MOD_TARGET_INTENSITY", ordinal)).unwrap(), 250000);
                        assert_eq!(actual.engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", -1)).unwrap(), 0);
                        for ((a, b), baseline) in a.chunks_mut(partition).zip(b.chunks_mut(partition)).zip(baseline.chunks_mut(partition)) {
                            actual.render(a).unwrap();
                            reference.render(b).unwrap();
                            unchanged.render(baseline).unwrap();
                        }
                        for (a, baseline) in a.iter().zip(&baseline) {
                            assert_eq!(a.map(f32::to_bits), baseline.map(f32::to_bits), "binding must preserve the active static route before edits");
                        }
                    }
                    let mut audible_edit = false;
                    for (name, generic, encoded, decoded, target) in [
                        ("ENGINE_PAR_INTMOD_INTENSITY", ordinal, 750000, 0.125, depth),
                        ("ENGINE_PAR_INTMOD_BYPASS", -1, 0, 0., bypass_id),
                        ("ENGINE_PAR_MOD_TARGET_MP_INTENSITY", ordinal, 1000000, 1., depth),
                        ("ENGINE_PAR_INTMOD_BYPASS", -1, 1, 1., bypass_id),
                        ("ENGINE_PAR_INTMOD_BYPASS", -1, 0, 0., bypass_id),
                        ("ENGINE_PAR_MOD_TARGET_INTENSITY", ordinal, 0, 0., depth),
                    ] {
                        actual.set_engine_parameter(address(name, generic), encoded).unwrap();
                        assert_eq!(actual.engine_parameter(address(name, generic)).unwrap(), encoded);
                        reference.edit_controls(reference.active_plan(), None,
                            &[ControlWrite { id: target, value: ControlValue::Real(decoded) }]).unwrap();
                        for ((a, b), baseline) in a.chunks_mut(partition).zip(b.chunks_mut(partition)).zip(baseline.chunks_mut(partition)) {
                            actual.render(a).unwrap();
                            reference.render(b).unwrap();
                            unchanged.render(baseline).unwrap();
                        }
                        assert!(a.iter().flatten().all(|v| v.is_finite()));
                        assert!(a.iter().flatten().any(|&v| v != 0.));
                        for (a, b) in a.iter().zip(&b) { assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits)); }
                        audible_edit |= a.iter().zip(&baseline).any(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits));
                    }
                    assert!(audible_edit, "native rate writes must change PCM, not just readback");
                    for rt in [&mut actual, &mut reference, &mut unchanged] {
                        rt.note_off(input(id), None).unwrap();
                        rt.render(&mut [[0.; 2]; 64]).unwrap();
                        assert_eq!(rt.voice_count(), 0);
                    }
                }
            }
        }
    }

    #[test]
    fn deferred_rate_saved_bypass_preserves_late_receiver_clock() {
        for retrigger in [false, true] {
            let group = deferred_source_group(true,
                vec![ModTarget { slot: Some(9), ..target("frequency", 0.25) }], false, retrigger);
            let mut out = translation();
            out.source_modulators(7, &group).unwrap();
            let info = out.group(7, &group).unwrap().unwrap();
            out.ir.source_indices.groups = vec![None; 8];
            out.ir.source_indices.groups[7] = Some(info.group);
            let source = out.ir.source_indices.modulators.iter()
                .find(|m| m.group == 7 && m.slot == 2 && !m.external).unwrap().runtime;
            out.ir.assets.push(ir::Asset { location: ir::AssetLocation::Path("authored-late-rate".into()),
                residency: Default::default(), encoding: ir::Encoding::Wav, root_key: None, loops: vec![] });
            let mut zone = ir::Zone::new(ir::AssetRef(0));
            zone.group = Some(info.group);
            zone.routes = info.routes;
            zone.pitch = ir::KeyTracking::Fixed;
            zone.velocity = ir::VelocityResponse::None;
            out.ir.zones.push(zone);
            let make = |without_driver: bool| {
                let mut instrument = out.ir.clone();
                if without_driver {
                    instrument.zones[0].routes.retain(|r| Some(instrument.routes[r.0].source) != source);
                }
                let plan = sampler_core::lower::lower(&instrument, 48000,
                    vec![sampler_core::Pcm::new(48000, vec![[0.5; 2]; 8192].into_boxed_slice()).unwrap()], |_, p| Ok(p)).unwrap();
                let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
                let mut runtime = sampler_core::Runtime::new(plan, limits).unwrap();
                runtime.render(&mut [[0.; 2]; 2400]).unwrap();
                runtime.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Clap,
                    port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
                let mut pcm = [[0.; 2]; 4096];
                runtime.render(&mut pcm).unwrap();
                pcm
            };
            let (actual, unchanged) = (make(false), make(true));
            for (frame, (a, b)) in actual.iter().zip(&unchanged).enumerate() {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits), "retrigger {retrigger}, frame {frame}");
            }
        }
    }

    #[test]
    fn deferred_rate_free_receiver_keeps_active_static_route_but_refuses_live_controls() {
        for bypass in [false, true] {
            for mixed in [false, true] {
                let mut targets = vec![ModTarget { slot: Some(9), ..target("frequency", 0.25) }];
                if mixed { targets.push(target("pan", 0.)); }
                let group = deferred_source_group(bypass, targets, false, false);
                let mut out = translation();
                out.source_modulators(7, &group).unwrap();
                out.group(7, &group).unwrap().unwrap();
                let source = out.ir.source_indices.modulators.iter()
                    .find(|m| m.group == 7 && m.slot == 2 && !m.external).unwrap().runtime;
                let rate = out.ir.routes.iter().enumerate().find(|(_, r)| Some(r.source) == source && matches!(r.target, ir::Target::SourceRate(_)));
                assert_eq!(rate.is_some(), !bypass, "retain an active static route, never run the saved-bypassed route");
                if let Some((rate, _)) = rate {
                    assert!(!out.ir.source_indices.route_controls.iter().any(|(r, _, _)| r.0 == rate));
                }
                assert!(!out.ir.source_indices.control_aliases.iter().any(|a| a.address.group == 7 && a.address.slot == 2),
                    "never publish a partial source bypass through a sibling target");
                assert!(out.ir.unsupported.iter().any(|u| u.feature == "LFO rate live controls"
                    && u.reason == ir::Reason::UnknownLaw && u.value.contains("frequency")));
            }
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
            let pan = ModTarget {
                lag_ms: 15,
                invert: true,
                ..target("pan", 0.5)
            };
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
        let lfo = Lfo {
            structured: false,
            version: 0x73,
            waveform: 5,
            initial_values: [0., 1., 0.5, 0.],
            records: [
                LfoRecord {
                    flag: true,
                    values: [-1., 0., 1.],
                },
                LfoRecord {
                    flag: false,
                    values: [-1., 0., 1.],
                },
            ],
            trailing_flag: false,
            trailing_values: Some([0.; 5]),
            additional_flag: Some(true),
        };
        let source = t
            .lfo("g", &lfo, true)
            .expect("authored zero-wave Multi still has a bipolar output");
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
        let filter = ir::Processor::Filter(ir::Filter {
            kind: ir::FilterKind::LowPass { poles: 2 },
            cutoff: ir::Frequency::Hertz(1000.),
            resonance: ir::Resonance::Q(1.),
        });
        t.ir.chains = vec![
            ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![filter; 2],
                post_amplitude: vec![]
            };
            4
        ];
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
    fn legacy_cutoff_span_does_not_change_an_adjacent_sv_slot() {
        let mut t = translation();
        let filter = ir::Processor::Filter(ir::Filter {
            kind: ir::FilterKind::LowPass { poles: 2 },
            cutoff: ir::Frequency::Hertz(1000.),
            resonance: ir::Resonance::Q(0.7),
        });
        t.ir.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![filter],
            post_amplitude: vec![filter],
        });
        t.cutoff_octaves
            .insert((0, 1), 8.96);
        for (slot, expected) in [(0, -30.), (5, -26.88)] {
            let route = t
                .route(
                    "group",
                    ir::ModulatorRef(0),
                    true,
                    &ModTarget {
                        slot: Some(slot),
                        ..target("filterCutoff", -0.25)
                    },
                    Some((ir::ChainRef(0), &[(0, 0), (5, 1)])),
                )
                .unwrap();
            let ir::Depth::Pitch(depth) = t.ir.routes[route.0].depth else {
                panic!("pitch depth")
            };
            assert!(
                (depth.semitones() - expected).abs() < 1e-6,
                "a legacy post-amplitude slot must not change the neighboring SV law"
            );
        }
    }

    /// One native legacy EQ band (the only EQ owner Kontakt translation emits).
    fn legacy_peak(gain_db: f32) -> ir::Processor {
        ir::Processor::Eq(ir::fx::eq::Config::band(ir::fx::eq::Band {
            law: ir::fx::eq::Law::KontaktLegacy,
            frequency: 0.5,
            q: 0.25,
            gain_db,
            ..ir::fx::eq::Band::new(ir::fx::eq::Kind::Peak, 0., 0., 0.)
        }))
    }

    #[test]
    fn eq_frequency_and_width_routes_keep_the_physical_band() {
        let mut t = translation();
        let peak = legacy_peak(6.);
        t.ir.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![peak; 3],
            post_amplitude: vec![],
        });
        t.engine.push(sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_FREQ2".into(),
            group: 7,
            slot: 5,
            generic: -1,
            value: 700000,
        });
        t.engine.push(sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_BW3".into(),
            group: 7,
            slot: 5,
            generic: -1,
            value: 400000,
        });
        t.eq_controls(7, ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)]);
        for (control, default, name) in [(4, 0.7, "ENGINE_PAR_FREQ2"), (8, 0.4, "ENGINE_PAR_BW3")] {
            assert!(
                matches!(t.ir.controls[control].value, ir::ControlValue::Continuous { min: 0., max: 1., default: v, unit: ir::ControlUnit::Percent } if v == default)
            );
            assert_eq!(t.ir.source_indices.control_aliases[control].parameter, name);
        }
        for (name, index, parameter) in [
            ("eqFreq2", 1, ir::ProcessorParameter::Cutoff),
            ("eqBandwidth3", 2, ir::ProcessorParameter::Resonance),
        ] {
            let target = ModTarget {
                slot: Some(5),
                ..target(name, -0.25)
            };
            let route = t
                .route(
                    "g",
                    ir::ModulatorRef(0),
                    true,
                    &target,
                    Some((ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)])),
                )
                .expect("native EQ knob route reaches its physical band");
            assert_eq!(
                t.ir.routes[route.0].target,
                ir::Target::Processor {
                    chain: ir::ChainRef(0),
                    index,
                    parameter
                }
            );
            assert_eq!(t.ir.routes[route.0].depth, ir::Depth::Normalized(-0.25));
        }
    }

    #[test]
    fn eq_live_owner_clamps_saved_gain_like_v1_knobs() {
        let mut t = translation();
        t.ir.chains.push(ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![legacy_peak(40.)],
            post_amplitude: vec![],
        });
        t.eq_controls(7, ir::ChainRef(0), &[(5, 0)]);
        assert!(
            matches!(
                t.ir.controls[0].value,
                ir::ControlValue::Continuous { default: 18., .. }
            ),
            "v1 clamps normalized EQ knobs before conversion; a saved outlier must not invalidate the instrument"
        );
    }

    #[test]
    fn native_one_band_eq_saved_and_live_controls_reach_production_audio() {
        let mut public = 22_i32.to_le_bytes().repeat(2);
        for value in [1000_f32, 1., 9.] { public.extend(value.to_le_bytes()); }
        let slot = crate::effects::Slot { slot: 5, module: 0x18, version: 0x92,
            bypass: false, output_gain: 1., dry_level: 0., output_set: false, public };
        let built = crate::effects::chain_with(std::slice::from_ref(&slot), crate::effects::Scope::Voice,
            None, Some((7, -1)), (7, -1), &[]);
        assert_eq!(built.filter_slots, [(5, 1)]);
        assert!(matches!(built.processors[1], ir::Processor::Eq(_)));
        let mut t = translation();
        t.source_eq_values(7, &[slot]);
        t.ir.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: built.processors, post_amplitude: vec![] });
        t.eq_controls(7, ir::ChainRef(0), &built.filter_slots);
        assert_eq!(t.ir.processor_controls.len(), 3);
        assert!(matches!(t.ir.controls[1].value, ir::ControlValue::Continuous { default, .. }
            if default == f64::from(sampler_core::KontaktLegacyEqSettings::frequency_knob(1000.))));
        t.ir.assets.push(ir::Asset { location: ir::AssetLocation::Path("authored".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav, root_key: None, loops: vec![] });
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.chain = Some(ir::ChainRef(0));
        zone.pitch = ir::KeyTracking::Fixed;
        zone.velocity = ir::VelocityResponse::None;
        t.ir.zones.push(zone);
        let pcm = sampler_core::Pcm::new(48000, (0..16384).map(|i| {
            let x = (std::f64::consts::TAU * 1000. * i as f64 / 48000.).sin() as f32 * 0.1;
            [x, -x]
        }).collect::<Vec<_>>().into_boxed_slice()).unwrap();
        let plan = sampler_core::lower::lower(&t.ir, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
        let address = |name| sampler_core::EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id(name).unwrap(), group: 7, slot: 5, generic: -1 };
        assert!(rt.set_engine_parameter(sampler_core::EngineParameterAddress { slot: 4, ..address("ENGINE_PAR_GAIN1") }, 0).is_err());
        for name in ["ENGINE_PAR_GAIN1", "ENGINE_PAR_FREQ1", "ENGINE_PAR_BW1"] {
            rt.set_engine_parameter(address(name), 500000).unwrap();
        }
        rt.set_engine_parameter(address("ENGINE_PAR_GAIN1"), 750000).unwrap();
        rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native,
            port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.).unwrap();
        let mut before = [[0.; 2]; 1024];
        rt.render(&mut before).unwrap();
        rt.set_engine_parameter(address("ENGINE_PAR_GAIN1"), 0).unwrap();
        let mut settle = [[0.; 2]; 8192]; rt.render(&mut settle).unwrap();
        let mut after = [[0.; 2]; 1024]; rt.render(&mut after).unwrap();
        let energy = |frames: &[[f32; 2]]| frames.iter().map(|f| f64::from(f[0]).powi(2)).sum::<f64>();
        assert!(energy(&before) > 0.01);
        assert!(energy(&after) < energy(&before) * 0.5,
            "physical native gain edit must change held-note sound");
        assert!(after.iter().all(|f| f[0].is_finite() && (f[0] + f[1]).abs() < 1e-6));
    }

    #[test]
    fn eq_saved_gain_metadata_precedes_script_init() {
        let mut public = Vec::new();
        for kind in [24i32, 24] {
            public.extend(kind.to_le_bytes());
        }
        for values in [[300f32, 1., 0.], [1000., 1., 9.], [6000., 1., -18.]] {
            for value in values {
                public.extend(value.to_le_bytes());
            }
        }
        let slot = crate::effects::Slot {
            slot: 5,
            module: 0x18,
            version: 0x92,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public,
        };
        let mut t = translation();
        t.source_eq_values(7, &[slot]);
        let values = &t.ir.source_indices.engine_values;
        assert_eq!(values.len(), 9);
        for (band, expected) in [500000, 750000, 0].into_iter().enumerate() {
            assert_eq!(values[band * 3].value, expected);
            assert_eq!(
                (
                    values[band * 3].group,
                    values[band * 3].slot,
                    values[band * 3].generic
                ),
                (7, 5, -1)
            );
            assert_eq!(
                sampler_core::engine_parameter_name(values[band * 3].parameter),
                Some(
                    [
                        "$ENGINE_PAR_GAIN1",
                        "$ENGINE_PAR_GAIN2",
                        "$ENGINE_PAR_GAIN3"
                    ][band]
                )
            );
        }
    }

    #[test]
    fn eq_gain_routes_keep_band_and_physical_slot_identity() {
        for (name, index) in [("eqGain1", 0), ("eqGain2", 1), ("eqGain3", 2)] {
            let mut t = translation();
            let peak = legacy_peak(0.);
            t.ir.chains.push(ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![peak; 3],
                post_amplitude: vec![],
            });
            t.engine.push(sampler_ksp::EnginePar {
                parameter: "$ENGINE_PAR_GAIN2".into(),
                group: 7,
                slot: 5,
                generic: -1,
                value: 750000,
            });
            t.eq_controls(7, ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)]);
            assert_eq!(t.ir.processor_controls.len(), 9);
            assert_eq!(
                t.ir.source_indices.control_aliases[3].parameter,
                "ENGINE_PAR_GAIN2"
            );
            assert!(matches!(
                t.ir.controls[3].value,
                ir::ControlValue::Continuous {
                    default: 9.,
                    unit: ir::ControlUnit::Decibels,
                    ..
                }
            ));
            let target = ModTarget {
                slot: Some(5),
                ..target(name, -0.25)
            };
            t.route(
                "g",
                ir::ModulatorRef(0),
                true,
                &target,
                Some((ir::ChainRef(0), &[(5, 0), (5, 1), (5, 2)])),
            )
            .expect("authored EQ gain must reach its physical band");
            assert_eq!(
                t.ir.routes[0].target,
                ir::Target::Processor {
                    chain: ir::ChainRef(0),
                    index,
                    parameter: ir::ProcessorParameter::Gain
                }
            );
            assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(-0.25));
        }
    }

    #[test]
    fn native_filter_q_and_gain_routes_add_normalized_depth() {
        for processor in [
            ir::Processor::LadderLP4(ir::LadderLP4 {
                address: None,
                gain: -0.25,
                cutoff: 0.5,
                resonance: 0.,
                cutoff_path: true,
                taps: 4,
                high_quality: false,
            }),
            ir::Processor::Daft(ir::Daft {
                address: None,
                gain: 0.,
                cutoff: 0.5,
                resonance: 0.,
                highpass: false,
            }),
        ] {
            for (name, parameter) in [
                ("filterQ", ir::ProcessorParameter::Resonance),
                ("Gain", ir::ProcessorParameter::Gain),
            ] {
                let mut t = translation();
                t.ir.chains.push(ir::Chain {
                    scope: ir::Scope::Voice,
                    pre_amplitude: vec![processor],
                    post_amplitude: vec![],
                });
                let target = ModTarget {
                    slot: Some(5),
                    ..target(name, -0.25)
                };
                t.route(
                    "g",
                    ir::ModulatorRef(0),
                    true,
                    &target,
                    Some((ir::ChainRef(0), &[(5, 0)])),
                )
                .expect("v1 executes native normalized Q/Gain routes");
                assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(-0.25));
                assert_eq!(
                    t.ir.routes[0].target,
                    ir::Target::Processor {
                        chain: ir::ChainRef(0),
                        index: 0,
                        parameter
                    }
                );
            }
        }
    }

    #[test]
    fn native_filter_cutoff_modulation_adds_normalized_depth() {
        for processor in [
            ir::Processor::LadderLP4(ir::LadderLP4 {
                address: None,
                gain: 0.,
                cutoff: 0.5,
                resonance: 0.,
                cutoff_path: true,
                taps: 4,
                high_quality: false,
            }),
            ir::Processor::Daft(ir::Daft {
                address: None,
                gain: 0.,
                cutoff: 0.5,
                resonance: 0.,
                highpass: false,
            }),
        ] {
            let mut t = translation();
            t.ir.chains.push(ir::Chain {
                scope: ir::Scope::Voice,
                pre_amplitude: vec![processor],
                post_amplitude: vec![],
            });
            let target = ModTarget {
                slot: Some(5),
                ..target("filterCutoff", -0.25)
            };
            t.route(
                "g",
                ir::ModulatorRef(0),
                true,
                &target,
                Some((ir::ChainRef(0), &[(5, 0)])),
            )
            .unwrap();
            assert_eq!(t.ir.routes[0].depth, ir::Depth::Normalized(-0.25));
            assert_eq!(
                t.ir.routes[0].target,
                ir::Target::Processor {
                    chain: ir::ChainRef(0),
                    index: 0,
                    parameter: ir::ProcessorParameter::Cutoff
                }
            );
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
        t.route("g", source, true, &target("pan", 1.0), None)
            .unwrap();
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
        assert_eq!(reasons, [ir::Reason::NotModeled, ir::Reason::NotModeled]);
        assert!(t.ir.unsupported[1].value.contains("cutoff () destination slot 0"));
    }

    #[test]
    fn saved_depth_sign_is_independent_of_invert_and_source_polarity() {
        for unipolar in [false, true] {
            for invert in [false, true] {
                let mut t = translation();
                let signed = ModTarget {
                    unknown_flags: 0x12,
                    invert,
                    ..target("pitch", 0.25)
                };
                t.route("g", ir::ModulatorRef(0), unipolar, &signed, None)
                    .expect("signed pitch route");
                assert_eq!(
                    t.ir.routes[0].depth,
                    ir::Depth::Pitch(ir::Pitch::Semitones(-3.0))
                );
                assert_eq!(t.ir.routes[0].invert, invert);
            }
        }
    }

    #[test]
    fn digital_multi_components_take_the_saved_digital_fade_clock() {
        let mut t = translation();
        let mut raw = Lfo {
            structured: false,
            version: 0x73,
            waveform: 6,
            initial_values: [100., 4., 0.5, 0.25],
            records: [
                LfoRecord { flag: false, values: [-1., 0., 0.] },
                LfoRecord { flag: false, values: [-1., 0., 0.] },
            ],
            trailing_flag: false,
            trailing_values: Some([0.5, 0.25, 0.125, 0., 0.]),
            additional_flag: Some(false),
        };
        let faded = t.lfo("g", &raw, true).unwrap().shape;
        assert!(
            matches!(faded, ir::LfoShape::NativeFaded { wave: ir::NativeWave::Multi { digital: true, .. }, fade_ms } if fade_ms == 100.),
            "{faded:?}"
        );
        assert!(t.lfo("g", &raw, false).is_none(), "free-running fade stays unverified");
        raw.additional_flag = Some(true);
        assert!(t.lfo("g", &raw, true).is_none(), "alternate fade stays unverified");
    }

    #[test]
    fn digital_sine_keeps_saved_level_phase_sync_and_checked_fade_diagnostic() {
        let mut t = translation();
        let mut raw = Lfo {
            structured: false,
            version: 0x73,
            waveform: 6,
            initial_values: [0., 4., 0.5, 0.125],
            records: [
                LfoRecord {
                    flag: true,
                    values: [-1., 0., 0.],
                },
                LfoRecord {
                    flag: false,
                    values: [-1., 0., 0.],
                },
            ],
            trailing_flag: false,
            trailing_values: Some([0.3, 0., 0., 0., 0.]),
            additional_flag: Some(false),
        };
        for normalize in [false, true] {
            raw.records[0].flag = normalize;
            let source = t.lfo("g", &raw, true).unwrap();
            assert_eq!(source.shape, ir::LfoShape::SineScaled(-f64::from(0.3f32)));
            assert_eq!(source.phase, 0.125);
            assert_eq!(source.rate, ir::Frequency::Hertz(4.));
            assert_eq!(source.fade_in, ir::Time::Milliseconds(0.));
        }
        raw.records[0].values[0] = 0.25;
        assert_eq!(
            t.lfo("g", &raw, true).unwrap().rate,
            ir::Frequency::Beats(1.)
        );
        raw.initial_values[0] = 10.;
        assert!(
            t.lfo("g", &raw, true).is_none(),
            "frequency-synced fade remains unverified"
        );
        raw.records[0].values[0] = -1.;
        let faded = t.lfo("g", &raw, true).unwrap();
        assert_eq!(
            faded.shape,
            ir::LfoShape::DigitalSine {
                level: -f64::from(0.3f32),
                fade_ms: 10.
            }
        );
        assert_eq!(faded.fade_in, ir::Time::ZERO);
        raw.records[1].flag = true;
        assert!(t.lfo("g", &raw, true).is_none(), "synced fade remains unverified");
        raw.records[1].flag = false;
        raw.initial_values[3] = 2.;
        assert!(t.lfo("g", &raw, true).is_none(), "unbounded phase remains unverified");
        raw.initial_values[3] = 0.25;
        raw.additional_flag = Some(true);
        assert!(t.lfo("g", &raw, true).is_none(), "alternate fade remains unverified");
        raw.additional_flag = Some(false);
        assert!(
            t.lfo("g", &raw, false).is_none(),
            "free-running fade remains unverified"
        );
        raw.trailing_flag = true;
        assert!(
            t.lfo("g", &raw, true).is_none(),
            "delay-sync flag remains unverified"
        );
        raw.trailing_flag = false;
        raw.initial_values[0] = -1.;
        assert!(t.lfo("g", &raw, true).is_none());
        raw.initial_values[0] = f32::NAN;
        assert!(t.lfo("g", &raw, true).is_none());
        raw.initial_values[0] = 5001.;
        assert!(t.lfo("g", &raw, true).is_none());
        raw.initial_values[0] = 0.;
        raw.trailing_values = Some([f32::NAN, 0., 0., 0., 0.]);
        assert!(t.lfo("g", &raw, true).is_none());
        // With the fade off, any weights are the native Digital Multi mixer.
        raw.trailing_values = Some([0.3, 0.2, 0., 0., 0.]);
        assert!(matches!(
            t.lfo("g", &raw, true).unwrap().shape,
            ir::LfoShape::Native(ir::NativeWave::Multi { digital: true, .. })
        ));
        raw.trailing_values = Some([0.3, 0., 0., 0., 0.]);
        raw.additional_flag = Some(true);
        assert!(t.lfo("g", &raw, true).is_some(), "the fade profile byte is moot without a fade");
        assert_eq!(t.ir.unsupported.len(), 10);
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
        assert_eq!(sine.shape, ir::LfoShape::Native(ir::NativeWave::Sine));
        assert_eq!(sine.rate, ir::Frequency::Hertz(4.0));
        assert_eq!(sine.fade_in, ir::Time::Milliseconds(10.0));
        assert_eq!(sine.phase, 0.25);
        // Native legacy Multi's softened triangle is not the generic triangle.
        assert!(
            t.lfo("g", &lfo(5, Some([0.0, 0.0, 0.7, 0.0, 0.0])), false)
                .is_none()
        );
        let mut synced = lfo(1, None);
        synced.records[0].values[0] = 0.25;
        assert_eq!(
            t.lfo("g", &synced, true).unwrap().rate,
            ir::Frequency::Beats(1.0)
        );
        // Native waveform ids (modulators.md BLfo* table): saw 3, held noise 4,
        // rectangle with any pulse width; the held grid counts from phase < 0.5.
        assert_eq!(t.lfo("g", &lfo(3, None), true).unwrap().shape, ir::LfoShape::Native(ir::NativeWave::Saw));
        assert_eq!(t.lfo("g", &lfo(2, None), true).unwrap().shape, ir::LfoShape::Native(ir::NativeWave::Triangle));
        let mut narrow = lfo(1, None);
        narrow.initial_values[2] = 0.25;
        assert_eq!(t.lfo("g", &narrow, true).unwrap().shape, ir::LfoShape::Native(ir::NativeWave::Rectangle(0.25)));
        assert_eq!(t.lfo("g", &lfo(4, None), true).unwrap().shape, ir::LfoShape::Native(ir::NativeWave::Held));
        let mut late = lfo(4, None);
        late.initial_values[3] = 0.5;
        assert!(t.lfo("g", &late, true).is_none());
        assert!(
            t.lfo("g", &lfo(5, Some([0.5, 0.0, 0.5, 0.0, 0.0])), true)
                .is_none()
        );
        assert_eq!(t.ir.unsupported.len(), 3);
        // Native Multi: stored (sine, pulse, triangle, doubled saw, noise) land in
        // runtime order (sine, saw, triangle, pulse, noise); wave 6 is the digital mixer.
        let mut multi = lfo(5, Some([1.0, 2.0, 3.0, 4.0, 5.0]));
        multi.initial_values[0] = 0.0;
        let shape = |digital| {
            ir::LfoShape::Native(ir::NativeWave::Multi {
                weights: [1.0, 4.0, 3.0, 2.0, 5.0],
                width: 0.5,
                normalize: true,
                digital,
            })
        };
        assert_eq!(t.lfo("g", &multi, true).unwrap().shape, shape(false));
        multi.waveform = 6;
        assert_eq!(t.lfo("g", &multi, true).unwrap().shape, shape(true));
        // Outside the proved domain: a fade, a pulse width beyond the builder's
        // clamp, and noise with a start phase at or past one half.
        let mut faded = multi.clone();
        faded.initial_values[0] = 5.0;
        let mut wide = multi.clone();
        wide.initial_values[2] = 0.99;
        let mut late = multi.clone();
        late.initial_values[3] = 0.5;
        for bad in [faded, wide, late] {
            assert!(t.lfo("g", &bad, true).is_none());
        }
        assert_eq!(t.ir.unsupported.len(), 6);
    }

    #[test]
    fn flex_uses_the_native_curve_floor_and_start_level() {
        use ni_file::kontakt::objects::{EnvelopeFlex, FlexPoint};
        let point = |time_ms, level, curve| FlexPoint { time_ms, level, curve };
        let flex = |points: Vec<FlexPoint>, tail: Vec<u8>| EnvelopeFlex {
            points,
            sustain: 1,
            unknown_index: 0,
            unknown_tail: tail,
        };
        // Quick attack ending in a zero level: no leading points, 1 ms floor,
        // curve 0.5 is linear and 0.25 has ratio 49.
        let b = flex_breakpoints(&flex(
            vec![point(0.0, 0.0, 0.5), point(20.0, 1.0, 0.25), point(30.0, 0.0, 0.75)],
            vec![0; 13],
        ));
        assert_eq!(b.sustain, Some(1));
        assert!(b.flex_release);
        assert_eq!(b.points[0].time, ir::Time::Milliseconds(1.0));
        assert_eq!(b.points[0].shape, ir::Curve::Linear);
        assert_eq!(b.points[1].shape, ir::Curve::Exponential(49f64.ln()));
        let ir::Curve::Exponential(fast) = b.points[2].shape else { panic!() };
        assert!(fast < 0.0);
        // A first row longer than 0.1 s holds the start level 5 ms; a nonzero last
        // level adds the start point; the sustain index follows.
        let b = flex_breakpoints(&flex(
            vec![point(200.0, 1.0, 0.5), point(10.0, 0.5, 0.5), point(10.0, 0.25, 0.5)],
            vec![0; 13],
        ));
        assert_eq!(b.points[0].time, ir::Time::Milliseconds(0.0));
        assert_eq!(b.points[0].level, 0.25);
        assert_eq!(b.points[1].time, ir::Time::Milliseconds(5.0));
        assert_eq!(b.points[1].level, 0.25);
        assert_eq!(b.points[2].time, ir::Time::Milliseconds(200.0));
        assert_eq!(b.sustain, Some(3));
        // The v0x12 bypass byte (+0x195) turns the release scaling off.
        let mut tail = vec![0, 1];
        tail.extend([0; 13]);
        let b = flex_breakpoints(&flex(vec![point(1.0, 1.0, 0.5), point(1.0, 0.0, 0.5)], tail));
        assert!(!b.flex_release);
        // The v0x12 loop-jump byte (+0x194) returns the envelope to the point
        // before the sustain row instead of holding there.
        let mut tail = vec![1, 0];
        tail.extend([0; 13]);
        let b = flex_breakpoints(&flex(
            vec![point(1.0, 0.0, 0.5), point(1.0, 1.0, 0.5), point(1.0, 0.0, 0.5)],
            tail,
        ));
        assert_eq!(b.sustain, Some(1));
        assert_eq!(b.loop_range, Some((0, 1)));
        assert!(b.flex_release);
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

/// Kontakt DBD (`BParEnv_DBD`, 0x41) v0x12 as the universal breakpoint envelope.
/// Stored fields (reader `0x140cffd40`, resolver `0x1409651b0` mode 2): `values[0]`
/// and `values[2]` are unit knobs for the two durations (decay law,
/// `fconv_u2n_env_d`), `values[1]` is the level, read raw by the static
/// preparation (`0x140ae5d70`); the flag byte zeroes level and second duration.
/// The live re-preparation (`0x140ae4b50`) uses the same decay law. The start
/// is 1/3 (`0x144760108`) with linear ramps (`0x140ae43d0`). Frames are
/// `round(ms · rate / 1000)` in the lowering, the native `trunc(ms·0.001·R + 0.5)`.
/// Refused: other versions (legacy tails unproven) and a nonzero first duration
/// followed by a zero second one (unproven transition, the engine refuses it too).
fn dbd_breakpoints(dbd: &ni_file::kontakt::objects::EnvelopeDbd) -> Option<ir::Breakpoints> {
    if dbd.version != 0x12 || !dbd.values.iter().all(|v| v.is_finite()) {
        return None;
    }
    let [first, level, second] = dbd.values;
    let point = |ms: f64, level: f64| ir::Breakpoint {
        time: ir::Time::Milliseconds(ms),
        level,
        shape: ir::Curve::Linear,
    };
    if dbd.flag.unwrap_or(0) != 0 {
        return Some(ir::Breakpoints {
            points: vec![point(0.0, 0.0)],
            ..Default::default()
        });
    }
    let decay = |knob: f32| f64::from(sampler_core::native_mod_laws::u2n_env_d(knob));
    let (first, second) = (decay(first), decay(second));
    if first > 0.0 && second == 0.0 {
        return None;
    }
    Some(ir::Breakpoints {
        points: vec![
            // 1/3 is the native f32 start (0x144760108), reached at once.
            point(0.0, f64::from(1.0f32 / 3.0)),
            point(first, f64::from(level)),
            point(second, 0.0),
        ],
        ..Default::default()
    })
}

/// Kontakt Flex (`BEnvelopeFM7`) as the universal breakpoint envelope.
/// Point times are deltas in milliseconds with the setter's 1 ms floor
/// (0x140ae5a10); levels are linear gain; the curve is the setter's ratio in
/// closed form ([`sampler_core::blocks::envelope::flex_curve`] uses the same
/// law). Setup (0x14094ce70) starts from the LAST point's level, and when the
/// first duration exceeds 0.1 s holds that level 5 ms before the first row; both
/// become leading points here. The release scales the first release segment
/// (see `Breakpoints::flex_release`) unless the stored bypass byte is set.
fn flex_breakpoints(flex: &ni_file::kontakt::objects::EnvelopeFlex) -> ir::Breakpoints {
    type Point = ni_file::kontakt::objects::FlexPoint;
    let level = |p: &Point| f64::from(p.level.clamp(0.0, 1.0));
    let last = flex.points.last().map_or(0.0, level);
    let step = |ms: f64, level: f64| ir::Breakpoint {
        time: ir::Time::Milliseconds(ms),
        level,
        shape: ir::Curve::Linear,
    };
    let mut points = Vec::with_capacity(flex.points.len() + 2);
    if last != 0.0 {
        points.push(step(0.0, last));
    }
    if flex.points.first().is_some_and(|p| p.time_ms.max(1.0) * 0.001f32 > 0.1f32) {
        points.push(step(5.0, last));
    }
    let lead = points.len();
    points.extend(flex.points.iter().map(|p| ir::Breakpoint {
        time: ir::Time::Milliseconds(f64::from(p.time_ms.max(1.0))),
        level: level(p),
        shape: match f64::from(sampler_core::blocks::envelope::flex_ratio(p.curve)).ln() {
            k if k == 0.0 => ir::Curve::Linear,
            k => ir::Curve::Exponential(k),
        },
    }));
    ir::Breakpoints {
        points,
        sustain: Some(flex.sustain as usize + lead),
        // The saved loop-jump byte (+0x194) makes the envelope jump back to
        // the point before the sustain row instead of holding there
        // (native advance 0x140ae57dd).
        loop_range: flex
            .unknown_version_word()
            .is_some_and(|word| word & 0xff != 0)
            .then_some((flex.unknown_index as usize + lead, flex.sustain as usize + lead))
            .filter(|&(start, end)| start <= end),
        flex_release: !flex.unknown_version_word().is_some_and(|word| word >> 8 != 0),
    }
}

/// Whether a saved Flex record asks for the loop jump at the sustain row
/// (the +0x194 byte of the v0x12 tail).
fn word_loop_jump(flex: &ni_file::kontakt::objects::EnvelopeFlex) -> bool {
    flex.unknown_version_word().is_some_and(|word| word & 0xff != 0)
}

/// Exponential bow constant for a shaper or flex-envelope segment:
/// `k = BOW_K * |curvature|`.
/// Measured, not in the manual (the shaper only now; Flex uses the recovered
/// native law): fitted to Kontakt 8 renders of Vista 3 Cellos
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

/// A modulation assignment whose destination is another modulator's intensity.
struct IntensityScale {
    at: String,
    /// Module slot of the destination modulator in the source's group.
    slot: usize,
    source: ir::ModulatorRef,
    depth: f64,
    flags: u8,
    /// Inversion helpers (`fcinv_flip`/`fcinv_neg`) have no proved meaning yet.
    inverted: bool,
    /// The assignment's shaper, applied to the unipolar source (`ftable`).
    shape: Option<ir::ShapeRef>,
    /// Saved lag milliseconds (0: none).
    lag_ms: i16,
}

impl Translation {
    /// The internal modulator in `source`'s group at module slot `slot`.
    fn slot_modulator(&self, source: ir::ModulatorRef, slot: usize) -> Option<ir::ModulatorRef> {
        let mods = &self.ir.source_indices.modulators;
        let group = mods.iter().find(|m| m.runtime == Some(source))?.group;
        mods.iter().find(|m| m.group == group && m.slot == slot && !m.external)?.runtime
    }

    /// The group's own amplitude envelope: the internal AHDSR whose primary
    /// volume route drives the voice. Another AHDSR in the group is a
    /// modulator source, not the voice's envelope.
    fn amplitude_envelope(&self, group: usize) -> Option<ir::ModulatorRef> {
        self.amplitude_envelopes.get(group).copied().flatten()
    }

    /// Attach each intensity assignment to the routes of the modulator it
    /// targets. Only what sampler-core's ID25 preparation accepts is attached
    /// (saved flags 0x10/0x14, depth and route intensity in 0..=1, unshaped and
    /// unlagged, a non-live source); the rest is reported, never approximated.
    fn apply_intensity_scales(&mut self) {
        for scale in std::mem::take(&mut self.intensity_scales) {
            let Some(destination) = self.slot_modulator(scale.source, scale.slot) else {
                self.unsupported(
                    &scale.at,
                    "modulation of a modulator's intensity",
                    format!("no translated modulator at slot {}", scale.slot),
                    ir::Reason::NotModeled,
                );
                continue;
            };
            let bipolar = matches!(
                self.ir.modulators.get(scale.source.0).map(|m| &m.source),
                Some(
                    ir::ModulationSource::Lfo(_)
                        | ir::ModulationSource::PitchBend
                        | ir::ModulationSource::ScriptBipolar(_)
                )
            );
            // Flag 0x02 is the stored depth sign (already in `depth`); 0x04 picks
            // the combination expression; 0x10 is the saved-target marker.
            let mut why = None;
            if !matches!(scale.flags & !0x02, 0x10 | 0x14) {
                why = Some(format!("flags 0x{:02x}", scale.flags));
            } else if !(-1. ..=1.).contains(&scale.depth) {
                why = Some(format!("depth {}", scale.depth));
            } else if scale.inverted {
                why = Some("inverted".into());
            } else if scale.shape.is_some() && bipolar {
                // The compiler's fifth argument picks ftable($c, frange_uni(s))
                // or ftable($c, s); they agree only for unipolar sources.
                why = Some("shaped bipolar source".into());
            }
            let targets: Vec<usize> = (0..self.ir.routes.len())
                .filter(|&r| self.ir.routes[r].source == destination)
                .collect();
            for r in targets {
                let (saved, unit) = self.route_intensity.get(&r).copied().unwrap_or((f64::NAN, f64::NAN));
                // Saved intensities of ±1e-7 are float32 residue from the file: the
                // route is silent, so an intensity scale on it cannot be heard.
                // ponytail: 1e-6 cutoff, a route this small is below the output's float32 noise floor.
                if saved.is_finite() && saved.abs() <= 1e-6 {
                    continue;
                }
                let route = &self.ir.routes[r];
                let reason = why.clone().or_else(|| {
                    if !unit.is_finite() || unit == 0. || !(0. ..=1.).contains(&saved) {
                        Some(format!("route intensity {saved}"))
                    } else if route.target == ir::Target::SampleStart {
                        Some("start-offset route".into())
                    } else {
                        None
                    }
                });
                if let Some(reason) = reason {
                    self.unsupported(
                        &scale.at,
                        "modulation of a modulator's intensity",
                        reason,
                        ir::Reason::NotModeled,
                    );
                    continue;
                }
                let assignment = ir::RouteScale {
                    source: scale.source,
                    shape: scale.shape,
                    law: ir::ScaleLaw::KontaktIntensity { depth: scale.depth, flags: scale.flags, unit },
                    lag_ms: f64::from(scale.lag_ms),
                };
                let route = ir::RouteRef(r);
                match self.ir.routes[r].scale {
                    None => self.ir.routes[r].scale = Some(assignment),
                    Some(first) => {
                        // Each template's `$in` is the lane's current value, so
                        // assignments chain; same-expression ones commute, which
                        // is the only order-independent case proved.
                        let same = |a: ir::ScaleLaw, b: ir::ScaleLaw| match (a, b) {
                            (
                                ir::ScaleLaw::KontaktIntensity { flags: a, .. },
                                ir::ScaleLaw::KontaktIntensity { flags: b, .. },
                            ) => a & 4 == b & 4,
                            _ => false,
                        };
                        let chained = &self.ir.source_indices.chained_scales;
                        let held = chained.iter().filter(|c| c.0 == route).count();
                        let reason = if held >= sampler_core::MORE_SCALES {
                            Some("more intensity assignments than the router chains")
                        } else if !same(first.law, assignment.law)
                            || chained.iter().any(|c| c.0 == route && !same(c.1.law, assignment.law))
                        {
                            Some("mixed combination expressions on one route")
                        } else {
                            None
                        };
                        match reason {
                            Some(reason) => self.unsupported(
                                &scale.at,
                                "modulation of a modulator's intensity",
                                reason,
                                ir::Reason::NotModeled,
                            ),
                            None => self.ir.source_indices.chained_scales.push((route, assignment)),
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod time_machine_tests {
    use super::*;

    /// Both spellings of a group-level write reach the same stretch field; another group's write does not.
    #[test]
    fn group_writes_accept_generic_minus_one_and_zero() {
        let write = |generic, group, value| sampler_ksp::EnginePar {
            parameter: "ENGINE_PAR_SPEED".into(),
            value,
            group,
            slot: -1,
            generic,
        };
        let corpus = [write(-1, 0, 750_000)];
        assert_eq!(group_engine_write(&corpus, "ENGINE_PAR_SPEED", 0), Some(750_000));
        let measured = [write(0, 0, 250_000), write(0, 1, 900_000)];
        assert_eq!(group_engine_write(&measured, "ENGINE_PAR_SPEED", 0), Some(250_000));
        assert_eq!(group_engine_write(&measured, "ENGINE_PAR_SPEED", 2), None);
        let both = [write(0, 0, 250_000), write(-1, 0, 750_000)];
        assert_eq!(group_engine_write(&both, "ENGINE_PAR_SPEED", 0), Some(750_000));
    }

    /// A v0x106 source record as Conflux stores it (common record defaults),
    /// plus the mode-specific extension.
    fn record(mode: u32, speed: (f32, f32, f32), extension: &[u8]) -> Vec<u8> {
        let mut r = vec![0, 0x06, 0x01];
        r.extend(mode.to_le_bytes());
        r.extend(1.0f32.to_le_bytes()); // common float
        r.push(0); // flag
        r.extend(1u32.to_le_bytes()); // enum
        r.push(0); // flag
        for v in [speed.0, speed.1, speed.2] {
            r.extend(v.to_le_bytes());
        }
        r.push(0); // timing flag
        r.extend(extension);
        r
    }

    fn parse(bytes: Vec<u8>) -> ni_file::kontakt::objects::BParSrcMode {
        ni_file::kontakt::objects::BParSrcMode::read(std::io::Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn time_machine_records_map_per_the_spec() {
        // TM1: smooth F, grain F, HQ B.
        let mut tail = Vec::new();
        tail.extend(0.75f32.to_le_bytes());
        tail.extend(12.5f32.to_le_bytes());
        tail.push(0);
        let tm1 = parse(record(SOURCE_MODE_TM1, (-1.0, 0.0, 1.0), &tail));
        let mut notes = Vec::new();
        let t = stretch_from_record(&tm1, &mut notes).unwrap();
        assert_eq!((t.kind, t.speed, t.smooth, t.grain_ms, t.high_quality), (ir::StretchKind::TimeMachine1, 1.0, 0.75, 12.5, false));
        assert!(notes.is_empty());
        // Tone Machine (stored 1): TM1 machine-record fields, reported as an unmeasured stand-in.
        let tone = parse(record(SOURCE_MODE_TONE, (-1.0, 0.0, 1.0), &tail));
        let mut notes = Vec::new();
        let t = stretch_from_record(&tone, &mut notes).unwrap();
        assert_eq!((t.kind, t.smooth, t.grain_ms), (ir::StretchKind::ToneMachine, 0.75, 12.5));
        assert!(notes.iter().any(|n| n.0.starts_with("tone machine backend")));
        notes.clear();
        // TM2: transient copy B, size I, grain profile I; reported as stand-in.
        let ext = [1, 0xe8, 3, 0, 0, 2, 0, 0, 0];
        let tm2 = parse(record(SOURCE_MODE_TM2, (-1.0, 0.0, 1.0), &ext));
        let t = stretch_from_record(&tm2, &mut notes).unwrap();
        assert_eq!((t.transient_copy, t.transient_size, t.grain_profile), (true, 1000, 2));
        assert_eq!(notes.len(), 1);
        // A synced timing value is reported, not guessed.
        let synced = parse(record(SOURCE_MODE_TM2, (3.0, 5.0, 1.0), &ext));
        notes.clear();
        stretch_from_record(&synced, &mut notes).unwrap();
        assert!(notes.iter().any(|n| n.0.contains("tempo-synced")));
    }

    #[test]
    fn the_stored_mode_ids_name_the_machines_and_dfd_stays_a_sampler() {
        assert_eq!(machine_kind(4), Some(ir::Machine::BeatMachine));
        assert_eq!(machine_kind(6), Some(ir::Machine::S1200 { filter_mode: None }));
        assert_eq!(machine_kind(7), Some(ir::Machine::Mp60));
        assert_eq!(machine_kind(SOURCE_MODE_TM1), None);
        assert_eq!(source_mode_name(SOURCE_MODE_TM1), "Time Machine");
        assert_eq!(source_mode_name(SOURCE_MODE_TM2), "Time Machine 2");
        assert_eq!(source_mode_name(SOURCE_MODE_TM_PRO), "Time Machine Pro");
        assert_eq!(SOURCE_MODE_DFD, 3);
    }

    #[test]
    fn beat_machine_slice_fields_map_to_atk_rel_and_flag() {
        use ni_file::kontakt::objects::{BParSrcMode, SourceField, SourceValue as V};
        let field = |name: &'static str, value| SourceField {
            offset: 0,
            name,
            value,
        };
        let record = |fields: Vec<SourceField>| BParSrcMode {
            version: 0x106,
            mode: 4,
            fields,
            bytes: 0,
        };
        let full = record(vec![
            field("slice_float_1", V::Float(32.0)),
            field("slice_float_2", V::Float(600.0)),
            field("slice_flag_1", V::Flag(true)),
        ]);
        let expected = ir::BeatSlices {
            attack_ms: 32.0,
            release_ms: 600.0,
            flag: true,
        };
        assert_eq!(beat_slices_from_record(&full), Some(expected));
        let partial = record(vec![field("slice_float_1", V::Float(1.0))]);
        assert_eq!(beat_slices_from_record(&partial), None);
    }
}

#[cfg(test)]
mod doctor_envelope_coefficients_tests {
    use super::*;
    #[test]
    fn scalar_envelope_coefficients_retain_native_decay_landmarks_and_attack_direction() {
        let (fast, fall) = ahdsr_coefficients(0.5);
        let (slow, _) = ahdsr_coefficients(-0.5);
        let progress = |coefficient: f64, time: f64| (coefficient * time).exp_m1() / coefficient.exp_m1();
        assert!(progress(fast, 0.5) > 0.5);
        assert!(progress(slow, 0.5) < 0.5);
        // KONTAKT_REFERENCE.md section 17: remaining travel measured during decay.
        for (time, measured) in [(0.125, 0.67), (0.375, 0.30), (0.625, 0.11), (0.875, 0.026)] {
            assert!((1.0 - progress(fall, time) - measured).abs() < 0.04);
        }
        assert_eq!(ahdsr_coefficients(-2.), ahdsr_coefficients(-1.));
        assert_eq!(ahdsr_coefficients(2.), ahdsr_coefficients(1.));
    }
}

#[cfg(test)]
mod velocity_volume_law_tests {
    use super::*;

    #[test]
    fn full_and_default_route_intensities_pick_their_velocity_laws() {
        assert_eq!(velocity_volume_law(1.0), Some(ir::VelocityResponse::Linear));
        // Stored as f32 in the NKI (the Beat Machine route reads 0.3937009871).
        assert_eq!(
            velocity_volume_law(f64::from(0.393_700_99_f32)),
            Some(ir::VelocityResponse::VolumeTable)
        );
        assert_eq!(velocity_volume_law(0.5), None);
        assert_eq!(velocity_volume_law(0.0), None);
    }
}
