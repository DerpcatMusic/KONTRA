//! Keys every script slot shares (KSP's PGS): integer arrays of up to 256
//! elements and strings. Setting a value runs `on pgs_changed` in each slot.

use std::sync::Arc;

use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;
use crate::queue::Work;
use crate::script::{Kind, Running};

/// Elements a key may hold (KSP manual, PGS).
const MOST: usize = 256;
/// Post-init reserve carrying forward v3 e80a9f22b9's 64-key allowance per kind.
/// Each extra key reserves a name entry (and up to 256 integer cells).
const RUNTIME_KEYS: usize = 64;
/// Implementation admission policy, not a measured vendor limit: at 256 cells
/// per key this caps initialized integer values at 8 MiB. Raising it increases
/// preparation memory. Refusals are diagnosed. Lookups are a binary search
/// (64 last-key reads of each kind at this cap: 34 µs in release, against a
/// 1.333 ms block; a linear scan took 10.9 ms).
const INIT_KEYS: usize = 8192;

struct IntKey {
    name: Text,
    start: usize,
    len: usize,
}

/// Prepared PGS key/value limits and reserved backing allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgsBudget {
    /// Maximum integer keys, including the reserved callback allowance.
    pub integer_keys: usize,
    /// Maximum string keys, including the reserved callback allowance.
    pub string_keys: usize,
    /// Maximum number of `i32` cells across all integer keys.
    pub integer_values: usize,
    /// Reserved vector capacity in bytes, excluding allocator overhead.
    pub bytes: usize,
}

pub(crate) struct Pgs {
    ints: Vec<IntKey>,
    values: Vec<i32>,
    texts: Vec<(Text, Text)>,
    /// `ints` and `texts` positions in key order, for a binary search.
    int_order: Vec<u32>,
    text_order: Vec<u32>,
    int_limit: usize,
    text_limit: usize,
    value_limit: usize,
    /// An `on pgs_changed` run waits in the queue.
    queued: bool,
}

impl Pgs {
    pub(crate) fn new() -> Self {
        Self {
            ints: Vec::new(),
            values: Vec::new(),
            texts: Vec::new(),
            int_order: Vec::new(),
            text_order: Vec::new(),
            int_limit: INIT_KEYS,
            text_limit: INIT_KEYS,
            value_limit: INIT_KEYS * MOST,
            queued: false,
        }
    }

    /// Called after all slots have initialized, before the engine reaches audio.
    pub(crate) fn prepare_runtime(&mut self) {
        let (int_keys, text_keys, cells) = (self.ints.len(), self.texts.len(), self.values.len());
        self.int_limit = self.ints.len() + RUNTIME_KEYS;
        self.text_limit = self.texts.len() + RUNTIME_KEYS;
        self.value_limit = self.values.len() + RUNTIME_KEYS * MOST;
        self.ints.reserve_exact(RUNTIME_KEYS);
        self.texts.reserve_exact(RUNTIME_KEYS);
        self.int_order.reserve_exact(RUNTIME_KEYS);
        self.text_order.reserve_exact(RUNTIME_KEYS);
        let orders = (self.int_order.len(), self.text_order.len());
        self.int_order.resize(orders.0 + RUNTIME_KEYS, 0);
        self.text_order.resize(orders.1 + RUNTIME_KEYS, 0);
        std::hint::black_box(self.int_order.as_mut_slice());
        std::hint::black_box(self.text_order.as_mut_slice());
        self.int_order.truncate(orders.0);
        self.text_order.truncate(orders.1);
        self.values.reserve_exact(RUNTIME_KEYS * MOST);
        // Touch every admitted callback slot off audio, then hide the empty
        // entries. Keep the writes observable so optimization cannot remove them.
        for _ in 0..RUNTIME_KEYS {
            self.ints.push(IntKey {
                name: Text::EMPTY,
                start: 0,
                len: 0,
            });
            self.texts.push((Text::EMPTY, Text::EMPTY));
        }
        self.values.resize(self.value_limit, 0);
        std::hint::black_box(self.ints.as_mut_slice());
        std::hint::black_box(self.texts.as_mut_slice());
        std::hint::black_box(self.values.as_mut_slice());
        self.ints.truncate(int_keys);
        self.texts.truncate(text_keys);
        self.values.truncate(cells);
    }

