//! Public compiler/binder/Runtime and headless mirror requirements. All NOT_RUN.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;
use sampler_core::waveform::{Property, attachment_key};
use sampler_ksp::{Environment, Limits as KspLimits, Script, ScriptView};

fn environment(slot: u8) -> Environment {
    Environment { slot, zones: [(27, [0, 0, 127]), (91, [0, 0, 127])].into(), ..Default::default() }
}
fn script(source: &str, slot: u8) -> Script {
    sampler_ksp::compile_with(source, 48000, KspLimits::LIBRARY, &[], &environment(slot)).unwrap()
}
fn prepared(scripts: Vec<Script>) -> Prepared {
    sampler_ksp::bind_modules(scripts, Prepared::new(48000, vec![], vec![], 1).unwrap()).unwrap()
}
fn limits(plan: &Prepared) -> Limits {
    let mut limits = Limits::for_plan(plan, 16, 16);
    limits.behaviors = 16;
    limits.behavior_cells = plan.behavior_local_count() * 16;
    limits
}
fn runtime(source: &str) -> (Runtime, ScriptView) {
    let script = script(source, 0);
    let view = script.view();
    let plan = prepared(vec![script]);
    let limits = limits(&plan);
    (Runtime::new(plan, limits).unwrap(), view)
}
fn context(rt: &Runtime) -> ControlContext {
    ControlContext { performance: rt.performance(0).unwrap(), origin: ChannelAddress {
        protocol: Protocol::Native, port: 0, group: 0, channel: 0,
    }, channels: 1 }
}
fn invoke(rt: &mut Runtime, plan: PlanId, slot: u8, name: &str) {
    invoke_id(rt, plan, sampler_ksp::derived_control_id(slot, name));
}
fn invoke_id(rt: &mut Runtime, plan: PlanId, id: ControlId) {
    rt.invoke_control(context(rt), plan, None, ControlWrite {
        id, value: ControlValue::Integer(1),
    }).unwrap();
    rt.flush_behaviors(|_, _, _| true);
}
fn trigger_once(rt: &mut Runtime, key: u8) {
    let note = rt.trigger(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0,
        key, external_id: Some(1) }, key, 1.).unwrap();
    rt.release(note).unwrap();
    rt.flush_behaviors(|_, _, _| true);
    rt.flush_ended(|_| true);
}
fn wave(view: &ScriptView, name: &str) -> sampler_ui_ir::Waveform {
    let model = view.ui(&|_| None).unwrap();
    model.widgets.iter().find(|w| w.name == name).unwrap().waveform.clone().unwrap()
}
fn ui(view: &ScriptView, name: &str) -> i32 {
    view.model().interface.widgets.iter().find(|w| w.name == name).unwrap().ui_id
}
fn stored(rt: &Runtime, plan: PlanId, instance: u16, id: i32, p: Property, index: i32) -> Option<i64> {
    rt.script_store(plan, ScriptInstanceId(instance), p.key(id, index)).unwrap()
}
const INIT: &str = "on init declare ui_waveform $w(1,1) declare ui_button $apply
    attach_zone($w,27,3) set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,12000)
    set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,77) end on";

#[test]
fn waveform_runtime_state_and_projection_roundtrip_without_audio_heap() {
    let source = format!("{INIT} on ui_control($apply)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,24000)
        set_ui_wf_property($w,$UI_WF_PROP_FLAGS,0,11)
        set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,99)
        set_ui_wf_property($w,$UI_WF_PROP_MIDI_DRAG_START_NOTE,0,65) end on");
    let (mut rt, mut view) = runtime(&source);
    let plan = rt.active_plan();
    let id = ui(&view, "$w");
    let apply = sampler_ksp::derived_control_id(0, "$apply");
    support::without_heap(|| {
        invoke_id(&mut rt, plan, apply);
        assert_eq!(stored(&rt, plan, 0, id, Property::Cursor, 0), Some(24000));
        assert_eq!(stored(&rt, plan, 0, id, Property::Table, 3), Some(99));
        assert_eq!(stored(&rt, plan, 0, id, Property::Table, 77), None);
        assert!(rt.take_fault().is_none());
    });
    // State is authoritative before a UI exists/drains; projection is off audio.
    assert_eq!(wave(&view, "$w").cursor_us, 12000);
    rt.drain_effects(|e| { assert!(view.apply_ui_effect_for(plan, ScriptInstanceId(0), e)); true });
    assert_eq!(wave(&view, "$w"), sampler_ui_ir::Waveform {
        zone: 27, flags: 11, cursor_us: 24000, table: vec![0,0,0,99], highlighted: None, midi_start_note: 65,
    });
}

