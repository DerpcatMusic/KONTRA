//! Kontakt's Transient Master (0x43) and Feedback Compressor (0x4c) onto
//! `ir::Transient` and `ir::Compressor` (docs/re/engine/dyn-compressors.md,
//! dyn-limiters-transient.md; saved layouts from v2 sampler-kontakt
//! effects/dynamics.rs).

use kontra_ir as ir;
use kontra_law::common::DB_GAIN;
use kontra_law::kontakt::{FEEDBACK_KNEE, FEEDBACK_THRESHOLD};

/// The record's effect and, when it sets something not carried, its name.
pub fn effect(
    module: u16,
    version: u16,
    public: &[u8],
) -> Option<(ir::EffectKind, Option<&'static str>)> {
    if version != 0x10 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(4 * n..4 * n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let byte = |n: usize| public.get(n).map(|&b| b != 0);
    match module {
        // Attack, sustain, gain, then the clip switch as an i32 0 or 1.
        0x43 => {
            let clip = f(3).map(f32::to_bits).and_then(|bits| match bits {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            })?;
            Some((
                ir::EffectKind::Transient(transient(f(0)?, f(1)?, f(2)?, clip)),
                None,
            ))
        }
        // Input, make-up, attack, release, ratio, mix and an unbound float;
        // then high quality, link and side chain bytes.
        0x4c => {
            let (saved, link) = feedback_saved(public)?;
            let c = feedback(saved, link);
            Some((
                ir::EffectKind::Compressor(c),
                byte(30)?.then_some("Feedback Compressor side chain"),
            ))
        }
        _ => None,
    }
}

/// A version 0x10 Feedback Compressor record's six knobs (input, make-up,
/// attack, release, ratio, mix, each 0..=1) and its link byte.
fn feedback_saved(public: &[u8]) -> Option<([f32; 6], bool)> {
    let f = |n: usize| {
        let bytes = public.get(4 * n..4 * n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let saved = [f(0)?, f(1)?, f(2)?, f(3)?, f(4)?, f(5)?];
    Some((saved, *public.get(29)? != 0))
}

/// The Feedback Compressor's script knobs (`$ENGINE_PAR_FCOMP_*`): every
/// move rebuilds the compressor from all six through [`feedback`], as the
/// native setter derives each field from all of them (the input drives the
/// threshold down and the make-up up; the ratio moves the knee).
pub fn feedback_model(public: &[u8]) -> Option<ir::KnobModel> {
    use ir::SlotKnob::{Attack, Input, Makeup, Mix, Ratio, Release};
    let (saved, link) = feedback_saved(public)?;
    let mut positions = saved.to_vec();
    positions.push(f32::from(u8::from(link)));
    Some(ir::KnobModel {
        knobs: [Input, Makeup, Attack, Release, Ratio, Mix].into(),
        saved: positions.into(),
        build: |p| {
            let (knobs, link) = (p.get(..6)?.try_into().ok()?, *p.get(6)? > 0.5);
            Some(ir::EffectKind::Compressor(feedback(knobs, link)))
        },
    })
}

/// The Feedback Compressor's KSP names, each a knob of [`feedback_model`].
pub fn feedback_rows() -> [(&'static str, ir::KnobTarget, ir::Law); 6] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::{Attack, Input, Makeup, Mix, Ratio, Release};
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    [
        ("$ENGINE_PAR_FCOMP_INPUT", Slot(Input), unit()),
        ("$ENGINE_PAR_FCOMP_MAKEUP", Slot(Makeup), unit()),
        ("$ENGINE_PAR_FCOMP_ATTACK", Slot(Attack), unit()),
        ("$ENGINE_PAR_FCOMP_RELEASE", Slot(Release), unit()),
        ("$ENGINE_PAR_FCOMP_RATIO", Slot(Ratio), unit()),
        ("$ENGINE_PAR_FCOMP_MIX", Slot(Mix), unit()),
    ]
}

/// Setter 0x141ac87b0: attack `2(x − ½)·120` and sustain `2(x − ½)·115` dB
/// per envelope unit; gain `(x − ½)·12` dB above the centre, `·48` below.
/// The core's follower rates and thresholds (in `kontra-dsp-dynamics`) come
/// from process 0x141aaac90 and reset 0x141aa0b80.
fn transient(attack: f32, sustain: f32, gain: f32, clip: bool) -> ir::Transient {
    let [attack, sustain, gain] = [attack, sustain, gain].map(|x| x.clamp(0.0, 1.0) - 0.5);
    ir::Transient {
        attack: 2.0 * attack * 120.0,
        sustain: 2.0 * sustain * 115.0,
        gain_db: gain * if gain >= 0.0 { 12.0 } else { 48.0 },
        clip,
    }
}

/// Setter 0x141ac56d0 and ratio law 0x141a91d10: drive `10^(2.5x − ½)` into
/// the detector and the audio, make-up `10^(2.5x − 2)`, attack
/// `0.0499x + 0.0001` s, release `1.15x + 0.05` s, ratio `1 + 19x`; the knee
/// runs from `T = max(r, 2)^0.2·0.70394 − 0.67251` to `T·k(r)`. The detector
/// reads the output (a feedback design).
// ponytail: the native quartic knee, its gain smoother and 10 Hz detector
// high-pass, and the 2x/4x sub-steps are the standard soft knee and one-pole
// ballistics here; no native recording says how far apart they sound.
fn feedback([input, makeup, attack, release, ratio, mix]: [f32; 6], link: bool) -> ir::Compressor {
    let [input, makeup, attack, release, ratio, mix] =
        [input, makeup, attack, release, ratio, mix].map(|x| x.clamp(0.0, 1.0));
    let r = 1.0 + 19.0 * ratio;
    let db = |x: f32| DB_GAIN.inverse(x).unwrap_or(0.0);
    let threshold = FEEDBACK_THRESHOLD.eval_unclamped(r.max(2.0));
    let knee = FEEDBACK_KNEE.eval_unclamped(r);
    let drive_db = 50.0 * input - 10.0;
    let knee_db = db(knee).max(0.0);
    ir::Compressor {
        threshold_db: db(threshold) + knee_db / 2.0 - drive_db,
        ratio: r,
        knee_db,
        attack_ms: 1000.0 * (0.049_896_6 * attack + 0.000_103_4),
        release_ms: 1000.0 * (1.15 * release + 0.05),
        makeup_db: 50.0 * makeup - 40.0 + drive_db,
        makeup_slope: 0.0,
        link,
        mix,
        detector: ir::Detector::Output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feedback_model_is_the_reader_at_the_saved_knobs() {
        let mut public: Vec<u8> = [0.4f32, 0.8, 0.5, 0.5, 0.25, 1.0, 0.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        public.extend([1, 1, 0]);
        let (read, _) = effect(0x4c, 0x10, &public).unwrap();
        let model = feedback_model(&public).unwrap();
        assert_eq!((model.build)(&model.saved), Some(read));
    }

    #[test]
    fn transient_centres_are_neutral_and_the_clip_is_an_integer() {
        let mut public: Vec<u8> = [0.5f32; 3].iter().flat_map(|f| f.to_le_bytes()).collect();
        public.extend(1u32.to_le_bytes());
        let Some((ir::EffectKind::Transient(t), None)) = effect(0x43, 0x10, &public) else {
            panic!()
        };
        assert_eq!(
            (t.attack, t.sustain, t.gain_db, t.clip),
            (0.0, 0.0, 0.0, true)
        );
        public[12] = 2;
        assert!(effect(0x43, 0x10, &public).is_none());
        assert!((transient(0.9, 0.2, 0.25, false).gain_db + 12.0).abs() < 1e-5);
    }

    #[test]
    fn the_feedback_compressor_drives_its_threshold_down_and_its_output_up() {
        let quiet = feedback([0.2, 0.8, 0.5, 0.5, 0.25, 1.0], true);
        let loud = feedback([0.6, 0.8, 0.5, 0.5, 0.25, 1.0], true);
        assert!((quiet.threshold_db - loud.threshold_db - 20.0).abs() < 1e-3);
        assert!((loud.makeup_db - quiet.makeup_db - 20.0).abs() < 1e-3);
        assert!((quiet.ratio - 5.75).abs() < 1e-5 && quiet.detector == ir::Detector::Output);
        assert!((quiet.release_ms - 625.0).abs() < 1e-3);
    }
}
