//! Amplifier models: Twang (`0x23`), Jump (`0x4d`), Van51 (`0x52`), AC Box
//! (`0x53`), Hot Solo (`0x54` v`0x10`), and Bass Pro (`0x61`), mapped onto
//! [`ir::AmpStack`].

use kontra_ir as ir;

use crate::module::{
    Cursor, MixMode, Module, RAW_LAW, UNIT_LAW, bool_f, eq_db, exp_range, ids, knob_row, unit,
};

pub(crate) const fn base_amp(voicing: ir::AmpVoicing) -> ir::AmpStack {
    ir::AmpStack {
        voicing,
        preamp_gain: 1.0,
        alt_preamp_gain: 0.0,
        drive: 0.35,
        master: 1.0,
        high_gain: false,
        bright: false,
        low_boost: false,
        high_boost: false,
        bass_db: 0.0,
        mid_db: 0.0,
        mid_hz: 650.0,
        treble_db: 0.0,
        presence_db: 0.0,
        resonance_db: 0.0,
        tone_cut_hz: 18_000.0,
        tremolo_hz: 4.0,
        tremolo_depth: 0.0,
        geq_enabled: false,
        geq_db: [0.0; 9],
        geq_gain_db: 0.0,
        mono: false,
    }
}

/// Twang (`0x23`, v`0x50`/`0x51`, `$EFFECT_TYPE_TWANG` = 23).
/// Version `0x50` stores `mono` inverted (`mono ^ 1`); `0x51` stores `mono` directly.
pub mod twang {
    use super::*;

    crate::define_effect! {
        struct Twang,
        init: INIT,
        id: ids::TWANG,
        module_id: 0x23,
        versions: (0x50, 0x51),
        name: "Twang",
        script_type: 23,
        mix: MixMode::InPlace,
        build: |s| ir::EffectKind::AmpStack(amp(s)),
        fields: [
            (volume: f32  = 0.5,   "$ENGINE_PAR_TW_VOLUME"),
            (treble: f32  = 0.5,   "$ENGINE_PAR_TW_TREBLE"),
            (mid:    f32  = 0.5,   "$ENGINE_PAR_TW_MID"),
            (bass:   f32  = 0.5,   "$ENGINE_PAR_TW_BASS"),
            (bright: bool = false, "$ENGINE_PAR_TW_BRIGHT"),
            (mono:   bool = false, "$ENGINE_PAR_TW_MONO"),
        ]
    }

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<Twang> {
        if !matches!(version, 0x50 | 0x51) {
            return None;
        }
        let mut cur = Cursor::new(public);
        let volume = cur.f32()?;
        let treble = cur.f32()?;
        let mid = cur.f32()?;
        let bass = cur.f32()?;
        let bright = cur.bool()?;
        let raw_mono = cur.bool()?;
        Some(Twang {
            volume,
            treble,
            mid,
            bass,
            bright,
            mono: if version == 0x50 { !raw_mono } else { raw_mono },
        })
    }

    #[must_use]
    pub fn amp(s: Twang) -> ir::AmpStack {
        let vol = unit(s.volume);
        ir::AmpStack {
            preamp_gain: 1.70 * 15.0f32.powf(vol - 0.5),
            drive: vol,
            master: 1.0,
            bright: s.bright,
            bass_db: eq_db(s.bass, 7.5),
            mid_db: eq_db(s.mid, 8.0),
            mid_hz: 600.0,
            treble_db: eq_db(s.treble, 9.0),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::CleanCombo)
        }
    }
}

/// Jump (`0x4d`, v`0x10`, `$EFFECT_TYPE_JUMP` = 31).
pub mod jump {
    use super::*;

    crate::define_effect! {
        struct Jump,
        init: INIT,
        id: ids::JUMP,
        module_id: 0x4d,
        versions: (0x10, 0x10),
        name: "Jump",
        script_type: 31,
        mix: MixMode::InPlace,
        saved: 0x10,
        build: |s| ir::EffectKind::AmpStack(amp(s)),
        fields: [
            (preamp:   f32  = 0.5,   "$ENGINE_PAR_JMP_PREAMP"),
            (master:   f32  = 0.5,   "$ENGINE_PAR_JMP_MASTER"),
            (presence: f32  = 0.5,   "$ENGINE_PAR_JMP_PRESENCE"),
            (treble:   f32  = 0.5,   "$ENGINE_PAR_JMP_TREBLE"),
            (mid:      f32  = 0.5,   "$ENGINE_PAR_JMP_MID"),
            (bass:     f32  = 0.5,   "$ENGINE_PAR_JMP_BASS"),
            (higain:   bool = false, "$ENGINE_PAR_JMP_HIGAIN"),
            (mono:     bool = false, "$ENGINE_PAR_JMP_MONO"),
        ]
    }

