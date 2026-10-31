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
    // Lengths are collected first and written afterwards: nothing in this walk
    // reads `len`, so the write order cannot change which note an off pairs
    // with. It keeps the loop over an immutable borrow.
    let mut lengths: Vec<(usize, u32)> = Vec::new();
    for (i, event) in events.iter().enumerate() {
        let status = event.status & 0xf0;
        let channel = event.status & 0x0f;
        let key = event.d1;
        let ends = status == 0x80 || (status == 0x90 && event.d2 == 0);
        if ends {
            if let Some(at) = open.iter().rposition(|&(c, k, _)| c == channel && k == key) {
                let (_, _, on) = open.remove(at);
                let on_tick = events.get(on).map_or(event.tick, |e| e.tick);
                lengths.push((on, event.tick.saturating_sub(on_tick)));
            }
        } else if status == 0x90 {
            open.push((channel, key, i));
        }
    }
    for (on, len) in lengths {
        if let Some(slot) = events.get_mut(on) {
            slot.len = len;
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
    /// Every event the file holds, in the order they were read or inserted.
    /// An event's id is its index here, so inserting one never renumbers the
    /// ids a script already holds.
    events: Vec<Event>,
    /// Those ids in tick order, which is what the cursor walks.
    order: Vec<usize>,
    /// The place in `order` of the event the script reads.
    cursor: usize,
    /// Highest tick reached, which is what `mf_get_length` reports.
    length: u32,
    tracks: u16,
    division: u16,
}

/// Big-endian `u16` at `at`, or `None` when the slice ends first.
fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let field: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(field))
}

/// Big-endian `u32` at `at`, or `None` when the slice ends first.
fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(field))
}

