#![cfg(feature = "library-access")]
use std::{path::Path, process::Command};

fn inspect(path: &Path, seed: Option<&str>, profile: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_corpus-health"));
    command
        .arg("inspect")
        .arg(path)
        .args(["--ui-only", "--initialize"])
        .env("KONTRA_SCAN_ACTIVE", "1")
        .env("KONTRA_INSPECT_READ_ONLY_INIT", "1")
        .env_remove("KONTRA_UVI_AUDIT_SEED");
    if let Some(seed) = seed {
        command.env("KONTRA_UVI_AUDIT_SEED", seed);
    }
    if profile {
        command.arg("--profile-init");
    }
    command.output().unwrap()
}

#[test]
fn explicit_seed_reproduces_on_off_model_and_missing_seed_keeps_defaults() {
    let root = std::env::temp_dir().join(format!("kontra-seed-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("fixture.uvip");
    std::fs::write(&path,"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[setSize(320,200) Knob('Seed',math.random(1,1000000),1,1000000)]]></script></ScriptProcessor></EventProcessors></Program></UVI4>").unwrap();
    let plain = inspect(&path, Some("611"), false);
    let profiled = inspect(&path, Some("611"), true);
    assert!(plain.status.success() && profiled.status.success());
    let plain: serde_json::Value = serde_json::from_slice(&plain.stdout).unwrap();
    let profiled: serde_json::Value = serde_json::from_slice(&profiled.stdout).unwrap();
    assert_eq!(plain["mode"]["audit_seed"], 611);
    assert_eq!(profiled["mode"]["audit_seed"], 611);
    assert_eq!(plain["ui"]["interfaces"], profiled["ui"]["interfaces"]);
    let ordinary = inspect(&path, None, false);
    assert!(ordinary.status.success());
    let ordinary: serde_json::Value = serde_json::from_slice(&ordinary.stdout).unwrap();
    assert!(ordinary["mode"]["audit_seed"].is_null());
    let invalid = inspect(&path, Some("-1"), false);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("unsigned 32-bit integer"));
    std::fs::remove_dir_all(root).unwrap();
}
