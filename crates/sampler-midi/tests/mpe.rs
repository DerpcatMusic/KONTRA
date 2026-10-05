use sampler_core::{
    ChannelAddress, Envelope, Error, Event, Expression, Limits, NoteId, Pcm, Playback, Prepared,
    Protocol, Region, Runtime,
};
use sampler_midi::{Applied, ApplyError, Mpe, Packets, Zone};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime() -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![Pcm {
                rate: 48000,
                frames: (0..8192)
                    .map(|i| {
                        let phase = f64::from(i) * std::f64::consts::TAU * 0.017;
                        [phase.cos() as f32, phase.sin() as f32]
                    })
                    .collect(),
            }],
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
        .unwrap(),
        Limits {
            notes: 8,
            channels: 4,
            families: 8,
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
            rt.schedule_event(8, Event::KeyUp(overlapping)).unwrap();
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
