//! Corpus health scoreboard: does every installed library instrument load,
//! compile its scripts, and sound? One JSON line per instrument, plus a
//! markdown summary of totals and the top failure reasons.
//!
//! ```text
//! corpus_health run OUT.jsonl [--shard I/N] [ROOT ...]   resumable; appends to OUT
//! corpus_health summary OUT.jsonl SUMMARY.md
//! ```
//!
//! Each item is loaded the way the product host loads it (`sampler_kontakt::load`
//! with scripts on, keys narrowed to one covered key; UVI programs through the
//! bank loader), plays one note at middle velocity for a second, releases it,
//! and renders five more seconds. Decoded samples live in memory only; the
//! JSON holds counts and the first error text, never library content.
//!
//! A crash (abort, OOM kill) leaves a `started` line without a result. On
//! resume that item is recorded as `crash` and skipped, so one bad library
//! cannot stall the run.
mod heap;

use sampler_core::{Limits, Outcome, Runtime};
use sampler_midi::{Applied, Ingress, Packets, TimedPacket, Version};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

/// A numeric field of a `/proc/self` file ("VmHWM:", "read_bytes:" ...).
fn proc_field(file: &str, key: &str) -> u64 {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

/// Minor and major page faults so far.
fn faults() -> (u64, u64) {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let rest = stat.rsplit_once(')').map_or("", |x| x.1);
    let f: Vec<u64> = rest.split_whitespace().skip(7).take(3).filter_map(|v| v.parse().ok()).collect();
    (f.first().copied().unwrap_or(0), f.get(2).copied().unwrap_or(0))
}

/// Start a fresh measurement window: peak RSS and peak heap restart here.
fn reset_peaks() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    heap::reset_peak();
}

const HOLD_SECONDS: f64 = 1.0;
const TAIL_SECONDS: f64 = 5.0;
const VELOCITY: u8 = 64;

#[derive(Clone, Debug)]
enum Item {
    Kontakt(PathBuf),
    Multi(PathBuf),
    UviProgram { bank: PathBuf, program: String },
    UviLoose(PathBuf),
}

impl Item {
    fn id(&self) -> String {
        match self {
            Item::Kontakt(p) | Item::Multi(p) | Item::UviLoose(p) => p.to_string_lossy().into(),
            Item::UviProgram { bank, program } => format!("{}::{program}", bank.display()),
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Item::Kontakt(_) => "kontakt",
            Item::Multi(_) => "kontakt-multi",
            Item::UviProgram { .. } => "uvi-program",
            Item::UviLoose(_) => "uvi-loose",
        }
    }
}

fn roots(explicit: &[String]) -> Vec<PathBuf> {
    if !explicit.is_empty() {
        return explicit.iter().map(PathBuf::from).collect();
    }
    let mut roots = Vec::new();
    for variable in ["KONTRA_KONTAKT_LIBRARIES", "KONTRA_UVI_LIBRARIES"] {
        if let Some(paths) = std::env::var_os(variable) {
            roots.extend(std::env::split_paths(&paths));
        }
    }
    if roots.is_empty() {
        roots.extend(
            [
                "/mnt/MAIN_STORAGE/Libraries/Kontakt",
                "/mnt/MAIN_STORAGE/Libraries/UVI",
            ]
            .into_iter()
            .map(PathBuf::from)
            .filter(|r| r.is_dir()),
        );
    }
    roots
}

/// Collect library files. A directory is visited once (by canonical path), and
/// `KONTRA project recovery` folders are skipped: they hold recursive copies
/// of the library tree plus saved presets, not library content.
fn walk(dir: &Path, files: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if dir.is_file() {
        files.push(dir.to_owned());
        return;
    }
    if dir
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains("KONTRA project recovery"))
        || !seen.insert(dir.canonicalize().unwrap_or_else(|_| dir.to_owned()))
    {
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, files, seen);
        } else if path.extension().is_some_and(|e| {
            ["nki", "nkm", "ufs", "uvip"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        }) {
            files.push(path);
        }
    }
}

fn items(roots: &[PathBuf]) -> Vec<Item> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        walk(root, &mut files, &mut seen);
    }
    files.sort();
    files.dedup();
    let mut items = Vec::new();
    for path in files {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "nki" => items.push(Item::Kontakt(path)),
            "nkm" => items.push(Item::Multi(path)),
            "uvip" => items.push(Item::UviLoose(path)),
            "ufs" => match sampler_uvi::Bank::open(&path) {
                Ok(bank) => {
                    items.extend(bank.programs().into_iter().map(|program| Item::UviProgram {
                        bank: path.clone(),
                        program,
                    }))
                }
                // An unopenable bank is one failed item.
                Err(_) => items.push(Item::UviLoose(path)),
            },
            _ => {}
        }
    }
    items
}

/// Strip paths and numbers so equal causes group together.
fn normalize(message: &str) -> String {
    let words: Vec<String> = message
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .map(|word| {
            if word.contains('/') || word.contains('\\') {
                "<path>".to_string()
            } else {
                let mut out = String::new();
                let mut digits = false;
                for c in word.chars() {
                    if c.is_ascii_digit() {
                        if !digits {
                            out.push('N');
                        }
                        digits = true;
                    } else {
                        digits = false;
                        out.push(c);
                    }
                }
                out
            }
        })
        .collect();
    let text = words.join(" ");
    text.chars().take(140).collect()
}

/// What to play: a key, a velocity, and an articulation switch key to tap first.
#[derive(Clone, Copy)]
struct Pick {
    key: u8,
    velocity: u8,
    switch: Option<u8>,
}

