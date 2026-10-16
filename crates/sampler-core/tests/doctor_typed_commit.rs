//! Authored typed row transactions. No native files or installed banks.
use sampler_core::*;
mod support;
const MODE: ControlId = ControlId(200);
fn basic_runtime() -> Runtime {
    let prepared = Prepared::new(48_000, vec![], vec![], 0)
        .unwrap()
        .with_controls(vec![ControlDefinition {
            id: MODE,
            domain: ControlDomain::Integer { min: 0, max: 100 },
            default: ControlValue::Integer(3),
        }])
        .unwrap();
    let limits = Limits::for_plan(&prepared, 4, 4);
    Runtime::new(prepared, limits).unwrap()
}
fn overlay(rt: &Runtime, value: i64) -> NativeSnapshotOverlay {
    NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(rt.active_plan()).unwrap(),
        ScriptStateBuffer::default(),
        Box::from([ControlWrite {
            id: MODE,
            value: ControlValue::Integer(value),
        }]),
    )
}
fn submit(client: &mut ControlClient, incoming: NativeSnapshotOverlay) -> u64 {
    client
        .submit(ControlRequest {
            plan: incoming.expected.plan,
            expected_revision: None,
            operation: ControlOperation::NativeSnapshotOverlay(incoming),
        })
        .unwrap()
}
const REAL: ControlId = ControlId(201);
const TOGGLE: ControlId = ControlId(202);
const MENU: ControlId = ControlId(203);
const OTHER_MENU: ControlId = ControlId(204);
// Preserve a real-cell NaN payload as exact integer bits.
const CELL_BITS: i64 = 0x7ff8_0000_0000_0123;

