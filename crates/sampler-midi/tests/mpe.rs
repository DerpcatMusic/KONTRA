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
            note_cells: 0,
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
    // Manager range 96 (the admitted limit): a full manager bend adds 96
    // semitones on top of the 48-semitone member bend, past the step limit.
    for (cc, value) in [(101, 0), (100, 0), (6, 96)] {
        apply(&mut mpe, &mut rt, packet(0xb0, 0, cc, value)).unwrap();
    }
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
        assert_eq!(apply(&mut mpe,&mut rt,packet(0x80,1,61,0)),Ok(Applied::Ignored),
            "the capacity-rejected press still owes its own actual physical Off");
        assert!(rt.input_held(a).unwrap()&&rt.input_held(b).unwrap());
        assert_eq!((expression(&rt,a),expression(&rt,b)),(before_a,before_b));
        assert_eq!(
            apply(&mut mpe, &mut foreign, bend(0, 0)),
            Err(ApplyError::Core(Error::StaleHandle))
        );
        assert_eq!(
            apply(&mut mpe, &mut rt, packet(0x90, 3, 60, 127)),
            Ok(Applied::Unsupported)
        );
        assert_eq!(apply(&mut mpe,&mut rt,packet(0x80,3,60,0)),Ok(Applied::Ignored),
            "an unsupported-profile press is absorbed without releasing either admitted owner");
        assert!(rt.input_held(a).unwrap()&&rt.input_held(b).unwrap());
        assert_eq!((expression(&rt,a),expression(&rt,b)),(before_a,before_b));
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
        assert_eq!(apply(&mut mpe,&mut rt,packet(0x80,1,61,0)),Ok(Applied::Ignored));
        rt.flush_ended(|_| true);
        assert_eq!(apply(&mut mpe,&mut rt,packet(0x90,1,61,127)),Err(ApplyError::Core(Error::Capacity)),
            "Core NOTE_END does not acknowledge the two still-held physical wire keys");
        assert_eq!(apply(&mut mpe,&mut rt,packet(0x80,1,61,0)),Ok(Applied::Ignored));
        for (channel,owner) in [(1,a),(2,b)] {
            let Applied::Released{note,..}=apply(&mut mpe,&mut rt,packet(0x80,channel,60,0)).unwrap() else {panic!("actual physical terminal lost")};
            assert_eq!(note,owner,"a retired Core slot keeps its original wire terminal identity");
        }
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

/// MIDI 2.0 (M2-104) min-center-max 7-to-32-bit upscaling, as an oracle.
fn spec7(value: u32) -> u32 {
    let shifted = value << 25;
    if value <= 64 {
        return shifted;
    }
    let mut repeat = (value & 63) << 19;
    let mut out = shifted;
    while repeat != 0 {
        out |= repeat;
        repeat >>= 6;
    }
    out
}

#[test]
fn pressure_and_timbre_keep_member_snapshots_and_preserve_unrelated_expression() {
    let scaled = spec7;
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
            // 96 semitones is admitted; 97 is the first out-of-range sensitivity.
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 6, 97)),
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
                Ok(Applied::Expression { owners: 2 })
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, a, 38, 0)),
                Ok(Applied::Expression { owners: 2 })
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
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 64).unwrap(), 0);
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
fn manager_selection_controls_and_member_expression_have_separate_owners() {
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut rt = runtime();
        let domain = rt.performance(0).unwrap();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 1, 8).unwrap();
        support::without_heap(|| {
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, manager, 96, 0)),
                Ok(Applied::Unsupported)
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, manager, 97, 0)),
                Ok(Applied::Unsupported)
            );
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, manager, 1, 127)),
                Ok(Applied::Controller)
            );
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 1).unwrap(), u32::MAX);
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, member, 1, 0)),
                Ok(Applied::Unsupported)
            );
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 1).unwrap(), u32::MAX);
            let note = start(&mut mpe, &mut rt, member, 60);
            apply(&mut mpe, &mut rt, packet(0xb0, member, 74, 127)).unwrap();
            assert_eq!(expression(&rt, note).timbre, u32::MAX);
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 74).unwrap(), 0);
            assert_eq!(rt.note_controller(note, 1).unwrap(), u32::MAX);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 1, 0)).unwrap();
            assert_eq!(rt.note_controller(note, 1).unwrap(), u32::MAX);
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 1).unwrap(), 0);
            assert_eq!(
                apply(&mut mpe, &mut rt, packet(0xb0, member, 64, 127)),
                Ok(Applied::Ignored)
            );
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 64).unwrap(), 0);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 64, 127)).unwrap();
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 64).unwrap(), u32::MAX);
            apply(&mut mpe, &mut rt, packet(0x80, member, 60, 0)).unwrap();
            assert!(rt.note(note).unwrap().2);
            apply(&mut mpe, &mut rt, packet(0xb0, manager, 64, 0)).unwrap();
            assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: member }, 64).unwrap(), 0);
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
            note_cells: 0,
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

