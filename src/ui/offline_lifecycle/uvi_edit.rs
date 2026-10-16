//! Exact current-owner Lua edits, separate from Kontakt's ControlIngress.
use super::*;

pub(super) fn valid(control: &str, value: f64, timeout_ms: u16) -> bool {
    value.is_finite()
        && (1..=10000).contains(&timeout_ms)
        && switch_control_id(control).is_some_and(|id| id.0 >> 64 == 0x5556490000000000)
}
pub(super) fn run(
    core: &mut V2Core,
    owner: &mut SwitchOwner,
    generation: u64,
    control: &str,
    value: f64,
    timeout_ms: u16,
) -> Value {
    let mut report = json!({"generation":generation,"active_generation":core.epoch(0),
        "control":control,"requested_value":value,"timeout_ms":timeout_ms,"queued":false,
        "execution_observation":"unverified","owner_acknowledged":false,"ordered_drained":false,
        "scope":"Exact epoch/Arc-validated UiBridge edit followed by ordinary FIFO owner acknowledgement and ordered command drain; no KSP ingress, per-edit execution counter, callback-success or native-equivalence claim"});
    let refuse = |mut report: Value, reason| {
        report["status"] = json!(reason);
        report
    };
    if !valid(control, value, timeout_ms) {
        return refuse(report, "invalid_input");
    }
    if generation != owner.request {
        return refuse(report, "stale_generation");
    }
    let Some(bridge) = owner.scripts.uvi.clone() else {
        return refuse(report, "owner_unavailable");
    };
    if let Err(error) = core.uvi_diagnostic_progress(0, generation, &bridge) {
        report["owner_error"] = json!(format!("{error:?}"));
        return refuse(report, "owner_unavailable");
    }
    let id = switch_control_id(control).unwrap();
    let Some(before_value) = bridge.value(id) else {
        return refuse(report, "unknown_control");
    };
    if let Err(error) = core.enable_uvi_diagnostics(0, generation, &bridge) {
        report["owner_error"] = json!(format!("{error:?}"));
        return refuse(report, "owner_unavailable");
    }
    let before_revision = bridge.revision();
    let before_faults = bridge.runtime_faults();
    report["value_before"] = json!(before_value);
    report["revision_before"] = json!(before_revision);
    report["runtime_faults_before"] = json!(before_faults);
    // Validate immediately before mutation. V2Core and SwitchOwner are borrowed
    // for this action; no publication can swap the owner between this and edit.
    if let Err(error) = core.uvi_diagnostic_progress(0, generation, &bridge) {
        report["owner_error"] = json!(format!("{error:?}"));
        return refuse(report, "owner_unavailable");
    }
    let queued = bridge.edit(id, value);
    report["queued"] = json!(queued);
    if !queued {
        return refuse(report, "queue_refused");
    }
    let ack = super::uvi::acknowledge_ui_edit(core, owner, &bridge, timeout_ms);
    let observed = ack["ordered_drained"] == true;
    report["owner_acknowledged"] = ack["owner_acknowledged"].clone();
    report["ordered_drained"] = json!(observed);
    report["acknowledgement"] = ack;
    report["execution_observation"] = json!(if observed {
        "ordered_owner_attempt_observed"
    } else {
        "unverified"
    });
    report["status"] = json!(if observed {
        "acknowledged_and_drained"
    } else {
        "queued_unverified"
    });
    report["value_after"] = json!(bridge.value(id));
    report["revision_after"] = json!(bridge.revision());
    report["runtime_faults_after"] = json!(bridge.runtime_faults());
    report["new_runtime_faults_observed"] = json!(bridge.runtime_faults() != before_faults);
    report
}

