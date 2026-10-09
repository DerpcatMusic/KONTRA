//! Immediate and custom UVI host save/recall, independent of native state.
use super::*;

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
