//! MIDI 2.0 channel voice packets (UMP message type 4) as the events the
//! core already plays: notes and per-note pitch as host notes and
//! expressions, per-note controllers as [`Event::PerNoteCc`], and channel
//! controllers, pressure and bend as MIDI 1.0 packets at the engine's
//! resolution (UMP spec M2-104-UM, 7.4).

use crate::{Event, HostNote, HostPattern, NoteExpression};

/// Semitones a full per-note pitch bend reaches: the MPE default, as the
/// UMP spec sets none until per-note bend sensitivity (RPN 7) arrives.
pub const PER_NOTE_BEND_SEMITONES: f64 = 48.0;

/// Note-on attribute type 3: pitch 7.9, a key and 512ths of a semitone.
const ATTRIBUTE_PITCH: u8 = 3;

impl Event {
    /// A MIDI 2.0 channel voice packet as a core event; None for any other
    /// packet and for those the core has no use for (program, RPN/NRPN,
    /// per-note management). The group is the port.
    #[must_use]
    pub fn midi2([word, data]: [u32; 2]) -> Option<Self> {
        if word >> 28 != 0x4 {
            return None;
        }
        let [_, group_status, byte2, byte3] = word.to_be_bytes();
        let (status, channel) = (group_status >> 4, group_status & 15);
        let port = (word >> 24) as u8 & 15;
        let (key, unit) = (byte2 & 127, data as f32 / u32::MAX as f32);
        let pattern = HostPattern {
            port: i32::from(port),
            channel: i32::from(channel),
            key: i32::from(key),
            id: -1,
            clap: false,
        };
        let velocity = f64::from((data >> 16) as u16) / f64::from(u16::MAX);
        let midi1 =
            |status: u8, data1: u8, data2: u8| match Self::midi1(status | channel, data1, data2) {
                Self::Ump([w, extra]) => Self::Ump([w | u32::from(port) << 24, extra]),
                other => other,
            };
        // 32-bit values down to MIDI 1.0's 7 bits (and 14 for bend).
        let seven = (data >> 25) as u8;
        Some(match status {
            0x0 | 0x1 => Self::PerNoteCc {
                pattern,
                registered: status == 0x0,
                index: byte3,
                value: unit,
            },
            0x6 => Self::Expression(
                pattern,
                NoteExpression::Tune(
                    (f64::from(data) - f64::from(crate::RECEIVER_PER_NOTE_CENTER))
                        / f64::from(crate::RECEIVER_PER_NOTE_CENTER)
                        * PER_NOTE_BEND_SEMITONES,
                ),
            ),
            0x8 => Self::NoteOffVelocity(pattern, velocity),
            0x9 => Self::NoteOn {
                note: HostNote {
                    port,
                    channel,
                    key,
                    id: -1,
                    clap: false,
                },
                velocity,
                tune: if byte3 == ATTRIBUTE_PITCH {
                    f64::from(data as u16) / 512.0 - f64::from(key)
                } else {
                    0.0
                },
            },
            0xA => midi1(0xA0, key, seven),
            0xB => midi1(0xB0, byte2, seven),
            0xD => midi1(0xD0, seven, 0),
            0xE => {
                let bend = data >> 18;
                midi1(0xE0, (bend & 127) as u8, (bend >> 7) as u8)
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A type-4 packet on group 2, channel 5.
    fn packet(status: u8, byte2: u8, byte3: u8, data: u32) -> [u32; 2] {
        let word = 0x4200_0000
            | u32::from(status) << 20
            | 5 << 16
            | u32::from(byte2) << 8
            | u32::from(byte3);
        [word, data]
    }

    fn at(key: i32) -> HostPattern {
        HostPattern {
            port: 2,
            channel: 5,
            key,
            id: -1,
            clap: false,
        }
    }

    fn midi1_on_port(status: u8, data1: u8, data2: u8) -> Event {
        match Event::midi1(status, data1, data2) {
            Event::Ump([w, extra]) => Event::Ump([w | 2 << 24, extra]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn per_note_controllers_keep_their_key_index_and_full_value() {
        for (status, registered) in [(0x0, true), (0x1, false)] {
            let event = Event::midi2(packet(status, 64, 7, u32::MAX)).unwrap();
            assert_eq!(
                event,
                Event::PerNoteCc {
                    pattern: at(64),
                    registered,
                    index: 7,
                    value: 1.0,
                }
            );
            assert_eq!((event.port(), event.channel()), (Some(2), Some(5)));
        }
        let Some(Event::PerNoteCc { value, .. }) = Event::midi2(packet(0x1, 64, 7, 0)) else {
            panic!()
        };
        assert_eq!(value, 0.0);
    }

    #[test]
    fn per_note_pitch_bend_is_a_tune_about_the_centre() {
        let tune = |data| match Event::midi2(packet(0x6, 60, 0, data)) {
            Some(Event::Expression(p, NoteExpression::Tune(t))) if p == at(60) => t,
            other => panic!("{other:?}"),
        };
        assert_eq!(tune(0x8000_0000), 0.0);
        assert_eq!(tune(0), -PER_NOTE_BEND_SEMITONES);
        assert!((tune(u32::MAX) - PER_NOTE_BEND_SEMITONES).abs() < 1e-6);
    }

    #[test]
    fn notes_take_16_bit_velocities_and_the_pitch_attribute() {
        let on = Event::midi2(packet(
            0x9,
            60,
            ATTRIBUTE_PITCH,
            0xFFFF_0000 | 61 << 9 | 256,
        ));
        let Some(Event::NoteOn {
            note,
            velocity,
            tune,
        }) = on
        else {
            panic!("{on:?}")
        };
        assert_eq!((note.port, note.channel, note.key, note.id), (2, 5, 60, -1));
        assert_eq!((velocity, tune), (1.0, 1.5));
        // Another attribute leaves the key's own pitch.
        let Some(Event::NoteOn { tune, velocity, .. }) =
            Event::midi2(packet(0x9, 60, 1, 0x8000_1234))
        else {
            panic!()
        };
        assert_eq!(tune, 0.0);
        assert!((velocity - 0.5).abs() < 1e-4);
        assert_eq!(
            Event::midi2(packet(0x8, 60, 0, 0xFFFF_0000)),
            Some(Event::NoteOffVelocity(at(60), 1.0))
        );
    }

    #[test]
    fn channel_messages_arrive_as_midi1_packets_on_their_port() {
        assert_eq!(
            Event::midi2(packet(0xB, 74, 0, u32::MAX)),
            Some(midi1_on_port(0xB5, 74, 127))
        );
        assert_eq!(
            Event::midi2(packet(0xA, 60, 0, 0x8000_0000)),
            Some(midi1_on_port(0xA5, 60, 64))
        );
        assert_eq!(
            Event::midi2(packet(0xD, 0, 0, u32::MAX)),
            Some(midi1_on_port(0xD5, 127, 0))
        );
        // 0x8000_0000 is the 14-bit 8192 centre: LSB 0, MSB 64.
        assert_eq!(
            Event::midi2(packet(0xE, 0, 0, 0x8000_0000)),
            Some(midi1_on_port(0xE5, 0, 64))
        );
        assert_eq!(
            Event::midi2(packet(0xE, 0, 0, u32::MAX)),
            Some(midi1_on_port(0xE5, 127, 127))
        );
    }

    #[test]
    fn other_packets_are_left_alone() {
        // Program change, RPN, per-note management, and a MIDI 1.0 packet.
        for status in [0x2, 0x3, 0x4, 0x5, 0xC, 0xF] {
            assert_eq!(Event::midi2(packet(status, 1, 2, 3)), None);
        }
        match Event::midi1(0x90, 60, 100) {
            Event::Ump(words) => assert_eq!(Event::midi2(words), None),
            other => panic!("{other:?}"),
        }
    }
}
