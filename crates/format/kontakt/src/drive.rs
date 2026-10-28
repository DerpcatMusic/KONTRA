//! Saturation, Distortion and Lo-Fi onto the engine's waveshaper and
//! reducer: their saved records (readers 0x140d02c30, 0x140d00c60,
//! 0x140d01f20, via v2 sampler-kontakt effects/mod.rs) and knob laws.
//! Group racks only: the engine runs these on each voice.

use kontra_format::Notes;
use kontra_ir as ir;

/// Lo-Fi's controls at or above this are off (native threshold 0x3f7fff58).
const ACTIVE_BELOW: f32 = f32::from_bits(0x3f7f_ff58);

/// Distortion's fixed DC blocker, a Butterworth high-pass (template copy
/// 0x1408373c0; at this corner its 48 kHz coefficients match v2's to 2e-7).
const DC_HZ: f32 = 14.96;

/// The record of module `id` as a drive effect, or none (then reported as
/// not carried).
pub(crate) fn effect(
    id: u16,
    version: u16,
    public: &[u8],
    at: &str,
    notes: &mut Notes,
) -> Option<ir::EffectKind> {
    if !at.starts_with("group") {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let i = |n: usize| Some(i32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?));
    match id {
        // Version 0x70 of the same id is the Surround Panner.
        0x1d if version != 0x70 => saturation(f(0)?, i(4)?, at, notes),
        0x1e => distortion(i(0)?, f(4)?, f(8)?),
        0x20 => {
            if public.get(8).is_some_and(|&b| b != 0) {
                notes.add(ir::Impact::Audible, "Lo-Fi flag (law unknown)", at);
            }
            Some(lofi(f(0)?, f(4)?, f(9)?, f(13)?))
        }
        _ => None,
    }
}

/// Shape −1..=1; algorithm 0 Classic, 1 Enhanced, 2 Drums.
fn saturation(shape: f32, algorithm: i32, at: &str, notes: &mut Notes) -> Option<ir::EffectKind> {
    if algorithm != 0 {
        // ponytail: Enhanced and Drums are a table and a peak follower (v2
        // sampler-core dsp/color/saturator.rs); none in the corpus yet.
        notes.add(ir::Impact::Audible, "Saturation Enhanced or Drums", at);
        return None;
    }
    Some(ir::EffectKind::Shaper(ir::Shaper {
        curve: ir::DriveCurve::Soft(shape.clamp(-1.0, 1.0)),
        lowpass_hz: None,
        highpass_hz: None,
    }))
}

/// Mode 0 Tube, 1 Transistor; drive and damping 0..=1.
fn distortion(mode: i32, drive: f32, damping: f32) -> Option<ir::EffectKind> {
    let drive = drive.clamp(0.0, 1.0);
    let curve = match mode {
        0 => ir::DriveCurve::Tube(drive),
        1 => ir::DriveCurve::Power(drive),
        _ => return None,
    };
    Some(ir::EffectKind::Shaper(ir::Shaper {
        curve,
        lowpass_hz: Some(damping_hz(damping)),
        highpass_hz: Some(DC_HZ),
    }))
}

/// The damping low-pass's corner (template copy 0x1408373c0, v2
/// sampler-core dsp/color/distortion.rs): a normalized cutoff from Kontakt's
/// 8-term exp series, of 44011.98 Hz × 0.99 × 0.99878 (the LP-to-LP
/// transform's angle) over two.
pub(crate) fn damping_hz(damping: f32) -> f32 {
    let x = 6.15 * (1.0 - 0.4 * f64::from(damping.clamp(0.0, 1.0)));
    let (mut term, mut exp) = (1.0, 1.0);
    for n in 1..=8 {
        term *= x / f64::from(n);
        exp += term;
    }
    let normalized = ((exp - 1.0) * 0.002_572_831_7).clamp(0.0, 1.0) + 0.002;
    (normalized * 44_011.976_047_904_19 / 2.0 * 0.99 * 0.998_780_904_966_9) as f32
}

/// Bits, frequency, noise level and colour, each 0..=1.
fn lofi(bits: f32, frequency: f32, noise: f32, color: f32) -> ir::EffectKind {
    // The hold (decompile 0x140aad440: `φ < P ? φ += 1 : (φ −= P,
    // capture)`, P = 1/frequency): a capture, then P frames held. Each is
    // off from the native threshold.
    let off = |x: f32| if x < ACTIVE_BELOW { x } else { 1.0 };
    ir::EffectKind::Crush(ir::Crush {
        rate: off(frequency),
        bits: 16.0 * off(bits),
        noise: if noise >= 0.001 { noise * 0.075 } else { 0.0 },
        noise_tone: 1.0 - 0.9 * color.clamp(0.0, 1.0),
    })
}

