use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, ReleaseOptions, Runtime,
    ScriptInstanceId, Trigger,
};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
fn compile(source: &str) -> Result<sampler_ksp::Script, sampler_ksp::Error> {
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
}
fn runtime(source: &str) -> Runtime {
    let script = compile(source).unwrap();
    let note_cells = script.note_cells() * 8;
    let pcm =
        [0.125, 0.25, -0.125, -0.25].map(|v| Pcm::new(48000, Box::from([[v; 2]; 32])).unwrap());
    let regions = (0..4)
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 61,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let plan = script
        .bind(
            Prepared::new(48000, pcm.into(), regions, 8)
                .unwrap()
                .with_groups(2, vec![Some(0), Some(1), Some(0), Some(1)])
                .unwrap()
                .with_releases(
                    vec![
                        Trigger::Attack,
                        Trigger::Attack,
                        Trigger::GateRelease,
                        Trigger::GateRelease,
                    ],
                    ReleaseOptions::default(),
                    ReleaseOptions::default(),
                )
                .unwrap(),
        )
        .unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 16,
            voices: 24,
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
#[test]
fn source_group_selection_routes_overlapping_attacks_and_release_layers_at_first_wait() {
    let source = "on init declare $count end on
      on note
        $count := $NUM_GROUPS
        disallow_group($ALL_GROUPS)
        allow_group($EVENT_NOTE - 60)
        wait(125)
        disallow_group($ALL_GROUPS) { late note edit is ignored }
      end on
      on release
        disallow_group($ALL_GROUPS)
        allow_group(1 - ($EVENT_NOTE - 60))
        wait(125)
        disallow_group($ALL_GROUPS) { cannot rewrite committed automatic release }
      end on";
    for block in [1, 7, 64] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let a = rt.trigger(input(60), 60, 1.).unwrap();
            let b = rt.trigger(input(61), 61, 1.).unwrap();
            let plan = rt.active_plan();
            assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(2));
            let mut audio = [[0.; 2]; 8];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio, [[0.375; 2]; 8]);
            assert!(rt.note_group_allowed(a, 0).unwrap());
            assert!(!rt.note_group_allowed(a, 1).unwrap());
            assert!(rt.note_group_allowed(b, 1).unwrap());
            rt.key_up(a, None).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio, [[0.; 2]; 8]); // b attack 0.25 and a release -0.25.
            assert!(!rt.note_group_allowed(a, 1).unwrap());
            rt.key_up(b, None).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio, [[-0.375; 2]; 8]);
            rt.panic();
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
        });
    }
}
#[test]
fn release_generated_notes_capture_current_groups_and_bad_indices_fault_atomically() {
    let source = "on note
        ignore_event($EVENT_ID)
      end on
      on release
        disallow_group($ALL_GROUPS)
        allow_group($NUM_GROUPS - 1)
        play_note(60,127,0,0)
        disallow_group($ALL_GROUPS)
        allow_group(0)
        play_note(60,127,0,0)
      end on";
    let mut rt = runtime(source);
    support::without_heap(|| {
        let n = rt.trigger(input(60), 60, 1.).unwrap();
        rt.key_up(n, None).unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.375; 2]; 8]);
    });
    for value in [-2, -1, 2, i32::MAX] {
        let source = format!(
            "on note disallow_group($ALL_GROUPS) allow_group(0) allow_group({value}) end on"
        );
        let mut rt = runtime(&source);
        support::without_heap(|| {
            let n = rt.trigger(input(60), 60, 1.).unwrap();
            assert!(rt.note_group_allowed(n, 0).unwrap());
            assert!(!rt.note_group_allowed(n, 1).unwrap());
            let mut faults = 0;
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(
                    outcome,
                    sampler_core::Outcome::Fault(sampler_core::Error::InvalidInput)
                );
                faults += 1;
                true
            });
            assert_eq!(faults, 1);
            assert_eq!(rt.voice_count(), 0);
        });
    }
    for source in [
        "on init declare $ALL_GROUPS end on",
        "on init declare $NUM_GROUPS end on",
    ] {
        assert!(compile(source).is_err());
    }
    // v2: group edits outside note callbacks warn and do nothing.
    assert!(
        !compile("on init allow_group(0) end on")
            .unwrap()
            .warnings()
            .is_empty()
    );
    assert!(
        !sampler_ksp::compile(
            "on init declare ui_button $B end on on ui_control($B) allow_group(0) end on",
            48000,
            sampler_ksp::Limits {
                source_bytes: 1024,
                instructions: 64,
                variables: 4,
                array_cells: 0
            },
            &[("$B", sampler_core::ControlId(1))]
        )
        .unwrap()
        .warnings()
        .is_empty()
    );
}

