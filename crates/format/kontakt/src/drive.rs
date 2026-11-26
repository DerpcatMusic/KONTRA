//! Saturation, Distortion, Lo-Fi, Skreamer, Tape Saturator, DStortion, Supercharger GT, Bite and Freak onto the engine's waveshaper,
//! reducer and ring modulator: their saved records (readers [ref],
//! [ref], via v2 sampler-kontakt effects/mod.rs) and knob laws.
//! Any rack: the engine runs them on each voice in a group rack and on the
//! sum in a bus or instrument rack.

use std::borrow::Cow;

use kontra_format::Notes;
use kontra_ir as ir;

/// Lo-Fi's controls at or above this are off (native threshold 0x3f7fff58).
const ACTIVE_BELOW: f32 = f32::from_bits(0x3f7f_ff58);

/// Distortion's fixed DC blocker, a Butterworth high-pass at 36.8 Hz:
/// Kontakt 8.13.1's Tube 0.5 sweep (2026-11-14) sits 7.0, 4.6, 2.4 and 1.0
/// dB under a 14.96 Hz one (v2's corner, template copy [ref]) at 25,
/// 31, 40 and 50 Hz, a 36.0 to 37.1 Hz Butterworth at each.
const DC_HZ: f32 = 36.8;

/// 17-point odd curves over 0..=1 for Saturation's Enhanced mode at knob
/// settings 0, 0.25, 0.5, 0.75 and 1.0, fitted to Kontakt 8.13.1
/// (`docs/re/kontakt-bindings/saturation-curves.tsv`, max error < 0.01).
const ENHANCED_0: [f32; 17] = [
    0.0000, 0.0625, 0.1250, 0.1875, 0.2500, 0.3125, 0.3750, 0.4375, 0.5000, 0.5625, 0.6250, 0.6875,
    0.7500, 0.8125, 0.8750, 0.9375, 1.0000,
];
const ENHANCED_25: [f32; 17] = [
    0.0000, 0.1023, 0.1955, 0.2804, 0.3583, 0.4299, 0.4959, 0.5569, 0.6135, 0.6660, 0.7149, 0.7605,
    0.8031, 0.8430, 0.8805, 0.9157, 0.9489,
];
const ENHANCED_50: [f32; 17] = [
    0.0000, 0.1964, 0.3540, 0.4783, 0.5761, 0.6531, 0.7138, 0.7616, 0.7993, 0.8291, 0.8527, 0.8713,
    0.8860, 0.8977, 0.9070, 0.9143, 0.9201,
];
const ENHANCED_75: [f32; 17] = [
    0.0000, 0.3410, 0.5637, 0.6860, 0.7484, 0.7793, 0.7945, 0.8018, 0.8054, 0.8071, 0.8080, 0.8084,
    0.8086, 0.8087, 0.8087, 0.8088, 0.8088,
];
const ENHANCED_100: [f32; 17] = [
    0.0000, 0.4931, 0.6800, 0.7239, 0.7328, 0.7346, 0.7349, 0.7350, 0.7350, 0.7350, 0.7350, 0.7350,
    0.7350, 0.7350, 0.7350, 0.7350, 0.7350,
];

/// Adjacent lower/upper curve pairs for [`ir::DriveCurve::Blend`].
#[allow(clippy::indexing_slicing)]
const fn pair(a: [f32; 17], b: [f32; 17]) -> [f32; 34] {
    let mut out = [0.0; 34];
    let mut i = 0;
    while i < 17 {
        out[i] = a[i];
        out[i + 17] = b[i];
        i += 1;
    }
    out
}

static ENHANCED_PAIRS: [[f32; 34]; 4] = [
    pair(ENHANCED_0, ENHANCED_25),
    pair(ENHANCED_25, ENHANCED_50),
    pair(ENHANCED_50, ENHANCED_75),
    pair(ENHANCED_75, ENHANCED_100),
];

/// 17-point odd curves over 0..=1 for Saturation's Drums mode at knob
/// settings 0, 0.25, 0.5, 0.75 and 1.0, fitted to Kontakt 8.13.1
/// (`docs/re/kontakt-bindings/saturation-curves.tsv`).
const DRUMS_25: [f32; 17] = [
    0.0000, 0.0752, 0.1486, 0.2201, 0.2898, 0.3578, 0.4241, 0.4888, 0.5519, 0.6136, 0.6738, 0.7326,
    0.7900, 0.8461, 0.9009, 0.9545, 1.0000,
];
const DRUMS_50: [f32; 17] = [
    0.0000, 0.1342, 0.2611, 0.3766, 0.4787, 0.5665, 0.6406, 0.7019, 0.7520, 0.7924, 0.8247, 0.8504,
    0.8706, 0.8866, 0.8991, 0.9089, 0.9166,
];
const DRUMS_75: [f32; 17] = [
    0.0000, 0.2738, 0.4728, 0.5819, 0.6203, 0.6311, 0.6335, 0.6339, 0.6340, 0.6340, 0.6340, 0.6340,
    0.6340, 0.6340, 0.6340, 0.6340, 0.6340,
];
const DRUMS_100: [f32; 17] = [
    0.0000, 0.5289, 0.5761, 0.5779, 0.5780, 0.5780, 0.5780, 0.5780, 0.5780, 0.5780, 0.5780, 0.5780,
    0.5780, 0.5780, 0.5780, 0.5780, 0.5780,
];

static DRUMS_PAIRS: [[f32; 34]; 4] = [
    pair(ENHANCED_0, DRUMS_25),
    pair(DRUMS_25, DRUMS_50),
    pair(DRUMS_50, DRUMS_75),
    pair(DRUMS_75, DRUMS_100),
];

