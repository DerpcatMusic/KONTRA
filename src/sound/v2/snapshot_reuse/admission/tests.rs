//! Authored bytes exercise the real Descriptor path without installed content.
use super::super::{Descriptor, Seed, fixture};
use super::*;
use std::{path::PathBuf, sync::Arc};

const NAME: &str = "Admission Authority";
const SAVED: &str = "on init set_snapshot_type(3) declare $saved make_persistent($saved) end on";

fn files(
    source: &str,
    snapshot: Vec<u8>,
) -> (
    tempfile::TempDir,
    Arc<Descriptor>,
    Vec<sampler_ksp::ScriptView>,
    PathBuf,
) {
    let directory = fixture::directory();
    let base = directory.path().join("Admission.nki");
    let path = directory.path().join("Incoming.nksn");
    std::fs::write(
        &base,
        fixture::base_instrument(NAME, &[Some(source), None, None, None, None]),
    )
    .unwrap();
    std::fs::write(&path, snapshot).unwrap();
    let views = vec![
        sampler_ksp::compile(source, 48_000, sampler_ksp::Limits::LIBRARY, &[])
            .unwrap()
            .view(),
    ];
    let descriptor = Seed::capture(&base, 48_000)
        .unwrap()
        .finish(&views)
        .unwrap();
    (directory, descriptor, views, path)
}
fn script_snapshot(rows: &[&str]) -> Vec<u8> {
    fixture::script_only_snapshot(NAME, "", &[rows, &[], &[], &[], &[]])
}

#[test]
fn policy_refusal_distinguishes_unread_payload_and_uninstrumented_work_from_zero_calls() {
    for mode in [0, 2] {
        let source = format!(
            "on init set_snapshot_type({mode}) declare $saved make_persistent($saved) end on"
        );
        let (_directory, descriptor, views, path) = files(&source, script_snapshot(&["$saved 9"]));
        // A missing candidate is intentional: policy refuses before any read.
        std::fs::remove_file(&path).unwrap();
        let (result, report) = descriptor.prepare_reported(&path, 48_000, &views);
        assert!(matches!(result, Err(Refusal::InitRequired)));
        let report = report.json();
        assert_eq!(report["stage"], "policy");
        assert_eq!(report["refusal"], "init_required");
        assert_eq!(report["snapshot_policy"]["native_mode"], mode);
        assert_eq!(report["snapshot_policy"]["reruns_init"], true);
        assert!(report["incoming"].is_null());
        assert!(report["prepared"].is_null());
        assert_eq!(report["calls"]["snapshot_read_attempts"], 0);
        assert_eq!(report["calls"]["resource_authority_checks"], 0);
        assert_eq!(report["unmeasured"]["file_io"], "not_instrumented");
        assert_eq!(
            report["unmeasured"]["script_init_execution"],
            "not_instrumented"
        );
        assert!(report["audio_committed"].is_null());
    }
}