    #[must_use]
    pub fn amp(s: Jump) -> ir::AmpStack {
        let pre = unit(s.preamp);
        let mst = unit(s.master);
        ir::AmpStack {
            preamp_gain: 0.097 * 25.0f32.powf(pre - 0.5),
            drive: pre,
            master: 2.0 * mst,
            high_gain: s.higain,
            bass_db: eq_db(s.bass, 4.0),
            mid_db: eq_db(s.mid, 6.0),
            mid_hz: 800.0,
            treble_db: eq_db(s.treble, 4.0),
            presence_db: eq_db(s.presence, 6.0),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::BritishHighGain)
        }
    }
}

/// Van51 (`0x52`, v`0x10`, `$EFFECT_TYPE_VAN51` = 32).
pub mod van51 {
    use super::*;

    crate::define_effect! {
        struct Van51,
        init: INIT,
        id: ids::VAN51,
        module_id: 0x52,
        versions: (0x10, 0x10),
        name: "Van51",
        script_type: 32,
        mix: MixMode::InPlace,
        saved: 0x10,
        build: |s| ir::EffectKind::AmpStack(amp(s)),
        fields: [
            (pregain_rhythm: f32  = 0.45,  "$ENGINE_PAR_V5_PREGAINRHYTHM"),
            (pregain_lead:   f32  = 0.65,  "$ENGINE_PAR_V5_PREGAINLEAD"),
            (bass:           f32  = 0.5,   "$ENGINE_PAR_V5_BASS"),
            (mid:            f32  = 0.5,   "$ENGINE_PAR_V5_MID"),
            (treble:         f32  = 0.5,   "$ENGINE_PAR_V5_TREBLE"),
            (postgain:       f32  = 0.5,   "$ENGINE_PAR_V5_POSTGAIN"),
            (resonance:      f32  = 0.5,   "$ENGINE_PAR_V5_RESONANCE"),
            (presence:       f32  = 0.5,   "$ENGINE_PAR_V5_PRESENCE"),
            (lead_channel:   bool = false, "$ENGINE_PAR_V5_LEADCHANNEL"),
            (higain:         bool = false, "$ENGINE_PAR_V5_HIGAIN"),
            (bright:         bool = false, "$ENGINE_PAR_V5_BRIGHT"),
            (crunch:         bool = false, "$ENGINE_PAR_V5_CRUNCH"),
            (mono:           bool = false, "$ENGINE_PAR_V5_MONO"),
        ]
    }

    #[must_use]
    pub fn amp(s: Van51) -> ir::AmpStack {
        let rhythm = unit(s.pregain_rhythm);
        let lead = unit(s.pregain_lead);
        let (active, alt) = if s.lead_channel {
            (lead, rhythm)
        } else {
            (rhythm, lead)
        };
        let channel_mult = if s.lead_channel { 3.8 } else { 1.0 };
        let post = unit(s.postgain);
        ir::AmpStack {
            preamp_gain: 16.0 * 8.0f32.powf(active - 0.5) * channel_mult,
            alt_preamp_gain: 16.0 * 8.0f32.powf(alt - 0.5),
            drive: active,
            master: 2.0 * post,
            high_gain: s.higain,
            bright: s.bright,
            low_boost: s.crunch,
            high_boost: s.lead_channel,
            bass_db: eq_db(s.bass, 6.0),
            mid_db: eq_db(s.mid, 7.0),
            mid_hz: 750.0,
            treble_db: eq_db(s.treble, 5.0),
            presence_db: eq_db(s.presence, 6.0),
            resonance_db: eq_db(s.resonance, 6.0),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::ModernHighGain)
        }
    }
}

