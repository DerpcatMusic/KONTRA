//! Freak (module 0x68) in its Oscillator mode onto the engine's ring
//! modulator (`ir::RingMod`). The laws are the native core's setters
//! (ctor 0x141963480, setters 0x141964260.., read through v2 sampler-core
//! dsp/color/freak.rs, which matched vectors of the emulated core),
//! confirmed by Kontakt 8.13.1 renders (fx-measured/freak-bite.md):
//! FREQUENCY is `±range·(2x − 1)³` (wide range 5000 Hz: 40, 625 and 5000
//! Hz at 0.6, 0.75 and 1, a fresh unit's 0.513572 is 0.1 Hz), TYPE 0 a
//! full-depth tremolo, 0.5 a ring, 1 a single shifted tone.
//! Not carried: the Radio and Sidechain modes and FEEDBACK (the implicit
//! feedback loop); the translator reports them.

use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

/// The controls carried, as a script sets them: each 0..=1 but
/// `wide_range` and `gate` (0 or 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    pub kind: f32,
    pub frequency: f32,
    pub harmonics: f32,
    pub antifold: f32,
    pub stereo: f32,
    pub mix: f32,
    pub wide_range: f32,
    pub gate: f32,
}

/// A fresh unit as a script loads it (Kontakt 8.13.1 read-back).
pub const FRESH: Knobs = Knobs {
    kind: 0.5,
    frequency: 0.513_572,
    harmonics: 0.0,
    antifold: 0.0,
    stereo: 0.5,
    mix: 1.0,
    wide_range: 1.0,
    gate: 1.0,
};

/// The Oscillator mode's index (Radio 0, Oscillator 1, Sidechain 2).
pub const OSCILLATOR: i32 = 1;

/// The ring modulator at `k`.
#[must_use]
pub fn freak(k: Knobs) -> ir::EffectKind {
    let unit = |x: f32| x.clamp(0.0, 1.0);
    let range = if k.wide_range >= 0.5 { 5_000.0 } else { 200.0 };
    let t = 2.0 * unit(k.frequency) - 1.0;
    let hz = t * t * t * range;
    // TYPE: below the middle a bias (AM at 0), above it the quadrature
    // branch (a shift at 1), each the square of the distance.
    let kind = 2.0 * unit(k.kind) - 1.0;
    let amount = kind * kind;
    let (bias, quadrature) = if kind < 0.0 {
        (amount, 0.0)
    } else {
        (0.0, amount)
    };
    // ANTIFOLD lifts the input's high-pass from 8 Hz toward the carrier.
    let antifold = ((hz.abs() + 200.0).ln() * 17.312_34 - 36.376_3) * unit(k.antifold);
    let mix = unit(k.mix);
    ir::EffectKind::RingMod(ir::RingMod {
        hz,
        spread: (2.0 * unit(k.stereo) - 1.0) * 0.25,
        bias,
        quadrature,
        drive: 13.5 * unit(k.harmonics) + 0.5,
        highpass_hz: PITCH_INDEX.eval_unclamped(antifold * 5.0 + 1_381.881_6),
        gate: k.gate >= 0.5,
        wet: mix.sqrt(),
        dry: (1.0 - mix).sqrt(),
    })
}

/// A saved record (apply 0x140aba860, field order from v2's reader):
/// TYPE, HARMONICS, FEEDBACK, MIX, TUNING, CARRIER, WIDTH, FREQUENCY,
/// ANTIFOLD, STEREO, CONTOUR, BP_FREQ and RELEASE as floats, then DEMOD,
/// GATE, WIDE_RANGE and BP_FILTER and one more as bytes, then the mode as
/// an i32. The knobs carried, FEEDBACK and the mode.
#[must_use]
pub fn saved(public: &[u8]) -> Option<(Knobs, f32, i32)> {
    let f = |n: usize| {
        Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?)).filter(|v| v.is_finite())
    };
    let byte = |n: usize| Some(if *public.get(n)? != 0 { 1.0 } else { 0.0 });
    let mode = i32::from_le_bytes(public.get(57..61)?.try_into().ok()?);
    let knobs = Knobs {
        kind: f(0)?,
        harmonics: f(4)?,
        mix: f(12)?,
        frequency: f(28)?,
        antifold: f(32)?,
        stereo: f(36)?,
        gate: byte(53)?,
        wide_range: byte(54)?,
    };
    Some((knobs, f(8)?, mode))
}

/// A unit as a script moves it, from `k`, in [`rows`] order.
#[must_use]
pub fn model(k: Knobs) -> ir::KnobModel {
    use ir::SlotKnob::Knob;
    let saved = [
        k.kind,
        k.frequency,
        k.harmonics,
        k.antifold,
        k.stereo,
        k.mix,
        k.wide_range,
        k.gate,
    ];
    ir::KnobModel {
        knobs: (0..8).map(Knob).collect(),
        saved: saved.into(),
        build: |p| {
            let [
                kind,
                frequency,
                harmonics,
                antifold,
                stereo,
                mix,
                wide_range,
                gate,
            ] = *p
            else {
                return None;
            };
            Some(freak(Knobs {
                kind,
                frequency,
                harmonics,
                antifold,
                stereo,
                mix,
                wide_range,
                gate,
            }))
        },
    }
}

/// The KSP names of [`model`]'s knobs: the floats as set, WIDE_RANGE and
/// GATE by their index.
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 8] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let index = || ir::Law::Linear {
        lo: 0.0,
        hi: 1_000_000.0,
    };
    [
        ("$ENGINE_PAR_FREAK_TYPE", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_FREAK_FREQUENCY", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_FREAK_HARMONICS", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_FREAK_ANTIFOLD", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_FREAK_STEREO", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_FREAK_MIX", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_FREAK_WIDE_RANGE", Slot(Knob(6)), index()),
        ("$ENGINE_PAR_FREAK_GATE", Slot(Knob(7)), index()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(k: Knobs) -> ir::RingMod {
        let ir::EffectKind::RingMod(r) = freak(k) else {
            panic!()
        };
        r
    }

    #[test]
    fn frequency_and_type_follow_the_measured_laws() {
        let fresh = ring(FRESH);
        assert!((fresh.hz - 0.1).abs() < 1e-3, "{}", fresh.hz);
        assert_eq!(
            (fresh.bias, fresh.quadrature, fresh.spread),
            (0.0, 0.0, 0.0)
        );
        assert!(
            (fresh.highpass_hz - 8.18).abs() < 0.05,
            "{}",
            fresh.highpass_hz
        );
        for (x, hz) in [(0.6, 40.0), (0.75, 625.0), (1.0, 5_000.0), (0.0, -5_000.0)] {
            let r = ring(Knobs {
                frequency: x,
                ..FRESH
            });
            assert!((r.hz / hz - 1.0).abs() < 1e-3, "{x}: {}", r.hz);
        }
        let am = ring(Knobs { kind: 0.0, ..FRESH });
        let shift = ring(Knobs { kind: 1.0, ..FRESH });
        assert_eq!((am.bias, shift.quadrature), (1.0, 1.0));
    }
}
