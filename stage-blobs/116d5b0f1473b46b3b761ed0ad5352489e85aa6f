//! Script voice parameters reach the audio: change_vol/pan, fades,
//! set_engine_par group volume and purge_group.
use sampler_core::{
    Envelope, GroupParams, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime,
};

fn runtime(source: &str) -> Runtime {
    authored(source, GroupParams::default())
}

fn compile(source: &str) -> sampler_ksp::Script {
    sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
    .unwrap()
}

/// Group 0 authored at `group0`, baked into its region's gain as lowering does.
fn authored(source: &str, group0: GroupParams) -> Runtime {
    modules(vec![compile(source)], group0)
}

fn modules(scripts: Vec<sampler_ksp::Script>, group0: GroupParams) -> Runtime {
    modules_with(scripts, group0, Ok)
}

fn modules_with(
    scripts: Vec<sampler_ksp::Script>,
    group0: GroupParams,
    shape: impl FnOnce(Prepared) -> Result<Prepared, sampler_core::Error>,
) -> Runtime {
    let note_cells = scripts.iter().map(|s| s.note_cells()).sum::<usize>() * 8;
    let pcm = [0.5, 0.25].map(|v| Pcm::new(48000, Box::from([[v; 2]; 48000])).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60 + sample as u8,
            key_high: 60 + sample as u8,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: if sample == 0 {
                10f32.powf(group0.decibels as f32 / 20.)
            } else {
                1.
            },
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let prepared = Prepared::new(48000, pcm.into(), regions, 2)
        .unwrap()
        .with_groups(2, vec![Some(0), Some(1)])
        .and_then(|p| p.with_group_params(vec![group0, GroupParams::default()]))
        .and_then(|p| p.with_group_envelope_parameters(0, 0, 5, Envelope::default()))
        .and_then(|p| {
            let bindings = p.engine_parameter_bindings().to_vec();
            p.with_engine_parameters(
                bindings,
                vec![
                    sampler_core::EngineLookup {
                        group: 0,
                        owner: -1,
                        target: false,
                        name: "ENV_AHDSR".into(),
                        index: 5,
                    },
                    sampler_core::EngineLookup {
                        group: 1,
                        owner: -1,
                        target: false,
                        name: "ENV_FILTER".into(),
                        index: 8,
                    },
                ],
            )
        })
        .and_then(shape)
        .unwrap();
    let plan = sampler_ksp::bind_modules(scripts, prepared).unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 8,
            voices: 8,
            expressions: 8,
            decisions: 0,
            commands: 16,
            behaviors: 8,
            behavior_fuel: 4096,
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

/// Last frame of a 256-frame render.
fn level(rt: &mut Runtime) -> [f32; 2] {
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    audio[255]
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[test]
fn note_volume_pan_and_fades_shape_the_voice() {
    let mut rt = runtime(
        "on note
           change_vol($EVENT_ID, -6021, 0)
           change_pan($EVENT_ID, 1000, 0)
           wait(10000)
           change_vol($EVENT_ID, 6021, 1)
           set_event_par($EVENT_ID, $EVENT_PAR_PAN, 0)
           wait(10000)
           fade_out($EVENT_ID, 2000, 1)
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    // -6 dB and hard right: balance law silences the left.
    assert!(close(level(&mut rt), [0.0, 0.25]));
    level(&mut rt);
    // Frames 512..768: back to 0 dB and centre (waits are 480 frames).
    assert!(close(level(&mut rt), [0.5, 0.5]));
    level(&mut rt);
    // Faded out and stopped: the voice is gone.
    assert_eq!(level(&mut rt), [0.0; 2]);
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn fade_curve_array_selectors_reach_audio_and_use_time_mirrored_fade_out() {
    // Independent quarter-time values from the documented five equations.
    let gains = [
        (0.25, 0.75),
        (0.38268343, 0.92387953),
        (0.14644661, 0.85355339),
        (0.0625, 0.5625),
        (0.4375, 0.9375),
    ];
    for (index, (fade_in, fade_out)) in gains.into_iter().enumerate() {
        for out in [false, true] {
            for block in [1, 17, 128] {
                let call = if out {
                    "fade_out($EVENT_ID,100000,$stop,%curves[$index])"
                } else {
                    "fade_in($EVENT_ID,100000,%curves[$index])"
                };
                let mut rt = runtime(&format!(
                    "on init declare $index := {index} declare $stop := 0
                     declare %curves[5] := ($NI_FADE_LINEAR,$NI_FADE_EQUAL_POWER,
                     $NI_FADE_S_CURVE,$NI_FADE_EXPONENTIAL,$NI_FADE_LOGARITHMIC)
                     end on on note {call} end on"
                ));
                rt.trigger(input(60), 60, 1.).unwrap();
                let mut audio = [[0.; 2]; 1201];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                let gain = if out { fade_out } else { fade_in };
                assert!(
                    audio[1199]
                        .iter()
                        .all(|value| (value - 0.5 * gain).abs() < 2e-7),
                    "curve {index}, out {out}, block {block}, elapsed1200: {:?}",
                    audio[1199]
                );
                assert!(rt.take_fault().is_none());
            }
        }
    }
}

#[test]
fn fade_curve_inside_mix_cell_matches_exponential_not_endpoint_interpolation() {
    // Sample-end clock: index15 is elapsed16, inside the 64-frame gain cell.
    for out in [false, true] {
        for block in [1, 17, 128] {
            let call = if out {
                "fade_out($EVENT_ID,8000,0,$NI_FADE_EXPONENTIAL)"
            } else {
                "fade_in($EVENT_ID,8000,$NI_FADE_EXPONENTIAL)"
            };
            let mut control = runtime("on note end on");
            control.trigger(input(60), 60, 1.).unwrap();
            assert_eq!(control.voice_count(), 1);
            let mut unfaded = [[0.; 2]; 129];
            for chunk in unfaded.chunks_mut(block) {
                control.render(chunk).unwrap();
            }
            assert!(unfaded.iter().all(|frame| *frame == [0.5; 2]));
            let mut rt = runtime(&format!("on note {call} end on"));
            rt.trigger(input(60), 60, 1.).unwrap();
            assert_eq!(rt.voice_count(), 1);
            assert!(rt.take_fault().is_none());
            let mut audio = [[0.; 2]; 129];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(rt.now(), control.now());
            assert_ne!(audio[15], unfaded[15]);
            let t = 16.0_f32 / 384.0; // Exactly8000us at48kHz.
            let expected = 0.5 * if out { (1.0 - t).powi(2) } else { t.powi(2) };
            assert!(
                audio[15]
                    .iter()
                    .all(|value| (value - expected).abs() < 1e-7),
                "out {out}, block {block}, elapsed16: {:?}, expected {expected}",
                audio[15]
            );
            assert!(rt.take_fault().is_none());
        }
    }
}

#[test]
fn fade_curve_script_clock_and_omitted_linear_match_sample_end_control() {
    for origin in [0, 128] {
        for out in [false, true] {
            for block in [1, 17, 128] {
                let mut previous = None;
                for selector in ["", ",$NI_FADE_LINEAR"] {
                    let call = if out {
                        format!("fade_out($EVENT_ID,8000,0{selector})")
                    } else {
                        format!("fade_in($EVENT_ID,8000{selector})")
                    };
                    let mut rt = runtime(&format!("on note {call} end on"));
                    rt.render(&mut vec![[0.; 2]; origin]).unwrap();
                    assert_eq!(rt.now(), origin as u64);
                    rt.trigger(input(60), 60, 1.).unwrap();
                    assert_eq!(rt.now(), origin as u64); // Callback runs synchronously.
                    let mut audio = [[0.; 2]; 385];
                    for chunk in audio.chunks_mut(block) {
                        rt.render(chunk).unwrap();
                    }
                    for index in [0, 15, 383, 384] {
                        let t = ((index + 1) as f32 / 384.).min(1.);
                        let expected = 0.5 * if out { 1. - t } else { t };
                        assert!(
                            audio[index].iter().all(|v| (v - expected).abs() < 1e-7),
                            "origin {origin}, out {out}, block {block}, index {index}: {:?}",
                            audio[index]
                        );
                    }
                    if let Some(default) = previous {
                        assert_eq!(audio, default); // Optional selector keeps legacy PCM bits.
                    }
                    previous = Some(audio);
                    assert!(rt.take_fault().is_none());
                }
            }
        }
    }
}

#[test]
fn fade_curve_unknown_selector_faults_without_a_silent_linear_substitution() {
    for selector in [-1, 5, 99] {
        let mut rt = runtime(&format!(
            "on init declare $curve := {selector} declare $id := 0 end on
             on note $id := $EVENT_ID end on
             on controller fade_out($id,100000,1,$curve) end on"
        ));
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        assert_eq!(level(&mut rt), [0.5; 2]);
        assert_eq!(rt.voice_count(), 1);
        let origin = rt.now();
        let performance = rt.performance(0).unwrap();
        let callback = rt
            .dispatch_controller(performance, input(60).channel_address(), 1, 1, u32::MAX)
            .unwrap()
            .unwrap();
        assert_eq!(rt.now(), origin);
        assert_eq!(
            rt.behavior_outcome(callback),
            Ok(Some(sampler_core::Outcome::Fault(sampler_core::Error::InvalidInput)))
        );
        assert!(matches!(
            rt.take_fault(),
            Some((_, sampler_core::Error::InvalidInput))
        ));
        // Plan-owned failure cannot close this note; rejection must not install a fade.
        let mut audio = [[0.; 2]; 256];
        rt.render(&mut audio).unwrap();
        assert!(
            audio.iter().all(|frame| *frame == [0.5; 2]),
            "{selector}: {:?}",
            audio[255]
        );
        assert_eq!(rt.now(), origin + 256);
        assert_eq!(rt.note(note), Ok((60, 1., true)));
        assert_eq!(rt.voice_count(), 1);
        assert!(rt.take_fault().is_none());
    }
}

#[test]
fn fade_curve_note_owned_fault_releases_only_its_owner_at_the_callback_clock() {
    let mut rt = runtime(
        "on init declare $curve := 99 end on
         on note if ($EVENT_NOTE = 60) fade_out($EVENT_ID,100000,1,$curve) end if end on",
    );
    let unaffected = rt.trigger(input(61), 61, 1.).unwrap();
    assert_eq!(level(&mut rt), [0.25; 2]);
    let origin = rt.now();
    let faulty = rt.trigger(input(60), 60, 1.).unwrap();
    assert_eq!(rt.now(), origin);
    assert!(matches!(
        rt.take_fault(),
        Some((_, sampler_core::Error::InvalidInput))
    ));
    // Existing fault policy closes the initiating note, not an unaffected note.
    assert_eq!(rt.note(faulty), Ok((60, 1., false)));
    assert_eq!(rt.note(unaffected), Ok((61, 1., true)));
    assert_eq!(rt.voice_count(), 1);
    let mut audio = [[0.; 2]; 256];
    rt.render(&mut audio).unwrap();
    assert!(audio.iter().all(|frame| *frame == [0.25; 2]));
    assert_eq!(rt.now(), origin + 256);
    assert_eq!(rt.note(unaffected), Ok((61, 1., true)));
    assert!(rt.take_fault().is_none());
}

#[test]
fn engine_volume_and_purge_address_one_group() {
    let mut rt = runtime(
        "on init
           set_engine_par($ENGINE_PAR_VOLUME, 0, 0, -1, -1)
         end on
         on note
           if ($EVENT_NOTE = 61)
             set_engine_par($ENGINE_PAR_VOLUME, 500000, 0, -1, -1)
             set_engine_par($ENGINE_PAR_TUNE, 500000, 1, -1, -1)
             purge_group(1, 0)
           end if
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    // on init's write stands: group 0 starts at the volume floor.
    assert!(close(level(&mut rt), [0.0; 2]));
    rt.trigger(input(61), 61, 1.).unwrap();
    // 500000 is -6.02 dB for group 0 (both of its notes); group 1 is purged.
    assert!(close(level(&mut rt), [0.2506; 2]), "{:?}", level(&mut rt));
}

#[test]
fn volume_envelope_attack_applies_to_voices_that_start_after_it() {
    let mut rt = runtime(
        "on note
           { 200809 is 10 ms (480 frames) on Kontakt's attack law. }
           set_engine_par($ENGINE_PAR_ATTACK, 200809, 0, find_mod(0, \"ENV_AHDSR\"), -1)
           { Another modulator's attack is left to the host. }
           set_engine_par($ENGINE_PAR_ATTACK, 0, 1, find_mod(1, \"ENV_FILTER\"), -1)
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    let level = level(&mut rt)[0];
    assert!((level - 0.5 * 256. / 480.).abs() < 0.01, "{level}");
}

#[test]
fn attack_curve_bends_the_attack_ramp() {
    let first = |curve: i32| {
        let mut rt = runtime(&format!(
            "on note
               set_engine_par($ENGINE_PAR_ATTACK, 200809, 0, find_mod(0, \"ENV_AHDSR\"), -1)
               set_engine_par($ENGINE_PAR_ATK_CURVE, {curve}, 0, find_mod(0, \"ENV_AHDSR\"), -1)
             end on"
        ));
        rt.trigger(input(60), 60, 1.).unwrap();
        level(&mut rt)[0]
    };
    // Full curve starts fast, zero curve starts slow.
    assert!(
        first(1_000_000) > first(0) * 2.,
        "{} {}",
        first(1_000_000),
        first(0)
    );
}

#[test]
fn engine_volume_reads_the_authored_value_and_sets_it_absolutely() {
    let mut rt = authored(
        "on note
           { The authored -6 dB group reads back as 500000. }
           if (abs(get_engine_par($ENGINE_PAR_VOLUME, 0, -1, -1) - 500000) < 20)
             { 629960 is 0 dB, not +6 dB on top of the authored value. }
             set_engine_par($ENGINE_PAR_VOLUME, 629960, 0, -1, -1)
           end if
         end on",
        GroupParams {
            decibels: -6.0,
            ..GroupParams::default()
        },
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    assert!(close(level(&mut rt), [0.5; 2]), "{:?}", level(&mut rt));
}

#[test]
fn timer_listener_plays_notes_on_its_period() {
    let mut rt = runtime(
        "on init
           declare $n
           set_listener($NI_SIGNAL_TIMER_MS, 10000)
         end on
         on listener
           if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TIMER_MS)
             inc($n)
             if ($n = 2)
               { The second tick stops the timer. }
               change_listener_par($NI_SIGNAL_TIMER_MS, 0)
             end if
             play_note(60, 100, 0, 100000)
           end if
         end on",
    );
    // 10 ms is 480 frames: silent before the first tick, sounding after.
    assert_eq!(level(&mut rt), [0.0; 2]);

    level(&mut rt);
    // Velocity 100: 0.5 · 100/127.
    assert!(close(level(&mut rt), [0.3937; 2]));
    for _ in 0..25 {
        level(&mut rt);
    }
    // Two 100 ms notes, then no more ticks.
    assert_eq!(level(&mut rt), [0.0; 2]);
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn pgs_writes_reach_every_slot_and_run_pgs_changed() {
    assert_pgs_writes_reach_reader("pgs_changed");
}

#[test]
fn pgs_writes_reach_every_slot_and_run_underscored_pgs_changed() {
    assert_pgs_writes_reach_reader("_pgs_changed");
}

fn assert_pgs_writes_reach_reader(callback: &str) {
    let writer = compile(
        "on note
           ignore_event($EVENT_ID)
           pgs_set_key_val(SHARED, 0, 5)
         end on",
    );
    let reader = compile(&format!(
        "on init
           pgs_create_key(SHARED, 1)
         end on
         on {callback}
           if (pgs_key_exists(SHARED) and pgs_get_key_val(SHARED, 0) = 5)
             play_note(61, 127, 0, 100000)
           end if
         end on"
    ));
    let mut rt = modules(vec![writer, reader], GroupParams::default());
    rt.trigger(input(60), 60, 1.).unwrap();
    // Only the reader's note 61 sounds.
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
}

#[test]
fn sort_and_array_equal_run_at_runtime() {
    let mut rt = runtime(
        "on init
           declare %a[4] := (59, 61, 58, 60)
           declare %b[4] := (61, 60, 59, 58)
         end on
         on note
           ignore_event($EVENT_ID)
           sort(%a, 1)
           if (array_equal(%a, %b))
             sort(%a, 0, 1, 3)
             { 61, 58, 59, 60 }
             if (%a[1] = 58 and %a[3] = 60)
               play_note(61, 127, 0, 100000)
             end if
           end if
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
}

#[test]
fn runtime_ui_requests_update_the_model() {
    let source = "on init
           declare ui_label $offline(1, 1)
           set_listener($NI_SIGNAL_TIMER_MS, 10000)
         end on
         on listener
           set_control_par(get_ui_id($offline), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)
           set_control_par_str(get_ui_id($offline), $CONTROL_PAR_PICTURE, \"online\")
           set_key_color(60, $KEY_COLOR_CYAN)
         end on";
    let mut model = compile(source);
    let mut rt = runtime(source);
    for _ in 0..3 {
        level(&mut rt);
    }
    let mut view = model.view();
    let mut applied = 0;
    rt.drain_effects(|e| {
        applied += usize::from(model.apply_ui_effect(e));
        assert!(view.apply_ui_effect(e));
        true
    });
    assert_eq!(view.ui(&|_| None).unwrap(), model.ui(&|_| None).unwrap());
    let w = &model.model().interface.widgets[0];
    assert_eq!(applied, 3);
    assert_eq!(view.model().interface.keys[60].color, Some(8));
    assert_eq!(w.int("$CONTROL_PAR_HIDE"), Some(16));
    assert_eq!(
        w.properties.get("$CONTROL_PAR_PICTURE"),
        Some(&sampler_ksp::model::Value::Text("online".into()))
    );
}

#[test]
fn key_down_reads_held_input_keys() {
    let mut rt = runtime(
        "on note
           ignore_event($EVENT_ID)
           if (%KEY_DOWN[60] = 1 and %KEY_DOWN[61] = 0 and search(%KEY_DOWN, 1) = 60)
             play_note(61, 127, 0, 100000)
           end if
         end on",
    );
    rt.trigger(input(60), 60, 1.).unwrap();
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
}

/// Fades start off the 64-frame grid; the output must not depend on how the
/// host splits the render into blocks.
#[test]
fn a_script_fade_renders_identically_for_every_block_size() {
    let render = |block: usize| {
        let mut rt = runtime(
            "on note
               wait(10000)
               fade_out($EVENT_ID, 20000, 1)
             end on",
        );
        rt.trigger(input(60), 60, 1.).unwrap();
        let mut out = vec![[0.0; 2]; 4096];
        for chunk in out.chunks_mut(block) {
            rt.render(chunk).unwrap();
        }
        assert_eq!(rt.voice_count(), 0);
        out
    };
    let reference = render(64);
    assert!(reference[600] != [0.0; 2], "the fade is audible");
    for block in [7, 61] {
        assert!(render(block) == reference, "block {block} differs");
    }
}

#[test]
fn effect_slot_writes_drive_a_bus_mix_block() {
    use sampler_core::{
        Bus, BusSend, ControlDefinition, ControlDomain, ControlRange, ControlValue, Processor,
        SlotKind, slot_control,
    };
    let range = |kind: SlotKind| ControlRange {
        control: slot_control(kind, -1, 2, 1),
        low: 0.,
        high: kind.max(),
        ramp_frames: 4,
    };
    let definition = |kind: SlotKind, value| ControlDefinition {
        id: slot_control(kind, -1, 2, 1),
        domain: ControlDomain::Real {
            min: 0.,
            max: kind.max(),
        },
        default: ControlValue::Real(value),
    };
    let mut rt = modules_with(
        vec![compile(
            "on note
               if ($EVENT_NOTE = 60)
                 set_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 396851, -1, 2, 1)
                 set_engine_par($ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN, 0, -1, 2, 1)
               end if
               if ($EVENT_NOTE = 61)
                 set_engine_par($ENGINE_PAR_EFFECT_BYPASS, 1, -1, 2, 1)
               end if
             end on",
        )],
        GroupParams::default(),
        |p| {
            let mut controls = p.controls().to_vec();
            controls.extend([
                definition(SlotKind::Dry, 0.),
                definition(SlotKind::Output, 1.),
                definition(SlotKind::Bypass, 0.),
            ]);
            p.with_controls(controls)?.with_buses(
                vec![Bus {
                    processors: vec![
                        Processor::Mix {
                            count: 1,
                            dry: range(SlotKind::Dry),
                            wet: range(SlotKind::Output),
                            bypass: range(SlotKind::Bypass),
                        },
                        Processor::Gain(2.),
                    ],
                    sends: vec![BusSend {
                        bus: None,
                        gain: 1.,
                    }],
                    tail_frames: 0,
                }],
                vec![Some(0), Some(0)],
            )
        },
    );
    // Note 60 is 0.5 through the block: wet 2.0 at first.
    rt.trigger(input(61), 61, 1.).unwrap();
    // Bypassed: the dry 0.25 only.
    assert!(close(level(&mut rt), [0.25; 2]), "{:?}", level(&mut rt));
    rt.trigger(input(60), 60, 1.).unwrap();
    // Dry level 1.0 (unity), output gain 0: the dry 0.5 plus the still bypassed 0.25.
    let [l, _] = level(&mut rt);
    assert!((l - 0.75).abs() < 1e-3, "{l}");
}

#[test]
fn block_fuel_spreads_callbacks_over_blocks_without_changing_the_result() {
    let source = "on init
                    declare $i
                  end on
                  on note
                    while ($i < 40)
                      inc($i)
                    end while
                  end on";
    let mut rt = runtime(source);
    rt.set_behavior_block_fuel(16);
    rt.trigger(input(60), 60, 1.).unwrap();
    // 40 loop iterations at 16 instructions a block take several blocks.
    assert_eq!(level(&mut rt), [0.; 2], "the note waits for its callback");
    for _ in 0..40 {
        level(&mut rt);
    }
    assert!(close(level(&mut rt), [0.5; 2]));
}
