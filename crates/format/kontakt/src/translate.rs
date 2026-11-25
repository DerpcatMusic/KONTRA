//! A Kontakt program as the instrument model: groups, zones, start rules,
//! envelopes and scripts. Kontakt stores gains and tunes as ratios.

use std::collections::HashMap;
use std::path::Path;

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::KontaktChunks;
use ni_file::kontakt::objects::{
    BParScript, Group, GroupList, InternalModArray16, LoopArray, Modulator, StartCriteriaParams,
    VoiceGroups, VoiceLimit, ZoneList, ZoneSlices,
};

use crate::Error;
use crate::external::external;
use crate::laws;
use crate::resources::Resources;
use crate::samples::Resolver;
use crate::timed::{knob_route, knob_routes, timed};

const GROUP_LIST: u16 = 0x33;
const ZONE_LIST: u16 = 0x34;
const ZONE_SLICES: u16 = 0x35;
const LOOPS: u16 = 0x39;
const INTERNAL_MODS: u16 = 0x3b;
const GROUP_DYNAMICS: u16 = 0x4a;
const SCRIPT: u16 = 6;
const VOICE_GROUPS: u16 = 0x32;
const START_CRITERIA: u16 = 0x38;
/// The browser's preview data.
const QUICK_BROWSE: u16 = 0x4e;

