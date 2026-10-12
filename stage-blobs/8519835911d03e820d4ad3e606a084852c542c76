//! Region membership and generation-owned per-note selection masks.
use crate::{Error, NoteId, Prepared, Runtime};

impl Prepared {
    /// Assign each authored region to an optional dense group. Ungrouped regions
    /// are always eligible. Empty groups are valid; group identity is plan-local.
    pub fn with_groups(mut self, count: u32, regions: Vec<Option<u32>>) -> Result<Self, Error> {
        if regions.len() != self.region_count()
            || regions.iter().flatten().any(|&group| group >= count)
        {
            return Err(Error::InvalidInput);
        }
        self.group_count = count;
        self.region_groups = regions.into_boxed_slice();
        Ok(self)
    }

    pub fn with_source_zones(mut self, ids: Vec<u32>) -> Result<Self, Error> {
        if ids.len() != self.region_count() || ids.contains(&0) {
            return Err(Error::InvalidInput);
        }
        self.region_zone_ids = ids.into_boxed_slice();
        Ok(self)
    }

    pub fn group_count(&self) -> u32 {
        self.group_count
    }
}

#[derive(Clone, Copy)]
pub(super) enum GroupView {
    Note(usize),
    Release(usize),
    Committed,
}

pub(super) struct GroupState {
    words: usize,
    stages: usize,
    // Each module has an editable view; the final audio selection has one snapshot.
    cells: Box<[u64]>,
}

impl GroupState {
    pub fn new(count: u32, notes: usize, stages: usize) -> Result<Self, Error> {
        let words = usize::try_from(count)
            .map_err(|_| Error::Capacity)?
            .div_ceil(64);
        let stages = stages.checked_add(1).ok_or(Error::Capacity)?;
        let len = words
            .checked_mul(
                stages
                    .checked_mul(2)
                    .and_then(|n| n.checked_add(1))
                    .ok_or(Error::Capacity)?,
            )
            .and_then(|n| n.checked_mul(notes))
            .ok_or(Error::Capacity)?;
        let mut cells = Vec::new();
        cells.try_reserve_exact(len).map_err(|_| Error::Capacity)?;
        cells.resize(len, u64::MAX);
        Ok(Self {
            words,
            stages,
            cells: cells.into_boxed_slice(),
        })
    }

    fn range(&self, note: usize, view: GroupView) -> std::ops::Range<usize> {
        let stage = match view {
            GroupView::Note(stage) => stage,
            GroupView::Release(stage) => self.stages + stage,
            GroupView::Committed => self.stages * 2,
        };
        let start = (note * (self.stages * 2 + 1) + stage) * self.words;
        start..start + self.words
    }

    pub fn view(&self, note: usize, view: GroupView) -> &[u64] {
        &self.cells[self.range(note, view)]
    }

    pub fn admit(&mut self, note: usize, parent: Option<usize>) {
        let draft = self.range(note, GroupView::Note(0));
        if let Some(parent) = parent {
            self.cells
                .copy_within(self.range(parent, GroupView::Note(0)), draft.start);
        } else {
            self.cells[draft].fill(u64::MAX);
        }
        self.commit(note);
    }

    pub fn commit(&mut self, note: usize) {
        self.commit_at(note, 0);
    }

    pub fn view_at(&self, note: usize, stage: usize) -> &[u64] {
        &self.cells[self.range(note, GroupView::Note(stage))]
    }

    pub fn inherit(&mut self, note: usize, entry: usize, parent: Option<(usize, GroupView)>) {
        let target = self.range(note, GroupView::Note(entry));
        if let Some((parent, stage)) = parent {
            self.cells
                .copy_within(self.range(parent, stage), target.start);
        } else {
            self.cells[target].fill(u64::MAX);
        }
    }

    pub fn forward(&mut self, note: usize, from: usize, through: usize) {
        for stage in from + 1..=through {
            self.cells.copy_within(
                self.range(note, GroupView::Note(from)),
                self.range(note, GroupView::Note(stage)).start,
            );
        }
    }

    pub fn commit_at(&mut self, note: usize, stage: usize) {
        self.cells.copy_within(
            self.range(note, GroupView::Note(stage)),
            self.range(note, GroupView::Committed).start,
        );
    }