#[test]
fn script_note_off_keeps_fifo_pairing_and_member_expression_until_the_actual_host_keyup() {
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        support::without_heap(|| {
            let old = start(&mut mpe, &mut rt, member, 60);
            rt.replace_script_key_up_at(old, rt.now()).unwrap();
            assert!(!rt.key_down(old).unwrap() && rt.input_held(old).unwrap());
            rt.flush_ended(|_| panic!("early script off must not consume the pending input"));
            let fresh = start(&mut mpe, &mut rt, member, 60);
            assert_eq!(
                apply(&mut mpe, &mut rt, bend(member, 9216)),
                Ok(Applied::Expression { owners: 2 })
            );
            assert_eq!(expression(&rt, old), expression(&rt, fresh));
            assert!(
                matches!(apply(&mut mpe, &mut rt, packet(0x80, member, 60, 0)), Ok(Applied::Released { note, .. }) if note == old)
            );
            assert!(!rt.input_held(old).unwrap());
            assert!(rt.key_down(fresh).unwrap() && rt.input_held(fresh).unwrap());
            assert_eq!(
                apply(&mut mpe, &mut rt, bend(member, 10240)),
                Ok(Applied::Expression { owners: 1 })
            );
            assert_eq!(expression(&rt, old).pitch_semitones, pitch(9216, 48.));
            assert_eq!(
                apply(&mut mpe, &mut rt, bend(manager, 12288)),
                Ok(Applied::Expression { owners: 2 })
            );
            assert_eq!(
                expression(&rt, old).pitch_semitones,
                pitch(9216, 48.) + pitch(12288, 2.)
            );
            assert_eq!(
                expression(&rt, fresh).pitch_semitones,
                pitch(10240, 48.) + pitch(12288, 2.)
            );
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn mpe_filter_destinations_follow_captured_notes_across_member_channel_reuse() {
    use sampler_core::{
        ControlRange, Parameter, Processor, StateVariableFilter, SvfMode, VoiceChain,
    };
    let runtime = || {
        let plan = Prepared::new(
            48000,
            vec![
                Pcm::new(
                    48000,
                    (0..512)
                        .map(|i| [(i as f32 * 0.13).sin(), (i as f32 * 0.07).cos()])
                        .collect(),
                )
                .unwrap(),
            ],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::new(0, 0, 0, 1., 64).unwrap(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_controls(vec![sampler_core::ControlDefinition {
            id: sampler_core::ControlId(1),
            domain: sampler_core::ControlDomain::Toggle,
            default: sampler_core::ControlValue::Toggle(false),
        }])
        .unwrap()
        .with_voice_chains(
            vec![
                VoiceChain::new(
                    vec![],
                    vec![Processor::StateVariable(StateVariableFilter {
                        mode: SvfMode::LowPass,
                        cutoff_hz: Parameter::Expression {
                            source: sampler_core::ExpressionSource::Timbre,
                            low: 500.,
                            high: 8000.,
                        },
                        // A note-owned destination may share another global control with
                        // a ramp. Both scopes must reach the same filter coefficient grid.
                        q: Parameter::Control(ControlRange {
                            control: sampler_core::ControlId(1),
                            low: 0.5,
                            high: 3.,
                            ramp_frames: 24,
                        }),
                    })],
                    32,
                )
                .unwrap(),
            ],
            vec![Some(0)],
        )
        .unwrap();
        Runtime::new(
            plan,
            Limits {
                notes: 4,
                channels: 4,
                performances: 1,
                families: 4,
                voices: 4,
                expressions: 4,
                decisions: 0,
                commands: 4,
                behaviors: 0,
                behavior_fuel: 0,
                behavior_cells: 0,
                note_cells: 0,
            },
        )
        .unwrap()
    };
    for block in [1, 7, 64] {
        let mut actual = runtime();
        let mut expected = runtime();
        let mut mpe = Mpe::new(&actual, 7, 3, Zone::Lower, 2, 4).unwrap();
        support::without_heap(|| {
            apply(&mut mpe, &mut actual, packet(0xd0, 1, 127, 0)).unwrap();
            apply(&mut mpe, &mut actual, packet(0xb0, 1, 74, 0)).unwrap();
            let old = start(&mut mpe, &mut actual, 1, 60);
            let native_input = |id| sampler_core::Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(id),
            };
            let original = Expression {
                pressure: u32::MAX,
                timbre: 0,
                ..Expression::default()
            };
            let old_reference = expected
                .trigger_with_expression(native_input(1), 60, 1., original)
                .unwrap();
            let compare = |actual: &mut Runtime, expected: &mut Runtime, count: usize| {
                let mut a = [[0.; 2]; 96];
                let mut b = [[0.; 2]; 96];
                for chunk in a[..count].chunks_mut(block) {
                    actual.render(chunk).unwrap();
                }
                expected.render(&mut b[..count]).unwrap();
                assert_eq!(a, b);
            };
            compare(&mut actual, &mut expected, 16);
            apply(&mut mpe, &mut actual, packet(0x80, 1, 60, 0)).unwrap();
            expected.key_up(old_reference, None).unwrap();
            apply(&mut mpe, &mut actual, packet(0xb0, 1, 74, 127)).unwrap();
            let new = start(&mut mpe, &mut actual, 1, 60);
            let new_reference = expected
                .trigger_with_expression(
                    native_input(2),
                    60,
                    1.,
                    Expression {
                        timbre: u32::MAX,
                        ..original
                    },
                )
                .unwrap();
            assert_eq!(expression(&actual, old), original);
            assert_eq!(expression(&actual, new).timbre, u32::MAX);
            for rt in [&mut actual, &mut expected] {
                rt.edit_controls(
                    rt.active_plan(),
                    None,
                    &[sampler_core::ControlWrite {
                        id: sampler_core::ControlId(1),
                        value: sampler_core::ControlValue::Toggle(true),
                    }],
                )
                .unwrap();
            }
            compare(&mut actual, &mut expected, 8);
            apply(&mut mpe, &mut actual, packet(0xb0, 1, 74, 0)).unwrap();
            expected
                .set_expression(expected.expression_id(new_reference).unwrap(), original)
                .unwrap();
            compare(&mut actual, &mut expected, 96);
            actual.panic();
            expected.panic();
            actual.flush_ended(|_| true);
            expected.flush_ended(|_| true);
            assert_eq!(
                (
                    actual.note_count(),
                    actual.voice_count(),
                    actual.expression_count()
                ),
                (0, 0, 0)
            );
        });
    }
}

#[test]
fn host_owned_notes_follow_zone_bends_transposition_and_pedals() {
    let mut rt = runtime_with_channels(16);
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 15, 4).unwrap();
    let input = sampler_core::Input {
        protocol: Protocol::Midi1,
        port: 7,
        group: 3,
        channel: 0,
        key: 60,
        external_id: Some(9),
    };
    let note = mpe.trigger(&mut rt, 0, input, 1.0).unwrap();
    // A wire note on the same key stays a separate owner.
    start(&mut mpe, &mut rt, 0, 60);
    apply(&mut mpe, &mut rt, bend(0, 16383)).unwrap();
    assert_eq!(expression(&rt, note).pitch_semitones, pitch(16383, 2.0));
    mpe.transpose(&mut rt, -12.0).unwrap();
    assert_eq!(
        expression(&rt, note).pitch_semitones,
        pitch(16383, 2.0) - 12.0
    );
    apply(&mut mpe, &mut rt, bend(0, 8192)).unwrap();
    assert_eq!(
        expression(&rt, note).pitch_semitones,
        -12.0,
        "bends keep the offset"
    );
    assert_eq!(
        apply(&mut mpe, &mut rt, packet(0xb0, 0, 64, 127)).unwrap(),
        Applied::Pedal
    );
    rt.note_off(input, None).unwrap();
    assert!(rt.note(note).unwrap().2, "sustain holds the host note");
    apply(&mut mpe, &mut rt, packet(0xb0, 0, 64, 0)).unwrap();
    assert!(!rt.note(note).unwrap().2);
    assert_eq!(
        mpe.trigger(&mut runtime(), 0, input, 1.0),
        Err(ApplyError::Core(Error::StaleHandle))
    );
}

#[test]
fn midi2_messages_play_the_zone_at_full_precision() {
    let mut rt = runtime();
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    let midi2 = |mpe: &mut Mpe, rt: &mut Runtime, status: u8, a: u8, b: u8, data: u32| {
        let first = 0x4300_0000 | (u32::from(status) << 16) | (u32::from(a) << 8) | u32::from(b);
        mpe.apply(rt, Packets::new(&[first, data]).next().unwrap().unwrap())
    };
    // Velocity 0x8001 of 16 bits, beyond any 7-bit step.
    let Applied::Started(note) = midi2(&mut mpe, &mut rt, 0x90, 60, 0, 0x8001_0000).unwrap() else {
        panic!("not admitted")
    };
    assert_eq!(rt.note(note).unwrap().1, f64::from(0x8001u16) / 65535.0);
    // A quarter-step 32-bit bend over the default 2 semitones.
    midi2(&mut mpe, &mut rt, 0xe0, 0, 0, 0xa000_0000).unwrap();
    let pitch = expression(&rt, note).pitch_semitones;
    assert!(
        (pitch - 2.0 * f64::from(0x2000_0000u32) / f64::from(0x7fff_ffffu32)).abs() < 1e-12,
        "{pitch}"
    );
    // Pressure keeps all 32 bits.
    midi2(&mut mpe, &mut rt, 0xd0, 0, 0, 0x1234_5678).unwrap();
    assert_eq!(expression(&rt, note).pressure, 0x1234_5678);
    // Registered Controller 0:0 sets the bend range: 12 semitones.
    midi2(&mut mpe, &mut rt, 0x20, 0, 0, 12 << 25).unwrap();
    assert_eq!(mpe.pitch_sensitivities_for(sampler_midi::Version::Midi2).0, 12.);
    assert_eq!(mpe.pitch_ranges().0, 2, "a MIDI2 RC does not replace MIDI1 RPN state");
    let expected=12.0*f64::from(0x2000_0000u32)/f64::from(0x7fff_ffffu32);
    assert!((expression(&rt,note).pitch_semitones-expected).abs()<1e-12);
    assert_eq!(expression(&rt,note).pressure,0x1234_5678);
}

#[test]
fn script_owned_attacks_keep_mpe_expression_pairing_and_release() {
    let mut rt = runtime();
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    apply(&mut mpe, &mut rt, bend(0, 12288)).unwrap();
    apply(&mut mpe, &mut rt, bend(1, 9216)).unwrap();
    support::without_heap(|| {
        let word = packet(0x90, 1, 60, 100);
        let Applied::Started(note) = mpe
            .apply_silent(&mut rt, Packets::new(&[word]).next().unwrap().unwrap())
            .unwrap()
        else {
            panic!("silent note not admitted")
        };
        assert_eq!(rt.voice_count(), 0, "the Lua owner selects the attack");
        assert_eq!(
            expression(&rt, note).pitch_semitones,
            pitch(12288, 2.0) + pitch(9216, 48.0)
        );
        rt.forward_attack(note).unwrap();
        assert_eq!(rt.voice_count(), 1);
        let off = packet(0x80, 1, 60, 0);
        assert!(
            matches!(mpe.apply_silent(&mut rt, Packets::new(&[off]).next().unwrap().unwrap()).unwrap(), Applied::Released { note: released, .. } if released == note)
        );
        assert!(!rt.key_down(note).unwrap());
        rt.render(&mut [[0.; 2]; 512]).unwrap();
        assert_eq!(rt.voice_count(), 0);
    });
}

#[test]
fn physical_rpn_coarse_fine_and_relative_steps_keep_literal_pitch_and_tail_ownership() {
    // Independent values from preserved shared_primary_mpe_rpn_tests: physical
    // selectors are separate, coarse data clears cents, and RP-018 steps one cent.
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut rt = runtime();
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        support::without_heap(|| {
            apply(&mut mpe, &mut rt, bend(manager, 4096)).unwrap();
            apply(&mut mpe, &mut rt, bend(member, 7168)).unwrap();
            let tail = start(&mut mpe, &mut rt, member, 60);
            apply(&mut mpe, &mut rt, packet(0x80, member, 60, 0)).unwrap();
            let active = start(&mut mpe, &mut rt, member, 61);
            for (channel, cc) in [(member, 101), (manager, 101), (member, 100), (manager, 100)] {
                apply(&mut mpe, &mut rt, packet(0xb0, channel, cc, 0)).unwrap();
            }
            for (channel, cc, value) in [
                (member, 6, 12),
                (member, 38, 50),
                (manager, 6, 4),
                (manager, 38, 25),
            ] {
                apply(&mut mpe, &mut rt, packet(0xb0, channel, cc, value)).unwrap();
            }
            assert_eq!(mpe.pitch_sensitivities(), (4.25, 12.5));
            assert_eq!(expression(&rt, active).pitch_semitones, -3.6875);
            assert_eq!(expression(&rt, tail).pitch_semitones, -8.125);
            apply(&mut mpe, &mut rt, packet(0xb0, member, 96, 99)).unwrap();
            assert!((expression(&rt, active).pitch_semitones - -3.68875).abs() < 1e-12);
            apply(&mut mpe, &mut rt, packet(0xb0, member, 97, 127)).unwrap();
            assert_eq!(mpe.pitch_sensitivities(), (4.25, 12.5));
            apply(&mut mpe, &mut rt, packet(0xb0, member, 38, 99)).unwrap();
            apply(&mut mpe, &mut rt, packet(0xb0, member, 6, 8)).unwrap();
            assert_eq!(mpe.pitch_sensitivities(), (4.25, 8.));
            assert_eq!(expression(&rt, active).pitch_semitones, -3.125);
            // Null and NRPN selectors must not reuse the last valid RPN0.
            for (cc, value) in [
                (101, 127),
                (100, 127),
                (6, 80),
                (38, 99),
                (96, 0),
                (99, 0),
                (98, 0),
                (6, 90),
            ] {
                apply(&mut mpe, &mut rt, packet(0xb0, member, cc, value)).unwrap();
            }
            assert_eq!(mpe.pitch_sensitivities(), (4.25, 8.));
            assert_eq!(expression(&rt, tail).pitch_semitones, -8.125);
        });
    }
}

#[test]
fn midi2_registered_sensitivity_retains_cents_and_legacy_parameter_ccs_do_not_rebind_it() {
    let mut rt = runtime();
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    let note = start(&mut mpe, &mut rt, 1, 60);
    apply(&mut mpe, &mut rt, bend(1, 7168)).unwrap();
    support::without_heap(|| {
        let data = (12u32 << 25) | (50u32 << 18);
        let words = [0x4321_0000, data];
        mpe.apply(&mut rt, Packets::new(&words).next().unwrap().unwrap())
            .unwrap();
        assert_eq!(mpe.pitch_sensitivities(), (2., 48.));
        assert_eq!(mpe.pitch_sensitivities_for(sampler_midi::Version::Midi2), (2., 12.5));
        assert_eq!(expression(&rt, note).pitch_semitones, -6.,
            "a MIDI2 Registered Controller cannot retune a retained MIDI1 owner");
        for cc in [0u8, 6, 32, 38, 98, 99, 100, 101] {
            let words = [0x43b1_0000 | u32::from(cc) << 8, u32::MAX];
            assert_eq!(
                mpe.apply(&mut rt, Packets::new(&words).next().unwrap().unwrap()),
                Ok(Applied::Ignored)
            );
        }
        assert_eq!(mpe.pitch_sensitivities(), (2., 48.));
        assert_eq!(mpe.pitch_sensitivities_for(sampler_midi::Version::Midi2), (2., 12.5));
        assert_eq!(expression(&rt, note).pitch_semitones, -6.,
            "a MIDI2 Registered Controller cannot retune a retained MIDI1 owner");
    });
}


#[test]
fn silent_host_admission_matches_wire_silent_expression_before_the_script_attack() {
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut host = runtime(); let mut wire = runtime(); let mut sounding = runtime();
        let mut hm = Mpe::new(&host, 7, 3, zone, 2, 8).unwrap();
        let mut wm = Mpe::new(&wire, 7, 3, zone, 2, 8).unwrap();
        let mut sm = Mpe::new(&sounding, 7, 3, zone, 2, 8).unwrap();
        for (mpe, rt) in [(&mut hm, &mut host), (&mut wm, &mut wire), (&mut sm, &mut sounding)] {
            apply(mpe, rt, bend(manager, 12288)).unwrap(); apply(mpe, rt, bend(member, 9216)).unwrap();
            apply(mpe, rt, packet(0xd0, member, 70, 0)).unwrap();
            apply(mpe, rt, packet(0xb0, member, 74, 40)).unwrap();
        }
        let input = sampler_core::Input { protocol: Protocol::Midi1, port: 7, group: 3,
            channel: member, key: 60, external_id: Some(47) };
        let note = hm.trigger_silent(&mut host, member, input, 100. / 127.).unwrap();
        let word = packet(0x90, member, 60, 100);
        let Applied::Started(wn) = wm.apply_silent(&mut wire, Packets::new(&[word]).next().unwrap().unwrap()).unwrap() else {
            panic!("wire silent admission failed")
        };
        let sn = sm.trigger(&mut sounding, member, input, 100. / 127.).unwrap();
        assert_eq!(expression(&host, note), expression(&wire, wn));
        assert_eq!(expression(&host, note), expression(&sounding, sn));
        assert_eq!((host.voice_count(), wire.voice_count(), sounding.voice_count()), (0, 0, 1));
        host.forward_attack(note).unwrap(); wire.forward_attack(wn).unwrap();
        for (mpe, rt) in [(&mut hm, &mut host), (&mut wm, &mut wire), (&mut sm, &mut sounding)] {
            apply(mpe, rt, bend(member, 10240)).unwrap();
            apply(mpe, rt, packet(0xd0, member, 90, 0)).unwrap();
            apply(mpe, rt, packet(0xb0, member, 74, 10)).unwrap();
        }
        assert_eq!(expression(&host, note), expression(&wire, wn));
        assert_eq!(expression(&host, note), expression(&sounding, sn));
        let mut a = [[0.; 2]; 96]; let mut b = a; let mut c = a;
        host.render(&mut a).unwrap(); wire.render(&mut b).unwrap(); sounding.render(&mut c).unwrap();
        assert_eq!(a, b); assert_eq!(a, c);
    }
}

#[test]
fn host_tuning_is_independent_of_member_bends_and_reused_channel_release_snapshots() {
    for (zone, manager, member, other) in [(Zone::Lower, 0, 1, 2), (Zone::Upper, 15, 14, 13)] {
        let mut rt = runtime(); let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        let input = |channel, id| sampler_core::Input { protocol: Protocol::Midi1, port: 7,
            group: 3, channel, key: 60, external_id: Some(id) };
        mpe.transpose(&mut rt, 3.).unwrap();
        apply(&mut mpe, &mut rt, bend(manager, 12288)).unwrap();
        apply(&mut mpe, &mut rt, bend(member, 9216)).unwrap();
        let a = mpe.trigger_silent_with_tuning(&mut rt, member, input(member, 11), 1., 0.75).unwrap();
        let b = mpe.trigger_silent_with_tuning(&mut rt, member, input(member, 12), 1., -0.5).unwrap();
        let c = mpe.trigger_with_tuning(&mut rt, other, input(other, 13), 1., 1.5).unwrap();
        let combined = pitch(12288, 2.) + pitch(9216, 48.) + 3.;
        assert_eq!(expression(&rt, a).pitch_semitones, combined + 0.75);
        assert_eq!(expression(&rt, b).pitch_semitones, combined - 0.5);
        rt.forward_attack(a).unwrap(); rt.forward_attack(b).unwrap();
        let other_before = expression(&rt, c);
        mpe.set_note_tuning(&mut rt, b, 1.25).unwrap();
        assert_eq!(expression(&rt, b).pitch_semitones, combined + 1.25,
            "channel bend then native tuning must preserve both components");
        assert_eq!(expression(&rt, a).pitch_semitones, combined + 0.75);
        apply(&mut mpe, &mut rt, bend(member, 10240)).unwrap();
        let combined = pitch(12288, 2.) + pitch(10240, 48.) + 3.;
        assert_eq!(expression(&rt, a).pitch_semitones, combined + 0.75);
        assert_eq!(expression(&rt, b).pitch_semitones, combined + 1.25);
        assert_eq!(expression(&rt, c), other_before);
        rt.note_off_exact(a, None).unwrap();
        let released = expression(&rt, a);
        let reused = mpe.trigger_silent_with_tuning(&mut rt, member, input(member, 14), 1., -1.).unwrap();
        rt.forward_attack(reused).unwrap();
        apply(&mut mpe, &mut rt, bend(member, 8192)).unwrap();
        assert_eq!(expression(&rt, a), released, "member reuse must not retarget the released owner");
        assert_eq!(expression(&rt, b).pitch_semitones, pitch(12288, 2.) + 3. + 1.25);
        assert_eq!(expression(&rt, reused).pitch_semitones, pitch(12288, 2.) + 3. - 1.);
        apply(&mut mpe, &mut rt, bend(manager, 0)).unwrap();
        assert_eq!(expression(&rt, a).pitch_semitones, pitch(10240, 48.) + 3. + 0.75 - 2.);
        let before = (rt.note_count(), rt.voice_count(), rt.expression_count());
        assert_eq!(mpe.trigger_silent_with_tuning(&mut rt, member, input(member, 15), 1., f64::NAN),
            Err(ApplyError::Core(Error::InvalidInput)));
        assert_eq!((rt.note_count(), rt.voice_count(), rt.expression_count()), before);
        let retained = expression(&rt, b);
        assert_eq!(mpe.set_note_tuning(&mut rt, b, f64::INFINITY), Err(Error::InvalidInput));
        assert_eq!(expression(&rt, b), retained);
    }
}


#[test]
fn public_host_trigger_rejects_out_of_zone_channels_before_any_admission() {
    for (zone, manager, member, rejected) in [
        (Zone::Lower, 0, 1, [2, 15, 16, 30, 255]),
        (Zone::Upper, 15, 14, [13, 0, 16, 30, 255]),
    ] {
        let mut rt = runtime_with_channels(16);
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 1, 8).unwrap();
        let input = |channel, id| sampler_core::Input {
            protocol: Protocol::Midi1,
            port: 7,
            group: 3,
            channel,
            key: 60,
            external_id: Some(id),
        };
        let manager_note = mpe.trigger(&mut rt, manager, input(manager, 1), 0.75).unwrap();
        let member_note = mpe.trigger(&mut rt, member, input(member, 2), 0.5).unwrap();
        apply(&mut mpe, &mut rt, bend(member, 9216)).unwrap();
        rt.render(&mut [[0.0; 2]; 64]).unwrap();
        let before = (
            rt.now(), rt.note_count(), rt.expression_count(), rt.voice_count(),
            rt.pending_commands(), rt.note(manager_note).unwrap(), rt.note(member_note).unwrap(),
            expression(&rt, manager_note), expression(&rt, member_note),
        );
        for channel in rejected {
            // Keep Input valid: the public zone-channel argument itself must
            // be checked before indexing its sixteen retained channel rows.
            assert_eq!(
                mpe.trigger(&mut rt, channel, input(member, 3), 1.0),
                Err(ApplyError::Core(Error::InvalidInput)),
                "zone={zone:?}, rejected channel={channel}"
            );
            assert_eq!(
                (
                    rt.now(), rt.note_count(), rt.expression_count(), rt.voice_count(),
                    rt.pending_commands(), rt.note(manager_note).unwrap(), rt.note(member_note).unwrap(),
                    expression(&rt, manager_note), expression(&rt, member_note),
                ),
                before,
                "refusal must preserve the existing held owners and clock"
            );
        }
        let next = mpe.trigger(&mut rt, member, input(member, 3), 1.0).unwrap();
        assert_eq!(rt.note_count(), before.1 + 1);
        assert_eq!(expression(&rt, next), expression(&rt, member_note));
    }
}