impl Pick {
    fn warning(&self) -> Option<String> {
        match (self.switch, self.velocity != VELOCITY) {
            (Some(s), _) => Some(format!(
                "played at vel {} after switch key {s} (no plain zone at {VELOCITY})",
                self.velocity
            )),
            (None, true) => Some(format!(
                "played at vel {} (no zone at {VELOCITY})",
                self.velocity
            )),
            _ => None,
        }
    }
}

/// The key most zones cover at the test velocity, nearest middle C, outside
/// the articulation switch keys. Falls back to the velocity nearest 64 that
/// any zone covers, then to playing a switch key first.
fn pick_key(ir: &sampler_ir::Instrument) -> Option<Pick> {
    let switch: HashSet<u8> = ir
        .articulations
        .iter()
        .flat_map(|a| a.switch_keys.iter().copied())
        .collect();
    let best = |v: u8, allow_switch: bool| {
        (0..=127u8)
            .filter(|k| allow_switch || !switch.contains(k))
            .map(|k| {
                let count = ir
                    .zones
                    .iter()
                    .filter(|z| {
                        (z.keys.low..=z.keys.high).contains(&k)
                            && (z.velocities.low..=z.velocities.high).contains(&v)
                    })
                    .count();
                (count, std::cmp::Reverse(k.abs_diff(60)), k)
            })
            .filter(|(count, ..)| *count > 0)
            .max()
            .map(|(.., k)| k)
    };
    let mut velocities: Vec<u8> = (1..=127).collect();
    velocities.sort_by_key(|v| (v.abs_diff(VELOCITY), *v));
    for v in &velocities {
        if let Some(key) = best(*v, false) {
            return Some(Pick { key, velocity: *v, switch: None });
        }
    }
    let first = ir.articulations.iter().flat_map(|a| a.switch_keys.iter().copied()).next()?;
    for v in &velocities {
        if let Some(key) = best(*v, true) {
            return Some(Pick { key, velocity: *v, switch: Some(first) });
        }
    }
    None
}

/// The error type's variant name, taken from its `Debug` form.
fn kind_of(debug: &str) -> String {
    debug
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

fn categories(unsupported: &[sampler_ir::Unsupported]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for u in unsupported {
        *map.entry(normalize(&u.feature)).or_insert(0) += 1;
    }
    map
}

/// What a load produced: a Kontakt/UVI-loose plan played through MIDI ingress,
/// or a UVI program whose Lua scripts run through `scripted::Player`.
enum Subject {
    Plan(sampler_kontakt::Loaded),
    Scripted(sampler_uvi::scripted::Program),
}

impl Subject {
    fn instrument(&self) -> &sampler_ir::Instrument {
        match self {
            Subject::Plan(l) => &l.instrument,
            Subject::Scripted(p) => &p.instrument,
        }
    }
}

enum Rig {
    Midi(Box<Runtime>, Ingress),
    Scripted(Box<sampler_uvi::scripted::Player>),
}

struct Sound {
    selection: Value,
    perf: Value,
    note: String,
    peak: f32,
    finite: bool,
    stuck_voices: usize,
    stuck_notes: usize,
    tail_peak: f32,
    faults: Vec<String>,
}

/// Per-reason counts over every candidate region, plus the first records.
fn selection_summary(records: Vec<sampler_core::SelectionRecord>) -> Value {
    let mut counts = BTreeMap::<String, usize>::new();
    let (mut suppressed, mut unmapped) = (0, 0);
    for r in &records {
        suppressed += usize::from(r.suppressed);
        unmapped += usize::from(r.candidates.is_empty() && !r.suppressed);
        for c in &r.candidates {
            let reason = c.rejected.map_or("accepted".to_string(), |x| format!("{x:?}"));
            *counts.entry(reason).or_default() += 1;
        }
    }
    let first: Vec<Value> = records
        .iter()
        .take(3)
        .map(|r| {
            let candidates: Vec<Value> = r
                .candidates
                .iter()
                .take(6)
                .map(|c| json!({"region": c.region, "group": c.group, "rejected": c.rejected.map(|x| format!("{x:?}"))}))
                .collect();
            json!({"key": r.key, "velocity": r.velocity, "trigger": format!("{:?}", r.trigger), "suppressed": r.suppressed, "candidates": r.candidates.len(), "first": candidates})
        })
        .collect();
    json!({"records": records.len(), "suppressed": suppressed, "key_unmapped": unmapped, "verdicts": counts, "first": first})
}

/// A fresh runtime for `item` with `ccs` already applied, for the MPE probe.
fn fresh_runtime(item: &Item, ccs: &[(u8, u8)]) -> Option<Runtime> {
    let (_, Some((Subject::Plan(loaded), _))) = load_item(item) else {
        return None;
    };
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
        behavior_cells: loaded.plan.behavior_local_count().saturating_mul(16),
        note_cells: loaded.plan.note_cell_count().saturating_mul(64),
    };
    let mut rt = Runtime::new(loaded.plan, limits).ok()?;
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    for &(cc, v) in ccs {
        let word = [0x20B0_0000 | u32::from(cc) << 8 | u32::from(v)];
        let packet = Packets::new(&word).next()?.ok()?;
        ingress.apply(&mut rt, packet).ok()?;
    }
    Some(rt)
}

/// Does a note on an MPE member channel follow per-note bend and pressure?
fn mpe_probe(item: &Item, pick: Pick, ccs: &[(u8, u8)]) -> Value {
    if fresh_runtime(item, ccs).is_none() {
        return json!({"error": "no runtime"});
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sampler_midi::mpe_response(
            || fresh_runtime(item, ccs).expect("runtime built once already"),
            pick.key,
        )
    }));
    match result {
        Ok(Ok(r)) => json!({
            "pitch_ratio": r.pitch_ratio,
            "pressure_db": r.pressure_db,
            "pitch_responds": r.pitch_responds(),
            "pressure_responds": r.pressure_responds(),
        }),
        Ok(Err(e)) => json!({"error": normalize(&format!("{e:?}"))}),
        Err(_) => json!({"error": "panic"}),
    }
}

