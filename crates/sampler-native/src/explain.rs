//! `explain`: what KONTRA does with a note scenario, and why.
//!
//! Static half: every zone and group of the translated IR is judged against the
//! scenario (key, velocity, controllers, trigger, group start tests) with the
//! reason. Conditions only the runtime resolves (round robin position, random
//! draws, articulation and axis switches) are reported as conditional, never guessed.
//! Runtime half: the same scenario through the real `Runtime`, the path playback uses,
//! so the static verdicts can be checked against what sounds.
use crate::render_kontakt;
use sampler_core::{Outcome, Prepared};
use sampler_ir::{
    AssetLocation, Gain, GroupStart, Instrument, Output, StartJoin, StartTest, Take, Trigger,
    VelocityResponse, Zone,
};
use sampler_midi::{Ingress, Packets, TimedPacket, Version};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

/// Native render block; runtime onsets are known to this resolution.
const BLOCK: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub key: u8,
    pub velocity: u8,
    pub start_ms: f64,
    pub length_ms: f64,
}

/// `KEY:VELOCITY[:START_MS[:LENGTH_MS]]`, e.g. `60:100:0:1500`.
pub fn parse_note(text: &str) -> Result<Note, String> {
    let parts: Vec<&str> = text.split(':').collect();
    if !(2..=4).contains(&parts.len()) {
        return Err(format!("note {text:?}: expected KEY:VELOCITY[:START_MS[:LENGTH_MS]]"));
    }
    let bad = |what| format!("note {text:?}: bad {what}");
    let key: u8 = parts[0].parse().map_err(|_| bad("key"))?;
    let velocity: u8 = parts[1].parse().map_err(|_| bad("velocity"))?;
    let start_ms: f64 = match parts.get(2) {
        Some(s) => s.parse().map_err(|_| bad("start"))?,
        None => 0.0,
    };
    let length_ms: f64 = match parts.get(3) {
        Some(s) => s.parse().map_err(|_| bad("length"))?,
        None => 1000.0,
    };
    if key > 127 || !(1..=127).contains(&velocity) || start_ms < 0.0 || length_ms <= 0.0 {
        return Err(format!("note {text:?}: key 0-127, velocity 1-127, start >= 0, length > 0"));
    }
    Ok(Note { key, velocity, start_ms, length_ms })
}

/// `CC=VALUE`, e.g. `1=64`.
pub fn parse_controller(text: &str) -> Result<(u8, u8), String> {
    let (cc, value) = text.split_once('=').ok_or_else(|| format!("controller {text:?}: expected CC=VALUE"))?;
    let cc: u8 = cc.parse().map_err(|_| format!("controller {text:?}: bad CC"))?;
    let value: u8 = value.parse().map_err(|_| format!("controller {text:?}: bad value"))?;
    if cc > 127 || value > 127 {
        return Err(format!("controller {text:?}: CC and value 0-127"));
    }
    Ok((cc, value))
}

pub struct Scenario {
    pub notes: Vec<Note>,
    pub controllers: BTreeMap<u8, u8>,
    pub tail_ms: f64,
}

