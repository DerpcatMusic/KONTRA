//! Stable note snapshots, owned by the same bounded timeline that refers to them.
use super::{Command, EventId, Expression, GroupMask, HostRef, PerNoteInput, SCRIPT_MOD_SOURCES};
use super::script::Kind;
use crate::ksp::Fade;

pub const MAX_COMMANDS: usize = 256;
// Pedal-up and terminal fades have their own quota, never the note-off reserve.
const MAX_STOP_COMMANDS: usize = 256;
// A first freeze per existing voice or accepted queued start; separate from releases.
const MAX_FREEZE_COMMANDS: usize = super::MAX_VOICES + MAX_COMMANDS;
/// First-dispatch history has its own bounded quota, without reducing starts.
const MAX_OBSERVE_COMMANDS: usize = MAX_COMMANDS;
// One release per live script event, beyond the ordinary and stop budgets.
pub(super) const COMMAND_CAPACITY: usize = MAX_COMMANDS + MAX_STOP_COMMANDS + MAX_FREEZE_COMMANDS + MAX_OBSERVE_COMMANDS + crate::ksp::EVENT_CAPACITY;

#[derive(Clone, Copy)]
pub(super) struct Handle(u16);

#[derive(Clone, Copy)]
pub(super) struct Start {
    pub host_note: Option<HostRef>,
    pub per_note: Option<PerNoteInput>,
    pub script_mods: [i32; SCRIPT_MOD_SOURCES],
    pub note: u8,
    pub velocity: u8,
    pub onset_velocity: crate::midi::Velocity,
    pub release_velocity: Option<crate::midi::Velocity>,
    pub owner: Option<(u8, u8)>,
    pub counter_stop: Option<u64>,
    pub offset_us: u64,
    pub volume: f32,
    pub tune: f64,
    pub pan: f32,
    pub whole: bool,
    pub groups: GroupMask,
    pub group_start_applied: bool,
    pub native_release: bool,
    pub force_completion: bool,
    pub expression: Option<Expression>,
}

#[derive(Clone, Copy)]
pub(super) struct Release {
    pub host_note: Option<HostRef>,
    pub input_root: bool,
    pub input_sostenuto: bool,
    pub per_note: Option<PerNoteInput>,
    pub script_mods: [i32; SCRIPT_MOD_SOURCES],
    pub trigger: Option<(u8, u8)>,
    pub onset_velocity: crate::midi::Velocity,
    pub release_velocity: Option<crate::midi::Velocity>,
    pub groups: GroupMask,
    pub expression: Option<Expression>,
}

#[derive(Clone, Copy)]
pub(super) enum Note {
    Start(Start),
    Release(Release),
}

impl Note {
    pub fn host_note(&self) -> Option<HostRef> {
        match self { Self::Start(n) => n.host_note, Self::Release(n) => n.host_note }
    }
    pub fn per_note(&self) -> Option<PerNoteInput> {
        match self { Self::Start(n) => n.per_note, Self::Release(n) => n.per_note }
    }
}

pub(super) struct Commands {
    rows: Vec<Command>,
    notes: Vec<Option<Note>>,
    free: Vec<u16>,
}

impl Default for Commands {
    fn default() -> Self {
        Self { rows: Vec::with_capacity(COMMAND_CAPACITY),
            notes: Vec::with_capacity(COMMAND_CAPACITY),
            free: Vec::with_capacity(COMMAND_CAPACITY) }
    }
}

impl Commands {
    pub fn len(&self) -> usize { self.rows.len() }
    pub fn iter(&self) -> std::slice::Iter<'_, Command> { self.rows.iter() }
    /// Apply first-dispatch history without exposing mutable timeline rows.
    pub fn apply_observations(&mut self, mut observe: impl FnMut(u8)) {
        for command in &mut self.rows {
            if !command.applied && let Kind::ObserveNote(note) = command.kind {
                observe(note);
                command.applied = true;
            }
        }
    }
    pub fn get(&self, index: usize) -> Option<&Command> { self.rows.get(index) }
    pub fn as_slice(&self) -> &[Command] { &self.rows }
    #[cfg(test)]
    pub fn insert(&mut self, index: usize, command: Command) { self.rows.insert(index, command); }

