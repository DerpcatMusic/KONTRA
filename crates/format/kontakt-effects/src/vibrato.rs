//! Vibrato/Chorus (module 0x69, saved v0x10) onto the engine's swept delay
//! ([`ir::Delay`]). The saved record: DEPTH and COLOR as i32 at 0 and 4,
//! BLEND, RATE, WIDTH and MIX as floats at 8..24, then a byte (1 in every
//! factory preset).
//!
//! Measured wet on Kontakt 8.13.1 through the oracle (a steady 1 kHz tone
//! and a sweep, 2026-11-14; docs/re/engine/fx-measured/psyche-vibrato.md):
//! - the vibrato is a delay swept by a near sine (third harmonic 4% of the
//!   fundamental at DEPTH 2): [`SWING_MS`] peak to peak by DEPTH, at
//!   `0.814·28.7^RATE` Hz (0.83, 2.17, 6.0, 17.0 Hz at 0, .3, .6, .9);
//! - its line rolls off as [`BODY`] (−1.3 dB at 1 kHz, −7 dB above 6 kHz,
//!   COLOR 0), with a level per DEPTH ([`GAIN`]);
//! - BLEND 1 mixes the dry signal in at the vibrato's level (the 12 Hz beat
//!   of a chorus), MIX blends the input against all of it.
//!
//! Not carried: COLOR 1 and 2 (their own resonant responses: +5 dB at 635 Hz,
//! or +4.5 dB at 250 Hz and 1 kHz, against COLOR 0; they play COLOR 0), the
//! line's 4 dB ripple as the scanner sweeps, the delay's centre (unmeasured;
//! 1 ms), BLEND's law between its ends (taken linear) and WIDTH's (taken as
//! the right sweep a quarter cycle ahead at 1; measured L/R correlation
//! 0.57 there).

use kontra_ir as ir;

/// The saved fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vibrato {
    pub depth: i32,
    pub color: i32,
    pub blend: f32,
    pub rate: f32,
    pub width: f32,
    pub mix: f32,
}

/// A fresh Vibrato/Chorus (its INIT preset; Kontakt 8.13.1 reads the same
/// back from a script-loaded one).
pub const INIT: Vibrato = Vibrato {
    depth: 0,
    color: 0,
    blend: 0.0,
    rate: 0.6,
    width: 0.0,
    mix: 1.0,
};

/// The sweep, ms peak to peak, by DEPTH I..VI.
const SWING_MS: [f32; 6] = [0.63, 0.56, 0.82, 1.10, 1.35, 1.78];
/// The vibrato's gain by DEPTH, with [`BODY`]'s −1.25 dB: its 1 kHz tone
/// measured −0.72, −1.24, −0.95, −2.49, −3.09 and −2.96 dB re the input.
const GAIN: [f32; 6] = [0.930, 0.876, 0.906, 0.759, 0.708, 0.719];
/// The delay's centre, ms (unmeasured; past the largest swing's half).
const CENTRE_MS: f32 = 1.0;
/// COLOR 0's line: a second-order high shelf to −6.4 dB about 3.8 kHz,
/// fitted to the sweep (0.74 dB rms, 1.65 dB at most, 30 Hz..16 kHz).
static BODY: [ir::Section; 1] = [ir::Section {
    hz: 3819.8,
    num: [1.0, 1.0900, 0.4775],
    den: [1.0, 1.5773, 1.0],
}];

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Vibrato> {
    if version != 0x10 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let i = |n: usize| Some(i32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    Some(Vibrato {
        depth: i(0)?,
        color: i(4)?,
        blend: f(8)?,
        rate: f(12)?,
        width: f(16)?,
        mix: f(20)?,
    })
}

#[must_use]
pub fn delay(v: Vibrato) -> ir::Delay {
    let d = usize::try_from(v.depth.clamp(0, 5)).unwrap_or(0);
    let swing = SWING_MS.get(d).copied().unwrap_or(0.63);
    let gain = GAIN.get(d).copied().unwrap_or(0.93);
    let (blend, mix) = (v.blend.clamp(0.0, 1.0), v.mix.clamp(0.0, 1.0));
    let wet = mix * (1.0 - 0.5 * blend) * gain;
    ir::Delay {
        time: ir::DelayTime::Ms(CENTRE_MS),
        depth_ms: 0.5 * swing,
        second_head: false,
        input: ir::IDENTITY,
        feedback: [[0.0; 2]; 2],
        output: [[wet, 0.0], [0.0, wet]],
        damping_hz: f32::INFINITY,
        low_cut_hz: 0.0,
        steep_low_cut: false,
        body: &BODY,
        triangle_s: None,
        shape: &[],
        noise: [0.0; 2],
        high_shelf: None,
        spread_ms: 0.0,
        rate: ir::Rate::Hz(0.814 * 28.7f32.powf(v.rate.clamp(0.0, 1.0))),
        phase: 0.25 * v.width.clamp(0.0, 1.0),
        direct: mix * 0.5 * blend * gain + (1.0 - mix),
    }
}

/// The record as a script moves it: every knob move rebuilds the delay
/// through [`delay`].
#[must_use]
pub fn model(v: Vibrato) -> ir::KnobModel {
    use ir::SlotKnob::Knob;
    ir::KnobModel {
        knobs: (0..6).map(Knob).collect(),
        saved: [
            v.depth as f32,
            v.color as f32,
            v.blend,
            v.rate,
            v.width,
            v.mix,
        ]
        .into(),
        build: |p| {
            let [depth, color, blend, rate, width, mix] = *p else {
                return None;
            };
            Some(ir::EffectKind::Delay(delay(Vibrato {
                depth: depth.round() as i32,
                color: color.round() as i32,
                blend,
                rate,
                width,
                mix,
            })))
        },
    }
}

/// The KSP names of [`model`]'s knobs: DEPTH and COLOR are indices (the
/// raw value), the rest saved as set.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let index = || ir::Law::Linear {
        lo: 0.0,
        hi: 1_000_000.0,
    };
    [
        ("$ENGINE_PAR_VC_DEPTH", Slot(Knob(0)), index()),
        ("$ENGINE_PAR_VC_COLOR", Slot(Knob(1)), index()),
        ("$ENGINE_PAR_VC_BLEND", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_VC_RATE", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_VC_WIDTH", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_VC_MIX", Slot(Knob(5)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The INIT preset's record (`- INIT Vibrato-Chorus -.nkp`).
    #[test]
    fn the_init_record_reads_as_the_fresh_unit() {
        let mut public = [0u8, 0, 0, 0, 0, 0, 0, 0].to_vec();
        for v in [0.0f32, 0.6, 0.0, 1.0] {
            public.extend(v.to_le_bytes());
        }
        public.push(1);
        assert_eq!(saved(0x10, &public), Some(INIT));
        let d = delay(INIT);
        let ir::Rate::Hz(hz) = d.rate else {
            panic!("Hz");
        };
        assert!((hz - 6.0).abs() < 0.15, "{hz}");
        assert_eq!(d.direct, 0.0);
        // MIX 0 passes the input alone.
        let dry = delay(Vibrato { mix: 0.0, ..INIT });
        assert_eq!((dry.output, dry.direct), ([[0.0; 2]; 2], 1.0));
    }
}
