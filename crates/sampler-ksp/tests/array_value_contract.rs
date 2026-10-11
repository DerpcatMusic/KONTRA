//! Authored source regressions: NOT_RUN / NOT_COMPILED; native behavior UNKNOWN.
use sampler_core::*;
use sampler_ksp::{Environment, Limits as KspLimits};

fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
    sampler_ksp::compile(source, 48000, KspLimits::LIBRARY, &[])
}

fn runtime(source: &str) -> Runtime {
    let script = compile(source).unwrap();
    assert!(script.warnings().is_empty(), "{:?}", script.warnings());
    let note_cells = script.note_cells() * 4;
    let prepared = script
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
                128,
            )
            .unwrap()
            .with_groups(1, vec![Some(0)])
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
            behavior_fuel: 4096,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap()
}

fn trigger(rt: &mut Runtime, key: u8) {
    rt.trigger(
        Input {
            protocol: Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key,
            external_id: Some(1),
        },
        key,
        1.,
    )
    .unwrap();
    rt.flush_behaviors(|_, _, outcome| {
        assert_eq!(outcome, Outcome::Finished);
        true
    });
}

fn cell(rt: &Runtime, index: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), index)
        .unwrap()
}

fn admission_error(declarations: &str, call: &str, expected: &str) {
    for callback in ["init", "note"] {
        let source = if callback == "init" {
            format!("on init\n{declarations}\ndeclare $r := {call}\nend on")
        } else {
            format!("on init\n{declarations}\ndeclare $r\nend on\non note\n$r := {call}\nend on")
        };
        // The same sema gate runs before fresh init and before callback lowering.
        let init_error =
            sampler_ksp::initialize(&source, KspLimits::LIBRARY, &Environment::default())
                .err()
                .expect("initializer must reject the call");
        let compile_error = compile(&source)
            .err()
            .expect("compiler must reject the call");
        for error in [init_error, compile_error] {
            assert_eq!(error.message, expected, "{source}");
            assert_eq!(error.offset, source.find(call).unwrap(), "{source}");
            assert!(error.line > 1 && error.column > 1, "{error:?}");
        }
    }
}

#[test]
fn array_equal_rejects_real_operands_in_either_position_before_init_or_lowering() {
    for call in [
        "array_equal(?a, %b)",
        "array_equal(%b, ?a)",
        "array_equal(?a, ?a)",
    ] {
        admission_error(
            "declare ?a[2]\ndeclare %b[2]",
            call,
            "array_equal does not accept real arrays",
        );
    }
}

#[test]
fn array_equal_reports_declared_unequal_lengths_instead_of_false() {
    admission_error(
        "declare %a[2]\ndeclare %b[3]",
        "array_equal(%a, %b)",
        "array_equal requires arrays of the same size",
    );
}

#[test]
fn array_equal_rejects_unowned_or_text_operands_without_an_initializer_panic() {
    for call in [
        "array_equal(%a, %CC)",
        "array_equal(%CC, %a)",
        "array_equal(%KEY_DOWN, %CC)",
        "array_equal(!text, %a)",
        "array_equal(%a, !text)",
        "array_equal(!text, !text)",
    ] {
        admission_error(
            "declare %a[2]\ndeclare !text[2]",
            call,
            "array_equal requires declared integer arrays",
        );
    }
}

#[test]
fn search_preserves_real_rejection_value_type_and_paired_endpoint_rules() {
    for (declarations, call, message) in [
        (
            "declare ?a[2]",
            "search(?a, 0.0)",
            "search does not accept real arrays",
        ),
        (
            "declare %a[2]",
            "search(%a, 0.0)",
            "search value type differs from the array",
        ),
        (
            "declare !a[2]",
            "search(!a, 0)",
            "search value type differs from the array",
        ),
        (
            "",
            "search(%KEY_DOWN, 0.0)",
            "search value type differs from the array",
        ),
        (
            "declare %a[2]",
            "search(%a, 0, 1)",
            "search requires both range endpoints",
        ),
    ] {
        admission_error(declarations, call, message);
    }
}

#[test]
fn search_rejects_system_arrays_without_a_runtime_reader() {
    for array in [
        "%POLY_AT",
        "%GROUPS_SELECTED",
        "%KEY_DOWN_OCT",
        "!NI_DND_ITEMS_ARRAY",
    ] {
        admission_error(
            "",
            &format!("search({array}, 0)"),
            "search of this array is not available",
        );
    }
}

#[test]
fn search_rejects_context_unavailable_system_readers_instead_of_silent_fallback() {
    for array in ["%GROUPS_AFFECTED", "%CC_TOUCHED", "%EVENT_PAR"] {
        let source =
            format!("on init declare $r end on\non ui_update $r := search({array}, 0) end on");
        let error = compile(&source).err().expect("context must be rejected");
        assert_eq!(
            error.message,
            "search of this array is not available in this callback"
        );
        assert_eq!(error.offset, source.find("search(").unwrap());
    }
}

#[test]
fn array_equal_admission_also_covers_functions_and_persistence_callbacks() {
    for body in [
        "function compare $r := array_equal(%a, %b) end function\non note call compare end on",
        "on persistence_changed $r := array_equal(%a, %b) end on",
    ] {
        let source = format!("on init declare %a[2] declare %b[3] declare $r end on\n{body}");
        let error = sampler_ksp::initialize(&source, KspLimits::LIBRARY, &Environment::default())
            .err()
            .expect("all bodies must pass array admission");
        assert_eq!(
            error.message,
            "array_equal requires arrays of the same size"
        );
        assert_eq!(error.offset, source.find("array_equal(").unwrap());
    }
}

