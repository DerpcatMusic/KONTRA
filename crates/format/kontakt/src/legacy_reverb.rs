//! Kontakt's legacy Reverb (0x15) onto `ir::Reverb`. Natively it is the
//! classic eight-comb, four-allpass network; here its laws drive the one
//! feedback-delay reverb.
//!
//! Saved fields, version 0x50 (v2 sampler-kontakt effects/mod.rs): predelay
//! ms, size, colour, stereo, damping. Laws from setter case 0x3e of
//! 0x1409619b0, reset 0x14094c760 and audio 0x140aa3d10:
//! - comb feedback `0.3·size + 0.65` (0x144f38764, 0x144f387f0) over combs of
//!   25.31..36.67 ms (doubles 0x144f38aa8..0x144f38af0, times 0.001·rate);
//! - each comb's one-pole keeps `1 − 0.35·(1 − colour)` of the new sample
//!   (0x144f38778);
//! - allpasses at gain 0.5, 5.1..12.6 ms;
//! - the input low-pass takes `x = 0.75·(1 − damping) + 0.25` (0x144714900),
//!   corner `(e^(6.15x) − 1)·0.0025728 + 0.002` of 22,006 Hz (0x14095b2d0:
//!   0x144f38a60, 0x144f38620);
//! - stereo mixes the channels from mono (0) to apart (1).

use kontra_ir as ir;

/// The combs' mean length, ms.
const COMB_MS: f32 = 31.247;
/// The feedback-delay lines' mean length at size 1, ms (kontra-dsp-reverb).
const LINES_MS: f32 = 51.36;
/// The fdn's damping and diffusion knobs at 1 (kontra-dsp-reverb).
const FDN_DAMP: f32 = 0.85;
const FDN_DIFFUSION: f32 = 0.7;

/// The record's reverb, `None` when a field is missing.
pub(crate) fn effect(version: u16, public: &[u8]) -> Option<ir::EffectKind> {
    let (predelay_ms, knobs) = saved(version, public)?;
    Some(ir::EffectKind::Reverb(reverb(predelay_ms, knobs)))
}

/// The saved predelay ms and size, colour, stereo, damping knobs.
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<(f32, [f32; 4])> {
    if version != 0x50 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(4 * n..4 * n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    Some((f(0)?, [f(1)?, f(2)?, f(3)?, f(4)?]))
}

/// The native network's wet level over the fdn's at the same laws: input
/// ×0.015 (0x144f38664) into the eight combs, their sum through the four
/// allpasses, written out unscaled. Simulating that network from the
/// decompile and the fdn on a unit impulse (3 s, 48 kHz) gives tail
/// amplitudes 0.052..0.068 of the fdn's over size 0..1 and colour 0..1;
/// this is their geometric mean, within ±1.2 dB of each.
pub(crate) const WET: f32 = 0.062;

// ponytail: the native lines hold 0x1fff frames, so it drops a predelay past
// 170 ms at 48 kHz to none; kept as set.
pub(crate) fn reverb(predelay_ms: f32, knobs: [f32; 4]) -> ir::Reverb {
    let [size, colour, stereo, damping] = knobs.map(|x| x.clamp(0.0, 1.0));
    let feedback = 0.3 * size + 0.65;
    let x = 0.75 * (1.0 - damping) + 0.25;
    let corner = ((6.15 * x).exp() - 1.0) * 0.002_572_831_7;
    ir::Reverb {
        // 60 dB over the loops a comb runs at that feedback.
        seconds: 3.0 * COMB_MS * 0.001 / -feedback.log10(),
        size: COMB_MS / LINES_MS,
        size_curve: [1.0, 0.0],
        damping: 0.35 * (1.0 - colour) / FDN_DAMP,
        diffusion: 0.5 / FDN_DIFFUSION,
        predelay_ms: predelay_ms.clamp(0.0, 180.0),
        high_cut_hz: (corner.clamp(0.0, 1.0) + 0.002) * 22_006.0,
        low_shelf_db: 0.0,
        stereo,
        modulation_ms: 0.0,
        modulation_hz: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_sets_the_tail_and_damping_the_input_corner() {
        let small = reverb(20.0, [0.0, 1.0, 1.0, 0.5]);
        let large = reverb(20.0, [1.0, 1.0, 1.0, 0.5]);
        // Feedback 0.65 and 0.95 over 31 ms combs.
        assert!((small.seconds - 0.501).abs() < 0.01, "{}", small.seconds);
        assert!((large.seconds - 4.21).abs() < 0.02, "{}", large.seconds);
        assert_eq!(small.damping, 0.0);
        // Damping 0.5: x = 0.625, about 2.6 kHz.
        assert!(
            (small.high_cut_hz - 2_632.0).abs() < 20.0,
            "{}",
            small.high_cut_hz
        );
        // No damping: open.
        assert!(reverb(0.0, [0.0; 4]).high_cut_hz > 20_000.0);
    }
}
