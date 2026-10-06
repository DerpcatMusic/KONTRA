use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    compile_bound(source, &[])
}
fn compile_bound(
    source: &str,
    bindings: &[(&str, ControlId)],
) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 8192,
            instructions: 512,
            variables: 16,
            array_cells: 16,
        },
        bindings,
    )
}
fn plan(source: &str) -> Prepared {
    compile(source)
        .unwrap()
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[1.; 2]; 16])).unwrap()],
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
            .unwrap(),
        )
        .unwrap()
}
fn limits(plan: &Prepared) -> Limits {
    let behavior_cells = plan.behavior_local_count() * 4;
    let note_cells = plan.note_cell_count() * 4;
    Limits {
        notes: 4,
        performances: 2,
        channels: 2,
        families: 4,
        voices: 4,
        expressions: 4,
        decisions: 0,
        commands: 4,
        behaviors: 4,
        behavior_cells,
        behavior_fuel: 512,
        note_cells,
    }
}
fn runtime(source: &str) -> Runtime {
    let plan = plan(source);
    let limits = limits(&plan);
    Runtime::new(plan, limits).unwrap()
}
fn origin() -> ChannelAddress {
    ChannelAddress {
        protocol: Protocol::Midi1,
        port: 1,
        group: 2,
        channel: 3,
    }
}

#[test]
fn source_controller_modules_keep_ordered_cc_views_globals_and_ui_bindings() {
    let first = "on init declare $value := 11 end on
        on controller
            $value := %CC[1]
            ignore_controller
            set_controller(2,%CC[1])
        end on";
    let second = "on init declare $value := 22 declare $seen
            declare ui_button $button
        end on
        on ui_control($button) $value := 77 end on
        on controller
            $value := %CC[1]
            $seen := %CC[2]
            ignore_controller
            wait(125)
            set_controller(3,$seen / 2)
        end on";
    let third = "on init declare $value := 33 end on
        on controller $value := %CC[3] end on";
    let button = ControlId(12);
    let prepared = sampler_ksp::bind_controller_chain(
        vec![
            compile(first).unwrap(),
            compile_bound(second, &[("$button", button)]).unwrap(),
            compile(third).unwrap(),
        ],
        Prepared::new(48000, vec![], vec![], 0).unwrap(),
    )
    .unwrap();
    let budget = limits(&prepared);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let generation = rt.active_plan();
        let domain = rt.performance(1).unwrap();
        rt.dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 0), Ok(127));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(0));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(127));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 0), Ok(33));
        assert_eq!(rt.controller(domain, 3), Ok(0));
        rt.render(&mut [[0.; 2]; 7]).unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 0), Ok(63));
        assert_eq!(
            rt.controller(domain, 3),
            Ok((u64::from(u32::MAX) * 63 / 127) as u32)
        );
        assert_eq!(rt.controller(domain, 1), Ok(0));
        assert_eq!(rt.controller(domain, 2), Ok(0));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.invoke_control(
            control_context(&rt),
            generation,
            None,
            ControlWrite {
                id: button,
                value: ControlValue::Integer(1),
            },
        )
        .unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 0), Ok(127));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(77));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(2), 0), Ok(63));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.pending_commands()),
            (0, 0, 0)
        );
    });
    for source in ["on note end on", "on release end on"] {
        assert!(matches!(
            sampler_ksp::bind_controller_chain(
                vec![compile(first).unwrap(), compile(source).unwrap()],
                Prepared::new(48000, vec![], vec![], 0).unwrap()
            ),
            Err(Error::InvalidInput)
        ));
    }
    assert!(matches!(
        sampler_ksp::bind_controller_chain(
            vec![
                compile_bound(second, &[("$button", button)]).unwrap(),
                compile_bound(second, &[("$button", button)]).unwrap()
            ],
            Prepared::new(48000, vec![], vec![], 0).unwrap()
        ),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        sampler_ksp::bind_controller_chain(
            vec![compile(first).unwrap()],
            Prepared::new(44100, vec![], vec![], 0).unwrap()
        ),
        Err(Error::InvalidInput)
    ));
}

