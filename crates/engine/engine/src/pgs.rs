//! Keys every script slot shares (KSP's PGS): integer arrays of up to 256
//! elements and strings. Setting a value runs `on pgs_changed` in each slot.

use std::sync::Arc;

use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;
use crate::queue::Work;
use crate::script::{Kind, Running};

/// Elements a key may hold (KSP manual, PGS).
const MOST: usize = 256;

pub(crate) struct Pgs {
    ints: Vec<(Text, Box<[i32]>)>,
    texts: Vec<(Text, Text)>,
    /// An `on pgs_changed` run waits in the queue.
    queued: bool,
}

impl Pgs {
    pub(crate) fn new() -> Self {
        Self {
            ints: Vec::new(),
            texts: Vec::new(),
            queued: false,
        }
    }

    fn values(&mut self, key: Text) -> Option<&mut [i32]> {
        self.ints
            .iter_mut()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| &mut **v)
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
    // ponytail: a key first created after init allocates on the audio thread.
    pub(crate) fn pgs_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        let key = call.texts.first().copied().unwrap_or(Text::EMPTY);
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let flag = |b: bool| HostValue::Int(i32::from(b));
        Some(match call.action {
            Action::PgsCreateKey => {
                if self.pgs.values(key).is_none() {
                    let len = usize::try_from(int(0)).unwrap_or(0).clamp(1, MOST);
                    self.pgs.ints.push((key, vec![0; len].into()));
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
                if self.pgs.text(key).is_none() {
                    self.pgs.texts.push((key, Text::EMPTY));
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

    /// Queues one `on pgs_changed` run for the changes so far.
    fn pgs_changed(&mut self) {
        if !self.pgs.queued
            && self
                .plan
                .scripts
                .iter()
                .any(|s| s.callbacks.pgs_changed.is_some())
        {
            self.pgs.queued = self.queue.push(self.now, Work::PgsChanged);
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
