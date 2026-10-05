use sampler_core::{
    ChannelAddress, Envelope, Error, Event, Expression, Limits, NoteId, Pcm, Playback, Prepared,
    Protocol, Region, Runtime,
};
use sampler_midi::{Applied, ApplyError, Mpe, Packets, Zone};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime() -> Runtime {
    runtime_with_channels(4)
}
fn runtime_with_channels(channels: usize) -> Runtime {
    runtime_with_modulation(channels, sampler_core::Modulation::default())
}
fn runtime_with_modulation(channels: usize, modulation: sampler_core::Modulation) -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![
                Pcm::new(
                    48000,
                    (0..8192)
                        .map(|i| {
                            let phase = f64::from(i) * std::f64::consts::TAU * 0.017;
                            [phase.cos() as f32, phase.sin() as f32]
                        })
                        .collect(),
                )
                .unwrap(),
            ],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 61,
                root_key: None,
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 1.0,
                envelope: Envelope::new(0, 0, 0, 1.0, 256).unwrap(),
                playback: Playback::default(),
            }],
            2,
        )
        .unwrap()
        .with_modulation(modulation),
        Limits {
            notes: 8,
            channels,
            performances: 1,
            families: 8,
            decisions: 0,
            expressions: 8,
            voices: 8,
            commands: 8,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
        },
    )
    .unwrap()
}
fn packet(status: u8, channel: u8, a: u8, b: u8) -> u32 {
    0x2300_0000 | (u32::from(status | channel) << 16) | (u32::from(a) << 8) | u32::from(b)
}
fn bend(channel: u8, value: u16) -> u32 {
    packet(0xe0, channel, (value & 127) as u8, (value >> 7) as u8)
}
fn apply(mpe: &mut Mpe, rt: &mut Runtime, word: u32) -> Result<Applied, ApplyError> {
    mpe.apply(rt, Packets::new(&[word]).next().unwrap().unwrap())
}
fn start(mpe: &mut Mpe, rt: &mut Runtime, channel: u8, key: u8) -> NoteId {
    let Applied::Started(note) = apply(mpe, rt, packet(0x90, channel, key, 127)).unwrap() else {
        panic!("not admitted")
    };
    note
}
fn expression(rt: &Runtime, note: NoteId) -> Expression {
    rt.expression(rt.expression_id(note).unwrap()).unwrap()
}
fn pitch(value: u16, range: f64) -> f64 {
    (f64::from(value) - 8192.0) * range / if value < 8192 { 8192.0 } else { 8191.0 }
}

