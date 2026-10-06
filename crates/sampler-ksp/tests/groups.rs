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
        "on init allow_group(0) end on",
    ] {
        assert!(compile(source).is_err());
    }
    assert!(
        sampler_ksp::compile(
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
        .is_err()
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
