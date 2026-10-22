//! Kontakt Stereo Tune (module 0x70, record version 0x10): four saved knobs on 0..=1.
//!
//! The record is four floats in order (reader `0x140ac6120`): split, drift, spread, mix. The DSP is
//! `sampler_core::dsp::stereo_tune` (native apply `0x140abbce0`, verified bit-exact against Kontakt 8).

/// Stereo Tune controls in saved (0..=1) units, in record order.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// One-pole tone coefficient: `(900 * split + 100)` Hz at the native rate scale.
    pub split: f64,
    /// Tap pitch drift, `ratio = 2^(...)` per the native apply.
    pub drift: f64,
    /// Cross-channel spread.
    pub spread: f64,
    /// Wet amount, linear.
    pub mix: f64,
}

impl Config {
    /// The field whose value is out of range, or `None` when the configuration is valid.
    pub fn check(&self) -> Result<(), &'static str> {
        for (name, value) in [
            ("stereo tune split", self.split),
            ("stereo tune drift", self.drift),
            ("stereo tune spread", self.spread),
            ("stereo tune mix", self.mix),
        ] {
            if !(value.is_finite() && (0. ..=1.).contains(&value)) {
                return Err(name);
            }
        }
        Ok(())
    }
}
