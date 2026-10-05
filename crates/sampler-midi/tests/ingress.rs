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