/// Controllers a player would have up: mod wheel, expression and CC2 high,
/// plus every plain controller the instrument's modulators read. (Controllers
/// scripts read directly are not listed in the IR, so they are not covered.)
fn musical_ccs(ir: &sampler_ir::Instrument) -> Vec<(u8, u8)> {
    let mut ccs = vec![(1, 100), (2, 100), (11, 127)];
    for m in &ir.modulators {
        if let sampler_ir::ModulationSource::Controller(n) = m.source {
            let plain = n < 120 && ![0, 6, 32, 38, 64, 65, 66, 67, 68, 69, 98, 99, 100, 101].contains(&n);
            if plain && !ccs.iter().any(|c| c.0 == n) {
                ccs.push((n, 100));
            }
        }
    }
    ccs
}

fn play(subject: Subject, pick: Pick, diagnose: bool, ccs: &[(u8, u8)]) -> Result<Sound, String> {
    let (plan_rate, behavior_locals, note_cells) = match &subject {
        Subject::Plan(l) => (
            l.plan.sample_rate(),
            l.plan.behavior_local_count(),
            l.plan.note_cell_count(),
        ),
        Subject::Scripted(p) => (p.plan.sample_rate(), 0, 0),
    };
    let rate = plan_rate;
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
        behavior_cells: behavior_locals.saturating_mul(16),
        note_cells: note_cells.saturating_mul(64),
    };
    let mut rig = match subject {
        Subject::Plan(loaded) => {
            let mut rt = Runtime::new(loaded.plan, limits)
                .map_err(|e| format!("prepare: runtime: {e}"))?;
            rt.set_voice_stealing(Some(sampler_core::Stealing::for_limits(
                rt.sample_rate(),
                limits.voices,
            )))
            .map_err(|e| format!("prepare: runtime: {e}"))?;
            rt.record_selections(diagnose);
            let mut groups = [None; 16];
            groups[0] = Some(Version::Midi1);
            Rig::Midi(Box::new(rt), Ingress::new(0, groups))
        }
        Subject::Scripted(program) => Rig::Scripted(Box::new(
            sampler_uvi::scripted::Player::new(program, limits, rate)
                .map_err(|e| format!("prepare: player: {e}"))?,
        )),
    };
    let frame = |seconds: f64| (seconds * f64::from(rate)).round() as usize;
    let release_at = frame(HOLD_SECONDS);
    let total = release_at + frame(TAIL_SECONDS);
    let key = pick.key;
    let on = [0x2090_0000 | u32::from(key) << 8 | u32::from(pick.velocity)];
    let off = [0x2080_0000 | u32::from(key) << 8];
    let on = Packets::new(&on)
        .next()
        .unwrap()
        .map_err(|e| format!("{e:?}"))?;
    let off = Packets::new(&off)
        .next()
        .unwrap()
        .map_err(|e| format!("{e:?}"))?;
    let mut buffer = [[0.0f32; 2]; 64];
    let deadline = buffer.len() as f64 / f64::from(rate);
    let mut block_times: Vec<f64> = Vec::with_capacity(total / buffer.len() + 1);
    let (mut peak_voices, mut audio_allocs) = (0usize, 0usize);
    let (mut peak, mut tail_peak, mut finite) = (0.0f32, 0.0f32, true);
    let sw = u32::from(pick.switch.unwrap_or(0)) << 8;
    // Controllers first, then the switch key taps, then the note.
    let mut switch_words: Vec<[u32; 1]> = ccs
        .iter()
        .map(|&(cc, v)| [0x20B0_0000 | u32::from(cc) << 8 | u32::from(v)])
        .collect();
    if pick.switch.is_some() {
        switch_words.push([0x2090_0000 | sw | 64]);
        switch_words.push([0x2080_0000 | sw]);
    }
    let note_index = switch_words.len();
    let mut note = String::from("not sent");
    let mut faults = Vec::new();
    for begin in (0..total).step_by(buffer.len()) {
        let len = buffer.len().min(total - begin);
        let (t0, a0);
        match &mut rig {
            Rig::Midi(rt, ingress) => {
                let mut batch = Vec::new();
                if begin == 0 {
                    for word in &switch_words {
                        batch.push(TimedPacket {
                            offset: 0,
                            packet: Packets::new(word)
                                .next()
                                .unwrap()
                                .map_err(|e| format!("{e:?}"))?,
                        });
                    }
                    batch.push(TimedPacket {
                        offset: 0,
                        packet: on,
                    });
                }
                if (begin..begin + len).contains(&release_at) {
                    batch.push(TimedPacket {
                        offset: release_at - begin,
                        packet: off,
                    });
                }
                (t0, a0) = (Instant::now(), heap::calls());
                ingress
                    .render(
                        rt,
                        &mut buffer[..len],
                        &batch,
                        batch.len(),
                        |i, result| {
                            if begin == 0 && i == note_index {
                                note = match result {
                                    Ok(Applied::Started(_)) => "started".into(),
                                    other => format!("{other:?}"),
                                };
                            }
                        },
                    )
                    .map_err(|e| format!("render: {e:?}"))?;
            }
            Rig::Scripted(player) => {
                if begin == 0 {
                    note = match player.note_on(key, f64::from(pick.velocity) / 127.0) {
                        Ok(()) => "started".into(),
                        Err(e) => format!("{e:?}"),
                    };
                }
                (t0, a0) = (Instant::now(), heap::calls());
                let cut = if (begin..begin + len).contains(&release_at) {
                    release_at - begin
                } else {
                    len
                };
                player
                    .render(&mut buffer[..cut])
                    .map_err(|e| format!("render: {e:?}"))?;
                if cut < len {
                    player.note_off(key).map_err(|e| format!("release: {e:?}"))?;
                    player
                        .render(&mut buffer[cut..len])
                        .map_err(|e| format!("render: {e:?}"))?;
                }
            }
        }
        block_times.push(t0.elapsed().as_secs_f64());
        audio_allocs += heap::calls() - a0;
        let rt: &mut Runtime = match &mut rig {
            Rig::Midi(rt, _) => rt,
            Rig::Scripted(_) => {
                peak_voices = peak_voices.max(match &rig {
                    Rig::Scripted(p) => p.runtime().voice_count(),
                    _ => 0,
                });
                for x in buffer[..len].iter().flatten() {
                    finite &= x.is_finite();
                    peak = peak.max(x.abs());
                    if begin + len > total - frame(1.0) {
                        tail_peak = tail_peak.max(x.abs());
                    }
                }
                continue;
            }
        };
        peak_voices = peak_voices.max(rt.voice_count());
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) && faults.len() < 8 {
                faults.push(normalize(&format!("{outcome:?}")));
            }
            true
        });
        rt.flush_ended(|_| true);
        for x in buffer[..len].iter().flatten() {
            finite &= x.is_finite();
            peak = peak.max(x.abs());
            // The last second of the five-second tail.
            if begin + len > total - frame(1.0) {
                tail_peak = tail_peak.max(x.abs());
            }
        }
    }
    let selection = match &mut rig {
        Rig::Midi(rt, _) if diagnose => selection_summary(rt.take_selection_records()),
        _ => Value::Null,
    };
    let rt: &Runtime = match &rig {
        Rig::Midi(rt, _) => rt,
        Rig::Scripted(p) => p.runtime(),
    };
    block_times.sort_by(f64::total_cmp);
    let q = |f: f64| block_times[((block_times.len() - 1) as f64 * f) as usize];
    let st = rt.stats();
    let perf = json!({
        "block_frames": buffer.len(),
        "deadline_ms": deadline * 1e3,
        "block_p50_ms": q(0.5) * 1e3,
        "block_p99_ms": q(0.99) * 1e3,
        "block_max_ms": q(1.0) * 1e3,
        "deadline_misses": block_times.iter().filter(|t| **t > deadline).count(),
        "blocks": block_times.len(),
        "peak_voices": peak_voices,
        "audio_thread_allocs": audio_allocs,
        "stream_underruns": st.stream_underruns,
        "cold_starts": st.cold_starts,
        "voice_drops": st.voice_drops,
        "resident_bytes": rt.resident_bytes(),
        "stream_cache_bytes": st.stream_cache_bytes,
    });
    Ok(Sound {
        selection,
        perf,
        note,
        peak,
        finite,
        stuck_voices: rt.voice_count(),
        stuck_notes: rt.note_count(),
        tail_peak,
        faults,
    })
}