impl Scenario {
    /// Controllers not given stay at 0; power-on values are not modelled here.
    fn controller(&self, cc: u8) -> u8 {
        self.controllers.get(&cc).copied().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Verdict {
    Accepted,
    Conditional(String),
    Rejected(&'static str, String),
}

impl Verdict {
    fn json(&self) -> Value {
        match self {
            Verdict::Accepted => json!({"verdict": "accepted"}),
            Verdict::Conditional(why) => json!({"verdict": "conditional", "reason": why}),
            Verdict::Rejected(kind, why) => json!({"verdict": "rejected", "kind": kind, "reason": why}),
        }
    }
}

fn db(linear: f64) -> f64 {
    20.0 * linear.max(1e-12).log10()
}

fn gain_db(gain: Gain) -> f64 {
    match gain {
        Gain::Decibels(d) => d,
        Gain::Linear(l) => db(l),
    }
}

/// Start-test fold, joining each row to the next with the previous row's join,
/// the left-to-right order the native start evaluator uses. `None` means a
/// round-robin or random test that the static pass cannot decide.
fn start_gate(tests: &[GroupStart], key: u8, scenario: &Scenario) -> Option<bool> {
    if tests.is_empty() {
        return Some(true);
    }
    let mut active = None;
    for (i, start) in tests.iter().enumerate() {
        let next = match start.test {
            StartTest::Key { low, high } => Some((low..=high).contains(&key)),
            StartTest::Controller { controller, low, high } => {
                Some((low..=high).contains(&scenario.controller(controller)))
            }
            StartTest::RoundRobin(_) | StartTest::Random => None,
        };
        active = if i == 0 { next } else { combine(active, next, tests[i - 1].next) };
    }
    active
}

fn combine(a: Option<bool>, b: Option<bool>, join: StartJoin) -> Option<bool> {
    match join {
        StartJoin::And => match (a, b) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        },
        StartJoin::AndNot => combine(a, b.map(|x| !x), StartJoin::And),
        StartJoin::Or => match (a, b) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        },
    }
}

/// Judge one zone for one note-on. `held` are keys already down when it starts.
fn judge(ins: &Instrument, zone: &Zone, note: &Note, held: &[u8], scenario: &Scenario) -> Verdict {
    let key = note.key;
    if !(zone.keys.low..=zone.keys.high).contains(&key) {
        return Verdict::Rejected("key", format!("key {key} outside {}-{}", zone.keys.low, zone.keys.high));
    }
    if !(zone.velocities.low..=zone.velocities.high).contains(&note.velocity) {
        return Verdict::Rejected(
            "velocity",
            format!("velocity {} outside {}-{}", note.velocity, zone.velocities.low, zone.velocities.high),
        );
    }
    for c in &zone.conditions {
        let value = scenario.controller(c.controller);
        if !(c.low..=c.high).contains(&value) {
            return Verdict::Rejected("controller", format!("CC{} = {value} outside {}-{}", c.controller, c.low, c.high));
        }
    }
    match zone.trigger {
        Trigger::Attack => {}
        Trigger::First if !held.is_empty() => {
            return Verdict::Rejected("trigger", "first: another key is already held".into());
        }
        Trigger::First => {}
        Trigger::Legato if held.is_empty() => return Verdict::Rejected("trigger", "legato: no key is held".into()),
        Trigger::Legato => {}
        Trigger::Transition { low, high } => match held.last() {
            None => return Verdict::Rejected("trigger", "transition: no held key".into()),
            Some(&last) => {
                let step = i32::from(key) - i32::from(last);
                if !(i32::from(low)..=i32::from(high)).contains(&step) {
                    return Verdict::Rejected("trigger", format!("transition: interval {step} outside {low}..={high}"));
                }
            }
        },
        Trigger::KeyRelease | Trigger::GateRelease => {
            return Verdict::Rejected("trigger", "release trigger: sounds at key-up, see release_triggers".into());
        }
    }
    let mut open = Vec::new();
    if zone.articulation.is_some() || !zone.axes.is_empty() {
        open.push("articulation/axis switch state (runtime)".to_string());
    }
    if let Some(selection) = zone.selection {
        let sequence = &ins.sequences[selection.sequence.0];
        match selection.take {
            Take::Index(i) => open.push(format!(
                "round robin position {} of {} ({:?})",
                i + 1,
                sequence.takes,
                sequence.policy
            )),
            Take::Probability { low, high } => open.push(format!("random draw in [{low}, {high})")),
        }
    }
    if let Some(group) = zone.group.map(|g| &ins.groups[g.0]) {
        match start_gate(&group.start, key, scenario) {
            Some(true) => {}
            Some(false) => return Verdict::Rejected("group-start", "group start test failed".into()),
            None => open.push("group start test depends on round robin/random".into()),
        }
    }
    if open.is_empty() { Verdict::Accepted } else { Verdict::Conditional(open.join("; ")) }
}

/// Volume path in dB, zone to output. Envelope times and insert parameters are not read here.
fn volume_path(ins: &Instrument, zone: &Zone, velocity: u8) -> Value {
    let velocity_db = match zone.velocity {
        VelocityResponse::None => 0.0,
        VelocityResponse::Linear => db(f64::from(velocity) / 127.0),
        VelocityResponse::Power(exponent) => exponent * db(f64::from(velocity) / 127.0),
    };
    let group = zone.group.map(|g| &ins.groups[g.0]);
    let group_db = group.map_or(0.0, |g| gain_db(g.gain));
    let output = match group.map(|g| &g.output) {
        Some(Output::Bus(bus)) => format!("bus {}", bus.0),
        _ => "master".to_string(),
    };
    json!({
        "zone_db": gain_db(zone.gain),
        "velocity_db": velocity_db,
        "group_db": group_db,
        "total_db": gain_db(zone.gain) + velocity_db + group_db,
        "output": output,
        "amp_envelope": zone.amplitude.is_some(),
        "zone_chain": zone.chain.is_some(),
        "group_chain": group.is_some_and(|g| g.chain.is_some()),
    })
}

fn sample_name(ins: &Instrument, zone: &Zone) -> String {
    match zone.asset.map(|a| &ins.assets[a.0].location) {
        Some(AssetLocation::Path(path)) => path.clone(),
        Some(AssetLocation::KontaktFile { id }) => format!("kontakt file id {id}"),
        None => "dormant (no sample)".into(),
    }
}

fn group_of(ins: &Instrument, zone: &Zone) -> String {
    zone.group.map_or("(no group)".into(), |g| ins.groups[g.0].name.clone())
}

/// One runtime observation: a voice sounding for a key/velocity at a block start.
#[derive(Clone, Copy, Debug)]
struct Heard {
    frame: usize,
    group: u32,
    key: u8,
    velocity: u8,
    level: f32,
}

struct Play {
    observed: Vec<Heard>,
    errors: Vec<String>,
    max_voices: usize,
    peak: f32,
    /// Time from the last key-up until no voice sounds, at block resolution.
    silent_after_frames: Option<usize>,
    last_off: usize,
    rate: u32,
}

/// Drive the runtime with the scenario, the same MIDI path playback takes.
fn play(plan: Prepared, mut scripts: Vec<sampler_ksp::ScriptView>, scenario: &Scenario) -> io::Result<Play> {
    let rate = plan.sample_rate();
    let frame = |ms: f64| (ms * f64::from(rate) / 1000.0).round() as usize;
    let mut events: Vec<(usize, u32)> = scenario
        .controllers
        .iter()
        .map(|(&cc, &value)| (0, 0x2000_0000 | 0xB0 << 16 | u32::from(cc) << 8 | u32::from(value)))
        .collect();
    for note in &scenario.notes {
        let on = frame(note.start_ms);
        let off = frame(note.start_ms + note.length_ms);
        events.push((on, 0x2000_0000 | 0x90 << 16 | u32::from(note.key) << 8 | u32::from(note.velocity)));
        events.push((off, 0x2000_0000 | 0x80 << 16 | u32::from(note.key) << 8));
    }
    events.sort_by_key(|e| e.0);
    let last_off = scenario.notes.iter().map(|n| frame(n.start_ms + n.length_ms)).max().unwrap_or(0);
    let count = last_off + frame(scenario.tail_ms);
    let words: Vec<[u32; 1]> = events.iter().map(|&(_, word)| [word]).collect();
    let packets = events
        .iter()
        .zip(&words)
        .map(|(&(offset, _), word)| {
            let packet = Packets::new(word).next().expect("one word").map_err(|e| io::Error::other(format!("{e:?}")))?;
            Ok(TimedPacket { offset, packet })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    let mut rt = render_kontakt::runtime(plan)?;
    let mut out = Play {
        observed: Vec::new(),
        errors: Vec::new(),
        max_voices: 0,
        peak: 0.0,
        silent_after_frames: None,
        last_off,
        rate,
    };
    let mut buffer = [[0.0f32; 2]; BLOCK];
    let mut next = 0;
    let mut last_audible = None;
    for begin in (0..count).step_by(BLOCK) {
        let len = BLOCK.min(count - begin);
        let mut batch = Vec::new();
        while next < packets.len() && packets[next].offset < begin + len {
            batch.push(TimedPacket { offset: packets[next].offset - begin, ..packets[next] });
            next += 1;
        }
        let mut failures = Vec::new();
        ingress
            .render(&mut rt, &mut buffer[..len], &batch, batch.len(), |index, result| {
                if let Err(e) = result {
                    failures.push(format!("frame {}: {e:?}", begin + batch[index].offset));
                }
            })
            .map_err(|e| io::Error::other(format!("MIDI block: {e:?}")))?;
        sampler_ksp::service_ui_effects(&mut rt, &mut scripts)
            .map_err(|e| io::Error::other(format!("ui effects: {e:?}")))?;
        out.errors.append(&mut failures);
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) {
                out.errors.push(format!("script outcome: {outcome:?}"));
            }
            true
        });
        rt.flush_ended(|_| true);
        for tap in rt.voice_taps().into_iter().flatten() {
            out.observed.push(Heard { frame: begin, group: tap.group, key: tap.key, velocity: tap.velocity, level: tap.level });
        }
        out.max_voices = out.max_voices.max(rt.voice_count());
        if rt.audible_voice_count() > 0 {
            last_audible = Some(begin + len);
        }
        out.peak = buffer[..len].iter().flatten().fold(out.peak, |p, x| p.max(x.abs()));
    }
    out.silent_after_frames = last_audible.map(|at| at.saturating_sub(last_off));
    Ok(out)
}

