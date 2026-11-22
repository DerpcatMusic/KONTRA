//! Kontakt's legacy Reverb (0x15) onto `ir::Reverb` as its own comb
//! network: Freeverb's tunings, per channel eight combs into four allpasses.
//!
//! Saved fields, version 0x50 (`fx_legacy_delay_reverb_mod.tsv:64, 68` and
//! `held_fields.tsv:3, 6`): predelay ms (0), size (4), stereo (8), colour (12),
//! damping (16). Laws from setter case 0x3e of
//! [ref], reset [ref] and audio [ref]:
//! - combs of 25.31..36.67 ms (doubles [ref]..[ref]) and
//!   allpasses of 12.61, 10, 7.73, 5.10 ms ([ref],
//!   [ref]), each channel's 0.5215 ms longer
//!, lengths `ms·0.001·rate` truncated;
//! - comb feedback `0.3·size + 0.65` and its
//!   one-pole keeping `1 − 0.35·(1 − colour)` of the new sample
//!; allpass gain 0.5 (`v = buf − g·x`, `buf ← g·v + x`);
//! - the channels mix toward their mean by `1 − stereo` (the mix reads
//!   `(stereo + 1)/2`, [ref]), then ×0.015, then a
//!   low-pass: `x = 0.75·(1 − damping) + 0.25`, corner
//!   `(e^(6.15x) − 1)·0.0025728 + 0.002` of 22,006 Hz ([ref]:
//!   [ref]), detuned by 0.99·0.99878 ([ref],
//!   [ref]); written out unscaled.
//!
//! That low-pass (+0x1100, vtable [ref]) is a two-pole Butterworth:
//! init [ref] designs a 2-pole, 0-ripple Chebyshev prototype
//!, set [ref] moves it to the corner by the LP→LP
//! transform at resonance 0 and unity DC gain. That is the
//! bilinear Butterworth, −3 dB at the corner.

use kontra_ir as ir;
use kontra_law::kontakt::LEGACY_REVERB_HIGH_CUT_HZ;

static COMBS: ir::CombNetwork = ir::CombNetwork {
    combs_ms: [
        25.306_122, 26.938_776, 28.956_916, 30.748_299, 32.244_9, 33.809_525, 35.306_12, 36.666_667,
    ],
    allpasses_ms: [12.607_71, 10.0, 7.732_426, 5.102_041],
    spread_ms: 0.521_541_95,
    input_gain: 0.015,
};

/// The record's reverb, `None` when a field is missing.
pub fn effect(version: u16, public: &[u8]) -> Option<ir::EffectKind> {
    let (predelay_ms, knobs) = saved(version, public)?;
    Some(ir::EffectKind::Reverb(reverb(predelay_ms, knobs)))
}

/// The saved predelay ms and `[size, colour, stereo, damping]` knobs.
/// Wire order in `BParFXReverb` v0x50 is `[predelay, size, stereo, colour, damping]`
/// (`fx_legacy_delay_reverb_mod.tsv:64, 68`), so field 2 (`stereo`) and field 3
/// (`colour`) are swapped into [`reverb`]'s `[size, colour, stereo, damping]` order.
pub fn saved(version: u16, public: &[u8]) -> Option<(f32, [f32; 4])> {
    if version != 0x50 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(4 * n..4 * n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    Some((f(0)?, [f(1)?, f(3)?, f(2)?, f(4)?]))
}

// ponytail: the native lines hold 0x1fff frames, so it drops a predelay past
// 170 ms at 48 kHz to none; kept as set.
pub fn reverb(predelay_ms: f32, knobs: [f32; 4]) -> ir::Reverb {
    let [size, colour, stereo, damping] = knobs.map(|x| x.clamp(0.0, 1.0));
    let feedback = 0.3 * size + 0.65;
    let x = 0.75 * (1.0 - damping) + 0.25;
    ir::Reverb {
        // 60 dB over the loops a mean comb runs at that feedback.
        seconds: 3.0 * COMBS.mean_s() / -feedback.log10(),
        size: 1.0,
        network: Some(ir::Network::Combs(ir::Shared::Borrowed(&COMBS))),
        damping: 0.35 * (1.0 - colour),
        diffusion: 0.5,
        predelay_ms: predelay_ms.clamp(0.0, 180.0),
        high_cut_hz: LEGACY_REVERB_HIGH_CUT_HZ.eval(x),
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

    #[test]
    fn saved_wire_order_reads_stereo_at_offset_8_and_colour_at_offset_12() {
        // Wire order: predelay(0), size(4), stereo(8), colour(12), damping(16).
        let public: Vec<u8> = [15.0f32, 0.5, 0.25, 0.8, 0.4]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let (predelay, knobs) = saved(0x50, &public).unwrap();
        assert_eq!(predelay, 15.0);
        assert_eq!(knobs, [0.5, 0.8, 0.25, 0.4]);
        let Some(ir::EffectKind::Reverb(r)) = effect(0x50, &public) else {
            panic!("a reverb");
        };
        assert_eq!(r.stereo, 0.25);
        assert!((r.damping - 0.35 * (1.0 - 0.8)).abs() < 1e-6);
    }
}
