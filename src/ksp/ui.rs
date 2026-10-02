//! Script UI state: control IDs, control properties and the performance view.
//! Properties are keyed by constant values; names are recovered only when an
//! `Interface` snapshot is built for the host.

use super::builtins::{self as b, FIRST_UI_ID};
use super::compile::{Program, Ty, VarId};
use super::runtime::{copy_text, refresh_range};
use super::vm::Memory;
use super::{Control, Interface, Value};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

const MAX_CONTROLS: usize = 16_384;
const NO_CONTROL: u32 = u32::MAX;

#[derive(Clone, Debug)]
pub enum Prop {
    Int(i32),
    Str(String),
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub text: String,
    pub value: i32,
    pub visible: bool,
}

#[derive(Clone, Debug)]
pub struct ControlState {
    pub var: VarId,
    pub props: Vec<(i32, Prop)>,
    /// Static metadata need not be recopied on every scalar value edit.
    pub revision: u64,
    pub menu: Vec<MenuItem>,
    pub frozen: bool,
    pub spare_menu: Vec<MenuItem>,
    pub spare_text: Vec<(i32, String)>,
    /// read_persistent_var can precede add_menu_item during initialization.
    pub native_menu_index: Option<usize>,
}

impl ControlState {
    pub fn selected_menu(&self, prog: &Program, mem: &Memory) -> Option<usize> {
        let var = &prog.vars[self.var as usize];
        if var.ui.as_deref() != Some("ui_menu") { return None; }
        self.menu.iter().position(|m| m.value == mem.ints[var.slot as usize])
    }

    /// An unset or stale menu value selects its first item.
    pub fn snap_menu(&mut self, prog: &Program, mem: &mut Memory) {
        if let Some(index) = self.native_menu_index {
            let Some(item) = self.menu.get(index) else { return };
            let var = &prog.vars[self.var as usize];
            mem.ints[var.slot as usize] = item.value;
            self.native_menu_index = None;
            return;
        }
        let Some(first) = self.menu.first().map(|i| i.value) else { return };
        let var = &prog.vars[self.var as usize];
        if var.ty == Ty::Int && var.len.is_none() && self.selected_menu(prog, mem).is_none() {
            mem.ints[var.slot as usize] = first;
        }
    }

    /// Native records select by position; host records and script assignments
    /// select by assigned value. Only native menu restoration uses this path.
    pub fn restore_menu(&mut self, prog: &Program, mem: &mut Memory, value: &Value, building: bool) {
        self.native_menu_index = None;
        if prog.vars[self.var as usize].ui.as_deref() == Some("ui_menu")
            && let Value::NativeInt { native_int } = value
        {
            self.native_menu_index = usize::try_from(*native_int).ok();
            if !building && self.native_menu_index.is_none_or(|i| i >= self.menu.len()) {
                self.native_menu_index = None;
                if let Some(item) = self.menu.first() {
                    mem.ints[prog.vars[self.var as usize].slot as usize] = item.value;
                }
            }
        }
        self.snap_menu(prog, mem);
    }

