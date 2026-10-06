use sampler_core::*;
use sampler_ksp::{Widget, compile};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

const ENABLED: ControlId = ControlId(0x10001);
const LEVEL: ControlId = ControlId(0x10002);
const SLIDER: ControlId = ControlId(0x10003);
const BUTTON: ControlId = ControlId(0x10004);
const BINDINGS: &[(&str, ControlId)] = &[
    ("$enabled", ENABLED),
    ("$level", LEVEL),
    ("$slider", SLIDER),
    ("$button", BUTTON),
];
const SOURCE: &str = "on init
    make_perfview
    declare ui_knob $level(-100, 100, -10)
    declare ui_switch $enabled
    declare ui_slider $slider(-2147483648,2147483647)
    declare ui_button $button
    declare polyphonic $seen
    $enabled := 1
    $level := 30
end on
on note
    ignore_event($EVENT_ID)
    $seen := $level
    wait(1000)
    if ($enabled = 1)
        if ($level > $seen)
            play_note($EVENT_NOTE, 127, 0, 1000)
        end if
    end if
    $button := 1
end on
on release
    $slider := $seen
end on";
fn limits() -> sampler_ksp::Limits {
    sampler_ksp::Limits {
        source_bytes: 4096,
        instructions: 64,
        variables: 8,
        array_cells: 0,
    }
}
fn plan() -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[1.; 2]; 512].into_boxed_slice()).unwrap()],
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
    .unwrap()
}
#[test]
fn source_controls_drive_audio_without_a_ui_and_keep_polyphonic_memory_separate() {
    for block in [1, 7, 64] {
        let script = compile(SOURCE, 48000, limits(), BINDINGS).unwrap();
        assert_eq!(script.note_cells(), 1);
        assert!(script.has_performance_view());
        assert_eq!(
            script.controls()[0].widget,
            Widget::Knob { display_ratio: -10 }
        );
        let presentation = script.controls().to_vec();
        let mut rt = Runtime::new(
            script.bind(plan()).unwrap(),
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
                behavior_fuel: 32,
                behavior_cells: 12,
                note_cells: 4,
            },
        )
        .unwrap();
        let generation = rt.active_plan();
        // No window/renderer/presentation survives to own musical state.
        drop(presentation);
        let mut pcm = [[0.; 2]; 144];
        support::without_heap(|| {
            let input = Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            };
            let note = rt.trigger(input, 60, 1.).unwrap();
            for chunk in pcm[..24].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            rt.edit_controls(
                generation,
                Some(0),
                &[ControlWrite {
                    id: LEVEL,
                    value: ControlValue::Integer(60),
                }],
            )
            .unwrap();
            for chunk in pcm[24..72].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(
                rt.control_value(generation, BUTTON),
                Ok(ControlValue::Integer(1))
            );
            rt.key_up(note, None).unwrap();
            assert_eq!(
                rt.control_value(generation, SLIDER),
                Ok(ControlValue::Integer(30))
            );
            for chunk in pcm[72..].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
        });
        assert!(pcm[..48].iter().all(|f| *f == [0.; 2]));
        assert!(pcm[48..96].iter().all(|f| *f == [1.; 2]));
        assert!(pcm[96..].iter().all(|f| *f == [0.; 2]));
    }
    // Stable names bind to the same IDs when source declaration order changes.
    let reordered = SOURCE.replace(
        "declare ui_knob $level(-100, 100, -10)\n    declare ui_switch $enabled",
        "declare ui_switch $enabled\n    declare ui_knob $level(-100, 100, -10)",
    );
    let original = compile(SOURCE, 48000, limits(), BINDINGS)
        .unwrap()
        .bind(plan())
        .unwrap();
    let changed = compile(&reordered, 48000, limits(), BINDINGS)
        .unwrap()
        .bind(plan())
        .unwrap();
    assert_eq!(original.controls(), changed.controls());
}

