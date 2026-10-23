//! Diagnostics for the 0-error goal. Every run record becomes stable-coded
//! diagnostics (error / warn / info) attributed to a library and an item path.
//! `health` counts them per library and per code, writes JSON and the
//! LIBRARY_GAPS markdown, and exits non-zero on errors (only new ones when a
//! baseline is given).
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Per-item budgets. Over budget is a warning: it is a deadline miss, not a
/// wrong result. ponytail: fixed budgets, per-tier budgets if quick and full diverge.
const LOAD_DEADLINE_MS: u64 = 10_000;
const PLAY_DEADLINE_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub code: &'static str,
    pub severity: Severity,
    pub library: String,
    pub path: String,
    /// Occurrences affected: unsupported features, voices, notes; 1 for a whole item.
    pub impact: u64,
    pub message: String,
}

impl Diag {
    /// Identity across runs: the same problem on the same item is the same error.
    fn fingerprint(&self) -> String {
        format!("{}|{}|{}", self.code, self.library, self.path)
    }

    fn json(&self) -> Value {
        json!({
            "code": self.code,
            "severity": self.severity.name(),
            "library": self.library,
            "path": self.path,
            "impact": self.impact,
            "message": self.message,
        })
    }
}

/// Library name: the folder under the Kontakt or UVI root, else the item's own folder.
pub fn library_of(id: &str) -> String {
    let parts: Vec<&str> = id.split('/').collect();
    let under_root = parts
        .iter()
        .position(|p| *p == "Kontakt" || *p == "UVI")
        .and_then(|i| parts.get(i + 1));
    under_root
        .or_else(|| parts.len().checked_sub(2).and_then(|i| parts.get(i)))
        .map_or_else(|| "(other)".to_string(), |s| (*s).to_string())
}

/// A count from a number or the length of an array of faults.
fn count(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_array().map(|a| a.len() as u64))
        .unwrap_or(0)
}

