//! Recalling saved values into the running scripts (a preset of the loaded
//! instrument) without building the engine again; see [`Recall`].

use kontra_plan::{Recall, Saved};
use kontra_vm::Text;

use crate::Engine;
use crate::script::{Kind, Running};

impl Engine {
    /// What a recall does to this instrument's scripts, as `init` asked.
    #[must_use]
    pub fn recall_law(&self) -> Recall {
        self.recall
    }

    /// Recalls `saved` (by slot) into the scripts, keeping their state: every
    /// voice fades out, each slot's values land, then its persistence-changed
    /// callback runs. Allocates nothing.
    pub fn recall(&mut self, saved: &[Saved]) {
        self.steal_where(self.fade_frames(), |_| true);
        for (slot, values) in (0u8..).zip(saved) {
            self.restore(slot, &values.ints, &values.texts);
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
            texts
                .iter()
                .for_each(|&(cell, t)| vm.set_text(cell as usize, t));
        }
        let script = self.plan.scripts.get(usize::from(slot));
        if let Some(changed) = script.and_then(|s| s.callbacks.persistence_changed) {
            self.run_callback(slot, changed, Running::new(Kind::Init, None));
        }
    }
}