impl MidiFile {
    /// Parses a standard MIDI file. Returns `None` when the header or a track
    /// chunk is malformed; a file with no channel events parses as empty.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.get(..4)? != b"MThd" {
            return None;
        }
        let header = be_u32(bytes, 4)? as usize;
        let tracks = be_u16(bytes, 10)?;
        let division = be_u16(bytes, 12)?;
        if division == 0 || division & 0x8000 != 0 {
            // SMPTE division is not what these libraries use; refuse rather than guess.
            return None;
        }
        let mut events = Vec::new();
        let mut length = 0;
        let mut at = 8usize.saturating_add(header);
        let mut track = 0usize;
        while let Some(chunk) = bytes.get(at..) {
            if chunk.get(..4) != Some(b"MTrk".as_slice()) {
                break;
            }
            let Some(len) = be_u32(chunk, 4) else {
                break;
            };
            let end = at
                .saturating_add(8)
                .saturating_add(len as usize)
                .min(bytes.len());
            let Some(body) = bytes.get(at.saturating_add(8)..end) else {
                break;
            };
            let from = events.len();
            read_track(body, track, &mut events, &mut length);
            if let Some(added) = events.get_mut(from..) {
                pair_notes(added);
            }
            at = end;
            track += 1;
        }
        // mf_insert_file pairs each Note Off into its Note On (with a length
        // parameter) and discards the Note Off, so a script never sees 0x8n.
        // A Note On at velocity 0 ends a note as well ([`pair_notes`]), so it
        // goes with the Note Off: left in, the cursor could read it and a
        // script would pass its velocity 0 to `play_note`, which clamps it to
        // 1 and sounds a note the file never asked for.
        events.retain(|e| {
            let status = e.status & 0xf0;
            status != 0x80 && !(status == 0x90 && e.d2 == 0)
        });
        // Tick order, and the file's own order within a tick: the sort is
        // stable and the ids arrive in read order. Sorting by status as well
        // would move a controller a script stored before a note-on.
        let mut order: Vec<usize> = (0..events.len()).collect();
        let by_tick = |&id: &usize| events.get(id).map_or(0, |e| e.tick);
        order.sort_by_key(by_tick);
        Some(Self {
            events,
            order,
            cursor: 0,
            length,
            tracks,
            division,
        })
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
        // `n - 1` panics at `i32::MIN`, and this runs on the audio thread.
        let Some(want) = n.checked_sub(1).and_then(|m| u16::try_from(m).ok()) else {
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
        let Some(e) = usize::try_from(id)
            .ok()
            .and_then(|i| self.events.get_mut(i))
        else {
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
        self.under_cursor().map_or(self.length, |e| e.tick)
    }

    /// The event the cursor reads.
    fn under_cursor(&self) -> Option<&Event> {
        let id = *self.order.get(self.cursor)?;
        self.events.get(id)
    }

    /// The event the cursor reads, to change.
    fn under_cursor_mut(&mut self) -> Option<&mut Event> {
        let id = *self.order.get(self.cursor)?;
        self.events.get_mut(id)
    }

    /// Next active event at or after place `from` in the tick order; past the
    /// end when none.
    fn next_active(&self, from: usize) -> usize {
        let from = from.min(self.order.len());
        let rest = self.order.get(from..).unwrap_or_default();
        rest.iter()
            .position(|&id| self.events.get(id).is_some_and(|e| e.active))
            .map_or(self.order.len(), |offset| from + offset)
    }

    /// Last active event at or before `from`, or `None` when there is none:
    /// the backward steps skip a removed event as the forward ones do, and a
    /// step with nothing to land on leaves the cursor where it is.
    fn prev_active(&self, from: usize) -> Option<usize> {
        let end = from.saturating_add(1).min(self.order.len());
        self.order
            .get(..end)
            .unwrap_or_default()
            .iter()
            .rposition(|&id| self.events.get(id).is_some_and(|e| e.active))
    }

    /// `mf_insert_event`: adds an event, on `track`, and returns its id. Its
    /// id is new, so the ids a script already holds still name their events;
    /// only the cursor, a place in the tick order, moves when the new event
    /// lands at or before it.
    pub fn insert(&mut self, track: u16, tick: u32, status: u8, d1: u8, d2: u8) -> usize {
        let id = self.events.len();
        self.events.push(Event {
            tick,
            status,
            d1,
            d2,
            track,
            len: 0,
            active: true,
        });
        let events = &self.events;
        let at = self
            .order
            .partition_point(|&i| events.get(i).is_some_and(|e| e.tick < tick));
        self.order.insert(at, id);
        if at <= self.cursor {
            self.cursor += 1;
        }
        self.length = self.length.max(tick);
        id
    }

    /// `mf_remove_event`: deactivates the event with that id; the cursor
    /// moves on when it was reading it.
    pub fn remove(&mut self, id: usize) -> bool {
        match self.events.get_mut(id) {
            Some(e) if e.active => {
                e.active = false;
                if self.order.get(self.cursor) == Some(&id) {
                    self.cursor = self.next_active(self.cursor);
                }
                true
            }
            _ => false,
        }
    }

    /// `mf_set_byte_one` / `mf_set_byte_two` act on the event under the cursor.
    pub fn set_byte_one(&mut self, value: u8) {
        if let Some(e) = self.under_cursor_mut() {
            e.d1 = value;
        }
    }

    pub fn set_byte_two(&mut self, value: u8) {
        if let Some(e) = self.under_cursor_mut() {
            e.d2 = value;
        }
    }

    pub fn set_command(&mut self, value: u8) {
        if let Some(e) = self.under_cursor_mut() {
            e.status = value;
        }
    }

    pub fn command(&self) -> u8 {
        self.under_cursor().map_or(0, |e| e.status)
    }

    pub fn byte_one(&self) -> u8 {
        self.under_cursor().map_or(0, |e| e.d1)
    }

    pub fn byte_two(&self) -> u8 {
        self.under_cursor().map_or(0, |e| e.d2)
    }

    /// Place in the tick order of the first event at or after `tick`.
    fn seek(&self, tick: u32) -> usize {
        let at = self
            .order
            .partition_point(|&id| self.events.get(id).is_some_and(|e| e.tick < tick));
        self.next_active(at)
    }

    pub fn next_at(&mut self, tick: u32) {
        self.cursor = self.seek(tick);
    }

    pub fn prev_at(&mut self, tick: u32) {
        let first = self.seek(tick);
        if let Some(at) = self.prev_active(first.saturating_sub(1)) {
            self.cursor = at;
        }
    }

    pub fn first(&mut self) {
        self.cursor = self.next_active(0);
    }

    pub fn last(&mut self) {
        if let Some(at) = self.prev_active(self.order.len().saturating_sub(1)) {
            self.cursor = at;
        }
    }

    pub fn advance(&mut self) {
        self.cursor = self.next_active(self.cursor.saturating_add(1));
    }

    pub fn retreat(&mut self) {
        if let Some(at) = self.prev_active(self.cursor.saturating_sub(1)) {
            self.cursor = at;
        }
    }

    pub fn set_pos(&mut self, tick: u32) {
        self.cursor = self.seek(tick);
    }

    pub fn reset(&mut self) {
        self.cursor = self.next_active(0);
    }
}

