//! The Solid G-EQ's and Solid Bus Comp's script knob models (`$ENGINE_PAR_SEQ_*`, `$ENGINE_PAR_SCOMP_*`).

use kontra_ir as ir;

/// The Solid G-EQ's knobs and bell switches: every move rebuilds the four
/// bands from all of them through [`crate::laws::solid_eq`], as the core
/// couples them (a shelf's gain also moves its corner). `saved` is LF gain
/// and Hz, LMF and HMF gain, Hz and Q, HF gain and Hz (each 0..=1, the KSP
/// value in millionths), then the LF and HF bell switches (0 or 1).
pub(crate) fn solid_model(saved: [f32; 10], bells: [bool; 2]) -> ir::KnobModel {
    use ir::SlotKnob::{BandBell, BandGain, BandHz, BandWidth};
    let mut positions = saved.to_vec();
    positions.extend(bells.map(|on| f32::from(u8::from(on))));
    ir::KnobModel {
        knobs: [
            BandGain(0),
            BandHz(0),
            BandGain(1),
            BandHz(1),
            BandWidth(1),
            BandGain(2),
            BandHz(2),
            BandWidth(2),
            BandGain(3),
            BandHz(3),
            BandBell(0),
            BandBell(3),
        ]
        .into(),
        saved: positions.into(),
        build: |p| {
            let [lg, lh, mg, mh, mq, hg, hh, hq, fg, fh, lb, hb] = *p else {
                return None;
            };
            let bell = |on: f32, shelf| if on > 0.5 { ir::Shape::Peak } else { shelf };
            let (lf, hf) = (
                bell(lb, ir::Shape::LowShelf),
                bell(hb, ir::Shape::HighShelf),
            );
            let bands =
                crate::laws::solid_eq([lg, lh], lf, [mg, mh, mq], [hg, hh, hq], [fg, fh], hf);
            Some(ir::EffectKind::Eq(bands))
        },
    }
}

/// The Solid Bus Comp's knobs: every move rebuilds the compressor from
/// all six and the link through [`crate::laws::bus_compressor`] (the ratio
/// switch also moves the threshold).
pub(crate) fn bus_model(saved: [f32; 6], link: bool) -> ir::KnobModel {
    use ir::SlotKnob::{Attack, Makeup, Mix, Ratio, Release, Threshold};
    let mut positions = saved.to_vec();
    positions.push(f32::from(u8::from(link)));
    ir::KnobModel {
        knobs: [Threshold, Makeup, Attack, Release, Ratio, Mix].into(),
        saved: positions.into(),
        build: |p| {
            let (knobs, link) = (p.get(..6)?.try_into().ok()?, *p.get(6)? > 0.5);
            Some(ir::EffectKind::Compressor(
                crate::laws::bus_compressor(knobs, link).0,
            ))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bell_switch_turns_the_lf_shelf_into_a_peak() {
        let model = solid_model([0.5; 10], [false, false]);
        let lf = |p: &[f32]| match (model.build)(p) {
            Some(ir::EffectKind::Eq(bands)) => bands[0].shape,
            _ => panic!(),
        };
        let mut p = model.saved.to_vec();
        assert_eq!(lf(&p), ir::Shape::LowShelf);
        p[10] = 1.0;
        assert_eq!(lf(&p), ir::Shape::Peak);
    }
}
