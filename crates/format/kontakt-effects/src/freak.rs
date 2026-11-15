//! Freak (module 0x68) onto the engine's ring modulator (`ir::RingMod`),
//! in all three modes. The laws are the native core's setters (ctor
//! 0x141963480, setters 0x141964260.., process 0x141966ad0, read through
//! v2 sampler-core dsp/color/freak.rs, which matched vectors of the
//! emulated core); Oscillator mode is confirmed by Kontakt 8.13.1 renders
//! (fx-measured/freak-bite.md): FREQUENCY is `±range·(2x − 1)³` (wide
//! range 5000 Hz: 40, 625 and 5000 Hz at 0.6, 0.75 and 1, a fresh unit's
//! 0.513572 is 0.1 Hz), TYPE 0 a full-depth tremolo, 0.5 a ring, 1 a
//! single shifted tone. Pitches below are the core's exponential table
//! (`PITCH_INDEX`) at the setters' offsets.

use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

/// The controls, as a script sets them: each 0..=1 but the switches
/// (`wide_range`, `gate`, `demod`, `bp_filter`: 0 or 1) and `mode`.
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
    pub feedback: f32,
    pub tuning: f32,
    pub carrier: f32,
    pub width: f32,
    pub contour: f32,
    pub bp_freq: f32,
    pub release: f32,
    pub demod: f32,
    pub bp_filter: f32,
    /// Radio 0, Oscillator 1, Sidechain 2.
    pub mode: f32,
}

/// A fresh unit as a script loads it: the Oscillator controls from a
/// Kontakt 8.13.1 read-back, the rest the core's init values.
pub const FRESH: Knobs = Knobs {
    kind: 0.5,
    frequency: 0.513_572,
    harmonics: 0.0,
    antifold: 0.0,
    stereo: 0.5,
    mix: 1.0,
    wide_range: 1.0,
    gate: 1.0,
    feedback: 0.0,
    tuning: 0.5,
    carrier: 1.0,
    width: 0.5,
    contour: 0.5,
    bp_freq: 0.5,
    release: 0.5,
    demod: 0.0,
    bp_filter: 0.0,
    mode: OSCILLATOR as f32,
};

/// The Oscillator mode's index (Radio 0, Oscillator 1, Sidechain 2).
pub const OSCILLATOR: i32 = 1;

/// The ring modulator at `k`.
#[must_use]
pub fn freak(k: Knobs) -> ir::EffectKind {
    let unit = |x: f32| x.clamp(0.0, 1.0);
    let on = |x: f32| x >= 0.5;
    let pitch = |index: f32| PITCH_INDEX.eval_unclamped(index);
    // TYPE: below the middle a bias (AM at 0), above it the quadrature
    // branch (a shift at 1), each the square of the distance.
    let kind = 2.0 * unit(k.kind) - 1.0;
    let amount = kind * kind;
    let (bias, quadrature) = if kind < 0.0 {
        (amount, 0.0)
    } else {
        (0.0, amount)
    };
    let drive = 13.5 * unit(k.harmonics) + 0.5;
    let feedback = 0.9 * unit(k.feedback) * unit(k.feedback);
    let (mut hz, mut highpass_hz) = (0.0, 0.0);
    let source = match k.mode.round() as i32 {
        0 => {
            // The carrier and the receiver's centre meet at TUNING 0.5.
            let c = unit(k.carrier);
            hz = pitch(200.0 * c + 1_881.881_6);
            let (a, b) = (40.0 * c + 100.0, 24.0 - 12.0 * c);
            let centre = (a - b) + 2.0 * b * unit(k.tuning);
            // WIDTH: the band-pass is `0.1 + 1.9x` octaves wide.
            let octaves = 1.9 * unit(k.width) + 0.1;
            let o = 2f32.powf(0.5 * octaves);
            ir::RingSource::Radio {
                gain: 3.0 * unit(k.harmonics) + 1.0,
                tuned_hz: pitch(5.0 * centre + 1_381.881_6),
                damping: (o * o - 1.0) / o,
                makeup: if octaves <= 1.0 {
                    1.0 / (octaves * 0.555_555_6 + 0.444_444_45)
                } else {
                    1.0
                },
                product: on(k.demod),
                noise: 0.299 * feedback * feedback + 0.001,
            }
        }
        2 => {
            let c = unit(k.release).powi(3);
            ir::RingSource::Sidechain {
                contour: unit(k.contour),
                attack: 0.195 * c + 0.005,
                release: 9.99 * c + 0.01,
                bandpass_hz: on(k.bp_filter).then(|| pitch(455.0 * unit(k.bp_freq) + 1_551.881_6)),
            }
        }
        _ => {
            let range = if on(k.wide_range) { 5_000.0 } else { 200.0 };
            let t = 2.0 * unit(k.frequency) - 1.0;
            hz = t * t * t * range;
            // ANTIFOLD lifts the input's high-pass from 8 Hz toward the carrier.
            let antifold = ((hz.abs() + 200.0).ln() * 17.312_34 - 36.376_3) * unit(k.antifold);
            highpass_hz = pitch(antifold * 5.0 + 1_381.881_6);
            ir::RingSource::Oscillator
        }
    };
    let mix = unit(k.mix);
    ir::EffectKind::RingMod(ir::RingMod {
        hz,
        spread: (2.0 * unit(k.stereo) - 1.0) * 0.25,
        bias,
        quadrature,
        drive,
        highpass_hz,
        gate: on(k.gate),
        feedback,
        source,
        wet: mix.sqrt(),
        dry: (1.0 - mix).sqrt(),
    })
}

