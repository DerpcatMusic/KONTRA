use sampler_core::{
    Duration, Envelope, Error, Inheritance, Input, Instruction, Limits, Outcome, Pcm, PlanError,
    Playback, Prepared, Program, Protocol, Region, Runtime, Velocity, WaitLifetime,
};
mod support;

fn limits() -> Limits {
    Limits {
        notes: 16,
        channels: 2,
        performances: 1,
        families: 16,
        decisions: 0,
        expressions: 16,
        voices: 16,
        commands: 16,
        behaviors: 2,
        behavior_fuel: 8,
        behavior_cells: 2,
    }
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn plan(value: f32, code: Option<Vec<Instruction>>) -> Prepared {
    let prepared = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[value; 2]; 64].into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 2).unwrap(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap();
    if let Some(code) = code {
        prepared
            .with_programs(
                vec![
                    Program::new(code)
                        .unwrap()
                        .with_wait_lifetime(WaitLifetime::Callback),
                ],
                Some(0),
            )
            .unwrap()
    } else {
        prepared
    }
}

#[test]
fn old_callbacks_and_tails_keep_their_plan_across_sample_exact_adoption() {
    let play = Instruction::Play {
        transpose: 0,
        velocity: Velocity::Fixed(1.),
        inheritance: Inheritance::Independent,
        duration: Duration::Frames(4),
    };
    for block in [1, 3, 7, 20] {
        let old = plan(
            0.25,
            Some(vec![play, Instruction::Wait(8), play, Instruction::End]),
        );
        let (mut rt, mut control) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
        let old_plan = rt.active_plan();
        let request = control.submit(Box::new(plan(0.5, None))).unwrap();
        let mut audio = [[0.; 2]; 20];
        support::without_heap(|| {
            let old_note = rt.trigger(input(1), 60, 1.).unwrap();
            rt.release_at(old_note, 3).unwrap();
            rt.render(&mut audio[..2]).unwrap();
            assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
            let new_plan = rt.active_plan();
            assert_ne!(old_plan, new_plan);
            let new_note = rt.trigger(input(2), 60, 1.).unwrap();
            rt.release_at(new_note, 10).unwrap();
            assert_eq!(rt.note_plan(old_note), Ok(old_plan));
            assert_eq!(rt.note_plan(new_note), Ok(new_plan));
            for chunk in audio[2..].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            rt.flush_ended(|_| false);
            assert_eq!(rt.collect_retired_plans(), 0);
            let mut callbacks = 0;
            rt.flush_behaviors(|_, origin, outcome| {
                assert_eq!((origin, outcome), (old_note, Outcome::Finished));
                callbacks += 1;
                true
            });
            rt.flush_ended(|_| false);
            assert_eq!(
                rt.collect_retired_plans(),
                0,
                "rejected terminal retains the old plan"
            );
            let mut terminals = 0;
            rt.flush_ended(|_| {
                terminals += 1;
                true
            });
            assert_eq!((callbacks, terminals, rt.note_count()), (1, 2, 0));
            assert_eq!(rt.collect_retired_plans(), 1);
            assert_eq!(rt.plan_request(old_plan), Err(Error::StaleHandle));
            assert_eq!(rt.plan_request(new_plan), Ok(request));
            assert_eq!(rt.plan_count(), 1);
        });
        let expected = [
            0.25, 0.25, 0.75, 0.75, 0.75, 0.625, 0.5, 0.5, 0.75, 0.75, 0.75, 0.5, 0.25, 0.125, 0.,
            0., 0., 0., 0., 0.,
        ];
        assert_eq!(audio, expected.map(|value| [value; 2]), "block {block}");
        let retired = control.retired().unwrap();
        assert_eq!(retired.request, 0);
        assert_eq!(retired.prepared.sample_count(), 1);
        assert!(control.retired().is_none());
    }
}

#[test]
fn bounded_transfers_preserve_pending_and_active_ownership_until_capacity_returns() {
    let (mut rt, mut control) =
        Runtime::with_plan_updates(plan(0.25, None), limits(), 2, 1).unwrap();
    let first = control.submit(Box::new(plan(0.5, None))).unwrap();
    let mut old_note = None;
    support::without_heap(|| {
        old_note = Some(rt.trigger(input(1), 60, 1.).unwrap());
        assert_eq!(rt.poll_plan_update(), Ok(Some(first)));
        assert_eq!(rt.plan_count(), 2);
    });
    let active = rt.active_plan();
    let second = control.submit(Box::new(plan(0.75, None))).unwrap();
    let fourth = Box::new(plan(1., None));
    let original_address = &*fourth as *const Prepared;
    let rejected = control.submit(fourth).unwrap_err();
    assert_eq!(rejected.reason, PlanError::Capacity);
    assert_eq!(&*rejected.prepared as *const Prepared, original_address);
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Err(PlanError::Capacity));
        assert_eq!(rt.active_plan(), active);
        rt.panic();
        rt.flush_ended(|_| false);
        assert_eq!(rt.poll_plan_update(), Err(PlanError::Capacity));
        rt.flush_ended(|origin| {
            assert_eq!(origin, input(1));
            true
        });
        assert_eq!(rt.note_plan(old_note.unwrap()), Err(Error::StaleHandle));
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(
            rt.poll_plan_update(),
            Err(PlanError::Capacity),
            "full return queue blocks adoption without dropping the queued plan"
        );
        assert_eq!(rt.active_plan(), active);
    });
    assert_eq!(control.retired().unwrap().request, 0);
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(second)));
        assert_eq!(rt.plan_request(active), Err(Error::StaleHandle));
    });
    assert_eq!(control.retired().unwrap().request, first);
    assert_eq!(control.submit(rejected.prepared).unwrap(), 3);
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(3)));
    });
    assert_eq!(control.retired().unwrap().request, second);
    drop(control);
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Err(PlanError::Disconnected));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.panic();
    });
}

