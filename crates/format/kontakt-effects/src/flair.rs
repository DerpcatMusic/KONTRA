//! Flair (`$EFFECT_TYPE_FLAIR`, 42) onto [`ir::CombBank`], with the laws
//! read from Kontakt 8.13.1's `ni::effektrig::dsp::Flair` (setParam
//! [ref], lane writer [ref]; addresses are `.rdata`
//! constants). Four comb voices at the chord's intervals over
//! `f(Pitch + 12)`, fed back at `0.999·sin(π/2·fb)` made pitch-even.
//!
//! ponytail: the KSP-to-lane map and the raw scaling of Pitch, Chord and
//! Mode are unread: each knob is mapped to its lane by meaning, Pitch as
//! semitones 0..=96, Chord and Mode as indices. Rate Unit (host sync,
//! [ref] through the unit table [ref]) is not read
//! yet, so `$ENGINE_PAR_FLAIR_RATE_UNIT` has no row: a script that sets it
//! goes unanswered (and is listed so) instead of silently running free.

use kontra_ir as ir;

/// A new Flair's knobs as Kontakt reads them back, in [`model`]'s order:
/// mode, chord, invert, rate, rate unit, feedback, amount, width, pitch,
/// damping, voices, detune, mix, offset, scan mode.
pub const INIT: [f32; 15] = [
    0.0, 0.0, 0.0, 0.360_584, 0.0, 0.4, 1.0, 0.0, 48.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0,
];

/// Voices 1..=3's intervals per chord, voice 0 always 1;
/// [ref]'s float constants (decoded in lane-scripts/flair/re/
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

/// Each voice's detune at 100 %, a frequency factor's slope ([ref]:
/// +40, −40, −60, +60 cents; [ref], b58, b70, b80).
const DETUNE: [f32; 4] = [0.023_373_9, -0.022_840_0, -0.034_063_7, 0.035_264_9];

/// `f(MIDI 12 + 96p)`: 440·2^((96p − 57)/12) Hz (96 [ref], 57
/// [ref], 440 [ref]).
fn pitch_hz(p: f32) -> f32 {
    440.0 * ((96.0 * p - 57.0) / 12.0).exp2()
}

/// The comb at knob positions `p` (see [`INIT`]).
pub fn flair(p: [f32; 15]) -> ir::EffectKind {
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
    // Loop gain `0.999·sin(π/2·fb)` raised to
    // `1/e`, `e = 2^(8·pitch − 4) = F/F(48)`: a
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
        // Voices: three equal-power fades over thirds (3, [ref]).
        voices: 3.0 * voices,
        // Damping `0.25·p³`.
        damping: 0.25 * damping.powi(3),
        // Amount: the swing `p²`, the scan's depth `p`.
        depth: amount * amount,
        // Rate `440·2^(11.5p − 16.25)` Hz ([ref]/e4, case 1).
        rate: ir::Rate::Hz(440.0 * (11.5 * rate - 16.25).exp2()),
        // Offset: the still head `0.2·p` further back.
        offset: 0.2 * offset,
        polarity: if invert >= 0.5 {
            ir::Polarity::Inverted
        } else {
            ir::Polarity::Normal
        },
        mode,
        scan_depth: amount,
        scan_shape,
        width,
        mix,
    })
}

/// Decode a saved `BParFXFlair` (`0x5c`, v0x51, 66 bytes) into [`INIT`]'s
/// knob order (`Kontakt 8.exe` [ref] ([ref] = 97.0`),
/// and `.rdata` parameter table [ref]..[ref]):
/// 10 floats (`amount`, `pitch_norm`, `rate`, `width`, `mix`, `voices`,
/// `feedback`, `damping`, `detune`, `offset`), 1 bool (`invert_phase`),
/// 3 `i32`s (`mode` 0..=2, `scan_mode` 0..=2, `chord` 0..=23), then 3 sync
/// floats + 1 sync bool.
#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<[f32; 15]> {
    if version != 0x51 || public.len() < 66 {
        return None;
    }
    let mut cur = crate::module::Cursor::new(public);
    let [
        amount,
        pitch_norm,
        rate,
        width,
        mix,
        voices,
        feedback,
        damping,
        detune,
        offset,
    ] = cur.f32_array()?;
    let pitch = (97.0 * pitch_norm.clamp(0.0, 1.0)).trunc().clamp(0.0, 96.0);
    let invert = crate::module::bool_f(cur.bool()?);
    let mode = cur.i32()?;
    let scan_mode = cur.i32()?;
    let chord = cur.i32()?;
    if !(0..=2).contains(&mode) || !(0..=2).contains(&scan_mode) || !(0..=23).contains(&chord) {
        return None;
    }
    let unit = crate::module::bool_f(cur.f32()? > 1e-9);
    Some([
        mode as f32,
        chord as f32,
        invert,
        rate,
        unit,
        feedback,
        amount,
        width,
        pitch,
        damping,
        voices,
        detune,
        mix,
        offset,
        scan_mode as f32,
    ])
}