fn var_len(bytes: &[u8], at: &mut usize) -> u32 {
    let mut value = 0u32;
    while let Some(&b) = bytes.get(*at) {
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
        let Some(&byte) = bytes.get(at) else {
            break;
        };
        let mut status = byte;
        if status == 0xff || status == 0xf0 || status == 0xf7 {
            at += 1;
            // SMF 1.0: meta and sysex events cancel running status.
            running = None;
            // Only a meta event carries a type byte: a sysex is `F0 <len>
            // <data>`, so a byte skipped here would be the length's own first
            // byte, and the length would be read from the data.
            if status == 0xff && at < bytes.len() {
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
        let Some(data) = bytes.get(at..at.saturating_add(count)) else {
            break;
        };
        let Some(&d1) = data.first() else {
            break;
        };
        let d2 = data.get(1).copied().unwrap_or(0);
        at += count;
        out.push(Event {
            tick,
            status,
            d1,
            d2,
            track,
            len: 0,
            active: true,
        });
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
        // `n - 1` would overflow here, on the audio thread.
        assert_eq!(f.by_track(i32::MIN), -1);
    }

    /// 960 ticks/quarter, one track: a controller, then a note, both at 0.
    fn same_tick() -> Vec<u8> {
        let track: Vec<u8> = vec![
            0x00, 0xB0, 0x07, 0x64, // +0  controller 7 = 100
            0x00, 0x90, 0x3c, 0x46, // +0  note on 60 vel 70
            0x60, 0xFF, 0x2F, 0x00, // end of track
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
    fn events_at_one_tick_keep_the_files_order() {
        let mut f = MidiFile::parse(&same_tick()).expect("parses");
        f.first();
        assert_eq!(f.command(), 0xB0, "the controller was moved after the note");
        f.advance();
        assert_eq!(f.command(), 0x90);
    }

    /// A sysex event (a GM reset, as files carry) then a note: `F0 <len>
    /// <data>`, with its length a variable-length quantity like `0x81 0x00`.
    #[test]
    fn a_multi_byte_sysex_length_is_read_as_one_length() {
        let data = [0x7e, 0x7f, 0x09, 0x01, 0xf7];
        let mut track: Vec<u8> = vec![
            0x00, 0xf0, 0x81, 0x00, // +0  sysex, length 128 (two bytes)
        ];
        track.extend_from_slice(&data);
        track.extend_from_slice(&[0u8; 123]); // …to the length it declared
        track.extend_from_slice(&[0x60, 0x90, 0x3c, 0x46]); // note on 60, 96 ticks on
        track.extend_from_slice(&[0x60, 0xff, 0x2f, 0x00]);
        let mut file = b"MThd".to_vec();
        file.extend_from_slice(&6u32.to_be_bytes());
        file.extend_from_slice(&0u16.to_be_bytes());
        file.extend_from_slice(&1u16.to_be_bytes());
        file.extend_from_slice(&960u16.to_be_bytes());
        file.extend_from_slice(b"MTrk");
        file.extend_from_slice(&(track.len() as u32).to_be_bytes());
        file.extend_from_slice(&track);
        let f = MidiFile::parse(&file).expect("parses");
        assert_eq!(
            (
                f.events.len(),
                f.event_par(0, PAR_POS),
                f.event_par(0, PAR_ID)
            ),
            (1, 96, 0),
            "the note after the sysex is at tick 96: {} events",
            f.events.len()
        );
    }

    /// The SMF spec cancels running status after a meta or sysex event, so a
    /// data byte after one starts nothing.
    #[test]
    fn a_meta_event_cancels_running_status() {
        let track: Vec<u8> = vec![
            0x00, 0x90, 0x3c, 0x46, // note on 60 (running status 0x90)
            0x00, 0xff, 0x01, 0x02, 0x41, 0x42, // text meta: "AB"
            0x00, 0x40, 0x46, // a bare data byte: no status to run
        ];
        let mut file = b"MThd".to_vec();
        file.extend_from_slice(&6u32.to_be_bytes());
        file.extend_from_slice(&0u16.to_be_bytes());
        file.extend_from_slice(&1u16.to_be_bytes());
        file.extend_from_slice(&960u16.to_be_bytes());
        file.extend_from_slice(b"MTrk");
        file.extend_from_slice(&(track.len() as u32).to_be_bytes());
        file.extend_from_slice(&track);
        let f = MidiFile::parse(&file).expect("parses");
        assert_eq!(f.events.len(), 1, "the data byte invented an event");
    }

    #[test]
    fn an_insert_keeps_the_ids_and_the_cursor_the_script_holds() {
        let mut f = MidiFile::parse(&sample()).expect("parses");
        // Two notes: id 0 at tick 0, id 1 at 1920. The script walks to the
        // second and holds its id.
        f.next_at(1920);
        assert_eq!((f.pos(), f.byte_one()), (1920, 0x40));
        let id = f.insert(0, 0, 0x90, 72, 100);
        assert_eq!(id, 2, "the new event takes a new id");
        assert_eq!(f.event_par(0, PAR_POS), 0);
        assert_eq!(f.event_par(1, PAR_POS), 1920, "id 1 still names its note");
        assert_eq!(f.event_par(2, PAR_POS), 0);
        assert_eq!(
            (f.pos(), f.byte_one()),
            (1920, 0x40),
            "the cursor moved onto the inserted event"
        );
        // Inserting after the cursor leaves it alone.
        let id = f.insert(0, 4000, 0x90, 40, 100);
        assert_eq!(id, 3);
        assert_eq!((f.pos(), f.byte_one()), (1920, 0x40));
    }

    #[test]
    fn backward_and_reset_navigation_skip_a_removed_event() {
        let mut f = MidiFile::parse(&sample()).expect("parses");
        assert!(f.remove(0));
        f.first();
        assert_eq!(f.pos(), 1920, "first() landed on the removed event");
        f.reset();
        assert_eq!(f.pos(), 1920, "reset() landed on the removed event");
        // Nothing active is earlier than the first active event, so a
        // backward step leaves the cursor where it is instead of moving it
        // onto the removed id 0.
        f.retreat();
        assert_eq!(f.pos(), 1920, "retreat() landed on the removed event");
        f.prev_at(0);
        assert_eq!(f.pos(), 1920, "prev_at(0) landed on the removed event");
        f.last();
        assert_eq!(f.pos(), 1920);
        f.prev_at(5000);
        assert_eq!(f.pos(), 1920, "prev_at() landed on the removed event");
        // Removing the event under the cursor moves the cursor off it.
        f.first();
        assert!(f.remove(1));
        assert_eq!(f.command(), 0, "the cursor still reads the removed event");
    }

    /// 960 ticks/quarter, one track: note on 60, the note on at velocity 0
    /// that ends it, then note on 64. A zero-velocity Note On is an off.
    fn zero_velocity_off() -> Vec<u8> {
        let track: Vec<u8> = vec![
            0x00, 0x90, 0x3c, 0x46, // +0    note on 60 vel 70
            0x87, 0x40, 0x90, 0x3c, 0x00, // +960  note on 60 vel 0 (an off)
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
    fn insert_file_discards_a_zero_velocity_note_on() {
        // It ends the note like a Note Off, so it is dropped with it: readable,
        // a script would hand its velocity 0 to `play_note` and sound a note
        // the file never asked for.
        let mut f = MidiFile::parse(&zero_velocity_off()).expect("parses");
        assert_eq!(f.events.len(), 2, "the velocity-0 note on survived");
        assert_eq!(f.event_par(0, PAR_NOTE_LENGTH), 960);
        assert_eq!(f.event_par(1, PAR_POS), 1920);
        let mut seen = Vec::new();
        f.first();
        loop {
            let c = f.command();
            if c == 0 {
                break;
            }
            seen.push((f.pos(), f.byte_one(), f.byte_two()));
            f.advance();
        }
        assert_eq!(seen, vec![(0, 0x3c, 0x46), (1920, 0x40, 0x46)], "{seen:?}");
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
        assert_eq!(
            (f.pos(), f.command(), f.byte_one(), f.byte_two()),
            (0, 0x90, 0x3c, 0x46)
        );
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
        assert!(
            seen.iter().all(|(_, c)| c & 0xf0 != 0x80),
            "note off survived: {seen:?}"
        );
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
