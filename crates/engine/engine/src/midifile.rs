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
//!
//! Export areas (`mf_set_export_area`, `mf_copy_export_area`,
//! `mf_set_num_export_areas`) are deliberately not modelled. They name
//! ranges for MIDI drag-and-drop export, which only `$CONTROL_PAR_DND_BEHAVIOUR`
//! labels with `$CONTROL_PAR_MIDI_EXPORT_AREA_IDX` and `save_midi_file` read
//! (KSP manual, MIDI Object Commands); KONTRA has neither, so the engine
//! leaves those calls reported as unanswered rather than storing areas
//! nothing would read.

/// The `mf_get_event_par`/`mf_set_event_par` parameters. These must match
/// `$EVENT_PAR_*` in `crates/script/ksp/data/constants.tsv`; the host test
/// `mf_event_pars_match_the_script_constants` keeps the two in step.
pub const PAR_POS: i32 = 200;
pub const PAR_NOTE_LENGTH: i32 = 201;
pub const PAR_ID: i32 = 202;
pub const PAR_TRACK_NR: i32 = 203;

/// `mf_insert_file` pairs each Note Off into the Note On that started it: the
/// Note On keeps the ticks up to the off in [`Event::len`], and the Note Off is
/// dropped so a script never sees `0x8n`. A Note On with velocity 0 ends a note
/// as well. Events arrive in tick order within one track.
fn pair_notes(events: &mut [Event]) {
    let mut open: Vec<(u8, u8, usize)> = Vec::new();
    for i in 0..events.len() {
        let status = events[i].status & 0xf0;
        let channel = events[i].status & 0x0f;
        let key = events[i].d1;
        let ends = status == 0x80 || (status == 0x90 && events[i].d2 == 0);
        if ends {
            if let Some(at) = open.iter().rposition(|&(c, k, _)| c == channel && k == key) {
                let (_, _, on) = open.remove(at);
                let len = events[i].tick.saturating_sub(events[on].tick);
                events[on].len = len;
            }
        } else if status == 0x90 {
            open.push((channel, key, i));
        }
    }
}

/// One addressable event, at an absolute tick position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub tick: u32,
    pub status: u8,
    pub d1: u8,
    pub d2: u8,
    /// The track it was read from, which `$EVENT_PAR_TRACK_NR` reports.
    pub track: u16,
    /// Ticks from this Note On to the Note Off that ends it, which is
    /// `$EVENT_PAR_NOTE_LENGTH`. Zero for anything that is not a note.
    pub len: u32,
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
            let from = events.len();
            read_track(&bytes[at + 8..end], track, &mut events, &mut length);
            pair_notes(&mut events[from..]);
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

    /// `by_track`: the id of the first event on track `n`, numbering tracks
    /// from 1 as scripts do (`by_track($t + 1)`), or -1 when the track has no
    /// events.
    pub fn by_track(&self, n: i32) -> i32 {
        let Some(want) = u16::try_from(n - 1).ok() else {
            return -1;
        };
        self.events
            .iter()
            .position(|e| e.track == want)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1)
    }

    /// `mf_get_event_par`: what `par` reads on the event `id`. Unknown
    /// parameters and unknown ids read 0, as Kontakt does for a miss.
    pub fn event_par(&self, id: i32, par: i32) -> i32 {
        let Some(e) = usize::try_from(id).ok().and_then(|i| self.events.get(i)) else {
            return 0;
        };
        match par {
            PAR_POS => e.tick as i32,
            PAR_NOTE_LENGTH => e.len as i32,
            PAR_ID => id,
            PAR_TRACK_NR => i32::from(e.track),
            _ => 0,
        }
    }

    /// `mf_set_event_par`. Only a note's length can be written: moving an
    /// event's position would renumber the ids a script holds, so that one is
    /// refused rather than silently reordering the file.
    pub fn set_event_par(&mut self, id: i32, par: i32, value: i32) {
        let Some(e) = usize::try_from(id).ok().and_then(|i| self.events.get_mut(i)) else {
            return;
        };
        if par == PAR_NOTE_LENGTH {
            let len = u32::try_from(value.max(0)).unwrap_or(u32::MAX);
            e.len = len;
            self.length = self.length.max(e.tick.saturating_add(len));
        }
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
        self.events.insert(at, Event { tick, status, d1, d2, track: 0, len: 0, active: true });
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
    let track = u16::try_from(track).unwrap_or(u16::MAX);
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
        out.push(Event { tick, status, d1, d2, track, len: 0, active: true });
        *length = (*length).max(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    /// One track, division 960: a Note On at tick 0 and its Note Off at 960.
    fn one_note() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"MThd");
        v.extend_from_slice(&6u32.to_be_bytes());
        v.extend_from_slice(&[0, 0, 0, 1, 0x03, 0xC0]);
        v.extend_from_slice(b"MTrk");
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(&[0x00, 0x90, 60, 100]);
        v.extend_from_slice(&[0x87, 0x40, 0x80, 60, 0]);
        v.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        v
    }

    #[test]
    fn a_note_off_becomes_the_note_ons_length() {
        let f = MidiFile::parse(&one_note()).expect("parses");
        assert_eq!(f.events.len(), 1, "the Note Off is dropped");
        assert_eq!(f.event_par(0, PAR_POS), 0);
        assert_eq!(f.event_par(0, PAR_NOTE_LENGTH), 960);
        assert_eq!(f.event_par(0, PAR_ID), 0);
        assert_eq!(f.event_par(0, PAR_TRACK_NR), 0);
    }

    #[test]
    fn setting_a_note_length_changes_what_it_reads() {
        let mut f = MidiFile::parse(&one_note()).expect("parses");
        f.set_event_par(0, PAR_NOTE_LENGTH, 480);
        assert_eq!(f.event_par(0, PAR_NOTE_LENGTH), 480);
        f.set_event_par(0, PAR_NOTE_LENGTH, -5);
        assert_eq!(f.event_par(0, PAR_NOTE_LENGTH), 0, "a negative length is 0");
    }

    #[test]
    fn by_track_finds_the_first_event_on_a_one_based_track() {
        let f = MidiFile::parse(&one_note()).expect("parses");
        assert_eq!(f.by_track(1), 0);
        assert_eq!(f.by_track(2), -1);
        assert_eq!(f.by_track(0), -1);
    }

    #[test]
    fn a_position_write_is_refused_and_a_miss_reads_zero() {
        let mut f = MidiFile::parse(&one_note()).expect("parses");
        f.set_event_par(0, PAR_POS, 500);
        assert_eq!(f.event_par(0, PAR_POS), 0, "positions are not rewritten");
        assert_eq!(f.event_par(99, PAR_NOTE_LENGTH), 0);
        assert_eq!(f.event_par(0, 12345), 0);
    }

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