/// Single-encoding module descriptor for Flair (`0x5c`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::FLAIR,
    module_id: 0x5c,
    versions: (0, u32::MAX),
    name: "Flair",
    script_type: 42,
    mix: crate::module::MixMode::OwnMix,
    knobs: crate::module::KNOBS.split_at(15).0,
    build: |p| Some(flair(p.try_into().ok()?)),
};

/// A Flair's knobs from saved positions as a script moves them.
pub fn model_from(saved: [f32; 15]) -> ir::KnobModel {
    MODULE.knob_model(&saved)
}

/// A Flair's knobs as a script moves them: every move rebuilds it from all.
pub fn model() -> ir::KnobModel {
    model_from(INIT)
}

/// What `$EFFECT_TYPE_FLAIR` (42) loads: [`INIT`], every knob through [`model`].
pub fn slot_type() -> ir::SlotType {
    MODULE.slot_type(&INIT)
}

/// The KSP names of [`model`]'s knobs (excluding the rate unit).
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 14] {
    use crate::module::{RAW_LAW, UNIT_LAW, knob_row};
    [
        knob_row("$ENGINE_PAR_FLAIR_MODE", 0, RAW_LAW),
        knob_row("$ENGINE_PAR_FLAIR_CHORD", 1, RAW_LAW),
        knob_row("$ENGINE_PAR_FLAIR_INVERT_PHASE", 2, RAW_LAW),
        knob_row("$ENGINE_PAR_FLAIR_RATE", 3, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_FEEDBACK", 5, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_AMOUNT", 6, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_WIDTH", 7, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_PITCH", 8, RAW_LAW),
        knob_row("$ENGINE_PAR_FLAIR_DAMPING", 9, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_VOICES", 10, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_DETUNE", 11, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_MIX", 12, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_OFFSET", 13, UNIT_LAW),
        knob_row("$ENGINE_PAR_FLAIR_SCANMODE", 14, RAW_LAW),
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
            (c.mode, c.polarity, c.mix),
            (ir::CombMode::Standard, ir::Polarity::Normal, 0.5)
        );
    }

    #[test]
    fn saved_decodes_v51_flair_record() {
        let mut public = Vec::new();
        // amount, pitch (0.5 -> trunc(48.5) = 48), rate, width, mix, voices, feedback, damping, detune, offset
        for v in [1.0f32, 0.5, 0.360_584, 0.25, 0.5, 0.5, 0.4, 0.1, 0.2, 0.3] {
            public.extend(v.to_le_bytes());
        }
        public.push(1); // invert_phase
        public.extend(1i32.to_le_bytes()); // mode = ThruZero
        public.extend(2i32.to_le_bytes()); // scan_mode = SawDown
        public.extend(3i32.to_le_bytes()); // chord = 3
        for v in [-1.0f32, 0.0, 0.0] {
            public.extend(v.to_le_bytes());
        }
        public.push(0);
        let decoded = saved(0x51, &public).unwrap();
        assert_eq!(decoded[0], 1.0); // mode
        assert_eq!(decoded[1], 3.0); // chord
        assert_eq!(decoded[2], 1.0); // invert
        assert_eq!(decoded[8], 48.0); // pitch
        assert_eq!(decoded[14], 2.0); // scan_mode
        let ir::EffectKind::CombBank(c) = flair(decoded) else {
            panic!("comb bank");
        };
        assert_eq!(
            (c.mode, c.scan_shape, c.polarity),
            (
                ir::CombMode::ThruZero,
                ir::ScanShape::SawDown,
                ir::Polarity::Inverted
            )
        );
    }
}
