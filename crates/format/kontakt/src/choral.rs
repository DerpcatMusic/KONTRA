//! Choral (`$EFFECT_TYPE_CHORAL`, 43) onto [`ir::Ensemble`]. Measured in
//! Kontakt 8.13.1 (scratch/lanes/shared/modfx-audio/choral*: impulses
//! through the factory INIT preset; modfx-sweep: every knob's text); the
//! per-mode voice tables are Kontakt's own (0x14193cd70, read for v2):
//! - six voices in pairs, each a delay line swept by its own sine, turned
//!   in the stereo field by Width, summed at `mix·input·norm·0.707`;
//! - every voice also passes its input straight through, so Mix 100 % keeps
//!   an undelayed share (measured 0.50 at Width 100 %, one voice);
//! - first tap `G0·(5·Delay + 0.1)` s (mode 0: 0.4 + 15·Delay ms; modes 1–3
//!   at 7.15, 11.08, 7.06 ms, all `G0[0]`);
//! - the sweep spans `(R − 1)/(2·Hz)` s, `R = 2^(0.025·(0.99·a² + 0.01))`
//!   (33.6 ms measured at Amount 100 %, 0.25 Hz).

use kontra_ir as ir;

/// A new Choral's knobs as Kontakt reads them back, in [`model`]'s order:
/// rate, mode, amount, voices, delay, width, feedback, scatter, invert, mix.
pub(crate) const INIT: [f32; 10] = [0.0, 0.0, 0.494_95, 0.0, 0.18, 0.5, 0.0, 0.0, 0.0, 0.995];

/// Per mode and voice: delay scale (s), LFO rate scale, LFO start
/// (cycles), filter corner (Hz), and whether that filter is a high-pass.
type Mode = ([f32; 6], [f32; 6], [f32; 6], [f32; 6], [bool; 6]);

const MODES: [Mode; 4] = [
    (
        [0.003, 0.003, 0.007, 0.007, 0.011, 0.011],
        [1.0, 1.0, 1.428_571, 1.43, 1.724_2, 1.727_65],
        [0.0, 0.5, 0.0, 0.5, 0.0, 0.5],
        [5000.0, 5000.0, 5000.0, 5000.0, 10_000.0, 10_000.0],
        [false; 6],
    ),
    (
        [0.007, 0.007, 0.007, 0.007, 0.013, 0.013],
        [1.0; 6],
        [0.0, 0.5, 0.333_3, 0.833_3, 0.666_7, 0.166_7],
        [4000.0, 4000.0, 6000.0, 6000.0, 10_000.0, 10_000.0],
        [false; 6],
    ),
    (
        [0.011; 6],
        [1.0; 6],
        [0.0, 0.5, 0.5, 0.0, 0.25, 0.75],
        [12_000.0, 12_000.0, 150.0, 150.0, 20_000.0, 20_000.0],
        [false, false, true, true, false, false],
    ),
    (
        [0.007, 0.007, 0.011, 0.011, 0.013, 0.013],
        [1.0, 1.0, 1.428_571, 1.428_571, 1.724_2, 1.724_2],
        [0.0, 0.0, 0.0, 0.25, 0.0, 0.25],
        [20_000.0; 6],
        [false; 6],
    ),
];

