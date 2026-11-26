//! Amplifier models: Twang (`0x23`), Jump (`0x4d`), Van51 (`0x52`), AC Box
//! (`0x53`), Hot Solo (`0x54` v`0x10`), and Bass Pro (`0x61`), mapped onto
//! [`ir::AmpStack`].

use kontra_ir as ir;

use crate::module::{MixMode, Module, RAW_LAW, UNIT_LAW, f32_le, ids, knob_row};

#[inline]
fn unit(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

#[inline]
fn eq_db(x: f32, span: f32) -> f32 {
    (unit(x) * 2.0 - 1.0) * span
}

#[inline]
fn exp_range(x: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(unit(x))
}

#[inline]
fn flag(v: f32) -> bool {
    v >= 0.5
}

#[inline]
fn bool_f(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

const fn base_amp(voicing: ir::AmpVoicing) -> ir::AmpStack {
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
///
/// Saved record (18 bytes, `FFFFBB`): `volume`, `treble`, `mid`, `bass`,
/// `bright`, `mono`. Version `0x50` stores `mono` inverted (`mono ^ 1`);
/// version `0x51` stores `mono` directly.
pub mod twang {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Twang {
        pub volume: f32,
        pub treble: f32,
        pub mid: f32,
        pub bass: f32,
        pub bright: bool,
        pub mono: bool,
    }

    pub const INIT: Twang = Twang {
        volume: 0.5,
        treble: 0.5,
        mid: 0.5,
        bass: 0.5,
        bright: false,
        mono: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<Twang> {
        if !matches!(version, 0x50 | 0x51) {
            return None;
        }
        let raw_mono = *public.get(17)? != 0;
        let mono = if version == 0x50 { !raw_mono } else { raw_mono };
        Some(Twang {
            volume: f32_le(public, 0)?,
            treble: f32_le(public, 4)?,
            mid: f32_le(public, 8)?,
            bass: f32_le(public, 12)?,
            bright: *public.get(16)? != 0,
            mono,
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

    pub static MODULE: Module = Module {
        id: ids::TWANG,
        module_id: 0x23,
        versions: (0x50, 0x51),
        name: "Twang",
        script_type: 23,
        mix: MixMode::InPlace,
        knobs: crate::module::KNOBS.split_at(6).0,
        build: |v| {
            let [volume, treble, mid, bass, bright, mono] = *v else {
                return None;
            };
            Some(ir::EffectKind::AmpStack(amp(Twang {
                volume,
                treble,
                mid,
                bass,
                bright: flag(bright),
                mono: flag(mono),
            })))
        },
    };

    #[must_use]
    pub fn model(s: Twang) -> ir::KnobModel {
        MODULE.knob_model(&[
            s.volume,
            s.treble,
            s.mid,
            s.bass,
            bool_f(s.bright),
            bool_f(s.mono),
        ])
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
        [
            knob_row("$ENGINE_PAR_TW_VOLUME", 0, UNIT_LAW),
            knob_row("$ENGINE_PAR_TW_TREBLE", 1, UNIT_LAW),
            knob_row("$ENGINE_PAR_TW_MID", 2, UNIT_LAW),
            knob_row("$ENGINE_PAR_TW_BASS", 3, UNIT_LAW),
            knob_row("$ENGINE_PAR_TW_BRIGHT", 4, RAW_LAW),
            knob_row("$ENGINE_PAR_TW_MONO", 5, RAW_LAW),
        ]
    }
}

/// Jump (`0x4d`, v`0x10`, `$EFFECT_TYPE_JUMP` = 31).
///
/// Saved record (26 bytes, `FFFFFFBB`): `preamp`, `master`, `presence`,
/// `treble`, `mid`, `bass`, `higain`, `mono`.
pub mod jump {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Jump {
        pub preamp: f32,
        pub master: f32,
        pub presence: f32,
        pub treble: f32,
        pub mid: f32,
        pub bass: f32,
        pub higain: bool,
        pub mono: bool,
    }

    pub const INIT: Jump = Jump {
        preamp: 0.5,
        master: 0.5,
        presence: 0.5,
        treble: 0.5,
        mid: 0.5,
        bass: 0.5,
        higain: false,
        mono: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<Jump> {
        if version != 0x10 {
            return None;
        }
        Some(Jump {
            preamp: f32_le(public, 0)?,
            master: f32_le(public, 4)?,
            presence: f32_le(public, 8)?,
            treble: f32_le(public, 12)?,
            mid: f32_le(public, 16)?,
            bass: f32_le(public, 20)?,
            higain: *public.get(24)? != 0,
            mono: *public.get(25)? != 0,
        })
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

    pub static MODULE: Module = Module {
        id: ids::JUMP,
        module_id: 0x4d,
        versions: (0x10, 0x10),
        name: "Jump",
        script_type: 31,
        mix: MixMode::InPlace,
        knobs: crate::module::KNOBS.split_at(8).0,
        build: |v| {
            let [preamp, master, presence, treble, mid, bass, higain, mono] = *v else {
                return None;
            };
            Some(ir::EffectKind::AmpStack(amp(Jump {
                preamp,
                master,
                presence,
                treble,
                mid,
                bass,
                higain: flag(higain),
                mono: flag(mono),
            })))
        },
    };

    #[must_use]
    pub fn model(s: Jump) -> ir::KnobModel {
        MODULE.knob_model(&[
            s.preamp,
            s.master,
            s.presence,
            s.treble,
            s.mid,
            s.bass,
            bool_f(s.higain),
            bool_f(s.mono),
        ])
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 8] {
        [
            knob_row("$ENGINE_PAR_JMP_PREAMP", 0, UNIT_LAW),
            knob_row("$ENGINE_PAR_JMP_MASTER", 1, UNIT_LAW),
            knob_row("$ENGINE_PAR_JMP_PRESENCE", 2, UNIT_LAW),
            knob_row("$ENGINE_PAR_JMP_TREBLE", 3, UNIT_LAW),
            knob_row("$ENGINE_PAR_JMP_MID", 4, UNIT_LAW),
            knob_row("$ENGINE_PAR_JMP_BASS", 5, UNIT_LAW),
            knob_row("$ENGINE_PAR_JMP_HIGAIN", 6, RAW_LAW),
            knob_row("$ENGINE_PAR_JMP_MONO", 7, RAW_LAW),
        ]
    }
}

/// Van51 (`0x52`, v`0x10`, `$EFFECT_TYPE_VAN51` = 32).
///
/// Saved record (37 bytes, `FFFFFFFFBBBBB`): `pregain_rhythm`, `pregain_lead`,
/// `bass`, `mid`, `treble`, `postgain`, `resonance`, `presence`,
/// `lead_channel`, `higain`, `bright`, `crunch`, `mono`.
pub mod van51 {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Van51 {
        pub pregain_rhythm: f32,
        pub pregain_lead: f32,
        pub bass: f32,
        pub mid: f32,
        pub treble: f32,
        pub postgain: f32,
        pub resonance: f32,
        pub presence: f32,
        pub lead_channel: bool,
        pub higain: bool,
        pub bright: bool,
        pub crunch: bool,
        pub mono: bool,
    }

    pub const INIT: Van51 = Van51 {
        pregain_rhythm: 0.45,
        pregain_lead: 0.65,
        bass: 0.5,
        mid: 0.5,
        treble: 0.5,
        postgain: 0.5,
        resonance: 0.5,
        presence: 0.5,
        lead_channel: false,
        higain: false,
        bright: false,
        crunch: false,
        mono: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<Van51> {
        if version != 0x10 {
            return None;
        }
        Some(Van51 {
            pregain_rhythm: f32_le(public, 0)?,
            pregain_lead: f32_le(public, 4)?,
            bass: f32_le(public, 8)?,
            mid: f32_le(public, 12)?,
            treble: f32_le(public, 16)?,
            postgain: f32_le(public, 20)?,
            resonance: f32_le(public, 24)?,
            presence: f32_le(public, 28)?,
            lead_channel: *public.get(32)? != 0,
            higain: *public.get(33)? != 0,
            bright: *public.get(34)? != 0,
            crunch: *public.get(35)? != 0,
            mono: *public.get(36)? != 0,
        })
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

    pub static MODULE: Module = Module {
        id: ids::VAN51,
        module_id: 0x52,
        versions: (0x10, 0x10),
        name: "Van51",
        script_type: 32,
        mix: MixMode::InPlace,
        knobs: crate::module::KNOBS.split_at(13).0,
        build: |v| {
            let [
                pregain_rhythm,
                pregain_lead,
                bass,
                mid,
                treble,
                postgain,
                resonance,
                presence,
                lead_channel,
                higain,
                bright,
                crunch,
                mono,
            ] = *v
            else {
                return None;
            };
            Some(ir::EffectKind::AmpStack(amp(Van51 {
                pregain_rhythm,
                pregain_lead,
                bass,
                mid,
                treble,
                postgain,
                resonance,
                presence,
                lead_channel: flag(lead_channel),
                higain: flag(higain),
                bright: flag(bright),
                crunch: flag(crunch),
                mono: flag(mono),
            })))
        },
    };

    #[must_use]
    pub fn model(s: Van51) -> ir::KnobModel {
        MODULE.knob_model(&[
            s.pregain_rhythm,
            s.pregain_lead,
            s.bass,
            s.mid,
            s.treble,
            s.postgain,
            s.resonance,
            s.presence,
            bool_f(s.lead_channel),
            bool_f(s.higain),
            bool_f(s.bright),
            bool_f(s.crunch),
            bool_f(s.mono),
        ])
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 13] {
        [
            knob_row("$ENGINE_PAR_V5_PREGAINRHYTHM", 0, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_PREGAINLEAD", 1, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_BASS", 2, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_MID", 3, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_TREBLE", 4, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_POSTGAIN", 5, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_RESONANCE", 6, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_PRESENCE", 7, UNIT_LAW),
            knob_row("$ENGINE_PAR_V5_LEADCHANNEL", 8, RAW_LAW),
            knob_row("$ENGINE_PAR_V5_HIGAIN", 9, RAW_LAW),
            knob_row("$ENGINE_PAR_V5_BRIGHT", 10, RAW_LAW),
            knob_row("$ENGINE_PAR_V5_CRUNCH", 11, RAW_LAW),
            knob_row("$ENGINE_PAR_V5_MONO", 12, RAW_LAW),
        ]
    }
}

/// AC Box (`0x53`, v`0x10`, `$EFFECT_TYPE_ACBOX` = 33).
///
/// Saved record (29 bytes, `FFFFFFFB`): `normal_volume`, `brilliant_volume`,
/// `bass`, `treble`, `tone_cut`, `tremolo_speed`, `tremolo_depth`, `mono`.
pub mod ac_box {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct AcBox {
        pub normal_volume: f32,
        pub brilliant_volume: f32,
        pub bass: f32,
        pub treble: f32,
        pub tone_cut: f32,
        pub tremolo_speed: f32,
        pub tremolo_depth: f32,
        pub mono: bool,
    }

    pub const INIT: AcBox = AcBox {
        normal_volume: 0.5,
        brilliant_volume: 0.5,
        bass: 0.5,
        treble: 0.5,
        tone_cut: 0.25,
        tremolo_speed: 0.4,
        tremolo_depth: 0.0,
        mono: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<AcBox> {
        if version != 0x10 {
            return None;
        }
        Some(AcBox {
            normal_volume: f32_le(public, 0)?,
            brilliant_volume: f32_le(public, 4)?,
            bass: f32_le(public, 8)?,
            treble: f32_le(public, 12)?,
            tone_cut: f32_le(public, 16)?,
            tremolo_speed: f32_le(public, 20)?,
            tremolo_depth: f32_le(public, 24)?,
            mono: *public.get(28)? != 0,
        })
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
            // Higher tone_cut darkens the power amp (rolls down from 18 kHz to 1080 Hz; -10.7 dB @ 2 kHz).
            tone_cut_hz: exp_range(1.0 - cut, 1_080.0, 18_000.0),
            tremolo_hz: exp_range(s.tremolo_speed, 0.8, 12.0),
            tremolo_depth: unit(s.tremolo_depth),
            mono: s.mono,
            ..base_amp(ir::AmpVoicing::ClassACombo)
        }
    }

    pub static MODULE: Module = Module {
        id: ids::AC_BOX,
        module_id: 0x53,
        versions: (0x10, 0x10),
        name: "AC Box",
        script_type: 33,
        mix: MixMode::InPlace,
        knobs: crate::module::KNOBS.split_at(8).0,
        build: |v| {
            let [
                normal_volume,
                brilliant_volume,
                bass,
                treble,
                tone_cut,
                tremolo_speed,
                tremolo_depth,
                mono,
            ] = *v
            else {
                return None;
            };
            Some(ir::EffectKind::AmpStack(amp(AcBox {
                normal_volume,
                brilliant_volume,
                bass,
                treble,
                tone_cut,
                tremolo_speed,
                tremolo_depth,
                mono: flag(mono),
            })))
        },
    };

    #[must_use]
    pub fn model(s: AcBox) -> ir::KnobModel {
        MODULE.knob_model(&[
            s.normal_volume,
            s.brilliant_volume,
            s.bass,
            s.treble,
            s.tone_cut,
            s.tremolo_speed,
            s.tremolo_depth,
            bool_f(s.mono),
        ])
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 8] {
        [
            knob_row("$ENGINE_PAR_AC_NORMALVOLUME", 0, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_BRILLIANTVOLUME", 1, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_BASS", 2, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_TREBLE", 3, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_TONECUT", 4, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_TREMOLOSPEED", 5, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_TREMOLODEPTH", 6, UNIT_LAW),
            knob_row("$ENGINE_PAR_AC_MONO", 7, RAW_LAW),
        ]
    }
}

/// Hot Solo (`0x54`, v`0x10` only, `$EFFECT_TYPE_HOTSOLO` = 34).
///
/// Saved record (34 bytes, `FFFFFFFFBB`): `pre_normal`, `pre_overdrive`,
/// `master`, `presence`, `depth`, `treble`, `mid`, `bass`, `overdrive`, `mono`.
pub mod hot_solo {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct HotSolo {
        pub pre_normal: f32,
        pub pre_overdrive: f32,
        pub master: f32,
        pub presence: f32,
        pub depth: f32,
        pub treble: f32,
        pub mid: f32,
        pub bass: f32,
        pub overdrive: bool,
        pub mono: bool,
    }

    pub const INIT: HotSolo = HotSolo {
        pre_normal: 0.45,
        pre_overdrive: 0.65,
        master: 0.5,
        presence: 0.5,
        depth: 0.5,
        treble: 0.5,
        mid: 0.5,
        bass: 0.5,
        overdrive: false,
        mono: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<HotSolo> {
        if version != 0x10 {
            return None;
        }
        Some(HotSolo {
            pre_normal: f32_le(public, 0)?,
            pre_overdrive: f32_le(public, 4)?,
            master: f32_le(public, 8)?,
            presence: f32_le(public, 12)?,
            depth: f32_le(public, 16)?,
            treble: f32_le(public, 20)?,
            mid: f32_le(public, 24)?,
            bass: f32_le(public, 28)?,
            overdrive: *public.get(32)? != 0,
            mono: *public.get(33)? != 0,
        })
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

    pub static MODULE: Module = Module {
        id: ids::HOT_SOLO,
        module_id: 0x54,
        versions: (0x10, 0x10),
        name: "Hot Solo",
        script_type: 34,
        mix: MixMode::InPlace,
        knobs: crate::module::KNOBS.split_at(10).0,
        build: |v| {
            let [
                pre_normal,
                pre_overdrive,
                master,
                presence,
                depth,
                treble,
                mid,
                bass,
                overdrive,
                mono,
            ] = *v
            else {
                return None;
            };
            Some(ir::EffectKind::AmpStack(amp(HotSolo {
                pre_normal,
                pre_overdrive,
                master,
                presence,
                depth,
                treble,
                mid,
                bass,
                overdrive: flag(overdrive),
                mono: flag(mono),
            })))
        },
    };

    #[must_use]
    pub fn model(s: HotSolo) -> ir::KnobModel {
        MODULE.knob_model(&[
            s.pre_normal,
            s.pre_overdrive,
            s.master,
            s.presence,
            s.depth,
            s.treble,
            s.mid,
            s.bass,
            bool_f(s.overdrive),
            bool_f(s.mono),
        ])
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 10] {
        [
            knob_row("$ENGINE_PAR_HS_PRENORMAL", 0, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_PREOVERDRIVE", 1, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_MASTER", 2, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_PRESENCE", 3, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_DEPTH", 4, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_TREBLE", 5, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_MID", 6, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_BASS", 7, UNIT_LAW),
            knob_row("$ENGINE_PAR_HS_OVERDRIVE", 8, RAW_LAW),
            knob_row("$ENGINE_PAR_HS_MONO", 9, RAW_LAW),
        ]
    }
}

/// Bass Pro (`0x61`, v`0x10`, `$EFFECT_TYPE_BASSPRO` = 47).
///
/// Saved record (73 bytes, 17 floats + 5 bools): `gain`, `bass`, `mid`,
/// `midfreq`, `treble`, `drive`, `master`, 9 graphic EQ bands (`40..10k` Hz),
/// `geq_volume`, `ultra_lo`, `ultra_hi`, `bright`, `geq`, `mono`.
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
        let mut geq_bands = [0.5; 9];
        for (i, slot) in geq_bands.iter_mut().enumerate() {
            *slot = f32_le(public, 28 + i * 4)?;
        }
        Some(BassPro {
            gain: f32_le(public, 0)?,
            bass: f32_le(public, 4)?,
            mid: f32_le(public, 8)?,
            midfreq: f32_le(public, 12)?,
            treble: f32_le(public, 16)?,
            drive: f32_le(public, 20)?,
            master: f32_le(public, 24)?,
            geq_bands,
            geq_volume: f32_le(public, 64)?,
            ultra_lo: *public.get(68)? != 0,
            ultra_hi: *public.get(69)? != 0,
            bright: *public.get(70)? != 0,
            geq: *public.get(71)? != 0,
            mono: *public.get(72)? != 0,
        })
    }

    #[must_use]
    pub fn amp(s: BassPro) -> ir::AmpStack {
        let g = unit(s.gain);
        let mst = unit(s.master);
        let geq_db = s.geq_bands.map(|b| eq_db(b, 12.0));
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
            geq_db,
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
                ultra_lo: flag(ultra_lo),
                ultra_hi: flag(ultra_hi),
                bright: flag(bright),
                geq: flag(geq),
                mono: flag(mono),
            })))
        },
    };

    #[must_use]
    pub fn model(s: BassPro) -> ir::KnobModel {
        MODULE.knob_model(&[
            s.gain,
            s.bass,
            s.mid,
            s.midfreq,
            s.treble,
            s.drive,
            s.master,
            s.geq_bands[0],
            s.geq_bands[1],
            s.geq_bands[2],
            s.geq_bands[3],
            s.geq_bands[4],
            s.geq_bands[5],
            s.geq_bands[6],
            s.geq_bands[7],
            s.geq_bands[8],
            s.geq_volume,
            bool_f(s.ultra_lo),
            bool_f(s.ultra_hi),
            bool_f(s.bright),
            bool_f(s.geq),
            bool_f(s.mono),
        ])
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 22] {
        [
            knob_row("$ENGINE_PAR_BASSPRO_GAIN", 0, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_BASS", 1, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_MID", 2, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_MIDFREQ", 3, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_TREBLE", 4, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_DRIVE", 5, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_MASTER", 6, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_40", 7, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_90", 8, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_180", 9, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_300", 10, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_500", 11, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_1K", 12, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_2K", 13, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_4K", 14, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_10K", 15, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ_VOLUME", 16, UNIT_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_ULTRALO", 17, RAW_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_ULTRAHI", 18, RAW_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_BRIGHT", 19, RAW_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_GEQ", 20, RAW_LAW),
            knob_row("$ENGINE_PAR_BASSPRO_MONO", 21, RAW_LAW),
        ]
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
        twang::MODULE.slot_type_model(twang::model(twang::INIT)),
        jump::MODULE.slot_type_model(jump::model(jump::INIT)),
        van51::MODULE.slot_type_model(van51::model(van51::INIT)),
        ac_box::MODULE.slot_type_model(ac_box::model(ac_box::INIT)),
        hot_solo::MODULE.slot_type_model(hot_solo::model(hot_solo::INIT)),
        bass_pro::MODULE.slot_type_model(bass_pro::model(bass_pro::INIT)),
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
        raw.push(1);
        raw.push(0);
        let old = twang::saved(0x50, &raw).unwrap();
        let new = twang::saved(0x51, &raw).unwrap();
        assert!(old.bright && old.mono);
        assert!(new.bright && !new.mono);
    }

    #[test]
    fn all_six_amp_models_roundtrip_and_decode_saved() {
        let m = twang::model(twang::INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::AmpStack(twang::amp(twang::INIT)))
        );
        let m = jump::model(jump::INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::AmpStack(jump::amp(jump::INIT)))
        );
        let m = van51::model(van51::INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::AmpStack(van51::amp(van51::INIT)))
        );
        let m = ac_box::model(ac_box::INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::AmpStack(ac_box::amp(ac_box::INIT)))
        );
        let m = hot_solo::model(hot_solo::INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::AmpStack(hot_solo::amp(hot_solo::INIT)))
        );
        let m = bass_pro::model(bass_pro::INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::AmpStack(bass_pro::amp(bass_pro::INIT)))
        );

        // Hot Solo rejects Bass Invader versions (0x51/0x52).
        let hs_bytes = vec![0u8; 34];
        assert!(hot_solo::saved(0x10, &hs_bytes).is_some());
        assert!(hot_solo::saved(0x51, &hs_bytes).is_none());
    }
}
