//! Kontakt's legacy Reverb (0x15) onto `ir::Reverb` as its own comb
//! network: Freeverb's tunings, per channel eight combs into four allpasses.
//!
//! Saved fields, version 0x50 (`fx_legacy_delay_reverb_mod.tsv:64, 68` and
//! `held_fields.tsv:3, 6`): predelay ms (0), size (4), stereo (8), colour (12),
//! damping (16). Laws from setter case 0x3e of
//! [ref], reset [ref] and audio [ref]:
//! - combs of 25.31..36.67 ms (doubles [ref]..[ref]) and
//!   allpasses of 12.61, 10, 7.73, 5.10 ms ([ref],
//!   [ref]), each channel's 0.5215 ms longer, lengths `ms·0.001·rate` truncated;
//! - comb feedback `0.3·size + 0.65` and its
//!   one-pole keeping `1 − 0.35·(1 − colour)` of the new sample; the allpasses run as plain delays: the reference 8.13
//!   renders each comb's first echo 35.44 ms (their summed lengths) later
//!   with no recirculation between (docs/re/engine/fx-measured/legacy-reverb.md);
//! - the channels mix toward their mean by `1 − stereo` (the mix reads
//!   `(stereo + 1)/2`, [ref]), then ×0.015, then a
//!   low-pass: `x = 0.75·(1 − damping) + 0.25`, corner
//!   `(e^(6.15x) − 1)·0.0025728 + 0.002` of 22,006 Hz ([ref]:
//!   [ref]), detuned by 0.99·0.99878 ([ref],
//!   [ref]); the wet written out ×3 (measured: damping 1 nulls at
//!   −88.6 dB with a gain of 3.0000).
//!
//! That low-pass (+0x1100, vtable [ref]) is a two-pole Butterworth:
//! init [ref] designs a 2-pole, 0-ripple Chebyshev prototype, set [ref] moves it to the corner by the LP→LP
//! transform at resonance 0 and unity DC gain. That is the
//! bilinear Butterworth, −3 dB at the corner.

use kontra_ir as ir;

/// Legacy cutoff law in Hz: 8-term Taylor series of `exp(6.15x) - 1`,
/// clamped to 1 after scaling by `0.0025728317`, plus `0.002`, over `44011.976 * 0.495` Hz.
fn legacy_cutoff_hz(x: f32) -> f32 {
    let z = 6.15_f64 * f64::from(x.clamp(0.0, 1.0));
    let mut sum = 0.0_f64;
    let mut term = 1.0_f64;
    for k in 1..=8 {
        term *= z / f64::from(k);
        sum += term;
    }
    let v = (sum * 0.002_572_831_7).min(1.0) + 0.002;
    (44_011.976 * 0.495 * v) as f32
}

static COMBS: ir::CombNetwork = ir::CombNetwork {
    combs_ms: [
        25.306_122, 26.938_776, 28.956_916, 30.748_299, 32.244_9, 33.809_525, 35.306_12, 36.666_667,
    ],
    allpasses_ms: [12.607_71, 10.0, 7.732_426, 5.102_041],
    spread_ms: 0.521_541_95,
    // 0.015 in, ×3 out.
    input_gain: 0.045,
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
    let [predelay, size, stereo, colour, damping] =
        crate::module::Cursor::new(public).f32_array()?;
    Some((predelay, [size, colour, stereo, damping]))
}

/// Builds the legacy Reverb IR in physical units. `predelay_ms` is preserved up to
/// 180 ms for script readback; `Combs` wraps predelays exceeding its 0x1fff-frame
/// ring (~170.65 ms at 48 kHz) to 0 frames, matching the reference 8.13 behavior.
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
        // The allpasses' gain: they only delay.
        diffusion: 0.0,
        predelay_ms: predelay_ms.clamp(0.0, 180.0),
        high_cut_hz: legacy_cutoff_hz(x),
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
        // Damping 0.5: x = 0.625, about 2.56 kHz (8-term Taylor cutoff law).
        assert!(
            (small.high_cut_hz - 2_562.0).abs() < 10.0,
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
