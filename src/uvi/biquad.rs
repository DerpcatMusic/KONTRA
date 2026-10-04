//! Original float32 direct-form-I Biquad Filter mathematics. Public controls:
//! https://lua.uvi.net/_elements.html#biquad-filter . Authored native comparisons
//! cover fresh voices, mono/stereo/multichannel histories and block segmentation.
//! The stereo sum order differs from the other channel counts. No vendor source
//! or preset/sample content is included. Connected and live controls stay gated.
use super::{
    dsp::{Frame, MAX_CHANNELS},
    host::ParameterValue,
    program::ProgramNode,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI BiquadFilter static unconnected highpass/lowpass has authored native comparisons at 32/44.1/48 kHz; native/Rust trigonometric rounding produced measured coefficient residuals up to 1.8e-7 (113 ULP for a small coefficient), output residuals up to 2.5e-4 in 32768-frame tone probes and 4.7e-7 for owned fixed/default settings; these are observed comparisons, not a universal error bound; other modes, key tracking, live controls and bypass lifecycle are unsupported";
const DEFAULTS: [(&str, f64); 5] = [
    ("Bypass", 0.),
    ("Freq", 1000.),
    ("Q", 0.),
    ("Mode", 1.),
    ("KeyTracking", 0.),
];
fn index(name: &str) -> Result<usize> {
    DEFAULTS
        .iter()
        .position(|p| p.0 == name)
        .with_context(|| format!("Unsupported BiquadFilter parameter {name}"))
}
fn number(value: &ParameterValue) -> Result<f64> {
    match value {
        ParameterValue::Number(v) if v.is_finite() => Ok(*v),
        ParameterValue::Boolean(v) => Ok(f64::from(u8::from(*v))),
        _ => bail!("BiquadFilter requires finite numeric parameters"),
    }
}
fn admissible(p: &[f64; 5]) -> Result<()> {
    ensure!(
        p.iter().all(|v| v.is_finite()),
        "Invalid BiquadFilter parameter"
    );
    ensure!(p[0] == 0., "BiquadFilter bypass lifecycle is unverified");
    ensure!(p[4] == 0., "BiquadFilter key tracking is unverified");
    ensure!(
        matches!(p[3], 0. | 1.),
        "BiquadFilter bandpass/notch modes are unverified"
    );
    ensure!(
        (0. ..=0.1).contains(&p[2]),
        "BiquadFilter resonance outside measured range"
    );
    ensure!(
        (150. ..=20_000.).contains(&p[1]),
        "BiquadFilter frequency outside measured range"
    );
    ensure!(
        p[3] == 0. || (p[1] >= 1000. && p[2] == 0.),
        "BiquadFilter lowpass outside measured range"
    );
    Ok(())
}
fn values(attributes: &BTreeMap<String, String>) -> Result<[f64; 5]> {
    let mut p = DEFAULTS.map(|p| p.1);
    for (name, raw) in attributes {
        if name == "Name" {
            continue;
        }
        p[index(name)?] = raw
            .parse()
            .with_context(|| format!("Invalid BiquadFilter parameter {name}"))?;
    }
    admissible(&p)?;
    Ok(p)
}
fn parameters(node: &ProgramNode) -> Result<[f64; 5]> {
    ensure!(supports(&node.kind), "Unsupported UVI biquad {}", node.kind);
    values(&node.attributes)
}
pub fn supports(kind: &str) -> bool {
    kind == "BiquadFilter"
}
pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}
fn check_write(p: &[f64; 5], name: &str, value: &ParameterValue) -> Result<()> {
    ensure!(
        number(value)? == p[index(name)?],
        "BiquadFilter live controls and bypass transitions are unverified"
    );
    Ok(())
}
/// Enforce the static boundary even while a keygroup has no constructed voice.
pub fn validate_static_write(
    attributes: &BTreeMap<String, String>,
    name: &str,
    value: &ParameterValue,
) -> Result<()> {
    check_write(&values(attributes)?, name, value)
}

