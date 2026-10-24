//! Tolerance bands for the Kontakt-fidelity axis, and the verdict rule that uses them.

use serde::{Deserialize, Serialize};

/// Acceptance bands. A modern model is "faithful" when its fitted error stays inside these.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Band {
    /// Magnitude shape error (max deviation after removing the mean offset), dB.
    pub mag_db: f64,
    /// Phase error in the audible band, degrees.
    pub phase_deg: f64,
    /// Time-constant error, percent.
    pub time_pct: f64,
    /// How much cleaner (dB of aliasing / noise, or any stability win) counts as "cleaner".
    pub cleaner_margin_db: f64,
}

impl Default for Band {
    fn default() -> Self {
        Self { mag_db: 0.5, phase_deg: 5.0, time_pct: 10.0, cleaner_margin_db: 3.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Verdict {
    /// Inside the Kontakt-fidelity band; no quality win over the other side.
    Faithful,
    /// Inside the band and measurably cleaner on aliasing, noise or stability.
    FaithfulCleaner,
    /// Outside the band: a deliberate character change.
    Differs,
    /// No Kontakt reference captured for this block; only absolute quality is reported.
    NoReference,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Faithful => "faithful",
            Verdict::FaithfulCleaner => "faithful + cleaner",
            Verdict::Differs => "differs from Kontakt (character change)",
            Verdict::NoReference => "no Kontakt reference",
        }
    }
}

/// Classify one implementation. `fits` is `(mag_err, phase_err, time_err)` after the best mapping,
/// `None` for an axis that has no reference data. `cleaner` is set when this side beats the other
/// on aliasing, noise or stability by at least the band's margin.
pub fn classify(band: &Band, fits: Option<(f64, Option<f64>, Option<f64>)>, cleaner: bool) -> Verdict {
    let Some((mag, phase, time)) = fits else {
        return Verdict::NoReference;
    };
    let inside = mag <= band.mag_db
        && phase.is_none_or(|p| p <= band.phase_deg)
        && time.is_none_or(|t| t <= band.time_pct);
    match (inside, cleaner) {
        (false, _) => Verdict::Differs,
        (true, true) => Verdict::FaithfulCleaner,
        (true, false) => Verdict::Faithful,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_follows_the_bands_and_the_cleaner_flag() {
        let band = Band::default();
        assert_eq!(classify(&band, None, true), Verdict::NoReference);
        assert_eq!(classify(&band, Some((0.3, Some(2.0), Some(5.0))), false), Verdict::Faithful);
        assert_eq!(classify(&band, Some((0.3, Some(2.0), Some(5.0))), true), Verdict::FaithfulCleaner);
        assert_eq!(classify(&band, Some((0.8, None, None)), true), Verdict::Differs);
        assert_eq!(classify(&band, Some((0.3, Some(6.0), None)), false), Verdict::Differs);
    }
}