#[test]
fn waveform_runtime_dynamic_symbol_and_random_operands_evaluate_once() {
    let prefix = "on init declare $index declare $value declare $result
        declare $next declare $p := $UI_WF_PROP_TABLE_VAL
        declare ui_waveform $w(1,1) declare ui_button $apply attach_zone($w,27,3) end on";
    let explicit = format!("{prefix} on ui_control($apply)
        $index := random(3,7) $value := random(77,99)
        set_ui_wf_property($w,$p,$index,$value)
        $result := get_ui_wf_property($w,$p,$index) $next := random(0,1000000) end on");
    let inline = format!("{prefix} on ui_control($apply)
        set_ui_wf_property($w,$p,random(3,7),random(77,99))
        $next := random(0,1000000) end on");
    let (mut a, av) = runtime(&explicit);
    let (mut b, bv) = runtime(&inline);
    a.seed_random(1234567); b.seed_random(1234567);
    let ap = a.active_plan(); let bp = b.active_plan();
    invoke(&mut a, ap, 0, "$apply"); invoke(&mut b, bp, 0, "$apply");
    let cell = |rt: &Runtime, plan, index| rt.script_cell(plan, ScriptInstanceId(0), index).unwrap();
    let index = cell(&a, ap, 0) as i32;
    let value = cell(&a, ap, 1);
    assert_eq!(cell(&a, ap, 2), value);
    assert_eq!(stored(&b, bp, 0, ui(&bv, "$w"), Property::Table, index), Some(value));
    assert_eq!(cell(&a, ap, 3), cell(&b, bp, 3));
    assert_eq!(stored(&a, ap, 0, ui(&av, "$w"), Property::Table, index), Some(value));
    assert!(a.take_fault().is_none()); assert!(b.take_fault().is_none());
}

#[test]
fn waveform_runtime_rejections_preserve_attachment_table_and_projection() {
    for operation in [
        "attach_zone($w,1,0)", // not a dense-region/selection ordinal
        "attach_zone($w,-1,0)",
        "attach_zone($apply,91,0)", // valid control, wrong kind/namespace
        "set_ui_wf_property($w,123,3,99)",
        "get_ui_wf_property($w,123,3)",
        "set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,-1,99)",
        "set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,65536,99)",
        "set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,1,99)",
    ] {
        let source = format!("{INIT} on ui_control($apply) {operation} end on");
        let (mut rt, mut view) = runtime(&source);
        let plan = rt.active_plan(); let id = ui(&view, "$w");
        let before = wave(&view, "$w");
        let apply = sampler_ksp::derived_control_id(0, "$apply");
        support::without_heap(|| {
            invoke_id(&mut rt, plan, apply);
            assert_eq!(rt.take_fault().map(|(_, e)| e), Some(Error::InvalidInput));
            assert_eq!(rt.script_store(plan, ScriptInstanceId(0), attachment_key(id)), Ok(Some(27)));
            assert_eq!(stored(&rt, plan, 0, id, Property::Table, 3), Some(77));
        });
        let mut effects = 0;
        rt.drain_effects(|e| { effects += 1; view.apply_ui_effect_for(plan, ScriptInstanceId(0), e); true });
        assert_eq!(effects, 0, "{operation}");
        assert_eq!(wave(&view, "$w"), before);
    }
}

#[test]
fn waveform_runtime_attachment_reset_cannot_cross_coalescing_boundary() {
    let source = format!("{INIT} on ui_control($apply)
        set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,88)
        attach_zone($w,91,11)
        set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,99) end on");
    let (mut rt, mut view) = runtime(&source);
    let plan = rt.active_plan(); let id = ui(&view, "$w");
    let apply = sampler_ksp::derived_control_id(0, "$apply");
    support::without_heap(|| {
        invoke_id(&mut rt, plan, apply);
        assert_eq!(stored(&rt, plan, 0, id, Property::Cursor, 0), Some(0));
        assert_eq!(stored(&rt, plan, 0, id, Property::MidiStart, 0), Some(60));
        assert_eq!(stored(&rt, plan, 0, id, Property::Highlight, 0), Some(-1));
    });
    let mut commands = Vec::new();
    rt.drain_effects(|e| { commands.push(view.service(e.service).unwrap());
        view.apply_ui_effect_for(plan, ScriptInstanceId(0), e); true });
    assert_eq!(commands, ["set_ui_wf_property", "attach_zone", "set_ui_wf_property"]);
    assert_eq!(wave(&view, "$w"), sampler_ui_ir::Waveform {
        zone: 91, flags: 11, cursor_us: 0, table: vec![0,0,0,99], highlighted: None, midi_start_note: 60,
    });
    assert!(rt.take_fault().is_none());
}

#[test]
fn waveform_runtime_tail_coalescing_respects_unrelated_effect_order() {
    let source = format!("{INIT} on ui_control($apply)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,1)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,2)
        message(5)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,3) end on");
    let (mut rt, view) = runtime(&source); let plan = rt.active_plan();
    invoke(&mut rt, plan, 0, "$apply");
    let mut seen = Vec::new();
    rt.drain_effects(|e| { seen.push((view.service(e.service).unwrap(), e.args().to_vec())); true });
    assert_eq!(seen.len(), 3);
    assert_eq!((seen[0].0, seen[0].1[3]), ("set_ui_wf_property", 2));
    assert_eq!(seen[1].0, "message");
    assert_eq!((seen[2].0, seen[2].1[3]), ("set_ui_wf_property", 3));
    assert!(rt.take_fault().is_none());
}

#[test]
fn waveform_runtime_sparse_store_pressure_faults_without_fake_readback_or_effect() {
    let source = "on init declare $index declare ui_waveform $w(1,1)
        attach_zone($w,27,3) end on
        on note if ($EVENT_NOTE = 61) attach_zone($w,91,11)
        else set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,$index,77) inc($index)
        end if end on";
    let (mut rt, mut view) = runtime(source); let plan = rt.active_plan(); let id = ui(&view, "$w");
    // Exactly 4096 runtime-created entries; the decoder's 64K index bound is not capacity.
    for expected in 1..=4096 {
        support::without_heap(|| { trigger_once(&mut rt, 60); });
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(expected));
        rt.drain_effects(|e| { view.apply_ui_effect_for(plan, ScriptInstanceId(0), e); true });
        assert!(rt.take_fault().is_none());
    }
    let before = wave(&view, "$w");
    support::without_heap(|| {
        trigger_once(&mut rt, 60);
        assert_eq!(rt.take_fault().map(|(_, e)| e), Some(Error::Capacity));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(4096));
        assert_eq!(stored(&rt, plan, 0, id, Property::Table, 4096), None);
    });
    let mut count = 0;
    rt.drain_effects(|_| { count += 1; true });
    assert_eq!(count, 0);
    assert_eq!(wave(&view, "$w"), before);
    // Attachment uses five existing keys and can reset even a completely full Store.
    support::without_heap(|| { trigger_once(&mut rt, 61); });
    assert_eq!(rt.script_store(plan, ScriptInstanceId(0), attachment_key(id)), Ok(Some(91)));
    assert_eq!(stored(&rt, plan, 0, id, Property::Table, 3), None);
    assert_eq!(stored(&rt, plan, 0, id, Property::Flags, 0), Some(11));
    assert!(rt.take_fault().is_none());
    rt.drain_effects(|e| { view.apply_ui_effect_for(plan, ScriptInstanceId(0), e); true });
    assert!(wave(&view, "$w").table.is_empty());
}

