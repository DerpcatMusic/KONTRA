//! A script's controls as its calls address them: UI ids, the cells their
//! values live in, and the other parameters a script sets and reads back
//! (`get_control_par`, `set_control_par`, `_arr`, saved menu values).
//!
//! A KSP control *is* its variable: `$CONTROL_PAR_VALUE` reads and writes the
//! variable's cell, so `$sw := 1` and `set_control_par(id, VALUE, 1)` are the
//! same write. While a callback runs the VM is lent to the interpreter and its
//! cells reach the engine only through the call, so value access takes them
//! as an argument. Other parameters (hide, picture state, range) are kept per
//! control as the script sets them.

use kontra_vm::{Action, HostCall, HostValue};

use crate::{LookPars, Store};

/// The script's UI controls as the engine sees them: ids, and where values live.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    /// The UI id of variable 0.
    pub first: i32,
    /// The parameter id that means a control's value.
    pub value_par: i32,
    /// The parameter ids of a menu's entry count and selected entry.
    pub num_items_par: i32,
    pub selected_item_par: i32,
    /// By UI id from `first` (every variable has one), the value cell of
    /// each control whose value is a number.
    pub cells: Vec<Option<u32>>,
    /// By UI id, the steps of each control whose value is an int array (a
    /// table's): `set_control_par_arr` writes them from its cell on.
    pub steps: Vec<u32>,
    /// (value cell, control) for every control with a value cell, sorted by
    /// cell: a save finds each cell's control in log time instead of
    /// scanning every control per cell (Areia: seconds on the audio thread).
    pub by_cell: Vec<(u32, u32)>,
    /// Where the UI-look builtins store what they set.
    pub look: LookPars,
    /// The parameter ids that read whether Shift, Alt and Ctrl were held at
    /// the control's last click; `i32::MIN` when the script never names one.
    pub key_pars: [i32; 3],
}

impl Controls {
    /// The control whose value lives in `cell`.
    #[must_use]
    pub fn of_cell(&self, cell: u32) -> Option<usize> {
        let at = self.by_cell.binary_search_by_key(&cell, |&(c, _)| c).ok()?;
        self.by_cell.get(at).map(|&(_, n)| n as usize)
    }
}

impl Controls {
    /// Index of the control with UI id `id`.
    #[must_use]
    pub fn index(&self, id: i32) -> Option<usize> {
        let n = usize::try_from(id.checked_sub(self.first)?).ok()?;
        (n < self.cells.len()).then_some(n)
    }

    /// Index of the control with `id`, and its value cell if `par` is the value.
    fn control(&self, id: i32, par: i32) -> Option<(usize, Option<usize>)> {
        let n = self.index(id)?;
        let cell = self.cells.get(n)?;
        let value = (par == self.value_par).then_some(*cell).flatten();
        Some((n, value.map(|c| c as usize)))
    }

    /// The cell of step `at` of the int array control `id` when `par` is its value.
    fn table_step(&self, id: i32, par: i32, at: i32) -> Option<usize> {
        let (n, cell) = self.control(id, par)?;
        let len = *self.steps.get(n)?;
        let at = u32::try_from(at).ok().filter(|&at| at < len)?;
        Some(cell? + at as usize)
    }
}

impl Store {
    /// The number parameters control `n` of script `slot` set, by parameter id.
    #[must_use]
    pub fn pars(&self, slot: u8, n: usize) -> &[(i32, i32)] {
        (self.pars.get(usize::from(slot)))
            .and_then(|s| s.get(n))
            .map_or(&[], Vec::as_slice)
    }

    /// What saved `value` restores in `cell` of script `slot`. A menu saves
    /// the index of its selected entry and restores that entry's value
    /// (Kontakt 8.13 oracle, `probes/menu_save_restore.lua`: 30 of 10/20/30
    /// saves "2" and reads back 30).
    pub fn saved_value(&mut self, c: &Controls, slot: u8, cell: u32, value: i64) -> i64 {
        let n = c.of_cell(cell);
        let index = usize::try_from(value).ok();
        n.zip(index)
            .and_then(|(n, index)| self.pick(slot, n, index))
            .map_or(value, i64::from)
    }

    /// `get_control_par` of control `id` of script `slot`, whose cells are `cells`.
    pub fn control_par(&mut self, c: &Controls, slot: u8, cells: &[i64], id: i32, par: i32) -> i32 {
        if let Some(key) = c.key_pars.iter().position(|&k| k == par && k != i32::MIN) {
            return self.click_key(slot, c.index(id), key);
        }
        let value = self.control_par_of(c, slot, cells, id, par);
        let menu_value = self.control_par_of(c, slot, cells, id, c.value_par);
        self.menu_par(c, slot, id, par, menu_value).unwrap_or(value)
    }

    fn control_par_of(&self, c: &Controls, slot: u8, cells: &[i64], id: i32, par: i32) -> i32 {
        match c.control(id, par) {
            Some((_, Some(cell))) => cells.get(cell).map_or(0, |&v| v as i32),
            Some((n, None)) => (self.pars(slot, n).iter())
                .find(|(p, _)| *p == par)
                .map_or(0, |&(_, v)| v),
            None => 0,
        }
    }

