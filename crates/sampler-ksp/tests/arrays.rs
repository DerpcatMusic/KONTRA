use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
fn compile(source: &str, array_cells: usize) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 64,
            array_cells,
        },
        &[],
    )
}
fn runtime(source: &str, array_cells: usize) -> Runtime {
    let script = compile(source, array_cells).unwrap();
    let note_cells = script.note_cells() * 4;
    let prepared = script
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[1.; 2]; 16])).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 0,
                    key_high: 127,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                128,
            )
            .unwrap(),
        )
        .unwrap();
    let behavior_cells = prepared.behavior_local_count() * 4;
    Runtime::new(
        prepared,
        Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 4,
            decisions: 0,
            commands: 4,
            behaviors: 4,
            behavior_fuel: 1024,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap()
}
fn input(id: i32, key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(id),
    }
}

#[test]
fn constants_sizes_initializers_and_large_arrays_use_owned_bounded_native_state() {
    let rt = runtime(
        "on init
        declare const $BASE := 60
        declare const $SIZE := 2 * 2
        declare %notes[$SIZE] := ($BASE, ... { continuation }
            $BASE + 1)
        declare %velocity[2] := (32)
        declare %zero[2]
        declare $result := num_elements(%notes) - (1 + 1)
        %notes[$SIZE - 1] := $BASE + 3
    end on",
        8,
    );
    for (cell, value) in [60, 61, 61, 63, 32, 32, 0, 0, 2].into_iter().enumerate() {
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell as u32),
            Ok(value)
        );
    }
    let mut rt = runtime(
        "on init declare %big[1000000] := (17, 23)
        declare $after := 42 end on
        on note %big[num_elements(%big) - 1] := $EVENT_NOTE end on",
        1_000_000,
    );
    support::without_heap(|| {
        let plan = rt.active_plan();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 999999), Ok(23));
        rt.trigger(input(1, 60), 60, 1.).unwrap();
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 999999), Ok(60));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1_000_000), Ok(42));
        rt.panic();
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
    });
}

#[test]
fn indexed_state_and_nested_reads_drive_overlapping_notes_and_release_callbacks() {
    let source = "on init declare const $BASE := 60
        declare %pitch[2] := (60,61)
        declare %velocity[2] := (32,64)
        declare %count[2]
        declare %released[2]
        declare polyphonic $slot
    end on
    on note
        ignore_event($EVENT_ID)
        $slot := $EVENT_NOTE - $BASE
        inc(%count[$slot]) dec(%count[$slot]) inc(%count[$slot])
        wait(125)
        play_note(%pitch[$slot], %velocity[%count[$slot] - 1 + $slot], 0, 125)
        %count[$slot] := %count[$slot] + num_elements(%pitch)
    end on
    on release %released[$slot] := 1 end on";
    for block in [1, 7, 64] {
        let mut rt = runtime(source, 8);
        support::without_heap(|| {
            let a = rt.trigger(input(1, 60), 60, 1.).unwrap();
            let b = rt.trigger(input(2, 61), 61, 1.).unwrap();
            rt.schedule_event(1, Event::KeyUp(a, None)).unwrap();
            rt.schedule_event(1, Event::KeyUp(b, None)).unwrap();
            let mut audio = [[0.; 2]; 16];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (frame, actual) in audio.iter().enumerate() {
                let expected = if (6..12).contains(&frame) {
                    32_f32 / 127. + 64_f32 / 127.
                } else {
                    0.
                };
                assert_eq!(*actual, [expected; 2], "{block}, {frame}");
            }
            for (cell, expected) in [(4, 3), (5, 3), (6, 1), (7, 1)] {
                assert_eq!(
                    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell),
                    Ok(expected)
                );
            }
            let mut done = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                done += 1;
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    done,
                    rt.note_count(),
                    rt.voice_count(),
                    rt.pending_commands()
                ),
                (4, 0, 0, 0)
            );
        });
    }
    let mut rt = runtime(
        "on init declare %wrap[2] := (2147483647,-2147483648) end on
        on note ignore_event($EVENT_ID) inc(%wrap[0]) dec(%wrap[1]) end on",
        2,
    );
    rt.trigger(input(1, 60), 60, 1.).unwrap();
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
        Ok(i64::from(i32::MIN))
    );
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
        Ok(i64::from(i32::MAX))
    );
}

#[test]
fn array_bounds_fault_without_overwriting_adjacent_state_or_assignment_destination() {
    for statement in [
        "%array[-1] := 7",
        "%array[2] := 7",
        "$result := %array[2147483647]",
        "inc(%array[2])",
    ] {
        let source = format!(
            "on init declare %array[2] := (10,20) declare $result := 99 end on
            on note ignore_event($EVENT_ID) {statement} end on"
        );
        let mut rt = runtime(&source, 2);
        support::without_heap(|| {
            rt.trigger(input(1, 60), 60, 1.).unwrap();
            for (cell, expected) in [(0, 10), (1, 20), (2, 99)] {
                assert_eq!(
                    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell),
                    Ok(expected)
                );
            }
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Fault(Error::InvalidInput));
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.pending_commands()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn malformed_dynamic_or_overbudget_declarations_fail_before_execution() {
    for source in [
        "on init declare %a[0] end on",
        "on init declare %a[-1] end on",
        "on init declare %a[1000001] end on",
        "on init declare %a[4] declare %b[5] end on",
        "on init declare $size := 2 declare %a[$size] end on",
        "on init declare %a[2] := () end on",
        "on init declare %a[2] := (1,2,3) end on",
        "on init declare %a[2] %a[2] := 1 end on",
        "on init declare %CC[2] end on",
        "on init declare const $x := 2 $x := 3 end on",
        "on init declare const $x := 2 end on on note inc($x) end on",
        "on init declare %a[2] declare $x := %a[0] end on",
        "on init declare %a[2] := ($EVENT_NOTE) end on",
        "on init declare %a[2] declare %a[2] end on",
        "on init declare polyphonic %a[2] end on",
        "on init declare $x end on on note $x := num_elements($x) end on",
        "on init declare %a[2] end on on note %a := 1 end on",
    ] {
        let error = match compile(source, 8) {
            Ok(_) => panic!("accepted {source}"),
            Err(error) => error,
        };
        assert!(error.offset <= source.len());
    }
    let nested = format!(
        "on init declare %a[1] declare $x end on on note $x := {}0{} end on",
        "%a[".repeat(70),
        "]".repeat(70)
    );
    assert!(compile(&nested, 1).is_err());
    assert!(compile("on init declare %a[1] end on", 0).is_err());
}