fn typed_prepared() -> Prepared {
    let callback = Program::new(vec![
        Instruction::ReadControl {
            control: MODE,
            local: 0,
        },
        Instruction::ReadControl {
            control: MENU,
            local: 1,
        },
        Instruction::ReadScriptCell { cell: 0, local: 2 },
        Instruction::ReadScriptCell { cell: 1, local: 3 },
        Instruction::Op(Op::TextIndex {
            text: TextRef::Cell(0),
            local: 4,
        }),
        // Mutate the live menu roster only AFTER observing every committed home.
        Instruction::SetLocal {
            local: 5,
            value: 10,
        },
        Instruction::SetLocal { local: 6, value: 2 },
        Instruction::SetLocal { local: 7, value: 1 },
        Instruction::SetLocal { local: 8, value: 0 },
        Instruction::SetLocal {
            local: 9,
            value: 55,
        },
        Instruction::Op(Op::Store {
            key: 5,
            local: 9,
            write: true,
        }),
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback);
    Prepared::new(48_000, vec![], vec![], 0)
        .unwrap()
        .with_controls(vec![
            ControlDefinition {
                id: MODE,
                domain: ControlDomain::Integer { min: 0, max: 100 },
                default: ControlValue::Integer(3),
            },
            ControlDefinition {
                id: REAL,
                domain: ControlDomain::Real { min: -1., max: 1. },
                default: ControlValue::Real(0.25),
            },
            ControlDefinition {
                id: TOGGLE,
                domain: ControlDomain::Toggle,
                default: ControlValue::Toggle(false),
            },
            ControlDefinition {
                id: MENU,
                domain: ControlDomain::Integer { min: 0, max: 100 },
                default: ControlValue::Integer(4),
            },
            ControlDefinition {
                id: OTHER_MENU,
                domain: ControlDomain::Integer { min: 0, max: 100 },
                default: ControlValue::Integer(5),
            },
        ])
        .unwrap()
        .with_script_instances(vec![vec![7, 8]])
        .unwrap()
        .with_script_resources(vec![ScriptResources {
            texts: vec!["before".into()],
            store: vec![
                ([10, 1, -1, 0], 2),
                ([10, 2, 0, 0], 21),
                ([10, 2, 1, 0], 47),
                ([11, 1, -1, 0], 1),
                ([11, 2, 0, 0], 61),
            ],
            store_capacity: 5,
            ..Default::default()
        }])
        .unwrap()
        .with_programs(vec![callback], None)
        .unwrap()
        .with_widgets(
            [(MENU, 10), (OTHER_MENU, 11)]
                .into_iter()
                .map(|(control, ui_id)| WidgetDefinition {
                    id: control,
                    source_slot: 0,
                    ui_id,
                    instance: ScriptInstanceId(0),
                    storage: WidgetStorage::Control(control),
                    drop: None,
                    menu: Some(WidgetMenuStorage {
                        count_key: [ui_id, 1, -1, 0],
                        value_key: [ui_id, 2, 0, 0],
                    }),
                    program: None,
                    stage: 0,
                })
                .collect(),
        )
        .unwrap()
}

fn typed_runtime() -> Runtime {
    let prepared = typed_prepared();
    let limits = Limits {
        behaviors: 2,
        behavior_cells: 2 * prepared.behavior_local_count(),
        ..Limits::for_plan(&prepared, 4, 0)
    };
    Runtime::new(prepared, limits).unwrap()
}

fn typed_overlay(rt: &Runtime, callback: bool) -> NativeSnapshotOverlay {
    let mut state = ScriptStateBuffer::default();
    state.texts.push(Text::new("after"));
    state.values = vec![
        ScriptStateEntry {
            address: ScriptStateAddress::Control(MODE),
            value: ScriptStateValue::Control(ControlValue::Integer(99)),
        },
        ScriptStateEntry {
            address: ScriptStateAddress::Control(TOGGLE),
            value: ScriptStateValue::Control(ControlValue::Toggle(true)),
        },
        ScriptStateEntry {
            address: ScriptStateAddress::Cell {
                instance: ScriptInstanceId(0),
                index: 0,
            },
            value: ScriptStateValue::Cell(CELL_BITS),
        },
        ScriptStateEntry {
            address: ScriptStateAddress::Cell {
                instance: ScriptInstanceId(0),
                index: 1,
            },
            value: ScriptStateValue::Cell(42),
        },
        ScriptStateEntry {
            address: ScriptStateAddress::Text {
                instance: ScriptInstanceId(0),
                index: 0,
            },
            value: ScriptStateValue::Text(0),
        },
    ];
    if callback {
        state.callbacks.push(ScriptStateCallback {
            program: 0,
            behavior: None,
            outcome: None,
        });
    }
    let mut incoming = NativeSnapshotOverlay::new(
        rt.snapshot_overlay_fence(rt.active_plan()).unwrap(),
        state,
        Box::from([ControlWrite {
            id: REAL,
            value: ControlValue::Real(-0.),
        }]),
    );
    incoming.menus = Box::from([
        SnapshotMenuPosition::new(MENU, 1),
        SnapshotMenuPosition::new(OTHER_MENU, 0),
    ]);
    incoming
}

fn assert_typed_initial(rt: &Runtime) {
    let plan = rt.active_plan();
    for (control, value) in [
        (MODE, ControlValue::Integer(3)),
        (REAL, ControlValue::Real(0.25)),
        (TOGGLE, ControlValue::Toggle(false)),
        (MENU, ControlValue::Integer(4)),
        (OTHER_MENU, ControlValue::Integer(5)),
    ] {
        assert_eq!(rt.control_base_value(plan, control), Ok(value));
        assert_eq!(rt.control_value(plan, control), Ok(value));
    }
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(7));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(8));
    assert_eq!(
        rt.script_text(plan, ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "before"
    );
}

