//! Which groups each note may play: one bit row per ledger slot.

/// Which groups each note may play, one bit row per ledger slot.
pub struct GroupMasks {
    words: usize,
    pub groups: usize,
    bits: Box<[u64]>,
}

impl GroupMasks {
    pub fn new(notes: usize, groups: usize) -> Self {
        let words = groups.div_ceil(64).max(1);
        Self {
            words,
            groups,
            bits: vec![u64::MAX; notes * words].into(),
        }
    }

    fn row(&mut self, slot: usize) -> &mut [u64] {
        let start = slot * self.words;
        self.bits
            .get_mut(start..start + self.words)
            .unwrap_or_default()
    }

    pub fn reset(&mut self, slot: usize) {
        self.row(slot).fill(u64::MAX);
    }

    pub fn copy(&mut self, from: usize, to: usize) {
        let (from, to) = (from * self.words, to * self.words);
        if from + self.words <= self.bits.len() && to + self.words <= self.bits.len() {
            self.bits.copy_within(from..from + self.words, to);
        }
    }

    /// A group past the last addresses every group.
    pub fn set(&mut self, slot: usize, group: i32, allowed: bool) {
        let all = usize::try_from(group).map_or(true, |g| g >= self.groups);
        let row = self.row(slot);
        if all {
            row.fill(if allowed { u64::MAX } else { 0 });
            return;
        }
        let g = group as usize;
        if let Some(word) = row.get_mut(g / 64) {
            let bit = 1u64 << (g % 64);
            if allowed {
                *word |= bit;
            } else {
                *word &= !bit;
            }
        }
    }

    pub fn allows(&self, slot: usize, group: u32) -> bool {
        let g = group as usize;
        self.bits
            .get(slot * self.words + g / 64)
            .is_none_or(|word| word & (1 << (g % 64)) != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_vm::event;

    #[test]
    fn group_masks() {
        let mut m = GroupMasks::new(2, 100);
        assert!(m.allows(0, 99));
        m.set(0, event::ALL_GROUPS, false);
        assert!(!m.allows(0, 3));
        m.set(0, 70, true);
        assert!(m.allows(0, 70) && !m.allows(0, 69));
        m.copy(0, 1);
        assert!(m.allows(1, 70) && !m.allows(1, 3));
        m.reset(1);
        assert!(m.allows(1, 3));
    }
}
