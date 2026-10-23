//! UI controls for the Luau front end: scalar `ui_*` variables and the
//! `set_control_par*` / `get_control_par*` properties. The state rules are the
//! interpreter's (`eval.rs`: `write_var`, `set_property`, `get_property`), so a
//! script that touches only these runs the same on both paths.
//!
//! Menus, array controls and real-typed controls are refused at compile time
//! ([`guard`]): their semantics need the widget model, which lives in the
//! interpreter, so such scripts stay on the interpreter.

use super::Compiler;
use crate::builtins::{self as b, Builtin};
use crate::eval::{GRID_X, GRID_Y, declared_range, real_to_int};
use crate::hir::{Arg, Hir, Ty, VarId, WidgetKind};
use mlua::{Lua, Table, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

/// What the runtime needs to know about one UI control, by UI index.
#[derive(Clone, Debug)]
pub struct UiMeta {
    /// Declared range; a write is clamped into it (`write_var`).
    pub range: Option<(i32, i32)>,
    /// `$CONTROL_PAR_TYPE`.
    pub control_type: i32,
}

/// Refuse controls this path cannot address. Counted by name, like other sites.
pub fn guard(hir: &Hir, unsupported: &mut BTreeMap<String, usize>) {
    for ui in &hir.uis {
        let v = &hir.vars[ui.var.0 as usize];
        let what = if ui.kind == WidgetKind::Menu {
            "menu ui control"
        } else if v.len.is_some() {
            "array ui control"
        } else if v.ty != Ty::Int {
            "non-integer ui control"
        } else {
            continue;
        };
        *unsupported.entry(what.to_string()).or_default() += 1;
    }
}

/// Metadata for every UI control, in UI index order.
pub fn meta(hir: &Hir) -> Vec<UiMeta> {
    hir.uis
        .iter()
        .map(|ui| UiMeta {
            range: declared_range(ui),
            control_type: ui.kind.control_type(),
        })
        .collect()
}

/// A `set_control_par*` / `get_control_par*` value: integer or text.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Int(i32),
    Text(String),
}

impl Prop {
    /// The interpreter's `V::int`: text is 0.
    fn int(&self) -> i32 {
        match self {
            Prop::Int(n) => *n,
            Prop::Text(_) => 0,
        }
    }
}

/// Control state, as the interpreter keeps it in `Initial`.
#[derive(Default)]
pub struct State {
    pub controls: Vec<i32>,
    pub properties: HashMap<(i32, i32), i32>,
    pub text: HashMap<(i32, i32), String>,
}

impl State {
    /// Every control starts at its declared minimum when 0 is outside the range.
    pub fn new(meta: &[UiMeta]) -> Self {
        let controls = meta
            .iter()
            .map(|m| m.range.map_or(0, |(lo, hi)| 0.clamp(lo.min(hi), hi.max(lo))))
            .collect();
        Self {
            controls,
            ..Self::default()
        }
    }

    fn ui_index(meta: &[UiMeta], id: i32) -> Option<usize> {
        let index = usize::try_from(id.checked_sub(b::FIRST_UI_ID)?).ok()?;
        (index < meta.len()).then_some(index)
    }

    /// `write_var` for a control: clamp into the declared range.
    pub fn write_control(&mut self, meta: &[UiMeta], ui: usize, n: i32) {
        self.controls[ui] = match meta[ui].range {
            Some((lo, hi)) => n.clamp(lo.min(hi), hi.max(lo)),
            None => n,
        };
    }

    /// `add_text_line`: the new line goes after a non-empty text (interpreter `AddTextLine`).
    pub fn add_text_line(&mut self, id: i32, par: i32, line: String) {
        let text = match self.text.get(&(id, par)) {
            Some(old) if !old.is_empty() => format!("{old}\n{line}"),
            _ => line,
        };
        self.text.insert((id, par), text);
    }

    /// `move_control`: grid position as two integer properties.
    pub fn move_control(&mut self, meta: &[UiMeta], id: i32, x: i32, y: i32) {
        self.set(meta, id, GRID_X, Prop::Int(x));
        self.set(meta, id, GRID_Y, Prop::Int(y));
    }

