//! Native take decisions are scoped to a prepared generation and retained by notes.
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

/// Reproducible native selection; seeds are explicit and scoped by owner address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TakePolicy {
    Sequential,
    Random {
        seed: u64,
    },
    /// Requires at least two takes; the preceding take is excluded.
    NoRepeat {
        seed: u64,
    },
    /// Every take once per bag; adjacent bags may repeat at their boundary.
    Shuffle {
        seed: u64,
    },
}

/// One coordinated take sequence. Scope storage is reserved on control and retained
/// for the plan generation's lifetime; exhausted scope capacity rejects admission.
#[derive(Clone, Copy, Debug)]
pub struct Sequence {
    pub takes: u32,
    pub policy: TakePolicy,
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
    pub shuffle_offset: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Global,
    Key(u8),
    Channel(ChannelAddress),
    ChannelKey(ChannelAddress, u8),
}

// PCG XSH-RR and seeding adapted from pcg-c-basic, Copyright 2014 Melissa
// O'Neill, Apache-2.0: https://www.pcg-random.org/download.html
// Rust adaptation adds an explicit bounded-draw failure instead of an unbounded loop.
#[derive(Clone, Copy)]
struct Pcg32 {
    state: u64,
    increment: u64,
}

impl Pcg32 {
    fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Self {
            state: 0,
            increment: (stream << 1) | 1,
        };
        rng.next();
        rng.state = rng.state.wrapping_add(seed);
        rng.next();
        rng
    }

    fn next(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.increment);
        ((((old >> 18) ^ old) >> 27) as u32).rotate_right((old >> 59) as u32)
    }

    fn bounded(&mut self, bound: u32) -> Result<u32, Error> {
        bounded(|| self.next(), bound)
    }
}

fn bounded(mut draw: impl FnMut() -> u32, bound: u32) -> Result<u32, Error> {
    let threshold = bound.wrapping_neg() % bound;
    for _ in 0..64 {
        let value = draw();
        if value >= threshold {
            return Ok(value % bound);
        }
    }
    Err(Error::RandomBudget)
}

impl Owner {
    fn stream(self) -> u64 {
        // Fixed, injective encoding of validated protocol/port/group/channel/key.
        // Stream identity does not depend on scope allocation or gesture ordering.
        fn address(a: ChannelAddress) -> u64 {
            let protocol = match a.protocol {
                super::Protocol::Native => 0,
                super::Protocol::Midi1 => 1,
                super::Protocol::Midi2 => 2,
                super::Protocol::Clap => 3,
                super::Protocol::Vst3 => 4,
            };
            protocol
                | (u64::from(a.port) << 3)
                | (u64::from(a.group) << 19)
                | (u64::from(a.channel) << 23)
        }
        match self {
            Self::Global => 0,
            Self::Key(key) => 1 | (u64::from(key) << 2),
            Self::Channel(a) => 2 | (address(a) << 2),
            Self::ChannelKey(a, key) => 3 | (u64::from(key) << 2) | (address(a) << 9),
        }
    }
}

#[derive(Clone, Copy)]
enum Progress {
    Sequential(u32),
    Random { rng: Pcg32, previous: Option<u32> },
    Shuffle { rng: Pcg32, remaining: u32 },
}

#[derive(Clone, Copy)]
struct Position {
    owner: Owner,
    progress: Progress,
}

pub(super) struct SequenceState {
    positions: Box<[Option<Position>]>,
    bags: Box<[u32]>,
}

#[derive(Clone, Copy)]
pub(super) struct PendingTake {
    cell: usize,
    position: Position,
    swap: Option<(usize, usize)>,
    pub take: Take,
}

impl SequenceState {
    pub fn check_size(cells: usize, entries: usize) -> Result<(), Error> {
        std::alloc::Layout::array::<Option<Position>>(cells).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<u32>(entries).map_err(|_| Error::Capacity)?;
        Ok(())
    }

    /// Construction/destruction belong on control, including during plan transfer.
    pub fn new(prepared: &super::Prepared) -> Self {
        let mut bags = vec![0; prepared.shuffle_entries].into_boxed_slice();
        for sequence in &prepared.sequences {
            if matches!(sequence.spec.policy, TakePolicy::Shuffle { .. }) {
                let takes = sequence.spec.takes as usize;
                for slot in 0..sequence.spec.capacity {
                    let start = sequence.shuffle_offset + slot * takes;
                    for (i, take) in bags[start..start + takes].iter_mut().enumerate() {
                        *take = i as u32;
                    }
                }
            }
        }
        Self {
            positions: vec![None; prepared.sequence_cells].into_boxed_slice(),
            bags,
        }
    }

