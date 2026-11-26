//! Skreamer Deluxe (module `0x78`, saved v`0x10`, `$EFFECT_TYPE_SKREAMERDLX` = 69)
//! onto [`ir::AmpStack`] (`AmpVoicing::MidOverdrive`).
//!
//! Saved record (25 bytes, `I + F*5 + B`): `mode` (`0..=2`), `drive`, `tone`,
//! `bass`, `mid`, `treble`, `mono`.

use kontra_ir as ir;

use crate::module::{MixMode, Module, RAW_LAW, UNIT_LAW, f32_le, i32_le, ids, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkreamerDeluxe {
    pub mode: i32,
    pub drive: f32,
    pub tone: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub mono: bool,
}

/// Fresh INIT defaults for a script-loaded Skreamer Deluxe.
pub const INIT: SkreamerDeluxe = SkreamerDeluxe {
    mode: 0,
    drive: 0.5,
    tone: 0.5,
    bass: 0.5,
    mid: 0.5,
    treble: 0.5,
    mono: false,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<SkreamerDeluxe> {
    if version != 0x10 {
        return None;
    }
    Some(SkreamerDeluxe {
        mode: i32_le(public, 0)?,
        drive: f32_le(public, 4)?,
        tone: f32_le(public, 8)?,
        bass: f32_le(public, 12)?,
        mid: f32_le(public, 16)?,
        treble: f32_le(public, 20)?,
        mono: *public.get(24)? != 0,
    })
}

/// Physical IR settings for `s`.
#[must_use]
pub fn amp(s: SkreamerDeluxe) -> ir::AmpStack {
    let mode = s.mode.clamp(0, 2) as u8;
    let drv = s.drive.clamp(0.0, 1.0);
    let tone = s.tone.clamp(0.0, 1.0);
    let eq = |x: f32, span: f32| (x.clamp(0.0, 1.0) * 2.0 - 1.0) * span;
    let mode_gain = match mode {
        1 => 1.15,
        2 => 1.55,
        _ => 2.75,
    };
    ir::AmpStack {
        voicing: ir::AmpVoicing::MidOverdrive(mode),
        preamp_gain: mode_gain * 8.5f32.powf(drv - 0.5),
        alt_preamp_gain: 0.0,
        drive: drv,
        master: 1.0,
        high_gain: mode == 2,
        bright: false,
        low_boost: false,
        high_boost: false,
        bass_db: eq(s.bass, 8.0),
        mid_db: eq(s.mid, 8.0),
        mid_hz: 720.0,
        treble_db: eq(s.treble, 8.0) + eq(tone, 14.5),
        presence_db: 0.0,
        resonance_db: 0.0,
        tone_cut_hz: 1_500.0 * (18_000.0f32 / 1_500.0).powf(tone),
        tremolo_hz: 4.0,
        tremolo_depth: 0.0,
        geq_enabled: false,
        geq_db: [0.0; 9],
        geq_gain_db: 0.0,
        mono: s.mono,
    }
}

/// Single-encoding module descriptor for Skreamer Deluxe (`0x78`).
pub static MODULE: Module = Module {
    id: ids::SKREAMER_DELUXE,
    module_id: 0x78,
    versions: (0x10, 0x10),
    name: "Skreamer Deluxe",
    script_type: 69,
    mix: MixMode::InPlace,
    knobs: crate::module::KNOBS.split_at(7).0,
    build: |v| {
        let [mode, drive, tone, bass, mid, treble, mono] = *v else {
            return None;
        };
        Some(ir::EffectKind::AmpStack(amp(SkreamerDeluxe {
            mode: mode.round() as i32,
            drive,
            tone,
            bass,
            mid,
            treble,
            mono: mono >= 0.5,
        })))
    },
};

#[must_use]
pub fn model(s: SkreamerDeluxe) -> ir::KnobModel {
    MODULE.knob_model(&[
        s.mode as f32,
        s.drive,
        s.tone,
        s.bass,
        s.mid,
        s.treble,
        if s.mono { 1.0 } else { 0.0 },
    ])
}

/// KSP `$ENGINE_PAR_SKDLX_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 7] {
    [
        knob_row("$ENGINE_PAR_SKDLX_MODE", 0, RAW_LAW),
        knob_row("$ENGINE_PAR_SKDLX_DRIVE", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_SKDLX_TONE", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_SKDLX_BASS", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_SKDLX_MID", 4, UNIT_LAW),
        knob_row("$ENGINE_PAR_SKDLX_TREBLE", 5, UNIT_LAW),
        knob_row("$ENGINE_PAR_SKDLX_MONO", 6, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_and_model_roundtrips() {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        for f in [0.7f32, 0.6, 0.4, 0.5, 0.8] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.push(1);
        let s = saved(0x10, &bytes).unwrap();
        assert_eq!(s.mode, 2);
        assert!(s.mono);
        assert_eq!(amp(s).voicing, ir::AmpVoicing::MidOverdrive(2));
        let m = model(INIT);
        assert_eq!(m.build(&m.saved), Some(ir::EffectKind::AmpStack(amp(INIT))));
    }
}
