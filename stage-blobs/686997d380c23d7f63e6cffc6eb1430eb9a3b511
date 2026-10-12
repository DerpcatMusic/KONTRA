//! One mutable MIDI object per instrument plan, shared by all script slots.
use crate::{Error, Prepared, Text};
use std::collections::BTreeSet;

pub const MIDI_MAX_EVENTS: usize = 1_000_000;
pub const MIDI_SERVICE: u16 = u16::MAX;
pub const MIDI_ASYNC_SIGNAL: u16 = u16::MAX;
pub const MIDI_CURRENT_EVENT: i32 = 0x3fff_fffd;
pub const MIDI_ALL_EVENTS: i32 = 0x3fff_fffe;
pub const MIDI_TRACK_FLAG: i32 = 0x4000_0000;
pub const MIDI_MARKS_FLAG: i32 = 0x2000_0000;

/// Parameter values are the frontend's resolved constants, never symbol hashes.
pub mod midi_par {
    pub const CHANNEL: i32 = 13;
    pub const COMMAND: i32 = 17;
    pub const BYTE_ONE: i32 = 18;
    pub const BYTE_TWO: i32 = 19;
    pub const POSITION: i32 = 20;
    pub const LENGTH: i32 = 21;
    pub const ID: i32 = 22;
    pub const TRACK: i32 = 23;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidiObjectEvent {
    pub id: i32,
    pub track: i32,
    pub position: i32,
    pub command: i32,
    pub byte_one: i32,
    pub byte_two: i32,
    pub channel: i32,
    pub length: i32,
    pub custom: [i32; 4],
    pub marks: i32,
}
impl MidiObjectEvent {
    fn get(self, parameter: i32) -> i32 {
        use midi_par::*;
        match parameter {
            0..=3 => self.custom[parameter as usize],
            CHANNEL => self.channel,
            COMMAND => self.command,
            BYTE_ONE => self.byte_one,
            BYTE_TWO => self.byte_two,
            POSITION => self.position,
            LENGTH => self.length,
            ID => self.id,
            TRACK => self.track,
            _ => 0,
        }
    }
    fn set(&mut self, parameter: i32, value: i32) {
        use midi_par::*;
        match parameter {
            0..=3 => self.custom[parameter as usize] = value,
            CHANNEL => self.channel = value,
            COMMAND => self.command = value,
            BYTE_ONE => self.byte_one = value,
            BYTE_TWO => self.byte_two = value,
            POSITION => self.position = value,
            LENGTH => self.length = value,
            TRACK => self.track = value,
            // IDs identify stored events and cannot be reassigned to collide.
            _ => {}
        }
    }
    fn valid(self) -> bool {
        self.track >= 0
            && self.position >= 0
            && self.length >= 0
            && (0..16).contains(&self.channel)
            && matches!(self.command, 0x90 | 0xa0 | 0xb0 | 0xc0 | 0xd0 | 0xe0)
            && (0..128).contains(&self.byte_one)
            && (0..128).contains(&self.byte_two)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiExportArea {
    pub name: Text,
    pub start_position: i32,
    pub end_position: i32,
    pub start_track: i32,
    pub end_track: i32,
}
impl Default for MidiExportArea {
    fn default() -> Self {
        Self {
            name: Text::default(),
            start_position: -1,
            end_position: -1,
            start_track: -1,
            end_track: -1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiAction {
    First,
    Last,
    Next,
    Previous,
    NextAt,
    PreviousAt,
    Id,
    Get(i32),
    Set(i32),
    GetParameter,
    SetParameter,
    Tracks,
    BufferSize,
    SetBufferSize,
    Insert,
    Remove,
    GetMark,
    SetMark,
    ExportArea,
    ExportCount,
    CopyExportArea,
    Reset,
    InsertFile,
    SaveFile,
}
impl MidiAction {
    pub fn arguments(self) -> usize {
        match self {
            Self::First
            | Self::Last
            | Self::Next
            | Self::Previous
            | Self::Set(_)
            | Self::SetBufferSize
            | Self::Remove
            | Self::ExportCount
            | Self::CopyExportArea => 1,
            Self::NextAt | Self::PreviousAt | Self::GetParameter | Self::GetMark => 2,
            Self::SetParameter | Self::SetMark | Self::InsertFile => 3,
            Self::ExportArea => 4,
            Self::Insert => 5,
            _ => 0,
        }
    }
    pub fn asynchronous(self) -> bool {
        matches!(
            self,
            Self::SetBufferSize | Self::Reset | Self::InsertFile | Self::SaveFile
        )
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct MidiObject {
    pub(crate) events: Vec<MidiObjectEvent>,
    selected: Option<i32>,
    inactive: usize,
    next_id: i32,
    pub(crate) exports: Vec<MidiExportArea>,
    pub(crate) filename: Text,
    pub(crate) division: u16,
    tracks: i32,
    pub(crate) jobs: Vec<MidiJob>,
    pub(crate) next_job: i32,
}
impl Clone for MidiObject {
    fn clone(&self) -> Self {
        let mut events = Vec::with_capacity(self.events.capacity());
        events.extend_from_slice(&self.events);
        let mut exports = Vec::with_capacity(self.exports.capacity());
        exports.extend_from_slice(&self.exports);
        let mut jobs = Vec::with_capacity(self.jobs.capacity());
        jobs.extend_from_slice(&self.jobs);
        Self {
            events,
            exports,
            selected: self.selected,
            inactive: self.inactive,
            next_id: self.next_id,
            filename: self.filename,
            division: self.division,
            tracks: self.tracks,
            jobs,
            next_job: self.next_job,
        }
    }
}
impl MidiObject {
    pub fn new(events: Vec<MidiObjectEvent>) -> Result<Self, Error> {
        let mut ids = BTreeSet::new();
        if events.len() > MIDI_MAX_EVENTS
            || events.iter().any(|e| {
                e.id < 0
                    || e.id >= MIDI_MARKS_FLAG
                    || e.track < 0
                    || e.position < 0
                    || !ids.insert(e.id)
            })
        {
            return Err(Error::InvalidInput);
        }
        let next_id = events
            .iter()
            .map(|e| e.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let tracks = events
            .iter()
            .map(|e| e.track)
            .max()
            .map_or(0, |t| t.saturating_add(1));
        Ok(Self {
            events,
            next_id,
            tracks,
            ..Default::default()
        })
    }
    /// Reserve virtual storage off audio only for plans that use MIDI commands.
    pub fn prepare_live(&mut self) {
        self.next_id = self.next_id.max(1);
        if self.division == 0 {
            self.division = 960;
        }
        self.jobs.reserve(256usize.saturating_sub(self.jobs.len()));
        self.events
            .reserve(MIDI_MAX_EVENTS.saturating_sub(self.events.len()));
        self.exports
            .reserve(512usize.saturating_sub(self.exports.len()));
        if self.exports.is_empty() {
            self.exports.push(MidiExportArea::default());
        }
    }
    pub fn first(&mut self, track: i32) {
        let _ = self.apply(MidiAction::First, &[track], None);
    }
    pub fn selected_id(&self) -> i32 {
        self.selected.unwrap_or(-1)
    }
    pub fn last_filename(&self) -> &str {
        self.filename.as_str()
    }
    fn matches(&self, e: &MidiObjectEvent, selector: i32) -> bool {
        matches_event(e, selector, self.selected)
    }
    fn find(&self, selector: i32) -> Option<&MidiObjectEvent> {
        self.events
            .iter()
            .filter(|e| self.matches(e, selector))
            .min_by_key(|e| (e.position, e.id))
    }
    /// Numeric edits never allocate; init reserves before applying buffer edits.
    pub fn apply(
        &mut self,
        action: MidiAction,
        args: &[i32],
        text: Option<Text>,
    ) -> Result<i32, Error> {
        use MidiAction::*;
        if args.len() < action.arguments() {
            return Err(Error::InvalidInput);
        }
        let a = |i: usize| args.get(i).copied().unwrap_or(0);
        let value = match action {
            First | Last | Next | Previous | NextAt | PreviousAt => {
                let track = a(0);
                let current = self.find(MIDI_CURRENT_EVENT).map(|e| (e.position, e.id));
                let forward = matches!(action, First | Next | NextAt);
                let candidates = self.events.iter().filter(|e| {
                    if track != -1 && e.track != track {
                        return false;
                    }
                    match action {
                        Next => current.is_some_and(|k| (e.position, e.id) > k),
                        Previous => current.is_some_and(|k| (e.position, e.id) < k),
                        NextAt => e.position > a(1),
                        PreviousAt => e.position < a(1),
                        _ => true,
                    }
                });
                self.selected = if forward {
                    candidates.min_by_key(|e| (e.position, e.id))
                } else {
                    candidates.max_by_key(|e| (e.position, e.id))
                }
                .map(|e| e.id);
                0
            }
            Id => self.selected_id(),
            Get(parameter) => self
                .find(MIDI_CURRENT_EVENT)
                .map_or(0, |e| e.get(parameter)),
            GetParameter => self.find(a(0)).map_or(0, |e| e.get(a(1))),
            Set(_) | SetParameter => {
                let (selector, parameter, value) = if action == SetParameter {
                    (a(0), a(1), a(2))
                } else {
                    (
                        MIDI_CURRENT_EVENT,
                        match action {
                            Set(parameter) => parameter,
                            _ => unreachable!(),
                        },
                        a(0),
                    )
                };
                if parameter == midi_par::ID {
                    let Some(old) = self.find(selector).map(|e| e.id) else {
                        return Ok(0);
                    };
                    if value < 0
                        || value >= MIDI_MARKS_FLAG
                        || self
                            .events
                            .iter()
                            .filter(|e| self.matches(e, selector))
                            .count()
                            != 1
                        || self.events.iter().any(|e| e.id == value && e.id != old)
                    {
                        return Err(Error::InvalidInput);
                    }
                    self.events.iter_mut().find(|e| e.id == old).unwrap().id = value;
                    if self.selected == Some(old) {
                        self.selected = Some(value);
                    }
                    self.next_id = self.next_id.max(value + 1);
                    return Ok(0);
                }
                let selected = self.selected;
                for e in &mut self.events {
                    let matches = matches_event(e, selector, selected);
                    if matches {
                        e.set(parameter, value);
                    }
                }
                0
            }
            Tracks => self.tracks.max(
                self.events
                    .iter()
                    .map(|e| e.track)
                    .max()
                    .map_or(0, |t| t.saturating_add(1)),
            ),
            BufferSize => self.inactive as i32,
            SetBufferSize => {
                let count = usize::try_from(a(0)).map_err(|_| Error::InvalidInput)?;
                if count + self.events.len() > MIDI_MAX_EVENTS {
                    return Err(Error::InvalidInput);
                }
                if count + self.events.len() > self.events.capacity() {
                    return Err(Error::Capacity);
                }
                self.inactive = count;
                0
            }
            Insert => {
                if self.inactive == 0 {
                    return Ok(-1);
                }
                if self.events.len() == self.events.capacity() {
                    return Err(Error::Capacity);
                }
                let id = self.next_id;
                if id >= MIDI_MARKS_FLAG {
                    return Err(Error::Capacity);
                }
                self.events.push(MidiObjectEvent {
                    id,
                    track: a(0),
                    position: a(1),
                    command: a(2),
                    byte_one: a(3),
                    byte_two: a(4),
                    ..Default::default()
                });
                self.inactive -= 1;
                self.next_id += 1;
                id
            }
            Remove => {
                let selected = self.selected;
                let selector = a(0);
                let before = self.events.len();
                self.events
                    .retain(|e| !matches_event(e, selector, selected));
                self.inactive += before - self.events.len();
                if self
                    .selected
                    .is_some_and(|id| !self.events.iter().any(|e| e.id == id))
                {
                    self.selected = None;
                }
                0
            }
            GetMark => self
                .find(a(0))
                .map_or(0, |e| i32::from(e.marks & a(1) != 0)),
            SetMark => {
                if !(0..=1).contains(&a(2)) || a(1) & !1023 != 0 {
                    return Err(Error::InvalidInput);
                }
                let selector = a(0);
                let selected = self.selected;
                for e in &mut self.events {
                    if matches_event(e, selector, selected) {
                        if a(2) == 1 {
                            e.marks |= a(1);
                        } else {
                            e.marks &= !a(1);
                        }
                    }
                }
                0
            }
            ExportCount => {
                let count = usize::try_from(a(0)).map_err(|_| Error::InvalidInput)?;
                if !(1..=512).contains(&count) {
                    return Err(Error::InvalidInput);
                }
                if count > self.exports.capacity() {
                    return Err(Error::Capacity);
                }
                self.exports.resize(count, MidiExportArea::default());
                0
            }
            CopyExportArea => {
                let index = usize::try_from(a(0)).map_err(|_| Error::InvalidInput)?;
                let edit = *self.exports.first().ok_or(Error::InvalidInput)?;
                *self.exports.get_mut(index).ok_or(Error::InvalidInput)? = edit;
                0
            }
            ExportArea => {
                let first_pos = self.events.iter().map(|e| e.position).min().unwrap_or(0);
                let last_pos = self
                    .events
                    .iter()
                    .map(|e| e.position.saturating_add(e.length))
                    .max()
                    .unwrap_or(0);
                let first_track = self
                    .events
                    .iter()
                    .map(|e| e.track)
                    .min()
                    .unwrap_or(0)
                    .min(0);
                let last_track = self
                    .events
                    .iter()
                    .map(|e| e.track)
                    .max()
                    .unwrap_or(0)
                    .max(self.tracks.saturating_sub(1));
                let range = |start: i32, end: i32, first: i32, last: i32| {
                    let (s, e) = (
                        if start == -1 { first } else { start },
                        if end == -1 { last } else { end },
                    );
                    (s.min(e), s.max(e))
                };
                let (start_position, end_position) = range(a(0), a(1), first_pos, last_pos);
                let (start_track, end_track) = range(a(2), a(3), first_track, last_track);
                let area = MidiExportArea {
                    name: text.unwrap_or_default(),
                    start_position,
                    end_position,
                    start_track,
                    end_track,
                };
                *self.exports.first_mut().ok_or(Error::InvalidInput)? = area;
                self.events
                    .iter()
                    .filter(|e| {
                        e.position >= start_position
                            && e.position <= end_position
                            && e.track >= start_track
                            && e.track <= end_track
                    })
                    .find(|e| !e.valid())
                    .map_or(0, |e| e.id)
            }
            Reset => {
                self.events.clear();
                self.inactive = 0;
                self.selected = None;
                self.filename.clear();
                self.tracks = 0;
                0
            }
            InsertFile | SaveFile => return Err(Error::InvalidInput),
        };
        Ok(value)
    }
}
fn matches_event(e: &MidiObjectEvent, selector: i32, selected: Option<i32>) -> bool {
    match selector {
        MIDI_ALL_EVENTS => true,
        MIDI_CURRENT_EVENT => Some(e.id) == selected,
        n if n >= 0 && n & MIDI_TRACK_FLAG != 0 => e.track == n & !MIDI_TRACK_FLAG,
        n if n >= 0 && n & MIDI_MARKS_FLAG != 0 => e.marks & (n & !MIDI_MARKS_FLAG) != 0,
        n => e.id == n,
    }
}

impl Prepared {
    pub fn with_midi_object(mut self, mut object: MidiObject) -> Self {
        if self
            .programs
            .iter()
            .flat_map(|p| p.code.iter())
            .any(|i| matches!(i, crate::Instruction::Op(crate::Op::Midi { .. })))
        {
            object.prepare_live();
        }
        self.midi_object = object;
        self
    }
}

/// Control-thread decoded file; the payload returns through ControlReply for disposal.
#[derive(Debug)]
pub struct MidiCompletion {
    pub job: i32,
    pub instance: crate::ScriptInstanceId,
    pub success: bool,
    pub events: Vec<MidiObjectEvent>,
    pub filename: Text,
    pub division: u16,
    pub tracks: i32,
    pub export: MidiExportArea,
    pub path: Text,
    ranges: Vec<(i32, i32, i32)>,
}
impl MidiCompletion {
    pub fn empty(job: i32, instance: crate::ScriptInstanceId) -> Self {
        Self {
            job,
            instance,
            success: true,
            events: Vec::new(),
            filename: Text::default(),
            division: 960,
            tracks: 0,
            export: MidiExportArea::default(),
            path: Text::default(),
            ranges: Vec::new(),
        }
    }
    pub fn capture(job: i32, instance: crate::ScriptInstanceId) -> Self {
        let mut result = Self::empty(job, instance);
        result.events = Vec::with_capacity(MIDI_MAX_EVENTS);
        result
    }
    pub fn read_file(&mut self, path: &str) {
        self.success = false;
        // ponytail: bound control-thread input to 256 MiB; stream larger SMF files if a library needs them.
        if std::fs::metadata(path).is_ok_and(|m| m.len() <= 256 * 1024 * 1024) {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok((events, division, tracks)) = read_smf(&bytes) {
                    let mut ranges = std::collections::BTreeMap::<i32, (i32, i32)>::new();
                    for e in &events {
                        let r = ranges
                            .entry(e.track)
                            .or_insert((e.position, e.position.saturating_add(e.length)));
                        r.0 = r.0.min(e.position);
                        r.1 = r.1.max(e.position.saturating_add(e.length));
                    }
                    self.ranges = ranges
                        .into_iter()
                        .map(|(track, (first, last))| (track, first, last))
                        .collect();
                    self.events = events;
                    self.division = division;
                    self.tracks = tracks;
                    self.filename = Text::new(path.rsplit(['/', '\\']).next().unwrap_or_default());
                    self.success = true;
                }
            }
        }
    }
    pub fn save_file(&self, path: &str) -> Result<(), Error> {
        let bytes = write_smf(&self.events, self.division, self.tracks, self.export)?;
        use std::io::Write;
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::path::Path::new(path);
        let parent = path.parent().ok_or(Error::InvalidInput)?;
        let temporary = parent.join(format!(
            ".kontra-midi-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| Error::InvalidInput)?;
        let result = file
            .write_all(&bytes)
            .and_then(|_| file.sync_all())
            .and_then(|_| {
                drop(file);
                std::fs::rename(&temporary, path)
            });
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|_| Error::InvalidInput)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MidiJob {
    pub id: i32,
    pub instance: crate::ScriptInstanceId,
    pub action: MidiAction,
    pub args: [i32; 5],
    pub initial: bool,
    pub completed: Option<bool>,
    pub text: Option<Text>,
}
impl MidiObject {
    /// Shared MIDI/NKA ID domain. Exhaustion rejects rather than reusing an ID.
    pub fn allocate_async_id(&mut self) -> Result<i32, Error> {
        self.next_job = self.next_job.checked_add(1).ok_or(Error::Capacity)?;
        Ok(self.next_job)
    }
    pub fn queue_initial(
        &mut self,
        slot: u8,
        action: MidiAction,
        args: [i32; 5],
        text: Option<Text>,
    ) -> Result<i32, Error> {
        if !action.asynchronous() {
            return Err(Error::InvalidInput);
        }
        self.prepare_live();
        if self.jobs.len() == self.jobs.capacity() {
            return Err(Error::Capacity);
        }
        let id = self.next_job.checked_add(1).ok_or(Error::Capacity)?;
        self.jobs.push(MidiJob {
            id,
            instance: crate::ScriptInstanceId(u16::from(slot)),
            action,
            args,
            initial: true,
            completed: None,
            text,
        });
        self.next_job = id;
        Ok(id)
    }
    pub fn finish_initial(&mut self, slot: u8, id: i32, defer_callback: bool) -> Option<i32> {
        let index = self.jobs.iter().position(|j| {
            j.initial && j.completed.is_none() && j.id == id && j.instance.0 == u16::from(slot)
        })?;
        let job = self.jobs[index];
        let result = match job.action {
            MidiAction::InsertFile => self.insert_file(
                job.text.unwrap_or_default().as_str(),
                [job.args[0], job.args[1], job.args[2]],
            ),
            MidiAction::SaveFile => {
                let mut output = MidiCompletion::empty(id, job.instance);
                output.events = self.events.clone();
                output.division = self.division;
                output.tracks = self.tracks;
                output.export = self.exports.first().copied().unwrap_or_default();
                output.save_file(job.text.unwrap_or_default().as_str())
            }
            _ => self.apply(job.action, &job.args, None).map(|_| ()),
        };
        let success = result.is_ok();
        if defer_callback {
            self.jobs[index].completed = Some(success);
        } else {
            self.jobs.remove(index);
        }
        Some(i32::from(success))
    }
    pub fn bind_initial_instances(
        &mut self,
        slots: &[(u8, crate::ScriptInstanceId)],
    ) -> Result<(), Error> {
        for job in self.jobs.iter_mut().filter(|j| j.initial) {
            job.instance = slots
                .iter()
                .find(|(slot, _)| u16::from(*slot) == job.instance.0)
                .map(|(_, instance)| *instance)
                .ok_or(Error::InvalidInput)?;
        }
        Ok(())
    }
    pub fn insert_file(&mut self, path: &str, args: [i32; 3]) -> Result<(), Error> {
        let mut file = MidiCompletion::empty(0, crate::ScriptInstanceId(0));
        file.read_file(path);
        if !file.success {
            return Err(Error::InvalidInput);
        }
        self.prepare_live();
        self.import(&file, args)
    }
    pub(crate) fn import(&mut self, file: &MidiCompletion, args: [i32; 3]) -> Result<(), Error> {
        let [track_offset, position_offset, mode] = args;
        if !(0..=2).contains(&mode)
            || track_offset < 0
            || position_offset < 0
            || file.events.iter().any(|e| {
                e.track < 0
                    || e.position < 0
                    || e.length < 0
                    || e.track.checked_add(track_offset).is_none()
                    || e.position.checked_add(position_offset).is_none()
            })
        {
            return Err(Error::InvalidInput);
        }
        let keep = |e: &MidiObjectEvent| {
            mode != 0
                && (mode == 2
                    || file
                        .ranges
                        .binary_search_by_key(&(e.track - track_offset), |r| r.0)
                        .ok()
                        .is_none_or(|i| {
                            let (_, first, last) = file.ranges[i];
                            e.position < first.saturating_add(position_offset)
                                || e.position > last.saturating_add(position_offset)
                        }))
        };
        let retained = self.events.iter().filter(|e| keep(e)).count();
        if retained + file.events.len() + self.inactive > MIDI_MAX_EVENTS
            || retained + file.events.len() > self.events.capacity()
            || self
                .next_id
                .checked_add(file.events.len() as i32)
                .is_none_or(|v| v >= MIDI_MARKS_FLAG)
        {
            return Err(Error::Capacity);
        }
        self.events.retain(keep);
        for e in &file.events {
            self.events.push(MidiObjectEvent {
                id: self.next_id,
                track: e.track + track_offset,
                position: e.position + position_offset,
                ..*e
            });
            self.next_id += 1;
        }
        self.selected = None;
        self.filename = file.filename;
        self.division = file.division;
        self.tracks = if mode == 0 {
            file.tracks.saturating_add(track_offset)
        } else {
            self.tracks.max(file.tracks.saturating_add(track_offset))
        };
        Ok(())
    }
}

fn read_smf(bytes: &[u8]) -> Result<(Vec<MidiObjectEvent>, u16, i32), Error> {
    fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8], Error> {
        let end = at.checked_add(count).ok_or(Error::InvalidInput)?;
        let slice = bytes.get(*at..end).ok_or(Error::InvalidInput)?;
        *at = end;
        Ok(slice)
    }
    fn vlq(bytes: &[u8], at: &mut usize) -> Result<u32, Error> {
        let mut value = 0;
        for _ in 0..4 {
            let byte = take(bytes, at, 1)?[0];
            value = value << 7 | u32::from(byte & 127);
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(Error::InvalidInput)
    }
    let mut at = 0;
    if take(bytes, &mut at, 4)? != b"MThd" {
        return Err(Error::InvalidInput);
    }
    let len = u32::from_be_bytes(take(bytes, &mut at, 4)?.try_into().unwrap()) as usize;
    let header = take(bytes, &mut at, len)?;
    if header.len() < 6 {
        return Err(Error::InvalidInput);
    }
    let format = u16::from_be_bytes([header[0], header[1]]);
    let tracks = u16::from_be_bytes([header[2], header[3]]);
    let division = u16::from_be_bytes([header[4], header[5]]);
    if format > 1
        || tracks == 0
        || (format == 0 && tracks != 1)
        || division == 0
        || division & 0x8000 != 0
    {
        return Err(Error::InvalidInput);
    }
    let mut events: Vec<MidiObjectEvent> = Vec::new();
    for track in 0..tracks {
        if take(bytes, &mut at, 4)? != b"MTrk" {
            return Err(Error::InvalidInput);
        }
        let len = u32::from_be_bytes(take(bytes, &mut at, 4)?.try_into().unwrap()) as usize;
        let data = take(bytes, &mut at, len)?;
        let (mut pos, mut tick, mut running) = (0, 0u32, 0u8);
        let mut held = vec![std::collections::VecDeque::<usize>::new(); 16 * 128];
        while pos < data.len() {
            tick = tick
                .checked_add(vlq(data, &mut pos)?)
                .filter(|v| *v <= i32::MAX as u32)
                .ok_or(Error::InvalidInput)?;
            let first = *data.get(pos).ok_or(Error::InvalidInput)?;
            let status = if first & 128 != 0 {
                pos += 1;
                first
            } else {
                running
            };
            match status {
                0x80..=0xef => {
                    running = status;
                    let command = status & 0xf0;
                    let channel = status & 15;
                    let count = if matches!(command, 0xc0 | 0xd0) { 1 } else { 2 };
                    let b = take(data, &mut pos, count)?;
                    if b.iter().any(|v| *v >= 128) {
                        return Err(Error::InvalidInput);
                    }
                    let (one, two) = (b[0], b.get(1).copied().unwrap_or(0));
                    let key = usize::from(channel) * 128 + usize::from(one);
                    if command == 0x80 || command == 0x90 && two == 0 {
                        if let Some(index) = held[key].pop_front() {
                            events[index].length = tick as i32 - events[index].position;
                        }
                    } else {
                        if events.len() == MIDI_MAX_EVENTS {
                            return Err(Error::Capacity);
                        }
                        let index = events.len();
                        events.push(MidiObjectEvent {
                            id: index as i32 + 1,
                            track: i32::from(track),
                            position: tick as i32,
                            command: i32::from(command),
                            channel: i32::from(channel),
                            byte_one: i32::from(one),
                            byte_two: i32::from(two),
                            ..Default::default()
                        });
                        if command == 0x90 {
                            held[key].push_back(index);
                        }
                    }
                }
                0xff => {
                    running = 0;
                    take(data, &mut pos, 1)?;
                    let count = vlq(data, &mut pos)? as usize;
                    take(data, &mut pos, count)?;
                }
                0xf0 | 0xf7 => {
                    running = 0;
                    let count = vlq(data, &mut pos)? as usize;
                    take(data, &mut pos, count)?;
                }
                _ => return Err(Error::InvalidInput),
            }
        }
    }
    Ok((events, division, i32::from(tracks)))
}
fn write_smf(
    events: &[MidiObjectEvent],
    division: u16,
    source_tracks: i32,
    area: MidiExportArea,
) -> Result<Vec<u8>, Error> {
    fn vlq(out: &mut Vec<u8>, mut n: u32) {
        let mut b = [0; 4];
        let mut i = 3;
        b[i] = (n & 127) as u8;
        while {
            n >>= 7;
            n != 0
        } {
            i -= 1;
            b[i] = (n & 127) as u8 | 128;
        }
        out.extend_from_slice(&b[i..]);
    }
    let first_track = if area.start_track < 0 {
        0
    } else {
        area.start_track
    };
    let last_track = if area.end_track < 0 {
        events
            .iter()
            .map(|e| e.track)
            .max()
            .unwrap_or(0)
            .max(source_tracks.saturating_sub(1))
    } else {
        area.end_track
    };
    let tracks = last_track
        .checked_sub(first_track)
        .and_then(|v| v.checked_add(1))
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(Error::InvalidInput)?;
    if tracks == 0 || tracks > 65535 || division == 0 || division & 0x8000 != 0 {
        return Err(Error::InvalidInput);
    }
    let mut output = b"MThd\0\0\0\x06\0\x01".to_vec();
    output.extend_from_slice(&(tracks as u16).to_be_bytes());
    output.extend_from_slice(&division.to_be_bytes());
    let start = area.start_position.max(0);
    for track in first_track..=last_track {
        let mut messages = Vec::new();
        for e in events.iter().filter(|e| {
            e.track == track
                && e.position >= start
                && (area.end_position < 0 || e.position <= area.end_position)
        }) {
            if !e.valid() {
                return Err(Error::InvalidInput);
            }
            messages.push((
                e.position - start,
                e.id,
                [
                    e.command as u8 | e.channel as u8,
                    e.byte_one as u8,
                    e.byte_two as u8,
                ],
            ));
            if e.command == 0x90 {
                messages.push((
                    e.position
                        .checked_add(e.length)
                        .ok_or(Error::InvalidInput)?
                        - start,
                    e.id,
                    [0x80 | e.channel as u8, e.byte_one as u8, 0],
                ));
            }
        }
        messages.sort_by_key(|m| (m.0, m.2[0] & 0xf0 != 0x80, m.1));
        let mut data = Vec::new();
        let mut last = 0;
        for (pos, _, bytes) in messages {
            let delta = (pos - last) as u32;
            if delta > 0x0fff_ffff {
                return Err(Error::InvalidInput);
            }
            vlq(&mut data, delta);
            data.extend_from_slice(
                &bytes[..if matches!(bytes[0] & 0xf0, 0xc0 | 0xd0) {
                    2
                } else {
                    3
                }],
            );
            last = pos;
        }
        data.extend_from_slice(&[0, 0xff, 0x2f, 0]);
        output.extend_from_slice(b"MTrk");
        output.extend_from_slice(&(data.len() as u32).to_be_bytes());
        output.extend(data);
    }
    Ok(output)
}

impl crate::Runtime {
    pub(super) fn start_midi_jobs(&mut self, plan: crate::PlanId) {
        let Some(generation) = self.plans.get_mut(plan.0) else {
            return;
        };
        for job in &mut generation.midi_object.jobs {
            if !job.initial {
                continue;
            }
            if self.ops.effects.len() == crate::ops::EFFECT_CAPACITY {
                break;
            }
            let kind = match (job.completed, job.action) {
                (Some(_), _) => 5, // Already applied off audio; notify the callback only.
                (None, MidiAction::SetBufferSize) => 1,
                (None, MidiAction::Reset) => 2,
                (None, MidiAction::InsertFile) => 3,
                (None, MidiAction::SaveFile) => 4,
                _ => continue,
            };
            self.ops.effects.push_back(crate::Effect {
                plan,
                instance: Some(job.instance),
                service: MIDI_SERVICE,
                args: [
                    kind,
                    i64::from(job.id),
                    i64::from(job.args[0]),
                    i64::from(job.args[1]),
                    i64::from(job.args[2]),
                    0,
                ],
                count: 5,
                text: job.text,
            });
            job.initial = false;
            generation.callbacks += 1;
        }
    }

    pub(super) fn request_midi(
        &mut self,
        id: crate::BehaviorId,
        plan: crate::PlanId,
        action: MidiAction,
        args: [i32; 5],
        text: Option<Text>,
    ) -> Result<i32, Error> {
        if self.ops.effects.len() == crate::ops::EFFECT_CAPACITY {
            return Err(Error::Capacity);
        }
        let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[c.program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        if generation.midi_object.jobs.len() == generation.midi_object.jobs.capacity() {
            return Err(Error::Capacity);
        }
        let job = generation
            .midi_object
            .next_job
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let callbacks = generation.callbacks.checked_add(1).ok_or(Error::Capacity)?;
        generation.midi_object.jobs.push(MidiJob {
            id: job,
            instance,
            action,
            args,
            initial: false,
            completed: None,
            text,
        });
        generation.midi_object.next_job = job;
        generation.callbacks = callbacks;
        let kind = match action {
            MidiAction::SetBufferSize => 1,
            MidiAction::Reset => 2,
            MidiAction::InsertFile => 3,
            MidiAction::SaveFile => 4,
            _ => return Err(Error::InvalidInput),
        };
        self.ops.effects.push_back(crate::Effect {
            plan,
            instance: Some(instance),
            service: MIDI_SERVICE,
            args: [
                kind,
                i64::from(job),
                i64::from(args[0]),
                i64::from(args[1]),
                i64::from(args[2]),
                0,
            ],
            count: 5,
            text,
        });
        Ok(job)
    }
    pub fn capture_midi(
        &self,
        plan: crate::PlanId,
        output: &mut MidiCompletion,
    ) -> Result<(), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let object = &generation.midi_object;
        if !object.jobs.iter().any(|j| {
            !j.initial
                && j.id == output.job
                && j.instance == output.instance
                && j.action == MidiAction::SaveFile
        }) {
            return Err(Error::StaleHandle);
        }
        if output.events.capacity() < object.events.len() {
            return Err(Error::Capacity);
        }
        output.events.clear();
        output.events.extend_from_slice(&object.events);
        output.division = object.division;
        output.tracks = object.tracks;
        output.export = object.exports.first().copied().unwrap_or_default();
        Ok(())
    }
    pub fn complete_midi(
        &mut self,
        plan: crate::PlanId,
        output: &mut MidiCompletion,
    ) -> Result<(), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let index = generation
            .midi_object
            .jobs
            .iter()
            .position(|j| !j.initial && j.id == output.job && j.instance == output.instance)
            .ok_or(Error::StaleHandle)?;
        let job = generation.midi_object.jobs[index];
        let callback = self.preflight_async_callback(plan, job.instance)?;
        let generation = self.plans.get_mut(plan.0).unwrap();
        if let Some(success) = job.completed {
            output.success = success;
        } else if output.success {
            let result = if job.action == MidiAction::InsertFile {
                generation
                    .midi_object
                    .import(output, [job.args[0], job.args[1], job.args[2]])
            } else if job.action == MidiAction::SaveFile {
                Ok(())
            } else {
                generation
                    .midi_object
                    .apply(job.action, &job.args, None)
                    .map(|_| ())
            };
            output.success = result.is_ok();
        }
        generation.midi_object.jobs.remove(index);
        generation.callbacks -= 1;
        self.finish_async_callback(plan, output.job, output.success, callback)
    }

    pub(super) fn preflight_async_callback(
        &mut self,
        plan: crate::PlanId,
        instance: crate::ScriptInstanceId,
    ) -> Result<Option<(crate::SignalProgram, crate::behavior::PlanContext)>, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let callback = generation
            .prepared
            .signal_programs
            .iter()
            .find(|p| {
                p.signal == MIDI_ASYNC_SIGNAL
                    && generation.prepared.programs[p.program].script_instance == Some(instance)
            })
            .copied();
        let Some(p) = callback else {
            return Ok(None);
        };
        let context = if self.performance_state.current.is_empty() {
            crate::behavior::PlanContext::Bare
        } else {
            crate::behavior::PlanContext::Control(crate::control::ControlEvent {
                performance: 0,
                origin: crate::ChannelAddress {
                    protocol: crate::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                },
                channels: 1,
                stage: p.stage,
                interaction: crate::WidgetInteraction::default(),
            })
        };
        self.validate_plan_context(plan, p.program, context)?;
        Ok(Some((p, context)))
    }
    pub(super) fn finish_async_callback(
        &mut self,
        plan: crate::PlanId,
        job: i32,
        success: bool,
        callback: Option<(crate::SignalProgram, crate::behavior::PlanContext)>,
    ) -> Result<(), Error> {
        if let Some((p, context)) = callback {
            let id = self.admit_plan_context(plan, p.program, context)?;
            self.behaviors.get_mut(id.0).unwrap().async_result = Some((job, i32::from(success)));
            self.resume_behavior(id);
        }
        for index in 0..self.behaviors.slots.len() {
            let Some(c) = self.behaviors.slots[index].value else {
                continue;
            };
            if c.outcome.is_none()
                && c.async_wait == Some(job)
                && self.behavior_plan(c.owner) == Ok(plan)
            {
                let id = crate::BehaviorId(self.behaviors.id(index));
                self.resume_behavior(id);
            }
        }
        Ok(())
    }
}
