//! End-to-end authored-file inspector path. This fixture has no samples; PCM
//! continuity is covered by the product/core transaction fixtures separately.
use super::*;
use crate::support::RwLockExt;
#[path = "../../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod fixture;

#[test]
fn snapshot_actions_retain_actual_loader_owner_and_report_no_install() {
    let dir = fixture::directory();
    let base = dir.path().join("Inspector.nki");
    let a = dir.path().join("A.nksn");
    let b = dir.path().join("B.nksn");
    let source = "on init set_snapshot_type(3) make_perfview declare ui_knob $value(0,1000,1) $value := 250 make_persistent($value) end on";
    std::fs::write(
        &base,
        fixture::base_instrument("Inspector", &[Some(source), None, None, None, None]),
    )
    .unwrap();
    std::fs::write(
        &a,
        fixture::script_only_snapshot("Inspector", "", &[&["$value 800"], &[], &[], &[], &[]]),
    )
    .unwrap();
    std::fs::write(
        &b,
        fixture::script_only_snapshot("Inspector", "", &[&["$value 100"], &[], &[], &[], &[]]),
    )
    .unwrap();
    let report = switch_probe(&json!({"schema":1,"note_frames":128,"steps":[
        {"op":"load","path":base},
        {"op":"snapshot","path":a,"release_held":true},
        {"op":"snapshot","path":b,"release_held":true}
    ]}))
    .unwrap();
    assert_eq!(report["steps"][0]["outcome"], "published_and_probed");
    for step in [1, 2] {
        let record = &report["steps"][step];
        assert_eq!(record["outcome"], "snapshot_applied_same_owner", "{record}");
        assert_eq!(record["old_owner_preserved"], true);
        assert_eq!(record["control_structure_same"], true);
        assert_eq!(record["after"]["audio_generation"], 1);
        assert_eq!(record["after"]["request"], 1);
        assert_ne!(
            record["before"]["control_state_sha256"],
            record["after"]["control_state_sha256"]
        );
        assert!(record.get("publish_ms").is_none());
        assert!(record.get("load_metrics").is_none());
    }
}

#[test]
fn snapshot_action_reports_init_required_instead_of_secretly_reloading() {
    let dir = fixture::directory();
    let base = dir.path().join("Init.nki");
    let preset = dir.path().join("Saved.nksn");
    let source = "on init set_snapshot_type(0) declare ui_knob $value(0,1000,1) make_persistent($value) end on";
    std::fs::write(
        &base,
        fixture::base_instrument("Init", &[Some(source), None, None, None, None]),
    )
    .unwrap();
    std::fs::write(
        &preset,
        fixture::script_only_snapshot("Init", "", &[&["$value 800"], &[], &[], &[], &[]]),
    )
    .unwrap();
    let report=switch_probe(&json!({"schema":1,"note_frames":128,"steps":[{"op":"load","path":base},{"op":"snapshot","path":preset}]})).unwrap();
    let record = &report["steps"][1];
    assert_eq!(record["outcome"], "snapshot_reuse_refused");
    assert_eq!(record["refusal"], "init_required");
    assert_eq!(record["old_owner_preserved"], true);
    assert_eq!(
        record["before"]["control_state_sha256"],
        record["after"]["control_state_sha256"]
    );
}

#[test]
fn actual_editor_keeps_face_and_control_namespace_across_compatible_preset() {
    use crate::plugin::{
        snapshot_reuse_tests::{gui_apply, gui_fixture},
        ui_activity::Count,
    };
    let (_dir, params, mut core, preset) = gui_fixture();
    let generation = core.epoch(0);
    let mut editor = crate::ui::tests::Harness::new(&params, 1180., 780.);
    editor.ui.focus("tab-rack");
    editor.idle(4);
    let before = params.shared.ui_activity.snapshot().unwrap();
    assert!(
        before[Count::FaceNew as usize] > 0,
        "fixture actually displayed the authored face"
    );
    let prefix = format!("part-0-epoch-{generation}-script-0-ir-0");
    assert!(editor.ui.scene().unwrap().surface(&prefix).is_some());
    gui_apply(&params, &mut core, &preset);
    editor.idle(4);
    let after = params.shared.ui_activity.snapshot().unwrap();
    assert_eq!(
        after[Count::FaceNew as usize],
        before[Count::FaceNew as usize]
    );
    assert_eq!(core.epoch(0), generation);
    assert!(editor.ui.scene().unwrap().surface(&prefix).is_some());
    assert_eq!(params.selection.read_unpoisoned().parts[0].snapshot, preset);
}