#[test]
fn source_controls_reject_ambiguous_identity_and_invalid_declarations() {
    for bindings in [
        &[][..],
        &BINDINGS[..3],
        &[("$enabled", ENABLED), ("$enabled", LEVEL)],
        &[("$enabled", ENABLED), ("$level", ENABLED)],
    ] {
        assert!(compile(SOURCE, 48000, limits(), bindings).is_err());
    }
    let mut unused = BINDINGS.to_vec();
    unused.push(("$missing", ControlId(999)));
    assert!(compile(SOURCE, 48000, limits(), &unused).is_err());
    for declaration in [
        "declare ui_knob $level(100,-100,1)",
        "declare ui_knob $level(0,100,0)",
        "declare ui_slider $level(-2147483649,0)",
        "declare ui_slider $level(0,2147483648)",
        "declare ui_button $level $level := 2",
        "declare ui_switch $level $level := -1",
        "declare ui_slider $level(0,100) $level := $other",
        "declare ui_slider $level(0,100) declare ui_slider $level(0,100)",
    ] {
        let text = format!("on init {declaration} end on on release end on");
        let error = compile(&text, 48000, limits(), &[("$level", LEVEL)])
            .err()
            .expect("invalid declaration accepted");
        assert!(text.is_char_boundary(error.offset));
    }
}

