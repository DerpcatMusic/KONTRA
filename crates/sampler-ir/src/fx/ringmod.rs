//! Ring modulator: the input times a sine carrier, crossfaded with the dry input.
//!
//! Laws measured through Kontakt 8 (Wine oracle, DC stimulus, 2026-10-22; vectors in
//! `docs/re/engine/k-modfx-freak-ring-rotator.md`):
//! - carrier frequency `0.625 * 128^x` Hz for the stored frequency `x` in 0..=1, times 50 in
//!   fast mode (105, 353.5, 1189.5, 4000 Hz at x = 0.25, 0.5, 0.75, 1 in fast mode);
//! - output `(1 - mix) * dry + mix * 1.6 * dry * carrier`, so the wet peak is 1.6 times the dry level.
//! Edge, FM and the LFO are not yet measured and are not represented here.

/// Ring modulator controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Carrier frequency in Hz (positive, finite).
    pub frequency: f64,
    /// Crossfade from dry (0) to ring (1), linear.
    pub mix: f64,
    /// Wet gain applied to `input * carrier` before the crossfade (Kontakt: 1.6).
    pub gain: f64,
}

impl Config {
    /// Kontakt's wet gain: wet peak over dry level at mix 1.
    pub const KONTAKT_GAIN: f64 = 1.6;

    /// The field whose value is out of range, or `None` when the configuration is valid.
    pub fn check(&self) -> Result<(), &'static str> {
        if !(self.frequency.is_finite() && self.frequency > 0.) {
            return Err("ringmod frequency");
        }
        if !(self.mix.is_finite() && (0. ..=1.).contains(&self.mix)) {
            return Err("ringmod mix");
        }
        if !(self.gain.is_finite() && self.gain >= 0.) {
            return Err("ringmod gain");
        }
        Ok(())
    }
}
