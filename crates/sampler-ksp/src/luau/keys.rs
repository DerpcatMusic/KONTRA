//! Key-switch interface state for the Luau front end: `set_key_color` / `set_key_type` /
//! `set_key_name` / `set_key_pressed` (+ `_support`), `set_keyrange` / `remove_keyrange`,
//! and their getters. The rules are the interpreter's (`eval.rs`, `SetKeyColor` ..
//! `RemoveKeyrange`): the 128 keys and the ranges live in `Initial::model.interface`, and
//! [`check_init`](super::check_init) compares them. Out-of-range keys are no-ops; the
//! interpreter's warning is not modelled.

use super::Compiler;
use crate::builtins::Builtin;
use crate::hir::Arg;

/// Lua side: 128 keys (index 0..127) with optional fields, the ranges, and the support flag.
/// `MODEL` comes from the PGS prelude, which is emitted first.
pub(super) const PRELUDE: &str = "\
local KEYS = {}
for i = 0, 127 do KEYS[i] = {} end
MODEL.keys = KEYS
MODEL.key_ranges = {}
MODEL.key_pressed_support = false
local function SET_KEY(k, field, v)
  local key = KEYS[k]
  if key ~= nil then key[field] = v end
  return 0
end
local function GET_KEY(k, field, default)
  local key = KEYS[k]
  if key == nil or key[field] == nil then return default end
  return key[field]
end
local function SET_KEYRANGE(low, high, name)
  local ranges = MODEL.key_ranges
  ranges[#ranges + 1] = { low = low, high = high, name = name }
  return 0
end
local function SET_KEY_SUPPORT(v)
  MODEL.key_pressed_support = v ~= 0
  return 0
end
local function REMOVE_KEYRANGE(k)
  local kept = {}
  for _, r in ipairs(MODEL.key_ranges) do
    if not (r.low <= k and k <= r.high) then kept[#kept + 1] = r end
  end
  MODEL.key_ranges = kept
  return 0
end
";

impl Compiler<'_> {
    /// Key-switch builtins; `None` leaves the call to the next module.
    pub(super) fn key_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        Some(match builtin {
            Builtin::SetKeyColor | Builtin::SetKeyType | Builtin::SetKeyPressed => {
                let field = match builtin {
                    Builtin::SetKeyColor => "color",
                    Builtin::SetKeyType => "kind",
                    _ => "pressed",
                };
                let key = self.int_arg(args.first())?;
                let value = self.int_arg(args.get(1))?;
                format!("SET_KEY({key}, \"{field}\", {value})")
            }
            Builtin::SetKeyName => {
                let key = self.int_arg(args.first())?;
                let name = self.text_arg(args.get(1))?;
                format!("SET_KEY({key}, \"name\", {name})")
            }
            Builtin::GetKeyColor => format!("GET_KEY({}, \"color\", 16)", self.int_arg(args.first())?),
            Builtin::GetKeyType => format!("GET_KEY({}, \"kind\", 0)", self.int_arg(args.first())?),
            Builtin::GetKeyName => format!("GET_KEY({}, \"name\", \"\")", self.int_arg(args.first())?),
            Builtin::SetKeyPressedSupport => {
                format!("SET_KEY_SUPPORT({})", self.int_arg(args.first())?)
            }
            Builtin::SetKeyrange => {
                let (low, high) = (self.int_arg(args.first())?, self.int_arg(args.get(1))?);
                let name = self.text_arg(args.get(2))?;
                format!("SET_KEYRANGE({low}, {high}, {name})")
            }
            Builtin::RemoveKeyrange => format!("REMOVE_KEYRANGE({})", self.int_arg(args.first())?),
            _ => return None,
        })
    }
}
