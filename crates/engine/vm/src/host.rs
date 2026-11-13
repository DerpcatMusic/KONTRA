//! The calls a script makes into the engine: [`HostApi`].

use crate::{Action, Text};
#[cfg(doc)]
use crate::{Instance, Returns};

/// One host call: the action, its arguments and the instance cells (for
/// actions that take arrays by reference as `base, len` pairs).
pub struct HostCall<'a> {
    /// What the script asked for.
    pub action: Action,
    /// Opaque context the engine gave the callback ([`Instance::start`]), e.g. its event.
    pub context: u64,
    /// The number arguments in order: an integer as it is, a real as its
    /// `f64` bits, an array as its first cell and length.
    pub ints: &'a [i64],
    /// The text arguments in order.
    pub texts: &'a [Text],
    /// The script's number variables. An action writes only the cells its
    /// arguments name (an array it fills, a control's value it sets).
    pub cells: &'a mut [i64],
    /// The script's text variables (a saved text read back mid-init).
    pub text_cells: &'a mut [Text],
    /// Where a builtin that returns text writes it (then returns [`HostValue::Text`]).
    pub text_out: &'a mut Text,
}

/// What a host call returns. Must match the call's [`Returns`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostValue {
    Nothing,
    Int(i32),
    Real(f64),
    /// The text is in [`HostCall::text_out`].
    Text,
    /// Not done yet: the callback waits this many microseconds, then makes
    /// the same call again (`wait_async` on a command still running).
    Wait(u64),
}

/// The engine side of every non-pure action, implemented once (`kontra-engine`).
pub trait HostApi {
    fn call(&mut self, call: HostCall<'_>) -> HostValue;

    /// A read-only answer the host keeps ready (a count, a flag): no call
    /// record, no text or cell access. `None` sends the action to [`Self::call`].
    fn query(&mut self, _action: Action, _ints: &[i64]) -> Option<i32> {
        None
    }
}
