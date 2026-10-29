//! Recalling saved values into a running instrument (a preset or snapshot of
//! the instrument already loaded) without building it again.
//!
//! Scripts choose what a recall does to them ([`Recall`]); the engine keeps
//! its plan, samples and effects either way. A [`Recall::Keep`] recall happens
//! in place, here, on the audio thread; a [`Recall::Restart`] one starts a new
//! engine on the same plan, whose `init` runs on a fresh state.

use kontra_vm::Text;

use crate::Engine;
use crate::script::{Kind, Running};

/// What recalling saved values does to an instrument's scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recall {
    /// `init` runs again on a fresh state, then the values land, then the
    /// persistence-changed callback runs.
    Restart,
    /// The scripts keep their state: the values land, then the
    /// persistence-changed callback runs.
    Keep,
}

impl Recall {
    /// The law a script's snapshot type picks: even types run `init` again,
    /// odd ones keep the state (KSP manual, `set_snapshot_type`).
    fn of_type(kind: i64) -> Self {
        if kind & 1 == 0 {
            Self::Restart
        } else {
            Self::Keep
        }
    }
}

/// One script slot's saved values, as cell writes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub ints: Vec<(u32, i64)>,
    pub texts: Vec<(u32, Text)>,
}

impl Engine {
    /// What a recall of saved values does to this instrument's scripts, as
    /// they asked during `init`.
    #[must_use]
    pub fn recall_law(&self) -> Recall {
        self.recall
    }

    /// A script set the snapshot type.
    pub(crate) fn set_snapshot_type(&mut self, kind: i64) {
        self.recall = Recall::of_type(kind);
    }

    /// Recalls `saved` (by script slot) into the running scripts, keeping
    /// their state: every voice fades out, each slot's values land, then its
    /// persistence-changed callback runs. A slot missing from `saved` keeps
    /// its values and still hears the callback. Allocates nothing.
    pub fn recall(&mut self, saved: &[Saved]) {
        let fade = self.fade_frames();
        self.steal_where(fade, |_| true);
        for slot in 0..self.plan.scripts.len() {
            let Ok(slot) = u8::try_from(slot) else {
                break;
            };
            let values = saved.get(usize::from(slot));
            let ints = values.map_or(&[][..], |s| s.ints.as_slice());
            let texts = values.map_or(&[][..], |s| s.texts.as_slice());
            self.restore(slot, ints, texts);
        }
    }

    /// Writes saved values into slot `slot`'s cells (a menu's saved entry
    /// back to its value), then runs its persistence-changed callback.
    pub(crate) fn restore(&mut self, slot: u8, ints: &[(u32, i64)], texts: &[(u32, Text)]) {
        for &(cell, value) in ints {
            let value = self.saved_value(slot, cell, value);
            if let Some(vm) = self.vm_mut(slot) {
                vm.set_cell(cell as usize, value);
            }
        }
        if let Some(vm) = self.vm_mut(slot) {
            for &(cell, text) in texts {
                vm.set_text(cell as usize, text);
            }
        }
        let changed = (self.plan.scripts.get(usize::from(slot)))
            .and_then(|s| s.callbacks.persistence_changed);
        if let Some(changed) = changed {
            self.run_callback(slot, changed, Running::new(Kind::Init, None));
        }
    }
}
