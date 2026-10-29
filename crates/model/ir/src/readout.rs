//! What a script reads back as a parameter's text (`get_engine_par_disp`).

use crate::Law;

/// A parameter's text: `law` of its 0..=1 setting written as `form`, with
/// fixed texts at the setting's two ends where the format has them.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    pub law: Law,
    pub form: Form,
    /// The text at setting 0 and at setting 1, when it is not the law's.
    pub ends: [Option<&'static str>; 2],
}

/// How a readout writes its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// Five places, two decimals under ±1, thousands as `k`: ms, Hz, dB.
    Padded,
    /// One decimal: a percentage or a bipolar amount.
    Decimal,
    /// Five places, one decimal.
    Wide,
    /// A whole number.
    Whole,
    /// `1:` and the ratio in four places, one decimal.
    Ratio,
    /// `Center`, or `n L` / `n R` of the setting.
    Pan,
}
