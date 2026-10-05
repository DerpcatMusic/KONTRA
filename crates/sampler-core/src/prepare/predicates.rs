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
                .any(|c| c.controller >= 128 || c.low > c.high)
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
        self.conditions = conditions.into_boxed_slice();
        self.compile_selection();
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
                            let value = state.controllers[usize::from(c.controller)];
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