/// All diagnostics one run record carries. Pure: safe to test on JSON.
pub fn classify(r: &Value) -> Vec<Diag> {
    let id = r["id"].as_str().unwrap_or("?");
    let library = library_of(id);
    let mut out = Vec::new();
    let mut push = |code: &'static str, severity, impact: u64, message: String| {
        out.push(Diag {
            code,
            severity,
            library: library.clone(),
            path: id.to_string(),
            impact,
            message,
        })
    };
    let status = r["status"].as_str().unwrap_or("done");
    let load_error = r["load"]["error"].as_str().unwrap_or("unknown").to_string();
    match status {
        "timeout" => push("RUN-TIMEOUT", Severity::Error, 1, load_error.clone()),
        "crash" => push("RUN-CRASH", Severity::Error, 1, load_error.clone()),
        "panic" => push("RUN-PANIC", Severity::Error, 1, load_error.clone()),
        _ if r["load"]["ok"] == false => push("LOAD-FAIL", Severity::Error, 1, load_error),
        _ => {}
    }
    let failed = r["scripts"]["compile_failed"].as_u64().unwrap_or(0);
    if failed > 0 {
        let first = r["scripts"]["compile_errors"][0].as_str().unwrap_or("");
        push("SCRIPT-COMPILE", Severity::Error, failed, first.to_string());
    }
    for (kind, n) in r["scripts"]["warnings"].as_object().into_iter().flatten() {
        push(
            "SCRIPT-WARN",
            Severity::Warn,
            n.as_u64().unwrap_or(1),
            format!("script {kind}"),
        );
    }
    for (feature, n) in r["unsupported"].as_object().into_iter().flatten() {
        push(
            "UNSUPPORTED",
            Severity::Warn,
            n.as_u64().unwrap_or(1),
            feature.clone(),
        );
    }
    let sound = &r["sound"];
    if let Some(e) = sound["error"].as_str() {
        push("PLAY-ERROR", Severity::Error, 1, e.to_string());
    } else if !sound.is_null() {
        if sound["note"].as_str().is_some_and(|n| n != "started") {
            push(
                "NOTE-NOT-STARTED",
                Severity::Error,
                1,
                sound["note"].as_str().unwrap_or("").to_string(),
            );
        }
        if sound["sounds"] == true {
            // Silence check passed; nothing to report.
        } else if r["musical"]["sounds"] == true {
            let needs: Vec<String> = r["musical"]["needs_cc"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| format!("CC{c}"))
                .collect();
            push(
                "SOUND-NEEDS-CC",
                Severity::Info,
                1,
                format!("needs controller {}", needs.join(",")),
            );
        } else if sound["note"].as_str() == Some("started") {
            let why = sound["why_silent"]
                .as_str()
                .unwrap_or("silent at a covered key");
            push("SOUND-SILENT", Severity::Error, 1, why.to_string());
        }
        if sound["finite"] == false {
            push(
                "SOUND-NONFINITE",
                Severity::Error,
                1,
                "non-finite samples".into(),
            );
        }
        let stuck = count(&sound["stuck_voices"]) + count(&sound["stuck_notes"]);
        if stuck > 0 {
            push(
                "SOUND-STUCK",
                Severity::Warn,
                stuck,
                "voices or notes never released".into(),
            );
        }
        let faults = count(&sound["script_faults"]);
        if faults > 0 {
            push(
                "SCRIPT-FAULT",
                Severity::Warn,
                faults,
                "script raised at runtime".into(),
            );
        }
    }
    let suspects = count(&sound["selection"]["suspect"]);
    if suspects > 0 {
        let first = sound["selection"]["suspect"][0].as_str().unwrap_or("");
        push("SOUND-SUSPECT", Severity::Warn, suspects, first.to_string());
    }
    if r["perf"]["load_ms"]
        .as_u64()
        .is_some_and(|ms| ms > LOAD_DEADLINE_MS)
    {
        push(
            "DEADLINE-LOAD",
            Severity::Warn,
            1,
            format!("load over {LOAD_DEADLINE_MS} ms"),
        );
    }
    if r["perf"]["play_ms"]
        .as_u64()
        .is_some_and(|ms| ms > PLAY_DEADLINE_MS)
    {
        push(
            "DEADLINE-PLAY",
            Severity::Warn,
            1,
            format!("play over {PLAY_DEADLINE_MS} ms"),
        );
    }
    out
}

#[derive(Default)]
struct Lib {
    items: u64,
    error: u64,
    warn: u64,
    info: u64,
    codes: BTreeMap<&'static str, u64>,
}

fn severity_index(s: Severity) -> usize {
    match s {
        Severity::Error => 0,
        Severity::Warn => 1,
        Severity::Info => 2,
    }
}

/// Load records from a run file or a directory of runs (dedup as the summary does).
fn records(run: &Path) -> Vec<Value> {
    super::all_records(run)
        .into_iter()
        .filter(|r| r["status"] != "started")
        .filter(|r| {
            !r["id"]
                .as_str()
                .unwrap_or("")
                .contains("KONTRA project recovery")
        })
        .collect()
}

