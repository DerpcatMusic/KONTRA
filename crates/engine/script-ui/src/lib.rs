//! What scripts write to their controls' look and read back: menu entries,
//! control texts, the page and the keyboard's keys. The engine answers a
//! script's UI calls from here and the host's UI reads it back; nothing
//! here touches audio.
//!
//! Measured (probes/menu_readback.lua, key_constants.lua): a menu's entries
//! are indexed in the order the script added them; `$CONTROL_PAR_NUM_ITEMS`
//! counts them all, hidden or not; `$CONTROL_PAR_SELECTED_ITEM_IDX` is the
//! index of the entry holding the menu's value; an untouched key reads colour
//! and type 0.

use kontra_vm::{Action, HostCall, HostValue, Text};

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

/// The parameter ids `set_text`, `move_control_px`, `hide_part` and the like
/// store under, so the host can read a control's look back (07-APP.md §5.1).
#[derive(Clone, Copy, Debug, Default)]
pub struct LookPars {
    pub text: i32,
    pub label: i32,
    pub help: i32,
    pub unit: i32,
    pub default: i32,
    pub pos_x: i32,
    pub pos_y: i32,
    pub hide: i32,
    /// `move_control`'s grid cell.
    pub grid_x: i32,
    pub grid_y: i32,
    pub steps_shown: i32,
    pub picture: i32,
    pub picture_state: i32,
    /// The UI ids that address the instrument's wallpaper and icon.
    pub wallpaper: i32,
    pub icon: i32,
}

/// Every script slot's look state. Controls are addressed by their index in
/// their script (the engine resolves a UI id to it).
#[derive(Debug)]
pub struct Store {
    /// Per script slot, by control: its menu entries.
    menus: Box<[Box<[Vec<MenuItem>]>]>,
    /// Per script slot, by control: the text parameters it set.
    texts: Box<[Box<[Texts]>]>,
    keys: Box<[Key]>,
    ranges: Vec<KeyRange>,
    /// Per script slot.
    pages: Box<[Page]>,
    /// Per script slot, by control: moves on every write to its look.
    generations: Box<[Box<[u32]>]>,
}

