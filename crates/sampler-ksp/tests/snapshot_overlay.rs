//! Authored same-owner snapshot transaction oracles; no installed instruments.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;
use sampler_ir::Saved;
use sampler_ksp::{Environment, Script, ScriptView, model::Location, setup::*};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

fn compile(source: &str, slot: u8) -> Script {
    sampler_ksp::compile_with(
        source,
        48_000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &Environment {
            slot,
            ..Default::default()
        },
    )
    .unwrap()
}
fn limits() -> SnapshotSavedLimits {
    SnapshotSavedLimits {
        value_entries: SNAPSHOT_OVERLAY_VALUE_CAPACITY,
        text_bytes: 8 << 20,
    }
}
fn slot(slot: u8, values: Vec<(&str, Saved)>) -> SnapshotSavedSlot {
    SnapshotSavedSlot {
        slot,
        values: values
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
    }
}
fn runtime(scripts: Vec<Script>, behaviors: usize, pcm: bool) -> (Runtime, Vec<ScriptView>) {
    let views = scripts.iter().map(Script::view).collect();
    let base = if pcm {
        Prepared::new(
            48_000,
            vec![Pcm::new(48_000, vec![[0.25; 2]; 4096].into_boxed_slice()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
    } else {
        Prepared::new(48_000, vec![], vec![], 0).unwrap()
    };
    let prepared = sampler_ksp::bind_modules(scripts, base).unwrap();
    let mut capacity = Limits::for_plan(&prepared, 4, 4);
    capacity.behaviors = behaviors;
    capacity.behavior_cells = prepared.behavior_local_count() * behaviors;
    (Runtime::new(prepared, capacity).unwrap(), views)
}
fn cell(rt: &Runtime, view: &ScriptView, instance: u16, name: &str) -> i64 {
    let Location::Cells { offset, len: 1 } = view
        .setup()
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap()
        .storage
    else {
        panic!("scalar home")
    };
    rt.script_cell(rt.active_plan(), ScriptInstanceId(instance), offset)
        .unwrap()
}
fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}

const ALL: &str = r#"on init
    set_snapshot_type(3)
    declare ui_knob $knob(0,127,1) make_persistent($knob)
    declare $saved := 11 make_persistent($saved)
    declare ~real := 1.5 make_persistent(~real)
    declare %ints[2] := (1,2) make_persistent(%ints)
    declare ?reals[2] := (0.25,0.5) make_persistent(?reals)
    declare @text := "default" make_persistent(@text)
    declare !texts[2] := ("a","b") make_persistent(!texts)
    declare $instrument := 22 make_instr_persistent($instrument)
    declare $ordinary := 33 declare @ordinary_text := "initial"
    declare ui_switch $edit declare $calls declare $pgs_seen declare @picture_seen
    pgs_create_key(SHARED,1) pgs_set_key_val(SHARED,0,7)
    set_control_par(get_ui_id($edit),$CONTROL_PAR_HIDE,0)
    set_control_par_str(get_ui_id($edit),$CONTROL_PAR_PICTURE,"initial.png")
end on
on ui_control($edit)
    $instrument := 222 $ordinary := 333 @ordinary_text := "live"
    set_control_par(get_ui_id($edit),$CONTROL_PAR_HIDE,1)
    set_control_par_str(get_ui_id($edit),$CONTROL_PAR_PICTURE,"live.png")
    pgs_set_key_val(SHARED,0,91)
end on
on persistence_changed
    inc($calls) $pgs_seen := pgs_get_key_val(SHARED,0)
    @picture_seen := get_control_par_str(get_ui_id($edit),$CONTROL_PAR_PICTURE)
    set_control_par_str(get_ui_id($knob),$CONTROL_PAR_LABEL,"snapshot")
end on"#;
fn all_rows() -> Vec<(&'static str, Saved)> {
    vec![
        ("$knob", Saved::Int(87)),
        ("$saved", Saved::Int(42)),
        ("~real", Saved::Real(-2.75)),
        ("%ints", Saved::Ints(vec![13, -7])),
        ("?reals", Saved::Reals(vec![0.125, -0.0])),
        ("@text", Saved::Text("incoming".into())),
        ("!texts", Saved::Texts(vec!["left".into(), "right".into()])),
    ]
}

fn assert_frontend_opaque_refusal(script: Script) {
    let (mut rt, views) = runtime(vec![script], 8, false);
    let plan = rt.active_plan();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let mut overlay = NativeSnapshotOverlay::new(
        fence,
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(9))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut overlay),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            overlay.refusal,
            Some(SnapshotOverlayRefusal::UnsafeCallback)
        );
        assert!(overlay.committed.is_none());
        assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
        assert_eq!(cell(&rt, &views[0], 0, "$saved"), 1);
    });
}

