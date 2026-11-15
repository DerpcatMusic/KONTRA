//! Psyche Delay (module 0x63, saved v0x10) onto the engine's pitch-shifting
//! echo ([`ir::Shifter`]). The saved record: TIME (ms, or a count of synced
//! units), FEEDBACK, DETUNE, PITCH, LR OFFSET and CROSS FB as floats at
//! 0..24, the stereo-reverse, stereo-detune and reverse bytes at 24..27,
//! the synced unit's quarter-note beats at 27 (−1 when free); two floats
//! and a byte after it move nothing heard here.
//!
//! Laws (Kontakt 8.13.1, the kernel read from the binary for v2 and
//! measured wet only through the oracle, 2026-11-14,
//! docs/re/engine/fx-measured/psyche-vibrato.md):
//! - TIME is ms (the knob 2000·x, 5 ms at 0); the right side runs
//!   `1 − LR_OFFSET/2` of it;
//! - each repeat is scaled by 0.98·FEEDBACK; CROSS FB is the other side's
//!   share of each line's feedback;
//! - PITCH is `round(24·x − 12)` semitones, DETUNE `x − ½` semitones on
//!   both sides, or only on the left with STEREO DETUNE;
//! - REVERSE plays both sides backwards; STEREO REVERSE flips the right
//!   side's direction against the left's.

use kontra_ir as ir;

/// The saved fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Psyche {
    pub ms: f32,
    pub feedback: f32,
    pub detune: f32,
    pub pitch: f32,
    pub offset: f32,
    pub cross: f32,
    pub reverse_stereo: bool,
    pub detune_stereo: bool,
    pub reverse: bool,
    /// A synced TIME's quarter-note beats per unit; TIME is then a count.
    pub beats: Option<f32>,
}

/// A fresh Psyche Delay (its INIT preset; Kontakt 8.13.1 reads TIME .25,
/// FEEDBACK .25, the others .5 or off back from a script-loaded one).
pub const INIT: Psyche = Psyche {
    ms: 500.0,
    feedback: 0.25,
    detune: 0.5,
    pitch: 0.5,
    offset: 0.5,
    cross: 0.5,
    reverse_stereo: false,
    detune_stereo: false,
    reverse: false,
    beats: None,
};

/// Each repeat's gain per FEEDBACK.
const LOOP_GAIN: f32 = 0.98;
/// The shortest TIME, ms.
const SHORTEST_MS: f32 = 5.0;
/// TIME's ms at the knob's full turn.
const LONGEST_MS: f32 = 2000.0;

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<Psyche> {
    if version != 0x10 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |n: usize| public.get(n).map(|&b| b != 0);
    Some(Psyche {
        ms: f(0)?,
        feedback: f(4)?,
        detune: f(8)?,
        pitch: f(12)?,
        offset: f(16)?,
        cross: f(20)?,
        reverse_stereo: flag(24)?,
        detune_stereo: flag(25)?,
        reverse: flag(26)?,
        beats: Some(f(27)?).filter(|&b| b > 0.0),
    })
}

#[must_use]
pub fn shifter(p: Psyche) -> ir::Shifter {
    let semis = (p.pitch.clamp(0.0, 1.0) * 24.0 - 12.0).round_ties_even();
    let detune = p.detune.clamp(0.0, 1.0) - 0.5;
    let left = ((semis + detune) / 12.0).exp2();
    let right = if p.detune_stereo {
        (semis / 12.0).exp2()
    } else {
        left
    };
    // The right side runs forward when the two switches agree.
    let l = if p.reverse { -left } else { left };
    let r = if p.reverse == p.reverse_stereo {
        right
    } else {
        -right
    };
    ir::Shifter {
        time: match p.beats {
            Some(beats) => ir::DelayTime::Beats(p.ms.max(1.0) * beats),
            None => ir::DelayTime::Ms(p.ms.clamp(SHORTEST_MS, LONGEST_MS)),
        },
        right: 1.0 - 0.5 * p.offset.clamp(0.0, 1.0),
        speed: [l, r],
        feedback: LOOP_GAIN * p.feedback.clamp(0.0, 1.0),
        cross: p.cross.clamp(0.0, 1.0),
    }
}

/// The record as a script moves it: every knob move rebuilds the echo
/// through [`shifter`]; the synced unit holds as saved.
#[must_use]
pub fn model(p: Psyche) -> ir::KnobModel {
    use ir::SlotKnob::Knob;
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    ir::KnobModel {
        knobs: (0..9).map(Knob).collect(),
        saved: [
            p.ms,
            p.feedback,
            p.detune,
            p.pitch,
            p.offset,
            p.cross,
            on(p.reverse_stereo),
            on(p.detune_stereo),
            on(p.reverse),
            p.beats.unwrap_or(-1.0),
        ]
        .into(),
        build: |v| {
            let [
                ms,
                feedback,
                detune,
                pitch,
                offset,
                cross,
                rs,
                ds,
                rev,
                beats,
            ] = *v
            else {
                return None;
            };
            Some(ir::EffectKind::Shifter(shifter(Psyche {
                ms,
                feedback,
                detune,
                pitch,
                offset,
                cross,
                reverse_stereo: rs > 0.0,
                detune_stereo: ds > 0.0,
                reverse: rev > 0.0,
                beats: Some(beats).filter(|&b| b > 0.0),
            })))
        },
    }
}

/// The KSP names of [`model`]'s knobs: TIME saves 2000 ms per unit, the
/// switches save on at any value above 0, the rest as set (bindings table
/// `fx_raum_delight_psyche_vibrato_phasis_replika.tsv`).
#[must_use]
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 9] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        (
            "$ENGINE_PAR_PSYDL_TIME",
            Slot(Knob(0)),
            ir::Law::Linear {
                lo: 0.0,
                hi: LONGEST_MS,
            },
        ),
        ("$ENGINE_PAR_PSYDL_FEEDBACK", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_PSYDL_DETUNE", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_PSYDL_PITCH", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_PSYDL_LR_OFFSET", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_PSYDL_CROSS_FEEDBACK", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_PSYDL_REVERSE_STEREO", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_PSYDL_DETUNE_STEREO", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_PSYDL_REVERSE", Slot(Knob(8)), unit()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The INIT preset's record (`- INIT PsycheDelay -.nkp`).
    #[test]
    fn the_init_record_reads_as_the_fresh_unit() {
        let mut public = Vec::new();
        for v in [500.0f32, 0.25, 0.5, 0.5, 0.5, 0.5] {
            public.extend(v.to_le_bytes());
        }
        public.extend([0, 0, 0]);
        for v in [-1.0f32, 0.0, 1.0] {
            public.extend(v.to_le_bytes());
        }
        public.push(0);
        assert_eq!(saved(0x10, &public), Some(INIT));
        let s = shifter(INIT);
        assert_eq!(s.time, ir::DelayTime::Ms(500.0));
        assert_eq!((s.right, s.speed, s.cross), (0.75, [1.0, 1.0], 0.5));
        assert!((s.feedback - 0.245).abs() < 1e-6);
    }

    #[test]
    fn pitch_detune_and_reverse_set_each_sides_speed() {
        let up = shifter(Psyche {
            pitch: 0.75,
            detune: 0.7,
            detune_stereo: true,
            ..INIT
        });
        assert!((up.speed[0] - (6.2f32 / 12.0).exp2()).abs() < 1e-5);
        assert!((up.speed[1] - (0.5f32).exp2()).abs() < 1e-5);
        let back = shifter(Psyche {
            reverse: true,
            reverse_stereo: true,
            ..INIT
        });
        assert_eq!(back.speed, [-1.0, 1.0]);
    }
}
