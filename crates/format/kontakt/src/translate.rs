//! A Kontakt program as the instrument model: groups, zones, start rules,
//! envelopes and scripts. Kontakt stores gains and tunes as ratios.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use kontra_ir as ir;
use ni_file::kontakt::KontaktChunks;
use ni_file::kontakt::objects::{
    BParScript, ExternalModArray32, Group, GroupList, InternalModArray16, LoopArray, ModSource,
    Modulator, StartCriteriaParams, VoiceGroups, VoiceLimit, ZoneList,
};

use crate::Error;
use crate::samples::Resolver;

const GROUP_LIST: u16 = 0x33;
const ZONE_LIST: u16 = 0x34;
const LOOPS: u16 = 0x39;
const INTERNAL_MODS: u16 = 0x3b;
const EXTERNAL_MODS: u16 = 0x3c;
const SCRIPT: u16 = 6;
const VOICE_GROUPS: u16 = 0x32;

pub(crate) fn instrument(path: &Path, chunks: &KontaktChunks) -> Result<ir::Instrument, Error> {
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
        preload: u32::try_from(params.dfd_channel_preload_size).unwrap_or(61_440),
        ..ir::Instrument::DEFAULT
    };
    // Instrument volume, pan and tune apply to every group.
    let base = Base {
        volume_db: db(params.volume),
        pan: params.pan,
        tune: semitones(params.tune) + f64::from(params.transpose),
    };
    for chunk in program.0.children.iter().filter(|c| c.id == SCRIPT) {
        let script = BParScript::try_from(chunk)
            .and_then(|s| s.params())
            .map_err(decode("script"))?;
        match script.text {
            _ if script.bypass => {}
            Some(text) if !text.trim().is_empty() => out.scripts.push(ir::Script {
                name: script.description.unwrap_or_default(),
                dialect: ir::Dialect::Ksp,
                source: text,
                saved: script.persistent,
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
    let mut criteria = Vec::new();
    for (index, group) in groups.groups.iter().enumerate() {
        let (g, rows) = translate_group(group, &base, &mut notes)
            .map_err(|e| Error::new(path, "group", format!("{index}: {e}")))?;
        out.groups.push(g);
        criteria.push(rows);
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
    let table = chunks
        .filename_table()
        .transpose()
        .map_err(decode("sample table"))?
        .unwrap_or_default();
    let mut resolver = Resolver::new(path).map_err(|e| Error::new(path, "samples", e))?;
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
    out.diagnostics = notes.finish();
    Ok(out)
}

struct Base {
    volume_db: f32,
    pan: f32,
    tune: f64,
}

fn db(linear: f32) -> f32 {
    20.0 * linear.max(1e-6).log10()
}

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
        let Some(n) = group
            .params()
            .ok()
            .and_then(|p| usize::try_from(p.voice_group_index - 1).ok())
            .filter(|&n| n < 128)
        else {
            continue;
        };
        let at = *index[n].get_or_insert_with(|| {
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

/// Kontakt's attack curve knob (−1 concave ..= 1 convex) as a segment shape.
/// The engine rounds the base to f32 before taking its log
/// (docs/re/engine/k-mod-envelopes.md).
fn attack_shape(curve: f32) -> f32 {
    let c = f64::from(curve.clamp(-1.0, 1.0));
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

fn translate_group(
    group: &Group,
    base: &Base,
    notes: &mut Notes,
) -> Result<(ir::Group, Vec<StartCriteriaParams>), ni_file::Error> {
    let v = group.params()?;
    let mut out = ir::Group {
        name: v.name.clone(),
        volume_db: db(v.volume) + base.volume_db,
        pan: (v.pan + base.pan).clamp(-1.0, 1.0),
        tune: semitones(v.tune) + base.tune,
        keytrack: if v.key_tracking { 1.0 } else { 0.0 },
        trigger: if v.release_trigger {
            ir::Trigger::Release
        } else {
            ir::Trigger::Attack
        },
        purged: group.source_identity().is_ok_and(|s| s.flag == 1),
        ..ir::Group::DEFAULT
    };
    let slot = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
        for (n, m) in InternalModArray16::try_from(chunk)?.slots()? {
            let p = m.params()?;
            let volume = p
                .targets
                .iter()
                .any(|t| t.param == "volume" && t.slot.is_none());
            match &p.modulator {
                // ponytail: one volume envelope per group; Kontakt multiplies a
                // second one in, which is reported below until a library needs it.
                Modulator::Ahdsr(e)
                    if volume
                        && !out
                            .mods
                            .iter()
                            .any(|m| matches!(m.kind, ir::ModKind::Envelope)) =>
                {
                    // Stored times are milliseconds; the sustain knob is cubic.
                    out.envelope = ir::Envelope {
                        attack: e.attack_ms.max(0.0) / 1000.0,
                        hold: e.hold_ms.max(0.0) / 1000.0,
                        decay: e.decay_ms.max(0.0) / 1000.0,
                        sustain: e.sustain.clamp(0.0, 1.0).powi(3),
                        release: e.release_ms.max(0.0) / 1000.0,
                        attack_shape: attack_shape(e.attack_curve),
                        fall_shape: ir::Envelope::KONTAKT_FALL,
                    };
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: slot(n),
                        kind: ir::ModKind::Envelope,
                    });
                }
                _ => notes.add(ir::Impact::Audible, "internal modulator", &p.name),
            }
        }
    }
    if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
        for (n, m) in ExternalModArray32::try_from(chunk)?.slots()? {
            let p = m.params()?;
            let source = match p.source {
                ModSource::Velocity => Some(ir::Source::Velocity),
                ModSource::MidiCc(cc) => Some(ir::Source::Controller(cc)),
                _ => None,
            };
            match (source, p.targets.as_slice()) {
                (Some(source), [t]) if t.param == "volume" && t.slot.is_none() => {
                    out.mods.push(ir::Mod {
                        name: p.name.clone(),
                        slot: slot(n),
                        kind: ir::ModKind::Volume {
                            source,
                            intensity: t.signed_intensity(),
                            curve: ir::Curve::Kontakt {
                                cubed: source == ir::Source::Velocity,
                            },
                        },
                    });
                }
                _ => notes.add(ir::Impact::Audible, "external modulator", &p.name),
            }
        }
    }
    Ok((out, v.start_criteria.items))
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
    let mut looped = None;
    for (n, l) in loops.into_iter().flat_map(|l| &l.items).enumerate() {
        if !matches!(l.mode, 1 | 2) || l.loop_length <= 0 || l.loop_start < 0 {
            continue;
        }
        if looped.is_some() {
            notes.add(ir::Impact::Audible, "several loops", format!("loop {n}"));
            continue;
        }
        if l.alternating_loop {
            notes.add(ir::Impact::Audible, "alternating loop", "");
        }
        let from = l.loop_start as u64;
        looped = Some(ir::Loop {
            start: from,
            end: from + l.loop_length as u64,
            crossfade: l.x_fade_length.max(0) as u64,
        });
    }
    Some(ir::Zone {
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
        looped,
        reverse,
    })
}

/// Diagnostics folded by feature: one entry with a count and the first detail.
#[derive(Default)]
struct Notes(BTreeMap<(ir::Impact, String), (usize, String)>);

impl Notes {
    fn add(&mut self, impact: ir::Impact, feature: &str, detail: impl ToString) {
        let entry = self
            .0
            .entry((impact, feature.into()))
            .or_insert((0, detail.to_string()));
        entry.0 += 1;
    }

    fn finish(self) -> Vec<ir::Diagnostic> {
        self.0
            .into_iter()
            .rev()
            .map(|((impact, feature), (count, first))| ir::Diagnostic {
                impact,
                detail: format!("{count}× (first: {first})"),
                feature,
            })
            .collect()
    }
}