#[test]
fn frontend_ignored_persistence_and_reachable_function_or_warning_refuse() {
    for body in [
        "on persistence_changed set_snapshot_type(3) end on",
        "function opaque set_snapshot_type(3) end function on persistence_changed call opaque end on",
        "on persistence_changed $seen := $NOTE_HELD end on",
    ] {
        let source = format!(
            "on init set_snapshot_type(3) declare $saved := 1 make_persistent($saved) declare $seen end on {body}"
        );
        assert_frontend_opaque_refusal(compile(&source, 0));
    }
}

#[test]
fn unrelated_opaque_function_does_not_taint_clean_persistence() {
    let source = "on init set_snapshot_type(3) declare $saved := 1 make_persistent($saved) declare $seen declare ui_switch $edit end on
        function opaque set_snapshot_type(3) end function
        on ui_control($edit) call opaque end on
        on persistence_changed $seen := $saved end on";
    let script = compile(source, 0);
    assert!(
        script
            .coverage()
            .iter()
            .any(|&(name, coverage, count)| name == "set_snapshot_type"
                && coverage == sampler_ksp::Coverage::Ignored
                && count == 1)
    );
    let (mut rt, views) = runtime(vec![script], 8, false);
    let plan = rt.active_plan();
    let mut overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(9))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    support::without_heap(|| {
        rt.apply_snapshot_overlay(&mut overlay).unwrap();
    });
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 9);
    assert_eq!(cell(&rt, &views[0], 0, "$seen"), 9);
}

#[test]
fn frontend_opaque_listener_body_cannot_prove_a_parked_driver() {
    let source = "on init set_snapshot_type(3) declare $saved := 1 make_persistent($saved) set_listener($NI_SIGNAL_TIMER_MS,1000) end on
        on listener set_snapshot_type(3) end on";
    let (mut rt, views) = runtime(vec![compile(source, 0)], 8, false);
    rt.render(&mut []).unwrap();
    let plan = rt.active_plan();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let mut overlay = NativeSnapshotOverlay::new(
        fence,
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(9))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut overlay),
            Err(Error::NotReady)
        );
        assert_eq!(
            overlay.refusal,
            Some(SnapshotOverlayRefusal::PendingCommands)
        );
        assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
        assert_eq!(cell(&rt, &views[0], 0, "$saved"), 1);
    });
}

#[cfg(feature = "cache")]
#[test]
fn initializer_cache_relowers_frontend_opaque_snapshot_denial() {
    let source = "on init set_snapshot_type(3) declare $saved := 1 make_persistent($saved) end on on persistence_changed set_snapshot_type(3) end on";
    let initialized = sampler_ksp::initialize(
        source,
        sampler_ksp::Limits::LIBRARY,
        &Environment::default(),
    )
    .unwrap();
    let bytes = serde_json::to_vec(&initialized.capture_initialized().unwrap()).unwrap();
    let cached = serde_json::from_slice(&bytes).unwrap();
    let restored =
        sampler_ksp::restore_initialized(source, sampler_ksp::Limits::LIBRARY, cached).unwrap();
    let script = sampler_ksp::compile_initialized(
        source,
        48_000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        restored,
    )
    .unwrap();
    assert_frontend_opaque_refusal(script);
}