    /// `set_control_par_arr`: a table's step, else index 0 as `set_control_par`.
    pub fn set_control_par_at(
        &mut self,
        c: &Controls,
        slot: u8,
        cells: &mut [i64],
        (id, par, value, at): (i32, i32, i32, i32),
    ) {
        match c.table_step(id, par, at) {
            Some(cell) => {
                if let Some(v) = cells.get_mut(cell) {
                    *v = i64::from(value);
                }
            }
            None if at == 0 => self.set_control_par(c, slot, cells, id, par, value),
            None => {}
        }
    }

    /// `get_control_par_arr`: a table's step, else the parameter itself.
    pub fn control_par_at(
        &mut self,
        c: &Controls,
        slot: u8,
        cells: &[i64],
        (id, par, at): (i32, i32, i32),
    ) -> i32 {
        match c.table_step(id, par, at) {
            Some(cell) => cells.get(cell).map_or(0, |&v| v as i32),
            None => self.control_par(c, slot, cells, id, par),
        }
    }

    /// `set_control_par` of control `id` of script `slot`, whose cells are `cells`.
    // ponytail: a parameter first set after init allocates its slot on the
    // audio thread; preallocate per control if a script does that per note.
    pub fn set_control_par(
        &mut self,
        c: &Controls,
        slot: u8,
        cells: &mut [i64],
        id: i32,
        par: i32,
        value: i32,
    ) {
        // A menu's value selects by index: its variable takes that entry's
        // value, 0 past the end (Kontakt 8.13 oracle, 2026-10-29).
        let items = self.menu(slot, c.index(id));
        let menu = par == c.value_par && !items.is_empty();
        let entry = usize::try_from(value).ok().and_then(|n| items.get(n));
        let picked = entry.map_or(0, |i| i.value);
        let value = if menu { picked } else { value };
        match c.control(id, par) {
            Some((_, Some(cell))) => {
                if let Some(v) = cells.get_mut(cell) {
                    *v = i64::from(value);
                }
            }
            Some((n, None)) => {
                let Some(pars) = (self.pars.get_mut(usize::from(slot))).and_then(|s| s.get_mut(n))
                else {
                    return;
                };
                let changed = match pars.iter_mut().find(|(p, _)| *p == par) {
                    Some(slot) => std::mem::replace(&mut slot.1, value) != value,
                    None => {
                        pars.push((par, value));
                        true
                    }
                };
                if changed {
                    self.touch(slot, n);
                }
            }
            None => {}
        }
    }

    /// `$CONTROL_PAR_NUM_ITEMS`, and `$CONTROL_PAR_SELECTED_ITEM_IDX` or
    /// `$CONTROL_PAR_VALUE` (both the selected index, KSP manual
    /// `add_menu_item`) of a menu whose value is `value`: the first entry
    /// with it, else the entry it last selected. `None` for other parameters.
    // ponytail: the selection follows the values the engine sees (reads and
    // user edits); a script that assigns a matching then an unmatched value
    // with no read between keeps the selection before both.
    fn menu_par(&mut self, c: &Controls, slot: u8, id: i32, par: i32, value: i32) -> Option<i32> {
        let n = c.index(id);
        let count = self.menu(slot, n).len();
        if count == 0 {
            return None;
        }
        if par == c.num_items_par {
            return i32::try_from(count).ok();
        }
        if par != c.selected_item_par && par != c.value_par {
            return None;
        }
        let at = self.select(slot, n, value)?;
        i32::try_from(at).ok()
    }

    /// Answers script `slot`'s menu, control-text, look and keyboard
    /// actions; `None` for others.
    pub fn ui_call(
        &mut self,
        c: &Controls,
        slot: u8,
        call: &mut HostCall<'_>,
    ) -> Option<HostValue> {
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let (id, a, b) = (int(0), int(1), int(2));
        let n = c.index(id);
        let look: LookPars = c.look;
        // A menu selects its first entry as it gets it (Kontakt 8.13 oracle).
        let first = call.action == Action::AddMenuItem && self.menu(slot, n).is_empty();
        if let Some(value) = self.call(slot, n, Some(&look), call) {
            if first && !self.menu(slot, n).is_empty() {
                self.set_control_par(c, slot, call.cells, id, c.value_par, 0);
            }
            return Some(value);
        }
        let pars: &[(i32, i32)] = match call.action {
            Action::SetKnobUnit => &[(look.unit, a)],
            Action::SetKnobDefval => &[(look.default, a)],
            Action::HidePart => &[(look.hide, a)],
            Action::SetTableStepsShown => &[(look.steps_shown, a)],
            Action::AttachLevelMeter => &[
                (look.meter, int(3)),
                (look.meter_group, int(1)),
                (look.meter_generic, int(4)),
            ],
            Action::MoveControl => &[(look.grid_x, a), (look.grid_y, b)],
            Action::MoveControlPx => &[(look.pos_x, a), (look.pos_y, b)],
            _ => return None,
        };
        for &(par, value) in pars {
            self.set_control_par(c, slot, call.cells, id, par, value);
        }
        Some(HostValue::Nothing)
    }
}