/// A route to module parameter `param` of group insert `insert`, through
/// the knob law of the effect it moves; none when `param` is not one of
/// these effects'. The modulation compiler (FUN_140873040) emits
/// `$in + E·intensity` on the target's own value: Saturation's path
/// (FUN_140aa4d20) clamps it to the signed shape, Lo-Fi's (FUN_140aa34b0)
/// reads bits as 16·v and the hold from the frequency knob, both refreshed
/// every 32 frames. Distortion (0x40) is outside the compiler's signed
/// families, so it reads an LFO in its unipolar view.
pub(crate) fn route(
    source: ir::Source,
    insert: u8,
    name: &str,
    intensity: f32,
) -> Option<ir::ModKind> {
    let (param, scale) = match name {
        "formantTalk" => (ir::EffectParam::FormantTalk, 1.0),
        "formantSize" => (ir::EffectParam::FormantSize, 1.0),
        "formantSharp" => (ir::EffectParam::FormantSharp, 1.0),
        "shaper" | "distortionIntensity" => (ir::EffectParam::ShaperAmount, 1.0),
        "bitdepth" => (ir::EffectParam::CrushBits, 16.0),
        "downsample" => (ir::EffectParam::CrushRate, 1.0),
        _ => return None,
    };
    Some(ir::ModKind::EffectParam {
        source,
        insert,
        param,
        depth: intensity * scale,
        unipolar: param == ir::EffectParam::ShaperAmount && name == "distortionIntensity",
    })
}

/// Reports resonance routes to a formant insert: the target table
/// (FUN_140a6d410) gives the formant its own `formantSharp` (0x26), and
/// what `filterQ` (0x20) does there is not read yet.
pub(crate) fn formant_q(inserts: &[Option<ir::Effect>], mods: &[ir::Mod], notes: &mut Notes) {
    for m in mods {
        if let ir::ModKind::Resonance { insert, .. } = m.kind
            && inserts
                .get(usize::from(insert))
                .and_then(Option::as_ref)
                .is_some_and(|e| matches!(e.kind, ir::EffectKind::Formant(_)))
        {
            notes.add(ir::Impact::Audible, "modulator -> formant filterQ", &m.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_damping_corner_reproduces_kontakts_poles() {
        // v2's decompiled poles at 48 kHz for damping 0, 0.5, 1; a bilinear
        // one-pole at `damping_hz` has pole (1 − g)/(1 + g), g = tan(π f/fs).
        for (damping, pole, tolerance) in [
            (0.0, -0.748_475_234_218_23, 2e-3),
            (0.5, 0.326_787_237_731_62, 1e-6),
            (1.0, 0.746_823_690_647_603, 1e-6),
        ] {
            let g = (std::f64::consts::PI * f64::from(damping_hz(damping)) / 48_000.0).tan();
            let got = (1.0 - g) / (1.0 + g);
            assert!((got - pole).abs() < tolerance, "{damping}: {got}");
        }
    }

    #[test]
    fn lofi_controls_map_to_the_reducer() {
        let ir::EffectKind::Crush(c) = lofi(0.5, 0.5, 1.0, 0.0) else {
            panic!()
        };
        assert_eq!(
            (c.rate, c.bits, c.noise, c.noise_tone),
            (0.5, 8.0, 0.075, 1.0)
        );
        let ir::EffectKind::Crush(off) = lofi(1.0, 1.0, 0.0, 1.0) else {
            panic!()
        };
        assert_eq!((off.rate, off.bits, off.noise), (1.0, 16.0, 0.0));
    }

    #[test]
    fn module_routes_move_the_effect_through_its_knob_law() {
        let depth = |param| match route(ir::Source::Velocity, 2, param, 0.5) {
            Some(ir::ModKind::EffectParam {
                insert: 2, depth, ..
            }) => Some(depth),
            _ => None,
        };
        assert_eq!(depth("shaper"), Some(0.5));
        assert_eq!(depth("bitdepth"), Some(8.0));
        assert_eq!(depth("formantTalk"), Some(0.5));
        assert_eq!(depth("volume"), None);
    }
}