#[cfg(feature = "cache")]
#[test]
fn cached_initializer_restore_rechecks_array_equal_admission() {
    let source = "on init declare %a[2] declare %b[2] end on\non note if (array_equal(%a, %b)) exit end if end on";
    let init =
        sampler_ksp::initialize(source, KspLimits::LIBRARY, &Environment::default()).unwrap();
    let bytes = serde_json::to_vec(&init.capture_initialized().unwrap()).unwrap();
    for (changed, expected) in [
        (
            source.replace("%b[2]", "%b[3]"),
            "array_equal requires arrays of the same size",
        ),
        (
            source.replace("%b", "?b"),
            "array_equal does not accept real arrays",
        ),
    ] {
        let cached = serde_json::from_slice(&bytes).unwrap();
        let error = sampler_ksp::restore_initialized(&changed, KspLimits::LIBRARY, cached)
            .err()
            .expect("cache must not bypass sema");
        assert_eq!(error.message, expected);
    }
}

#[test]
fn integer_array_equal_compares_all_cells_in_init_and_after_callback_mutation() {
    let mut rt = runtime(
        "on init
        declare %a[3] := (-2147483648, 0, 2147483647)
        declare %b[3] := (-2147483648, 0, 2147483647)
        declare $initial := array_equal(%a, %b)
        declare $same
        declare $different
        declare $self
        end on
        on note
        ignore_event($EVENT_ID)
        $same := array_equal(%a, %b)
        %b[2] := 0
        $different := array_equal(%a, %b)
        $self := array_equal(%a, %a)
        end on",
    );
    assert_eq!(cell(&rt, 6), 1);
    trigger(&mut rt, 60);
    assert_eq!([cell(&rt, 7), cell(&rt, 8), cell(&rt, 9)], [1, 0, 1]);
}

#[test]
fn owned_search_intersects_inclusive_ranges_without_out_of_bounds_zero_hits() {
    for (needle, from, to, expected) in [
        (0, -10, 10, 1),
        (0, 2, 3, 3),
        (9, 0, 3, 0),
        (9, 1, 2, 2),
        (0, -10, -1, -1),
        (0, 4, 4, -1),
        (0, 4, 2147483647, -1),
        (0, 2147483647, 2147483647, -1),
        (0, 3, 2, -1),
        (0, -2147483648, 2147483647, 1),
        (8, -2147483648, 2147483647, -1),
    ] {
        let source = format!(
            "on init
            declare %a[4] := (9, 0, 9, 0)
            declare $from := {from}
            declare $to := {to}
            declare $initial := search(%a, {needle}, $from, $to)
            declare $result := 99
            end on
            on note ignore_event($EVENT_ID)
            $result := search(%a, {needle}, $from, $to) end on"
        );
        let mut rt = runtime(&source);
        assert_eq!(cell(&rt, 6), expected, "{source}");
        trigger(&mut rt, 60);
        assert_eq!(cell(&rt, 7), expected, "{source}");
    }
}

#[test]
fn fixed_system_search_obeys_ranges_in_both_init_and_note_callbacks() {
    for (needle, from, to, init, note) in [
        (0, 10, 20, 10, 10),
        (0, -10, -1, -1, -1),
        (0, 128, 2147483647, -1, -1),
        (0, 127, 2147483647, 127, 127),
        (1, 60, 60, -1, 60),
        (0, 20, 10, -1, -1),
    ] {
        let source = format!(
            "on init
            declare $initial := search(%KEY_DOWN, {needle}, {from}, {to})
            declare $result := 99 end on
            on note ignore_event($EVENT_ID)
            $result := search(%KEY_DOWN, {needle}, {from}, {to}) end on"
        );
        let mut rt = runtime(&source);
        assert_eq!(cell(&rt, 0), init, "{source}");
        trigger(&mut rt, 60);
        assert_eq!(cell(&rt, 1), note, "{source}");
    }
}

#[test]
fn affected_group_search_uses_live_count_even_with_explicit_ranges() {
    for key in [60, 61] {
        for (range, expected) in [
            ("", if key == 60 { 0 } else { -1 }),
            (", 0, 2147483647", if key == 60 { 0 } else { -1 }),
            (", 1, 2147483647", -1),
            (", -5, -1", -1),
        ] {
            let source = format!(
                "on init
                declare $initial := search(%GROUPS_AFFECTED, 0{range})
                declare $result := 99 end on
                on note ignore_event($EVENT_ID)
                $result := search(%GROUPS_AFFECTED, 0{range}) end on"
            );
            let mut rt = runtime(&source);
            assert_eq!(cell(&rt, 0), -1, "init has no affected groups: {source}");
            trigger(&mut rt, key);
            assert_eq!(cell(&rt, 1), expected, "{source}, key={key}");
        }
    }
}

#[test]
fn unbounded_owned_search_returns_first_match_or_minus_one() {
    let mut rt = runtime(
        "on init declare %a[4] := (9, 0, 9, 0)
        declare $initial := search(%a, 0)
        declare $result := 99
        declare $missing := 99 end on
        on note ignore_event($EVENT_ID)
        $result := search(%a, 9)
        $missing := search(%a, 8) end on",
    );
    assert_eq!(cell(&rt, 4), 1);
    trigger(&mut rt, 60);
    assert_eq!([cell(&rt, 5), cell(&rt, 6)], [0, -1]);
}
