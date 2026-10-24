//! Host recall at the script layer: the saved script state of a live instrument, restored into a
//! fresh runtime, gives the same variables, the same UI values, and no ui_control callback.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;

const SOURCE: &str = r#"on init
    declare ui_knob $k(0,127,1) make_persistent($k)
    declare $count make_persistent($count)
    declare $pc
    declare $temp
    declare %arr[3] := (1,2,3) make_persistent(%arr)
    declare !texts[2] := ("a","b") make_persistent(!texts)
    declare $pers := 5 make_persistent($pers)
    declare $instr := 2 make_instr_persistent($instr)
    end on
    on ui_control($k)
    inc($count)
    %arr[1] := %arr[1] + 10
    end on
    on persistence_changed
    inc($pc)
    end on"#;

fn runtime() -> (Runtime, ScriptStateBuffer) {
    let script = sampler_ksp::compile_with(
        SOURCE,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &sampler_ksp::Environment::default(),
    )
    .unwrap();
    let state = sampler_ksp::persistent_state_buffer(&[script.view()]).unwrap();
    let plan = sampler_ksp::bind_modules(
        vec![script],
        Prepared::new(48000, vec![], vec![], 0).unwrap(),
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let rt = Runtime::new(plan, limits).unwrap();
    (rt, state)
}

fn values(state: &ScriptStateBuffer) -> Vec<ScriptStateValue> {
    state.values.iter().map(|e| e.value).collect()
}

#[test]
fn saved_script_state_restores_identically_without_firing_ui_control() {
    // Live: move the knob (fires ui_control), then save.
    let (mut live, mut saved) = runtime();
    let plan = live.active_plan();
    live.capture_script_state(plan, &mut saved).unwrap();
    live.invoke_widget(
        ControlContext {
            performance: live.performance(0).unwrap(),
            origin: ChannelAddress {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
            },
            channels: 1,
        },
        plan,
        None,
        &[WidgetEdit {
            id: sampler_ksp::derived_control_id(0, "$k"),
            index: 0,
            interaction: Default::default(),
            value: WidgetValue::Integer(37),
        }],
    )
    .unwrap();
    live.capture_script_state(plan, &mut saved).unwrap();
    let live_values = values(&saved);
    let live_texts: Vec<_> = saved.values.iter().filter_map(|e| saved.text(e.value).cloned()).collect();
    assert!(live_values.contains(&ScriptStateValue::Cell(1)), "ui_control must have counted once");

    // Fresh instance: restore the saved state, then save again.
    let (mut fresh, mut restored) = runtime();
    let fresh_plan = fresh.active_plan();
    fresh.capture_script_state(fresh_plan, &mut restored).unwrap();
    // The saved buffer's layout is the same source's, so its values restore directly.
    restored.values.clone_from(&saved.values);
    restored.callbacks.clone_from(&saved.callbacks);
    let revision = fresh.control_revision(fresh_plan).unwrap();
    let _ = fresh.restore_script_state(fresh_plan, Some(revision), &mut restored).unwrap();
    fresh.capture_script_state(fresh_plan, &mut restored).unwrap();

    assert_eq!(
        values(&restored),
        live_values,
        "restored variables differ from the saved ones (a ui_control callback may have re-fired)"
    );
    let fresh_texts: Vec<_> = restored.values.iter().filter_map(|e| restored.text(e.value).cloned()).collect();
    assert_eq!(fresh_texts, live_texts, "text values differ after reload");
}
