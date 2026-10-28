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

pub(crate) struct Ui {
    /// Per script slot, by control: its menu entries.
    menus: Box<[Box<[Vec<MenuItem>]>]>,
    /// Per script slot, by control: the text parameters it set.
    texts: Box<[Box<[Texts]>]>,
    keys: Box<[Key]>,
    ranges: Vec<KeyRange>,
}

impl Ui {
    /// Empty stores for scripts with these control counts.
    pub(crate) fn new(controls: impl Iterator<Item = usize> + Clone) -> Self {
        Self {
            menus: controls
                .clone()
                .map(|n| vec![Vec::new(); n].into())
                .collect(),
            texts: controls.map(|n| vec![Vec::new(); n].into()).collect(),
            keys: vec![Key::UNTOUCHED; KEYS].into(),
            ranges: Vec::new(),
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

    /// Answers the menu, control-text and keyboard actions; `None` for others.
    pub(crate) fn ui_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
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
                }
                HostValue::Nothing
            }
            Action::SetMenuItemStr | Action::SetMenuItemValue | Action::SetMenuItemVisibility => {
                let (action, value) = (call.action, int(2));
                if let Some(i) = at.and_then(|n| self.menu_mut(id)?.get_mut(n)) {
                    match action {
                        Action::SetMenuItemStr => i.text = text,
                        Action::SetMenuItemValue => i.value = value,
                        _ => i.visible = value != 0,
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
                if let Some(texts) = self.texts_mut(id) {
                    match texts.iter_mut().find(|(p, _)| *p == par) {
                        Some(t) => t.1 = text,
                        None => texts.push((par, text)),
                    }
                }
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
            _ => return None,
        })
    }
}