/// Saturation's Classic algorithm, shape −1..=1.
pub(crate) fn saturation(shape: f32) -> ir::EffectKind {
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Soft(shape.clamp(-1.0, 1.0)),
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::NONE,
    })
}

/// Saturation across all three algorithms: 0 Classic, 1 Enhanced, 2 Drums.
/// `shape` is saved in −1..=1 (KSP `$ENGINE_PAR_SHAPE` 0..1_000_000 maps linearly
/// onto −1..=1; Enhanced and Drums read `u = (shape + 1)/2` over 0..=1).
pub(crate) fn saturation_mode(algorithm: i32, shape: f32) -> Option<ir::EffectKind> {
    if algorithm == 0 {
        return Some(saturation(shape));
    }
    let shape = shape.clamp(-1.0, 1.0);
    let pairs = match algorithm {
        1 => &ENHANCED_PAIRS,
        2 => &DRUMS_PAIRS,
        _ => return None,
    };
    let u = ((shape + 1.0) * 0.5).clamp(0.0, 1.0);
    let scaled = u * 4.0;
    let seg = (scaled.floor() as usize).min(3);
    let t = (scaled - seg as f32).clamp(0.0, 1.0);
    Some(ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve: ir::DriveCurve::Blend {
            drive: 1.0,
            rows: Cow::Borrowed(pairs.get(seg)?),
            t,
        },
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::NONE,
    }))
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

/// Reports the routes the engine cannot apply (KT-25): any `FilterCutoff`,
/// `FilterResonance`, `EqBandGain`, or `EffectParam` route targeting an empty
/// slot or an incompatible effect, plus resonance routes to a formant (the
/// target table [ref] gives the formant its own `formantSharp`, 0x26; what
/// `filterQ`, 0x20, does there is not read yet).
pub(crate) fn unmoved(inserts: &[Option<ir::Effect>], mods: &[ir::Mod], notes: &mut Notes) {
    let at = |insert: u8| inserts.get(usize::from(insert)).and_then(Option::as_ref);
    let is_filter = |e: &ir::Effect| {
        matches!(
            e.kind,
            ir::EffectKind::Filter(_) | ir::EffectKind::Comb(_) | ir::EffectKind::Clocked(_)
        )
    };
    for m in mods {
        let gap = match m.route {
            ir::ModRoute {
                target: ir::ParamAddr::FilterResonance { insert },
                ..
            } if at(insert).is_some_and(|e| matches!(e.kind, ir::EffectKind::Formant(_))) => {
                "modulator -> formant filterQ"
            }
            ir::ModRoute {
                target:
                    ir::ParamAddr::FilterCutoff { insert } | ir::ParamAddr::FilterResonance { insert },
                ..
            } if !at(insert).is_some_and(is_filter) => "modulator -> effect without that control",
            ir::ModRoute {
                target: ir::ParamAddr::EqBandGain { insert, band },
                ..
            } if !(band < 4
                && at(insert).is_some_and(|e| matches!(e.kind, ir::EffectKind::Eq(_)))) =>
            {
                "modulator -> effect without that control"
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
        assert!(matches!(
            m.build(&m.saved),
            Some(ir::EffectKind::Overdrive(_))
        ));
    }

    #[test]
    fn the_damping_corner_reproduces_kontakts_poles() {
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
    fn saturation_modes_cover_classic_enhanced_and_drums() {
        assert!(matches!(
            saturation_mode(0, 0.5),
            Some(ir::EffectKind::Shaper(ir::Shaper {
                curve: ir::DriveCurve::Soft(s),
                ..
            })) if (s - 0.5).abs() < 1e-6
        ));
        assert!(matches!(
            saturation_mode(1, 0.0),
            Some(ir::EffectKind::Shaper(ir::Shaper {
                curve: ir::DriveCurve::Blend { .. },
                ..
            }))
        ));
        assert!(matches!(
            saturation_mode(2, 1.0),
            Some(ir::EffectKind::Shaper(ir::Shaper {
                curve: ir::DriveCurve::Blend { .. },
                ..
            }))
        ));
        assert_eq!(saturation_mode(3, 0.0), None);
    }

    #[test]
    fn kt25_unmoved_diagnoses_empty_and_incompatible_filter_and_eq_routes() {
        let gain_slot = Some(ir::Effect {
            kind: ir::EffectKind::Gain(1.0),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        let clocked_slot = Some(ir::Effect {
            kind: ir::EffectKind::Clocked(ir::Clocked {
                shape: ir::ClockedShape::Allpass,
                cutoff: 0.5,
                resonance: 0.5,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        let inserts = vec![gain_slot, None, clocked_slot];
        let mk = |name: &str, target: ir::ParamAddr| ir::Mod {
            name: name.into(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::Velocity, target, 0.5),
        };
        let mods = vec![
            mk("cut_on_gain", ir::ParamAddr::FilterCutoff { insert: 0 }),
            mk("res_on_empty", ir::ParamAddr::FilterResonance { insert: 1 }),
            mk(
                "eq_on_gain",
                ir::ParamAddr::EqBandGain { insert: 0, band: 0 },
            ),
            // Clocked filter at slot 2 IS valid for cutoff & resonance.
            mk("cut_on_clocked", ir::ParamAddr::FilterCutoff { insert: 2 }),
            mk(
                "res_on_clocked",
                ir::ParamAddr::FilterResonance { insert: 2 },
            ),
        ];
        let mut notes = Notes::default();
        unmoved(&inserts, &mods, &mut notes);
        let diags = notes.finish();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].feature, "modulator -> effect without that control");
        assert!(diags[0].detail.starts_with("3×"), "{diags:?}");
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
