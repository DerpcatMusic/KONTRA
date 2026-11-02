//! Keys every script slot shares (KSP's PGS): integer arrays of up to 256
//! elements and strings. Setting a value runs `on pgs_changed` in each slot.

use std::sync::Arc;

use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;
use crate::queue::Work;
use crate::script::{Kind, Running};

/// Elements a key may hold (KSP manual, PGS).
const MOST: usize = 256;
/// Extra keys of each kind admitted after initialization.
const RUNTIME_KEYS: usize = 64;
/// Preparation admission bound per key kind; overflow is reported as unhandled.
const INIT_KEYS: usize = 8192;

struct IntKey {
    name: Text,
    start: usize,
    len: usize,
}

/// Prepared PGS limits and backing allocation, in bytes (excluding allocator overhead).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgsBudget {
    pub integer_keys: usize,
    pub string_keys: usize,
    pub integer_values: usize,
    pub bytes: usize,
}

pub(crate) struct Pgs {
    ints: Vec<IntKey>,
    values: Vec<i32>,
    texts: Vec<(Text, Text)>,
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
            int_limit: 0,
            text_limit: 0,
            value_limit: 0,
            queued: false,
        }
    }

    /// Called after all slots have initialized, before the engine reaches audio.
    pub(crate) fn prepare_runtime(&mut self) {
        self.int_limit = self.ints.len() + RUNTIME_KEYS;
        self.text_limit = self.texts.len() + RUNTIME_KEYS;
        self.value_limit = self.values.len() + RUNTIME_KEYS * MOST;
        self.ints.reserve_exact(RUNTIME_KEYS);
        self.texts.reserve_exact(RUNTIME_KEYS);
        self.values.reserve_exact(RUNTIME_KEYS * MOST);
    }

    pub(crate) fn budget(&self) -> PgsBudget {
        PgsBudget {
            integer_keys: self.int_limit,
            string_keys: self.text_limit,
            integer_values: self.value_limit,
            bytes: self.ints.capacity() * std::mem::size_of::<IntKey>()
                + self.texts.capacity() * std::mem::size_of::<(Text, Text)>()
                + self.values.capacity() * std::mem::size_of::<i32>(),
        }
    }

    fn create_int(&mut self, key: Text, len: usize, building: bool) -> bool {
        if self.ints.iter().any(|entry| entry.name == key) {
            return true;
        }
        if (building && self.ints.len() >= INIT_KEYS)
            || (!building
                && (self.ints.len() >= self.int_limit
                    || self.values.len() + len > self.value_limit))
        {
            return false;
        }
        let start = self.values.len();
        self.values.resize(start + len, 0);
        self.ints.push(IntKey {
            name: key,
            start,
            len,
        });
        true
    }

    fn create_text(&mut self, key: Text, building: bool) -> bool {
        if self.texts.iter().any(|(name, _)| *name == key) {
            return true;
        }
        if (building && self.texts.len() >= INIT_KEYS)
            || (!building && self.texts.len() >= self.text_limit)
        {
            return false;
        }
        self.texts.push((key, Text::EMPTY));
        true
    }

    fn values(&mut self, key: Text) -> Option<&mut [i32]> {
        let entry = self.ints.iter().find(|entry| entry.name == key)?;
        self.values.get_mut(entry.start..entry.start + entry.len)
    }

    fn text(&mut self, key: Text) -> Option<&mut Text> {
        self.texts
            .iter_mut()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }
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
                if !self.pgs.create_int(key, len, self.building) {
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
                if !self.pgs.create_text(key, self.building) {
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
