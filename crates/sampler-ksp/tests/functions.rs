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
        assert_eq!(rt.note_cell(a, 0), Ok(23));
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
    assert!(compile(&source, 8).unwrap_err().message.contains("budget"));
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
