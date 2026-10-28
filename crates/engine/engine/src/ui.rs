//! What scripts write to their controls' look and read back: menu entries,
//! control texts and the keyboard's keys. The host's UI reads it from here.
//!
//! Measured (probes/menu_readback.lua, key_constants.lua): a menu's entries
//! are indexed in the order the script added them; `$CONTROL_PAR_NUM_ITEMS`
//! counts them all, hidden or not; `$CONTROL_PAR_SELECTED_ITEM_IDX` is the
//! index of the entry holding the menu's value; an untouched key reads colour
//! and type 0.

use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;

/// Keys on the keyboard a script colours and names.
const KEYS: usize = 128;

/// One `ui_menu` entry.
#[derive(Clone, Copy, Debug)]
pub struct MenuItem {
    pub text: Text,
    pub value: i32,
    pub visible: bool,
}

/// How the keyboard shows a key, in the script's own numbers.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub color: i32,
    pub kind: i32,
    pub name: Text,
    pub pressed: bool,
}

impl Key {
    const UNTOUCHED: Self = Self {
        color: 0,
        kind: 0,
        name: Text::EMPTY,
        pressed: false,
    };
}

/// A named key range (`set_keyrange`).
#[derive(Clone, Copy, Debug)]
pub struct KeyRange {
    pub low: u8,
    pub high: u8,
    pub name: Text,
}

/// A control's text parameters, by parameter id.
type Texts = Vec<(i32, Text)>;

/// The script's page as it set it: size, wallpaper, colour, title.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub skin_offset: i32,
    pub color: Option<i32>,
    /// `set_ui_height` (grid rows) and the pixel setters.
    pub height_rows: Option<i32>,
    pub height_px: Option<i32>,
    pub width_px: Option<i32>,
    /// `make_perfview`.
    pub performance: bool,
    pub title: Text,
    pub wallpaper: Text,
    pub wallpaper_frame: i32,
    pub icon: Text,
    pub icon_hide: i32,
}

impl Page {
    const UNSET: Self = Self {
        skin_offset: 0,
        color: None,
        height_rows: None,
        height_px: None,
        width_px: None,
        performance: false,
        title: Text::EMPTY,
        wallpaper: Text::EMPTY,
        wallpaper_frame: 0,
        icon: Text::EMPTY,
        icon_hide: 0,
    };
}

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

pub(crate) struct Ui {
    /// Per script slot, by control: its menu entries.
    menus: Box<[Box<[Vec<MenuItem>]>]>,
    /// Per script slot, by control: the text parameters it set.
    texts: Box<[Box<[Texts]>]>,
    keys: Box<[Key]>,
    ranges: Vec<KeyRange>,
    /// Per script slot.
    pages: Box<[Page]>,
    /// Per script slot, by control: see [`ControlLook::generation`].
    generations: Box<[Box<[u32]>]>,
}

impl Ui {
    /// Empty stores for scripts with these control counts.
    pub(crate) fn new(controls: impl Iterator<Item = usize> + Clone) -> Self {
        Self {
            menus: controls
                .clone()
                .map(|n| vec![Vec::new(); n].into())
                .collect(),
            texts: controls
                .clone()
                .map(|n| vec![Vec::new(); n].into())
                .collect(),
            keys: vec![Key::UNTOUCHED; KEYS].into(),
            ranges: Vec::new(),
            pages: controls.clone().map(|_| Page::UNSET).collect(),
            generations: controls.map(|n| vec![0; n].into()).collect(),
        }
    }
}

impl Engine {
    /// The keyboard's keys as the scripts left them, by note number.
    #[must_use]
    pub fn keys(&self) -> &[Key] {
        &self.ui.keys
    }

    /// The named key ranges, in the order they were set.
    #[must_use]
    pub fn key_ranges(&self) -> &[KeyRange] {
        &self.ui.ranges
    }

    /// The entries of script `slot`'s menu with UI id `id`.
    #[must_use]
    pub fn menu(&self, slot: u8, id: i32) -> &[MenuItem] {
        let n = self.control_index_in(slot, id);
        n.and_then(|n| self.ui.menus.get(usize::from(slot))?.get(n))
            .map_or(&[], Vec::as_slice)
    }

