//! Standard MIDI files for the KSP MIDI object: a note on and its note off
//! become one event with a length (the manual's rule); everything else a
//! script cannot address (meta events, sysex) is dropped on read.

/// One event of the MIDI object. `command` is the status without its channel
/// (`0x90` note on, `0xB0` controller, ...); a note's `length` is in ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Event {
    pub id: i32,
    pub track: i32,
    pub pos: i32,
    pub command: i32,
    pub byte1: i32,
    pub byte2: i32,
    pub channel: i32,
    pub length: i32,
    /// `$EVENT_PAR_0..3`, free for the script.
    pub custom: [i32; 4],
    /// `$MARK_n` bits.
    pub marks: u32,
}

impl Event {
    /// Whether a file can carry it: a command KSP names, 7-bit data, a real
    /// channel and non-negative time.
    #[must_use]
    pub fn valid(&self) -> bool {
        matches!(self.command, 0x90 | 0xA0 | 0xB0 | 0xC0 | 0xD0 | 0xE0)
            && (0..16).contains(&self.channel)
            && (0..128).contains(&self.byte1)
            && (0..128).contains(&self.byte2)
            && self.track >= 0
            && self.pos >= 0
            && self.length >= -1
    }
}

/// The part of the object an export (a `save_midi_file`) writes; -1 is the
/// object's own edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub pos: (i32, i32),
    pub tracks: (i32, i32),
}

impl Area {
    /// The object's whole extent.
    pub const WHOLE: Self = Self {
        pos: (-1, -1),
        tracks: (-1, -1),
    };
}

/// A file's events (ids unset: the object numbers them), its tick
/// resolution and track count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Smf {
    pub events: Vec<Event>,
    pub division: u16,
    pub tracks: i32,
    pub area: Area,
}

/// Most events a file or the object holds (the manual's buffer limit).
pub const MAX_EVENTS: usize = 1_000_000;

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn byte(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }
    fn be32(&mut self) -> Option<usize> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?) as usize)
    }
    fn vlq(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.byte()?;
            v = v << 7 | u32::from(b & 127);
            if b & 128 == 0 {
                return Some(v);
            }
        }
        None
    }
}

/// Parses a format 0 or 1 file with a ticks-per-quarter division; None when
/// it is anything else or damaged. A note off with no note on is ignored, a
/// note on never switched off lasts to the track's end.
#[must_use]
pub fn read(bytes: &[u8]) -> Option<Smf> {
    let mut file = Cursor(bytes);
    (file.take(4)? == b"MThd").then_some(())?;
    let len = file.be32()?;
    let mut head = Cursor(file.take(len)?);
    let format = u16::from_be_bytes(head.take(2)?.try_into().ok()?);
    let tracks = u16::from_be_bytes(head.take(2)?.try_into().ok()?);
    let division = u16::from_be_bytes(head.take(2)?.try_into().ok()?);
    if format > 1 || tracks == 0 || division == 0 || division & 0x8000 != 0 {
        return None;
    }
    let mut events: Vec<Event> = Vec::new();
    for track in 0..i32::from(tracks) {
        (file.take(4)? == b"MTrk").then_some(())?;
        let len = file.be32()?;
        let mut data = Cursor(file.take(len)?);
        let (mut tick, mut running) = (0i64, 0u8);
        // Indices of the notes still sounding, per channel and key.
        let mut held: Vec<Vec<usize>> = vec![Vec::new(); 16 * 128];
        while !data.0.is_empty() {
            tick += i64::from(data.vlq()?);
            let at = i32::try_from(tick).ok()?;
            let status = match data.0.first()? {
                s if s & 128 != 0 => data.byte()?,
                _ if running != 0 => running,
                _ => return None,
            };
            match status {
                0x80..=0xEF => {
                    running = status;
                    let command = i32::from(status & 0xF0);
                    let channel = i32::from(status & 15);
                    let one_byte = matches!(command, 0xC0 | 0xD0);
                    let b1 = data.byte()?;
                    let b2 = if one_byte { 0 } else { data.byte()? };
                    if (b1 | b2) & 128 != 0 {
                        return None;
                    }
                    let held = held.get_mut((channel * 128) as usize + usize::from(b1))?;
                    if command == 0x80 || command == 0x90 && b2 == 0 {
                        if !held.is_empty()
                            && let Some(e) = events.get_mut(held.remove(0))
                        {
                            e.length = at - e.pos;
                        }
                        continue;
                    }
                    if events.len() == MAX_EVENTS {
                        return None;
                    }
                    if command == 0x90 {
                        held.push(events.len());
                    }
                    events.push(Event {
                        track,
                        pos: at,
                        command,
                        channel,
                        byte1: i32::from(b1),
                        byte2: i32::from(b2),
                        ..Event::default()
                    });
                }
                0xFF => {
                    running = 0;
                    data.byte()?;
                    let n = data.vlq()? as usize;
                    data.take(n)?;
                }
                0xF0 | 0xF7 => {
                    running = 0;
                    let n = data.vlq()? as usize;
                    data.take(n)?;
                }
                _ => return None,
            }
        }
        // Unfinished notes end with the track.
        for i in held.into_iter().flatten() {
            if let Some(e) = events.get_mut(i) {
                e.length = (tick as i32 - e.pos).max(0);
            }
        }
    }
    Some(Smf {
        events,
        division,
        tracks: i32::from(tracks),
        area: Area::WHOLE,
    })
}

