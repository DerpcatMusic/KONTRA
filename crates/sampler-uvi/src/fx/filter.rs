//! UVI `filter` effects onto sampler_ir::fx::filter.
//!
//! Only OnePole has a checked native kernel (its coefficient is authored in the isolated byte
//! checks; the public Freq to coefficient conversion is the one-pole `1 - exp(-2 pi f / fs)`
//! stand-in, the multiplier is not proved). Every other UVI filter element is named here with
//! the exact thing that is not proved, so the report says why it is dropped instead of calling
//! it an unknown element.

use roxmltree::Node;
use sampler_ir::fx::filter::{Config, CutoffLaw, Gap, ResonanceLaw, Response, Topology};
use sampler_ir::Processor;

/// The UVI filter elements and why each one without a mapping has none.
const GAPS: &[(&str, &str)] = &[
    ("BiquadFilter", "Mode numbering (0..3), the 0..1 Q law and the native biquad coefficients are not extracted"),
    ("BrickwallFilter", "Slope (0..2) steepness and the native brickwall design are not extracted"),
    ("CombFilter", "comb topology (Mode 0..1), the 0..1 Q-to-feedback law and delay length law are not extracted"),
    ("CrossOverFilter", "a three-way split feeds separate output buses; neither the crossover alignment nor the bus routing is modelled"),
    ("DigitalFilter", "Slope (0..7) and Spread (-48..48) semantics and the Q law are not extracted"),
    ("FxFilter", "legacy FX Filter low/high frequency and Q laws are not extracted"),
    ("HighpassFilterRL", "RL inductor model (Model 1..2, Inductor 2..3) and Drive law are not extracted"),
    ("IIRLFOFilter", "LFO-driven IIR design is not extracted"),
    ("MoogFilter", "Moog ladder topology, 0..1 Q law and saturation are not extracted"),
    ("PhasorFilter", "allpass chain order law (1..12), CutoffMode and Spread are not extracted"),
    ("Rez", "Mode (0..2) resonator topology is not extracted"),
    ("TalkBox", "vowel tables for VowelMode (0..4) and FilterMode are not extracted"),
    ("UviFilter", "MappingMode (0..2) cutoff mapping and the filter bank behind it are not extracted"),
    ("VowelFilter", "formant tables for the 11 FilterA/FilterB vowels and the Formant/Morph laws are not extracted"),
    ("XpanderFilter", "the 37 Xpander modes, Algorithm, DistortionType and Fat laws are not extracted"),
];

/// Whether `tag` is a UVI filter element this module owns.
pub(crate) fn handles(tag: &str) -> bool {
    tag == "OnePole" || GAPS.iter().any(|(name, _)| *name == tag)
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
    fn every_other_filter_names_what_is_missing() {
        for (tag, reason) in GAPS {
            let gap = run(&format!("<{tag}/>"), None).err().unwrap();
            assert_eq!((gap.name, gap.reason), (*tag, *reason));
            assert!(handles(tag));
        }
        assert!(!handles("Reverb"));
    }
}