    pub fn push(&mut self, at: u32, channel: u8, input_channel: Option<u8>, id: EventId, kind: Kind) -> bool {
        // Note rows must be admitted together with their reserved snapshot.
        if matches!(kind, Kind::Start(_) | Kind::Release(_)) { return false; }
        let stop = kind.is_stop();
        if stop {
            // Coalesce identical same-frame stops, without crossing a later
            // fade or opposite pedal state for the same target.
            let prior = self.rows.iter().rev().find(|c| c.at == at && c.channel == channel && c.input_channel == input_channel && match (c.kind, kind) {
                (Kind::Fade(_), Kind::Fade(_)) => c.id == id,
                (Kind::Control { target: a, control: crate::midi::Control::Cc(ca, _), .. }, Kind::Control { target: b, control: crate::midi::Control::Cc(cb, _), .. }) => a == b && ca == cb,
                _ => false,
            });
            if prior.is_some_and(|c| match (c.kind, kind) {
                (Kind::Fade(Fade::Out { duration_us: a, stop: true }), Kind::Fade(Fade::Out { duration_us: b, stop: true })) => a == b,
                (Kind::Control { target: a, control: ca, root_pedal_handled: ra, .. }, Kind::Control { target: b, control: cb, root_pedal_handled: rb, .. }) => a == b && ca == cb && ra == rb,
                _ => false,
            }) { return true }
        }
        // ponytail: bounded quota scans over at most COMMAND_CAPACITY entries;
        // cache counts if dense MIDI profiles make this a hot path.
        let full = self.rows.len() == COMMAND_CAPACITY
            || if stop {
                self.rows.iter().filter(|c| c.kind.is_stop()).count() >= MAX_STOP_COMMANDS
            } else if matches!(kind, Kind::Freeze(_)) {
                self.rows.iter().filter(|c| matches!(c.kind, Kind::Freeze(_))).count() >= MAX_FREEZE_COMMANDS
            } else if matches!(kind, Kind::ObserveNote(_)) {
                self.rows.iter().filter(|c| matches!(c.kind, Kind::ObserveNote(_))).count() >= MAX_OBSERVE_COMMANDS
            } else {
                self.ordinary_full()
            };
        if full {
            return false;
        }
        let i = self.rows.as_slice().partition_point(|c| c.at <= at);
        self.rows.insert(i, Command { at, channel, input_channel, id, kind, applied: false });
        true
    }

    fn ordinary_full(&self) -> bool {
        self.rows.iter()
            .filter(|c| !c.kind.is_stop() && !matches!(c.kind, Kind::Release(_) | Kind::Freeze(_) | Kind::ObserveNote(_)))
            .count() >= MAX_COMMANDS
    }

    // Snapshot reservation and its timeline row cannot escape separately.
    // Applied rows keep their snapshots until clear/retain removes the row.
    pub fn push_note(&mut self, at: u32, channel: u8, input_channel: Option<u8>, id: EventId, note: Note) -> bool {
        // Admission precedes reservation so rejection cannot consume a slot.
        // Releases retain the existing full-timeline budget.
        if matches!(note, Note::Start(_)) && self.ordinary_full()
            || self.rows.len() == COMMAND_CAPACITY { return false; }
        let index = self.rows.partition_point(|command| command.at <= at);
        let Some(kind) = self.reserve_note(note) else { return false; };
        self.rows.insert(index, Command { at, channel, input_channel, id, kind, applied: false });
        true
    }

    fn reserve_note(&mut self, note: Note) -> Option<Kind> {
        let index = if let Some(index) = self.free.pop() {
            self.notes[usize::from(index)] = Some(note);
            index
        } else {
            if self.notes.len() == COMMAND_CAPACITY { return None; }
            let index = self.notes.len() as u16;
            self.notes.push(Some(note));
            index
        };
        Some(match note { Note::Start(_) => Kind::Start(Handle(index)),
            Note::Release(_) => Kind::Release(Handle(index)) })
    }

    fn handle(kind: Kind) -> Option<Handle> {
        match kind { Kind::Start(h) | Kind::Release(h) => Some(h), _ => None }
    }

    pub fn note(&self, kind: Kind) -> Option<&Note> {
        Self::handle(kind).and_then(|h| self.notes.get(usize::from(h.0))).and_then(Option::as_ref)
    }

    pub fn start(&self, command: &Command) -> Option<&Start> {
        match self.note(command.kind) { Some(Note::Start(n)) => Some(n), _ => None }
    }

    pub fn for_each_start_mut(&mut self, mut visit: impl FnMut(&Command, &mut Start)) {
        for command in &self.rows {
            if let Kind::Start(h) = command.kind
                && let Some(Note::Start(note)) = &mut self.notes[usize::from(h.0)] {
                visit(command, note);
            }
        }
    }

