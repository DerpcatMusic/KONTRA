//! Sequential take decisions are scoped to a prepared generation and retained by notes.
use super::{ChannelAddress, Error, FamilyId, Index, NoteId, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceScope {
    Global,
    /// Logical selection key, including the integer part of absolute note pitch.
    Key,
    /// Original protocol/port/group/channel; generated notes inherit this address.
    Channel,
    ChannelKey,
}

/// One sequential round robin. Scope storage is reserved on control and retained
/// for the plan generation's lifetime; exhausted scope capacity rejects admission.
#[derive(Clone, Copy, Debug)]
pub struct Sequence {
    pub takes: u32,
    pub scope: SequenceScope,
    pub capacity: usize,
}

/// A zero-based take within a sequence in the note's prepared generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Take {
    pub sequence: usize,
    pub index: u32,
}

pub(super) struct PreparedSequence {
    pub spec: Sequence,
    pub offset: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Global,
    Key(u8),
    Channel(ChannelAddress),
    ChannelKey(ChannelAddress, u8),
}

#[derive(Clone, Copy)]
struct Position {
    owner: Owner,
    next: u32,
}

pub(super) struct SequenceState(Box<[Option<Position>]>);

#[derive(Clone, Copy)]
pub(super) struct PendingTake {
    cell: usize,
    owner: Owner,
    pub take: Take,
}

impl SequenceState {
    pub fn check_size(cells: usize) -> Result<(), Error> {
        std::alloc::Layout::array::<Option<Position>>(cells).map_err(|_| Error::Capacity)?;
        Ok(())
    }

    /// Construction/destruction belong on control, including during plan transfer.
    pub fn new(cells: usize) -> Self {
        Self(vec![None; cells].into_boxed_slice())
    }

    pub fn choose(
        &self,
        sequence: usize,
        prepared: &PreparedSequence,
        address: ChannelAddress,
        key: u8,
    ) -> Result<PendingTake, Error> {
        let owner = match prepared.spec.scope {
            SequenceScope::Global => Owner::Global,
            SequenceScope::Key => Owner::Key(key),
            SequenceScope::Channel => Owner::Channel(address),
            SequenceScope::ChannelKey => Owner::ChannelKey(address, key),
        };
        // ponytail: bounded scope scan; index it if measured scoped workloads need it.
        let cells = &self.0[prepared.offset..prepared.offset + prepared.spec.capacity];
        let index = cells
            .iter()
            .position(|p| p.is_some_and(|p| p.owner == owner))
            .or_else(|| cells.iter().position(Option::is_none))
            .ok_or(Error::Capacity)?;
        Ok(PendingTake {
            cell: prepared.offset + index,
            owner,
            take: Take {
                sequence,
                index: cells[index].map_or(0, |p| p.next),
            },
        })
    }

    pub fn commit(&mut self, pending: PendingTake, takes: u32) {
        let next = if pending.take.index == takes - 1 {
            0
        } else {
            pending.take.index + 1
        };
        self.0[pending.cell] = Some(Position {
            owner: pending.owner,
            next,
        });
    }
}

#[derive(Clone, Copy)]
pub(super) struct Decision {
    pub take: Take,
    pub next: Option<Index>,
}

impl Runtime {
    /// Retained choices consume their own declared budget until the note retires,
    /// even if its sources have ended or terminal delivery is backpressured.
    pub fn decision_count(&self) -> usize {
        self.decisions.count()
    }

    pub fn note_take(&self, note: NoteId, sequence: usize) -> Result<Option<u32>, Error> {
        let note = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if sequence
            >= self
                .plans
                .get(note.plan.0)
                .unwrap()
                .prepared
                .sequences
                .len()
        {
            return Err(Error::InvalidInput);
        }
        let mut next = note.first_decision;
        while let Some(index) = next {
            let decision = self.decisions.slots[index.get()].value.unwrap();
            if decision.take.sequence == sequence {
                return Ok(Some(decision.take.index));
            }
            next = decision.next;
        }
        Ok(None)
    }

    pub fn family_take(&self, family: FamilyId) -> Result<Option<Take>, Error> {
        let family = self.families.get(family.0).ok_or(Error::StaleHandle)?;
        Ok(family
            .decision
            .map(|index| self.decisions.slots[index.get()].value.unwrap().take))
    }

    pub(super) fn record_take(&mut self, note: NoteId, take: Take) -> Index {
        let note = self.notes.get_mut(note.0).unwrap();
        let id = self
            .decisions
            .insert(Decision {
                take,
                next: note.first_decision,
            })
            .expect("preflighted take decision capacity");
        let index = Index::new(id.index);
        note.first_decision = Some(index);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_take_wraps_without_overflow_and_choose_does_not_mutate_state() {
        let address = ChannelAddress {
            protocol: crate::Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        };
        let prepared = PreparedSequence {
            spec: Sequence {
                takes: u32::MAX,
                scope: SequenceScope::Global,
                capacity: 1,
            },
            offset: 0,
        };
        let mut state = SequenceState::new(1);
        state.0[0] = Some(Position {
            owner: Owner::Global,
            next: u32::MAX - 1,
        });
        for _ in 0..3 {
            assert_eq!(
                state.choose(0, &prepared, address, 60).unwrap().take.index,
                u32::MAX - 1
            );
        }
        let choice = state.choose(0, &prepared, address, 60).unwrap();
        state.commit(choice, prepared.spec.takes);
        assert_eq!(
            state.choose(0, &prepared, address, 60).unwrap().take.index,
            0
        );
    }
}