fn scripts(subject: &Subject) -> Value {
    let ir = subject.instrument();
    let failed: Vec<String> = ir
        .unsupported
        .iter()
        .filter(|u| u.feature == "script")
        .map(|u| normalize(&u.value))
        .collect();
    let mut warnings = BTreeMap::<String, usize>::new();
    for u in &ir.unsupported {
        if let Some(kind) = u.feature.strip_prefix("script ") {
            let kind = kind.split(':').next().unwrap_or(kind);
            *warnings.entry(kind.to_string()).or_default() += 1;
        }
    }
    let bound = match subject {
        Subject::Plan(l) => l.scripts.len(),
        Subject::Scripted(p) => usize::from(p.host.handles_notes()),
    };
    json!({
        "declared": ir.behaviors.len(),
        "bound": bound,
        "compile_failed": failed.len(),
        "compile_errors": failed.into_iter().take(3).collect::<Vec<_>>(),
        "warnings": warnings,
    })
}

fn load_item(item: &Item) -> (Value, Option<(Subject, Pick)>) {
    let failed = |stage: &str, kind: String, reason: String| {
        (
            json!({"ok": false, "stage": stage, "kind": kind, "error": normalize(&reason), "raw": reason.lines().next().unwrap_or("").chars().take(300).collect::<String>()}),
            None,
        )
    };
    match item {
        Item::Kontakt(path) => {
            let ir = match sampler_kontakt::read(path) {
                Ok(read) => read.instrument,
                Err(e) => return failed("parse", kind_of(&format!("{e:?}")), e.to_string()),
            };
            let Some(pick) = pick_key(&ir) else {
                return failed("note-on/selection", "NoZone".into(), "no zone covers any key at any velocity".into());
            };
            let options = sampler_kontakt::Options {
                keys: pick.key..=pick.key,
                scripts: true,
                ..Default::default()
            };
            match sampler_kontakt::load(path, &options, |_| {}) {
                Ok(loaded) => (
                    json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": loaded.instrument.zones.len()}),
                    Some((Subject::Plan(loaded), pick)),
                ),
                Err(e) => failed("load", kind_of(&format!("{e:?}")), e.to_string()),
            }
        }
        Item::UviProgram { bank, program } => {
            let virtual_path = bank.join(program);
            let ir = match sampler_uvi::translate_path(&virtual_path) {
                Ok(t) => t.instrument,
                Err(e) => return failed("translate-to-IR", kind_of(&format!("{e:?}")), e.to_string()),
            };
            let Some(pick) = pick_key(&ir) else {
                return failed("note-on/selection", "NoZone".into(), "no zone covers any key at any velocity".into());
            };
            let bank = match sampler_uvi::Bank::open(bank) {
                Ok(b) => b,
                Err(e) => return failed("container/decrypt", kind_of(&format!("{e:?}")), e.to_string()),
            };
            // Scripts may play keys outside the picked one: keep every zone.
            let options = sampler_kontakt::Options::default();
            match sampler_uvi::load_program_scripted_with_options(&bank, program, &options) {
                Ok(program) => (
                    json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": program.instrument.zones.len()}),
                    Some((Subject::Scripted(program), pick)),
                ),
                Err(e) => failed("load", kind_of(&format!("{e:?}")), e.to_string()),
            }
        }
        Item::UviLoose(path) => {
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
            {
                return failed("container/decrypt", "BankOpen".into(), "bank does not open".into());
            }
            match sampler_uvi::load(path, 48000) {
                Ok(loaded) => match pick_key(&loaded.instrument) {
                    Some(pick) => (
                        json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": loaded.instrument.zones.len()}),
                        Some((Subject::Plan(loaded), pick)),
                    ),
                    None => failed("note-on/selection", "NoZone".into(), "no zone covers any key at any velocity".into()),
                },
                Err(e) => failed("load", kind_of(&format!("{e:?}")), e.to_string()),
            }
        }
        Item::Multi(path) => match sampler_kontakt::read_multi(path) {
            Ok(multi) if multi.sample_names.is_empty() => {
                failed("parse", "EmptyMulti".into(), "no sample references in multi".into())
            }
            Ok(multi) => (
                json!({"ok": true, "programs": multi.programs.len(), "samples": multi.sample_names.len()}),
                None,
            ),
            Err(e) => failed("parse", kind_of(&format!("{e:?}")), e.to_string()),
        },
    }
}

