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
use sampler_core::{Limits, Outcome, Runtime};
use sampler_midi::{Applied, Ingress, Packets, TimedPacket, Version};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

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

fn categories(unsupported: &[sampler_ir::Unsupported]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for u in unsupported {
        *map.entry(normalize(&u.feature)).or_insert(0) += 1;
    }
    map
}

struct Sound {
    note: String,
    peak: f32,
    finite: bool,
    stuck_voices: usize,
    stuck_notes: usize,
    tail_peak: f32,
    faults: Vec<String>,
}

fn play(loaded: sampler_kontakt::Loaded, pick: Pick) -> Result<Sound, String> {
    let plan = loaded.plan;
    let rate = plan.sample_rate();
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
        behavior_cells: plan.behavior_local_count().saturating_mul(16),
        note_cells: plan.note_cell_count().saturating_mul(64),
    };
    let mut rt = Runtime::new(plan, limits).map_err(|e| format!("runtime: {e}"))?;
    rt.set_voice_stealing(Some(sampler_core::Stealing::for_limits(
        rt.sample_rate(),
        limits.voices,
    )))
    .map_err(|e| format!("runtime: {e}"))?;
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
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    #[allow(unused_mut)]
    let mut ingress = Ingress::new(0, groups);
    let mut buffer = [[0.0f32; 2]; 256];
    let (mut peak, mut tail_peak, mut finite) = (0.0f32, 0.0f32, true);
    let sw = u32::from(pick.switch.unwrap_or(0)) << 8;
    let switch_words: Vec<[u32; 1]> = if pick.switch.is_some() {
        vec![[0x2090_0000 | sw | 64], [0x2080_0000 | sw]]
    } else {
        vec![]
    };
    let note_index = if pick.switch.is_some() { 2 } else { 0 };
    let mut note = String::from("not sent");
    let mut faults = Vec::new();
    for begin in (0..total).step_by(buffer.len()) {
        let len = buffer.len().min(total - begin);
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
        ingress
            .render(
                &mut rt,
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
            .map_err(|e| format!("block: {e:?}"))?;
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
    Ok(Sound {
        note,
        peak,
        finite,
        stuck_voices: rt.voice_count(),
        stuck_notes: rt.note_count(),
        tail_peak,
        faults,
    })
}

fn scripts(loaded: &sampler_kontakt::Loaded) -> Value {
    let ir = &loaded.instrument;
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
    json!({
        "declared": ir.behaviors.len(),
        "bound": loaded.scripts.len(),
        "compile_failed": failed.len(),
        "compile_errors": failed.into_iter().take(3).collect::<Vec<_>>(),
        "warnings": warnings,
    })
}

fn load_item(item: &Item) -> (Value, Option<(sampler_kontakt::Loaded, Pick)>) {
    let failed = |reason: String| {
        (
            json!({"ok": false, "error": normalize(&reason), "raw": reason.lines().next().unwrap_or("").chars().take(300).collect::<String>()}),
            None,
        )
    };
    match item {
        Item::Kontakt(path) => {
            let ir = match sampler_kontakt::read(path) {
                Ok(read) => read.instrument,
                Err(e) => return failed(e.to_string()),
            };
            let Some(pick) = pick_key(&ir) else {
                return failed("no zone covers any key at any velocity".into());
            };
            let options = sampler_kontakt::Options {
                keys: pick.key..=pick.key,
                scripts: true,
                ..Default::default()
            };
            match sampler_kontakt::load(path, &options, |_| {}) {
                Ok(loaded) => (
                    json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": loaded.instrument.zones.len()}),
                    Some((loaded, pick)),
                ),
                Err(e) => failed(e.to_string()),
            }
        }
        Item::UviProgram { bank, program } => {
            let virtual_path = bank.join(program);
            let ir = match sampler_uvi::translate_path(&virtual_path) {
                Ok(t) => t.instrument,
                Err(e) => return failed(e.to_string()),
            };
            let Some(pick) = pick_key(&ir) else {
                return failed("no zone covers any key at any velocity".into());
            };
            let bank = match sampler_uvi::Bank::open(bank) {
                Ok(b) => b,
                Err(e) => return failed(e.to_string()),
            };
            let options = sampler_kontakt::Options {
                keys: pick.key..=pick.key,
                ..Default::default()
            };
            match sampler_uvi::load_program_with_options(&bank, program, &options) {
                Ok(loaded) => (
                    json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": loaded.instrument.zones.len()}),
                    Some((loaded, pick)),
                ),
                Err(e) => failed(e.to_string()),
            }
        }
        Item::UviLoose(path) => {
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
            {
                return failed("bank does not open".into());
            }
            match sampler_uvi::load(path, 48000) {
                Ok(loaded) => match pick_key(&loaded.instrument) {
                    Some(pick) => (
                        json!({"ok": true, "key": pick.key, "velocity": pick.velocity, "warning": pick.warning(), "zones": loaded.instrument.zones.len()}),
                        Some((loaded, pick)),
                    ),
                    None => failed("no zone covers any key at any velocity".into()),
                },
                Err(e) => failed(e.to_string()),
            }
        }
        Item::Multi(path) => match sampler_kontakt::read_multi(path) {
            Ok(multi) if multi.sample_names.is_empty() => {
                failed("no sample references in multi".into())
            }
            Ok(multi) => (
                json!({"ok": true, "programs": multi.programs.len(), "samples": multi.sample_names.len()}),
                None,
            ),
            Err(e) => failed(e.to_string()),
        },
    }
}

fn check(item: &Item) -> Value {
    let start = Instant::now();
    let (load, loaded) = load_item(item);
    let load_ms = start.elapsed().as_millis() as u64;
    let mut record = json!({
        "id": item.id(),
        "kind": item.kind(),
        "status": "done",
        "load": load,
    });
    if let Some((loaded, pick)) = loaded {
        record["scripts"] = scripts(&loaded);
        record["unsupported"] = json!(categories(&loaded.instrument.unsupported));
        record["unsupported_total"] = json!(loaded.instrument.unsupported.len());
        match play(loaded, pick) {
            Ok(s) => {
                let db = |p: f32| {
                    if p > 0.0 {
                        json!(20.0 * f64::from(p).log10())
                    } else {
                        Value::Null
                    }
                };
                record["sound"] = json!({
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
                    reason_set.insert("audible output 5 s after release (> -60 dBFS)".into());
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
    text += "\n";
    std::fs::write(md, text).expect("write summary");
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
        _ => eprintln!(
            "usage: corpus_health run OUT.jsonl [--shard I/N] [ROOT ...] | summary OUT.jsonl SUMMARY.md"
        ),
    }
}