fn object(version: u16, public: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(version.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((public.len() as u32).to_le_bytes());
    bytes.extend(public);
    bytes.extend(0u32.to_le_bytes());
    bytes
}

/// Independent v1 native framing: zero groups, two empty fixed racks, sixteen
/// empty buses, then five physical saved slots. Native chunks remain present.
fn native_snapshot() -> Vec<u8> {
    let mut public = fixture::chunk(0x33, &0u32.to_le_bytes());
    for _ in 0..2 {
        public.extend([0, 0x12, 0]);
        public.extend([0; 8]);
    }
    for _ in 0..16 {
        public.extend([1, 0x11, 0]);
        public.extend([0; 12]);
    }
    for slot in 0..5 {
        let rows: &[&str] = if slot == 0 { &["$saved 9"] } else { &[] };
        public.extend((rows.len() as u32).to_le_bytes());
        for row in rows {
            public.extend((row.len() as u32).to_le_bytes());
            public.extend(row.as_bytes());
        }
    }
    let mut payload = fixture::chunk(0x4f, &object(1, &public));
    let mut metadata = 0u32.to_le_bytes().to_vec();
    for text in [NAME, ""] {
        metadata.extend((text.encode_utf16().count() as u32).to_le_bytes());
        metadata.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    }
    payload.extend(fixture::chunk(0x51, &object(1, &metadata)));
    fixture::container(&payload)
}

#[test]
fn native_refusal_records_decoded_domains_without_decoding_saved_rows_again() {
    let (_directory, descriptor, views, path) = files(SAVED, native_snapshot());
    let (result, report) = descriptor.prepare_reported(&path, 48_000, &views);
    assert!(matches!(result, Err(Refusal::NativeState)));
    let report = report.json();
    assert_eq!(report["stage"], "native_payload");
    assert_eq!(report["incoming"]["groups"], 0);
    assert_eq!(report["incoming"]["effect_chunks"], 18);
    assert_eq!(report["incoming"]["effect_chunk_bytes"], 2 * 11 + 16 * 15);
    assert_eq!(report["incoming"]["saved_slots"], 5);
    assert_eq!(report["incoming"]["saved_rows"], 1);
    assert_eq!(report["calls"]["snapshot_read_attempts"], 1);
    assert_eq!(report["calls"]["resource_authority_checks"], 1);
    assert_eq!(report["calls"]["saved_decode_attempts"], 0);
    assert_eq!(report["calls"]["saved_schema_attempts"], 0);
    assert!(report["prepared"].is_null());
}

#[test]
fn saved_decode_schema_and_menu_refusals_keep_the_exact_observed_stage() {
    let cases: &[(&str, &[&str], Refusal, &str, usize)] = &[
        (
            SAVED,
            &["$saved broken"],
            Refusal::SavedState,
            "saved_decode",
            0,
        ),
        (
            "on init set_snapshot_type(3) declare %saved[2] make_persistent(%saved) end on",
            &["%saved 9"],
            Refusal::SavedState,
            "saved_schema",
            1,
        ),
        (
            "on init set_snapshot_type(3) declare ui_menu $saved add_menu_item($saved,\"A\",20) add_menu_item($saved,\"B\",80) make_persistent($saved) end on",
            &["$saved 1"],
            Refusal::MenuRosterUnverified,
            "saved_schema",
            1,
        ),
    ];
    for (source, rows, reason, stage, schema_attempts) in cases {
        let (_directory, descriptor, views, path) = files(source, script_snapshot(rows));
        let (result, report) = descriptor.prepare_reported(&path, 48_000, &views);
        assert!(matches!(result, Err(actual) if actual == *reason));
        let report = report.json();
        assert_eq!(report["stage"], *stage);
        assert_eq!(report["refusal"], serde_json::to_value(reason).unwrap());
        assert_eq!(report["incoming"]["saved_rows"], 1);
        assert_eq!(report["calls"]["snapshot_read_attempts"], 1);
        assert_eq!(report["calls"]["saved_decode_attempts"], 1);
        assert_eq!(report["calls"]["saved_schema_attempts"], *schema_attempts);
        assert_eq!(report["calls"]["resource_authority_checks"], 1);
        assert!(report["prepared"].is_null());
        if *stage == "saved_schema" {
            assert_eq!(report["saved_refusal"]["category"], "row");
            assert_eq!(report["saved_refusal"]["slot"], 0);
            assert_eq!(
                report["saved_refusal"]["row_reason"],
                if *reason == Refusal::MenuRosterUnverified {
                    "menu_roster_unverified"
                } else {
                    "count"
                }
            );
        } else {
            assert!(report["saved_refusal"].is_null());
        }
    }
}

#[test]
fn successful_preparation_and_audio_receipt_are_distinct_from_callback_completion() {
    let source = "on init set_snapshot_type(3) declare $saved make_persistent($saved) declare $calls end on on persistence_changed inc($calls) end on";
    let (_directory, descriptor, views, path) = files(source, script_snapshot(&["$saved 9"]));
    let (result, mut report) = descriptor.prepare_reported(&path, 48_000, &views);
    assert_eq!(result.unwrap().state.values.len(), 1);
    let prepared = report.json();
    assert_eq!(prepared["stage"], "prepared");
    assert_eq!(prepared["outcome"], "prepared");
    assert_eq!(prepared["calls"]["resource_authority_checks"], 2);
    assert_eq!(prepared["calls"]["snapshot_read_attempts"], 1);
    assert_eq!(prepared["prepared"]["script_value_homes"], 1);
    assert_eq!(prepared["prepared"]["persistence_callbacks"], 1);
    assert!(prepared["audio_committed"].is_null());
    // Unit-test report transitions; actual queued transaction semantics are
    // covered independently by plugin/snapshot_reuse_tests and the core suite.
    report.queued();
    assert_eq!(report.json()["outcome"], "queued");
    report.audio(true, None);
    let admitted = report.json();
    assert_eq!(admitted["stage"], "audio_admission");
    assert_eq!(admitted["audio_committed"], true);
    assert_eq!(
        admitted["unmeasured"]["callback_completion"],
        "not_observed"
    );
    report.audio(
        false,
        Some(sampler_core::SnapshotOverlayRefusal::ActiveEvents),
    );
    assert_eq!(report.json()["audio_refusal"], "active_events");
    assert_eq!(report.json()["audio_committed"], false);
    report.unrecognized_receipt();
    assert!(report.json()["audio_committed"].is_null());
    assert!(report.json()["audio_refusal"].is_null());
}

#[test]
fn lost_worker_evidence_is_unknown_instead_of_a_fabricated_zero_read_count() {
    let mut report = Report::new(Some(SnapshotMode::ScriptWithoutInit));
    report.worker_lost(Refusal::Worker);
    let report = report.json();
    assert_eq!(report["stage"], "worker");
    assert_eq!(report["refusal"], "worker");
    assert!(report["calls"].is_null());
    assert!(report["incoming"].is_null());
}

#[cfg(feature = "plugin")]
#[test]
fn actual_plugin_trace_publishes_the_audio_receipt_and_keeps_unmeasured_work_explicit() {
    use crate::{
        plugin::snapshot_reuse_tests::{gui_apply, gui_fixture},
        support::MutexExt,
    };
    let (_directory, params, mut core, path) = gui_fixture();
    gui_apply(&params, &mut core, &path);
    let view = params.shared.view.lock_unpoisoned();
    let trace = view.parts[0].trace.as_ref().unwrap();
    assert_eq!(trace["details"]["snapshot_apply"], "retained_owner");
    let report = &trace["details"]["snapshot_reuse_admission"];
    assert_eq!(report["stage"], "audio_admission");
    assert_eq!(report["outcome"], "committed");
    assert_eq!(report["audio_committed"], true);
    assert!(report["audio_refusal"].is_null());
    assert_eq!(report["calls"]["snapshot_read_attempts"], 1);
    assert_eq!(report["calls"]["resource_authority_checks"], 2);
    assert_eq!(report["prepared"]["script_value_homes"], 1);
    assert_eq!(
        report["unmeasured"]["runtime_owner_creation"],
        "not_instrumented"
    );
    assert_eq!(report["unmeasured"]["ui_rebuild"], "not_instrumented");
    assert_eq!(report["unmeasured"]["callback_completion"], "not_observed");
}
