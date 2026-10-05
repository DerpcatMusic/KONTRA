use sampler_core::{
    Envelope, Error, Event, GateRelease, Inheritance, Input, KeyRelease, Limits, Pcm, Playback,
    Prepared, Protocol, Region, ReleaseCause, ReleaseContext, Runtime, Sequence, SequenceScope,
    Take, TakePolicy,
};
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}
fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 2,
        performances: 1,
        families: 8,
        expressions: 8,
        voices: 8,
        decisions: 8,
        commands: 4,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
    }
}
fn prepared(value: f32) -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[value; 2]])).unwrap()],
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
    .with_variation(
        vec![Sequence {
            takes: 1,
            policy: TakePolicy::Shuffle { seed: 7 },
            scope: SequenceScope::Global,
            capacity: 1,
        }],
        vec![Some(Take {
            sequence: 0,
            index: 0,
        })],
        1,
        1,
    )
    .unwrap()
}

#[test]
fn physical_fifo_context_survives_eof_pedals_replacement_and_terminal_retry() {
    let (mut rt, mut control) = Runtime::with_plan_updates(prepared(0.5), limits(), 2, 1).unwrap();
    control.submit(Box::new(prepared(1.))).unwrap();
    let old_plan = rt.active_plan();
    support::without_heap(|| {
        rt.render(&mut [[0.; 2]; 4]).unwrap();
        let channel = rt.register_channel(input().channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let first = rt.trigger(input(), 60, 0.75).unwrap();
        rt.render(&mut [[0.; 2]; 2]).unwrap(); // attack source already ended
        let second = rt.trigger(input(), 60, 0.25).unwrap();
        let velocity = 65534. / 65535.;
        assert_eq!(rt.note_off(input(), Some(velocity)), Ok(first));
        let mut expected = ReleaseContext {
            admitted_at: 4,
            key: Some(KeyRelease {
                at: 6,
                velocity: Some(velocity),
                cause: ReleaseCause::KeyUp,
            }),
            gate: None,
        };
        assert_eq!(rt.release_context(first), Ok(expected));
        assert!(rt.key_down(second).unwrap());
        assert_eq!(rt.note(first).unwrap(), (60, 0.75, true));
        assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
        assert_eq!(rt.note_plan(first), Ok(old_plan));
        assert_eq!(
            rt.note_take(first, sampler_core::Trigger::Attack, 0),
            Ok(Some(0))
        );
        rt.render(&mut [[0.; 2]; 3]).unwrap();
        rt.sustain(channel, false).unwrap();
        expected.gate = Some(GateRelease {
            at: 9,
            cause: ReleaseCause::Pedal,
        });
        assert_eq!(rt.release_context(first), Ok(expected));
        assert!(rt.note(second).unwrap().2);
        rt.flush_ended(|_| false);
        assert_eq!(rt.release_context(first), Ok(expected));
        assert_eq!(rt.collect_retired_plans(), 0);
        // Repeated forced closure cannot rewrite the original key/gate evidence.
        rt.release(first).unwrap();
        rt.panic();
        assert_eq!(rt.release_context(first), Ok(expected));
        rt.flush_ended(|_| true);
        assert_eq!(rt.release_context(first), Err(Error::StaleHandle));
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(control.retired().unwrap());
}

#[test]
fn scheduled_release_context_is_sample_exact_and_first_key_up_wins() {
    let run = |partition: &[usize]| {
        let mut rt = Runtime::new(prepared(1.), limits()).unwrap();
        let mut result = None;
        support::without_heap(|| {
            let channel = rt.register_channel(input().channel_address()).unwrap();
            rt.sostenuto(channel, false).unwrap();
            let note = rt.trigger(input(), 60, 1.).unwrap();
            rt.sostenuto(channel, true).unwrap();
            rt.schedule_event(3, Event::KeyUp(note, Some(0.123456789)))
                .unwrap();
            rt.schedule_event(3, Event::KeyUp(note, Some(0.9))).unwrap();
            rt.schedule_event(7, Event::Sostenuto(channel, false))
                .unwrap();
            let mut audio = [[0.; 2]; 10];
            let mut at = 0;
            let mut block = 0;
            while at < audio.len() {
                let count = partition[block % partition.len()].min(audio.len() - at);
                rt.render(&mut audio[at..at + count]).unwrap();
                at += count;
                block += 1;
            }
            let context = rt.release_context(note).unwrap();
            assert_eq!(
                context,
                ReleaseContext {
                    admitted_at: 0,
                    key: Some(KeyRelease {
                        at: 3,
                        velocity: Some(0.123456789),
                        cause: ReleaseCause::KeyUp
                    }),
                    gate: Some(GateRelease {
                        at: 7,
                        cause: ReleaseCause::Pedal
                    })
                }
            );
            assert_eq!(rt.pending_commands(), 0);
            result = Some((audio, context));
        });
        result.unwrap()
    };
    let expected = run(&[1]);
    assert_eq!(run(&[10]), expected);
    assert_eq!(run(&[0, 3, 0, 4, 2]), expected);
}

#[test]
fn invalid_velocity_and_full_queue_leave_keys_available_for_valid_release() {
    let mut rt = Runtime::new(
        prepared(1.),
        Limits {
            commands: 1,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(), 60, 1.).unwrap();
        let context = rt.release_context(note).unwrap();
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.0001] {
            assert_eq!(rt.note_off(input(), Some(value)), Err(Error::InvalidInput));
            assert_eq!(rt.key_up(note, Some(value)), Err(Error::InvalidInput));
            assert_eq!(
                rt.schedule_event(1, Event::KeyUp(note, Some(value))),
                Err(Error::InvalidInput)
            );
            assert_eq!(rt.release_context(note), Ok(context));
            assert_eq!(rt.pending_commands(), 0);
        }
        rt.schedule_event(10, Event::KeyUp(note, Some(1.))).unwrap();
        assert_eq!(
            rt.schedule_event(2, Event::KeyUp(note, Some(0.5))),
            Err(Error::Capacity)
        );
        assert_eq!(rt.release_context(note), Ok(context));
        assert_eq!(rt.note_off(input(), Some(0.)), Ok(note)); // immediate needs no queue slot
        assert_eq!(
            rt.release_context(note).unwrap().key.unwrap().velocity,
            Some(0.)
        );
        assert_eq!(rt.pending_commands(), 0);
        rt.flush_ended(|_| true);
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        let replacement = rt.trigger(input(), 60, 1.).unwrap();
        assert_eq!(
            rt.release_context(replacement),
            Ok(ReleaseContext {
                admitted_at: 5,
                key: None,
                gate: None
            })
        );
        assert_eq!(rt.release_context(note), Err(Error::StaleHandle));
        rt.key_up(replacement, None).unwrap();
        assert_eq!(
            rt.release_context(replacement)
                .unwrap()
                .key
                .unwrap()
                .velocity,
            None
        );
        let before = rt.release_context(replacement).unwrap();
        rt.key_up(replacement, Some(1.)).unwrap();
        assert_eq!(rt.release_context(replacement), Ok(before));
    });
}

#[test]
fn native_closure_causes_do_not_fabricate_physical_release_velocity() {
    let mut rt = Runtime::new(prepared(1.), limits()).unwrap();
    support::without_heap(|| {
        let root = rt.trigger(input(), 60, 1.).unwrap();
        let linked = rt.child(root, 60, 1., true, Inheritance::Linked).unwrap();
        let independent = rt
            .child(root, 60, 1., false, Inheritance::Independent)
            .unwrap();
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        rt.release(root).unwrap();
        for (note, cause) in [
            (root, ReleaseCause::Explicit),
            (linked, ReleaseCause::Parent),
        ] {
            let context = rt.release_context(note).unwrap();
            assert_eq!(
                context.key,
                Some(KeyRelease {
                    at: 2,
                    cause,
                    velocity: None
                })
            );
            assert_eq!(context.gate, Some(GateRelease { at: 2, cause }));
        }
        assert!(rt.release_context(independent).unwrap().gate.is_none());
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        rt.panic();
        let context = rt.release_context(independent).unwrap();
        assert_eq!(context.key.unwrap().cause, ReleaseCause::Panic);
        assert_eq!(
            context.gate,
            Some(GateRelease {
                at: 3,
                cause: ReleaseCause::Panic
            })
        );
        rt.flush_ended(|_| true);
        let hard = rt.trigger(input(), 60, 1.).unwrap();
        rt.all_sound_off(input().channel_address()).unwrap();
        assert!(rt.key_down(hard).unwrap());
        assert_eq!(rt.release_context(hard).unwrap().key, None);
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        rt.note_off(input(), Some(0.875)).unwrap();
        let context = rt.release_context(hard).unwrap();
        assert_eq!(
            context.gate,
            Some(GateRelease {
                at: 3,
                cause: ReleaseCause::AllSoundOff
            })
        );
        assert_eq!(
            context.key,
            Some(KeyRelease {
                at: 5,
                cause: ReleaseCause::KeyUp,
                velocity: Some(0.875)
            })
        );
        rt.flush_ended(|_| true);
        let channel = rt.register_channel(input().channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let all = rt.trigger(input(), 60, 1.).unwrap();
        rt.all_notes_off(input().channel_address()).unwrap();
        assert_eq!(
            rt.release_context(all).unwrap().key.unwrap().cause,
            ReleaseCause::AllNotesOff
        );
        assert!(rt.release_context(all).unwrap().gate.is_none());
        rt.sustain(channel, false).unwrap();
        assert_eq!(
            rt.release_context(all).unwrap().gate.unwrap().cause,
            ReleaseCause::Pedal
        );
    });
}