    pub fn for_each_release_mut(&mut self, mut visit: impl FnMut(&Command, &mut Release)) {
        for command in &self.rows {
            if let Kind::Release(h) = command.kind
                && let Some(Note::Release(note)) = &mut self.notes[usize::from(h.0)] {
                visit(command, note);
            }
        }
    }

    pub fn apply(&mut self, index: usize, visit: impl FnOnce(&Command, Option<&Note>)) {
        let command = &self.rows[index];
        if !command.applied { visit(command, self.note(command.kind)); }
        self.rows[index].applied = true;
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Command, Option<&Note>) -> bool) {
        let notes = &mut self.notes;
        let free = &mut self.free;
        self.rows.retain(|command| {
            let handle = Self::handle(command.kind);
            let note = handle.and_then(|h| notes.get(usize::from(h.0))).and_then(Option::as_ref);
            if keep(command, note) { return true; }
            if let Some(handle) = handle {
                notes[usize::from(handle.0)] = None;
                free.push(handle.0);
            }
            false
        });
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.notes.clear();
        self.free.clear();
    }

    #[cfg(test)]
    pub fn snapshot_counts(&self) -> [usize; 3] {
        [self.rows.len(), self.notes.iter().filter(|n| n.is_some()).count(), self.free.len()]
    }
}

#[cfg(all(test, feature = "plugin"))]
mod tests {
    use super::*;
    use crate::engine::EventId;

    fn snapshot(id: u32) -> Note {
        let mut script_mods = [0; SCRIPT_MOD_SOURCES];
        script_mods[0] = id as i32;
        script_mods[SCRIPT_MOD_SOURCES - 1] = -(id as i32);
        Note::Release(Release { host_note: None, input_root: true,
            input_sostenuto: id.is_multiple_of(2), per_note: None, script_mods,
            trigger: Some(((id % 128) as u8, 127)), onset_velocity: crate::midi::Velocity::FULL,
            release_velocity: None, groups: GroupMask::all(), expression: None })
    }

    #[test]
    fn full_release_admission_retains_applied_snapshots_and_reuses_only_removed_rows_without_heap() {
        let mut commands = Commands::default();
        assert_eq!(commands.snapshot_counts(), [0, 0, 0]);
        assert!(std::mem::size_of::<Command>() <= 256,
            "a timeline row must not carry either full note snapshot");
        assert_eq!(crate::plugin::tests::allocations(|| {
            // Releases retain the entire preexisting timeline bound, including
            // rows beyond EVENT_CAPACITY; no new release-only quota is invented.
            for id in 0..COMMAND_CAPACITY as u32 {
                assert!(commands.push_note(id, 0, Some(4), EventId(id), snapshot(id)));
            }
            assert_eq!(commands.snapshot_counts(), [COMMAND_CAPACITY, COMMAND_CAPACITY, 0]);
            assert!(!commands.push_note(u32::MAX, 0, Some(4), EventId(u32::MAX), snapshot(u32::MAX)));
            let mut applications = 0;
            commands.apply(0, |_, _| applications += 1);
            commands.apply(0, |_, _| applications += 1);
            assert_eq!(applications, 1);
            assert_eq!(commands.snapshot_counts()[1], COMMAND_CAPACITY);
            commands.retain(|command, _| command.id.0 % 2 == 0);
            assert_eq!(commands.snapshot_counts(), [COMMAND_CAPACITY / 2, COMMAND_CAPACITY / 2, COMMAND_CAPACITY / 2]);
            for index in 0..COMMAND_CAPACITY / 2 {
                let id = (COMMAND_CAPACITY + index) as u32;
                assert!(commands.push_note(id, 7, Some(4), EventId(id), snapshot(id)));
            }
            for command in commands.iter() {
                let Some(Note::Release(release)) = commands.note(command.kind) else { panic!("missing live snapshot"); };
                assert_eq!(release.script_mods[0], command.id.0 as i32);
                assert_eq!(release.script_mods[SCRIPT_MOD_SOURCES - 1], -(command.id.0 as i32));
                assert_eq!(release.input_sostenuto, command.id.0 % 2 == 0);
            }
            assert!(commands.get(0).unwrap().applied);
            commands.clear();
            assert_eq!(commands.snapshot_counts(), [0, 0, 0]);
            // Clear retains prepared capacity and permits every slot again.
            for id in 0..COMMAND_CAPACITY as u32 {
                assert!(commands.push_note(id, 0, None, EventId(id), snapshot(id)));
            }
            commands.clear();
        }), 0);
    }
}
