//! Baseline failure witnesses, not regression expectations for the fixed loop.
use super::*;

impl ControlCell {
    pub(crate) fn loop_audit_new(id: sampler_ui_ir::ControlId, value: f64) -> Self {
        Self {
            id,
            value: AtomicU64::new(value.to_bits()),
        }
    }
}

#[test]
fn loop_audit_full_queue_and_nonfinite_values_publish_unadmitted_state() {
    let shared = Shared::default();
    let part = shared.part(0).unwrap();
    let id = sampler_ui_ir::ControlId(42);
    *part.controls.lock().unwrap() = vec![ControlCell {
        id,
        value: AtomicU64::new(0f64.to_bits()),
    }]
    .into();
    while shared.control_edits.push((0, id, 0.)).is_ok() {}
    assert!(!shared.set_control(0, id, 99.));
    assert_eq!(
        part.control_values(),
        [(id, 99.)],
        "queue rejection occurs after optimistic mutation"
    );
    shared.control_edits.pop();
    assert!(
        shared.set_control(0, id, f64::NAN),
        "the production bridge has no finite-value guard"
    );
    assert!(part.control_values()[0].1.is_nan());
}

#[test]
fn loop_audit_pending_edit_has_no_source_generation() {
    let shared = Shared::default();
    let part = shared.part(0).unwrap();
    part.generation.store(1, Ordering::Release);
    let id = sampler_ui_ir::ControlId(42);
    assert!(shared.set_control(0, id, 25.));
    part.generation.store(2, Ordering::Release);
    assert_eq!(
        shared.control_edits.pop(),
        Some((0, id, 25.)),
        "replacement cannot identify stale ingress"
    );
}
