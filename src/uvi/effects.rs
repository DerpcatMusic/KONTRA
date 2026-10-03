//! Native-unit, 1..12-channel UVI EQ and impulse processors.
//!
//! Parameter facts: https://lua.uvi.net/_elements.html and Falcon manual
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_manual_PRINT.pdf
//! Observed XML names/version fields are format facts, not copied DSP/source.
//! The filters use original bilinear-transform mathematics. The FFT engine is
//! shared with Kontakt, but its Kontakt-specific preparation laws are not used.
//! DigitalEq and ThreeBandShelves equations were compared with the official
//! Workstation VST2 using authored PCM16 impulses (all shapes/eight slopes,
//! Q=0.3/0.70710678/2, bandwidth=0.2/1/2, gain scales=0/1/2).
//! ParametricEQ fixed bands were compared using authored native impulses;
//! legacy shelf gain/slope conventions are preserved independently of DigitalEq.
//! Authored native IR probes establish fixed bus routing, power normalization,
//! reverb duration windows, stereo input diffusion and sequential width mixing.

use super::{
    dsp::{Frame, MAX_CHANNELS},
    exciter::{self, Exciter},
    host::ParameterValue,
    program::ProgramNode,
    resampling,
    sample::Sample,
};
use crate::fx::convolution::Convolver;
use anyhow::{Context, Result, bail, ensure};
use realfft::num_complex::Complex64;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

pub const FIDELITY_DIAGNOSTIC: &str = "UVI IR reconstruction endpoint behavior for uncommon sample-rate ratios and non-48-kHz IReverb preparation remain unverified against native audio";
const IR_BYTES: usize = 128 << 20;

type Parameters = BTreeMap<String, ParameterValue>;

pub fn supports(kind: &str) -> bool {
    matches!(
        kind,
        "DigitalEq"
            | "ParametricEQ"
            | "ThreeBandShelves"
            | "Convolver"
            | "SampledReverb"
            | "Exciter"
    )
}

fn number(value: &ParameterValue) -> Result<f64> {
    match value {
        ParameterValue::Number(n) if n.is_finite() => Ok(*n),
        ParameterValue::Boolean(b) => Ok(f64::from(u8::from(*b))),
        _ => bail!("UVI effect parameter requires a finite number or boolean"),
    }
}

fn n(p: &Parameters, name: &str) -> f64 {
    number(&p[name]).expect("validated parameter")
}
fn on(p: &Parameters, name: &str) -> bool {
    n(p, name) != 0.
}
fn db(value: f64) -> f64 {
    10f64.powf(value / 20.)
}

fn band(name: &str) -> Option<(&str, usize)> {
    let split = name.find(|c: char| c.is_ascii_digit())?;
    let index = name[split..].parse::<usize>().ok()?;
    (1..=16)
        .contains(&index)
        .then_some((&name[..split], index - 1))
}

/// Validation performs no file access; it rejects unsupported active controls.
/// Resource paths are deliberately absent from error messages.
pub fn validate(node: &ProgramNode) -> Result<()> {
    if exciter::supports(&node.kind) {
        return exciter::validate(node);
    }
    parameters(node).map(|_| ())
}

fn check(kind: &str, name: &str, value: &ParameterValue) -> Result<()> {
    if name == "SamplePath" && matches!(kind, "Convolver" | "SampledReverb") {
        ensure!(
            matches!(value, ParameterValue::Text(s) if !s.is_empty()),
            "Empty UVI impulse resource"
        );
        return Ok(());
    }
    let (low, high, integer) = match (kind, name) {
        (_, "Bypass") => (0., 1., true),
        ("DigitalEq", "StereoMode") => (0., 1., true),
        ("DigitalEq", "Transpose") => (-10., 10., false),
        ("DigitalEq", "KeyTracking") => (-1., 1., false),
        ("DigitalEq", "OverallGain") => (-30., 30., false),
        ("DigitalEq", "GainScale") => (-2., 2., false),
        ("ThreeBandShelves", "FreqLowMid") => (20., 1000., false),
        ("ThreeBandShelves", "FreqMidHigh") => (2000., 20000., false),
        ("ThreeBandShelves", "GainLow" | "GainMid" | "GainHigh") => (-24., 24., false),
        ("Convolver" | "SampledReverb", "Dry" | "Wet") => (0., 1., false),
        ("Convolver" | "SampledReverb", "NormalizePower") => (0., 1., true),
        ("Convolver", "ConvolverVersion") | ("SampledReverb", "SampledReverbVersion") => {
            (1., 1., true)
        }
        ("SampledReverb", "Time") => (0.1, 1., false),
        ("SampledReverb", "DampingLow" | "DampingHigh" | "Width") => (-1., 1., false),
        ("SampledReverb", "PreDelay") => (0., 100., false),
        ("SampledReverb", "UseWindowAtOriginalSize") => (0., 1., true),
        ("DigitalEq", _) => match band(name).map(|(name, _)| name) {
            Some("Enabled" | "Visible") => (0., 1., true),
            Some("Type") => (0., 6., true),
            Some("Slope") => (0., 7., true),
            Some("Channels") => (0., 2., true),
            // Native setters retain frequencies outside the UI range. The
            // coefficient builder clamps the base frequency before transpose.
            Some("Freq") => (f64::NEG_INFINITY, f64::INFINITY, false),
            Some("Q") => (0.018, 28.284, false),
            Some("Gain") => (-30., 30., false),
            Some("Bandwidth") => (0.01, 10., false),
            _ => bail!("Unsupported DigitalEq parameter {name}"),
        },
        ("ParametricEQ", _) => match band(name)
            .filter(|(_, index)| *index < 8)
            .map(|(name, _)| name)
        {
            Some("Enable") => (0., 1., true),
            Some("Freq") => (10., 20000., false),
            Some("Q") => (0.2, 12., false),
            Some("Gain") => (-30., 20., false),
            _ => bail!("Unsupported ParametricEQ parameter {name}"),
        },
        _ => bail!("Unsupported {kind} parameter {name}"),
    };
    let v = number(value)?;
    ensure!(
        (low..=high).contains(&v) && (!integer || v.fract() == 0.),
        "Invalid {kind} parameter {name}"
    );
    ensure!(
        kind != "DigitalEq" || name != "KeyTracking" || v == 0.,
        "DigitalEq KeyTracking needs per-note context"
    );
    ensure!(
        kind != "SampledReverb" || !matches!(name, "DampingLow" | "DampingHigh") || v <= 0.,
        "IReverb positive damping envelope is not implemented"
    );
    Ok(())
}

