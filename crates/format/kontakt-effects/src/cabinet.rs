//! Cabinet (module `0x24`, saved v`0x50`, `$EFFECT_TYPE_CABINET` = 24) onto
//! [`ir::Cabinet`].
//!
//! Saved record (20 bytes, `FFFFI`): `size` (`0..=1`), `air` (`0..=1`),
//! `treble` (`-1..=1`), `bass` (`-1..=1`), `cabinet_type` (`i32`, `0..=11`).

use kontra_ir as ir;

use crate::module::{MixMode, Module, RAW_LAW, UNIT_LAW, f32_le, i32_le, ids, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cabinet {
    pub size: f32,
    pub air: f32,
    pub treble: f32,
    pub bass: f32,
    pub cabinet_type: i32,
}

/// Fresh INIT defaults for a script-loaded Cabinet.
pub const INIT: Cabinet = Cabinet {
    size: 0.5,
    air: 0.25,
    treble: 0.0,
    bass: 0.0,
    cabinet_type: 0,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Cabinet> {
    if version != 0x50 {
        return None;
    }
    Some(Cabinet {
        size: f32_le(public, 0)?,
        air: f32_le(public, 4)?,
        treble: f32_le(public, 8)?,
        bass: f32_le(public, 12)?,
        cabinet_type: i32_le(public, 16)?,
    })
}

/// Physical IR settings for `c`.
#[must_use]
pub fn cabinet(c: Cabinet) -> ir::Cabinet {
    ir::Cabinet {
        model: c.cabinet_type.clamp(0, 11) as u8,
        size: c.size.clamp(0.0, 1.0),
        air: c.air.clamp(0.0, 1.0),
        bass_db: c.bass.clamp(-1.0, 1.0) * 12.0,
        treble_db: c.treble.clamp(-1.0, 1.0) * 12.0,
    }
}

/// Single-encoding module descriptor for Cabinet (`0x24`).
pub static MODULE: Module = Module {
    id: ids::CABINET,
    module_id: 0x24,
    versions: (0x50, 0x50),
    name: "Cabinet",
    script_type: 24,
    mix: MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(5).0,
    build: |v| {
        let [size, air, treble, bass, cabinet_type] = *v else {
            return None;
        };
        Some(ir::EffectKind::Cabinet(cabinet(Cabinet {
            size,
            air,
            treble,
            bass,
            cabinet_type: cabinet_type.round() as i32,
        })))
    },
};

#[must_use]
pub fn model(c: Cabinet) -> ir::KnobModel {
    MODULE.knob_model(&[c.size, c.air, c.treble, c.bass, c.cabinet_type as f32])
}

/// KSP `$ENGINE_PAR_CB_*` and `$ENGINE_PAR_CABINET_TYPE` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 5] {
    const BIPOLAR_TONE: ir::Law = ir::Law::Linear { lo: -1.0, hi: 1.0 };
    [
        knob_row("$ENGINE_PAR_CB_SIZE", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_CB_AIR", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_CB_TREBLE", 2, BIPOLAR_TONE),
        knob_row("$ENGINE_PAR_CB_BASS", 3, BIPOLAR_TONE),
        knob_row("$ENGINE_PAR_CABINET_TYPE", 4, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_model_roundtrips() {
        let mut bytes = Vec::new();
        for f in [0.6f32, 0.4, -0.5, 0.75] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.extend(3i32.to_le_bytes());
        let c = saved(0x50, &bytes).unwrap();
        assert_eq!(c.cabinet_type, 3);
        assert!((c.treble + 0.5).abs() < 1e-6);
        let ir_cab = cabinet(c);
        assert_eq!(ir_cab.model, 3);
        assert!((ir_cab.treble_db + 6.0).abs() < 1e-4);
        assert!((ir_cab.bass_db - 9.0).abs() < 1e-4);

        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::Cabinet(cabinet(INIT)))
        );
    }
}