fn rich_voice_runtime() -> Runtime {
    use sampler_core::{ColorSettings, CurveSettings, ModProgram, ModRoute, ModSource, ModTarget, Parameter,
        Processor, Rectifier, StateVariableFilter, SvfMode, VoiceChain};
    let pcm=Pcm::new(48000,(0..16384).map(|i|{
        let x=((i*137)%4096) as f32/2048.-1.;[x,x*0.5]
    }).collect()).unwrap();
    let chain=VoiceChain::new(vec![Processor::Gain(1.5),Processor::Color(ColorSettings::curve(CurveSettings::Rectifier(Rectifier::Full)))],
        vec![Processor::StateVariable(StateVariableFilter{mode:SvfMode::LowPass,
            cutoff_hz:Parameter::Constant(4000.),q:Parameter::Constant(0.8)})],0).unwrap();
    let mut tone=ModRoute::new(1,ModTarget::Tone,60.);tone.shape=Some(0);
    let program=ModProgram{sources:vec![ModSource::Pressure,ModSource::Timbre],
        routes:vec![ModRoute::new(0,ModTarget::Decibels,6.),tone],
        shapes:vec![vec![(0.,-1.),(0.5,0.),(1.,0.)]],..Default::default()};
    let prepared=Prepared::new(48000,vec![pcm],vec![Region{sample:0,key_low:60,key_high:61,root_key:None,
        velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,256).unwrap(),playback:Playback::default()}],2).unwrap()
        .with_voice_chains(vec![chain],vec![Some(0)]).unwrap()
        .with_voice_modulation(vec![program],vec![Some(0)],vec![0]).unwrap();
    let limits=Limits::for_plan(&prepared,8,16);Runtime::new(prepared,limits).unwrap()
}
fn rich_sum(actual:&[[f32;2]],a:&[[f32;2]],b:&[[f32;2]]) {
    for (frame,((out,a),b)) in actual.iter().zip(a).zip(b).enumerate() {
        for c in 0..2 { assert!((out[c]-(a[c]+b[c])).abs()<3e-6,"frame {frame},channel {c}"); }
    }
}
#[test]
fn rich_authored_voice_chain_keeps_mpe_host_identities_and_held_member_isolation() {
    let mut actual=rich_voice_runtime();let mut first=rich_voice_runtime();let mut second=rich_voice_runtime();
    let mut am=Mpe::new(&actual,7,3,Zone::Lower,2,8).unwrap();
    let mut fm=Mpe::new(&first,7,3,Zone::Lower,2,8).unwrap();let mut sm=Mpe::new(&second,7,3,Zone::Lower,2,8).unwrap();
    let host=|channel,id|sampler_core::Input{protocol:Protocol::Midi1,port:7,group:3,channel,key:60,external_id:Some(id)};
    let a=am.trigger_with_tuning(&mut actual,1,host(1,101),1.,0.5).unwrap();
    let b=am.trigger_with_tuning(&mut actual,2,host(2,202),1.,-0.5).unwrap();
    fm.trigger_with_tuning(&mut first,1,host(1,101),1.,0.5).unwrap();
    sm.trigger_with_tuning(&mut second,2,host(2,202),1.,-0.5).unwrap();
    assert_ne!(actual.expression_id(a),actual.expression_id(b));
    let mut x=[[0.;2];128];let mut y=x;let mut z=x;
    support::without_heap(||{
        actual.render(&mut x).unwrap();first.render(&mut y).unwrap();second.render(&mut z).unwrap();
        rich_sum(&x,&y,&z);
        let before=expression(&actual,b);let voices=actual.voice_count();
        for word in [packet(0xb0,1,74,8),packet(0xd0,1,90,0),bend(1,8704)] {
            apply(&mut am,&mut actual,word).unwrap();apply(&mut fm,&mut first,word).unwrap();
        }
        assert_eq!(expression(&actual,b),before);assert_eq!(actual.voice_count(),voices);
        actual.render(&mut x).unwrap();first.render(&mut y).unwrap();second.render(&mut z).unwrap();rich_sum(&x,&y,&z);
        actual.note_off_exact(a,None).unwrap();
        let released=expression(&actual,a);
        let next=am.trigger_with_tuning(&mut actual,1,host(1,303),1.,1.25).unwrap();
        apply(&mut am,&mut actual,bend(1,8192)).unwrap();
        assert_eq!(expression(&actual,a),released);
        assert!((expression(&actual,next).pitch_semitones-1.25).abs()<1e-12);
        assert_eq!(expression(&actual,b),before);
        actual.panic();first.panic();second.panic();
        actual.render(&mut x).unwrap();first.render(&mut y).unwrap();second.render(&mut z).unwrap();
    });
    assert_eq!((actual.nonfinite_frames(),actual.stream_underruns()),(0,0));
}


