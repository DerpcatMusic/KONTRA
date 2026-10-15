//! Source-free product worker/queue/publication tests, including races between
//! actual committed snapshots and later requests. No DAW or installed library.
use super::*;
#[path = "../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod fixture;

const SOURCE: &str = r#"on init
 set_snapshot_type(3) make_perfview
 declare ui_knob $value(0,1000,1) $value := 250 make_persistent($value)
 declare ui_switch $private make_instr_persistent($private)
 declare ui_label $status(1,1)
end on
on ui_control($private) set_text($status,"private retained") end on
on persistence_changed set_text($status,"snapshot applied") end on"#;

struct Harness {
    _dir: tempfile::TempDir,
    params: SamplerParams,
    core: V2Core,
    a: String,
    b: String,
    value: sampler_ui_ir::ControlId,
    private: sampler_ui_ir::ControlId,
}
fn harness() -> Harness {
    let dir = fixture::directory();
    let base = dir.path().join("Retained.nki");
    std::fs::write(
        &base,
        fixture::base_instrument("Retained", &[Some(SOURCE), None, None, None, None]),
    )
    .unwrap();
    let a = dir.path().join("A.nksn");
    let b = dir.path().join("B.nksn");
    std::fs::write(
        &a,
        fixture::script_only_snapshot("Retained", "", &[&["$value 800"], &[], &[], &[], &[]]),
    )
    .unwrap();
    std::fs::write(
        &b,
        fixture::script_only_snapshot("Retained", "", &[&["$value 100"], &[], &[], &[], &[]]),
    )
    .unwrap();
    let seed = crate::sound::v2::snapshot_reuse::Seed::capture(&base, 48000).unwrap();
    let mut candidate = super::tests::transaction_candidate_with_source("Retained", SOURCE);
    candidate.scripts.snapshot_reuse = seed.finish(&candidate.scripts.views);
    assert!(candidate.scripts.snapshot_reuse.is_some());
    candidate.scripts.resource_path = Some(base.clone());
    candidate.scripts.resources = Some(sampler_kontakt::Resources::for_program(&base, 0).unwrap());
    let control = |name: &str| {
        let field = candidate.scripts.views[0]
            .setup()
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap();
        let sampler_ksp::model::Location::Control(id) = field.storage else {
            panic!("authored control home")
        };
        sampler_ui_ir::ControlId(id.0)
    };
    let value = control("$value");
    let private = control("$private");
    let mut params = SamplerParams::new();
    params.shared.ui_activity = Arc::new(ui_activity::Activity::new(true));
    params.selection.write_unpoisoned().parts = vec![Part {
        path: base.to_string_lossy().into(),
        ..Default::default()
    }];
    assert!(load_part(&params, 0, Some(candidate)));
    let (slot, _, part) = params.shared.ready.pop().unwrap();
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(slot, part);
    Harness {
        _dir: dir,
        params,
        core,
        a: a.to_string_lossy().into(),
        b: b.to_string_lossy().into(),
        value,
        private,
    }
}
impl Harness {
    fn queue(&self, path: &str) {
        let part = self.params.selection.read_unpoisoned().parts[0].clone();
        assert!(self.params.shared.queue_snapshot(0, &part, path.into()));
        assert!(
            prepare_snapshot(&self.params).is_none(),
            "eligible snapshot queues rather than prepares replacement"
        );
    }
    fn audio(&mut self) {
        self.core.begin_block(&BlockInfo {
            frames: 128,
            ..Default::default()
        });
        self.core.render(128);
        self.core.end_block(128, &mut |_| true);
        let epoch = self.core.epoch(0);
        self.core.take_effects(0, &mut |instance, effect| {
            self.params
                .shared
                .effects
                .push((0, epoch, instance, *effect))
                .is_ok()
        });
        service_current_ui(&self.params);
    }
    fn poll(&self) {
        snapshot_reuse::poll(&self.params);
    }
    fn selected(&self) -> String {
        self.params.selection.read_unpoisoned().parts[0]
            .snapshot
            .clone()
    }
}