fn fingerprints(baseline: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(baseline).unwrap_or_default();
    let doc: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    doc["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["severity"] == "error")
        .map(|d| {
            format!(
                "{}|{}|{}",
                d["code"].as_str().unwrap_or(""),
                d["library"].as_str().unwrap_or(""),
                d["path"].as_str().unwrap_or("")
            )
        })
        .collect()
}

/// `health RUN... [--json OUT] [--md OUT] [--baseline OLD.json]`. Exit 1 on errors.
pub fn run(args: &[String]) -> i32 {
    let mut runs = Vec::<PathBuf>::new();
    let (mut json_out, mut md_out, mut baseline) = (None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => json_out = it.next().map(PathBuf::from),
            "--md" => md_out = it.next().map(PathBuf::from),
            "--baseline" => baseline = it.next().map(PathBuf::from),
            _ => runs.push(PathBuf::from(arg)),
        }
    }
    if runs.is_empty() {
        eprintln!(
            "usage: corpus-health health RUN... [--json OUT] [--md OUT] [--baseline OLD.json]"
        );
        return 2;
    }
    let mut libs = BTreeMap::<String, Lib>::new();
    let mut diags = Vec::new();
    let mut total = 0u64;
    for run in &runs {
        for r in records(run) {
            total += 1;
            let lib = library_of(r["id"].as_str().unwrap_or("?"));
            libs.entry(lib).or_default().items += 1;
            diags.extend(classify(&r));
        }
    }
    let mut codes = BTreeMap::<&'static str, (Severity, u64, u64, BTreeSet<String>)>::new();
    let mut totals = [0u64; 3];
    for d in &diags {
        totals[severity_index(d.severity)] += 1;
        let lib = libs.entry(d.library.clone()).or_default();
        match d.severity {
            Severity::Error => lib.error += 1,
            Severity::Warn => lib.warn += 1,
            Severity::Info => lib.info += 1,
        }
        *lib.codes.entry(d.code).or_default() += 1;
        let c = codes
            .entry(d.code)
            .or_insert((d.severity, 0, 0, BTreeSet::new()));
        c.1 += 1;
        c.2 += d.impact;
        c.3.insert(d.library.clone());
    }
    let fresh: Vec<&Diag> = match &baseline {
        Some(path) => {
            let known = fingerprints(path);
            diags
                .iter()
                .filter(|d| d.severity == Severity::Error && !known.contains(&d.fingerprint()))
                .collect()
        }
        None => diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect(),
    };

    let mut ranked: Vec<(&String, &Lib)> = libs.iter().collect();
    ranked.sort_by(|a, b| {
        b.1.error
            .cmp(&a.1.error)
            .then(b.1.warn.cmp(&a.1.warn))
            .then(a.0.cmp(b.0))
    });
    println!(
        "items {total}: errors {} warnings {} info {}",
        totals[0], totals[1], totals[2]
    );
    println!(
        "{:<40} {:>6} {:>7} {:>7} {:>6}",
        "library", "items", "errors", "warns", "info"
    );
    for (name, lib) in &ranked {
        println!(
            "{:<40} {:>6} {:>7} {:>7} {:>6}",
            name, lib.items, lib.error, lib.warn, lib.info
        );
    }
    let mut code_rows: Vec<_> = codes.iter().collect();
    code_rows.sort_by(|a, b| a.1.0.cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)));
    println!(
        "\n{:<20} {:<6} {:>7} {:>8} {:>5}",
        "code", "sev", "count", "impact", "libs"
    );
    for (code, (sev, n, impact, libs)) in &code_rows {
        println!(
            "{:<20} {:<6} {:>7} {:>8} {:>5}",
            code,
            sev.name(),
            n,
            impact,
            libs.len()
        );
    }
    if !fresh.is_empty() {
        println!(
            "\n{} error(s) {}:",
            fresh.len(),
            if baseline.is_some() {
                "new against baseline"
            } else {
                ""
            }
        );
        for d in fresh.iter().take(50) {
            println!("  {} {} {}: {}", d.code, d.library, d.path, d.message);
        }
    }

    let diag_json: Vec<Value> = diags.iter().map(Diag::json).collect();
    let doc = json!({
        "items": total,
        "totals": {"error": totals[0], "warn": totals[1], "info": totals[2]},
        "libraries": ranked.iter().map(|(n, l)| json!({"library": n, "items": l.items, "error": l.error, "warn": l.warn, "info": l.info, "codes": l.codes})).collect::<Vec<_>>(),
        "codes": code_rows.iter().map(|(c, (s, n, i, l))| json!({"code": c, "severity": s.name(), "count": n, "impact": i, "libraries": l.len()})).collect::<Vec<_>>(),
        "diagnostics": diag_json,
    });
    if let Some(path) = &json_out {
        std::fs::write(path, serde_json::to_string_pretty(&doc).unwrap_or_default())
            .unwrap_or_else(|e| eprintln!("write {}: {e}", path.display()));
    }
    if let Some(path) = &md_out {
        let md = markdown(total, totals, &runs, &ranked, &code_rows);
        std::fs::write(path, md).unwrap_or_else(|e| eprintln!("write {}: {e}", path.display()));
    }
    i32::from(!fresh.is_empty())
}

