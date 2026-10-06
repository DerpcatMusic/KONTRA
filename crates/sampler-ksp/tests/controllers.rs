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
    for invalid in [
        "on note ignore_controller end on",
        "on controller play_note(60,127,0,-1) end on",
        "on controller ignore_event($EVENT_ID) end on",
        "on init set_controller(1,1) end on",
        "on note $CC_NUM end on",
    ] {
        assert!(compile(invalid).is_err(), "{invalid}");
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
    assert!(compile("on note set_controller(7,$CC_NUM) end on").is_err());
}
