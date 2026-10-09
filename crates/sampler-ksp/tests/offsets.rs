use sampler_core::*;
use std::num::NonZeroU32;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(source: Option<&str>, rate: u32, asset_rate: u32, playback: Playback) -> Runtime {
    let pcm = (0..192)
        .map(|i| {
            [
                (i as f32 * 0.23).sin() * 0.25,
                (i as f32 * 0.17).cos() * 0.25,
            ]
        })
        .collect::<Vec<_>>();
    let mut plan = Prepared::new(
        rate,
        vec![Pcm::new(asset_rate, pcm.into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback,
        }],
        1,
    )
    .unwrap();
    if let Some(source) = source {
        plan = sampler_ksp::compile(
            source,
            rate,
            sampler_ksp::Limits {
                source_bytes: 4096,
                instructions: 128,
                variables: 2,
                array_cells: 0,
            },
            &[],
        )
        .unwrap()
        .bind(plan)
        .unwrap();
    }
    let cells = plan.behavior_local_count();
    Runtime::new(
        plan,
        Limits {
            notes: 2,
            channels: 0,
            performances: 1,
            families: 1,
            voices: 1,
            expressions: 2,
            decisions: 0,
            commands: 2,
            behaviors: 1,
            behavior_cells: cells,
            behavior_fuel: 128,
            note_cells: 0,
        },
    )
    .unwrap()
}

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}

#[test]
fn source_offsets_match_elapsed_source_audio_without_shifting_the_event_clock() {
    for rate in [48000, 96000] {
        for asset_rate in [48000, 96000] {
            for direction in [Direction::Forward, Direction::Reverse] {
                for shape in [
                    LoopShape::Wrap,
                    LoopShape::PingPong,
                    LoopShape::Crossfade { frames: 8 },
                ] {
                    for transpose in [0., 12.] {
                        for offset in [0u32, 125, 1500, 2500, 5000] {
                            let playback = Playback {
                                start: 4,
                                end: Some(164),
                                direction,
                                loop_range: Some(Loop {
                                    start: 64,
                                    end: 112,
                                    shape,
                                    mode: LoopMode::Continuous,
                                    passes: NonZeroU32::new(2),
                                }),
                                transpose_semitones: transpose,
                                loop_slots: [None; 8],
                            };
                            let source = format!(
                                "on init declare $offset := {offset} end on
                                 on note ignore_event($EVENT_ID)
                                 play_note(60, 127, $offset, 0) end on"
                            );
                            // Independent reference: render from the view's origin,
                            // then discard elapsed source time. Beyond the first
                            // outward loop edge, the reference is the unlooped view.
                            let mut reference_view = playback;
                            let edge = if direction == Direction::Forward {
                                108
                            } else {
                                100
                            };
                            if u64::from(offset) * u64::from(asset_rate) >= edge * 1_000_000 {
                                reference_view.loop_range = None;
                            }
                            let mut reference = runtime(None, rate, asset_rate, reference_view);
                            let mut expected = [[0.; 2]; 1024];
                            reference.trigger(input(), 60, 1.).unwrap();
                            reference.render(&mut expected).unwrap();
                            let elapsed = offset as usize * rate as usize
                                / 1_000_000
                                / if transpose == 0. { 1 } else { 2 };
                            for block in [1, 7, 64] {
                                let mut rt = runtime(Some(&source), rate, asset_rate, playback);
                                support::without_heap(|| {
                                    rt.trigger(input(), 60, 1.).unwrap();
                                    assert_eq!(rt.pending_commands(), 0);
                                    let mut audio = [[0.; 2]; 512];
                                    for chunk in audio.chunks_mut(block) {
                                        rt.render(chunk).unwrap();
                                    }
                                    assert_eq!(
                                        audio,
                                        expected[elapsed..elapsed + audio.len()],
                                        "{rate}/{asset_rate}, {direction:?}, {shape:?}, pitch {transpose}, offset {offset}, block {block}"
                                    );
                                    rt.flush_behaviors(|_, _, outcome| {
                                        assert_eq!(outcome, Outcome::Finished);
                                        true
                                    });
                                    rt.note_off(input(), None).unwrap();
                                    rt.flush_ended(|_| true);
                                    assert_eq!(
                                        (rt.note_count(), rt.voice_count(), rt.family_count()),
                                        (0, 0, 0)
                                    );
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn fractional_source_offsets_keep_resampler_history_at_every_partition() {
    let source = "on init declare $offset := 125 end on
        on note ignore_event($EVENT_ID)
        play_note(60,127,($offset / 5) * 5,$offset - 125) end on";
    for direction in [Direction::Forward, Direction::Reverse] {
        let view = Playback {
            direction,
            ..Playback::default()
        };
        let mut reference = runtime(None, 48000, 12000, view);
        let mut expected = [[0.; 2]; 1030];
        reference.trigger(input(), 60, 1.).unwrap();
        reference.render(&mut expected).unwrap();
        for block in [1, 7, 64] {
            let mut rt = runtime(Some(source), 48000, 12000, view);
            support::without_heap(|| {
                rt.trigger(input(), 60, 1.).unwrap();
                let mut audio = [[0.; 2]; 1024];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                // 125 us = 1.5 source frames = six output frames. Truncating or
                // rounding the source offset, or resetting its guards, fails this.
                assert_eq!(audio, expected[6..]);
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    true
                });
                rt.note_off(input(), None).unwrap();
                rt.flush_ended(|_| true);
                assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
            });
        }
    }
}

#[test]
fn invalid_evaluated_offsets_fault_before_child_or_duration_publication() {
    for expression in ["-1", "2147483647 + 1"] {
        let source = format!(
            "on note ignore_event($EVENT_ID)
             play_note(60,127,{expression},1000) end on"
        );
        let mut rt = runtime(Some(&source), 48000, 48000, Playback::default());
        support::without_heap(|| {
            rt.trigger(input(), 60, 1.).unwrap();
            assert_eq!(
                (
                    rt.note_count(),
                    rt.voice_count(),
                    rt.family_count(),
                    rt.pending_commands()
                ),
                (1, 0, 0, 0)
            );
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Fault(Error::InvalidInput));
                true
            });
            rt.note_off(input(), None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}
