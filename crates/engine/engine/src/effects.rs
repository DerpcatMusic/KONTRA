//! Effects folded into each voice's stereo matrix: every carried kind is
//! linear and stateless, so a rack of them is one 2×2 matrix (Kontakt runs
//! group inserts per voice; a linear instrument insert commutes with the sum).

use kontra_ir as ir;
use kontra_voice::Gain;

pub(crate) const IDENTITY: Gain = [[1.0, 0.0], [0.0, 1.0]];

/// `a` after `b`.
pub(crate) fn product(a: Gain, b: Gain) -> Gain {
    let [[a00, a01], [a10, a11]] = a;
    let [[b00, b01], [b10, b11]] = b;
    [
        [a00 * b00 + a01 * b10, a00 * b01 + a01 * b11],
        [a10 * b00 + a11 * b10, a10 * b01 + a11 * b11],
    ]
}

/// A -1..=1 balance.
pub(crate) fn balance(pan: f32) -> Gain {
    [[1.0 - pan.max(0.0), 0.0], [0.0, 1.0 + pan.min(0.0)]]
}

/// A rack, slot 0 first, as one matrix; bypassed slots pass. A dynamics
/// slot does not run in a matrix, so its output gain (its make-up) does not
/// apply either.
pub(crate) fn rack(slots: &[Option<ir::Effect>]) -> Gain {
    slots
        .iter()
        .flatten()
        .filter(|e| !e.bypass && !dynamics(e.kind))
        .fold(IDENTITY, |m, e| product(matrix(e), m))
}

/// A compressor or limiter: it runs in instrument and bus racks only.
pub(crate) fn dynamics(kind: ir::EffectKind) -> bool {
    matches!(
        kind,
        ir::EffectKind::Compressor(_) | ir::EffectKind::Limiter(_)
    )
}

pub(crate) fn matrix(e: &ir::Effect) -> Gain {
    let m = match e.kind {
        ir::EffectKind::Gain(g) => [[g, 0.0], [0.0, g]],
        ir::EffectKind::Invert { phase, swap } => {
            let s = if phase { -1.0 } else { 1.0 };
            if swap {
                [[0.0, s], [s, 0.0]]
            } else {
                [[s, 0.0], [0.0, s]]
            }
        }
        // Kontakt's Stereo Modeller (v2 sampler-core dsp/color/stereo.rs): above 0.5 the
        // side grows, below it the channels blend; then a balance.
        ir::EffectKind::Stereo { width, pan } => {
            let width = if width >= 0.5 {
                let s = 2.0 * width - 1.0;
                [[1.0 + s, -s], [-s, 1.0 + s]]
            } else {
                let a = 0.5 - width;
                [[1.0 - a, a], [a, 1.0 - a]]
            };
            product(balance(pan), width)
        }
        // Its output gain trims processing that does not run here (a
        // compressor's makeup: ANALOG STRINGS stores +9 dB), so it passes.
        ir::EffectKind::Other => return IDENTITY,
        // Its taps run in the instrument rack (racks.rs).
        ir::EffectKind::Sends(_) => IDENTITY,
        // Its bands run on summed audio (filters.rs, racks.rs).
        ir::EffectKind::Eq(_) | ir::EffectKind::Filter(_) | ir::EffectKind::Formant(_) => IDENTITY,
        // Its wet path runs in instrument and bus racks (racks.rs); a
        // compressor's or limiter's output is its wet path.
        ir::EffectKind::Compressor(_) | ir::EffectKind::Limiter(_) => [[0.0; 2]; 2],
        // Its wet path runs in instrument and bus racks (racks.rs).
        ir::EffectKind::OtherWet
        | ir::EffectKind::Convolution { .. }
        | ir::EffectKind::Reverb(_) => [[0.0; 2]; 2],
    };
    let (o, d) = (e.output, e.dry);
    [
        [m[0][0] * o + d, m[0][1] * o],
        [m[1][0] * o, m[1][1] * o + d],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(kind: ir::EffectKind) -> Option<ir::Effect> {
        Some(ir::Effect {
            kind,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })
    }

    #[test]
    fn racks_fold_in_slot_order() {
        let swap = fx(ir::EffectKind::Invert {
            phase: false,
            swap: true,
        });
        let hard_left = fx(ir::EffectKind::Stereo {
            width: 0.5,
            pan: -1.0,
        });
        // Swap then pan left: the right source comes out on the left only.
        assert_eq!(rack(&[swap, hard_left]), [[0.0, 1.0], [0.0, 0.0]]);
        let mono = fx(ir::EffectKind::Stereo {
            width: 0.0,
            pan: 0.0,
        });
        assert_eq!(rack(&[mono, None]), [[0.5, 0.5], [0.5, 0.5]]);
        let mut off = fx(ir::EffectKind::Gain(0.0));
        if let Some(e) = &mut off {
            e.bypass = true;
        }
        assert_eq!(rack(&[off]), IDENTITY);
    }
}