fn markdown(
    total: u64,
    totals: [u64; 3],
    runs: &[PathBuf],
    ranked: &[(&String, &Lib)],
    codes: &[(&&'static str, &(Severity, u64, u64, BTreeSet<String>))],
) -> String {
    let mut md = String::from(
        "# Library gaps\n\n\
         Generated by `tools/corpus-health/run.sh health RUN... --md docs/re/engine/LIBRARY_GAPS.md`.\n\
         Do not edit by hand. Codes and severities are defined in `tools/corpus-health/src/health.rs`.\n\n",
    );
    let sources: Vec<String> = runs.iter().map(|r| format!("`{}`", r.display())).collect();
    md += &format!(
        "Sources: {}. Items {total}: {} errors, {} warnings, {} info.\n\n",
        sources.join(", "),
        totals[0],
        totals[1],
        totals[2]
    );
    md += "## Per code (ranked)\n\n| code | severity | count | impact | libraries |\n|---|---|---:|---:|---:|\n";
    for (code, (sev, n, impact, libs)) in codes {
        md += &format!(
            "| {code} | {} | {n} | {impact} | {} |\n",
            sev.name(),
            libs.len()
        );
    }
    md += "\n## Per library (ranked by errors)\n\n| library | items | errors | warnings | info | top codes |\n|---|---:|---:|---:|---:|---|\n";
    for (name, lib) in ranked {
        let mut top: Vec<_> = lib.codes.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        let top: Vec<String> = top
            .iter()
            .take(3)
            .map(|(c, n)| format!("{c} {n}"))
            .collect();
        md += &format!(
            "| {name} | {} | {} | {} | {} | {} |\n",
            lib.items,
            lib.error,
            lib.warn,
            lib.info,
            top.join(", ")
        );
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_load_failure_and_deadline_are_classified() {
        let r = json!({
            "id": "/x/Kontakt/Lib A/Instruments/a.nki",
            "status": "done",
            "load": {"ok": false, "error": "bad header"},
            "perf": {"load_ms": LOAD_DEADLINE_MS + 1},
        });
        let codes: Vec<_> = classify(&r)
            .iter()
            .map(|d| (d.code, d.library.clone()))
            .collect();
        assert_eq!(
            codes,
            vec![
                ("LOAD-FAIL", "Lib A".to_string()),
                ("DEADLINE-LOAD", "Lib A".to_string())
            ]
        );
    }

    #[test]
    fn library_is_the_folder_under_the_root_or_the_parent() {
        assert_eq!(
            library_of("/m/Libraries/Kontakt/Conflux 1.1/Multis/x.nkm"),
            "Conflux 1.1"
        );
        assert_eq!(library_of("tools/fixtures/a.uvip"), "fixtures");
    }

    #[test]
    fn silence_is_an_error_but_needs_controller_is_info() {
        let silent = json!({"id": "a/Kontakt/L/x.nki", "load": {"ok": true},
            "sound": {"note": "started", "sounds": false}});
        assert_eq!(classify(&silent)[0].code, "SOUND-SILENT");
        assert_eq!(classify(&silent)[0].severity, Severity::Error);
        let cc = json!({"id": "a/Kontakt/L/x.nki", "load": {"ok": true},
            "sound": {"note": "started", "sounds": false}, "musical": {"sounds": true, "needs_cc": [1]}});
        assert_eq!(classify(&cc)[0].code, "SOUND-NEEDS-CC");
        assert_eq!(classify(&cc)[0].severity, Severity::Info);
    }
}
