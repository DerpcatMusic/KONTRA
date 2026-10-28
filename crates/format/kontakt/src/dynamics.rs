//! Kontakt's Transient Master (0x43) and Feedback Compressor (0x4c) onto
//! `ir::Transient` and `ir::Compressor` (docs/re/engine/dyn-compressors.md,
//! dyn-limiters-transient.md; saved layouts from v2 sampler-kontakt
//! effects/dynamics.rs).

use kontra_ir as ir;

/// The record's effect and, when it sets something not carried, its name.
pub(crate) fn effect(
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
            let c = feedback([f(0)?, f(1)?, f(2)?, f(3)?, f(4)?, f(5)?], byte(29)?);
            Some((
                ir::EffectKind::Compressor(c),
                byte(30)?.then_some("Feedback Compressor side chain"),
            ))
        }
        _ => None,
    }
}

/// Setter 0x141ac87b0: attack `2(x − ½)·120` and sustain `2(x − ½)·115` dB
/// per envelope unit; gain `(x − ½)·12` dB above the centre, `·48` below.
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
    let db = |x: f32| 20.0 * x.log10();
    let threshold = r.max(2.0).powf(0.2) * 0.703_942_9 - 0.672_507_3;
    let knee = 3.601_805_4 / r + 0.840_284_1 - 1.440_552_4 / (r.sqrt() * r) + 0.05;
    let drive_db = 50.0 * input - 10.0;
    let knee_db = db(knee).max(0.0);
    ir::Compressor {
        threshold_db: db(threshold) + knee_db / 2.0 - drive_db,
        ratio: r,
        knee_db,
        attack_ms: 1000.0 * (0.049_896_6 * attack + 0.000_103_4),
        release_ms: 1000.0 * (1.15 * release + 0.05),
        makeup_db: 50.0 * makeup - 40.0 + drive_db,
        link,
        mix,
        detector: ir::Detector::Output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
