use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
use sampler_midi::{Applied, ApplyError, Ingress, Packets, Version};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime() -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[1.; 2]; 32].into_boxed_slice()).unwrap()],
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
        .unwrap(),
        Limits {
            notes: 4,
            channels: 2,
            families: 4,
            expressions: 4,
            voices: 4,
            commands: 2,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
        },
    )
    .unwrap()
}
fn apply(ingress: &Ingress, rt: &mut Runtime, words: &[u32]) -> Result<Applied, ApplyError> {
    ingress.apply(rt, Packets::new(words).next().unwrap().unwrap())
}

#[test]
fn native_ump_notes_pedals_and_terminal_identity_do_no_heap_work() {
    let mut rt = runtime();
    let mut groups = [None; 16];
    groups[3] = Some(Version::Midi2);
    let ingress = Ingress::new(7, groups);
    support::without_heap(|| {
        let Applied::Started(note) = apply(&ingress, &mut rt, &[0x439a_3c00, 0x8001_ffff]).unwrap()
        else {
            panic!()
        };
        assert_eq!(rt.note(note).unwrap().1, 32769.0 / 65535.0);
        apply(&ingress, &mut rt, &[0x43ba_4000, 0x8000_0000]).unwrap();
        let off = apply(&ingress, &mut rt, &[0x438a_3cfe, 0x1234_5678]).unwrap();
        assert!(matches!(off, Applied::Released { note: n, .. } if n == note));
        let mut audio = [[0.; 2]; 2];
        rt.render(&mut audio).unwrap();
        assert!(audio[0][0] > 0.5);
        rt.flush_ended(|_| panic!("sustain retains the note"));
        apply(&ingress, &mut rt, &[0x43ba_4000, 0x7fff_ffff]).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 2]);
        let mut ends = 0;
        rt.flush_ended(|input| {
            assert_eq!(
                (input.port, input.group, input.channel, input.key),
                (7, 3, 10, 60)
            );
            assert_eq!(input.protocol, sampler_core::Protocol::Midi2);
            ends += 1;
            true
        });
        assert_eq!(ends, 1);
        let Applied::Started(zero) = apply(&ingress, &mut rt, &[0x439a_3c00, 0]).unwrap() else {
            panic!()
        };
        assert_eq!(rt.note(zero).unwrap(), (60, 0.0, true));
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn protocol_scope_and_unsupported_messages_cannot_create_partial_notes() {
    let mut rt = runtime();
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let ingress = Ingress::new(0, groups);
    assert_eq!(
        apply(&ingress, &mut rt, &[0x4190_3c00, u32::MAX]),
        Err(ApplyError::DisabledGroup)
    );
    assert_eq!(
        apply(&ingress, &mut rt, &[0x4090_3c00, u32::MAX]),
        Err(ApplyError::ProtocolMismatch)
    );
    assert_eq!(rt.note_count(), 0);
    let Applied::Started(n) = apply(&ingress, &mut rt, &[0x2090_3c7f]).unwrap() else {
        panic!()
    };
    assert!(
        matches!(apply(&ingress, &mut rt, &[0x2090_3c00]).unwrap(), Applied::Released { note, velocity: None, .. } if note == n)
    );
    rt.flush_ended(|_| true);
    let ingress = Ingress::new(0, [Some(Version::Midi2); 16]);
    assert_eq!(
        apply(&ingress, &mut rt, &[0x4090_3c03, 0xffff_7800]),
        Ok(Applied::Unsupported)
    );
    assert_eq!(
        apply(&ingress, &mut rt, &[0x4060_3c00, 0x8000_0000]),
        Ok(Applied::Unsupported)
    );
    assert_eq!(rt.note_count(), 0);
}

#[test]
fn block_timing_order_and_rejections_are_partition_invariant_without_heap() {
    use sampler_midi::{BlockError, TimedPacket};
    let words = [
        [0x4090_3c00, 0xffff_0000],
        [0x40b0_4000, u32::MAX],
        [0x4080_3c00, 0],
        [0x40b0_4000, 0],
        [0x4190_3c00, 0xffff_0000],
        [0x4090_3c00, 0xffff_0000],
        [0x4080_3c00, 0],
        [0x4090_3c00, 0xffff_0000],
        [0x4080_3c00, 0],
    ];
    let offsets = [0, 4, 8, 12, 14, 14, 14, 16, 19];
    let packets: [_; 9] = std::array::from_fn(|i| TimedPacket {
        offset: offsets[i],
        packet: Packets::new(&words[i]).next().unwrap().unwrap(),
    });
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi2);
    let ingress = Ingress::new(0, groups);
    for block in 1..=32 {
        let mut rt = runtime();
        let mut output = [[0.; 2]; 32];
        let mut next = 0;
        let mut outcomes = 0;
        support::without_heap(|| {
            for begin in (0..32).step_by(block) {
                let end = (begin + block).min(32);
                let mut local = [packets[0]; 9];
                let mut count = 0;
                while next < packets.len() && packets[next].offset < end {
                    local[count] = TimedPacket {
                        offset: packets[next].offset - begin,
                        ..packets[next]
                    };
                    count += 1;
                    next += 1;
                }
                ingress
                    .render(
                        &mut rt,
                        &mut output[begin..end],
                        &local[..count],
                        9,
                        |_, result| {
                            if outcomes == 4 {
                                assert_eq!(result, Err(ApplyError::DisabledGroup));
                            } else {
                                assert!(result.is_ok());
                            }
                            outcomes += 1;
                        },
                    )
                    .unwrap();
                rt.flush_ended(|_| true);
            }
        });
        assert_eq!(outcomes, 9);
        assert_eq!(rt.note_count(), 0);
        for (frame, sample) in output.iter().enumerate() {
            let expected = if frame < 12 || (16..19).contains(&frame) {
                1.
            } else {
                0.
            };
            assert_eq!(*sample, [expected; 2], "block {block}, frame {frame}");
        }
    }
    let mut rt = runtime();
    let mut output = [[42.; 2]; 8];
    assert_eq!(
        ingress.render(&mut rt, &mut output, &packets[..2], 1, |_, _| panic!()),
        Err(BlockError::EventBudget)
    );
    assert_eq!(
        ingress.render(&mut rt, &mut output, &packets[..3], 3, |_, _| panic!()),
        Err(BlockError::InvalidOffset { index: 2 })
    );
    assert_eq!(
        ingress.render(
            &mut rt,
            &mut output,
            &[packets[1], packets[0]],
            2,
            |_, _| panic!()
        ),
        Err(BlockError::InvalidOffset { index: 1 })
    );
    assert_eq!((rt.now(), rt.note_count()), (0, 0));
    assert_eq!(output, [[42.; 2]; 8]);
    ingress
        .render(&mut rt, &mut [], &packets[..1], 1, |_, result| {
            assert!(matches!(result, Ok(Applied::Started(_))));
        })
        .unwrap();
    assert_eq!((rt.now(), rt.note_count()), (0, 1));
}

