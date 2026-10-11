use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn compile(source: &str, instructions: usize) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    let controls: &[(&str, ControlId)] = if source.contains("ui_button") {
        &[("$button", ControlId(7))]
    } else {
        &[]
    };
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions,
            variables: 16,
            array_cells: 16,
        },
        controls,
    )
}
fn runtime(source: &str) -> Runtime {
    let plan = compile(source, 4096)
        .unwrap()
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[1.; 2]; 32])).unwrap()],
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
        .unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    let note_cells = plan.note_cell_count() * 4;
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            voices: 4,
            expressions: 4,
            decisions: 0,
            commands: 8,
            behaviors: 8,
            behavior_fuel: 1024,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap()
}
fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(i32::from(key)),
    }
}

#[test]
fn shared_function_waits_inherit_each_callback_and_return_without_an_extra_forward() {
    for block in [1, 7, 64] {
        let mut rt = runtime(
            "on init declare $calls declare ui_button $button end on
          function tick() inc($calls) wait(125) inc($calls) end function
          function twice call tick() inc($calls) end function
          on note call twice end on
          on release call twice() end on
          on controller call twice end on
          on ui_control($button) call twice end on",
        );
        support::without_heap(|| {
            let note = rt.trigger(input(60), 60, 1.).unwrap();
            assert_eq!(rt.voice_count(), 1, "wait in a function forwards its note");
            rt.key_up(note, None).unwrap();
            let domain = rt.performance(0).unwrap();
            rt.dispatch_controller(domain, input(60).channel_address(), 1, 1, 0x12345678)
                .unwrap();
            rt.invoke_control(
                control_context(&rt),
                rt.active_plan(),
                None,
                ControlWrite {
                    id: ControlId(7),
                    value: ControlValue::Integer(1),
                },
            )
            .unwrap();
            assert_eq!(rt.controller(domain, 1), Ok(0x12345678));
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                Ok(4)
            );
            let mut audio = [[0.; 2]; 7];
            for part in audio.chunks_mut(block) {
                rt.render(part).unwrap();
            }
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                Ok(12)
            );
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
    let mut rt = runtime(
        "function transpose change_note($EVENT_ID,61) end function
        on note call transpose change_note($EVENT_ID,60) end on",
    );
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(
            audio, [[1.; 2]; 1],
            "function return must not forward an intermediate key"
        );
    });
}