impl Store {
    /// Empty stores for scripts with these control counts.
    pub fn new(controls: impl Iterator<Item = usize> + Clone) -> Self {
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

    /// The keyboard's keys as the scripts left them, by note number.
    #[must_use]
    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    /// The named key ranges, in the order they were set.
    #[must_use]
    pub fn key_ranges(&self) -> &[KeyRange] {
        &self.ranges
    }

    /// The entries of control `n` of script `slot`.
    #[must_use]
    pub fn menu(&self, slot: u8, n: Option<usize>) -> &[MenuItem] {
        n.and_then(|n| self.menus.get(usize::from(slot))?.get(n))
            .map_or(&[], Vec::as_slice)
    }

    /// The text parameters control `n` of script `slot` set.
    #[must_use]
    pub fn texts(&self, slot: u8, n: usize) -> &[(i32, Text)] {
        self.texts
            .get(usize::from(slot))
            .and_then(|t| t.get(n))
            .map_or(&[], Vec::as_slice)
    }

    /// How often control `n` of script `slot` changed look.
    #[must_use]
    pub fn generation(&self, slot: u8, n: usize) -> u32 {
        self.generations
            .get(usize::from(slot))
            .and_then(|g| g.get(n))
            .copied()
            .unwrap_or(0)
    }

    /// Script `slot`'s page.
    #[must_use]
    pub fn page(&self, slot: u8) -> Option<&Page> {
        self.pages.get(usize::from(slot))
    }

    /// Marks control `n` of script `slot` as changed in look. Only a write
    /// that changes what is stored counts, so a script that sets the same
    /// look every tick leaves the UI idle.
    pub fn touch(&mut self, slot: u8, n: usize) {
        if let Some(g) = self
            .generations
            .get_mut(usize::from(slot))
            .and_then(|g| g.get_mut(n))
        {
            *g = g.wrapping_add(1);
        }
    }

    /// Sets text parameter `par` of control `n`; `line` appends a line.
    fn set_text_par(&mut self, slot: u8, n: Option<usize>, par: i32, text: Text, line: bool) {
        let Some(n) = n else {
            return;
        };
        let Some(texts) = self
            .texts
            .get_mut(usize::from(slot))
            .and_then(|t| t.get_mut(n))
        else {
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
            self.touch(slot, n);
        }
    }

    fn menu_mut(&mut self, slot: u8, n: Option<usize>) -> Option<&mut Vec<MenuItem>> {
        self.menus.get_mut(usize::from(slot))?.get_mut(n?)
    }

    /// `set_control_par` on the instrument's wallpaper or icon.
    fn page_par(&mut self, slot: u8, look: &LookPars, call: &HostCall<'_>) -> Option<HostValue> {
        if !matches!(
            call.action,
            Action::SetControlPar | Action::SetControlParStr
        ) {
            return None;
        }
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let (id, par, value) = (int(0), int(1), int(2));
        // Both 0 when the front end set no look ids (Lua).
        if look.wallpaper == look.icon || (id != look.wallpaper && id != look.icon) {
            return None;
        }
        let text = call.texts.first().copied().unwrap_or(Text::EMPTY);
        let wallpaper = id == look.wallpaper;
        let page = self.pages.get_mut(usize::from(slot))?;
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

    /// Answers the menu, control-text, page and keyboard actions of script
    /// `slot`, whose first argument names control `n` (when it is one);
    /// `None` for others. `look` is `None` when the script has no look ids.
    pub fn call(
        &mut self,
        slot: u8,
        n: Option<usize>,
        look: Option<&LookPars>,
        call: &mut HostCall<'_>,
    ) -> Option<HostValue> {
        if let Some(value) = look.and_then(|look| self.page_par(slot, look, call)) {
            return Some(value);
        }
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let text = call.texts.first().copied().unwrap_or(Text::EMPTY);
        let at = usize::try_from(int(1)).ok();
        let key = usize::try_from(int(0)).ok().filter(|&k| k < KEYS);
        let mut out = |t: Text| {
            *call.text_out = t;
            HostValue::Text
        };
        let item = |s: &Self| at.and_then(|i| s.menu(slot, n).get(i).copied());
        Some(match call.action {
            // ponytail: a menu that grows after init allocates on the audio
            // thread; the manual allows entries only in `on init`.
            Action::AddMenuItem => {
                let value = int(1);
                if let Some(m) = self.menu_mut(slot, n) {
                    m.push(MenuItem {
                        text,
                        value,
                        visible: true,
                    });
                    if let Some(n) = n {
                        self.touch(slot, n);
                    }
                }
                HostValue::Nothing
            }
            Action::SetMenuItemStr | Action::SetMenuItemValue | Action::SetMenuItemVisibility => {
                let (action, value) = (call.action, int(2));
                if let Some(i) = at.and_then(|i| self.menu_mut(slot, n)?.get_mut(i)) {
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
                    if let Some(n) = n.filter(|_| changed) {
                        self.touch(slot, n);
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
                self.set_text_par(slot, n, int(1), text, false);
                HostValue::Nothing
            }
            Action::GetControlParStr => {
                let par = int(1);
                let found = n
                    .and_then(|n| self.texts(slot, n).iter().find(|(p, _)| *p == par))
                    .map_or(Text::EMPTY, |&(_, t)| t);
                out(found)
            }
            Action::SetKeyColor
            | Action::SetKeyType
            | Action::SetKeyName
            | Action::SetKeyPressed => {
                let (action, value) = (call.action, int(1));
                if let Some(k) = key.and_then(|k| self.keys.get_mut(k)) {
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
                let k = key.and_then(|k| self.keys.get(k)).copied();
                let k = k.unwrap_or(Key::UNTOUCHED);
                HostValue::Int(match call.action {
                    Action::GetKeyColor => k.color,
                    Action::GetKeyType => k.kind,
                    _ => i32::from(k.pressed),
                })
            }
            Action::GetKeyName => out(key
                .and_then(|k| self.keys.get(k))
                .map_or(Text::EMPTY, |k| k.name)),
            // ponytail: a range set after init allocates on the audio thread.
            Action::SetKeyrange => {
                let note = |i: usize| u8::try_from(int(i).clamp(0, 127)).unwrap_or(0);
                let (low, high) = (note(0).min(note(1)), note(0).max(note(1)));
                self.ranges.retain(|r| r.high < low || r.low > high);
                self.ranges.push(KeyRange {
                    low,
                    high,
                    name: text,
                });
                HostValue::Nothing
            }
            Action::RemoveKeyrange => {
                let k = int(0);
                self.ranges
                    .retain(|r| !(i32::from(r.low)..=i32::from(r.high)).contains(&k));
                HostValue::Nothing
            }
            Action::SetText
            | Action::AddTextLine
            | Action::SetKnobLabel
            | Action::SetControlHelp => {
                let look = look?;
                let par = match call.action {
                    Action::SetKnobLabel => look.label,
                    Action::SetControlHelp => look.help,
                    _ => look.text,
                };
                self.set_text_par(slot, n, par, text, call.action == Action::AddTextLine);
                HostValue::Nothing
            }
            Action::SetSkinOffset
            | Action::SetUiColor
            | Action::SetUiHeight
            | Action::SetUiHeightPx
            | Action::SetUiWidthPx
            | Action::SetScriptTitle
            | Action::MakePerfview => {
                let (action, id) = (call.action, int(0));
                let page = self.pages.get_mut(usize::from(slot))?;
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
