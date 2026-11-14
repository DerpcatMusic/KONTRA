//! KSP's MIDI object: one list of events every script slot shares, edited
//! and navigated by `mf_*`, filled from and written to standard MIDI files.
//!
//! A buffer of inactive events is what a script inserts into (the manual's
//! rule: inserting lowers the buffer size, removing raises it, a file does
//! not touch it). Files are read and written off the audio thread through
//! the file request queue; `mf_insert_file` in `on init` reads at once.

use kontra_plan::files::{FileReply, FileRequest};
use kontra_plan::midi_file::{self, Area, Event, MAX_EVENTS, Smf};
use kontra_vm::{Action, HostCall, HostValue, Text, event};

use crate::Engine;
use crate::queue::Exit;
use crate::script::Kind;

/// Most export areas (the manual's limit).
const AREAS: usize = 512;

#[derive(Debug)]
pub(crate) struct MidiObject {
    events: Vec<Event>,
    /// The id the position marker is on.
    selected: Option<i32>,
    /// Inactive events: what `mf_insert_event` can still activate.
    buffer: usize,
    next_id: i32,
    tracks: i32,
    division: u16,
    /// Export area 0 is the edit buffer `save_midi_file` writes.
    areas: Vec<(Area, Text)>,
    filename: Text,
}

impl MidiObject {
    pub(crate) fn new() -> Self {
        Self {
            events: Vec::new(),
            selected: None,
            buffer: 0,
            next_id: 1,
            tracks: 0,
            division: 960,
            areas: vec![(Area::WHOLE, Text::EMPTY)],
            filename: Text::EMPTY,
        }
    }
}

/// Which way the position marker moves.
#[derive(Clone, Copy, PartialEq)]
enum Move {
    First,
    Last,
    Next,
    Prev,
    NextAt(i32),
    PrevAt(i32),
}

fn get(e: &Event, par: i32) -> i32 {
    match par {
        0..=3 => e.custom.get(par as usize).copied().unwrap_or(0),
        event::MIDI_CHANNEL => e.channel,
        event::MIDI_COMMAND => e.command,
        event::MIDI_BYTE_1 => e.byte1,
        event::MIDI_BYTE_2 => e.byte2,
        event::POS => e.pos,
        event::NOTE_LENGTH => e.length,
        event::ID => e.id,
        event::TRACK_NR => e.track,
        _ => 0,
    }
}

fn set(e: &mut Event, par: i32, value: i32) {
    match par {
        0..=3 => {
            if let Some(c) = e.custom.get_mut(par as usize) {
                *c = value;
            }
        }
        event::MIDI_CHANNEL => e.channel = value,
        event::MIDI_COMMAND => e.command = value,
        event::MIDI_BYTE_1 => e.byte1 = value,
        event::MIDI_BYTE_2 => e.byte2 = value,
        event::POS => e.pos = value,
        event::NOTE_LENGTH => e.length = value,
        event::TRACK_NR => e.track = value,
        // An id names an event for good.
        _ => {}
    }
}

impl MidiObject {
    /// Whether `selector` (`$ALL_EVENTS`, `$CURRENT_EVENT`, `by_track`,
    /// `by_marks` or an event id) names `e`.
    fn names(&self, selector: i32, e: &Event) -> bool {
        match selector {
            event::ALL => true,
            event::CURRENT => self.selected == Some(e.id),
            s if s & event::BY_TRACK != 0 => e.track == s & !event::BY_TRACK,
            s if s & event::BY_MARKS != 0 => e.marks & (s & !event::BY_MARKS) as u32 != 0,
            id => e.id == id,
        }
    }

    fn first_named(&self, selector: i32) -> Option<&Event> {
        self.events.iter().find(|e| self.names(selector, e))
    }

