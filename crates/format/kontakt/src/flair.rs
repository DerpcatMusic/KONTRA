//! Flair (`$EFFECT_TYPE_FLAIR`, 42) onto [`ir::CombBank`], with the laws
//! read from Kontakt 8.13.1's `ni::effektrig::dsp::Flair` (setParam
//! `FUN_141961220`, lane writer `FUN_1419614e0`; addresses are `.rdata`
//! constants). Four comb voices at the chord's intervals over
//! `f(Pitch + 12)`, fed back at `0.999·sin(π/2·fb)` made pitch-even.
//!
//! ponytail: the KSP-to-lane map and the raw scaling of Pitch, Chord and
//! Mode are unread: each knob is mapped to its lane by meaning, Pitch as
//! semitones 0..=96, Chord and Mode as indices. Rate Unit (host sync) is
//! unread: the rate is always free Hz.

use kontra_ir as ir;

/// A new Flair's knobs as Kontakt reads them back, in [`model`]'s order:
/// mode, chord, invert, rate, rate unit, feedback, amount, width, pitch,
/// damping, voices, detune, mix, offset, scan mode.
pub(crate) const INIT: [f32; 15] = [
    0.0, 0.0, 0.0, 0.360_584, 0.0, 0.4, 1.0, 0.0, 48.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0,
];

/// Voices 1..=3's intervals per chord, voice 0 always 1 (0x14470bad4);
/// `FUN_141960700`'s float constants (decoded in lane-scripts/flair/re/
/// chords.py, which lists each address).
const CHORDS: [[f32; 3]; 24] = [
    [5.0 / 4.0, 3.0 / 2.0, 2.0],
    [5.0 / 4.0, 3.0 / 2.0, 15.0 / 8.0],
    [6.0 / 5.0, 3.0 / 2.0, 2.0],
    [6.0 / 5.0, 3.0 / 2.0, 9.0 / 5.0],
    [9.0 / 8.0, 5.0 / 4.0, 3.0 / 2.0],
    [9.0 / 8.0, 3.0 / 2.0, 2.0],
    [4.0 / 3.0, 3.0 / 2.0, 2.0],
    [11.0 / 9.0, 3.0 / 2.0, 2.0],
    [5.0 / 4.0, 3.0 / 2.0, 7.0 / 4.0],
    [7.0 / 5.0, 28.0 / 15.0, 112.0 / 45.0],
    [5.0 / 4.0, 2.0, 12.0 / 5.0],
    [12.0 / 11.0, 9.0 / 8.0, 6.0 / 5.0],
    [9.0 / 8.0, 81.0 / 64.0, 729.0 / 512.0],
    [6.0 / 5.0, 36.0 / 25.0, 216.0 / 125.0],
    [5.0 / 4.0, 25.0 / 16.0, 125.0 / 64.0],
    [4.0 / 3.0, 16.0 / 9.0, 64.0 / 27.0],
    [7.0 / 5.0, 49.0 / 25.0, 343.0 / 125.0],
    [3.0 / 2.0, 9.0 / 4.0, 27.0 / 8.0],
    [2.0, 4.0, 8.0],
    [2.0, 3.0, 4.0],
    [2.0, 4.0, 6.0],
    [3.0, 5.0, 7.0],
    [1.441_295_4, 1.550_425_8, 1.729_295_6],
    [1.0006, 1.0009, 1.0012],
];

/// Each voice's detune at 100 %, a frequency factor's slope (`FUN_141961f60`:
/// +40, −40, −60, +60 cents; 0x147699b60, b58, b70, b80).
const DETUNE: [f32; 4] = [0.023_373_9, -0.022_840_0, -0.034_063_7, 0.035_264_9];