#[test]
fn full_note_pool_does_not_skip_later_release_in_the_block() {
    use sampler_midi::TimedPacket;
    let on = [0x4090_3c00, 0xffff_0000];
    let off = [0x4080_3c00, 0];
    let mut events = [TimedPacket {
        offset: 0,
        packet: Packets::new(&on).next().unwrap().unwrap(),
    }; 6];
    events[5].packet = Packets::new(&off).next().unwrap().unwrap();
    let mut rt = runtime();
    let ingress = Ingress::new(0, [Some(Version::Midi2); 16]);
    let mut audio = [[0.; 2]; 4];
    support::without_heap(|| {
        ingress
            .render(&mut rt, &mut audio, &events, 6, |index, result| {
                if index == 4 {
                    assert_eq!(result, Err(ApplyError::Core(sampler_core::Error::Capacity)));
                } else {
                    assert!(result.is_ok());
                }
            })
            .unwrap();
        rt.flush_ended(|_| true);
    });
    assert_eq!(audio, [[3.; 2]; 4]);
    assert_eq!((rt.note_count(), rt.voice_count()), (3, 3));
}

#[test]
fn all_notes_off_respects_pedals_and_channel_scope_at_exact_offsets() {
    use sampler_midi::TimedPacket;
    for version in [Version::Midi1, Version::Midi2] {
        for pedal in [0, 64, 66] {
            let (prefix, velocity, down) = match version {
                Version::Midi1 => (0x2300_0000, 127, 127),
                Version::Midi2 => (0x4300_0000, 0xffff_0000, u32::MAX),
            };
            let encode = |status: u32, index: u32, value: u32| {
                if version == Version::Midi1 {
                    [prefix | status << 16 | index << 8 | value, 0]
                } else {
                    [prefix | status << 16 | index << 8, value]
                }
            };
            let words = [
                encode(0x92, 60, velocity),
                encode(0x93, 60, velocity),
                encode(0xb2, pedal, down),
                encode(0x92, 60, velocity),
                encode(0xb2, 123, 0),
                encode(0xb2, pedal, 0),
                encode(0xb3, 123, 0),
            ];
            let offsets = [0, 0, 2, 3, 4, 6, 8];
            let packets: [_; 7] = std::array::from_fn(|i| TimedPacket {
                offset: offsets[i],
                packet: Packets::new(&words[i]).next().unwrap().unwrap(),
            });
            let mut groups = [None; 16];
            groups[3] = Some(version);
            let ingress = Ingress::new(4, groups);
            for block in 1..=16 {
                let mut rt = runtime();
                let mut output = [[0.; 2]; 16];
                let mut next = 0;
                let mut outcomes = 0;
                let mut terminals = 0;
                support::without_heap(|| {
                    for begin in (0..16).step_by(block) {
                        let end = (begin + block).min(16);
                        let mut local = [packets[0]; 7];
                        let mut count = 0;
                        while next < packets.len() && packets[next].offset < end {
                            local[count] = TimedPacket {
                                offset: packets[next].offset - begin,
                                ..packets[next]
                            };
                            count += 1;
                            next += 1;
                        }
                        ingress
                            .render(
                                &mut rt,
                                &mut output[begin..end],
                                &local[..count],
                                7,
                                |_, result| {
                                    if outcomes == 4 || outcomes == 6 {
                                        assert_eq!(
                                            result,
                                            Ok(Applied::AllNotesOff {
                                                released: if outcomes == 4 { 2 } else { 1 }
                                            })
                                        );
                                    } else {
                                        assert!(result.is_ok());
                                    }
                                    outcomes += 1;
                                },
                            )
                            .unwrap();
                        rt.flush_ended(|input| {
                            assert_eq!((input.port, input.group, input.key), (4, 3, 60));
                            terminals += 1;
                            true
                        });
                    }
                    assert_eq!((outcomes, terminals, rt.note_count()), (7, 3, 0));
                });
                for (frame, actual) in output.iter().enumerate() {
                    let expected = match frame {
                        0..=2 => 2.,
                        3 => 3.,
                        4..=5 if pedal == 64 => 3.,
                        4..=5 if pedal == 66 => 2.,
                        4..=7 => 1.,
                        _ => 0.,
                    };
                    assert_eq!(
                        *actual, [expected; 2],
                        "{version:?}, pedal {pedal}, block {block}, frame {frame}"
                    );
                }
            }
        }
    }
}

