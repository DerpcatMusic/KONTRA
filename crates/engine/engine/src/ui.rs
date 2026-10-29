//! The engine's side of the scripts' UI calls: it names the control a call
//! addresses and hands the call to [`kontra_script_ui::Store`]; the look
//! parameters that live with a control's number parameters (`hide_part`,
//! `move_control`, ...) are written here.

use kontra_script_ui::{ControlLook, Key, KeyRange, MenuItem, Page};
use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;

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

    /// The user clicked control `id` of script `slot` holding `keys`
    /// (`[shift, alt, control]`); call it before [`Engine::set_control`].
    pub fn set_click_keys(&mut self, slot: u8, id: i32, keys: [bool; 3]) {
        self.ui.click(slot, self.control_index_in(slot, id), keys);
    }

    /// Script `slot`'s control with UI id `id`, as the script left it.
    #[must_use]
    pub fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>> {
        let n = self.control_index_in(slot, id)?;
        let s = usize::from(slot);
        let cell = self.plan.scripts.get(s)?.controls.cells.get(n).copied()?;
        let menu = self.ui.menu(slot, Some(n));
        let mut value = cell.and_then(|c| self.script_cell_in(slot, c as usize));
        // A menu shows the entry it last selected through a value no entry holds.
        if let Some(v) = value
            && !menu.is_empty()
            && !menu.iter().any(|i| i64::from(i.value) == v)
        {
            value = menu
                .get(self.ui.selected(slot, n))
                .map(|i| i64::from(i.value));
        }
        Some(ControlLook {
            value,
            pars: self.control_pars.get(s)?.get(n).map_or(&[], Vec::as_slice),
            texts: self.ui.texts(slot, n),
            menu,
            generation: self.ui.generation(slot, n),
        })
    }

    /// Cell `n` of script `slot` (a table's values, say).
    #[must_use]
    pub fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64> {
        self.vms.get(usize::from(slot))?.as_ref()?.cell(n)
    }

    /// Cell `n` of script `slot` as a save keeps it: a menu's value as the
    /// index of its entry, which a restore turns back into the value (see
    /// `saved_value`); any other cell as it is.
    #[must_use]
    pub fn script_saved_in(&self, slot: u8, n: usize) -> Option<i64> {
        let value = self.script_cell_in(slot, n)?;
        let controls = &self.plan.scripts.get(usize::from(slot))?.controls;
        let control = u32::try_from(n).ok().and_then(|c| controls.of_cell(c));
        let index = control.and_then(|c| self.ui.saved_index(slot, c, value));
        Some(index.map_or(value, |i| i as i64))
    }

    /// Script `slot`'s machine: a Lua page's widgets, the state it saves.
    pub fn vm(&self, slot: u8) -> Option<&kontra_vm::Vm> {
        self.vms.get(usize::from(slot))?.as_ref()
    }
    pub fn vm_mut(&mut self, slot: u8) -> Option<&mut kontra_vm::Vm> {
        self.vms.get_mut(usize::from(slot))?.as_mut()
    }

    /// Every group's amplitude envelope and inserts as they play now (after
    /// init, as its scripts left them). Not real-time.
    #[must_use]
    pub fn group_sounds(&self) -> Vec<(kontra_voice::Ahdsr, Vec<Option<kontra_ir::Effect>>)> {
        (self.groups.iter())
            .map(|g| (g.envelope, g.inserts.clone()))
            .collect()
    }

    /// The editor set group `group`'s amplitude envelope and, when given,
    /// one of its inserts: notes from the next one on take them, and
    /// sounding notes re-read what they can (as a script's write).
    pub fn set_group_sound(
        &mut self,
        group: usize,
        envelope: kontra_voice::Ahdsr,
        insert: Option<(usize, kontra_ir::Effect)>,
    ) {
        let Some(g) = self.groups.get_mut(group) else {
            return;
        };
        g.envelope = envelope;
        if let Some((slot, effect)) = insert
            && let Some(at) = g.inserts.get_mut(slot)
        {
            *at = Some(effect);
        }
        self.refresh_all(None);
    }

    /// Text cell `n` of script `slot` (a persistent `@` variable, say).
    #[must_use]
    pub fn script_text_in(&self, slot: u8, n: usize) -> Option<Text> {
        self.vms.get(usize::from(slot))?.as_ref()?.text(n).copied()
    }

    /// Marks control `n` of the running script as changed in look.
    pub(crate) fn touch(&mut self, n: usize) {
        self.ui.touch(self.running.slot, n);
    }

    /// `$CONTROL_PAR_NUM_ITEMS`, and `$CONTROL_PAR_SELECTED_ITEM_IDX` or
    /// `$CONTROL_PAR_VALUE` (both the selected index, KSP manual
    /// `add_menu_item`) of a menu whose value is `value`: the first entry
    /// with it, else the entry it last selected. `None` for other parameters.
    // ponytail: the selection follows the values the engine sees (reads and
    // user edits); a script that assigns a matching then an unmatched value
    // with no read between keeps the selection before both.
    pub(crate) fn menu_par(&mut self, id: i32, par: i32, value: i32) -> Option<i32> {
        let controls = &self.script()?.controls;
        let (items, par_of) = (controls.num_items_par, controls.selected_item_par);
        let value_par = controls.value_par;
        let count = self.menu(self.running.slot, id).len();
        if count == 0 {
            return None;
        }
        if par == items {
            return i32::try_from(count).ok();
        }
        if par != par_of && par != value_par {
            return None;
        }
        let (slot, n) = (
            self.running.slot,
            self.control_index_in(self.running.slot, id),
        );
        let at = self.ui.select(slot, n, value)?;
        i32::try_from(at).ok()
    }

    /// The picture behind the font id `id` script `slot` got from
    /// `get_font_id`: a bitmap font the host draws from `pictures/<name>`.
    #[must_use]
    pub fn font(&self, slot: u8, id: i32) -> Option<&str> {
        self.ui.font(slot, id)
    }

    /// Answers the menu, control-text, look and keyboard actions; `None` for others.
    pub(crate) fn ui_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let (id, a, b) = (int(0), int(1), int(2));
        let slot = self.running.slot;
        let n = self.control_index_in(slot, id);
        let look = self.script().map(|s| s.controls.look);
        // A menu selects its first entry as it gets it (Kontakt 8.13 oracle).
        let first = call.action == Action::AddMenuItem && self.menu(slot, id).is_empty();
        if let Some(value) = self.ui.call(slot, n, look.as_ref(), call) {
            if first && !self.menu(slot, id).is_empty() {
                let par = self.script().map_or(-1, |s| s.controls.value_par);
                self.set_control_par(call.cells, id, par, 0);
            }
            return Some(value);
        }
        let look = look?;
        let pars: &[(i32, i32)] = match call.action {
            Action::SetKnobUnit => &[(look.unit, a)],
            Action::SetKnobDefval => &[(look.default, a)],
            Action::HidePart => &[(look.hide, a)],
            Action::SetTableStepsShown => &[(look.steps_shown, a)],
            Action::AttachLevelMeter => &[(look.meter, int(3))],
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