/// AC Box (`0x53`, v`0x10`, `$EFFECT_TYPE_ACBOX` = 33).
pub mod ac_box {
    use super::*;

    crate::define_effect! {
        struct AcBox,
        init: INIT,
        id: ids::AC_BOX,
        module_id: 0x53,
        versions: (0x10, 0x10),
        name: "AC Box",
        script_type: 33,
        mix: MixMode::InPlace,
        saved: 0x10,
        build: |s| ir::EffectKind::AmpStack(amp(s)),
        fields: [
            (normal_volume:    f32  = 0.5,   "$ENGINE_PAR_AC_NORMALVOLUME"),
            (brilliant_volume: f32  = 0.5,   "$ENGINE_PAR_AC_BRILLIANTVOLUME"),
            (bass:             f32  = 0.5,   "$ENGINE_PAR_AC_BASS"),
            (treble:           f32  = 0.5,   "$ENGINE_PAR_AC_TREBLE"),
            (tone_cut:         f32  = 0.25,  "$ENGINE_PAR_AC_TONECUT"),
            (tremolo_speed:    f32  = 0.4,   "$ENGINE_PAR_AC_TREMOLOSPEED"),
            (tremolo_depth:    f32  = 0.0,   "$ENGINE_PAR_AC_TREMOLODEPTH"),
            (mono:             bool = false, "$ENGINE_PAR_AC_MONO"),
        ]
    }

    #[must_use]
    pub fn amp(s: AcBox) -> ir::AmpStack {
        let norm = unit(s.normal_volume);
        let brill = unit(s.brilliant_volume);
        let cut = unit(s.tone_cut);
        ir::AmpStack {
            preamp_gain: 0.2 + 1.2 * norm + 1.87 * 15.0f32.powf(brill - 0.5),
            alt_preamp_gain: brill,
            drive: (0.5 * norm + 0.5 * brill).clamp(0.0, 1.0),
            master: 1.0,
            bass_db: eq_db(s.bass, 6.0),
            mid_db: 0.0,
            mid_hz: 700.0,
            treble_db: eq_db(s.treble, 12.5),
            tone_cut_hz: exp_range(1.0 - cut, 1_080.0, 18_000.0),
            tremolo_hz: exp_range(s.tremolo_speed, 0.8, 12.0),
            tremolo_depth: unit(s.tremolo_depth),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::ClassACombo)
        }
    }
}

/// Hot Solo (`0x54`, v`0x10` only, `$EFFECT_TYPE_HOTSOLO` = 34).
pub mod hot_solo {
    use super::*;

    crate::define_effect! {
        struct HotSolo,
        init: INIT,
        id: ids::HOT_SOLO,
        module_id: 0x54,
        versions: (0x10, 0x10),
        name: "Hot Solo",
        script_type: 34,
        mix: MixMode::InPlace,
        saved: 0x10,
        build: |s| ir::EffectKind::AmpStack(amp(s)),
        fields: [
            (pre_normal:    f32  = 0.45,  "$ENGINE_PAR_HS_PRENORMAL"),
            (pre_overdrive: f32  = 0.65,  "$ENGINE_PAR_HS_PREOVERDRIVE"),
            (master:        f32  = 0.5,   "$ENGINE_PAR_HS_MASTER"),
            (presence:      f32  = 0.5,   "$ENGINE_PAR_HS_PRESENCE"),
            (depth:         f32  = 0.5,   "$ENGINE_PAR_HS_DEPTH"),
            (treble:        f32  = 0.5,   "$ENGINE_PAR_HS_TREBLE"),
            (mid:           f32  = 0.5,   "$ENGINE_PAR_HS_MID"),
            (bass:          f32  = 0.5,   "$ENGINE_PAR_HS_BASS"),
            (overdrive:     bool = false, "$ENGINE_PAR_HS_OVERDRIVE"),
            (mono:          bool = false, "$ENGINE_PAR_HS_MONO"),
        ]
    }

