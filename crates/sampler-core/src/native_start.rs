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
        // Kontakt: one draw over every mode-4 row (groups in order, then slots);
        // each row matches only its own ordinal (docs/re/KONTAKT_START_CONDITIONS.md).
        let mut ordinal = 0u32;
        self.native_random_ordinals = groups
            .iter()
            .map(|rows| {
                rows.iter()
                    .map(|r| {
                        (r.test == StartTest::Random).then(|| {
                            ordinal += 1;
                            ordinal - 1
                        })
                    })
                    .collect()
            })
            .collect();
        self.native_random_rows = ordinal;
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
        if self
            .group_mutes
            .get(group as usize)
            .is_some_and(|m| m.load(std::sync::atomic::Ordering::Acquire))
        {
            return false;
        }
        if self.residency.as_ref().is_some_and(|r| !r.allowed(group)) {
            return false;
        }
        let Some(rows) = self.native_start.get(group as usize) else {
            return true;
        };
        let ordinal = |row: usize| {
            self.native_random_ordinals
                .get(group as usize)
                .and_then(|o| o.get(row))
                .copied()
                .flatten()
        };
        let test = |test, ordinal: Option<u32>| match test {
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
            // One draw per admitted note, shared by every mode-4 row (see `admit`).
            StartTest::Random => ordinal == Some(state.native_draw),
        };
        let Some(first) = rows.first() else {
            return true;
        };
        let mut active = test(first.test, ordinal(0));
        for (i, pair) in rows.windows(2).enumerate() {
            let next = test(pair[1].test, ordinal(i + 1));
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
    /// Only the low 32 bits of `seed` reach the generator (Kontakt's seed word is 32-bit).
    pub fn reset_native_cycles(&mut self, seed: u64) {
        for slot in &mut self.plans.slots {
            if let Some(generation) = &mut slot.value {
                generation.native_cycle = 0;
                generation.native_random = NativeRandom::new(seed as u32);
                generation.native_draw = 0;
                generation.sequences.reset(&generation.prepared);
            }
        }
    }
}

/// Kontakt 8 `random(from, to)` generator, as used by the mode-4 start draw.
///
/// MT19937 with `init_genrand` seeding and standard tempering. Kontakt 8
/// `0x1410ebf10` seeds it, `0x1410ec0c0` generates, and the global
/// `0x14a6381e0` backs `0x1414d5e80`. The binary keeps a double-buffered
/// layout; its stream matches standard MT19937 (checked in the tests below).
/// Open: the native per-process seed. The global is seeded from
/// `std::random_device` (`0x1414e7ec0`), so no fixed native seed is recovered.
#[derive(Clone, Copy)]
pub(crate) struct NativeRandom {
    words: [u32; 624],
    index: usize,
}

impl NativeRandom {
    pub(crate) fn new(seed: u32) -> Self {
        let mut words = [0; 624];
        words[0] = seed;
        for i in 1..624 {
            let prev = words[i - 1];
            words[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Self { words, index: 624 }
    }

    fn word(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.words[i] & 0x8000_0000) | (self.words[(i + 1) % 624] & 0x7fff_ffff);
                self.words[i] =
                    self.words[(i + 397) % 624] ^ (y >> 1) ^ (0x9908_b0df & (y & 1).wrapping_neg());
            }
            self.index = 0;
        }
        let mut y = self.words[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// Uniform in `0..n`, as Kontakt's `random(0, n - 1)`: Lemire multiply-high
    /// with rejection over 32-bit words (`0x1414e6140`), first word most
    /// significant. `n == 0` draws nothing and returns 0 (no mode-4 row).
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let words = if n - 1 > u64::from(u32::MAX) { 2 } else { 1 };
        let bits = 32 * words;
        let n = u128::from(n);
        let threshold = ((1u128 << bits) - n) % n;
        loop {
            let mut x = 0u128;
            for _ in 0..words {
                x = (x << 32) | u128::from(self.word());
            }
            let product = x * n;
            if product & ((1u128 << bits) - 1) >= threshold {
                return (product >> bits) as u64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NativeRandom;

    #[test]
    fn mt19937_matches_reference_vectors() {
        // Seed 5489 is the reference default; 10000th output is the C++ std::mt19937 check value.
        let mut rng = NativeRandom::new(5489);
        assert_eq!(rng.word(), 3_499_211_612);
        for _ in 1..9_999 {
            rng.word();
        }
        assert_eq!(rng.word(), 4_123_659_995);
    }

    #[test]
    fn bounded_mapping_uses_multiply_high_with_rejection() {
        // First word 3499211612: 3499211612 * 6 >> 32 = 4, low word above the threshold.
        assert_eq!(NativeRandom::new(5489).below(6), 4);
        assert_eq!(NativeRandom::new(5489).below(0), 0);
        // Two-word path (span above 32 bits): vector from the same stream.
        assert_eq!(NativeRandom::new(5489).below(1 << 33), 6_998_423_224);
    }
}
