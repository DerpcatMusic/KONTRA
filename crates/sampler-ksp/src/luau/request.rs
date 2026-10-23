//! Host requests for the Luau front end: builtins the interpreter records in
//! `Model::requests` for the instrument's host to act on (`save_array`,
//! `load_ir_sample`, `set_ui_color`, ...). The Luau path keeps the same
//! (command, arguments) list, so [`compare`](super::compare) checks it.

use super::{Compiler, lua_str};
use crate::builtins::Builtin;
use crate::hir::{Arg, Ty};
use crate::model::Value;
use mlua::{Lua, Table};
use std::{cell::RefCell, rc::Rc};

/// Recorded requests, in call order: command name and arguments.
pub type Log = Rc<RefCell<Vec<(String, Vec<Value>)>>>;

/// Register `HOST.request(command, { {kind, value}, ... })` on the instrument's `HOST` table.
/// Kinds: `"s"` text, `"r"` real, `"i"` integer.
pub fn install(lua: &Lua, host: &Table, log: &Log) -> Result<(), String> {
    let err = |e: mlua::Error| e.to_string();
    let log = Rc::clone(log);
    host.set(
        "request",
        lua.create_function(move |_, (command, args): (String, Table)| {
            let mut values = Vec::new();
            for pair in args.sequence_values::<Table>() {
                let pair = pair?;
                let value = match pair.get::<String>(1)?.as_str() {
                    "s" => Value::Text(pair.get(2)?),
                    "r" => Value::Real(pair.get(2)?),
                    _ => Value::Int(pair.get::<f64>(2)? as i32),
                };
                values.push(value);
            }
            log.borrow_mut().push((command, values));
            Ok(0)
        })
        .map_err(err)?,
    )
    .map_err(err)
}

impl Compiler<'_> {
    /// Requests the interpreter records. `None` leaves the call unsupported.
    pub(super) fn request_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        use Builtin::*;
        if !matches!(
            builtin,
            SetVoiceLimit | LoadIrSample | SaveArray | AttachLevelMeter | FsNavigate | LoadNativeUi
                | SetNksNavName | SetNksNavPar | ResetNksNav | SetUiColor
        ) {
            return None;
        }
        let mut pairs = Vec::with_capacity(args.len());
        for arg in args {
            pairs.push(match arg {
                // A variable is recorded by its name (`%a`), as the interpreter does.
                Arg::Var(v, _) => format!("{{\"s\", {}}}", lua_str(&self.hir.vars[v.0 as usize].name)),
                Arg::Key(k) => format!("{{\"s\", {}}}", lua_str(k)),
                Arg::Expr(e) => match e.ty {
                    Ty::Str => format!("{{\"s\", {}}}", self.expr(e)),
                    Ty::Real => format!("{{\"r\", {}}}", self.expr(e)),
                    _ => format!("{{\"i\", {}}}", self.int(e)),
                },
                _ => return None,
            });
        }
        Some(format!("HOST.request({}, {{{}}})", lua_str(builtin.name()), pairs.join(", ")))
    }
}
