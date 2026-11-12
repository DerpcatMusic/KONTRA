//! Phasis (`$EFFECT_TYPE_PHASIS`, 41) onto [`ir::Notches`]. Measured in
//! Kontakt 8.13.1 (scratch/lanes/shared/modfx-audio/phasis.*: impulses
//! through the factory INIT preset; modfx-sweep: every knob's text):
//! `NOTCHES` second-order allpasses at the centre summed with the input, one
//! notch each; Spread is their Q, `½·6.48^(1 − 2·spread)` (0.002 octave per
//! notch at spread 0, ¼, ½, ¾, 1); Mix blends that sum with the input;
//! `INVERT_PHASE` inverts the allpasses (notches and peaks swap; the
//! manual's "Invert Mod Mix" text describes this one); Amount 100 % swings
//! the centre ±4.6 octaves.

use kontra_ir as ir;

/// A new Phasis's knobs as Kontakt reads them back, in [`model`]'s order:
/// rate, rate unit, Ultra, amount, centre, stereo, spread, feedback, mod
/// mix, notches, invert phase, invert mod mix, mix.
pub(crate) const INIT: [f32; 13] = [
    0.391_294, 0.0, 0.0, 1.0, 0.499_922, 0.5, 0.5, 0.210_526, 0.0, 2.0, 0.0, 1.0, 1.0,
];

/// The notch phaser at knob positions `p` (see [`INIT`]).
pub(crate) fn phasis(p: [f32; 13]) -> ir::EffectKind {
    let [
        rate,
        _unit,
        ultra,
        amount,
        centre,
        stereo,
        spread,
        feedback,
        mod_mix,
        notches,
        invert,
        invert_mod,
        mix,
    ] = p;
    let [rate, amount, centre, stereo, spread, feedback, mod_mix, mix] =
        [rate, amount, centre, stereo, spread, feedback, mod_mix, mix].map(|x| x.clamp(0.0, 1.0));
    // Centre: 46 Hz·184^x measured (169, 622, 2,273 Hz at ¼, ½, ¾); with
    // Ultra on, its text (830.6 Hz at ½, measured the same).
    let hz = if ultra > 0.0 {
        1.4 * (12.75 * centre).exp()
    } else {
        46.0 * 184f32.powf(centre)
    };
    // ponytail: the stereo knob's phase law is unmeasured (one channel
    // recorded); ±¼ cycle at its ends.
    let polarity = if invert_mod > 0.0 { -1.0 } else { 1.0 };
    ir::EffectKind::Notches(ir::Notches {
        sections: notches.round().clamp(2.0, 12.0) as u8,
        hz,
        q: 0.5 * 6.48f32.powf(1.0 - 2.0 * spread),
        octaves: 4.6 * amount * (1.0 - mod_mix),
        q_octaves: polarity * 6.48f32.log2() * amount * mod_mix,
        // Its text is 0..95 %. ponytail: one loop round the sections at that
        // gain matches Kontakt within 0.5 dB to 25 %; at 100 % Kontakt shows a
        // −16 dB floor with a peak at the centre this loop does not.
        feedback: 0.95 * feedback,
        // Its text: 0.01 Hz to 26 Hz, `26·e^(8.1(x − 1))`. ponytail: synced
        // units and Ultra's faster rates are unmeasured; Hz as without.
        rate: ir::Rate::Hz(26.0 * (8.1 * (rate - 1.0)).exp()),
        phase: 0.5 * (stereo - 0.5),
        mix,
        invert: invert > 0.0,
    })
}

/// A Phasis's knobs as a script moves them: every move rebuilds it from all.
pub(crate) fn model() -> ir::KnobModel {
    ir::KnobModel {
        knobs: (0..13).map(ir::SlotKnob::Knob).collect(),
        saved: INIT.into(),
        build: |p| Some(phasis(p.try_into().ok()?)),
    }
}

/// What `$EFFECT_TYPE_PHASIS` (41) loads, as Morphology Evolved, Conflux
/// and Shreddage load it: [`INIT`], every knob through [`model`].
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: phasis(INIT),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 41,
        subtype: false,
        reads: 0,
        kind: 41,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs: a straight 0..1 for the continuous
/// ones (their texts are 0..100 % straight lines, or the laws above); the
/// stepped ones (notches 2..12, the switches, the unit) are set as raw
/// values.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 13] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_PHASIS_RATE", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_PHASIS_RATE_UNIT", Slot(Knob(1)), raw()),
        ("$ENGINE_PAR_PHASIS_ULTRA", Slot(Knob(2)), raw()),
        ("$ENGINE_PAR_PHASIS_AMOUNT", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_PHASIS_CENTER", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_PHASIS_STEREO", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_PHASIS_SPREAD", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_PHASIS_FEEDBACK", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_PHASIS_MOD_MIX", Slot(Knob(8)), unit()),
        ("$ENGINE_PAR_PHASIS_NOTCHES", Slot(Knob(9)), raw()),
        ("$ENGINE_PAR_PHASIS_INVERT_PHASE", Slot(Knob(10)), raw()),
        ("$ENGINE_PAR_PHASIS_INVERT_MOD_MIX", Slot(Knob(11)), raw()),
        ("$ENGINE_PAR_PHASIS_MIX", Slot(Knob(12)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_phasis_is_kontakts_init() {
        // Kontakt 8.13.1 reads back two notches, centre 622 Hz, spread 50 %
        // (Q ½), rate 0.19 Hz, feedback 20 %.
        let ir::EffectKind::Notches(n) = phasis(INIT) else {
            panic!("a notch phaser");
        };
        assert_eq!(n.sections, 2);
        assert!((n.hz - 622.0).abs() < 4.0, "{}", n.hz);
        assert!((n.q - 0.5).abs() < 1e-3);
        assert!(matches!(n.rate, ir::Rate::Hz(hz) if (hz - 0.19).abs() < 0.01));
        assert!((n.feedback - 0.2).abs() < 1e-3 && !n.invert);
    }
}
