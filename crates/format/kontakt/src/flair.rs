//! Flair (`$EFFECT_TYPE_FLAIR`, 42) onto [`ir::Ensemble`]. Measured in
//! Kontakt 8.13.1 (scratch/lanes/shared/modfx-audio/flair.*: 58 impulse
//! steps through the factory INIT preset; modfx-sweep: every knob's text):
//! - up to four comb voices, the first at `1/f(Pitch + 12)` (15.28, 3.81,
//!   0.945 ms at Pitch 24, 48, 72), the others at the [`CHORDS`] ratios;
//! - each voice is `(u + delayed)/2`, its line fed `u = c·x + g·delayed`:
//!   `g = sin(π·fb/2)`, `c = (1 − g)^¼` (echoes within 1 % at 40 and 80 %);
//! - N voices at `1/√N` each (0.25, 0.176, 0.143, 0.125 at Mix 50 %);
//! - Invert flips the echoes and the loop (alternating echoes).

use kontra_ir as ir;

/// A new Flair's knobs as Kontakt reads them back, in [`model`]'s order:
/// mode, chord, invert, rate, rate unit, feedback, amount, width, pitch,
/// damping, voices, detune, mix, offset, scan mode.
pub(crate) const INIT: [f32; 15] = [
    0.0, 0.0, 0.0, 0.360_584, 0.0, 0.4, 1.0, 0.0, 48.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0,
];

/// The four voices' frequency ratios per chord, read off the 24 impulse
/// steps (each within 2 cents of the fraction, chord 22 as measured).
const CHORDS: [[f32; 4]; 24] = {
    const fn r(n: f32, d: f32) -> f32 {
        n / d
    }
    [
        [1.0, r(5.0, 4.0), r(3.0, 2.0), 2.0],
        [1.0, r(5.0, 4.0), r(3.0, 2.0), r(15.0, 8.0)],
        [1.0, r(6.0, 5.0), r(3.0, 2.0), 2.0],
        [1.0, r(6.0, 5.0), r(3.0, 2.0), r(9.0, 5.0)],
        [1.0, r(9.0, 8.0), r(5.0, 4.0), r(3.0, 2.0)],
        [1.0, r(9.0, 8.0), r(3.0, 2.0), 2.0],
        [1.0, r(4.0, 3.0), r(3.0, 2.0), 2.0],
        [1.0, r(11.0, 9.0), r(3.0, 2.0), 2.0],
        [1.0, r(5.0, 4.0), r(3.0, 2.0), r(7.0, 4.0)],
        [1.0, r(7.0, 5.0), r(28.0, 15.0), r(112.0, 45.0)],
        [1.0, r(5.0, 4.0), 2.0, r(12.0, 5.0)],
        [1.0, r(12.0, 11.0), r(9.0, 8.0), r(6.0, 5.0)],
        [1.0, r(9.0, 8.0), r(81.0, 64.0), r(729.0, 512.0)],
        [1.0, r(6.0, 5.0), r(36.0, 25.0), r(216.0, 125.0)],
        [1.0, r(5.0, 4.0), r(25.0, 16.0), r(125.0, 64.0)],
        [1.0, r(4.0, 3.0), r(16.0, 9.0), r(64.0, 27.0)],
        [1.0, r(7.0, 5.0), r(49.0, 25.0), r(343.0, 125.0)],
        [1.0, r(3.0, 2.0), r(9.0, 4.0), r(27.0, 8.0)],
        [1.0, 2.0, 4.0, 8.0],
        [1.0, 2.0, 3.0, 4.0],
        [1.0, 2.0, 4.0, 6.0],
        [1.0, 3.0, 5.0, 7.0],
        [1.0, 1.4414, 1.5489, 1.7288],
        [1.0, 1.0, 1.0, 1.0],
    ]
};

/// Detune 100 % in cents per voice (measured on the major chord).
const DETUNE: [f32; 4] = [42.0, -38.0, -58.0, 64.0];

/// A Thru Zero copy's delay, ×(1 + this·Offset): +0.378 and +0.756 ms on
/// 3.81 ms at Offset 50 and 100 %.
const OFFSET: f32 = 0.198;

/// Amount 100 % swings the delay this far either side of its rest: the
/// widest measured over eight LFO samples (0.04× to 1.85×).
const SWING: f32 = 0.96;

/// Scan: the window's half-width in voices and its peak, from the steps at
/// Voices 0, ⅓, ½ and 1 (0.213 on a voice, 0.039 on each neighbour, 0.148
/// on two half way, of a lone voice's 0.25).
const SCAN_WIDTH: f32 = 1.345;
const SCAN_PEAK: f32 = 0.852;

