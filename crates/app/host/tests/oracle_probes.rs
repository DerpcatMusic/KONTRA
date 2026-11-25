//! Every KSP oracle probe with a measurement beside it
//! (`tools/kontakt-oracle/probes/ksp/NAME.kspp` and `NAME.measured`, the
//! `label<TAB>value` lines `ksp_probe.py` printed for Kontakt 8.13) reads the
//! same values in KONTRA. A line marked `<TAB>unstable` read differently in
//! repeated Kontakt runs (uninitialised memory) and is not compared.
#![cfg(test)]

use std::path::Path;
use std::sync::Arc;

use kontra_engine::{Engine, Plan};
use kontra_stream::{Store, StoreConfig};

/// The probe as one init: `S` lines run in order, `R` lines store their
/// value, and the values are sent as one message at the end.
fn script(probe: &str) -> (String, Vec<&str>) {
    let mut body = Vec::new();
    let mut labels = Vec::new();
    for line in probe.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix("S ") {
            body.push(s.to_owned());
        } else if let Some(r) = line.strip_prefix("R ") {
            let (label, expr) = r.split_once(' ').expect("R label expr");
            body.push(format!("%oracle_r[{}] := {expr}", labels.len()));
            labels.push(label);
        } else {
            assert!(
                line.is_empty() || line.starts_with('#'),
                "unsupported probe line: {line}"
            );
        }
    }
    let n = labels.len().max(1);
    let source = format!(
        "on init\n declare %oracle_r[{n}]\n declare @oracle_out\n declare $oracle_i\n{}\n \
         while ($oracle_i < {n})\n  @oracle_out := @oracle_out & %oracle_r[$oracle_i] & \" \"\n  \
         inc($oracle_i)\n end while\n message(@oracle_out)\nend on\n",
        body.join("\n")
    );
    (source, labels)
}

fn run(source: &str) -> String {
    let script = kontra_ksp::compile(source, &[]).unwrap();
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
        let (source, labels) = script(&probe);
        let ours = run(&source);
        let ours: Vec<&str> = ours.split_whitespace().collect();
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
            let at = labels.iter().position(|l| *l == label).expect(label);
            let got = ours.get(at).copied().unwrap_or("none");
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
