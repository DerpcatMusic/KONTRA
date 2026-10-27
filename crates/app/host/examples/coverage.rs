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
use kontra_host::script::UNDERSTOOD_SYMBOLS;
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
    {
        let Ok(mut r) = report.lock() else { return };
        r.instruments += 1;
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
                r.faults.push((name, f));
            }
        }
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

type Counts = Vec<(Action, u32)>;

/// Init, one note on and off: the actions script `slot` calls and leaves
/// unanswered, and its faults if any. Zones are dropped so nothing loads.
fn run(
    ir: &ir::Instrument,
    slot: usize,
    store: &mut Store,
) -> Option<(Counts, Counts, Option<String>)> {
    let mut ir = ir.clone();
    let script = ir.scripts.get(slot)?.clone();
    ir.scripts = vec![script];
    ir.samples.clear();
    for g in &mut ir.groups {
        g.zones.clear();
    }
    let loaded = kontra_host::instrument::build(ir, RATE, store);
    loaded.plan.script.as_ref()?;
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
        .filter(|f| *f != kontra_engine::Faults::default())
        .map(|f| format!("{f:?}"));
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
    for (a, u) in &rows {
        let (calls, unanswered) = r.calls.get(&a.id()).copied().unwrap_or_default();
        let status = if a.is_pure() {
            "vm"
        } else if probe.answers(*a) {
            if unanswered == 0 {
                "answered"
            } else {
                "partial"
            }
        } else if calls > unanswered {
            "partial"
        } else {
            "missing"
        };
        *tally.entry(status).or_default() += 1;
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
    let understood = |n: &str| UNDERSTOOD_SYMBOLS.iter().any(|u| u.eq_ignore_ascii_case(n));
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
        "Status: `vm` the VM answers it; `answered` the engine answers it and no run left a call unanswered; `partial` some calls or arguments go unanswered; `missing` the engine never answers it. Calls are summed over init, one note on (key 60) and off, once per distinct script.\n"
    );
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