#[test]
fn nested_function_branches_loops_and_exit_keep_polyphonic_state_and_caller_targets() {
    let mut rt = runtime(
        "on init declare polyphonic $n declare $calls end on
      function tick inc($calls) end function
      function work
        while ($n < 3)
          inc($n)
          if ($n = 1) continue end if
          call tick
        end while
        select($EVENT_NOTE)
        case 60 $n := $n + 10
        case 61 $n := $n + 20
        end select
      end function
      function stop if ($EVENT_NOTE = 60) exit end if end function
      on note
        call work()
        call work
        call stop
        $n := $n + 100
      end on",
    );
    support::without_heap(|| {
        let a = rt.trigger(input(60), 60, 1.).unwrap();
        let b = rt.trigger(input(61), 61, 1.).unwrap();
        assert_eq!(rt.note_cell(a, 0), Ok(123));
        assert_eq!(rt.note_cell(b, 0), Ok(143));
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(4)
        );
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn invalid_functions_and_expansion_pressure_fail_during_compilation() {
    for source in [
        "function recurse call recurse end function on note end on",
        "function same end function function same end function on note end on",
        "function takes(1) end function on note end on",
        "function empty end function on note call empty(1) end on",
        "function $bad end function on note end on",
        "function dead unsupported_command end function on note end on",
        "function dead if (0) call missing end if end function on note end on",
        "function wrong if (1) end function on note end on",
        "function stray continue end function on note while(1) call stray end while end on",
    ] {
        assert!(compile(source, 4096).is_err(), "{source}");
    }
    // v2: functions resolve regardless of order; context-only operations warn.
    for (source, warns) in [
        (
            "on note call later end on function later end function",
            false,
        ),
        (
            "function first call second end function function second end function on note end on",
            false,
        ),
        (
            "function cc ignore_controller end function on note call cc end on",
            true,
        ),
        (
            "function event change_note($EVENT_ID,61) end function on release call event end on",
            false,
        ),
    ] {
        let script = compile(source, 4096).unwrap();
        assert_eq!(!script.warnings().is_empty(), warns, "{source}");
    }
    let mut source = String::from("on init declare $a end on function f0 inc($a) end function ");
    for i in 1..12 {
        source.push_str(&format!(
            "function f{i} call f{} call f{} end function ",
            i - 1,
            i - 1
        ));
    }
    source.push_str("on note call f11 end on");
    // v2: functions are subroutines, so doubling call trees no longer expand.
    assert!(compile(&source, 128).is_ok());
    assert!(
        compile(&source, 8)
            .err()
            .unwrap()
            .message
            .contains("budget")
    );
    let source = "on init declare $a end on function increment inc($a) end function on note call increment call increment end on";
    assert!(compile(source, 4).is_err());
    assert!(compile(source, 64).is_ok());
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
fn function_exit_init_resumes_nested_and_repeated_callers_but_callback_exit_stops() {
    let rt = runtime(
        "on init
            declare $leaf declare $parent declare $caller declare $dead
            call parent call parent inc($caller) exit inc($dead)
         end on
         function leaf
            while (1)
                select(1) case 1 inc($leaf) exit inc($dead) end select
                inc($dead)
            end while
            inc($dead)
         end function
         function parent call leaf inc($parent) exit inc($dead) end function",
    );
    for (cell, expected) in [2, 2, 1, 0].into_iter().enumerate() {
        assert_eq!(rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell as u32), Ok(expected));
    }
    let rt = runtime(
        "on init declare $leaf declare $caller declare $dead end on
         function leaf inc($leaf) exit inc($dead) end function
         on persistence_changed call leaf call leaf inc($caller) exit inc($dead) end on",
    );
    for (cell, expected) in [2, 1, 0].into_iter().enumerate() {
        assert_eq!(rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell as u32), Ok(expected));
    }
}

#[test]
fn function_exit_runtime_resumes_callers_without_forwarding_intermediate_pitch() {
    let mut rt = runtime(
        "on init declare $leaf declare $parent declare $caller declare $dead end on
         function leaf
            while (1)
                select($EVENT_NOTE)
                case 60 change_note($EVENT_ID,61) inc($leaf) exit inc($dead)
                end select
                inc($dead)
            end while
         end function
         function parent call leaf change_note($EVENT_ID,60) inc($parent) exit inc($dead) end function
         on note call parent call parent inc($caller) exit inc($dead) end on",
    );
    support::without_heap(|| {
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        for (cell, expected) in [2, 2, 1, 0].into_iter().enumerate() {
            assert_eq!(rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell as u32), Ok(expected));
        }
        assert_eq!(rt.note_pitch(note), Ok(NotePitch::Key(60)));
        assert_eq!((rt.voice_count(), rt.family_count()), (1, 1));
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[1.; 2]; 1], "only the caller's final pitch reaches selection");
        let mut finished = 0;
        rt.flush_behaviors(|_, owner, outcome| {
            assert_eq!((owner, outcome), (BehaviorOwner::Note(note), Outcome::Finished));
            finished += 1;
            true
        });
        assert_eq!(finished, 1);
    });
}

// Synthetic two-stage plans make actual downstream delivery observable. No
// private program inspection or emitted-Forward count stands in for delivery.
fn function_exit_plan(sources: &[&str]) -> Prepared {
    let samples = [0.25, 0.5].map(|value| {
        Pcm::new(48000, Box::from([[value; 2]; 1024])).unwrap()
    });
    let regions = (0..2).map(|sample| Region {
        sample,
        key_low: 0,
        key_high: 127,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }).collect();
    sampler_ksp::bind_modules(
        sources.iter().map(|source| compile(source, 4096).unwrap()).collect(),
        Prepared::new(48000, samples.into(), regions, 256).unwrap()
            .with_groups(2, vec![Some(0), Some(1)]).unwrap()
            .with_releases(
                vec![Trigger::Attack, Trigger::GateRelease],
                ReleaseOptions::default(),
                ReleaseOptions::default(),
            ).unwrap(),
    ).unwrap()
}

