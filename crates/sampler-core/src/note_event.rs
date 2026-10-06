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
    pub pending_callbacks: usize,
    pub pending_releases: usize,
    pub release_start: usize,
    pub release_routed: bool,
    pub release_queued: bool,
    pub entry: usize,
    pub routed: bool,
    /// The generated event was admitted with a positive/fixed duration policy.
    pub fixed_duration: bool,
    pub source_offset_micros: u32,
    source_id: Option<i32>,
}

impl NoteEvent {
    pub(super) fn new(pitch: NotePitch, velocity: f64) -> Self {
        let initial = NoteProperties { pitch, velocity };
        Self {
            initial,
            pending_callbacks: 0,
            pending_releases: 0,
            release_start: 0,
            release_routed: false,
            release_queued: false,
            entry: 0,
            routed: false,
            fixed_duration: false,
            source_offset_micros: 0,
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

    /// The sustain pedal (CC64) holds no gates; its value still reaches
    /// controllers and behaviors, which implement sustain themselves
    /// (Kontakt's `NO_SYS_SCRIPT_PEDAL`). Sostenuto is unaffected.
    pub fn with_script_sustain(mut self, script: bool) -> Self {
        self.script_sustain = script;
        self
    }

    /// Release-trigger families never fire on note release; behaviors that
    /// own release samples play them (Kontakt's `NO_SYS_SCRIPT_RLS_TRIG`).
    pub fn with_script_release_triggers(mut self, script: bool) -> Self {
        self.script_release_triggers = script;
        self
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

    /// Properties in the input/creating module; late edits do not change committed audio.
    pub fn note_event(&self, note: NoteId) -> Result<NoteProperties, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        self.note_event_at(note, self.note_events[note.0.index].entry)?
            .ok_or(Error::InvalidInput)
    }

    /// Edit the event view atomically. A pending attack consumes this view when
    /// forwarded. Already committed voices/releases and physical pairing stay intact.
    pub fn edit_note_event(&mut self, note: NoteId, value: NoteProperties) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        self.edit_note_event_at(note, self.note_events[note.0.index].entry, value)
    }

    /// Read one reached module projection; None means this event never entered it.
    /// The index equal to the prepared stage count is the final native boundary.
    pub fn note_event_at(
        &self,
        note: NoteId,
        stage: usize,
    ) -> Result<Option<NoteProperties>, Error> {
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        Ok(self
            .plans
            .get(plan.0)
            .unwrap()
            .projections
            .get(note.0.index, stage)?
            .properties)
    }

    /// Edit only a reached module view. Forwarded downstream copies stay unchanged.
    pub fn edit_note_event_at(
        &mut self,
        note: NoteId,
        stage: usize,
        value: NoteProperties,
    ) -> Result<(), Error> {
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        if !value.pitch.valid()
            || !value.velocity.is_finite()
            || !(0.0..=1.0).contains(&value.velocity)
        {
            return Err(Error::InvalidInput);
        }
        let cell = self
            .plans
            .get_mut(plan.0)
            .unwrap()
            .projections
            .get_mut(note.0.index, stage)?;
        if cell.properties.is_none() {
            return Err(Error::InvalidInput);
        }
        cell.properties = Some(value);
        Ok(())
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ReleaseStage {
    #[default]
    Unreached,
    Pending,
    Suppressed,
    Forwarded,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Projection {
    pub properties: Option<NoteProperties>,
    pub forwarded: bool,
    pub release: ReleaseStage,
    pub release_reserved: bool,
}

pub(super) struct NoteProjections {
    stages: usize,
    cells: Box<[Projection]>,
}

impl NoteProjections {
    pub fn new(stages: usize, notes: usize) -> Result<Self, Error> {
        let stages = stages.checked_add(1).ok_or(Error::Capacity)?;
        let count = stages.checked_mul(notes).ok_or(Error::Capacity)?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(count)
            .map_err(|_| Error::Capacity)?;
        cells.resize(count, Projection::default());
        Ok(Self {
            stages,
            cells: cells.into_boxed_slice(),
        })
    }
    pub fn get(&self, note: usize, stage: usize) -> Result<&Projection, Error> {
        if stage >= self.stages {
            return Err(Error::InvalidInput);
        }
        Ok(&self.cells[note * self.stages + stage])
    }
    pub fn get_mut(&mut self, note: usize, stage: usize) -> Result<&mut Projection, Error> {
        if stage >= self.stages {
            return Err(Error::InvalidInput);
        }
        Ok(&mut self.cells[note * self.stages + stage])
    }
    pub fn admit(&mut self, note: usize, entry: usize, properties: NoteProperties) {
        let start = note * self.stages;
        self.cells[start..start + self.stages].fill(Projection::default());
        self.cells[start + entry].properties = Some(properties);
    }
    pub fn forward(&mut self, note: usize, from: usize, through: usize) {
        let properties = self.get(note, from).unwrap().properties;
        for stage in from..=through {
            let cell = self.get_mut(note, stage).unwrap();
            cell.properties = properties;
            cell.forwarded = stage != through;
        }
    }
}
