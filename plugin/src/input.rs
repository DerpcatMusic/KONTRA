//! Host events as engine events: notes become [`Event::NoteOn`] and
//! [`Event::NoteOff`] on their port and channel, everything else on a MIDI
//! channel travels as its UMP packet.

use kontra_host::rack::{Event, HostNote, HostPattern};
use moose::core::ump::{encode_ump_channel_voice_1, encode_ump_channel_voice_2};
use moose::prelude::EventBody;

/// `e` for the engine; None for what is not addressed to it (parameters,
/// transport, SysEx).
pub fn event(e: &moose::prelude::Event) -> Option<Event> {
    let port = e.port;
    let on = |channel: u8, key: u8, velocity: f64| Event::NoteOn {
        note: HostNote {
            port,
            channel,
            key,
            id: -1,
            clap: false,
        },
        velocity,
        tune: 0.0,
    };
    let off = |channel: u8, key: u8| {
        Event::NoteOff(HostPattern {
            port: i32::from(port),
            channel: i32::from(channel),
            key: i32::from(key),
            id: -1,
            clap: false,
        })
    };
    Some(match e.body {
        // A MIDI 1.0 note-on at velocity 0 is a note-off.
        EventBody::NoteOn {
            channel,
            note,
            velocity: 0,
            ..
        }
        | EventBody::NoteOff { channel, note, .. }
        | EventBody::NoteOff2 { channel, note, .. } => off(channel, note),
        EventBody::NoteOn {
            channel,
            note,
            velocity,
            ..
        } => on(channel, note, f64::from(velocity) / 127.0),
        EventBody::NoteOn2 {
            channel,
            note,
            velocity,
            ..
        } => on(channel, note, f64::from(velocity) / 65535.0),
        _ => {
            let [a, b, ..] = encode_ump_channel_voice_1(&e.body)
                .or_else(|| encode_ump_channel_voice_2(&e.body))?;
            Event::Ump([a, b])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use moose::prelude::Event as HostEvent;

    #[test]
    fn notes_become_notes_and_controllers_packets() {
        let note = |velocity| {
            event(&HostEvent::new(
                0,
                EventBody::NoteOn {
                    group: 0,
                    channel: 3,
                    note: 60,
                    velocity,
                },
            ))
        };
        let Some(Event::NoteOn {
            note: n, velocity, ..
        }) = note(127)
        else {
            panic!("{:?}", note(127));
        };
        assert_eq!((n.channel, n.key, velocity), (3, 60, 1.0));
        assert!(matches!(note(0), Some(Event::NoteOff(p)) if p.key == 60 && p.channel == 3));
        let cc = event(&HostEvent::new(
            0,
            EventBody::ControlChange {
                group: 0,
                channel: 1,
                cc: 64,
                value: 127,
            },
        ));
        let Some(e @ Event::Ump(_)) = cc else {
            panic!("{cc:?}");
        };
        assert_eq!(e.channel(), Some(1));
        let param = HostEvent::new(0, EventBody::ParamChange { id: 0, value: 1.0 });
        assert!(event(&param).is_none());
    }
}