#[test]
fn function_and_callback_exit_deliver_note_release_controller_once_and_stop_plan_tail() {
    let plan = function_exit_plan(&[
        "on init declare $entered declare $after declare $dead declare ui_button $button end on
         function leaf inc($entered) exit inc($dead) end function
         on note call leaf inc($after) exit inc($dead) end on
         on release call leaf inc($after) exit inc($dead) end on
         on controller call leaf inc($after) exit inc($dead) end on
         on ui_control($button) call leaf inc($after) exit inc($dead) end on",
        "on init declare $notes declare $releases declare $controllers end on
         on note inc($notes) end on
         on release inc($releases) end on
         on controller inc($controllers) end on",
    ]);
    let limits = Limits::for_plan(&plan, 4, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        let generation = rt.active_plan();
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.25; 2]; 1]);
        assert_eq!(rt.voice_count(), 1);
        rt.key_up(note, None).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.5; 2]; 1], "one release layer, not duplicated or omitted");
        let domain = rt.performance(0).unwrap();
        rt.dispatch_controller(domain, input(60).channel_address(), 1, 1, 0x12345678).unwrap();
        assert_eq!(rt.controller(domain, 1), Ok(0x12345678));
        rt.invoke_control(control_context(&rt), generation, None, ControlWrite {
            id: ControlId(7), value: ControlValue::Integer(1),
        }).unwrap();
        for (cell, expected) in [4, 4, 0].into_iter().enumerate() {
            assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), cell as u32), Ok(expected));
        }
        for cell in 0..3 {
            assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), cell), Ok(1));
        }
        let mut finished = 0;
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            finished += 1;
            true
        });
        assert_eq!(finished, 7, "four originating callbacks and three downstream stages");
    });
}

#[test]
fn function_exit_after_wait_preserves_wait_time_stage_delivery_without_duplicates() {
    let plan = function_exit_plan(&[
        "on init declare $entered declare $returned declare $caller declare $dead end on
         function leaf
            inc($entered) change_note($EVENT_ID,61) wait(125)
            change_note($EVENT_ID,60) exit inc($dead)
         end function
         function parent call leaf inc($returned) exit inc($dead) end function
         on note call parent call parent inc($caller) end on",
        "on init declare $seen declare $calls end on
         on note $seen := $EVENT_NOTE inc($calls) end on",
    ]);
    let limits = Limits::for_plan(&plan, 4, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        let generation = rt.active_plan();
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        assert_eq!(rt.voice_count(), 1, "the first intentional wait still forwards immediately");
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(61));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 1), Ok(0));
        rt.render(&mut [[0.; 2]; 7]).unwrap();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 1), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 2), Ok(0));
        let mut audio = [[0.; 2]; 7];
        rt.render(&mut audio).unwrap();
        assert!(audio.iter().all(|frame| *frame == [0.25; 2]));
        for (cell, expected) in [2, 2, 1, 0].into_iter().enumerate() {
            assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), cell as u32), Ok(expected));
        }
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(1));
        assert_eq!(rt.note_event_at(note, 0).unwrap().unwrap().pitch, NotePitch::Key(60));
        assert_eq!(rt.note_event_at(note, 1).unwrap().unwrap().pitch, NotePitch::Key(61));
        assert_eq!((rt.voice_count(), rt.family_count()), (1, 1));
        let mut finished = 0;
        rt.flush_behaviors(|_, owner, outcome| {
            assert_eq!((owner, outcome), (BehaviorOwner::Note(note), Outcome::Finished));
            finished += 1;
            true
        });
        assert_eq!(finished, 2);
    });
}

