//! The engine's side of the scripts' UI calls: it names the control a call
//! addresses and hands the call to [`kontra_script_ui::Store`]; the look
//! parameters that live with a control's number parameters (`hide_part`,
//! `move_control`, ...) are written here.

use kontra_script_ui::{Key, KeyRange, MenuItem, Page};
use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;

/// One control as the script left it: what the host builds its widget from.
#[derive(Clone, Copy, Debug)]
pub struct ControlLook<'a> {
    pub value: Option<i64>,
    /// Number parameters it set, by parameter id.
    pub pars: &'a [(i32, i32)],
    /// Text parameters it set, by parameter id.
    pub texts: &'a [(i32, Text)],
    pub menu: &'a [MenuItem],
    /// Moves on every write to its look (not its value, which lives in a
    /// script cell): a UI redraws the control when this or the value moved.
    pub generation: u32,
}

impl Engine {
    /// The keyboard's keys as the scripts left them, by note number.
    #[must_use]
    pub fn keys(&self) -> &[Key] {
        self.ui.keys()
    }

    /// The named key ranges, in the order they were set.
    #[must_use]
    pub fn key_ranges(&self) -> &[KeyRange] {
        self.ui.key_ranges()
    }

    /// The entries of script `slot`'s menu with UI id `id`.
    #[must_use]
    pub fn menu(&self, slot: u8, id: i32) -> &[MenuItem] {
        self.ui.menu(slot, self.control_index_in(slot, id))
    }

    /// Script `slot`'s page.
    #[must_use]
    pub fn page(&self, slot: u8) -> Option<&Page> {
        self.ui.page(slot)
    }

    /// Script `slot`'s control with UI id `id`, as the script left it.
    #[must_use]
    pub fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>> {
        let n = self.control_index_in(slot, id)?;
        let s = usize::from(slot);
        let cell = self.plan.scripts.get(s)?.controls.cells.get(n).copied()?;
        Some(ControlLook {
            value: cell.and_then(|c| self.script_cell_in(slot, c as usize)),
            pars: self.control_pars.get(s)?.get(n).map_or(&[], Vec::as_slice),
            texts: self.ui.texts(slot, n),
            menu: self.ui.menu(slot, Some(n)),
            generation: self.ui.generation(slot, n),
        })
    }

    /// Cell `n` of script `slot` (a table's values, say).
    #[must_use]
    pub fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64> {
        self.vms.get(usize::from(slot))?.as_ref()?.cell(n)
    }

    /// Marks control `n` of the running script as changed in look.
    pub(crate) fn touch(&mut self, n: usize) {
        self.ui.touch(self.running.slot, n);
    }

    /// `$CONTROL_PAR_NUM_ITEMS` and `$CONTROL_PAR_SELECTED_ITEM_IDX` of a menu
    /// whose value is `value`; `None` for other parameters.
    pub(crate) fn menu_par(&self, id: i32, par: i32, value: i32) -> Option<i32> {
        let controls = &self.script()?.controls;
        let items = self.menu(self.running.slot, id);
        if items.is_empty() {
            return None;
        }
        if par == controls.num_items_par {
            return i32::try_from(items.len()).ok();
        }
        if par != controls.selected_item_par {
            return None;
        }
        let at = items.iter().position(|i| i.value == value);
        Some(at.and_then(|n| i32::try_from(n).ok()).unwrap_or(-1))
    }

    /// Answers the menu, control-text, look and keyboard actions; `None` for others.
    pub(crate) fn ui_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let (id, a, b) = (int(0), int(1), int(2));
        let slot = self.running.slot;
        let n = self.control_index_in(slot, id);
        let look = self.script().map(|s| s.controls.look);
        if let Some(value) = self.ui.call(slot, n, look.as_ref(), call) {
            return Some(value);
        }
        let look = look?;
        let pars: &[(i32, i32)] = match call.action {
            Action::SetKnobUnit => &[(look.unit, a)],
            Action::SetKnobDefval => &[(look.default, a)],
            Action::HidePart => &[(look.hide, a)],
            Action::SetTableStepsShown => &[(look.steps_shown, a)],
            Action::MoveControl => &[(look.grid_x, a), (look.grid_y, b)],
            Action::MoveControlPx => &[(look.pos_x, a), (look.pos_y, b)],
            _ => return None,
        };
        for &(par, value) in pars {
            self.set_control_par(call.cells, id, par, value);
        }
        Some(HostValue::Nothing)
    }
}
