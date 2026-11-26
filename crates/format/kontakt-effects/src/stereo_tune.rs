//! Stereo Tune (module `0x70`, saved v`0x10`, `$EFFECT_TYPE_STEREOTUNE` = 61)
//! onto [`ir::StereoDetune`].
//!
//! Saved record (16 bytes, `Fx4`): `split`, `drift`, `spread`, `mix`, each in `0..=1`.
//!
//! Laws:
//! - `split_hz = 900 * split + 100` Hz (100..1000 Hz crossover);
//! - `f = 0.85 * 0.4 * drift + 0.06`, `ratio = 2^(4 * f^3 / 12)` (`1.0594631^(4 * f^3)`);
//! - `spread` and `mix` pass directly in `0..=1`.

use kontra_ir as ir;

use crate::module::{UNIT_LAW, f32_le, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoTune {
    pub split: f32,
    pub drift: f32,
    pub spread: f32,
    pub mix: f32,
}

/// Fresh INIT defaults for a script-loaded Stereo Tune.
pub const INIT: StereoTune = StereoTune {
    split: 0.25,
    drift: 0.3,
    spread: 0.5,
    mix: 0.5,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<StereoTune> {
    if version != 0x10 {
        return None;
    }
    Some(StereoTune {
        split: f32_le(public, 0)?,
        drift: f32_le(public, 4)?,
        spread: f32_le(public, 8)?,
        mix: f32_le(public, 12)?,
    })
}

/// Physical IR settings for `s`.
#[must_use]
pub fn detune(s: StereoTune) -> ir::StereoDetune {
    let split = s.split.clamp(0.0, 1.0);
    let drift = s.drift.clamp(0.0, 1.0);
    let f = 0.85 * 0.4 * drift + 0.06;
    let semis = 4.0 * f * f * f;
    ir::StereoDetune {
        split_hz: 900.0 * split + 100.0,
        ratio: (semis / 12.0).exp2(),
        spread: s.spread.clamp(0.0, 1.0),
        mix: s.mix.clamp(0.0, 1.0),
    }
}

/// Single-encoding module descriptor for Stereo Tune (`0x70`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::STEREO_TUNE,
    module_id: 0x70,
    versions: (0x10, 0x10),
    name: "Stereo Tune",
    script_type: 61,
    mix: crate::module::MixMode::OwnMix,
    knobs: crate::module::KNOBS.split_at(4).0,
    build: |v| {
        let [split, drift, spread, mix] = *v else {
            return None;
        };
        Some(ir::EffectKind::StereoDetune(detune(StereoTune {
            split,
            drift,
            spread,
            mix,
        })))
    },
};

#[must_use]
pub fn model(s: StereoTune) -> ir::KnobModel {
    MODULE.knob_model(&[s.split, s.drift, s.spread, s.mix])
}

/// KSP `$ENGINE_PAR_STEREOTUNE_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 4] {
    [
        knob_row("$ENGINE_PAR_STEREOTUNE_SPLIT", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_STEREOTUNE_DRIFT", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_STEREOTUNE_SPREAD", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_STEREOTUNE_MIX", 3, UNIT_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_laws_match_endpoints() {
        let bytes: Vec<u8> = [0.0f32, 0.5, 0.75, 1.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let s = saved(0x10, &bytes).unwrap();
        assert_eq!(s.split, 0.0);
        assert_eq!(s.drift, 0.5);
        assert_eq!(s.spread, 0.75);
        assert_eq!(s.mix, 1.0);
        assert!(saved(0x20, &bytes).is_none());

        let d0 = detune(StereoTune {
            split: 0.0,
            drift: 0.0,
            spread: 0.0,
            mix: 0.0,
        });
        assert!((d0.split_hz - 100.0).abs() < 1e-4);
        assert!(d0.ratio > 1.0 && d0.ratio < 1.001);

        let d1 = detune(StereoTune {
            split: 1.0,
            drift: 1.0,
            spread: 1.0,
            mix: 1.0,
        });
        assert!((d1.split_hz - 1000.0).abs() < 1e-4);
        let expected_ratio = (4.0 * 0.4f32.powi(3) / 12.0).exp2();
        assert!((d1.ratio - expected_ratio).abs() < 1e-5);
    }

    #[test]
    fn knob_model_roundtrips() {
        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::StereoDetune(detune(INIT)))
        );
    }
}
