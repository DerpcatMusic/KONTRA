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
    /// voice fades out, every slot's values land, then each slot's
    /// persistence-changed callback runs, `read_persistent_var` reading the
    /// recalled values. Returns the values recalled before, to free off the
    /// audio thread. Allocates nothing.
    pub fn recall(&mut self, saved: Box<[Saved]>) -> Box<[Saved]> {
        self.steal_where(self.fade_frames(), |_| true);
        for (slot, values) in (0u8..).zip(saved.iter()) {
            self.write_saved(slot, &values.ints, &values.texts);
        }
        let old = std::mem::replace(&mut self.recalled, saved);
        for slot in (0u8..).take(self.recalled.len()) {
            self.persistence_changed(slot);
        }
        old
    }

    /// `read_persistent_var`: the saved values of the running slot in
    /// `range` (texts or numbers) back into its cells, as recalled last or
    /// as loaded.
    pub(crate) fn read_persistent(
        &mut self,
        range: std::ops::Range<u32>,
        text: bool,
        cells: &mut [i64],
        text_cells: &mut [Text],
    ) {
        let slot = self.running.slot;
        let recalled = std::mem::take(&mut self.recalled);
        let plan = std::sync::Arc::clone(&self.plan);
        let (ints, texts) = match (
            recalled.get(usize::from(slot)),
            plan.scripts.get(usize::from(slot)),
        ) {
            (Some(s), _) => (s.ints.as_slice(), s.texts.as_slice()),
            (None, Some(s)) => (s.restore.as_slice(), s.restore_texts.as_slice()),
            (None, None) => (&[][..], &[][..]),
        };
        if text {
            for &(cell, value) in texts.iter().filter(|(c, _)| range.contains(c)) {
                if let Some(t) = text_cells.get_mut(cell as usize) {
                    *t = value;
                }
            }
        } else {
            for &(cell, value) in ints.iter().filter(|(c, _)| range.contains(c)) {
                let value = self.saved_value(slot, cell, value);
                if let Some(c) = cells.get_mut(cell as usize) {
                    *c = value;
                }
            }
        }
        self.recalled = recalled;
    }

    /// Writes saved values into slot `slot`'s cells (a menu's saved entry
    /// back to its value), then runs its persistence-changed callback.
    pub(crate) fn restore(&mut self, slot: u8, ints: &[(u32, i64)], texts: &[(u32, Text)]) {
        self.write_saved(slot, ints, texts);
        self.persistence_changed(slot);
    }

    fn write_saved(&mut self, slot: u8, ints: &[(u32, i64)], texts: &[(u32, Text)]) {
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
    }

    fn persistence_changed(&mut self, slot: u8) {
        let script = self.plan.scripts.get(usize::from(slot));
        if let Some(changed) = script.and_then(|s| s.callbacks.persistence_changed) {
            self.run_callback(slot, changed, Running::new(Kind::Init, None));
        }
    }
}
