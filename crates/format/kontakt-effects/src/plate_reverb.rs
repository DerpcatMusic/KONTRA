//! Plate Reverb (module `0x57`, saved v`0x10`, `$EFFECT_TYPE_PLATEREVERB` = 38)
//! onto [`ir::Reverb`] with [`ir::Network::Plate`].
//!
//! Saved record (20 bytes, `Fx5`): `decay`, `low_shelf`, `high_damp`,
//! `predelay`, `stereo`, each in `0..=1`.
//!
//! Laws:
//! - `seconds = 6.4 * decay + 0.556` s (556..6956 ms);
//! - `low_shelf_db = 5.625 - 34.375 * low_shelf` dB (`20 * ((1 - ls) * 1.71875 - 1.4375)`);
//! - `damping = high_damp`, `high_cut_hz = (1 - high_damp) * 4851 + 6000` Hz;
//! - `predelay_ms = 250 * predelay` ms (0..250 ms);
//! - `stereo` passes in `0..=1`.

use kontra_ir as ir;

use crate::module::{UNIT_LAW, f32_le, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateReverb {
    pub decay: f32,
    pub low_shelf: f32,
    pub high_damp: f32,
    pub predelay: f32,
    pub stereo: f32,
}

/// Fresh INIT defaults for a script-loaded Plate Reverb.
pub const INIT: PlateReverb = PlateReverb {
    decay: 0.5,
    low_shelf: 0.5,
    high_damp: 0.5,
    predelay: 0.0,
    stereo: 1.0,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<PlateReverb> {
    if version != 0x10 {
        return None;
    }
    Some(PlateReverb {
        decay: f32_le(public, 0)?,
        low_shelf: f32_le(public, 4)?,
        high_damp: f32_le(public, 8)?,
        predelay: f32_le(public, 12)?,
        stereo: f32_le(public, 16)?,
    })
}

/// Physical IR reverb settings for `p`.
#[must_use]
pub fn reverb(p: PlateReverb) -> ir::Reverb {
    let d = p.decay.clamp(0.0, 1.0);
    let ls = p.low_shelf.clamp(0.0, 1.0);
    let hd = p.high_damp.clamp(0.0, 1.0);
    let pre = p.predelay.clamp(0.0, 1.0);
    let st = p.stereo.clamp(0.0, 1.0);
    ir::Reverb {
        seconds: 6.4 * d + 0.556,
        size: 1.0,
        network: Some(ir::Network::Plate),
        damping: hd,
        diffusion: 0.9,
        predelay_ms: 250.0 * pre,
        high_cut_hz: (1.0 - hd) * 4851.0 + 6000.0,
        low_shelf_db: 20.0 * ((1.0 - ls) * 1.71875 - 1.4375),
        stereo: st,
        modulation_ms: 0.0,
        modulation_hz: 0.0,
    }
}

/// Single-encoding module descriptor for Plate Reverb (`0x57`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::PLATE_REVERB,
    module_id: 0x57,
    versions: (0x10, 0x10),
    name: "Plate Reverb",
    script_type: 38,
    mix: crate::module::MixMode::WetDry,
    knobs: crate::module::KNOBS.split_at(5).0,
    build: |v| {
        let [decay, low_shelf, high_damp, predelay, stereo] = *v else {
            return None;
        };
        Some(ir::EffectKind::Reverb(reverb(PlateReverb {
            decay,
            low_shelf,
            high_damp,
            predelay,
            stereo,
        })))
    },
};

#[must_use]
pub fn model(p: PlateReverb) -> ir::KnobModel {
    MODULE.knob_model(&[p.decay, p.low_shelf, p.high_damp, p.predelay, p.stereo])
}

/// KSP `$ENGINE_PAR_PR_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 5] {
    [
        knob_row("$ENGINE_PAR_PR_DECAY", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_PR_LOWSHELF", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_PR_HIDAMP", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_PR_PREDELAY", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_PR_STEREO", 4, UNIT_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_laws_match_endpoints() {
        let bytes: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.5]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let p = saved(0x10, &bytes).unwrap();
        let r = reverb(p);
        assert!((r.seconds - 0.556).abs() < 1e-5);
        assert!((r.low_shelf_db - 5.625).abs() < 1e-4);
        assert!((r.high_cut_hz - 10_851.0).abs() < 1e-3);
        assert!((r.predelay_ms - 250.0).abs() < 1e-4);
        assert!((r.stereo - 0.5).abs() < 1e-6);

        let r_max = reverb(PlateReverb {
            decay: 1.0,
            low_shelf: 1.0,
            high_damp: 1.0,
            predelay: 0.0,
            stereo: 1.0,
        });
        assert!((r_max.seconds - 6.956).abs() < 1e-4);
        assert!((r_max.low_shelf_db + 28.75).abs() < 1e-4);
        assert!((r_max.high_cut_hz - 6000.0).abs() < 1e-3);
    }

    #[test]
    fn knob_model_roundtrips() {
        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::Reverb(reverb(INIT)))
        );
    }
}