    #[must_use]
    pub fn amp(s: HotSolo) -> ir::AmpStack {
        let norm = unit(s.pre_normal);
        let od = unit(s.pre_overdrive);
        let (active, alt) = if s.overdrive { (od, norm) } else { (norm, od) };
        let mst = unit(s.master);
        ir::AmpStack {
            preamp_gain: 4.58 * 6.0f32.powf(active - 0.5),
            alt_preamp_gain: 4.58 * 6.0f32.powf(alt - 0.5),
            drive: active,
            master: 2.0 * mst,
            high_gain: s.overdrive,
            bass_db: eq_db(s.bass, 6.0),
            mid_db: eq_db(s.mid, 6.0),
            mid_hz: 720.0,
            treble_db: eq_db(s.treble, 5.0),
            presence_db: eq_db(s.presence, 6.0),
            resonance_db: eq_db(s.depth, 4.0),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::LeadHighGain)
        }
    }
}

/// Bass Pro (`0x61`, v`0x10`, `$EFFECT_TYPE_BASSPRO` = 47).
pub mod bass_pro {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct BassPro {
        pub gain: f32,
        pub bass: f32,
        pub mid: f32,
        pub midfreq: f32,
        pub treble: f32,
        pub drive: f32,
        pub master: f32,
        pub geq_bands: [f32; 9],
        pub geq_volume: f32,
        pub ultra_lo: bool,
        pub ultra_hi: bool,
        pub bright: bool,
        pub geq: bool,
        pub mono: bool,
    }

    pub const INIT: BassPro = BassPro {
        gain: 0.5,
        bass: 0.5,
        mid: 0.5,
        midfreq: 0.5,
        treble: 0.5,
        drive: 0.35,
        master: 0.5,
        geq_bands: [0.5; 9],
        geq_volume: 0.5,
        ultra_lo: false,
        ultra_hi: false,
        bright: false,
        geq: false,
        mono: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<BassPro> {
        if version != 0x10 {
            return None;
        }
        let mut cur = Cursor::new(public);
        Some(BassPro {
            gain: cur.f32()?,
            bass: cur.f32()?,
            mid: cur.f32()?,
            midfreq: cur.f32()?,
            treble: cur.f32()?,
            drive: cur.f32()?,
            master: cur.f32()?,
            geq_bands: cur.f32_array()?,
            geq_volume: cur.f32()?,
            ultra_lo: cur.bool()?,
            ultra_hi: cur.bool()?,
            bright: cur.bool()?,
            geq: cur.bool()?,
            mono: cur.bool()?,
        })
    }

    #[must_use]
    pub fn amp(s: BassPro) -> ir::AmpStack {
        let g = unit(s.gain);
        let mst = unit(s.master);
        ir::AmpStack {
            preamp_gain: 2.71 * 5.0f32.powf(g - 0.5),
            drive: unit(s.drive),
            master: 2.0 * mst,
            bright: s.bright,
            low_boost: s.ultra_lo,
            high_boost: s.ultra_hi,
            bass_db: eq_db(s.bass, 10.0),
            mid_db: eq_db(s.mid, 10.0),
            mid_hz: exp_range(s.midfreq, 220.0, 3_000.0),
            treble_db: eq_db(s.treble, 10.0),
            geq_enabled: s.geq,
            geq_db: s.geq_bands.map(|b| eq_db(b, 12.0)),
            geq_gain_db: eq_db(s.geq_volume, 12.0),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::BassTube)
        }
    }

    pub static MODULE: Module = Module {
        id: ids::BASS_PRO,
        module_id: 0x61,
        versions: (0x10, 0x10),
        name: "Bass Pro",
        script_type: 47,
        mix: MixMode::InPlace,
        knobs: crate::module::KNOBS.split_at(22).0,
        build: |v| {
            let [
                gain,
                bass,
                mid,
                midfreq,
                treble,
                drive,
                master,
                g0,
                g1,
                g2,
                g3,
                g4,
                g5,
                g6,
                g7,
                g8,
                geq_volume,
                ultra_lo,
                ultra_hi,
                bright,
                geq,
                mono,
            ] = *v
            else {
                return None;
            };
            Some(ir::EffectKind::AmpStack(amp(BassPro {
                gain,
                bass,
                mid,
                midfreq,
                treble,
                drive,
                master,
                geq_bands: [g0, g1, g2, g3, g4, g5, g6, g7, g8],
                geq_volume,
                ultra_lo: ultra_lo >= 0.5,
                ultra_hi: ultra_hi >= 0.5,
                bright: bright >= 0.5,
                geq: geq >= 0.5,
                mono: mono >= 0.5,
            })))
        },
    };