#[test]
fn member_pitch_freezes_at_physical_key_up_and_manager_still_reaches_tails() {
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        let channel = rt
            .register_channel(ChannelAddress {
                protocol: Protocol::Midi1,
                port: 7,
                group: 3,
                channel: member,
            })
            .unwrap();
        support::without_heap(|| {
            apply(&mut mpe, &mut rt, bend(manager, 12288)).unwrap();
            apply(&mut mpe, &mut rt, bend(member, 9216)).unwrap();
            let old = start(&mut mpe, &mut rt, member, 60);
            let overlapping = start(&mut mpe, &mut rt, member, 60);
            let initial = pitch(12288, 2.0) + pitch(9216, 48.0);
            assert_eq!(expression(&rt, old).pitch_semitones, initial);
            rt.sustain(channel, true).unwrap();
            assert!(
                matches!(apply(&mut mpe, &mut rt, packet(0x80, member, 60, 0)), Ok(Applied::Released { note, .. }) if note == old)
            );
            assert!(!rt.key_down(old).unwrap());
            assert!(rt.note(old).unwrap().2);
            assert_eq!(
                apply(&mut mpe, &mut rt, bend(member, 10240)),
                Ok(Applied::Expression { owners: 1 })
            );
            assert_eq!(expression(&rt, old).pitch_semitones, initial);
            let reused = start(&mut mpe, &mut rt, member, 61);
            assert_eq!(expression(&rt, reused), expression(&rt, overlapping));
            assert_eq!(
                apply(&mut mpe, &mut rt, bend(manager, 0)),
                Ok(Applied::Expression { owners: 3 })
            );
            assert_eq!(
                expression(&rt, old).pitch_semitones,
                pitch(9216, 48.0) - 2.0
            );
            assert_eq!(
                expression(&rt, overlapping).pitch_semitones,
                pitch(10240, 48.0) - 2.0
            );
            // A native scheduled key-up is authoritative too; the adapter keeps
            // no competing key state, and due work executes before the controller.
            rt.schedule_event(8, Event::KeyUp(overlapping, None))
                .unwrap();
            rt.render(&mut [[0.0; 2]; 8]).unwrap();
            assert_eq!(
                apply(&mut mpe, &mut rt, bend(member, 8192)),
                Ok(Applied::Expression { owners: 1 })
            );
            assert_eq!(
                expression(&rt, overlapping).pitch_semitones,
                pitch(10240, 48.0) - 2.0
            );
            assert_eq!(expression(&rt, reused).pitch_semitones, -2.0);
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn failed_gestures_and_binding_pressure_do_not_publish_partial_state() {
    let mut rt = runtime();
    let mut foreign = runtime();
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 2).unwrap();
    support::without_heap(|| {
        let a = start(&mut mpe, &mut rt, 1, 60);
        apply(&mut mpe, &mut rt, bend(2, 16383)).unwrap();
        let b = start(&mut mpe, &mut rt, 2, 60);
        let before_a = expression(&rt, a);
        let before_b = expression(&rt, b);
        assert_eq!(before_b.pitch_semitones, 48.0);
        assert_eq!(
            apply(&mut mpe, &mut rt, bend(0, 16383)),
            Err(ApplyError::Core(Error::InvalidInput))
        );
        assert_eq!(expression(&rt, a), before_a);
        assert_eq!(expression(&rt, b), before_b);
        assert_eq!(
            apply(&mut mpe, &mut rt, packet(0x90, 1, 61, 127)),
            Err(ApplyError::Core(Error::Capacity))
        );
        assert_eq!(rt.note_count(), 2);
        assert_eq!(
            apply(&mut mpe, &mut foreign, bend(0, 0)),
            Err(ApplyError::Core(Error::StaleHandle))
        );
        assert_eq!(
            apply(&mut mpe, &mut rt, packet(0x90, 3, 60, 127)),
            Ok(Applied::Unsupported)
        );
        assert_eq!(
            apply(&mut mpe, &mut rt, 0x2391_3c7f ^ 0x0100_0000),
            Err(ApplyError::DisabledGroup)
        );
        rt.panic();
        rt.flush_ended(|_| false);
        assert_eq!(
            apply(&mut mpe, &mut rt, packet(0x90, 1, 61, 127)),
            Err(ApplyError::Core(Error::Capacity))
        );
        rt.flush_ended(|_| true);
        let replacement = start(&mut mpe, &mut rt, 1, 60);
        // The failed manager gesture did not change idle controller state.
        assert_eq!(expression(&rt, replacement).pitch_semitones, 0.0);
        assert_eq!(rt.expression_id(a), Err(Error::StaleHandle));
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn mpe_audio_is_identical_across_host_partitions() {
    let events = [
        (0, bend(1, 9216)),
        (0, packet(0x90, 1, 60, 127)),
        (128, bend(1, 10240)),
        (144, packet(0x80, 1, 60, 0)),
        (160, bend(1, 8192)),
        (176, packet(0x90, 1, 60, 127)),
        (192, bend(0, 12288)),
        (224, packet(0x80, 1, 60, 0)),
    ];
    let mut baseline = [[0.0; 2]; 512];
    for partition in [1, 7, 64, 256, 512] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
        let mut audio = [[0.0; 2]; 512];
        support::without_heap(|| {
            let mut next = 0;
            for block_start in (0..audio.len()).step_by(partition) {
                let end = (block_start + partition).min(audio.len());
                let mut cursor = block_start;
                while next < events.len() && events[next].0 < end {
                    let (at, word) = events[next];
                    rt.render(&mut audio[cursor..at]).unwrap();
                    apply(&mut mpe, &mut rt, word).unwrap();
                    cursor = at;
                    next += 1;
                }
                rt.render(&mut audio[cursor..end]).unwrap();
            }
            rt.flush_ended(|_| true);
            assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
        });
        if partition == 1 {
            baseline = audio
        } else {
            assert_eq!(baseline, audio)
        }
        assert!(audio[..400].iter().any(|frame| frame[0].abs() > 0.5));
        assert!(audio[480..].iter().all(|frame| *frame == [0.0; 2]));
    }
}

#[test]
fn pressure_and_timbre_keep_member_snapshots_and_preserve_unrelated_expression() {
    let scaled = |value: u32| (u64::from(value) * u64::from(u32::MAX) / 127) as u32;
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        support::without_heap(|| {
            apply(&mut mpe, &mut rt, packet(0xd0, manager, 32, 0)).unwrap();
            apply(&mut mpe, &mut rt, packet(0xd0, member, 64, 0)).unwrap();
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 74, 70)).unwrap();
            apply(&mut mpe, &mut rt, packet(0xb0, member, 74, 80)).unwrap();
            let old = start(&mut mpe, &mut rt, member, 60);
            assert_eq!(
                (expression(&rt, old).pressure, expression(&rt, old).timbre),
                (scaled(64), scaled(86))
            );
            let owner = rt.expression_id(old).unwrap();
            let authored = Expression {
                gain: 0.5,
                pan: -0.25,
                pitch_semitones: 7.0,
                ..expression(&rt, old)
            };
            rt.set_expression(owner, authored).unwrap();
            apply(&mut mpe, &mut rt, packet(0xd0, member, 95, 0)).unwrap();
            assert_eq!(
                expression(&rt, old),
                Expression {
                    pressure: scaled(95),
                    ..authored
                }
            );
            apply(&mut mpe, &mut rt, packet(0x80, member, 60, 0)).unwrap();
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xd0, member, 10, 0)),
                Ok(Applied::Expression { owners: 0 })
            );
            apply(&mut mpe, &mut rt, packet(0xb0, member, 74, 0)).unwrap();
            assert_eq!(
                expression(&rt, old),
                Expression {
                    pressure: scaled(95),
                    ..authored
                }
            );
            let new = start(&mut mpe, &mut rt, member, 60);
            assert_eq!(
                (expression(&rt, new).pressure, expression(&rt, new).timbre),
                (scaled(32), scaled(6))
            );
            apply(&mut mpe, &mut rt, packet(0xd0, manager, 100, 0)).unwrap();
            assert_eq!(
                (expression(&rt, old).pressure, expression(&rt, new).pressure),
                (scaled(100), scaled(100))
            );
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 74, 0)).unwrap();
            assert_eq!(
                (expression(&rt, old).timbre, expression(&rt, new).timbre),
                (scaled(16), 0)
            );
            apply(&mut mpe, &mut rt, packet(0xb0, member, 74, 127)).unwrap();
            assert_eq!(expression(&rt, new).timbre, scaled(63));
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 74, 127)).unwrap();
            assert_eq!(
                (expression(&rt, old).timbre, expression(&rt, new).timbre),
                (u32::MAX, u32::MAX)
            );
            apply(&mut mpe, &mut rt, packet(0xd0, member, 0, 0)).unwrap();
            apply(&mut mpe, &mut rt, packet(0xd0, manager, 0, 0)).unwrap();
            assert_eq!(
                (expression(&rt, old).pressure, expression(&rt, new).pressure),
                (scaled(95), 0)
            );
            assert_eq!(
                (
                    expression(&rt, old).gain,
                    expression(&rt, old).pan,
                    expression(&rt, old).pitch_semitones
                ),
                (0.5, -0.25, 7.0)
            );
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn rpn_sensitivity_is_zone_wide_transactional_and_keeps_released_member_pitch() {
    for (zone, manager, a, b) in [(Zone::Lower, 0, 1, 2), (Zone::Upper, 15, 14, 13)] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        support::without_heap(|| {
            assert_eq!(mpe.pitch_ranges(), (2, 48));
            apply(&mut mpe, &mut rt, bend(a, 10240)).unwrap();
            apply(&mut mpe, &mut rt, bend(b, 9216)).unwrap();
            let tail = start(&mut mpe, &mut rt, a, 60);
            apply(&mut mpe, &mut rt, packet(0x80, a, 60, 0)).unwrap();
            let active_a = start(&mut mpe, &mut rt, a, 61);
            let active_b = start(&mut mpe, &mut rt, b, 60);
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 24)),
                Ok(Applied::Unsupported)
            );
            // Either selector byte may arrive first. One byte alone is not RPN 0.
            apply(&mut mpe, &mut rt, packet(0xb0, a, 100, 0)).unwrap();
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 24)),
                Ok(Applied::Unsupported)
            );
            apply(&mut mpe, &mut rt, packet(0xb0, a, 101, 0)).unwrap();
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 24)),
                Ok(Applied::Expression { owners: 2 })
            );
            assert_eq!(mpe.pitch_ranges(), (2, 24));
            assert_eq!(expression(&rt, tail).pitch_semitones, pitch(10240, 48.0));
            assert_eq!(
                expression(&rt, active_a).pitch_semitones,
                pitch(10240, 24.0)
            );
            assert_eq!(expression(&rt, active_b).pitch_semitones, pitch(9216, 24.0));
            for index in [101, 100] {
                apply(&mut mpe, &mut rt, packet(0xb0, manager, index, 0)).unwrap();
            }
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 6, 12)).unwrap();
            apply(&mut mpe, &mut rt, bend(manager, 12288)).unwrap();
            let global = pitch(12288, 12.0);
            assert_eq!(
                expression(&rt, tail).pitch_semitones,
                pitch(10240, 48.0) + global
            );
            apply(&mut mpe, &mut rt, bend(b, 16383)).unwrap();
            let before = [
                expression(&rt, tail),
                expression(&rt, active_a),
                expression(&rt, active_b),
            ];
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 96)),
                Err(ApplyError::Core(Error::InvalidInput))
            );
            assert_eq!(mpe.pitch_ranges(), (12, 24));
            assert_eq!(
                [
                    expression(&rt, tail),
                    expression(&rt, active_a),
                    expression(&rt, active_b)
                ],
                before
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, manager, 6, 97)),
                Err(ApplyError::Core(Error::InvalidInput))
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 38, 1)),
                Ok(Applied::Unsupported)
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 38, 0)),
                Ok(Applied::Configuration)
            );
            // NRPN selection disables RPN data entry; null RPN does likewise.
            apply(&mut mpe, &mut rt, packet(0xb0, a, 99, 0)).unwrap();
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 0)),
                Ok(Applied::Unsupported)
            );
            for index in [101, 100] {
                apply(&mut mpe, &mut rt, packet(0xb0, a, index, 127)).unwrap();
            }
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 0)),
                Ok(Applied::Unsupported)
            );
            for index in [100, 101] {
                apply(&mut mpe, &mut rt, packet(0xb0, a, index, 0)).unwrap();
            }
            apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 0)).unwrap();
            assert_eq!(expression(&rt, active_a).pitch_semitones, global);
            assert_eq!(expression(&rt, active_b).pitch_semitones, global);
            assert_eq!(expression(&rt, tail), before[0]);
            // Zero range must not erase raw bend positions needed on restoration.
            apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 24)).unwrap();
            assert_eq!(
                [
                    expression(&rt, tail),
                    expression(&rt, active_a),
                    expression(&rt, active_b)
                ],
                before
            );
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn manager_pedals_capture_the_zone_once_and_member_pedals_are_ignored() {
    for (zone, manager, a, b) in [(Zone::Lower, 0, 1, 2), (Zone::Upper, 15, 14, 13)] {
        let mut rt = runtime_with_channels(3);
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        support::without_heap(|| {
            let old = start(&mut mpe, &mut rt, a, 60);
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, manager, 64, 127)),
                Ok(Applied::Pedal)
            );
            // Other protocol, port and group domains never inherit this pedal.
            for (protocol, port, group) in [
                (Protocol::Midi2, 7, 3),
                (Protocol::Midi1, 8, 3),
                (Protocol::Midi1, 7, 4),
            ] {
                let input = sampler_core::Input {
                    protocol,
                    port,
                    group,
                    channel: a,
                    key: 60,
                    external_id: None,
                };
                let outside = rt.trigger(input, 60, 1.0).unwrap();
                rt.note_off(input, None).unwrap();
                assert!(!rt.note(outside).unwrap().2);
            }
            apply(&mut mpe, &mut rt, packet(0x80, a, 60, 0)).unwrap();
            let captured = start(&mut mpe, &mut rt, b, 60);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 66, 127)).unwrap();
            let late = start(&mut mpe, &mut rt, a, 61);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 66, 127)).unwrap();
            apply(&mut mpe, &mut rt, packet(0x80, b, 60, 0)).unwrap();
            apply(&mut mpe, &mut rt, packet(0x80, a, 61, 0)).unwrap();
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 64, 0)),
                Ok(Applied::Ignored)
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, b, 66, 0)),
                Ok(Applied::Ignored)
            );
            for note in [old, captured, late] {
                assert!(rt.note(note).unwrap().2);
            }
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 64, 0)).unwrap();
            assert!(!rt.note(old).unwrap().2);
            assert!(rt.note(captured).unwrap().2);
            assert!(!rt.note(late).unwrap().2);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 66, 0)).unwrap();
            assert!(!rt.note(captured).unwrap().2);
            rt.render(&mut [[0.0; 2]; 256]).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn zone_pedal_admission_is_atomic_and_pedal_up_needs_no_channel_capacity() {
    let mut rt = runtime_with_channels(2);
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    let address = ChannelAddress {
        protocol: Protocol::Midi1,
        port: 7,
        group: 3,
        channel: 1,
    };
    let channel = rt.register_channel(address).unwrap();
    support::without_heap(|| {
        let note = start(&mut mpe, &mut rt, 1, 60);
        assert_eq!(
            apply(&mut mpe, &mut rt, packet(0xb0, 0, 64, 127)),
            Err(ApplyError::Core(Error::Capacity))
        );
        assert_eq!(rt.pedals(channel), Ok((false, false)));
        // Failed zone admission did not consume the one remaining reservation.
        let second = rt
            .register_channel(ChannelAddress {
                channel: 2,
                ..address
            })
            .unwrap();
        rt.sustain(channel, true).unwrap();
        apply(&mut mpe, &mut rt, packet(0x80, 1, 60, 0)).unwrap();
        assert!(rt.note(note).unwrap().2);
        assert_eq!(
            apply(&mut mpe, &mut rt, packet(0xb0, 0, 64, 0)),
            Ok(Applied::Pedal)
        );
        assert_eq!(rt.pedals(channel), Ok((false, false)));
        assert_eq!(rt.pedals(second), Ok((false, false)));
        assert!(!rt.note(note).unwrap().2);
        rt.render(&mut [[0.0; 2]; 256]).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
    for (zone, manager, member) in [(Zone::Lower, 0, 15), (Zone::Upper, 15, 0)] {
        let mut rt = runtime_with_channels(16);
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 15, 8).unwrap();
        support::without_heap(|| {
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 64, 127)).unwrap();
            let note = start(&mut mpe, &mut rt, member, 60);
            apply(&mut mpe, &mut rt, packet(0x80, member, 60, 0)).unwrap();
            assert!(rt.note(note).unwrap().2);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 64, 0)).unwrap();
            assert!(!rt.note(note).unwrap().2);
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn mpe_drives_prepared_native_modulation_and_muted_sources_keep_their_position() {
    use sampler_core::{Destination, ExpressionSource, Modulation, Route};
    let modulation = Modulation::new(
        vec![
            Route {
                source: ExpressionSource::Pressure,
                destination: Destination::LinearGain {
                    zero: 0.0,
                    one: 1.0,
                },
            },
            Route {
                source: ExpressionSource::Timbre,
                destination: Destination::StereoBalance {
                    zero: -0.5,
                    one: 0.5,
                },
            },
            Route {
                source: ExpressionSource::Timbre,
                destination: Destination::PitchSemitones {
                    zero: 0.0,
                    one: 12.0,
                },
            },
        ],
        3,
    )
    .unwrap();
    let mut rt = runtime_with_modulation(4, modulation);
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    support::without_heap(|| {
        apply(&mut mpe, &mut rt, packet(0xb0, 1, 74, 0)).unwrap();
        let note = start(&mut mpe, &mut rt, 1, 60);
        let mut audio = [[0.0; 2]; 64];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.0; 2]; 64]);
        apply(&mut mpe, &mut rt, packet(0xd0, 1, 127, 0)).unwrap();
        rt.render(&mut audio).unwrap();
        for (index, frame) in audio.iter().enumerate() {
            let phase = (64 + index) as f64 * std::f64::consts::TAU * 0.017;
            assert_eq!(*frame, [phase.cos() as f32, phase.sin() as f32 * 0.5]);
        }
        apply(&mut mpe, &mut rt, packet(0xb0, 1, 74, 127)).unwrap();
        rt.render(&mut audio).unwrap();
        for (index, frame) in audio.iter().enumerate() {
            let phase = (128 + 2 * index) as f64 * std::f64::consts::TAU * 0.017;
            assert!((f64::from(frame[0]) - phase.cos() * 0.5).abs() < 0.0001);
            assert!((f64::from(frame[1]) - phase.sin()).abs() < 0.0001);
        }
        assert_eq!(
            expression(&rt, note),
            Expression {
                pressure: u32::MAX,
                timbre: u32::MAX,
                ..Expression::default()
            }
        );
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(
            (rt.note_count(), rt.expression_count(), rt.voice_count()),
            (0, 0, 0)
        );
    });
}