#[test]
fn exact_snapshot_mapper_rejects_missing_duplicate_foreign_types_counts_domains_and_limits() {
    let view = compile(ALL, 4).view();
    let expected = snapshot_saved_state(&[view.clone()], &[slot(4, all_rows())], limits()).unwrap();
    assert_eq!(expected.values.len(), 10);
    assert_eq!(expected.texts.len(), 3);
    for (mut rows, name, reason) in [
        (
            {
                let mut rows = all_rows();
                rows.pop();
                rows
            },
            "!texts",
            SnapshotSavedRowError::Missing,
        ),
        (
            {
                let mut rows = all_rows();
                rows.push(("$saved", Saved::Int(2)));
                rows
            },
            "$saved",
            SnapshotSavedRowError::Duplicate,
        ),
        (
            {
                let mut rows = all_rows();
                rows.push(("$instrument", Saved::Int(2)));
                rows
            },
            "$instrument",
            SnapshotSavedRowError::NotSnapshotPersistent,
        ),
        (
            {
                let mut rows = all_rows();
                rows[1].1 = Saved::Real(2.);
                rows
            },
            "$saved",
            SnapshotSavedRowError::Type,
        ),
        (
            {
                let mut rows = all_rows();
                rows[3].1 = Saved::Ints(vec![1]);
                rows
            },
            "%ints",
            SnapshotSavedRowError::Count,
        ),
        (
            {
                let mut rows = all_rows();
                rows[0].1 = Saved::Int(128);
                rows
            },
            "$knob",
            SnapshotSavedRowError::Domain,
        ),
        (
            {
                let mut rows = all_rows();
                rows[1].1 = Saved::Int(i64::MAX);
                rows
            },
            "$saved",
            SnapshotSavedRowError::Domain,
        ),
        (
            {
                let mut rows = all_rows();
                rows[2].1 = Saved::Real(f64::NAN);
                rows
            },
            "~real",
            SnapshotSavedRowError::Domain,
        ),
        (
            {
                let mut rows = all_rows();
                rows[5].1 = Saved::Text("x".repeat(TEXT_CAPACITY + 1));
                rows
            },
            "@text",
            SnapshotSavedRowError::TextCapacity,
        ),
    ] {
        let error = snapshot_saved_state(
            &[view.clone()],
            &[slot(4, std::mem::take(&mut rows))],
            limits(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            SnapshotSavedError::Row {
                slot: 4,
                name: name.into(),
                reason
            }
        );
    }
    assert!(matches!(
        snapshot_saved_state(
            &[view.clone()],
            &[slot(4, all_rows())],
            SnapshotSavedLimits {
                value_entries: 9,
                ..limits()
            }
        ),
        Err(SnapshotSavedError::Capacity)
    ));
    assert!(matches!(
        snapshot_saved_state(
            &[view.clone()],
            &[slot(4, all_rows())],
            SnapshotSavedLimits {
                text_bytes: TEXT_CAPACITY * 3 - 1,
                ..limits()
            }
        ),
        Err(SnapshotSavedError::Capacity)
    ));
    assert!(matches!(
        snapshot_saved_state(
            &[view.clone()],
            &[slot(4, all_rows()), slot(4, vec![])],
            limits()
        ),
        Err(SnapshotSavedError::InvalidSlots)
    ));
    assert!(matches!(
        snapshot_saved_state(&[view], &[slot(0, all_rows())], limits()),
        Err(SnapshotSavedError::InvalidSlots)
    ));
}

#[test]
fn persisted_menu_indices_are_refused_until_the_live_roster_has_an_atomic_fence() {
    let source = "on init declare ui_menu $menu add_menu_item($menu,\"A\",20) add_menu_item($menu,\"B\",80) make_persistent($menu) end on";
    let view = compile(source, 0).view();
    assert_eq!(
        snapshot_saved_state(
            &[view],
            &[slot(0, vec![("$menu", Saved::Int(1))])],
            limits()
        )
        .unwrap_err(),
        SnapshotSavedError::Row {
            slot: 0,
            name: "$menu".into(),
            reason: SnapshotSavedRowError::MenuRosterUnverified,
        }
    );
    let view = compile(
        "on init declare $integer make_persistent($integer) end on",
        0,
    )
    .view();
    let state = snapshot_saved_state(
        &[view],
        &[slot(0, vec![("$integer", Saved::Int(1))])],
        limits(),
    )
    .unwrap();
    assert_eq!(state.values[0].value, ScriptStateValue::Cell(1));
}

#[test]
fn sparse_slot_zero_and_four_keep_live_banks_private_properties_pgs_and_views() {
    let last = "on init declare $last := 5 make_persistent($last) declare $calls declare $pgs_seen end on on persistence_changed inc($calls) $pgs_seen := pgs_get_key_val(SHARED,0) end on";
    let (mut rt, mut views) = runtime(vec![compile(ALL, 0), compile(last, 4)], 16, false);
    let plan = rt.active_plan();
    let runtime_id = rt.id();
    let setup_owner = views[0].setup() as *const _;
    let edit = sampler_ksp::derived_control_id(0, "$edit");
    rt.invoke_control(
        ControlContext {
            performance: rt.performance(0).unwrap(),
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
        ControlWrite {
            id: edit,
            value: ControlValue::Integer(1),
        },
    )
    .unwrap();
    let mut hide_key = None;
    rt.drain_effects(|effect| {
        let view = &mut views[usize::from(effect.instance.unwrap().0)];
        if view.service(effect.service) == Some("set_control_par") {
            hide_key = Some([
                effect.args[0] as i32,
                effect.args[1] as i32,
                sampler_ksp::PROPERTY_TAG,
                sampler_ksp::PROPERTY_TAG,
            ]);
        }
        view.apply_ui_effect(effect);
        true
    });
    let hide_key = hide_key.unwrap();
    assert_eq!(
        rt.script_store(plan, ScriptInstanceId(0), hide_key)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        rt.script_store(plan, ScriptInstanceId(0), [123, 456, 789, 0])
            .unwrap(),
        None
    );
    let calls = [
        cell(&rt, &views[0], 0, "$calls"),
        cell(&rt, &views[1], 1, "$calls"),
    ];
    let state = snapshot_saved_state(
        &views,
        &[
            slot(4, vec![("$last", Saved::Int(77))]),
            slot(1, vec![]),
            slot(0, all_rows()),
        ],
        limits(),
    )
    .unwrap();
    let mut overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        state,
        Box::new([]),
    );
    support::without_heap(|| {
        rt.apply_snapshot_overlay(&mut overlay).unwrap();
    });
    assert!(overlay.committed.is_some());
    assert!(
        overlay
            .state
            .callbacks
            .iter()
            .all(|callback| callback.outcome == Some(Outcome::Finished))
    );
    assert_eq!(rt.active_plan(), plan);
    assert_eq!(rt.id(), runtime_id);
    assert_eq!(views[0].setup() as *const _, setup_owner);
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 42);
    assert_eq!(cell(&rt, &views[0], 0, "$instrument"), 222);
    assert_eq!(cell(&rt, &views[0], 0, "$ordinary"), 333);
    assert_eq!(cell(&rt, &views[1], 1, "$last"), 77);
    for instance in 0..2 {
        assert_eq!(
            cell(&rt, &views[instance], instance as u16, "$calls"),
            calls[instance] + 1
        );
        assert_eq!(
            cell(&rt, &views[instance], instance as u16, "$pgs_seen"),
            91
        );
    }
    assert_eq!(
        rt.script_store(plan, ScriptInstanceId(0), hide_key)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        rt.script_store(plan, ScriptInstanceId(0), [123, 456, 789, 0])
            .unwrap(),
        None
    );
    let ordinary = views[0]
        .setup()
        .fields
        .iter()
        .find(|field| field.name == "@ordinary_text")
        .unwrap();
    let Location::Texts { offset, .. } = ordinary.storage else {
        panic!()
    };
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), offset)
            .unwrap()
            .as_str(),
        "live"
    );
    assert_eq!(
        views[0].model().interface.widgets[1].text("$CONTROL_PAR_PICTURE"),
        Some("live.png")
    );
    let Location::Texts { offset, .. } = views[0]
        .setup()
        .fields
        .iter()
        .find(|field| field.name == "@picture_seen")
        .unwrap()
        .storage
    else {
        panic!()
    };
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), offset)
            .unwrap()
            .as_str(),
        "live.png"
    );
    sampler_ksp::service_ui_effects(&mut rt, &mut views).unwrap();
    assert_eq!(
        views[0].model().interface.widgets[0].text("$CONTROL_PAR_LABEL"),
        Some("snapshot")
    );
    // Private text-property reads still observe the live setting, rather than an
    // initialization cache, after the retained model consumes callback effects.
    assert_eq!(
        views[0].model().interface.widgets[1].text("$CONTROL_PAR_PICTURE"),
        Some("live.png")
    );
    assert_eq!(
        rt.apply_snapshot_overlay(&mut overlay),
        Err(Error::RevisionConflict)
    );
}