fn zone_loops(
    chunk: Option<&ni_file::kontakt::Chunk>,
    muted: bool,
    notes: &mut Notes,
) -> Result<Option<LoopArray>, ni_file::Error> {
    match chunk.map(LoopArray::try_from).transpose() {
        Ok(loops) => Ok(loops),
        Err(e) if muted => {
            notes.add(ir::Impact::Audible, "muted zone loops", e);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}
/// Stored fields the translator maps, and those that are editor state only.
// `unknown_tail` is the opaque public suffix every program stores.
#[cfg(test)]
const USED_PROGRAM: &[&str] = &[
    "name",
    "transpose",
    "volume",
    "pan",
    "tune",
    "dfd_channel_preload_size",
    "low_key",
    "high_key",
    "low_velocity",
    "high_velocity",
    "default_key_switch",
];
#[cfg(test)]
const COSMETIC_PROGRAM: &[&str] = &[
    "unknown_tail",
    "loading_flags",
    "num_bytes_samples_total",
    "library_id",
    "fingerprint",
    "cat_icon_idx",
    "instrument_credits",
    "instrument_author",
    "instrument_url",
    "instrument_cat1",
    "instrument_cat2",
    "instrument_cat3",
    "resource_container_filename",
    "wallpaper_filename",
];
#[cfg(test)]
const USED_GROUP: &[&str] = &[
    "name",
    "volume",
    "pan",
    "tune",
    "key_tracking",
    "release_trigger",
    "rls_trig_counter",
    "start_criteria",
    "voice_group_index",
    "muted",
    "soloed",
    "fx_idx_amp_split_point",
    "release_trigger_note_monophonic",
    "reverse",
];
#[cfg(test)]
const COSMETIC_GROUP: &[&str] = &[];
#[cfg(test)]
const USED_ZONE: &[&str] = &[
    "sample_start",
    "sample_end",
    "sample_start_mod_range",
    "low_velocity",
    "high_velocity",
    "low_key",
    "high_key",
    "fade_low_velocity",
    "fade_high_velocity",
    "fade_low_key",
    "fade_high_key",
    "root_key",
    "zone_volume",
    "zone_pan",
    "zone_tune",
    "sample_present",
    "filename_id",
    "sample_rate",
    "num_channels",
    "num_frames",
];
// The sample header's own copies (`root_note`, `tuning`, format); the zone's
// `root_key` and `zone_tune` carry what plays.
// `filename_prefix` is the sample's name prefix; the file table resolves the sample.
#[cfg(test)]
const COSMETIC_ZONE: &[&str] = &[
    "filename_prefix",
    "reserved1",
    "reserved2",
    "reserved3",
    "reserved4",
    "root_note",
    "tuning",
    "sample_data_type",
];

/// Fast typed equivalent of `notes.ignored_fields("program", p, USED_PROGRAM, COSMETIC_PROGRAM)`,
/// exhaustively destructuring [`ProgramPublicParams`](ni_file::kontakt::objects::ProgramPublicParams)
/// so a new field fails to compile until classified.
fn check_ignored_program_fields(
    notes: &mut Notes,
    p: &ni_file::kontakt::objects::ProgramPublicParams,
) {
    let ni_file::kontakt::objects::ProgramPublicParams {
        name: _,
        num_bytes_samples_total: _,
        transpose: _,
        volume: _,
        pan: _,
        tune: _,
        low_velocity: _,
        high_velocity: _,
        low_key: _,
        high_key: _,
        default_key_switch: _,
        dfd_channel_preload_size: _,
        library_id: _,
        fingerprint: _,
        loading_flags: _,
        group_solo,
        cat_icon_idx: _,
        instrument_credits: _,
        instrument_author: _,
        instrument_url: _,
        instrument_cat1: _,
        instrument_cat2: _,
        instrument_cat3: _,
        resource_container_filename: _,
        wallpaper_filename: _,
        unknown_tail: _,
    } = p;
    if *group_solo {
        notes.ignored("program.group_solo", group_solo);
    }
}

/// Fast typed equivalent of `notes.ignored_fields("zone", z, USED_ZONE, COSMETIC_ZONE)`.
/// Avoids formatting `{z:#?}` once per zone (42k+ allocations on large libraries)
/// while exhaustively destructuring [`ZoneParams`] so any newly added field fails
/// to compile until explicitly classified.
fn check_ignored_zone_fields(notes: &mut Notes, z: &ni_file::kontakt::objects::ZoneParams) {
    let ni_file::kontakt::objects::ZoneParams {
        sample_start: _,
        sample_end: _,
        sample_start_mod_range: _,
        low_velocity: _,
        high_velocity: _,
        low_key: _,
        high_key: _,
        fade_low_velocity: _,
        fade_high_velocity: _,
        fade_low_key: _,
        fade_high_key: _,
        root_key: _,
        zone_volume: _,
        zone_pan: _,
        zone_tune: _,
        sample_present: _,
        filename_id: _,
        filename_prefix: _,
        sample_data_type: _,
        sample_rate: _,
        num_channels: _,
        num_frames: _,
        reserved1: _,
        reserved2: _,
        root_note: _,
        tuning: _,
        reserved3: _,
        reserved4: _,
        unknown_tail,
    } = z;
    if !unknown_tail.is_empty() {
        notes.ignored("zone.unknown_tail", "(nested)");
    }
}

/// Fast typed equivalent of `notes.ignored_fields("group", v, USED_GROUP, COSMETIC_GROUP)`,
/// exhaustively destructuring [`GroupParams`](ni_file::kontakt::objects::GroupParams)
/// so a new field fails to compile until classified.
fn check_ignored_group_fields(notes: &mut Notes, v: &ni_file::kontakt::objects::GroupParams) {
    let ni_file::kontakt::objects::GroupParams {
        name: _,
        volume: _,
        pan: _,
        tune: _,
        key_tracking: _,
        reverse: _,
        release_trigger: _,
        release_trigger_note_monophonic: _,
        rls_trig_counter: _,
        midi_channel,
        voice_group_index: _,
        fx_idx_amp_split_point: _,
        muted: _,
        soloed: _,
        interp_quality,
        start_criteria: _,
        unknown_tail,
    } = v;
    // `ignored_fields` treats 0 and -1 as unset.
    if !matches!(midi_channel, 0 | -1) {
        notes.ignored("group.midi_channel", midi_channel);
    }
    if !matches!(interp_quality, 0 | -1) {
        notes.ignored("group.interp_quality", interp_quality);
    }
    if !unknown_tail.is_empty() {
        notes.ignored("group.unknown_tail", "(nested)");
    }
}

/// The translated instrument with its notes and every structural issue
/// [`ir::validate`] finds, so a bad reference is reported at load.
fn finish(mut out: ir::Instrument, notes: Notes) -> ir::Instrument {
    out.diagnostics = notes.finish();
    let issues = ir::validate(&out);
    out.diagnostics.extend(issues);
    out
}

/// `saved`: a snapshot's persistent values per script slot, over the
/// instrument's own.
pub(crate) fn instrument(
    path: &Path,
    chunks: &KontaktChunks,
    monolith: Option<ni_file::file_container::NIFileContainer>,
    saved: &[Vec<String>],
    encrypted: Option<bool>,
    native: Option<crate::snapshot::Native>,
) -> Result<ir::Instrument, Error> {
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let mut program = chunks
        .program()
        .ok_or_else(|| Error::new(path, "program", "missing"))?
        .map_err(decode("program"))?;
    let params = program.params().map_err(decode("program parameters"))?;
    let mut notes = Notes::default();
    check_ignored_program_fields(&mut notes, &params);
    // The programs' own playable range (a stored 0..127 / 1..127 is the whole keyboard).
    if params.low_key != 0
        || params.high_key != 127
        || params.low_velocity > 1
        || params.high_velocity != 127
    {
        notes.ignored(
            "program key/velocity range",
            format!(
                "keys {}..{}, velocity {}..{}",
                params.low_key, params.high_key, params.low_velocity, params.high_velocity
            ),
        );
    }
    crate::ignored::chunks(
        &mut notes,
        "program",
        &program.0,
        &[
            SCRIPT,
            GROUP_LIST,
            ZONE_LIST,
            VOICE_GROUPS,
            0x3a,
            0x3d,
            0x45,
            0x4b,
        ],
        &[QUICK_BROWSE],
    );
    let mut out = ir::Instrument {
        name: params.name.clone(),
        // −1 (none) and anything off the keyboard fail the conversion.
        default_keyswitch: u8::try_from(params.default_key_switch)
            .ok()
            .filter(|k| *k < 128),
        source_path: std::path::absolute(path).ok(),
        library_path: (encrypted == Some(true))
            .then(|| std::path::absolute(path).ok())
            .flatten()
            .and_then(|p| crate::resources::known_root(&p)),
        voices: 256,
        output: ir::Output {
            volume_db: db(params.volume),
            pan: params.pan.clamp(-1.0, 1.0),
            controllers: output_controllers(program.version(), &program.0.private_data),
            ..ir::Output::UNITY
        },
        ..ir::Instrument::DEFAULT
    };
    // Its pitch modulation's 2^x is the quadratic at [ref]: the
    // mantissa `f − 0.33971·(f − f²)` on the exponent
    // `n + 127`. Measured on a ±2 st pitch-bend route: +1 → 1.954 st,
    // −1 → −1.958, ±¼ → 0.480/−0.485, within 0.02 cents of this.
    out.route_pitch = ir::Exp2::Quadratic { c: 0.33971 };
    // The instrument's tune applies to every group.
    let base = Base {
        tune: laws::semitones(params.tune) + f64::from(params.transpose),
        preload: params.dfd_channel_preload_size,
    };
    let mut resources = None;
    let (links, others) = crate::links::links(&program.0.private_data);
    for name in others {
        let kind = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
        notes.add(
            ir::Impact::Audible,
            &format!("MIDI automation of {kind}"),
            &name,
        );
    }
    for (slot, chunk) in (program.0.children.iter().filter(|c| c.id == SCRIPT)).enumerate() {
        let script = BParScript::try_from(chunk)
            .and_then(|s| s.params())
            .map_err(decode("script"))?;
        let inline = script.text.filter(|t| !t.trim().is_empty());
        let linked = if !script.bypass && inline.is_none() {
            script
                .textfile_name
                .as_deref()
                .filter(|n| !n.is_empty())
                .and_then(|file| {
                    let res = resources.get_or_insert_with(|| Resources::open(path));
                    let clean = file.replace('\\', "/");
                    let clean = clean.trim_start_matches('/');
                    let bytes = res
                        .read(&format!("scripts/{clean}"))
                        .or_else(|| res.read(&format!("scripts/{clean}.txt")))
                        .or_else(|| res.read(clean))?;
                    Some(BParScript::decode_source(bytes))
                })
        } else {
            None
        };
        match inline.or(linked) {
            _ if script.bypass => {}
            Some(text) if !text.trim().is_empty() => out.scripts.push(ir::Script {
                name: script.description.unwrap_or_default(),
                slot,
                dialect: ir::Dialect::Ksp,
                data: data_files(&text, path, &mut resources),
                source: text,
                saved: crate::snapshot::over(
                    script.persistent,
                    saved.get(slot).map_or(&[], Vec::as_slice),
                ),
                links: (links.iter())
                    .filter(|(s, _)| usize::from(*s) == slot)
                    .map(|&(_, l)| l)
                    .collect(),
                container: None,
            }),
            _ => {
                if let Some(file) = script.textfile_name.filter(|n| !n.is_empty()) {
                    notes.add(ir::Impact::Broken, "linked script file", file);
                }
            }
        }
    }
    for why in resources.iter().flat_map(|r| &r.unreadable) {
        notes.add(ir::Impact::Broken, "resource archive", why);
    }
    let mut groups = program
        .0
        .find_first(GROUP_LIST)
        .ok_or_else(|| Error::new(path, "group list", "missing"))
        .and_then(|c| GroupList::try_from(c).map_err(decode("group list")))?;
    let mut resolver = Resolver::new(path, monolith).map_err(|e| Error::new(path, "samples", e))?;
    let (table, others) = crate::impulse::file_tables(chunks)
        .transpose()
        .map_err(decode("sample table"))?
        .unwrap_or_default();
    let mut impulses = crate::impulse::Impulses::new(others, &mut resolver, path, resources);
    let mut criteria = Vec::new();
    let mut slots = crate::effects::Slots::default();
    // A snapshot's racks and group effects stand in for the base's.
    let mut group_fx = Vec::new();
    let snapshot_racks = native.is_some();
    if let Some(native) = native {
        crate::effects::replace_racks(&mut program.0.children, native.racks);
        crate::snapshot::unmatched(native.groups.len(), groups.groups.len(), &mut notes);
        group_fx = native.groups;
    }
    for (index, group) in groups.groups.iter_mut().enumerate() {
        let mut saved = group_fx.get_mut(index).and_then(Option::take);
        // A snapshot's params, source and modulators stand in for the group's own.
        if let Some(s) = &mut saved {
            s.apply(group)
                .map_err(|e| Error::new(path, "snapshot group", format!("{index}: {e}")))?;
        }
        let group = &*group;
        let t = translate_group(group, index, &base, &mut notes)
            .map_err(|e| Error::new(path, "group", format!("{index}: {e}")))?;
        let mut g = t.group;
        if let Some(purged) = saved.as_ref().and_then(|s| s.purged) {
            g.purged = purged;
        }
        let fx = saved.map(|s| s.fx);
        impulses.snapshot = fx.is_some();
        g.inserts = crate::effects::group(group, fx, index, &mut impulses, &mut notes, &mut slots);
        crate::drive::unmoved(&g.inserts, &g.mods, &mut notes);
        out.groups.push(g);
        criteria.push(t.criteria);
    }
    impulses.snapshot = snapshot_racks;
    let racks = crate::effects::program(&program, &mut impulses, &mut notes);
    let mut buses = racks.buses;
    (out.inserts, out.sends, out.main) = (racks.inserts, racks.sends, racks.main);
    slots.laws.extend(racks.laws.laws);
    slots.shape_slots.extend(racks.laws.shape_slots);
    slots.subtypes.extend(racks.laws.subtypes);
    slots.types.extend(racks.laws.types);
    slots.models.extend(racks.laws.models);
    slots.held.extend(racks.laws.held);
    out.slot_laws = std::mem::take(&mut slots.laws);
    out.slot_models = std::mem::take(&mut slots.models);
    slot_shapes(&mut out, slots, &mut buses);
    // A script may load any response of the library's `ir_samples` by name.
    if out
        .scripts
        .iter()
        .any(|s| contains_ascii_ci(&s.source, "load_ir_sample"))
    {
        out.responses = impulses.library();
    }
    out.impulses = impulses.out;
    // Responses are samples after the zones' (indices into the same table).
    let responses = impulses.samples;
    // Bus 0 is the main mix; Kontakt's bus n is n + 1.
    if !buses.is_empty() {
        out.buses.push(ir::Bus::new("Main"));
        out.buses.extend(buses);
        for (g, group) in out.groups.iter_mut().zip(&groups.groups) {
            if let Some(n) = group
                .bus_route()
                .filter(|&n| usize::from(n) + 1 < out.buses.len())
            {
                g.bus = usize::from(n) + 1;
            }
        }
    }
    start_rules(&mut out.groups, &criteria, &mut notes);
    let limits = program
        .0
        .find_first(VOICE_GROUPS)
        .map(VoiceGroups::try_from)
        .transpose()
        .map_err(decode("voice groups"))?;
    voice_groups(&mut out, &groups, limits.as_ref());
    // Effective group mute: when any group is soloed, every non-soloed group
    // is muted (`soloed` overrides `muted`); otherwise each group keeps its
    // own `muted` flag. Zones still load so scripts and future un-muting see them.
    let any_soloed = groups
        .groups
        .iter()
        .any(|g| g.params().is_ok_and(|p| p.soloed));
    for (out_group, raw_group) in out.groups.iter_mut().zip(&groups.groups) {
        if let Ok(p) = raw_group.params() {
            out_group.muted = if any_soloed { !p.soloed } else { p.muted };
        }
    }
    let zones = program
        .0
        .find_first(ZONE_LIST)
        .ok_or_else(|| Error::new(path, "zone list", "missing"))
        .and_then(|c| ZoneList::try_from(c).map_err(decode("zone list")))?;
    let mut sample_of: HashMap<u32, Option<usize>> = HashMap::new();
    resolver.warm_zones(&zones, &table);
    // A zone's id is its place in the zone list, saved or not.
    for (zone_id, (zone, &group)) in (0u32..).zip(zones.zones().iter().zip(&zones.group_ids)) {
        let z = zone.params().map_err(decode("zone"))?;
        check_ignored_zone_fields(&mut notes, &z);
        crate::ignored::chunks(
            &mut notes,
            "zone",
            &zone.0,
            &[LOOPS, ZONE_SLICES],
            &[QUICK_BROWSE],
        );
        if let Some(chunk) = zone.0.find_first(ZONE_SLICES) {
            // `ZoneSlices` (0x35) stores wave-editor slice markers; Kontakt's
            // audio engine only consumes them in Beat Machine (mode 4, already
            // diagnosed on the group) and slice-trigger modes.
            if let Err(e) = ZoneSlices::try_from(chunk) {
                notes.ignored("zone chunk 0x35 ZoneSlices", e.to_string());
            }
        }
        if !z.sample_present {
            continue;
        }
        let id = u32::try_from(z.filename_id).unwrap_or(u32::MAX);
        let sample = *sample_of.entry(id).or_insert_with(|| {
            let Some(name) = table.get(&id) else {
                notes.add(
                    ir::Impact::Broken,
                    "missing sample table entry",
                    format!("zone {zone_id} filename_id {}", z.filename_id),
                );
                return None;
            };
            let found = resolver
                .resolve(name)
                .and_then(|at| at.ok_or("not found".into()));
            match found {
                Ok(at) => {
                    out.samples.push(ir::Sample {
                        name: name.clone(),
                        file: at.file,
                        offset: at.offset,
                        len: at.size,
                        channels: z.num_channels,
                        frames: u64::try_from(z.num_frames).unwrap_or(0),
                        rate: u32::try_from(z.sample_rate).unwrap_or(0),
                        keystream: at.keystream,
                        raw: None,
                    });
                    Some(out.samples.len() - 1)
                }
                Err(e) if e == "not found" => {
                    // Where the first folder looked would have it, which is
                    // how the player finds a moved one.
                    let at = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
                    let at = at
                        .parent()
                        .unwrap_or(Path::new(""))
                        .join(name.replace(['\\', '|'], "/"));
                    notes.add_missing(&at);
                    None
                }
                Err(e) => {
                    notes.add(ir::Impact::Broken, "missing sample", format!("{name}: {e}"));
                    None
                }
            }
        });
        let (Some(sample), Some(g)) = (sample, out.groups.get_mut(group as usize)) else {
            continue;
        };
        let frames = out.samples.get(sample).map_or(0, |s| s.frames);
        let loops = zone_loops(zone.0.find_first(LOOPS), g.muted, &mut notes)
            .map_err(decode("zone loops"))?;
        let reverse =
            g.zones.first().is_some_and(|z| z.reverse) || criteria_reverse(group, &groups);
        let streaming = g.stream.is_some();
        if let Some(mut translated) = translate_zone(
            &z,
            loops.as_ref(),
            sample,
            frames,
            reverse,
            streaming,
            &mut notes,
        ) {
            translated.id = zone_id;
            if !zone.0.private_data.is_empty() {
                match zone.zone_envelopes() {
                    Ok(envs) => {
                        translated.envelopes =
                            envs.iter().filter_map(laws::zone_envelope).collect();
                    }
                    Err(e) => {
                        notes.add(ir::Impact::Audible, "zone envelopes", e.to_string());
                    }
                }
            }
            kontra_law::record(|| kontra_law::Application {
                id: kontra_law::LawId("kontakt.zone_gain_pan_tune.v1"),
                location: format!("groups/{group}/zones/{}", g.zones.len()),
                input: format!(
                    "volume={:?}, pan={:?}, tune_ratio={:?}",
                    z.zone_volume, z.zone_pan, z.zone_tune
                ),
                output: format!(
                    "volume_db={:?}, pan={:?}, tune={:?}",
                    translated.volume_db, translated.pan, translated.tune
                ),
            });
            g.zones.push(translated);
        }
    }
    let first = out.samples.len();
    out.impulses.iter_mut().for_each(|i| i.sample += first);
    for (_, sample) in &mut out.responses {
        *sample += first;
    }
    out.samples.extend(responses);
    Ok(finish(out, notes))
}

/// The slots a script may read, load into or switch shapes on.
fn slot_shapes(out: &mut ir::Instrument, slots: crate::effects::Slots, buses: &mut [ir::Bus]) {
    if !slots.shape_slots.is_empty() {
        out.slot_shapes.shapes = crate::laws::filter_shapes();
        kontra_law::record(|| kontra_law::Application {
            id: kontra_law::LawId("kontakt.filter_shapes.v1"),
            location: "slot_shapes/shapes".into(),
            input: "()".into(),
            output: format!("{:?}", out.slot_shapes.shapes),
        });
        out.slot_shapes.slots = slots.shape_slots;
    }
    // What a script reads of a saved slot, whether or not it loads types:
    // Conflux reads $ENGINE_PAR_BITE_BITS with no `$ENGINE_PAR_EFFECT_`.
    out.slot_shapes.saved_types = slots.types;
    out.slot_shapes.held = slots.held;
    let loads = |s: &ir::Script| {
        contains_ascii_ci(&s.source, "$ENGINE_PAR_EFFECT_")
            || contains_ascii_ci(&s.source, "$ENGINE_PAR_SEND_EFFECT_")
    };
    if out.scripts.iter().any(loads) {
        // Every Kontakt rack has 8 slots, and a script may load into an
        // empty one the file does not save.
        let racks = [&mut out.inserts, &mut out.sends, &mut out.main].into_iter();
        let groups = out.groups.iter_mut().map(|g| &mut g.inserts);
        for rack in racks
            .chain(groups)
            .chain(buses.iter_mut().map(|b| &mut b.inserts))
        {
            if rack.len() < SLOTS {
                rack.resize(SLOTS, None);
            }
        }
        // A filter a script loads into an empty slot switches shapes as a
        // saved one does (group slots load in `on init` only).
        let empty = |at, rack: &[Option<ir::Effect>]| {
            let slots = (0u8..).zip(rack.iter());
            slots
                .filter(|(_, s)| s.is_none())
                .map(move |(n, _)| (at, n))
                .collect::<Vec<_>>()
        };
        let mut open = empty(ir::RackAt::Inserts, &out.inserts);
        open.extend(empty(ir::RackAt::Sends, &out.sends));
        open.extend(empty(ir::RackAt::Main, &out.main));
        for (b, bus) in (0u8..).zip(buses.iter()) {
            open.extend(empty(ir::RackAt::Bus(b), &bus.inserts));
        }
        if !open.is_empty() && out.slot_shapes.shapes.is_empty() {
            out.slot_shapes.shapes = crate::laws::filter_shapes();
            kontra_law::record(|| kontra_law::Application {
                id: kontra_law::LawId("kontakt.filter_shapes.v1"),
                location: "slot_shapes/shapes".into(),
                input: "()".into(),
                output: format!("{:?}", out.slot_shapes.shapes),
            });
        }
        out.slot_shapes.slots.extend(open);
        out.slot_shapes.types = crate::script_knobs::slot_types();
        out.slot_shapes.fresh = crate::held::fresh();
        // A fresh IRC reads back AUTO_GAIN 1, every other setting plain.
        out.slot_shapes.fresh_normalize = Some(crate::laws::AUTO_GAIN);
        out.slot_shapes.subtypes = slots.subtypes;
    }
}

/// "Accept standard controllers for Volume and Pan" (MIDI CC 7 and 10; the
/// program's private byte 4) and the CC 7 range in dB (an i32 at byte 6),
/// program versions 0xa8..=0xb5; earlier versions keep on, 0 dB. Kontakt
/// 8.13.1 runs them as generated KSP ([ref], "use std. CC7 / CC10"):
/// `$ENGINE_PAR_VOLUME = %CC[7]·k`, k by range −12/−6/0/+6/+12 dB =
/// 3113/3937/4956/6243/7874 and any other range 4956; the volume par's gain
/// is `(par/629960.52)³`. So the fader is `top·(v/127)³`, `top` that cube at
/// 127 (−0.023 dB for "0 dB"); CC 10 is `(v − 63.5)/63.5`. A script's
/// `ignore_controller` stops both.
fn output_controllers(version: u16, private: &[u8]) -> Option<ir::OutputControllers> {
    let saved = (version >= 0xa8).then(|| {
        let top = private.get(6..10).and_then(|b| b.try_into().ok());
        (private.get(4).copied(), top.map(i32::from_le_bytes))
    });
    let (on, range) = match saved {
        Some((Some(on), Some(range))) => (on != 0, range),
        _ => (true, 0),
    };
    let k = match range {
        -12 => 3113.0,
        -6 => 3937.0,
        6 => 6243.0,
        12 => 7874.0,
        _ => 4956.0,
    };
    let top_db = 60.0 * (127.0 * k / kontra_law::common::UNITY_VOLUME as f32).log10();
    on.then_some(ir::OutputControllers {
        volume: 7,
        pan: 10,
        top_db,
        power: 3,
        expression: None,
    })
}

struct Base {
    tune: f64,
    /// The program's DFD preload, bytes per channel.
    preload: i32,
}

pub(crate) fn db(linear: f32) -> f32 {
    kontra_law::common::DB_GAIN
        .inverse(linear.max(1e-6))
        .unwrap_or(0.0)
}

/// Slots in each Kontakt effect rack.
const SLOTS: usize = 8;
fn criteria_reverse(group: u32, groups: &GroupList) -> bool {
    groups
        .groups
        .get(group as usize)
        .and_then(|g| g.params().ok())
        .is_some_and(|p| p.reverse)
}

/// Groups name a voice group 1-based (0 is none). All 128 exist in Kontakt;
/// the file stores only those changed from the default (1 voice, oldest
/// first, released first, 10 ms fade).
///
/// The instrument's own limit (Instrument Options: Max Voices, the stealing
/// mode and fade) is the chunk's first voice limit: measured in the oracle,
/// it steals as a voice group does, "any" the oldest, voices released but
/// still sounding counted.
fn voice_groups(out: &mut ir::Instrument, groups: &GroupList, limits: Option<&VoiceGroups>) {
    if let Some(l) = limits.map(|l| voice_group(&l.voice_limit)) {
        out.voices = l.voices;
        out.steal = l.steal;
        out.steal_released_first = l.prefer_released;
        out.steal_fade_ms = l.fade_ms;
    }
    let mut index = [None::<usize>; 128];
    for (g, group) in out.groups.iter_mut().zip(&groups.groups) {
        let Some((n, slot)) = group
            .params()
            .ok()
            .and_then(|p| usize::try_from(p.voice_group_index - 1).ok())
            .and_then(|n| Some((n, index.get_mut(n)?)))
        else {
            continue;
        };
        let at = *slot.get_or_insert_with(|| {
            let limit = limits.and_then(|l| l.groups.get(n)?.as_ref());
            out.voice_groups.push(limit.map_or(
                ir::VoiceGroup {
                    voices: 1,
                    steal: ir::Steal::Oldest,
                    prefer_released: true,
                    fade_ms: 10.0,
                    exclusive: None,
                    per_note: false,
                    legato: false,
                    returns: false,
                    ..ir::VoiceGroup::DEFAULT
                },
                |l| voice_group(&l.voice_limit),
            ));
            out.voice_groups.len() - 1
        });
        g.voice_group = Some(at);
    }
}

fn voice_group(v: &VoiceLimit) -> ir::VoiceGroup {
    ir::VoiceGroup {
        voices: u16::try_from(v.max_num_voices.max(1)).unwrap_or(u16::MAX),
        // Kontakt 8.13 "Any" (0) selects the oldest voice in the measured case.
        steal: match v.kill_mode {
            2 => ir::Steal::Newest,
            3 => ir::Steal::Highest,
            4 => ir::Steal::Lowest,
            _ => ir::Steal::Oldest,
        },
        prefer_released: v.prefer_released,
        fade_ms: v.ms_fade_time.max(0) as f32,
        exclusive: u8::try_from(v.exclusion_group).ok().filter(|&s| s < 16),
        per_note: false,
        legato: false,
        returns: false,
        ..ir::VoiceGroup::DEFAULT
    }
}

/// ASCII case-insensitive substring check (KSP identifiers and keywords are case-insensitive).
fn contains_ascii_ci(haystack: &str, needle: &str) -> bool {
    let n = needle.len();
    if n == 0 {
        return true;
    }
    haystack
        .as_bytes()
        .windows(n)
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Split `source` on ASCII case-insensitive occurrences of `needle`, returning
/// the slices immediately after each match (preserving original casing in payloads).
fn split_ascii_ci<'a>(source: &'a str, needle: &str) -> Vec<&'a str> {
    let lower = source.to_ascii_lowercase();
    let pat = needle.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&pat) {
        let after = from + rel + pat.len();
        if let Some(rest) = source.get(after..) {
            out.push(rest);
        }
        from = after;
    }
    out
}