fn configure_mcm(mpe: &mut Mpe, rt: &mut Runtime, manager: u8, members: u8) -> Applied {
    apply(mpe, rt, packet(0xb0, manager, 101, 0)).unwrap();
    apply(mpe, rt, packet(0xb0, manager, 100, 6)).unwrap();
    apply(mpe, rt, packet(0xb0, manager, 6, members)).unwrap()
}

#[test]
fn mcm_shrink_stops_only_changed_channel_owners_and_resets_their_real_cc_banks() {
    for (zone, manager, kept, removed) in [(Zone::Lower, 0, 1, 3), (Zone::Upper, 15, 14, 12)] {
        let mut rt = runtime_with_channels(16);
        let mut reference = runtime_with_channels(16);
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 15, 8).unwrap();
        let mut other = Mpe::new(&reference, 7, 3, zone, 2, 8).unwrap();
        let domain = rt.performance(0).unwrap();
        apply(&mut mpe, &mut rt, packet(0xb0, manager, 1, 127)).unwrap();
        apply(&mut other, &mut reference, packet(0xb0, manager, 1, 127)).unwrap();
        apply(&mut mpe, &mut rt, bend(kept, 9216)).unwrap();
        apply(&mut other, &mut reference, bend(kept, 9216)).unwrap();
        let retained = start(&mut mpe, &mut rt, kept, 60);
        let retained_reference = start(&mut other, &mut reference, kept, 60);
        let stopped = start(&mut mpe, &mut rt, removed, 61);
        rt.render(&mut [[0.; 2]; 64]).unwrap(); reference.render(&mut [[0.; 2]; 64]).unwrap();
        let before = expression(&rt, retained);
        support::without_heap(|| {
            assert_eq!(configure_mcm(&mut mpe, &mut rt, manager, 2), Applied::Configuration);
            assert_eq!(expression(&rt, retained), before);
            assert!(!matches!(rt.input_held(stopped), Ok(true)));
            assert_eq!(rt.voice_count(), 1);
            let address = |channel| ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel };
            assert_eq!(rt.channel_controller(domain, address(kept), 1), Ok(u32::MAX));
            assert_eq!(rt.channel_controller(domain, address(removed), 1), Ok(0));
            assert_eq!(rt.channel_controller(domain, address(removed), 11), Ok(u32::MAX));
            assert_eq!(rt.controller(domain, 1), Ok(0), "explicit global state is not a copy of the last channel");
            assert_eq!(expression(&rt, retained), expression(&reference, retained_reference));
            let mut actual = [[0.; 2]; 128]; let mut expected = [[0.; 2]; 128];
            rt.render(&mut actual).unwrap(); reference.render(&mut expected).unwrap();
            assert_eq!(actual, expected, "same retained source/cursor must equal the independent narrow-zone runtime");
        });
    }
}