#[test]
fn modules_without_controller_callbacks_keep_their_positions_and_ui_ownership() {
    let button = ControlId(19);
    let prepared = sampler_ksp::bind_controller_chain(
        vec![
            compile("on init declare $value := 11 end on").unwrap(),
            compile("on controller ignore_controller set_controller(2,%CC[1]) end on").unwrap(),
            compile_bound(
                "on init declare $value := 22 declare ui_button $button end on
                on ui_control($button) $value := 77 end on",
                &[("$button", button)],
            )
            .unwrap(),
            compile(
                "on init declare $value end on
                on controller $value := %CC[2] end on",
            )
            .unwrap(),
            compile("on init declare $value := 44 end on").unwrap(),
        ],
        Prepared::new(48000, vec![], vec![], 0).unwrap(),
    )
    .unwrap();
    assert_eq!(prepared.stages().len(), 5);
    assert_eq!(
        prepared
            .stages()
            .iter()
            .map(|s| s.controller.is_some())
            .collect::<Vec<_>>(),
        vec![false, true, false, true, false]
    );
    let budget = limits(&prepared);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let generation = rt.active_plan();
        let domain = rt.performance(1).unwrap();
        rt.dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap();
        rt.invoke_control(
            control_context(&rt),
            generation,
            None,
            ControlWrite {
                id: button,
                value: ControlValue::Integer(1),
            },
        )
        .unwrap();
        for (module, value) in [(0, 11), (2, 77), (3, 127), (4, 44)] {
            assert_eq!(
                rt.script_cell(generation, ScriptInstanceId(module), 0),
                Ok(value)
            );
        }
        assert_eq!(rt.controller(domain, 1), Ok(0));
        assert_eq!(rt.controller(domain, 2), Ok(u32::MAX));
    });
}

#[test]
fn controller_generated_notes_retain_routing_and_assets_without_physical_keys() {
    let source = "on init declare %ids[2] end on
        on controller
            ignore_controller
            wait(125)
            %ids[$CC_NUM - 1] := play_note(60,127,125,0)
        end on";
    for block in [1, 7, 64] {
        let prepared = plan(source);
        let budget = limits(&prepared);
        let (mut rt, mut worker) = Runtime::with_plan_updates(prepared, budget, 2, 1).unwrap();
        let old = rt.active_plan();
        let a = origin();
        let b = ChannelAddress {
            port: 9,
            group: 3,
            channel: 15,
            ..a
        };
        support::without_heap(|| {
            for (index, address) in [a, b].into_iter().enumerate() {
                let domain = rt.performance(index).unwrap();
                rt.set_controller(domain, 11, index as u32 + 20).unwrap();
                rt.dispatch_controller(
                    domain,
                    address,
                    1 << address.channel,
                    index as u8 + 1,
                    u32::MAX,
                )
                .unwrap();
            }
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        });
        worker
            .submit(Box::new(Prepared::new(48000, vec![], vec![], 0).unwrap()))
            .unwrap();
        support::without_heap(|| {
            rt.poll_plan_update().unwrap();
            let mut silence = [[0.; 2]; 6];
            rt.render(&mut silence).unwrap();
            assert_eq!(silence, [[0.; 2]; 6]);
            rt.render(&mut []).unwrap();
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (2, 2, 2)
            );
            for index in 0..2 {
                let alias = rt.script_cell(old, ScriptInstanceId(0), index).unwrap() as i32;
                let note = rt.resolve_source_event(old, alias).unwrap().unwrap();
                assert_eq!(rt.note_plan(note), Ok(old));
                assert_eq!(rt.input_held(note), Ok(false));
                assert_eq!(rt.note_controller(note, 11), Ok(index + 20));
                assert_eq!(rt.resolve_source_event(rt.active_plan(), alias), Ok(None));
            }
            // The copied input address includes port/group/channel, independently
            // of the callback's target mask; scoped hard silence reaches one root.
            assert_eq!(rt.all_sound_off(b), Ok(1));
            let mut audio = [[0.; 2]; 20];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(&audio[..10], &[[1.; 2]; 10]);
            assert_eq!(&audio[10..], &[[0.; 2]; 10]);
            rt.flush_behaviors(|_, owner, outcome| {
                assert_eq!(owner, BehaviorOwner::Plan(old));
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| panic!("generated notes must not send host terminals"));
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.family_count()),
                (0, 0, 0)
            );
            assert_eq!(rt.collect_retired_plans(), 1);
        });
        assert!(worker.retired().is_some());
    }
}

