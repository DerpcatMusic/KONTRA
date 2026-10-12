use sampler_midi::{
    Attribute, ChannelVoice, Controllers, Message, Packets, Truncated, Value, Version,
};

fn decode(words: &[u32]) -> ChannelVoice {
    let mut packets = Packets::new(words);
    let voice = packets.next().unwrap().unwrap().channel_voice().unwrap();
    assert!(packets.next().is_none());
    voice
}

#[test]
fn all_message_types_frame_without_guessing_or_resynchronizing() {
    let lengths = [1, 1, 1, 2, 2, 4, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4];
    for (mt, &length) in lengths.iter().enumerate() {
        let words = [(mt as u32) << 28, 0x4090_3c00, 0xffff_ffff, 0];
        let mut packets = Packets::new(&words[..length]);
        let packet = packets.next().unwrap().unwrap();
        assert_eq!(packet.message_type(), mt as u8);
        assert_eq!(packet.words(), &words[..length]);
        assert!(packets.next().is_none());
        for available in 1..length {
            let mut short = Packets::new(&words[..available]);
            assert_eq!(
                short.next(),
                Some(Err(Truncated {
                    expected_words: length,
                    available_words: available
                }))
            );
            assert!(short.next().is_none());
        }
    }
    assert!(Packets::new(&[]).next().is_none());
    let mixed = [
        0x0000_0000,
        0xb000_0000,
        0x2090_3c7f,
        0x4090_3c00,
        0x4090_3d00,
        0x1234_0000,
    ];
    let sizes: Vec<_> = Packets::new(&mixed)
        .map(|p| p.unwrap().words().len())
        .collect();
    assert_eq!(sizes, [1, 3, 2]); // channel-looking payload never starts a new packet
}

#[test]
fn midi2_wire_vectors_preserve_resolution_and_controller_identity() {
    let attribute = Attribute {
        kind: 3,
        data: 0x789a,
    };
    let cases = [
        (
            0x439a_3c03,
            0x1234_789a,
            Message::NoteOn {
                key: 60,
                velocity: Value::Bits16(0x1234),
                attribute,
            },
        ),
        (
            0x438a_3c03,
            0xabcd_789a,
            Message::NoteOff {
                key: 60,
                velocity: Some(Value::Bits16(0xabcd)),
                attribute,
            },
        ),
        (
            0x43aa_3c00,
            0xfedc_ba98,
            Message::PolyPressure {
                key: 60,
                value: Value::Bits32(0xfedc_ba98),
            },
        ),
        (
            0x43ba_4a00,
            0x0000_0001,
            Message::Control {
                index: 74,
                value: Value::Bits32(1),
            },
        ),
        (
            0x43da_0000,
            0x8000_0001,
            Message::ChannelPressure(Value::Bits32(0x8000_0001)),
        ),
        (
            0x43ea_0000,
            0x8000_0000,
            Message::PitchBend(Value::Bits32(0x8000_0000)),
        ),
        (
            0x436a_3c00,
            0xffff_ffff,
            Message::PerNotePitch {
                key: 60,
                value: u32::MAX,
            },
        ),
        (
            0x430a_3cff,
            0x1234_5678,
            Message::PerNoteControl {
                space: Controllers::Registered,
                key: 60,
                index: 255,
                value: 0x1234_5678,
            },
        ),
        (
            0x431a_3c80,
            0x8765_4321,
            Message::PerNoteControl {
                space: Controllers::Assignable,
                key: 60,
                index: 128,
                value: 0x8765_4321,
            },
        ),
        (
            0x432a_027f,
            0x1234_5678,
            Message::ChannelControl {
                space: Controllers::Registered,
                bank: 2,
                index: 127,
                value: 0x1234_5678,
            },
        ),
        (
            0x433a_037e,
            0x8765_4321,
            Message::ChannelControl {
                space: Controllers::Assignable,
                bank: 3,
                index: 126,
                value: 0x8765_4321,
            },
        ),
        (
            0x434a_0007,
            0xffff_ffff,
            Message::RelativeControl {
                space: Controllers::Registered,
                bank: 0,
                index: 7,
                delta: -1,
            },
        ),
        (
            0x435a_7f7f,
            0x8000_0000,
            Message::RelativeControl {
                space: Controllers::Assignable,
                bank: 127,
                index: 127,
                delta: i32::MIN,
            },
        ),
        (
            0x43ca_0001,
            0x4500_1234,
            Message::Program {
                program: 69,
                bank: Some([18, 52]),
            },
        ),
        (
            0x43ca_0000,
            0x4500_1234,
            Message::Program {
                program: 69,
                bank: None,
            },
        ),
        (
            0x43fa_3c03,
            0xffff_ffff,
            Message::PerNoteManagement {
                key: 60,
                detach: true,
                reset: true,
            },
        ),
    ];
    for (first, second, message) in cases {
        assert_eq!(
            decode(&[first, second]),
            ChannelVoice {
                version: Version::Midi2,
                group: 3,
                channel: 10,
                message
            }
        );
    }
    assert!(
        Packets::new(&[0x4370_0000, 0])
            .next()
            .unwrap()
            .unwrap()
            .channel_voice()
            .is_none()
    );
}