fn ms(frames: usize, rate: u32) -> f64 {
    frames as f64 * 1000.0 / f64::from(rate)
}

/// Release-trigger candidates for a note's key-up: key, velocity and controllers.
/// Group start tests and round robin are not applied to releases in this version.
fn release_matches(zone: &Zone, note: &Note, scenario: &Scenario) -> bool {
    matches!(zone.trigger, Trigger::KeyRelease | Trigger::GateRelease)
        && (zone.keys.low..=zone.keys.high).contains(&note.key)
        && (zone.velocities.low..=zone.velocities.high).contains(&note.velocity)
        && zone.conditions.iter().all(|c| (c.low..=c.high).contains(&scenario.controller(c.controller)))
}

/// Full report for one scenario: the static chain per note, the runtime observation,
/// and the ranked suspicious list.
pub fn report(loaded: sampler_kontakt::Loaded, scenario: &Scenario) -> io::Result<Value> {
    let sampler_kontakt::Loaded { instrument: ins, plan, scripts, .. } = loaded;
    let script_count = scripts.len();
    let rate = plan.sample_rate();
    let mut suspicious: Vec<String> = Vec::new();

    let mut features: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for item in &ins.unsupported {
        let entry = features.entry(item.feature.to_string()).or_insert((0, item.location.to_string()));
        entry.0 += 1;
    }
    let mut unsupported: Vec<(usize, String, String)> =
        features.into_iter().map(|(feature, (count, first))| (count, feature, first)).collect();
    unsupported.sort_by(|a, b| b.0.cmp(&a.0));
    for (count, feature, first) in &unsupported {
        suspicious.push(format!("unsupported x{count}: {feature} (first at {first})"));
    }

    // Static chain, notes in start order. Held keys are those still down at onset.
    let mut order: Vec<usize> = (0..scenario.notes.len()).collect();
    order.sort_by(|&a, &b| scenario.notes[a].start_ms.total_cmp(&scenario.notes[b].start_ms));
    let mut held: Vec<(u8, f64)> = Vec::new();
    let mut static_notes = Vec::new();
    for &index in &order {
        let note = scenario.notes[index];
        held.retain(|&(_, end)| end > note.start_ms);
        let held_keys: Vec<u8> = held.iter().map(|&(key, _)| key).collect();
        let mut rejected: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut candidates = Vec::new();
        let mut accepted: BTreeSet<Option<usize>> = BTreeSet::new();
        let mut open: BTreeSet<Option<usize>> = BTreeSet::new();
        for (zi, zone) in ins.zones.iter().enumerate() {
            let verdict = judge(&ins, zone, &note, &held_keys, scenario);
            match &verdict {
                Verdict::Rejected(kind, _) => *rejected.entry(kind).or_default() += 1,
                Verdict::Accepted => {
                    accepted.insert(zone.group.map(|g| g.0));
                }
                Verdict::Conditional(_) => {
                    open.insert(zone.group.map(|g| g.0));
                }
            }
            if !matches!(verdict, Verdict::Rejected(..)) {
                candidates.push(json!({
                    "zone": zi,
                    "group": group_of(&ins, zone),
                    "sample": sample_name(&ins, zone),
                    "keys": [zone.keys.low, zone.keys.high],
                    "velocities": [zone.velocities.low, zone.velocities.high],
                    "verdict": verdict.json(),
                    "volume": volume_path(&ins, zone, note.velocity),
                }));
            }
        }
        let releases: Vec<Value> = ins
            .zones
            .iter()
            .enumerate()
            .filter(|(_, zone)| release_matches(zone, &note, scenario))
            .map(|(zi, zone)| json!({"zone": zi, "group": group_of(&ins, zone), "sample": sample_name(&ins, zone), "trigger": format!("{:?}", zone.trigger)}))
            .collect();
        if candidates.is_empty() {
            suspicious.push(format!(
                "note {} vel {} at {} ms: no zone passes key/velocity/controller/trigger, silent by the IR",
                note.key, note.velocity, note.start_ms
            ));
        }
        held.push((note.key, note.start_ms + note.length_ms));
        static_notes.push((index, note, held_keys, candidates, rejected, releases, accepted, open));
    }

    let play = play(plan, scripts, scenario)?;
    let tail_frames = (scenario.tail_ms * f64::from(rate) / 1000.0) as usize;
    let mut notes_out = Vec::new();
    for (index, note, held_keys, candidates, rejected, releases, accepted, open) in static_notes {
        let start = (note.start_ms * f64::from(rate) / 1000.0).round() as usize;
        let end = start + (note.length_ms * f64::from(rate) / 1000.0).round() as usize;
        let heard: Vec<&Heard> = play
            .observed
            .iter()
            .filter(|h| h.key == note.key && h.velocity == note.velocity && h.frame + BLOCK > start && h.frame < end + tail_frames)
            .collect();
        let onset = heard.iter().map(|h| h.frame).min().map(|f| ms(f.saturating_sub(start), rate));
        let mut heard_groups: BTreeMap<u32, (usize, f32)> = BTreeMap::new();
        for h in &heard {
            let entry = heard_groups.entry(h.group).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 = entry.1.max(h.level);
        }
        let heard_names: Vec<Value> = heard_groups
            .iter()
            .map(|(&g, &(n, level))| {
                let name = ins.groups.get(g as usize).map_or("(unknown)".to_string(), |grp| grp.name.clone());
                json!({"group_index": g, "group": name, "observations": n, "peak_level": level})
            })
            .collect();
        // Runtime groups as static indices; `None` stands for an out-of-range or unknown group.
        let heard_set: BTreeSet<Option<usize>> = heard_groups
            .keys()
            .map(|&g| ((g as usize) < ins.groups.len()).then_some(g as usize))
            .collect();
        // Scripts (allow/disallow_group, play_note) and round robin decide what
        // actually sounds, so a count is reported, not one line per group.
        let unsounded = accepted.difference(&heard_set).flatten().count();
        if unsounded > 0 {
            suspicious.push(format!(
                "note {} vel {} at {} ms: {unsounded} groups accepted by the IR did not sound (scripts or round robin may gate them)",
                note.key, note.velocity, note.start_ms
            ));
        }
        let static_groups: BTreeSet<Option<usize>> = accepted.union(&open).copied().collect();
        for g in heard_set.difference(&static_groups) {
            let name = g.map_or("(unknown)".to_string(), |g| ins.groups[g].name.clone());
            suspicious.push(format!(
                "note {} vel {} at {} ms: group {} sounded but no zone of it passed the static chain",
                note.key, note.velocity, note.start_ms, name
            ));
        }
        notes_out.push(json!({
            "index": index,
            "key": note.key,
            "velocity": note.velocity,
            "start_ms": note.start_ms,
            "length_ms": note.length_ms,
            "held_before": held_keys,
            "candidates": candidates,
            "rejected_by": rejected,
            "release_triggers_matched": releases,
            "runtime": {
                "onset_ms": onset,
                "onset_resolution_ms": ms(BLOCK, rate),
                "heard": heard_names,
            },
        }));
    }

    let groups_out: Vec<Value> = ins
        .groups
        .iter()
        .enumerate()
        .map(|(gi, g)| {
            let zones = ins.zones.iter().filter(|z| z.group.is_some_and(|r| r.0 == gi)).count();
            let output = match &g.output {
                Output::Master => "master".to_string(),
                Output::Bus(bus) => format!("bus {}", bus.0),
            };
            json!({
                "index": gi,
                "name": g.name,
                "zones": zones,
                "start_tests": g.start.len(),
                "output": output,
                "monophonic_release": g.monophonic_release,
            })
        })
        .collect();

    for error in &play.errors {
        suspicious.push(format!("runtime: {error}"));
    }
    let mut seen = BTreeSet::new();
    suspicious.retain(|s| seen.insert(s.clone()));

    Ok(json!({
        "schema": "explain-note-v1",
        "instrument": {"name": ins.name, "groups": ins.groups.len(), "zones": ins.zones.len(), "scripts": script_count, "sample_rate": rate},
        "scenario": {
            "notes": scenario.notes.iter().map(|n| json!({"key": n.key, "velocity": n.velocity, "start_ms": n.start_ms, "length_ms": n.length_ms})).collect::<Vec<_>>(),
            "controllers": scenario.controllers.iter().map(|(c, v)| json!({"cc": c, "value": v})).collect::<Vec<_>>(),
            "controller_default": "0 unless given",
            "tail_ms": scenario.tail_ms,
        },
        "unsupported": unsupported.iter().map(|(n, f, at)| json!({"feature": f, "count": n, "first": at})).collect::<Vec<_>>(),
        "groups": groups_out,
        "notes": notes_out,
        "runtime": {
            "max_voices": play.max_voices,
            "voice_taps_limit": 16,
            "peak": play.peak,
            "silent_after_last_key_up_ms": play.silent_after_frames.map(|f| ms(f, rate)),
            "last_key_up_ms": ms(play.last_off, rate),
        },
        "suspicious": suspicious,
    }))
}