#[test]
fn mcm_newest_overlap_wins_and_zero_disables_the_selected_zone_without_stranding_owners() {
    let mut rt = runtime_with_channels(16);
    let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 15, 8).unwrap();
    let kept = start(&mut mpe, &mut rt, 6, 60);
    let moved = start(&mut mpe, &mut rt, 14, 61);
    assert_eq!(configure_mcm(&mut mpe, &mut rt, 15, 7), Applied::Configuration);
    assert_eq!(mpe.configuration(), Some([7, 7]));
    assert!(rt.input_held(kept).unwrap());
    assert!(!matches!(rt.input_held(moved), Ok(true)));
    assert_eq!(apply(&mut mpe, &mut rt, packet(0x90, 14, 60, 127)), Ok(Applied::Ignored));
    assert_eq!(configure_mcm(&mut mpe, &mut rt, 0, 15), Applied::Configuration);
    assert_eq!(mpe.configuration(), Some([15, 0]));
    assert!(rt.input_held(kept).unwrap());
    assert_eq!(configure_mcm(&mut mpe, &mut rt, 0, 0), Applied::Configuration);
    assert_eq!(mpe.configuration(), Some([0, 0]));
    assert!(!matches!(rt.input_held(kept), Ok(true)));
    assert_eq!(rt.voice_count(), 0);
    // With both zones off, this selected instrument deliberately falls back
    // to conventional manager-channel reception; no member is still MPE.
    assert!(!mpe.accepts_channel(1)); assert!(mpe.accepts_channel(0));
    let conventional = start(&mut mpe, &mut rt, 0, 60);
    assert_eq!(expression(&rt, conventional).pitch_semitones, 0.);
}

#[test]
fn mcm_data_lsb_and_invalid_members_do_not_resize_or_leak_into_controller_state() {
    for (zone, manager, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
        let mut rt = runtime_with_channels(16);
        let mut mpe = Mpe::new(&rt, 7, 3, zone, 2, 8).unwrap();
        assert_eq!(configure_mcm(&mut mpe, &mut rt, manager, 1), Applied::Configuration);
        let configured = mpe.configuration();
        let domain = rt.performance(0).unwrap();
        let address = ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel: manager };
        let old_lsb = rt.channel_controller(domain, address, 38).unwrap();
        for index in [38, 96, 97] {
            assert_eq!(apply(&mut mpe, &mut rt, packet(0xb0, manager, index, 127)), Ok(Applied::Ignored));
            assert_eq!(mpe.configuration(), configured);
        }
        assert_eq!(rt.channel_controller(domain, address, 38), Ok(old_lsb));
        // CC38 is unused ONLY while RPN6 is selected; RPN0 cents retain
        // their ordinary meaning and cannot accidentally resize a zone.
        for (cc, value) in [(101, 0), (100, 0), (6, 12), (38, 50)] {
            apply(&mut mpe, &mut rt, packet(0xb0, manager, cc, value)).unwrap();
        }
        assert_eq!(mpe.pitch_sensitivities().0, 12.5);
        assert_eq!(mpe.configuration(), configured);
        for (cc, value) in [(101, 0), (100, 6)] {
            apply(&mut mpe, &mut rt, packet(0xb0, manager, cc, value)).unwrap();
        }
        assert_eq!(apply(&mut mpe, &mut rt, packet(0xb0, manager, 6, 16)), Ok(Applied::Ignored));
        assert_eq!(apply(&mut mpe, &mut rt, packet(0xb0, manager, 6, 127)), Ok(Applied::Ignored));
        assert_eq!(mpe.configuration(), configured);
        // RPN selection is physical-channel local. A member cannot resize a zone.
        assert_eq!(configure_mcm(&mut mpe, &mut rt, member, 15), Applied::Ignored);
        assert_eq!(mpe.configuration(), configured);
        // Null and NRPN selection cancel MCM interpretation without mutating it.
        for (index, value) in [(101,127), (100,127), (6,2), (99,0), (98,6), (6,2)] {
            apply(&mut mpe, &mut rt, packet(0xb0, manager, index, value)).unwrap();
            assert_eq!(mpe.configuration(), configured);
        }
    }
}

#[test]
fn each_selected_mcm_restores_sensitivity_and_preserves_released_member_snapshots() {
    for (zone, manager, member) in [(Zone::Lower,0,1),(Zone::Upper,15,14)] {
        let mut rt = runtime_with_channels(16);
        let mut mpe = Mpe::new(&rt,7,3,zone,2,8).unwrap();
        for channel in [manager,member] {
            for (cc,value) in [(101,0),(100,0),(6,12)] { apply(&mut mpe,&mut rt,packet(0xb0,channel,cc,value)).unwrap(); }
        }
        apply(&mut mpe,&mut rt,bend(manager,12288)).unwrap();
        apply(&mut mpe,&mut rt,bend(member,9216)).unwrap();
        let tail = start(&mut mpe,&mut rt,member,60);
        apply(&mut mpe,&mut rt,packet(0x80,member,60,0)).unwrap();
        let held = start(&mut mpe,&mut rt,member,61);
        mpe.set_bend_range(Some(24));
        assert_eq!(configure_mcm(&mut mpe,&mut rt,manager,2),Applied::Configuration);
        assert_eq!(mpe.pitch_sensitivities(),(2.,48.));
        assert_eq!(expression(&rt,held).pitch_semitones,pitch(12288,2.)+pitch(9216,48.));
        assert_eq!(expression(&rt,tail).pitch_semitones,pitch(12288,2.)+pitch(9216,12.));
        apply(&mut mpe,&mut rt,bend(member,10240)).unwrap();
        assert_eq!(expression(&rt,held).pitch_semitones,pitch(12288,2.)+pitch(10240,48.));
        assert_eq!(expression(&rt,tail).pitch_semitones,pitch(12288,2.)+pitch(9216,12.));
    }
}

#[test]
fn mcm_from_conventional_mode_stops_old_mapped_owners_and_is_domain_checked() {
    let mut rt = runtime_with_channels(16);
    let mut mpe = Mpe::new(&rt,7,3,Zone::Lower,15,8).unwrap();
    mpe.set_manual_active(false);
    let old = start(&mut mpe,&mut rt,0,60);
    assert_eq!(configure_mcm(&mut mpe,&mut rt,0,2),Applied::Configuration);
    assert!(!matches!(rt.input_held(old),Ok(true)));
    assert_eq!(mpe.configuration(),Some([2,0]));
    let before = mpe.configuration();
    assert_eq!(apply(&mut mpe,&mut rt,packet(0xb0,0,6,7)^0x0100_0000),Err(ApplyError::DisabledGroup));
    assert_eq!(mpe.configuration(),before);
    let mut foreign = runtime_with_channels(16);
    assert_eq!(apply(&mut mpe,&mut foreign,packet(0xb0,0,6,7)),Err(ApplyError::Core(Error::StaleHandle)));
    assert_eq!(mpe.configuration(),before);
}

#[test]
#[ignore = "MAX_STEP is 2048 (132 semitones); no MIDI input can exceed it now, so this rejection is unreachable. Re-enable if a smaller step limit returns."]
fn rejected_mcm_pitch_reset_preserves_configuration_held_siblings_controllers_and_pcm() {
    fn prepared_pair() -> (Runtime, Mpe, NoteId, NoteId) {
        let mut rt = runtime_with_channels(16);
        let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 15, 8).unwrap();
        for (cc, value) in [(101, 0), (100, 0), (6, 0)] {
            apply(&mut mpe, &mut rt, packet(0xb0, 1, cc, value)).unwrap();
        }
        apply(&mut mpe, &mut rt, bend(1, 16383)).unwrap();
        apply(&mut mpe, &mut rt, packet(0xb0, 0, 1, 127)).unwrap();
        let held = mpe.trigger_with_tuning(&mut rt, 1, sampler_core::Input {
            protocol: Protocol::Midi1, port: 7, group: 3, channel: 1, key: 60, external_id: Some(91),
        }, 1., 49.).unwrap();
        let sibling = start(&mut mpe, &mut rt, 3, 61);
        // PreselectRPN6 before the tested Data Entry message: selector input
        // itself is a separate accepted input, not part of refused membership.
        for (cc, value) in [(101, 0), (100, 6)] {
            apply(&mut mpe, &mut rt, packet(0xb0, 0, cc, value)).unwrap();
        }
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        (rt, mpe, held, sibling)
    }
    let (mut rt, mut mpe, held, sibling) = prepared_pair();
    let (mut reference, _, _, _) = prepared_pair();
    let before = (rt.now(), rt.note_count(), rt.voice_count(), rt.expression_count(),
        rt.pending_commands(), expression(&rt, held), expression(&rt, sibling), mpe.pitch_sensitivities());
    support::without_heap(|| {
        // Range 0 admits tuning 49. Resetting to 48 with the retained positive
        // bend would require 97 semitones, beyond the 96-semitone step limit.
        assert_eq!(apply(&mut mpe, &mut rt, packet(0xb0, 0, 6, 2)), Err(ApplyError::Core(Error::InvalidInput)));
        assert_eq!(mpe.configuration(), None);
        assert_eq!((rt.now(), rt.note_count(), rt.voice_count(), rt.expression_count(),
            rt.pending_commands(), expression(&rt, held), expression(&rt, sibling), mpe.pitch_sensitivities()), before);
        assert!(rt.input_held(held).unwrap()); assert!(rt.input_held(sibling).unwrap());
        let domain = rt.performance(0).unwrap();
        for channel in [1, 3] { assert_eq!(rt.channel_controller(domain,
            ChannelAddress { protocol: Protocol::Midi1, port: 7, group: 3, channel }, 1), Ok(u32::MAX)); }
        let mut actual = [[0.; 2]; 128]; let mut expected = actual;
        rt.render(&mut actual).unwrap(); reference.render(&mut expected).unwrap();
        assert_eq!(actual, expected, "rejection cannot cancel sibling or change source cursor/history");
        mpe.set_note_tuning(&mut rt, held, 0.).unwrap();
        assert_eq!(apply(&mut mpe, &mut rt, packet(0xb0, 0, 6, 2)), Ok(Applied::Configuration));
        assert_eq!(mpe.configuration(), Some([2, 0]));
        assert!(!matches!(rt.input_held(sibling), Ok(true)));
    });
}