    pub fn choose(
        &self,
        sequence: usize,
        prepared: &PreparedSequence,
        address: ChannelAddress,
        key: u8,
    ) -> Result<PendingTake, Error> {
        let spec = prepared.spec;
        let owner = match spec.scope {
            SequenceScope::Global => Owner::Global,
            SequenceScope::Key => Owner::Key(key),
            SequenceScope::Channel => Owner::Channel(address),
            SequenceScope::ChannelKey => Owner::ChannelKey(address, key),
        };
        // ponytail: bounded scope scan; index it if measured scoped workloads need it.
        let cells = &self.positions[prepared.offset..prepared.offset + spec.capacity];
        let index = cells
            .iter()
            .position(|p| p.is_some_and(|p| p.owner == owner))
            .or_else(|| cells.iter().position(Option::is_none))
            .ok_or(Error::Capacity)?;
        let mut progress = cells[index]
            .map(|p| p.progress)
            .unwrap_or_else(|| match spec.policy {
                TakePolicy::Sequential => Progress::Sequential(0),
                TakePolicy::Random { seed } | TakePolicy::NoRepeat { seed } => Progress::Random {
                    rng: Pcg32::new(seed, owner.stream()),
                    previous: None,
                },
                TakePolicy::Shuffle { seed } => Progress::Shuffle {
                    rng: Pcg32::new(seed, owner.stream()),
                    remaining: 0,
                },
            });
        let mut swap = None;
        let take = match &mut progress {
            Progress::Sequential(next) => {
                let take = *next;
                *next = if take == spec.takes - 1 { 0 } else { take + 1 };
                take
            }
            Progress::Random { rng, previous } => {
                let avoid = previous.filter(|_| matches!(spec.policy, TakePolicy::NoRepeat { .. }));
                let mut take = rng.bounded(spec.takes - u32::from(avoid.is_some()))?;
                if avoid.is_some_and(|previous| take >= previous) {
                    take += 1;
                }
                *previous = Some(take);
                take
            }
            Progress::Shuffle { rng, remaining } => {
                if *remaining == 0 {
                    *remaining = spec.takes;
                }
                let start = prepared.shuffle_offset + index * spec.takes as usize;
                let selected = start + rng.bounded(*remaining)? as usize;
                *remaining -= 1;
                swap = Some((selected, start + *remaining as usize));
                self.bags[selected]
            }
        };
        Ok(PendingTake {
            cell: prepared.offset + index,
            position: Position { owner, progress },
            swap,
            take: Take {
                sequence,
                index: take,
            },
        })
    }

    pub fn commit(&mut self, pending: PendingTake) {
        if let Some((a, b)) = pending.swap {
            self.bags.swap(a, b);
        }
        self.positions[pending.cell] = Some(pending.position);
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
                policy: TakePolicy::Sequential,
                scope: SequenceScope::Global,
                capacity: 1,
            },
            offset: 0,
            shuffle_offset: 0,
        };
        let mut state = SequenceState {
            positions: vec![None].into_boxed_slice(),
            bags: Box::new([]),
        };
        state.positions[0] = Some(Position {
            owner: Owner::Global,
            progress: Progress::Sequential(u32::MAX - 1),
        });
        for _ in 0..3 {
            assert_eq!(
                state.choose(0, &prepared, address, 60).unwrap().take.index,
                u32::MAX - 1
            );
        }
        let choice = state.choose(0, &prepared, address, 60).unwrap();
        state.commit(choice);
        assert_eq!(
            state.choose(0, &prepared, address, 60).unwrap().take.index,
            0
        );
    }
    #[test]
    fn pcg_matches_published_vector_and_bounded_draws_have_a_hard_limit() {
        let mut rng = Pcg32::new(42, 54);
        assert_eq!(
            std::array::from_fn::<_, 6, _>(|_| rng.next()),
            [
                0xa15c02b7, 0x7b47f409, 0xba1d3330, 0x83d2f293, 0xbfa4784b, 0xcbed606e
            ]
        );
        let mut draws = 0;
        assert_eq!(
            bounded(
                || {
                    draws += 1;
                    0
                },
                3
            ),
            Err(Error::RandomBudget)
        );
        assert_eq!(draws, 64);
        let mut values = [0, 7].into_iter();
        assert_eq!(bounded(|| values.next().unwrap(), 3), Ok(1));
        assert_eq!(values.next(), None);
        assert_eq!(bounded(|| u32::MAX, u32::MAX), Ok(0));
        assert_eq!(bounded(|| 0, 1), Ok(0));
    }
}