#[test]
fn mpe_retains_release_velocity_for_both_zones_and_distinguishes_zero_note_on() {
    for (zone, member) in [(Zone::Lower, 1), (Zone::Upper, 14)] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        support::without_heap(|| {
            for (status, value, expected) in [
                (0x80, 17, Some(17. / 127.)),
                (0x80, 0, Some(0.)),
                (0x90, 0, None),
            ] {
                let note = start(&mut mpe, &mut rt, member, 60);
                let at = rt.now() + 2;
                rt.render(&mut [[0.; 2]; 2]).unwrap();
                apply(&mut mpe, &mut rt, packet(status, member, 60, value)).unwrap();
                assert_eq!(
                    rt.release_context(note).unwrap().key,
                    Some(sampler_core::KeyRelease {
                        at,
                        velocity: expected,
                        cause: sampler_core::ReleaseCause::KeyUp,
                    })
                );
                rt.panic();
                rt.flush_ended(|_| true);
            }
        });
    }
}

#[test]
fn articulation_switch_on_one_member_routes_all_members_without_changing_expression_identity() {
    use sampler_core::{Keyswitch, SelectionPolicy};
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[0.5; 2]; 4])).unwrap()],
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
    .unwrap()
    .with_articulations(
        vec![Some(99)],
        vec![Keyswitch {
            key: 12,
            articulation: 99,
        }],
        SelectionPolicy::Onset,
        SelectionPolicy::Onset,
    )
    .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 3,
            performances: 2,
            families: 4,
            expressions: 4,
            voices: 4,
            decisions: 0,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
        },
    )
    .unwrap();
    let domain = rt.performance(1).unwrap();
    let mut mpe = Mpe::new_in(&rt, domain, 0, 3, Zone::Lower, 3, 4).unwrap();
    support::without_heap(|| {
        let switch = start(&mut mpe, &mut rt, 1, 12);
        assert!(rt.note_selection(switch).unwrap().consumed_switch);
        assert_eq!(rt.voice_count(), 0);
        let a = start(&mut mpe, &mut rt, 2, 60);
        let b = start(&mut mpe, &mut rt, 3, 61);
        assert_ne!(rt.expression_id(a), rt.expression_id(b));
        assert_eq!(rt.note_selection(a).unwrap().articulation, 99);
        assert_eq!(rt.note_selection(a).unwrap().performance, domain);
        assert_eq!(rt.articulation(rt.performance(0).unwrap()), Ok(0));
        assert_eq!(rt.note_selection(b).unwrap().articulation, 99);
        apply(&mut mpe, &mut rt, packet(0x80, 1, 12, 0)).unwrap();
        assert!(rt.key_down(a).unwrap() && rt.key_down(b).unwrap());
        let mut out = [[0.; 2]; 4];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[1.; 2]; 4]);
        rt.panic();
        rt.flush_ended(|_| true);
    });
}
