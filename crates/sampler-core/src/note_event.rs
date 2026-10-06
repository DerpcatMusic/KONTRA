//! Script-visible note properties, independent of physical input and committed audio.
use super::{Error, NoteId, NotePitch, PlanId, Prepared, Runtime};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteProperties {
    pub pitch: NotePitch,
    pub velocity: f64,
}

#[derive(Clone, Copy)]
pub(super) struct NoteEvent {
    pub initial: NoteProperties,
    pub current: NoteProperties,
    /// The generated event was admitted with a positive/fixed duration policy.
    pub fixed_duration: bool,
    source_id: Option<i32>,
}

impl NoteEvent {
    pub(super) fn new(pitch: NotePitch, velocity: f64) -> Self {
        let initial = NoteProperties { pitch, velocity };
        Self {
            initial,
            current: initial,
            fixed_duration: false,
            source_id: None,
        }
    }
}

impl Prepared {
    /// Bound positive source aliases for a frontend's reserved integer namespace.
    /// Native generational handles are unaffected. The default is i32::MAX.
    pub fn with_source_event_limit(mut self, maximum: i32) -> Result<Self, Error> {
        if maximum <= 0 {
            return Err(Error::InvalidInput);
        }
        self.source_event_limit = maximum;
        Ok(self)
    }
}

impl Runtime {
    /// Export a positive signed-32 source identity, distinct from host IDs and
    /// native slot indices. Repeated exports return the same identity. IDs never
    /// wrap or repeat within a runtime, including across panic and plan changes.
    pub fn source_event_id(&mut self, note: NoteId) -> Result<i32, Error> {
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        if let Some(id) = self.note_events[note.0.index].source_id {
            return Ok(id);
        }
        let id = self.reserve_source_id(plan)?;
        self.publish_source_id(note, id)?;
        Ok(id)
    }

    /// Resolve a source identity only in its originating prepared generation.
    /// A retired note or an unknown integer resolves to None; a stale/foreign
    /// plan is an error. Aliases do not pin notes or extend their lifetimes.
    pub fn resolve_source_event(&self, plan: PlanId, id: i32) -> Result<Option<NoteId>, Error> {
        self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let Ok(index) = self.source_ids.binary_search_by_key(&id, |&(id, _)| id) else {
            return Ok(None);
        };
        let note = self.source_ids[index].1;
        Ok(self
            .notes
            .get(note.0)
            .filter(|n| n.plan == plan)
            .map(|_| note))
    }

    pub(super) fn reserve_source_id(&mut self, plan: PlanId) -> Result<i32, Error> {
        let maximum = self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .source_event_limit;
        let next = self
            .last_source_id
            .checked_add(1)
            .filter(|&id| id <= maximum)
            .ok_or(Error::Capacity)?;
        self.last_source_id = next;
        Ok(self.last_source_id)
    }

    pub(super) fn publish_source_id(&mut self, note: NoteId, id: i32) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if id <= 0 || self.note_events[note.0.index].source_id.is_some() {
            return Err(Error::InvalidInput);
        }
        if self.source_ids.len() == self.source_ids.capacity() {
            // ponytail: full indexes compact O(note capacity); replace with a
            // bounded hash index if churn benchmarks make admission cost material.
            let notes = &self.notes;
            self.source_ids
                .retain(|&(_, note)| notes.get(note.0).is_some());
        }
        // One alias per live note and capacity >= note slots guarantees room.
        // Still guard push so a future invariant regression cannot allocate here.
        if self.source_ids.len() == self.source_ids.capacity() {
            return Err(Error::Capacity);
        }
        let at = self.source_ids.partition_point(|&(value, _)| value < id);
        self.source_ids.insert(at, (id, note));
        self.note_events[note.0.index].source_id = Some(id);
        Ok(())
    }

    /// Admission properties, including full-resolution velocity and absolute pitch.
    /// Generated notes capture their own admission, not their parent's input.
    pub fn initial_note_properties(&self, note: NoteId) -> Result<NoteProperties, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        Ok(self.note_events[note.0.index].initial)
    }

    /// Current script-visible properties; late edits do not change committed audio.
    pub fn note_event(&self, note: NoteId) -> Result<NoteProperties, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        Ok(self.note_events[note.0.index].current)
    }

    /// Edit the event view atomically. A pending attack consumes this view when
    /// forwarded. Already committed voices/releases and physical pairing stay intact.
    pub fn edit_note_event(&mut self, note: NoteId, value: NoteProperties) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if !value.pitch.valid()
            || !value.velocity.is_finite()
            || !(0.0..=1.0).contains(&value.velocity)
        {
            return Err(Error::InvalidInput);
        }
        self.note_events[note.0.index].current = value;
        Ok(())
    }
}