/// The library data files a script loads with a literal `load_array(%x, 2)`:
/// `Resources/data/x.nka`, by lowercase array name without its sigil.
fn data_files(
    source: &str,
    path: &Path,
    resources: &mut Option<Resources>,
) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for call in split_ascii_ci(source, "load_array(") {
        let Some((array, mode)) = call.split_once(')').and_then(|(a, _)| a.split_once(',')) else {
            continue;
        };
        let stem = array.trim().get(1..).unwrap_or_default().to_lowercase();
        if mode.trim() != "2" || stem.is_empty() || files.iter().any(|(s, _)| *s == stem) {
            continue;
        }
        let found = resources
            .get_or_insert_with(|| Resources::open(path))
            .read(&format!("data/{stem}.nka"));
        if let Some(bytes) = found {
            files.push((stem, bytes));
        }
    }
    // A Creator Tools performance view the script loads by a literal name:
    // `Resources/performance_view/<name>.nckp`, kept as `<name>.nckp`.
    for call in split_ascii_ci(source, "load_performance_view(") {
        let Some(name) = call
            .split_once(')')
            .map(|(a, _)| a.trim().trim_matches('"'))
        else {
            continue;
        };
        let file = format!("{name}.nckp");
        if name.is_empty()
            || name.contains(['"', '/', '\\'])
            || files.iter().any(|(s, _)| *s == file)
        {
            continue;
        }
        let found = resources
            .get_or_insert_with(|| Resources::open(path))
            .read(&format!("performance_view/{file}"));
        if let Some(bytes) = found {
            files.push((file, bytes));
        }
    }
    // A Kontakt 8 `native_ui` page the script loads by a literal name: every
    // module under `Resources/native_ui/`, kept as `native_ui/<path>`, and
    // the entry's name as `native_ui.entry`.
    if let Some(entry) = split_ascii_ci(source, "load_native_ui(")
        .into_iter()
        .next()
        .and_then(|call| call.split_once(')'))
        .map(|(a, _)| a.trim().trim_matches('"').to_owned())
        .filter(|e| !e.is_empty() && !e.contains(['"', '\\']))
    {
        let resources = resources.get_or_insert_with(|| Resources::open(path));
        for module in resources.under("native_ui") {
            if !module
                .rsplit('.')
                .next()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("nui"))
            {
                continue;
            }
            if let Some(bytes) = resources.read(&format!("native_ui/{module}")) {
                files.push((format!("native_ui/{module}"), bytes));
            }
        }
        files.push(("native_ui.entry".into(), entry.into_bytes()));
    }
    files
}