#[test]
fn linearized_preset_overrides_snapshot_homes_after_fifo_ui_writes_and_requires_live_epoch() {
    let (mut rt, mut views) = runtime(vec![compile(ALL, 0)], 8, false);
    let plan = rt.active_plan();
    let prepared_fence = rt.snapshot_overlay_fence(plan).unwrap();
    let state = snapshot_saved_state(&views, &[slot(0, all_rows())], limits()).unwrap();
    let sequence = Arc::new(AtomicU64::new(7));
    let mut overlay = NativeSnapshotOverlay::linearized(plan, state, Box::new([]));
    assert_eq!(
        rt.apply_snapshot_overlay(&mut overlay),
        Err(Error::InvalidInput)
    );
    overlay.epochs = vec![SnapshotOverlayEpoch {
        sequence: sequence.clone(),
        expected: 7,
    }]
    .into_boxed_slice();
    let edit = sampler_ksp::derived_control_id(0, "$edit");
    let context = ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    let (mut rt, mut client) = rt.with_control_updates(2, 8199).unwrap();
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::Invoke(
                context,
                ControlWrite {
                    id: edit,
                    value: ControlValue::Integer(1),
                },
            ),
        })
        .unwrap();
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::NativeSnapshotOverlay(overlay),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    assert!(client.reply().unwrap().result.is_ok());
    sampler_ksp::service_ui_effects(&mut rt, &mut views).unwrap();
    assert_ne!(rt.snapshot_overlay_fence(plan).unwrap(), prepared_fence);
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let reply = client.reply().unwrap();
    assert!(reply.result.is_ok());
    let ControlOperation::NativeSnapshotOverlay(overlay) = reply.command.operation else {
        panic!()
    };
    assert!(overlay.committed.is_some());
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 42);
    assert_eq!(cell(&rt, &views[0], 0, "$instrument"), 222);
    assert_eq!(cell(&rt, &views[0], 0, "$ordinary"), 333);
    assert_eq!(cell(&rt, &views[0], 0, "$pgs_seen"), 91);
    sampler_ksp::service_ui_effects(&mut rt, &mut views).unwrap();
    let state = snapshot_saved_state(&views, &[slot(0, all_rows())], limits()).unwrap();
    let mut stale = NativeSnapshotOverlay::linearized(plan, state, Box::new([]));
    stale.epochs = vec![SnapshotOverlayEpoch {
        sequence: sequence.clone(),
        expected: 7,
    }]
    .into_boxed_slice();
    sequence.store(8, Ordering::Release);
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut stale),
            Err(Error::RevisionConflict)
        );
    });
    assert_eq!(stale.refusal, Some(SnapshotOverlayRefusal::StaleEpoch));
    assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
}

