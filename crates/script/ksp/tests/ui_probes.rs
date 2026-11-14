//! The KSP UI probe suite (`tools/ksp-ui/gen.py`) compiles: every page the
//! oracle and KONTRA are diffed on is a script both can load.

use std::path::Path;

#[test]
fn every_ui_probe_compiles() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/ksp-ui/probes");
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ksp") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        if let Err(e) = kontra_ksp::compile(&source, &[]) {
            failures.push(format!("{}: {e}", path.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