/// The comb at knob positions `p` (see [`INIT`]).
pub(crate) fn flair(p: [f32; 15]) -> ir::EffectKind {
    let [
        mode,
        chord,
        invert,
        rate,
        _unit,
        feedback,
        amount,
        width,
        pitch,
        damping,
        voices,
        detune,
        mix,
        offset,
        scan_mode,
    ] = p;
    let [
        rate,
        feedback,
        amount,
        width,
        damping,
        voices,
        detune,
        mix,
        offset,
    ] = [
        rate, feedback, amount, width, damping, voices, detune, mix, offset,
    ]
    .map(|x| x.clamp(0.0, 1.0));
    let (thru_zero, scan) = (mode.round() as i32 == 1, mode.round() as i32 >= 2);
    let invert = invert > 0.0;
    // Its text: 0.01 to 16.4 Hz, `16.4·e^(7.96(x − 1))`. ponytail: synced
    // units are unmeasured; Hz as without.
    let hz = 16.4 * (7.96 * (rate - 1.0)).exp();
    let root_ms = 1000.0 / (440.0 * 2f32.powf((pitch.round().clamp(0.0, 96.0) - 57.0) / 12.0));
    let ratios = CHORDS
        .get(chord.round().max(0.0) as usize)
        .copied()
        .unwrap_or([1.0; 4]);
    let g = (std::f32::consts::FRAC_PI_2 * feedback).sin();
    let c = (1.0 - g).sqrt().sqrt();
    // ponytail: two points (18.6 kHz at 50 %, 7.4 kHz at 100 %, a one-pole
    // in the loop fitted to the echoes); between and below, this curve.
    let damping_hz = if damping > 0.0 {
        46_800.0 * 0.158f32.powf(damping)
    } else {
        f32::INFINITY
    };
    // Voices: 1 + 3·x of them, the last fading in; Scan runs all four
    // under its window instead.
    let count = 1.0 + 3.0 * voices;
    let gains: [f32; 4] = std::array::from_fn(|k| match scan {
        true => SCAN_PEAK,
        false => (count - k as f32).clamp(0.0, 1.0),
    });
    let norm = match scan {
        true => 1.0,
        false => 1.0 / gains.iter().map(|x| x * x).sum::<f32>().sqrt(),
    };
    // Width: both lines take both inputs, out as sum and difference, the
    // right LFO half a cycle on (two copies at rest·(1 ∓ 0.56), heard in
    // both channels). ponytail: measured at 0 and 100 % only, linear between.
    let half = 0.5 * width;
    let input = [[c * (1.0 - half), c * half], [c * half, c * (1.0 - half)]];
    let side = std::f32::consts::FRAC_1_SQRT_2 * width;
    let stage = |k: f32| {
        [
            [k * (1.0 - width + side), k * side],
            [k * side, k * (1.0 - width - side)],
        ]
    };
    let (sign, copies) = match (invert, thru_zero) {
        (true, false) => (-1.0, 1.0),
        _ => (1.0, if thru_zero { 0.5 } else { 1.0 }),
    };
    let voice = |k: usize, copy: bool| {
        let (Some(&ratio), Some(&gain), Some(&cents)) =
            (ratios.get(k), gains.get(k), DETUNE.get(k))
        else {
            return ir::EnsembleVoice::SILENT;
        };
        let rest = root_ms / ratio * 2f32.powf(-cents * detune / 1200.0);
        let level = mix * norm * gain * copies;
        // Thru Zero's copy rests unswept, `Offset` along; inverted, it
        // cancels its voice.
        let (rest, swing, polarity) = match copy {
            true => (
                rest * (1.0 + OFFSET * offset),
                0.0,
                if invert { -1.0 } else { 1.0 },
            ),
            false if scan => (rest, 0.0, 1.0),
            false => (rest, SWING * amount * rest, 1.0),
        };
        if level == 0.0 {
            return ir::EnsembleVoice::SILENT;
        }
        ir::EnsembleVoice {
            ms: rest - swing,
            depth_ms: 2.0 * swing,
            hz,
            start: 0.0,
            phase: half,
            damping_hz,
            low_cut_hz: 0.0,
            input,
            feedback: sign * g,
            output: stage(polarity * sign * level * (1.0 + g) / 2.0),
            direct: stage(polarity * level * c / 2.0),
        }
    };
    let lines = std::array::from_fn(|v| match (v < 4, thru_zero) {
        (true, _) => voice(v, false),
        (false, true) => voice(v - 4, true),
        (false, false) => ir::EnsembleVoice::SILENT,
    });
    let shape = match scan_mode.round() as i32 {
        1 => ir::ScanShape::SawUp,
        2 => ir::ScanShape::SawDown,
        _ => ir::ScanShape::Triangle,
    };
    // ponytail: the scan's depth (all four voices at Amount 100 %) and its
    // phase are inferred; three shapes seen at one moment each.
    let scan = scan.then_some(ir::Scan {
        voices: 4,
        width: SCAN_WIDTH,
        position: 3.0 * voices,
        depth: 4.0 * amount,
        hz,
        shape,
    });
    ir::EffectKind::Ensemble(ir::Ensemble {
        voices: lines,
        dry: 1.0 - mix,
        scan,
    })
}