#[test]
fn controller_note_ids_support_fixed_stops_fault_cleanup_and_scoped_wait_cancellation() {
    let mut faulted = runtime(
        "on init declare %values[1] end on
        on controller ignore_controller play_note(60,127,0,125)
            %values[1] := 7
        end on",
    );
    support::without_heap(|| {
        let domain = faulted.performance(0).unwrap();
        let callback = faulted
            .dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap()
            .unwrap();
        assert_eq!(
            faulted.behavior_outcome(callback),
            Ok(Some(Outcome::Fault(Error::InvalidInput)))
        );
        let mut audio = [[0.; 2]; 8];
        faulted.render(&mut audio).unwrap();
        assert_eq!(&audio[..6], &[[1.; 2]; 6]);
        assert_eq!(&audio[6..], &[[0.; 2]; 2]);
        faulted.flush_behaviors(|_, _, _| true);
        faulted.flush_ended(|_| panic!("no host key was admitted"));
        assert_eq!(
            (
                faulted.note_count(),
                faulted.expression_count(),
                faulted.pending_commands()
            ),
            (0, 0, 0)
        );
    });
    let mut rt = runtime(
        "on init declare $id end on
        on controller ignore_controller
            if (%CC[1] = 127)
                $id := play_note(60,127,0,1000)
            else
                note_off($id,0)
            end if
        end on",
    );
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        rt.dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap();
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.pending_commands()),
            (1, 1, 1)
        );
        rt.dispatch_controller(domain, origin(), 1 << 3, 1, 0)
            .unwrap();
        assert_eq!((rt.voice_count(), rt.pending_commands()), (0, 0));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| panic!("no host key was admitted"));
        assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
    });
    // Admission failure leaves a previously stored ID and timer queue unchanged.
    let prepared = plan(
        "on init declare $id := 99 end on
        on controller $id := play_note(60,127,0,1000) end on",
    );
    let mut budget = limits(&prepared);
    budget.notes = 1;
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let held = Input {
            protocol: origin().protocol,
            port: origin().port,
            group: origin().group,
            channel: origin().channel,
            key: 60,
            external_id: Some(3),
        };
        rt.note_on(held, 60, 1.).unwrap();
        let domain = rt.performance(0).unwrap();
        let id = rt
            .dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap()
            .unwrap();
        assert_eq!(
            rt.behavior_outcome(id),
            Ok(Some(Outcome::Fault(Error::Capacity)))
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(99)
        );
        assert_eq!(
            (
                rt.note_count(),
                rt.expression_count(),
                rt.voice_count(),
                rt.pending_commands()
            ),
            (1, 1, 0, 0)
        );
        rt.note_off(held, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
    });
    let mut rt = runtime("on controller ignore_controller wait(125) play_note(60,127,0,0) end on");
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let a = rt
            .dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap()
            .unwrap();
        let other = ChannelAddress {
            port: 9,
            ..origin()
        };
        let b = rt
            .dispatch_controller(domain, other, 1 << 3, 1, u32::MAX)
            .unwrap()
            .unwrap();
        rt.all_sound_off(origin()).unwrap();
        assert_eq!(rt.behavior_outcome(a), Ok(Some(Outcome::Cancelled)));
        assert_eq!(rt.behavior_outcome(b), Ok(None));
        assert_eq!(rt.pending_commands(), 1);
        let mut audio = [[0.; 2]; 32];
        rt.render(&mut audio).unwrap();
        assert_eq!(&audio[6..22], &[[1.; 2]; 16]);
        assert!(
            audio[..6]
                .iter()
                .chain(&audio[22..])
                .all(|frame| *frame == [0.; 2])
        );
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| panic!("no host key was admitted"));
        assert_eq!(
            (
                rt.note_count(),
                rt.expression_count(),
                rt.pending_commands()
            ),
            (0, 0, 0)
        );
    });
}

#[test]
fn source_controllers_consume_remap_and_forward_before_wait_without_reentering() {
    let mut rt = runtime(
        "on init declare $calls declare $number declare $latest end on
      on controller
        inc($calls)
        if ($CC_NUM = 1)
          ignore_controller
          set_controller(7, %CC[$CC_NUM])
        end if
        wait(125)
        $number := $CC_NUM
        $latest := %CC[$CC_NUM]
        ignore_controller
      end on",
    );
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        let a = rt
            .dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap()
            .unwrap();
        let b = rt
            .dispatch_controller(domain, origin(), 1 << 3, 11, 0x80000001)
            .unwrap()
            .unwrap();
        assert_eq!(rt.controller(domain, 1), Ok(0));
        assert_eq!(rt.controller(domain, 7), Ok(u32::MAX));
        // Automatic forwarding keeps MIDI 2 precision, not the source's 7-bit view.
        assert_eq!(rt.controller(domain, 11), Ok(0x80000001));
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(2)
        );
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 7), Ok(0));
        rt.render(&mut [[0.; 2]; 6]).unwrap();
        assert_eq!(rt.behavior_outcome(a), Ok(None));
        rt.render(&mut []).unwrap();
        for id in [a, b] {
            assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
        }
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
            Ok(11)
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
            Ok(64)
        );
        assert_eq!(rt.controller(domain, 11), Ok(0x80000001));
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
}