#[test]
fn midi1_zero_note_on_and_midi2_zero_note_on_remain_distinct() {
    assert_eq!(
        decode(&[0x2090_3c00]).message,
        Message::NoteOff {
            key: 60,
            velocity: None,
            attribute: Attribute::default()
        }
    );
    assert_eq!(
        decode(&[0x4090_3c00, 0]).message,
        Message::NoteOn {
            key: 60,
            velocity: Value::Bits16(0),
            attribute: Attribute::default()
        }
    );
    assert_eq!(
        decode(&[0x2080_3c7f]).message,
        Message::NoteOff {
            key: 60,
            velocity: Some(Value::Bits7(127)),
            attribute: Attribute::default()
        }
    );
    assert_eq!(
        decode(&[0x20e0_7f7f]).message,
        Message::PitchBend(Value::Bits14(16383))
    );
    assert_eq!(
        decode(&[0x20e0_0040]).message,
        Message::PitchBend(Value::Bits14(8192))
    );
    assert_eq!(Value::Bits16(65535).normalized(), 1.0);
    assert_eq!(Value::Bits32(u32::MAX).normalized(), 1.0);
    assert!(Value::Bits32(0x8000_0001).normalized() > Value::Bits32(0x8000_0000).normalized());
}

#[test]
fn addresses_and_reserved_fields_do_not_discard_valid_messages() {
    for group in 0..16 {
        for channel in 0..16 {
            let first = 0x40a0_bcfe | (group << 24) | (channel << 16);
            let event = decode(&[first, 0x1234_5678]);
            assert_eq!((event.group, event.channel), (group as u8, channel as u8));
            assert_eq!(
                event.message,
                Message::PolyPressure {
                    key: 60,
                    value: Value::Bits32(0x1234_5678)
                }
            );
        }
    }
    assert_eq!(
        decode(&[0x40cf_fffd, 0xc5ff_92b4]).message,
        Message::Program {
            program: 69,
            bank: Some([18, 52])
        }
    );
    let mut seed = 42u32;
    for _ in 0..10_000 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let words = [seed, !seed, seed.rotate_left(7), seed.rotate_right(3)];
        let mut consumed = 0;
        for result in Packets::new(&words) {
            match result {
                Ok(packet) => {
                    consumed += packet.words().len();
                    if let Some(v) = packet.channel_voice() {
                        assert!(v.group < 16 && v.channel < 16);
                    }
                }
                Err(error) => consumed += error.available_words,
            }
        }
        assert_eq!(consumed, words.len());
    }
}
