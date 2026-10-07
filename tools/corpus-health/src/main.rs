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
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
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

const HOLD_SECONDS: f64 = 1.5;
const TAIL_SECONDS: f64 = 0.5;
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

type Loading = (Value, Option<(Subject, Pick)>);

/// What a load produced.
enum Subject {
    /// Fully built from dummy audio (parse tier) or decoded (loose files).
    Plan(Box<sampler_kontakt::Loaded>),
    /// A Kontakt instrument or loose UVI program streamed like the host does.
    Streamed(Box<sampler_kontakt::Streamed>),
    /// A UVI program whose Lua scripts run, its samples streamed.
    Scripted(Box<sampler_uvi::scripted::Program>),
    /// Translated only (parse tier for UVI): no plan, no audio.
    Ir(Box<sampler_ir::Instrument>),
}

impl Subject {
    fn instrument(&self) -> &sampler_ir::Instrument {
        match self {
            Subject::Plan(l) => &l.instrument,
            Subject::Streamed(s) => &s.loaded.instrument,
            Subject::Scripted(p) => &p.instrument,
            Subject::Ir(i) => i,
        }
    }
    fn loaded(&self) -> Option<&sampler_kontakt::Loaded> {
        match self {
            Subject::Plan(l) => Some(l),
            Subject::Streamed(s) => Some(&s.loaded),
            _ => None,
        }
    }
}

/// What keeps a streamed plan's pages coming.
type Keep = Option<(sampler_kontakt::Streamer, Vec<sampler_core::Pcm>)>;

