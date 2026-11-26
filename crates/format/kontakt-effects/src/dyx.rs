//! DYX / AET Filter (module `0x1b`, saved v`0x60`, `$EFFECT_TYPE_AET_FILTER` = 25)
//! onto [`ir::SpectralMorph`].
//!
//! Saved record (13 bytes, `FFIB`): `morph` (`0..=1`), `amount` (`0..=1`),
//! `bank` (`i32`), `tracking` (`u8`, bool). Live KSP drives `morph` and `amount`
//! via `$ENGINE_PAR_EXP_FILTER_MORPH` and `$ENGINE_PAR_EXP_FILTER_AMOUNT`.

use kontra_ir as ir;

use crate::module::{MixMode, Module, UNIT_LAW, f32_le, i32_le, ids, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dyx {
    pub morph: f32,
    pub amount: f32,
    pub bank: i32,
    pub tracking: bool,
}

/// Fresh INIT defaults for a script-loaded AET Filter.
pub const INIT: Dyx = Dyx {
    morph: 0.5,
    amount: 0.75,
    bank: 0,
    tracking: false,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Dyx> {
    if version != 0x60 {
        return None;
    }
    Some(Dyx {
        morph: f32_le(public, 0)?,
        amount: f32_le(public, 4)?,
        bank: i32_le(public, 8)?,
        tracking: *public.get(12)? != 0,
    })
}

/// Physical IR settings for `d`.
#[must_use]
pub fn spectral_morph(d: Dyx) -> ir::SpectralMorph {
    ir::SpectralMorph {
        morph: d.morph.clamp(0.0, 1.0),
        amount: d.amount.clamp(0.0, 1.0),
        bank: d.bank.clamp(0, 3) as u8,
        tracking: d.tracking,
    }
}

/// Single-encoding module descriptor for DYX / AET Filter (`0x1b`).
pub static MODULE: Module = Module {
    id: ids::DYX,
    module_id: 0x1b,
    versions: (0x60, 0x60),
    name: "DYX",
    script_type: 25,
    mix: MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(2).0,
    build: |v| {
        let [morph, amount, bank, tracking] = *v else {
            return None;
        };
        Some(ir::EffectKind::SpectralMorph(spectral_morph(Dyx {
            morph,
            amount,
            bank: bank.round() as i32,
            tracking: tracking >= 0.5,
        })))
    },
};

#[must_use]
pub fn model(d: Dyx) -> ir::KnobModel {
    MODULE.knob_model(&[
        d.morph,
        d.amount,
        d.bank as f32,
        if d.tracking { 1.0 } else { 0.0 },
    ])
}

/// KSP `$ENGINE_PAR_EXP_FILTER_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 2] {
    [
        knob_row("$ENGINE_PAR_EXP_FILTER_MORPH", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_EXP_FILTER_AMOUNT", 1, UNIT_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_model_roundtrips() {
        let mut bytes = Vec::new();
        bytes.extend(0.35f32.to_le_bytes());
        bytes.extend(0.85f32.to_le_bytes());
        bytes.extend(2i32.to_le_bytes());
        bytes.push(1);
        let d = saved(0x60, &bytes).unwrap();
        assert_eq!(d.bank, 2);
        assert!(d.tracking);
        let ir_m = spectral_morph(d);
        assert_eq!(ir_m.bank, 2);
        assert!(ir_m.tracking);

        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::SpectralMorph(spectral_morph(INIT)))
        );
    }
}
