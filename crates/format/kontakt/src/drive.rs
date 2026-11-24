//! Saturation, Distortion, Lo-Fi, Skreamer, Tape Saturator, DStortion, Supercharger GT, Bite and Freak onto the engine's waveshaper,
//! reducer and ring modulator: their saved records (readers [ref],
//! [ref], via v2 sampler-kontakt effects/mod.rs) and knob laws.
//! Any rack: the engine runs them on each voice in a group rack and on the
//! sum in a bus or instrument rack.

use kontra_format::Notes;
use kontra_ir as ir;

/// Lo-Fi's controls at or above this are off (native threshold 0x3f7fff58).
const ACTIVE_BELOW: f32 = f32::from_bits(0x3f7f_ff58);

/// Distortion's fixed DC blocker, a Butterworth high-pass at 36.8 Hz:
/// Kontakt 8.13.1's Tube 0.5 sweep (2026-11-14) sits 7.0, 4.6, 2.4 and 1.0
/// dB under a 14.96 Hz one (v2's corner, template copy [ref]) at 25,
/// 31, 40 and 50 Hz, a 36.0 to 37.1 Hz Butterworth at each.
const DC_HZ: f32 = 36.8;

/// Saturation's Classic algorithm, shape −1..=1 (the others are reported
/// by its row in effects.rs).
pub(crate) fn saturation(shape: f32) -> ir::EffectKind {
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Soft(shape.clamp(-1.0, 1.0)),
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::NONE,
    })
}

/// Mode 0 Tube, 1 Transistor; drive and damping 0..=1.
pub(crate) fn distortion(mode: i32, drive: f32, damping: f32) -> Option<ir::EffectKind> {
    let drive = drive.clamp(0.0, 1.0);
    let curve = match mode {
        0 => ir::DriveCurve::Tube(drive),
        1 => ir::DriveCurve::Power(drive),
        _ => return None,
    };
    Some(ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve,
        lowpass_hz: Some(damping_hz(damping)),
        highpass_hz: Some(DC_HZ),
        sections: ir::Sections::NONE,
    }))
}

/// The damping low-pass's corner (template copy [ref], v2
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
pub(crate) fn lofi(bits: f32, frequency: f32, noise: f32, color: f32) -> ir::EffectKind {
    // The hold (decompile [ref]: `φ < P ? φ += 1 : (φ −= P,
    // capture)`, P = 1/frequency): a capture, then P frames held. Each is
    // off from the native threshold.
    let off = |x: f32| if x < ACTIVE_BELOW { x } else { 1.0 };
    ir::EffectKind::Crush(ir::Crush {
        rate: off(frequency),
        bits: 16.0 * off(bits),
        noise: if noise >= 0.001 { noise * 0.075 } else { 0.0 },
        noise_tone: 1.0 - 0.9 * color.clamp(0.0, 1.0),
        converter: None,
    })
}

/// A route to module parameter `param` of group insert `insert`, through
/// the knob law of the effect it moves; none when `param` is not one of
/// these effects'. The modulation compiler emits
/// `$in + E·intensity` on the target's own value: Saturation's path
/// clamps it to the signed shape, Lo-Fi's
/// reads bits as 16·v and the hold from the frequency knob, both refreshed
/// every 32 frames. Distortion (0x40) is outside the compiler's signed
/// families, so it reads an LFO in its unipolar view.
pub(crate) fn route(
    source: ir::Source,
    insert: u8,
    name: &str,
    intensity: f32,
) -> Option<ir::ModRoute> {
    // A release-trigger counter is read at note-on only; a voice's effect
    // never sees it, so it is reported instead.
    if matches!(source, ir::Source::HeldTime { .. }) {
        return None;
    }
    let (param, scale) = match name {
        "formantTalk" => (ir::EffectParam::FormantTalk, 1.0),
        "formantSize" => (ir::EffectParam::FormantSize, 1.0),
        "formantSharp" => (ir::EffectParam::FormantSharp, 1.0),
        "shaper" | "distortionIntensity" => (ir::EffectParam::ShaperAmount, 1.0),
        "bitdepth" => (ir::EffectParam::CrushBits, 16.0),
        "downsample" => (ir::EffectParam::CrushRate, 1.0),
        // The legacy EQ's centre is 20·1000^v (eq-kontakt.md §1.1), its
        // source scaled by 0.5 (compiler string [ref]).
        _ => match name
            .strip_prefix("eqFreq")
            .and_then(|n| n.parse::<u8>().ok())
        {
            Some(n @ 1..=3) => (ir::EffectParam::BellOctaves(n - 1), 0.5 * 1000f32.log2()),
            _ => return None,
        },
    };
    Some(
        ir::ModRoute::new(
            source,
            ir::ParamAddr::EffectParam { insert, param },
            intensity * scale,
        )
        .span(scale)
        .law(
            if param == ir::EffectParam::ShaperAmount && name == "distortionIntensity" {
                ir::RouteLaw::Unipolar
            } else {
                ir::RouteLaw::Linear
            },
        ),
    )
}

/// Reports the routes the engine cannot apply: an effect route to an empty
/// slot or to an effect without that control, and resonance routes to a
/// formant (the target table [ref] gives the formant its own
/// `formantSharp`, 0x26; what `filterQ`, 0x20, does there is not read yet).
pub(crate) fn unmoved(inserts: &[Option<ir::Effect>], mods: &[ir::Mod], notes: &mut Notes) {
    let at = |insert: u8| inserts.get(usize::from(insert)).and_then(Option::as_ref);
    for m in mods {
        let gap = match m.route {
            ir::ModRoute {
                target: ir::ParamAddr::FilterResonance { insert },
                ..
            } if at(insert).is_some_and(|e| matches!(e.kind, ir::EffectKind::Formant(_))) => {
                "modulator -> formant filterQ"
            }
            ir::ModRoute {
                target: ir::ParamAddr::EffectParam { insert, param },
                ..
            } if !at(insert).is_some_and(|e| param.add(&mut e.kind.clone(), 0.0)) => {
                "modulator -> effect without that control"
            }
            _ => continue,
        };
        notes.add(ir::Impact::Audible, gap, &m.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skreamers_saved_record_builds_its_overdrive() {
        let public: Vec<u8> = [0.5f32; 5].iter().flat_map(|v| v.to_le_bytes()).collect();
        let m = crate::skreamer::model(&public).expect("model");
        assert!(matches!(m.build(&m.saved), Some(ir::EffectKind::Overdrive(_))));
    }

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
            Some(ir::ModRoute {
                target: ir::ParamAddr::EffectParam { insert: 2, .. },
                depth,
                ..
            }) => Some(depth),
            _ => None,
        };
        assert_eq!(depth("shaper"), Some(0.5));
        assert_eq!(depth("bitdepth"), Some(8.0));
        assert_eq!(depth("formantTalk"), Some(0.5));
        assert_eq!(depth("eqFreq1"), Some(0.25 * 1000f32.log2()));
        assert_eq!(depth("eqFreq4"), None);
        assert_eq!(depth("volume"), None);
        let counter = ir::Source::HeldTime { span_ms: 100 };
        assert_eq!(route(counter, 2, "shaper", 0.5), None);
    }
}
