//! Musical routing domains are independent of physical/expressive MIDI channels.
use crate::{Error, Event, NoteId, Runtime, RuntimeId, Trigger};

/// A fixed performance domain, valid only in its originating runtime. Domains are
/// budgeted at construction and cannot be recycled while notes refer to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerformanceId {
    runtime: RuntimeId,
    index: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionPolicy {
    #[default]
    Onset,
    Current,
}

/// Physical input key consumed by a native latched articulation switch.
#[derive(Clone, Copy, Debug)]
pub struct Keyswitch {
    pub key: u8,
    pub articulation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSnapshot {
    pub performance: PerformanceId,
    pub articulation: u32,
    pub consumed_switch: bool,
}

#[derive(Clone, Copy, Default)]
pub(super) struct NoteSelection {
    pub performance: usize,
    pub articulation: u32,
    pub consumed_switch: bool,
}

impl Runtime {
    /// Domain zero is the default for trigger/note_on and existing MIDI ingress.
    pub fn performance(&self, index: usize) -> Result<PerformanceId, Error> {
        if index >= self.articulations.len() {
            return Err(Error::InvalidInput);
        }
        Ok(PerformanceId {
            runtime: self.id(),
            index,
        })
    }

    pub(super) fn performance_index(&self, id: PerformanceId) -> Result<usize, Error> {
        if id.runtime != self.id() || id.index >= self.articulations.len() {
            return Err(Error::StaleHandle);
        }
        Ok(id.index)
    }

    pub fn articulation(&self, id: PerformanceId) -> Result<u32, Error> {
        Ok(self.articulations[self.performance_index(id)?])
    }

    pub fn set_articulation(&mut self, id: PerformanceId, value: u32) -> Result<(), Error> {
        self.schedule_event(self.now, Event::Articulation(id, value))
    }

    pub fn note_selection(&self, note: NoteId) -> Result<SelectionSnapshot, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let state = self.selections[note.0.index];
        Ok(SelectionSnapshot {
            performance: self.performance(state.performance).unwrap(),
            articulation: state.articulation,
            consumed_switch: state.consumed_switch,
        })
    }

    pub(super) fn release_articulation(&self, note: NoteId, trigger: Trigger) -> u32 {
        let n = self.notes.get(note.0).unwrap();
        let state = self.selections[note.0.index];
        match self.plans.get(n.plan.0).unwrap().prepared.release_selection
            [trigger.release_index().unwrap()]
        {
            SelectionPolicy::Onset => state.articulation,
            SelectionPolicy::Current => self.articulations[state.performance],
        }
    }
}
