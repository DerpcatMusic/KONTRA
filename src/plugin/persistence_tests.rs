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

#[cfg(feature = "shots")]
#[test]
#[ignore = "requires installed Kontakt corpus"]
fn conflux_builtin_report_contexts() {
    let root = std::env::var_os("KONTRA_PERSISTENCE_CORPUS").map(PathBuf::from).unwrap();
    let params = SamplerParams::new();
    params.selection.write().unwrap().parts = vec![Part {
        path: root.join("Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki").display().to_string(),
        ..Default::default()
    }];
    assert!(load_part(&params, 0, None));
    let view = params.shared.view.lock().unwrap();
    let instrument = view.parts[0].instrument.as_ref().unwrap();
    let commands = ["get_menu_item_value", "get_menu_item_str", "get_zone_par", "set_event_par_arr", "ignore_controller"];
    let mut unresolved = std::collections::BTreeSet::new();
    for missing in &instrument.unsupported {
        let Some(command) = commands.iter().find(|c| missing.feature.ends_with(&format!(": {c}"))) else { continue };
        if missing.feature.starts_with("script Unsupported:") { unresolved.insert(*command); }
        let mut parts = missing.location.rsplitn(2, " line ");
        let line = parts.next().unwrap().parse::<usize>().unwrap();
        let name = parts.next().unwrap();
        let source = &instrument.behaviors.iter().find(|b| b.name == name).unwrap().source;
        let text = source.lines().nth(line - 1).unwrap();
        let symbols: Vec<_> = text.split(|c:char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|word| word.starts_with("ZONE_PAR_") || word.starts_with("EVENT_PAR_")).collect();
        println!("BUILTIN_CONTEXT {}", serde_json::json!({"builtin":command,"line":line,
            "symbols":symbols,"outside_controller":missing.value.contains("outside on controller"),
            "init_no_effect":missing.value.contains("has no effect in on init"),
            "ignored_runtime":missing.value.contains("not executed at runtime"),
            "kind":missing.feature.split(":").next().unwrap()}));
    }
    println!("CONFLUX_UNKNOWN_COMMANDS {}", unresolved.len());
    assert!(unresolved.is_empty(), "unresolved builtins: {unresolved:?}");
}
