//! What real Kontakt libraries ask of KONTRA, and what it answers: every KSP
//! action and catalog name their scripts use, whether the compiler takes the
//! script, whether the engine answers the action (a zero-argument probe and
//! one init + note run per distinct script), and every feature the Kontakt
//! translator could not carry into the IR. Writes a ranked markdown report.
//! `cargo run --release -p kontra-host --example coverage -- <out.md> <library dir>...`

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kontra_engine::{Action, Engine, Event, HostNote, HostPattern, SampleTime, Timed, ir};
use kontra_host::script::{runs, understood};
use kontra_ksp::Builtin;
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;

#[derive(Default)]
struct Use {
    scripts: usize,
    libraries: BTreeSet<String>,
}

#[derive(Default)]
struct Report {
    instruments: usize,
    load_failures: Vec<(PathBuf, String)>,
    /// Distinct script sources seen.
    scripts: BTreeSet<u64>,
    compile_failures: Vec<(String, String)>,
    actions: BTreeMap<u16, Use>,
    symbols: BTreeMap<String, Use>,
    /// Calls and unanswered calls, summed over one run per distinct script.
    calls: BTreeMap<u16, (u64, u64)>,
    faults: Vec<(String, String)>,
    /// (impact, feature) -> instruments, libraries, an example.
    gaps: BTreeMap<(ir::Impact, String), (usize, BTreeSet<String>, String)>,
    /// `on <callback>` -> its use.
    callbacks: BTreeMap<String, Use>,
    /// What each distinct script uses: features keyed as the ranked list
    /// keys them (`a:<action id>`, `s:<name>`, `c:<callback>`, `x:...`).
    script_uses: BTreeMap<u64, BTreeSet<String>>,
    /// Each instrument: its library, its scripts, its IR gap keys.
    instruments_seen: Vec<(String, Vec<u64>, BTreeSet<String>)>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let out: PathBuf = args
        .next()
        .ok_or("usage: coverage <out.md> <dir>...")?
        .into();
    let mut files = Vec::new();
    for dir in args {
        walk(Path::new(&dir), &mut files);
    }
    files.sort();
    println!("{} instruments", files.len());

    let report = Mutex::new(Report::default());
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let Ok(mut store) = Store::new(StoreConfig::DEFAULT) else {
                    return;
                };
                while let Some(path) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    survey(path, &mut store, &report);
                }
            });
        }
    });
    let report = report.into_inner().map_err(|_| "a worker panicked")?;
    std::fs::write(&out, render(&report)?)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, found);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nki")) {
            found.push(p);
        }
    }
}