    /// Moves the marker among the events of `track` (-1: all) in time order.
    fn step(&mut self, how: Move, track: i32) {
        let key = |e: &Event| (e.pos, e.id);
        let here = self
            .selected
            .and_then(|id| self.events.iter().find(|e| e.id == id))
            .map(key);
        let hits = self.events.iter().filter(|e| track < 0 || e.track == track);
        let found = match how {
            Move::First => hits.min_by_key(|e| key(e)),
            Move::Last => hits.max_by_key(|e| key(e)),
            Move::Next => hits
                .filter(|e| here.is_some_and(|h| key(e) > h))
                .min_by_key(|e| key(e)),
            Move::Prev => hits
                .filter(|e| here.is_some_and(|h| key(e) < h))
                .max_by_key(|e| key(e)),
            Move::NextAt(at) => hits.filter(|e| e.pos > at).min_by_key(|e| key(e)),
            Move::PrevAt(at) => hits.filter(|e| e.pos < at).max_by_key(|e| key(e)),
        };
        self.selected = found.map(|e| e.id);
    }

    fn insert(&mut self, [track, pos, command, byte1, byte2]: [i32; 5]) -> i32 {
        if self.buffer == 0 {
            return -1;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.buffer -= 1;
        self.events.push(Event {
            id,
            track,
            pos,
            command,
            byte1,
            byte2,
            ..Event::default()
        });
        self.tracks = self.tracks.max(track.saturating_add(1));
        id
    }

    fn remove(&mut self, selector: i32) {
        let before = self.events.len();
        let this = &*self;
        let keep: Vec<bool> = this
            .events
            .iter()
            .map(|e| !this.names(selector, e))
            .collect();
        let mut keep = keep.into_iter();
        self.events.retain(|_| keep.next().unwrap_or(true));
        self.buffer += before - self.events.len();
        if self
            .selected
            .is_some_and(|id| !self.events.iter().any(|e| e.id == id))
        {
            self.selected = None;
        }
    }

    fn reset(&mut self) {
        *self = Self {
            areas: std::mem::take(&mut self.areas),
            next_id: self.next_id,
            ..Self::new()
        };
    }

    /// Takes a read file in: `mode` 0 replaces every event, 1 only those
    /// inside the span the file covers on a track, 2 merges.
    fn insert_file(&mut self, smf: Smf, [track_by, pos_by, mode]: [i32; 3], name: Text) -> bool {
        let shift = |e: &Event| (e.track.checked_add(track_by), e.pos.checked_add(pos_by));
        if smf
            .events
            .iter()
            .any(|e| shift(e).0.is_none() || shift(e).1.is_none())
        {
            return false;
        }
        // Per track the file covers: its first and last tick, shifted.
        let mut spans: Vec<(i32, i32, i32)> = Vec::new();
        for e in &smf.events {
            let (track, from, to) = (
                e.track + track_by,
                e.pos + pos_by,
                e.pos + pos_by + e.length,
            );
            match spans.iter_mut().find(|s| s.0 == track) {
                Some(s) => (s.1, s.2) = (s.1.min(from), s.2.max(to)),
                None => spans.push((track, from, to)),
            }
        }
        let overlaps = |e: &Event| {
            spans
                .iter()
                .any(|s| s.0 == e.track && (s.1..=s.2).contains(&e.pos))
        };
        match mode {
            0 => self.events.clear(),
            1 => self.events.retain(|e| !overlaps(e)),
            _ => {}
        }
        if self.events.len() + smf.events.len() > MAX_EVENTS {
            return false;
        }
        self.events.reserve(smf.events.len());
        for e in smf.events {
            let id = self.next_id;
            self.next_id += 1;
            self.events.push(Event {
                id,
                track: e.track + track_by,
                pos: e.pos + pos_by,
                ..e
            });
        }
        self.selected = None;
        self.filename = name;
        self.division = smf.division;
        let tracks = smf.tracks.saturating_add(track_by);
        self.tracks = if mode == 0 {
            tracks
        } else {
            self.tracks.max(tracks)
        };
        true
    }

    /// The object as an export: events, resolution and area 0.
    fn export(&self) -> Smf {
        Smf {
            events: self.events.clone(),
            division: self.division,
            tracks: self.tracks,
            area: self.areas.first().map_or(Area::WHOLE, |a| a.0),
        }
    }

    /// `mf_set_export_area`: area 0 becomes the given span; the answer is 0,
    /// or the id of the first event in it that is not valid MIDI.
    fn set_area(&mut self, name: Text, area: Area) -> i32 {
        let ordered = |(a, b): (i32, i32)| {
            if a >= 0 && b >= 0 && a > b {
                (b, a)
            } else {
                (a, b)
            }
        };
        let area = Area {
            pos: ordered(area.pos),
            tracks: ordered(area.tracks),
        };
        if let Some(first) = self.areas.first_mut() {
            *first = (area, name);
        }
        let within = |v: i32, (a, b): (i32, i32)| (a < 0 || v >= a) && (b < 0 || v <= b);
        self.events
            .iter()
            .filter(|e| within(e.pos, area.pos) && within(e.track, area.tracks))
            .find(|e| !e.valid())
            .map_or(0, |e| e.id)
    }
}

impl Engine {
    /// The MIDI object's builtins; None for any other action.
    pub(crate) fn midi_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        use Action::*;
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let m = &mut self.midi;
        let step = |m: &mut MidiObject, how, track| {
            m.step(how, track);
            HostValue::Nothing
        };
        let current =
            |m: &MidiObject, par| m.first_named(event::CURRENT).map_or(0, |e| get(e, par));
        let edit = |m: &mut MidiObject, selector, par, value| {
            let this = &*m;
            let hit: Vec<bool> = this
                .events
                .iter()
                .map(|e| this.names(selector, e))
                .collect();
            m.events
                .iter_mut()
                .zip(hit)
                .filter(|(_, h)| *h)
                .for_each(|(e, _)| set(e, par, value));
            HostValue::Nothing
        };
        Some(match call.action {
            MfGetFirst => step(m, Move::First, int(0)),
            MfGetLast => step(m, Move::Last, int(0)),
            MfGetNext => step(m, Move::Next, int(0)),
            MfGetPrev => step(m, Move::Prev, int(0)),
            MfGetNextAt => step(m, Move::NextAt(int(1)), int(0)),
            MfGetPrevAt => step(m, Move::PrevAt(int(1)), int(0)),
            MfGetId => HostValue::Int(m.selected.unwrap_or(-1)),
            MfGetCommand => HostValue::Int(current(m, event::MIDI_COMMAND)),
            MfGetByteOne => HostValue::Int(current(m, event::MIDI_BYTE_1)),
            MfGetByteTwo => HostValue::Int(current(m, event::MIDI_BYTE_2)),
            MfGetChannel => HostValue::Int(current(m, event::MIDI_CHANNEL)),
            MfGetPos => HostValue::Int(current(m, event::POS)),
            MfGetLength => HostValue::Int(current(m, event::NOTE_LENGTH)),
            MfGetTrackIdx => HostValue::Int(current(m, event::TRACK_NR)),
            MfSetCommand => edit(m, event::CURRENT, event::MIDI_COMMAND, int(0)),
            MfSetByteOne => edit(m, event::CURRENT, event::MIDI_BYTE_1, int(0)),
            MfSetByteTwo => edit(m, event::CURRENT, event::MIDI_BYTE_2, int(0)),
            MfSetChannel => edit(m, event::CURRENT, event::MIDI_CHANNEL, int(0)),
            MfSetPos => edit(m, event::CURRENT, event::POS, int(0)),
            MfSetLength => edit(m, event::CURRENT, event::NOTE_LENGTH, int(0)),
            MfSetTrackIdx => edit(m, event::CURRENT, event::TRACK_NR, int(0)),
            MfGetEventPar => HostValue::Int(m.first_named(int(0)).map_or(0, |e| get(e, int(1)))),
            MfSetEventPar => edit(m, int(0), int(1), int(2)),
            MfGetNumTracks => HostValue::Int(m.tracks),
            MfGetBufferSize => HostValue::Int(m.buffer as i32),
            MfInsertEvent => HostValue::Int(m.insert([int(0), int(1), int(2), int(3), int(4)])),
            MfRemoveEvent => {
                m.remove(int(0));
                HostValue::Nothing
            }
            MfGetMark => HostValue::Int(i32::from(
                m.first_named(int(0))
                    .is_some_and(|e| e.marks & int(1) as u32 != 0),
            )),
            MfSetMark => {
                let (mark, on) = (int(1) as u32, int(2) != 0);
                let this = &*m;
                let hit: Vec<bool> = this.events.iter().map(|e| this.names(int(0), e)).collect();
                for (e, _) in m.events.iter_mut().zip(hit).filter(|(_, h)| *h) {
                    e.marks = if on { e.marks | mark } else { e.marks & !mark };
                }
                HostValue::Nothing
            }
            MfSetExportArea => {
                let name = call.texts.first().copied().unwrap_or(Text::EMPTY);
                let area = Area {
                    pos: (int(0), int(1)),
                    tracks: (int(2), int(3)),
                };
                HostValue::Int(m.set_area(name, area))
            }
            MfSetNumExportAreas => {
                let n = usize::try_from(int(0)).unwrap_or(0).clamp(1, AREAS);
                m.areas.resize(n, (Area::WHOLE, Text::EMPTY));
                HostValue::Nothing
            }
            MfCopyExportArea => {
                let edit = m.areas.first().copied();
                let slot = usize::try_from(int(0))
                    .ok()
                    .and_then(|i| m.areas.get_mut(i));
                if let (Some(edit), Some(slot)) = (edit, slot) {
                    *slot = edit;
                }
                HostValue::Nothing
            }
            MfGetLastFilename => {
                *call.text_out = m.filename;
                HostValue::Text
            }
            MfSetBufferSize => {
                let wanted = usize::try_from(int(0)).unwrap_or(0);
                let ok = m.events.len() + wanted <= MAX_EVENTS;
                if ok {
                    m.buffer = wanted;
                    // ponytail: reserves where the script calls it; Kontakt does it in `on init`.
                    m.events.reserve(wanted);
                }
                HostValue::Int(self.start_async(Exit::from(ok)))
            }
            MfReset => {
                m.reset();
                HostValue::Int(self.start_async(Exit::Done))
            }
            MfInsertFile | LoadMidiFile | SaveMidiFile => {
                let path = call.texts.first().copied().unwrap_or(Text::EMPTY);
                let save = call.action == SaveMidiFile;
                // The older load_midi_file replaces everything, like mode 0.
                let args = if call.action == MfInsertFile {
                    [int(0), int(1), int(2)]
                } else {
                    [0; 3]
                };
                return Some(HostValue::Int(self.midi_file(path, args, save)));
            }
            _ => return None,
        })
    }