fn parameters(node: &ProgramNode) -> Result<Parameters> {
    ensure!(supports(&node.kind), "Unsupported UVI effect {}", node.kind);
    let mut p = Parameters::new();
    let mut put = |name: &str, value: f64| {
        p.insert(name.into(), ParameterValue::Number(value));
    };
    put("Bypass", 0.);
    match node.kind.as_str() {
        "DigitalEq" => {
            for (key, value) in [
                ("StereoMode", 0.),
                ("Transpose", 0.),
                ("KeyTracking", 0.),
                ("OverallGain", 0.),
                ("GainScale", 1.),
            ] {
                put(key, value);
            }
            for index in 1..=16 {
                for (key, value) in [
                    ("Enabled", 1.),
                    ("Visible", 1.),
                    ("Type", 0.),
                    ("Slope", 1.),
                    ("Channels", 0.),
                    ("Freq", 1000.),
                    ("Q", std::f64::consts::FRAC_1_SQRT_2),
                    ("Gain", 0.),
                    ("Bandwidth", 1.),
                ] {
                    put(&format!("{key}{index}"), value);
                }
            }
        }
        "ParametricEQ" => {
            for (index, frequency) in [30., 70., 180., 520., 1870., 3900., 7400., 16000.]
                .into_iter()
                .enumerate()
            {
                for (key, value) in [
                    ("Freq", frequency),
                    (
                        "Q",
                        if matches!(index, 1 | 6) {
                            12.
                        } else {
                            f64::from(0.7_f32)
                        },
                    ),
                    ("Gain", 0.),
                    ("Enable", 0.),
                ] {
                    put(&format!("{key}{}", index + 1), value);
                }
            }
        }
        "ThreeBandShelves" => {
            for (key, value) in [
                ("FreqLowMid", 200.),
                ("FreqMidHigh", 4000.),
                ("GainLow", 0.),
                ("GainMid", 0.),
                ("GainHigh", 0.),
            ] {
                put(key, value);
            }
        }
        "Convolver" | "SampledReverb" => {
            put("Dry", if node.kind == "Convolver" { 0. } else { 0.5 });
            put("Wet", if node.kind == "Convolver" { 1. } else { 0.5 });
            put("NormalizePower", 1.);
            if node.kind == "SampledReverb" {
                for (key, value) in [
                    ("Time", 1.),
                    ("DampingLow", 0.),
                    ("DampingHigh", 0.),
                    ("PreDelay", 50.),
                    ("Width", 0.),
                    ("UseWindowAtOriginalSize", 0.),
                ] {
                    put(key, value);
                }
            }
        }
        _ => unreachable!(),
    }
    for (key, raw) in &node.attributes {
        if key == "Name" {
            continue;
        }
        let value = if key == "SamplePath" {
            ParameterValue::Text(raw.clone())
        } else {
            ParameterValue::Number(
                raw.parse()
                    .with_context(|| format!("Invalid {} parameter {key}", node.kind))?,
            )
        };
        check(&node.kind, key, &value)?;
        p.insert(key.clone(), value);
    }
    // Native legacy XML without its version field migrates IReverb to
    // original-size windowing, even when a retained flag says otherwise.
    if node.kind == "SampledReverb" && !node.attributes.contains_key("SampledReverbVersion") {
        p.insert("UseWindowAtOriginalSize".into(), ParameterValue::Number(1.));
    }
    Ok(p)
}

#[derive(Clone, Copy)]
struct Section {
    // Normalized numerator b0,b1,b2 and denominator a1,a2.
    c: [f64; 5],
    z: [[f64; 2]; MAX_CHANNELS],
    channels: u8,
}
impl Section {
    fn new(c: [f64; 6], channels: u8) -> Self {
        Self {
            c: [
                c[0] / c[3],
                c[1] / c[3],
                c[2] / c[3],
                c[4] / c[3],
                c[5] / c[3],
            ],
            z: [[0.; 2]; MAX_CHANNELS],
            channels,
        }
    }
    fn step(&mut self, x: f32, ch: usize) -> f32 {
        let [b0, b1, b2, a1, a2] = self.c;
        let x = f64::from(x);
        let y = b0 * x + self.z[ch][0];
        self.z[ch] = [b1 * x - a1 * y + self.z[ch][1], b2 * x - a2 * y];
        y as f32
    }
}

// Butterworth prototype transformed to a band of twice the selected order.
// Native authored impulses confirm slopes 0/1/2/4/7 for shapes 2/3/6,
// including the order-dependent bandwidth scaling of the peaking numerator.
fn band_coefficients(
    kind: u8,
    frequency: f64,
    gain: f64,
    bandwidth: f64,
    q: f64,
    order: usize,
    rate: f64,
) -> Vec<[f64; 6]> {
    let frequency = frequency.min(rate * 0.499);
    let g = (std::f64::consts::PI * frequency / rate).tan();
    let width = g * ((bandwidth * 0.5).exp2() - (-bandwidth * 0.5).exp2());
    let a = 10f64.powf(gain / (40. * order as f64));
    let poles = |b: f64| {
        let mut real = Vec::new();
        let mut pairs = Vec::new();
        let mut normalization = Complex64::new(1., 0.);
        for prototype in prototype_poles(q, order) {
            let p = prototype * b;
            let root = (p * p - 4. * g * g).sqrt();
            for pole in [(p + root) * 0.5, (p - root) * 0.5] {
                normalization *= 1. - pole;
                let z = (1. + pole) / (1. - pole);
                if z.im.abs() < 1e-10 {
                    real.push(z.re);
                } else if z.im > 0. {
                    pairs.push([-2. * z.re, z.norm_sqr()]);
                }
            }
        }
        for pair in real.chunks_exact(2) {
            pairs.push([-pair[0] - pair[1], pair[0] * pair[1]]);
        }
        (pairs, normalization.re)
    };
    let (den, normalization) = poles(if kind == 6 { width / a } else { width });
    if kind == 6 {
        let (num, _) = poles(width * a);
        den.into_iter()
            .zip(num)
            .map(|([a1, a2], [b1, b2])| {
                let scale = (1. + a1 + a2) / (1. + b1 + b2);
                [scale, scale * b1, scale * b2, 1., a1, a2]
            })
            .collect()
    } else {
        let divisor = normalization.powf(1. / order as f64);
        let (scale, numerator) = if kind == 2 {
            (width / divisor, [1., 0., -1.])
        } else {
            (
                (1. + g * g) / divisor,
                [
                    1.,
                    -2. * (2. * std::f64::consts::PI * frequency / rate).cos(),
                    1.,
                ],
            )
        };
        den.into_iter()
            .map(|[a1, a2]| {
                [
                    scale * numerator[0],
                    scale * numerator[1],
                    scale * numerator[2],
                    1.,
                    a1,
                    a2,
                ]
            })
            .collect()
    }
}

// Generalized Butterworth spectral factor. For even order N the squared
// response is 1 / (1 + x^(2N) + (1/Q² - 2) x^N). Select the stable roots
// of D(s)D(-s). Order three is a first order plus a Q-scaled second order.
fn prototype_poles(q: f64, order: usize) -> Vec<Complex64> {
    if order == 1 {
        return vec![Complex64::new(-1., 0.)];
    }
    if order == 3 {
        let p = -1. / (q * std::f64::consts::SQRT_2);
        let d = Complex64::new(p * p - 4., 0.).sqrt();
        return vec![Complex64::new(-1., 0.), (p + d) * 0.5, (p - d) * 0.5];
    }
    let c = (1. / (q * q) - 2.)
        * if (order / 2).is_multiple_of(2) {
            1.
        } else {
            -1.
        };
    let discriminant = Complex64::new(c * c - 4., 0.).sqrt();
    let ys = [(-c + discriminant) * 0.5, (-c - discriminant) * 0.5];
    let mut poles = Vec::new();
    for y in ys {
        for k in 0..order {
            let p = Complex64::from_polar(
                y.norm().powf(1. / order as f64),
                (y.arg() + 2. * std::f64::consts::PI * k as f64) / order as f64,
            );
            if p.re < 0. {
                poles.push(p);
            }
        }
    }
    poles
}

fn digital_pairs(poles: &[Complex64], g: f64) -> Vec<[f64; 2]> {
    let mut real = Vec::new();
    let mut pairs = Vec::new();
    for &p in poles {
        let z = (1. + p * g) / (1. - p * g);
        if p.im.abs() < 1e-10 {
            real.push(z.re);
        } else if p.im > 0. {
            pairs.push([-2. * z.re, z.norm_sqr()]);
        }
    }
    for pair in real.chunks(2) {
        pairs.push(if pair.len() == 2 {
            [-pair[0] - pair[1], pair[0] * pair[1]]
        } else {
            [-pair[0], 0.]
        });
    }
    pairs
}

fn pass_coefficients(high: bool, frequency: f64, q: f64, order: usize, rate: f64) -> Vec<[f64; 6]> {
    let g = (std::f64::consts::PI * frequency.min(rate * 0.499) / rate).tan();
    digital_pairs(&prototype_poles(q, order), g)
        .into_iter()
        .enumerate()
        .map(|(index, [a1, a2])| {
            let first = order % 2 == 1 && index == order / 2;
            let b0 = if high { 1. - a1 + a2 } else { 1. + a1 + a2 } / if first { 2. } else { 4. };
            [
                b0,
                if high { -b0 } else { b0 } * if first { 1. } else { 2. },
                if first { 0. } else { b0 },
                1.,
                a1,
                a2,
            ]
        })
        .collect()
}

