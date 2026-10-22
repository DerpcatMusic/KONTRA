//! KSP symbol usage across an instrument library, classified by the same SYMBOLS.tsv
//! table the plugin's Diagnostics and `tools/re/ksp_symbols.py doctor` read.
//!
//!   ksp OUT.jsonl [--md REPORT.md] PATH...   one record per instrument and symbol;
//!                                             PATH is a .nki or a folder of them
//!   ksp-summary OUT.jsonl                     per-library view of those records
//!
//! The report ranks symbols that are not `full` by the instruments that use them.
use sampler_ksp::symbols::{self, Status};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};

/// `Lib/Instruments/X.nki` belongs to `Lib`; an instrument elsewhere belongs to its folder.
pub fn library_of(path: &Path) -> String {
    let parent = path.parent();
    let name = |p: Option<&Path>| {
        p.and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    };
    match name(parent) {
        Some(n) if n.eq_ignore_ascii_case("instruments") => name(parent.and_then(Path::parent)),
        other => other,
    }
    .unwrap_or_else(|| "(root)".into())
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(read) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            collect(&entry, out);
        } else if entry
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            out.push(entry);
        }
    }
}

/// Symbol uses of one instrument: name -> (status, occurrences).
fn instrument_symbols(path: &Path) -> Result<BTreeMap<String, (Status, u64)>, String> {
    let reports =
        sampler_kontakt::read_parity_reports_summary(path).map_err(|e| format!("{e:?}"))?;
    let mut uses = BTreeMap::new();
    for report in reports
        .iter()
        .filter(|r| r.lang == sampler_parity::Lang::Ksp)
    {
        for construct in &report.constructs {
            let status = symbols::of_construct(&construct.name, construct.status);
            let entry = uses.entry(construct.name.clone()).or_insert((status, 0u64));
            entry.1 += 1;
        }
    }
    Ok(uses)
}

fn record_for(path: &Path, uses: &BTreeMap<String, (Status, u64)>) -> Vec<Value> {
    let library = library_of(path);
    let full = uses.values().filter(|(s, _)| s.is_full()).count();
    let mut out = vec![json!({
        "kind": "instrument",
        "library": library,
        "path": path.display().to_string(),
        "symbols": uses.len(),
        "full": full,
    })];
    for (name, (status, occurrences)) in uses {
        out.push(json!({
            "kind": "symbol",
            "library": library,
            "path": path.display().to_string(),
            "symbol": name,
            "status": status.name(),
            "code": status.code(),
            "occurrences": occurrences,
            "corpus_rank": symbols::lookup(name).and_then(|s| s.corpus_rank),
        }));
    }
    out
}

pub fn run(args: &[String]) -> i32 {
    let mut out = None;
    let mut md = None;
    let mut roots = Vec::<PathBuf>::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--md" => md = it.next().map(PathBuf::from),
            _ if out.is_none() => out = Some(PathBuf::from(arg)),
            _ => roots.push(PathBuf::from(arg)),
        }
    }
    let Some(out) = out.filter(|_| !roots.is_empty()) else {
        eprintln!("usage: corpus-health ksp OUT.jsonl [--md REPORT.md] PATH...");
        return 2;
    };
    let mut files = Vec::new();
    for root in &roots {
        collect(root, &mut files);
    }
    let mut records = Vec::new();
    let mut failed = 0usize;
    for file in &files {
        match instrument_symbols(file) {
            Ok(uses) => records.extend(record_for(file, &uses)),
            Err(error) => {
                // Bounded and path-only: the reader error can carry authored names.
                eprintln!("ksp: skipped {}: {error}", file.display());
                failed += 1;
                records.push(json!({"kind": "error", "library": library_of(file), "path": file.display().to_string()}));
            }
        }
    }
    let Ok(mut writer) = std::fs::File::create(&out) else {
        eprintln!("ksp: cannot create {}", out.display());
        return 2;
    };
    for record in &records {
        let _ = writeln!(writer, "{record}");
    }
    let instruments = files.len() - failed;
    println!(
        "ksp: {instruments} instruments, {failed} unreadable, {} records -> {}",
        records.len(),
        out.display()
    );
    if let Some(md) = md {
        if let Err(error) = std::fs::write(&md, ranked_markdown(&records)) {
            eprintln!("ksp: cannot write {}: {error}", md.display());
            return 2;
        }
        println!("ksp: ranked report -> {}", md.display());
    }
    0
}

