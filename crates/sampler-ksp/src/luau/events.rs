//! Event builtins for the Luau front end: `change_vol` / `change_tune` /
//! `change_pan` and `ignore_event` on the current note. The rules are the
//! interpreter's (`lower.rs` `write_param`, `SuppressAttack`); the engine side is
//! [`crate::engine::EngineSink`]. Anything else returns `None` and stays unsupported.

use super::{Compiler, ex};
use crate::builtins::{Builtin, SysVar};
use crate::hir::{Arg, ExprKind};

impl Compiler<'_> {
    /// `None` leaves the call unsupported (refused at compile time).
    pub(super) fn event_builtin(&mut self, builtin: Builtin, args: &[Arg]) -> Option<String> {
        use Builtin::*;
        Some(match builtin {
            // Target codes are read by `Host`: 0 Decibels, 1 Pitch, 2 Pan.
            ChangeVol | ChangeTune | ChangePan => {
                let target = match builtin {
                    ChangeVol => 0,
                    ChangeTune => 1,
                    _ => 2,
                };
                let event = self.int(ex(args, 0)?);
                let value = self.int(ex(args, 1)?);
                // Omitted flag is absolute, as the interpreter's `arg` yields 0.
                let relative = match args.get(2) {
                    None => "0".to_string(),
                    Some(Arg::Expr(e)) => format!("({} == 1 and 1 or 0)", self.int(e)),
                    Some(_) => return None,
                };
                format!("HOST.param({event}, {target}, {value}, {relative})")
            }
            // Only the current note's own event (`$EVENT_ID` in `on note`); other events refused.
            IgnoreEvent => match (self.note_ctx, ex(args, 0)) {
                (true, Some(e)) if matches!(e.kind, ExprKind::Sys(SysVar::EventId)) => "HOST.suppress_attack(EV)".to_string(),
                _ => return None,
            },
            _ => return None,
        })
    }
}
