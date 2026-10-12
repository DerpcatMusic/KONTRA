use super::{Candidate, Prepared};
use crate::{Error, performance::State};
use std::{collections::BTreeMap, ops::Range};

/// Inclusive native full-resolution CC condition. Conditions on a region are ANDed;
/// a condition gates selection and never generates a controller-triggered note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ControllerCondition {
    pub controller: u8,
    pub low: u32,
    pub high: u32,
}

/// The virtual controller whose value is the interval from the previous held
/// key ([`crate::previous_key_value`]); conditions on it select recorded
/// transitions, first notes and legato notes.
pub const PREVIOUS_KEY: u8 = 128;
/// Virtual controllers `AXIS_BASE..AXIS_BASE + MAX_AXES` hold the active
/// choice of each nested selector.
pub const AXIS_BASE: u8 = 129;
pub const MAX_AXES: usize = 4;

impl Prepared {
    /// Compile conditions in original region order. The budget bounds the supplied
    /// condition count before canonicalization/sharing, not a Cartesian state table.
    pub fn with_controllers(
        mut self,
        regions: Vec<Vec<ControllerCondition>>,
        max_conditions: usize,
    ) -> Result<Self, Error> {
        if regions.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        let count = regions
            .iter()
            .try_fold(0usize, |sum, r| sum.checked_add(r.len()))
            .ok_or(Error::Capacity)?;
        if count > max_conditions {
            return Err(Error::Capacity);
        }
        let mut intern = BTreeMap::new();
        for (region, mut conditions) in self.regions.iter_mut().zip(regions) {
            if conditions
                .iter()
                .any(|c| c.controller >= AXIS_BASE + MAX_AXES as u8 || c.low > c.high)
            {
                return Err(Error::InvalidInput);
            }
            conditions.sort_unstable();
            let mut used = 0;
            for i in 0..conditions.len() {
                let c = conditions[i];
                if used != 0 && conditions[used - 1].controller == c.controller {
                    let previous = &mut conditions[used - 1];
                    previous.low = previous.low.max(c.low);
                    previous.high = previous.high.min(c.high);
                    if previous.low > previous.high {
                        return Err(Error::InvalidInput);
                    }
                } else if c.low != 0 || c.high != u32::MAX {
                    conditions[used] = c;
                    used += 1;
                }
            }
            conditions.truncate(used);
            region.conditions = if conditions.is_empty() {
                None
            } else {
                let next = intern.len();
                Some(*intern.entry(conditions).or_insert(next))
            };
        }
        let mut conditions = vec![Box::from([]); intern.len()];
        for (set, index) in intern {
            conditions[index] = set.into_boxed_slice();
        }
        self.tracks_previous = conditions
            .iter()
            .any(|set| set.iter().any(|c| c.controller == PREVIOUS_KEY));
        self.conditions = conditions.into_boxed_slice();
        self.compile_selection();
        Ok(self)
    }

    /// Add keyswitches that set nested selector choices: `(key, axis, choice)`.
    /// A key already used by an articulation switch is refused.
    pub fn with_axis_switches(mut self, keys: Vec<(u8, usize, u32)>) -> Result<Self, Error> {
        for (key, axis, choice) in keys {
            let slot = self
                .keyswitches
                .get_mut(usize::from(key))
                .ok_or(Error::InvalidInput)?;
            if axis >= MAX_AXES || choice > 0xffff || slot.is_some() {
                return Err(Error::InvalidInput);
            }
            *slot = Some(crate::AXIS_SWITCH | (axis as u32) << 16 | choice);
        }
        Ok(self)
    }

    /// Choose the coherent articulation/controller version independently at each
    /// release phase. Velocity and expression retain their own policies.
    pub fn with_release_selection(
        mut self,
        key: crate::SelectionPolicy,
        gate: crate::SelectionPolicy,
    ) -> Self {
        self.release_selection = [key, gate];
        self
    }

