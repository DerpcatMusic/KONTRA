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
        Arc, Condvar, Mutex, OnceLock,
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
    /// One program of a multi (internal: a multi is checked program by program).
    MultiProgram { path: PathBuf, index: usize },
    UviProgram { bank: PathBuf, program: String },
    UviLoose(PathBuf),
}

impl Item {
    fn id(&self) -> String {
        match self {
            Item::Kontakt(p) | Item::Multi(p) | Item::UviLoose(p) => p.to_string_lossy().into(),
            Item::UviProgram { bank, program } => format!("{}::{program}", bank.display()),
            Item::MultiProgram { path, index } => format!("{}#{index}", path.display()),
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Item::Kontakt(_) => "kontakt",
            Item::Multi(_) | Item::MultiProgram { .. } => "kontakt-multi",
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

type BankSlot = Arc<OnceLock<Result<Arc<sampler_uvi::Bank>, Arc<sampler_uvi::AccessError>>>>;
static BANKS: Mutex<BTreeMap<PathBuf, BankSlot>> = Mutex::new(BTreeMap::new());

/// A bank opened once per process and shared by every program of it: opening
/// decodes the directory (and a content key), far dearer than one program.
fn bank(path: &Path) -> Result<Arc<sampler_uvi::Bank>, Arc<sampler_uvi::AccessError>> {
    let slot = BANKS.lock().unwrap().entry(path.to_owned()).or_default().clone();
    slot.get_or_init(|| sampler_uvi::Bank::open(path).map(Arc::new).map_err(Arc::new))
        .clone()
}

/// `items`, remembered for a day in `~/.cache/kontakto-corpus/items.tsv`
/// (`kind<TAB>id` per line, the `list` format): opening every bank to list its
/// programs costs a minute of CPU. Explicit roots and `CH_RESCAN=1` skip it.
fn cached_items(explicit: &[String]) -> Vec<Item> {
    let cache = dirs_home().join(".cache/kontakto-corpus/items.tsv");
    let fresh = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 86400);
    if explicit.is_empty() && fresh && std::env::var_os("CH_RESCAN").is_none() {
        let text = std::fs::read_to_string(&cache).unwrap_or_default();
        let items: Vec<Item> = text
            .lines()
            .filter_map(|l| {
                let (kind, id) = l.split_once('\t')?;
                Some(match kind {
                    "kontakt" => Item::Kontakt(id.into()),
                    "kontakt-multi" => Item::Multi(id.into()),
                    "uvi-loose" => Item::UviLoose(id.into()),
                    "uvi-program" => {
                        let (bank, program) = id.split_once("::")?;
                        Item::UviProgram { bank: bank.into(), program: program.into() }
                    }
                    _ => return None,
                })
            })
            .collect();
        if !items.is_empty() {
            return items;
        }
    }
    let items = items(&roots(explicit));
    if explicit.is_empty() {
        let text: String = items.iter().map(|i| format!("{}\t{}\n", i.kind(), i.id())).collect();
        let _ = std::fs::write(&cache, text);
    }
    items
}

/// Every item under `roots`; banks open in parallel (and stay open for the run).
fn items(roots: &[PathBuf]) -> Vec<Item> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        walk(root, &mut files, &mut seen);
    }
    files.sort();
    files.dedup();
    let ext = |p: &Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let banks: Vec<&PathBuf> = files.iter().filter(|p| ext(p) == "ufs").collect();
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..8.min(banks.len()) {
            s.spawn(|| {
                while let Some(p) = banks.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let _ = bank(p);
                }
            });
        }
    });
    let mut items = Vec::new();
    for path in &files {
        match ext(path).as_str() {
            "nki" => items.push(Item::Kontakt(path.clone())),
            "nkm" => items.push(Item::Multi(path.clone())),
            "uvip" => items.push(Item::UviLoose(path.clone())),
            "ufs" => match bank(path) {
                Ok(bank) => items.extend(bank.programs().into_iter().map(|program| Item::UviProgram {
                    bank: path.clone(),
                    program,
                })),
                // An unopenable bank is one failed item.
                Err(_) => items.push(Item::UviLoose(path.clone())),
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
    sampler_ir::rank_features(unsupported.iter().map(|u| u.feature.as_str())).into_iter().collect()
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
    /// One sentence on a silent note ([`sampler_core::why_silent`]); set when
    /// selections were recorded.
    why_silent: Option<String>,
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
    /// Stream, play one short note; the second (controller) pass only for a
    /// silent item. No MPE probe, no selection diagnosis.
    Quick,
    /// Stream, play, probe.
    Full,
}

impl Tier {
    fn name(self) -> &'static str {
        match self {
            Tier::Parse => "parse",
            Tier::Quick => "quick",
            Tier::Full => "full",
        }
    }
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
/// `crates/sampler-kontakt/src/load.rs` as `sampler_kontakt::load`.
fn module_of(file: &str) -> String {
    let (krate, rest) = file
        .split_once("crates/")
        .map_or(("", file), |(_, r)| r.split_once("/src/").unwrap_or(("", r)));
    let module = rest.trim_end_matches(".rs").trim_end_matches("/mod").replace('/', "::");
    format!("{}::{module}", krate.replace('-', "_"))
}

/// Where a failure with no source location surfaced: the entry point of its stage.
fn stage_entry(stage: &str, kind: &str) -> &'static str {
    match (stage, kind) {
        ("container/decrypt", _) => "sampler_uvi::Bank::open",
        ("translate-to-IR", _) => "sampler_uvi::translate_program",
        ("parse", _) => "sampler_kontakt::read",
        ("note-on/selection", _) => "corpus_health::pick_key",
        ("prepare" | "bind" | "lower", _) => "sampler_kontakt::finish",
        ("script-compile", _) => "sampler_kontakt::load",
        _ => "sampler_kontakt::load_read_streamed",
    }
}

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
    let location = typed
        .and_then(|t| t.location())
        .map(|l| format!("{}:{}", module_of(l.file()), l.line()))
        .unwrap_or_else(|| stage_entry(stage, &kind).to_string());
    (
        json!({"ok": false, "stage": stage, "kind": kind, "at": at, "where": location, "error": normalize(&reason), "raw": reason.lines().next().unwrap_or("").chars().take(300).collect::<String>()}),
        None,
    )
}

