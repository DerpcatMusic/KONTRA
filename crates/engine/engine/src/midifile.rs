//! MIDI files a script inserts (`mf_insert_file`) and walks (`mf_get_next*`).
//!
//! Positions are **ticks**, the unit `mf_get_pos` returns and scripts compare
//! against: Session Guitarist declares `$l141c := 4*4*960` and loops while
//! `mf_get_pos() < (segment+1)*$l141c`, on a file whose division is 960
//! ticks per quarter. Milliseconds would make that comparison meaningless.
//!
//! Only channel voice messages are kept. Meta and sysex events advance the
//! clock but are not addressable, which matches how a script walks a file:
//! it asks for the command, byte one and byte two.

/// One addressable event, at an absolute tick position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub tick: u32,
    pub status: u8,
    pub d1: u8,
    pub d2: u8,
    /// `mf_insert_event` activates, `mf_remove_event` deactivates; the cursor
    /// skips inactive events, as Kontakt does.
    pub active: bool,
}

/// A parsed file plus the cursor scripts walk.
#[derive(Clone, Debug, Default)]
pub struct MidiFile {
    events: Vec<Event>,
    cursor: usize,
    /// Highest tick reached, which is what `mf_get_length` reports.
    length: u32,
    tracks: u16,
    division: u16,
}

impl MidiFile {
    /// Parses a standard MIDI file. Returns `None` when the header or a track
    /// chunk is malformed; a file with no channel events parses as empty.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
            return None;
        }
        let header = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        let tracks = u16::from_be_bytes([bytes[10], bytes[11]]);
        let division = u16::from_be_bytes([bytes[12], bytes[13]]);
        if division == 0 || division & 0x8000 != 0 {
            // SMPTE division is not what these libraries use; refuse rather than guess.
            return None;
        }
        let mut events = Vec::new();
        let mut length = 0;
        let mut at = 8 + header;
        let mut track = 0usize;
        while at + 8 <= bytes.len() && &bytes[at..at + 4] == b"MTrk" {
            let len = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
                as usize;
            let end = at.saturating_add(8).saturating_add(len).min(bytes.len());
            read_track(&bytes[at + 8..end], track, &mut events, &mut length);
            at = end;
            track += 1;
        }
        // mf_insert_file pairs each Note Off into its Note On (with a length
        // parameter) and discards the Note Off, so a script never sees 0x8n.
        events.retain(|e| e.status & 0xf0 != 0x80);
        events.sort_by_key(|e| (e.tick, e.status));
        Some(Self { events, cursor: 0, length, tracks, division })
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn division(&self) -> u16 {
        self.division
    }

    pub fn tracks(&self) -> u16 {
        self.tracks
    }

    pub fn length(&self) -> u32 {
        self.length
    }

    /// Tick of the event under the cursor, or the file's end when past it.
    pub fn pos(&self) -> u32 {
        self.events.get(self.cursor).map_or(self.length, |e| e.tick)
    }

    /// Next active event at or after the cursor.
    fn next_active(&self, from: usize) -> usize {
        let mut i = from.min(self.events.len());
        while i < self.events.len() && !self.events[i].active {
            i += 1;
        }
        i
    }

    /// `mf_insert_event`: activates an event; returns its id (its index).
    pub fn insert(&mut self, tick: u32, status: u8, d1: u8, d2: u8) -> usize {
        let at = self.events.partition_point(|e| e.tick < tick);
        self.events.insert(at, Event { tick, status, d1, d2, active: true });
        self.length = self.length.max(tick);
        at
    }

    /// `mf_remove_event`: deactivates the event with that id.
    pub fn remove(&mut self, id: usize) -> bool {
        match self.events.get_mut(id) {
            Some(e) if e.active => {
                e.active = false;
                true
            }
            _ => false,
        }
    }

    /// `mf_set_byte_one` / `mf_set_byte_two` act on the event under the cursor.
    pub fn set_byte_one(&mut self, value: u8) {
        if let Some(e) = self.events.get_mut(self.cursor) {
            e.d1 = value;
        }
    }

    pub fn set_byte_two(&mut self, value: u8) {
        if let Some(e) = self.events.get_mut(self.cursor) {
            e.d2 = value;
        }
    }

    pub fn set_command(&mut self, value: u8) {
        if let Some(e) = self.events.get_mut(self.cursor) {
            e.status = value;
        }
    }

    pub fn command(&self) -> u8 {
        self.events.get(self.cursor).map_or(0, |e| e.status)
    }

    pub fn byte_one(&self) -> u8 {
        self.events.get(self.cursor).map_or(0, |e| e.d1)
    }

    pub fn byte_two(&self) -> u8 {
        self.events.get(self.cursor).map_or(0, |e| e.d2)
    }

    /// Index of the first event at or after `tick`.
    fn seek(&self, tick: u32) -> usize {
        self.next_active(self.events.partition_point(|e| e.tick < tick))
    }

    pub fn next_at(&mut self, tick: u32) {
        self.cursor = self.seek(tick);
    }

    pub fn prev_at(&mut self, tick: u32) {
        let first = self.seek(tick);
        self.cursor = first.saturating_sub(1);
    }

    pub fn first(&mut self) {
        self.cursor = 0;
    }

    pub fn last(&mut self) {
        self.cursor = self.events.len().saturating_sub(1);
    }

    pub fn advance(&mut self) {
        self.cursor = self.next_active(self.cursor.saturating_add(1));
    }

    pub fn retreat(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn set_pos(&mut self, tick: u32) {
        self.cursor = self.seek(tick);
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }
}

