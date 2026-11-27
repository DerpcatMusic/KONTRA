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

/// Kontakt's Distortion (Tube mode) measured 97-point bipolar curves evenly over −1..=1
/// at DRIVE 0.25, 0.5, 0.75, and 1.0 (DRIVE 0 is identity ramp).
const TUBE_POINTS: usize = 97;

#[allow(clippy::indexing_slicing)]
const fn tube_identity() -> [f32; TUBE_POINTS] {
    let mut out = [0.0; TUBE_POINTS];
    let mut i = 0;
    while i < TUBE_POINTS {
        out[i] = -1.0 + 2.0 * (i as f32) / ((TUBE_POINTS - 1) as f32);
        i += 1;
    }
    out
}

#[allow(clippy::indexing_slicing)]
const fn pair_tube(a: [f32; TUBE_POINTS], b: [f32; TUBE_POINTS]) -> [f32; TUBE_POINTS * 2] {
    let mut out = [0.0; TUBE_POINTS * 2];
    let mut i = 0;
    while i < TUBE_POINTS {
        out[i] = a[i];
        out[i + TUBE_POINTS] = b[i];
        i += 1;
    }
    out
}

#[rustfmt::skip]
const TUBE_25: [f32; TUBE_POINTS] = [
    -0.7305, -0.7127, -0.7059, -0.6960, -0.6869, -0.6777, -0.6683, -0.6590,
    -0.6495, -0.6404, -0.6314, -0.6223, -0.6131, -0.6038, -0.5946, -0.5845,
    -0.5769, -0.5675, -0.5577, -0.5493, -0.5404, -0.5311, -0.5208, -0.5122,
    -0.5031, -0.4792, -0.4543, -0.4289, -0.4029, -0.3765, -0.3498, -0.3228,
    -0.2959, -0.2691, -0.2427, -0.2169, -0.1919, -0.1680, -0.1453, -0.1242,
    -0.1046, -0.0868, -0.0707, -0.0564, -0.0437, -0.0323, -0.0220, -0.0129,
    0.0000, 0.0197, 0.0427, 0.0637, 0.0847, 0.1056, 0.1265, 0.1474,
    0.1683, 0.1892, 0.2101, 0.2309, 0.2519, 0.2728, 0.2936, 0.3145,
    0.3354, 0.3563, 0.3771, 0.3980, 0.4189, 0.4398, 0.4606, 0.4815,
    0.5023, 0.5230, 0.5438, 0.5646, 0.5854, 0.6061, 0.6272, 0.6479,
    0.6683, 0.6861, 0.7081, 0.7285, 0.7493, 0.7701, 0.7910, 0.8120,
    0.8329, 0.8535, 0.8743, 0.8950, 0.9160, 0.9365, 0.9584, 0.9758,
    1.0097,
];

#[rustfmt::skip]
const TUBE_50: [f32; TUBE_POINTS] = [
    -0.5698, -0.5652, -0.5639, -0.5617, -0.5597, -0.5576, -0.5553, -0.5532,
    -0.5513, -0.5494, -0.5468, -0.5444, -0.5417, -0.5389, -0.5367, -0.5347,
    -0.5340, -0.5334, -0.5302, -0.5271, -0.5272, -0.5206, -0.5191, -0.5154,
    -0.5079, -0.4859, -0.4619, -0.4370, -0.4111, -0.3844, -0.3569, -0.3288,
    -0.3002, -0.2714, -0.2426, -0.2142, -0.1863, -0.1595, -0.1340, -0.1103,
    -0.0887, -0.0697, -0.0535, -0.0402, -0.0300, -0.0229, -0.0179, -0.0192,
    0.0000, 0.0523, 0.1184, 0.1650, 0.2054, 0.2393, 0.2681, 0.2926,
    0.3137, 0.3319, 0.3480, 0.3623, 0.3754, 0.3874, 0.3987, 0.4094,
    0.4197, 0.4297, 0.4395, 0.4492, 0.4587, 0.4682, 0.4777, 0.4872,
    0.4966, 0.5054, 0.5143, 0.52353, 0.5324, 0.5414, 0.5509, 0.5597,
    0.5687, 0.5767, 0.5867, 0.5956, 0.6049, 0.6141, 0.6235, 0.6329,
    0.6423, 0.6513, 0.6606, 0.6697, 0.6792, 0.6884, 0.6983, 0.7062,
    0.7215,
];