    pub fn visible_menu<'a>(&'a self, prog: &Program, mem: &Memory) -> impl Iterator<Item = &'a MenuItem> {
        let selected = self.selected_menu(prog, mem);
        // Kontakt keeps a hidden selected item until another item is selected.
        self.menu.iter().enumerate().filter(move |(i, m)| m.visible || Some(*i) == selected).map(|(_, m)| m)
    }

    pub fn get(&self, par: i32) -> Option<&Prop> {
        self.props.iter().find(|(p, _)| *p == par).map(|(_, v)| v)
    }

    pub fn set_int(&mut self, par: i32, value: i32) -> Result<(), &'static str> {
        if matches!(self.get(par), Some(Prop::Int(n)) if *n == value) { return Ok(()) }
        self.revision = self.revision.wrapping_add(1);
        let full = self.frozen && self.props.len() == self.props.capacity();
        match self.props.iter_mut().find(|(p, _)| *p == par) {
            Some((_, v @ Prop::Str(_))) if self.frozen => {
                let Prop::Str(text) = std::mem::replace(v, Prop::Int(value)) else { unreachable!() };
                self.spare_text.push((par, text));
            }
            Some((_, v)) => *v = Prop::Int(value),
            None if full => return Err("KSP property capacity exhausted"),
            None => self.props.push((par, Prop::Int(value))),
        }
        Ok(())
    }

    pub fn set_str(&mut self, par: i32, value: &str) -> Result<(), &'static str> {
        if matches!(self.get(par), Some(Prop::Str(s)) if s == value) { return Ok(()) }
        let loading = !self.frozen;
        let dst = self.str_mut(par)?;
        super::vm::put_text(dst, value, loading).map_err(|e| e.0)
    }

    pub fn str_mut(&mut self, par: i32) -> Result<&mut String, &'static str> {
        self.revision = self.revision.wrapping_add(1);
        let i = match self.props.iter().position(|(p, _)| *p == par) {
            Some(i) if matches!(self.props[i].1, Prop::Str(_)) => i,
            Some(i) if self.frozen => {
                let at = self
                    .spare_text
                    .iter()
                    .position(|(p, _)| *p == par)
                    .ok_or("KSP string property was not prepared")?;
                self.props[i].1 = Prop::Str(self.spare_text.swap_remove(at).1);
                i
            }
            None if self.frozen => return Err("KSP string property was not prepared"),
            _ => {
                self.props.retain(|(p, _)| *p != par);
                self.props.push((par, Prop::Str(String::new())));
                self.props.len() - 1
            }
        };
        let Prop::Str(s) = &mut self.props[i].1 else {
            unreachable!()
        };
        Ok(s)
    }

    pub fn prepare(&mut self, bytes: usize, menu: bool) {
        self.frozen = false;
        self.props.reserve(64);
        self.spare_text.retain(|(par, _)| {
            !self
                .props
                .iter()
                .any(|(p, v)| p == par && matches!(v, Prop::Str(_)))
        });
        self.spare_text.reserve(self.props.len() + 7);
        for par in [
            b::CONTROL_PAR_TEXT,
            b::CONTROL_PAR_LABEL,
            b::CONTROL_PAR_HELP,
            b::CONTROL_PAR_PICTURE,
            b::CONTROL_PAR_SHORT_NAME,
            b::CONTROL_PAR_AUTOMATION_NAME,
        ] {
            if self.get(par).is_none() {
                self.set_str(par, "").unwrap();
            }
        }
        for (_, p) in &mut self.props {
            if let Prop::Str(s) = p {
                s.reserve(bytes.saturating_sub(s.len()));
            }
        }
        if !matches!(self.get(b::CONTROL_PAR_UNIT), Some(Prop::Str(_)))
            && !self
                .spare_text
                .iter()
                .any(|(p, _)| *p == b::CONTROL_PAR_UNIT)
        {
            if self.get(b::CONTROL_PAR_UNIT).is_none() {
                self.set_int(b::CONTROL_PAR_UNIT, 0).unwrap();
            }
            self.spare_text
                .push((b::CONTROL_PAR_UNIT, String::with_capacity(bytes)));
        }
        for item in &mut self.menu {
            item.text.reserve(bytes.saturating_sub(item.text.len()));
        }
        if menu {
            // ponytail: 16 additional runtime menu rows; raise this bounded
            // headroom if a real instrument requires more dynamic rows.
            self.menu.reserve(16);
            self.spare_menu.reserve(16);
            while self.spare_menu.len() < 16 {
                self.spare_menu.push(MenuItem {
                    text: String::with_capacity(bytes),
                    value: 0,
                    visible: true,
                });
            }
        }
        self.frozen = true;
    }
}