#[test]
fn function_exit_returns_are_not_deferred_note_flush_barriers() {
    let plan = function_exit_plan(&[
        "on init declare $child declare $after declare $dead end on
         function leaf $child := play_note(64,127,0,100000) exit inc($dead) end function
         on note
            ignore_event($EVENT_ID)
            disallow_group($ALL_GROUPS)
            call leaf
            inc($after)
            change_note($child,60)
            set_event_par_arr($child,$EVENT_PAR_ALLOW_GROUP,1,0)
         end on",
        "on init declare $seen declare $calls end on
         on note $seen := $EVENT_NOTE inc($calls) end on",
    ]);
    let limits = Limits::for_plan(&plan, 4, 8);
    let mut rt = Runtime::new(plan, limits).unwrap();
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        let generation = rt.active_plan();
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 1), Ok(1));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(0), 2), Ok(0));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 0), Ok(64));
        assert_eq!(rt.script_cell(generation, ScriptInstanceId(1), 1), Ok(1));
        let alias = rt.script_cell(generation, ScriptInstanceId(0), 0).unwrap() as i32;
        let child = rt.resolve_source_event(generation, alias).unwrap().unwrap();
        assert_eq!(rt.note_event_at(child, 0).unwrap().unwrap().pitch, NotePitch::Key(60));
        assert_eq!(rt.note_event_at(child, 1).unwrap().unwrap().pitch, NotePitch::Key(64));
        assert!(rt.note_group_allowed_at(child, 1, 0).unwrap());
        assert!(!rt.note_group_allowed_at(child, 1, 1).unwrap());
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.25; 2]; 1]);
        assert_eq!(rt.voice_count(), 1);
    });
}

#[test]
fn function_exit_frames_survive_each_pc_fuel_boundary_wait_and_plan_switch_by_instance() {
    const SOURCE: &str = "on init declare $leaf declare $parent declare $caller declare $dead end on
        function leaf inc($leaf) wait(125) exit inc($dead) end function
        function parent call leaf inc($parent) exit inc($dead) end function
        on note call parent call parent inc($caller) end on";
    const SENTINEL: &str = "on init
        declare $leaf := 100 declare $parent := 200 declare $caller := 300 declare $dead := 400
        end on on note end on";
    for fuel in [1, 2, 3, 1024] {
        let old = function_exit_plan(&[SOURCE, SOURCE]);
        let new = function_exit_plan(&[SENTINEL, SENTINEL]);
        let limits = Limits::for_plan(&old, 4, 8);
        let (mut rt, mut control) = Runtime::with_plan_updates(old, limits, 2, 1).unwrap();
        let original = rt.active_plan();
        let request = control.submit(Box::new(new)).unwrap();
        rt.set_behavior_block_fuel(fuel);
        support::without_heap(|| {
            let note = rt.trigger(input(60), 60, 1.).unwrap();
            rt.render(&mut [[0.; 2]; 1]).unwrap();
            assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
            let active = rt.active_plan();
            assert_ne!(active, original);
            assert_eq!(rt.note_plan(note), Ok(original));
            let mut finished = 0;
            for _ in 0..512 {
                rt.render(&mut [[0.; 2]; 1]).unwrap();
                rt.flush_behaviors(|_, owner, outcome| {
                    assert_eq!((owner, outcome), (BehaviorOwner::Note(note), Outcome::Finished));
                    finished += 1;
                    true
                });
                if finished == 2 { break; }
            }
            assert_eq!(finished, 2, "both original instance callbacks must return");
            for instance in [ScriptInstanceId(0), ScriptInstanceId(1)] {
                for (cell, expected) in [2, 2, 1, 0].into_iter().enumerate() {
                    assert_eq!(rt.script_cell(original, instance, cell as u32), Ok(expected));
                }
                for (cell, expected) in [100, 200, 300, 400].into_iter().enumerate() {
                    assert_eq!(rt.script_cell(active, instance, cell as u32), Ok(expected));
                }
            }
            assert_eq!((rt.voice_count(), rt.family_count()), (1, 1));
            assert_eq!(rt.note_plan(note), Ok(original));
            assert!(rt.now() >= 12, "both intentional waits must retain their timing");
            if fuel == 1 {
                assert!(rt.preemptions() > 10, "one instruction per block separates every executed PC");
            }
        });
    }
}

#[test]
fn function_exit_never_hides_invalid_unreachable_source() {
    for source in [
        "on init declare $n call bad end on function bad exit $n := \"wrong\" end function",
        "on init declare $n end on function bad exit $n := \"wrong\" end function on note call bad end on",
        "function bad exit unsupported_command end function on note call bad end on",
        "function bad exit call missing end function on note call bad end on",
        "function bad exit if (1) end function on note call bad end on",
        "on note exit unsupported_command end on",
    ] {
        assert!(compile(source, 4096).is_err(), "{source}");
    }
}
