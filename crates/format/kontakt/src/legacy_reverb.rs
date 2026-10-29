//! Kontakt's legacy Reverb (0x15) onto `ir::Reverb` as its own comb
//! network: Freeverb's tunings, per channel eight combs into four allpasses.
//!
//! Saved fields, version 0x50 (v2 sampler-kontakt effects/mod.rs): predelay
//! ms, size, colour, stereo, damping. Laws from setter case 0x3e of
//! 0x1409619b0, reset 0x14094c760 and audio 0x140aa3d10:
//! - combs of 25.31..36.67 ms (doubles 0x144f38aa8..0x144f38af0) and
//!   allpasses of 12.61, 10, 7.73, 5.10 ms (0x144f38a90, 0x1447149c8,
//!   0x144f38a80, 0x144f38a58), each channel's 0.5215 ms longer
//!   (0x144f38980), lengths `ms·0.001·rate` truncated;
//! - comb feedback `0.3·size + 0.65` (0x144f38764, 0x144f387f0) and its
//!   one-pole keeping `1 − 0.35·(1 − colour)` of the new sample
//!   (0x144f38778); allpass gain 0.5 (`v = buf − g·x`, `buf ← g·v + x`);
//! - the channels mix toward their mean by `1 − stereo` (the mix reads
//!   `(stereo + 1)/2`, 0x14470bad0), then ×0.015 (0x144f38664), then a
//!   low-pass: `x = 0.75·(1 − damping) + 0.25` (0x144714900), corner
//!   `(e^(6.15x) − 1)·0.0025728 + 0.002` of 22,006 Hz (0x14095b2d0:
//!   0x144f38a60, 0x144f38620), detuned by 0.99·0.99878 (0x144f389d0,
//!   0x144f389e8); written out unscaled.
//!
//! That low-pass (+0x1100, vtable 0x144e95870) is a two-pole Butterworth:
//! init 0x1408de730 designs a 2-pole, 0-ripple Chebyshev prototype
//! (0x14082a0b0), set 0x14095b2d0 moves it to the corner by the LP→LP
//! transform (0x1408e4970) at resonance 0 and unity DC gain. That is the
//! bilinear Butterworth, −3 dB at the corner.

use kontra_ir as ir;

static COMBS: ir::CombNetwork = ir::CombNetwork {
    combs_ms: [
        25.306_122, 26.938_776, 28.956_916, 30.748_299, 32.244_9, 33.809_525, 35.306_12, 36.666_667,
    ],
    allpasses_ms: [12.607_71, 10.0, 7.732_426, 5.102_041],
    spread_ms: 0.521_541_95,
    input_gain: 0.015,
};

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

// ponytail: the native lines hold 0x1fff frames, so it drops a predelay past
// 170 ms at 48 kHz to none; kept as set.
pub(crate) fn reverb(predelay_ms: f32, knobs: [f32; 4]) -> ir::Reverb {
    let [size, colour, stereo, damping] = knobs.map(|x| x.clamp(0.0, 1.0));
    let feedback = 0.3 * size + 0.65;
    let x = 0.75 * (1.0 - damping) + 0.25;
    let corner = ((6.15 * x).exp() - 1.0) * 0.002_572_831_7;
    ir::Reverb {
        // 60 dB over the loops a mean comb runs at that feedback.
        seconds: 3.0 * COMBS.mean_s() / -feedback.log10(),
        size: 1.0,
        network: Some(ir::Network::Combs(&COMBS)),
        damping: 0.35 * (1.0 - colour),
        diffusion: 0.5,
        predelay_ms: predelay_ms.clamp(0.0, 180.0),
        high_cut_hz: (corner.clamp(0.0, 1.0) + 0.002) * 22_006.0 * 0.988_793,
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
        assert_eq!(reverb(0.0, [0.0; 4]).damping, 0.35);
        // Damping 0.5: x = 0.625, about 2.6 kHz.
        assert!(
            (small.high_cut_hz - 2_603.0).abs() < 20.0,
            "{}",
            small.high_cut_hz
        );
        // No damping: open.
        assert!(reverb(0.0, [0.0; 4]).high_cut_hz > 20_000.0);
    }
}
