use sampler_core::*;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
fn limits() -> sampler_ksp::Limits {
    sampler_ksp::Limits {
        source_bytes: 131072,
        instructions: 16384,
        variables: 16,
        array_cells: 16,
    }
}
fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, limits(), &[])
}
fn runtime(source: &str, fuel: usize) -> Runtime {
    let script = compile(source).unwrap();
    let plan = script
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[1.; 2]; 8])).unwrap()],
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
    let cells = plan.behavior_local_count() * 8;
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 0,
            performances: 1,
            families: 8,
            voices: 8,
            expressions: 8,
            decisions: 0,
            commands: 8,
            behaviors: 8,
            behavior_fuel: fuel,
            behavior_cells: cells,
            note_cells: 0,
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
fn nested_selects_keep_first_match_across_waits_and_drive_independent_audio() {
    let source = "on init declare %seen[2] end on
      on note
        ignore_event($EVENT_ID)
        select ($EVENT_NOTE)
          case 60 to 61
            select ($EVENT_NOTE)
              case 60
                wait(125)
                play_note(60,127,00H,0)
                %seen[0] := 10
              case 61
                wait(250)
                play_note(60,127,0,0)
                %seen[1] := 20
            end select
            inc(%seen[$EVENT_NOTE - 60])
          case 080000000H to 07FFFFFFFH
            %seen[0] := 99
        end select
      end on
      on release
        select ($EVENT_NOTE)
          case 60 dec(%seen[0])
          case 61 dec(%seen[1])
        end select
      end on";
    for block in [1, 7, 64] {
        let mut rt = runtime(source, 256);
        support::without_heap(|| {
            let a = rt.trigger(input(60), 60, 1.).unwrap();
            let b = rt.trigger(input(61), 61, 1.).unwrap();
            let mut audio = [[0.; 2]; 24];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (frame, actual) in audio.iter().enumerate() {
                let expected =
                    u8::from((6..14).contains(&frame)) + u8::from((12..20).contains(&frame));
                assert_eq!(*actual, [f32::from(expected); 2], "frame {frame}");
            }
            let p = rt.active_plan();
            assert_eq!(rt.script_cell(p, ScriptInstanceId(0), 0), Ok(11));
            assert_eq!(rt.script_cell(p, ScriptInstanceId(0), 1), Ok(21));
            rt.key_up(a, None).unwrap();
            rt.key_up(b, None).unwrap();
            assert_eq!(rt.script_cell(p, ScriptInstanceId(0), 0), Ok(10));
            assert_eq!(rt.script_cell(p, ScriptInstanceId(0), 1), Ok(20));
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}
#[test]
fn constants_ranges_overlap_and_hex_literals_preserve_signed_integer_semantics() {
    for (literal, expected) in [
        ("080000000H", 1),
        ("-2147483647", 1),
        ("-4", 2),
        ("-2", 2),
        ("0", 3),
        ("+0FFFFFFFFh", 2),
        ("-0FFFFFFFFH", 4),
        ("07FFFFFFFH", 4),
        ("-080000000H", 1),
    ] {
        let source = format!(
            "on init declare $result declare const $LOW := -4
          declare $value := {literal} end on
          on note ignore_event($EVENT_ID)
            select ($value)
              case 080000000H to -2147483647 $result := 1
              case -1 to $LOW $result := 2
              case 0 $result := 3 $value := 100
              case 0 to 07FFFFFFFH $result := 4
            end select
            select (0) end select
          end on"
        );
        let mut rt = runtime(&source, 128);
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.).unwrap();
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                Ok(expected),
                "{literal}"
            );
        });
    }
    let source = "on init declare $result declare $i end on on note ignore_event($EVENT_ID)
        while ($i < 4) inc($i) select ($i)
          case 1 to 3 continue
          case 4 $result := 7
        end select $result := $result + 2 end while end on";
    let mut rt = runtime(source, 256);
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
            Ok(9)
        );
    });
}
#[test]
fn case_dispatch_and_parsing_are_bounded_and_dead_cases_are_validated() {
    let source = format!(
        "on note ignore_event($EVENT_ID) select (0) case 0 exit {}end select end on",
        "case 1 wait(1) ".repeat(1000)
    );
    let mut rt = runtime(&source, 16);
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
    let source = format!(
        "on note ignore_event($EVENT_ID) select (0) {}end select end on",
        "case 1 wait(1) ".repeat(1000)
    );
    let mut rt = runtime(&source, 16);
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::FuelExhausted);
            true
        });
    });
    let deep = format!(
        "on note {}exit {}end on",
        "select (0) case 0 ".repeat(200),
        "end select ".repeat(200)
    );
    assert!(compile(&deep).is_ok());
    // v2: `1H` is a valid hex literal and an empty select is a no-op.
    assert!(compile("on note select(1H) end select end on").is_ok());
    for body in [
        "case 0",
        "select (0) wait(1) end select",
        "select(0) case 0 else end select",
        "select(0) case 0 end if",
        "select(0) case $EVENT_NOTE end select",
        "select(0) case 0 to $EVENT_NOTE end select",
        "select(0) case 0 unknown() end select",
        "if (0 = 1) case 0 end if",
        "select(0) case 0 if(1=1) case 1 end if end select",
        "select(0) case 0 to end select",
        "end select",
        "select(0) case 0100000000H end select",
        "select(0x10) end select",
    ] {
        let source = format!("on note {body} end on");
        assert!(compile(&source).is_err(), "{body}");
    }
    assert!(
        sampler_ksp::compile(
            &deep,
            48000,
            sampler_ksp::Limits {
                instructions: 64,
                ..limits()
            },
            &[]
        )
        .is_err()
    );
}