#[cfg(all(test, feature = "shots"))]
mod tests {
    use super::*;
    use sampler_core::{Limits, Prepared, Runtime};
    use sampler_uvi::scripted::{Driver, ScriptThread};
    #[test]
    fn uvi_edit_fences_stale_invalid_and_foreign_owners_before_real_callback_and_provider() {
        let xml = include_str!("../../../crates/sampler-uvi/tests/fixtures/wave_view_request.uvip");
        let (thread, loaded) = ScriptThread::spawn(xml.into(), (), Default::default()).unwrap();
        let bridge = loaded.ui;
        let prepared = Prepared::new(48000, vec![], vec![], 1).unwrap();
        let limits = Limits::for_plan(&prepared, 8, 8);
        let mut part = Part::new(
            Runtime::new(prepared, limits).unwrap(),
            crate::sound::tree::MixTree::instrument("authored Lua edit"),
        )
        .unwrap();
        part.set_uvi_inspector_fixture_script(Driver::new(thread, vec![], 48000));
        part.epoch = 7;
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(part)));
        let mut owner = SwitchOwner {
            request: 7,
            ..Default::default()
        };
        owner.scripts.uvi = Some(bridge.clone());
        let id = sampler_uvi::script::control_id(1, 0);
        let control = format!("0x{:032x}", id.0);
        for (generation, target, value, status) in [
            (6, control.clone(), 2., "stale_generation"),
            (
                7,
                "0x00000000000000000000000000000000".into(),
                2.,
                "invalid_input",
            ),
            (7, control.clone(), f64::NAN, "invalid_input"),
        ] {
            let report = run(&mut core, &mut owner, generation, &target, value, 1000);
            assert_eq!(report["queued"], false);
            assert_eq!(report["status"], status);
            assert_eq!(bridge.value(id), Some(1.));
        }
        let (_foreign_thread, foreign) =
            ScriptThread::spawn(xml.into(), (), Default::default()).unwrap();
        owner.scripts.uvi = Some(foreign.ui.clone());
        assert_eq!(
            run(&mut core, &mut owner, 7, &control, 2., 1000)["queued"],
            false
        );
        assert_eq!(foreign.ui.value(id), Some(1.));
        owner.scripts.uvi = Some(bridge.clone());
        let report = run(&mut core, &mut owner, 7, &control, 2., 3000);
        assert_eq!(report["queued"], true);
        assert_eq!(report["owner_acknowledged"], true);
        assert_eq!(report["ordered_drained"], true);
        assert_eq!(
            report["execution_observation"],
            "ordered_owner_attempt_observed"
        );
        assert_eq!(bridge.value(id), Some(2.));
        let face = bridge.interface();
        assert!(
            matches!(&face.widgets[1].uvi_wave_view_request.as_ref().unwrap().sample,
            sampler_ui_ir::UviWaveViewSample::Manual(path) if path == "wave-a.wav")
        );
        // Real display IO, using the same authored sample authority as the UI.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("authored.uvip");
        std::fs::write(&path, xml).unwrap();
        let mut bytes = b"RIFF".to_vec();
        bytes.extend(36u32.wrapping_add(8192).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(8000u32.to_le_bytes());
        bytes.extend(16000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(8192u32.to_le_bytes());
        for n in 0..4096 {
            bytes.extend((if n % 2 == 0 { 4096i16 } else { -4096i16 }).to_le_bytes());
        }
        std::fs::write(dir.path().join("wave-a.wav"), bytes).unwrap();
        let mut assets = crate::ui::ir_view::Assets::default();
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            assets.prepare(
                &path,
                &face,
                sampler_ui_ir::PageRef(0),
                sampler_ui_ir::Presentation::Bitmap,
                1.,
                &Default::default(),
            );
            let observed = assets.wave_view_observations(&face);
            if observed["rows"][0]["status"] == "ready" {
                assert_eq!(observed["rows"][0]["frames"], 4096);
                assert_eq!(observed["requested_resolved_envelopes"], json!([1, 1, 1]));
                break;
            }
            assert!(
                Instant::now() < end,
                "actual inspector edit did not reach the real display provider"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
