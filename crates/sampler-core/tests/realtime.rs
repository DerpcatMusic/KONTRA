use sampler_core::{Error, Expression, Inheritance, Input, Limits, Pcm, Protocol, Runtime};
mod support;

#[test]
fn ownership_pressure_render_and_retirement_do_no_heap_work() {
    let pcm = [Pcm {
        rate: 48000,
        frames: Box::from([[0.25; 2]; 16]),
    }];
    let mut rt = fixture_runtime(
        48000,
        &pcm,
        Limits {
            notes: 8,
            channels: 4,
            expressions: 4,
            families: 4,
            voices: 4,
            commands: 4,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Midi2,
        port: 0,
        group: 15,
        channel: 15,
        key: 60,
        external_id: None,
    };
    support::without_heap(|| {
        for _ in 0..100 {
            let root = rt.note_on(input, 60, 1.0 / 65535.0).unwrap();
            let child = rt.child(root, 67, 1.0, false, Inheritance::Linked).unwrap();
            let snapshot = rt
                .child(root, 72, 1.0, true, Inheritance::Snapshot)
                .unwrap();
            let reused = rt.note_on(input, 60, 1.0).unwrap();
            let e = rt.expression_id(root).unwrap();
            rt.set_expression(
                e,
                Expression {
                    pressure: u32::MAX - 1,
                    gain: 0.5,
                    ..Expression::default()
                },
            )
            .unwrap();
            let detached = rt.detach_expression(child).unwrap();
            assert_eq!(rt.expression(detached).unwrap().pressure, u32::MAX - 1);
            assert_eq!(rt.note_on(input, 60, 1.0), Err(Error::Capacity));
            let family = rt.create_family(child).unwrap();
            let a = rt
                .start_family(
                    family,
                    0,
                    rt.now() + 1,
                    1.0,
                    sampler_core::Envelope::default(),
                    sampler_core::Playback::default(),
                )
                .unwrap();
            rt.start_family(
                family,
                0,
                rt.now() + 2,
                1.0,
                sampler_core::Envelope::default(),
                sampler_core::Playback::default(),
            )
            .unwrap();
            rt.finish_family(family).unwrap();
            rt.start(snapshot, 0, rt.now() + 3, 1.0).unwrap();
            rt.start(reused, 0, rt.now() + 4, 1.0).unwrap();
            assert_eq!(rt.release_at(root, rt.now() + 5), Err(Error::Capacity));
            rt.release(root).unwrap(); // Cancels linked release; detached family survives.
            rt.stop_voice(a).unwrap();
            assert_eq!(rt.family_voice_count(family), Ok(1));
            rt.render(&mut [[0.0; 2]; 8]).unwrap();
            rt.stop_family(family).unwrap();
            rt.panic();
            rt.flush_ended(|_| false);
            assert_eq!(rt.note_count(), 2);
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.family_count(),
                    rt.voice_count(),
                    rt.expression_count(),
                    rt.pending_commands()
                ),
                (0, 0, 0, 0, 0)
            );
        }
    });
}

#[test]
fn pedal_and_expression_timeline_pressure_do_no_heap_work() {
    use sampler_core::Event;
    let pcm = [Pcm {
        rate: 48000,
        frames: Box::from([[0.25; 2]; 64]),
    }];
    let mut rt = fixture_runtime(
        48000,
        &pcm,
        Limits {
            notes: 4,
            channels: 1,
            expressions: 4,
            families: 4,
            voices: 4,
            commands: 4,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Midi2,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    support::without_heap(|| {
        let ch = rt.register_channel(input.channel_address()).unwrap();
        for _ in 0..100 {
            let n = rt.note_on(input, 60, 1.0).unwrap();
            rt.start(n, 0, rt.now(), 1.0).unwrap();
            rt.schedule_event(rt.now() + 1, Event::Sostenuto(ch, true))
                .unwrap();
            rt.schedule_event(rt.now() + 2, Event::KeyUp(n)).unwrap();
            rt.schedule_event(
                rt.now() + 3,
                Event::Expression(
                    n,
                    Expression {
                        gain: 0.5,
                        ..Expression::default()
                    },
                ),
            )
            .unwrap();
            rt.schedule_event(rt.now() + 4, Event::Sostenuto(ch, false))
                .unwrap();
            assert_eq!(rt.release_at(n, rt.now() + 5), Err(Error::Capacity));
            rt.render(&mut [[0.0; 2]; 8]).unwrap();
            rt.flush_ended(|_| false);
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.expression_count(),
                    rt.pending_commands()
                ),
                (0, 0, 0)
            );
        }
    });
}

fn fixture_runtime(rate: u32, pcm: &[Pcm], limits: Limits) -> Result<Runtime, sampler_core::Error> {
    Runtime::new(
        sampler_core::Prepared::new(rate, pcm.to_vec(), Vec::new(), 0)?,
        limits,
    )
}

#[test]
fn prepared_native_selection_and_owned_asset_retirement_do_no_heap_work() {
    use sampler_core::{Prepared, Region};
    let plan = Prepared::new(
        48000,
        vec![Pcm {
            rate: 48000,
            frames: Box::new([[0.25; 2]; 8]),
        }],
        vec![
            Region {
                playback: sampler_core::Playback::default(),
                envelope: sampler_core::Envelope::default(),
                sample: 0,
                key_low: 0,
                key_high: 127,
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 1.0
            };
            2
        ],
        256,
    )
    .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 0,
            expressions: 4,
            families: 4,
            voices: 2,
            commands: 4,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    support::without_heap(|| {
        for _ in 0..100 {
            let note = rt.trigger(input, 60, 0.5).unwrap();
            assert_eq!(rt.trigger(input, 60, 0.5), Err(Error::Capacity));
            assert_eq!(rt.note_count(), 1);
            let mut audio = [[0.0; 2]; 16];
            rt.render(&mut audio).unwrap();
            assert!(audio[..8].iter().all(|f| *f == [0.25; 2]));
            assert!(audio[8..].iter().all(|f| *f == [0.0; 2]));
            rt.release(note).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.family_count(), rt.expression_count()),
                (0, 0, 0)
            );
        }
    }); // Owned asset destruction occurs only after this callback scope.
}