#[test]
fn boolean_precedence_normalization_and_constant_evaluation_share_native_operations() {
    for (expression, expected) in [
        ("1 or 0 and 0", 1),
        ("1 xor 1 or 1", 1),
        ("1 or 1 xor 1", 0),
        ("not 2 = 3", 1),
        ("not 2 < 3", 0),
        ("2 and -3", 1),
        ("0 or -2", 1),
        ("-2 xor 7", 0),
        ("not .not. -1", 1),
        ("(2 .and. 3) = 2 and ((4 + 2) * 3 = 18)", 1),
        ("in_range(2+1,1,3)", 1),
        ("in_range(1,1,3)", 1),
        ("in_range(2,3,1)", 0),
        ("in_range(07FFFFFFFH,080000000H,07FFFFFFFH)", 1),
        ("in_range(-1,0,1)", 0),
        ("(1 or 0) = 1 and (4 .or. 2) = 6", 1),
    ] {
        let source = format!(
            "on init declare const $C := {expression}
            declare $initial := $C declare $value declare $branch end on
            on note ignore_event($EVENT_ID) $value := {expression}
            if ({expression}) $branch := 1 else $branch := 0 end if end on"
        );
        let mut rt = runtime(&source, 512);
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.).unwrap();
            for cell in 0..3 {
                assert_eq!(
                    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), cell),
                    Ok(expected),
                    "{expression}: cell {cell}"
                );
            }
        });
    }
}
#[test]
fn boolean_short_circuit_guards_arrays_but_never_hides_malformed_source_or_runtime_xor_reads() {
    let source = "on init declare %a[1] := (17) declare $i := -1 declare $result end on
      on note ignore_event($EVENT_ID)
        if ($i >= 0 and $i < num_elements(%a) and %a[$i] = 17) $result := 99
        else $result := 1 end if
        if ($result or %a[99]) inc($result) end if
        $i := -2
        while ($i) inc($i) inc($result) end while
      end on";
    let mut rt = runtime(source, 256);
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.).unwrap();
        assert_eq!(
            rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
            Ok(4)
        );
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
    });
    // An unguarded read outside the array is 0, so the expression still runs.
    for (expression, expected) in [
        ("1 and %a[99]", 0),
        ("0 or %a[99]", 0),
        ("1 xor %a[99]", 1),
        ("0 xor %a[99]", 0),
    ] {
        let source = format!(
            "on init declare %a[1] declare $result := 7 end on
            on note ignore_event($EVENT_ID) $result := {expression} end on"
        );
        let mut rt = runtime(&source, 128);
        support::without_heap(|| {
            rt.trigger(input(60), 60, 1.).unwrap();
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1),
                Ok(expected)
            );
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
        });
    }
    for expression in [
        "1 or missing()",
        "0 and $missing",
        "not",
        "1 and",
        "xor 1",
        "in_range(1,2)",
        "in_range(1,2,3,4)",
    ] {
        assert!(
            compile(&format!("on note if ({expression}) exit end if end on")).is_err(),
            "{expression}"
        );
    }
    assert!(
        compile("on init declare $runtime declare const $BAD := 0 and $runtime end on").is_err()
    );
    assert!(
        compile(&format!(
            "on note if ({}0) exit end if end on",
            "not ".repeat(300)
        ))
        .is_err()
    );
}
