//! Wow/Flutter (`$EFFECT_TYPE_WOWFLUTTER`, 56) onto a modulated
//! [`ir::Delay`]: a tape's pitch wobble.
//!
//! Measured at INIT in Kontakt 8.13.1 (scratch/laneDSP/b9): a pitch wobble
//! of about 7.6 cents rms at 1.4..1.7 Hz. A sine swing of `D` s at `f` Hz
//! bends pitch by `2πfD`, so 0.7 ms at 1.5 Hz. Wow sets that swing, Speed
//! its rate; Flutter adds a fast shallow swing, so a Flutter-led setting
//! runs at ~10 Hz instead; Age darkens the tape; Stereo offsets the right
//! LFO. Saturation, Scrape and Gate are not played.

use kontra_ir as ir;

/// A new Wow/Flutter's knobs, in [`model`]'s order: wow, flutter, speed,
/// stereo, age.
pub(crate) const INIT: [f32; 5] = [0.5, 0.3, 0.5, 0.5, 0.3];

/// The delay at knob positions `p` (see [`INIT`]).
pub(crate) fn wow_flutter(p: [f32; 5]) -> ir::Delay {
    let [wow, flutter, speed, stereo, age] = p.map(|x| x.clamp(0.0, 1.0));
    let wow_ms = 1.4 * wow;
    let flutter_ms = 0.1 * flutter;
    // One LFO: the larger pitch bend (2πfD) sets which swing is heard.
    let wow_hz = 1.5 * 4f32.powf(speed - 0.5);
    let flutter_hz = 10.0 * 2f32.powf(speed - 0.5);
    let (depth_ms, hz) = match wow_ms * wow_hz >= flutter_ms * flutter_hz {
        true => (wow_ms + flutter_ms, wow_hz),
        false => (flutter_ms + wow_ms * wow_hz / flutter_hz, flutter_hz),
    };
    ir::Delay {
        time: ir::DelayTime::Ms(5.0),
        depth_ms,
        second_head: false,
        input: ir::IDENTITY,
        feedback: [[0.0; 2]; 2],
        output: ir::IDENTITY,
        damping_hz: 18_000.0 * (3_000.0f32 / 18_000.0).powf(age),
        low_cut_hz: 0.0,
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        rate: ir::Rate::Hz(hz),
        phase: 0.25 * stereo,
    }
}

/// A Wow/Flutter's knobs as a script moves them: every move rebuilds it.
pub(crate) fn model() -> ir::KnobModel {
    ir::KnobModel {
        knobs: (0..5).map(ir::SlotKnob::Knob).collect(),
        saved: INIT.into(),
        build: |p| Some(ir::EffectKind::Delay(wow_flutter(p.try_into().ok()?))),
    }
}

/// What `$EFFECT_TYPE_WOWFLUTTER` (56) loads: [`INIT`], fully wet.
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: ir::EffectKind::Delay(wow_flutter(INIT)),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 56,
        subtype: false,
        reads: 0,
        kind: 56,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs, and Mix as the slot's wet share.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        ("$ENGINE_PAR_WOWFLUTTER_WOW", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_FLUTTER", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_SPEED", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_STEREO", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_WOWFLUTTER_AGE", Slot(Knob(4)), unit()),
        (
            "$ENGINE_PAR_WOWFLUTTER_MIX",
            Slot(ir::SlotKnob::Mix),
            unit(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_wobbles_about_eight_cents_at_one_and_a_half_hertz() {
        let d = wow_flutter(INIT);
        let ir::Rate::Hz(hz) = d.rate else {
            panic!("Hz")
        };
        // Peak ratio 2πfD; rms cents = 1200·log2(1 + peak)/√2.
        let peak = std::f32::consts::TAU * hz * d.depth_ms / 1000.0;
        let cents = 1200.0 * (1.0 + peak).log2() / std::f32::consts::SQRT_2;
        assert!(
            (1.4..1.8).contains(&hz) && (6.0..10.0).contains(&cents),
            "{hz} Hz {cents} c"
        );
    }
}
