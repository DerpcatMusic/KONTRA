//! PGS (script-to-script integer and text keys) and `set_script_title` for the Luau
//! front end. The state rules are the interpreter's (`eval.rs`: `PgsCreateKey` ..
//! `SetScriptTitle`): keys live in `Initial::model`, and [`compare`](super::compare)
//! checks them, so these are instance state, not per-callback temporaries.

use super::Compiler;
use crate::builtins::Builtin;
use crate::hir::{Arg, Ty};

/// Lua side of the PGS tables. Always emitted: the chunk returns the tables so the host
/// can read them, and empty tables cost nothing. Keys are text; integer values are
/// stored 1-based with the length in `n` (the interpreter's `Vec<i32>`).
pub(super) const PRELUDE: &str = "\
local PGS, PGS_TEXT, MODEL = {}, {}, {}
local function PGS_CREATE(k, size)
  if PGS[k] == nil then
    local t = { n = math.min(math.max(size, 0), 256) }
    for i = 1, t.n do t[i] = 0 end
    PGS[k] = t
  end
  return 0
end
local function PGS_EXISTS(k) return PGS[k] ~= nil and 1 or 0 end
local function PGS_GET(k, i)
  local t = PGS[k]
  if t == nil or i < 0 or i >= t.n then return 0 end
  return t[i + 1]
end
local function PGS_SET(k, i, v)
  local t = PGS[k]
  if t ~= nil and i >= 0 and i < t.n then t[i + 1] = v end
  return 0
end
local function PGS_STR_CREATE(k)
  if PGS_TEXT[k] == nil then PGS_TEXT[k] = \"\" end
  return 0
end
local function PGS_STR_EXISTS(k) return PGS_TEXT[k] ~= nil and 1 or 0 end
local function PGS_STR_GET(k) return PGS_TEXT[k] or \"\" end
local function PGS_STR_SET(k, v)
  if PGS_TEXT[k] ~= nil then PGS_TEXT[k] = v end
  return 0
end
local function SET_TITLE(t)
  MODEL.title = t
  return 0
end
";

impl Compiler<'_> {
    /// A text argument as a Lua expression. Reals are refused (their text form is the
    /// interpreter's `real_text`, not modelled here); integers print as decimal, as `V::text`.
    fn text_arg(&mut self, arg: Option<&Arg>) -> Option<String> {
        match arg {
            Some(Arg::Key(k)) => Some(super::lua_str(k)),
            Some(Arg::Expr(e)) => match e.ty {
                Ty::Str => Some(self.expr(e)),
                Ty::Real => None,
                _ => Some(format!("tostring({})", self.int(e))),
            },
            _ => None,
        }
    }

    fn int_arg(&mut self, arg: Option<&Arg>) -> Option<String> {
        match arg {
            Some(Arg::Expr(e)) => Some(self.int(e)),
            _ => None,
        }
    }

    /// PGS and title builtins. `None` leaves the call unsupported (counted by the caller).
    pub(super) fn pgs_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        if builtin == Builtin::SetScriptTitle {
            return Some(format!("SET_TITLE({})", self.text_arg(args.first())?));
        }
        let key = self.text_arg(args.first())?;
        Some(match builtin {
            Builtin::PgsCreateKey => format!("PGS_CREATE({key}, {})", self.int_arg(args.get(1))?),
            Builtin::PgsKeyExists => format!("PGS_EXISTS({key})"),
            Builtin::PgsSetKeyVal => {
                let (index, value) = (self.int_arg(args.get(1))?, self.int_arg(args.get(2))?);
                format!("PGS_SET({key}, {index}, {value})")
            }
            Builtin::PgsGetKeyVal => format!("PGS_GET({key}, {})", self.int_arg(args.get(1))?),
            Builtin::PgsCreateStrKey => format!("PGS_STR_CREATE({key})"),
            Builtin::PgsStrKeyExists => format!("PGS_STR_EXISTS({key})"),
            Builtin::PgsSetStrKeyVal => format!("PGS_STR_SET({key}, {})", self.text_arg(args.get(1))?),
            Builtin::PgsGetStrKeyVal => format!("PGS_STR_GET({key})"),
            _ => return None,
        })
    }
}
