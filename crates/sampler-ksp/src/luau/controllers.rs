//! Controllers for the Luau front end: `on controller` (`%CC[]`, `$CC_NUM`,
//! `ignore_controller`) and `set_controller` in `on init`. The rules are the runtime's
//! (`controller_event.rs`, `behavior.rs`): the input bank holds the last value of each
//! controller number, `%CC[n]` converts it to KSP units (0..127; pitch bend at 128 is
//! -8192..8191), and an `on controller` body forwards its CC unless it calls
//! `ignore_controller`. `set_controller` inside `on controller` (it injects a new CC)
//! and waits there are refused at compile time.
//!
//! ponytail: one input bank for every origin. The runtime keeps one per MIDI channel,
//! so a driver feeding several channels needs per-origin banks.

use super::Compiler;
use crate::builtins::Builtin;
use crate::hir::{Arg, CallbackKind};

/// Lua side. `HOST.cc` is the input bank (0..129, raw u32 values) with a `live` flag:
/// `%CC[]` reads 0 until `on init` has run, as the interpreter does there.
pub(super) const PRELUDE: &str = "\
local CCB = HOST.cc
local SETCC = HOST.set_controller
local CCN = 0
local CCIG = false
local function CCV(n)
  if CCB.live ~= true then return 0 end
  if n < 0 or n >= 130 then error('%CC index out of range') end
  local v = CCB[n]
  if n == 128 then return math.floor(v / 262144) - 8192 end
  return math.floor((v * 127 + 2147483647) / 4294967295)
end
local function CCIGN()
  CCIG = true
  return 0
end
";

impl Compiler<'_> {
    /// `%CC[n]`: the bank value in KSP units. Readable everywhere, as the runtime allows.
    pub(super) fn cc_read(&mut self, index: &crate::hir::Expr) -> String {
        format!("CCV({})", self.int(index))
    }

    /// `$CC_NUM`: only inside `on controller`.
    pub(super) fn cc_number(&mut self) -> String {
        match self.cc_ctx {
            Some(CallbackKind::Controller) => "CCN".into(),
            _ => self.unsupported("sys CcNum outside on controller"),
        }
    }

    /// Controller builtins; `None` leaves the call to the next module.
    pub(super) fn cc_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        Some(match builtin {
            // Inside `on controller` it clears the forwarding. Elsewhere in a callback the
            // interpreter warns and does nothing; in a function the caller's context decides
            // what it means, so it is refused.
            Builtin::IgnoreController => match self.cc_ctx {
                Some(CallbackKind::Controller) => "CCIGN()".into(),
                Some(_) => "0".into(),
                None => self.unsupported("ignore_controller in function"),
            },
            Builtin::SetController => match (self.cc_ctx, args.first(), args.get(1)) {
                (Some(CallbackKind::Init), Some(Arg::Expr(n)), Some(Arg::Expr(v))) => {
                    let (n, v) = (self.int(n), self.int(v));
                    format!("SETCC({n}, {v})")
                }
                _ => self.unsupported("set_controller outside on init"),
            },
            _ => return None,
        })
    }
}
