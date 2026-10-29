//! Encode the exact inspector input clock as a type-0 MIDI file for the oracle.
use anyhow::{Result, bail, ensure};
use kontra_engine::Event;

use crate::scenario::Input;

const MAX_DELTA: u64 = 0x0fff_ffff;

fn vlq(mut n: u64, out: &mut Vec<u8>) {
    let mut bytes = [0u8; 4];
    let mut at = bytes.len();
    at -= 1;
    bytes[at] = (n & 0x7f) as u8;
    n >>= 7;
    while n != 0 {
        at -= 1;
        bytes[at] = ((n & 0x7f) as u8) | 0x80;
        n >>= 7;
    }
    if let Some(slice) = bytes.get(at..) {
        out.extend_from_slice(slice);
    }
}

pub fn write(events: &[Input]) -> Result<Vec<u8>> {
    // 24,000 ticks/quarter at 500,000 µs/quarter = 48,000 ticks/second.
    let mut track = vec![0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20];
    let mut last = 0u64;
    for input in events {
        ensure!(input.frame >= last, "scenario events are not ordered");
        let mut delta = input.frame - last;
        while delta > MAX_DELTA {
            vlq(MAX_DELTA, &mut track);
            track.extend_from_slice(&[0xff, 1, 0]);
            delta -= MAX_DELTA;
        }
        vlq(delta, &mut track);
        match input.event {
            Event::NoteOn { note, velocity, .. } => {
                ensure!(
                    note.channel == 0 && note.port == 0,
                    "oracle only supports channel 1"
                );
                let velocity = (velocity * 127.0).round().clamp(0.0, 127.0) as u8;
                track.extend_from_slice(&[0x90, note.key, velocity]);
            }
            Event::NoteOff(note) => {
                ensure!(
                    note.channel == 0 && note.port == 0,
                    "oracle only supports channel 1"
                );
                track.extend_from_slice(&[0x80, u8::try_from(note.key)?, 0]);
            }
            Event::Ump([word, _]) if word >> 28 == 2 => {
                let status = (word >> 16) as u8;
                ensure!(
                    status & 0x0f == 0 && status & 0xf0 == 0xb0,
                    "oracle only supports channel 1 CC events"
                );
                track.extend_from_slice(&[status, (word >> 8) as u8, word as u8]);
            }
            _ => bail!("scenario event is not representable by the oracle MIDI path"),
        }
        last = input.frame;
    }
    track.extend_from_slice(&[0, 0xff, 0x2f, 0]);
    let mut output = b"MThd\0\0\0\x06\0\0\0\x01\x5d\xc0MTrk".to_vec();
    output.extend_from_slice(&u32::try_from(track.len())?.to_be_bytes());
    output.extend_from_slice(&track);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scenario_uses_one_tick_per_frame_and_keeps_event_order() {
        let events = crate::scenario::user();
        let midi = write(&events).unwrap();
        assert_eq!(midi.get(12..14), Some(&[0x5d, 0xc0][..]));
        assert_eq!(midi.get(14..18), Some(&b"MTrk"[..]));
        assert!(midi.windows(3).any(|w| w == [0xb0, 1, 127]));
        assert!(midi.windows(3).any(|w| w == [0x90, 60, 100]));
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;
    #[test]
    fn midi_round_trip_keeps_every_event_at_its_source_frame() {
        let events = crate::scenario::user();
        let midi = write(&events).unwrap();
        let data = &midi[22..];
        let mut at = 7usize; // initial tempo meta event
        let mut frame = 0u64;
        let mut decoded = Vec::new();
        loop {
            let mut delta = 0u64;
            loop {
                let byte = data[at];
                at += 1;
                delta = (delta << 7) | u64::from(byte & 0x7f);
                if byte & 0x80 == 0 {
                    break;
                }
            }
            frame += delta;
            let status = data[at];
            at += 1;
            if status == 0xff {
                break;
            }
            let a = data[at];
            let b = data[at + 1];
            at += 2;
            decoded.push((frame, status, a, b));
        }
        let expected: Vec<_> = events
            .iter()
            .map(|e| match e.event {
                Event::NoteOn { note, velocity, .. } => {
                    (e.frame, 0x90, note.key, (velocity * 127.0).round() as u8)
                }
                Event::NoteOff(note) => (e.frame, 0x80, note.key as u8, 0),
                Event::Ump([word, _]) => {
                    (e.frame, (word >> 16) as u8, (word >> 8) as u8, word as u8)
                }
                _ => panic!("unexpected scenario event"),
            })
            .collect();
        assert_eq!(decoded, expected);
    }
}