/// Read JSONL records back, skipping any line that is not a record.
fn read_records(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Symbols that are not full, ranked by the instruments that use them (then occurrences).
fn ranked_markdown(records: &[Value]) -> String {
    #[derive(Default)]
    struct Row {
        code: String,
        instruments: BTreeSet<String>,
        libraries: BTreeSet<String>,
        occurrences: u64,
        corpus_rank: Option<u64>,
    }
    let mut rows = BTreeMap::<String, Row>::new();
    for r in records
        .iter()
        .filter(|r| r["kind"] == "symbol" && r["code"] != "KSP-FULL")
    {
        let row = rows
            .entry(r["symbol"].as_str().unwrap_or("?").into())
            .or_default();
        row.code = r["code"].as_str().unwrap_or("?").into();
        row.instruments
            .insert(r["path"].as_str().unwrap_or("?").into());
        row.libraries
            .insert(r["library"].as_str().unwrap_or("?").into());
        row.occurrences += r["occurrences"].as_u64().unwrap_or(0);
        row.corpus_rank = r["corpus_rank"].as_u64().or(row.corpus_rank);
    }
    let mut ranked: Vec<_> = rows.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.instruments
            .len()
            .cmp(&a.1.instruments.len())
            .then(b.1.occurrences.cmp(&a.1.occurrences))
            .then(a.0.cmp(&b.0))
    });
    let mut md = String::from(
        "# KSP symbols not full, by instruments affected\n\n\
         Weight = instruments that use the symbol. Status from docs/re/ksp/SYMBOLS.tsv.\n\n\
         | # | code | symbol | instruments | libraries | occurrences | corpus rank |\n|---:|---|---|---:|---:|---:|---:|\n",
    );
    for (i, (name, row)) in ranked.iter().enumerate() {
        let rank = row.corpus_rank.map_or("-".into(), |r| r.to_string());
        md.push_str(&format!(
            "| {} | {} | `{}` | {} | {} | {} | {} |\n",
            i + 1,
            row.code,
            name,
            row.instruments.len(),
            row.libraries.len(),
            row.occurrences,
            rank
        ));
    }
    md.push_str(&format!("\n{} symbols not full.\n", ranked.len()));
    md
}

/// Per-library view: instruments, percent of used symbols that are full, and blockers.
pub fn summary(args: &[String]) -> i32 {
    let [records] = args else {
        eprintln!("usage: corpus-health ksp-summary OUT.jsonl");
        return 2;
    };
    let records = read_records(Path::new(records));
    let mut libs = BTreeMap::<String, (usize, usize, usize, usize, BTreeMap<String, usize>)>::new();
    for r in &records {
        let lib = r["library"].as_str().unwrap_or("?").to_string();
        let entry = libs.entry(lib).or_default();
        match r["kind"].as_str() {
            Some("instrument") => {
                entry.0 += 1;
                entry.1 += r["symbols"].as_u64().unwrap_or(0) as usize;
                entry.2 += r["full"].as_u64().unwrap_or(0) as usize;
            }
            Some("error") => entry.3 += 1,
            Some("symbol") if r["code"] != "KSP-FULL" => {
                *entry
                    .4
                    .entry(r["symbol"].as_str().unwrap_or("?").into())
                    .or_default() += 1;
            }
            _ => {}
        }
    }
    println!(
        "| library | instruments | unreadable | symbols used | % full | top blockers (instruments) |"
    );
    println!("|---|---:|---:|---:|---:|---|");
    for (lib, (instruments, symbols, full, errors, blockers)) in &libs {
        let pct = if *symbols == 0 {
            0.0
        } else {
            100.0 * *full as f64 / *symbols as f64
        };
        let mut top: Vec<_> = blockers.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let top: Vec<String> = top
            .iter()
            .take(5)
            .map(|(s, n)| format!("{s} ({n})"))
            .collect();
        println!(
            "| {lib} | {instruments} | {errors} | {symbols} | {pct:.1} | {} |",
            top.join(", ")
        );
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_the_folder_above_instruments() {
        assert_eq!(
            library_of(Path::new("/x/Conflux/Instruments/Conflux.nki")),
            "Conflux"
        );
        assert_eq!(library_of(Path::new("/x/Loose/a.nki")), "Loose");
    }

    #[test]
    fn report_ranks_non_full_symbols_by_instruments_affected() {
        let rec = |path: &str, symbol: &str, code: &str, n: u64| {
            json!({
                "kind": "symbol", "library": "L", "path": path, "symbol": symbol,
                "status": "x", "code": code, "occurrences": n, "corpus_rank": 3,
            })
        };
        let records = vec![
            rec("a.nki", "rare", "KSP-UNSUPPORTED", 50),
            rec("a.nki", "common", "KSP-SIMPLIFIED", 1),
            rec("b.nki", "common", "KSP-SIMPLIFIED", 1),
            rec("a.nki", "fine", "KSP-FULL", 9),
        ];
        let md = ranked_markdown(&records);
        let common = md.find("`common`").unwrap();
        let rare = md.find("`rare`").unwrap();
        assert!(
            common < rare,
            "two instruments outrank one heavy use:\n{md}"
        );
        assert!(!md.contains("`fine`"));
        assert!(md.contains("2 symbols not full."));
    }
}