#[test]
fn plan_validation_and_reserved_callback_cells_are_control_side_contracts() {
    let (mut rt, mut control) =
        Runtime::with_plan_updates(plan(0.25, None), limits(), 2, 1).unwrap();
    let wrong_rate = Box::new(Prepared::new(44100, vec![], vec![], 0).unwrap());
    let rejected = control.submit(wrong_rate).unwrap_err();
    assert_eq!(rejected.reason, PlanError::SampleRate);
    let too_wide = Box::new(plan(0.5, Some(vec![Instruction::ReadKey { local: 1 }])));
    assert_eq!(
        control.submit(too_wide).unwrap_err().reason,
        PlanError::LocalCapacity
    );
    let valid = plan(
        0.5,
        Some(vec![
            Instruction::ReadKey { local: 0 },
            Instruction::Wait(2),
            Instruction::End,
        ]),
    );
    assert_eq!(control.submit(Box::new(valid)).unwrap(), 1);
    support::without_heap(|| {
        assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 3]).unwrap();
        let mut completed = None;
        rt.flush_behaviors(|id, owner, outcome| {
            assert_eq!((owner, outcome), (note, Outcome::Finished));
            completed = Some(id);
            false
        });
        assert_eq!(rt.behavior_local(completed.unwrap(), 0), Ok(60));
        rt.flush_behaviors(|_, _, _| true);
        rt.release(note).unwrap();
        rt.flush_ended(|_| true);
    });
    let retired = control.retired().unwrap();
    let another = Runtime::new(*retired.prepared, limits()).unwrap();
    assert_eq!(
        rt.plan_request(another.active_plan()),
        Err(Error::StaleHandle)
    );
    drop(rt);
    assert_eq!(
        control
            .submit(Box::new(plan(0.25, None)))
            .unwrap_err()
            .reason,
        PlanError::Disconnected
    );
    assert!(matches!(
        Runtime::with_plan_updates(plan(0., None), limits(), 1, 1),
        Err(Error::InvalidInput)
    ));
}

#[test]
fn control_thread_destroys_returned_assets_while_audio_only_moves_ownership() {
    let (mut rt, mut control) = Runtime::with_plan_updates(
        plan(0., None),
        Limits {
            decisions: 1,
            ..limits()
        },
        2,
        1,
    )
    .unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    let timeout = std::time::Duration::from_secs(5);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            for request in 1..=64 {
                assert_eq!(
                    control
                        .submit(Box::new(
                            plan(request as f32 / 64., None)
                                .with_variation(
                                    vec![sampler_core::Sequence {
                                        takes: 1,
                                        policy: sampler_core::TakePolicy::Shuffle { seed: 41 },
                                        scope: sampler_core::SequenceScope::Global,
                                        capacity: 1
                                    }],
                                    vec![Some(sampler_core::Take {
                                        sequence: 0,
                                        index: 0
                                    })],
                                    1,
                                    1,
                                )
                                .unwrap()
                        ))
                        .unwrap(),
                    request
                );
                ready_tx.send(()).unwrap();
                done_rx.recv_timeout(timeout).unwrap();
                // This destructor is on the control thread, outside the audio guard.
                assert_eq!(control.retired().unwrap().request, request - 1);
            }
        });
        for request in 1..=64 {
            ready_rx.recv_timeout(timeout).unwrap();
            support::without_heap(|| {
                assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
                rt.trigger(input(request as i32), 60, 1.).unwrap();
                let mut frame = [[0.; 2]; 1];
                rt.render(&mut frame).unwrap();
                assert_eq!(frame, [[request as f32 / 64.; 2]]);
                rt.panic();
                rt.flush_ended(|_| true);
                rt.collect_retired_plans();
            });
            done_tx.send(()).unwrap();
        }
    });
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn shared_pcm_keeps_its_buffer_across_plan_adoption_and_control_side_retirement() {
    let frames: Box<[sampler_core::Frame]> = Box::from([[0.25, -0.25]; 4]);
    let original = frames.as_ptr();
    let pcm = Pcm::new(48000, frames).unwrap();
    assert_eq!(pcm.frames().as_ptr(), original);
    let alias = pcm.clone();
    assert_eq!(alias.frames().as_ptr(), original);
    assert_eq!(alias.sample_rate(), 48000);
    let prepare = |gain| {
        Prepared::new(
            48000,
            vec![pcm.clone()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain,
                envelope: Envelope::default(),
                playback: Playback {
                    loop_range: Some(sampler_core::Loop {
                        start: 0,
                        end: 4,
                        mode: sampler_core::LoopMode::Continuous,
                    }),
                    ..Playback::default()
                },
            }],
            1,
        )
        .unwrap()
    };
    let (mut rt, mut control) = Runtime::with_plan_updates(prepare(1.0), limits(), 2, 1).unwrap();
    let request = control.submit(Box::new(prepare(0.5))).unwrap();
    drop(pcm);
    drop(alias);
    support::without_heap(|| {
        let old = rt.trigger(input(1), 60, 1.0).unwrap();
        let mut frame = [[0.0; 2]; 1];
        rt.render(&mut frame).unwrap();
        assert_eq!(frame, [[0.25, -0.25]]);
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        rt.trigger(input(2), 60, 1.0).unwrap();
        rt.render(&mut frame).unwrap();
        assert_eq!(frame, [[0.375, -0.375]]);
        rt.release(old).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    // Destroy the old plan on control; the active plan owns the same PCM buffer.
    drop(control.retired().unwrap());
    support::without_heap(|| {
        let mut audio = [[0.0; 2]; 128];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.125, -0.125]; 128]);
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
    });
    for (rate, frames) in [
        (0, Box::from([[0.0; 2]])),
        (48000, Box::from([])),
        (48000, Box::from([[f32::NAN, 0.0]])),
        (48000, Box::from([[0.0, f32::INFINITY]])),
    ] {
        assert!(matches!(Pcm::new(rate, frames), Err(Error::InvalidInput)));
    }
}

