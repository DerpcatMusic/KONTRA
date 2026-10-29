//! A Kontakt program as the instrument model: groups, zones, start rules,
//! envelopes and scripts. Kontakt stores gains and tunes as ratios.

use std::collections::HashMap;
use std::path::Path;

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::KontaktChunks;
use ni_file::kontakt::objects::{
    BParScript, Group, GroupList, InternalModArray16, InternalModParams, LoopArray, Modulator,
    StartCriteriaParams, VoiceGroups, VoiceLimit, ZoneList,
};

use crate::Error;
use crate::external::external;
use crate::laws;
use crate::resources::Resources;
use crate::samples::Resolver;

const GROUP_LIST: u16 = 0x33;
const ZONE_LIST: u16 = 0x34;
const LOOPS: u16 = 0x39;
const INTERNAL_MODS: u16 = 0x3b;
const SCRIPT: u16 = 6;
const VOICE_GROUPS: u16 = 0x32;

/// `saved`: a snapshot's persistent values per script slot, in place of the
/// instrument's own where a slot has some.
pub(crate) fn instrument(
    path: &Path,
    chunks: &KontaktChunks,
    monolith: Option<ni_file::file_container::NIFileContainer>,
    saved: &[Vec<String>],
) -> Result<ir::Instrument, Error> {
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let program = chunks
        .program()
        .ok_or_else(|| Error::new(path, "program", "missing"))?
        .map_err(decode("program"))?;
    let params = program.params().map_err(decode("program parameters"))?;
    let mut notes = Notes::default();
    let mut out = ir::Instrument {
        name: params.name.clone(),
        voices: 256,
        output: ir::Output {
            volume_db: db(params.volume),
            pan: params.pan.clamp(-1.0, 1.0),
            controllers: output_controllers(program.version(), &program.0.private_data),
            ..ir::Output::UNITY
        },
        ..ir::Instrument::DEFAULT
    };
    // Its pitch modulation's 2^x is the quadratic at 0x140832440: the
    // mantissa `f − 0.33971·(f − f²)` (0x144f39240) on the exponent
    // `n + 127`. Measured on a ±2 st pitch-bend route: +1 → 1.954 st,
    // −1 → −1.958, ±¼ → 0.480/−0.485, within 0.02 cents of this.
    out.route_pitch = ir::Exp2::Quadratic { c: 0.33971 };
    // The instrument's tune applies to every group.
    let base = Base {
        tune: semitones(params.tune) + f64::from(params.transpose),
        preload: params.dfd_channel_preload_size,
    };
    let mut resources = None;
    let links = crate::links::links(&program.0.private_data);
    for (slot, chunk) in (program.0.children.iter().filter(|c| c.id == SCRIPT)).enumerate() {
        let script = BParScript::try_from(chunk)
            .and_then(|s| s.params())
            .map_err(decode("script"))?;
        match script.text {
            _ if script.bypass => {}
            Some(text) if !text.trim().is_empty() => out.scripts.push(ir::Script {
                name: script.description.unwrap_or_default(),
                dialect: ir::Dialect::Ksp,
                data: data_files(&text, path, &mut resources),
                source: text,
                saved: (saved.get(slot).filter(|s| !s.is_empty()).cloned())
                    .unwrap_or(script.persistent),
                links: (links.iter())
                    .filter(|(s, _)| usize::from(*s) == slot)
                    .map(|&(_, l)| l)
                    .collect(),
            }),
            _ => {
                if let Some(file) = script.textfile_name.filter(|n| !n.is_empty()) {
                    notes.add(ir::Impact::Broken, "linked script file", file);
                }
            }
        }
    }
    let groups = program
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
    for (index, group) in groups.groups.iter().enumerate() {
        let t = translate_group(group, index, &base, &mut notes)
            .map_err(|e| Error::new(path, "group", format!("{index}: {e}")))?;
        let mut g = t.group;
        g.inserts = crate::effects::group(group, index, &mut impulses, &mut notes, &mut slots);
        crate::drive::unmoved(&g.inserts, &g.mods, &mut notes);
        // ponytail: a filter a modulator moves passes unfiltered until filters
        // run per voice; its routes are reported.
        for slot in t.modulated {
            let insert = g
                .inserts
                .get_mut(usize::from(slot))
                .and_then(Option::as_mut);
            if let Some(e) = insert.filter(|e| matches!(e.kind, ir::EffectKind::Filter(_))) {
                e.kind = ir::EffectKind::Other;
            }
        }
        out.groups.push(g);
        criteria.push(t.criteria);
    }
    let racks = crate::effects::program(&program, &mut impulses, &mut notes);
    let mut buses = racks.buses;
    (out.inserts, out.sends, out.main) = (racks.inserts, racks.sends, racks.main);
    slots.laws.extend(racks.laws.laws);
    slots.shape_slots.extend(racks.laws.shape_slots);
    slots.subtypes.extend(racks.laws.subtypes);
    out.slot_laws = slots.laws;
    if !slots.shape_slots.is_empty() {
        out.slot_shapes.shapes = crate::laws::filter_shapes();
        out.slot_shapes.slots = slots.shape_slots;
    }
    let loads = |s: &ir::Script| s.source.contains("$ENGINE_PAR_EFFECT_");
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
        for (b, bus) in (0u8..).zip(&buses) {
            open.extend(empty(ir::RackAt::Bus(b), &bus.inserts));
        }
        if !open.is_empty() && out.slot_shapes.shapes.is_empty() {
            out.slot_shapes.shapes = crate::laws::filter_shapes();
        }
        out.slot_shapes.slots.extend(open);
        out.slot_shapes.types = crate::script_knobs::slot_types();
        out.slot_shapes.subtypes = std::mem::take(&mut slots.subtypes);
    }
    // A script may load any response of the library's `ir_samples` by name.
    if out
        .scripts
        .iter()
        .any(|s| s.source.contains("load_ir_sample"))
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
    let muted: Vec<bool> = groups
        .groups
        .iter()
        .map(|g| g.params().map(|p| p.muted).unwrap_or(true))
        .collect();
    let zones = program
        .0
        .find_first(ZONE_LIST)
        .ok_or_else(|| Error::new(path, "zone list", "missing"))
        .and_then(|c| ZoneList::try_from(c).map_err(decode("zone list")))?;
    let mut sample_of: HashMap<u32, Option<usize>> = HashMap::new();
    for (zone, &group) in zones.zones().iter().zip(&zones.group_ids) {
        let z = zone.params().map_err(decode("zone"))?;
        if !z.sample_present || muted.get(group as usize).copied().unwrap_or(true) {
            continue;
        }
        let id = u32::try_from(z.filename_id).unwrap_or(u32::MAX);
        let sample = *sample_of.entry(id).or_insert_with(|| {
            let name = table.get(&id)?;
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
                    });
                    Some(out.samples.len() - 1)
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
        let loops = zone
            .0
            .find_first(LOOPS)
            .map(LoopArray::try_from)
            .transpose();
        let loops = loops.map_err(decode("zone loops"))?;
        let reverse =
            g.zones.first().is_some_and(|z| z.reverse) || criteria_reverse(group, &groups);
        if let Some(z) = translate_zone(&z, loops.as_ref(), sample, frames, reverse, &mut notes) {
            g.zones.push(z);
        }
    }
    let first = out.samples.len();
    for i in &mut out.impulses {
        i.sample += first;
    }
    for (_, sample) in &mut out.responses {
        *sample += first;
    }
    out.samples.extend(responses);
    out.diagnostics = notes.finish();
    Ok(out)
}

