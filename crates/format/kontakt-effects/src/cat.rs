//! Cat (module `0x55`, saved v`0x10`, `$EFFECT_TYPE_CAT` = 35) onto
//! [`ir::AmpStack`] (`AmpVoicing::HardClipPedal`).
//!
//! Saved record (29 bytes, `FFFFFFFB`): `volume`, `distortion`, `filter`,
//! `bass`, `balls`, `treble`, `tone`, `mono`.

use kontra_ir as ir;

use crate::module::{MixMode, Module, RAW_LAW, UNIT_LAW, f32_le, ids, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cat {
    pub volume: f32,
    pub distortion: f32,
    pub filter: f32,
    pub bass: f32,
    pub balls: f32,
    pub treble: f32,
    pub tone: f32,
    pub mono: bool,
}

/// Fresh INIT defaults for a script-loaded Cat distortion pedal.
pub const INIT: Cat = Cat {
    volume: 0.5,
    distortion: 0.5,
    filter: 0.35,
    bass: 0.5,
    balls: 0.5,
    treble: 0.5,
    tone: 0.5,
    mono: false,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Cat> {
    if version != 0x10 {
        return None;
    }
    Some(Cat {
        volume: f32_le(public, 0)?,
        distortion: f32_le(public, 4)?,
        filter: f32_le(public, 8)?,
        bass: f32_le(public, 12)?,
        balls: f32_le(public, 16)?,
        treble: f32_le(public, 20)?,
        tone: f32_le(public, 24)?,
        mono: *public.get(28)? != 0,
    })
}

/// Physical IR settings for `c`.
#[must_use]
pub fn amp(c: Cat) -> ir::AmpStack {
    let dist = c.distortion.clamp(0.0, 1.0);
    let filt = c.filter.clamp(0.0, 1.0);
    let vol = c.volume.clamp(0.0, 1.0);
    let eq = |x: f32, span: f32| (x.clamp(0.0, 1.0) * 2.0 - 1.0) * span;
    ir::AmpStack {
        voicing: ir::AmpVoicing::HardClipPedal,
        preamp_gain: 2.15 * 1.56f32.powf(dist - 0.5),
        alt_preamp_gain: 0.0,
        drive: dist,
        master: 2.0 * vol,
        high_gain: dist > 0.65,
        bright: false,
        low_boost: false,
        high_boost: false,
        bass_db: eq(c.bass, 6.0),
        mid_db: eq(c.tone, 6.0),
        mid_hz: 950.0,
        treble_db: eq(c.treble, 6.0),
        presence_db: 0.0,
        resonance_db: eq(c.balls, 6.0),
        // Classic reverse filter knob: higher values darken the post low-pass (~-10.1 dB @ 2 kHz at max).
        tone_cut_hz: 1_120.0 * (14_000.0f32 / 1_120.0).powf(1.0 - filt),
        tremolo_hz: 4.0,
        tremolo_depth: 0.0,
        geq_enabled: false,
        geq_db: [0.0; 9],
        geq_gain_db: 0.0,
        mono: c.mono,
    }
}

/// Single-encoding module descriptor for Cat (`0x55`).
pub static MODULE: Module = Module {
    id: ids::CAT,
    module_id: 0x55,
    versions: (0x10, 0x10),
    name: "Cat",
    script_type: 35,
    mix: MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(8).0,
    build: |v| {
        let [volume, distortion, filter, bass, balls, treble, tone, mono] = *v else {
            return None;
        };
        Some(ir::EffectKind::AmpStack(amp(Cat {
            volume,
            distortion,
            filter,
            bass,
            balls,
            treble,
            tone,
            mono: mono >= 0.5,
        })))
    },
};

#[must_use]
pub fn model(c: Cat) -> ir::KnobModel {
    MODULE.knob_model(&[
        c.volume,
        c.distortion,
        c.filter,
        c.bass,
        c.balls,
        c.treble,
        c.tone,
        if c.mono { 1.0 } else { 0.0 },
    ])
}

/// KSP `$ENGINE_PAR_CT_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 8] {
    [
        knob_row("$ENGINE_PAR_CT_VOLUME", 0, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_DISTORTION", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_FILTER", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_BASS", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_BALLS", 4, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_TREBLE", 5, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_TONE", 6, UNIT_LAW),
        knob_row("$ENGINE_PAR_CT_MONO", 7, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_model_roundtrips() {
        let mut bytes = Vec::new();
        for f in [0.6f32, 0.8, 0.25, 0.5, 0.7, 0.4, 0.55] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.push(1);
        let c = saved(0x10, &bytes).unwrap();
        assert!(c.mono);
        assert!((c.distortion - 0.8).abs() < 1e-6);
        let m = model(INIT);
        assert_eq!(m.build(&m.saved), Some(ir::EffectKind::AmpStack(amp(INIT))));
    }
}
