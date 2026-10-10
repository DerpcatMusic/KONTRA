//! Synthetic source fixtures for KONTRA's runtime-only later-stage policy.
//! All tests are authored NOT_RUN; no Kontakt ordering/timing/ABI assertion.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;

fn prepared(sources: &[&str]) -> Prepared {
    let scripts = sources
        .iter()
        .enumerate()
        .map(|(slot, source)| {
            sampler_ksp::compile_with(
                source,
                48000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                &sampler_ksp::Environment {
                    slot: slot as u8,
                    ..Default::default()
                },
            )
            .unwrap()
        })
        .collect();
    let pcm = [0.5, 0.25].map(|value| Pcm::new(48000, Box::from([[value; 2]; 48000])).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60 + sample as u8,
            key_high: 60 + sample as u8,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    sampler_ksp::bind_modules(
        scripts,
        Prepared::new(48000, pcm.into(), regions, 0).unwrap(),
    )
    .unwrap()
}
fn limits(plan: &Prepared, behaviors: usize) -> Limits {
    let mut limits = Limits::for_plan(plan, 16, 16);
    limits.behaviors = behaviors;
    limits.behavior_cells = plan.behavior_local_count() * behaviors;
    limits.performances = 2;
    limits
}
fn runtime(sources: &[&str], behaviors: usize) -> Runtime {
    let plan = prepared(sources);
    let limits = limits(&plan, behaviors);
    Runtime::new(plan, limits).unwrap()
}
fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Midi1,
        port: 1,
        group: 2,
        channel: 3,
        key,
        external_id: Some(i32::from(key)),
    }
}
fn note(rt: &mut Runtime, key: u8) {
    rt.trigger(input(key), key, 1.).unwrap();
}
fn cell(rt: &Runtime, instance: u16, cell: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(instance), cell)
        .unwrap()
}