#[test]
fn typed_overlay_commits_all_homes_before_callback_and_refreshes_live_menu_values() {
    let mut rt = typed_runtime();
    let plan = rt.active_plan();
    let mut incoming = typed_overlay(&rt, true);
    support::without_heap(|| {
        assert_eq!(rt.apply_snapshot_overlay(&mut incoming), Ok((8, 1)));
        let callback = &incoming.state.callbacks[0];
        assert_eq!(callback.outcome, Some(Outcome::Finished));
        let id = callback.behavior.unwrap();
        for (local, value) in [99, 47, CELL_BITS, 42, i64::from(name_index("after"))]
            .into_iter()
            .enumerate()
        {
            assert_eq!(rt.behavior_local(id, local as u16), Ok(value));
        }
        assert_eq!(
            rt.control_base_value(plan, TOGGLE),
            Ok(ControlValue::Toggle(true))
        );
        let ControlValue::Real(value) = rt.control_base_value(plan, REAL).unwrap() else {
            panic!()
        };
        assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
        assert_eq!(rt.control_value(plan, MENU), Ok(ControlValue::Integer(47)));
        assert_eq!(
            rt.control_value(plan, OTHER_MENU),
            Ok(ControlValue::Integer(61))
        );
        assert_eq!(
            rt.script_text(plan, ScriptInstanceId(0), 0)
                .unwrap()
                .as_str(),
            "after"
        );
        assert_eq!(incoming.committed.unwrap().control_revision, 1);
        rt.flush_behaviors(|_, _, _| true);
    });
    // The earlier callback replaced the current ordinal's semantic value.
    let mut next = typed_overlay(&rt, false);
    support::without_heap(|| {
        assert_eq!(rt.apply_snapshot_overlay(&mut next), Ok((8, 2)));
        assert_eq!(rt.control_value(plan, MENU), Ok(ControlValue::Integer(55)));
    });
}

#[test]
fn typed_overlay_late_malformed_rows_leave_every_home_revision_and_callback_untouched() {
    for malformed in 0..7 {
        let mut rt = typed_runtime();
        let plan = rt.active_plan();
        let fence = rt.snapshot_overlay_fence(plan).unwrap();
        let mut incoming = typed_overlay(&rt, true);
        match malformed {
            0 => incoming.state.values[4].value = ScriptStateValue::Cell(9),
            1 => incoming.state.values[4].value = ScriptStateValue::Text(1),
            2 => {
                incoming.state.values[4].address = ScriptStateAddress::Text {
                    instance: ScriptInstanceId(1),
                    index: 0,
                }
            }
            3 => incoming.state.texts[0] = Text::new(&"x".repeat(TEXT_CAPACITY + 1)),
            4 => incoming.native[0].value = ControlValue::Integer(1),
            5 => incoming.menus[1].position = 1,
            6 => incoming.state.callbacks[0].program = usize::MAX,
            _ => panic!(),
        }
        let raw = incoming.state.values.clone();
        support::without_heap(|| {
            assert_eq!(
                rt.apply_snapshot_overlay_with_gate(&mut incoming, || {
                    panic!("late malformed batch cannot reach host gate")
                }),
                Err(Error::InvalidInput)
            );
            assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::InvalidInput));
            assert!(incoming.committed.is_none());
            assert_eq!(incoming.state.values, raw);
            assert_eq!(incoming.state.callbacks[0].behavior, None);
            assert_eq!(incoming.state.callbacks[0].outcome, None);
            assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
            assert_typed_initial(&rt);
        });
        // Repair and retry the refused producer object; no commit token survived refusal.
        let repaired = typed_overlay(&rt, true);
        incoming.state.values = repaired.state.values;
        incoming.state.texts = repaired.state.texts;
        incoming.state.callbacks = repaired.state.callbacks;
        incoming.native = repaired.native;
        incoming.menus = repaired.menus;
        support::without_heap(|| {
            assert_eq!(rt.apply_snapshot_overlay(&mut incoming), Ok((8, 1)));
            assert_eq!(incoming.state.callbacks[0].outcome, Some(Outcome::Finished));
            assert_eq!(rt.control_value(plan, MENU), Ok(ControlValue::Integer(47)));
        });
    }
}