    pub(crate) fn budget(&self) -> PgsBudget {
        PgsBudget {
            integer_keys: self.int_limit,
            string_keys: self.text_limit,
            integer_values: self.value_limit,
            bytes: (self.int_order.capacity() + self.text_order.capacity()) * 4
                + self.ints.capacity() * std::mem::size_of::<IntKey>()
                + self.texts.capacity() * std::mem::size_of::<(Text, Text)>()
                + self.values.capacity() * std::mem::size_of::<i32>(),
        }
    }

    fn create_int(&mut self, key: Text, len: usize) -> bool {
        let Err(at) = find(&self.int_order, |n| self.ints.get(n).map(|e| e.name), key) else {
            return true;
        };
        if self.ints.len() >= self.int_limit || self.values.len() + len > self.value_limit {
            return false;
        }
        let start = self.values.len();
        self.values.resize(start + len, 0);
        self.int_order.insert(at, self.ints.len() as u32);
        self.ints.push(IntKey {
            name: key,
            start,
            len,
        });
        true
    }

    fn create_text(&mut self, key: Text) -> bool {
        let Err(at) = find(&self.text_order, |n| self.texts.get(n).map(|e| e.0), key) else {
            return true;
        };
        if self.texts.len() >= self.text_limit {
            return false;
        }
        self.text_order.insert(at, self.texts.len() as u32);
        self.texts.push((key, Text::EMPTY));
        true
    }

    fn values(&mut self, key: Text) -> Option<&mut [i32]> {
        let at = find(&self.int_order, |n| self.ints.get(n).map(|e| e.name), key).ok()?;
        let entry = self.ints.get(*self.int_order.get(at)? as usize)?;
        self.values.get_mut(entry.start..entry.start + entry.len)
    }

    fn text(&mut self, key: Text) -> Option<&mut Text> {
        let at = find(&self.text_order, |n| self.texts.get(n).map(|e| e.0), key).ok()?;
        let n = *self.text_order.get(at)? as usize;
        self.texts.get_mut(n).map(|(_, v)| v)
    }
}

/// Where `key` sits in `order` (positions sorted by their name), or where
/// it would go.
fn find(order: &[u32], name: impl Fn(usize) -> Option<Text>, key: Text) -> Result<usize, usize> {
    order.binary_search_by(|&n| {
        let name = name(n as usize).unwrap_or(Text::EMPTY);
        name.as_str().cmp(key.as_str())
    })
}