/// Load only zones overlapping the scenario's keys (as `render` does for played keys),
/// with scripts, so a full multi-GB library never loads for one note.
pub fn load(instrument: &Path, scenario: &Scenario) -> io::Result<sampler_kontakt::Loaded> {
    let low = scenario.notes.iter().map(|n| n.key).min().unwrap_or(0);
    let high = scenario.notes.iter().map(|n| n.key).max().unwrap_or(127);
    let options = sampler_kontakt::Options { keys: low..=high, scripts: true, ..Default::default() };
    sampler_kontakt::load(instrument, &options, |_| {}).map_err(|e| io::Error::other(e.to_string()))
}

const USAGE: &str = "usage: explain INSTRUMENT --note KEY:VEL[:START_MS[:LEN_MS]] [--note ...] [--cc N=V]... [--tail-ms T] [--json]";

/// CLI entry: `sampler-native explain ...`.
pub fn run(args: &[String]) -> io::Result<()> {
    let usage = || io::Error::other(USAGE);
    let mut instrument = None;
    let mut notes = Vec::new();
    let mut controllers = BTreeMap::new();
    let mut tail_ms = 3000.0;
    let mut json_out = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--note" => notes.push(parse_note(rest.next().ok_or_else(usage)?).map_err(io::Error::other)?),
            "--cc" => {
                let (cc, value) = parse_controller(rest.next().ok_or_else(usage)?).map_err(io::Error::other)?;
                controllers.insert(cc, value);
            }
            "--tail-ms" => tail_ms = rest.next().and_then(|t| t.parse().ok()).ok_or_else(usage)?,
            "--json" => json_out = true,
            other if instrument.is_none() && !other.starts_with("--") => instrument = Some(other.to_string()),
            _ => return Err(usage()),
        }
    }
    let instrument = instrument.ok_or_else(usage)?;
    if notes.is_empty() {
        return Err(usage());
    }
    let scenario = Scenario { notes, controllers, tail_ms };
    let loaded = load(Path::new(&instrument), &scenario)?;
    let value = report(loaded, &scenario)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value).map_err(io::Error::other)?);
    } else {
        print_table(&value);
    }
    Ok(())
}

