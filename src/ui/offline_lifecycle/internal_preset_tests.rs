use super::*;
use crate::sound::tree::MixTree;
use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};

fn fixture(directory: &Path) -> (V2Core, SwitchOwner, ControlIngress, i32, SwitchPlan) {
    let a = directory.join("a.nka");
    let b = directory.join("b.nka");
    std::fs::write(&a, "%data\n0\n").unwrap();
    std::fs::write(&b, "%data\n1\n").unwrap();
    let source = format!(
        r#"on init
        make_perfview
        declare ui_switch $choice
        declare %source[1] := (0)
        declare %data[1]
        declare $done := 0
        declare $job := -1
        pgs_create_key(LOAD, 1)
    end on
    on ui_control($choice)
        pgs_set_key_val(LOAD, 0, $choice)
    end on
    on pgs_changed
        $done := 0
        if (pgs_get_key_val(LOAD, 0) = 0)
            $job := load_array_str(%data, "{}")
        else
            $job := load_array_str(%data, "{}")
        end if
        while ($done = 0)
            wait(1000)
        end while
        %source[0] := %data[0]
        purge_group(0, 1 - %source[0])
        purge_group(1, %source[0])
    end on
    on async_complete
        if ($NI_ASYNC_ID = $job)
            $done := 1
        end if
    end on
    on note
        disallow_group($ALL_GROUPS)
        allow_group(%source[0])
    end on"#,
        a.display(),
        b.display()
    );
    let script = sampler_ksp::compile(&source, 48_000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let view = script.view();
    let id = view
        .model()
        .interface
        .widgets
        .iter()
        .find(|w| w.name == "$choice")
        .unwrap()
        .ui_id;
    let pcm = |value: f32| {
        let p = Pcm::new(48_000, vec![[value; 2]; 1024].into()).unwrap();
        p.set_residency_loader(move || Ok(vec![(0, vec![[value; 2]; 1024].into())]))
            .unwrap();
        p
    };
    let region = |sample| Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    let base = Prepared::new(
        48_000,
        vec![pcm(0.25), pcm(0.5)],
        vec![region(0), region(1)],
        2,
    )
    .unwrap()
    .with_groups(2, vec![Some(0), Some(1)])
    .unwrap();
    let prepared = sampler_ksp::bind_modules(vec![script], base).unwrap();
    let limits = Limits::for_plan(&prepared, 8, 8);
    let mut part = Part::new(
        Runtime::new(prepared, limits).unwrap(),
        MixTree::instrument("source-free internal preset"),
    )
    .unwrap();
    part.epoch = 1;
    let ingress = part.ui_controls.take().unwrap();
    let mut core = V2Core::with_parts(1, 48_000.);
    core.install(0, Some(Box::new(part)));
    let owner = SwitchOwner {
        request: 1,
        scripts: ScriptUi {
            views: vec![view],
            ..Default::default()
        },
        ..Default::default()
    };
    let plan: SwitchPlan = serde_json::from_value(json!({"schema":1,"notes_after_widgets":true,"residency_wait_ms":1000,
        "key":60,"velocity":127,"note_frames":512,"observed_script_cells":[{"slot":0,"name":"%source","index":0}],
        "steps":[{"op":"load","path":"unused.nki"}]})).unwrap();
    (core, owner, ingress, id, plan)
}

#[test]
fn widget_note_observes_real_pgs_array_load_source_pcm_and_retained_owner_aba() {
    let directory = tempfile::tempdir().unwrap();
    let (mut core, mut owner, mut ingress, id, plan) = fixture(directory.path());
    let mut peaks = Vec::new();
    for (action, choice) in [0, 1, 0].into_iter().enumerate() {
        if action != 0 {
            let (faces, _, _) = switch_state(&core, &mut owner);
            let targets = switch_widget_target(&faces, 0, id);
            assert_eq!(
                switch_submit_widget(
                    Some(&mut ingress),
                    &targets,
                    0,
                    0,
                    SwitchValue::Integer(choice),
                    true
                ),
                (Some(true), None)
            );
            switch_render(&mut core, &mut owner, 128).unwrap();
            let callbacks = step(&mut core, &mut owner.scripts.views).unwrap();
            assert_eq!(
                callbacks["activation_readiness"],
                "finite_callbacks_not_pending"
            );
        }
        let observed = switch_script_cells(&core, &owner, &plan.observed_script_cells).unwrap();
        assert_eq!(observed[0]["value"], choice);
        let note = switch_widget_note(&mut core, &mut owner, &plan, 1, true, action).unwrap();
        assert_eq!(note["status"], "observed");
        assert_eq!(note["audio_generation"], 1);
        assert!(note["signal"]["nonzero_frames"].as_u64().unwrap() > 0);
        peaks.push(note["signal"]["peak"].as_f64().unwrap());
    }
    assert!(peaks[1] > peaks[0] * 1.8, "B must play its distinct source");
    assert!(
        (peaks[2] - peaks[0]).abs() < 1e-6,
        "A source must return after B purge/reload"
    );
    assert_eq!(core.epoch(0), 1);
}

#[test]
fn widget_note_query_validation_old_defaults_and_stale_owners_are_explicit() {
    let default: SwitchPlan = serde_json::from_value(json!({"schema":1,"steps":[]})).unwrap();
    assert!(!default.notes_after_widgets);
    assert!(default.observed_script_cells.is_empty());
    for query in [
        json!({"slot":5,"name":"%a"}),
        json!({"slot":0,"name":"@text"}),
        json!({"slot":0,"name":"%a","index":16777217}),
    ] {
        let query: ObservedScriptCell = serde_json::from_value(query).unwrap();
        assert!(validate_script_cells(&[query]).is_err());
    }
    assert!(switch_probe(&json!({"schema":1,"notes_after_widgets":true,"steps":[{"op":"load","path":"must-not-open"}]})).is_err());
    let directory = tempfile::tempdir().unwrap();
    let (mut core, mut owner, _ingress, _id, plan) = fixture(directory.path());
    let epoch = core.epoch(0);
    assert_eq!(
        switch_widget_note(&mut core, &mut owner, &plan, 0, true, 0).unwrap()["status"],
        "skipped_unadmitted_or_stale_owner"
    );
    assert_eq!(
        switch_widget_note(&mut core, &mut owner, &plan, 1, false, 0).unwrap()["status"],
        "skipped_unadmitted_or_stale_owner"
    );
    assert_eq!(core.epoch(0), epoch);
    assert!(owner.held.is_none());
    for query in [
        ObservedScriptCell {
            slot: 1,
            name: "%source".into(),
            index: 0,
        },
        ObservedScriptCell {
            slot: 0,
            name: "%missing".into(),
            index: 0,
        },
        ObservedScriptCell {
            slot: 0,
            name: "%source".into(),
            index: 1,
        },
    ] {
        assert!(switch_script_cells(&core, &owner, &[query]).is_err());
    }
    owner.request = 2;
    assert!(switch_script_cells(&core, &owner, &plan.observed_script_cells).is_err());
}