impl Engine {
    /// Answers a PGS action; `None` for any other.
    pub(crate) fn pgs_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        let key = call.texts.first().copied().unwrap_or(Text::EMPTY);
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let flag = |b: bool| HostValue::Int(i32::from(b));
        Some(match call.action {
            Action::PgsCreateKey => {
                let len = usize::try_from(int(0)).unwrap_or(0).clamp(1, MOST);
                if !self.pgs.create_int(key, len) {
                    self.count_unhandled(call.action);
                }
                HostValue::Nothing
            }
            Action::PgsKeyExists => flag(self.pgs.values(key).is_some()),
            Action::PgsSetKeyVal => {
                let (index, value) = (int(0), int(1));
                let slot = self
                    .pgs
                    .values(key)
                    .and_then(|v| v.get_mut(usize::try_from(index).ok()?));
                if let Some(slot) = slot {
                    *slot = value;
                    self.pgs_changed();
                }
                HostValue::Nothing
            }
            Action::PgsGetKeyVal => {
                let index = usize::try_from(int(0)).ok();
                let v = self.pgs.values(key).and_then(|v| v.get(index?).copied());
                HostValue::Int(v.unwrap_or(0))
            }
            Action::PgsCreateStrKey => {
                if !self.pgs.create_text(key) {
                    self.count_unhandled(call.action);
                }
                HostValue::Nothing
            }
            Action::PgsStrKeyExists => flag(self.pgs.text(key).is_some()),
            Action::PgsSetStrKeyVal => {
                let value = call.texts.get(1).copied().unwrap_or(Text::EMPTY);
                if let Some(text) = self.pgs.text(key) {
                    *text = value;
                    self.pgs_changed();
                }
                HostValue::Nothing
            }
            Action::PgsGetStrKeyVal => {
                *call.text_out = self.pgs.text(key).copied().unwrap_or(Text::EMPTY);
                HostValue::Text
            }
            _ => return None,
        })
    }

    /// Queues one `on pgs_changed` run for the changes so far, a sample
    /// later: a callback that sets a key on every run cannot hold the block.
    fn pgs_changed(&mut self) {
        if !self.pgs.queued
            && self
                .plan
                .scripts
                .iter()
                .any(|s| s.callbacks.pgs_changed.is_some())
        {
            let next = crate::SampleTime(self.now.0 + 1);
            self.pgs.queued = self.queue.push(next, Work::PgsChanged);
        }
    }

    /// Runs `on pgs_changed` in every slot, in slot order.
    pub(crate) fn run_pgs_changed(&mut self) {
        self.pgs.queued = false;
        let plan = Arc::clone(&self.plan);
        for (slot, script) in (0u8..).zip(&plan.scripts) {
            if let Some(entry) = script.callbacks.pgs_changed {
                self.run_callback(slot, entry, Running::new(Kind::Pgs, None));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_admission_limit_reports_refusals_and_keeps_duplicates() {
        let mut pgs = Pgs::new();
        for i in 0..INIT_KEYS {
            let name = Text::from_str_cut(&format!("k{i}")).0;
            assert!(pgs.create_int(name, 1), "integer key {i}");
            assert!(pgs.create_text(name), "string key {i}");
        }
        let existing = Text::from_str_cut("k0").0;
        let extra = Text::from_str_cut("overflow").0;
        assert!(pgs.create_int(existing, MOST));
        assert!(pgs.create_text(existing));
        assert_eq!(pgs.values(existing).unwrap().len(), 1);
        assert!(!pgs.create_int(extra, 1));
        assert!(!pgs.create_text(extra));
        pgs.prepare_runtime();
        assert!(pgs.create_int(extra, MOST));
        assert!(pgs.create_text(extra));
    }

    /// A callback reading the last of a full set of keys, 64 times, must
    /// fit one 64-frame block at 48 kHz (1.333 ms) with room to spare.
    /// Run with `--ignored --release`.
    #[test]
    #[ignore = "timing; run in release"]
    fn the_last_key_is_found_well_inside_a_block() {
        let mut pgs = Pgs::new();
        for i in 0..INIT_KEYS {
            let name = Text::from_str_cut(&format!("key_{i}")).0;
            assert!(pgs.create_int(name, 1));
            assert!(pgs.create_text(name));
        }
        let last = Text::from_str_cut(&format!("key_{}", INIT_KEYS - 1)).0;
        let start = std::time::Instant::now();
        let runs = 1000;
        for _ in 0..runs {
            for _ in 0..64 {
                assert!(std::hint::black_box(pgs.values(last)).is_some());
                assert!(std::hint::black_box(pgs.text(last)).is_some());
            }
        }
        let per_block = start.elapsed() / runs;
        eprintln!("64 last-key reads of each kind: {per_block:?}");
        assert!(per_block.as_secs_f64() < 1.333e-3 / 4.0, "{per_block:?}");
    }
}
