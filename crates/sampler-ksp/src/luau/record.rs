//! UI-record builtins for the Luau front end: the `set_text` / `set_knob_*` /
//! `hide_part` / `move_control*` family that writes control properties, and the
//! interface fields (`set_ui_height_px`, `make_perfview`, `message`, ...). The rules are
//! the interpreter's (`eval.rs`), so the properties and text mirror that
//! [`compare`](super::compare) already checks carries them. Warnings are not modelled.

use super::Compiler;
use crate::builtins::{self as b, Builtin};
use crate::eval::TABLE_STEPS_SHOWN;
use crate::hir::Arg;

/// Lua side: the interface fields this path models, read back by the host. Always emitted.
pub(super) const PRELUDE: &str = "\
local UI = {}
MODEL.ui = UI
MODEL.messages = {}
local function SET_UI(name, v)
  UI[name] = v
  return 0
end
local function PERFVIEW()
  UI.performance_view = true
  return 0
end
local function MESSAGE(text)
  if #MODEL.messages < 1000 then MODEL.messages[#MODEL.messages + 1] = text end
  return 0
end
";

impl Compiler<'_> {
    /// The control a builtin's first argument names, as its UI index. `Err` for a non-variable
    /// (refused); `Ok(None)` for a variable that is not a control (the interpreter does nothing).
    fn control_of(&self, arg: Option<&Arg>) -> Result<Option<usize>, ()> {
        match arg {
            Some(Arg::Var(v, _)) => Ok(self.hir.vars[v.0 as usize].ui.map(|u| u as usize)),
            Some(Arg::Place(p)) => Ok(self.hir.vars[p.var().0 as usize].ui.map(|u| u as usize)),
            _ => Err(()),
        }
    }

    /// Text arguments as Lua, as in `pgs.rs`.
    pub(super) fn text_of(&mut self, arg: Option<&Arg>) -> Option<String> {
        match arg {
            Some(Arg::Key(k)) => Some(super::lua_str(k)),
            Some(Arg::Expr(e)) => match e.ty {
                crate::hir::Ty::Str => Some(self.expr(e)),
                crate::hir::Ty::Real => None,
                _ => Some(format!("tostring({})", self.int(e))),
            },
            _ => None,
        }
    }

    fn int_of(&mut self, arg: Option<&Arg>) -> Option<String> {
        match arg {
            Some(Arg::Expr(e)) => Some(self.int(e)),
            _ => None,
        }
    }

    /// Interface and control-property builtins. `None` leaves the call unsupported.
    pub(super) fn record_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        use Builtin::*;
        Some(match builtin {
            // Interpreter: no effect in `on init` and no state.
            ExposeControls => "0".into(),
            MakePerfview => "PERFVIEW()".into(),
            Message => format!("MESSAGE({})", self.text_of(args.first())?),
            SetUiHeight => format!("SET_UI(\"height_grid\", {})", self.int_of(args.first())?),
            SetUiHeightPx => format!("SET_UI(\"height_px\", {})", self.int_of(args.first())?),
            SetUiWidthPx => format!("SET_UI(\"width_px\", {})", self.int_of(args.first())?),
            SetSkinOffset => format!("SET_UI(\"skin_offset\", {})", self.int_of(args.first())?),
            SetText | SetKnobLabel | SetControlHelp | AddTextLine => {
                let Ok(ui) = self.control_of(args.first()) else { return None };
                let text = self.text_of(args.get(1))?;
                let Some(ui) = ui else { return Some("0".into()) };
                let id = b::FIRST_UI_ID + ui as i32;
                let par = match builtin {
                    SetText | AddTextLine => b::CONTROL_PAR_TEXT,
                    SetKnobLabel => b::CONTROL_PAR_LABEL,
                    _ => b::CONTROL_PAR_HELP,
                };
                if builtin == AddTextLine {
                    format!("HOST.add_text_line({id}, {par}, {text})")
                } else {
                    format!("HOST.set_par({id}, {par}, {text})")
                }
            }
            SetKnobUnit | SetKnobDefval | HidePart | SetTableStepsShown => {
                let Ok(ui) = self.control_of(args.first()) else { return None };
                let value = self.int_of(args.get(1))?;
                let Some(ui) = ui else { return Some("0".into()) };
                let par = match builtin {
                    SetKnobUnit => b::CONTROL_PAR_UNIT,
                    SetKnobDefval => b::CONTROL_PAR_DEFAULT_VALUE,
                    HidePart => b::CONTROL_PAR_HIDE,
                    _ => TABLE_STEPS_SHOWN,
                };
                format!("HOST.set_par({}, {par}, {value})", b::FIRST_UI_ID + ui as i32)
            }
            MoveControl | MoveControlPx => {
                let Ok(ui) = self.control_of(args.first()) else { return None };
                let (x, y) = (self.int_of(args.get(1))?, self.int_of(args.get(2))?);
                let Some(ui) = ui else { return Some("0".into()) };
                let id = b::FIRST_UI_ID + ui as i32;
                if builtin == MoveControl {
                    format!("HOST.move_control({id}, {x}, {y})")
                } else {
                    format!("HOST.move_px({id}, {x}, {y})")
                }
            }
            _ => return None,
        })
    }
}