#[test]
fn remapped_pedals_use_the_shared_gate_while_consumed_pedals_leave_it_alone() {
    for source in [
        "on controller ignore_controller end on",
        "on controller if ($CC_NUM = 1) ignore_controller set_controller(64,%CC[1]) end if end on",
    ] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let domain = rt.performance(0).unwrap();
            let input = Input {
                protocol: origin().protocol,
                port: 1,
                group: 2,
                channel: 3,
                key: 60,
                external_id: Some(7),
            };
            let n = rt.trigger(input, 60, 1.).unwrap();
            let remap = source.contains("set_controller");
            rt.dispatch_controller(
                domain,
                origin(),
                1 << 3,
                if remap { 1 } else { 64 },
                u32::MAX,
            )
            .unwrap();
            rt.key_up(n, None).unwrap();
            assert_eq!(rt.note(n).unwrap().2, remap);
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            if remap {
                rt.dispatch_controller(domain, origin(), 1 << 3, 1, 0)
                    .unwrap();
                assert!(!rt.note(n).unwrap().2);
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    true
                });
            }
            rt.render(&mut [[0.; 2]; 32]).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
        });
    }
    assert!(compile("on note $CC_NUM end on").is_err());
    // v2: operations outside their event context compile like Kontakt, with
    // a warning, and do nothing (or fault at runtime for play_note -1).
    for warned in [
        "on note ignore_controller end on",
        "on controller play_note(60,127,0,-1) end on",
        "on controller ignore_event($EVENT_ID) end on",
        "on init set_controller(1,1) end on",
    ] {
        assert!(!compile(warned).unwrap().warnings().is_empty(), "{warned}");
    }
}

#[test]
fn note_and_release_controller_operations_retain_the_original_domain_across_waits() {
    for block in [1, 7, 64] {
        let mut rt = runtime(
            "on init declare polyphonic $saved end on
          on note
            $saved := %CC[1]
            wait(125)
            set_controller(7,$saved)
            set_controller(11,%CC[1])
          end on
          on release wait(250) set_controller(64,%CC[2]) end on",
        );
        support::without_heap(|| {
            for (index, value) in [(0, 63), (1, 127)] {
                let domain = rt.performance(index).unwrap();
                let address = ChannelAddress {
                    channel: index as u8 + 3,
                    ..origin()
                };
                rt.dispatch_controller(
                    domain,
                    address,
                    1 << address.channel,
                    1,
                    (value * u64::from(u32::MAX) / 127) as u32,
                )
                .unwrap();
                rt.dispatch_controller(domain, address, 1 << address.channel, 2, u32::MAX)
                    .unwrap();
                let note = rt
                    .trigger_in(
                        domain,
                        Input {
                            protocol: Protocol::Midi1,
                            port: 1,
                            group: 2,
                            channel: index as u8 + 3,
                            key: 60,
                            external_id: Some(index as i32),
                        },
                        NotePitch::Key(60),
                        1.,
                        Expression::default(),
                    )
                    .unwrap();
                rt.key_up(note, None).unwrap();
                rt.dispatch_controller(domain, address, 1 << address.channel, 1, 0)
                    .unwrap();
            }
            let mut audio = [[0.; 2]; 13];
            for part in audio.chunks_mut(block) {
                rt.render(part).unwrap();
            }
            for (index, value) in [(0, 63), (1, 127)] {
                let domain = rt.performance(index).unwrap();
                assert_eq!(
                    rt.controller(domain, 7),
                    Ok((value * u64::from(u32::MAX) / 127) as u32)
                );
                assert_eq!(rt.controller(domain, 11), Ok(0));
                assert_eq!(rt.controller(domain, 64), Ok(u32::MAX));
                let channel = rt
                    .register_channel(ChannelAddress {
                        protocol: Protocol::Midi1,
                        port: 1,
                        group: 2,
                        channel: index as u8 + 3,
                    })
                    .unwrap();
                assert_eq!(rt.pedals(channel), Ok((true, false)));
            }
            let mut completed = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                completed += 1;
                true
            });
            assert_eq!(completed, 4);
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
        });
    }
    // v2: $CC_NUM outside on controller reads 0 with a warning.
    let script = compile("on note set_controller(7,$CC_NUM) end on").unwrap();
    assert!(!script.warnings().is_empty());
}