/// A Flair's knobs as a script moves them: every move rebuilds it from all.
pub(crate) fn model() -> ir::KnobModel {
    ir::KnobModel {
        knobs: (0..15).map(ir::SlotKnob::Knob).collect(),
        saved: INIT.into(),
        build: |p| Some(flair(p.try_into().ok()?)),
    }
}

/// What `$EFFECT_TYPE_FLAIR` (42) loads, as Morphology Evolved, Conflux
/// and Shreddage load it: [`INIT`], every knob through [`model`].
pub(crate) fn slot_type() -> ir::SlotType {
    let effect = ir::Effect {
        kind: flair(INIT),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    ir::SlotType {
        id: 42,
        subtype: false,
        reads: 0,
        kind: 42,
        effect: Some(effect),
        laws: [].into(),
        model: Some(model()),
    }
}

/// The KSP names of [`model`]'s knobs: a straight 0..1 for the continuous
/// ones; mode, chord, pitch (a note), the unit and the switches are raw.
pub(crate) fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 15] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
    [
        ("$ENGINE_PAR_FLAIR_MODE", Slot(Knob(0)), raw()),
        ("$ENGINE_PAR_FLAIR_CHORD", Slot(Knob(1)), raw()),
        ("$ENGINE_PAR_FLAIR_INVERT_PHASE", Slot(Knob(2)), raw()),
        ("$ENGINE_PAR_FLAIR_RATE", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_FLAIR_RATE_UNIT", Slot(Knob(4)), raw()),
        ("$ENGINE_PAR_FLAIR_FEEDBACK", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_FLAIR_AMOUNT", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_FLAIR_WIDTH", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_FLAIR_PITCH", Slot(Knob(8)), raw()),
        ("$ENGINE_PAR_FLAIR_DAMPING", Slot(Knob(9)), unit()),
        ("$ENGINE_PAR_FLAIR_VOICES", Slot(Knob(10)), unit()),
        ("$ENGINE_PAR_FLAIR_DETUNE", Slot(Knob(11)), unit()),
        ("$ENGINE_PAR_FLAIR_MIX", Slot(Knob(12)), unit()),
        ("$ENGINE_PAR_FLAIR_OFFSET", Slot(Knob(13)), unit()),
        ("$ENGINE_PAR_FLAIR_SCANMODE", Slot(Knob(14)), raw()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kontakt 8.13.1 with Amount and Feedback 0, Mix 50 %, all four voices
    /// of chord 0: the root comb at 3.82 ms (C4, Pitch 48 + 12), the major
    /// chord's 5/4, 3/2 and 2 above it, each echo 0.125, and the input at
    /// 0.5 + 0.5·0.5·4·0.25 = 1.0.
    #[test]
    fn four_voices_play_the_measured_chord() {
        let mut p = INIT;
        (p[5], p[6], p[10]) = (0.0, 0.0, 1.0);
        let ir::EffectKind::Ensemble(e) = flair(p) else {
            panic!("an ensemble");
        };
        let ms: Vec<f32> = e.voices.iter().take(4).map(|v| v.ms).collect();
        let want = [3.822, 3.058, 2.548, 1.911];
        assert!(
            ms.iter().zip(want).all(|(a, b)| (a - b).abs() < 2e-3),
            "{ms:?}"
        );
        assert!(
            e.voices
                .iter()
                .take(4)
                .all(|v| (v.output[0][0] - 0.125).abs() < 1e-6)
        );
        let input = e.dry + e.voices.iter().map(|v| v.direct[0][0]).sum::<f32>();
        assert!((input - 1.0).abs() < 1e-5, "{input}");
    }
}
