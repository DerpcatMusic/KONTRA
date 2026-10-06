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

    pub fn group_count(&self) -> u32 {
        self.group_count
    }
}

pub(super) struct GroupState {
    words: usize,
    // Each slot has an editable view and the snapshot committed with its attack.
    cells: Box<[u64]>,
}

impl GroupState {
    pub fn new(count: u32, notes: usize) -> Result<Self, Error> {
        let words = usize::try_from(count)
            .map_err(|_| Error::Capacity)?
            .div_ceil(64);
        let len = words
            .checked_mul(2)
            .and_then(|n| n.checked_mul(notes))
            .ok_or(Error::Capacity)?;
        let mut cells = Vec::new();
        cells.try_reserve_exact(len).map_err(|_| Error::Capacity)?;
        cells.resize(len, u64::MAX);
        Ok(Self {
            words,
            cells: cells.into_boxed_slice(),
        })
    }

    fn range(&self, note: usize, committed: bool) -> std::ops::Range<usize> {
        let start = (note * 2 + usize::from(committed)) * self.words;
        start..start + self.words
    }

    pub fn view(&self, note: usize, committed: bool) -> &[u64] {
        &self.cells[self.range(note, committed)]
    }

    pub fn admit(&mut self, note: usize, parent: Option<usize>) {
        let draft = self.range(note, false);
        if let Some(parent) = parent {
            self.cells
                .copy_within(self.range(parent, false), draft.start);
        } else {
            self.cells[draft].fill(u64::MAX);
        }
        self.commit(note);
    }

    pub fn commit(&mut self, note: usize) {
        self.cells
            .copy_within(self.range(note, false), self.range(note, true).start);
    }

    fn edit(&mut self, note: usize, group: Option<u32>, allowed: bool) {
        let range = self.range(note, false);
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
        if self.release_times[note.0.index].groups_forwarded {
            return Ok(false);
        }
        self.plans
            .get_mut(n.plan.0)
            .unwrap()
            .groups
            .commit(note.0.index);
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
        self.apply_due();
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        let generation = self.plans.get_mut(plan.0).unwrap();
        if group.is_some_and(|group| group >= generation.prepared.group_count) {
            return Err(Error::InvalidInput);
        }
        generation.groups.edit(note.0.index, group, allowed);
        Ok(())
    }

    pub fn note_group_allowed(&self, note: NoteId, group: u32) -> Result<bool, Error> {
        let plan = self.notes.get(note.0).ok_or(Error::StaleHandle)?.plan;
        let generation = self.plans.get(plan.0).unwrap();
        if group >= generation.prepared.group_count {
            return Err(Error::InvalidInput);
        }
        let mask = generation.groups.view(note.0.index, false);
        Ok(mask[group as usize / 64] & (1 << (group % 64)) != 0)
    }
}
