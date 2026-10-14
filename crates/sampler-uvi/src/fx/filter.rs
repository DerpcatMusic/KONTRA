//! UVI `filter` effects onto sampler_ir::fx::filter.
//!
//! BiquadFilter, MoogFilter, Rez, XpanderFilter, VowelFilter, CombFilter and DigitalFilter are native laws of the engine's cascade core (UVI
//! Workstation 4.0.9, checked under kemu; see `sampler-core::dsp::filter::cascade`). OnePole is
//! the SVF one-pole (its Freq to coefficient law is not proved). Every other UVI filter element
//! is named here with the exact thing that is not proved, so the report says why it is dropped
//! instead of calling it an unknown element. Key tracking is taken from the keygroup's middle
//! key (the native uses the playing note) and reported.

use roxmltree::Node;
use sampler_ir::fx::filter::{Config, CutoffLaw, Gap, ResonanceLaw, Response, Topology, Uvi, Variant};
use sampler_ir::Processor;

/// The UVI filter elements and why each one without a mapping has none.
const GAPS: &[(&str, &str)] = &[
    ("BrickwallFilter", "Slope (0..2) steepness and the native brickwall design are not extracted"),
    ("CrossOverFilter", "a three-way split feeds separate output buses; neither the crossover alignment nor the bus routing is modelled"),
    ("FxFilter", "legacy FX Filter low/high frequency and Q laws are not extracted"),
    ("HighpassFilterRL", "RL inductor model (Model 1..2, Inductor 2..3) and Drive law are not extracted"),
    ("IIRLFOFilter", "LFO-driven IIR design is not extracted"),
    ("PhasorFilter", "allpass chain order law (1..12), CutoffMode and Spread are not extracted"),
    ("TalkBox", "vowel tables for VowelMode (0..4) and FilterMode are not extracted"),
    ("UviFilter", "MappingMode (0..2) cutoff mapping and the filter bank behind it are not extracted"),
];

/// The elements with a native law.
const NATIVE: &[&str] = &["OnePole", "BiquadFilter", "MoogFilter", "Rez", "XpanderFilter", "VowelFilter", "CombFilter", "DigitalFilter"];

/// Whether `tag` is a UVI filter element this module owns.
pub(crate) fn handles(tag: &str) -> bool {
    NATIVE.contains(&tag) || GAPS.iter().any(|(name, _)| *name == tag)
}