/// "Accept standard controllers for Volume and Pan" (MIDI CC 7 and 10; the
/// program's private byte 4) and the top of the CC 7 range in dB (an i32 at
/// byte 6): Kontakt 8.13's instrument options saved and read back, program
/// versions 0xa8..=0xb5 (400 corpus instruments: all on, top 0 dB). Earlier
/// versions keep those defaults. Measured: CC 7 sets the fader to
/// `top·(v/127)³` in place of the saved volume, CC 10 the balance
/// `(v − 63.5)/63.5`; a script's `ignore_controller` stops both.
fn output_controllers(version: u16, private: &[u8]) -> Option<ir::OutputControllers> {
    let saved = (version >= 0xa8).then(|| {
        let top = private.get(6..10).and_then(|b| b.try_into().ok());
        (private.get(4).copied(), top.map(i32::from_le_bytes))
    });
    let (on, top_db) = match saved {
        Some((Some(on), Some(top))) => (on != 0, top as f32),
        _ => (true, 0.0),
    };
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
    20.0 * linear.max(1e-6).log10()
}

/// Slots in each Kontakt effect rack.
const SLOTS: usize = 8;
fn semitones(ratio: f32) -> f64 {
    12.0 * f64::from(ratio.max(1e-6)).log2()
}

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
fn voice_groups(out: &mut ir::Instrument, groups: &GroupList, limits: Option<&VoiceGroups>) {
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
        // 0 is "Any": the oldest, as Kontakt's default.
        steal: match v.kill_mode {
            2 => ir::Steal::Newest,
            3 => ir::Steal::Highest,
            4 => ir::Steal::Lowest,
            _ => ir::Steal::Oldest,
        },
        prefer_released: v.prefer_released,
        fade_ms: v.ms_fade_time.max(0) as f32,
        exclusive: u8::try_from(v.exclusion_group).ok().filter(|&s| s < 16),
    }
}