#[derive(Debug)]
pub struct Ui {
    /// Kontakt ID per variable, assigned in declaration order; 0 before declaration.
    var_ids: Vec<i32>,
    /// Control index per ID offset, for IDs of UI controls.
    id_controls: Vec<u32>,
    pub controls: Vec<ControlState>,
    pub fonts: Vec<String>,
    pub performance: bool,
    pub width: i32,
    pub height: i32,
    pub title: String,
    pub wallpaper: String,
    pub wallpaper_state: i32,
    pub background_color: Option<u32>,
    pub skin_offset: i32,
    pub listeners: BTreeMap<&'static str, i32>,
    pub diagnostics: BTreeSet<Cow<'static, str>>,}

impl Ui {
    pub fn new(vars: usize) -> Self {
        Self {
            var_ids: vec![0; vars],
            id_controls: Vec::with_capacity(vars),
            controls: Vec::new(),
            fonts: Vec::new(),
            performance: false,
            width: 632,
            height: 350,
            title: String::new(),
            wallpaper: String::new(),
            wallpaper_state: 0,
            background_color: None,
            skin_offset: 0,
            listeners: BTreeMap::new(),
            diagnostics: BTreeSet::new(),        }
    }

    pub fn var_id(&self, v: VarId) -> i32 {
        self.var_ids[v as usize]
    }

    /// Assign the next ID. Returns false when the variable was already declared.
    pub fn declare(&mut self, v: VarId) -> bool {
        if self.var_ids[v as usize] != 0 {
            return false;
        }
        self.var_ids[v as usize] = FIRST_UI_ID + self.id_controls.len() as i32;
        self.id_controls.push(NO_CONTROL);
        true
    }

    pub fn add_control(
        &mut self,
        v: VarId,
        kind: &str,
        params: &[i32],
    ) -> Result<(), &'static str> {
        if self.controls.len() >= MAX_CONTROLS {
            return Err("KSP control limit");
        }
        let mut c = ControlState {
            var: v,
            props: Vec::with_capacity(8),
            revision: 0,
            menu: Vec::new(),
            spare_menu: Vec::new(),
            spare_text: Vec::new(),
            frozen: false,
            native_menu_index: None,
        };
        c.set_int(b::CONTROL_PAR_POS_X, 0)?;
        c.set_int(b::CONTROL_PAR_POS_Y, 0)?;
        c.set_int(b::CONTROL_PAR_WIDTH, 85)?;
        c.set_int(
            b::CONTROL_PAR_HEIGHT,
            if kind == "ui_knob" { 40 } else { 18 },
        )?;
        c.set_int(b::CONTROL_PAR_HIDE, 0)?;
        if matches!(kind, "ui_knob" | "ui_slider" | "ui_value_edit") {
            let [min, max, ..] = params else {
                return Err("Control range missing");
            };
            c.set_int(b::CONTROL_PAR_MIN_VALUE, *min)?;
            c.set_int(b::CONTROL_PAR_MAX_VALUE, *max)?;
        }
        if kind == "ui_file_selector" {
            c.set_str(b::CONTROL_PAR_FILEPATH, "")?;
            c.set_str(b::CONTROL_PAR_BASEPATH, "")?;
        }
        if kind == "ui_table" {
            let [_, _, range, ..] = params else { return Err("Table range missing"); };
            c.set_int(b::CONTROL_PAR_MIN_VALUE, if *range < 0 { *range } else { 0 })?;
            c.set_int(b::CONTROL_PAR_MAX_VALUE, range.saturating_abs().max(1))?;
        }
        let id = (self.var_ids[v as usize] - FIRST_UI_ID) as usize;
        self.id_controls[id] = self.controls.len() as u32;
        self.controls.push(c);
        Ok(())
    }