    /// `mf_insert_file`, `load_midi_file` and `save_midi_file` of `path`: at
    /// once in `on init`; otherwise the control side carries the file and
    /// `on async_complete` follows.
    fn midi_file(&mut self, path: Text, args: [i32; 3], save: bool) -> i32 {
        let name = Text::from_str_cut(path.as_str().rsplit(['/', '\\']).next().unwrap_or("")).0;
        if !save && (self.building || self.running.kind == Kind::Init) {
            let script = self.script();
            let bytes = script.and_then(|s| s.read_file(path.as_str()));
            let smf = bytes.and_then(|b| midi_file::read(&b));
            let ok = smf.is_some_and(|smf| self.midi.insert_file(smf, args, name));
            return self.start_async(Exit::from(ok));
        }
        let request = FileRequest {
            slot: self.running.slot,
            id: 0,
            base: 0,
            // Not zero: zero asks the player to pick a file.
            len: 1,
            kind: kontra_plan::files::ArrayKind::Int,
            path,
            name,
            save,
            midi: Some(args),
            detect: None,
        };
        self.request_file(request)
    }

    /// The control side's answer to a MIDI file request: a load's parsed
    /// file lands in the object; a save's room takes a copy of the object to
    /// write. Returns whether the reply was a MIDI one.
    pub(crate) fn midi_file_done(&mut self, reply: &mut FileReply) -> bool {
        let r = reply.request;
        let Some(args) = r.midi else { return false };
        let exit = match (r.save, reply.written, reply.midi.take()) {
            (true, None, _) => {
                // ponytail: the copy allocates on the audio thread; a save is rare.
                reply.midi = Some(self.midi.export());
                return true;
            }
            (true, Some(written), _) => Exit::from(written),
            (false, _, Some(smf)) => Exit::from(self.midi.insert_file(smf, args, r.name)),
            (false, _, None) => Exit::Failed,
        };
        self.queue_async(r.slot, r.id, exit);
        true
    }
}

pub(crate) use MidiObject as Object;