/// The library data files a script loads with a literal `load_array(%x, 2)`:
/// `Resources/data/x.nka`, by lowercase array name without its sigil.
fn data_files(
    source: &str,
    path: &Path,
    resources: &mut Option<Resources>,
) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for call in source.split("load_array(").skip(1) {
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
    for call in source.split("load_performance_view(").skip(1) {
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
    files
}

struct Translated {
    group: ir::Group,
    criteria: Vec<StartCriteriaParams>,
    /// Insert slots whose cutoff a route not carried moves.
    modulated: Vec<u8>,
}

fn translate_group(
    group: &Group,
    group_index: usize,
    base: &Base,
    notes: &mut Notes,
) -> Result<Translated, ni_file::Error> {
    let v = group.params()?;
    let mut out = ir::Group {
        name: v.name.clone(),
        volume_db: db(v.volume),
        pan: v.pan.clamp(-1.0, 1.0),
        tune: semitones(v.tune) + base.tune,
        keytrack: if v.key_tracking { 1.0 } else { 0.0 },
        trigger: if v.release_trigger {
            ir::Trigger::Release
        } else {
            ir::Trigger::Attack
        },
        purged: group.source_identity().is_ok_and(|s| s.flag == 1),
        stream: laws::stream(group.source_identity().map_or(0, |s| s.mode), base.preload),
        ..ir::Group::DEFAULT
    };
    if let Ok(Some(source)) = group.wavetable_source() {
        out.wavetable = Some(crate::wavetable::wavetable(&source, &v.name, notes));
        // A voice reads anywhere in its table at once.
        out.stream = None;
    }
    let slot = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    let mut modulated = Vec::new();
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
    if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
        for (n, m) in InternalModArray16::try_from(chunk)?.slots()? {
            let p = m.params()?;
            out.routes.push(ir::Routes {
                slot: slot(n),
                name: p.name.clone(),
                names: p.targets.iter().map(|t| t.name.clone()).collect(),
            });
            let volume = p
                .targets
                .iter()
                .any(|t| t.param == "volume" && t.slot.is_none());
            match &p.modulator {
                // The first volume AHDSR is the amplitude envelope; others route
                // as breakpoint envelopes (`timed`).
                Modulator::Ahdsr(e)
                    if volume
                        && !out
                            .mods
                            .iter()
                            .any(|m| matches!(m.kind, ir::ModKind::Envelope)) =>
                {
                    out.envelope = laws::envelope(e);
                    kontra_law::record(|| kontra_law::Application {
                        id: kontra_law::LawId("kontakt.ahdsr_envelope.v1"),
                        location: format!("groups/{group_index}/envelope"),
                        input: format!("{e:?}"),
                        output: format!("{:?}", out.envelope),
                    });
                    let target = p.targets.iter().position(|t| t.param == "volume");
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: slot(n),
                        target: target.map_or(0, slot),
                        bypass: false,
                        kind: ir::ModKind::Envelope,
                    });
                }
                Modulator::Flex(_) | Modulator::Lfo(_) | Modulator::Ahdsr(_) => {
                    let (lfos, envelopes) = (out.lfos.len(), out.envelopes.len());
                    timed(&mut out, &p, slot(n), notes, &mut modulated);
                    let made = if out.lfos.len() > lfos {
                        u8::try_from(lfos).ok().map(ir::Source::Lfo)
                    } else if out.envelopes.len() > envelopes {
                        u8::try_from(envelopes).ok().map(ir::Source::Envelope)
                    } else {
                        None
                    };
                    timed_slots.extend(made.map(|s| (n, s)));
                }
                m => {
                    let kind = match m {
                        Modulator::Flex(_) | Modulator::Lfo(_) | Modulator::Ahdsr(_) => "timed",
                        Modulator::Dbd(_) => "DBD envelope",
                        Modulator::Glide(_) => "glide",
                        Modulator::Follower(_) => "envelope follower",
                        Modulator::Other { .. } => "other",
                    };
                    for t in &p.targets {
                        if t.param == "filterCutoff" {
                            modulated.extend(t.slot);
                        }
                        let to = if t.slot.is_some() { "module " } else { "" };
                        let feature = format!("internal modulator {kind} -> {to}{}", t.param);
                        notes.add(ir::Impact::Audible, &feature, &p.name);
                    }
                }
            }
        }
    }
    external(
        group,
        &timed_slots,
        release_counter,
        &mut out,
        notes,
        &mut modulated,
    )?;
    Ok(Translated {
        group: out,
        criteria: v.start_criteria.items,
        modulated,
    })
}