fn put_vlq(out: &mut Vec<u8>, n: u32) {
    let groups = (1..4).filter(|&i| n >> (7 * i) != 0).count();
    for i in (0..=groups).rev() {
        out.push((n >> (7 * i)) as u8 & 127 | if i > 0 { 128 } else { 0 });
    }
}

/// Writes `smf`'s events inside its export area as a format 1 file, one
/// track per object track of the area; None when an event in the area is
/// not valid MIDI or the file would not fit its own limits.
#[must_use]
pub fn write(smf: &Smf) -> Option<Vec<u8>> {
    let (events, area) = (&smf.events, smf.area);
    let edge = |a: i32, b: i32, lo: i32, hi: i32| {
        let (a, b) = (if a < 0 { lo } else { a }, if b < 0 { hi } else { b });
        (a.min(b), a.max(b))
    };
    let first_pos = events.iter().map(|e| e.pos).min().unwrap_or(0);
    let last_pos = events
        .iter()
        .map(|e| e.pos.saturating_add(e.length))
        .max()
        .unwrap_or(0);
    let (p0, p1) = edge(area.pos.0, area.pos.1, first_pos, last_pos);
    let (t0, t1) = edge(
        area.tracks.0,
        area.tracks.1,
        0,
        smf.tracks.saturating_sub(1).max(0),
    );
    let count = u16::try_from(t1 - t0 + 1).ok().filter(|&n| n > 0)?;
    let mut out = b"MThd\0\0\0\x06\0\x01".to_vec();
    out.extend((count).to_be_bytes());
    out.extend(smf.division.max(1).to_be_bytes());
    for track in t0..=t1 {
        // (tick, off first, id) keeps a note's off before the next note on.
        let mut messages: Vec<(i32, bool, i32, [u8; 3], usize)> = Vec::new();
        for e in events
            .iter()
            .filter(|e| e.track == track && (p0..=p1).contains(&e.pos))
        {
            if !e.valid() {
                return None;
            }
            let status = (e.command | e.channel) as u8;
            let size = if matches!(e.command, 0xC0 | 0xD0) {
                2
            } else {
                3
            };
            messages.push((
                e.pos - p0,
                false,
                e.id,
                [status, e.byte1 as u8, e.byte2 as u8],
                size,
            ));
            if e.command == 0x90 {
                let off = e.pos.checked_add(e.length.max(0))? - p0;
                messages.push((
                    off,
                    true,
                    e.id,
                    [0x80 | e.channel as u8, e.byte1 as u8, 0],
                    3,
                ));
            }
        }
        messages.sort_by_key(|m| (m.0, !m.1, m.2));
        let mut data = Vec::new();
        let mut last = 0;
        for (tick, _, _, bytes, size) in messages {
            put_vlq(
                &mut data,
                u32::try_from(tick - last).ok().filter(|&d| d < 1 << 28)?,
            );
            data.extend(bytes.get(..size)?);
            last = tick;
        }
        data.extend([0, 0xFF, 0x2F, 0]);
        out.extend(b"MTrk");
        out.extend(u32::try_from(data.len()).ok()?.to_be_bytes());
        out.extend(data);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(track: i32, pos: i32, key: i32, length: i32) -> Event {
        Event {
            track,
            pos,
            command: 0x90,
            byte1: key,
            byte2: 100,
            length,
            ..Event::default()
        }
    }

    #[test]
    fn a_written_file_reads_back_with_its_note_lengths() {
        let events = vec![
            Event {
                id: 1,
                ..note(0, 0, 60, 240)
            },
            Event {
                id: 2,
                ..note(0, 480, 62, 120)
            },
            Event {
                id: 3,
                track: 1,
                pos: 10,
                command: 0xB0,
                byte1: 7,
                byte2: 90,
                ..Event::default()
            },
        ];
        let smf = Smf {
            events,
            division: 480,
            tracks: 2,
            area: Area::WHOLE,
        };
        let bytes = write(&smf).expect("valid events");
        let back = read(&bytes).expect("our own file");
        assert_eq!((back.division, back.tracks), (480, 2));
        let mut got: Vec<_> = back
            .events
            .iter()
            .map(|e| (e.track, e.pos, e.command, e.byte1, e.length))
            .collect();
        got.sort_unstable();
        assert_eq!(
            got,
            [
                (0, 0, 0x90, 60, 240),
                (0, 480, 0x90, 62, 120),
                (1, 10, 0xB0, 7, 0)
            ]
        );
    }

    #[test]
    fn an_area_clips_and_an_invalid_event_fails_the_write() {
        let mut smf = Smf {
            events: vec![note(0, 0, 60, 10), note(0, 100, 61, 10)],
            division: 96,
            tracks: 1,
            area: Area {
                pos: (50, -1),
                tracks: (-1, -1),
            },
        };
        let back = read(&write(&smf).unwrap()).unwrap();
        // The file starts at the area's start.
        assert_eq!((back.events.len(), back.events[0].pos), (1, 50));
        smf.events[0].byte1 = 200;
        smf.area = Area::WHOLE;
        assert!(write(&smf).is_none());
    }

    #[test]
    fn damaged_files_are_refused() {
        assert!(read(b"MThd").is_none());
        assert!(read(b"nope").is_none());
    }
}