/// The pipeline stage a record ended in: the failing stage, or the last one
/// reached. Stages inside `sampler_kontakt::load` are not separable until its
/// errors carry them, so those report as `load`.
/// The tail is measured 4 to 5 s after release: a stored release that long
/// explains it.
fn long_release(r: &Value) -> bool {
    r["stored_release_s"].as_f64().is_some_and(|s| s >= 4.0)
}

fn stage_of(r: &Value) -> &'static str {
    if r["load"]["ok"] != true {
        return match r["load"]["stage"].as_str() {
            Some("parse") => "parse",
            Some("translate-to-IR") => "translate-to-IR",
            Some("container/decrypt") => "container/decrypt",
            Some("note-on/selection") => "note-on/selection",
            _ => "load",
        };
    }
    if r["kind"] == "kontakt-multi" {
        return "ok";
    }
    if r["scripts"]["compile_failed"].as_u64().unwrap_or(0) > 0 {
        return "script-compile";
    }
    let s = &r["sound"];
    if let Some(e) = s["error"].as_str() {
        return if e.starts_with("prepare") { "prepare" } else { "render" };
    }
    if s["sounds"] != true && r["musical"]["sounds"] == true {
        return "needs-controller";
    }
    if s["note"] != "started" || s["sounds"] != true {
        return "note-on/selection";
    }
    if s["finite"] == false || !s["script_faults"].as_array().is_none_or(Vec::is_empty) {
        return "render";
    }
    if s["stuck_voices"].as_u64().unwrap_or(0) > 0
        && s["tail_peak_db"].as_f64().is_some_and(|d| d > -60.0)
        && !long_release(r)
    {
        return "release";
    }
    "ok"
}