fn var_len(bytes: &[u8], at: &mut usize) -> u32 {
    let mut value = 0u32;
    while *at < bytes.len() {
        let b = bytes[*at];
        *at += 1;
        value = (value << 7) | u32::from(b & 0x7f);
        if b & 0x80 == 0 {
            break;
        }
    }
    value
}

fn read_track(bytes: &[u8], track: usize, out: &mut Vec<Event>, length: &mut u32) {
    let _ = track;
    let mut at = 0usize;
    let mut tick = 0u32;
    let mut running: Option<u8> = None;
    while at < bytes.len() {
        let delta = var_len(bytes, &mut at);
        tick = tick.saturating_add(delta);
        if at >= bytes.len() {
            break;
        }
        let mut status = bytes[at];
        if status == 0xff || status == 0xf0 || status == 0xf7 {
            at += 1;
            if status != 0xf7 && at < bytes.len() {
                at += 1; // meta type
            }
            let len = var_len(bytes, &mut at) as usize;
            at = at.saturating_add(len).min(bytes.len());
            continue;
        }
        if status & 0x80 != 0 {
            at += 1;
            running = Some(status);
        } else {
            match running {
                Some(s) => status = s,
                None => break, // running status with nothing to run
            }
        }
        let high = status & 0xf0;
        let count = if high == 0xc0 || high == 0xd0 { 1 } else { 2 };
        if at + count > bytes.len() {
            break;
        }
        let d1 = bytes[at];
        let d2 = if count > 1 { bytes[at + 1] } else { 0 };
        at += count;
        out.push(Event { tick, status, d1, d2, active: true });
        *length = (*length).max(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 960 ticks/quarter, one track: note on at 0, note off at 960, note on at 1920.
    fn sample() -> Vec<u8> {
        let track: Vec<u8> = vec![
            0x00, 0x90, 0x3c, 0x46, // +0    note on 60 vel 70
            0x87, 0x40, 0x80, 0x3c, 0x40, // +960  note off 60
            0x87, 0x40, 0x90, 0x40, 0x46, // +960  note on 64 vel 70
            0x00, 0xff, 0x2f, 0x00, // end of track
        ];
        let mut file = b"MThd".to_vec();
        file.extend_from_slice(&6u32.to_be_bytes());
        file.extend_from_slice(&0u16.to_be_bytes());
        file.extend_from_slice(&1u16.to_be_bytes());
        file.extend_from_slice(&960u16.to_be_bytes());
        file.extend_from_slice(b"MTrk");
        file.extend_from_slice(&(track.len() as u32).to_be_bytes());
        file.extend_from_slice(&track);
        file
    }

    #[test]
    fn parses_channel_events_in_ticks() {
        let mut f = MidiFile::parse(&sample()).expect("parses");
        assert_eq!(f.division(), 960);
        assert_eq!(f.tracks(), 1);
        assert_eq!(f.length(), 1920);
        assert_eq!((f.pos(), f.command(), f.byte_one(), f.byte_two()), (0, 0x90, 0x3c, 0x46));
        f.next_at(1920);
        assert_eq!((f.pos(), f.command(), f.byte_one()), (1920, 0x90, 0x40));
    }

    #[test]
    fn insert_file_discards_note_offs() {
        // KSP Reference Manual 18.2: mf_insert_file pairs Note Off into its
        // Note On and discards it, so a script never reads 0x8n.
        let mut f = MidiFile::parse(&sample()).expect("parses");
        f.first();
        assert_eq!(f.command(), 0x90);
        let mut seen = Vec::new();
        loop {
            let c = f.command();
            if c == 0 {
                break;
            }
            seen.push((f.pos(), c));
            f.advance();
        }
        assert!(seen.iter().all(|(_, c)| c & 0xf0 != 0x80), "note off survived: {seen:?}");
        assert_eq!(seen.len(), 2, "{seen:?}");
    }

    #[test]
    fn seek_lands_on_the_first_event_at_or_after_the_tick() {
        let mut f = MidiFile::parse(&sample()).expect("parses");
        f.next_at(961);
        assert_eq!(f.pos(), 1920);
        f.next_at(5000);
        assert_eq!(f.pos(), 1920, "past the end stays at the end");
        assert_eq!(f.command(), 0);
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(MidiFile::parse(b"").is_none());
        assert!(MidiFile::parse(b"RIFF....WAVE").is_none());
    }
}