#[test]
fn compatible_snapshot_keeps_runtime_ui_resources_and_ingress_then_next_tick_does_not_reload() {
    let mut h = harness();
    let atoms = h.params.shared.part(0).unwrap();
    let generation = h.core.epoch(0);
    assert!(h.params.shared.set_control_at(0, generation, h.private, 1.));
    h.audio();
    let before = h.core.snapshot_test_owner(0).unwrap();
    let (descriptor, tree, index, resources) = {
        let scripts = atoms.scripts.lock_unpoisoned();
        let view = h.params.shared.view.lock_unpoisoned();
        (
            scripts.snapshot_reuse.clone().unwrap(),
            view.parts[0].tree.clone(),
            view.parts[0].mapping_index.clone(),
            scripts.resource_path.clone(),
        )
    };
    h.queue(&h.a);
    h.audio();
    h.poll();
    assert_eq!(h.selected(), h.a);
    assert_eq!(h.core.control_value(0, h.value), Some(800.));
    assert_eq!(h.core.control_value(0, h.private), Some(1.));
    assert_eq!(h.core.snapshot_test_owner(0), Some(before));
    assert_eq!(h.core.epoch(0), generation);
    assert!(Arc::ptr_eq(
        &descriptor,
        atoms
            .scripts
            .lock_unpoisoned()
            .snapshot_reuse
            .as_ref()
            .unwrap()
    ));
    assert_eq!(atoms.scripts.lock_unpoisoned().resource_path, resources);
    let view = h.params.shared.view.lock_unpoisoned();
    assert_eq!(view.parts[0].tree, tree);
    if let (Some(old), Some(new)) = (&index, &view.parts[0].mapping_index) {
        assert!(Arc::ptr_eq(old, new));
    }
    assert_eq!(
        view.parts[0].trace.as_ref().unwrap()["details"]["snapshot_apply"],
        "retained_owner"
    );
    drop(view);
    assert!(!load_part(&h.params, 0, None));
    assert!(h.params.shared.ready.pop().is_none());
    // Same ingress accepts the next gesture, with its coherent current revision.
    assert!(h.params.shared.set_control_at(0, generation, h.private, 0.));
    h.audio();
    assert_eq!(h.core.control_value(0, h.private), Some(0.));
    h.core.play(0, CoreEvent::midi1(0x90, 60, 100));
    assert!(h.core.render(128).buses[0][0].iter().any(|v| *v != 0.));
}

#[test]
fn later_request_cancels_queued_older_snapshot_without_full_reload() {
    let mut h = harness();
    let before = h.core.snapshot_test_owner(0);
    h.queue(&h.a);
    h.queue(&h.b);
    h.audio();
    h.poll();
    assert_eq!(h.selected(), h.b);
    assert_eq!(h.core.control_value(0, h.value), Some(100.));
    assert_eq!(h.core.snapshot_test_owner(0), before);
    assert!(h.params.shared.snapshot_request.lock_unpoisoned().is_none());
}

#[test]
fn committed_older_receipt_does_not_invalidate_newer_same_base_request() {
    let mut h = harness();
    h.queue(&h.a);
    h.audio();
    // A really committed, but B is requested before the worker receives A.
    h.queue(&h.b);
    h.poll();
    assert_eq!(h.selected(), h.a);
    h.audio();
    h.poll();
    assert_eq!(h.selected(), h.b);
    assert_eq!(h.core.control_value(0, h.value), Some(100.));
    assert!(h.params.shared.snapshot_request.lock_unpoisoned().is_none());
}

#[test]
fn host_recall_cancels_overlay_before_native_apply_and_cannot_publish_its_metadata() {
    let mut h = harness();
    h.queue(&h.a);
    h.params.reload_ui_controls();
    h.audio();
    h.poll();
    assert_eq!(h.core.control_value(0, h.value), Some(250.));
    assert!(h.selected().is_empty());
    assert!(h.params.shared.snapshot_request.lock_unpoisoned().is_none());
}