/// Routes an envelope (flex, or an AHDSR that is not the amplitude envelope)
/// or an LFO to volume, pitch, pan and filter cutoff; reports the targets and
/// shapes not mapped, and in `unmapped` the slots of cutoffs it drops.
fn timed(
    out: &mut ir::Group,
    p: &InternalModParams,
    slot: u16,
    notes: &mut Notes,
    unmapped: &mut Vec<u8>,
) {
    // The modulator's flags: [router open, bypass, retrigger, unknown].
    const BYPASS: usize = 1;
    const RETRIGGER: usize = 2;
    let report = |notes: &mut Notes, unmapped: &mut Vec<u8>, what: &str| {
        for t in &p.targets {
            if t.param == "filterCutoff" {
                unmapped.extend(t.slot);
            }
            let feature = format!("internal modulator {what} -> {}", t.param);
            notes.add(ir::Impact::Audible, &feature, &p.name);
        }
    };
    let mut envelope = |e: ir::Breakpoints| {
        let n = u8::try_from(out.envelopes.len()).ok()?;
        out.envelopes.push(e);
        Some(ir::Source::Envelope(n))
    };
    let (source, what) = match &p.modulator {
        Modulator::Flex(e) => (envelope(laws::flex(e)), "flex envelope"),
        Modulator::Ahdsr(e) => (envelope(laws::ahdsr(e)), "AHDSR"),
        Modulator::Lfo(l) => {
            let clock = if p.unknown_flags.get(RETRIGGER).is_some_and(|&f| f != 0) {
                ir::Clock::Voice
            } else {
                ir::Clock::Free
            };
            let (Some(lfo), Ok(n)) = (laws::lfo(l, clock), u8::try_from(out.lfos.len())) else {
                return report(notes, unmapped, "LFO (synced fade or unknown wave)");
            };
            out.lfos.push(lfo);
            (Some(ir::Source::Lfo(n)), "LFO")
        }
        _ => return,
    };
    let Some(source) = source else {
        return report(notes, unmapped, what);
    };
    let flex = what == "flex envelope";
    let amp = out
        .mods
        .iter()
        .any(|m| matches!(m.kind, ir::ModKind::Envelope));
    for (target, t) in p.targets.iter().enumerate() {
        let kind = match (t.param.as_str(), t.slot) {
            // An LFO's volume route is linear in its unipolar swing (v2
            // sampler-core v1_voice_controls volume target, frange_uni 0x14071a840).
            ("volume", None) => ir::ModKind::Volume {
                source,
                intensity: t.signed_intensity(),
                curve: ir::Curve::Depth { power: 1 },
                // ponytail: an LFO or envelope route's lag is not read (only
                // controller routes are measured); read it if one is saved.
                tau_ms: 0.0,
            },
            ("pitch", None) => ir::ModKind::Pitch {
                source,
                semitones: laws::PITCH_SEMITONES,
                depth: t.signed_intensity(),
                tau_ms: 0.0,
            },
            ("pan", None) => ir::ModKind::Pan {
                source,
                amount: t.signed_intensity(),
            },
            ("filterCutoff", Some(insert)) => ir::ModKind::Cutoff {
                source,
                insert,
                depth: t.signed_intensity(),
            },
            ("filterQ", Some(insert)) => ir::ModKind::Resonance {
                source,
                insert,
                depth: t.signed_intensity(),
            },
            (param, Some(insert)) if laws::eq_band_index(param).is_some() => {
                match laws::eq_gain(source, insert, param, t.signed_intensity()) {
                    Some(kind) => kind,
                    None => continue,
                }
            }
            (param, Some(insert))
                if let Some(kind) =
                    crate::drive::route(source, insert, param, t.signed_intensity()) =>
            {
                kind
            }
            _ => {
                let to = if t.slot.is_some() { "module " } else { "" };
                let feature = format!("internal modulator {what} -> {to}{}", t.param);
                notes.add(ir::Impact::Audible, &feature, &p.name);
                continue;
            }
        };
        if flex
            && !amp
            && matches!(kind, ir::ModKind::Volume { .. })
            && let Some(e) = out.envelopes.last()
        {
            out.envelope = laws::flex_gate(e);
        }
        out.mods.push(ir::Mod {
            name: p.name.clone(),
            slot,
            target: u16::try_from(target).unwrap_or(u16::MAX),
            bypass: p.unknown_flags.get(BYPASS).is_some_and(|&f| f != 0),
            kind,
        });
    }
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
    // One draw over every random row of the instrument (binary 0x140876bd0).
    let randoms = rows().filter(|c| c.mode == RANDOM).count();
    let mut random_position = 0;
    for (group, rows) in groups.iter_mut().zip(criteria) {
        // A row's next_criteria joins it to the row after it. ponytail: joins
        // fold left to right; Kontakt's mixed AND/OR precedence is untraced.
        let mut join = ir::Join::And;
        for c in rows {
            let this = join;
            join = match c.next_criteria {
                1 => ir::Join::AndNot,
                2 => ir::Join::Or,
                _ => ir::Join::And,
            };
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
                    position: u8::try_from(c.cycle_class).unwrap_or(0),
                    length: u8::try_from(cycle).unwrap_or(u8::MAX),
                    random: false,
                },
                RANDOM => {
                    random_position += 1;
                    ir::StartRule::RoundRobin {
                        position: random_position - 1,
                        length: u8::try_from(randoms).unwrap_or(u8::MAX),
                        random: true,
                    }
                }
                other => {
                    notes.add(ir::Impact::Audible, "start option", format!("mode {other}"));
                    continue;
                }
            };
            group.start.push((this, rule));
        }
    }
}

