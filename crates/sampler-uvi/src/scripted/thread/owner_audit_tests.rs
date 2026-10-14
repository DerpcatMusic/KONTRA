use super::*;
use crate::script::{Command, control_id};
use std::time::{Duration, Instant};

fn xml(script: &str) -> Arc<str> {
    format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>").into()
}
fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !done() {
        assert!(Instant::now() < deadline, "bounded owner fixture timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn calls(audit: &OwnerAuditSnapshot, name: &str) -> u64 {
    audit
        .stages
        .iter()
        .find(|stage| stage.name == name)
        .unwrap()
        .calls
}
fn counter(audit: &OwnerAuditSnapshot, name: &str) -> u64 {
    audit
        .counters
        .iter()
        .find(|counter| counter.name == name)
        .unwrap()
        .value
}

#[test]
fn owner_audit_is_optional_and_preserves_callbacks_commands_full_interface_and_faults() {
    fn run(audit: Option<Arc<OwnerAudit>>) -> (Interface, Vec<Command>) {
        let script = "local k=Knob('Control',0,0,1); local label=Label('State'); k.changed=function(self) label.text='changed'; spawn(function() wait(2); label.text='settled'; postEvent{type=Event.Controller,controller=1,value=17}; error('authored fixture fault') end) end";
        let enabled = audit.is_some();
        let (mut thread, loaded) =
            ScriptThread::spawn_owner(xml(script), (), Config::default(), None, audit).unwrap();
        assert_eq!(loaded.ui.owner_audit().is_some(), enabled);
        assert!(loaded.ui.edit(control_id(1, 0), 0.75));
        thread.tick(2.);
        let mut commands = Vec::new();
        until(|| {
            thread.drain(&mut commands);
            loaded.ui.runtime_faults().0 == 1 && !commands.is_empty()
        });
        let face = loaded.ui.interface();
        assert_eq!(face.widgets[1].text, "settled");
        if let Some(audit) = loaded.ui.owner_audit() {
            assert_eq!(calls(&audit, "loading"), 1);
            assert_eq!(calls(&audit, "initial_snapshot"), 1);
            assert!(calls(&audit, "events") > 0 && calls(&audit, "ui_requests") > 0);
            assert!(calls(&audit, "advance") > 0 && calls(&audit, "advance_sample_poll") > 0);
            assert!(calls(&audit, "ui_publish") > 0 && calls(&audit, "interface_export") > 0);
            for stage in [
                "export_registry",
                "export_topology",
                "export_widgets",
                "export_page",
                "export_validate",
            ] {
                assert!(
                    calls(&audit, stage) > 0,
                    "missing coarse export stage {stage}"
                );
            }
            assert!(calls(&audit, "face_equality") > 0 && calls(&audit, "findings_publish") > 0);
            assert_eq!(counter(&audit, "commands_pushed"), commands.len() as u64);
            assert!(counter(&audit, "coroutine_resumes") >= 2);
            assert!(counter(&audit, "snapshot_widgets_total") >= 2);
            assert!(counter(&audit, "snapshot_bindings_total") >= 1);
            // No Lua values, authored code or error messages in aggregates.
            let json = serde_json::to_string(&audit).unwrap();
            assert!(!json.contains("authored fixture fault") && !json.contains("settled"));
        }
        ((*face).clone(), commands)
    }
    let ordinary = run(None);
    let observed = run(Some(Arc::new(OwnerAudit::default())));
    assert_eq!(ordinary, observed);
}

#[test]
fn owner_audit_tracks_backpressure_and_snapshots_do_not_wait_for_owner() {
    let script = "function onNote(e) for i=1,1100 do postEvent{type=Event.Controller,controller=1,value=17} end end";
    let audit = Arc::new(OwnerAudit::default());
    let (mut thread, loaded) =
        ScriptThread::spawn_owner(xml(script), (), Config::default(), None, Some(audit)).unwrap();
    thread.note_on(1, 60, 100);
    until(|| counter(&loaded.ui.owner_audit().unwrap(), "queue_full_retries") > 0);
    // The owner is sleeping/retrying the full bounded queue. Snapshot reads
    // numeric atomics directly and cannot enqueue a request behind that queue.
    let snapshot = loaded.ui.owner_audit().unwrap();
    assert_eq!(counter(&snapshot, "commands_collected"), 1100);
    assert_eq!(counter(&snapshot, "backlog_high_water"), 1100);
    let mut commands = Vec::new();
    until(|| {
        thread.drain(&mut commands);
        commands.len() == 1100
    });
    until(|| calls(&loaded.ui.owner_audit().unwrap(), "commands_push") > 0);
    let snapshot = loaded.ui.owner_audit().unwrap();
    assert_eq!(counter(&snapshot, "commands_pushed"), 1100);
    assert!(calls(&snapshot, "command_backpressure") > 0);
    assert!(calls(&snapshot, "backpressure_ui_requests") > 0);
}