#[test]
fn waveform_runtime_outbox_pressure_rejects_reset_before_mutating_any_key() {
    let source = format!("{INIT} on ui_control($apply) attach_zone($w,91,0) end on
        on note message(5) end on");
    let (mut rt, view) = runtime(&source); let plan = rt.active_plan(); let id = ui(&view, "$w");
    // Fill with unrelated effects using the actual bound public note callback.
    for n in 0..EFFECT_CAPACITY {
        let note = rt.trigger(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0,
            key: 60, external_id: Some(n as i32) }, 60, 1.).unwrap();
        rt.release(note).unwrap();
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
    }
    let apply = sampler_ksp::derived_control_id(0, "$apply");
    support::without_heap(|| {
        invoke_id(&mut rt, plan, apply);
        assert_eq!(rt.take_fault().map(|(_, e)| e), Some(Error::Capacity));
        assert_eq!(rt.script_store(plan, ScriptInstanceId(0), attachment_key(id)), Ok(Some(27)));
        assert_eq!(stored(&rt, plan, 0, id, Property::Cursor, 0), Some(12000));
        assert_eq!(stored(&rt, plan, 0, id, Property::Table, 3), Some(77));
    });
    let mut count = 0;
    rt.drain_effects(|e| { assert_eq!(view.service(e.service), Some("message")); count += 1; true });
    assert_eq!(count, EFFECT_CAPACITY);
    assert_eq!(rt.dropped_effects(), 0); // waveform rejection is an explicit fault, not silent drop
}