    #[must_use]
    pub fn model(s: BassPro) -> ir::KnobModel {
        let mut arr = [0.0f32; 22];
        arr[..7].copy_from_slice(&[
            s.gain, s.bass, s.mid, s.midfreq, s.treble, s.drive, s.master,
        ]);
        arr[7..16].copy_from_slice(&s.geq_bands);
        arr[16] = s.geq_volume;
        arr[17..22].copy_from_slice(&[
            bool_f(s.ultra_lo),
            bool_f(s.ultra_hi),
            bool_f(s.bright),
            bool_f(s.geq),
            bool_f(s.mono),
        ]);
        MODULE.knob_model(&arr)
    }

    #[must_use]
    pub fn slot_type() -> ir::SlotType {
        MODULE.slot_type_model(model(INIT))
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 22] {
        const NAMES: [&str; 22] = [
            "$ENGINE_PAR_BASSPRO_GAIN",
            "$ENGINE_PAR_BASSPRO_BASS",
            "$ENGINE_PAR_BASSPRO_MID",
            "$ENGINE_PAR_BASSPRO_MIDFREQ",
            "$ENGINE_PAR_BASSPRO_TREBLE",
            "$ENGINE_PAR_BASSPRO_DRIVE",
            "$ENGINE_PAR_BASSPRO_MASTER",
            "$ENGINE_PAR_BASSPRO_GEQ_40",
            "$ENGINE_PAR_BASSPRO_GEQ_90",
            "$ENGINE_PAR_BASSPRO_GEQ_180",
            "$ENGINE_PAR_BASSPRO_GEQ_300",
            "$ENGINE_PAR_BASSPRO_GEQ_500",
            "$ENGINE_PAR_BASSPRO_GEQ_1K",
            "$ENGINE_PAR_BASSPRO_GEQ_2K",
            "$ENGINE_PAR_BASSPRO_GEQ_4K",
            "$ENGINE_PAR_BASSPRO_GEQ_10K",
            "$ENGINE_PAR_BASSPRO_GEQ_VOLUME",
            "$ENGINE_PAR_BASSPRO_ULTRALO",
            "$ENGINE_PAR_BASSPRO_ULTRAHI",
            "$ENGINE_PAR_BASSPRO_BRIGHT",
            "$ENGINE_PAR_BASSPRO_GEQ",
            "$ENGINE_PAR_BASSPRO_MONO",
        ];
        std::array::from_fn(|i| {
            knob_row(
                NAMES.get(i).copied().unwrap_or(""),
                i as u8,
                if i < 17 { UNIT_LAW } else { RAW_LAW },
            )
        })
    }
}

/// All KSP knob rows for the six amplifier modules.
#[must_use]
pub fn rows() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    let mut out = Vec::with_capacity(67);
    out.extend(twang::rows());
    out.extend(jump::rows());
    out.extend(van51::rows());
    out.extend(ac_box::rows());
    out.extend(hot_solo::rows());
    out.extend(bass_pro::rows());
    out
}

/// Fresh [`ir::SlotType`] entries for all six script-loadable amplifier modules.
#[must_use]
pub fn slot_types() -> Vec<ir::SlotType> {
    vec![
        twang::slot_type(),
        jump::slot_type(),
        van51::slot_type(),
        ac_box::slot_type(),
        hot_solo::slot_type(),
        bass_pro::slot_type(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twang_v50_inverts_mono_bit_and_v51_keeps_it() {
        let mut raw = Vec::new();
        for f in [0.4f32, 0.6, 0.5, 0.7] {
            raw.extend(f.to_le_bytes());
        }
        raw.extend([1u8, 0]);
        let old = twang::saved(0x50, &raw).unwrap();
        let new = twang::saved(0x51, &raw).unwrap();
        assert!(old.bright && old.mono && new.bright && !new.mono);
        // Hot Solo rejects Bass Invader versions (0x51/0x52).
        let hs_bytes = vec![0u8; 34];
        assert!(hot_solo::saved(0x10, &hs_bytes).is_some());
        assert!(hot_solo::saved(0x51, &hs_bytes).is_none());
    }
}