#[test]
fn typed_overlay_stale_revision_and_epoch_reject_before_any_writes() {
    use std::sync::{Arc, atomic::AtomicU64};
    for stale_epoch in [false, true] {
        let mut rt = typed_runtime();
        let fence = rt.snapshot_overlay_fence(rt.active_plan()).unwrap();
        let mut incoming = typed_overlay(&rt, true);
        let refusal = if stale_epoch {
            incoming.epochs = Box::from([SnapshotOverlayEpoch {
                sequence: Arc::new(AtomicU64::new(2)),
                expected: 1,
            }]);
            SnapshotOverlayRefusal::StaleEpoch
        } else {
            incoming.expected.script_revision += 1;
            SnapshotOverlayRefusal::RevisionConflict
        };
        support::without_heap(|| {
            assert_eq!(
                rt.apply_snapshot_overlay(&mut incoming),
                Err(Error::RevisionConflict)
            );
            assert_eq!(incoming.refusal, Some(refusal));
            assert!(incoming.state.callbacks[0].behavior.is_none());
            assert_eq!(rt.snapshot_overlay_fence(rt.active_plan()).unwrap(), fence);
            assert_typed_initial(&rt);
        });
    }
}

#[test]
fn typed_overlay_fifo_preserves_preceding_edits_strict_conflict_and_linearized_recall() {
    use std::sync::{Arc, atomic::AtomicU64};
    let rt = basic_runtime();
    let strict = overlay(&rt, 99);
    let plan = rt.active_plan();
    let mut linearized = NativeSnapshotOverlay::linearized(
        plan,
        ScriptStateBuffer::default(),
        Box::from([ControlWrite {
            id: MODE,
            value: ControlValue::Integer(88),
        }]),
    );
    linearized.epochs = Box::from([SnapshotOverlayEpoch {
        sequence: Arc::new(AtomicU64::new(4)),
        expected: 4,
    }]);
    let (mut rt, mut client) = rt.with_control_updates(3, 8).unwrap();
    let first = client
        .submit(ControlRequest {
            plan,
            expected_revision: Some(0),
            operation: ControlOperation::Edit(Box::from([ControlWrite {
                id: MODE,
                value: ControlValue::Integer(7),
            }])),
        })
        .unwrap();
    let second = submit(&mut client, strict);
    let third = submit(&mut client, linearized);
    let mut gate_calls = 0;
    support::without_heap(|| {
        for (request, value) in [(first, 7), (second, 7), (third, 88)] {
            assert_eq!(
                rt.poll_control_update_with_snapshot_gate(|| {
                    gate_calls += 1;
                    true
                }),
                Ok(Some(request))
            );
            assert_eq!(
                rt.control_value(plan, MODE),
                Ok(ControlValue::Integer(value))
            );
        }
        assert_eq!(gate_calls, 1);
        assert_eq!(rt.control_revision(plan), Ok(2));
    });
    let first = client.reply().unwrap();
    let second = client.reply().unwrap();
    let third = client.reply().unwrap();
    assert_eq!(first.result, Ok((1, 1)));
    assert_eq!(second.result, Err(Error::RevisionConflict));
    assert_eq!(third.result, Ok((1, 2)));
    let ControlOperation::NativeSnapshotOverlay(incoming) = third.command.operation else {
        panic!()
    };
    assert_eq!(incoming.expected.control_revision, 1);
    assert_eq!(incoming.committed.unwrap().control_revision, 2);
}

#[test]
fn typed_overlay_full_bound_and_oversize_rejection_do_not_allocate_or_free() {
    let prepared = Prepared::new(48_000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![7; SNAPSHOT_OVERLAY_VALUE_CAPACITY]])
        .unwrap();
    let limits = Limits::for_plan(&prepared, 4, 0);
    let mut rt = Runtime::new(prepared, limits).unwrap();
    let plan = rt.active_plan();
    let mut producer = None;
    let requested_bytes = support::allocated_bytes(|| {
        producer = Some(NativeSnapshotOverlay::new(
            rt.snapshot_overlay_fence(plan).unwrap(),
            ScriptStateBuffer::default(),
            Box::new([]),
        ));
    });
    assert_eq!(
        requested_bytes, 0,
        "an empty payload has no operation-scratch allocation"
    );
    let mut incoming = producer.unwrap();
    // Public rows may be filled AFTER construction, as product menus are today.
    incoming.state.values = (0..SNAPSHOT_OVERLAY_VALUE_CAPACITY)
        .map(|index| ScriptStateEntry {
            address: ScriptStateAddress::Cell {
                instance: ScriptInstanceId(0),
                index: index as u32,
            },
            value: ScriptStateValue::Cell(index as i64),
        })
        .collect();
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut incoming),
            Ok((SNAPSHOT_OVERLAY_VALUE_CAPACITY, 1))
        );
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 8191), Ok(8191));
    });
    incoming.expected = rt.snapshot_overlay_fence(plan).unwrap();
    incoming.committed = None;
    incoming.state.values.push(ScriptStateEntry {
        address: ScriptStateAddress::Cell {
            instance: ScriptInstanceId(0),
            index: 8192,
        },
        value: ScriptStateValue::Cell(99),
    });
    let fence = rt.snapshot_overlay_fence(plan).unwrap();
    support::without_heap(|| {
        assert_eq!(
            rt.apply_snapshot_overlay(&mut incoming),
            Err(Error::Capacity)
        );
        assert_eq!(incoming.refusal, Some(SnapshotOverlayRefusal::Capacity));
        assert_eq!(rt.snapshot_overlay_fence(plan).unwrap(), fence);
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 8191), Ok(8191));
    });
}