    /// `move_control_px` (interpreter `MoveControlPx`). Kontakt sets or clears only the
    /// whole-control hide bit; a negative position sets it and leaves the placement alone.
    pub fn move_px(&mut self, id: i32, x: i32, y: i32) {
        let hide = self.properties.get(&(id, b::CONTROL_PAR_HIDE)).copied().unwrap_or(0);
        match crate::ui::px_move(x, y) {
            Some((x, y)) => {
                self.properties.insert((id, b::CONTROL_PAR_HIDE), hide & !b::HIDE_WHOLE_CONTROL);
                self.properties.insert((id, b::CONTROL_PAR_POS_X), x);
                self.properties.insert((id, b::CONTROL_PAR_POS_Y), y);
            }
            None => {
                self.properties.insert((id, b::CONTROL_PAR_HIDE), hide | b::HIDE_WHOLE_CONTROL);
            }
        }
    }

    /// `set_property`.
    pub fn set(&mut self, meta: &[UiMeta], id: i32, par: i32, value: Prop) {
        if par == b::CONTROL_PAR_VALUE
            && let Some(ui) = Self::ui_index(meta, id)
        {
            self.write_control(meta, ui, value.int());
            return;
        }
        match value {
            Prop::Text(s) => {
                self.text.insert((id, par), s);
            }
            Prop::Int(n) => {
                self.properties.insert((id, par), n);
            }
        }
    }

    /// `get_property`, minus the menu branches (menus are refused at compile time).
    pub fn get(&self, meta: &[UiMeta], id: i32, par: i32) -> Prop {
        let ui = Self::ui_index(meta, id);
        if let Some(ui) = ui {
            // No menu items exist, so the selected item is never found.
            if Some(par) == b::control_par("$CONTROL_PAR_SELECTED_ITEM_IDX") {
                return Prop::Int(-1);
            }
            if par == b::CONTROL_PAR_VALUE {
                return Prop::Int(self.controls[ui]);
            }
            if par == b::CONTROL_PAR_TYPE {
                return Prop::Int(meta[ui].control_type);
            }
            if par == b::CONTROL_PAR_NUM_ITEMS {
                return Prop::Int(0);
            }
        }
        if let Some(n) = self.properties.get(&(id, par)) {
            return Prop::Int(*n);
        }
        if let Some(s) = self.text.get(&(id, par)) {
            return Prop::Text(s.clone());
        }
        if let Some(ui) = ui {
            let range = meta[ui].range;
            match par {
                b::CONTROL_PAR_MIN_VALUE => return Prop::Int(range.map_or(0, |r| r.0)),
                b::CONTROL_PAR_MAX_VALUE => return Prop::Int(range.map_or(0, |r| r.1)),
                _ => {}
            }
        }
        Prop::Int(0)
    }
}

/// Lua value to a KSP property value (`V::I` / `V::R` / `V::S`).
fn prop_of(value: Value) -> mlua::Result<Prop> {
    Ok(match value {
        Value::Integer(n) => Prop::Int(n as i32),
        Value::Number(r) => Prop::Int(real_to_int(r)),
        Value::String(s) => Prop::Text(s.to_str()?.to_string()),
        other => return Err(mlua::Error::runtime(format!("control value {other:?}"))),
    })
}