fn print_table(value: &Value) {
    let i = &value["instrument"];
    println!("instrument {} · {} groups · {} zones · {} scripts", i["name"], i["groups"], i["zones"], i["scripts"]);
    for u in value["unsupported"].as_array().into_iter().flatten() {
        println!("unsupported x{}: {} (first at {})", u["count"], u["feature"], u["first"]);
    }
    for n in value["notes"].as_array().into_iter().flatten() {
        println!(
            "\nnote key {} vel {} at {} ms for {} ms · held before {}",
            n["key"], n["velocity"], n["start_ms"], n["length_ms"], n["held_before"]
        );
        for c in n["candidates"].as_array().into_iter().flatten().take(40) {
            println!(
                "  {:<11} zone {:>5} · {} · {:.1} dB · {} ({})",
                c["verdict"]["verdict"].as_str().unwrap_or(""),
                c["zone"],
                c["group"],
                c["volume"]["total_db"].as_f64().unwrap_or(0.0),
                c["sample"],
                c["verdict"]["reason"].as_str().unwrap_or("")
            );
        }
        let candidates = n["candidates"].as_array().map_or(0, Vec::len);
        if candidates > 40 {
            println!("  ... {} more candidates", candidates - 40);
        }
        println!("  rejected: {}", n["rejected_by"]);
        println!(
            "  runtime: onset {} ms (block {:.1} ms), heard {}",
            n["runtime"]["onset_ms"].as_f64().map_or("none".into(), |v| format!("{v:.1}")),
            n["runtime"]["onset_resolution_ms"].as_f64().unwrap_or(0.0),
            n["runtime"]["heard"]
        );
    }
    println!("\nruntime: max voices {}, peak {:.4}", value["runtime"]["max_voices"], value["runtime"]["peak"].as_f64().unwrap_or(0.0));
    let suspicious = value["suspicious"].as_array().map_or(0, Vec::len);
    println!("\nsuspicious ({suspicious}):");
    for s in value["suspicious"].as_array().into_iter().flatten() {
        println!("  - {}", s.as_str().unwrap_or(""));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_and_controllers_parse_with_defaults_and_reject_out_of_range() {
        assert_eq!(
            parse_note("60:100").unwrap(),
            Note { key: 60, velocity: 100, start_ms: 0.0, length_ms: 1000.0 }
        );
        assert_eq!(parse_note("36:1:250:40").unwrap().length_ms, 40.0);
        assert!(parse_note("128:100").is_err());
        assert!(parse_note("60:0").is_err());
        assert!(parse_note("60:100:0:0").is_err());
        assert_eq!(parse_controller("1=64").unwrap(), (1, 64));
        assert!(parse_controller("1").is_err());
        assert!(parse_controller("128=1").is_err());
    }

    #[test]
    fn start_joins_fold_left_to_right_and_unknown_tests_stay_open() {
        use StartJoin::{And, AndNot, Or};
        assert_eq!(combine(Some(true), Some(false), AndNot), Some(true));
        assert_eq!(combine(Some(true), Some(true), AndNot), Some(false));
        assert_eq!(combine(Some(false), None, And), Some(false));
        assert_eq!(combine(Some(true), None, Or), Some(true));
        assert_eq!(combine(Some(false), None, Or), None);
        let key = |low, high, next| GroupStart { slot: 0, test: StartTest::Key { low, high }, next };
        let scenario = Scenario { notes: vec![], controllers: BTreeMap::new(), tail_ms: 0.0 };
        let tests = [key(0, 59, Or), key(60, 127, And)];
        // (key <= 59) OR (key >= 60): always true, for any key.
        assert_eq!(start_gate(&tests, 10, &scenario), Some(true));
        assert_eq!(start_gate(&tests, 90, &scenario), Some(true));
        let rr = GroupStart { slot: 0, test: StartTest::RoundRobin(1), next: And };
        assert_eq!(start_gate(&[rr], 60, &scenario), None);
    }
}
