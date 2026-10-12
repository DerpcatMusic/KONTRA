//! Recovery of the dirty fix-load@039da7cd transfer regressions, with actual
//! callback and multi-chunk generation coverage. Authored sources only.
use super::*;

fn table() -> Part {
    let script = sampler_ksp::compile(
        "on init declare ui_knob $k(0,127,1) declare ui_table %table[32768](1,1,127) end on
         on ui_control($k) %table[0] := $k %table[4096] := $k %table[32767] := $k end on
         on note %table[0] := 71 %table[4096] := 72 %table[32767] := 73 end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let view = script.view();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 1).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 128, 8);
    let mut part = Part::new(
        Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("capture"),
    )
    .unwrap();
    part.prepare_persistence(&[view], "").unwrap();
    part
}

fn sweep(part: &mut Part, ingress: &mut ControlIngress) {
    for _ in 0..32 {
        ingress.refresh();
        while part.runtime.poll_control_update().unwrap().is_some() {}
        ingress.settle();
        assert!(
            ingress.captures.iter().map(Vec::capacity).sum::<usize>()
                <= sampler_core::WIDGET_EDIT_CAPACITY
        );
        if ingress.capture_cursor.is_none()
            && !ingress.capturing
            && ingress.captured_revision == ingress.widget_revision.load(Ordering::Acquire)
        {
            return;
        }
    }
    panic!("bounded readback did not finish its sweep");
}

fn edit(part: &mut Part, ingress: &mut ControlIngress, value: f64) {
    let knob = sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0, "$k").0);
    assert!(ingress.submit(knob, value));
    while part.runtime.poll_control_update().unwrap().is_some() {}
    part.runtime.render(&mut [[0.; 2]; 16]).unwrap();
    part.publish_widget_revision();
    ingress.settle();
}

fn cells(ingress: &ControlIngress) -> &[i32] {
    let table = sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0, "%table").0);
    let sampler_ui_ir::Value::Integers(values) = &ingress.widget_values[&table] else {
        panic!("table values")
    };
    values
}

#[test]
fn typed_readback_retains_at_most_one_transfer_chunk() {
    let part = table();
    let ingress = part.ui_controls.as_ref().unwrap();
    let retained: usize = ingress.captures.iter().map(Vec::capacity).sum();
    assert!(
        retained <= sampler_core::WIDGET_EDIT_CAPACITY,
        "typed readback retains {retained} Text-sized cells; its transfer must be bounded"
    );
}

#[test]
fn unchanged_typed_readback_does_not_keep_queueing_captures() {
    let mut part = table();
    let mut ingress = part.ui_controls.take().unwrap();
    sweep(&mut part, &mut ingress);
    for _ in 0..4 {
        assert!(!ingress.refresh());
        assert!(
            part.runtime.poll_control_update().unwrap().is_none(),
            "unchanged typed readback must not enqueue another audio-owner capture"
        );
    }
}

#[test]
fn typed_readback_visits_all_chunks_after_callback_changes() {
    let mut part = table();
    let mut ingress = part.ui_controls.take().unwrap();
    sweep(&mut part, &mut ingress);
    edit(&mut part, &mut ingress, 25.0);
    sweep(&mut part, &mut ingress);
    for index in [0, 4096, 32767] {
        assert_eq!(cells(&ingress)[index], 25);
    }
    ingress.refresh();
    assert!(part.runtime.poll_control_update().unwrap().is_none());
}

#[test]
fn typed_readback_repeats_sweep_when_callback_changes_earlier_chunk() {
    let mut part = table();
    let mut ingress = part.ui_controls.take().unwrap();
    sweep(&mut part, &mut ingress);
    edit(&mut part, &mut ingress, 25.0);
    ingress.refresh();
    while part.runtime.poll_control_update().unwrap().is_some() {}
    ingress.settle();
    assert_eq!(cells(&ingress)[0], 25);
    // The first chunk has been captured; a later callback changes it again.
    edit(&mut part, &mut ingress, 37.0);
    sweep(&mut part, &mut ingress);
    for index in [0, 4096, 32767] {
        assert_eq!(cells(&ingress)[index], 37);
    }
}

#[test]
fn typed_readback_observes_musical_callbacks_from_production_render() {
    let mut part = table();
    let mut ingress = part.ui_controls.take().unwrap();
    sweep(&mut part, &mut ingress);
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    core.begin_block(&BlockInfo {
        frames: 16,
        ..Default::default()
    });
    core.event(0, Event::midi1(0x90, 60, 100));
    core.render(16);
    assert_ne!(
        ingress.widget_revision.load(Ordering::Acquire),
        ingress.captured_revision
    );
    let part = core.parts[0].as_mut().unwrap();
    sweep(part, &mut ingress);
    for (index, value) in [(0, 71), (4096, 72), (32767, 73)] {
        assert_eq!(cells(&ingress)[index], value);
    }
}