#[test]
fn all_notes_off_needs_no_spare_capacity_and_preserves_other_input_domains() {
    use sampler_core::{Error, Event, Expression, Input, Protocol};
    let mut rt = runtime();
    let mut groups = [None; 16];
    groups[3] = Some(Version::Midi2);
    let ingress = Ingress::new(4, groups);
    let input = Input {
        protocol: Protocol::Midi2,
        port: 4,
        group: 3,
        channel: 2,
        key: 60,
        external_id: None,
    };
    support::without_heap(|| {
        let notes = [
            rt.trigger(input, 60, 1.).unwrap(),
            rt.trigger(Input { port: 5, ..input }, 60, 1.).unwrap(),
            rt.trigger(Input { group: 4, ..input }, 60, 1.).unwrap(),
            rt.trigger(
                Input {
                    protocol: Protocol::Midi1,
                    ..input
                },
                60,
                1.,
            )
            .unwrap(),
        ];
        for channel in [0, 1] {
            rt.register_channel(sampler_core::ChannelAddress {
                channel,
                ..input.channel_address()
            })
            .unwrap();
        }
        rt.schedule_event(100, Event::Expression(notes[0], Expression::default()))
            .unwrap();
        rt.schedule_event(100, Event::Expression(notes[1], Expression::default()))
            .unwrap();
        assert_eq!(rt.pending_commands(), 2);
        assert_eq!(
            apply(&ingress, &mut rt, &[0x43b2_7b00, 1]),
            Ok(Applied::Unsupported)
        );
        assert_eq!(
            rt.all_notes_off(sampler_core::ChannelAddress {
                group: 16,
                ..input.channel_address()
            }),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.key_down(notes[0]), Ok(true));
        assert_eq!(
            apply(&ingress, &mut rt, &[0x43b2_7b00, 0]),
            Ok(Applied::AllNotesOff { released: 1 })
        );
        assert_eq!(
            apply(&ingress, &mut rt, &[0x43b2_7b00, 0]),
            Ok(Applied::AllNotesOff { released: 0 })
        );
        assert_eq!(rt.key_down(notes[0]), Ok(false));
        for note in &notes[1..] {
            assert_eq!(rt.key_down(*note), Ok(true));
            assert!(rt.note(*note).unwrap().2);
        }
        assert_eq!((rt.voice_count(), rt.pending_commands()), (3, 1));
        rt.flush_ended(|_| false);
        assert_eq!(rt.note_count(), 4);
        let mut ends = 0;
        rt.flush_ended(|origin| {
            assert_eq!(origin, input);
            ends += 1;
            true
        });
        assert_eq!(ends, 1);
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
}

#[test]
fn all_sound_off_keeps_fifo_pairing_and_late_key_cleanup_without_resurrection() {
    use sampler_core::Event;
    for version in [Version::Midi1, Version::Midi2] {
        let (on, off, pedal_on, pedal_off, silence, invalid) = match version {
            Version::Midi1 => (
                [0x2090_3c7f, 0],
                [0x2080_3c00, 0],
                [0x20b0_407f, 0],
                [0x20b0_4000, 0],
                [0x20b0_7800, 0],
                [0x20b0_7801, 0],
            ),
            Version::Midi2 => (
                [0x4090_3c00, 0xffff_0000],
                [0x4080_3c00, 0],
                [0x40b0_4000, u32::MAX],
                [0x40b0_4000, 0],
                [0x40b0_7800, 0],
                [0x40b0_7800, 1],
            ),
        };
        let mut groups = [None; 16];
        groups[0] = Some(version);
        let ingress = Ingress::new(0, groups);
        let mut rt = runtime();
        support::without_heap(|| {
            let Applied::Started(old) = apply(&ingress, &mut rt, &on).unwrap() else {
                panic!()
            };
            apply(&ingress, &mut rt, &pedal_on).unwrap();
            rt.render(&mut [[0.; 2]; 2]).unwrap();
            assert_eq!(apply(&ingress, &mut rt, &invalid), Ok(Applied::Unsupported));
            assert_eq!(rt.voice_count(), 1);
            assert_eq!(
                apply(&ingress, &mut rt, &silence),
                Ok(Applied::AllSoundOff { stopped: 1 })
            );
            rt.flush_ended(|_| panic!("physical input must survive hard silence"));
            assert_eq!(rt.key_down(old), Ok(true));
            assert_eq!(rt.voice_count(), 0);
            // A key-up can still be scheduled for a silent, physically held input.
            rt.schedule_event(20, Event::KeyUp(old)).unwrap();
            let Applied::Started(new) = apply(&ingress, &mut rt, &on).unwrap() else {
                panic!()
            };
            assert!(
                matches!(apply(&ingress, &mut rt, &off), Ok(Applied::Released { note, .. }) if note == old)
            );
            assert_eq!(rt.key_down(new), Ok(true));
            assert_eq!(
                rt.pending_commands(),
                0,
                "early physical release cancels later stale key-up even with sustain down"
            );
            let mut ends = 0;
            rt.flush_ended(|_| {
                ends += 1;
                true
            });
            assert_eq!(ends, 1);
            let mut audio = [[0.; 2]; 24];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[1.; 2]; 24]);
            assert!(
                matches!(apply(&ingress, &mut rt, &off), Ok(Applied::Released { note, .. }) if note == new)
            );
            assert!(
                rt.note(new).unwrap().2,
                "pedal state remains active for new notes"
            );
            apply(&ingress, &mut rt, &pedal_off).unwrap();
            rt.flush_ended(|_| {
                ends += 1;
                true
            });
            assert_eq!((ends, rt.note_count(), rt.voice_count()), (2, 0, 0));
        });
    }
}