fn failed(stage: &str, kind: &str, reason: String) -> Loading {
    (
        json!({"ok": false, "stage": stage, "kind": kind, "where": stage_entry(stage, kind), "error": normalize(&reason), "raw": reason.lines().next().unwrap_or("").chars().take(300).collect::<String>()}),
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

/// What the audio thread itself spent while the scripts ran inline.
#[derive(Default)]
struct AudioCost {
    allocs: usize,
    seconds: f64,
}

/// Wake the scripts exactly when they asked to run, serving streamed pages.
/// The host runs Lua on a script thread; here it runs inline, so only the
/// runtime's own calls (`service_streaming`, `render`) count as audio work.
fn scripted_render(
    rt: &mut Runtime,
    driver: &mut sampler_uvi::scripted::Driver<sampler_uvi::script::ScriptHost>,
    horizon: Option<u32>,
    out: &mut [[f32; 2]],
    cost: &mut AudioCost,
) -> Result<(), sampler_core::Error> {
    let mut done = 0;
    while done < out.len() {
        let left = out.len() - done;
        let due = driver.wake(rt)?;
        let step = due.map_or(left, |d| d.max(1).min(left));
        let (t, a) = (Instant::now(), heap::calls());
        if let Some(h) = horizon {
            let _ = rt.service_streaming(h);
        }
        rt.render(&mut out[done..done + step])?;
        cost.allocs += heap::calls() - a;
        cost.seconds += t.elapsed().as_secs_f64();
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
    let views: Vec<sampler_ksp::ScriptView> = match &subject {
        Subject::Plan(l) => l.scripts.clone(),
        Subject::Streamed(s) => s.loaded.scripts.clone(),
        _ => Vec::new(),
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
    let mut block_times: Vec<f64> = Vec::with_capacity(total / buffer.len() + 1);
    // Allocation calls on this (the render) thread: the note-on block, the
    // release block, and every other block.
    let (mut allocs_on, mut allocs_off, mut allocs_steady) = (0usize, 0usize, 0usize);
    let mut peak_voices = 0usize;
    // Lua on the audio thread's clock: the host runs it on a script thread.
    let (mut script_allocs, mut script_seconds) = (0usize, 0.0f64);
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
    let mut script_faults: Vec<sampler_core::ScriptFault> = Vec::new();
    stage("render");
    for begin in (0..total).step_by(buffer.len()) {
        let len = buffer.len().min(total - begin);
        let has_release = (begin..begin + len).contains(&release_at);
        let (t0, a0);
        let mut audio_cost: Option<AudioCost> = None;
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
                let mut cost = AudioCost::default();
                let cut = if has_release { release_at - begin } else { len };
                scripted_render(rt, driver, *horizon, &mut buffer[..cut], &mut cost)
                    .map_err(|e| format!("render: {e:?}"))?;
                if cut < len {
                    driver
                        .note_off(rt, key)
                        .map_err(|e| format!("release: {e:?}"))?;
                    scripted_render(rt, driver, *horizon, &mut buffer[cut..len], &mut cost)
                        .map_err(|e| format!("render: {e:?}"))?;
                }
                script_allocs += (heap::calls() - a0).saturating_sub(cost.allocs);
                script_seconds += t0.elapsed().as_secs_f64() - cost.seconds;
                audio_cost = Some(cost);
            }
        }
        block_times.push(audio_cost.as_ref().map_or_else(|| t0.elapsed().as_secs_f64(), |c| c.seconds));
        let used = audio_cost.as_ref().map_or_else(|| heap::calls() - a0, |c| c.allocs);
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
            rt.flush_behaviors_at(|_, _, outcome, program| {
                if !matches!(outcome, Outcome::Finished | Outcome::Cancelled) && faults.len() < 8 {
                    let callback = sampler_ksp::callback_of(&views, program);
                    let error = format!("{outcome:?}");
                    faults.push(normalize(&format!("{error} in {callback}")));
                    script_faults.push(sampler_core::ScriptFault { callback, error });
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
    let mut why_silent = None;
    let selection = match &mut rig {
        Rig::Midi { rt, .. } if diagnose => {
            let records = rt.take_selection_records();
            why_silent = sampler_core::why_silent(key, &records, &script_faults);
            selection_summary(records)
        }
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
        "audio_thread_allocs": allocs_on + allocs_off + allocs_steady,
        "script_inline_allocs": script_allocs,
        "script_inline_ms": script_seconds * 1e3,
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
        why_silent,
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
        Item::Kontakt(path) | Item::MultiProgram { path, .. } => {
            stage("parse");
            let t = Instant::now();
            let read = match item {
                Item::MultiProgram { index, .. } => sampler_kontakt::read_program(path, *index),
                _ => sampler_kontakt::read(path),
            };
            let kontakt = match read {
                Ok(k) => k,
                Err(e) => return load_failure(&e, "parse"),
            };
            let read_ms = t.elapsed().as_millis() as u64;
            let Some(pick) = pick_key(&kontakt.instrument) else {
                return no_zone();
            };
            let pick_ms = t.elapsed().as_millis() as u64 - read_ms;
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
                // One buffer, shared by every asset (clones share the samples).
                let silence = sampler_core::Pcm::new(48000, vec![[0.0f32; 2]; 8192].into_boxed_slice())
                    .expect("placeholder audio");
                let pcm = kept.iter().map(|_| silence.clone()).collect();
                let labels = kept
                    .iter()
                    .map(|&a| locations[a].display().to_string())
                    .collect();
                return match sampler_kontakt::finish(instrument, pcm, labels, &options) {
                    Ok(l) => {
                        let mut record = ok(pick, &l.instrument);
                        let lower_ms = t.elapsed().as_millis() as u64 - read_ms - pick_ms;
                        record["phases_ms"] = json!({"read": read_ms, "pick": pick_ms, "lower": lower_ms});
                        (record, Some((Subject::Plan(Box::new(l)), pick)))
                    }
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
        Item::UviProgram { bank: bank_path, program } => {
            stage("container/decrypt");
            let bank = match bank(bank_path) {
                Ok(b) => b,
                Err(e) => return load_failure(&*e, "container/decrypt"),
            };
            stage("parse");
            let t = Instant::now();
            let ir = match sampler_uvi::translate_program(&bank, program) {
                Ok(i) => i,
                Err(e) => return load_failure(&*e, "translate-to-IR"),
            };
            let translate_ms = t.elapsed().as_millis() as u64;
            let Some(pick) = pick_key(&ir) else {
                return no_zone();
            };
            if ctx.tier == Tier::Parse {
                let mut record = ok(pick, &ir);
                record["phases_ms"] = json!({"translate": translate_ms, "pick": t.elapsed().as_millis() as u64 - translate_ms});
                return (record, Some((Subject::Ir(Box::new(ir)), pick)));
            }
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
                // `check` plays it program by program.
                Ok(multi) => (
                    json!({"ok": true, "programs": multi.programs.len(), "samples": multi.sample_names.len()}),
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
    if r["tier"] == "parse" {
        return "ok";
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

/// A multi is a rack of programs: each is loaded and played as an instrument
/// of its own (the first sixteen, four in the quick tier). The record is the
/// loudest program's, with a `multi` section for them all; it is ok only if
/// every program is.
fn check_multi(item: &Item, path: &Path, ctx: &Ctx) -> Value {
    let start = Instant::now();
    let (load, _) = load_item(item, ctx);
    let mut record = json!({"id": item.id(), "kind": item.kind(), "status": "done", "tier": ctx.tier.name(), "load": load});
    let programs = record["load"]["programs"].as_u64().unwrap_or(0) as usize;
    if record["load"]["ok"] == true {
        let cap = match ctx.tier {
            Tier::Full => 16,
            Tier::Quick => 4,
            Tier::Parse => 1, // each program re-reads the whole multi
        };
        let subs: Vec<Value> = (0..programs.min(cap))
            .map(|index| check_one(&Item::MultiProgram { path: path.into(), index }, ctx))
            .collect();
        let loudness = |r: &Value| r["sound"]["peak_db"].as_f64().unwrap_or(f64::NEG_INFINITY);
        let best = subs
            .iter()
            .max_by(|a, b| loudness(a).total_cmp(&loudness(b)))
            .cloned();
        // A rack slot may hold no instrument: a program with no zones is empty, not broken.
        let empty = |r: &Value| r["load"]["kind"] == "NoZone";
        let subs: Vec<Value> = subs;
        let ok = |r: &Value| matches!(r["stage"].as_str(), Some("ok" | "needs-controller")) || empty(r);
        let stage = subs.iter().find(|r| !ok(r)).map_or("ok", |r| r["stage"].as_str().unwrap_or("load")).to_string();
        let load_ms: u64 = subs.iter().map(|r| r["load_ms"].as_u64().unwrap_or(0)).sum();
        let summary = json!({
            "programs": programs,
            "checked": subs.len(),
            "ok": subs.iter().filter(|r| ok(r)).count(),
            "empty": subs.iter().filter(|r| empty(r)).count(),
            "sounding": subs.iter().filter(|r| r["sound"]["sounds"] == true || r["musical"]["sounds"] == true).count(),
            "finite": subs.iter().all(|r| r["sound"]["finite"] != false),
            "results": subs.iter().enumerate().map(|(i, r)| json!({
                "index": i, "stage": r["stage"], "peak_db": r["sound"]["peak_db"],
                "finite": r["sound"]["finite"], "error": r["load"]["error"].as_str().or(r["sound"]["error"].as_str()),
                "where": r["load"]["where"],
            })).collect::<Vec<_>>(),
        });
        if let Some(mut best) = best {
            best["id"] = record["id"].clone();
            best["kind"] = record["kind"].clone();
            best["load"]["programs"] = json!(programs);
            best["load_ms"] = json!(load_ms);
            best["perf"]["load_ms"] = json!(load_ms);
            record = best;
        }
        record["multi"] = summary;
        record["stage"] = json!(stage);
    } else {
        record["stage"] = json!(stage_of(&record));
    }
    record["ms"] = json!(start.elapsed().as_millis() as u64);
    record
}

fn check(item: &Item, ctx: &Ctx) -> Value {
    match item {
        Item::Multi(path) => check_multi(item, path, ctx),
        _ => check_one(item, ctx),
    }
}

fn check_one(item: &Item, ctx: &Ctx) -> Value {
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
        "tier": ctx.tier.name(),
        "load": load,
    });
    let mut play_ms = 0u64;
    let mut peak_heap = 0;
    if let Some((subject, pick)) = loaded {
        record["scripts"] = scripts(&subject);
        record["unsupported"] = json!(categories(&subject.instrument().unsupported));
        record["unsupported_total"] = json!(subject.instrument().unsupported.len());
        record["unsupported_ranked"] = json!(
            sampler_ir::rank_features(subject.instrument().unsupported.iter().map(|u| u.feature.as_str()))
                .into_iter()
                .take(10)
                .collect::<Vec<_>>()
        );
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
        if ctx.tier != Tier::Parse && !matches!(subject, Subject::Ir(_)) {
            let played = Instant::now();
            let ccs = musical_ccs(subject.instrument(), &dynamics);
            let silent = |s: &Sound| s.peak <= 1e-4 || s.note != "started";
            let reload = |ccs: &[(u8, u8)], diagnose: bool| match load_item(item, ctx) {
                (_, Some((s, p))) => play(s, p, diagnose, ccs).ok(),
                _ => None,
            };
            let first = play(subject, pick, false, &[]);
            let first = match first {
                Ok(mut s) if silent(&s) && matches!(item, Item::Kontakt(_) | Item::MultiProgram { .. }) => {
                    // Selection records allocate, so they only run on a second
                    // pass over an item that was silent.
                    if let Some(d) = reload(&[], true) {
                        s.selection = d.selection;
                        s.why_silent = d.why_silent;
                    }
                    Ok(s)
                }
                other => other,
            };
            // The same note again with the controllers up; if only that
            // sounds, find the controller it needs.
            if let Ok(d) = &first {
                // The quick tier only asks again of an item that was silent.
                let t = Instant::now();
                if silent(d)
                    && let Some(m) = reload(&ccs, false)
                {
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
                record["perf_musical_ms"] = json!(t.elapsed().as_millis() as u64);
                // Five full loads per probe: one instrument in eight (by id hash),
                // or all with CH_MPE=all.
                let sampled = std::env::var_os("CH_MPE").is_some()
                    || item.id().bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) % 8 == 0;
                if ctx.tier == Tier::Full
                    && sampled
                    && std::env::var_os("CH_NOMPE").is_none()
                    && let Item::Kontakt(path) = item
                {
                    let t = Instant::now();
                    // The probe decodes fully: keep it out of the peak memory.
                    peak_heap = heap::peak();
                    record["mpe"] = mpe_probe(path, pick, &ccs);
                    record["perf_mpe_ms"] = json!(t.elapsed().as_millis() as u64);
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
                        "why_silent": s.why_silent,
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
        "peak_heap_bytes": if peak_heap > 0 { peak_heap } else { heap::peak() },
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
        Item::MultiProgram { path, .. } => path,
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

/// Admission control for the worker pool: a job starts when its expected peak
/// heap fits in MemAvailable minus a 12 GiB reserve (re-read every time, so
/// other tenants of the machine count) and fewer jobs run than the load allows.
struct Budget {
    state: Mutex<(u64, usize)>, // reserved bytes, jobs running
    wake: Condvar,
    workers: usize,
}

const RESERVE: u64 = 12 << 30;

impl Budget {
    /// Jobs allowed at once: all workers on an idle machine, falling toward two
    /// as other tenants' load (the 1-minute average less our own jobs) reaches
    /// one and a half runnable tasks per core.
    fn allowed(&self, running: usize) -> usize {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get()) as f64;
        let load = std::fs::read_to_string("/proc/loadavg")
            .ok()
            .and_then(|l| l.split_whitespace().next()?.parse::<f64>().ok())
            .unwrap_or(0.0);
        let others = (load - running as f64).max(0.0);
        let room = ((cores * 1.5 - others) / (cores * 1.5)).clamp(0.0, 1.0);
        2.max((self.workers as f64 * room).round() as usize).min(self.workers)
    }

    fn acquire(&self, need: u64) -> u64 {
        let mut state = self.state.lock().unwrap();
        loop {
            let (reserved, running) = *state;
            // Running jobs already show in MemAvailable, so their reservation
            // is added back: this is the room for jobs that are yet to ramp up.
            let room = mem_available().saturating_sub(RESERVE) + reserved;
            let fits = reserved + need <= room;
            if running == 0 || (running < self.allowed(running) && fits) {
                state.0 += need;
                state.1 += 1;
                return need;
            }
            state = self.wake.wait_timeout(state, std::time::Duration::from_millis(500)).unwrap().0;
        }
    }
    fn release(&self, need: u64) {
        let mut state = self.state.lock().unwrap();
        state.0 = state.0.saturating_sub(need);
        state.1 = state.1.saturating_sub(1);
        self.wake.notify_all();
    }
}

fn mem_available() -> u64 {
    proc_field("/proc/meminfo", "MemAvailable:") * 1024
}

thread_local! {
    /// Where the last panic on this thread came from: crate::module::fn.
    static PANIC_AT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Panic hook: remember the first frame in our own crates (needs symbols; the
/// `corpus` profile keeps them) and the panic location.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let function = trace
            .lines()
            .filter_map(|l| l.trim().split_once(": ").map(|x| x.1))
            .find(|f| (f.starts_with("sampler_") || f.starts_with("corpus_health")) && !f.contains("panic"))
            .map(|f| f.rsplit_once("::h").map_or(f, |x| x.0).to_string());
        let location = info.location().map(|l| format!("{}:{}", module_of(l.file()), l.line()));
        let at = match (function, location) {
            (Some(f), Some(l)) => format!("{f} ({l})"),
            (Some(f), None) => f,
            (None, Some(l)) => l,
            (None, None) => "unknown".into(),
        };
        PANIC_AT.with(|p| *p.borrow_mut() = at);
    }));
}

struct Job {
    id: String,
    since: Instant,
    abandoned: Arc<AtomicBool>,
    held: u64,
}

/// Everything the workers share. Workers are detached threads: one that hangs
/// is abandoned (its item recorded as a timeout, its memory released, a
/// replacement started) and dies with the process.
struct Pool {
    queue: Vec<(Item, String, u64)>,
    next: AtomicUsize,
    completed: AtomicUsize,
    spawned: AtomicUsize,
    running: Mutex<Vec<Option<Job>>>,
    file: Mutex<std::fs::File>,
    budget: Budget,
    ctx: Ctx,
    timeout: std::time::Duration,
}

impl Pool {
    fn write(&self, v: &Value) {
        let mut f = self.file.lock().unwrap();
        writeln!(f, "{v}").unwrap();
        f.flush().unwrap();
    }
}

fn spawn_worker(pool: &Arc<Pool>) {
    let w = pool.spawned.fetch_add(1, Ordering::Relaxed);
    let pool = pool.clone();
    std::thread::Builder::new()
        .name(format!("worker-{w}"))
        .stack_size(64 << 20)
        .spawn(move || worker(&pool, w))
        .expect("spawn worker");
}

fn worker(pool: &Pool, w: usize) {
    WORKER.with(|c| c.set(w));
    let total = pool.queue.len();
    loop {
        let i = pool.next.fetch_add(1, Ordering::Relaxed);
        let Some((item, key, hint)) = pool.queue.get(i) else {
            return;
        };
        let held = pool.budget.acquire(*hint);
        let id = item.id();
        pool.write(&json!({"id": id, "status": "started", "worker": w}));
        let abandoned = Arc::new(AtomicBool::new(false));
        pool.running.lock().unwrap()[w] = Some(Job {
            id: id.clone(),
            since: Instant::now(),
            abandoned: abandoned.clone(),
            held,
        });
        stage("start");
        PANIC_AT.with(|p| p.borrow_mut().clear());
        let mut record = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(item, &pool.ctx)))
            .unwrap_or_else(|payload| {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                let at = current_stage(w);
                let place = PANIC_AT.with(|p| p.borrow().clone());
                json!({"id": id, "kind": item.kind(), "status": "panic", "stage": at,
                    "load": {"ok": false, "stage": at, "kind": "Panic", "where": place,
                    "error": normalize(&format!("panic: {message}")),
                    "raw": message.lines().next().unwrap_or("").chars().take(300).collect::<String>()}})
            });
        if abandoned.load(Ordering::Relaxed) {
            return; // the watchdog already recorded a timeout and replaced us
        }
        record["worker"] = json!(w);
        record["cache_key"] = json!(key);
        pool.running.lock().unwrap()[w] = None;
        pool.budget.release(held);
        pool.write(&record);
        let n = pool.completed.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!(
            "[{n}/{total}] w{w} {} {}ms {id}",
            record["stage"].as_str().unwrap_or("?"),
            record["ms"]
        );
    }
}

fn watchdog(pool: &Arc<Pool>) {
    let total = pool.queue.len();
    while pool.completed.load(Ordering::Relaxed) < total {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut hung = Vec::new();
        for (w, slot) in pool.running.lock().unwrap().iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|j| j.since.elapsed() > pool.timeout) {
                hung.push((w, slot.take().unwrap()));
            }
        }
        for (w, job) in hung {
            job.abandoned.store(true, Ordering::Relaxed);
            let at = current_stage(w);
            pool.write(&json!({"id": job.id, "kind": "unknown", "status": "timeout", "worker": w,
                "stage": at, "load": {"ok": false, "stage": at, "kind": "Timeout",
                "where": format!("worker {w} stuck in stage {at}"),
                "error": format!("no result after {} s", pool.timeout.as_secs())}}));
            pool.budget.release(job.held);
            let n = pool.completed.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("[{n}/{total}] w{w} TIMEOUT in {at}: {}", job.id);
            spawn_worker(pool);
        }
    }
}

fn run(out: &Path, opts: &Opts) -> i32 {
    let tier = match opts.tier.as_deref() {
        Some("parse") => Tier::Parse,
        Some("quick") => Tier::Quick,
        _ => Tier::Full,
    };
    let tier_name = tier.name();
    let mut all = cached_items(&opts.roots);
    if let Some((i, n)) = opts.shard {
        all = all.into_iter().skip(i).step_by(n).collect();
    }
    if tier == Tier::Quick && opts.only.is_empty() {
        let wanted: HashSet<String> = quick_ids();
        all.retain(|i| wanted.contains(&i.id()));
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
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
        .expect("open output");
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let workers = opts.workers.unwrap_or(cores.min(12)).clamp(1, 12);
    let mut pool = Pool {
        queue: Vec::new(),
        next: AtomicUsize::new(0),
        completed: AtomicUsize::new(0),
        spawned: AtomicUsize::new(0),
        running: Mutex::new((0..64).map(|_| None).collect()),
        file: Mutex::new(file),
        budget: Budget { state: Mutex::new((0, 0)), wake: Condvar::new(), workers },
        ctx: Ctx { tier, workers },
        timeout: std::time::Duration::from_secs(opts.timeout.unwrap_or(if tier == Tier::Parse { 300 } else { 600 })),
    };
    // An item that was started twice and never finished took the process down.
    for (id, n) in &starts {
        if !finished.contains(id) && *n >= 2 {
            pool.write(&json!({"id": id, "kind": "unknown", "status": "crash", "stage": "crash",
                "load": {"ok": false, "stage": "crash", "kind": "Crash", "where": "process",
                "error": "process died twice on this item (abort, OOM or hang)"}}));
            finished.insert(id.clone());
        }
    }
    let tree = tree_hash();
    let mut queue = Vec::new();
    let mut reused = 0;
    for item in all {
        let id = item.id();
        if finished.contains(&id) {
            continue;
        }
        let key = cache_key(&item, tier_name, &tree);
        if opts.reuse
            && tier != Tier::Quick
            && let Some(p) = prior.get(&id)
            && p["cache_key"] == key
            && !is_failure(p)
            && p["status"] == "done"
        {
            let mut p = p.clone();
            p["reused"] = json!(true);
            pool.write(&p);
            reused += 1;
            continue;
        }
        let hint = prior
            .get(&id)
            .and_then(|p| p["perf"]["peak_heap_bytes"].as_u64())
            .unwrap_or(if tier == Tier::Parse { 256 << 20 } else { 1 << 30 })
            .min(8 << 30);
        queue.push((item, key, hint));
    }
    // Biggest first, so the long jobs start early and the pool drains evenly.
    // UVI programs last: their banks open in the background meanwhile, and
    // opening a cold bank takes up to a minute.
    queue.sort_by_key(|q| (matches!(q.0, Item::UviProgram { .. }), std::cmp::Reverse(q.2)));
    let banks: Vec<PathBuf> = queue
        .iter()
        .filter_map(|q| match &q.0 {
            Item::UviProgram { bank, .. } => Some(bank.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let next_bank = Arc::new(AtomicUsize::new(0));
    for _ in 0..3.min(banks.len()) {
        let (banks, next_bank) = (banks.clone(), next_bank.clone());
        std::thread::spawn(move || {
            while let Some(p) = banks.get(next_bank.fetch_add(1, Ordering::Relaxed)) {
                let _ = bank(p);
            }
        });
    }
    pool.queue = queue;
    let total = pool.queue.len();
    eprintln!(
        "{total} to run, {reused} reused, {workers} workers, {} MiB free beyond the {} GiB reserve, tier {tier_name}",
        mem_available().saturating_sub(RESERVE) >> 20,
        RESERVE >> 30
    );
    install_panic_hook();
    let pool = Arc::new(pool);
    for _ in 0..workers.min(total) {
        spawn_worker(&pool);
        // Stagger the starts so the memory gate sees the first jobs ramp up.
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    watchdog(&pool);
    0
}

/// The ids of the quick sample: `CH_QUICK_FILE` or the embedded `quick.txt`.
fn quick_ids() -> HashSet<String> {
    let text = std::env::var_os("CH_QUICK_FILE")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| QUICK.to_string());
    text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect()
}

/// `quick [--workers N] [--baseline RUN]`: run the fixed sample, keep it as
/// `current.jsonl` (the last run becomes `previous.jsonl`), and print what
/// changed against the previous run (or `--baseline`).
fn quick(extra: &[String]) -> i32 {
    let flag = |name: &str| extra.iter().position(|a| a == name).and_then(|i| extra.get(i + 1));
    let dir = dirs_home().join(".cache/kontakto-corpus/quick");
    std::fs::create_dir_all(&dir).expect("quick dir");
    let (cur, prev) = (dir.join("current.jsonl"), dir.join("previous.jsonl"));
    if cur.exists() {
        let _ = std::fs::rename(&cur, &prev);
    }
    let opts = Opts {
        tier: Some("quick".into()),
        workers: flag("--workers").and_then(|v| v.parse().ok()),
        timeout: flag("--timeout").and_then(|v| v.parse().ok()),
        ..Default::default()
    };
    let started = Instant::now();
    let code = run(&cur, &opts);
    let wall = started.elapsed().as_secs();
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    println!(
        "quick tier: {} items, {wall} s wall (target 180 s), load average at end {}",
        all_records(&cur).iter().filter(|r| r["status"] != "started").count(),
        load.split_whitespace().next().unwrap_or("?")
    );
    let against = flag("--baseline").map(PathBuf::from).or_else(|| prev.exists().then(|| prev.clone()));
    match against {
        Some(old) => diff(&old, &cur),
        None => {
            println!("(no previous quick run to compare with)");
            diff(&cur, &cur);
        }
    }
    code
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// `quick-list RUN...`: the fixed sample, from finished runs (oldest first).
/// Per library: three instruments spread over its sorted ids. Then every
/// distinct failure mode (library, stage, cause: two members each), everything
/// that regressed between the runs, and four multis. Records of the old harness
/// (no `tier`) say nothing about UVI audio, so their UVI failures are ignored.
fn quick_list(runs: &[String]) {
    let library = |id: &str| {
        let rest = id.split("/Libraries/").nth(1).unwrap_or(id);
        rest.split('/').take(2).collect::<Vec<_>>().join("/")
    };
    let id_of = |r: &Value| r["id"].as_str().unwrap_or_default().to_string();
    let mut latest = BTreeMap::<String, Value>::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut ever_ok = HashSet::new();
    for run in runs {
        for r in all_records(Path::new(run)) {
            if r["status"] == "started" || r["tier"] == "parse" {
                continue;
            }
            let id = id_of(&r);
            let artefact = r["kind"] == "uvi-program" && r["tier"].is_null();
            if artefact {
                continue;
            }
            if !is_failure(&r) {
                ever_ok.insert(id.clone());
            } else if ever_ok.contains(&id) {
                ids.insert(id.clone()); // regressed
            }
            latest.insert(id, r);
        }
    }
    let mut by_lib = BTreeMap::<String, Vec<String>>::new();
    for id in latest.keys() {
        by_lib.entry(library(id)).or_default().push(id.clone());
    }
    for (_, members) in &by_lib {
        let plain: Vec<_> = members.iter().filter(|i| !i.ends_with(".nkm")).collect();
        let take = plain.len().min(3);
        for k in 0..take {
            ids.insert(plain[k * plain.len() / take.max(1)].clone());
        }
    }
    let mut modes = BTreeMap::<(String, String, String), Vec<String>>::new();
    for (id, r) in &latest {
        if is_failure(r) && !r["kind"].as_str().is_some_and(|k| k == "kontakt-multi") {
            let why = r["load"]["error"].as_str().or(r["sound"]["error"].as_str()).unwrap_or("").chars().take(60).collect();
            modes.entry((library(id), r["stage"].as_str().unwrap_or("").into(), why)).or_default().push(id.clone());
        }
    }
    for members in modes.values() {
        ids.insert(members[0].clone());
        ids.insert(members[members.len() / 2].clone());
    }
    let multis: Vec<_> = latest.keys().filter(|i| i.ends_with(".nkm")).collect();
    for k in 0..multis.len().min(4) {
        ids.insert(multis[k * multis.len() / 4].clone());
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

/// Audio-thread allocation calls of a record (the old format had one counter).
fn allocs(r: &Value) -> u64 {
    let p = &r["perf"]["render"];
    p["audio_thread_allocs"].as_u64().unwrap_or_else(|| {
        ["audio_allocs_note_on", "audio_allocs_release", "audio_allocs_steady"]
            .iter()
            .map(|k| p[*k].as_u64().unwrap_or(0))
            .sum()
    })
}

fn misses(r: &Value) -> u64 {
    r["perf"]["render"]["deadline_misses"].as_u64().unwrap_or(0)
}

fn finite_ok(r: &Value) -> bool {
    r["sound"]["finite"] != false
}

fn sounds(r: &Value) -> bool {
    r["sound"]["sounds"] == true || r["musical"]["sounds"] == true
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn pct(old: f64, new: f64) -> String {
    if old > 0.0 { format!("{:+.0}%", (new / old - 1.0) * 100.0) } else { "n/a".into() }
}

/// `diff OLD NEW`: totals of NEW, then what a change fixed, broke or made
/// slower. Each side is a `.jsonl` or a directory of shard files. Perf deltas
/// above 25 % and above an absolute floor (50 ms load, 64 MiB heap) are listed.
fn diff(old: &Path, new: &Path) {
    let index = |p: &Path| -> BTreeMap<String, Value> {
        all_records(p)
            .into_iter()
            .filter(|r| r["status"] != "started")
            .map(|r| (r["id"].as_str().unwrap_or_default().to_string(), r))
            .collect()
    };
    let stage = |r: &Value| match r["stage"].as_str() {
        Some(s) => s.to_string(),
        None if r["status"] != "done" => "crash".into(),
        None => stage_of(r).into(),
    };
    let why = |r: &Value| {
        let text = r["load"]["error"]
            .as_str()
            .or(r["sound"]["error"].as_str())
            .or(r["sound"]["why_silent"].as_str())
            .or(r["sound"]["script_faults"][0].as_str())
            .unwrap_or("");
        let at = r["load"]["where"].as_str().unwrap_or("");
        format!("{at} {text}").trim().chars().take(110).collect::<String>()
    };
    let (old, new) = (index(old), index(new));
    // A part that sounds once its controllers are up is working.
    let good = |s: &str| matches!(s, "ok" | "needs-controller");
    let (mut fixed, mut regressed, mut added, mut moved, mut flags) = (vec![], vec![], vec![], vec![], vec![]);
    let (mut slow, mut quick_load, mut fat) = (vec![], vec![], vec![]);
    let mut common = vec![];
    for (id, n) in &new {
        let ns = stage(n);
        match old.get(id) {
            None if !good(&ns) => added.push(format!("{id}  [{ns}] {}", why(n))),
            None => {}
            Some(o) => {
                common.push((o, n));
                let os = stage(o);
                match (good(&os), good(&ns)) {
                    (false, true) => fixed.push(format!("{id}  [{os} -> {ns}]")),
                    (true, false) => regressed.push(format!("{id}  [{os} -> {ns}] {}", why(n))),
                    _ if os != ns => moved.push(format!("{id}  [{os} -> {ns}] {}", why(n))),
                    _ => {}
                }
                if finite_ok(o) && !finite_ok(n) {
                    flags.push(format!("{id}  now produces non-finite samples"));
                }
                if sounds(o) && !sounds(n) {
                    flags.push(format!("{id}  stopped sounding"));
                }
                if misses(o) == 0 && misses(n) > 0 {
                    flags.push(format!("{id}  deadline misses 0 -> {}", misses(n)));
                }
                if allocs(o) == 0 && allocs(n) > 0 {
                    flags.push(format!("{id}  audio-thread allocations 0 -> {}", allocs(n)));
                }
                let (a, b) = (o["perf"]["load_ms"].as_f64().or(o["load_ms"].as_f64()), n["perf"]["load_ms"].as_f64().or(n["load_ms"].as_f64()));
                if let (Some(a), Some(b)) = (a, b) {
                    if b > a * 1.25 && b - a > 50.0 {
                        slow.push(format!("{id}  load {a:.0} -> {b:.0} ms"));
                    } else if a > b * 1.25 && a - b > 50.0 {
                        quick_load.push(format!("{id}  load {a:.0} -> {b:.0} ms"));
                    }
                }
                if let (Some(a), Some(b)) = (o["perf"]["peak_heap_bytes"].as_f64(), n["perf"]["peak_heap_bytes"].as_f64())
                    && b > a * 1.25 && b - a > 64.0 * 1048576.0 {
                        fat.push(format!("{id}  heap {:.0} -> {:.0} MiB", a / 1048576.0, b / 1048576.0));
                    }
            }
        }
    }
    let count = |m: &BTreeMap<String, Value>, f: &dyn Fn(&Value) -> bool| m.values().filter(|r| f(r)).count();
    let line = |m: &BTreeMap<String, Value>| {
        format!(
            "{} items: ok {} (incl. needs-controller), sounding {}, non-finite {}, deadline-miss {}, audio-alloc {}",
            m.len(),
            count(m, &|r| good(&stage(r))),
            count(m, &sounds),
            count(m, &|r| !finite_ok(r)),
            count(m, &|r| misses(r) > 0),
            count(m, &|r| allocs(r) > 0),
        )
    };
    println!("\nnow:      {}", line(&new));
    if old.keys().ne(new.keys()) || old.values().ne(new.values()) {
        println!("previous: {}", line(&old));
    }
    for (name, list) in [
        ("regressed", &regressed),
        ("fixed", &fixed),
        ("new failure", &added),
        ("failing stage changed", &moved),
        ("flags changed", &flags),
    ] {
        println!("\n{name}: {}", list.len());
        for l in list.iter().take(40) {
            println!("  {l}");
        }
    }
    let load = |side: &dyn Fn(&(&Value, &Value)) -> Value| -> Vec<f64> {
        common.iter().filter_map(|c| side(c)["perf"]["load_ms"].as_f64().or(side(c)["load_ms"].as_f64())).collect()
    };
    let heap = |side: &dyn Fn(&(&Value, &Value)) -> Value| -> Vec<f64> {
        common.iter().filter_map(|c| side(c)["perf"]["peak_heap_bytes"].as_f64()).collect()
    };
    let p99 = |side: &dyn Fn(&(&Value, &Value)) -> Value| -> Vec<f64> {
        common.iter().filter_map(|c| side(c)["perf"]["render"]["block_p99_ms"].as_f64()).collect()
    };
    let (o, n): (&dyn Fn(&(&Value, &Value)) -> Value, &dyn Fn(&(&Value, &Value)) -> Value) = (&|c| c.0.clone(), &|c| c.1.clone());
    let (lo, ln) = (load(o), load(n));
    println!("\nperf delta over {} items present in both:", common.len());
    println!("  load ms     median {:.0} -> {:.0} ({}), total {:.1} -> {:.1} s", median(lo.clone()), median(ln.clone()), pct(median(lo.clone()), median(ln.clone())), lo.iter().sum::<f64>() / 1e3, ln.iter().sum::<f64>() / 1e3);
    let (ho, hn) = (heap(o), heap(n));
    println!("  peak heap   median {:.0} -> {:.0} MiB, max {:.0} -> {:.0} MiB", median(ho.clone()) / 1048576.0, median(hn.clone()) / 1048576.0, ho.iter().copied().fold(0.0, f64::max) / 1048576.0, hn.iter().copied().fold(0.0, f64::max) / 1048576.0);
    let (po, pn) = (p99(o), p99(n));
    println!("  block p99   median {:.3} -> {:.3} ms ({})", median(po.clone()), median(pn.clone()), pct(median(po), median(pn)));
    for (name, list) in [("load time up", &slow), ("load time down", &quick_load), ("peak heap up", &fat)] {
        println!("  {name}: {}", list.len());
        for l in list.iter().take(10) {
            println!("    {l}");
        }
    }
    let failing: Vec<_> = new.iter().filter(|(_, r)| !good(&stage(r))).collect();
    println!("\nfailing now: {}", failing.len());
    for (id, r) in failing.iter().take(40) {
        println!("  [{}] {}  {}", stage(r), id.rsplit('/').next().unwrap_or(id), why(r));
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
            for item in cached_items(rest) {
                println!("{}\t{}", item.kind(), item.id());
            }
            0
        }
        [cmd, runs @ ..] if cmd == "quick-list" => {
            quick_list(runs);
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