#[test]
fn mcm_classifies_the_actual_physical_controller_address_before_committing_surviving_expressions() {
    let mut rt = runtime_with_channels(16); let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 15, 8).unwrap();
    let note = mpe.trigger(&mut rt, 1, sampler_core::Input { protocol: Protocol::Midi1,
        port: 7, group: 3, channel: 3, key: 60, external_id: Some(73) }, 1.).unwrap();
    assert_eq!(configure_mcm(&mut mpe, &mut rt, 0, 2), Applied::Configuration);
    assert_eq!(mpe.configuration(), Some([2, 0]));
    assert!(!matches!(rt.input_held(note), Ok(true)), "actual physical channel3 changes roles even if gesture channel1 was separately supplied");
    let next = start(&mut mpe, &mut rt, 1, 60);
    assert!(rt.input_held(next).unwrap());
}

fn apply_midi2(mpe: &mut Mpe, rt: &mut Runtime, channel: u8, status: u8, a: u8, b: u8, data: u32) -> Result<Applied, ApplyError> {
    let words = [0x4300_0000 | u32::from(status | channel) << 16 | u32::from(a) << 8 | u32::from(b), data];
    mpe.apply(rt, Packets::new(&words).next().unwrap().unwrap())
}

#[test]
fn ordinary_midi2_channels_are_not_midi1_mpe_members_and_keep_independent_ranges_and_rich_pcm() {
    let mut actual = rich_voice_runtime(); let mut a = rich_voice_runtime(); let mut b = rich_voice_runtime();
    let mut am = Mpe::new(&actual, 7, 3, Zone::Lower, 1, 8).unwrap();
    let mut fm = Mpe::new(&a, 7, 3, Zone::Lower, 1, 8).unwrap();
    let mut sm = Mpe::new(&b, 7, 3, Zone::Lower, 1, 8).unwrap();
    for receiver in [&mut am, &mut fm, &mut sm] { receiver.use_midi2_channel_voice().unwrap(); }
    let Applied::Started(first) = apply_midi2(&mut am, &mut actual, 0, 0x90, 60, 0, 0xffff_0000).unwrap() else { panic!() };
    let Applied::Started(other) = apply_midi2(&mut am, &mut actual, 14, 0x90, 60, 0, 0xffff_0000).unwrap() else { panic!() };
    apply_midi2(&mut fm, &mut a, 0, 0x90, 60, 0, 0xffff_0000).unwrap();
    apply_midi2(&mut sm, &mut b, 14, 0x90, 60, 0, 0xffff_0000).unwrap();
    let first_id = actual.expression_id(first).unwrap(); let other_id = actual.expression_id(other).unwrap();
    let mut out = [[0.; 2]; 64]; let mut left = out; let mut right = out;
    support::without_heap(|| {
        // Even after a narrow negotiatedMIDI1 zone, actualMIDI2channel14
        // remains admitted and source0 bend cannot become a global manager.
        assert_eq!(configure_mcm(&mut am, &mut actual, 0, 1), Applied::Configuration);
        assert!(actual.input_held(first).unwrap()); assert!(actual.input_held(other).unwrap());
        for (channel, status, a1, b1, value) in [(0, 0xe0, 0, 0, 0xa000_0000),
            (14, 0xd0, 0, 0, 0x1234_5678), (0, 0xb0, 74, 0, 0x1000_0000),
            (14, 0x20, 0, 0, (12 << 25) | (50 << 18)), (14, 0xe0, 0, 0, 0xc000_0000)] {
            let before = if channel == 0 { expression(&actual, other) } else { expression(&actual, first) };
            apply_midi2(&mut am, &mut actual, channel, status, a1, b1, value).unwrap();
            if channel == 0 { apply_midi2(&mut fm, &mut a, channel, status, a1, b1, value).unwrap(); }
            else { apply_midi2(&mut sm, &mut b, channel, status, a1, b1, value).unwrap(); }
            assert_eq!(if channel == 0 { expression(&actual, other) } else { expression(&actual, first) }, before);
            actual.render(&mut out).unwrap(); a.render(&mut left).unwrap(); b.render(&mut right).unwrap();
            rich_sum(&out, &left, &right);
            assert_eq!(actual.expression_id(first), Ok(first_id)); assert_eq!(actual.expression_id(other), Ok(other_id));
            assert_eq!(actual.voice_count(), 2);
        }
        assert_eq!(am.channel_pitch_sensitivity(sampler_midi::Version::Midi2, 0), Ok(2.));
        assert_eq!(am.channel_pitch_sensitivity(sampler_midi::Version::Midi2, 14), Ok(12.5));
        assert_eq!(expression(&actual, first).pitch_semitones, 2. * 536_870_912. / 2_147_483_647.);
        assert_eq!(expression(&actual, other).pitch_semitones, 12.5 * 1_073_741_824. / 2_147_483_647.);
        apply_midi2(&mut am, &mut actual, 14, 0x80, 60, 0, 0).unwrap();
        apply_midi2(&mut sm, &mut b, 14, 0x80, 60, 0, 0).unwrap();
        apply_midi2(&mut am, &mut actual, 14, 0xe0, 0, 0, 0xa000_0000).unwrap();
        apply_midi2(&mut sm, &mut b, 14, 0xe0, 0, 0, 0xa000_0000).unwrap();
        assert_eq!(expression(&actual, other).pitch_semitones, 12.5 * 536_870_912. / 2_147_483_647.,
            "ordinary channel expression reaches releasingMIDI2notes; it is not the MPE memberfreeze law");
        actual.render(&mut out).unwrap(); a.render(&mut left).unwrap(); b.render(&mut right).unwrap(); rich_sum(&out, &left, &right);
    });
}

#[test]
fn strict_wire_protocols_and_explicit_host_domain_coexist_without_property_replacement() {
    let mut rt = runtime_with_channels(48); let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 2, 8).unwrap();
    mpe.enable_host_channel_projection().unwrap(); mpe.use_midi2_channel_voice().unwrap();
    apply(&mut mpe, &mut rt, bend(0, 12288)).unwrap();
    apply(&mut mpe, &mut rt, bend(1, 9216)).unwrap();
    apply_midi2(&mut mpe, &mut rt, 1, 0xd0, 0, 0, 0x1234_5678).unwrap();
    let wire1 = start(&mut mpe, &mut rt, 1, 60);
    let Applied::Started(wire2) = apply_midi2(&mut mpe, &mut rt, 1, 0x90, 60, 0, 0xffff_0000).unwrap() else { panic!() };
    let native = |id| sampler_core::Input { protocol: Protocol::Native, port: 7, group: 3,
        channel: 1, key: 60, external_id: Some(id) };
    // Independent serialized-value endpoint law: positive14-bit denominator
    //8191, positive32-bit denominator2^31-1; centre and negative endpoints exact.
    let manager = 2. * 4096. / 8191.; let member = 48. * 1024. / 8191.;
    let host_after_wire2 = manager + 48. * 536_870_912. / 2_147_483_647.;
    let wire2_after = 2. * 536_870_912. / 2_147_483_647.;
    let host = mpe.trigger_with_tuning(&mut rt, 1, native(101), 1., 0.25).unwrap();
    assert_eq!(expression(&rt, host).pitch_semitones, manager + member + 0.25);
    assert_eq!(expression(&rt, host).pressure, 0x1234_5678);
    assert_eq!(expression(&rt, wire1).pitch_semitones, manager + member);
    assert_eq!(expression(&rt, wire2).pitch_semitones, 0.);
    assert_eq!(expression(&rt, wire1).pressure, 0);
    support::without_heap(|| {
        apply_midi2(&mut mpe, &mut rt, 1, 0xe0, 0, 0, 0xa000_0000).unwrap();
        assert_eq!(expression(&rt, wire1).pitch_semitones, manager + member);
        assert_eq!(expression(&rt, wire2).pitch_semitones, wire2_after);
        assert_eq!(expression(&rt, host).pitch_semitones, host_after_wire2 + 0.25,
            "HostChannel retains its own48member range and priorMidi1managerbend");
        apply(&mut mpe, &mut rt, packet(0xd0, 1, 127, 0)).unwrap();
        assert_eq!(expression(&rt, host).pitch_semitones, host_after_wire2 + 0.25);
        assert_eq!(expression(&rt, host).pressure, u32::MAX);
        assert_eq!(expression(&rt, wire2).pressure, 0x1234_5678);
        let future = mpe.trigger(&mut rt, 1, native(102), 1.).unwrap();
        assert_eq!(expression(&rt, future).pitch_semitones, host_after_wire2);
        assert_eq!(expression(&rt, future).pressure, u32::MAX);
        mpe.set_note_tuning(&mut rt, host, 0.5).unwrap();
        let frozen_wire2 = expression(&rt, wire2);
        apply(&mut mpe, &mut rt, packet(0x80, 1, 60, 0)).unwrap();
        assert!(!rt.input_held(wire1).unwrap());
        assert!(rt.input_held(wire2).unwrap()); assert!(rt.input_held(host).unwrap());
        assert_eq!(expression(&rt, wire2), frozen_wire2);
        rt.note_off_exact(host, None).unwrap(); let released = expression(&rt, host);
        apply(&mut mpe, &mut rt, bend(1, 10240)).unwrap();
        assert_eq!(expression(&rt, host), released, "host member reuse must not change releasedidentity");
        assert_eq!(expression(&rt, wire2), frozen_wire2);
        apply_midi2(&mut mpe, &mut rt, 1, 0x80, 60, 0, 0).unwrap();
        assert!(!rt.input_held(wire2).unwrap()); assert!(rt.input_held(future).unwrap());
    });
}