/// A saved record (reader 0x140ac5790, schema `Fx13Bx5I`): TYPE,
/// HARMONICS, FEEDBACK, MIX, TUNING, CARRIER, WIDTH, FREQUENCY, ANTIFOLD,
/// STEREO, CONTOUR, BP_FREQ and RELEASE as floats, then DEMOD, GATE,
/// WIDE_RANGE, BP_FILTER and one more as bytes, then the mode as an i32.
#[must_use]
pub fn saved(public: &[u8]) -> Option<Knobs> {
    let f = |n: usize| {
        Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?)).filter(|v| v.is_finite())
    };
    let byte = |n: usize| Some(if *public.get(n)? != 0 { 1.0 } else { 0.0 });
    let mode = i32::from_le_bytes(public.get(57..61)?.try_into().ok()?);
    Some(Knobs {
        kind: f(0)?,
        harmonics: f(4)?,
        feedback: f(8)?,
        mix: f(12)?,
        tuning: f(16)?,
        carrier: f(20)?,
        width: f(24)?,
        frequency: f(28)?,
        antifold: f(32)?,
        stereo: f(36)?,
        contour: f(40)?,
        bp_freq: f(44)?,
        release: f(48)?,
        demod: byte(52)?,
        gate: byte(53)?,
        wide_range: byte(54)?,
        bp_filter: byte(55)?,
        mode: mode.clamp(0, 2) as f32,
    })
}

impl Knobs {
    fn to_array(self) -> [f32; 18] {
        [
            self.kind,
            self.frequency,
            self.harmonics,
            self.antifold,
            self.stereo,
            self.mix,
            self.wide_range,
            self.gate,
            self.feedback,
            self.tuning,
            self.carrier,
            self.width,
            self.contour,
            self.bp_freq,
            self.release,
            self.demod,
            self.bp_filter,
            self.mode,
        ]
    }

    fn from_array(
        [
            kind,
            frequency,
            harmonics,
            antifold,
            stereo,
            mix,
            wide_range,
            gate,
            feedback,
            tuning,
            carrier,
            width,
            contour,
            bp_freq,
            release,
            demod,
            bp_filter,
            mode,
        ]: [f32; 18],
    ) -> Self {
        Self {
            kind,
            frequency,
            harmonics,
            antifold,
            stereo,
            mix,
            wide_range,
            gate,
            feedback,
            tuning,
            carrier,
            width,
            contour,
            bp_freq,
            release,
            demod,
            bp_filter,
            mode,
        }
    }
}

/// A unit as a script moves it, from `k`, in [`rows`] order.
#[must_use]
pub fn model(k: Knobs) -> ir::KnobModel {
    use ir::SlotKnob::Knob;
    ir::KnobModel {
        knobs: (0..18).map(Knob).collect(),
        saved: k.to_array().into(),
        build: |p| Some(freak(Knobs::from_array(p.get(..18)?.try_into().ok()?))),
    }
}

/// The KSP names of [`model`]'s knobs: the floats as set, the switches
/// and MODE by their index.
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 18] {
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
        ("$ENGINE_PAR_FREAK_FEEDBACK", Slot(Knob(8)), unit()),
        ("$ENGINE_PAR_FREAK_TUNING", Slot(Knob(9)), unit()),
        ("$ENGINE_PAR_FREAK_CARRIER", Slot(Knob(10)), unit()),
        ("$ENGINE_PAR_FREAK_WIDTH", Slot(Knob(11)), unit()),
        ("$ENGINE_PAR_FREAK_CONTOUR", Slot(Knob(12)), unit()),
        ("$ENGINE_PAR_FREAK_BP_FREQ", Slot(Knob(13)), unit()),
        ("$ENGINE_PAR_FREAK_RELEASE", Slot(Knob(14)), unit()),
        ("$ENGINE_PAR_FREAK_DEMOD", Slot(Knob(15)), index()),
        ("$ENGINE_PAR_FREAK_BP_FILTER", Slot(Knob(16)), index()),
        ("$ENGINE_PAR_FREAK_MODE", Slot(Knob(17)), index()),
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
    fn every_freak_position_fits_the_live_knobs() {
        // A model longer than the live array drops every script write.
        assert!(model(FRESH).saved.len() <= ir::KnobModel::MAX);
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
