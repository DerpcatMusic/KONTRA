//! Tests for the shared `kontra_format::Format` contract, protection reporting,
//! and Linux path/container reliability in `kontra-format-kontakt`.
#![cfg(test)]

use std::path::{Path, PathBuf};

use kontra_format::{
    Confidence, Format, FormatId, LoadCx, LoadOptions, Notes, ProtectionStatus, ProtectionSystem,
    Registry, ScanCollector,
};
use kontra_format_kontakt::{Kontakt, inspect_protection};

#[test]
fn kontakt_format_sniffs_magic_and_extensions() {
    let mut reg = Registry::new();
    reg.register(Kontakt);

    // NIS container magic ("hsin" at offset 12) + .nki extension -> Exact
    let mut nis_head = [0u8; 20];
    nis_head[12..16].copy_from_slice(b"hsin");
    let (fmt, conf) = reg.sniff(Path::new("Strings.nki"), &nis_head).unwrap();
    assert_eq!(fmt.id(), FormatId::Kontakt);
    assert_eq!(conf, Confidence::Exact);

    // NKS v2 LE magic without extension -> Header
    let nks_head = 0x7FA8_9012u32.to_le_bytes();
    let (fmt, conf) = reg.sniff(Path::new("unnamed_patch"), &nks_head).unwrap();
    assert_eq!(fmt.id(), FormatId::Kontakt);
    assert_eq!(conf, Confidence::Header);

    // Extension only -> Extension
    let (fmt, conf) = reg.sniff(Path::new("Patch.nksn"), b"unknown").unwrap();
    assert_eq!(fmt.id(), FormatId::Kontakt);
    assert_eq!(conf, Confidence::Extension);
}

#[test]
fn registry_loads_unencrypted_fixture_via_format_trait() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/glide-time.nki");
    let mut reg = Registry::new();
    reg.register(Kontakt);

    assert_eq!(
        reg.probe_protection(&fixture).unwrap(),
        ProtectionStatus::Open
    );

    let mut notes = Notes::default();
    let opts = LoadOptions::default();
    let mut cx = LoadCx::new(&mut notes, &opts);
    let inst = reg.load(&fixture, &mut cx).unwrap();
    assert!(!inst.groups.is_empty());

    let mut collector = ScanCollector::default();
    Kontakt.scan(&fixture, &mut collector).unwrap();
    assert_eq!(collector.entries.len(), 1);
    assert_eq!(collector.entries[0].protection, ProtectionStatus::Open);
}

#[test]
fn encrypted_kontakt_preset_without_nicnt_reports_native_access_protection() {
    // Copy an encrypted real .nki (when installed) into an isolated temp dir
    // without its .nicnt to prove ProtectionStatus::Protected is reported cleanly.
    let encrypted_nki =
        Path::new("/data/main/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki");
    if !encrypted_nki.is_file() {
        return;
    }
    let root = std::env::temp_dir().join(format!("kontra-kontakt-locked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let inst_dir = root.join("Instruments");
    std::fs::create_dir_all(&inst_dir).unwrap();
    let detached = inst_dir.join("Detached.nki");
    std::fs::copy(encrypted_nki, &detached).unwrap();

    let status = inspect_protection(&detached).unwrap();
    assert!(status.is_protected(), "expected Protected, got {status:?}");
    assert_eq!(status.system(), Some(&ProtectionSystem::NativeAccess));

    let err = kontra_format_kontakt::load(&detached).unwrap_err();
    assert!(err.protection().is_some_and(ProtectionStatus::is_protected));
    let fmt_err = kontra_format::FormatError::from(err);
    assert!(
        fmt_err
            .protection()
            .is_some_and(ProtectionStatus::is_protected)
    );

    let mut collector = ScanCollector::default();
    Kontakt.scan(&root, &mut collector).unwrap();
    assert_eq!(collector.entries.len(), 1);
    assert!(collector.entries[0].protection.is_protected());

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn loose_resources_under_matches_case_insensitively_on_linux() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("kontra-res-under-ci-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Resources/Pictures")).unwrap();
    std::fs::create_dir_all(root.join("Instruments")).unwrap();
    std::fs::write(root.join("Resources/Pictures/Knob.PNG"), b"png").unwrap();
    let patch = root.join("Instruments/Patch.nki");
    std::fs::write(&patch, b"").unwrap();

    let mut res = kontra_format_kontakt::Resources::open(&patch);
    // Backslash path and different case must resolve on Linux
    assert_eq!(res.read(r"PICTURES\knob.png"), Some(b"png".to_vec()));
    assert_eq!(res.read("../Resources/Pictures/Knob.PNG"), None);

    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn scan_reports_unreadable_entry_and_keeps_scanning() {
    let root = std::env::temp_dir().join(format!("kontra-kontakt-scan-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let broken = root.join("Missing.nki");
    std::os::unix::fs::symlink("Absent.nki", &broken).unwrap();
    std::fs::write(root.join("Another.nki"), b"fixture").unwrap();

    let mut out = ScanCollector::default();
    Kontakt.scan(&root, &mut out).unwrap();
    assert!(
        out.issues
            .iter()
            .any(|(path, _, status)| path == &broken && status.is_open())
    );
    assert!(
        out.entries.len() == 1
            || out
                .issues
                .iter()
                .any(|(path, _, _)| path == &root.join("Another.nki"))
    );
    std::fs::remove_dir_all(root).unwrap();
}