struct Translated {
    group: ir::Group,
    criteria: Vec<StartCriteriaParams>,
}

fn translate_group(
    group: &Group,
    group_index: usize,
    base: &Base,
    notes: &mut Notes,
) -> Result<Translated, ni_file::Error> {
    let v = group.params()?;
    check_ignored_group_fields(notes, &v);
    crate::ignored::chunks(
        notes,
        "group",
        &group.0,
        &[
            INTERNAL_MODS,
            0x3c,
            0x3a,
            0x45,
            0x3d,
            0x4b,
            GROUP_DYNAMICS,
            START_CRITERIA,
        ],
        &[],
    );
    // `BParGroupDynamics` (0x4A, 64 floats at BGroup + 0x680, [ref]
    // / [ref]) has zero audio-engine consumers in Kontakt 8.13.1
    // and is all-zero in every measured library; report only non-zero or broken payloads.
    match group.dynamics() {
        Ok(Some(d)) if d.has_nonzero() => {
            notes.add(ir::Impact::Audible, "group dynamics (0x4a)", &v.name);
        }
        Ok(Some(_) | None) => {}
        Err(e) => notes.add(
            ir::Impact::Audible,
            "unreadable group dynamics",
            format!("{}: {e}", v.name),
        ),
    }
    let source_identity = match group.source_identity() {
        Ok(id) => Some(id),
        Err(e) => {
            notes.add(
                ir::Impact::Audible,
                "unreadable source mode",
                format!("{}: {e}", v.name),
            );
            None
        }
    };
    let stream_mode = source_identity.map_or(0, |s| s.mode);
    if let Some(label) = unsupported_source_mode(stream_mode) {
        notes.add(ir::Impact::Audible, label, &v.name);
    } else if !matches!(stream_mode, 0 | 2 | 3 | 5 | 8 | 9) {
        notes.add(
            ir::Impact::Audible,
            &format!("source mode {stream_mode}"),
            &v.name,
        );
    }
    let mut out = ir::Group {
        name: v.name.clone(),
        volume_db: db(v.volume),
        pan: v.pan.clamp(-1.0, 1.0),
        tune: laws::semitones(v.tune) + base.tune,
        keytrack: if v.key_tracking { 1.0 } else { 0.0 },
        trigger: if v.release_trigger {
            ir::Trigger::Release
        } else {
            ir::Trigger::Attack
        },
        // Monophonic (on by default with Release Trigger): measured per key,
        // a repeat's note-on cuts the key's sounding release voice; other
        // keys' release voices sound on (release_trigger.lua, Kontakt 8.13).
        repeat_cuts: v.release_trigger && v.release_trigger_note_monophonic,
        // KSP manual, play_note(): in DFD mode a sample offset greater than
        // the zone's S.Mod is ignored and no offset applies.
        offset_limited: stream_mode == 3,
        purged: source_identity.is_some_and(|s| s.flag == 1),
        stream: laws::stream(stream_mode, base.preload),
        speed: matches!(stream_mode, 2 | 5 | 8).then_some(1.0), // Time Machine 1, 2, Pro
        amp_split: u8::try_from(v.fx_idx_amp_split_point.clamp(0, 8)).unwrap_or(8),
        ..ir::Group::DEFAULT
    };
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.stream.v1"),
        location: format!("groups/{group_index}/stream"),
        input: format!("mode={stream_mode:?}, preload={:?}", base.preload),
        output: format!("{:?}", out.stream),
    });
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.group_gain_pan_tune.v1"),
        location: format!("groups/{group_index}"),
        input: format!(
            "volume={:?}, pan={:?}, tune_ratio={:?}, base_tune={:?}",
            v.volume, v.pan, v.tune, base.tune
        ),
        output: format!(
            "volume_db={:?}, pan={:?}, tune={:?}",
            out.volume_db, out.pan, out.tune
        ),
    });
    match group.wavetable_source() {
        Ok(Some(source)) => {
            out.wavetable = Some(crate::wavetable::wavetable(&source, &v.name, notes));
            // A voice reads anywhere in its table at once.
            out.stream = None;
        }
        Ok(None) => {}
        Err(e) => notes.add(
            ir::Impact::Broken,
            "unreadable wavetable source",
            format!("{}: {e}", v.name),
        ),
    }
    let slot = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    let release_counter = laws::release_counter(v.release_trigger, v.rls_trig_counter);
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.release_counter.v1"),
        location: format!("groups/{group_index}/release_counter"),
        input: format!(
            "release_trigger={:?}, time_ms={:?}",
            v.release_trigger, v.rls_trig_counter
        ),
        output: format!("{release_counter:?}"),
    });
    // Each timed modulator's slot and the source it became.
    let mut timed_slots = Vec::new();
    // Slots routed to another modulator's knob, resolved once every slot is translated.
    let mut knobs = Vec::new();
    if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
        for (n, m) in InternalModArray16::try_from(chunk)?.slots()? {
            let p = m.params()?;
            out.routes.push(ir::Routes {
                slot: slot(n),
                name: p.name.clone(),
                names: p.targets.iter().map(|t| t.name.clone()).collect(),
                depths: p.targets.iter().map(|t| t.signed_intensity()).collect(),
            });
            let volume = p
                .targets
                .iter()
                .any(|t| t.param == "volume" && t.slot.is_none());
            // [router open (editor state), bypass, retrigger, ?]; the timed
            // modulators read them (`timed`), the amplitude envelope and the
            // kinds not carried do not.
            let amp_env = volume
                && matches!(p.modulator, Modulator::Ahdsr(_))
                && !out.mods.iter().any(|m| {
                    matches!(
                        m.route,
                        ir::ModRoute {
                            target: ir::ParamAddr::AmpEnvelope,
                            ..
                        }
                    )
                });
            let reads_flags = matches!(
                p.modulator,
                Modulator::Flex(_) | Modulator::Lfo(_) | Modulator::StepMod(_) | Modulator::Dbd(_)
            ) || (matches!(p.modulator, Modulator::Ahdsr(_)) && !amp_env);
            if !reads_flags {
                let is_glide = matches!(p.modulator, Modulator::Glide(_));
                // Retrigger (`+0xab8`, flag 2) is only read by free-running modulators
                // with `(+0xa4) & 0x80 != 0` (LFO and StepMod, [ref]);
                // envelopes, glide, and followers ignore it.
                for (flag, what) in [(1, "bypass"), (3, "flag 3")] {
                    if (amp_env && matches!(flag, 1 | 3)) || (is_glide && flag == 1) {
                        continue;
                    }
                    if p.unknown_flags.get(flag).is_some_and(|&b| b != 0) {
                        notes.ignored(&format!("internal modulator {what}"), &p.name);
                    }
                }
            }
            // `p.unknown_id` (`+0x538`) is an editor group-linking modulator instance id
            // ([ref],11384`); unused by `BGroup::UpdateTemplates` and DSP.
            match &p.modulator {
                // The first volume AHDSR is the amplitude envelope; if it also
                // has secondary targets (e.g. filterCutoff, pitch), those route
                // as a breakpoint copy via `timed`.
                Modulator::Ahdsr(e)
                    if volume
                        && !out.mods.iter().any(|m| {
                            matches!(
                                m.route,
                                ir::ModRoute {
                                    target: ir::ParamAddr::AmpEnvelope,
                                    ..
                                }
                            )
                        }) =>
                {
                    out.envelope = laws::envelope(e);
                    if let Some(t) = p
                        .targets
                        .iter()
                        .find(|t| t.param == "volume" && t.slot.is_none())
                    {
                        let scaled = p.unknown_flags.get(3).is_some_and(|&b| b != 0);
                        laws::envelope_depth(&mut out.envelope, t.signed_intensity(), scaled);
                    }
                    kontra_law::record(|| kontra_law::Application {
                        id: kontra_law::LawId("kontakt.ahdsr_envelope.v1"),
                        location: format!("groups/{group_index}/envelope"),
                        input: format!("{e:?}"),
                        output: format!("{:?}", out.envelope),
                    });
                    let target = p
                        .targets
                        .iter()
                        .position(|t| t.param == "volume" && t.slot.is_none());
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: slot(n),
                        target: target.map_or(0, slot),
                        bypass: p.unknown_flags.get(1).is_some_and(|&b| b != 0),
                        route: ir::ModRoute::AMP_ENVELOPE,
                    });
                    if p.targets.len() > 1
                        && let Some(src) = timed(&mut out, &p, group_index, slot(n), target, notes)
                    {
                        timed_slots.push((n, src));
                    }
                }
                Modulator::Flex(_)
                | Modulator::Lfo(_)
                | Modulator::StepMod(_)
                | Modulator::Ahdsr(_)
                | Modulator::Dbd(_) => {
                    if let Some(src) = timed(&mut out, &p, group_index, slot(n), None, notes) {
                        timed_slots.push((n, src));
                    }
                }
                Modulator::Glide(g)
                    if let Some(t) = p
                        .targets
                        .iter()
                        .find(|t| t.param == "pitch" && t.slot.is_none()) =>
                {
                    let bypassed = p.unknown_flags.get(1).is_some_and(|&b| b != 0);
                    if !bypassed {
                        let law = laws::glide(g, t.signed_intensity());
                        kontra_law::record(|| kontra_law::Application {
                            id: kontra_law::LawId("kontakt.glide.v1"),
                            location: format!("groups/{group_index}/glide"),
                            input: format!("{g:?}"),
                            output: format!("{law:?}"),
                        });
                        out.glide = Some(law);
                    }
                    for t in p
                        .targets
                        .iter()
                        .filter(|t| t.param != "pitch" || t.slot.is_some())
                    {
                        let feature = format!("internal modulator glide -> {}", t.param);
                        notes.add(ir::Impact::Audible, &feature, &p.name);
                    }
                }
                m => {
                    let kind = match m {
                        Modulator::Flex(_)
                        | Modulator::Lfo(_)
                        | Modulator::StepMod(_)
                        | Modulator::Ahdsr(_)
                        | Modulator::Dbd(_) => "timed",
                        Modulator::Glide(_) => "glide",
                        Modulator::Follower(_) => "envelope follower",
                        Modulator::Other { .. } => "other",
                    };
                    for t in &p.targets {
                        let to = if t.slot.is_some() { "module " } else { "" };
                        let feature = format!("internal modulator {kind} -> {to}{}", t.param);
                        notes.add(ir::Impact::Audible, &feature, &p.name);
                    }
                }
            }
            if p.targets.iter().any(knob_route) {
                knobs.push((n, p));
            }
        }
    }
    knob_routes(&mut out, &knobs, &timed_slots, notes);
    external(
        group,
        group_index,
        &timed_slots,
        release_counter,
        &mut out,
        notes,
    )?;
    Ok(Translated {
        group: out,
        criteria: v.start_criteria.items,
    })
}