#[test]
fn ordinary_restore_checks_every_variant_pair_before_callback_or_value_mutation() {
    let addresses = [
        ScriptStateAddress::Control(MODE),
        ScriptStateAddress::Cell {
            instance: ScriptInstanceId(0),
            index: 0,
        },
        ScriptStateAddress::Text {
            instance: ScriptInstanceId(0),
            index: 0,
        },
    ];
    let values = [
        ScriptStateValue::Control(ControlValue::Integer(99)),
        ScriptStateValue::Cell(CELL_BITS),
        ScriptStateValue::Text(0),
    ];
    for (address_kind, address) in addresses.into_iter().enumerate() {
        for (value_kind, value) in values.into_iter().enumerate() {
            if address_kind == value_kind {
                continue;
            }
            let mut rt = typed_runtime();
            let plan = rt.active_plan();
            let revision = rt.script_state_revision(plan).unwrap();
            let mut state = ScriptStateBuffer {
                values: vec![ScriptStateEntry { address, value }],
                texts: vec![Text::new("after")],
                callbacks: vec![ScriptStateCallback {
                    program: 0,
                    behavior: None,
                    outcome: None,
                }],
            };
            support::without_heap(|| {
                assert_eq!(
                    rt.restore_script_state(plan, Some(revision.0), &mut state),
                    Err(Error::InvalidInput)
                );
                assert_eq!(rt.script_state_revision(plan).unwrap(), revision);
                assert_eq!(state.callbacks[0].behavior, None);
                assert_eq!(state.callbacks[0].outcome, None);
                assert_typed_initial(&rt);
            });
        }
    }
}

#[test]
fn ordinary_restore_late_rejection_preserves_prefix_and_valid_retry_observes_all_homes() {
    let mut rt = typed_runtime();
    let plan = rt.active_plan();
    let revision = rt.control_revision(plan).unwrap();
    let mut state = typed_overlay(&rt, true).state;
    state.values[4].value = ScriptStateValue::Text(99);
    support::without_heap(|| {
        assert_eq!(
            rt.restore_script_state(plan, Some(revision), &mut state),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.control_revision(plan).unwrap(), revision);
        assert_eq!(state.callbacks[0].behavior, None);
        assert_typed_initial(&rt);
    });
    state.values[4].value = ScriptStateValue::Text(0);
    support::without_heap(|| {
        assert_eq!(
            rt.restore_script_state(plan, Some(revision), &mut state),
            Ok((5, revision + 1))
        );
        let id = state.callbacks[0].behavior.unwrap();
        assert_eq!(state.callbacks[0].outcome, Some(Outcome::Finished));
        for (local, expected) in [99, 4, CELL_BITS, 42, i64::from(name_index("after"))]
            .into_iter()
            .enumerate()
        {
            assert_eq!(rt.behavior_local(id, local as u16), Ok(expected));
        }
        assert_eq!(
            rt.control_value(plan, TOGGLE),
            Ok(ControlValue::Toggle(true))
        );
    });
}