fn digital_shelf_coefficients(
    high: bool,
    frequency: f64,
    q: f64,
    gain: f64,
    order: usize,
    rate: f64,
) -> Vec<[f64; 6]> {
    let g = (std::f64::consts::PI * frequency.min(rate * 0.499) / rate).tan();
    let a = 10f64.powf(gain / (40. * order as f64));
    let poles = prototype_poles(q, order);
    let num = digital_pairs(&poles, if high { g / a } else { g * a });
    let den = digital_pairs(&poles, if high { g * a } else { g / a });
    den.into_iter()
        .zip(num)
        .map(|([a1, a2], [b1, b2])| {
            let scale = if high {
                (1. + a1 + a2) / (1. + b1 + b2)
            } else {
                (1. - a1 + a2) / (1. - b1 + b2)
            };
            [scale, scale * b1, scale * b2, 1., a1, a2]
        })
        .collect()
}

// ThreeBandShelves has first-order shelf transitions. The mid gain is a
// common level and the outer shelves carry the differences from that level.
fn shelf_coefficients(high: bool, frequency: f64, gain: f64, rate: f64) -> [f64; 6] {
    let g = (std::f64::consts::PI * frequency.min(rate * 0.49) / rate).tan();
    let a = 10f64.powf(gain / 40.);
    if high {
        [
            a * a + a * g,
            -a * a + a * g,
            0.,
            1. + a * g,
            a * g - 1.,
            0.,
        ]
    } else {
        [1. + a * g, a * g - 1., 0., 1. + g / a, g / a - 1., 0.]
    }
}

// Native legacy peaking EQ uses the conventional RBJ Q control. Its shelves
// map Q=0.2..12 to slope=0.1..1 and use twice the displayed dB gain.
fn parametric_coefficients(index: usize, frequency: f64, q: f64, gain: f64, rate: f64) -> [f64; 6] {
    let omega = std::f64::consts::TAU * frequency.min(rate * 0.499) / rate;
    let (s, c) = omega.sin_cos();
    if matches!(index, 2..=5) {
        let a = 10f64.powf(gain / 40.);
        let alpha = s / (2. * q);
        return [
            1. + alpha * a,
            -2. * c,
            1. - alpha * a,
            1. + alpha / a,
            -2. * c,
            1. - alpha / a,
        ];
    }
    let a = db(gain);
    let slope = 0.1 + 0.9 * (q - 0.2) / 11.8;
    let beta = s * ((a * a + 1.) * (1. / slope - 1.) + 2. * a).sqrt();
    if index == 1 {
        [
            a * ((a + 1.) - (a - 1.) * c + beta),
            2. * a * ((a - 1.) - (a + 1.) * c),
            a * ((a + 1.) - (a - 1.) * c - beta),
            (a + 1.) + (a - 1.) * c + beta,
            -2. * ((a - 1.) + (a + 1.) * c),
            (a + 1.) + (a - 1.) * c - beta,
        ]
    } else {
        [
            a * ((a + 1.) + (a - 1.) * c + beta),
            -2. * a * ((a - 1.) + (a + 1.) * c),
            a * ((a + 1.) + (a - 1.) * c - beta),
            (a + 1.) - (a - 1.) * c + beta,
            2. * ((a - 1.) - (a + 1.) * c),
            (a + 1.) - (a - 1.) * c - beta,
        ]
    }
}

pub struct EqProcessor {
    sections: Vec<Section>,
    gain: f32,
    mid_side: bool,
}
impl EqProcessor {
    fn new(kind: &str, p: &Parameters, rate: f64) -> Self {
        let mut sections = Vec::new();
        let gain;
        if kind == "DigitalEq" {
            gain = db(n(p, "OverallGain")) as f32;
            for i in 1..=16 {
                let value = |key: &str| n(p, &format!("{key}{i}"));
                if value("Enabled") == 0. {
                    continue;
                }
                let shape = value("Type") as u8;
                let band_gain = value("Gain") * n(p, "GainScale");
                if shape >= 4 && band_gain == 0. {
                    continue;
                }
                let frequency = value("Freq").clamp(10., 22000.) * n(p, "Transpose").exp2();
                let channel = value("Channels") as u8;
                let order = [1, 2, 3, 4, 6, 8, 12, 16][value("Slope") as usize];
                if matches!(shape, 2 | 3 | 6) {
                    for c in band_coefficients(
                        shape,
                        frequency,
                        band_gain,
                        value("Bandwidth"),
                        value("Q"),
                        order,
                        rate,
                    ) {
                        sections.push(Section::new(c, channel));
                    }
                } else if shape < 2 {
                    for c in pass_coefficients(shape == 1, frequency, value("Q"), order, rate) {
                        sections.push(Section::new(c, channel));
                    }
                } else {
                    for c in digital_shelf_coefficients(
                        shape == 5,
                        frequency,
                        value("Q"),
                        band_gain,
                        order,
                        rate,
                    ) {
                        sections.push(Section::new(c, channel));
                    }
                }
            }
        } else if kind == "ParametricEQ" {
            gain = 1.;
            for i in 1..=8 {
                let value = |key: &str| n(p, &format!("{key}{i}"));
                if value("Enable") == 0. || ((2..=7).contains(&i) && value("Gain") == 0.) {
                    continue;
                }
                let frequency = value("Freq");
                let q = value("Q");
                let coefficients = if matches!(i, 1 | 8) {
                    // The native HP/LP gain fields are retained but have no
                    // effect on their measured two-pole responses.
                    pass_coefficients(i == 1, frequency, q, 2, rate)[0]
                } else {
                    parametric_coefficients(i - 1, frequency, q, value("Gain"), rate)
                };
                sections.push(Section::new(coefficients, 0));
            }
        } else {
            // Native first-order shelves with a shared middle-band level.
            gain = db(n(p, "GainMid")) as f32;
            for (shape, freq, level) in
                [(4, "FreqLowMid", "GainLow"), (5, "FreqMidHigh", "GainHigh")]
            {
                let g = n(p, level) - n(p, "GainMid");
                if g != 0. {
                    sections.push(Section::new(
                        shelf_coefficients(shape == 5, n(p, freq), g, rate),
                        0,
                    ));
                }
            }
        }
        Self {
            sections,
            gain,
            mid_side: kind == "DigitalEq" && on(p, "StereoMode"),
        }
    }
    fn process(&mut self, io: &mut [Frame], channels: usize) {
        for f in io {
            if self.mid_side {
                for pair in f[..channels].as_chunks_mut::<2>().0 {
                    let (l, r) = (pair[0], pair[1]);
                    pair[0] = (l + r) * std::f32::consts::FRAC_1_SQRT_2;
                    pair[1] = (l - r) * std::f32::consts::FRAC_1_SQRT_2;
                }
            }
            for section in &mut self.sections {
                for (ch, x) in f[..channels].iter_mut().enumerate() {
                    if section.channels == 0 || section.channels as usize == ch % 2 + 1 {
                        *x = section.step(*x, ch);
                    }
                }
            }
            if self.mid_side {
                for pair in f[..channels].as_chunks_mut::<2>().0 {
                    let (m, s) = (pair[0], pair[1]);
                    pair[0] = (m + s) * std::f32::consts::FRAC_1_SQRT_2;
                    pair[1] = (m - s) * std::f32::consts::FRAC_1_SQRT_2;
                }
            }
            f[..channels].iter_mut().for_each(|x| *x *= self.gain);
        }
    }
}

