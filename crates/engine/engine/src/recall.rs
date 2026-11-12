//! Recalling saved values into the running scripts (a preset of the loaded
//! instrument) without building the engine again; see [`Recall`].

use kontra_plan::{Recall, Saved};
use kontra_vm::Text;

use crate::Engine;
use crate::queue::{EventQueue, Work};
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
        self.reset();
        for (slot, values) in (0u8..).zip(saved.iter()) {
            self.write_saved(slot, &values.ints, &values.texts);
        }
        let old = std::mem::replace(&mut self.recalled, saved);
        let slots = (0u8..).take(self.recalled.len());
        slots.for_each(|slot| self.persistence_changed(slot));
        old
    }

    /// Resets the engine as a snapshot load does (KSP manual: "Loading a
    /// snapshot always resets Kontakt's audio engine"): every voice fades
    /// out, every note counts as released (its pending end starts nothing)
    /// and every callback waiting to go on ends. Allocates nothing.
    pub fn reset(&mut self) {
        self.steal_where(self.fade_frames(), |_| true);
        self.notes.release_all();
        let mut queue = std::mem::replace(&mut self.queue, EventQueue::new(0));
        queue.retain(|work| match *work {
            Work::Resume(slot, co, ..) => {
                if let Some(vm) = self.vm_mut(slot) {
                    vm.cancel(co);
                }
                false
            }
            _ => true,
        });
        self.queue = queue;
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
        if self.building {
            self.reloaded.push((slot, range.clone(), text));
        }
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
            for &(cell, value) in in_range(texts, &range) {
                if let Some(t) = text_cells.get_mut(cell as usize) {
                    *t = value;
                }
            }
        } else {
            for &(cell, value) in in_range(ints, &range) {
                let value = self.saved_value(slot, cell, value);
                if let Some(c) = cells.get_mut(cell as usize) {
                    *c = value;
                }
            }
        }
        self.recalled = recalled;
    }

    /// The end of `on init` at load: slot `slot`'s saved values land, except
    /// in the cells `read_persistent_var` already reloaded. The script's
    /// later writes to those stand (Kontakt 8.13.1: ANALOG reloads
    /// `%steps__n`, zeroes two of its cells and saves them as 0).
    pub(crate) fn restore_after_init(&mut self, slot: u8) {
        let plan = std::sync::Arc::clone(&self.plan);
        let Some(script) = plan.scripts.get(usize::from(slot)) else {
            return;
        };
        let reloaded = std::mem::take(&mut self.reloaded);
        let fresh = |cell: u32, text: bool| {
            let mut here = reloaded.iter().filter(|r| r.0 == slot && r.2 == text);
            !here.any(|r| r.1.contains(&cell))
        };
        for &(cell, value) in script.restore.iter().filter(|e| fresh(e.0, false)) {
            self.write_saved(slot, &[(cell, value)], &[]);
        }
        for &(cell, text) in script.restore_texts.iter().filter(|e| fresh(e.0, true)) {
            self.write_saved(slot, &[], &[(cell, text)]);
        }
        self.reloaded = reloaded;
    }

    /// Writes saved values into slot `slot`'s cells (a menu's saved entry
    /// back to its value).
    pub(crate) fn write_saved(&mut self, slot: u8, ints: &[(u32, i64)], texts: &[(u32, Text)]) {
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

    pub(crate) fn persistence_changed(&mut self, slot: u8) {
        let script = self.plan.scripts.get(usize::from(slot));
        if let Some(changed) = script.and_then(|s| s.callbacks.persistence_changed) {
            self.run_callback(slot, changed, Running::new(Kind::Init, None));
        }
    }
}

/// The writes to cells in `range`, of `saved` sorted by cell.
fn in_range<'a, T>(saved: &'a [(u32, T)], range: &std::ops::Range<u32>) -> &'a [(u32, T)] {
    let from = saved.partition_point(|(cell, _)| *cell < range.start);
    let to = saved.partition_point(|(cell, _)| *cell < range.end);
    saved.get(from..to).unwrap_or_default()
}