#[test]
fn waveform_runtime_widgets_instances_cells_and_interned_symbol_maps_do_not_alias() {
    let source = "on init declare $cell := 123
        declare ui_waveform $w(1,1) declare ui_waveform $other(1,1) declare ui_button $apply
        attach_zone($w,27,3) attach_zone($other,91,0)
        set_ui_wf_property($other,$UI_WF_PROP_PLAY_CURSOR,0,9000) end on
        on ui_control($apply) set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,24000) end on";
    let a = script(source, 0);
    // Change symbol encounter order in slot 1 without inventing vendor ordinals.
    let b = script(&source.replace("declare $cell := 123", "declare $p := $UI_WF_PROP_TABLE_VAL declare $cell := 123"), 1);
    let mut av = a.view(); let bv = b.view(); let id = ui(&av, "$w");
    let plan = prepared(vec![a,b]); let limits = limits(&plan);
    let mut rt = Runtime::new(plan, limits).unwrap(); let plan = rt.active_plan();
    invoke(&mut rt, plan, 0, "$apply");
    assert_eq!(stored(&rt, plan, 0, id, Property::Cursor, 0), Some(24000));
    assert_eq!(stored(&rt, plan, 1, ui(&bv, "$w"), Property::Cursor, 0), Some(0));
    assert_eq!(stored(&rt, plan, 0, ui(&av, "$other"), Property::Cursor, 0), Some(9000));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(123));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(1), 1), Ok(123));
    rt.drain_effects(|e| {
        assert!(!av.apply_ui_effect_for(plan, ScriptInstanceId(1), e));
        assert!(av.apply_ui_effect_for(plan, ScriptInstanceId(0), e)); true
    });
    assert_eq!(wave(&av, "$w").cursor_us, 24000);
    assert_eq!(wave(&bv, "$w").cursor_us, 0);
    assert!(rt.take_fault().is_none());
}

#[test]
fn waveform_runtime_retained_old_plan_state_isolated_and_stale_projection_rejected() {
    let old_source = format!("{INIT} on note wait(1000)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,24000) end on");
    let old = script(&old_source, 0); let mut old_view = old.view(); let id = ui(&old_view, "$w");
    let old = prepared(vec![old]); let limits = limits(&old);
    let (mut rt, mut control) = Runtime::with_plan_updates(old, limits, 2, 1).unwrap();
    let old_plan = rt.active_plan();
    let note = rt.trigger(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0,
        key: 60, external_id: Some(1) }, 60, 1.).unwrap();
    let new = script(&INIT.replace("12000", "5000"), 0); let mut new_view = new.view();
    let request = control.submit(Box::new(prepared(vec![new]))).unwrap();
    assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
    let new_plan = rt.active_plan(); assert_ne!(old_plan, new_plan);
    let new_id = ui(&new_view, "$w");
    support::without_heap(|| {
        rt.render(&mut [[0.;2];64]).unwrap();
        assert_eq!(stored(&rt, old_plan, 0, id, Property::Cursor, 0), Some(24000));
        assert_eq!(stored(&rt, new_plan, 0, new_id, Property::Cursor, 0), Some(5000));
    });
    let mut count = 0;
    rt.drain_effects(|e| {
        assert_eq!(e.plan, old_plan);
        assert!(!new_view.apply_ui_effect_for(new_plan, ScriptInstanceId(0), e));
        assert!(old_view.apply_ui_effect_for(old_plan, ScriptInstanceId(0), e));
        count += 1; true
    });
    assert_eq!(count, 1);
    assert_eq!(wave(&new_view, "$w").cursor_us, 5000);
    assert_eq!(rt.note_plan(note), Ok(old_plan));
    assert!(rt.take_fault().is_none());
}