#[test]
fn release_group_commit_survives_pedal_hold_and_late_callback_edits() {
    let source = "on note disallow_group($ALL_GROUPS) allow_group(0) end on
        on release disallow_group($ALL_GROUPS) allow_group(1) wait(125)
          disallow_group($ALL_GROUPS) allow_group(0) end on";
    let mut rt = runtime(source);
    support::without_heap(|| {
        let channel = rt.register_channel(input(60).channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let n = rt.trigger(input(60), 60, 1.).unwrap();
        rt.key_up(n, None).unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.125; 2]; 8]);
        assert!(rt.note_group_allowed(n, 0).unwrap());
        assert!(!rt.note_group_allowed(n, 1).unwrap());
        assert!(!rt.forward_release_groups(n).unwrap());
        rt.sustain(channel, false).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[-0.25; 2]; 8]);
    });
}

#[test]
fn suppressed_release_waits_and_forwards_once_with_groups_and_pedals_at_exact_samples() {
    use sampler_core::{Event, KeyRelease, Outcome, ReleaseCause, ReleaseReserve};
    let source = "on note disallow_group($ALL_GROUPS) allow_group(0) end on
      on release
        ignore_event($EVENT_ID)
        wait(125)
        disallow_group($ALL_GROUPS) allow_group(1)
        note_off($EVENT_ID,42)
        wait(125)
        disallow_group($ALL_GROUPS) allow_group(0)
        note_off($EVENT_ID)
      end on";
    for pedal_up in [5, 22] {
        for block in [1, 7, 64] {
            let mut rt = runtime(source);
            support::without_heap(|| {
                let channel = rt.register_channel(input(60).channel_address()).unwrap();
                rt.sustain(channel, true).unwrap();
                let note = rt.trigger(input(60), 60, 1.).unwrap();
                rt.schedule_event(2, Event::KeyUp(note, Some(0.25)))
                    .unwrap();
                rt.schedule_event(pedal_up, Event::Sustain(channel, false))
                    .unwrap();
                let mut audio = [[0.; 2]; 32];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                let end = 11usize.max(pedal_up as usize);
                assert!(audio[..end].iter().all(|f| *f == [0.125; 2]));
                assert!(audio[end..].iter().all(|f| *f == [-0.25; 2]));
                let context = rt.release_context(note).unwrap();
                assert_eq!(
                    context.key.unwrap(),
                    KeyRelease {
                        at: 2,
                        velocity: Some(0.25),
                        cause: ReleaseCause::KeyUp
                    }
                );
                assert_eq!(context.gate.unwrap().at, end as u64);
                assert!(!rt.resume_release(note).unwrap());
                rt.render(&mut [[0.; 2]; 64]).unwrap();
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    true
                });
                rt.flush_ended(|_| true);
                assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
                assert_eq!(rt.release_reserve(), ReleaseReserve::default());
            });
        }
    }
    let mut rt = runtime("on release wait(0) ignore_event($EVENT_ID) end on");
    let note = rt.trigger(input(60), 60, 1.).unwrap();
    rt.key_up(note, None).unwrap();
    assert!(rt.release_context(note).unwrap().gate.is_some()); // Forwarded cannot be ignored later.
}