/// Start options: key switches, controller ranges and round robins. Cycles count
/// per instrument; their length is the largest stored position plus one.
fn start_rules(groups: &mut [ir::Group], criteria: &[Vec<StartCriteriaParams>], notes: &mut Notes) {
    const ON_KEY: i32 = 1;
    const ON_CONTROLLER: i32 = 2;
    const CYCLE: i32 = 3;
    const RANDOM: i32 = 4;
    let midi = |v: i16| u8::try_from(v).ok().filter(|v| *v < 128);
    let rows = || criteria.iter().flatten();
    let cycle = rows()
        .filter(|c| c.mode == CYCLE)
        .map(|c| c.cycle_class + 1)
        .max()
        .unwrap_or(0);
    // One draw over every random row of the instrument (binary [ref]).
    let randoms = rows().filter(|c| c.mode == RANDOM).count();
    let mut random_position: usize = 0;
    for (group, rows) in groups.iter_mut().zip(criteria) {
        // A row's next_criteria joins it to the next active row.
        // Native Kontakt compiles start criteria into an unparenthesized KSP `if(not ...)`
        // where `and` binds tighter than `or`, as `Behaviour::starts` evaluates it.
        let mut join = ir::Join::And;
        for c in rows {
            if c.sequencer_only {
                notes.add(
                    ir::Impact::Audible,
                    "start option sequencer_only",
                    format!("group {} mode {}", group.name, c.mode),
                );
            }
            let rule = match c.mode {
                0 => continue,
                ON_KEY => match (midi(c.key_min), midi(c.key_max)) {
                    (Some(lo), Some(hi)) if lo <= hi => ir::StartRule::Keyswitch { range: lo..=hi },
                    _ => continue,
                },
                ON_CONTROLLER => match (midi(c.controller), midi(c.cc_min), midi(c.cc_max)) {
                    (Some(cc), Some(lo), Some(hi)) if lo <= hi => {
                        ir::StartRule::Controller { cc, range: lo..=hi }
                    }
                    _ => continue,
                },
                CYCLE => ir::StartRule::RoundRobin {
                    scope: ir::RrScope::Global,
                    position: u8::try_from(c.cycle_class).unwrap_or(0),
                    length: u8::try_from(cycle).unwrap_or(u8::MAX),
                    random: false,
                    no_repeat: false,
                    release_redraws: false,
                },
                RANDOM => {
                    let pos = random_position;
                    random_position += 1;
                    // Binary [ref] (KONTAKT_START_CONDITIONS.md:60):
                    // uniform draw with no no-repeat check and fresh draw on release.
                    ir::StartRule::RoundRobin {
                        scope: ir::RrScope::Global,
                        position: u8::try_from(pos).unwrap_or(u8::MAX),
                        length: u8::try_from(randoms).unwrap_or(u8::MAX),
                        random: true,
                        no_repeat: false,
                        release_redraws: true,
                    }
                }
                other => {
                    notes.add(ir::Impact::Audible, "start option", format!("mode {other}"));
                    continue;
                }
            };
            let this = if group.start.is_empty() {
                ir::Join::And
            } else {
                join
            };
            group.start.push((this, rule));
            join = match c.next_criteria {
                1 => ir::Join::AndNot,
                2 => ir::Join::Or,
                _ => ir::Join::And,
            };
        }
    }
}