#[test]
fn waveform_init_alias_imported_placeholder_and_multiple_widgets_use_hir_identity() {
    use sampler_ksp::model::{PerformanceControl, PerformanceView, WidgetKind};
    let source = "on init load_performance_view(\"owned\")
        declare ui_waveform $w(1,1) declare ui_table %read[4](1,1,1000000)
        declare $p := $UI_WF_PROP_TABLE_VAL
        attach_zone($Imported,91,3) attach_zone($w,27,3)
        set_ui_wf_property($Imported,$p,3,99) set_ui_wf_property($w,$p,3,77)
        %read[0] := get_ui_wf_property($Imported,$p,3)
        %read[1] := get_ui_wf_property($w,$p,3)
        %read[2] := get_ui_wf_property($w,$p,77)
        %read[3] := get_ui_wf_property($Imported,$p,77) end on";
    let mut env = environment(0);
    let mut imported = PerformanceControl::assumed("$Imported", WidgetKind::Waveform);
    imported.params = vec![1,1];
    env.performance_view = PerformanceView { controls: vec![imported], ..Default::default() };
    let script = sampler_ksp::compile_with(source, 48000, KspLimits::LIBRARY, &[], &env).unwrap();
    let view = script.view();
    assert_eq!(wave(&view, "$Imported").table, [0,0,0,99]);
    assert_eq!(wave(&view, "$w").table, [0,0,0,77]);
    let model = view.ui(&|_| None).unwrap();
    assert_eq!(model.widgets.iter().find(|w| w.name == "%read").unwrap().value,
        Some(sampler_ui_ir::Value::Integers(vec![99,77,0,0])));
}

#[cfg(feature = "cache")]
#[test]
fn waveform_cache_hit_miss_roundtrip_and_missing_captured_source_rejection() {
    let source = format!("{INIT} on ui_control($apply)
        set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,24000) end on");
    let fresh = sampler_ksp::initialize(&source, KspLimits::LIBRARY, &environment(0)).unwrap();
    let bytes = serde_json::to_vec(&fresh.capture_initialized().unwrap()).unwrap();
    let restored = sampler_ksp::restore_initialized(&source, KspLimits::LIBRARY,
        serde_json::from_slice(&bytes).unwrap()).unwrap();
    let cached = sampler_ksp::compile_initialized(&source, 48000, KspLimits::LIBRARY, &[], restored).unwrap();
    let mut cv = cached.view(); let fv = script(&source,0).view();
    assert_eq!(wave(&cv,"$w"), wave(&fv,"$w"));
    let plan = prepared(vec![cached]); let limits = limits(&plan);
    let mut rt = Runtime::new(plan,limits).unwrap(); let plan = rt.active_plan();
    invoke(&mut rt,plan,0,"$apply");
    rt.drain_effects(|e| { cv.apply_ui_effect_for(plan,ScriptInstanceId(0),e); true });
    assert_eq!(wave(&cv,"$w").cursor_us,24000);
    // Synthetic corrupted/incompatible source domain, not native cache behavior.
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["zones"] = serde_json::json!([]);
    let bad = sampler_ksp::restore_initialized(&source,KspLimits::LIBRARY,
        serde_json::from_slice(&serde_json::to_vec(&json).unwrap()).unwrap()).unwrap();
    assert!(sampler_ksp::compile_initialized(&source,48000,KspLimits::LIBRARY,&[],bad).is_err());
    assert!(rt.take_fault().is_none());
}