fn control_context(rt: &sampler_core::Runtime) -> sampler_core::ControlContext {
    sampler_core::ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: sampler_core::ChannelAddress {
            protocol: sampler_core::Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    }
}

#[test]
fn ui_callbacks_keep_module_domain_and_origin_through_generated_events_waits_and_plan_replacement()
{
    let button = ControlId(900);
    let prepared = sampler_ksp::bind_modules(
        vec![
            compile(
                "on init declare $calls declare $notes end on
            on controller inc($calls) ignore_controller set_controller(2,%CC[1]) end on
            on note inc($notes) ignore_event($EVENT_ID) end on",
            )
            .unwrap(),
            compile("on init declare $empty end on").unwrap(),
            compile_bound(
                "on init declare $seen declare $event declare ui_button $button end on
            on ui_control($button)
                $seen := %CC[2]
                $event := play_note(60,127,0,500)
                set_controller(3,$seen / 2)
                wait(125)
                set_controller(4,%CC[2])
            end on",
                &[("$button", button)],
            )
            .unwrap(),
            compile(
                "on init declare $notes declare $released declare $last end on
            on note inc($notes) change_note($EVENT_ID,61) end on
            on release inc($released) end on
            on controller $last := $CC_NUM end on",
            )
            .unwrap(),
        ],
        Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 61,
                key_high: 61,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap(),
    )
    .unwrap();
    let mut budget = limits(&prepared);
    budget.behaviors = 16;
    budget.behavior_cells = prepared.behavior_local_count() * 16;
    budget.commands = 16;
    let (mut rt, mut transfer) = Runtime::with_plan_updates(prepared, budget, 2, 1).unwrap();
    let old = rt.active_plan();
    let domain = rt.performance(1).unwrap();
    let mut callback = None;
    support::without_heap(|| {
        rt.dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
            .unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        callback = rt
            .invoke_control(
                ControlContext {
                    performance: domain,
                    origin: origin(),
                    channels: (1 << 3) | (1 << 4),
                },
                old,
                Some(0),
                ControlWrite {
                    id: button,
                    value: ControlValue::Integer(1),
                },
            )
            .unwrap()
            .1;
        assert_eq!(rt.behavior_outcome(callback.unwrap()), Ok(None));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(0), 1), Ok(0));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(2), 0), Ok(127));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(3), 0), Ok(1));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(3), 2), Ok(3));
        let alias = rt.script_cell(old, ScriptInstanceId(2), 1).unwrap() as i32;
        let note = rt.resolve_source_event(old, alias).unwrap().unwrap();
        assert_eq!(rt.input_held(note), Ok(false));
        assert_eq!(rt.note_plan(note), Ok(old));
        assert_eq!(rt.note_pitch(note), Ok(NotePitch::Key(61)));
        assert_eq!(rt.note_controller(note, 2), Ok(u32::MAX));
        assert_eq!(
            rt.note_controller(note, 3),
            Ok(0),
            "onset snapshot predates UI-generated CC"
        );
        assert_eq!(
            rt.controller(domain, 3),
            Ok((u64::from(u32::MAX) * 63 / 127) as u32)
        );
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 3), Ok(0));
        assert_eq!(
            rt.all_sound_off(ChannelAddress {
                channel: 4,
                ..origin()
            }),
            Ok(0)
        );
        assert_eq!(rt.voice_count(), 1);
    });
    transfer
        .submit(Box::new(Prepared::new(48000, vec![], vec![], 0).unwrap()))
        .unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        let mut audio = [[0.; 2]; 32];
        for chunk in audio.chunks_mut(7) {
            rt.render(chunk).unwrap();
        }
        assert_eq!(&audio[..24], &[[0.25; 2]; 24]);
        assert_eq!(&audio[24..], &[[0.; 2]; 8]);
        assert_eq!(rt.controller(domain, 4), Ok(u32::MAX));
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 4), Ok(0));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(3), 1), Ok(1));
        assert_eq!(rt.script_cell(old, ScriptInstanceId(3), 2), Ok(4));
        assert_eq!(
            rt.behavior_outcome(callback.unwrap()),
            Ok(Some(Outcome::Finished))
        );
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| panic!("UI-generated notes have no host terminal"));
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.expression_count()),
            (0, 0, 0)
        );
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(transfer.retired().unwrap());
}