    /// Control index for a Kontakt UI ID.
    pub fn control(&self, id: i32) -> Option<usize> {
        let i = usize::try_from(id.checked_sub(FIRST_UI_ID)?).ok()?;
        self.id_controls
            .get(i)
            .filter(|&&c| c != NO_CONTROL)
            .map(|&c| c as usize)
    }

    pub fn has_id(&self, id: i32) -> bool {
        id.checked_sub(FIRST_UI_ID).and_then(|i| usize::try_from(i).ok())
            .is_some_and(|i| i < self.id_controls.len())
    }

    /// Control index for a UI variable.
    pub fn control_of(&self, v: VarId) -> Option<usize> {
        self.control(self.var_ids[v as usize])
    }

    /// Copy properties, values and menu items into `out`, built by
    /// [`interface`](Self::interface), without allocating.
    /// Refresh `out`'s controls from `next` on, about `budget` properties'
    /// worth, advancing `next`; returns whether anything changed.
    pub fn refresh(
        &self,
        prog: &Program,
        mem: &Memory,
        out: &mut Interface,
        menu_spares: &mut [Vec<(String, i32)>],
        revisions: &mut [u64],
        value_revisions: &mut [u64],
        value_revision: &mut u64,
        next: &mut usize,
        value_at: &mut usize,
        budget: usize,
    ) -> bool {
        let (mut changed, mut left) = (
            std::mem::replace(&mut out.wallpaper_state, self.wallpaper_state)
                != self.wallpaper_state,
            budget,
        );
        changed |= std::mem::replace(&mut out.skin_offset, self.skin_offset) != self.skin_offset;
        changed |= std::mem::replace(&mut out.background_color, self.background_color) != self.background_color;
        for (index, (c, o)) in self.controls.iter().zip(&mut out.controls).enumerate().skip(*next) {
            if left == 0 {
                break;
            }
            let var = &prog.vars[c.var as usize];
            let revision = match var.ty {
                Ty::Int => mem.ints.revision(var.slot as usize),
                Ty::Real => mem.reals.revision(var.slot as usize),
                Ty::Str => mem.strs.revision(var.slot as usize),
            };
            if *value_at == 0 {
                *value_revision = revision;
                left = left.saturating_sub(1);
                let metadata_changed = revisions[index] != c.revision;
                if metadata_changed {
                    left = left.saturating_sub(c.props.len());
                    revisions[index] = c.revision;
                    for (par, v) in &c.props {
                        let Some(name) = prog.symbol_name(*par) else {
                            continue;
                        };
                        changed |= match (v, o.properties.get_mut(name)) {
                            (Prop::Int(n), Some(Value::Int(d))) => std::mem::replace(d, *n) != *n,
                            // Runtime::live reserved this inactive scalar slot
                            // off-thread. Replacing its storage-free marker
                            // preserves authored absence until the first setter.
                            (Prop::Int(n), Some(d)) if matches!(&*d, Value::IntArray(v) if v.is_empty() && v.capacity() == 0) => {
                                *d = Value::Int(*n);
                                true
                            }
                            (Prop::Str(s), Some(Value::Text(d))) => copy_text(d, s),
                            _ => false,
                        }
                    }
                }
                // Menu rows depend on their metadata and their own selected
                // value. Unrelated edits need not scan or recopy every menu.
                if !c.menu.is_empty() && (metadata_changed || value_revisions[index] != revision) {
                    left = left.saturating_sub(c.menu.len());
                    let count = c.visible_menu(prog, mem).count();
                    let spare = &mut menu_spares[index];
                    while o.menu.len() > count {
                        let mut item = o.menu.pop().unwrap();
                        item.0.clear();
                        item.1 = 0;
                        spare.push(item);
                        changed = true;
                    }
                    while o.menu.len() < count && !spare.is_empty() {
                        o.menu.push(spare.pop().unwrap());
                        changed = true;
                    }
                    for (m, (text, value)) in c.visible_menu(prog, mem).zip(&mut o.menu) {
                        changed |= copy_text(text, &m.text);
                        changed |= std::mem::replace(value, m.value) != m.value;
                    }
                }
            }
            if *value_at == 0 && value_revisions[index] == revision {
                *next += 1;
                continue;
            }
            let len = var.len.map_or(1, |n| n as usize);
            // Metadata can exceed this coarse budget; still make progress.
            let end = len.min(value_at.saturating_add(left.max(1)));
            if let Some(v) = o.properties.get_mut("$CONTROL_PAR_VALUE") {
                changed |= refresh_range(mem, var, v, *value_at..end);
            }
            left = left.saturating_sub(end - *value_at);
            if end < len {
                *value_at = end;
                break;
            }
            // A write between chunks leaves its newer revision pending for
            // the next refresh instead of claiming earlier cells are current.
            value_revisions[index] = *value_revision;
            *value_at = 0;
            *next += 1;
        }
        if *next >= self.controls.len().min(out.controls.len()) {
            *next = usize::MAX;
        }
        changed
    }