    /// Script `slot`'s page.
    #[must_use]
    pub fn page(&self, slot: u8) -> Option<&Page> {
        self.ui.pages.get(usize::from(slot))
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
            texts: self.ui.texts.get(s)?.get(n).map_or(&[], Vec::as_slice),
            menu: self.menu(slot, id),
            generation: self.ui.generations.get(s)?.get(n).copied().unwrap_or(0),
        })
    }

    /// Cell `n` of script `slot` (a table's values, say).
    #[must_use]
    pub fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64> {
        self.vms.get(usize::from(slot))?.as_ref()?.cell(n)
    }

    fn page_mut(&mut self) -> Option<&mut Page> {
        self.ui.pages.get_mut(usize::from(self.running.slot))
    }

    /// Sets text parameter `par` of control `id`; `line` appends a line.
    fn set_text_par(&mut self, id: i32, par: i32, text: Text, line: bool) {
        let Some(texts) = self.texts_mut(id) else {
            return;
        };
        let changed = match texts.iter_mut().find(|(p, _)| *p == par) {
            Some(t) if line && !t.1.as_str().is_empty() => {
                // ponytail: a label past 320 bytes keeps its first lines.
                t.1.push("\n") && t.1.push(text.as_str())
            }
            Some(t) => {
                let changed = t.1.as_str() != text.as_str();
                t.1 = text;
                changed
            }
            None => {
                texts.push((par, text));
                true
            }
        };
        if changed {
            self.touch_id(id);
        }
    }

    /// Marks control `n` of the running script as changed in look.
    /// Only a write that changes what is stored counts, so a script that
    /// sets the same look every tick leaves the UI idle.
    pub(crate) fn touch(&mut self, n: usize) {
        let slot = usize::from(self.running.slot);
        if let Some(g) = self.ui.generations.get_mut(slot).and_then(|g| g.get_mut(n)) {
            *g = g.wrapping_add(1);
        }
    }

    fn touch_id(&mut self, id: i32) {
        if let Some(n) = self.control_index_in(self.running.slot, id) {
            self.touch(n);
        }
    }

    fn menu_mut(&mut self, id: i32) -> Option<&mut Vec<MenuItem>> {
        let slot = usize::from(self.running.slot);
        let n = self.control_index_in(self.running.slot, id)?;
        self.ui.menus.get_mut(slot)?.get_mut(n)
    }

    fn texts_mut(&mut self, id: i32) -> Option<&mut Texts> {
        let slot = usize::from(self.running.slot);
        let n = self.control_index_in(self.running.slot, id)?;
        self.ui.texts.get_mut(slot)?.get_mut(n)
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

    /// `set_control_par` on the instrument's wallpaper or icon.
    fn page_par(&mut self, call: &HostCall<'_>) -> Option<HostValue> {
        if !matches!(
            call.action,
            Action::SetControlPar | Action::SetControlParStr
        ) {
            return None;
        }
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let look = self.script()?.controls.look;
        let (id, par, value) = (int(0), int(1), int(2));
        // Both 0 when the front end set no look ids (Lua).
        if look.wallpaper == look.icon || (id != look.wallpaper && id != look.icon) {
            return None;
        }
        let text = call.texts.first().copied().unwrap_or(Text::EMPTY);
        let wallpaper = id == look.wallpaper;
        let page = self.page_mut()?;
        match (call.action, wallpaper) {
            (Action::SetControlParStr, true) if par == look.picture => page.wallpaper = text,
            (Action::SetControlParStr, false) if par == look.picture => page.icon = text,
            (Action::SetControlPar, true) if par == look.picture_state => {
                page.wallpaper_frame = value;
            }
            (Action::SetControlPar, false) if par == look.hide => page.icon_hide = value,
            _ => {}
        }
        Some(HostValue::Nothing)
    }

    /// Answers the menu, control-text, look and keyboard actions; `None` for others.
    pub(crate) fn ui_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        if let Some(value) = self.page_par(call) {
            return Some(value);
        }
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let text = call.texts.first().copied().unwrap_or(Text::EMPTY);
        let (id, at) = (int(0), usize::try_from(int(1)).ok());
        let key = usize::try_from(int(0)).ok().filter(|&k| k < KEYS);
        let mut out = |t: Text| {
            *call.text_out = t;
            HostValue::Text
        };
        let item = |e: &Self| at.and_then(|n| e.menu(e.running.slot, id).get(n).copied());
        Some(match call.action {
            // ponytail: a menu that grows after init allocates on the audio
            // thread; the manual allows entries only in `on init`.
            Action::AddMenuItem => {
                let value = int(1);
                if let Some(m) = self.menu_mut(id) {
                    m.push(MenuItem {
                        text,
                        value,
                        visible: true,
                    });
                    self.touch_id(id);
                }
                HostValue::Nothing
            }
            Action::SetMenuItemStr | Action::SetMenuItemValue | Action::SetMenuItemVisibility => {
                let (action, value) = (call.action, int(2));
                if let Some(i) = at.and_then(|n| self.menu_mut(id)?.get_mut(n)) {
                    let changed = match action {
                        Action::SetMenuItemStr => i.text.as_str() != text.as_str(),
                        Action::SetMenuItemValue => i.value != value,
                        _ => i.visible != (value != 0),
                    };
                    match action {
                        Action::SetMenuItemStr => i.text = text,
                        Action::SetMenuItemValue => i.value = value,
                        _ => i.visible = value != 0,
                    }
                    if changed {
                        self.touch_id(id);
                    }
                }
                HostValue::Nothing
            }
            Action::GetMenuItemStr => out(item(self).map_or(Text::EMPTY, |i| i.text)),
            Action::GetMenuItemValue => HostValue::Int(item(self).map_or(0, |i| i.value)),
            Action::GetMenuItemVisibility => {
                HostValue::Int(item(self).map_or(0, |i| i32::from(i.visible)))
            }
            // ponytail: a text parameter first set after init allocates its
            // slot on the audio thread, as `set_control_par` does.
            Action::SetControlParStr => {
                let par = int(1);
                self.set_text_par(id, par, text, false);
                HostValue::Nothing
            }
            Action::GetControlParStr => {
                let par = int(1);
                let slot = usize::from(self.running.slot);
                let found = self
                    .control_index_in(self.running.slot, id)
                    .and_then(|n| self.ui.texts.get(slot)?.get(n))
                    .and_then(|t| t.iter().find(|(p, _)| *p == par))
                    .map_or(Text::EMPTY, |&(_, t)| t);
                out(found)
            }
            Action::SetKeyColor
            | Action::SetKeyType
            | Action::SetKeyName
            | Action::SetKeyPressed => {
                let (action, value) = (call.action, int(1));
                if let Some(k) = key.and_then(|k| self.ui.keys.get_mut(k)) {
                    match action {
                        Action::SetKeyColor => k.color = value,
                        Action::SetKeyType => k.kind = value,
                        Action::SetKeyName => k.name = text,
                        _ => k.pressed = value != 0,
                    }
                }
                HostValue::Nothing
            }
            Action::GetKeyColor | Action::GetKeyType | Action::GetKeyTriggerstate => {
                let k = key.and_then(|k| self.ui.keys.get(k)).copied();
                let k = k.unwrap_or(Key::UNTOUCHED);
                HostValue::Int(match call.action {
                    Action::GetKeyColor => k.color,
                    Action::GetKeyType => k.kind,
                    _ => i32::from(k.pressed),
                })
            }
            Action::GetKeyName => out(key
                .and_then(|k| self.ui.keys.get(k))
                .map_or(Text::EMPTY, |k| k.name)),
            // ponytail: a range set after init allocates on the audio thread.
            Action::SetKeyrange => {
                let note = |i: usize| u8::try_from(int(i).clamp(0, 127)).unwrap_or(0);
                let (low, high) = (note(0).min(note(1)), note(0).max(note(1)));
                self.ui.ranges.retain(|r| r.high < low || r.low > high);
                self.ui.ranges.push(KeyRange {
                    low,
                    high,
                    name: text,
                });
                HostValue::Nothing
            }
            Action::RemoveKeyrange => {
                let k = int(0);
                self.ui
                    .ranges
                    .retain(|r| !(i32::from(r.low)..=i32::from(r.high)).contains(&k));
                HostValue::Nothing
            }
            Action::SetText
            | Action::AddTextLine
            | Action::SetKnobLabel
            | Action::SetControlHelp => {
                let look = self.script()?.controls.look;
                let par = match call.action {
                    Action::SetKnobLabel => look.label,
                    Action::SetControlHelp => look.help,
                    _ => look.text,
                };
                self.set_text_par(id, par, text, call.action == Action::AddTextLine);
                HostValue::Nothing
            }
            Action::SetKnobUnit
            | Action::SetKnobDefval
            | Action::HidePart
            | Action::SetTableStepsShown
            | Action::MoveControl
            | Action::MoveControlPx => {
                let look = self.script()?.controls.look;
                let pars: &[(i32, i32)] = match call.action {
                    Action::SetKnobUnit => &[(look.unit, int(1))],
                    Action::SetKnobDefval => &[(look.default, int(1))],
                    Action::HidePart => &[(look.hide, int(1))],
                    Action::SetTableStepsShown => &[(look.steps_shown, int(1))],
                    Action::MoveControl => &[(look.grid_x, int(1)), (look.grid_y, int(2))],
                    _ => &[(look.pos_x, int(1)), (look.pos_y, int(2))],
                };
                for &(par, value) in pars {
                    self.set_control_par(call.cells, id, par, value);
                }
                HostValue::Nothing
            }
            Action::SetSkinOffset
            | Action::SetUiColor
            | Action::SetUiHeight
            | Action::SetUiHeightPx
            | Action::SetUiWidthPx
            | Action::SetScriptTitle
            | Action::MakePerfview => {
                let action = call.action;
                let page = self.page_mut()?;
                match action {
                    Action::SetSkinOffset => page.skin_offset = id,
                    Action::SetUiColor => page.color = Some(id),
                    Action::SetUiHeight => (page.height_rows, page.height_px) = (Some(id), None),
                    Action::SetUiHeightPx => (page.height_px, page.height_rows) = (Some(id), None),
                    Action::SetUiWidthPx => page.width_px = Some(id),
                    Action::SetScriptTitle => page.title = text,
                    _ => page.performance = true,
                }
                HostValue::Nothing
            }
            _ => return None,
        })
    }
}
