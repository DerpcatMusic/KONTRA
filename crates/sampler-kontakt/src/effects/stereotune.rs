//! Kontakt Stereo Tune (0x70, record version 0x10): four saved floats to the bus stereo tune stage.
//!
//! The reader (`0x140ac6120`) takes exactly 16 bytes and stores them in order at the apply's params
//! `+0x8` (split), `+0xc` (drift), `+0x10` (spread), `+0x14` (mix); apply `0x140abbce0` reads them back
//! in that order. The DSP is `sampler_core::dsp::stereo_tune`, bit-exact to Kontakt 8 kemu vectors.
//! Its state is bus-owned, so only a bus-scope chain lowers it (like the reverbs).
use super::{Lowering, Notes, Site};
use sampler_ir::fx::stereo_tune::Config;
use sampler_ir::{Processor, Reason};

/// Stored knobs in record order: split, drift, spread, mix.
pub(super) fn config(knobs: [f32; 4]) -> Config {
    let [split, drift, spread, mix] = knobs.map(f64::from);
    Config { split, drift, spread, mix }
}

/// 0x70 Stereo Tune in a bus chain: one stereo tune stage. Out-of-range stored knobs are not modelled.
pub(super) fn slot_stereo_tune(b: &mut Lowering, s: &Site, notes: &mut Notes, knobs: [f32; 4]) -> bool {
    let config = config(knobs);
    if config.check().is_err() {
        return false;
    }
    b.flush();
    b.out.processors.push(Processor::StereoTune(config));
    b.combined = s.gain;
    if s.dynamic.is_some() {
        notes.push(("Stereo Tune live controls".into(), "knobs are constants".into(), Reason::NotModeled));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{Scope, chain, test_support::slot};

    /// Record order is apply order: a saved 16-byte record lowers to split, drift, spread, mix.
    #[test]
    fn saved_record_lowers_in_apply_order() {
        let bytes: Vec<u8> = [0.1f32, 0.2, 0.3, 0.25].into_iter().flat_map(f32::to_le_bytes).collect();
        let mut fx = slot(0x70, bytes, 1.0);
        fx.version = 0x10;
        let built = chain(&[fx], Scope::Bus);
        let expected = Config { split: f64::from(0.1f32), drift: f64::from(0.2f32), spread: f64::from(0.3f32), mix: f64::from(0.25f32) };
        assert_eq!(built.processors, vec![sampler_ir::Processor::StereoTune(expected)]);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }

    #[test]
    fn out_of_range_knob_is_rejected() {
        assert!(config([1.5, 0.2, 0.3, 0.25]).check().is_err());
        assert!(config([0.1, 0.2, 0.3, 0.25]).check().is_ok());
    }
}