/// The chorus at knob positions `p` (see [`INIT`]).
pub(crate) fn choral(p: [f32; 10]) -> ir::EffectKind {
    let [
        rate,
        mode,
        amount,
        voices,
        delay,
        width,
        feedback,
        _scatter,
        invert,
        mix,
    ] = p;
    let [rate, amount, voices, delay, width, feedback, mix] =
        [rate, amount, voices, delay, width, feedback, mix].map(|x| x.clamp(0.0, 1.0));
    let [m0, m1, m2, m3] = MODES;
    let (scale, speeds, starts, corners, high_pass) = match mode.round() as i32 {
        ..=0 => m0,
        1 => m1,
        2 => m2,
        _ => m3,
    };
    let [first, ..] = scale;
    let hz = 7.4 * rate.powi(3) + 0.25;
    let ratio = 2f32.powf(0.025 * (0.99 * amount * amount + 0.01));
    let span = 5.0 * delay + 0.1;
    // Feedback: g = fb^¼·0.999, the input eased by (1 − min(g, .99)^4)^¼.
    let g = feedback.sqrt().sqrt() * 0.999;
    let input = (1.0 - g.min(0.99).powi(4)).sqrt().sqrt();
    // Pairs 2/3 and 4/5 fade in over Voices' two halves.
    let fade = |k: f32| (2.0 * voices - k).clamp(0.0, 1.0).sqrt();
    let gains = [1.0, 1.0, fade(0.0), fade(0.0), fade(1.0), fade(1.0)];
    let sum: f32 = gains.iter().sum();
    let wet = mix * input * (0.2 * sum + 0.6).sqrt() / sum * 0.707;
    let sign = if invert > 0.0 { -1.0 } else { 1.0 };
    let mut direct = 1.0 - mix;
    let voices = std::array::from_fn(|v| {
        let at = |t: [f32; 6]| t.get(v).copied().unwrap_or_default();
        // Pair k turns by ∓ Width·π/4·(1 − 2k/7), even voices left.
        let reach = 1.0 - (v / 2 * 2) as f32 / 7.0;
        let side = if v % 2 == 0 { -1.0 } else { 1.0 };
        let angle = side * width * std::f32::consts::FRAC_PI_4 * reach;
        let gain = wet * at(gains);
        // The pair's cross terms cancel in the straight-through share.
        direct += gain * input * angle.cos();
        // Longer voices feed back as if over the same time as voice 0.
        let fb = g.powf(at(scale) / first);
        ir::EnsembleVoice {
            ms: 1000.0 * at(scale) * span,
            hz: at(speeds) * hz,
            start: at(starts),
            corner_hz: at(corners),
            high_pass: high_pass.get(v).copied().unwrap_or_default(),
            level: gain * sign * (1.0 + fb),
            angle,
        }
    });
    ir::EffectKind::Ensemble(ir::Ensemble {
        voices,
        // The sweep `(R − 1)/(2·Hz)` s for every voice's rate.
        sweep: 500.0 * (ratio - 1.0),
        input,
        feedback: sign * g,
        direct,
    })
}

/// A Choral's knobs as a script moves them: every move rebuilds it from all.
pub(crate) fn model() -> ir::KnobModel {
    ir::KnobModel {
        knobs: (0..10).map(ir::SlotKnob::Knob).collect(),
        saved: INIT.into(),
        build: |p| Some(choral(p.try_into().ok()?)),
    }
}

/// What `$EFFECT_TYPE_CHORAL` (43) loads, as Morphology Evolved, Conflux
/// and Shreddage load it: [`INIT`], every knob through [`model`].
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: choral(INIT),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 43,
        subtype: false,
        reads: 0,
        kind: 43,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs: a straight 0..1 for the continuous
/// ones; mode (0..3) and the switches are set as raw values.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 10] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_CHORAL_RATE", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_CHORAL_MODE", Slot(Knob(1)), raw()),
        ("$ENGINE_PAR_CHORAL_AMOUNT", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_CHORAL_VOICES", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_CHORAL_DELAY", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_CHORAL_WIDTH", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_CHORAL_FEEDBACK", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_CHORAL_SCATTER", Slot(Knob(7)), raw()),
        ("$ENGINE_PAR_CHORAL_INVERT_PHASE", Slot(Knob(8)), raw()),
        ("$ENGINE_PAR_CHORAL_MIX", Slot(Knob(9)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_choral_is_kontakts_init() {
        // Kontakt 8.13.1 at INIT: one voice pair, first tap 3 ms·(5·0.18 + 0.1),
        // 0.25 Hz, half a cycle apart; Mix 99.5 % leaves the pair's
        // straight-through share, 0.354·2·cos(π/8).
        let ir::EffectKind::Ensemble(e) = choral(INIT) else {
            panic!("an ensemble");
        };
        let [a, b, c, ..] = e.voices;
        assert!(
            (a.ms - 3.0).abs() < 1e-3 && (b.ms - 3.0).abs() < 1e-3,
            "{}",
            a.ms
        );
        assert!((a.hz - 0.25).abs() < 1e-6 && (b.start - 0.5).abs() < 1e-6);
        assert_eq!(c.level, 0.0);
        let share = 0.005 + 0.995 * 0.354 * 2.0 * (std::f32::consts::PI / 8.0).cos();
        assert!((e.direct - share).abs() < 2e-3, "{}", e.direct);
    }
}
