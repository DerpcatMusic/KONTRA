//! The filters on Kontakt 8's control clock (`0x140ae9f90`) as
//! [`ir::Clocked`]: Phaser (filter type 13; setter `0x14095d770`, kernel
//! `0x140909810`, docs/re/engine/filt-phaser.md) and Vowel A and B (14, 15;
//! setters `0x14095dd00`, `0x14095e250`, kernel `0x140914a20`,
//! filt-legacy.md). Their 0..=1 knobs pass through; the engine runs the
//! laws.

use kontra_ir as ir;

use crate::script_knobs::SlotKnob;

/// Filter `kind`'s clocked shape, or `None` for another kind.
pub(crate) fn clocked(kind: i32, cutoff: f32, resonance: f32) -> Option<ir::Clocked> {
    let shape = match kind {
        13 => ir::ClockedShape::Allpass,
        14 => ir::ClockedShape::VowelLow,
        15 => ir::ClockedShape::VowelHigh,
        _ => return None,
    };
    Some(ir::Clocked {
        shape,
        cutoff,
        resonance,
    })
}

/// Cutoff and resonance move the knobs.
pub(crate) fn knobs() -> Vec<SlotKnob> {
    let unit = ir::Law::Linear { lo: 0.0, hi: 1.0 };
    vec![
        (
            ir::SlotKnob::Cutoff,
            ir::EffectParam::ClockedCutoff,
            unit.clone(),
        ),
        (
            ir::SlotKnob::Resonance,
            ir::EffectParam::ClockedResonance,
            unit,
        ),
    ]
}
