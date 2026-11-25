//! Every KSP oracle probe with a measurement beside it
//! (`tools/kontakt-oracle/probes/ksp/NAME.kspp` and `NAME.measured`, the
//! `label<TAB>value` lines `ksp_probe.py` printed for Kontakt 8.13) reads the
//! same values in KONTRA. A line marked `<TAB>unstable` read differently in
//! repeated Kontakt runs (uninitialised memory) and is not compared. A script
//! Kontakt rejects reads -1000000 everywhere; so does one KONTRA rejects.
#![cfg(test)]

use std::path::Path;
use std::sync::Arc;

use kontra_engine::{Engine, Plan};
use kontra_stream::{Store, StoreConfig};

/// The probe as one init: `S` lines run in order, `R` (integer) and `T`
/// (string) lines store their value, and the init ends with `message(cell)`:
/// one result per run, as a message holds 320 bytes. `X` lines follow it raw.
fn script(probe: &str, cell: &str) -> String {
    let (mut body, mut tail, mut r, mut t) = (Vec::new(), Vec::new(), 0, 0);
    for line in probe.lines().map(str::trim) {
        let read = |rest: &str| rest.split_once(' ').expect("label expr").1.to_owned();
        if let Some(s) = line.strip_prefix("S ") {
            body.push(s.to_owned());
        } else if let Some(rest) = line.strip_prefix("R ") {
            body.push(format!("%oracle_r[{r}] := {}", read(rest)));
            r += 1;
        } else if let Some(rest) = line.strip_prefix("T ") {
            body.push(format!("!oracle_t[{t}] := {}", read(rest)));
            t += 1;
        } else if let Some(x) = line.strip_prefix("X ") {
            tail.push(x.to_owned());
        } else {
            assert!(
                line.is_empty() || line.starts_with('#'),
                "unsupported probe line: {line}"
            );
        }
    }
    format!(
        "on init\n declare %oracle_r[{}]\n declare !oracle_t[{}]\n{}\n message({})\nend on\n{}\n",
        r.max(1),
        t.max(1),
        body.join("\n"),
        cell,
        tail.join("\n")
    )
}

/// Each result's label and the expression that reads its cell, in file order.
fn results(probe: &str) -> Vec<(&str, String)> {
    let (mut r, mut t) = (0, 0);
    let mut out = Vec::new();
    for line in probe.lines().map(str::trim) {
        let (cell, rest) = if let Some(rest) = line.strip_prefix("R ") {
            r += 1;
            (format!("%oracle_r[{}]", r - 1), rest)
        } else if let Some(rest) = line.strip_prefix("T ") {
            t += 1;
            (format!("!oracle_t[{}]", t - 1), rest)
        } else {
            continue;
        };
        out.push((rest.split_once(' ').expect("label expr").0, cell));
    }
    out
}

fn run(source: &str) -> String {
    let Ok(script) = kontra_ksp::compile(source, &[]) else {
        return "-1000000".to_owned();
    };
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(Vec::new(), 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), 48_000);
    e.process(&mut vec![[0.0; 2]; 64], &[]);
    e.last_message().to_owned()
}

#[test]
fn every_measured_probe_reads_as_kontakt_measured() {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/kontakt-oracle/probes/ksp");
    let (mut checked, mut unstable) = (0, 0);
    let mut wrong = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let measured = entry.path();
        if measured.extension().is_none_or(|e| e != "measured") {
            continue;
        }
        let probe = std::fs::read_to_string(measured.with_extension("kspp")).unwrap();
        let results = results(&probe);
        let name = measured.file_stem().unwrap().to_string_lossy().into_owned();
        for line in std::fs::read_to_string(&measured).unwrap().lines() {
            let mut cells = line.split('\t');
            let (Some(label), Some(want)) = (cells.next(), cells.next()) else {
                panic!("{name}: label<TAB>value, got {line:?}");
            };
            if cells.next() == Some("unstable") {
                unstable += 1;
                continue;
            }
            let cell = &results.iter().find(|r| r.0 == label).expect(label).1;
            let got = run(&script(&probe, cell));
            if got != want {
                wrong.push(format!("{name} {label}: Kontakt {want}, KONTRA {got}"));
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "no measured probe");
    assert!(
        unstable <= 9,
        "{unstable} unstable lines: measure them again"
    );
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