#[test]
fn rejected_secondary_host_controller_admission_does_not_change_wire_or_future_host_gestures() {
    let mut rt = runtime_with_channels(4); let mut mpe = Mpe::new(&rt, 7, 3, Zone::Lower, 1, 8).unwrap();
    mpe.enable_host_channel_projection().unwrap(); mpe.use_midi2_channel_voice().unwrap(); mpe.set_timbre_controller(Some(11));
    let domain = rt.performance(0).unwrap();
    for port in 0..2 { rt.dispatch_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port,
        group: 0, channel: 0 }, u16::MAX, 1, 7).unwrap(); }
    rt.dispatch_controller(domain, ChannelAddress { protocol: Protocol::Midi1, port: 2, group: 0, channel: 0 }, (1 << 14) - 1, 1, 7).unwrap();
    let native = |id| sampler_core::Input { protocol: Protocol::Native, port: 7, group: 3,
        channel: 0, key: 60, external_id: Some(id) };
    let note = mpe.trigger(&mut rt, 0, native(101), 1.).unwrap(); let before = expression(&rt, note);
    support::without_heap(|| {
        //46existingkeys: wire2manager needs1, separateNativeHostzone2.
        // Eachscopefits2free rows alone, theiruniondoesnot.
        assert_eq!(apply_midi2(&mut mpe, &mut rt, 0, 0xb0, 74, 0, u32::MAX), Err(ApplyError::Core(Error::Capacity)));
        assert_eq!(expression(&rt, note), before);
        assert_eq!(rt.channel_input_controller(domain, native(101).channel_address(), 11), Ok(u32::MAX));
        assert_eq!(rt.channel_controller(domain, ChannelAddress { protocol: Protocol::Midi2, port: 7, group: 3, channel: 0 }, 11), Ok(u32::MAX));
        let future = mpe.trigger(&mut rt, 0, native(102), 1.).unwrap();
        assert_eq!(expression(&rt, future), before);
        assert_eq!(rt.now(), 0); assert_eq!(rt.voice_count(), 2);
    });
}

fn raw_fifo_rejection_runtime()->sampler_core::Runtime{
    let samples=(0..8192).map(|frame|{let x=frame as f64;[(0.07*x).sin()as f32*0.25,(0.031*x).cos()as f32*0.125]}).collect::<Vec<_>>();
    let plan=sampler_core::Prepared::new(48000,vec![sampler_core::Pcm::new(48000,samples.into_boxed_slice()).unwrap()],vec![sampler_core::Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:sampler_core::Envelope::new(0,0,0,1.,0).unwrap(),playback:sampler_core::Playback::default()}],1).unwrap();
    let mut limits=sampler_core::Limits::for_plan(&plan,2,4);limits.channels=48;limits.expressions=2;
    sampler_core::Runtime::new(plan,limits).unwrap().with_receiver_capacity(1).unwrap()
}
fn raw_fifo_packet(midi2:bool,on:bool)->[u32;2]{
    [(if midi2{0x4000_0000}else{0x2000_0000})|(if on{0x0091_0000}else{0x0081_0000})|0x3c00|if !midi2&&on{127}else{0},if midi2&&on{0xffff_0000}else{0}]
}
fn raw_fifo_source(midi2:bool,port:u16,group:u8)->sampler_core::Input{
    sampler_core::Input{protocol:if midi2{sampler_core::Protocol::Midi2}else{sampler_core::Protocol::Midi1},port,group,channel:1,key:60,external_id:None}
}
fn raw_fifo_filler(channel:u8)->sampler_core::Input{
    sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel,key:60,external_id:None}
}
fn raw_fifo_pcm(actual:&mut sampler_core::Runtime,reference:&mut sampler_core::Runtime,frames:usize){
    let mut a=[[0.;2];64];let mut b=a;let mut remaining=frames;let mut energy=0.;
    while remaining!=0{let n=remaining.min(64);actual.render(&mut a[..n]).unwrap();reference.render(&mut b[..n]).unwrap();for(frame,want)in a[..n].iter().zip(&b[..n]){for c in 0..2{assert_eq!(frame[c],want[c],"exact source PCM/phase diverged after a rejected physical attack terminal");energy+=f64::from(want[c]).abs();}}remaining-=n;}
    assert!(energy>0.,"reference source must be independently sounding");assert_eq!(actual.now(),reference.now());
}

fn raw_fifo_unbound(rt:&sampler_core::Runtime)->Mpe{let mut owner=Mpe::new(rt,0,0,Zone::Lower,15,8).unwrap();owner.use_midi2_channel_voice().unwrap();owner}
fn raw_fifo_unbound_apply(owner:&mut Mpe,rt:&mut Runtime,midi2:bool,on:bool,source:sampler_core::Input)->Result<Applied,ApplyError>{let words=raw_fifo_packet(midi2,on);owner.apply_with_source(rt,Packets::new(&words).next().unwrap().unwrap(),source)}
#[test]
fn raw_rejected_unbound_attack_cannot_release_newer_same_tuple_after_capacity_frees(){
    for midi2 in [false,true]{
        let mut rt=raw_fifo_rejection_runtime();let mut owner=raw_fifo_unbound(&rt);let mut reference=raw_fifo_rejection_runtime();let mut reference_owner=raw_fifo_unbound(&reference);let source=raw_fifo_source(midi2,7,3);
        let a=rt.note_on(raw_fifo_filler(14),60,1.).unwrap();let b=rt.note_on(raw_fifo_filler(15),60,1.).unwrap();let before=(rt.now(),rt.note_count(),rt.expression_count());
        support::without_heap(||{assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source),Err(ApplyError::Core(Error::Capacity)));});assert_eq!((rt.now(),rt.note_count(),rt.expression_count()),before);
        rt.cancel_input_note(a).unwrap();rt.cancel_input_note(b).unwrap();rt.flush_ended(|_|true);let Applied::Started(note)=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source).unwrap()else{panic!()};assert!(matches!(raw_fifo_unbound_apply(&mut reference_owner,&mut reference,midi2,true,source),Ok(Applied::Started(_))));let expression=rt.expression_id(note).unwrap();let value=rt.expression(expression).unwrap();assert_eq!(rt.input_source(note),Ok(Some(source)));
        support::without_heap(||{raw_fifo_pcm(&mut rt,&mut reference,64);let now=rt.now();let result=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,false,source).unwrap();assert!(rt.note(note).unwrap().2,"older refused unbound raw attack stole the newer accepted physical gate");assert_eq!(result,Applied::Ignored);assert_eq!(rt.now(),now);assert_eq!(rt.expression(expression),Ok(value));raw_fifo_pcm(&mut rt,&mut reference,128);});
        let Applied::Released{note:released,..}=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,false,source).unwrap()else{panic!()};assert_eq!(released,note);assert!(matches!(raw_fifo_unbound_apply(&mut reference_owner,&mut reference,midi2,false,source),Ok(Applied::Released{..})));
        rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.voice_count(),rt.expression_count()),(0,0,0));assert_eq!(rt.expression(expression),Err(Error::StaleHandle));assert_eq!(rt.stats().nonfinite_frames,0);let mut releases=0;owner.drain_terminal_releases(|_|releases+=1);assert_eq!(releases,0);
    }
}
#[test]
fn raw_accepted_rejected_accepted_unbound_sequence_retains_older_and_newer_owners(){
    for midi2 in [false,true]{
        let mut rt=raw_fifo_rejection_runtime();let mut owner=raw_fifo_unbound(&rt);let mut reference=raw_fifo_rejection_runtime();let mut reference_owner=raw_fifo_unbound(&reference);let source=raw_fifo_source(midi2,2,4);
        let Applied::Started(older)=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source).unwrap()else{panic!()};assert!(matches!(raw_fifo_unbound_apply(&mut reference_owner,&mut reference,midi2,true,source),Ok(Applied::Started(_))));let filler=rt.note_on(raw_fifo_filler(15),60,1.).unwrap();assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source),Err(ApplyError::Core(Error::Capacity)));assert!(rt.note(older).unwrap().2);rt.cancel_input_note(filler).unwrap();rt.flush_ended(|_|true);
        let Applied::Started(newer)=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source).unwrap()else{panic!()};assert_ne!(older,newer);assert!(matches!(raw_fifo_unbound_apply(&mut reference_owner,&mut reference,midi2,true,source),Ok(Applied::Started(_))));
        support::without_heap(||{raw_fifo_pcm(&mut rt,&mut reference,64);let Applied::Released{note,..}=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,false,source).unwrap()else{panic!()};assert_eq!(note,older);assert!(matches!(raw_fifo_unbound_apply(&mut reference_owner,&mut reference,midi2,false,source),Ok(Applied::Released{..})));assert!(rt.note(newer).unwrap().2);raw_fifo_pcm(&mut rt,&mut reference,64);
            let result=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,false,source).unwrap();assert!(rt.note(newer).unwrap().2,"rejected middle unbound attack stole the newer accepted physical gate");assert_eq!(result,Applied::Ignored);raw_fifo_pcm(&mut rt,&mut reference,128);
        });
        let Applied::Released{note,..}=raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,false,source).unwrap()else{panic!()};assert_eq!(note,newer);assert!(matches!(raw_fifo_unbound_apply(&mut reference_owner,&mut reference,midi2,false,source),Ok(Applied::Released{..})));
        rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.voice_count(),rt.expression_count()),(0,0,0));assert_eq!(rt.stats().nonfinite_frames,0);let mut releases=0;owner.drain_terminal_releases(|_|releases+=1);assert_eq!(releases,0);
    }
}