pub struct ImpulseProcessor {
    sample: Option<Arc<Sample>>,
    convolvers: Vec<Convolver>,
    scratch: Vec<f32>,
    wet_frames: Vec<Frame>,
    max_block: usize,
}
impl ImpulseProcessor {
    fn empty(max_block: usize) -> Self {
        Self {
            sample: None,
            convolvers: Vec::new(),
            scratch: vec![0.; max_block],
            wet_frames: vec![[0.; MAX_CHANNELS]; max_block],
            max_block,
        }
    }
    fn prepare(
        &mut self,
        sample: Arc<Sample>,
        kind: &str,
        p: &Parameters,
        channels: usize,
        rate: f64,
    ) -> Result<()> {
        ensure!(
            sample.rate > 0 && sample.frames > 0 && (1..=MAX_CHANNELS).contains(&sample.channels),
            "Invalid UVI impulse metadata"
        );
        ensure!(
            sample.interleaved.len()
                == sample
                    .frames
                    .checked_mul(sample.channels)
                    .context("Impulse dimensions overflow")?
                && sample.interleaved.iter().all(|x| x.is_finite()),
            "Invalid UVI impulse samples"
        );
        // Native authored basis probes keep the bus width: channel c uses
        // IR channel c % IR width, including four-channel IRs on stereo buses.
        let duration = if kind == "SampledReverb" {
            n(p, "Time")
        } else {
            1.
        };
        let converted_length = resampling::reconstructed_frames(&sample, rate)?;
        let length = (converted_length as f64 * duration).floor() as usize;
        let pre = if kind == "SampledReverb" {
            (n(p, "PreDelay") * 0.001 * rate).round() as usize
        } else {
            0
        };
        ensure!(
            length > 0
                && length
                    .checked_add(pre)
                    .and_then(|l| l.checked_mul(
                        channels
                            .max(sample.channels)
                            .max(if kind == "SampledReverb" { 2 } else { 1 })
                            * 4
                    ))
                    .is_some_and(|b| b <= IR_BYTES),
            "UVI prepared impulse exceeds memory bound"
        );
        let kernel_channels = if kind == "SampledReverb" {
            sample.channels.max(2)
        } else {
            sample.channels
        };
        let mut kernels: Vec<Vec<f32>> = (0..kernel_channels)
            .map(|ch| {
                let ch = ch % sample.channels;
                let reconstructed = resampling::reconstruct_channel(&sample, ch, rate)?;
                ensure!(
                    reconstructed.len() == converted_length,
                    "UVI reconstructed impulse length mismatch"
                );
                let mut h = vec![0.; length + pre];
                h[pre..].copy_from_slice(&reconstructed[..length]);
                // Native IReverb applies a half-cosine over the retained IR.
                // The endpoint denominator includes one frame beyond its length.
                if kind == "SampledReverb" && (duration < 1. || on(p, "UseWindowAtOriginalSize")) {
                    for i in 0..length {
                        h[pre + i] *= (0.5
                            + 0.5 * (std::f64::consts::PI * i as f64 / (length + 1) as f64).cos())
                            as f32;
                    }
                }
                Ok(h)
            })
            .collect::<Result<_>>()?;
        if kind == "SampledReverb" && (n(p, "DampingLow") != 0. || n(p, "DampingHigh") != 0.) {
            let low_weight = (-n(p, "DampingLow")).powf(0.2);
            let high_weight = (-n(p, "DampingHigh")).powf(0.2);
            for h in &mut kernels {
                let mut low = h[pre..].to_vec();
                // Native authored shifted impulses establish a zero-phase
                // two-pass pole of 0.4, with held endpoint initialization.
                let mut filter = Section::new([0.6, 0., 0., 1., -0.4, 0.], 0);
                filter.z[0][0] = 0.4 * f64::from(low[0]);
                for x in &mut low {
                    *x = filter.step(*x, 0);
                }
                filter.z[0] = [0.4 * f64::from(low[length - 1]), 0.];
                for x in low.iter_mut().rev() {
                    *x = filter.step(*x, 0);
                }
                for (i, (x, low)) in h[pre..].iter_mut().zip(low).enumerate() {
                    // Damping age is normalized to the retained IR length,
                    // including zero padding; it excludes playback predelay.
                    let decay = (-10. * i as f64 / length as f64).exp();
                    let gain = |weight: f64| 1. / (1. - weight + weight * decay);
                    let high = f64::from(*x) - f64::from(low);
                    *x = (f64::from(low) * gain(low_weight) + high * gain(high_weight)) as f32;
                }
            }
        }
        if kind == "SampledReverb" && n(p, "Width") != 0. {
            let alpha = -n(p, "Width") / (2. + n(p, "Width"));
            let scale = (1. + alpha * alpha).sqrt().recip();
            // Native Width affects only the first stereo IR pair, even
            // when the response and processing bus have six or twelve channels.
            let (left_kernel, right_kernels) = kernels.split_at_mut(1);
            for (left, right) in left_kernel[0].iter_mut().zip(&mut right_kernels[0]) {
                *left = ((f64::from(*left) + alpha * f64::from(*right)) * scale) as f32;
                // Native width updates the right kernel using the updated left.
                *right = ((f64::from(*right) + alpha * f64::from(*left)) * scale) as f32;
            }
        }
        // Native IReverb optimizes length using its first prepared channel.
        // An empty response becomes identity only without predelay; with
        // predelay, the resulting nonempty all-zero kernel stays silent.
        if kind == "SampledReverb" && kernels[0][pre..].iter().all(|x| *x == 0.) {
            for h in &mut kernels {
                *h = if pre == 0 { vec![1.] } else { vec![0.; pre] };
            }
        }
        if on(p, "NormalizePower") {
            // Native Convolver normalizes the mean channel energy, preserving
            // channel-level ratios. Authored mono/stereo amplitude probes match.
            let energy = kernels
                .iter()
                .map(|h| h.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>())
                .sum::<f64>()
                / kernel_channels as f64;
            if energy > 0. {
                let gain = if kind == "SampledReverb" { 1.25 } else { 1. } / energy.sqrt();
                kernels
                    .iter_mut()
                    .flatten()
                    .for_each(|x| *x = (f64::from(*x) * gain) as f32);
            }
        }
        if kind == "SampledReverb" && !on(p, "NormalizePower") {
            kernels.iter_mut().flatten().for_each(|x| *x *= 0.125);
        }
        let convolvers = (0..channels)
            .map(|ch| Convolver::new(&kernels[ch % kernel_channels], self.max_block))
            .collect();
        self.convolvers = convolvers;
        self.sample = Some(sample);
        Ok(())
    }
    fn process(
        &mut self,
        io: &mut [Frame],
        p: &Parameters,
        channels: usize,
        kind: &str,
    ) -> Result<()> {
        ensure!(
            !self.convolvers.is_empty() || n(p, "Wet") == 0.,
            "UVI convolution requires an available impulse resource"
        );
        let (dry, wet) = (n(p, "Dry") as f32, n(p, "Wet") as f32);
        for block in io.chunks_mut(self.max_block) {
            for ch in 0..channels {
                for (x, f) in self.scratch[..block.len()].iter_mut().zip(block.iter()) {
                    *x = if kind == "SampledReverb" {
                        // Native wet diffusion also attenuates a mono bus by 0.8.
                        0.8 * f[ch] + if channels == 2 { 0.2 * f[ch ^ 1] } else { 0. }
                    } else {
                        f[ch]
                    };
                }
                if let Some(conv) = self.convolvers.get_mut(ch) {
                    conv.process(&mut self.scratch[..block.len()]);
                } else {
                    self.scratch[..block.len()].fill(0.);
                }
                for (f, x) in self.wet_frames.iter_mut().zip(&self.scratch) {
                    f[ch] = *x;
                }
            }
            for (f, w) in block.iter_mut().zip(&self.wet_frames) {
                for ch in 0..channels {
                    f[ch] = dry * f[ch] + wet * w[ch];
                }
            }
        }
        Ok(())
    }
}

pub enum ProcessorKind {
    Exciter(Box<Exciter>),
    Eq(EqProcessor),
    Impulse(ImpulseProcessor),
}

