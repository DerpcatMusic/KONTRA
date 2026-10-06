use sampler_core::*;
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
    let behavior_cells = plan.behavior_local_count() * 8;
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
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(i32::from(key)),
    }
}
#[test]
fn generated_ids_in_arrays_and_expressions_keep_each_original_release_owner() {
    let source = "on init declare %ids[8] declare $duration := 0 declare polyphonic $slot end on
      on note
        $slot := ($EVENT_NOTE - 60) * 4
        %ids[$slot] := $EVENT_ID
        ignore_event($EVENT_ID)
        %ids[$slot + 1] := play_note(60,127,0,-1)
        %ids[$slot + 2] := play_note(60,127,0,$duration)
        %ids[$slot + 3] := 7 + play_note(60,127,0,125) - 7
      end on
      on release
        %ids[$slot] := $EVENT_ID
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
        "on init declare $ID := play_note(60,127,0,0) end on",
        "on note if (1 or play_note(60,127,9,0)) exit end if end on",
    ] {
        assert!(compile(source).is_err());
    }
    let nested = format!(
        "on note {}0{} end on",
        "play_note(60,127,0,".repeat(65),
        ")".repeat(65)
    );
    assert!(compile(&nested).is_err());
}
