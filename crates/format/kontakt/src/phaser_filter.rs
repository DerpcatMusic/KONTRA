//! The Phaser filter (filter type 13) as an [`ir::Phaser`] without a sweep:
//! four first-order allpasses around `0.85·resonance` of feedback, half
//! their output and half the input. Kontakt 8's setter `0x14095d770` and
//! kernel `0x140909810` (docs/re/engine/filt-phaser.md; v2's port matched
//! them under kemu within 6.6e-7).

use kontra_ir as ir;

use crate::script_knobs::SlotKnob;

/// The rate the corner is held at. Kontakt's coefficient ignores the rate,
/// so its corner in Hz follows the host's; v3 keeps the 48 kHz one.
const RATE: f32 = 48_000.0;

/// The allpass corner for cutoff `c`: Kontakt's `(1 − f)/(1 + f)` with
/// `f = 0.0543·(e − 1)·0.33 + 0.02`, `e` the series of `e^(3c)` through its
/// sixth power, as a bilinear corner in Hz.
fn hz(c: f32) -> f32 {
    let v = 3.0 * c.clamp(0.0, 1.0);
    let e = (0..=6)
        .rev()
        .fold(0.0, |sum, n| sum * v / (n as f32 + 1.0) + 1.0);
    let f = (e - 1.0) * 0.054_310_93 * 0.33 + 0.02;
    f * RATE / std::f32::consts::PI
}

pub(crate) fn phaser(cutoff: f32, resonance: f32) -> ir::Phaser {
    ir::Phaser {
        stages: 4,
        triangle: false,
        hz: hz(cutoff),
        spread_hz: 0.0,
        exponent: 0.0,
        feedback: 0.85 * resonance.clamp(0.0, 1.0),
        rate: ir::Rate::Hz(0.0),
        phase: 0.0,
    }
}

/// Cutoff moves the corner, resonance the feedback.
// ponytail: Kontakt slews the cutoff knob by up to 1/(31r + 4) per 32
// frames (filt-phaser.md, live clock); here a knob write retunes at the
// next block.
pub(crate) fn knobs() -> Vec<SlotKnob> {
    let corner = (0..=64u8).map(|n| hz(f32::from(n) / 64.0)).collect();
    vec![
        (
            ir::SlotKnob::Cutoff,
            ir::EffectParam::PhaserHz,
            ir::Law::Table(corner),
        ),
        (
            ir::SlotKnob::Resonance,
            ir::EffectParam::PhaserFeedback,
            ir::Law::Linear { lo: 0.0, hi: 0.85 },
        ),
    ]
}