fn translate_zone(
    z: &ni_file::kontakt::objects::ZoneParams,
    loops: Option<&LoopArray>,
    sample: usize,
    frames: u64,
    reverse: bool,
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
        if !matches!(l.mode, 1 | 2) || l.loop_length <= 0 || l.loop_start < 0 {
            continue;
        }
        let from = l.loop_start as u64;
        found.push(ir::Loop {
            start: from,
            end: from + l.loop_length as u64,
            crossfade: l.x_fade_length.max(0) as u64,
            count: u32::try_from(l.loop_count.max(0)).unwrap_or(0),
            alternating: l.alternating_loop,
            until_release: l.mode == 2,
        });
    }
    // The slots are in no particular order; the model wants play order.
    found.sort_by_key(|l| l.end);
    Some(ir::Zone {
        mix: Vec::new(),
        sample,
        keys: midi(lk)..=midi(hk),
        velocities: midi(lv).max(1)..=midi(hv),
        velocity_fade: (midi(z.fade_low_velocity), midi(z.fade_high_velocity)),
        key_fade: (midi(z.fade_low_key), midi(z.fade_high_key)),
        root: midi(z.root_key),
        tune: semitones(z.zone_tune),
        volume_db: db(z.zone_volume),
        pan: z.zone_pan.clamp(-1.0, 1.0),
        start,
        end,
        start_mod: u64::try_from(z.sample_start_mod_range).unwrap_or(0),
        loops: found,
        reverse,
        slot: 0,
    })
}
