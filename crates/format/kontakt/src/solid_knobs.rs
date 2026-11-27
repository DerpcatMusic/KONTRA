//! The Solid G-EQ's and Solid Bus Comp's script knob models (`$ENGINE_PAR_SEQ_*`, `$ENGINE_PAR_SCOMP_*`).

use kontra_ir as ir;
use kontra_kontakt_effects::module::{MixMode, Module, ids};

/// Single-encoding module descriptor for Solid G-EQ (`0x44`).
pub(crate) static SOLID_EQ_MODULE: Module = Module {
    id: ids::SOLID_EQ,
    module_id: 0x44,
    versions: (0, u32::MAX),
    name: "Solid G-EQ",
    script_type: 28,
    mix: MixMode::InPlace,
    knobs: &[
        ir::SlotKnob::BandGain(0),
        ir::SlotKnob::BandHz(0),
        ir::SlotKnob::BandGain(1),
        ir::SlotKnob::BandHz(1),
        ir::SlotKnob::BandWidth(1),
        ir::SlotKnob::BandGain(2),
        ir::SlotKnob::BandHz(2),
        ir::SlotKnob::BandWidth(2),
        ir::SlotKnob::BandGain(3),
        ir::SlotKnob::BandHz(3),
        ir::SlotKnob::BandBell(0),
        ir::SlotKnob::BandBell(3),
    ],
    build: |p| {
        let [lg, lh, mg, mh, mq, hg, hh, hq, fg, fh, lb, hb] = *p.get(..12)? else {
            return None;
        };
        let bell = |on: f32, shelf| if on > 0.5 { ir::Shape::Peak } else { shelf };
        let (lf, hf) = (
            bell(lb, ir::Shape::LowShelf),
            bell(hb, ir::Shape::HighShelf),
        );
        let bands = crate::laws::solid_eq([lg, lh], lf, [mg, mh, mq], [hg, hh, hq], [fg, fh], hf);
        if let Some([hp_x, lp_x, hp_on, lp_on]) =
            p.get(12..16).and_then(|s| <[f32; 4]>::try_from(s).ok())
        {
            let hp_hz = (hp_on > 0.5).then(|| kontra_law::FormulaId::SolidEqHpHz.eval(hp_x));
            let lp_hz = (lp_on > 0.5).then(|| kontra_law::FormulaId::SolidEqLpHz.eval(lp_x));
            if hp_hz.is_some() || lp_hz.is_some() {
                return Some(ir::EffectKind::EqPass {
                    bands,
                    hp_hz,
                    lp_hz,
                });
            }
        }
        Some(ir::EffectKind::Eq(bands))
    },
};

/// Single-encoding module descriptor for Solid Bus Comp (`0x46`).
pub(crate) static SOLID_BUS_MODULE: Module = Module {
    id: ids::SOLID_BUS_COMP,
    module_id: 0x46,
    versions: (0, u32::MAX),
    name: "Solid Bus Comp",
    script_type: 29,
    mix: MixMode::OwnMix,
    knobs: &[
        ir::SlotKnob::Threshold,
        ir::SlotKnob::Makeup,
        ir::SlotKnob::Attack,
        ir::SlotKnob::Release,
        ir::SlotKnob::Ratio,
        ir::SlotKnob::Mix,
    ],
    build: |p| {
        let (knobs, link) = (p.get(..6)?.try_into().ok()?, *p.get(6)? > 0.5);
        Some(ir::EffectKind::Compressor(
            crate::laws::bus_compressor(knobs, link).0,
        ))
    },
};

/// The Solid G-EQ's knobs and bell switches: every move rebuilds the four
/// bands from all of them through [`crate::laws::solid_eq`], as the core
/// couples them (a shelf's gain also moves its corner). `saved` is LF gain
/// and Hz, LMF and HMF gain, Hz and Q, HF gain and Hz (each 0..=1, the KSP
/// value in millionths), then the LF and HF bell switches (0 or 1).
pub(crate) fn solid_model(saved: [f32; 10], bells: [bool; 2]) -> ir::KnobModel {
    let [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9] = saved;
    let [b0, b1] = bells.map(|on| f32::from(u8::from(on)));
    SOLID_EQ_MODULE.knob_model(&[p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, b0, b1])
}

/// Solid G-EQ v0x11 model including HP/LP pass-filter frequencies and enables.
pub(crate) fn solid_model_v11(
    saved: [f32; 10],
    bells: [bool; 2],
    pass_freq: [f32; 2],
    pass_on: [bool; 2],
) -> ir::KnobModel {
    let [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9] = saved;
    let [b0, b1] = bells.map(|on| f32::from(u8::from(on)));
    let [hp_f, lp_f] = pass_freq;
    let [hp_on, lp_on] = pass_on.map(|on| f32::from(u8::from(on)));
    SOLID_EQ_MODULE.knob_model(&[
        p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, b0, b1, hp_f, lp_f, hp_on, lp_on,
    ])
}

/// The Solid Bus Comp's knobs: every move rebuilds the compressor from
/// all six and the link through [`crate::laws::bus_compressor`] (the ratio
/// switch also moves the threshold).
pub(crate) fn bus_model(saved: [f32; 6], link: bool) -> ir::KnobModel {
    let [p0, p1, p2, p3, p4, p5] = saved;
    SOLID_BUS_MODULE.knob_model(&[p0, p1, p2, p3, p4, p5, f32::from(u8::from(link))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bell_switch_turns_the_lf_shelf_into_a_peak() {
        let model = solid_model([0.5; 10], [false, false]);
        let lf = |p: &[f32]| match model.build(p) {
            Some(ir::EffectKind::Eq(bands)) => bands[0].shape,
            _ => panic!(),
        };
        let mut p = model.saved.to_vec();
        assert_eq!(lf(&p), ir::Shape::LowShelf);
        p[10] = 1.0;
        assert_eq!(lf(&p), ir::Shape::Peak);
    }
}