/// Register the control functions on the instrument's `HOST` table.
pub fn install(lua: &Lua, host: &Table, state: &Rc<RefCell<State>>, meta: &Rc<[UiMeta]>) -> Result<(), String> {
    let err = |e: mlua::Error| e.to_string();
    let s = Rc::clone(state);
    host.set(
        "ctl_get",
        lua.create_function(move |_, ui: i64| Ok(i64::from(s.borrow().controls[ui as usize])))
            .map_err(err)?,
    )
    .map_err(err)?;
    let (s, m) = (Rc::clone(state), Rc::clone(meta));
    host.set(
        "ctl_set",
        lua.create_function(move |_, (ui, value): (i64, Value)| {
            s.borrow_mut().write_control(&m, ui as usize, prop_of(value)?.int());
            Ok(())
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    let (s, m) = (Rc::clone(state), Rc::clone(meta));
    host.set(
        "set_par",
        lua.create_function(move |_, (id, par, value): (i64, i64, Value)| {
            s.borrow_mut().set(&m, id as i32, par as i32, prop_of(value)?);
            Ok(())
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    let s = Rc::clone(state);
    host.set(
        "add_text_line",
        lua.create_function(move |_, (id, par, line): (i64, i64, String)| {
            s.borrow_mut().add_text_line(id as i32, par as i32, line);
            Ok(())
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    let (s, m) = (Rc::clone(state), Rc::clone(meta));
    host.set(
        "move_control",
        lua.create_function(move |_, (id, x, y): (i64, i64, i64)| {
            s.borrow_mut().move_control(&m, id as i32, x as i32, y as i32);
            Ok(())
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    let s = Rc::clone(state);
    host.set(
        "move_px",
        lua.create_function(move |_, (id, x, y): (i64, i64, i64)| {
            s.borrow_mut().move_px(id as i32, x as i32, y as i32);
            Ok(())
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    let (s, m) = (Rc::clone(state), Rc::clone(meta));
    host.set(
        "get_par",
        lua.create_function(move |_, (id, par): (i64, i64)| {
            // Text reads as 0 in the integer and real forms (`GetControlPar`).
            Ok(match s.borrow().get(&m, id as i32, par as i32) {
                Prop::Int(n) => i64::from(n),
                Prop::Text(_) => 0,
            })
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    let (s, m) = (Rc::clone(state), Rc::clone(meta));
    host.set(
        "get_par_str",
        lua.create_function(move |_, (id, par): (i64, i64)| {
            Ok(match s.borrow().get(&m, id as i32, par as i32) {
                Prop::Int(n) => n.to_string(),
                Prop::Text(t) => t,
            })
        })
        .map_err(err)?,
    )
    .map_err(err)?;
    Ok(())
}

fn arg_var(arg: Option<&Arg>) -> Option<VarId> {
    match arg {
        Some(Arg::Var(v, _)) => Some(*v),
        Some(Arg::Place(p)) => Some(p.var()),
        _ => None,
    }
}

impl Compiler<'_> {
    /// Control builtins. `None` leaves the call unsupported (counted by the caller).
    pub(super) fn control_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        let expr = |c: &mut Self, i: usize| match args.get(i) {
            Some(Arg::Expr(e)) => Some(c.expr(e)),
            _ => None,
        };
        Some(match builtin {
            Builtin::GetUiId => {
                // Interpreter: `FIRST_UI_ID + ui_of(var).unwrap_or(0)`, even for a non-control.
                let var = arg_var(args.first())?;
                let ui = self.hir.vars[var.0 as usize].ui.unwrap_or(0);
                format!("{}", b::FIRST_UI_ID + ui as i32)
            }
            Builtin::SetControlPar | Builtin::SetControlParReal | Builtin::SetControlParStr => {
                let id = self.int_expr(args.first())?;
                let par = self.int_expr(args.get(1))?;
                let value = expr(self, 2)?;
                format!("HOST.set_par({id}, {par}, {value})")
            }
            Builtin::GetControlPar | Builtin::GetControlParReal => {
                let id = self.int_expr(args.first())?;
                let par = self.int_expr(args.get(1))?;
                format!("HOST.get_par({id}, {par})")
            }
            Builtin::GetControlParStr => {
                let id = self.int_expr(args.first())?;
                let par = self.int_expr(args.get(1))?;
                format!("HOST.get_par_str({id}, {par})")
            }
            _ => return None,
        })
    }

    fn int_expr(&mut self, arg: Option<&Arg>) -> Option<String> {
        match arg {
            Some(Arg::Expr(e)) => Some(self.int(e)),
            _ => None,
        }
    }

    /// Scalar control read (`load` of a `Home::Control` variable).
    pub(super) fn ctl_get(ui: u32) -> String {
        format!("HOST.ctl_get({ui})")
    }
}