fn check(item: &Item) -> Value {
    reset_peaks();
    let (io0, (minflt0, majflt0)) = (proc_field("/proc/self/io", "read_bytes:"), faults());
    let start = Instant::now();
    let (load, loaded) = load_item(item);
    let load_ms = start.elapsed().as_millis() as u64;
    let heap_after_load = heap::live();
    let mut record = json!({
        "id": item.id(),
        "kind": item.kind(),
        "status": "done",
        "load": load,
    });
    if let Some((loaded, pick)) = loaded {
        record["scripts"] = scripts(&loaded);
        record["unsupported"] = json!(categories(&loaded.instrument().unsupported));
        record["unsupported_total"] = json!(loaded.instrument().unsupported.len());
        // Longest release any envelope stores: a tail up to that long is data.
        record["stored_release_s"] = json!(
            loaded
                .instrument()
                .modulators
                .iter()
                .filter_map(|m| match &m.source {
                    sampler_ir::ModulationSource::Envelope(e) if !e.one_shot => {
                        Some(e.release.seconds())
                    }
                    _ => None,
                })
                .fold(0.0f64, f64::max)
        );
        let ccs = musical_ccs(loaded.instrument());
        {
            let m = sampler_kontakt::articulation_migration(loaded.instrument());
            record["articulation"] = json!({
                "switches_found": m.switches_found,
                "migrated": m.migrated,
                "unrecognised": m.unrecognised.iter().take(5).map(|u| normalize(u)).collect::<Vec<_>>(),
            });
        }
        let silent = |s: &Sound| s.peak <= 1e-4 || s.note != "started";
        let first = play(loaded, pick, false, &[]);
        let first = match first {
            Ok(mut s) if silent(&s) && item.kind() == "kontakt" => {
                // Selection records allocate, so they only run on a second pass
                // over an item that was silent.
                if let (_, Some((again, pick))) = load_item(item)
                    && let Ok(d) = play(again, pick, true, &[]) {
                        s.selection = d.selection;
                    }
                Ok(s)
            }
            other => other,
        };
        // The same note again with the controllers up; if only that sounds, find
        // the controller it needs.
        if let Ok(d) = &first
            && item.kind() == "kontakt" {
                let again = |ccs: &[(u8, u8)]| match load_item(item) {
                    (_, Some((subject, pick))) => play(subject, pick, false, ccs).ok(),
                    _ => None,
                };
                if let Some(m) = again(&ccs) {
                    let mut musical = json!({
                        "ccs": ccs.iter().map(|c| json!([c.0, c.1])).collect::<Vec<_>>(),
                        "peak_db": if m.peak > 0.0 { json!(20.0 * f64::from(m.peak).log10()) } else { Value::Null },
                        "sounds": m.peak > 1e-4,
                        "note": m.note,
                    });
                    if silent(d) && m.peak > 1e-4 {
                        let alone = ccs.iter().find(|c| {
                            again(&[**c]).is_some_and(|x| x.peak > 1e-4)
                        });
                        musical["needs_cc"] = match alone {
                            Some(c) => json!([c.0]),
                            None => json!(ccs.iter().map(|c| c.0).collect::<Vec<_>>()),
                        };
                    }
                    record["musical"] = musical;
                    record["mpe"] = mpe_probe(item, pick, &ccs);
                }
            }
        match first {
            Ok(s) => {
                let db = |p: f32| {
                    if p > 0.0 {
                        json!(20.0 * f64::from(p).log10())
                    } else {
                        Value::Null
                    }
                };
                record["sound"] = json!({
                    "perf": s.perf,
                    "selection": s.selection,
                    "note": s.note,
                    "peak_db": db(s.peak),
                    "sounds": s.peak > 1e-4,
                    "finite": s.finite,
                    "stuck_voices": s.stuck_voices,
                    "stuck_notes": s.stuck_notes,
                    "tail_peak_db": db(s.tail_peak),
                    "script_faults": s.faults,
                });
            }
            Err(e) => record["sound"] = json!({"error": normalize(&e)}),
        }
    }
    record["stage"] = json!(stage_of(&record));
    record["perf"] = json!({
        "load_ms": load_ms,
        "peak_rss_kib": proc_field("/proc/self/status", "VmHWM:"),
        "peak_heap_bytes": heap::peak(),
        "heap_bytes_after_load": heap_after_load,
        // Timing is evidence only when the machine was otherwise idle: judge it
        // by the 1-minute load average at the end of the item.
        "loadavg1": std::fs::read_to_string("/proc/loadavg").ok().and_then(|l| l.split_whitespace().next()?.parse::<f64>().ok()),
        "disk_read_bytes": proc_field("/proc/self/io", "read_bytes:") - io0,
        "minor_faults": faults().0 - minflt0,
        "major_faults": faults().1 - majflt0,
        "render": record["sound"]["perf"].clone(),
    });
    record["load_ms"] = json!(load_ms);
    record["ms"] = json!(start.elapsed().as_millis() as u64);
    record
}

