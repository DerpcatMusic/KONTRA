//! Synthetic Original text/table edits and host state stay in RAM.
use super::*;
use sampler_core::{Limits, Prepared, Runtime};

const SCRIPT: &str = "on init\n declare ui_text_edit @name\n declare ui_table %table[3](1,1,1000)\n declare ui_text_edit @draft\n declare ui_table %unmarked[2](1,1,1000)\n make_persistent(@name)\n make_persistent(%table)\n end on";

#[test]
fn host_save_contains_live_original_text_and_array_values() {
    let script = sampler_ksp::compile(SCRIPT, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = script.ui(&|_| None).unwrap();
    let view = script.view();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 0);
    let mut part = CorePart::new(
        Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("host save"),
    )
    .unwrap();
    part.prepare_persistence(&[view.clone()], "").unwrap();
    let p = SamplerParams::new();
    p.selection.write().unwrap().parts = vec![Part {
        path: "synthetic".into(),
        ..Default::default()
    }];
    let atoms = p.shared.part(0).unwrap();
    atoms.generation.store(1, Ordering::Release);
    *atoms.ingress.lock().unwrap() = part.ui_controls.take();
    atoms.scripts.lock().unwrap().views.push(view);
    p.shared.view.lock().unwrap().parts[0].attempted = Some((
        "synthetic".into(),
        0,
        String::new(),
        48000f64.to_bits(),
        false,
        false,
        -1,
        Streaming::Auto,
    ));
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    for (name, value, index) in [
        (
            "@name",
            sampler_ui_ir::Value::Text("host edited text".into()),
            None,
        ),
        ("%table", sampler_ui_ir::Value::Integer(837), Some(1)),
        (
            "@draft",
            sampler_ui_ir::Value::Text("unmarked edit".into()),
            None,
        ),
        ("%unmarked", sampler_ui_ir::Value::Integer(419), Some(0)),
    ] {
        let widget = face.widgets.iter().find(|w| w.name == name).unwrap();
        assert!(p.shared.set_widget_at(0, 1, 0, widget, index, value));
    }
    assert_eq!(
        tests::allocations(|| {
            core.render(64);
        }),
        0,
        "audio capture must not allocate or free"
    );
    p.capture_ui_controls();
    let encoded = serde_json::to_string(&p.selection.read().unwrap().parts).unwrap();
    assert!(
        encoded.contains("host edited text"),
        "host save loses live Original text"
    );
    assert!(
        encoded.contains("837"),
        "host save loses live Original table cells"
    );
    let field = moose::params::Params::serialize_persist(&p);
    let recalled = SamplerParams::new();
    moose::params::Params::load_persist(&recalled, &field);
    let restored = recalled.selection.read().unwrap().parts[0].clone();
    let script = sampler_ksp::compile(SCRIPT, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let views = vec![script.view()];
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 0);
    let mut reloaded = CorePart::new(
        Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("host reload"),
    )
    .unwrap();
    reloaded
        .prepare_persistence(&views, &restored.script_state)
        .unwrap();
    let saved = reloaded
        .ui_controls
        .as_ref()
        .unwrap()
        .save_script_state()
        .unwrap();
    assert!(
        saved.contains("host edited text")
            && saved.contains("837")
            && saved.contains("unmarked edit")
            && saved.contains("419"),
        "host recall must restore text and array cells"
    );
    assert!(reloaded.prepare_persistence(&views, "{bad JSON").is_err());
    assert_eq!(
        reloaded
            .ui_controls
            .as_ref()
            .unwrap()
            .save_script_state()
            .unwrap(),
        saved,
        "rejected state cannot lose the current values"
    );
}

