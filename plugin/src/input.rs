//! Host events as engine events: notes become [`Event::NoteOn`] and
//! [`Event::NoteOff`] on their port and channel, everything else on a MIDI
//! channel travels as its UMP packet. Host-native notes ([`exact`]) keep
//! their note id and per-note expressions (CLAP, VST3, MPE).

use kontra_host::rack::{Event, HostNote, HostPattern, NoteExpression};
use moose::core::ump::{encode_ump_channel_voice_1, encode_ump_channel_voice_2};
use moose::core::{ExactAddress, ExactEventBody, ExactEventRef, ExactNoteAddress, ExactNoteKind};
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

/// A host-native note or expression for the engine; None leaves `e` to its
/// typed fallback ([`event`]), and so does an address out of range.
pub fn exact(e: ExactEventRef<'_>) -> Option<Event> {
    let unit = |v: f64| (0.0..=1.0).contains(&v).then_some(v);
    let expression = |address, clap, x: Option<NoteExpression>| {
        Some(Event::Expression(pattern(address, clap)?, x?))
    };
    match *e.body() {
        ExactEventBody::Note {
            kind,
            address,
            velocity,
        } => note(kind, address, velocity, true, 0.0),
        // VST3 tuning is in cents; the length is metadata (a NoteOff ends it).
        ExactEventBody::DetailedNote {
            kind,
            address,
            velocity,
            tuning,
            ..
        } => note(
            kind,
            address,
            f64::from(velocity),
            false,
            f64::from(tuning) / 100.0,
        ),
        // CLAP: 0 volume (linear 0..=4), 1 pan (0..=1), 2 tuning (semitones),
        // 5 brightness, 6 pressure.
        ExactEventBody::NoteExpression {
            expression_id,
            address,
            value,
        } => expression(
            address,
            true,
            match expression_id {
                0 if (0.0..=4.0).contains(&value) => Some(NoteExpression::Gain(value)),
                1 => unit(value).map(|v| NoteExpression::Pan(v * 2.0 - 1.0)),
                2 if (-120.0..=120.0).contains(&value) => Some(NoteExpression::Tune(value)),
                5 => unit(value).map(NoteExpression::Brightness),
                6 => unit(value).map(NoteExpression::Pressure),
                _ => None,
            },
        ),
        // VST3, normalized: 0 volume (0.25 unity, 1 is +12 dB), 1 pan,
        // 2 tuning (0.5 centre, ±120 semitones), 5 brightness.
        ExactEventBody::NormalizedNoteExpression {
            expression_id,
            address,
            value,
        } => expression(
            address,
            false,
            unit(value).and_then(|v| match expression_id {
                0 => Some(NoteExpression::Gain(v * 4.0)),
                1 => Some(NoteExpression::Pan(v * 2.0 - 1.0)),
                2 => Some(NoteExpression::Tune((v - 0.5) * 240.0)),
                5 => Some(NoteExpression::Brightness(v)),
                _ => None,
            }),
        ),
        // Anonymous VST3 poly pressure has a per-key typed fallback.
        ExactEventBody::DetailedPolyPressure { .. } if e.fallback().is_some() => None,
        ExactEventBody::DetailedPolyPressure { address, pressure } => expression(
            address,
            false,
            unit(f64::from(pressure)).map(NoteExpression::Pressure),
        ),
        _ => None,
    }
}

fn note(
    kind: ExactNoteKind,
    address: ExactNoteAddress,
    velocity: f64,
    clap: bool,
    tune: f64,
) -> Option<Event> {
    let pattern = pattern(address, clap)?;
    if !(0.0..=1.0).contains(&velocity) {
        return None;
    }
    Some(match kind {
        ExactNoteKind::On => {
            let (Ok(port), Ok(channel @ 0..16), Ok(key @ 0..128)) = (
                u8::try_from(pattern.port),
                u8::try_from(pattern.channel),
                u8::try_from(pattern.key),
            ) else {
                return None;
            };
            Event::NoteOn {
                note: HostNote {
                    port,
                    channel,
                    key,
                    id: pattern.id,
                    clap,
                },
                velocity,
                tune: if tune.is_finite() { tune } else { 0.0 },
            }
        }
        ExactNoteKind::Off => Event::NoteOffVelocity(pattern, velocity),
        ExactNoteKind::Choke => Event::Choke(pattern),
        _ => return None,
    })
}

/// The note `address` matches; -1 is a wildcard.
fn pattern(address: ExactNoteAddress, clap: bool) -> Option<HostPattern> {
    Some(HostPattern {
        port: raw(address.port)?,
        channel: raw(address.channel)?,
        key: raw(address.key)?,
        id: raw(address.note_id)?,
        clap,
    })
}

fn raw<T: Copy + Into<i32>>(a: ExactAddress<T>) -> Option<i32> {
    match a {
        ExactAddress::InvalidRaw(_) => None,
        a => Some(a.raw_i32()),
    }
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

    #[test]
    fn native_notes_keep_their_id_and_expressions() {
        use moose::core::{EventList, ExactEvent, LosslessEventRef};
        let decode = |body| {
            let mut events = EventList::with_capacity(1);
            events.try_push_exact(ExactEvent::new(0, body)).unwrap();
            let Some(LosslessEventRef::Exact(e)) = events.lossless_iter().next() else {
                panic!("no exact event");
            };
            exact(e)
        };
        let address = ExactNoteAddress::from_raw_signed(0, 2, 60, 117);
        let on = decode(ExactEventBody::Note {
            kind: ExactNoteKind::On,
            address,
            velocity: 0.75,
        });
        let Some(Event::NoteOn { note, velocity, .. }) = on else {
            panic!("{on:?}");
        };
        assert_eq!(
            (note.channel, note.key, note.id, velocity),
            (2, 60, 117, 0.75)
        );
        let tune = decode(ExactEventBody::NoteExpression {
            expression_id: 2,
            address,
            value: -1.5,
        });
        assert!(
            matches!(tune, Some(Event::Expression(p, NoteExpression::Tune(t))) if p.id == 117 && t == -1.5),
            "{tune:?}"
        );
        let vst3 = decode(ExactEventBody::DetailedNote {
            kind: ExactNoteKind::On,
            address,
            velocity: 0.5,
            tuning: 50.0,
            length: None,
        });
        assert!(
            matches!(vst3, Some(Event::NoteOn { tune, .. }) if tune == 0.5),
            "{vst3:?}"
        );
        let off = decode(ExactEventBody::Note {
            kind: ExactNoteKind::Off,
            address,
            velocity: 0.25,
        });
        assert!(matches!(off, Some(Event::NoteOffVelocity(p, v)) if p.id == 117 && v == 0.25));
    }
}