#[test]
fn native_scalar_and_snapshot_homes_admit_together_or_remain_wholly_unchanged() {
    const NATIVE: ControlId = ControlId(0x12345);
    let script = compile(
        "on init declare $saved := 1 make_persistent($saved) declare $seen end on on persistence_changed $seen := $saved end on",
        0,
    );
    let view = script.view();
    let plan = script
        .bind(
            Prepared::new(48_000, vec![], vec![], 0)
                .unwrap()
                .with_controls(vec![ControlDefinition {
                    id: NATIVE,
                    domain: ControlDomain::Integer { min: 0, max: 100 },
                    default: ControlValue::Integer(17),
                }])
                .unwrap(),
        )
        .unwrap();
    let capacity = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, capacity).unwrap();
    let plan = rt.active_plan();
    let state = snapshot_saved_state(
        &[view.clone()],
        &[slot(0, vec![("$saved", Saved::Int(42))])],
        limits(),
    )
    .unwrap();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let mut overlay = NativeSnapshotOverlay::new(
        fence,
        state,
        vec![ControlWrite {
            id: NATIVE,
            value: ControlValue::Integer(101),
        }]
        .into_boxed_slice(),
    );
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut overlay),
            Err(Error::InvalidInput)
        );
    });
    assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
    assert_eq!(cell(&rt, &view, 0, "$saved"), 1);
    assert_eq!(
        rt.control_base_value(plan, NATIVE).unwrap(),
        ControlValue::Integer(17)
    );
    assert!(overlay.state.callbacks[0].behavior.is_none());
    overlay.native[0].value = ControlValue::Integer(67);
    support::without_heap(|| {
        rt.apply_snapshot_overlay(&mut overlay).unwrap();
    });
    assert_eq!(cell(&rt, &view, 0, "$saved"), 42);
    assert_eq!(cell(&rt, &view, 0, "$seen"), 42);
    assert_eq!(
        rt.control_base_value(plan, NATIVE).unwrap(),
        ControlValue::Integer(67)
    );
}