/// Static, unconnected physical controls. Each owner gets independent history;
/// reset reconstructs a fresh voice rather than reusing another owner's state.
pub struct BiquadFilter {
    parameters: [f64; 5],
    channels: usize,
    coefficients: [f32; 5],
    history: [[f32; 4]; MAX_CHANNELS],
}
impl BiquadFilter {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Invalid BiquadFilter channel count"
        );
        ensure!(
            [32_000., 44_100., 48_000.].contains(&rate),
            "Unmeasured BiquadFilter sample rate"
        );
        let p = parameters(node)?;
        // RBJ high/lowpass with the independently measured native resonance
        // scales. Preserve float32 cutoff normalization and coefficient steps.
        let frequency = (p[1] as f32).min(rate as f32 * 0.499_f32);
        let angle = (frequency / rate as f32) * std::f32::consts::TAU;
        let (sine, cosine) = angle.sin_cos();
        let highpass = p[3] == 0.;
        let alpha = sine / (1. + p[2] as f32 * if highpass { 24. } else { 12. });
        let normalization = 1. / (1. + alpha);
        let term = 1. + if highpass { cosine } else { -cosine };
        let a1 = (cosine * -2.) * normalization;
        let a2 = (1. - alpha) * normalization;
        let b0 = (term * 0.5) * normalization;
        let b1 = term * normalization * if highpass { -1. } else { 1. };
        Ok(Self {
            parameters: p,
            channels,
            coefficients: [a1, a2, b0, b1, b0],
            history: [[0.; 4]; MAX_CHANNELS],
        })
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        Ok(ParameterValue::Number(self.parameters[index(name)?]))
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        check_write(&self.parameters, name, value)
    }
    pub fn clear(&mut self) {
        self.history.fill([0.; 4]);
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        let [a1, a2, b0, b1, b2] = self.coefficients;
        let stereo = self.channels == 2;
        let bias = 1e-20_f32;
        for frame in frames {
            for (x, history) in frame[..self.channels].iter_mut().zip(&mut self.history) {
                let [x1, x2, y1, y2] = *history;
                let wet = if stereo {
                    (((x1 * b1 + *x * b0) + x2 * b2) - y1 * a1) - y2 * a2
                } else {
                    (((x2 * b2 + x1 * b1) + (*x * b0 + bias)) - y1 * a1) - y2 * a2
                };
                *history = [*x, x1, wet, y1];
                *x = if stereo { wet } else { wet - bias };
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(attributes: &str) -> ProgramNode {
        let xml = format!("<BiquadFilter {attributes}/>");
        let document = roxmltree::Document::parse(&xml).unwrap();
        ProgramNode {
            parent: None,
            kind: "BiquadFilter".into(),
            name: None,
            attributes: document
                .root_element()
                .attributes()
                .map(|a| (a.name().into(), a.value().into()))
                .collect(),
            text: String::new(),
        }
    }
    #[test]
    fn authored_native_fixed_stereo_impulses() {
        for (attributes, native) in [
            (
                "Freq='161.43584' Q='.059999999' Mode='0'",
                [
                    0.9913036823272705,
                    -0.01746082305908203,
                    -0.017592132091522217,
                    -0.0177133958786726,
                    -0.01782473549246788,
                    -0.017926272004842758,
                    -0.018018130213022232,
                    -0.01810043305158615,
                ],
            ),
            (
                "Freq='6367.9556' Q='0' Mode='1'",
                [
                    0.09416542947292328,
                    0.2610778510570526,
                    0.28180962800979614,
                    0.17875602841377258,
                    0.0960492193698883,
                    0.04753077030181885,
                    0.022388488054275513,
                    0.010204214602708817,
                ],
            ),
        ] {
            let mut fx = BiquadFilter::new(&node(attributes), 2, 48000.).unwrap();
            let mut frames = [[0.; MAX_CHANNELS]; 8];
            frames[0][0] = 1.;
            fx.process(&mut frames).unwrap();
            for (frame, native) in frames.iter().zip(native) {
                assert!((f64::from(frame[0]) - native).abs() < 1e-6);
                assert_eq!(frame[1], 0.);
            }
        }
    }
    #[test]
    fn unverified_modes_ranges_rates_and_live_controls_stay_gated() {
        for attributes in [
            "Mode='2'",
            "Mode='.5'",
            "KeyTracking='.1'",
            "Q='.592' Mode='0'",
            "Q='.01' Mode='1'",
            "Freq='100' Mode='0'",
            "Freq='999' Mode='1'",
            "Freq='NaN'",
            "Unknown='0'",
            "Bypass='1'",
            "Bypass='2'",
        ] {
            assert!(validate(&node(attributes)).is_err(), "{attributes}");
        }
        for (channels, rate) in [(0, 48000.), (13, 48000.), (2, 96000.), (2, 47999.)] {
            assert!(BiquadFilter::new(&node(""), channels, rate).is_err());
        }
        let mut fx = BiquadFilter::new(&node(""), 2, 48000.).unwrap();
        for (name, value) in [
            ("Freq", 2000.),
            ("Q", 0.1),
            ("Mode", 0.),
            ("Bypass", 1.),
            ("KeyTracking", 0.1),
            ("Unknown", 0.),
        ] {
            assert!(
                fx.set_parameter(name, &ParameterValue::Number(value))
                    .is_err()
            );
        }
        fx.set_parameter("Bypass", &ParameterValue::Boolean(false))
            .unwrap();
        assert_eq!(fx.parameter("Freq").unwrap(), ParameterValue::Number(1000.));
    }
    #[test]
    fn static_write_uses_prepared_physical_settings() {
        let mut prepared = node("").attributes;
        prepared.insert("Freq".into(), "2000".into());
        validate_static_write(&prepared, "Freq", &ParameterValue::Number(2000.)).unwrap();
        assert!(validate_static_write(&prepared, "Freq", &ParameterValue::Number(1000.)).is_err());
        validate_static_write(&prepared, "Bypass", &ParameterValue::Boolean(false)).unwrap();
        assert!(
            validate_static_write(&prepared, "Freq", &ParameterValue::Text("2000".into())).is_err()
        );
    }
    #[test]
    fn rejected_edit_preserves_history_and_fresh_reset_is_independent() {
        let n = node("Freq='161.43584' Q='.059999999' Mode='0'");
        let mut one = BiquadFilter::new(&n, 1, 48000.).unwrap();
        let mut peer = BiquadFilter::new(&n, 1, 48000.).unwrap();
        let mut input = [[0.; MAX_CHANNELS]; 64];
        input[0][0] = 1.;
        let mut expected = input;
        peer.process(&mut expected).unwrap();
        let mut actual = input;
        one.process(&mut actual[..17]).unwrap();
        assert!(
            one.set_parameter("Freq", &ParameterValue::Number(2000.))
                .is_err()
        );
        one.process(&mut actual[17..]).unwrap();
        assert_eq!(actual, expected);
        one.clear();
        let mut reset = input;
        one.process(&mut reset).unwrap();
        assert_eq!(reset, expected);
        assert!(validate(&node("Bypass='1'")).is_err());
    }
}
