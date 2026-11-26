//! Kolor (module `0x79`, saved v`0x10`, `$EFFECT_TYPE_KOLOR` = 70) onto
//! [`ir::ColorSat`].
//!
//! Saved record (49 bytes, `I + F*11 + B`): `mode` (`0..=9`, matching
//! `$NI_KOLOR_*`), `drive`, `boost_level`, `bass_saver`, `lpf`, `hpf`, `bass`,
//! `mid`, `mid_freq`, `mid_q`, `treble`, `mix`, `boost`.

use kontra_ir as ir;

use crate::module::{MixMode, Module, RAW_LAW, UNIT_LAW, f32_le, i32_le, ids, knob_row};

/// Saved fields in record order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kolor {
    pub mode: i32,
    pub drive: f32,
    pub boost_level: f32,
    pub bass_saver: f32,
    pub lpf: f32,
    pub hpf: f32,
    pub bass: f32,
    pub mid: f32,
    pub mid_freq: f32,
    pub mid_q: f32,
    pub treble: f32,
    pub mix: f32,
    pub boost: bool,
}

/// Fresh INIT defaults for a script-loaded Kolor saturator.
pub const INIT: Kolor = Kolor {
    mode: 0,
    drive: 0.4,
    boost_level: 0.5,
    bass_saver: 0.0,
    lpf: 1.0,
    hpf: 0.0,
    bass: 0.5,
    mid: 0.5,
    mid_freq: 0.5,
    mid_q: 0.35,
    treble: 0.5,
    mix: 1.0,
    boost: false,
};

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Kolor> {
    if version != 0x10 {
        return None;
    }
    Some(Kolor {
        mode: i32_le(public, 0)?,
        drive: f32_le(public, 4)?,
        boost_level: f32_le(public, 8)?,
        bass_saver: f32_le(public, 12)?,
        lpf: f32_le(public, 16)?,
        hpf: f32_le(public, 20)?,
        bass: f32_le(public, 24)?,
        mid: f32_le(public, 28)?,
        mid_freq: f32_le(public, 32)?,
        mid_q: f32_le(public, 36)?,
        treble: f32_le(public, 40)?,
        mix: f32_le(public, 44)?,
        boost: *public.get(48)? != 0,
    })
}

/// Physical IR settings for `k`.
#[must_use]
pub fn color_sat(k: Kolor) -> ir::ColorSat {
    let unit = |x: f32| x.clamp(0.0, 1.0);
    let exp = |x: f32, lo: f32, hi: f32| lo * (hi / lo).powf(unit(x));
    let eq = |x: f32| (unit(x) * 2.0 - 1.0) * 12.0;
    ir::ColorSat {
        character: k.mode.clamp(0, 9) as u8,
        drive: unit(k.drive),
        boost: k.boost,
        boost_db: unit(k.boost_level) * 24.0,
        bass_saver: unit(k.bass_saver),
        lpf_hz: exp(k.lpf, 10.0, 20_000.0),
        hpf_hz: exp(k.hpf, 10.0, 20_000.0),
        bass_db: eq(k.bass),
        mid_db: eq(k.mid),
        mid_hz: exp(k.mid_freq, 150.0, 6_000.0),
        mid_q: exp(k.mid_q, 0.4, 6.0),
        treble_db: eq(k.treble),
        mix: unit(k.mix),
    }
}

/// Single-encoding module descriptor for Kolor (`0x79`).
pub static MODULE: Module = Module {
    id: ids::KOLOR,
    module_id: 0x79,
    versions: (0x10, 0x10),
    name: "Kolor",
    script_type: 70,
    mix: MixMode::OwnMix,
    knobs: crate::module::KNOBS.split_at(13).0,
    build: |v| {
        let [
            mode,
            drive,
            boost_level,
            bass_saver,
            lpf,
            hpf,
            bass,
            mid,
            mid_freq,
            mid_q,
            treble,
            mix,
            boost,
        ] = *v
        else {
            return None;
        };
        Some(ir::EffectKind::ColorSat(color_sat(Kolor {
            mode: mode.round() as i32,
            drive,
            boost_level,
            bass_saver,
            lpf,
            hpf,
            bass,
            mid,
            mid_freq,
            mid_q,
            treble,
            mix,
            boost: boost >= 0.5,
        })))
    },
};

#[must_use]
pub fn model(k: Kolor) -> ir::KnobModel {
    MODULE.knob_model(&[
        k.mode as f32,
        k.drive,
        k.boost_level,
        k.bass_saver,
        k.lpf,
        k.hpf,
        k.bass,
        k.mid,
        k.mid_freq,
        k.mid_q,
        k.treble,
        k.mix,
        if k.boost { 1.0 } else { 0.0 },
    ])
}

/// KSP `$ENGINE_PAR_KOLOR_*` knob rows.
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 13] {
    [
        knob_row("$ENGINE_PAR_KOLOR_MODE", 0, RAW_LAW),
        knob_row("$ENGINE_PAR_KOLOR_DRIVE", 1, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_BOOST_LEVEL", 2, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_BASS_SAVER", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_LPF", 4, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_HPF", 5, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_BASS", 6, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_MID", 7, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_MID_FREQ", 8, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_MID_Q", 9, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_TREBLE", 10, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_MIX", 11, UNIT_LAW),
        knob_row("$ENGINE_PAR_KOLOR_BOOST", 12, RAW_LAW),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_decodes_all_fields_and_roundtrips() {
        let mut bytes = 5i32.to_le_bytes().to_vec();
        for f in [0.7f32, 0.6, 0.3, 0.8, 0.2, 0.55, 0.45, 0.6, 0.5, 0.65, 0.9] {
            bytes.extend(f.to_le_bytes());
        }
        bytes.push(1);
        let k = saved(0x10, &bytes).unwrap();
        assert_eq!(k.mode, 5);
        assert!(k.boost);
        assert_eq!(color_sat(k).character, 5);

        let m = model(INIT);
        assert_eq!(
            m.build(&m.saved),
            Some(ir::EffectKind::ColorSat(color_sat(INIT)))
        );
    }
}