/// The library an instrument belongs to: the folder under the walked root.
fn library(path: &Path) -> String {
    let s = path.to_string_lossy();
    s.split("/Kontakt/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(&s)
        .to_string()
}

fn survey(path: &Path, store: &mut Store, report: &Mutex<Report>) {
    let lib = library(path);
    let ir = match kontra_format_kontakt::load(path) {
        Ok(ir) => ir,
        Err(e) => {
            if let Ok(mut r) = report.lock() {
                r.load_failures.push((path.to_path_buf(), e.to_string()));
            }
            return;
        }
    };
    let hashes: Vec<u64> = ir
        .scripts
        .iter()
        .filter(|s| s.dialect == ir::Dialect::Ksp)
        .map(|s| {
            let mut h = DefaultHasher::new();
            s.source.hash(&mut h);
            h.finish()
        })
        .collect();
    {
        let Ok(mut r) = report.lock() else { return };
        r.instruments += 1;
        let mut gaps: BTreeSet<String> = ir
            .diagnostics
            .iter()
            .map(|d| format!("g:{:?}:{}", d.impact, d.feature))
            .collect();
        // Features the file carries that the translator answers.
        if ir.scripts.iter().any(|s| !s.links.is_empty()) {
            gaps.insert("f:saved MIDI automation".into());
        }
        r.instruments_seen.push((lib.clone(), hashes, gaps));
        for d in &ir.diagnostics {
            let gap = r
                .gaps
                .entry((d.impact, d.feature.clone()))
                .or_insert_with(|| (0, BTreeSet::new(), d.detail.clone()));
            gap.0 += 1;
            gap.1.insert(lib.clone());
        }
    }
    for (slot, script) in ir.scripts.iter().enumerate() {
        if script.dialect != ir::Dialect::Ksp {
            continue;
        }
        let mut h = DefaultHasher::new();
        script.source.hash(&mut h);
        let hash = h.finish();
        let first = report.lock().is_ok_and(|mut r| r.scripts.insert(hash));
        if !first {
            continue;
        }
        let name = format!("{lib}: {} slot {slot} ({})", stem(path), script.name);
        let compiled = match kontra_ksp::compile(&script.source, &[]) {
            Ok(c) => c,
            Err(e) => {
                if let Ok(mut r) = report.lock() {
                    r.compile_failures.push((name, e.message.to_string()));
                    r.script_uses.insert(hash, ["x:compile".to_string()].into());
                }
                continue;
            }
        };
        let actions: BTreeSet<u16> = compiled
            .program
            .code
            .iter()
            .filter_map(|op| match op {
                kontra_engine::Op::Host { action, .. } => Some(action.id()),
                _ => None,
            })
            .collect();
        let run = run(&ir, slot, store);
        let Ok(mut r) = report.lock() else { return };
        let mut uses: BTreeSet<String> = actions.iter().map(|a| format!("a:{a}")).collect();
        uses.extend(compiled.symbols.iter().map(|n| format!("s:{n}")));
        let kinds: BTreeSet<String> = compiled
            .callbacks
            .iter()
            .map(|c| format!("{:?}", c.kind))
            .collect();
        for name in kinds {
            uses.insert(format!("c:{name}"));
            let u = r.callbacks.entry(name).or_default();
            u.scripts += 1;
            u.libraries.insert(lib.clone());
        }
        if run.as_ref().is_some_and(|(_, _, f)| f.is_some()) {
            uses.insert("x:fault".into());
        }
        r.script_uses.insert(hash, uses);
        for a in actions {
            let u = r.actions.entry(a).or_default();
            u.scripts += 1;
            u.libraries.insert(lib.clone());
        }
        for s in &compiled.symbols {
            let u = r.symbols.entry(s.to_string()).or_default();
            u.scripts += 1;
            u.libraries.insert(lib.clone());
        }
        if let Some((calls, unanswered, faults)) = run {
            for (a, n) in calls {
                r.calls.entry(a.id()).or_default().0 += u64::from(n);
            }
            for (a, n) in unanswered {
                r.calls.entry(a.id()).or_default().1 += u64::from(n);
            }
            if let Some(f) = faults {
                r.faults.push((name, describe(&f, &compiled)));
            }
        }
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

type Counts = Vec<(Action, u32)>;
type Run = (Counts, Counts, Option<kontra_engine::Faults>);

/// `f` with its first out-of-range array named from `script`'s variables,
/// and the ops that pushed its first zero divisor.
fn describe(f: &kontra_engine::Faults, script: &kontra_ksp::Script) -> String {
    let mut s = format!(
        "div0 {}, out of range {}, stack {}, texts cut {}, loop limits {}, no coroutine {}, bad code {}",
        f.divide_by_zero,
        f.out_of_range,
        f.stack_overflow,
        f.texts_cut,
        f.loop_limits,
        f.no_coroutine,
        f.bad_code
    );
    let name = |cell: u32, text: bool| {
        script.variables.iter().find_map(|v| {
            let (base, len, t) = match v.cells {
                kontra_ksp::Cells::Int { base, len } | kontra_ksp::Cells::Real { base, len } => {
                    (base, len, false)
                }
                kontra_ksp::Cells::Text { base, len } => (base, len, true),
                _ => return None,
            };
            (t == text && (base..base + len.max(1)).contains(&cell))
                .then(|| (v.name.to_string(), len))
        })
    };
    if let Some((base, text, index)) = f.first_out_of_range {
        let array = name(base, text).map_or("?".into(), |(n, len)| format!("{n}[{len}]"));
        let _ = write!(s, "; first: {array}[{index}]");
    }
    if let Some(pc) = f.first_divide_by_zero {
        // The ops that pushed the divisor, their cells named.
        let ops: Vec<String> = (pc.saturating_sub(8)..pc)
            .filter_map(|p| script.program.code.get(p as usize))
            .map(|op| match op {
                kontra_engine::Op::Load(n) => {
                    name(*n, false).map_or_else(|| format!("{op:?}"), |(n, _)| n)
                }
                kontra_engine::Op::LoadAt { base, .. } => {
                    name(*base, false).map_or_else(|| format!("{op:?}"), |(n, _)| format!("{n}[]"))
                }
                _ => format!("{op:?}"),
            })
            .collect();
        let _ = write!(s, "; first div0 at code {pc} after {}", ops.join(" "));
    }
    s
}

/// Init, one note on and off: the actions script `slot` calls and leaves
/// unanswered, and its faults if any. Zones are dropped so nothing loads.
fn run(ir: &ir::Instrument, slot: usize, store: &mut Store) -> Option<Run> {
    let mut ir = ir.clone();
    let script = ir.scripts.get(slot)?.clone();
    ir.scripts = vec![script];
    ir.samples.clear();
    for g in &mut ir.groups {
        g.zones.clear();
    }
    let loaded = kontra_host::instrument::build(ir, RATE, store);
    loaded.plan.scripts.first()?;
    let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let host = HostNote {
        port: 0,
        channel: 0,
        key: 60,
        id: -1,
        clap: true,
    };
    let pattern = HostPattern {
        port: 0,
        channel: 0,
        key: 60,
        id: -1,
        clap: true,
    };
    let on = [Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: host,
            velocity: 0.8,
            tune: 0.0,
        },
    }];
    let off = [Timed {
        at: SampleTime(0),
        body: Event::NoteOff(pattern),
    }];
    let mut block = vec![[0.0f32; 2]; 512];
    for n in 0..200 {
        let input: &[Timed<Event>] = match n {
            0 => &on,
            100 => &off,
            _ => &[],
        };
        engine.process(&mut block, input);
    }
    let faults = engine
        .script_faults()
        .into_iter()
        .find(|f| *f != kontra_engine::Faults::default());
    Some((engine.action_calls(), engine.unhandled_actions(), faults))
}

/// The KSP names an action answers to.
fn names(action: Action) -> String {
    let names: Vec<_> = Builtin::ALL
        .iter()
        .filter(|b| b.action() == Some(action))
        .map(|b| b.name())
        .collect();
    if names.is_empty() {
        format!("{action:?}")
    } else {
        names.join(", ")
    }
}

fn render(r: &Report) -> std::io::Result<String> {
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let plan = kontra_host::instrument::build(ir::Instrument::DEFAULT, RATE, &mut store).plan;
    let mut probe = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut rows: Vec<_> = r
        .actions
        .iter()
        .filter_map(|(&id, u)| Some((Action::from_id(id)?, u)))
        .collect();
    rows.sort_by_key(|(a, u)| {
        (
            std::cmp::Reverse(u.libraries.len()),
            std::cmp::Reverse(u.scripts),
            a.id(),
        )
    });
    let mut table = String::new();
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut status_of: BTreeMap<u16, &str> = BTreeMap::new();
    for (a, u) in &rows {
        let (calls, unanswered) = r.calls.get(&a.id()).copied().unwrap_or_default();
        // Real calls decide; the zero-argument probe only speaks for an
        // action no run reached.
        let status = if a.is_pure() {
            "vm"
        } else if calls > 0 && unanswered == 0 {
            "answered"
        } else if calls > unanswered {
            "partial"
        } else if calls == 0 && probe.answers(*a) {
            "answered"
        } else {
            "missing"
        };
        *tally.entry(status).or_default() += 1;
        status_of.insert(a.id(), status);
        let _ = writeln!(
            table,
            "| `{}` | {status} | {} | {} | {calls} | {unanswered} |",
            names(*a),
            u.libraries.len(),
            u.scripts
        );
    }
    let mut symbols: Vec<_> = r.symbols.iter().collect();
    symbols.sort_by_key(|(n, u)| {
        (
            std::cmp::Reverse(u.libraries.len()),
            std::cmp::Reverse(u.scripts),
            n.as_str(),
        )
    });
    let used_understood = symbols.iter().filter(|(n, _)| understood(n)).count();

    let mut s = String::new();
    let _ = writeln!(s, "# KSP and IR coverage of the Kontakt corpus\n");
    let _ = writeln!(
        s,
        "Generated by `crates/app/host/examples/coverage.rs`. {} instruments, {} distinct scripts, {} load failures, {} compile failures.\n",
        r.instruments,
        r.scripts.len(),
        r.load_failures.len(),
        r.compile_failures.len()
    );
    let _ = writeln!(
        s,
        "Status: `vm` the VM answers it; `answered` every call in the runs was answered (or, never reached, the engine answers a bare probe); `partial` some calls or arguments go unanswered; `missing` the engine never answers it. Calls are summed over init, one note on (key 60) and off, once per distinct script.\n"
    );
    s.push_str(&headline(r, &status_of));
    let _ = writeln!(s, "## Summary\n");
    let _ = writeln!(
        s,
        "- Actions used: {} ({})",
        rows.len(),
        tally
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        s,
        "- Catalog names used: {}, understood by the engine: {used_understood}",
        symbols.len()
    );
    let _ = writeln!(
        s,
        "- IR gaps: {} features the translator could not carry\n",
        r.gaps.len()
    );

    let _ = writeln!(s, "## IR gaps (translator), worst first\n");
    let _ = writeln!(
        s,
        "| Impact | Feature | Instruments | Libraries | Example |\n|---|---|---:|---:|---|"
    );
    let mut gaps: Vec<_> = r.gaps.iter().collect();
    gaps.sort_by_key(|((impact, f), (n, libs, _))| {
        (
            std::cmp::Reverse(*impact),
            std::cmp::Reverse(libs.len()),
            std::cmp::Reverse(*n),
            f.clone(),
        )
    });
    for ((impact, feature), (n, libs, example)) in gaps {
        let _ = writeln!(
            s,
            "| {impact:?} | {feature} | {n} | {} | {} |",
            libs.len(),
            example
                .replace('|', "/")
                .chars()
                .take(80)
                .collect::<String>()
        );
    }

    let _ = writeln!(s, "\n## Actions, most used first\n");
    let _ = writeln!(
        s,
        "| KSP | Status | Libraries | Scripts | Calls | Unanswered |\n|---|---|---:|---:|---:|---:|"
    );
    s.push_str(&table);

    let _ = writeln!(s, "\n## Catalog names (opaque symbols), most used first\n");
    let _ = writeln!(
        s,
        "| Name | Understood | Libraries | Scripts |\n|---|---|---:|---:|"
    );
    for (n, u) in symbols {
        let _ = writeln!(
            s,
            "| `{n}` | {} | {} | {} |",
            if understood(n) { "yes" } else { "no" },
            u.libraries.len(),
            u.scripts
        );
    }
    if !r.compile_failures.is_empty() {
        let _ = writeln!(s, "\n## Compile failures\n");
        for (name, e) in &r.compile_failures {
            let _ = writeln!(s, "- {name}: {e}");
        }
    }
    if !r.faults.is_empty() {
        let _ = writeln!(s, "\n## Runtime faults (init + one note)\n");
        for (name, f) in &r.faults {
            let _ = writeln!(s, "- {name}: {f}");
        }
    }
    if !r.load_failures.is_empty() {
        let _ = writeln!(s, "\n## Load failures\n");
        for (p, e) in &r.load_failures {
            let _ = writeln!(s, "- {}: {e}", p.display());
        }
    }
    Ok(s)
}

/// Whether feature `key` (as [`Report::script_uses`] keys it) is answered,
/// and its kind, status and weight (how much a user meets it).
fn judge(key: &str, status_of: &BTreeMap<u16, &str>) -> (bool, &'static str, String, u32) {
    let (kind, name) = key.split_once(':').unwrap_or(("?", key));
    match kind {
        "a" => {
            let status = name
                .parse()
                .ok()
                .and_then(|id: u16| status_of.get(&id))
                .copied()
                .unwrap_or("missing");
            let good = matches!(status, "vm" | "answered");
            (
                good,
                "command",
                status.into(),
                if status == "missing" { 3 } else { 2 },
            )
        }
        "s" => (understood(name), "name", "unused by engine".into(), 1),
        "c" => {
            let good = kontra_ksp::CallbackKind::ALL
                .iter()
                .find(|k| format!("{k:?}") == name)
                .is_some_and(|&k| runs(k));
            (good, "callback", "never runs".into(), 3)
        }
        "g" => {
            let impact = name.split(':').next().unwrap_or("");
            let w = match impact {
                "Broken" => 4,
                "Audible" => 3,
                "Latent" => 2,
                _ => 1,
            };
            (false, "IR gap", impact.into(), w)
        }
        "f" => (true, "file", "answered".into(), 1),
        _ => (false, "script", name.into(), 4),
    }
}

/// The headline (answered features, clean instruments and libraries) and
/// every open feature ranked by libraries x weight.
fn headline(r: &Report, status_of: &BTreeMap<u16, &str>) -> String {
    // feature -> (libraries, instruments)
    let mut seen: BTreeMap<String, (BTreeSet<&str>, usize)> = BTreeMap::new();
    let mut libraries: BTreeMap<&str, bool> = BTreeMap::new();
    let mut clean = 0;
    for (lib, hashes, gaps) in &r.instruments_seen {
        let mut used: BTreeSet<&String> = gaps.iter().collect();
        for h in hashes {
            used.extend(r.script_uses.get(h).into_iter().flatten());
        }
        let mut ok = true;
        for f in used {
            let e = seen.entry(f.clone()).or_default();
            e.0.insert(lib);
            e.1 += 1;
            ok &= judge(f, status_of).0;
        }
        clean += usize::from(ok);
        *libraries.entry(lib).or_insert(true) &= ok;
    }
    let answered = seen.keys().filter(|f| judge(f, status_of).0).count();
    let mut open: Vec<_> = seen
        .iter()
        .filter_map(|(f, (libs, n))| {
            let (good, kind, status, w) = judge(f, status_of);
            (!good).then(|| (libs.len() as u32 * w, f, kind, status, libs.len(), *n))
        })
        .collect();
    open.sort_by(|a, b| b.0.cmp(&a.0).then(b.5.cmp(&a.5)).then(a.1.cmp(b.1)));
    let mut s = String::new();
    let _ = writeln!(
        s,
        "## Headline\n\n**{answered} of {} features answered; {clean} of {} instruments and {} of {} libraries fully clean.**\n",
        seen.len(),
        r.instruments_seen.len(),
        libraries.values().filter(|&&c| c).count(),
        libraries.len()
    );
    for (f, (libs, n)) in seen.iter().filter(|(f, _)| f.starts_with("f:")) {
        let _ = writeln!(
            s,
            "File feature `{}`: {n} instruments, {} libraries.\n",
            &f[2..],
            libs.len()
        );
    }
    let _ = writeln!(
        s,
        "A feature is a KSP command, a catalog name, a callback a script uses, something the instrument file carries (saved MIDI automation), or a translator (IR) gap. A file feature is answered when the translator carries it. A command is answered when the engine (or the VM) answers every call; a name when the engine or the UI tree acts on it; a callback when the engine runs it. A gap is never answered. An instrument is clean when every feature it uses is answered.\n"
    );
    let _ = writeln!(
        s,
        "## Open features, ranked (libraries x weight)\n\nWeight: 4 broken or script failure, 3 audible gap / missing command / dead callback, 2 partial command or latent gap, 1 unused name or cosmetic gap.\n\n| Rank | Score | Kind | Feature | Status | Libraries | Instruments |\n|---:|---:|---|---|---|---:|---:|"
    );
    for (n, (score, f, kind, status, libs, inst)) in open.iter().enumerate() {
        let name = match f.split_once(':') {
            Some(("a", id)) => id
                .parse()
                .ok()
                .and_then(Action::from_id)
                .map_or(f.to_string(), names),
            Some(("g", rest)) => rest.split_once(':').map_or(rest, |(_, g)| g).to_string(),
            Some((_, n)) => n.to_string(),
            None => f.to_string(),
        };
        let _ = writeln!(
            s,
            "| {} | {score} | {kind} | `{}` | {status} | {libs} | {inst} |",
            n + 1,
            name.replace('|', "/")
        );
    }
    s.push('\n');
    s
}