#[test]
fn raw_unbound_full_decision_roster_stays_fenced_after_known_physical_acks_and_panic(){
    for midi2 in [false,true]{
        let mut rt=raw_fifo_rejection_runtime();let mut owner=Mpe::new(&rt,0,0,Zone::Lower,15,1).unwrap();owner.use_midi2_channel_voice().unwrap();let source=raw_fifo_source(midi2,7,3);
        let a=rt.note_on(raw_fifo_filler(14),60,1.).unwrap();let b=rt.note_on(raw_fifo_filler(15),60,1.).unwrap();
        for _ in 0..2{assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source),Err(ApplyError::Core(Error::Capacity)));assert!(!owner.wire_fenced());}
        assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source),Err(ApplyError::Core(Error::Capacity)));assert!(owner.wire_fenced());
        rt.cancel_input_note(a).unwrap();rt.cancel_input_note(b).unwrap();rt.flush_ended(|_|true);
        support::without_heap(||{
            for _ in 0..2{assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,false,source),Ok(Applied::Ignored));}
            rt.panic();assert!(owner.wire_fenced());
            assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,source),Err(ApplyError::Core(Error::Capacity)));
            assert_eq!((rt.note_count(),rt.voice_count(),rt.expression_count()),(0,0,0));
        });
        assert_eq!(rt.stats().nonfinite_frames,0);
    }
}


#[test]
fn raw_synchronous_unbound_malformed_sources_do_not_create_bind_blocking_debt() {
    for midi2 in [false,true] {
        let mut rt=raw_fifo_rejection_runtime();let mut owner=raw_fifo_unbound(&rt);let source=raw_fifo_source(midi2,7,3);let logical=sampler_core::Input{port:0,group:0,channel:1,key:60,..source};let before=(rt.now(),rt.note_count(),rt.voice_count(),rt.expression_count());
        support::without_heap(||{for bad in [sampler_core::Input{group:16,..source},sampler_core::Input{channel:16,..source},sampler_core::Input{key:128,..source}] {
            assert_eq!(raw_fifo_unbound_apply(&mut owner,&mut rt,midi2,true,bad),Err(ApplyError::Core(Error::InvalidInput)));
            assert_eq!(owner.trigger_with_source(&mut rt,1,logical,bad,0.5,0.),Err(ApplyError::Core(Error::InvalidInput)));
            assert_eq!((rt.now(),rt.note_count(),rt.voice_count(),rt.expression_count()),before);assert!(!owner.wire_fenced());
        }});
        // A genuine consuming bind proves the unbound owner has no hidden
        // Pending/Rejected/physical/binding/receipt state after all refusals.
        let bound=owner.bind(&mut rt).unwrap();let status=bound.wire_status(&rt).unwrap();assert_eq!((status.decisions,status.future_receipts,status.queued_receipts,status.active_receipts),(0,0,0,0));assert!(!status.fenced_unknown_debt);
        let valid=sampler_core::Input{group:15,channel:15,key:127,..source};let words=raw_fifo_packet(midi2,true);let Applied::Started(note)=bound.apply_with_source(&mut rt,Packets::new(&words).next().unwrap().unwrap(),valid).unwrap()else{panic!()};assert_eq!(rt.input_source(note),Ok(Some(valid)));
        let words=raw_fifo_packet(midi2,false);assert!(matches!(bound.apply_with_source(&mut rt,Packets::new(&words).next().unwrap().unwrap(),valid),Ok(Applied::Released{note:released,..})if released==note));rt.render(&mut [[0.;2];64]).unwrap();rt.flush_ended(|_|true);assert_eq!((rt.note_count(),rt.expression_count(),rt.voice_count()),(0,0,0));assert_eq!(rt.stats().nonfinite_frames,0);
    }
}

#[test]
fn explicit_host_ids_on_midi_protocol_inputs_keep_direct_trigger_and_exact_wire_isolation(){
    for protocol in [Protocol::Midi1,Protocol::Midi2] {
        let mut rt=runtime_with_channels(16);let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,2,4).unwrap();
        let first=sampler_core::Input{protocol,port:7,group:3,channel:1,key:60,external_id:Some(9101)};
        let second=sampler_core::Input{external_id:Some(9102),..first};
        support::without_heap(||{
            let a=mpe.trigger_with_tuning(&mut rt,1,first,1.,0.25).unwrap();
            let b=mpe.trigger_with_tuning(&mut rt,1,second,1.,-0.5).unwrap();assert_ne!(a,b);
            let raw=if protocol==Protocol::Midi1 {start(&mut mpe,&mut rt,1,60)}else{
                let Applied::Started(note)=apply_midi2(&mut mpe,&mut rt,1,0x90,60,0,0xffff_0000).unwrap()else{panic!("raw MIDI2 not admitted")};note
            };
            let expected=if protocol==Protocol::Midi1 {
                apply(&mut mpe,&mut rt,bend(1,12288)).unwrap();pitch(12288,48.)
            }else{
                apply_midi2(&mut mpe,&mut rt,1,0xe0,0,0,0xa000_0000).unwrap();48.*536_870_912./2_147_483_647.
            };
            assert_eq!(expression(&rt,a).pitch_semitones,expected+0.25);
            assert_eq!(expression(&rt,b).pitch_semitones,expected-0.5);
            assert_eq!(expression(&rt,raw).pitch_semitones,expected);
            let release=if protocol==Protocol::Midi1 {apply(&mut mpe,&mut rt,packet(0x80,1,60,0)).unwrap()}
                else{apply_midi2(&mut mpe,&mut rt,1,0x80,60,0,0).unwrap()};
            assert!(matches!(release,Applied::Released{note,..} if note==raw));
            assert!(rt.input_held(a).unwrap()&&rt.input_held(b).unwrap());assert!(!rt.input_held(raw).unwrap());
            rt.note_off(first,None).unwrap();assert!(!rt.input_held(a).unwrap());assert!(rt.input_held(b).unwrap());
            rt.note_off(second,None).unwrap();assert!(!rt.input_held(b).unwrap());
            // A packet-origin source remains anonymous even when a direct host
            // trigger legitimately uses this protocol with a real external ID.
            let words=if protocol==Protocol::Midi1 {[packet(0x90,1,61,127),0]}else{[0x4391_3d00,0xffff_0000]};
            let packet=Packets::new(if protocol==Protocol::Midi1{&words[..1]}else{&words[..]}).next().unwrap().unwrap();
            let before=(rt.now(),rt.note_count(),rt.expression_count(),rt.voice_count());
            assert_eq!(mpe.apply_with_source(&mut rt,packet,first),Err(ApplyError::Core(Error::InvalidInput)));
            assert_eq!((rt.now(),rt.note_count(),rt.expression_count(),rt.voice_count()),before);
        });
    }
}

#[test]
fn explicit_logical_midi_trigger_preserves_different_physical_host_source(){
    for protocol in [Protocol::Midi1,Protocol::Midi2] {
        let mut rt=runtime_with_channels(16);let mut mpe=Mpe::new(&rt,7,3,Zone::Lower,2,4).unwrap();
        let logical=sampler_core::Input{protocol,port:7,group:3,channel:1,key:60,external_id:None};
        let source=sampler_core::Input{protocol:Protocol::Clap,port:12,group:4,channel:9,key:61,external_id:Some(9201)};
        support::without_heap(||{
            let note=mpe.trigger_with_source(&mut rt,1,logical,source,0.75,0.375).unwrap();
            assert_eq!(rt.input_source(note),Ok(Some(source)));assert_eq!(rt.note(note).unwrap().0,logical.key);
            assert_eq!(rt.note(note).unwrap().1,0.75);
            let expected=if protocol==Protocol::Midi1 {apply(&mut mpe,&mut rt,bend(1,12288)).unwrap();pitch(12288,48.)}
                else{apply_midi2(&mut mpe,&mut rt,1,0xe0,0,0,0xa000_0000).unwrap();48.*536_870_912./2_147_483_647.};
            assert_eq!(expression(&rt,note).pitch_semitones,expected+0.375);
            // Exact logical Core terminal remains the existing API; the physical
            // source is metadata, not reclassified as a Native ledger admission.
            rt.note_off(logical,None).unwrap();assert!(!rt.input_held(note).unwrap());
            assert_eq!(rt.input_source(note),Ok(Some(source)));
            rt.panic();let mut ended=0;rt.flush_ended(|input|{assert_eq!(input,logical);ended+=1;true});assert_eq!(ended,1);
        });
    }
}