/// What a UVI filter element becomes: processors plus the parts it could not take.
pub(crate) struct Translated {
    pub processors: Vec<Processor>,
    /// `(feature, value)` pairs for controls that were set but are not modelled.
    pub unsupported: Vec<(&'static str, f64)>,
}

/// Translate one filter element. `number` reads an attribute with its default; `key` is the
/// middle key of the keygroup, which key tracking is measured from.
pub(crate) fn translate(
    node: Node,
    key: Option<u8>,
    number: impl Fn(&str, f64) -> Result<f64, String>,
) -> Result<Result<Translated, Gap>, String> {
    let tag = node.tag_name().name();
    if tag != "OnePole" && NATIVE.contains(&tag) {
        let tracking = number("KeyTracking", 0.0)?;
        let key = f64::from(key.unwrap_or(60));
        // Native 0x1419ef640 order: 2^(((note - 60) kt) / 12) times Freq, in f32.
        let hertz = (((key - 60.0) * tracking) as f32 * (1.0f32 / 12.0)).exp2()
            * number("Freq", 1000.0)?.clamp(20.0, 20_000.0) as f32;
        // Xpander takes the tracked frequency as is (its update clamps to the rate).
        let hertz = if tag == "XpanderFilter" { hertz } else { hertz.clamp(20.0, 20_000.0) };
        let q = number("Q", 0.0)?.clamp(0.0, 1.0) as f32;
        let mode = number("Mode", if tag == "BiquadFilter" { 1.0 } else { 0.0 })?.clamp(0.0, 3.0) as u8;
        let uvi = match tag {
            "BiquadFilter" => Uvi::Biquad { mode, hertz, q },
            "MoogFilter" => Uvi::Moog { hertz, q },
            "XpanderFilter" => Uvi::Xpander {
                mode: number("Mode", 3.0)?.clamp(0.0, 36.0) as u8,
                hertz,
                q,
                distortion: number("DistortionType", 0.0)?.clamp(0.0, 2.0) as u8,
                drive: number("Drive", 0.0)?.clamp(-20.0, 20.0) as f32,
                fat: number("Fat", 1.0)?.clamp(0.0, 1.0) as f32,
                algorithm: number("Algorithm", 0.0)?.clamp(0.0, 1.0) as u8,
                oversampling: number("Oversampling", 1.0)? != 0.0,
            },
            "VowelFilter" => Uvi::Vowel {
                morph: number("Morph", 0.0)?.clamp(0.0, 1.0) as f32,
                q: number("Q", 10.0)?.clamp(10.0, 20.0) as f32,
                formant: number("Formant", 0.0)?.clamp(-1.0, 1.0) as f32,
                a: number("FilterA", 0.0)?.clamp(0.0, 10.0) as u8,
                b: number("FilterB", 2.0)?.clamp(0.0, 10.0) as u8,
                mode: number("Mode", 1.0)?.clamp(0.0, 2.0) as u8,
                count: number("NumFormants", 3.0)?.clamp(1.0, 4.0) as u8,
            },
            // ponytail: the native ring (0.05 s past the block) bounds the delay; tracked
            // cutoffs under 20 Hz are held at 20 Hz instead of wrapping that ring.
            "DigitalFilter" => Uvi::Digital {
                kind: number("Type", 0.0)?.clamp(0.0, 6.0) as u8,
                // Slope 0..7 = order (table 0x141e731a8).
                order: [1, 2, 3, 4, 6, 8, 12, 16][number("Slope", 1.0)?.clamp(0.0, 7.0) as usize],
                hertz: number("Freq", 1000.0)?.clamp(10.0, 22_000.0) as f32,
                track: ((key - 60.0) * tracking.clamp(-1.0, 1.0)) as f32,
                spread: number("Spread", 0.0)?.clamp(-48.0, 48.0) as f32,
                q: number("Q", 0.707)?.clamp(0.018, 28.284) as f32,
                gain: number("Gain", 0.0)?.clamp(-30.0, 30.0) as f32,
                bandwidth: number("Bandwidth", 1.0)?.clamp(0.01, 10.0) as f32,
            },
            "CombFilter" => Uvi::Comb { hertz, q, negative: mode == 1 },
            _ => Uvi::Rez { mode: mode.min(2), hertz, q },
        };
        let mut config = Config::new(Topology::Cascade, Response::LowPass, 2);
        config.variant = Variant::Uvi(uvi);
        return Ok(Ok(Translated {
            processors: config.processors(None).map_err(|gap| gap.reason.to_string())?,
            unsupported: if tracking == 0.0 { Vec::new() } else { vec![("filter key tracking (keygroup middle key)", tracking)] },
        }));
    }
    if tag != "OnePole" {
        let reason = GAPS
            .iter()
            .find(|(name, _)| *name == tag)
            .map_or("not a filter element", |(_, reason)| *reason);
        return Ok(Err(Gap { name: GAPS.iter().find(|(n, _)| *n == tag).map_or("filter", |(n, _)| n), reason }));
    }
    let tracking = number("KeyTracking", 0.0)?;
    let mut config = Config::new(
        Topology::Svf,
        if number("Mode", 0.0)? == 0.0 { Response::LowPass } else { Response::HighPass },
        1,
    );
    config.cutoff = number("Freq", 1000.0)?;
    config.cutoff_law = CutoffLaw::Hertz;
    config.resonance = std::f64::consts::FRAC_1_SQRT_2;
    config.resonance_law = ResonanceLaw::Q;
    // Key tracking is measured from the keygroup's middle key, which the native module takes
    // from the playing note; until that is modelled the cutoff is tracked from the middle key
    // and the control is reported.
    config.key_tracking = tracking;
    let key = Some(key.unwrap_or(60));
    let mut processors = config.processors(key).map_err(|gap| gap.reason.to_string())?;
    // The native range is 20..20000 Hz.
    for p in &mut processors {
        if let Processor::Filter(f) = p {
            if let sampler_ir::Frequency::Hertz(hz) = &mut f.cutoff {
                *hz = hz.clamp(20.0, 20000.0);
            }
        }
    }
    Ok(Ok(Translated {
        processors,
        unsupported: if tracking == 0.0 {
            Vec::new()
        } else {
            vec![("OnePole key tracking (keygroup middle key)", tracking)]
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(xml).unwrap()
    }

    fn run(xml: &str, key: Option<u8>) -> Result<Translated, Gap> {
        let doc = parse(xml);
        let node = doc.root_element();
        translate(node, key, |name, default| {
            Ok(node.attribute(name).map_or(default, |v| v.parse().unwrap()))
        })
        .unwrap()
    }

    #[test]
    fn one_pole_tracks_the_keygroup_key_in_octaves() {
        let t = run(r#"<OnePole Freq="1000" KeyTracking="1" Mode="1"/>"#, Some(72)).unwrap();
        let [Processor::Filter(f)] = t.processors[..] else { panic!() };
        assert_eq!(f.kind, sampler_ir::FilterKind::HighPass { poles: 1 });
        assert_eq!(f.cutoff, sampler_ir::Frequency::Hertz(2000.0));
        assert_eq!(t.unsupported.len(), 1);
        let t = run(r#"<OnePole Freq="30000"/>"#, None).unwrap();
        let [Processor::Filter(f)] = t.processors[..] else { panic!() };
        assert_eq!(f.cutoff, sampler_ir::Frequency::Hertz(20000.0));
        assert!(t.unsupported.is_empty());
    }

    #[test]
    fn native_elements_are_cascade_laws() {
        let t = run(r#"<MoogFilter Freq="5000" Q="0.9"/>"#, None).unwrap();
        let [Processor::FilterBank(c)] = t.processors[..] else { panic!() };
        assert_eq!(c.variant, Variant::Uvi(Uvi::Moog { hertz: 5000.0, q: 0.9 }));
        let t = run(r#"<XpanderFilter Freq="500" Algorithm="1"/>"#, None).unwrap();
        let [Processor::FilterBank(c)] = t.processors[..] else { panic!() };
        assert!(matches!(c.variant, Variant::Uvi(Uvi::Xpander { mode: 3, algorithm: 1, oversampling: true, .. })));
        let t = run(r#"<VowelFilter/>"#, None).unwrap();
        let [Processor::FilterBank(c)] = t.processors[..] else { panic!() };
        assert!(matches!(c.variant, Variant::Uvi(Uvi::Vowel { b: 2, mode: 1, count: 3, .. })));
        let t = run(r#"<CombFilter Mode="1" Q="0.5"/>"#, None).unwrap();
        let [Processor::FilterBank(c)] = t.processors[..] else { panic!() };
        assert_eq!(c.variant, Variant::Uvi(Uvi::Comb { hertz: 1000.0, q: 0.5, negative: true }));
        let t = run(r#"<DigitalFilter Type="2" Slope="7" KeyTracking="0.5"/>"#, Some(72)).unwrap();
        let [Processor::FilterBank(c)] = t.processors[..] else { panic!() };
        assert!(matches!(c.variant, Variant::Uvi(Uvi::Digital { kind: 2, order: 16, track: 6.0, .. })));
        let t = run(r#"<BiquadFilter/>"#, None).unwrap();
        let [Processor::FilterBank(c)] = t.processors[..] else { panic!() };
        assert_eq!(c.variant, Variant::Uvi(Uvi::Biquad { mode: 1, hertz: 1000.0, q: 0.0 }));
    }

    #[test]
    fn every_other_filter_names_what_is_missing() {
        for (tag, reason) in GAPS {
            let gap = run(&format!("<{tag}/>"), None).err().unwrap();
            assert_eq!((gap.name, gap.reason), (*tag, *reason));
            assert!(handles(tag));
        }
        assert!(!handles("Reverb"));
    }
}