    // A projection onto one controller can only admit MORE regions than the full
    // conjunction. Its maximum overlap is therefore a safe upper bound. Taking
    // the minimum of these bounds avoids a Cartesian search across CC states.
    pub(super) fn controller_release_bound(&self, key: u8, trigger: crate::Trigger) -> usize {
        let range = self.range(key, trigger);
        let mut from = range.start;
        let mut total = 0;
        while from < range.end {
            let until = self.group_end(from, range.end);
            let candidates = &self.candidates[from..until];
            let mut controllers = 0u128;
            let mut counts = BTreeMap::<u32, usize>::new();
            for c in candidates {
                let r = self.regions[c.region];
                *counts.entry(r.take.map_or(0, |t| t.index)).or_default() += 1;
                if let Some(index) = r.conditions {
                    for condition in self.conditions[index].iter().filter(|c| c.controller < 128) {
                        controllers |= 1u128 << condition.controller;
                    }
                }
            }
            let mut bound = counts.values().copied().max().unwrap_or(0);
            while controllers != 0 {
                let controller = controllers.trailing_zeros() as u8;
                controllers &= controllers - 1;
                let mut takes = BTreeMap::<u32, Vec<(u64, bool)>>::new();
                for c in candidates {
                    let r = self.regions[c.region];
                    let condition = r.conditions.and_then(|index| {
                        let set = &self.conditions[index];
                        set.binary_search_by_key(&controller, |c| c.controller)
                            .ok()
                            .map(|i| set[i])
                    });
                    let (low, high) = condition.map_or((0, u32::MAX), |c| (c.low, c.high));
                    let events = takes.entry(r.take.map_or(0, |t| t.index)).or_default();
                    events.push((u64::from(low), true));
                    // u64 represents the exclusive end above u32::MAX exactly.
                    events.push((u64::from(high) + 1, false));
                }
                let mut maximum = 0;
                for events in takes.values_mut() {
                    events.sort_unstable(); // Ends before starts at a shared exclusive boundary.
                    let mut active = 0;
                    for &(_, start) in events.iter() {
                        active = if start { active + 1 } else { active - 1 };
                        maximum = maximum.max(active);
                    }
                }
                bound = bound.min(maximum);
            }
            // Different sequence groups can select independently.
            total += bound;
            from = until;
        }
        total
    }
}

// Cursor owns no borrowed runtime data. Commit can mutate ownership between reads
// without allocating candidate copies or publishing mutable prepared metadata.
pub(super) struct Matching {
    ranges: [Range<usize>; 2],
    which: usize,
    through: usize,
}

impl Matching {
    pub fn new(ranges: [Range<usize>; 2]) -> Self {
        Self {
            ranges,
            which: 0,
            through: 0,
        }
    }

    pub fn next_in_groups(
        &mut self,
        prepared: &Prepared,
        state: &State,
        velocity: f64,
        groups: Option<&[u64]>,
    ) -> Option<Candidate> {
        loop {
            let candidate = self.next(prepared, Some(state), Some(velocity))?;
            let group = prepared
                .region_groups
                .get(candidate.region)
                .copied()
                .flatten();
            if group.is_some_and(|group| !prepared.native_group_allowed(group, state)) {
                continue;
            }
            if groups.is_none_or(|mask| {
                prepared
                    .region_groups
                    .get(candidate.region)
                    .copied()
                    .flatten()
                    .is_none_or(|group| mask[group as usize / 64] & (1 << (group % 64)) != 0)
            }) {
                return Some(candidate);
            }
        }
    }

    #[inline]
    pub fn next(
        &mut self,
        prepared: &Prepared,
        state: Option<&State>,
        velocity: Option<f64>,
    ) -> Option<Candidate> {
        if state.is_none() || prepared.conditions.is_empty() {
            for index in self.ranges.iter_mut().flatten() {
                let candidate = prepared.candidates[index];
                let r = prepared.regions[candidate.region];
                if velocity.is_none_or(|v| r.velocity_low <= v && v <= r.velocity_high) {
                    return Some(candidate);
                }
            }
            return None;
        }
        while self.which < 2 {
            let range = &mut self.ranges[self.which];
            if range.start >= range.end {
                self.which += 1;
                self.through = 0;
                continue;
            }
            if range.start >= self.through {
                let condition =
                    prepared.regions[prepared.candidates[range.start].region].conditions;
                // Same-condition microphones are contiguous within sequence/articulation.
                // A run is evaluated once per pass, including rejected velocity layers.
                // ponytail: identical sets in different runs are reevaluated; add a
                // generation-owned sparse cache only if measured large mappings need it.
                self.through = prepared.condition_ends[range.start].min(range.end);
                let active = state.is_none_or(|state| {
                    condition.is_none_or(|index| {
                        prepared.conditions[index].iter().all(|c| {
                            let value = state.value(c.controller);
                            c.low <= value && value <= c.high
                        })
                    })
                });
                if !active {
                    range.start = self.through;
                    continue;
                }
            }
            let candidate = prepared.candidates[range.start];
            range.start += 1;
            let r = prepared.regions[candidate.region];
            if velocity.is_none_or(|v| r.velocity_low <= v && v <= r.velocity_high) {
                return Some(candidate);
            }
        }
        None
    }
}
