use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
use sampler_midi::{Applied, ApplyError, Ingress, Packets, Version};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime() -> Runtime {
    Runtime::new(
        Prepared::new(
            48000,
            vec![Pcm {
                rate: 48000,
                frames: vec![[1.; 2]; 32].into_boxed_slice(),
            }],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
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
