//! Composed native group starts. The script mask is an independent final gate.
use crate::{Error, Prepared, Runtime, performance::State};
use sampler_ir::{GroupStart, StartJoin, StartTest};

impl Prepared {
    pub fn with_native_criteria(
        mut self,
        groups: Vec<Vec<GroupStart>>,
        articulation_keys: Vec<Option<u8>>,
        default: Option<u8>,
    ) -> Result<Self, Error> {
        if groups.len() != self.group_count as usize
            || groups.iter().any(|rows| {
                rows.len() > 4
                    || rows.iter().any(|r| {
                        r.slot > 3
                            || match r.test {
                                StartTest::Key { low, high } => low > high || high > 127,
                                StartTest::Controller {
                                    controller,
                                    low,
                                    high,
                                } => controller > 127 || low > high || high > 127,
                                StartTest::RoundRobin(position) => position == 0,
                                StartTest::Random => false,
                            }
                    })
            })
            || default.is_some_and(|k| k > 127)
            || articulation_keys.iter().flatten().any(|&k| k > 127)
        {
            return Err(Error::InvalidInput);
        }
        self.native_rr_length = groups
            .iter()
            .flatten()
            .filter_map(|r| match r.test {
                StartTest::RoundRobin(position) => Some(position),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.native_random_groups = groups
            .iter()
            .enumerate()
            .filter(|(_, rows)| rows.iter().any(|r| r.test == StartTest::Random))
            .map(|(i, _)| i as u32)
            .collect();
        self.native_start = if groups.iter().all(Vec::is_empty) {
            Box::new([])
        } else {
            groups.into_iter().map(Vec::into_boxed_slice).collect()
        };
        self.native_articulation_keys = articulation_keys.into_boxed_slice();
        self.native_default_key = default;
        Ok(self)
    }

    pub(super) fn native_group_allowed(&self, group: u32, state: &State) -> bool {
        let Some(rows) = self.native_start.get(group as usize) else {
            return true;
        };
        let test = |test| match test {
            StartTest::Key { low, high } => state
                .native_key
                .or(self.native_default_key)
                .is_some_and(|key| (low..=high).contains(&key)),
            StartTest::Controller {
                controller,
                low,
                high,
            } => {
                let value = ((u64::from(state.controllers[usize::from(controller)]) * 127
                    + u64::from(u32::MAX) / 2)
                    / u64::from(u32::MAX)) as u8;
                (low..=high).contains(&value)
            }
            StartTest::RoundRobin(position) => {
                state.native_tick % u64::from(self.native_rr_length) + 1 == u64::from(position)
            }
            StartTest::Random => {
                // SplitMix64: explicit reset seed, no clock or voice-slot dependence.
                let mut value = state
                    .native_seed
                    .wrapping_add(state.native_tick.wrapping_mul(0x9e3779b97f4a7c15));
                value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
                value ^= value >> 31;
                self.native_random_groups[value as usize % self.native_random_groups.len()] == group
            }
        };
        let Some(first) = rows.first() else {
            return true;
        };
        let mut active = test(first.test);
        for pair in rows.windows(2) {
            let next = test(pair[1].test);
            active = match pair[0].next {
                StartJoin::And => active && next,
                StartJoin::AndNot => active && !next,
                StartJoin::Or => active || next,
            };
        }
        active
    }
}
impl Runtime {
    /// Explicit phase reset; captured selections and sounding voices stay intact.
    pub fn reset_native_cycles(&mut self, seed: u64) {
        for slot in &mut self.plans.slots {
            if let Some(generation) = &mut slot.value {
                generation.native_cycle = 0;
                generation.native_seed = seed;
                generation.sequences.reset(&generation.prepared);
            }
        }
    }
}