    pub fn interface(&self, prog: &Program, mem: &Memory) -> Interface {
        let controls = self
            .controls
            .iter()
            .map(|c| {
                let var = &prog.vars[c.var as usize];
                let mut properties = BTreeMap::new();
                for (par, v) in &c.props {
                    let name = prog
                        .symbol_name(*par)
                        .map_or_else(|| format!("#{par}"), str::to_owned);
                    let value = match v {
                        Prop::Int(n) => Value::Int(*n),
                        Prop::Str(s) => Value::Text(s.clone()),
                    };
                    properties.insert(name, value);
                }
                let slot = var.slot as usize;
                let value = match (var.ty, var.len) {
                    (Ty::Int, None) => Value::Int(mem.ints[slot]),
                    (Ty::Real, None) => Value::Real(mem.reals[slot]),
                    (Ty::Str, None) => Value::Text(mem.strs[slot].clone()),
                    (Ty::Int, Some(n)) => Value::IntArray(mem.ints[slot..slot + n as usize].to_vec()),
                    (Ty::Real, Some(n)) => Value::RealArray(mem.reals[slot..slot + n as usize].to_vec()),
                    (Ty::Str, Some(n)) => Value::Array(
                        mem.strs[slot..slot + n as usize]
                            .iter()
                            .map(|x| Value::Text(x.clone()))
                            .collect(),
                    ),
                };
                properties.insert("$CONTROL_PAR_VALUE".into(), value);
                if let Some(Some(menu)) = prog.picture_menus.get(&c.var) {
                    let id = self.var_ids[*menu as usize];
                    if id > 0 { properties.insert("picture menu".into(), Value::Int(id)); }
                }
                Control {
                    id: self.var_ids[c.var as usize],
                    variable: var.name.to_string(),
                    kind: var.ui.as_deref().unwrap_or_default().to_owned(),
                    properties,
                    menu: c
                        .visible_menu(prog, mem)
                        .map(|m| (m.text.clone(), m.value))
                        .collect(),
                }
            })
            .collect();
        let mut diagnostics: BTreeSet<String> =
            self.diagnostics.iter().map(|d| d.to_string()).collect();
        diagnostics.extend(prog.diagnostics.iter().cloned());
        diagnostics.extend(
            prog.errors
                .iter()
                .map(|e| format!("Callback disabled: {e}")),
        );
        Interface {
            performance: self.performance,
            width: self.width,
            height: self.height,
            title: self.title.clone(),
            wallpaper: self.wallpaper.clone(),
            wallpaper_state: self.wallpaper_state,
            background_color: self.background_color,
            skin_offset: self.skin_offset,
            controls,
            fonts: self.fonts.clone(),
            diagnostics,
            listeners: self
                .listeners
                .iter()
                .filter(|(_, value)| **value != 0)
                .map(|(k, v)| ((*k).to_owned(), *v))
                .collect(),
        }
    }
}