pub struct EffectProcessor {
    kind: String,
    channels: usize,
    rate: f64,
    parameters: Parameters,
    processor: ProcessorKind,
    irs: Option<Arc<HashMap<String, Arc<Sample>>>>,
}
impl EffectProcessor {
    pub fn new(
        node: &ProgramNode,
        channels: usize,
        rate: f64,
        max_block: usize,
        irs: Arc<HashMap<String, Arc<Sample>>>,
    ) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "UVI effects need 1..12 channels"
        );
        ensure!(
            rate.is_finite() && (8000. ..=192000.).contains(&rate),
            "Invalid UVI effect rate"
        );
        ensure!(
            (1..=8192).contains(&max_block),
            "Invalid UVI effect block size"
        );
        if exciter::supports(&node.kind) {
            return Ok(Self {
                kind: node.kind.clone(),
                channels,
                rate,
                parameters: BTreeMap::new(),
                processor: ProcessorKind::Exciter(Box::new(Exciter::new(node, channels, rate)?)),
                irs: None,
            });
        }
        let parameters = parameters(node)?;
        let processor = if matches!(
            node.kind.as_str(),
            "DigitalEq" | "ParametricEQ" | "ThreeBandShelves"
        ) {
            ProcessorKind::Eq(EqProcessor::new(&node.kind, &parameters, rate))
        } else {
            ProcessorKind::Impulse(ImpulseProcessor::empty(max_block))
        };
        let mut result = Self {
            kind: node.kind.clone(),
            channels,
            rate,
            parameters,
            processor,
            irs: if matches!(
                node.kind.as_str(),
                "DigitalEq" | "ParametricEQ" | "ThreeBandShelves"
            ) {
                None
            } else {
                Some(Arc::clone(&irs))
            },
        };
        if let Some(ParameterValue::Text(path)) = result.parameters.get("SamplePath")
            && let Some(sample) = irs.get(path)
        {
            result.load_resource(sample.clone())?;
        }
        Ok(result)
    }
    /// Install the renderer's shared lookup map without changing active DSP.
    pub fn replace_resources(&mut self, resources: Arc<HashMap<String, Arc<Sample>>>) {
        if let Some(current) = self.irs.as_mut() {
            *current = resources;
        }
    }
    /// Retained DSP buffer bytes, including inline processor state. Shared
    /// sample/map ownership, parameter-tree nodes, FFT plans and allocator
    /// bookkeeping are outside this processing-buffer count.
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.kind.capacity()
            + match &self.processor {
                ProcessorKind::Exciter(exciter) => exciter.memory_bytes(),
                ProcessorKind::Eq(eq) => eq.sections.capacity() * std::mem::size_of::<Section>(),
                ProcessorKind::Impulse(ir) => {
                    ir.convolvers
                        .iter()
                        .map(Convolver::memory_bytes)
                        .sum::<usize>()
                        + (ir.convolvers.capacity() - ir.convolvers.len())
                            * std::mem::size_of::<Convolver>()
                        + ir.scratch.capacity() * std::mem::size_of::<f32>()
                        + ir.wet_frames.capacity() * std::mem::size_of::<Frame>()
                }
            }
    }
    pub fn diagnostics(&self) -> &'static str {
        match &self.processor {
            ProcessorKind::Exciter(_) => exciter::FIDELITY_DIAGNOSTIC,
            ProcessorKind::Impulse(ir)
                if ir.sample.as_ref().is_some_and(|sample| {
                    !resampling::verified_rate(sample.rate, self.rate)
                        || (self.kind == "SampledReverb" && self.rate != 48000.)
                }) =>
            {
                FIDELITY_DIAGNOSTIC
            }
            _ => "",
        }
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        if let ProcessorKind::Exciter(exciter) = &self.processor {
            return exciter.parameter(name);
        }
        self.parameters
            .get(name)
            .cloned()
            .context("Unknown UVI effect parameter")
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        if let ProcessorKind::Exciter(exciter) = &mut self.processor {
            return exciter.set_parameter(name, value);
        }
        check(&self.kind, name, value)?;
        if self.parameters.get(name) == Some(value) {
            return Ok(());
        }
        let mut p = self.parameters.clone();
        p.insert(name.into(), value.clone());
        match &mut self.processor {
            ProcessorKind::Exciter(_) => unreachable!("delegated above"),
            ProcessorKind::Eq(eq) => {
                if name != "Bypass" && !name.starts_with("Visible") {
                    let mut replacement = EqProcessor::new(&self.kind, &p, self.rate);
                    if replacement.sections.len() == eq.sections.len()
                        && !["Enable", "Type", "Slope", "Channels", "StereoMode"]
                            .iter()
                            .any(|prefix| name.starts_with(prefix))
                    {
                        for (new, old) in replacement.sections.iter_mut().zip(&eq.sections) {
                            new.z = old.z;
                        }
                    }
                    *eq = replacement;
                }
            }
            ProcessorKind::Impulse(ir) => {
                let sample = if name == "SamplePath" {
                    match value {
                        ParameterValue::Text(path) => Some(
                            self.irs
                                .as_ref()
                                .context("UVI impulse resource map is unavailable")?
                                .get(path)
                                .context("UVI impulse resource is unavailable")?
                                .clone(),
                        ),
                        _ => unreachable!(),
                    }
                } else {
                    ir.sample.clone()
                };
                if matches!(
                    name,
                    "SamplePath"
                        | "Time"
                        | "PreDelay"
                        | "NormalizePower"
                        | "DampingLow"
                        | "DampingHigh"
                        | "Width"
                        | "UseWindowAtOriginalSize"
                ) && let Some(sample) = sample
                {
                    ir.prepare(sample, &self.kind, &p, self.channels, self.rate)?;
                }
            }
        }
        self.parameters = p;
        Ok(())
    }
    pub fn load_resource(&mut self, sample: Arc<Sample>) -> Result<()> {
        match &mut self.processor {
            ProcessorKind::Impulse(ir) => ir.prepare(
                sample,
                &self.kind,
                &self.parameters,
                self.channels,
                self.rate,
            ),
            _ => bail!("UVI EQ cannot load an impulse resource"),
        }
    }
    pub fn process(&mut self, io: &mut [Frame]) -> Result<()> {
        ensure!(
            io.iter()
                .flat_map(|f| &f[..self.channels])
                .all(|x| x.is_finite()),
            "Nonfinite UVI effect input"
        );
        if let ProcessorKind::Exciter(exciter) = &mut self.processor {
            return exciter.process(io);
        }
        if on(&self.parameters, "Bypass") {
            return Ok(());
        }
        match &mut self.processor {
            ProcessorKind::Exciter(_) => unreachable!("delegated above"),
            ProcessorKind::Eq(eq) => eq.process(io, self.channels),
            ProcessorKind::Impulse(ir) => {
                ir.process(io, &self.parameters, self.channels, &self.kind)?
            }
        }
        ensure!(
            io.iter()
                .flat_map(|f| &f[..self.channels])
                .all(|x| x.is_finite()),
            "Nonfinite UVI effect output"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::program::parse_program;
    use super::*;

    fn effect(xml: &str) -> ProgramNode {
        let mut graph =
            parse_program(&format!("<Program><Inserts>{xml}</Inserts></Program>")).unwrap();
        graph.nodes.remove(2)
    }
    fn impulse(channels: usize) -> Arc<Sample> {
        Arc::new(Sample {
            rate: 48_000,
            channels,
            frames: 3,
            interleaved: super::super::storage::Storage::from_f32(
                (0..3)
                    .flat_map(|i| {
                        (0..channels).map(move |ch| [0.5, -0.25, 0.125][i] * (ch + 1) as f32)
                    })
                    .collect(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            wavetable_cycle_frames: None,
            wavetable_image: false,
            riff_metadata: Vec::new(),
        })
    }
    #[test]
    fn authored_exciter_delegation_and_atomic_preflight() {
        let node = effect("<Exciter Amount='2' Mode='1' Oversampling='0'/>");
        assert!(supports(&node.kind));
        validate(&node).unwrap();
        let mut combined =
            EffectProcessor::new(&node, 2, 48000., 256, Arc::new(HashMap::new())).unwrap();
        let mut direct = Exciter::new(&node, 2, 48000.).unwrap();
        let mut input = vec![[0.; 12]; 512];
        input[0][0] = 0.125;
        input[17][1] = -0.25;
        let mut reference = input.clone();
        combined.process(&mut input).unwrap();
        direct.process(&mut reference).unwrap();
        assert_eq!(input, reference);
        assert!(
            combined
                .set_parameter("Amount", &ParameterValue::Number(3.))
                .is_err()
        );
        assert_eq!(
            combined.parameter("Amount").unwrap(),
            ParameterValue::Number(2.)
        );
        assert!(combined.diagnostics().contains("fitted"));
        assert!(
            combined.memory_bytes()
                >= std::mem::size_of::<EffectProcessor>() + direct.memory_bytes()
        );
        assert!(validate(&effect("<Exciter Oversampling='1'/>")).is_err());
        assert!(EffectProcessor::new(&node, 1, 48000., 256, Arc::new(HashMap::new())).is_err());
    }
    #[test]
    fn authored_parametric_eq_native_bands_and_channels() {
        let cases = [
            (
                1,
                0.7,
                6.,
                [
                    0.113850668073,
                    -0.0212006904185,
                    -0.0190346911550,
                    -0.0169402211905,
                ],
            ),
            (
                2,
                12.,
                6.,
                [
                    0.133396998048,
                    0.0171969458461,
                    0.0178823340684,
                    0.0183392483741,
                ],
            ),
            (
                3,
                0.7,
                6.,
                [
                    0.132702976465,
                    0.0143284164369,
                    0.0122004440054,
                    0.0101401610300,
                ],
            ),
            (
                7,
                12.,
                6.,
                [
                    0.466309189796,
                    -0.0601144991815,
                    -0.0547606833279,
                    -0.0489895232022,
                ],
            ),
            (
                8,
                2.,
                -6.,
                [
                    0.000517799577210,
                    0.00202989322133,
                    0.00393058639020,
                    0.00564602622762,
                ],
            ),
        ];
        for (band, q, gain, native) in cases {
            let mut node = effect(r#"<ParametricEQ/>"#);
            for (name, value) in [("Enable", 1.), ("Freq", 1000.), ("Q", q), ("Gain", gain)] {
                node.attributes
                    .insert(format!("{name}{band}"), value.to_string());
            }
            let mut fx =
                EffectProcessor::new(&node, 12, 48000., 64, Arc::new(HashMap::new())).unwrap();
            let mut io = vec![[0.; MAX_CHANNELS]; 1024];
            for (ch, sample) in io[0].iter_mut().enumerate() {
                *sample = 0.125 * (ch + 1) as f32 / 12.;
            }
            for block in io.chunks_mut(7) {
                fx.process(block).unwrap();
            }
            for (frame, expected) in io.iter().zip(native) {
                for (ch, sample) in frame.iter().enumerate() {
                    assert!(
                        (f64::from(*sample) - expected * (ch + 1) as f64 / 12.).abs() < 5e-8,
                        "native band {band}, channel {ch}"
                    );
                }
            }
            assert!(io.iter().flatten().all(|v| v.is_finite()));
            fx.set_parameter("Bypass", &ParameterValue::Boolean(true))
                .unwrap();
            let mut dry = [[0.25; MAX_CHANNELS]; 4];
            fx.process(&mut dry).unwrap();
            assert_eq!(dry, [[0.25; MAX_CHANNELS]; 4]);
            assert!(
                fx.set_parameter("Enable9", &ParameterValue::Number(1.))
                    .is_err()
            );
            assert!(
                fx.set_parameter("Q3", &ParameterValue::Number(f64::NAN))
                    .is_err()
            );
        }
    }

    #[test]
    fn authored_eq_live_frequency_contract() {
        // Original native impulses: stored frequency is unrestricted; the UI
        // range applies before transpose, then the DSP uses a 0.499-rate cap.
        let cases = [
            (
                0.,
                0.,
                [
                    0.0000817588079371,
                    0.000163410673849,
                    0.000163196906215,
                    0.000162983415066,
                ],
            ),
            (
                44000.,
                -1.,
                [
                    0.0584035329521,
                    0.0622315034270,
                    0.00407886737958,
                    0.000267343042651,
                ],
            ),
            (
                0.,
                1.,
                [
                    0.000163410804817,
                    0.000326394365402,
                    0.000325540982885,
                    0.000324689841364,
                ],
            ),
            (
                10.,
                -1.,
                [
                    0.0000408927735407,
                    0.0000817587933852,
                    0.0000817053005449,
                    0.0000816518440843,
                ],
            ),
            (
                22000.,
                1.,
                [
                    0.124608531594,
                    0.000780478119850,
                    -0.000775589665864,
                    0.000770731829107,
                ],
            ),
            (
                22350.607421875,
                0.,
                [
                    0.110457941890,
                    0.0257005766034,
                    -0.0197207462043,
                    0.0151322614402,
                ],
            ),
        ];
        for (frequency, transpose, native) in cases {
            let mut node = effect(r#"<DigitalEq GainScale="1" Type16="0" Slope16="0"/>"#);
            for i in 1..=16 {
                node.attributes
                    .insert(format!("Enabled{i}"), u8::from(i == 16).to_string());
            }
            node.attributes
                .insert("Transpose".into(), transpose.to_string());
            let mut fx =
                EffectProcessor::new(&node, 2, 48000., 64, Arc::new(HashMap::new())).unwrap();
            fx.set_parameter("Freq16", &ParameterValue::Number(frequency))
                .unwrap();
            assert!(fx.parameter("Freq16").unwrap() == ParameterValue::Number(frequency));
            let mut io = vec![[0.; MAX_CHANNELS]; 1024];
            io[0][0] = 0.125;
            fx.process(&mut io).unwrap();
            for (frame, expected) in io.iter().zip(native) {
                assert!(
                    (f64::from(frame[0]) - expected).abs() < 2e-8,
                    "Freq={frequency}, Transpose={transpose}"
                );
            }
            assert!(io.iter().flatten().all(|v| v.is_finite()));
            assert!(
                fx.set_parameter("Freq16", &ParameterValue::Number(f64::NAN))
                    .is_err()
            );
        }
    }

    #[test]
    fn authored_effects_impulse_eq_and_fragmentation() {
        let mut resources = HashMap::new();
        resources.insert("fixture".into(), impulse(12));
        let mut stereo = vec![0.; 48_000];
        stereo[0] = 0.5;
        stereo[1] = 0.25;
        resources.insert(
            "native_stereo".into(),
            Arc::new(Sample {
                rate: 48_000,
                channels: 2,
                frames: 24_000,
                interleaved: super::super::storage::Storage::from_f32(stereo).unwrap(),
                loops: Vec::new(),
                unity_note: None,
                wavetable_cycle_frames: None,
                wavetable_image: false,
                riff_metadata: Vec::new(),
            }),
        );
        let mut damping = vec![0.; 24_000];
        damping[1000] = 0.5;
        resources.insert(
            "native_damping".into(),
            Arc::new(Sample {
                rate: 48_000,
                channels: 1,
                frames: 24_000,
                interleaved: super::super::storage::Storage::from_f32(damping).unwrap(),
                loops: Vec::new(),
                unity_note: None,
                wavetable_cycle_frames: None,
                wavetable_image: false,
                riff_metadata: Vec::new(),
            }),
        );
        let mut zero_first = vec![0.; 48_000];
        zero_first[2001] = 0.5;
        resources.insert(
            "zero_first".into(),
            Arc::new(Sample {
                rate: 48_000,
                channels: 2,
                frames: 24_000,
                interleaved: super::super::storage::Storage::from_f32(zero_first).unwrap(),
                loops: Vec::new(),
                unity_note: None,
                wavetable_cycle_frames: None,
                wavetable_image: false,
                riff_metadata: Vec::new(),
            }),
        );
        let resources = Arc::new(resources);
        let node =
            effect(r#"<Convolver SamplePath="fixture" Dry="0.25" Wet="0.5" NormalizePower="0"/>"#);
        let input: Vec<Frame> = (0..131)
            .map(|i| {
                if i == 0 {
                    std::array::from_fn(|ch| (ch + 1) as f32)
                } else {
                    [0.; MAX_CHANNELS]
                }
            })
            .collect();
        let mut expected = input.clone();
        for (i, f) in expected.iter_mut().enumerate() {
            for (ch, x) in f.iter_mut().enumerate() {
                *x = if i < 3 {
                    0.5 * [0.5, -0.25, 0.125][i] * ((ch + 1) * (ch + 1)) as f32
                } else {
                    0.
                };
                if i == 0 {
                    *x += 0.25 * (ch + 1) as f32;
                }
            }
        }
        for block in [1, 7, 64, 131] {
            let mut fx =
                EffectProcessor::new(&node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
            let retained = fx.memory_bytes();
            assert!(retained > 3 * 12 * std::mem::size_of::<f32>());
            let mut actual = input.clone();
            for chunk in actual.chunks_mut(block) {
                fx.process(chunk).unwrap();
            }
            assert_eq!(fx.memory_bytes(), retained);
            for (a, b) in actual.iter().flatten().zip(expected.iter().flatten()) {
                assert!((a - b).abs() < 1e-4, "convolution {block}: {a} != {b}");
            }
            fx.set_parameter("Bypass", &ParameterValue::Boolean(true))
                .unwrap();
            let mut bypass = input.clone();
            fx.process(&mut bypass).unwrap();
            assert_eq!(bypass, input);
        }
        let mut narrower =
            EffectProcessor::new(&node, 6, 48000., 64, Arc::clone(&resources)).unwrap();
        let mut native_mapping = input.clone();
        narrower.process(&mut native_mapping).unwrap();
        for (actual, expected) in native_mapping.iter().zip(&expected) {
            assert!(
                actual[..6]
                    .iter()
                    .zip(&expected[..6])
                    .all(|(a, b)| (a - b).abs() < 1e-4)
            );
        }
        let mut fx = EffectProcessor::new(
            &effect(r#"<ThreeBandShelves GainLow="6" GainMid="6" GainHigh="6"/>"#),
            12,
            48000.,
            64,
            Arc::clone(&resources),
        )
        .unwrap();
        let mut dc = vec![[0.25; MAX_CHANNELS]; 256];
        fx.process(&mut dc).unwrap();
        assert!(
            dc.iter()
                .flatten()
                .all(|x| (*x - 0.25 * db(6.) as f32).abs() < 1e-6)
        );
        assert!(
            fx.set_parameter("FreqLowMid", &ParameterValue::Number(19.))
                .is_err()
        );
        assert!(
            fx.set_parameter("GainLow", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
        assert!(
            fx.set_parameter("Unknown", &ParameterValue::Number(0.))
                .is_err()
        );

        let mut eq_node = effect(r#"<DigitalEq GainScale="1"/>"#);
        for band in 1..=16 {
            eq_node
                .attributes
                .insert(format!("Enabled{band}"), "0".into());
        }
        for (name, value) in [
            ("Enabled1", "1"),
            ("Type1", "6"),
            ("Freq1", "1000"),
            ("Gain1", "6"),
        ] {
            eq_node.attributes.insert(name.into(), value.into());
        }
        let signal: Vec<Frame> = (0..4800)
            .map(|i| {
                std::array::from_fn(|ch| {
                    (i as f64 * 2. * std::f64::consts::PI / 48.).sin() as f32 * (ch + 1) as f32
                        / 12.
                })
            })
            .collect();
        let mut whole = signal.clone();
        let mut split = signal.clone();
        let mut fx =
            EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
        fx.process(&mut whole).unwrap();
        let mut fx =
            EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
        for c in split.chunks_mut(7) {
            fx.process(c).unwrap();
        }
        assert_eq!(whole, split);
        let energy = |data: &[Frame]| data[2400..].iter().map(|f| f[11].powi(2)).sum::<f32>();
        assert!(((energy(&whole) / energy(&signal)).sqrt() - db(6.) as f32).abs() < 1e-3);
        assert!(whole.iter().flatten().all(|x| x.is_finite()));
        fx.set_parameter("GainScale", &ParameterValue::Number(0.))
            .unwrap();
        let mut flat = signal.clone();
        fx.process(&mut flat).unwrap();
        assert_eq!(flat, signal);
        // Observable native audio from authored PCM16 impulse fixtures. These
        // checks distinguish the fourth-order band laws from ordinary biquads.
        for (kind, first) in [
            (0, 0.003916126675903797),
            (1, 0.9115866422653198),
            (2, 0.0019951756112277508),
            (3, 0.9368622899055481),
            (4, 1.032562494277954),
            (5, 1.9323405027389526),
            (6, 1.0228638648986816),
        ] {
            eq_node.attributes.insert("Type1".into(), kind.to_string());
            eq_node.attributes.insert("Freq1".into(), "1000".into());
            let mut fx =
                EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
            let mut impulse = vec![[0.; MAX_CHANNELS]; 64];
            impulse[0] = [1.; MAX_CHANNELS];
            fx.process(&mut impulse).unwrap();
            assert!(
                (f64::from(impulse[0][11]) - first).abs() < 3e-7,
                "native shape {kind}"
            );
        }
        for (slope, q, first) in [
            (0, 0.70710678, 0.06151176989078522),
            (2, 0.70710678, 0.00024700083304196596),
            (3, 0.3, 1.4908267075952608e-5),
            (3, 2., 1.5937812349875458e-5),
            (4, 0.70710678, 6.155352849646079e-8),
            (5, 0.70710678, 2.4344495863637405e-10),
            (6, 0.70710678, 3.805212929827587e-15),
            (7, 0.70710678, 5.945665029340912e-20),
        ] {
            eq_node.attributes.insert("Type1".into(), "0".into());
            eq_node
                .attributes
                .insert("Slope1".into(), slope.to_string());
            eq_node.attributes.insert("Q1".into(), q.to_string());
            let mut fx =
                EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
            let mut impulse = vec![[0.; MAX_CHANNELS]; 64];
            impulse[0] = [1.; MAX_CHANNELS];
            fx.process(&mut impulse).unwrap();
            assert!(
                (f64::from(impulse[0][11]) / first - 1.).abs() < 2e-5,
                "native slope {slope}, Q {q}"
            );
        }
        // Native Slope0 peaks are second order; Slope1 is fourth order.
        eq_node.attributes.insert("Type1".into(), "6".into());
        eq_node.attributes.insert("Slope1".into(), "0".into());
        eq_node.attributes.insert("Q1".into(), "0.70710678".into());
        let mut fx =
            EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
        let mut native_peak = [[0.; MAX_CHANNELS]; 8];
        native_peak[0] = [0.125; MAX_CHANNELS];
        fx.process(&mut native_peak).unwrap();
        for (frame, reference) in native_peak.iter().zip([
            0.12893584370613098,
            0.0075574531219899654,
            0.006888835225254297,
            0.006148382555693388,
            0.005352908279746771,
            0.004519074223935604,
            0.003663123119622469,
            0.002800636924803257,
        ]) {
            assert!((f64::from(frame[11]) - reference).abs() < 1e-8);
        }
        for (shape, slope, q, first) in [
            (2, 2, 0.3, 1.0570594895398244e-5),
            (3, 2, 2., 0.1173219159245491),
            (4, 0, 0.3, 0.13051669299602509),
            (4, 2, 2., 0.1276955008506775),
            (5, 0, 0.3, 0.23886579275131226),
            (5, 2, 2., 0.24414311349391937),
            (6, 2, 0.3, 0.12927746772766113),
            (6, 7, 0.70710678, 0.1275651901960373),
        ] {
            eq_node.attributes.insert("Type1".into(), shape.to_string());
            eq_node
                .attributes
                .insert("Slope1".into(), slope.to_string());
            eq_node.attributes.insert("Q1".into(), q.to_string());
            let mut fx =
                EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
            let mut impulse = [[0.; MAX_CHANNELS]];
            impulse[0] = [0.125; MAX_CHANNELS];
            fx.process(&mut impulse).unwrap();
            assert!(
                (f64::from(impulse[0][11]) - first).abs() < 4e-8,
                "native shape {shape}, slope {slope}, Q {q}"
            );
        }
        eq_node.attributes.insert("Slope1".into(), "1".into());
        eq_node.attributes.insert("Q1".into(), "0.70710678".into());
        for kind in 0..=6 {
            eq_node.attributes.insert("Type1".into(), kind.to_string());
            for frequency in [10., 22000.] {
                eq_node
                    .attributes
                    .insert("Freq1".into(), frequency.to_string());
                let mut fx =
                    EffectProcessor::new(&eq_node, 12, 8000., 64, Arc::clone(&resources)).unwrap();
                let mut x = input.clone();
                fx.process(&mut x).unwrap();
                assert!(x.iter().flatten().all(|x| x.is_finite()));
            }
        }
        eq_node.attributes.insert("Type1".into(), "6".into());
        eq_node.attributes.insert("Freq1".into(), "1000".into());
        for (mode, channel, left, right) in [
            (0, 1, 0.2557159662246704, 0.125),
            (0, 2, 0.25, 0.1278579831123352),
            (1, 1, 0.2542869448661804, 0.1292869597673416),
            (1, 2, 0.2514289617538452, 0.1235709935426712),
        ] {
            eq_node
                .attributes
                .insert("StereoMode".into(), mode.to_string());
            eq_node
                .attributes
                .insert("Channels1".into(), channel.to_string());
            let mut fx =
                EffectProcessor::new(&eq_node, 2, 48000., 64, Arc::clone(&resources)).unwrap();
            let mut impulse = [[0.; MAX_CHANNELS]];
            impulse[0][0] = 0.25;
            impulse[0][1] = 0.125;
            fx.process(&mut impulse).unwrap();
            assert!((f64::from(impulse[0][0]) - left).abs() < 6e-8);
            assert!((f64::from(impulse[0][1]) - right).abs() < 6e-8);
        }
        eq_node.attributes.insert("StereoMode".into(), "0".into());
        eq_node.attributes.insert("Channels1".into(), "0".into());
        for q in [0.018, 28.284] {
            for slope in 0..=7 {
                eq_node.attributes.insert("Type1".into(), "0".into());
                eq_node
                    .attributes
                    .insert("Slope1".into(), slope.to_string());
                eq_node.attributes.insert("Q1".into(), q.to_string());
                eq_node.attributes.insert("Freq1".into(), "10".into());
                let mut fx =
                    EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources)).unwrap();
                let mut bounded = vec![[0.; MAX_CHANNELS]; 512];
                bounded[0] = [1.; MAX_CHANNELS];
                fx.process(&mut bounded).unwrap();
                assert!(bounded.iter().flatten().all(|x| x.is_finite()));
            }
        }
        for shape in 2..=6 {
            eq_node.attributes.insert("Type1".into(), shape.to_string());
            for slope in 0..=7 {
                eq_node
                    .attributes
                    .insert("Slope1".into(), slope.to_string());
                for bandwidth in [0.01, 10.] {
                    eq_node
                        .attributes
                        .insert("Bandwidth1".into(), bandwidth.to_string());
                    let mut fx =
                        EffectProcessor::new(&eq_node, 12, 48000., 64, Arc::clone(&resources))
                            .unwrap();
                    let mut bounded = [[0.; MAX_CHANNELS]; 512];
                    bounded[0] = [1.; MAX_CHANNELS];
                    fx.process(&mut bounded).unwrap();
                    assert!(bounded.iter().flatten().all(|x| x.is_finite()));
                }
            }
        }
        let reverb = effect(
            r#"<SampledReverb SamplePath="fixture" NormalizePower="0" Time="1" PreDelay="1" Dry="0" Wet="1"/>"#,
        );
        let mut fx = EffectProcessor::new(&reverb, 12, 48000., 64, Arc::clone(&resources)).unwrap();
        let mut output = input.clone();
        fx.process(&mut output).unwrap();
        assert!(output[..48].iter().flatten().all(|x| x.abs() < 1e-5));
        assert!((output[48][11] - 7.2).abs() < 1e-4);
        assert!(
            fx.set_parameter("DampingHigh", &ParameterValue::Number(0.1))
                .is_err()
        );
        // Official native authored PCM16 basis captures: normalization is
        // after the sequential width transform, and Wet/Dry remain linear.
        for (kind, width, norm, left, right) in [
            ("Convolver", 0., 1, 0.3162277638912201, 0.15811388194561005),
            ("SampledReverb", 0., 0, 0.015625, 0.0078125),
            (
                "SampledReverb",
                1.,
                1,
                0.42515498399734497,
                0.12064718455076218,
            ),
            (
                "SampledReverb",
                -1.,
                1,
                0.3062463104724884,
                0.3186309337615967,
            ),
        ] {
            let extra = if kind == "SampledReverb" {
                format!(" Time=\"1\" PreDelay=\"0\" Width=\"{width}\" SampledReverbVersion=\"1\"")
            } else {
                String::new()
            };
            let node = effect(&format!(
                "<{kind} SamplePath=\"native_stereo\" Dry=\"0\" Wet=\"1\" NormalizePower=\"{norm}\"{extra}/>"
            ));
            for fragments in [1, 7, 64] {
                let mut fx =
                    EffectProcessor::new(&node, 2, 48000., 64, Arc::clone(&resources)).unwrap();
                let mut response = vec![[0.; MAX_CHANNELS]; 97];
                response[0][0] = 0.25;
                response[0][1] = 0.25;
                for block in response.chunks_mut(fragments) {
                    fx.process(block).unwrap();
                }
                assert!((f64::from(response[0][0]) - left).abs() < 8e-8);
                assert!((f64::from(response[0][1]) - right).abs() < 8e-8);
                assert!(response[1..].iter().flatten().all(|x| x.abs() < 1e-6));
            }
        }
        let damping = effect(
            r#"<SampledReverb SamplePath="native_damping" Dry="0" Wet="1" NormalizePower="0" PreDelay="0" SampledReverbVersion="1"/>"#,
        );
        let mut fx = EffectProcessor::new(&damping, 2, 48000., 64, Arc::clone(&resources)).unwrap();
        fx.set_parameter("DampingLow", &ParameterValue::Number(-0.5))
            .unwrap();
        let mut response = vec![[0.; MAX_CHANNELS]; 1100];
        response[0][0] = 0.25;
        response[0][1] = 0.25;
        for block in response.chunks_mut(7) {
            fx.process(block).unwrap();
        }
        for (frame, native) in response[997..1004].iter().zip([
            0.00018013461158261634,
            0.00045085424062563106,
            0.0011284296488156542,
            0.01844931044615805,
            0.0011310198169667274,
            0.0004529259240371175,
            0.00018137769075110555,
        ]) {
            assert!((f64::from(frame[0]) - native).abs() < 4e-9);
        }
        for (predelay, left, right) in [(0., 0.025, 0.00625), (10., 0., 0.)] {
            let empty_first = effect(&format!(
                "<SampledReverb SamplePath=\"zero_first\" Dry=\"0\" Wet=\"1\" NormalizePower=\"0\" PreDelay=\"{predelay}\" SampledReverbVersion=\"1\"/>"
            ));
            let mut fx =
                EffectProcessor::new(&empty_first, 2, 48000., 64, Arc::clone(&resources)).unwrap();
            let mut response = vec![[0.; MAX_CHANNELS]; 1600];
            response[0][0] = 0.25;
            fx.process(&mut response).unwrap();
            assert!((response[0][0] - left).abs() < 1e-8);
            assert!((response[0][1] - right).abs() < 1e-8);
            assert!(response[1..].iter().flatten().all(|x| x.abs() < 1e-6));
        }
        let missing = effect(r#"<Convolver/>"#);
        let mut fx =
            EffectProcessor::new(&missing, 12, 48000., 64, Arc::clone(&resources)).unwrap();
        assert!(fx.process(&mut [[0.; MAX_CHANNELS]]).is_err());
    }
}
