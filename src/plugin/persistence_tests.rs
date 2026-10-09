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

/// Opt-in corpus gate: library/script bytes and saved values never leave RAM.
#[cfg(feature = "shots")]
#[test]
#[ignore = "requires installed Kontakt corpus"]
fn corpus_project_save_reload_compares_every_persistent_and_ui_value() {
    fn install(params: &SamplerParams) -> V2Core {
        assert!(load_part(params, 0, None));
        let (_, _, part) = params.shared.ready.pop().unwrap();
        assert!(
            part.is_some(),
            "{}",
            params.shared.view.lock().unwrap().parts[0].status
        );
        let mut core = V2Core::with_parts(1, params.shared.rate());
        core.install(0, part);
        core
    }
    fn settle(params: &SamplerParams, core: &mut V2Core) {
        let atoms = params.shared.part(0).unwrap();
        for _ in 0..64 {
            core.render(64);
            atoms.refresh_controls(|id| core.control_value(0, id));
            if let Some(ingress) = atoms.ingress.lock().unwrap().as_mut() {
                ingress.refresh();
            }
            core.take_effects(0, &mut |instance, effect| {
                if effect.service == sampler_core::MIDI_SERVICE {
                    if let Some(ingress) = atoms.ingress.lock().unwrap().as_mut() {
                        ingress.service_midi(effect);
                    }
                    return true;
                }
                atoms.scripts.lock().unwrap().apply(instance, effect);
                true
            });
        }
    }
    let root = std::env::var_os("KONTRA_PERSISTENCE_CORPUS")
        .map(PathBuf::from)
        .expect("set KONTRA_PERSISTENCE_CORPUS to the Kontakt library folder");
    let mut failures = 0;
    for (library, relative) in [
        (
            "Conflux",
            "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
        ),
        (
            "Analog Strings",
            "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
        ),
        (
            "Dolce",
            "Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 02 7 1st Violins - Sustained.nki",
        ),
    ] {
        let path = root.join(relative);
        assert!(path.is_file(), "corpus fixture absent: {library}");
        let params = SamplerParams::new();
        params.selection.write().unwrap().parts = vec![Part {
            path: path.display().to_string(),
            ..Default::default()
        }];
        let mut core = install(&params);
        settle(&params, &mut core);
        let atoms = params.shared.part(0).unwrap();
        let persistent = atoms
            .scripts
            .lock()
            .unwrap()
            .views
            .iter()
            .map(|view| view.model().persistent.len())
            .sum::<usize>();
        assert!(persistent > 0, "fixture must contain persistent KSP state");
        let mut edits = 0;
        for face in params.shared.view.lock().unwrap().parts[0]
            .interfaces
            .iter()
        {
            for widget in &face.widgets {
                let sampler_ui_ir::Binding::Control(id) = widget.binding else {
                    continue;
                };
                let (sampler_ui_ir::Kind::Knob { range, .. }
                | sampler_ui_ir::Kind::Slider { range, .. }) = &widget.kind
                else {
                    continue;
                };
                let current = core.control_value(0, id).unwrap();
                let value = if current == range.min {
                    range.max
                } else {
                    range.min
                };
                if params.shared.set_control(0, id, value) {
                    edits += 1;
                }
                if edits == 3 {
                    break;
                }
            }
            if edits == 3 {
                break;
            }
        }
        assert!(edits > 0, "fixture needs an edited UI control");
        settle(&params, &mut core);
        let before_ui = core.widget_gate_values(0);
        let mut before_progress = Vec::new();
        core.widget_gate_behavior_progress(0, |p| before_progress.push(serde_json::json!({"program":p.program,"pc":p.pc,"waiting":p.waiting,"outcome":format!("{:?}",p.outcome)})));
        let fields = moose::params::Params::serialize_persist(&params);
        let before: serde_json::Value =
            serde_json::from_str(&params.selection.read().unwrap().parts[0].script_state).unwrap();
        let recalled = SamplerParams::new();
        moose::params::Params::load_persist(&recalled, &fields);
        let mut reopened = install(&recalled);
        let immediately: serde_json::Value = serde_json::from_str(
            &recalled
                .shared
                .part(0)
                .unwrap()
                .ingress
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .save_script_state()
                .unwrap(),
        )
        .unwrap();
        settle(&recalled, &mut reopened);
        let after_ui = reopened.widget_gate_values(0);
        assert_eq!(
            before_ui.len(),
            after_ui.len(),
            "UI roster changed: {library}"
        );
        let mut after_progress = Vec::new();
        reopened.widget_gate_behavior_progress(0, |p| after_progress.push(serde_json::json!({"program":p.program,"pc":p.pc,"waiting":p.waiting,"outcome":format!("{:?}",p.outcome)})));
        recalled.capture_ui_controls();
        let after: serde_json::Value =
            serde_json::from_str(&recalled.selection.read().unwrap().parts[0].script_state)
                .unwrap();
        let before_values = before["values"].as_array().unwrap();
        let after_values = after["values"].as_array().unwrap();
        assert_eq!(
            before["schema"], after["schema"],
            "schema changed: {library}"
        );
        assert_eq!(
            before_values.len(),
            after_values.len(),
            "roster changed: {library}"
        );
        let losses: Vec<_> = before_values
            .iter()
            .zip(after_values)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(index, (a, b))| {
                serde_json::json!({
                    "index": index,
                    "kind": a.as_object().unwrap().keys().next().unwrap(),
                    "before_hash": blake3::hash(a.to_string().as_bytes()).to_hex().to_string(),
                    "after_hash": blake3::hash(b.to_string().as_bytes()).to_hex().to_string(),
                })
            })
            .collect();
        let addresses = core.project_state_addresses(0);
        let scripts = atoms.scripts.lock().unwrap();
        let mut variable_losses = std::collections::BTreeMap::new();
        for loss in &losses {
            let address = addresses[loss["index"].as_u64().unwrap() as usize];
            for (instance, view) in scripts.views.iter().enumerate() {
                for (variable, persistent) in view.model().persistent.iter().enumerate() {
                    let contains = match (&persistent.location, address) {
                        (
                            sampler_ksp::model::Location::Control(id),
                            sampler_core::ScriptStateAddress::Control(actual),
                        ) => id.0 == actual.0,
                        (
                            sampler_ksp::model::Location::Cells { offset, len },
                            sampler_core::ScriptStateAddress::Cell {
                                instance: actual,
                                index,
                            },
                        )
                        | (
                            sampler_ksp::model::Location::Texts { offset, len },
                            sampler_core::ScriptStateAddress::Text {
                                instance: actual,
                                index,
                            },
                        ) => {
                            actual.0 as usize == instance
                                && (*offset..offset + len).contains(&index)
                        }
                        _ => false,
                    };
                    if contains {
                        *variable_losses
                            .entry(format!(
                                "slot{instance}/variable{variable}/{}",
                                blake3::hash(persistent.name.as_bytes()).to_hex()
                            ))
                            .or_insert(0) += 1;
                    }
                }
            }
        }
        let ui_losses: Vec<_> = before_ui
            .iter()
            .filter(|(id, value)| after_ui.get(id) != Some(value))
            .map(|(id, _)| id.0)
            .collect();
        println!(
            "PERSISTENCE_ROUNDTRIP {}",
            serde_json::json!({"library":library,
            "persistent_variables":persistent,"saved_values":before_values.len(),"ui_values":before_ui.len(),
            "ui_edits":edits,"losses":losses,"ui_loss_ids":ui_losses,"variable_losses":variable_losses,
            "before_progress":before_progress,"after_progress":after_progress,
            "immediate_losses": before_values.iter().zip(immediately["values"].as_array().unwrap()).filter(|(a,b)|a!=b).count()})
        );
        failures += losses.len() + ui_losses.len();
    }
    assert_eq!(
        failures, 0,
        "project save/reopen changed persistent or UI values"
    );
}
