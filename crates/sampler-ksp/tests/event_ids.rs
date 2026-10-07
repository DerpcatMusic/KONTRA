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
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        bindings,
    )
}
fn runtime(source: &str) -> Runtime {
    runtime_script(compile(source).unwrap())
}
fn runtime_script(script: sampler_ksp::Script) -> Runtime {
    let note_cells = script.note_cells() * 12;
    let plan = script
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[1.; 2]; 16])).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 61,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    let behavior_cells = plan.behavior_local_count() * 16;
    Runtime::new(
        plan,
        Limits {
            notes: 12,
            channels: 0,
            performances: 1,
            expressions: 12,
            families: 16,
            voices: 16,
            decisions: 0,
            commands: 16,
            // Generated notes also reserve their creating module's release.
            behaviors: 16,
            behavior_fuel: 1024,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap()
}
fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(i32::from(key)),
    }
}

#[test]
fn stored_pitch_and_velocity_targets_keep_audio_pairing_and_reused_slots_isolated() {
    let source = "on init declare %ids[1] end on
        on note
            if (%ids[0] = 0)
                %ids[0] := $EVENT_ID
            else
                change_note(%ids[0] + 0,62)
                change_velo(%ids[0],64)
            end if
        end on";
    for retired in [false, true] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let first = rt.trigger(input(60), 60, 1.).unwrap();
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            if retired {
                rt.note_off(input(60), None).unwrap();
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            }
            let second = rt.trigger(input(61), 61, 1.).unwrap();
            assert_eq!(
                rt.note_event(second).unwrap(),
                NoteProperties {
                    pitch: NotePitch::Key(61),
                    velocity: 1.
                }
            );
            if !retired {
                assert_eq!(
                    rt.note_event(first).unwrap(),
                    NoteProperties {
                        pitch: NotePitch::Key(62),
                        velocity: 64. / 127.
                    }
                );
                assert_eq!(
                    rt.initial_note_properties(first).unwrap(),
                    NoteProperties {
                        pitch: NotePitch::Key(60),
                        velocity: 1.
                    }
                );
                assert!(rt.input_held(first).unwrap());
            }
            let mut audio = [[0.; 2]; 8];
            rt.render(&mut audio).unwrap();
            // The stored edit updates source-visible properties of an already
            // running event. Neither old audio nor the new event is remapped.
            assert_eq!(audio, [[if retired { 1. } else { 2. }; 2]; 8]);
            if !retired {
                assert_eq!(rt.note_off(input(60), None), Ok(first));
            }
            assert_eq!(rt.note_off(input(61), None), Ok(second));
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.expression_count()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn generated_ids_in_arrays_and_expressions_keep_each_original_release_owner() {
    let source = "on init declare %ids[8] declare $duration := 0 declare polyphonic $slot
        declare polyphonic $mine end on
      on note
        $mine := 1
        $slot := ($EVENT_NOTE - 60) * 4
        %ids[$slot] := $EVENT_ID
        ignore_event($EVENT_ID)
        %ids[$slot + 1] := play_note(60,127,0,-1)
        %ids[$slot + 2] := play_note(60,127,0,$duration)
        %ids[$slot + 3] := 7 + play_note(60,127,0,125) - 7
      end on
      on release
        { The generated notes run this too, with their own (unset) polyphonics. }
        if ($mine = 1)
          %ids[$slot] := $EVENT_ID
        end if
      end on";
    for block in [1, 7, 64] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let a = rt.trigger(input(60), 60, 1.).unwrap();
            let b = rt.trigger(input(61), 61, 1.).unwrap();
            let plan = rt.active_plan();
            let ids: [i32; 8] = std::array::from_fn(|cell| {
                rt.script_cell(plan, ScriptInstanceId(0), cell as u32)
                    .unwrap() as i32
            });
            for (index, &id) in ids.iter().enumerate() {
                assert!(id > 0 && id <= 0x0fff_ffff);
                assert!(!ids[..index].contains(&id));
                assert!(rt.resolve_source_event(plan, id).unwrap().is_some());
            }
            assert_eq!(rt.resolve_source_event(plan, ids[0]), Ok(Some(a)));
            assert_eq!(rt.resolve_source_event(plan, ids[4]), Ok(Some(b)));
            let mut audio = [[0.; 2]; 8];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio[..6], [[6.; 2]; 6]);
            assert_eq!(audio[6..], [[4.; 2]; 2]);
            rt.key_up(a, None).unwrap();
            rt.key_up(b, None).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio, [[2.; 2]; 8]);
            assert_eq!(
                rt.script_cell(plan, ScriptInstanceId(0), 0),
                Ok(i64::from(ids[0]))
            );
            assert_eq!(
                rt.script_cell(plan, ScriptInstanceId(0), 4),
                Ok(i64::from(ids[4]))
            );
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            for id in ids {
                assert_eq!(rt.resolve_source_event(plan, id), Ok(None));
            }
            assert_eq!(rt.note_count(), 0);
            let replacement = rt.trigger(input(60), 60, 1.).unwrap();
            assert!(rt.source_event_id(replacement).unwrap() > *ids.iter().max().unwrap());
        });
    }
}
#[test]
fn event_expressions_remain_bounded_and_short_circuit_does_not_generate_skipped_notes() {
    let source = "on init declare $value end on on note ignore_event($EVENT_ID)
        $value := 1 or play_note(60,127,0,0)
        $value := 0 and play_note(60,127,0,0)
      end on";
    let mut rt = runtime(source);
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        assert_eq!(rt.note_count(), 1);
        assert_eq!(rt.voice_count(), 0);
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
    for source in [
        "on init declare const $ID := $EVENT_ID end on",
        "on note if (1 or play_note(60,127,$missing,0)) exit end if end on",
    ] {
        assert!(compile(source).is_err());
    }
    // v2: event operations in `on init` warn and do nothing.
    let script = compile("on init declare $ID := play_note(60,127,0,0) end on").unwrap();
    assert!(!script.warnings().is_empty());
    let nested = format!(
        "on note {}0{} end on",
        "play_note(60,127,0,".repeat(300),
        ")".repeat(300)
    );
    assert!(compile(&nested).is_err());
}