#[test]
fn tuning_adoption_preserves_held_notes_and_later_generated_children() {
    use sampler_core::Tuning;
    let pcm = Pcm::new(
        48000,
        (0..4096)
            .map(|i| {
                let x = (i as f64 * 0.07).sin() as f32 * 0.25;
                [x, -x]
            })
            .collect(),
    )
    .unwrap();
    let prepare = |offset, tracked: bool| {
        let tuning = Tuning::new([offset; 128]).unwrap();
        Prepared::new_tuned(
            48000,
            vec![pcm.clone()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: tracked.then_some(60),
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 1.0,
                envelope: Envelope::default(),
                playback: Playback {
                    start: 512,
                    transpose_semitones: if tracked { 0.0 } else { offset },
                    ..Playback::default()
                },
            }],
            1,
            &tuning,
        )
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![
                    Instruction::Wait(16),
                    Instruction::Play {
                        transpose: 0,
                        velocity: Velocity::Fixed(1.0),
                        inheritance: Inheritance::Independent,
                        duration: Duration::Frames(64),
                    },
                    Instruction::End,
                ])
                .unwrap(),
            ],
            None,
        )
        .unwrap()
    };
    for partition in [1, 7, 64, 256] {
        let (mut rt, mut control) =
            Runtime::with_plan_updates(prepare(0.25, true), limits(), 2, 1).unwrap();
        let mut old_reference = Runtime::new(prepare(0.25, false), limits()).unwrap();
        let mut new_reference = Runtime::new(prepare(-0.5, false), limits()).unwrap();
        let request = control.submit(Box::new(prepare(-0.5, true))).unwrap();
        support::without_heap(|| {
            let old = rt.trigger(input(1), 60, 1.0).unwrap();
            let reference = old_reference.trigger(input(1), 60, 1.0).unwrap();
            rt.render(&mut [[0.0; 2]; 32]).unwrap();
            old_reference.render(&mut [[0.0; 2]; 32]).unwrap();
            assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
            rt.trigger(input(2), 60, 1.0).unwrap();
            new_reference.trigger(input(2), 60, 1.0).unwrap();
            rt.start_behavior(old, 0).unwrap();
            old_reference.start_behavior(reference, 0).unwrap();
            let mut audio = [[0.0; 2]; 256];
            let mut expected_old = [[0.0; 2]; 256];
            let mut expected_new = [[0.0; 2]; 256];
            for ((out, a), b) in audio
                .chunks_mut(partition)
                .zip(expected_old.chunks_mut(partition))
                .zip(expected_new.chunks_mut(partition))
            {
                rt.render(out).unwrap();
                old_reference.render(a).unwrap();
                new_reference.render(b).unwrap();
            }
            for ((out, a), b) in audio.iter().zip(expected_old).zip(expected_new) {
                for c in 0..2 {
                    assert!((out[c] - (a[c] + b[c])).abs() < 0.000001);
                }
            }
            assert_eq!(rt.collect_retired_plans(), 0);
            rt.panic();
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, Outcome::Finished);
                true
            });
            rt.flush_ended(|_| true);
            assert_eq!(rt.collect_retired_plans(), 1);
        });
        drop(control.retired().unwrap());
    }
}
