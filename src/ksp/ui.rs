//! Script UI state: control IDs, control properties and the performance view.
//! Properties are keyed by constant values; names are recovered only when an
//! `Interface` snapshot is built for the host.

use super::builtins::{self as b, FIRST_UI_ID};
use super::compile::{Program, Ty, VarId};
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
    pub menu: Vec<MenuItem>,
}

impl ControlState {
    pub fn get(&self, par: i32) -> Option<&Prop> {
        self.props.iter().find(|(p, _)| *p == par).map(|(_, v)| v)
    }

    pub fn set_int(&mut self, par: i32, value: i32) {
        match self.props.iter_mut().find(|(p, _)| *p == par) {
            Some((_, v)) => *v = Prop::Int(value),
            None => self.props.push((par, Prop::Int(value))),
        }
    }

    pub fn set_str(&mut self, par: i32, value: &str) {
        match self.props.iter_mut().find(|(p, _)| *p == par) {
            Some((_, Prop::Str(s))) => {
                s.clear();
                s.push_str(value);
            }
            Some((_, v)) => *v = Prop::Str(value.to_owned()),
            None => self.props.push((par, Prop::Str(value.to_owned()))),
        }
    }

    pub fn str_mut(&mut self, par: i32) -> &mut String {
        let i = match self.props.iter().position(|(p, v)| *p == par && matches!(v, Prop::Str(_))) {
            Some(i) => i,
            None => {
                self.props.retain(|(p, _)| *p != par);
                self.props.push((par, Prop::Str(String::new())));
                self.props.len() - 1
            }
        };
        let Prop::Str(s) = &mut self.props[i].1 else { unreachable!() };
        s
    }
}

#[derive(Debug)]
pub struct Ui {
    /// Kontakt ID per variable, assigned in declaration order; 0 before declaration.
    var_ids: Vec<i32>,
    /// Control index per ID offset, for IDs of UI controls.
    id_controls: Vec<u32>,
    pub controls: Vec<ControlState>,
    pub performance: bool,
    pub width: i32,
    pub height: i32,
    pub title: String,
    pub wallpaper: String,
    pub listeners: BTreeMap<&'static str, i32>,
    pub diagnostics: BTreeSet<Cow<'static, str>>,
}

impl Ui {
    pub fn new(vars: usize) -> Self {
        Self {
            var_ids: vec![0; vars],
            id_controls: Vec::new(),
            controls: Vec::new(),
            performance: false,
            width: 632,
            height: 350,
            title: String::new(),
            wallpaper: String::new(),
            listeners: BTreeMap::new(),
            diagnostics: BTreeSet::new(),
        }
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

    pub fn add_control(&mut self, v: VarId, kind: &str, params: &[i32]) -> Result<(), &'static str> {
        if self.controls.len() >= MAX_CONTROLS {
            return Err("KSP control limit");
        }
        let mut c = ControlState { var: v, props: Vec::with_capacity(8), menu: Vec::new() };
        c.set_int(b::CONTROL_PAR_POS_X, 0);
        c.set_int(b::CONTROL_PAR_POS_Y, 0);
        c.set_int(b::CONTROL_PAR_WIDTH, 85);
        c.set_int(b::CONTROL_PAR_HEIGHT, if kind == "ui_knob" { 40 } else { 18 });
        c.set_int(b::CONTROL_PAR_HIDE, 0);
        if matches!(kind, "ui_knob" | "ui_slider" | "ui_value_edit") {
            let [min, max, ..] = params else { return Err("Control range missing") };
            c.set_int(b::CONTROL_PAR_MIN_VALUE, *min);
            c.set_int(b::CONTROL_PAR_MAX_VALUE, *max);
        }
        let id = (self.var_ids[v as usize] - FIRST_UI_ID) as usize;
        self.id_controls[id] = self.controls.len() as u32;
        self.controls.push(c);
        Ok(())
    }

    /// Control index for a Kontakt UI ID.
    pub fn control(&self, id: i32) -> Option<usize> {
        let i = usize::try_from(id.checked_sub(FIRST_UI_ID)?).ok()?;
        self.id_controls.get(i).filter(|&&c| c != NO_CONTROL).map(|&c| c as usize)
    }

    /// Control index for a UI variable.
    pub fn control_of(&self, v: VarId) -> Option<usize> {
        self.control(self.var_ids[v as usize])
    }

    pub fn note(&mut self, text: impl Into<Cow<'static, str>>) {
        self.diagnostics.insert(text.into());
    }

    pub fn interface(&self, prog: &Program, mem: &Memory) -> Interface {
        let controls = self
            .controls
            .iter()
            .map(|c| {
                let var = &prog.vars[c.var as usize];
                let mut properties = BTreeMap::new();
                for (par, v) in &c.props {
                    let name = prog.symbol_name(*par).map_or_else(|| format!("#{par}"), str::to_owned);
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
                    (Ty::Int, Some(n)) => Value::Array(mem.ints[slot..slot + n as usize].iter().map(|&x| Value::Int(x)).collect()),
                    (Ty::Real, Some(n)) => {
                        Value::Array(mem.reals[slot..slot + n as usize].iter().map(|&x| Value::Real(x)).collect())
                    }
                    (Ty::Str, Some(n)) => {
                        Value::Array(mem.strs[slot..slot + n as usize].iter().map(|x| Value::Text(x.clone())).collect())
                    }
                };
                properties.insert("$CONTROL_PAR_VALUE".into(), value);
                Control {
                    variable: var.name.to_string(),
                    kind: var.ui.as_deref().unwrap_or_default().to_owned(),
                    properties,
                    menu: c.menu.iter().filter(|m| m.visible).map(|m| (m.text.clone(), m.value)).collect(),
                }
            })
            .collect();
        let mut diagnostics: BTreeSet<String> = self.diagnostics.iter().map(|d| d.to_string()).collect();
        diagnostics.extend(prog.diagnostics.iter().cloned());
        diagnostics.extend(prog.errors.iter().map(|e| format!("Callback disabled: {e}")));
        Interface {
            performance: self.performance,
            width: self.width,
            height: self.height,
            title: self.title.clone(),
            wallpaper: self.wallpaper.clone(),
            controls,
            diagnostics,
            listeners: self.listeners.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
        }
    }
}