#[test]
fn batch_capacity_stale_epoch_bad_native_and_unsafe_reachable_function_refuse_without_writes() {
    let source = "on init declare $saved := 1 make_persistent($saved) declare $calls end on on persistence_changed inc($calls) end on";
    let (mut rt, views) = runtime(vec![compile(source, 0), compile(source, 4)], 1, false);
    let plan = rt.active_plan();
    let state = snapshot_saved_state(
        &views,
        &[
            slot(0, vec![("$saved", Saved::Int(9))]),
            slot(4, vec![("$saved", Saved::Int(8))]),
        ],
        limits(),
    )
    .unwrap();
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let mut overlay = NativeSnapshotOverlay::new(fence, state, Box::new([]));
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut overlay),
            Err(Error::Capacity)
        );
    });
    assert_eq!(overlay.refusal, Some(SnapshotOverlayRefusal::Capacity));
    assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 1);
    assert_eq!(cell(&rt, &views[1], 1, "$saved"), 1);
    assert!(
        overlay
            .state
            .callbacks
            .iter()
            .all(|callback| callback.behavior.is_none())
    );
    let (mut rt, views) = runtime(vec![compile(source, 0)], 1, false);
    let plan = rt.active_plan();
    let make_state = || {
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(9))])],
            limits(),
        )
        .unwrap()
    };
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    let epoch = Arc::new(AtomicU64::new(2));
    let mut overlay = NativeSnapshotOverlay::new(fence, make_state(), Box::new([]));
    overlay.epochs = vec![SnapshotOverlayEpoch {
        sequence: epoch.clone(),
        expected: 1,
    }]
    .into_boxed_slice();
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut overlay),
            Err(Error::RevisionConflict)
        );
    });
    assert_eq!(overlay.refusal, Some(SnapshotOverlayRefusal::StaleEpoch));
    overlay.epochs[0].expected = 2;
    overlay.native = vec![ControlWrite {
        id: ControlId(u128::MAX),
        value: ControlValue::Integer(0),
    }]
    .into_boxed_slice();
    assert_eq!(
        rt.apply_snapshot_overlay(&mut overlay),
        Err(Error::InvalidInput)
    );
    assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
    overlay.native = Box::new([]);
    support::without_heap(|| {
        rt.apply_snapshot_overlay(&mut overlay).unwrap();
    });
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 9);
    epoch.store(3, Ordering::Release);
    let unsafe_source = "on init declare $saved make_persistent($saved) end on function unsafe purge_group(0,0) end function on persistence_changed if ($saved=44) call unsafe end if end on";
    let (mut rt, views) = runtime(vec![compile(unsafe_source, 0)], 4, false);
    let plan = rt.active_plan();
    let mut overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(44))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    assert_eq!(
        rt.apply_snapshot_overlay(&mut overlay),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        overlay.refusal,
        Some(SnapshotOverlayRefusal::UnsafeCallback)
    );
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 0);
}

