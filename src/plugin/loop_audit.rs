//! Retained UI-loop admission and source-generation regression witnesses.
use super::*;

impl ControlCell {
    pub(crate) fn loop_audit_new(id: sampler_ui_ir::ControlId, value: f64) -> Self {
        Self {
            id,
            value: AtomicU64::new(value.to_bits()),
        }
    }
}

fn ingress() -> (Shared, crate::sound::v2::Part, sampler_ui_ir::ControlId) {
    let script = sampler_ksp::compile(
        "on init declare ui_knob $k(0,100,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let id = sampler_ui_ir::ControlId(script.controls()[0].definition.id.0);
    let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap();
    let plan = script.bind(prepared).unwrap();
    let limits = sampler_core::Limits::for_plan(&plan, 8, 0);
    let mut runtime = crate::sound::v2::Part::new(
        sampler_core::Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("test"),
    )
    .unwrap();
    let shared = Shared::default();
    shared.ensure_parts(1);
    let part = shared.part(0).unwrap();
    part.generation.store(1, Ordering::Release);
    *part.controls.lock().unwrap() = vec![ControlCell::loop_audit_new(id, 0.)].into();
    *part.ingress.lock().unwrap() = runtime.ui_controls.take();
    (shared, runtime, id)
}

#[test]
fn loop_audit_full_queue_and_nonfinite_values_publish_unadmitted_state() {
    let (shared, _runtime, id) = ingress();
    let part = shared.part(0).unwrap();
    for _ in 0..256 {
        assert!(shared.set_control(0, id, 0.));
    }
    assert!(!shared.set_control(0, id, 99.));
    assert_eq!(
        part.control_values(),
        [(id, 0.)],
        "queue rejection never changes authoritative state"
    );
    assert!(!shared.set_control(0, id, f64::NAN));
    assert!(part.control_values()[0].1.is_finite());
}

#[test]
fn loop_audit_pending_edit_has_no_source_generation() {
    let (shared, _runtime, id) = ingress();
    let part = shared.part(0).unwrap();
    assert!(shared.set_control_at(0, 1, id, 25.));
    part.generation.store(2, Ordering::Release);
    *part.ingress.lock().unwrap() = None;
    assert!(
        !shared.set_control_at(0, 1, id, 25.),
        "stale face cannot submit into a replacement"
    );
    assert_eq!(part.display_values(), [(id, 0.)]);
}

#[test]
fn publication_is_sparse_and_identical_updates_are_noops() {
    let script = sampler_ksp::compile(
        "on init declare ui_knob $k(0,100,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let face = script.ui(&|_| None).unwrap();
    let mut part = PartView {
        interfaces: vec![face.clone()].into(),
        ..Default::default()
    };
    let base = part.interfaces.clone();
    assert!(!part.publish_interface(&face));
    let mut changed = face.clone();
    changed.widgets[0].value_text = Some("updated".into());
    assert!(part.publish_interface(&changed));
    let updates = part.updates.clone();
    assert_eq!(part.ui_revision, 1);
    assert_eq!(part.updates[0].widgets.len(), 1);
    assert!(!part.publish_interface(&changed));
    assert!(Arc::ptr_eq(&updates, &part.updates));
    assert!(Arc::ptr_eq(&base, &part.interfaces));
    assert_eq!(part.ui_revision, 1);
    assert!(part.publish_interface(&face));
    assert_eq!(part.updates[0], Default::default());
}

#[test]
fn stale_epoch_effect_replay_cannot_mutate_the_new_source() {
    let script = sampler_ksp::compile("on init declare ui_knob $k(0,100,1) end on on ui_control($k) set_knob_label($k,\"changed\") end on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let view = script.view();
    let service = (0..1024)
        .find(|&n| view.service(n) == Some("set_knob_label"))
        .unwrap();
    let authored = script.ui(&|_| None).unwrap();
    let ui_id = script.model().interface.widgets[0].ui_id;
    let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap();
    let limits = sampler_core::Limits::for_plan(&prepared, 8, 0);
    let rt = sampler_core::Runtime::new(prepared, limits).unwrap();
    let shared = Shared::default();
    shared.ensure_parts(1);
    let part = shared.part(0).unwrap();
    part.generation.store(2, Ordering::Release);
    part.scripts.lock().unwrap().views.push(view);
    let keys = part.scripts.lock().unwrap().keys();
    shared.view.lock().unwrap().parts[0] = PartView {
        generation: 2,
        keys,
        interfaces: vec![authored].into(),
        ..Default::default()
    };
    let mut effect = sampler_core::Effect {
        plan: rt.active_plan(),
        instance: None,
        service,
        args: [0; sampler_core::EFFECT_ARGS],
        count: 1,
        text: Some(sampler_core::Text::new("changed")),
    };
    effect.args[0] = i64::from(ui_id);
    shared.effects.push((0, 1, 0, effect)).unwrap();
    shared.apply_effects();
    assert_eq!(shared.view.lock().unwrap().parts[0].ui_revision, 0);
    shared.effects.push((0, 2, 0, effect)).unwrap();
    shared.apply_effects();
    let published = shared.view.lock().unwrap();
    assert_eq!(published.parts[0].ui_revision, 1);
    assert_eq!(
        published.parts[0].updates[0].widgets[0]
            .1
            .value_text
            .as_deref(),
        Some("changed")
    );
}
