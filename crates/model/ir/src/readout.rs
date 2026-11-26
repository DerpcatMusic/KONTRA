//! What a script reads back as a parameter's text (`get_engine_par_disp`).

use crate::Law;

/// A parameter's text: `law` of its 0..=1 setting written as `form`, with
/// fixed texts at the setting's two ends where the format has them, and
/// optional overrides when the target slot carries a specific subtype id.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    pub law: Law,
    pub form: Form,
    /// The text at setting 0 and at setting 1, when it is not the law's.
    pub ends: [Option<&'static str>; 2],
    /// Subtype-specific overrides `(subtype_id, readout)` (e.g. filter cutoff
    /// ranges per filter family). Empty for subtype-independent parameters.
    pub by_subtype: std::sync::Arc<[(i32, Readout)]>,
}

impl Readout {
    /// Returns the subtype-specific readout when `subtype` matches an entry in
    /// [`Self::by_subtype`], otherwise `self`.
    #[must_use]
    pub fn for_subtype(&self, subtype: Option<i32>) -> &Self {
        subtype
            .and_then(|sub| self.by_subtype.iter().find(|(id, _)| *id == sub))
            .map_or(self, |(_, r)| r)
    }
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
    /// Two decimals (semitones).
    Hundredths,
    /// A whole number in five places (degrees).
    WideWhole,
    /// A whole number in three places (`%3.0f`).
    NarrowWhole,
    /// No text: a switch shows none.
    Blank,
    /// `x ` and one decimal: a multiple.
    Times,
    /// A whole number in three places and ` BPM`: a tempo.
    Bpm,
    /// A note name and octave, C-2 at 0 (`%s%d`): a pitch in notes.
    Note,
    /// Two decimals under 1, else one, unpadded: a rate in Hz.
    Rate,
    /// One of these texts: entry `floor(n·x)`, the last at 1.
    Menu(&'static [&'static str]),
}