#[test]
fn admitted_wait_and_fault_are_separate_from_value_commit_and_survive_owned_transport() {
    let source = "on init declare $saved make_persistent($saved) declare $calls declare $done end on on persistence_changed inc($calls) if ($saved=99) wait(1000) end if inc($done) end on";
    let fault = "on init declare $saved make_persistent($saved) end on on persistence_changed if ($saved=99) wait(1-2) end if end on";
    let (rt, views) = runtime(vec![compile(source, 0), compile(fault, 4)], 4, false);
    let (mut rt, mut client) = rt.with_control_updates(4, 8199).unwrap();
    let plan = rt.active_plan();
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::CaptureSnapshotFence(None),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let ControlOperation::CaptureSnapshotFence(Some(fence)) =
        client.reply().unwrap().command.operation
    else {
        panic!()
    };
    let state = snapshot_saved_state(
        &views,
        &[
            slot(0, vec![("$saved", Saved::Int(99))]),
            slot(4, vec![("$saved", Saved::Int(99))]),
        ],
        limits(),
    )
    .unwrap();
    client
        .submit(ControlRequest {
            plan,
            expected_revision: Some(fence.control_revision),
            operation: ControlOperation::NativeSnapshotOverlay(NativeSnapshotOverlay::new(
                fence,
                state,
                Box::new([]),
            )),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let reply = client.reply().unwrap();
    assert!(reply.result.is_ok());
    let ControlOperation::NativeSnapshotOverlay(mut overlay) = reply.command.operation else {
        panic!()
    };
    assert!(overlay.committed.is_some());
    assert_eq!(overlay.refusal, None);
    assert_eq!(overlay.state.callbacks[0].outcome, None);
    assert!(matches!(
        overlay.state.callbacks[1].outcome,
        Some(Outcome::Fault(_))
    ));
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 99);
    assert_eq!(cell(&rt, &views[1], 1, "$saved"), 99);
    support::without_heap(|| {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
    });
    rt.flush_behaviors(|_, _, _| true);
    rt.capture_script_state(plan, &mut overlay.state).unwrap();
    assert_eq!(overlay.state.callbacks[0].outcome, Some(Outcome::Finished));
    assert!(matches!(
        overlay.state.callbacks[1].outcome,
        Some(Outcome::Fault(_))
    ));
}

#[test]
fn active_pcm_and_due_controls_are_not_advanced_by_a_refused_overlay() {
    let source = "on init declare $saved := 1 make_persistent($saved) end on on note if ($saved=0) ignore_event($EVENT_ID) end if end on";
    let (mut rt, views) = runtime(vec![compile(source, 0)], 8, true);
    let plan = rt.active_plan();
    let state = snapshot_saved_state(
        &views,
        &[slot(0, vec![("$saved", Saved::Int(0))])],
        limits(),
    )
    .unwrap();
    let mut overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        state,
        Box::new([]),
    );
    rt.trigger(input(), 60, 1.).unwrap();
    let mut before = [[0.; 2]; 32];
    let mut after = [[0.; 2]; 32];
    rt.render(&mut before).unwrap();
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut overlay),
            Err(Error::NotReady)
        );
        rt.render(&mut after).unwrap();
    });
    assert_eq!(overlay.refusal, Some(SnapshotOverlayRefusal::ActiveEvents));
    assert!(before.iter().chain(after.iter()).all(|frame| frame[0] > 0.));
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 1);
    let (mut fresh, views) = runtime(vec![compile(source, 0)], 8, true);
    let plan = fresh.active_plan();
    let mut overlay = NativeSnapshotOverlay::new(
        fresh.snapshot_overlay_fence(plan).unwrap(),
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(0))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    support::without_heap(|| {
        fresh.apply_snapshot_overlay(&mut overlay).unwrap();
        fresh.trigger(input(), 60, 1.).unwrap();
        fresh.render(&mut after).unwrap();
    });
    assert!(after.iter().all(|frame| *frame == [0.; 2]));

    let source = "on init declare ui_knob $saved(0,127,1) make_persistent($saved) end on";
    let (mut rt, views) = runtime(vec![compile(source, 0)], 4, false);
    let plan = rt.active_plan();
    let id = sampler_ksp::derived_control_id(0, "$saved");
    rt.schedule_event(
        1,
        Event::Control(
            plan,
            ControlWrite {
                id,
                value: ControlValue::Integer(12),
            },
        ),
    )
    .unwrap();
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    assert_eq!(rt.now(), 1);
    assert_eq!(
        rt.control_base_value(plan, id).unwrap(),
        ControlValue::Integer(0)
    );
    let state = snapshot_saved_state(
        &views,
        &[slot(0, vec![("$saved", Saved::Int(99))])],
        limits(),
    )
    .unwrap();
    let overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        state,
        Box::new([]),
    );
    let (mut rt, mut client) = rt.with_control_updates(2, 8199).unwrap();
    client
        .submit(ControlRequest {
            plan,
            expected_revision: None,
            operation: ControlOperation::NativeSnapshotOverlay(overlay),
        })
        .unwrap();
    support::without_heap(|| {
        rt.poll_control_update().unwrap();
    });
    let reply = client.reply().unwrap();
    assert_eq!(reply.result, Err(Error::NotReady));
    let ControlOperation::NativeSnapshotOverlay(overlay) = reply.command.operation else {
        panic!()
    };
    assert_eq!(
        overlay.refusal,
        Some(SnapshotOverlayRefusal::PendingCommands)
    );
    assert_eq!(
        rt.control_base_value(plan, id).unwrap(),
        ControlValue::Integer(0)
    );
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    assert_eq!(
        rt.control_base_value(plan, id).unwrap(),
        ControlValue::Integer(12)
    );
}