#[test]
fn stored_note_off_replaces_durations_and_runs_original_release_once_at_exact_frames() {
    let source = "on init declare %ids[3] declare $released end on
      on note
        ignore_event($EVENT_ID)
        %ids[0] := play_note(60,127,0,125)
        %ids[1] := play_note(60,127,0,0)
        %ids[2] := play_note(60,127,0,-1)
        note_off(%ids[0])
        note_off(%ids[1],83)
        note_off(%ids[2],0)
        wait(21)
        note_off(%ids[0],125)
        note_off($EVENT_ID)
      end on
      on release inc($released) note_off($EVENT_ID) end on";
    for block in [1, 7, 64] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let original = rt.trigger(input(60), 60, 1.).unwrap();
            // A queued physical key-up is not a generated fixed duration.
            rt.schedule_event(10, Event::KeyUp(original, None)).unwrap();
            let mut audio = [[0.; 2]; 12];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio[..4], [[2.; 2]; 4]);
            assert_eq!(audio[4..8], [[1.; 2]; 4]);
            assert_eq!(audio[8..], [[0.; 2]; 4]);
            assert_eq!(rt.release_context(original).unwrap().key.unwrap().at, 2);
            // The original's release runs once; as in Kontakt, each of the
            // three generated notes also runs this script's release callback.
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 3),
                Ok(4)
            );
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
        });
    }
}