enum Rig {
    Midi {
        rt: Box<Runtime>,
        ingress: Ingress,
        horizon: Option<u32>,
        _keep: Keep,
    },
    Scripted {
        rt: Box<Runtime>,
        driver: Box<sampler_uvi::scripted::Driver<sampler_uvi::script::ScriptHost>>,
        horizon: Option<u32>,
        _keep: Option<sampler_uvi::scripted::Stream>,
    },
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

/// What a worker is doing, for failure records and the watchdog.
static STAGES: Mutex<[&str; 64]> = Mutex::new(["idle"; 64]);
thread_local! {
    static WORKER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn stage(name: &'static str) {
    let w = WORKER.with(std::cell::Cell::get);
    if let Ok(mut s) = STAGES.lock() {
        s[w.min(63)] = name;
    }
}
fn current_stage(worker: usize) -> &'static str {
    STAGES.lock().map_or("unknown", |s| s[worker.min(63)])
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tier {
    /// Parse, lower and bind with placeholder audio: no playing.
    Parse,
    /// Stream, play, probe.
    Full,
}

struct Ctx {
    tier: Tier,
    workers: usize,
}

fn stage_name(s: sampler_kontakt::Stage) -> &'static str {
    use sampler_kontakt::Stage::*;
    match s {
        Container => "container/decrypt",
        Parse => "parse",
        Translate => "translate-to-IR",
        SampleResolve => "sample-resolve",
        ScriptCompile => "script-compile",
        Bind => "bind",
        Prepare => "prepare",
    }
}

/// A failed load as a record: the typed stage, kind and source location when
/// the error carries them.
fn load_failure(e: &(dyn std::error::Error + 'static), fallback: &'static str) -> Loading {
    let typed = e.downcast_ref::<sampler_kontakt::LoadError>();
    let (stage, kind, at) = match typed {
        Some(t) => (
            t.stage().map_or(fallback, stage_name),
            format!("{:?}", t.kind()),
            t.location().map(|l| format!("{}:{}", l.file(), l.line())),
        ),
        None => (fallback, kind_of(&format!("{e:?}")), None),
    };
    let reason = e.to_string();
    (
        json!({"ok": false, "stage": stage, "kind": kind, "at": at, "error": normalize(&reason), "raw": reason.lines().next().unwrap_or("").chars().take(300).collect::<String>()}),
        None,
    )
}

fn failed(stage: &str, kind: &str, reason: String) -> Loading {
    (
        json!({"ok": false, "stage": stage, "kind": kind, "error": normalize(&reason), "raw": reason.lines().next().unwrap_or("").chars().take(300).collect::<String>()}),
        None,
    )
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

/// A fresh, fully decoded runtime for the MPE probe (which does not service
/// streaming), with `ccs` already applied.
fn fresh_runtime(path: &Path, key: u8, ccs: &[(u8, u8)]) -> Option<Runtime> {
    let options = sampler_kontakt::Options {
        keys: key..=key,
        scripts: true,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(path, &options, |_| {}).ok()?;
    let limits = limits_for(
        loaded.plan.behavior_local_count(),
        loaded.plan.note_cell_count(),
    );
    let mut rt = Runtime::new(loaded.plan, limits).ok()?;
    let mut ingress = new_ingress();
    for &(cc, v) in ccs {
        let word = [0x20B0_0000 | u32::from(cc) << 8 | u32::from(v)];
        let packet = Packets::new(&word).next()?.ok()?;
        ingress.apply(&mut rt, packet).ok()?;
    }
    Some(rt)
}

/// Does a note on an MPE member channel follow per-note bend and pressure?
fn mpe_probe(path: &Path, pick: Pick, ccs: &[(u8, u8)]) -> Value {
    if fresh_runtime(path, pick.key, ccs).is_none() {
        return json!({"error": "no runtime"});
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sampler_midi::mpe_response(
            || fresh_runtime(path, pick.key, ccs).expect("runtime built once already"),
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

fn limits_for(behavior_locals: usize, note_cells: usize) -> Limits {
    Limits {
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
    }
}

fn new_ingress() -> Ingress {
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    Ingress::new(0, groups)
}


/// Controllers a player would have up: mod wheel, expression and CC2 high,
/// plus every plain controller the instrument's modulators read. (Controllers
/// scripts read directly are not listed in the IR, so they are not covered.)
fn musical_ccs(ir: &sampler_ir::Instrument, dynamics: &[(u8, f64)]) -> Vec<(u8, u8)> {
    let mut ccs = vec![(1, 100), (2, 100), (11, 127)];
    for (n, _) in dynamics {
        if !ccs.iter().any(|c| c.0 == *n) {
            ccs.push((*n, 100));
        }
    }
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

/// Wake the scripts exactly when they asked to run, serving streamed pages.
fn scripted_render(
    rt: &mut Runtime,
    driver: &mut sampler_uvi::scripted::Driver<sampler_uvi::script::ScriptHost>,
    horizon: Option<u32>,
    feed: &mut sampler_uvi::scripted::MidiFeed,
    out: &mut [[f32; 2]],
) -> Result<(), sampler_core::Error> {
    let mut done = 0;
    while done < out.len() {
        let left = out.len() - done;
        let due = driver.wake(rt)?;
        feed.pump(driver, rt);
        let step = due.map_or(left, |d| d.max(1).min(left));
        if let Some(h) = horizon {
            let _ = rt.service_streaming(h);
        }
        rt.render(&mut out[done..done + step])?;
        done += step;
    }
    Ok(())
}

fn play(subject: Subject, pick: Pick, diagnose: bool, ccs: &[(u8, u8)]) -> Result<Sound, String> {
    stage("prepare");
    let (rate, locals, cells) = match &subject {
        Subject::Plan(l) => (
            l.plan.sample_rate(),
            l.plan.behavior_local_count(),
            l.plan.note_cell_count(),
        ),
        Subject::Streamed(s) => (
            s.loaded.plan.sample_rate(),
            s.loaded.plan.behavior_local_count(),
            s.loaded.plan.note_cell_count(),
        ),
        Subject::Scripted(p) => (p.plan.sample_rate(), 0, 0),
        Subject::Ir(_) => return Err("prepare: no plan".into()),
    };
    let stream_info = match &subject {
        Subject::Streamed(s) => json!({
            "full_bytes": s.report.full_bytes,
            "head_bytes": s.report.head_bytes,
            "pool_bytes": s.report.pool_bytes,
            "latency_p50_ms": s.report.latency_p50.as_secs_f64() * 1e3,
            "head_frames": s.report.head_frames,
        }),
        _ => Value::Null,
    };
    let limits = limits_for(locals, cells);
    let build = |plan, cache: Option<sampler_core::StreamCache>| -> Result<Box<Runtime>, String> {
        let mut rt = Runtime::new(plan, limits).map_err(|e| format!("prepare: runtime: {e}"))?;
        if let Some(cache) = cache {
            rt = rt.with_stream_cache(cache);
            // As the host does: a note whose pages are not resident yet starts
            // silent and fades in, instead of being refused.
            rt.set_cold_starts(true);
        }
        if std::env::var_os("CH_STEAL").is_some() {
        rt.set_voice_stealing(Some(sampler_core::Stealing::for_limits(
            rt.sample_rate(),
            limits.voices,
        )))
        .map_err(|e| format!("prepare: runtime: {e}"))?;
        }
        Ok(Box::new(rt))
    };
    let horizon_of = |head: usize| (head.max(sampler_core::PAGE_FRAMES) + 4096) as u32;
    let mut rig = match subject {
        Subject::Plan(l) => {
            let mut rt = build(l.plan, None)?;
            rt.record_selections(diagnose);
            Rig::Midi {
                rt,
                ingress: new_ingress(),
                horizon: None,
                _keep: None,
            }
        }
        Subject::Streamed(s) => {
            let sampler_kontakt::Streamed {
                loaded,
                assets,
                cache,
                streamer,
                report,
            } = *s;
            let mut rt = build(loaded.plan, Some(cache))?;
            rt.record_selections(diagnose);
            Rig::Midi {
                rt,
                ingress: new_ingress(),
                horizon: Some(horizon_of(report.head_frames)),
                _keep: Some((streamer, assets)),
            }
        }
        Subject::Scripted(p) => {
            let sampler_uvi::scripted::Program {
                plan,
                host,
                groups,
                stream,
                ..
            } = *p;
            let mut stream = stream;
            let horizon = stream.as_ref().map(|s| horizon_of(s.horizon));
            let cache = stream.as_mut().and_then(|s| s.cache.take());
            Rig::Scripted {
                rt: build(plan, cache)?,
                driver: Box::new(sampler_uvi::scripted::Driver::new(host, groups, rate)),
                horizon,
                _keep: stream,
            }
        }
        Subject::Ir(_) => unreachable!("checked above"),
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
    let mut feed = sampler_uvi::scripted::MidiFeed::default();
    let mut block_times: Vec<f64> = Vec::with_capacity(total / buffer.len() + 1);
    // Allocation calls on this (the render) thread: the note-on block, the
    // release block, and every other block.
    let (mut allocs_on, mut allocs_off, mut allocs_steady) = (0usize, 0usize, 0usize);
    let mut peak_voices = 0usize;
    let (mut peak, mut tail_peak, mut finite) = (0.0f32, 0.0f32, true);
    let sw = u32::from(pick.switch.unwrap_or(0)) << 8;
    // Controllers first, then the switch key taps, then the note.
    let mut pre_words: Vec<[u32; 1]> = ccs
        .iter()
        .map(|&(cc, v)| [0x20B0_0000 | u32::from(cc) << 8 | u32::from(v)])
        .collect();
    if pick.switch.is_some() {
        pre_words.push([0x2090_0000 | sw | 64]);
        pre_words.push([0x2080_0000 | sw]);
    }
    let note_index = pre_words.len();
    let mut note = String::from("not sent");
    let mut note_started = false;
    let mut note_other: Option<String> = None;
    let mut faults = Vec::new();
    stage("render");
    for begin in (0..total).step_by(buffer.len()) {
        let len = buffer.len().min(total - begin);
        let has_release = (begin..begin + len).contains(&release_at);
        let (t0, a0);
        match &mut rig {
            Rig::Midi {
                rt,
                ingress,
                horizon,
                ..
            } => {
                let mut batch = Vec::new();
                if begin == 0 {
                    for word in &pre_words {
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
                if has_release {
                    batch.push(TimedPacket {
                        offset: release_at - begin,
                        packet: off,
                    });
                }
                (t0, a0) = (Instant::now(), heap::calls());
                if let Some(h) = horizon {
                    let _ = rt.service_streaming(*h);
                }
                ingress
                    .render(
                        rt,
                        &mut buffer[..len],
                        &batch,
                        batch.len(),
                        |i, result| {
                            if begin == 0 && i == note_index {
                                // No allocation on the started path: the
                                // allocation counter covers this closure.
                                note_started = matches!(result, Ok(Applied::Started(_)));
                                if !note_started {
                                    note_other = Some(format!("{result:?}"));
                                }
                            }
                        },
                    )
                    .map_err(|e| format!("render: {e:?}"))?;
            }
            Rig::Scripted {
                rt,
                driver,
                horizon,
                ..
            } => {
                if begin == 0 {
                    let mut cc_ingress = new_ingress();
                    for word in &pre_words {
                        if let Some(Ok(packet)) = Packets::new(word).next() {
                            let _ = cc_ingress.apply(rt, packet);
                        }
                    }
                    for &(cc, value) in ccs {
                        driver.input(
                            rt,
                            sampler_uvi::scripted::HostInput::Controller {
                                cc,
                                value,
                                channel: 0,
                            },
                        );
                    }
                    let input = sampler_core::Input {
                        protocol: sampler_core::Protocol::Native,
                        port: 0,
                        group: 0,
                        channel: 0,
                        key,
                        external_id: None,
                    };
                    let velocity = f64::from(pick.velocity) / 127.0;
                    note = match rt
                        .note_on(input, key, velocity)
                        .and_then(|n| driver.note_on(rt, n, key, velocity))
                    {
                        Ok(()) => "started".into(),
                        Err(e) => format!("{e:?}"),
                    };
                }
                (t0, a0) = (Instant::now(), heap::calls());
                let cut = if has_release { release_at - begin } else { len };
                scripted_render(rt, driver, *horizon, &mut feed, &mut buffer[..cut])
                    .map_err(|e| format!("render: {e:?}"))?;
                if cut < len {
                    driver
                        .note_off(rt, key)
                        .map_err(|e| format!("release: {e:?}"))?;
                    scripted_render(rt, driver, *horizon, &mut feed, &mut buffer[cut..len])
                        .map_err(|e| format!("render: {e:?}"))?;
                }
            }
        }
        block_times.push(t0.elapsed().as_secs_f64());
        let used = heap::calls() - a0;
        if begin == 0 && matches!(rig, Rig::Midi { .. }) {
            note = if note_started {
                "started".into()
            } else {
                note_other.take().unwrap_or(note)
            };
        }
        if begin == 0 {
            allocs_on += used;
        } else if has_release {
            allocs_off += used;
        } else {
            allocs_steady += used;
        }
        let rt: &mut Runtime = match &mut rig {
            Rig::Midi { rt, .. } | Rig::Scripted { rt, .. } => rt,
        };
        peak_voices = peak_voices.max(rt.voice_count());
        if matches!(rig, Rig::Midi { .. }) {
            let Rig::Midi { rt, .. } = &mut rig else {
                unreachable!()
            };
            rt.flush_behaviors(|_, _, outcome| {
                if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) && faults.len() < 8 {
                    faults.push(normalize(&format!("{outcome:?}")));
                }
                true
            });
            rt.flush_ended(|_| true);
        }
        for x in buffer[..len].iter().flatten() {
            finite &= x.is_finite();
            peak = peak.max(x.abs());
            // The last quarter second of the tail.
            if begin + len > total - frame(0.25) {
                tail_peak = tail_peak.max(x.abs());
            }
        }
    }
    block_times.sort_by(f64::total_cmp);
    let q = |f: f64| block_times[((block_times.len() - 1) as f64 * f) as usize];
    let selection = match &mut rig {
        Rig::Midi { rt, .. } if diagnose => selection_summary(rt.take_selection_records()),
        _ => Value::Null,
    };
    let rt: &Runtime = match &rig {
        Rig::Midi { rt, .. } | Rig::Scripted { rt, .. } => rt,
    };
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
        "audio_allocs_note_on": allocs_on,
        "audio_allocs_release": allocs_off,
        "audio_allocs_steady": allocs_steady,
        "stream_underruns": st.stream_underruns,
        "cold_starts": st.cold_starts,
        "voice_drops": st.voice_drops,
        "resident_bytes": rt.resident_bytes(),
        "stream_cache_bytes": st.stream_cache_bytes,
        "stream": stream_info,
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
        Subject::Streamed(s) => s.loaded.scripts.len(),
        Subject::Scripted(p) => usize::from(p.host.handles_notes()),
        Subject::Ir(_) => 0,
    };
    json!({
        "declared": ir.behaviors.len(),
        "bound": bound,
        "compile_failed": failed.len(),
        "compile_errors": failed.into_iter().take(3).collect::<Vec<_>>(),
        "warnings": warnings,
    })
}

fn load_item(item: &Item, ctx: &Ctx) -> Loading {
    let policy = sampler_kontakt::StreamPolicy {
        decoders: 2,
        ..Default::default()
    };
    let ok = |pick: Pick, ir: &sampler_ir::Instrument| {
        json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": ir.zones.len()})
    };
    let no_zone = || {
        failed(
            "note-on/selection",
            "NoZone",
            "no zone covers any key at any velocity".into(),
        )
    };
    match item {
        Item::Kontakt(path) => {
            stage("parse");
            let kontakt = match sampler_kontakt::read(path) {
                Ok(k) => k,
                Err(e) => return load_failure(&e, "parse"),
            };
            let Some(pick) = pick_key(&kontakt.instrument) else {
                return no_zone();
            };
            let parse_only = ctx.tier == Tier::Parse;
            let options = sampler_kontakt::Options {
                keys: if parse_only { 0..=127 } else { pick.key..=pick.key },
                scripts: true,
                library: Some(path.clone()),
                ..Default::default()
            };
            if parse_only {
                // Lower and bind against placeholder audio: no decoding.
                stage("lower");
                let sampler_kontakt::Kontakt {
                    mut instrument,
                    locations,
                    ..
                } = kontakt;
                let kept = instrument.retain_zones(|_| true);
                let pcm = kept
                    .iter()
                    .map(|_| {
                        sampler_core::Pcm::new(48000, vec![[0.0f32; 2]; 8192].into_boxed_slice())
                            .expect("placeholder audio")
                    })
                    .collect();
                let labels = kept
                    .iter()
                    .map(|&a| locations[a].display().to_string())
                    .collect();
                return match sampler_kontakt::finish(instrument, pcm, labels, &options) {
                    Ok(l) => (
                        ok(pick, &l.instrument),
                        Some((Subject::Plan(Box::new(l)), pick)),
                    ),
                    Err(e) => load_failure(&e, "prepare"),
                };
            }
            stage("load");
            if std::env::var_os("CH_NOSTREAM").is_some() {
                return match sampler_kontakt::load(path, &options, |_| {}) {
                    Ok(l) => (ok(pick, &l.instrument), Some((Subject::Plan(Box::new(l)), pick))),
                    Err(e) => load_failure(&e, "load"),
                };
            }
            match sampler_kontakt::load_read_streamed(kontakt, &options, &policy, |_| {}) {
                Ok(s) => (
                    ok(pick, &s.loaded.instrument),
                    Some((Subject::Streamed(Box::new(s)), pick)),
                ),
                Err(e) => load_failure(&e, "load"),
            }
        }
        Item::UviProgram { bank, program } => {
            stage("parse");
            let ir = match sampler_uvi::translate_path(&bank.join(program)) {
                Ok(t) => t.instrument,
                Err(e) => return load_failure(&*e, "translate-to-IR"),
            };
            let Some(pick) = pick_key(&ir) else {
                return no_zone();
            };
            if ctx.tier == Tier::Parse {
                return (ok(pick, &ir), Some((Subject::Ir(Box::new(ir)), pick)));
            }
            stage("container/decrypt");
            let bank = match sampler_uvi::Bank::open(bank) {
                Ok(b) => b,
                Err(e) => return load_failure(&e, "container/decrypt"),
            };
            stage("load");
            match sampler_uvi::load_program_scripted_streamed(&bank, program, 48000, &policy) {
                Ok(p) => (
                    ok(pick, &p.instrument),
                    Some((Subject::Scripted(Box::new(p)), pick)),
                ),
                Err(e) => load_failure(&*e, "load"),
            }
        }
        Item::UviLoose(path) => {
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
            {
                return failed("container/decrypt", "BankOpen", "bank does not open".into());
            }
            stage("load");
            if ctx.tier == Tier::Parse {
                return match sampler_uvi::translate_path(path) {
                    Ok(t) => match pick_key(&t.instrument) {
                        Some(pick) => (
                            ok(pick, &t.instrument),
                            Some((Subject::Ir(Box::new(t.instrument)), pick)),
                        ),
                        None => no_zone(),
                    },
                    Err(e) => load_failure(&*e, "translate-to-IR"),
                };
            }
            match sampler_uvi::load_streamed(path, 48000, &policy) {
                Ok(s) => match pick_key(&s.loaded.instrument) {
                    Some(pick) => (
                        ok(pick, &s.loaded.instrument),
                        Some((Subject::Streamed(Box::new(s)), pick)),
                    ),
                    None => no_zone(),
                },
                Err(e) => load_failure(&*e, "load"),
            }
        }
        Item::Multi(path) => {
            stage("parse");
            match sampler_kontakt::read_multi(path) {
                Ok(multi) if multi.sample_names.is_empty() => failed(
                    "parse",
                    "EmptyMulti",
                    "no sample references in multi".into(),
                ),
                // A multi is a rack of programs, not one instrument: it parses,
                // but there is no loader that plays it.
                Ok(multi) => (
                    json!({"ok": true, "playable": false, "programs": multi.programs.len(), "samples": multi.sample_names.len()}),
                    None,
                ),
                Err(e) => load_failure(&e, "parse"),
            }
        }
    }
}


/// reached. Stages inside `sampler_kontakt::load` are not separable until its
/// errors carry them, so those report as `load`.
/// The tail is measured 4 to 5 s after release: a stored release that long
/// explains it.
fn long_release(r: &Value) -> bool {
    r["stored_release_s"].as_f64().is_some_and(|s| s >= 0.4)
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

fn check(item: &Item, ctx: &Ctx) -> Value {
    heap::job_start();
    // Process-wide counters only mean something for one job at a time.
    let serial = ctx.workers == 1;
    let (io0, (minflt0, majflt0)) = if serial {
        let _ = std::fs::write("/proc/self/clear_refs", "5");
        (proc_field("/proc/self/io", "read_bytes:"), faults())
    } else {
        (0, (0, 0))
    };
    let start = Instant::now();
    let (load, loaded) = load_item(item, ctx);
    let load_ms = start.elapsed().as_millis() as u64;
    let heap_after_load = heap::live();
    let mut record = json!({
        "id": item.id(),
        "kind": item.kind(),
        "status": "done",
        "tier": if ctx.tier == Tier::Parse { "parse" } else { "full" },
        "load": load,
    });
    let mut play_ms = 0u64;
    if let Some((subject, pick)) = loaded {
        record["scripts"] = scripts(&subject);
        record["unsupported"] = json!(categories(&subject.instrument().unsupported));
        record["unsupported_total"] = json!(subject.instrument().unsupported.len());
        let dynamics: Vec<(u8, f64)> = subject.loaded().map(|l| l.dynamics()).unwrap_or_default();
        if let Some(l) = subject.loaded() {
            record["needs_controller"] = json!(l.needs_controller());
            record["dynamics"] = json!(dynamics.iter().map(|d| json!([d.0, d.1])).collect::<Vec<_>>());
        }
        // Longest release any envelope stores: a tail up to that long is data.
        record["stored_release_s"] = json!(
            subject
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
        {
            let m = sampler_kontakt::articulation_migration(subject.instrument());
            record["articulation"] = json!({
                "switches_found": m.switches_found,
                "migrated": m.migrated,
                "unrecognised": m.unrecognised.iter().take(5).map(|u| normalize(u)).collect::<Vec<_>>(),
            });
        }
        if ctx.tier == Tier::Full && !matches!(subject, Subject::Ir(_)) {
            let played = Instant::now();
            let ccs = musical_ccs(subject.instrument(), &dynamics);
            let silent = |s: &Sound| s.peak <= 1e-4 || s.note != "started";
            let reload = |ccs: &[(u8, u8)], diagnose: bool| match load_item(item, ctx) {
                (_, Some((s, p))) => play(s, p, diagnose, ccs).ok(),
                _ => None,
            };
            let first = play(subject, pick, false, &[]);
            let first = match first {
                Ok(mut s) if silent(&s) && matches!(item, Item::Kontakt(_)) => {
                    // Selection records allocate, so they only run on a second
                    // pass over an item that was silent.
                    if let Some(d) = reload(&[], true) {
                        s.selection = d.selection;
                    }
                    Ok(s)
                }
                other => other,
            };
            // The same note again with the controllers up; if only that
            // sounds, find the controller it needs.
            if let Ok(d) = &first {
                if let Some(m) = reload(&ccs, false) {
                    let mut musical = json!({
                        "ccs": ccs.iter().map(|c| json!([c.0, c.1])).collect::<Vec<_>>(),
                        "peak_db": if m.peak > 0.0 { json!(20.0 * f64::from(m.peak).log10()) } else { Value::Null },
                        "sounds": m.peak > 1e-4,
                        "note": m.note,
                    });
                    if silent(d) && m.peak > 1e-4 {
                        let alone = ccs
                            .iter()
                            .find(|c| reload(&[**c], false).is_some_and(|x| x.peak > 1e-4));
                        musical["needs_cc"] = match alone {
                            Some(c) => json!([c.0]),
                            None => json!(ccs.iter().map(|c| c.0).collect::<Vec<_>>()),
                        };
                    }
                    record["musical"] = musical;
                }
                if let Item::Kontakt(path) = item {
                    record["mpe"] = mpe_probe(path, pick, &ccs);
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
            play_ms = played.elapsed().as_millis() as u64;
        }
    }
    record["stage"] = json!(stage_of(&record));
    record["perf"] = json!({
        "load_ms": load_ms,
        "play_ms": play_ms,
        "peak_heap_bytes": heap::peak(),
        "heap_bytes_after_load": heap_after_load,
        "workers": ctx.workers,
        // Timing is evidence only when the machine was otherwise idle: judge it
        // by the 1-minute load average at the end of the item.
        "loadavg1": std::fs::read_to_string("/proc/loadavg").ok().and_then(|l| l.split_whitespace().next()?.parse::<f64>().ok()),
        "peak_rss_kib": serial.then(|| proc_field("/proc/self/status", "VmHWM:")),
        "disk_read_bytes": serial.then(|| proc_field("/proc/self/io", "read_bytes:") - io0),
        "minor_faults": serial.then(|| faults().0 - minflt0),
        "major_faults": serial.then(|| faults().1 - majflt0),
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

/// The fixed quick-tier sample (ids, one per line), built by `quick-list`.
const QUICK: &str = include_str!("../quick.txt");

#[derive(Default)]
struct Opts {
    tier: Option<String>,
    shard: Option<(usize, usize)>,
    from: Option<PathBuf>,
    reuse: bool,
    failed_only: bool,
    only: Vec<String>,
    workers: Option<usize>,
    timeout: Option<u64>,
    roots: Vec<String>,
}

/// `*` and `?` wildcards.
fn glob(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some((b'?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

/// What a result depends on: the item file, the tier and the crates' git tree.
fn tree_hash() -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let tree = format!(
        "{}{}",
        git(&["rev-parse", "HEAD:crates"]),
        git(&["rev-parse", "HEAD:tools/corpus-health"])
    );
    if git(&["status", "--porcelain", "crates", "tools/corpus-health", "vendor"]).is_empty() {
        tree
    } else {
        // Uncommitted edits: never reuse results across them.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{tree}+dirty{now}")
    }
}

fn cache_key(item: &Item, tier: &str, tree: &str) -> String {
    let path = match item {
        Item::Kontakt(p) | Item::Multi(p) | Item::UviLoose(p) => p,
        Item::UviProgram { bank, .. } => bank,
    };
    let (mtime, size) = std::fs::metadata(path).map_or((0, 0), |m| {
        (
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
            m.len(),
        )
    });
    format!("{mtime}:{size}:{tier}:{tree}")
}

fn is_failure(r: &Value) -> bool {
    !matches!(r["stage"].as_str(), Some("ok" | "needs-controller"))
}

/// Memory the jobs in flight may hold, by their expected peak heap.
struct Budget {
    total: u64,
    used: Mutex<u64>,
    wake: Condvar,
}

impl Budget {
    fn acquire(&self, need: u64) -> u64 {
        let need = need.min(self.total);
        let mut used = self.used.lock().unwrap();
        while *used > 0 && *used + need > self.total {
            used = self.wake.wait(used).unwrap();
        }
        *used += need;
        need
    }
    fn release(&self, need: u64) {
        *self.used.lock().unwrap() -= need;
        self.wake.notify_all();
    }
}

fn mem_available() -> u64 {
    proc_field("/proc/meminfo", "MemAvailable:") * 1024
}

fn run(out: &Path, opts: &Opts) -> i32 {
    let tier = match opts.tier.as_deref() {
        Some("parse") => Tier::Parse,
        _ => Tier::Full,
    };
    let tier_name = opts.tier.clone().unwrap_or_else(|| "full".into());
    let mut all = items(&roots(&opts.roots));
    if let Some((i, n)) = opts.shard {
        all = all.into_iter().skip(i).step_by(n).collect();
    }
    if tier_name == "quick" {
        let wanted: HashSet<&str> = QUICK.lines().filter(|l| !l.is_empty()).collect();
        all.retain(|i| wanted.contains(i.id().as_str()));
    }
    if !opts.only.is_empty() {
        all.retain(|i| opts.only.iter().any(|g| glob(g, &i.id())));
    }
    let prior: BTreeMap<String, Value> = opts
        .from
        .as_deref()
        .map(|p| {
            all_records(p)
                .into_iter()
                .map(|r| (r["id"].as_str().unwrap_or_default().to_string(), r))
                .collect()
        })
        .unwrap_or_default();
    if opts.failed_only {
        all.retain(|i| prior.get(&i.id()).is_none_or(is_failure));
    }
    // Finished and started records of this output, from before a restart.
    let mut finished = HashSet::new();
    let mut starts = BTreeMap::<String, usize>::new();
    for r in read_records(out) {
        let id = r["id"].as_str().unwrap_or_default().to_string();
        if r["status"] == "started" {
            *starts.entry(id).or_default() += 1;
        } else {
            finished.insert(id);
        }
    }
    let file = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out)
            .expect("open output"),
    );
    let write = |v: &Value| {
        let mut f = file.lock().unwrap();
        writeln!(f, "{v}").unwrap();
        f.flush().unwrap();
    };
    // An item that was started twice and never finished took the process down.
    for (id, n) in &starts {
        if !finished.contains(id) && *n >= 2 {
            write(&json!({"id": id, "kind": "unknown", "status": "crash", "stage": "crash",
                "load": {"ok": false, "error": "process died twice on this item (abort, OOM or hang)"}}));
            finished.insert(id.clone());
        }
    }
    let tree = tree_hash();
    let key_tier = if tier == Tier::Parse { "parse" } else { "full" };
    let mut queue = Vec::new();
    let mut reused = 0;
    for item in all {
        let id = item.id();
        if finished.contains(&id) {
            continue;
        }
        let key = cache_key(&item, key_tier, &tree);
        if opts.reuse {
            if let Some(p) = prior.get(&id) {
                if p["cache_key"] == key && !is_failure(p) && p["status"] == "done" {
                    let mut p = p.clone();
                    p["reused"] = json!(true);
                    write(&p);
                    reused += 1;
                    continue;
                }
            }
        }
        let hint = prior
            .get(&id)
            .and_then(|p| p["perf"]["peak_heap_bytes"].as_u64())
            .unwrap_or(1 << 30);
        queue.push((item, key, hint));
    }
    // Biggest first, so the long jobs start early and the pool drains evenly.
    queue.sort_by_key(|q| std::cmp::Reverse(q.2));
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let workers = opts.workers.unwrap_or(cores.min(12)).max(1).min(63);
    let budget = Budget {
        total: mem_available().saturating_sub(12 << 30).max(4 << 30),
        used: Mutex::new(0),
        wake: Condvar::new(),
    };
    let timeout = std::time::Duration::from_secs(opts.timeout.unwrap_or(900));
    std::panic::set_hook(Box::new(|_| {}));
    let total = queue.len();
    eprintln!(
        "{total} to run, {reused} reused, {workers} workers, budget {} MiB, tier {tier_name}",
        budget.total >> 20
    );
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let finished_all = AtomicBool::new(false);
    let running: Mutex<Vec<Option<(String, Instant)>>> = Mutex::new(vec![None; workers]);
    let ctx = Ctx { tier, workers };
    let mut exit = 0;
    std::thread::scope(|s| {
        let dog = s.spawn(|| {
            while !finished_all.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let hung = running
                    .lock()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .find_map(|(w, r)| {
                        r.as_ref()
                            .filter(|(_, t)| t.elapsed() > timeout)
                            .map(|(id, _)| (w, id.clone()))
                    });
                if let Some((w, id)) = hung {
                    // A thread cannot be stopped: record it, then restart the
                    // process (the runner resumes; in-flight items retry once).
                    write(&json!({"id": id, "kind": "unknown", "status": "timeout", "worker": w,
                        "stage": current_stage(w), "load": {"ok": false, "stage": current_stage(w),
                        "kind": "Timeout", "error": format!("no result after {} s", timeout.as_secs())}}));
                    std::process::exit(75);
                }
            }
        });
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                let (queue, next, done, running, write, budget, ctx) =
                    (&queue, &next, &done, &running, &write, &budget, &ctx);
                s.spawn(move || {
                    WORKER.with(|c| c.set(w));
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((item, key, hint)) = queue.get(i) else {
                            break;
                        };
                        let held = budget.acquire(*hint);
                        let id = item.id();
                        write(&json!({"id": id, "status": "started", "worker": w}));
                        running.lock().unwrap()[w] = Some((id.clone(), Instant::now()));
                        stage("start");
                        let mut record = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            check(item, ctx)
                        }))
                        .unwrap_or_else(|payload| {
                            let message = payload
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_default();
                            let at = current_stage(w);
                            json!({"id": id, "kind": item.kind(), "status": "panic", "stage": at,
                                "load": {"ok": false, "stage": at, "kind": "Panic", "error": normalize(&format!("panic: {message}"))}})
                        });
                        record["worker"] = json!(w);
                        record["cache_key"] = json!(key);
                        running.lock().unwrap()[w] = None;
                        budget.release(held);
                        write(&record);
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        eprintln!(
                            "[{n}/{total}] w{w} {} {}ms {id}",
                            record["stage"].as_str().unwrap_or("?"),
                            record["ms"]
                        );
                    }
                })
            })
            .collect();
        for h in handles {
            if h.join().is_err() {
                exit = 1;
            }
        }
        finished_all.store(true, Ordering::Relaxed);
        let _ = dog.join();
    });
    exit
}

/// `quick`: run the fixed sample, keep it as `current.jsonl` (the last run
/// becomes `previous.jsonl`), and print what changed.
fn quick(extra: &[String]) -> i32 {
    let dir = dirs_home().join(".cache/kontakto-corpus/quick");
    std::fs::create_dir_all(&dir).expect("quick dir");
    let (cur, prev) = (dir.join("current.jsonl"), dir.join("previous.jsonl"));
    if cur.exists() {
        let _ = std::fs::rename(&cur, &prev);
    }
    let opts = Opts {
        tier: Some("quick".into()),
        from: prev.exists().then(|| prev.clone()),
        workers: extra.iter().position(|a| a == "--workers").and_then(|i| extra.get(i + 1)?.parse().ok()),
        ..Default::default()
    };
    let started = Instant::now();
    let code = run(&cur, &opts);
    println!("quick tier: {} s wall", started.elapsed().as_secs());
    if prev.exists() {
        diff(&prev, &cur);
    } else {
        println!("(no previous quick run to compare with)");
    }
    code
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// `quick-list RUN`: a fixed sample from a finished run, three per library
/// plus every instrument that failed, plus two multis.
fn quick_list(run_dir: &Path) {
    let records = all_records(run_dir);
    let library = |id: &str| {
        let rest = id.split("/Libraries/").nth(1).unwrap_or(id);
        rest.split('/').take(2).collect::<Vec<_>>().join("/")
    };
    let mut by_lib = BTreeMap::<String, Vec<&Value>>::new();
    for r in &records {
        by_lib.entry(library(r["id"].as_str().unwrap_or_default())).or_default().push(r);
    }
    let mut ids = std::collections::BTreeSet::new();
    for (_, rs) in by_lib {
        let mut rs = rs;
        rs.sort_by_key(|r| r["id"].as_str().unwrap_or_default().to_string());
        let kontakt_multi = rs.iter().filter(|r| r["kind"] == "kontakt-multi").count();
        let take = if kontakt_multi == rs.len() { 2 } else { 3 };
        let step = (rs.len() / take).max(1);
        for r in rs.iter().step_by(step).take(take) {
            ids.insert(r["id"].as_str().unwrap_or_default().to_string());
        }
        for r in &rs {
            if is_failure(r) && r["kind"] != "kontakt-multi" && ids.len() < 400 {
                ids.insert(r["id"].as_str().unwrap_or_default().to_string());
            }
        }
    }
    for id in ids {
        println!("{id}");
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
    let code = match args.as_slice() {
        [cmd, out, rest @ ..] if cmd == "run" => {
            let mut opts = Opts::default();
            let mut it = rest.iter();
            while let Some(arg) = it.next() {
                let mut value = || it.next().expect("flag needs a value").clone();
                match arg.as_str() {
                    "--shard" => {
                        let spec = value();
                        let (i, n) = spec.split_once('/').expect("--shard I/N");
                        opts.shard = Some((i.parse().unwrap(), n.parse().unwrap()));
                    }
                    "--tier" => opts.tier = Some(value()),
                    "--from" | "--failed-from" => {
                        opts.failed_only |= arg == "--failed-from";
                        opts.from = Some(PathBuf::from(value()));
                    }
                    "--reuse" => opts.reuse = true,
                    "--only" => opts.only.push(value()),
                    "--workers" => opts.workers = value().parse().ok(),
                    "--timeout" => opts.timeout = value().parse().ok(),
                    _ => opts.roots.push(arg.clone()),
                }
            }
            run(Path::new(out), &opts)
        }
        [cmd, rest @ ..] if cmd == "quick" => quick(rest),
        [cmd, rest @ ..] if cmd == "list" => {
            for item in items(&roots(rest)) {
                println!("{}\t{}", item.kind(), item.id());
            }
            0
        }
        [cmd, dir] if cmd == "quick-list" => {
            quick_list(Path::new(dir));
            0
        }
        [cmd, out, md] if cmd == "summary" => {
            summary(Path::new(out), Path::new(md));
            0
        }
        [cmd, old, new] if cmd == "diff" => {
            diff(Path::new(old), Path::new(new));
            0
        }
        _ => {
            eprintln!(
                "usage: corpus-health run OUT.jsonl [--tier parse|full|quick] [--shard I/N] [--only GLOB]\n\
                 \x20      [--from RUN [--reuse]] [--failed-from RUN] [--workers N] [--timeout S] [ROOT ...]\n\
                 \x20      | quick [--workers N] | quick-list RUN | summary OUT.jsonl SUMMARY.md | diff OLD NEW"
            );
            2
        }
    };
    std::process::exit(code);
}