#[rustfmt::skip]
const TUBE_75: [f32; TUBE_POINTS] = [
    -0.5660, -0.5657, -0.5645, -0.5633, -0.5621, -0.5608, -0.5593, -0.5581,
    -0.5554, -0.5654, -0.5552, -0.5467, -0.5453, -0.5477, -0.5471, -0.5436,
    -0.5454, -0.5470, -0.5453, -0.5352, -0.5391, -0.5314, -0.5313, -0.5292,
    -0.5163, -0.4921, -0.4712, -0.4481, -0.4242, -0.3993, -0.3733, -0.3465,
    -0.3190, -0.2909, -0.2625, -0.2340, -0.2057, -0.1780, -0.1513, -0.1262,
    -0.1030, -0.0823, -0.0645, -0.0499, -0.0390, -0.0344, -0.0266, -0.0251,
    0.0000, 0.0637, 0.1460, 0.2120, 0.2672, 0.3103, 0.3437, 0.3697,
    0.3897, 0.4053, 0.4175, 0.4272, 0.4348, 0.4418, 0.4477, 0.4528,
    0.4572, 0.4612, 0.4648, 0.4681, 0.4713, 0.4743, 0.4774, 0.4794,
    0.4863, 0.4883, 0.4904, 0.4922, 0.4938, 0.4954, 0.4971, 0.4982,
    0.4997, 0.5012, 0.5034, 0.5049, 0.5066, 0.5098, 0.5119, 0.5113,
    0.5167, 0.5181, 0.5198, 0.5216, 0.5233, 0.5252, 0.5272, 0.5288,
    0.5324,
];

#[rustfmt::skip]
const TUBE_100: [f32; TUBE_POINTS] = [
    -0.5421, -0.5437, -0.5448, -0.5456, -0.5475, -0.5472, -0.5484, -0.5488,
    -0.5499, -0.5485, -0.5401, -0.5313, -0.5303, -0.5325, -0.5338, -0.5310,
    -0.5334, -0.5352, -0.5351, -0.5278, -0.5325, -0.5260, -0.5248, -0.5243,
    -0.5180, -0.5010, -0.4830, -0.4638, -0.4451, -0.4232, -0.4006, -0.3768,
    -0.3520, -0.3263, -0.2997, -0.2722, -0.2444, -0.2164, -0.1887, -0.1615,
    -0.1356, -0.1113, -0.0895, -0.0711, -0.0563, -0.0478, -0.0486, -0.0455,
    0.0000, 0.1025, 0.2203, 0.2878, 0.3400, 0.3774, 0.4048, 0.4247,
    0.4391, 0.4495, 0.4580, 0.4647, 0.4696, 0.4736, 0.4767, 0.4812,
    0.4854, 0.4890, 0.4921, 0.4948, 0.4971, 0.5002, 0.5042, 0.5079,
    0.5112, 0.5134, 0.5156, 0.5175, 0.5188, 0.5204, 0.5218, 0.5225,
    0.52346, 0.5248, 0.5258, 0.5266, 0.5276, 0.5286, 0.5292, 0.5295,
    0.5295, 0.5296, 0.5300, 0.5294, 0.5340, 0.5300, 0.5318, 0.5320,
    0.5333,
];

static TUBE_PAIRS: [[f32; TUBE_POINTS * 2]; 4] = [
    pair_tube(tube_identity(), TUBE_25),
    pair_tube(TUBE_25, TUBE_50),
    pair_tube(TUBE_50, TUBE_75),
    pair_tube(TUBE_75, TUBE_100),
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
        0 => {
            let scaled = drive * 4.0;
            let seg = (scaled.floor() as usize).min(3);
            let t = (scaled - seg as f32).clamp(0.0, 1.0);
            ir::DriveCurve::AsymBlend {
                rows: Cow::Borrowed(TUBE_PAIRS.get(seg)?),
                t,
            }
        }
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