#[test]
fn dynamic_durations_in_shared_functions_use_the_real_note_ui_or_controller_context() {
    let button = ControlId(901);
    let prepared = |duration: i32| {
        let source = format!(
            "on init declare $duration := {duration} declare $event declare ui_button $fire end on
            function spawn
                $event := play_note(60,127,0,$duration)
            end function
            on note ignore_event($EVENT_ID) call spawn end on
            on ui_control($fire) call spawn end on
            on controller ignore_controller call spawn end on"
        );
        compile_bound(&source, &[("$fire", button)])
            .unwrap()
            .bind(plan("on init end on"))
            .unwrap()
    };
    for duration in [0, 125, -1, -2] {
        for controller in [false, true] {
            let prepared = prepared(duration);
            let budget = limits(&prepared);
            let mut rt = Runtime::new(prepared, budget).unwrap();
            support::without_heap(|| {
                let domain = rt.performance(1).unwrap();
                let callback = if controller {
                    rt.dispatch_controller(domain, origin(), 1 << 3, 1, u32::MAX)
                        .unwrap()
                        .unwrap()
                } else {
                    rt.invoke_control(
                        ControlContext {
                            performance: domain,
                            origin: origin(),
                            channels: 1 << 3,
                        },
                        rt.active_plan(),
                        None,
                        ControlWrite {
                            id: button,
                            value: ControlValue::Integer(1),
                        },
                    )
                    .unwrap()
                    .1
                    .unwrap()
                };
                let mut audio = [[0.; 2]; 32];
                rt.render(&mut audio).unwrap();
                if duration < 0 {
                    assert_eq!(
                        rt.behavior_outcome(callback),
                        Ok(Some(Outcome::Fault(Error::InvalidInput)))
                    );
                    assert_eq!(audio, [[0.; 2]; 32]);
                    assert_eq!(
                        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
                        Ok(0)
                    );
                } else {
                    assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
                    let length = if duration == 0 { 16 } else { 6 };
                    assert!(audio[..length].iter().all(|f| *f == [1.; 2]));
                    assert!(audio[length..].iter().all(|f| *f == [0.; 2]));
                }
                rt.flush_behaviors(|_, _, _| true);
                rt.flush_ended(|_| panic!("generated source has no host note"));
                assert_eq!(
                    (rt.note_count(), rt.voice_count(), rt.expression_count()),
                    (0, 0, 0)
                );
            });
        }
    }
    let prepared = prepared(-1);
    let budget = limits(&prepared);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let note = rt
            .trigger(
                Input {
                    protocol: Protocol::Clap,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(1),
                },
                60,
                1.,
            )
            .unwrap();
        let mut audio = [[0.; 2]; 3];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[1.; 2]; 3]);
        rt.key_up(note, None).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 3]);
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
    // v2: a constant parent-gate duration outside a note callback warns at
    // compile time and faults at runtime before publishing a note.
    let script = compile_bound(
        "on init declare ui_button $fire end on
        on ui_control($fire) play_note(60,127,0,-1) end on",
        &[("$fire", button)],
    )
    .unwrap();
    assert!(!script.warnings().is_empty());
}

#[test]
fn cc_touched_marks_the_controller_of_this_callback() {
    let source = "on init declare $touched declare $other declare $none end on
        on controller
            $touched := %CC_TOUCHED[$CC_NUM]
            $other := %CC_TOUCHED[$CC_NUM + 1]
            $none := search(%CC_TOUCHED, 1)
        end on";
    let prepared = sampler_ksp::bind_controller_chain(
        vec![compile(source).unwrap()],
        Prepared::new(48000, vec![], vec![], 0).unwrap(),
    )
    .unwrap();
    let budget = limits(&prepared);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    let generation = rt.active_plan();
    let domain = rt.performance(1).unwrap();
    rt.dispatch_controller(domain, origin(), 1 << 3, 7, u32::MAX)
        .unwrap();
    let cell = |rt: &Runtime, i| rt.script_cell(generation, ScriptInstanceId(0), i);
    assert_eq!(
        (cell(&rt, 0), cell(&rt, 1), cell(&rt, 2)),
        (Ok(1), Ok(0), Ok(7))
    );
}
