//! Tape Saturator (module 0x42) onto the engine's waveshaper: its saved
//! record (gain, warmth, roll-off, quality at 0, 4, 8, 12; bindings table
//! `fx_dynamics_dist_tape.tsv`) and the laws measured through Kontakt 8.13
//! (oracle sweep 2026-11-12, every knob pinned per step: a 200 Hz sine ramp
//! for the curve, white noise at −40 dBFS for the filters).
//!
//! - Curve: `x / √(1 + (d·x)²)` with `d = 0.152 · 10^(48·gain/20)`, the
//!   knob's displayed 0..48 dB of drive with the makeup that keeps small
//!   signals at unity, then +0.78 dB. Fit 0.17 dB RMS, 0.99 dB worst, over
//!   545 half-cycles of gain 0, ¼, ½, ¾, 1; third harmonic within 2.5 dB.
//! - Warmth: a low shelf at 1480 Hz, slope ½, of the displayed `6·w − 3` dB
//!   (fit 0.11 dB RMS, 60 Hz–10 kHz).
//! - Roll-off: a two-pole low-pass, `q` ½, at `11.6 + 12.1·r` kHz (the
//!   display reads 12..20 kHz; fit 0.2–1.05 dB RMS, 3–19.5 kHz).
//! - A 30 Hz Butterworth high-pass (0.17 dB RMS, 15–300 Hz).
//!
//! Not copied: the native curve table (DAT_144fc5510) and its
//! envelope-driven coefficient update; Quality's oversampling (its alias
//! measured at −94 dB, so the switch is read and changes nothing); the
//! 28–31 ms control ramps (a knob move retunes at the next block).

use kontra_ir as ir;

/// The record's knobs, each 0..=1, as the engine's waveshaper.
pub(crate) fn tape(gain: f32, warmth: f32, rolloff: f32) -> ir::EffectKind {
    let [gain, warmth, rolloff] = [gain, warmth, rolloff].map(|x| x.clamp(0.0, 1.0));
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Saturate(0.152 * 10f32.powf(48.0 * gain / 20.0)),
        lowpass_hz: Some(11_600.0 + 12_100.0 * rolloff),
        lowpass_q: Some(0.5),
        highpass_hz: Some(30.0),
        shelf: Some(ir::Bell {
            shape: ir::Shape::LowShelf,
            hz: 1_480.0,
            q: 0.5,
            gain_db: 6.0 * warmth - 3.0,
        }),
        gain: 10f32.powf(0.78 / 20.0),
    })
}

/// The saved knobs (gain, warmth, roll-off, quality) as a script moves
/// them: every move rebuilds the shaper through [`tape`].
pub(crate) fn model(public: &[u8]) -> Option<ir::KnobModel> {
    use ir::SlotKnob::Knob;
    let f = |n: usize| Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    let quality = f32::from(*public.get(12)?);
    Some(ir::KnobModel {
        knobs: [Knob(0), Knob(1), Knob(2), Knob(3)].into(),
        saved: [f(0)?, f(4)?, f(8)?, quality].into(),
        build: |p| {
            let [gain, warmth, rolloff, _quality] = *p else {
                return None;
            };
            Some(tape(gain, warmth, rolloff))
        },
    })
}

/// The KSP names of [`model`]'s knobs; each is the record's field as set
/// (bindings table; Quality stores 1 at any value ≥ 1).
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 4] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        ("$ENGINE_PAR_TP_GAIN", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_TP_WARMTH", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_TP_HF_ROLLOFF", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_TP_QUALITY", Slot(Knob(3)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured curve: at gain ½ a 200 Hz tone at −2.4 dBFS comes out
    /// 7.2 dB under its small-signal level (Kontakt: −5.9 vs +1.3 dB).
    #[test]
    fn half_gain_squashes_a_loud_tone_as_measured() {
        let ir::EffectKind::Shaper(s) = tape(0.5, 0.5, 1.0) else {
            panic!("a shaper");
        };
        let ir::DriveCurve::Saturate(d) = s.curve else {
            panic!("saturate");
        };
        let (quiet, loud) = (0.063f32, 0.76f32);
        let g = |x: f32| x / (1.0 + d * d * x * x).sqrt() / x;
        let squash = 20.0 * (g(loud) / g(quiet)).log10();
        assert!((squash + 7.2).abs() < 1.0, "{squash}");
    }
}