#[test]
fn exhausted_sequence_reserves_a_final_host_recall_cancellation_epoch() {
    let mut h = harness();
    let atoms = h.params.shared.part(0).unwrap();
    atoms.snapshot_sequence.store(u64::MAX - 2, Ordering::Release);
    h.queue(&h.a);
    assert_eq!(atoms.snapshot_sequence.load(Ordering::Acquire), u64::MAX - 1);
    h.params.reload_ui_controls();
    assert_eq!(atoms.snapshot_sequence.load(Ordering::Acquire), u64::MAX);
    h.audio();
    h.poll();
    assert_eq!(h.core.control_value(0, h.value), Some(250.));
    assert!(h.selected().is_empty());
    let part = h.params.selection.read_unpoisoned().parts[0].clone();
    assert!(!h.params.shared.queue_snapshot(0, &part, h.b.clone()));
    assert!(h.params.shared.snapshot_request.lock_unpoisoned().is_none());
}

#[test]
fn active_notes_refuse_purely_then_queue_explicit_full_load_fallback() {
    let mut h = harness();
    h.core.play(0, CoreEvent::midi1(0x90, 60, 100));
    h.audio();
    let before = h.core.snapshot_test_owner(0);
    h.queue(&h.a);
    h.audio();
    h.poll();
    assert_eq!(h.core.control_value(0, h.value), Some(250.));
    assert_eq!(h.core.snapshot_test_owner(0), before);
    assert!(h.selected().is_empty());
    let fallback = h.params.shared.snapshot_request.lock_unpoisoned();
    assert!(!fallback.as_ref().unwrap().allow_reuse);
    assert_eq!(fallback.as_ref().unwrap().path, h.a);
}

#[test]
fn stopped_audio_coalesces_gestures_with_at_most_two_owned_payloads() {
    let mut h = harness();
    h.queue(&h.a);
    h.queue(&h.b);
    h.queue(&h.a);
    h.queue(&h.b);
    let atoms = h.params.shared.part(0).unwrap();
    assert_eq!(atoms.snapshot_pending.lock_unpoisoned().len(), 2);
    assert_eq!(
        h.params
            .shared
            .snapshot_request
            .lock_unpoisoned()
            .as_ref()
            .unwrap()
            .path,
        h.b
    );
    h.audio();
    h.poll();
    assert!(h.selected().is_empty());
    assert!(atoms.snapshot_pending.lock_unpoisoned().is_empty());
    assert!(prepare_snapshot(&h.params).is_none());
    assert_eq!(atoms.snapshot_pending.lock_unpoisoned().len(), 1);
    h.audio();
    h.poll();
    assert_eq!(h.selected(), h.b);
    assert_eq!(h.core.control_value(0, h.value), Some(100.));
}

/// Authored fixture shared only with the real editor regression.
pub(crate) fn gui_fixture() -> (tempfile::TempDir, Arc<SamplerParams>, V2Core, String) {
    let Harness {
        _dir,
        params,
        core,
        a,
        ..
    } = harness();
    (_dir, Arc::new(params), core, a)
}

pub(crate) fn gui_apply(params: &SamplerParams, core: &mut V2Core, path: &str) {
    let part = params.selection.read_unpoisoned().parts[0].clone();
    assert!(params.shared.queue_snapshot(0, &part, path.into()));
    assert!(prepare_snapshot(params).is_none());
    core.begin_block(&BlockInfo {
        frames: 128,
        ..Default::default()
    });
    core.render(128);
    core.end_block(128, &mut |_| true);
    let epoch = core.epoch(0);
    core.take_effects(0, &mut |instance, effect| {
        params
            .shared
            .effects
            .push((0, epoch, instance, *effect))
            .is_ok()
    });
    service_current_ui(params);
    snapshot_reuse::poll(params);
}

#[test]
fn retained_host_filter_history_refuses_before_snapshot_values_can_change() {
    let mut h = harness();
    h.core.snapshot_test_tone_history(0, 0.25);
    h.queue(&h.a);
    h.audio();
    h.poll();
    assert_eq!(h.core.control_value(0, h.value), Some(250.));
    assert!(h.selected().is_empty());
    assert!(
        !h.params
            .shared
            .snapshot_request
            .lock_unpoisoned()
            .as_ref()
            .unwrap()
            .allow_reuse
    );
}