/// Unmodeled `BParSrcMode` values (`docs/re/engine/k-beat-s1200-mp60.md:10-11`):
/// `6` = S1200 Machine, `7` = MP60 Machine.
fn unsupported_source_mode(mode: u32) -> Option<&'static str> {
    Some(match mode {
        1 => "Tone Machine source",
        4 => "Beat Machine source",
        6 => "S1200 source",
        7 => "MP60 source",
        _ => return None,
    })
}

fn translate_zone(
    z: &ni_file::kontakt::objects::ZoneParams,
    loops: Option<&LoopArray>,
    sample: usize,
    frames: u64,
    reverse: bool,
    streaming: bool,
    notes: &mut Notes,
) -> Option<ir::Zone> {
    let midi = |v: i16| u8::try_from(v.clamp(0, 127)).unwrap_or(0);
    let (lk, hk, lv, hv) = (z.low_key, z.high_key, z.low_velocity, z.high_velocity);
    if lk > 127 || hk < 0 || lv > 127 || hv < 1 || lk > hk || lv > hv {
        return None; // No note can reach it.
    }
    let start = u64::try_from(z.sample_start).unwrap_or(0);
    // Kontakt stores the end relative to the end of the sample.
    let end = frames.saturating_sub(u64::from(z.sample_end.unsigned_abs()));
    if end <= start {
        notes.add(
            ir::Impact::Cosmetic,
            "empty zone",
            format!("{start}..{end}"),
        );
        return None;
    }
    // Mode 1 is until end, mode 2 until release (docs/re/engine/BASIC_BEHAVIORS.md
    // row 16; KSP's $LOOP_PAR_MODE numbers differ, docs-txt/ksp.txt:12944).
    let mut found = Vec::new();
    for l in loops.into_iter().flat_map(|l| &l.items) {
        match l.mode {
            0 => continue,
            1 | 2 if l.loop_length > 0 && l.loop_start >= 0 => {}
            1 | 2 => {
                let span = format!("{}+{}", l.loop_start, l.loop_length);
                notes.add(ir::Impact::Audible, "empty loop", span);
                continue;
            }
            mode => {
                notes.add(ir::Impact::Audible, "loop mode", format!("mode {mode}"));
                continue;
            }
        }
        let from = l.loop_start as u64;
        // Kontakt stores `loop_tuning` directly as a linear pitch ratio
        // ([ref], multiplied into playback step at [ref]).
        let tune_ratio = if l.loop_tuning > 0.0 && l.loop_tuning.is_finite() {
            l.loop_tuning
        } else {
            1.0
        };
        found.push(ir::Loop {
            start: from,
            end: from + l.loop_length as u64,
            crossfade: l.x_fade_length.max(0) as u64,
            count: u32::try_from(l.loop_count.max(0)).unwrap_or(0),
            alternating: l.alternating_loop,
            until_release: l.mode == 2,
            tune_ratio,
        });
    }
    // Preserve LoopArray slot order, matching Kontakt's loop-slot walk.
    // Sentinel `sample_start_mod_range < 0`: full zone
    // span when non-streaming (Sampler), 0 when streaming (DFD).
    let start_mod = if z.sample_start_mod_range < 0 {
        if streaming { 0 } else { end - start }
    } else {
        z.sample_start_mod_range as u64
    };
    Some(ir::Zone {
        id: 0,
        mix: Vec::new(),
        sample,
        keys: midi(lk)..=midi(hk),
        velocities: midi(lv).max(1)..=midi(hv),
        velocity_fade: (midi(z.fade_low_velocity), midi(z.fade_high_velocity)),
        key_fade: (midi(z.fade_low_key), midi(z.fade_high_key)),
        crossfades: Vec::new(),
        root: midi(z.root_key),
        tune: laws::semitones(z.zone_tune),
        volume_db: db(z.zone_volume),
        pan: z.zone_pan.clamp(-1.0, 1.0),
        start,
        end,
        start_mod,
        loops: found,
        reverse,
        slot: 0,
        envelopes: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use kontra_ir as ir;

    #[test]
    fn malformed_muted_loops_are_diagnosed_and_active_loops_fail() {
        let chunk = ni_file::kontakt::Chunk {
            id: super::LOOPS,
            data: vec![1, 0, 0x61, 0],
        };
        let mut notes = kontra_format::Notes::default();
        assert!(
            super::zone_loops(Some(&chunk), true, &mut notes)
                .unwrap()
                .is_none()
        );
        assert!(
            notes
                .finish()
                .iter()
                .any(|d| d.feature == "muted zone loops")
        );
        assert!(
            super::zone_loops(Some(&chunk), false, &mut kontra_format::Notes::default()).is_err()
        );
    }

    #[test]
    fn empty_and_unknown_loop_modes_are_audible_notes() {
        let z = ni_file::kontakt::objects::ZoneParams {
            sample_start: 0,
            sample_end: 0,
            sample_start_mod_range: 0,
            low_velocity: 1,
            high_velocity: 127,
            low_key: 0,
            high_key: 127,
            fade_low_velocity: 0,
            fade_high_velocity: 0,
            fade_low_key: 0,
            fade_high_key: 0,
            root_key: 60,
            zone_volume: 1.0,
            zone_pan: 0.0,
            zone_tune: 1.0,
            sample_present: true,
            filename_id: 0,
            filename_prefix: None,
            sample_data_type: 0,
            sample_rate: 48_000,
            num_channels: 2,
            num_frames: 1000,
            reserved1: 0,
            reserved2: None,
            root_note: 60,
            tuning: 0.0,
            reserved3: 0,
            reserved4: 0,
            unknown_tail: Vec::new(),
        };
        let entry = |mode, loop_start, loop_length| ni_file::kontakt::objects::Loop {
            mode,
            loop_start,
            loop_length,
            loop_count: 0,
            alternating_loop: false,
            loop_tuning: 0.0,
            x_fade_length: 0,
        };
        let loops = ni_file::kontakt::objects::LoopArray {
            mask: 0b0011_1111,
            slots: vec![0, 1, 2, 3, 4, 5],
            items: vec![
                entry(0, 0, 0),    // disabled: ignored silently
                entry(1, 10, 50),  // valid until-end
                entry(2, 100, 40), // valid until-release
                entry(1, 10, 0),   // empty length -> Audible note
                entry(2, -5, 20),  // negative start -> Audible note
                entry(3, 10, 20),  // unknown mode -> Audible note
            ],
        };
        let mut notes = kontra_format::Notes::default();
        let zone = super::translate_zone(&z, Some(&loops), 0, 1000, false, false, &mut notes)
            .expect("zone translated");
        assert_eq!(zone.loops.len(), 2);
        assert!(!zone.loops[0].until_release);
        assert!(zone.loops[1].until_release);
        let mut found: Vec<_> = notes
            .finish()
            .into_iter()
            .map(|d| (d.impact, d.feature, d.detail))
            .collect();
        found.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            found,
            [
                (
                    ir::Impact::Audible,
                    "empty loop".into(),
                    "2× (first: 10+0)".into()
                ),
                (
                    ir::Impact::Audible,
                    "loop mode".into(),
                    "1× (first: mode 3)".into()
                ),
            ]
        );
    }

    /// A script that only reads a saved module's control, with no
    /// `$ENGINE_PAR_EFFECT_` anywhere, still gets the saved value and the
    /// slot's type (Conflux reads `$ENGINE_PAR_BITE_BITS`).
    #[test]
    fn a_reading_script_gets_the_saved_controls_without_loading_types() {
        let mut out = ir::Instrument::DEFAULT;
        out.scripts.push(ir::Script {
            name: String::new(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source: "on init message(get_engine_par($ENGINE_PAR_BITE_BITS, -1, 0, 1)) end on"
                .into(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
            container: None,
        });
        let bits = ir::held_key("$ENGINE_PAR_BITE_BITS");
        let slots = crate::effects::Slots {
            types: vec![(ir::RackAt::Inserts, 0, 52)],
            held: vec![(ir::RackAt::Inserts, 0, bits, 8)],
            ..Default::default()
        };
        super::slot_shapes(&mut out, slots, &mut []);
        assert_eq!(out.slot_shapes.held, [(ir::RackAt::Inserts, 0, bits, 8)]);
        assert_eq!(out.slot_shapes.saved_types, [(ir::RackAt::Inserts, 0, 52)]);
    }

    #[test]
    fn negative_start_mod_range_sentinel_and_loop_order_and_tuning() {
        let z = ni_file::kontakt::objects::ZoneParams {
            sample_start: 100,
            sample_end: -100,
            sample_start_mod_range: -1,
            low_velocity: 1,
            high_velocity: 127,
            low_key: 0,
            high_key: 127,
            fade_low_velocity: 0,
            fade_high_velocity: 0,
            fade_low_key: 0,
            fade_high_key: 0,
            root_key: 60,
            zone_volume: 1.0,
            zone_pan: 0.0,
            zone_tune: 1.0,
            sample_present: true,
            filename_id: 0,
            filename_prefix: None,
            sample_data_type: 0,
            sample_rate: 48_000,
            num_channels: 2,
            num_frames: 1000,
            reserved1: 0,
            reserved2: None,
            root_note: 60,
            tuning: 0.0,
            reserved3: 0,
            reserved4: 0,
            unknown_tail: Vec::new(),
        };
        let loops = ni_file::kontakt::objects::LoopArray {
            mask: 0b0000_0011,
            slots: vec![0, 1],
            items: vec![
                // Slot 0 ends LATER than slot 1; slot order must be preserved.
                ni_file::kontakt::objects::Loop {
                    mode: 1,
                    loop_start: 500,
                    loop_length: 200,
                    loop_count: 2,
                    alternating_loop: false,
                    loop_tuning: 2.0,
                    x_fade_length: 0,
                },
                ni_file::kontakt::objects::Loop {
                    mode: 1,
                    loop_start: 150,
                    loop_length: 50,
                    loop_count: 0,
                    alternating_loop: false,
                    loop_tuning: 0.5,
                    x_fade_length: 0,
                },
            ],
        };
        let mut notes = kontra_format::Notes::default();
        let sampler = super::translate_zone(&z, Some(&loops), 0, 1000, false, false, &mut notes)
            .expect("sampler zone");
        assert_eq!(
            sampler.start_mod, 800,
            "-1 sentinel -> end - start in Sampler"
        );
        assert_eq!(sampler.loops.len(), 2);
        assert_eq!((sampler.loops[0].start, sampler.loops[0].end), (500, 700));
        assert_eq!((sampler.loops[1].start, sampler.loops[1].end), (150, 200));
        assert_eq!(sampler.loops[0].tune_ratio, 2.0);
        assert_eq!(sampler.loops[1].tune_ratio, 0.5);

        let dfd = super::translate_zone(&z, Some(&loops), 0, 1000, false, true, &mut notes)
            .expect("dfd zone");
        assert_eq!(dfd.start_mod, 0, "-1 sentinel -> 0 in DFD streaming");
    }

    #[test]
    fn kontakt_any_uses_oldest_without_released_preference() {
        let vg = super::voice_group(&ni_file::kontakt::objects::VoiceLimit {
            name: String::new(),
            max_num_voices: 4,
            kill_mode: 0,
            prefer_released: false,
            ms_fade_time: 10,
            exclusion_group: -1,
        });
        assert_eq!(vg.steal, ir::Steal::Oldest);
        assert!(!vg.prefer_released);
    }

    #[test]
    fn program_ignored_fields_fast_path_matches_debug_reflection() {
        for group_solo in [false, true] {
            let p = ni_file::kontakt::objects::ProgramPublicParams {
                name: "p".into(),
                volume: 0.5,
                tune: 1.0,
                high_key: 127,
                high_velocity: 127,
                library_id: 7,
                group_solo,
                instrument_author: "a".into(),
                resource_container_filename: Some(2),
                unknown_tail: vec![1, 2],
                ..Default::default()
            };
            let mut fast = kontra_format::Notes::default();
            super::check_ignored_program_fields(&mut fast, &p);
            let mut slow = kontra_format::Notes::default();
            slow.ignored_fields("program", &p, super::USED_PROGRAM, super::COSMETIC_PROGRAM);
            assert_eq!(fast.finish(), slow.finish());
        }
    }

    #[test]
    fn zone_ignored_fields_fast_path_matches_debug_reflection() {
        for tail in [Vec::new(), vec![1, 2, 3]] {
            let z = ni_file::kontakt::objects::ZoneParams {
                sample_start: 10,
                sample_end: 500,
                sample_present: true,
                filename_id: 3,
                filename_prefix: Some([1; 6]),
                root_note: 60,
                unknown_tail: tail,
                ..Default::default()
            };
            let mut fast = kontra_format::Notes::default();
            super::check_ignored_zone_fields(&mut fast, &z);
            let mut slow = kontra_format::Notes::default();
            slow.ignored_fields("zone", &z, super::USED_ZONE, super::COSMETIC_ZONE);
            assert_eq!(fast.finish(), slow.finish());
        }
    }

    #[test]
    fn group_ignored_fields_fast_path_matches_debug_reflection() {
        for (midi_channel, interp_quality, tail) in [
            (0, 0, Vec::new()),
            (-1, -1, Vec::new()),
            (3, 2, vec![1, 2]),
            (-2, 7, Vec::new()),
        ] {
            let v = ni_file::kontakt::objects::GroupParams {
                name: "g".into(),
                volume: 0.5,
                pan: 0.1,
                tune: 1.0,
                key_tracking: true,
                reverse: false,
                release_trigger: true,
                release_trigger_note_monophonic: true,
                rls_trig_counter: 4,
                midi_channel,
                voice_group_index: -1,
                fx_idx_amp_split_point: 8,
                muted: false,
                soloed: false,
                interp_quality,
                start_criteria: ni_file::kontakt::objects::StartCriteriaList {
                    mask: 1,
                    ..Default::default()
                },
                unknown_tail: tail,
            };
            let mut fast = kontra_format::Notes::default();
            super::check_ignored_group_fields(&mut fast, &v);
            let mut slow = kontra_format::Notes::default();
            slow.ignored_fields("group", &v, super::USED_GROUP, super::COSMETIC_GROUP);
            assert_eq!(fast.finish(), slow.finish());
        }
    }

    #[test]
    fn finish_reports_invalid_references() {
        let out = ir::Instrument {
            groups: vec![ir::Group {
                voice_group: Some(2),
                ..ir::Group::DEFAULT
            }],
            ..ir::Instrument::DEFAULT
        };
        let mut notes = kontra_format::Notes::default();
        notes.ignored("kept", 1);
        let out = super::finish(out, notes);
        let features: Vec<_> = out.diagnostics.iter().map(|d| d.feature.as_str()).collect();
        assert!(features.contains(&"ignored kept"), "{features:?}");
        assert!(
            features.contains(&"ir.group.voice_group_out_of_bounds"),
            "{features:?}"
        );
    }

    #[test]
    fn start_rules_ignore_skipped_join_handle_many_randoms_and_note_sequencer_only() {
        let row = |mode, next_criteria, key_min, key_max, sequencer_only| {
            ni_file::kontakt::objects::StartCriteriaParams {
                mode,
                next_criteria,
                key_min,
                key_max,
                controller: 0,
                cc_min: 0,
                cc_max: 0,
                cycle_class: 0,
                sequencer_only,
                slice_zone_idx: 0,
                slice_zone_slice_idx: 0,
            }
        };
        let mut groups = vec![ir::Group::DEFAULT];
        let mut notes = kontra_format::Notes::default();
        // Leading disabled row (mode 0) with next_criteria = 2 (Or) must not leak Or
        // onto the first active row; sequencer_only must emit a diagnostic.
        let criteria = vec![vec![
            row(0, 2, 0, 0, false),
            row(1, 0, 36, 36, true),
            row(0, 2, 0, 0, false),
            row(1, 0, 38, 38, false),
        ]];
        super::start_rules(&mut groups, &criteria, &mut notes);
        assert_eq!(
            groups[0].start,
            vec![
                (ir::Join::And, ir::StartRule::Keyswitch { range: 36..=36 }),
                (ir::Join::And, ir::StartRule::Keyswitch { range: 38..=38 }),
            ]
        );
        assert!(
            notes
                .finish()
                .iter()
                .any(|d| d.feature == "start option sequencer_only")
        );

        // > 255 RANDOM rows must not overflow u8.
        let mut many_groups = vec![ir::Group::DEFAULT; 260];
        let many_criteria = vec![vec![row(4, 0, 0, 0, false)]; 260];
        super::start_rules(
            &mut many_groups,
            &many_criteria,
            &mut kontra_format::Notes::default(),
        );
        assert_eq!(
            many_groups[259].start[0].1,
            ir::StartRule::RoundRobin {
                scope: ir::RrScope::Global,
                position: u8::MAX,
                length: u8::MAX,
                random: true,
                no_repeat: false,
                release_redraws: true,
            }
        );
    }

    #[test]
    fn kt11_s1200_and_mp60_source_modes_match_binary_ids() {
        // Binary BParSrcMode (docs/re/engine/k-beat-s1200-mp60.md:10-11):
        // 1 = Tone Machine, 4 = Beat Machine, 6 = S1200 Machine, 7 = MP60 Machine.
        assert_eq!(
            super::unsupported_source_mode(1),
            Some("Tone Machine source")
        );
        assert_eq!(
            super::unsupported_source_mode(4),
            Some("Beat Machine source")
        );
        assert_eq!(super::unsupported_source_mode(6), Some("S1200 source"));
        assert_eq!(super::unsupported_source_mode(7), Some("MP60 source"));
        assert_eq!(super::unsupported_source_mode(0), None);
        assert_eq!(super::unsupported_source_mode(3), None);
    }

    #[test]
    fn ascii_ci_script_helpers_match_uppercase_ksp() {
        assert!(super::contains_ascii_ci(
            "LOAD_IR_SAMPLE(\"hall\")",
            "load_ir_sample"
        ));
        assert!(super::contains_ascii_ci(
            "set_engine_par($engine_par_effect_type, 1, -1, 0, 1)",
            "$ENGINE_PAR_EFFECT_"
        ));
        assert_eq!(
            super::split_ascii_ci("LOAD_ARRAY(%Tbl, 2)", "load_array("),
            vec!["%Tbl, 2)"]
        );
    }

    #[test]
    fn group_dynamics_zero_is_silent_and_nonzero_or_malformed_is_diagnosed() {
        let group_with_dynamics = |body: Vec<u8>| {
            let mut private = Vec::new();
            // Minimal valid v0x74 Group private payload (name "G", defaults).
            let name = "G";
            private.extend_from_slice(&(name.len() as u32).to_le_bytes());
            for ch in name.encode_utf16() {
                private.extend_from_slice(&ch.to_le_bytes());
            }
            private.extend_from_slice(&1.0f32.to_le_bytes()); // volume
            private.extend_from_slice(&0.0f32.to_le_bytes()); // pan
            private.extend_from_slice(&1.0f32.to_le_bytes()); // tune
            private.extend_from_slice(&[1, 0, 0, 0, 0]); // key_tracking..
            private.extend_from_slice(&[0u8; 64]); // pad for optional fields
            let mut dyn_data = vec![0];
            dyn_data.extend_from_slice(&0x10u16.to_le_bytes());
            dyn_data.extend_from_slice(&body);
            ni_file::kontakt::objects::Group(ni_file::kontakt::StructuredObject {
                version: 0x74,
                private_data: private,
                public_data: Vec::new(),
                children: vec![ni_file::kontakt::Chunk {
                    id: super::GROUP_DYNAMICS,
                    data: dyn_data,
                }],
            })
        };
        let zero = group_with_dynamics(vec![0u8; 64 * 4]);
        assert!(zero.dynamics().unwrap().is_some_and(|d| !d.has_nonzero()));

        let mut nz = vec![0u8; 64 * 4];
        nz[0..4].copy_from_slice(&0.5f32.to_le_bytes());
        let nonzero = group_with_dynamics(nz);
        assert!(nonzero.dynamics().unwrap().is_some_and(|d| d.has_nonzero()));

        let malformed = group_with_dynamics(vec![0u8; 16]);
        assert!(malformed.dynamics().is_err());
    }
}