#[test]
fn immediate_host_save_captures_live_uvi_state_before_background_poll() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>local saves=0; Knob('Gain',0,0,100); function onSave() saves=saves+1; return saves end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (_owner, loaded) = sampler_uvi::scripted::ScriptThread::spawn(
        xml.into(),
        (),
        sampler_uvi::script::Config::default(),
    )
    .unwrap();
    let p = SamplerParams::new();
    p.selection.write().unwrap().parts = vec![Part {
        path: "synthetic.uvip".into(),
        ..Default::default()
    }];
    let source = p.selection.read().unwrap().parts[0].source();
    p.shared.view.lock().unwrap().parts[0].attempted = Some((
        "synthetic.uvip".into(),
        0,
        String::new(),
        48000f64.to_bits(),
        false,
        false,
        -1,
        Streaming::Auto,
    ));
    let atoms = p.shared.part(0).unwrap();
    {
        let mut scripts = atoms.scripts.lock().unwrap();
        scripts.uvi = Some(loaded.ui.clone());
        scripts.uvi_source = Some(source);
        scripts.uvi_revision = loaded.ui.revision();
    }
    let id = sampler_uvi::script::control_id(1, 0);
    let revision = loaded.ui.revision();
    assert!(loaded.ui.edit(id, 73.));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while loaded.ui.revision() == revision && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(loaded.ui.value(id), Some(73.));
    assert!(p.selection.read().unwrap().parts[0].uvi_state.is_empty());
    let field = moose::params::Params::serialize_persist(&p);
    let recalled = SamplerParams::new();
    moose::params::Params::load_persist(&recalled, &field);
    let saved = recalled.selection.read().unwrap().parts[0].clone();
    assert!(
        !saved.uvi_state.is_empty(),
        "host save must capture UVI before background poll"
    );
    let state: sampler_uvi::script::UiState = serde_json::from_str(&saved.uvi_state).unwrap();
    assert_eq!(
        state.custom,
        Some(sampler_uvi::script::SavedValue::Number(2.)),
        "initial snapshot plus one authored onSave per host save"
    );
    let host = sampler_uvi::script::ScriptHost::new_with_ui_state(
        xml,
        (),
        sampler_uvi::script::Config::default(),
        Some(&state),
    )
    .unwrap();
    assert_eq!(host.control_values(), vec![(id, 73.)]);
}

#[test]
fn host_save_captures_uvi_custom_state_without_ui_revision() {
    use sampler_uvi::{script::SavedValue, scripted::Script};
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>local notes=0; function onNote(e) notes=notes+1; postEvent{type=Event.Controller,controller=1,value=17} end; function onSave() return {notes=notes} end; function onLoad(s) notes=s.notes end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (mut owner, loaded) = sampler_uvi::scripted::ScriptThread::spawn(
        xml.into(),
        (),
        sampler_uvi::script::Config::default(),
    )
    .unwrap();
    let p = SamplerParams::new();
    p.selection.write().unwrap().parts = vec![Part {
        path: "synthetic.uvip".into(),
        ..Default::default()
    }];
    let source = p.selection.read().unwrap().parts[0].source();
    p.shared.view.lock().unwrap().parts[0].attempted = Some((
        "synthetic.uvip".into(),
        0,
        String::new(),
        48000f64.to_bits(),
        false,
        false,
        -1,
        Streaming::Auto,
    ));
    let atoms = p.shared.part(0).unwrap();
    {
        let mut scripts = atoms.scripts.lock().unwrap();
        scripts.uvi = Some(loaded.ui.clone());
        scripts.uvi_source = Some(source);
        scripts.uvi_revision = loaded.ui.revision();
    }
    let revision = loaded.ui.revision();
    owner.note_on(1, 60, 100);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut commands = Vec::new();
    while commands.is_empty() {
        owner.drain(&mut commands);
        assert!(
            std::time::Instant::now() < deadline,
            "calibrate that onNote completed"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(loaded.ui.revision(), revision);
    let field = moose::params::Params::serialize_persist(&p);
    let recalled = SamplerParams::new();
    moose::params::Params::load_persist(&recalled, &field);
    let saved = recalled.selection.read().unwrap().parts[0].clone();
    assert!(
        !saved.uvi_state.is_empty(),
        "host save must capture custom state without a UI revision"
    );
    let state = serde_json::from_str(&saved.uvi_state).unwrap();
    let host = sampler_uvi::script::ScriptHost::new_with_ui_state(
        xml,
        (),
        sampler_uvi::script::Config::default(),
        Some(&state),
    )
    .unwrap();
    assert_eq!(
        host.save_ui_state().unwrap().custom,
        Some(SavedValue::Table(vec![(
            SavedValue::String("notes".into()),
            SavedValue::Number(1.)
        )]))
    );

    // A pending source must not inherit the old instrument's custom state.
    {
        let mut selection = p.selection.write().unwrap();
        selection.parts[0].path = "different.uvip".into();
        selection.parts[0].uvi_state.clear();
        selection.parts[0].uvi_state_source.clear();
    }
    let field = moose::params::Params::serialize_persist(&p);
    moose::params::Params::load_persist(&recalled, &field);
    assert!(
        recalled.selection.read().unwrap().parts[0]
            .uvi_state
            .is_empty()
    );
}