#[test]
fn ui_can_stop_stored_events_and_stale_ids_cannot_stop_a_reused_slot() {
    let source = "on init declare $id declare $released declare ui_button $stop end on
      on note $id := $EVENT_ID end on
      on release inc($released) end on
      on ui_control($stop) note_off($id,0) end on";
    let mut rt = runtime_script(compile_bound(source, &[("$stop", ControlId(17))]).unwrap());
    support::without_heap(|| {
        let plan = rt.active_plan();
        let original = rt.trigger(input(60), 60, 1.).unwrap();
        rt.invoke_control(
            control_context(&rt),
            plan,
            None,
            ControlWrite {
                id: ControlId(17),
                value: ControlValue::Integer(1),
            },
        )
        .unwrap();
        assert!(!rt.key_down(original).unwrap());
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        assert!(rt.input_held(original).unwrap());
        assert_eq!(rt.note_off(input(60), None), Ok(original));
        rt.flush_ended(|_| true);
        let fresh = rt.note_on(input(60), 60, 1.).unwrap();
        rt.invoke_control(
            control_context(&rt),
            plan,
            None,
            ControlWrite {
                id: ControlId(17),
                value: ControlValue::Integer(0),
            },
        )
        .unwrap();
        assert!(rt.key_down(fresh).unwrap());
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(1));
        rt.panic();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
    assert!(compile("on note note_off(1,0,1) end on").is_err());
    // v2: init-time and multi-event note_off compile with a warning, no effect.
    for warned in [
        "on init note_off(1) end on",
        "on note note_off($ALL_EVENTS) end on",
        "on note note_off(by_marks(1)) end on",
    ] {
        assert!(!compile(warned).unwrap().warnings().is_empty(), "{warned}");
    }
    let mut rt = runtime("on note note_off($EVENT_ID,-1) end on");
    let original = rt.trigger(input(60), 60, 1.).unwrap();
    assert_eq!(rt.pending_commands(), 0);
    assert_eq!(
        rt.release_context(original).unwrap().key.unwrap().cause,
        ReleaseCause::BehaviorFault
    );
    rt.flush_behaviors(|_, _, outcome| {
        assert_eq!(outcome, Outcome::Fault(Error::InvalidInput));
        true
    });
}

#[test]
fn nested_release_dispatch_preserves_side_effect_order_and_wait_resume_boundaries() {
    let source = "on init declare %ids[2] declare $order end on
      on note %ids[$EVENT_NOTE - 60] := $EVENT_ID end on
      on release
        if ($EVENT_NOTE = 60)
          note_off(%ids[1])
          $order := $order * 10 + 1
        else
          $order := $order * 10 + 2
          wait(125)
          $order := $order * 10 + 3
        end if
      end on";
    let mut rt = runtime(source);
    support::without_heap(|| {
        let a = rt.trigger(input(60), 60, 1.).unwrap();
        let b = rt.trigger(input(61), 61, 1.).unwrap();
        rt.key_up(a, None).unwrap();
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
            Ok(21)
        );
        assert!(rt.release_context(b).unwrap().gate.is_some());
        assert!(rt.input_held(b).unwrap());
        rt.render(&mut [[0.; 2]; 6]).unwrap();
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
            Ok(21)
        );
        rt.render(&mut []).unwrap();
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
            Ok(213)
        );
        rt.key_up(b, None).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
    let mut rt = runtime(
        "on release ignore_event($EVENT_ID) note_off($EVENT_ID) play_note(60,127,0,-1) end on",
    );
    support::without_heap(|| {
        let n = rt.trigger(input(60), 60, 1.).unwrap();
        rt.key_up(n, None).unwrap();
        assert_eq!(rt.voice_count(), 0);
        rt.flush_behaviors(|_, _, outcome| {
            // Kontakt ignores a gate-linked note played on an ended gate.
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn stopping_a_pending_attack_finishes_without_sound_or_consuming_host_input() {
    for stop in ["note_off($EVENT_ID)", "note_off($EVENT_ID,0)"] {
        let mut rt = runtime(&format!(
            "on init declare $releases end on
             on note {stop} end on
             on release inc($releases) end on"
        ));
        support::without_heap(|| {
            let n = rt.trigger(input(60), 60, 1.).unwrap();
            assert!(rt.input_held(n).unwrap());
            assert!(!rt.key_down(n).unwrap());
            assert_eq!(rt.forward_attack(n), Ok(false));
            let mut audio = [[1.; 2]; 16];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[0.; 2]; 16]);
            assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
            let mut callbacks = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                callbacks += 1;
                true
            });
            assert_eq!(callbacks, 2);
            rt.flush_ended(|_| panic!("raw input still owns its terminal"));
            rt.key_up(n, None).unwrap();
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                Ok(1)
            );
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
        });
    }
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
fn a_release_the_script_asked_for_cannot_be_ignored_by_its_own_release_callback() {
    // Una Corda: note_off of its linked child runs the child's release
    // callback, which ignores the release as it does for host key-ups.
    let source = "on init declare $c end on
      on note
        ignore_event($EVENT_ID)
        $c := play_note(60,127,0,-1)
        note_off($c)
      end on
      on release if ($EVENT_ID = $c) ignore_event($EVENT_ID) end if end on";
    let mut rt = runtime(source);
    let host = rt.trigger(input(60), 60, 1.).unwrap();
    rt.release(host).unwrap();
    let mut audio = [[0.; 2]; 64];
    rt.render(&mut audio).unwrap();
    rt.flush_behaviors(|_, _, _| true);
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn play_note_outside_the_midi_ranges_is_ignored_and_returns_minus_one() {
    // Vista plays its "no key held" sentinel -1.
    let source = "on init declare $a declare $b declare $c end on
      on note
        $a := play_note(-1,100,0,0)
        $b := play_note(60,0,0,0)
        $c := play_note(128,100,0,-1)
      end on";
    let mut rt = runtime(source);
    rt.trigger(input(60), 60, 1.).unwrap();
    let mut audio = [[0.; 2]; 4];
    rt.render(&mut audio).unwrap();
    for cell in 0..3 {
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell),
            Ok(-1)
        );
    }
    rt.flush_behaviors(|_, _, outcome| {
        assert_eq!(outcome, Outcome::Finished);
        true
    });
}