#[test]
fn compiled_raw_endpoints_and_distinct_symbolic_callbacks_without_heap() {
    for address in [0, 16383] {
        for value in [0, 16383] {
            let sender = format!(
                "on note ignore_event($EVENT_ID) set_rpn({address},{value}) set_nrpn({address},{value}) end on"
            );
            let receiver = "on init declare $r declare $n declare $a declare $v end on
                on rpn if ($NI_CALLBACK_TYPE = $NI_CB_TYPE_RPN and $NI_CALLBACK_TYPE # $NI_CB_TYPE_NRPN)
                    inc($r) $a := $RPN_ADDRESS $v := $RPN_VALUE end if end on
                on nrpn if ($NI_CALLBACK_TYPE = $NI_CB_TYPE_NRPN and $NI_CALLBACK_TYPE # $NI_CB_TYPE_RPN)
                    inc($n) $a := $RPN_ADDRESS $v := $RPN_VALUE end if end on";
            let mut rt = runtime(&[&sender, receiver], 16);
            support::without_heap(|| {
                note(&mut rt, 60);
                assert_eq!(
                    (
                        cell(&rt, 1, 0),
                        cell(&rt, 1, 1),
                        cell(&rt, 1, 2),
                        cell(&rt, 1, 3)
                    ),
                    (1, 1, address, value)
                );
                assert_eq!(rt.take_fault(), None);
                let mut effects = 0;
                rt.drain_effects(|_| {
                    effects += 1;
                    true
                });
                assert_eq!(effects, 0, "RPN must not escape through the UI outbox");
            });
        }
    }
}

#[test]
fn nested_named_callback_constants_reach_pcm_through_public_compiler() {
    let mut rt = runtime(&[
        "on note ignore_event($EVENT_ID) set_rpn(16383,0) end on",
        "on init declare const $r := $NI_CB_TYPE_RPN declare const $n := $NI_CB_TYPE_NRPN end on
         function inner
            if ($NI_CALLBACK_TYPE = $r and $NI_CALLBACK_TYPE # $n and $RPN_ADDRESS = 16383 and $RPN_VALUE = 0)
                play_note(61,127,0,100000) end if
         end function
         function outer call inner end function
         on rpn call outer end on"], 16);
    support::without_heap(|| {
        note(&mut rt, 60);
        let mut audio = [[0.; 2]; 16];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio[15], [0.25; 2]);
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn payloads_survive_waits_and_other_messages_not_global_host_slots() {
    let mut rt = runtime(
        &[
            "on note ignore_event($EVENT_ID) set_rpn(0,16383) set_nrpn(16383,0) end on",
            "on init declare $ra := -1 declare $rv := -1 declare $na := -1 declare $nv := -1 end on
         on rpn wait(1000) $ra := $RPN_ADDRESS $rv := $RPN_VALUE end on
         on nrpn wait(1000) $na := $RPN_ADDRESS $nv := $RPN_VALUE end on",
        ],
        16,
    );
    support::without_heap(|| {
        note(&mut rt, 60);
        rt.set_host_value(2, 37).unwrap();
        rt.set_host_value(3, 42).unwrap();
        assert_eq!(cell(&rt, 1, 0), -1);
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        assert_eq!(
            (
                cell(&rt, 1, 0),
                cell(&rt, 1, 1),
                cell(&rt, 1, 2),
                cell(&rt, 1, 3)
            ),
            (0, 16383, 16383, 0)
        );
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn same_kind_overlapping_waits_keep_independent_raw_payloads() {
    let mut rt = runtime(&[
        "on note ignore_event($EVENT_ID) set_rpn(0,16383) set_rpn(16383,0) end on",
        "on init declare $first := -1 declare $last := -1 end on
         on rpn wait(1000) if ($RPN_ADDRESS=0) $first:=$RPN_VALUE else $last:=$RPN_VALUE end if end on"],16);
    support::without_heap(|| {
        note(&mut rt, 60);
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        assert_eq!((cell(&rt, 1, 0), cell(&rt, 1, 1)), (16383, 0));
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn later_stage_order_no_self_echo_and_monotonic_rebroadcast() {
    let first = "on init pgs_create_key(LOG,1) declare $calls end on
        on note ignore_event($EVENT_ID) set_rpn(7,9) end on
        on rpn inc($calls) set_rpn($RPN_ADDRESS,$RPN_VALUE) end on";
    let second = "on init declare $calls end on on rpn inc($calls)
        pgs_set_key_val(LOG,0,pgs_get_key_val(LOG,0)*10+1)
        set_nrpn($RPN_ADDRESS,$RPN_VALUE) end on";
    let third = "on init declare $calls declare $log declare $echo end on
        on rpn inc($calls) pgs_set_key_val(LOG,0,pgs_get_key_val(LOG,0)*10+2)
            $log:=pgs_get_key_val(LOG,0) set_rpn(1,2) end on
        on nrpn inc($echo) end on";
    let mut rt = runtime(&[first, second, third], 16);
    support::without_heap(|| {
        note(&mut rt, 60);
        assert_eq!(
            (
                cell(&rt, 0, 0),
                cell(&rt, 1, 0),
                cell(&rt, 2, 0),
                cell(&rt, 2, 1),
                cell(&rt, 2, 2)
            ),
            (0, 1, 1, 12, 1)
        );
        assert_eq!(rt.take_fault(), None);
    });
    // A sender in the middle does not deliver to an earlier receiver.
    let mut rt = runtime(
        &[
            "on init declare $calls end on on rpn inc($calls) end on",
            "on note ignore_event($EVENT_ID) set_rpn(0,0) end on",
        ],
        16,
    );
    note(&mut rt, 60);
    assert_eq!(cell(&rt, 0, 0), 0);
}

#[test]
fn invalid_address_or_value_faults_sender_without_any_delivery() {
    for command in ["set_rpn", "set_nrpn"] {
        for operands in ["-1,0", "16384,0", "0,-1", "0,16384"] {
            let sender = format!("on note ignore_event($EVENT_ID) {command}({operands}) end on");
            let mut rt = runtime(
                &[
                    &sender,
                    "on init declare $calls end on on rpn inc($calls) end on on nrpn inc($calls) end on",
                ],
                16,
            );
            support::without_heap(|| {
                note(&mut rt, 60);
                assert_eq!(
                    rt.take_fault().map(|(_, error)| error),
                    Some(Error::InvalidInput)
                );
                assert_eq!(cell(&rt, 1, 0), 0);
                let mut faulted = false;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Fault(Error::InvalidInput));
                    faulted = true;
                    true
                });
                assert!(faulted);
            });
        }
    }
}

#[test]
fn insufficient_fanout_capacity_is_explicit_and_atomic() {
    let receiver = "on init declare $calls end on on rpn inc($calls) end on";
    let mut rt = runtime(
        &[
            "on note ignore_event($EVENT_ID) set_rpn(0,0) end on",
            receiver,
            receiver,
        ],
        2,
    );
    support::without_heap(|| {
        note(&mut rt, 60);
        assert_eq!(
            rt.take_fault().map(|(_, error)| error),
            Some(Error::Capacity)
        );
        assert_eq!((cell(&rt, 1, 0), cell(&rt, 2, 0)), (0, 0));
    });
}

#[test]
fn repeated_messages_fill_waiting_arena_then_fault_without_unbounded_queue() {
    let mut rt = runtime(
        &[
            "on init declare $i end on on note ignore_event($EVENT_ID)
            while ($i<20) set_rpn($i,$i) inc($i) end while end on",
            "on init declare $calls end on on rpn inc($calls) wait(100000) end on",
        ],
        4,
    );
    support::without_heap(|| {
        note(&mut rt, 60);
        assert_eq!(cell(&rt, 1, 0), 3);
        assert_eq!(
            rt.take_fault().map(|(_, error)| error),
            Some(Error::Capacity)
        );
        rt.panic();
    });
}

#[test]
fn initialization_remains_warn_noop_without_runtime_replay() {
    let script = sampler_ksp::compile(
        "on init set_rpn(0,16383) set_nrpn(16383,0) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    assert!(!script.warnings().is_empty());
    let mut rt = runtime(
        &[
            "on init set_rpn(0,16383) set_nrpn(16383,0) end on",
            "on init declare $calls end on on rpn inc($calls) end on on nrpn inc($calls) end on",
        ],
        16,
    );
    rt.render(&mut [[0.; 2]; 64]).unwrap();
    assert_eq!(cell(&rt, 1, 0), 0);
}

#[test]
fn ordinary_controller_and_numeric_pgs_keep_their_existing_routes() {
    let mut rt = runtime(
        &[
            "on init pgs_create_key(SHARED,1) end on
         on controller ignore_controller set_rpn(12,34) set_controller(7,37)
            pgs_set_key_val(SHARED,0,5) end on",
            "on init declare $rpn declare $cc declare $pgs end on
         on rpn $rpn:=$RPN_VALUE end on
         on controller $cc:=%CC[7] end on
         on pgs_changed $pgs:=pgs_get_key_val(SHARED,0) end on",
        ],
        16,
    );
    support::without_heap(|| {
        rt.dispatch_controller(
            rt.performance(1).unwrap(),
            input(60).channel_address(),
            1 << 3,
            1,
            u32::MAX,
        )
        .unwrap();
        assert_eq!(
            (cell(&rt, 1, 0), cell(&rt, 1, 1), cell(&rt, 1, 2)),
            (34, 37, 5)
        );
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 7), Ok(0));
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn sender_and_receiver_waits_stay_in_retained_generation_and_part() {
    let sources = [
        "on note ignore_event($EVENT_ID) wait(1000) set_rpn(16383,0) end on",
        "on init declare $seen := -1 end on on rpn wait(1000) $seen:=$RPN_ADDRESS end on",
    ];
    let old = prepared(&sources);
    let budget = limits(&old, 16);
    let (mut rt, mut updates) = Runtime::with_plan_updates(old, budget, 2, 1).unwrap();
    let old_id = rt.active_plan();
    let mut other_part = runtime(&sources, 16);
    support::without_heap(|| note(&mut rt, 60));
    let request = updates.submit(Box::new(prepared(&sources))).unwrap();
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        rt.render(&mut [[0.; 2]; 64]).unwrap(); // old sender emits after replacement
        rt.render(&mut [[0.; 2]; 64]).unwrap(); // old receiver resumes after replacement
        assert_eq!(rt.script_cell(old_id, ScriptInstanceId(1), 0), Ok(16383));
        assert_eq!(cell(&rt, 1, 0), -1);
        assert_eq!(cell(&other_part, 1, 0), -1);
        assert_eq!(rt.take_fault(), None);
        rt.panic();
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(
            rt.script_cell(old_id, ScriptInstanceId(1), 0),
            Err(Error::StaleHandle)
        );
        assert_eq!(rt.start_plan_behavior(old_id, 0), Err(Error::StaleHandle));
    });
    // Independent instance state in the new part still receives its own message.
    note(&mut other_part, 60);
    other_part.render(&mut [[0.; 2]; 128]).unwrap();
    assert_eq!(cell(&other_part, 1, 0), 16383);
}

#[test]
fn receiver_generated_note_retains_physical_origin_and_performance() {
    let mut rt = runtime(
        &[
            "on controller ignore_controller set_nrpn(1,2) end on",
            "on init declare $event end on on nrpn $event:=play_note(61,127,0,100000) end on",
        ],
        16,
    );
    let physical = input(60).channel_address();
    // Origin channel 3 is deliberately outside target channel mask 4.
    let domain = rt.performance(1).unwrap();
    support::without_heap(|| {
        rt.dispatch_controller(domain, physical, 1 << 4, 1, u32::MAX)
            .unwrap();
        let alias = cell(&rt, 1, 0) as i32;
        let note = rt
            .resolve_source_event(rt.active_plan(), alias)
            .unwrap()
            .unwrap();
        assert_eq!(rt.note_plan(note), Ok(rt.active_plan()));
        assert_eq!(
            rt.all_sound_off(ChannelAddress {
                channel: 4,
                ..physical
            }),
            Ok(0)
        );
        assert_eq!(rt.all_sound_off(physical), Ok(1));
    });
}

#[test]
fn midi_and_nka_job_ids_wait_completion_and_instances_remain_independent() {
    let mut rt = runtime(
        &[
            include_str!("fixtures/rpn-service-neighbors.ksp"),
            "on init declare $r := -1 declare $n := -1 end on
         on rpn $r:=$RPN_VALUE end on on nrpn $n:=$RPN_VALUE end on",
        ],
        16,
    );
    let plan = rt.active_plan();
    let mut midi_effect = None;
    support::without_heap(|| {
        note(&mut rt, 60);
        assert_eq!(cell(&rt, 1, 0), 16383);
        assert_eq!(cell(&rt, 1, 1), -1);
        rt.drain_effects(|effect| {
            assert!(midi_effect.is_none());
            midi_effect = Some(*effect);
            true
        });
    });
    let midi_effect = midi_effect.unwrap();
    assert_eq!(midi_effect.service, MIDI_SERVICE);
    assert_eq!(midi_effect.instance, Some(ScriptInstanceId(0)));
    assert_eq!(midi_effect.plan, plan);
    let mut midi = MidiCompletion::empty(midi_effect.args[1] as i32, ScriptInstanceId(1));
    let mut nka_effect = None;
    support::without_heap(|| {
        assert_eq!(rt.complete_midi(plan, &mut midi), Err(Error::StaleHandle));
        midi.instance = ScriptInstanceId(0);
        rt.complete_midi(plan, &mut midi).unwrap();
        assert_eq!(cell(&rt, 1, 1), 0);
        rt.drain_effects(|effect| {
            assert!(nka_effect.is_none());
            nka_effect = Some(*effect);
            true
        });
    });
    let nka_effect = nka_effect.unwrap();
    assert_eq!(nka_effect.service, ARRAY_FILE_SERVICE);
    assert_eq!(nka_effect.instance, Some(ScriptInstanceId(0)));
    let mut nka = ArrayFileCompletion::from_effect(&nka_effect).unwrap();
    support::without_heap(|| {
        rt.capture_array_file(plan, &mut nka).unwrap();
        nka.instance = ScriptInstanceId(1);
        assert!(rt.complete_array_file(plan, &mut nka).is_err());
        nka.instance = ScriptInstanceId(0);
        // Own failed completion: never perform file I/O or claim a native NKA receipt.
        nka.success = false;
        rt.complete_array_file(plan, &mut nka).unwrap();
        assert_eq!((cell(&rt, 0, 3), cell(&rt, 0, 4)), (42, 2));
        assert_eq!((cell(&rt, 1, 0), cell(&rt, 1, 1)), (16383, 0));
        assert_eq!(rt.complete_midi(plan, &mut midi), Err(Error::StaleHandle));
        assert_eq!(
            rt.complete_array_file(plan, &mut nka),
            Err(Error::StaleHandle)
        );
        assert_eq!(rt.take_fault(), None);
    });
}

#[test]
fn parameter_binding_rejects_invalid_stage_and_duplicate_kind_and_resets_with_programs() {
    let plan = || prepared(&["on note set_rpn(0,0) end on", "on rpn end on"]);
    let receiver = ParameterProgram {
        kind: ParameterKind::Rpn,
        program: 1,
        stage: 1,
    };
    assert!(matches!(
        plan().with_parameter_programs(vec![ParameterProgram {
            stage: 2,
            ..receiver
        }]),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        plan().with_parameter_programs(vec![receiver, receiver]),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        plan().with_stages(vec![Stage::default()]),
        Err(Error::InvalidInput)
    ));
    assert!(
        plan()
            .with_programs(vec![], None)
            .unwrap()
            .with_stages(vec![])
            .is_ok()
    );
}