    pub fn begin_release(&mut self, note: usize, stage: usize) {
        self.cells.copy_within(
            self.range(note, GroupView::Committed),
            self.range(note, GroupView::Release(stage)).start,
        );
    }

    pub fn commit_release(&mut self, note: usize, stage: usize) {
        self.cells.copy_within(
            self.range(note, GroupView::Release(stage)),
            self.range(note, GroupView::Committed).start,
        );
    }

    pub fn edit(&mut self, note: usize, view: GroupView, group: Option<u32>, allowed: bool) {
        let range = self.range(note, view);
        let cells = &mut self.cells[range];
        if let Some(group) = group {
            let bit = 1 << (group % 64);
            let cell = &mut cells[group as usize / 64];
            *cell = if allowed { *cell | bit } else { *cell & !bit };
        } else {
            cells.fill(if allowed { u64::MAX } else { 0 });
        }
    }
}

impl Runtime {
    /// Commit the release callback's selection at its first forwarding boundary.
    /// Does not remap existing voices or repeat an already selected release phase.
    pub fn forward_release_groups(&mut self, note: NoteId) -> Result<bool, Error> {
        self.apply_due();
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if n.key_down() {
            return Err(Error::InvalidInput);
        }
        if let Some(stage) = self.release_times[note.0.index].release_stage {
            return self.forward_release_stage(note, stage);
        }
        if self.release_times[note.0.index].groups_forwarded
            || self.release_times[note.0.index].held
        {
            return Ok(false);
        }
        self.plans
            .get_mut(n.plan.0)
            .unwrap()
            .groups
            .commit_at(note.0.index, self.note_events[note.0.index].entry);
        self.release_times[note.0.index].groups_forwarded = true;
        Ok(true)
    }

    /// Edit the note's pending selection and future generated children's selection.
    /// Existing voices and the committed native release mapping are unchanged.
    /// `None` edits all declared groups; ungrouped regions remain eligible.
    pub fn set_note_group(
        &mut self,
        note: NoteId,
        group: Option<u32>,
        allowed: bool,
    ) -> Result<(), Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        self.set_note_group_at(note, self.note_events[note.0.index].entry, group, allowed)
    }

    pub fn set_note_group_at(
        &mut self,
        note: NoteId,
        stage: usize,
        group: Option<u32>,
        allowed: bool,
    ) -> Result<(), Error> {
        self.set_group_view(note, GroupView::Note(stage), group, allowed)
    }

    pub(super) fn set_group_view(
        &mut self,
        note: NoteId,
        view: GroupView,
        group: Option<u32>,
        allowed: bool,
    ) -> Result<(), Error> {
        self.apply_due();
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        let stage = match view {
            GroupView::Note(stage) | GroupView::Release(stage) => stage,
            GroupView::Committed => return Err(Error::InvalidInput),
        };
        let generation = self.plans.get_mut(plan.0).unwrap();
        if generation
            .projections
            .get(note.0.index, stage)?
            .properties
            .is_none()
        {
            return Err(Error::InvalidInput);
        }
        if group.is_some_and(|group| group >= generation.prepared.group_count) {
            return Err(Error::InvalidInput);
        }
        generation.groups.edit(note.0.index, view, group, allowed);
        Ok(())
    }

    pub fn note_group_allowed(&self, note: NoteId, group: u32) -> Result<bool, Error> {
        self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        self.note_group_allowed_at(note, self.note_events[note.0.index].entry, group)
    }

    pub fn note_group_allowed_at(
        &self,
        note: NoteId,
        stage: usize,
        group: u32,
    ) -> Result<bool, Error> {
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        let generation = self.plans.get(plan.0).unwrap();
        if group >= generation.prepared.group_count {
            return Err(Error::InvalidInput);
        }
        if generation
            .projections
            .get(note.0.index, stage)?
            .properties
            .is_none()
        {
            return Err(Error::InvalidInput);
        }
        let mask = generation.groups.view_at(note.0.index, stage);
        Ok(mask[group as usize / 64] & (1 << (group % 64)) != 0)
    }
}