#[test]
fn classified_parked_listener_survives_but_unfinished_body_refuses() {
    let source = "on init declare $saved make_persistent($saved) declare $ticks set_listener($NI_SIGNAL_TIMER_MS,1000) end on on listener inc($ticks) end on";
    let (mut rt, views) = runtime(vec![compile(source, 4)], 8, false);
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    let plan = rt.active_plan();
    let mut overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        snapshot_saved_state(
            &views,
            &[slot(4, vec![("$saved", Saved::Int(51))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    support::without_heap(|| {
        rt.apply_snapshot_overlay(&mut overlay).unwrap();
    });
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 51);
    let source = "on init declare $saved make_persistent($saved) set_listener($NI_SIGNAL_TIMER_MS,1000) end on on listener wait(1000000) end on";
    let (mut rt, views) = runtime(vec![compile(source, 0)], 8, false);
    rt.render(&mut vec![[0.; 2]; 48_001]).unwrap();
    let plan = rt.active_plan();
    let mut overlay = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(plan).unwrap(),
        snapshot_saved_state(
            &views,
            &[slot(0, vec![("$saved", Saved::Int(77))])],
            limits(),
        )
        .unwrap(),
        Box::new([]),
    );
    assert_eq!(
        rt.apply_snapshot_overlay(&mut overlay),
        Err(Error::NotReady)
    );
    assert!(matches!(
        overlay.refusal,
        Some(SnapshotOverlayRefusal::PendingCommands | SnapshotOverlayRefusal::ActiveCallbacks)
    ));
    assert_eq!(cell(&rt, &views[0], 0, "$saved"), 0);
}