#[test]
fn faulted_suppressed_release_cannot_retain_a_gate_or_reserved_layers() {
    let mut rt = runtime("on release ignore_event($EVENT_ID) wait(-1) end on");
    support::without_heap(|| {
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        rt.key_up(note, None).unwrap();
        assert_eq!(
            rt.release_context(note).unwrap().gate.unwrap().cause,
            sampler_core::ReleaseCause::BehaviorFault
        );
        assert_eq!(
            rt.release_reserve(),
            sampler_core::ReleaseReserve::default()
        );
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(
                outcome,
                sampler_core::Outcome::Fault(sampler_core::Error::InvalidInput)
            );
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
}

/// Una Corda's shape: the script ignores the host note and its release and
/// plays its own notes. The ignored host event has nothing left to sound, so
/// it ends (as a Kontakt event does) instead of holding its gate forever, and
/// its release layers never fire.
#[test]
fn an_ignored_silent_note_with_a_held_release_retires() {
    let mut rt = runtime(
        "on note ignore_event($EVENT_ID) end on
         on release ignore_event($EVENT_ID) end on",
    );
    let note = rt.trigger(input(60), 60, 1.).unwrap();
    let mut audio = [[0.; 2]; 8];
    rt.render(&mut audio).unwrap();
    rt.flush_behaviors(|_, _, _| true);
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 1, "a held key keeps its note");
    rt.key_up(note, None).unwrap();
    rt.render(&mut audio).unwrap();
    rt.flush_behaviors(|_, _, _| true);
    assert!(rt.release_context(note).unwrap().gate.is_none(), "release held");
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(audio, [[0.; 2]; 8]);
    assert_eq!(rt.release_reserve(), sampler_core::ReleaseReserve::default());
}

#[test]
fn a_scheduled_script_noteoff_can_forward_a_later_suppressed_physical_release() {
    use sampler_core::{Event, ScriptInstanceId};
    let source = "on init declare $released end on
      on note disallow_group($ALL_GROUPS) allow_group(0) note_off($EVENT_ID,125) end on
      on release inc($released) ignore_event($EVENT_ID)
        disallow_group($ALL_GROUPS) allow_group(1)
      end on";
    for block in [1, 7, 64] {
        let mut rt = runtime(source);
        support::without_heap(|| {
            let note = rt.trigger(input(60), 60, 1.).unwrap();
            rt.schedule_event(2, Event::KeyUp(note, None)).unwrap();
            let mut audio = [[0.; 2]; 12];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(audio[..6], [[0.125; 2]; 6]);
            assert_eq!(audio[6..], [[-0.25; 2]; 6]);
            let context = rt.release_context(note).unwrap();
            assert_eq!(context.key.unwrap().at, 2);
            assert_eq!(context.gate.unwrap().at, 6);
            assert!(!rt.input_held(note).unwrap());
            assert_eq!(
                rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
                Ok(1)
            );
            rt.panic();
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

/// Kontakt's NO_SYS_SCRIPT_PEDAL bypasses the engine's CC64 sustain: the
/// script owns the pedal, so key-up closes the gate while the pedal is down.
#[test]
fn no_sys_script_pedal_disables_native_sustain() {
    for (source, owned) in [
        ("on init SET_CONDITION(NO_SYS_SCRIPT_PEDAL) end on", true),
        ("on init end on", false),
    ] {
        let mut rt = runtime(source);
        let channel = rt.register_channel(input(60).channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let note = rt.trigger(input(60), 60, 1.).unwrap();
        rt.key_up(note, None).unwrap();
        assert_eq!(rt.pedals(channel).unwrap(), (true, false));
        assert_eq!(rt.release_context(note).unwrap().gate.is_some(), owned, "{source}");
        rt.sustain(channel, false).unwrap();
        assert!(rt.release_context(note).unwrap().gate.is_some(), "{source}");
    }
}