#[test]
fn init_only_controls_keep_native_attack_selection() {
    let script = compile(
        "on init make_perfview declare ui_slider $level(0,100) $level := 25 end on",
        48000,
        limits(),
        &[("$level", LEVEL)],
    )
    .unwrap();
    assert!(script.has_performance_view());
    let mut rt = Runtime::new(
        script.bind(plan()).unwrap(),
        Limits {
            notes: 1,
            channels: 0,
            performances: 1,
            families: 1,
            expressions: 1,
            voices: 1,
            decisions: 0,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    support::without_heap(|| {
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: None,
        };
        let note = rt.trigger(input, 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 4];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[1.; 2]; 4]);
        assert_eq!(
            rt.control_value(rt.active_plan(), LEVEL),
            Ok(ControlValue::Integer(25))
        );
        rt.release(note).unwrap();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn ui_handlers_wait_and_control_note_playback_without_fabricated_notes() {
    let source = "on init make_perfview
        declare ui_switch $enabled
        declare ui_button $button
        declare ui_slider $level(0,100)
    end on
    on ui_control($button)
        $enabled := 0
        wait(1000)
        if ($level >= 50)
            $enabled := 1
        end if
        $button := 0
    end on
    on note
        ignore_event($EVENT_ID)
        if ($enabled = 1)
            play_note($EVENT_NOTE, 127, 0, 1000)
        end if
    end on";
    let bindings = [
        ("$enabled", ENABLED),
        ("$button", BUTTON),
        ("$level", LEVEL),
    ];
    let script = compile(source, 48000, limits(), &bindings).unwrap();
    assert!(
        script
            .controls()
            .iter()
            .find(|c| c.variable == "$button")
            .unwrap()
            .callback
            .is_some()
    );
    let mut rt = Runtime::new(
        script.bind(plan()).unwrap(),
        Limits {
            notes: 2,
            channels: 0,
            performances: 1,
            families: 2,
            expressions: 2,
            voices: 2,
            decisions: 0,
            commands: 4,
            behaviors: 2,
            behavior_fuel: 32,
            behavior_cells: 6,
            note_cells: 0,
        },
    )
    .unwrap();
    support::without_heap(|| {
        let generation = rt.active_plan();
        let (_, callback) = rt
            .invoke_control(
                control_context(&rt),
                generation,
                Some(0),
                ControlWrite {
                    id: BUTTON,
                    value: ControlValue::Integer(1),
                },
            )
            .unwrap();
        assert_eq!((rt.note_count(), rt.expression_count()), (0, 0));
        rt.edit_controls(
            generation,
            None,
            &[ControlWrite {
                id: LEVEL,
                value: ControlValue::Integer(75),
            }],
        )
        .unwrap();
        rt.render(&mut [[0.; 2]; 49]).unwrap();
        assert_eq!(
            rt.control_value(generation, ENABLED),
            Ok(ControlValue::Integer(1))
        );
        assert_eq!(
            rt.control_value(generation, BUTTON),
            Ok(ControlValue::Integer(0))
        );
        assert_eq!(
            rt.behavior_outcome(callback.unwrap()),
            Ok(Some(Outcome::Finished))
        );
        rt.flush_behaviors(|_, owner, _| {
            assert_eq!(owner, BehaviorOwner::Plan(generation));
            true
        });
        let note = rt
            .trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.,
            )
            .unwrap();
        let mut output = [[0.; 2]; 49];
        rt.render(&mut output).unwrap();
        assert_eq!(output[..48], [[1.; 2]; 48]);
        assert_eq!(output[48], [0.; 2]);
        rt.release(note).unwrap();
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
    for invalid in [
        "on init declare ui_button $button end on on ui_control($button) $button := $NOTE_HELD end on",
        "on init declare ui_button $button declare polyphonic $p end on on ui_control($button) $p := 1 end on",
        "on init declare ui_button $button end on on ui_control($button) end on on ui_control($button) end on",
        "on init declare ui_button $button end on on ui_control($missing) end on",
    ] {
        assert!(compile(invalid, 48000, limits(), &[("$button", BUTTON)]).is_err());
    }
}

#[test]
fn globals_are_shared_across_waiting_callbacks_while_polyphonic_values_remain_per_note() {
    let source = "on init
        declare $shared := -2147483648
        declare $seen
        $shared := 0
        declare polyphonic $owned
        declare ui_button $button
    end on
    on note
        ignore_event($EVENT_ID)
        $owned := $shared
        $shared := 1
        wait(1000)
        if ($owned = 0)
            if ($shared = 7)
                play_note($EVENT_NOTE, 127, 0, 1000)
            end if
        end if
    end on
    on ui_control($button)
        $shared := 7
        wait(500)
        $seen := $shared
    end on
    on release
        $seen := $owned
    end on";
    for block in [1, 7, 64] {
        let script = compile(source, 48000, limits(), &[("$button", BUTTON)]).unwrap();
        assert_eq!(script.global_cells(), 2);
        assert_eq!(script.note_cells(), 1);
        let presentation = script.controls().to_vec();
        let mut rt = Runtime::new(
            script.bind(plan()).unwrap(),
            Limits {
                notes: 4,
                channels: 0,
                performances: 1,
                families: 4,
                expressions: 4,
                voices: 4,
                decisions: 0,
                commands: 8,
                behaviors: 8,
                behavior_fuel: 64,
                behavior_cells: 24,
                note_cells: 4,
            },
        )
        .unwrap();
        drop(presentation);
        support::without_heap(|| {
            let plan = rt.active_plan();
            let instance = ScriptInstanceId(0);
            assert_eq!(rt.script_cell(plan, instance, 0), Ok(0));
            assert_eq!(rt.script_cell(plan, instance, 1), Ok(0));
            let input = Input {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(1),
            };
            let first = rt.trigger(input, 60, 1.).unwrap();
            let second = rt
                .trigger(
                    Input {
                        external_id: Some(2),
                        ..input
                    },
                    60,
                    1.,
                )
                .unwrap();
            assert_eq!(rt.note_cell(first, 0), Ok(0));
            assert_eq!(rt.note_cell(second, 0), Ok(1));
            rt.invoke_control(
                control_context(&rt),
                plan,
                None,
                ControlWrite {
                    id: BUTTON,
                    value: ControlValue::Integer(1),
                },
            )
            .unwrap();
            let mut output = [[0.; 2]; 144];
            for chunk in output.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (frame, value) in output.iter().enumerate() {
                assert_eq!(*value, [f32::from((48..96).contains(&frame)); 2]);
            }
            assert_eq!(rt.script_cell(plan, instance, 0), Ok(7));
            assert_eq!(rt.script_cell(plan, instance, 1), Ok(7));
            rt.key_up(first, None).unwrap();
            assert_eq!(rt.script_cell(plan, instance, 1), Ok(0));
            rt.key_up(second, None).unwrap();
            assert_eq!(rt.script_cell(plan, instance, 1), Ok(1));
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.family_count()),
                (0, 0, 0)
            );
        });
    }
    for source in [
        "on init declare $a := 2147483648 end on",
        "on init declare $a := -2147483649 end on",
        "on init declare $a declare polyphonic $a end on",
        "on init declare $a declare ui_button $a end on",
        "on init declare $a := $EVENT_NOTE end on",
        "on init declare $a := end on",
        "on init declare $a declare $b $a := $b end on",
    ] {
        assert!(compile(source, 48000, limits(), &[]).is_err(), "{source}");
    }
    assert!(
        compile(
            "on init declare $a declare $b end on",
            48000,
            sampler_ksp::Limits {
                variables: 1,
                array_cells: 0,
                ..limits()
            },
            &[]
        )
        .is_err()
    );
}

#[test]
fn arithmetic_drives_a_waiting_sequence_through_native_audio_after_physical_release() {
    let source = "on init declare $step end on
        on note
            ignore_event($EVENT_ID)
            while ($step < 4)
                if (($step mod 2) = 0)
                    play_note($EVENT_NOTE, 127, 0, 500)
                end if
                inc($step)
                wait(1000)
            end while
        end on";
    for block in [1, 7, 64] {
        let prepared = compile(source, 48000, limits(), &[])
            .unwrap()
            .bind(plan())
            .unwrap();
        let mut rt = Runtime::new(
            prepared,
            Limits {
                notes: 4,
                channels: 0,
                performances: 1,
                families: 4,
                expressions: 4,
                voices: 4,
                decisions: 0,
                commands: 8,
                behaviors: 2,
                behavior_fuel: 64,
                behavior_cells: 8,
                note_cells: 0,
            },
        )
        .unwrap();
        support::without_heap(|| {
            let input = Input {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(1),
            };
            let note = rt.trigger(input, 60, 1.).unwrap();
            rt.schedule_event(1, Event::KeyUp(note, None)).unwrap();
            let mut output = [[0.; 2]; 224];
            for chunk in output.chunks_mut(block) {
                rt.render(&mut []).unwrap();
                rt.render(chunk).unwrap();
            }
            for (frame, value) in output.iter().enumerate() {
                assert_eq!(
                    *value,
                    [f32::from(frame < 24 || (96..120).contains(&frame)); 2]
                );
            }
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                Ok(4)
            );
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.family_count()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn waiting_ui_callback_controls_live_dsp_through_shared_values_without_a_window() {
    let source = "on init
        declare ui_slider $level(0,100)
        declare ui_button $button
        $level := 100
    end on
    on ui_control($button)
        $level := 0
        wait(125)
        $level := 100
    end on";
    for block in [1, 7, 64] {
        let prepared = compile(
            source,
            48000,
            limits(),
            &[("$level", LEVEL), ("$button", BUTTON)],
        )
        .unwrap()
        .bind(plan())
        .unwrap()
        .with_voice_chains(
            vec![
                sampler_core::VoiceChain::new(
                    vec![],
                    vec![sampler_core::Processor::ControlGain(
                        sampler_core::ControlRange {
                            control: LEVEL,
                            low: 0.,
                            high: 1.,
                            ramp_frames: 4,
                        },
                    )],
                    0,
                )
                .unwrap(),
            ],
            vec![Some(0)],
        )
        .unwrap();
        let cells = prepared.behavior_local_count();
        let mut rt = Runtime::new(
            prepared,
            Limits {
                notes: 2,
                channels: 0,
                performances: 1,
                families: 2,
                expressions: 2,
                voices: 2,
                decisions: 0,
                commands: 2,
                behaviors: 1,
                behavior_fuel: 32,
                behavior_cells: cells,
                note_cells: 0,
            },
        )
        .unwrap();
        support::without_heap(|| {
            let note = rt
                .trigger(
                    Input {
                        protocol: Protocol::Native,
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
            // Submitted before the callback's wait: its equal-time write must
            // execute before the resumed script restores LEVEL to 100.
            rt.schedule_event(
                6,
                sampler_core::Event::Control(
                    rt.active_plan(),
                    ControlWrite {
                        id: LEVEL,
                        value: ControlValue::Integer(50),
                    },
                ),
            )
            .unwrap();
            let (_, callback) = rt
                .invoke_control(
                    control_context(&rt),
                    rt.active_plan(),
                    None,
                    ControlWrite {
                        id: BUTTON,
                        value: ControlValue::Integer(1),
                    },
                )
                .unwrap();
            let callback = callback.unwrap();
            assert_eq!(
                rt.note_count(),
                1,
                "UI callbacks have no fabricated note owner"
            );
            let mut audio = [[0.; 2]; 12];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (actual, expected) in audio
                .iter()
                .zip([1., 0.75, 0.5, 0.25, 0., 0., 0., 0.25, 0.5, 0.75, 1., 1.])
            {
                assert_eq!(*actual, [expected; 2], "block {block}");
            }
            assert_eq!(
                rt.behavior_outcome(callback),
                Ok(Some(sampler_core::Outcome::Finished))
            );
            assert_eq!(
                rt.control_value(rt.active_plan(), LEVEL),
                Ok(ControlValue::Integer(100))
            );
            rt.key_up(note, None).unwrap();
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.pending_commands()),
                (0, 0, 0)
            );
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
