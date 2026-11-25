//! The control store, as the running script addresses it: what it reads
//! back with `get_control_par` and sets with `set_control_par`. The rules
//! live with the store ([`kontra_script_ui::Controls`]); the engine names
//! the script and lends its cells.

use kontra_script_ui::{Controls, Store};

use crate::Engine;

impl Engine {
    /// The running script's controls and the store, borrowed apart.
    fn controls(&mut self) -> Option<(&Controls, &mut Store, u8)> {
        let slot = self.running.slot;
        let script = self.plan.scripts.get(usize::from(slot))?;
        Some((&script.controls, &mut self.ui, slot))
    }

    /// What saved `value` restores in `cell` of script `slot`.
    pub(crate) fn saved_value(&mut self, slot: u8, cell: u32, value: i64) -> i64 {
        match self.plan.scripts.get(usize::from(slot)) {
            Some(s) => self.ui.saved_value(&s.controls, slot, cell, value),
            None => value,
        }
    }

    /// `fs_navigate(id, direction)` in script `slot`: asks the control side to
    /// pick the file before (0) or after (1) the one selector `id` picked,
    /// which runs its callback (script manual: always, so also at either end
    /// of the list, where the pick stays; that end is not measured). A full
    /// queue drops it.
    pub(crate) fn navigate(&mut self, slot: u8, id: i32, direction: i32) {
        let Some(script) = self.plan.scripts.get(usize::from(slot)) else {
            return;
        };
        let Some(n) = script.controls.index(id) else {
            return;
        };
        let (path, files) = self.ui.selector(slot, n, &script.controls.look);
        if self.files.len() < crate::script_files::FILES {
            self.files.push(kontra_plan::files::FileRequest {
                slot,
                id: 0,
                base: 0,
                len: 0,
                kind: kontra_plan::files::ArrayKind::Int,
                path,
                name: kontra_vm::Text::EMPTY,
                save: false,
                midi: None,
                detect: None,
                navigate: Some(kontra_plan::files::Navigate {
                    control: id,
                    next: direction == 1,
                    files,
                }),
            });
        }
    }

    /// Index of the control with UI id `id` in script `slot`.
    pub(crate) fn control_index_in(&self, slot: u8, id: i32) -> Option<usize> {
        self.plan.scripts.get(usize::from(slot))?.controls.index(id)
    }

    pub(crate) fn control_par(&mut self, cells: &[i64], id: i32, par: i32) -> i32 {
        (self.controls()).map_or(0, |(c, ui, slot)| ui.control_par(c, slot, cells, id, par))
    }

    /// `set_control_par_arr`: a table's step, else index 0 as `set_control_par`.
    pub(crate) fn set_control_par_at(
        &mut self,
        cells: &mut [i64],
        id: i32,
        par: i32,
        value: i32,
        at: i32,
    ) {
        if let Some((c, ui, slot)) = self.controls() {
            ui.set_control_par_at(c, slot, cells, (id, par, value, at));
        }
    }

    /// `get_control_par_arr`: a table's step, else the parameter itself.
    pub(crate) fn control_par_at(&mut self, cells: &[i64], id: i32, par: i32, at: i32) -> i32 {
        (self.controls()).map_or(0, |(c, ui, slot)| {
            ui.control_par_at(c, slot, cells, (id, par, at))
        })
    }

    pub(crate) fn set_control_par(&mut self, cells: &mut [i64], id: i32, par: i32, value: i32) {
        if let Some((c, ui, slot)) = self.controls() {
            ui.set_control_par(c, slot, cells, id, par, value);
        }
    }

    /// Answers the menu, control-text, look and keyboard actions; `None` for others.
    pub(crate) fn ui_call(
        &mut self,
        call: &mut kontra_vm::HostCall<'_>,
    ) -> Option<kontra_vm::HostValue> {
        match self.controls() {
            Some((c, ui, slot)) => ui.ui_call(c, slot, call),
            None => self.ui.call(self.running.slot, None, None, call),
        }
    }
}
