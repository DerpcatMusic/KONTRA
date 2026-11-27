//! Core Cell (module `0x5e`, saved v`0x51`, legacy placeholder with no KSP type).
//!
//! Saved record (40 bytes, 10 floats). In Kontakt 8 this legacy slot zeroes its
//! stereo output (`ir::EffectKind::Gain(0.0)`) and exposes no KSP parameters.

use kontra_ir as ir;

use crate::module::{Cursor, MixMode, Module, ids};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoreCell {
    pub params: [f32; 10],
}

/// Fresh INIT defaults for Core Cell.
pub const INIT: CoreCell = CoreCell { params: [0.0; 10] };

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<CoreCell> {
    if version != 0x51 {
        return None;
    }
    Some(CoreCell {
        params: Cursor::new(public).f32_array()?,
    })
}

/// Physical IR effect matching Kontakt's zeroed-output Core Cell placeholder.
#[must_use]
pub const fn effect(_c: CoreCell) -> ir::EffectKind {
    ir::EffectKind::Gain(0.0)
}

/// Single-encoding module descriptor for Core Cell (`0x5e`).
pub static MODULE: Module = Module {
    id: ids::CORE_CELL,
    module_id: 0x5e,
    versions: (0x51, 0x51),
    name: "Core Cell",
    script_type: -1,
    mix: MixMode::InPlace,
    knobs: &[],
    build: |_| Some(ir::EffectKind::Gain(0.0)),
};

#[must_use]
pub fn model(c: CoreCell) -> ir::KnobModel {
    MODULE.knob_model(&c.params)
}

/// Core Cell exposes no KSP engine parameters.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 0] {
    []
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_ten_floats_and_roundtrips() {
        let bytes: Vec<u8> = (0..10u32)
            .flat_map(|i| (i as f32 * 0.1).to_le_bytes())
            .collect();
        let c = saved(0x51, &bytes).unwrap();
        assert!((c.params[5] - 0.5).abs() < 1e-6);
        assert!(saved(0x50, &bytes).is_none());
        let m = model(c);
        assert_eq!(m.build(&m.saved), Some(ir::EffectKind::Gain(0.0)));
    }
}