/// `f(MIDI 12 + 96p)`: 440·2^((96p − 57)/12) Hz (96 0x144e79a68, 57
/// 0x144714a28, 440 0x1447617a0; `FUN_141961a30`).
fn pitch_hz(p: f32) -> f32 {
    440.0 * ((96.0 * p - 57.0) / 12.0).exp2()
}

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
        invert,
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
        invert, rate, feedback, amount, width, damping, voices, detune, mix, offset,
    ]
    .map(|x| x.clamp(0.0, 1.0));
    let pitch = (pitch / 96.0).clamp(0.0, 1.0);
    // Mode and Scan Mode `int(3p)` clamped 0..2 (cases 0xc, 0xd).
    let mode = match mode.round() as i32 {
        1 => ir::CombMode::ThruZero,
        2.. => ir::CombMode::Scan,
        _ => ir::CombMode::Standard,
    };
    let scan_shape = match scan_mode.round() as i32 {
        1 => ir::ScanShape::SawUp,
        2.. => ir::ScanShape::SawDown,
        _ => ir::ScanShape::Triangle,
    };
    let [a, b, c] = CHORDS
        .get(chord.round().clamp(0.0, 23.0) as usize)
        .copied()
        .unwrap_or([1.0; 3]);
    // Loop gain `0.999·sin(π/2·fb)` (0x144756360, 0x144f38a20) raised to
    // `1/e`, `e = 2^(8·pitch − 4) = F/F(48)` (0x1447149c0, 0x1447149a8): a
    // pass of a higher comb loses less, so all decay alike per second.
    let gain = 0.999 * (std::f32::consts::FRAC_PI_2 * feedback).sin();
    let feedback = gain.powf(1.0 / (8.0 * pitch - 4.0).exp2());
    ir::EffectKind::CombBank(ir::CombBank {
        root_hz: pitch_hz(pitch),
        ratio: [1.0, a, b, c],
        detune: DETUNE.map(|k| 1.0 + k * detune),
        // Pitch 0, the most detuned voice down: the lines' length.
        lowest_hz: pitch_hz(0.0) * (1.0 + DETUNE[2]),
        feedback,
        // Voices: three equal-power fades over thirds (3, 0x144f38a38).
        voices: 3.0 * voices,
        // Damping `0.25·p³` (0x1447148e4).
        damping: 0.25 * damping.powi(3),
        // Amount: the swing `p²` (`FUN_141961030`), the scan's depth `p`.
        depth: amount * amount,
        // Rate `440·2^(11.5p − 16.25)` Hz (0x144fc54e0/e4, case 1).
        rate: ir::Rate::Hz(440.0 * (11.5 * rate - 16.25).exp2()),
        // Offset: the still head `0.2·p` further back (0x144f38948).
        offset: 0.2 * offset,
        invert: invert >= 0.5,
        mode,
        scan_depth: amount,
        scan_shape,
        width,
        mix,
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

    /// INIT: C4 (261.6 Hz), the major chord, Feedback 40 % at Pitch 48
    /// `0.999·sin(0.2π)` a pass, the full swing at 0.1 Hz, Mix 50 %.
    #[test]
    fn init_maps_to_the_binary_laws() {
        let ir::EffectKind::CombBank(c) = flair(INIT) else {
            panic!("a comb bank");
        };
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3 * b.abs().max(1.0);
        assert!(near(c.root_hz, 261.626), "{}", c.root_hz);
        assert_eq!(c.ratio, [1.0, 1.25, 1.5, 2.0]);
        assert_eq!(c.detune, [1.0; 4]);
        assert!(near(c.feedback, 0.587_198), "{}", c.feedback);
        assert!(near(c.lowest_hz, 15.795), "{}", c.lowest_hz);
        assert_eq!(
            (c.voices, c.damping, c.depth, c.offset),
            (0.0, 0.0, 1.0, 0.0)
        );
        let ir::Rate::Hz(hz) = c.rate else {
            panic!("free rate");
        };
        assert!(near(hz, 0.1), "{hz}");
        assert_eq!(
            (c.mode, c.invert, c.mix),
            (ir::CombMode::Standard, false, 0.5)
        );
    }
}
