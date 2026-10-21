//! Kontakt Ring Modulator (0x64, record version 0x50): saved fields to the shared ring stage.
//!
//! Field order is the saved record (bindings in `docs/re/kontakt-bindings/fx_other.tsv`):
//! 0 fast mode, 1 ring (mix), 2 FM, 3 frequency, 4 edge, 5 LFO wave, 6 LFO rate, 7 LFO amount.
//! Measured laws (see `sampler_ir::fx::ringmod`): carrier `0.625 * 128^x` Hz (x50 in fast mode),
//! mix linear, wet gain 1.6. Edge, FM and the LFO are not measured yet; a non-default value
//! there is reported, and the module is not marked as modelled.
use super::{Lowering, Notes, Site};
use ni_file::kontakt::objects::{EffectField, EffectValue};
use sampler_ir::fx::ringmod::Config;
use sampler_ir::{Processor, Reason};

/// Frequency law `0.625 * 128^x` Hz, the fast-mode multiplier 50 (Wine oracle, 2026-10-22).
const FREQUENCY_BASE_HZ: f64 = 0.625;
const FREQUENCY_RATIO: f64 = 128.;
const FAST_MULTIPLIER: f64 = 50.;

fn number(value: &EffectValue) -> Option<f64> {
    match *value {
        EffectValue::Float(v) => Some(f64::from(v)),
        EffectValue::Integer(v) => Some(f64::from(v)),
        EffectValue::Byte(v) => Some(f64::from(v)),
        EffectValue::Floats(_) => None,
    }
}

/// The ring stage and the parameters whose laws are not measured (name, reason).
pub(super) fn ring_modulator(fields: &[EffectField]) -> Option<(Config, Vec<(&'static str, String)>)> {
    let [fast, ring, fm, frequency, edge, wave, _rate, amount] = fields else {
        return None;
    };
    let (fast, ring, fm, frequency, edge, wave, amount) = (
        number(&fast.value)?,
        number(&ring.value)?,
        number(&fm.value)?,
        number(&frequency.value)?,
        number(&edge.value)?,
        number(&wave.value)?,
        number(&amount.value)?,
    );
    if !(0.0..=1.0).contains(&ring) || !(0.0..=1.0).contains(&frequency) {
        return None;
    }
    let mut notes = Vec::new();
    for (name, value) in [("edge", edge), ("fm", fm), ("lfo amount", amount), ("lfo wave", wave)] {
        if value != 0. {
            notes.push((name, format!("stored {value}: law not measured")));
        }
    }
    let scale = if fast != 0. { FAST_MULTIPLIER } else { 1. };
    let config = Config {
        frequency: FREQUENCY_BASE_HZ * FREQUENCY_RATIO.powf(frequency) * scale,
        mix: ring,
        gain: Config::KONTAKT_GAIN,
    };
    Some((config, notes))
}

/// 0x64 Ring Modulator: one ring stage. Unmeasured parameters are noted and the slot is not modelled.
pub(super) fn slot_ring_modulator(b: &mut Lowering, s: &Site, notes: &mut Notes, fields: &[EffectField]) -> bool {
    let Some((config, unmeasured)) = ring_modulator(fields) else {
        return false;
    };
    b.flush();
    b.out.processors.push(Processor::RingMod(config));
    b.combined = s.gain;
    for (name, why) in &unmeasured {
        notes.push(((*name).into(), why.clone(), Reason::UnknownLaw));
    }
    if s.dynamic.is_some() {
        notes.push(("Ring Modulator live controls".into(), "knobs are constants".into(), Reason::NotModeled));
    }
    unmeasured.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fast: u8, ring: f32, fm: f32, frequency: f32, edge: f32, wave: i32, rate: f32, amount: f32) -> Vec<EffectField> {
        [
            EffectValue::Byte(fast),
            EffectValue::Float(ring),
            EffectValue::Float(fm),
            EffectValue::Float(frequency),
            EffectValue::Float(edge),
            EffectValue::Integer(wave),
            EffectValue::Float(rate),
            EffectValue::Float(amount),
        ]
        .into_iter()
        .map(|value| EffectField { name: "param_n", value })
        .collect()
    }

    /// Carrier Hz at the stored frequency, measured on the Wine oracle (fast mode multiplies by 50).
    #[test]
    fn carrier_frequency_follows_the_measured_points() {
        let slow = [(0.25f32, 2.0), (0.5, 7.5), (0.75, 24.0), (1.0, 80.0)];
        let fast = [(0.25f32, 105.0), (0.5, 353.5), (0.75, 1189.5), (1.0, 4000.0)];
        for (x, hz) in slow {
            let (config, _) = ring_modulator(&record(0, 1., 0., x, 0., 0, 0., 0.)).unwrap();
            // The slow points are one zero-crossing per second apart (about 1 Hz at the low end).
            assert!((config.frequency - hz).abs() <= 0.1 * hz + 0.5, "slow {x}: {} vs {hz}", config.frequency);
        }
        for (x, hz) in fast {
            let (config, _) = ring_modulator(&record(1, 1., 0., x, 0., 0, 0., 0.)).unwrap();
            assert!((config.frequency / hz - 1.).abs() < 0.005, "fast {x}: {} vs {hz}", config.frequency);
        }
    }

    #[test]
    fn unmeasured_parameters_are_reported_not_modelled() {
        let (config, notes) = ring_modulator(&record(0, 0.5, 0., 1., 0.25, 0, 0., 0.)).unwrap();
        assert_eq!((config.mix, config.gain), (0.5, Config::KONTAKT_GAIN));
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "edge");
        assert!(ring_modulator(&record(0, 0.5, 0., 1., 0., 0, 0., 0.)).unwrap().1.is_empty());
    }
}