fn read_records(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn run(out: &Path, shard: Option<(usize, usize)>, explicit: &[String]) {
    let mut all = items(&roots(explicit));
    if let Some((i, n)) = shard {
        all = all.into_iter().skip(i).step_by(n).collect();
    }
    let mut finished = HashSet::new();
    let mut crashed = Vec::new();
    // Results from sibling shard files of earlier runs are reused, so changing
    // the item list or shard count does not redo finished work.
    if let Some(dir) = out.parent() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let sibling = entry.path();
            if sibling != out && sibling.extension().is_some_and(|e| e == "jsonl") {
                for r in read_records(&sibling) {
                    if r["status"] != "started" {
                        finished.insert(r["id"].as_str().unwrap_or_default().to_string());
                    }
                }
            }
        }
    }
    let records = read_records(out);
    let mut started = HashSet::new();
    for r in &records {
        let id = r["id"].as_str().unwrap_or_default().to_string();
        match r["status"].as_str() {
            Some("started") => {
                started.insert(id);
            }
            _ => {
                finished.insert(id);
            }
        }
    }
    for id in &started {
        if !finished.contains(id) {
            crashed.push(id.clone());
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
        .expect("open output");
    for id in crashed {
        let record = json!({"id": id, "kind": "unknown", "status": "crash", "load": {"ok": false, "error": "process died (abort, OOM or hang)"}});
        writeln!(file, "{record}").unwrap();
        finished.insert(id);
    }
    std::panic::set_hook(Box::new(|_| {}));
    let total = all.len();
    for (n, item) in all.into_iter().enumerate() {
        let id = item.id();
        if finished.contains(&id) {
            continue;
        }
        writeln!(file, "{}", json!({"id": id, "status": "started"})).unwrap();
        file.flush().unwrap();
        let record = std::panic::catch_unwind(|| check(&item)).unwrap_or_else(|payload| {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            json!({"id": id, "kind": item.kind(), "status": "panic", "load": {"ok": false, "error": normalize(&format!("panic: {message}"))}})
        });
        writeln!(file, "{record}").unwrap();
        file.flush().unwrap();
        eprintln!("[{}/{total}] {} {}", n + 1, record["status"], id);
    }
}

/// One `OUT.jsonl`, or every `*.jsonl` in a directory (the shard files).
fn all_records(out: &Path) -> Vec<Value> {
    if !out.is_dir() {
        return read_records(out);
    }
    let mut files: Vec<_> = std::fs::read_dir(out)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    files.sort();
    let mut seen = HashSet::new();
    files
        .iter()
        .flat_map(|f| read_records(f))
        // A result supersedes a crash marker only if it came later; keep the first
        // finished record per id.
        .filter(|r| {
            r["status"] == "started" || seen.insert(r["id"].as_str().unwrap_or("").to_string())
        })
        .collect()
}

fn summary(out: &Path, md: &Path) {
    let records: Vec<Value> = all_records(out)
        .into_iter()
        .filter(|r| r["status"] != "started")
        .filter(|r| {
            !r["id"]
                .as_str()
                .unwrap_or("")
                .contains("KONTRA project recovery")
        })
        .collect();
    let total = records.len();
    let mut by_kind = BTreeMap::<String, [usize; 5]>::new(); // total, loaded, sounds, scripts failed, clean
    let mut reasons = BTreeMap::<String, usize>::new();
    let mut unsupported = BTreeMap::<String, usize>::new();
    let mut warnings = BTreeMap::<String, usize>::new();
    let mut peak_db = Vec::new();
    for r in &records {
        let kind = r["kind"].as_str().unwrap_or("?").to_string();
        let row = by_kind.entry(kind).or_default();
        row[0] += 1;
        let mut reason_set = HashSet::new();
        if r["load"]["ok"] == true {
            row[1] += 1;
        } else {
            reason_set.insert(format!(
                "load: {}",
                r["load"]["error"].as_str().unwrap_or("unknown")
            ));
        }
        let failed = r["scripts"]["compile_failed"].as_u64().unwrap_or(0);
        if failed > 0 {
            row[3] += 1;
            for e in r["scripts"]["compile_errors"]
                .as_array()
                .into_iter()
                .flatten()
                .take(1)
            {
                reason_set.insert(format!("script compile: {}", e.as_str().unwrap_or("")));
            }
        }
        if let Some(w) = r["scripts"]["warnings"].as_object() {
            for (k, v) in w {
                *warnings.entry(k.clone()).or_default() += usize::from(v.as_u64().unwrap_or(0) > 0);
            }
        }
        let sound = &r["sound"];
        if !sound.is_null() {
            if sound["error"].is_string() {
                reason_set.insert(format!("play: {}", sound["error"].as_str().unwrap()));
            } else {
                if sound["note"] != "started" {
                    reason_set.insert(format!(
                        "note not started: {}",
                        normalize(sound["note"].as_str().unwrap_or(""))
                    ));
                } else if sound["sounds"] == true {
                    row[2] += 1;
                    if let Some(db) = sound["peak_db"].as_f64() {
                        peak_db.push(db);
                    }
                } else if r["musical"]["sounds"] == true {
                    // Sounds once the controllers are up: not real silence.
                    row[2] += 1;
                    let needs: Vec<String> = r["musical"]["needs_cc"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|c| format!("CC{c}"))
                        .collect();
                    reason_set.insert(format!("needs controller: {}", needs.join(",")));
                } else {
                    reason_set.insert("silent at a covered key".into());
                }
                if sound["finite"] == false {
                    reason_set.insert("non-finite samples".into());
                }
                // Long quiet tails are not stuck: only audible (> -60 dBFS) output
                // 5 s after release counts. (A 30 s liveness check is not recorded.)
                if sound["stuck_voices"].as_u64().unwrap_or(0) > 0
                    && sound["tail_peak_db"].as_f64().is_some_and(|d| d > -60.0)
                {
                    reason_set.insert(if long_release(r) {
                        "long release (stored), not a fault".into()
                    } else {
                        "audible output 5 s after release (> -60 dBFS)".into()
                    });
                }
                for f in sound["script_faults"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(1)
                {
                    reason_set.insert(format!("script fault: {}", f.as_str().unwrap_or("")));
                }
            }
        }
        if reason_set.is_empty() {
            row[4] += 1;
        }
        for reason in reason_set {
            *reasons.entry(reason).or_default() += 1;
        }
        for k in r["unsupported"]
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys())
        {
            *unsupported.entry(k.clone()).or_default() += 1;
        }
    }
    let top = |map: &BTreeMap<String, usize>, n: usize| {
        let mut rows: Vec<_> = map.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        rows.into_iter()
            .take(n)
            .map(|(k, v)| format!("| {v} | {k} |"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut text = format!("# Corpus health\n\n{total} instruments and programs checked.\n\n");
    text += "| kind | total | loads | sounds | script compile failed | no failure |\n|---|---|---|---|---|---|\n";
    let mut sum = [0usize; 5];
    for (kind, row) in &by_kind {
        text += &format!(
            "| {kind} | {} | {} | {} | {} | {} |\n",
            row[0], row[1], row[2], row[3], row[4]
        );
        for i in 0..5 {
            sum[i] += row[i];
        }
    }
    text += &format!(
        "| all | {} | {} | {} | {} | {} |\n\n",
        sum[0], sum[1], sum[2], sum[3], sum[4]
    );
    peak_db.sort_by(|a, b| a.total_cmp(b));
    if let Some(median) = peak_db.get(peak_db.len() / 2) {
        text += &format!("Median peak of sounding instruments: {median:.1} dBFS.\n\n");
    }
    text +=
        "## Top failure reasons (instruments affected)\n\n| instruments | reason |\n|---|---|\n";
    text += &top(&reasons, 25);
    text += "\n\n## Top unsupported features in the load report (instruments affected)\n\n| instruments | feature |\n|---|---|\n";
    text += &top(&unsupported, 25);
    text += "\n\n## Script diagnostics (instruments with at least one)\n\n| instruments | kind |\n|---|---|\n";
    text += &top(&warnings, 10);
    text += "\n\n## By pipeline stage (where each record ended)\n\n| instruments | stage |\n|---|---|\n";
    let mut stages = BTreeMap::<String, usize>::new();
    let (mut sw, mut mig, mut mpe_n, mut pitch, mut press) = (0, 0, 0, 0, 0);
    let (mut misses, mut allocs, mut loads) = (0, 0, Vec::<f64>::new());
    for r in &records {
        *stages.entry(r["stage"].as_str().unwrap_or("(none)").to_string()).or_default() += 1;
        sw += r["articulation"]["switches_found"].as_u64().unwrap_or(0);
        mig += r["articulation"]["migrated"].as_u64().unwrap_or(0);
        if r["mpe"]["pitch_ratio"].is_number() {
            mpe_n += 1;
            pitch += usize::from(r["mpe"]["pitch_responds"] == true);
            press += usize::from(r["mpe"]["pressure_responds"] == true);
        }
        let p = &r["perf"];
        misses += usize::from(p["render"]["deadline_misses"].as_u64().unwrap_or(0) > 0);
        allocs += usize::from(p["render"]["audio_thread_allocs"].as_u64().unwrap_or(0) > 0);
        if let Some(ms) = p["load_ms"].as_f64() {
            loads.push(ms);
        }
    }
    text += &top(&stages, 20);
    loads.sort_by(f64::total_cmp);
    let q = |f: f64| loads.get(((loads.len().max(1) - 1) as f64 * f) as usize).copied().unwrap_or(0.0);
    text += &format!(
        "\n\n## Probes and performance\n\n- keyswitch articulations found {sw}, migrated to zone selectors {mig}\n- MPE (instruments probed {mpe_n}): pitch responds {pitch}, pressure responds {press}\n- instruments with at least one 64-frame deadline miss: {misses} (timing is only evidence when `loadavg1` is low)\n- instruments allocating on the audio thread: {allocs}\n- load time p50 {:.0} ms, p99 {:.0} ms, max {:.0} ms\n",
        q(0.5),
        q(0.99),
        q(1.0)
    );
    std::fs::write(md, text).expect("write summary");
}

/// `diff OLD NEW`: what a change fixed, broke, or made slower. Each side is a
/// `.jsonl` or a directory of shard files. Perf deltas above 25 % and above an
/// absolute floor (50 ms load, 32 MiB RSS) are listed.
fn diff(old: &Path, new: &Path) {
    let index = |p: &Path| -> BTreeMap<String, Value> {
        all_records(p)
            .into_iter()
            .map(|r| (r["id"].as_str().unwrap_or_default().to_string(), r))
            .collect()
    };
    let stage = |r: &Value| match r["stage"].as_str() {
        Some(s) => s.to_string(),
        None if r["status"] != "done" => "crash".into(),
        None => stage_of(r).into(),
    };
    let (old, new) = (index(old), index(new));
    let (mut fixed, mut regressed, mut added, mut moved, mut slow, mut fat) =
        (vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut ok = [0usize; 2];
    for (id, n) in &new {
        let ns = stage(n);
        ok[1] += usize::from(ns == "ok");
        match old.get(id) {
            None if ns != "ok" => added.push(format!("{id}  [{ns}]")),
            None => {}
            Some(o) => {
                let os = stage(o);
                match (os == "ok", ns == "ok") {
                    (false, true) => fixed.push(format!("{id}  [{os} -> ok]")),
                    (true, false) => regressed.push(format!("{id}  [ok -> {ns}]")),
                    _ if os != ns => moved.push(format!("{id}  [{os} -> {ns}]")),
                    _ => {}
                }
                let f = |r: &Value, a: &str, b: &str| r["perf"][a].as_f64().or(r[b].as_f64());
                if let (Some(a), Some(b)) = (f(o, "load_ms", "load_ms"), f(n, "load_ms", "load_ms"))
                    && b > a * 1.25 && b - a > 50.0 {
                        slow.push(format!("{id}  load {a:.0} -> {b:.0} ms"));
                    }
                if let (Some(a), Some(b)) = (
                    o["perf"]["peak_rss_kib"].as_f64(),
                    n["perf"]["peak_rss_kib"].as_f64(),
                )
                    && b > a * 1.25 && b - a > 32768.0 {
                        fat.push(format!("{id}  rss {:.0} -> {:.0} MiB", a / 1024.0, b / 1024.0));
                    }
            }
        }
    }
    ok[0] = old.iter().filter(|(_, r)| stage(r) == "ok").count();
    println!("items: {} -> {}; ok: {} -> {}", old.len(), new.len(), ok[0], ok[1]);
    for (name, list) in [
        ("fixed", &fixed),
        ("regressed", &regressed),
        ("new failure", &added),
        ("failing stage changed", &moved),
        ("load time up", &slow),
        ("peak RSS up", &fat),
    ] {
        println!("\n{name}: {}", list.len());
        for line in list.iter().take(40) {
            println!("  {line}");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, out, rest @ ..] if cmd == "run" => {
            let mut shard = None;
            let mut roots = Vec::new();
            let mut it = rest.iter();
            while let Some(arg) = it.next() {
                if arg == "--shard" {
                    let spec = it.next().expect("--shard I/N");
                    let (i, n) = spec.split_once('/').expect("--shard I/N");
                    shard = Some((i.parse().unwrap(), n.parse().unwrap()));
                } else {
                    roots.push(arg.clone());
                }
            }
            run(Path::new(out), shard, &roots);
        }
        [cmd, out, md] if cmd == "summary" => summary(Path::new(out), Path::new(md)),
        [cmd, old, new] if cmd == "diff" => diff(Path::new(old), Path::new(new)),
        _ => eprintln!(
            "usage: corpus_health run OUT.jsonl [--shard I/N] [ROOT ...] | summary OUT.jsonl SUMMARY.md | diff OLD NEW"
        ),
    }
}
