//! What the host reads of the scripts' UI: keys, menus, pages and each
//! control's look, from [`kontra_script_ui::Store`] and the script cells.

use kontra_script_ui::{ControlLook, Key, KeyRange, MenuItem, Page};
use kontra_vm::Text;

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

    /// The user clicked cell `cell` (0 but in a table) of control `id` of script `slot`
    /// holding `keys` (`[shift, alt, control]`); then [`Engine::set_control`] sets it.
    pub fn set_click(&mut self, slot: u8, id: i32, keys: [bool; 3], cell: u32) {
        self.ui
            .click(slot, self.control_index_in(slot, id), keys, cell);
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
            pars: self.ui.pars(slot, n),
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

    /// The picture behind the font id `id` script `slot` got from
    /// `get_font_id`: a bitmap font the host draws from `pictures/<name>`.
    #[must_use]
    pub fn font(&self, slot: u8, id: i32) -> Option<&str> {
        self.ui.font(slot, id)
    }
}
