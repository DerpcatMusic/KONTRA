//! The calls a script makes into the engine: [`HostApi`].

use crate::{Action, Text};
#[cfg(doc)]
use crate::{Instance, Returns};

/// One host call: the action, its arguments and the instance cells (for
/// actions that take arrays by reference as `base, len` pairs).
pub struct HostCall<'a> {
    pub action: Action,
    /// Opaque context the engine gave the callback ([`Instance::start`]), e.g. its event.
    pub context: u64,
    pub ints: &'a [i64],
    pub texts: &'a [Text],
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
}
