//! Freak (module `0x68`) onto the engine's ring modulator ([`ir::RingMod`])
//! in all three modes: Radio (0), Oscillator (1), Sidechain (2).

use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

use crate::module::{Cursor, MixMode, RAW_LAW, bool_f, i32_le, ids, unit};

/// The Oscillator mode's index (Radio 0, Oscillator 1, Sidechain 2).
pub const OSCILLATOR: i32 = 1;

crate::define_effect! {
    struct Knobs,
    init: FRESH,
    id: ids::FREAK,
    module_id: 0x68,
    versions: (0, u32::MAX),
    name: "Freak",
    script_type: 54,
    mix: MixMode::OwnMix,
    build: freak,
    fields: [
        (kind:       f32 = 0.5,               "$ENGINE_PAR_FREAK_TYPE"),
        (frequency:  f32 = 0.513_572,         "$ENGINE_PAR_FREAK_FREQUENCY"),
        (harmonics:  f32 = 0.0,               "$ENGINE_PAR_FREAK_HARMONICS"),
        (antifold:   f32 = 0.0,               "$ENGINE_PAR_FREAK_ANTIFOLD"),
        (stereo:     f32 = 0.5,               "$ENGINE_PAR_FREAK_STEREO"),
        (mix:        f32 = 1.0,               "$ENGINE_PAR_FREAK_MIX"),
        (wide_range: f32 = 1.0,               "$ENGINE_PAR_FREAK_WIDE_RANGE", RAW_LAW),
        (gate:       f32 = 1.0,               "$ENGINE_PAR_FREAK_GATE",       RAW_LAW),
        (feedback:   f32 = 0.0,               "$ENGINE_PAR_FREAK_FEEDBACK"),
        (tuning:     f32 = 0.5,               "$ENGINE_PAR_FREAK_TUNING"),
        (carrier:    f32 = 1.0,               "$ENGINE_PAR_FREAK_CARRIER"),
        (width:      f32 = 0.5,               "$ENGINE_PAR_FREAK_WIDTH"),
        (contour:    f32 = 0.5,               "$ENGINE_PAR_FREAK_CONTOUR"),
        (bp_freq:    f32 = 0.5,               "$ENGINE_PAR_FREAK_BP_FREQ"),
        (release:    f32 = 0.5,               "$ENGINE_PAR_FREAK_RELEASE"),
        (demod:      f32 = 0.0,               "$ENGINE_PAR_FREAK_DEMOD",      RAW_LAW),
        (bp_filter:  f32 = 0.0,               "$ENGINE_PAR_FREAK_BP_FILTER",  RAW_LAW),
        (mode:       f32 = OSCILLATOR as f32, "$ENGINE_PAR_FREAK_MODE",       RAW_LAW),
    ]
}

/// The ring modulator at `k`.
#[must_use]
pub fn freak(k: Knobs) -> ir::EffectKind {
    let on = |x: f32| x >= 0.5;
    let pitch = |index: f32| PITCH_INDEX.eval_unclamped(index);
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
            let c = unit(k.carrier);
            hz = pitch(200.0 * c + 1_881.881_6);
            let (a, b) = (40.0 * c + 100.0, 24.0 - 12.0 * c);
            let centre = (a - b) + 2.0 * b * unit(k.tuning);
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
                product: unit(k.demod),
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

/// Decodes a saved Freak record (`Fx13Bx5I`, 61 bytes).
#[must_use]
pub fn saved(public: &[u8]) -> Option<Knobs> {
    let mut cur = Cursor::new(public);
    let kind = cur.f32()?;
    let harmonics = cur.f32()?;
    let feedback = cur.f32()?;
    let mix = cur.f32()?;
    let tuning = cur.f32()?;
    let carrier = cur.f32()?;
    let width = cur.f32()?;
    let frequency = cur.f32()?;
    let antifold = cur.f32()?;
    let stereo = cur.f32()?;
    let contour = cur.f32()?;
    let bp_freq = cur.f32()?;
    let release = cur.f32()?;
    let demod = bool_f(cur.bool()?);
    let gate = bool_f(cur.bool()?);
    let wide_range = bool_f(cur.bool()?);
    let bp_filter = bool_f(cur.bool()?);
    let mode = i32_le(public, 57)?;
    Some(Knobs {
        kind,
        harmonics,
        feedback,
        mix,
        tuning,
        carrier,
        width,
        frequency,
        antifold,
        stereo,
        contour,
        bp_freq,
        release,
        demod,
        gate,
        wide_range,
        bp_filter,
        mode: mode.clamp(0, 2) as f32,
    })
}
