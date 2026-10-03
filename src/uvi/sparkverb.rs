//! Original, bounded SparkVerb feedback-network mathematics.
//!
//! Public parameter facts: https://lua.uvi.net/_elements.html#sparkverb.
//! Authored PCM16 impulses through official Workstation 4.0.9 Windows/Wine,
//! 2026-10-03, established the Hadamard topology, input normalization, shelves,
//! cut filters, stereo matrix, width, mix and predelay. No vendor source
//! expression, SDK source, commercial audio or extracted IR is included.
//! Static numerical models predict 29 complete 48-kHz authored native tails
//! within 1.3e-8. Delay vectors below are measured mathematical quantities;
//! general Room/Shape mapping and delay modulation remain unsupported.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "SparkVerb currently supports measured static 48-kHz delay layouts only; delay modulation, general Room/Shape mapping, mono insert promotion, live control smoothing and bypass transitions remain unsupported or native-unverified";

// name, default, minimum, maximum, integer. Native 4.0.9 defaults MixMode to 0.
const PARAMETERS: &[(&str, f64, f64, f64, bool)] = &[
    ("Bypass", 0., 0., 1., true),
    ("ModDepth", 4., 0., 20., false),
    ("ModRate", 1., 0.25, 4., false),
    ("Diffusion", 0.618, 0., 1., false),
    ("DiffusionStart", 5., 1., 10., false),
    ("Width", 1., 0., 1., false),
    ("RoomSize", 20., 4., 50., false),
    ("DecayTime", 1., 0.1, 10., false),
    ("DecayLow", 1., 0.1, 10., false),
    ("DecayHigh", 1., 0.1, 10., false),
    ("FreqLow", 250., 10., 1000., false),
    ("FreqHigh", 12000., 2000., 20000., false),
    ("Shape", 0., 0., 1., false),
    ("Mix", 0.5, 0., 1., false),
    ("Quality", 3., 2., 4., true),
    ("Mode", 1., 0., 2., true),
    ("HiCut", 0., 0., 1., true),
    ("LowCut", 0., 0., 1., true),
    ("Rolloff", 20000., 2000., 20000., false),
    ("DiffusionOnOff", 0., 0., 1., true),
    ("PreDelay", 0., 0., 100., false),
    ("MixMode", 0., 0., 1., true),
    ("SparkVerbVersion", 1., 1., 1., true),
];
type Parameters = BTreeMap<String, f64>;

pub fn supports(kind: &str) -> bool {
    kind == "SparkVerb"
}

fn checked(name: &str, value: &ParameterValue) -> Result<f64> {
    let (_, _, min, max, integer) = PARAMETERS
        .iter()
        .find(|p| p.0 == name)
        .with_context(|| format!("Unsupported SparkVerb parameter {name}"))?;
    let value = match value {
        ParameterValue::Number(v) => *v,
        ParameterValue::Boolean(v) => f64::from(u8::from(*v)),
        _ => bail!("SparkVerb requires a finite numeric parameter"),
    };
    ensure!(
        value.is_finite() && (*min..=*max).contains(&value) && (!integer || value.fract() == 0.),
        "Invalid SparkVerb parameter {name}"
    );
    Ok(f64::from(value as f32))
}

fn delays(p: &Parameters) -> Result<&'static [usize]> {
    ensure!(
        p["ModDepth"] == 0.,
        "SparkVerb modulation is not implemented"
    );
    if p["Quality"] == 3. && p["RoomSize"] == 20. {
        match p["Shape"] {
            0.5 => return Ok(&[1223, 1487, 1823, 2213, 2693, 3299, 4003, 4889]),
            1. => return Ok(&[787, 1049, 1423, 1901, 2557, 3449, 4637, 6247]),
            _ => (),
        }
    }
    ensure!(p["Shape"] == 0., "SparkVerb room shape is not implemented");
    match (p["Quality"] as u8, p["RoomSize"]) {
        (2, 20.) => Ok(&[1709, 2143, 2699, 3407]),
        (3, 4.) => Ok(&[367, 401, 443, 487, 541, 593, 659, 727]),
        (3, 10.) => Ok(&[907, 997, 1103, 1217, 1361, 1481, 1637, 1811]),
        (3, 20.) => Ok(&[1811, 1993, 2203, 2437, 2683, 2963, 3271, 3613]),
        (3, 50.) => Ok(&[4513, 4987, 5501, 6079, 6709, 7411, 8179, 9029]),
        (4, 20.) => Ok(&[
            1877, 1973, 2063, 2153, 2267, 2371, 2473, 2591, 2713, 2843, 2999, 3119, 3271, 3433,
            3581, 3761,
        ]),
        _ => bail!("SparkVerb RoomSize/Quality delay layout is not implemented"),
    }
}

fn parameters(node: &ProgramNode) -> Result<Parameters> {
    ensure!(supports(&node.kind), "Unsupported UVI effect {}", node.kind);
    let mut p: Parameters = PARAMETERS
        .iter()
        .map(|&(name, value, ..)| (name.into(), f64::from(value as f32)))
        .collect();
    for (name, raw) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let value = ParameterValue::Number(
            raw.parse()
                .with_context(|| format!("Invalid SparkVerb parameter {name}"))?,
        );
        p.insert(name.clone(), checked(name, &value)?);
    }
    delays(&p)?;
    Ok(p)
}

pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

#[derive(Default)]
struct Filter {
    b0: f64,
    b1: f64,
    pole: f64,
    state: f64,
}
impl Filter {
    fn tune(&mut self, d: usize, ratio: f64, frequency: f64, time: f64, high: bool, cut: bool) {
        let t = (std::f64::consts::PI * frequency / 48000.).tan();
        let (b0, b1, pole) = if cut {
            let den = 1. + t;
            if high {
                let gain = 1. / (1. + (24000. / frequency).powi(2)).sqrt();
                ((gain + t) / den, (t - gain) / den, (1. - t) / den)
            } else {
                let gain = 1. / (1. + (frequency / 24000.).powi(2)).sqrt();
                (gain / den, -gain / den, (1. - t) / den)
            }
        } else {
            let gain = (-1000f64.ln() * d as f64 / (2. * 48000. * time) * (1. / ratio - 1.)).exp();
            let a = gain.sqrt();
            if high {
                let den = 1. + a * t;
                (
                    (gain + a * t) / den,
                    (a * t - gain) / den,
                    (1. - a * t) / den,
                )
            } else {
                let den = 1. + t / a;
                ((1. + a * t) / den, (a * t - 1.) / den, (1. - t / a) / den)
            }
        };
        self.b0 = b0;
        self.b1 = b1;
        self.pole = pole;
    }
    fn step(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.state;
        self.state = self.b1 * x + self.pole * y;
        y
    }
}

struct Line {
    delay: Vec<f64>,
    position: usize,
    gain: f64,
    low: Filter,
    high: Filter,
    dc: f64,
}

pub struct SparkVerb {
    parameters: Parameters,
    lines: Vec<Line>,
    input_gain: f64,
    diffusion: [[Vec<f64>; 4]; 2],
    diffusion_positions: [[usize; 4]; 2],
    predelay: Vec<[f64; 2]>,
    pre_position: usize,
    rolloff: [f64; 2],
    rolloff_coefficient: f64,
}
impl SparkVerb {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            channels == 2,
            "SparkVerb requires a stereo bus; mono promotion is unverified"
        );
        ensure!(
            rate == 48000.,
            "SparkVerb requires a measured 48-kHz sample rate"
        );
        let parameters = parameters(node)?;
        let lines = delays(&parameters)?
            .iter()
            .map(|&d| Line {
                delay: vec![0.; d],
                position: 0,
                gain: 0.,
                low: Filter::default(),
                high: Filter::default(),
                dc: 0.,
            })
            .collect();
        let predelay = vec![[0.; 2]; (parameters["PreDelay"] * 48.) as usize];
        // Four input allpasses use pi/2 spacing. An observed 23-frame
        // stagger alternates channels/stages at this measured 48-kHz rate.
        let diffusion = std::array::from_fn(|ch| {
            std::array::from_fn(|stage| {
                let delay = (parameters["DiffusionStart"]
                    * 48.
                    * std::f64::consts::FRAC_PI_2.powi(stage as i32))
                .round() as usize;
                vec![0.; delay + if stage % 2 != ch { 23 } else { 0 }]
            })
        });
        let mut result = Self {
            parameters,
            diffusion,
            diffusion_positions: [[0; 4]; 2],
            lines,
            input_gain: 0.,
            predelay,
            pre_position: 0,
            rolloff: [0.; 2],
            rolloff_coefficient: 1.,
        };
        result.tune();
        Ok(result)
    }
    fn tune(&mut self) {
        let p = &self.parameters;
        let time = p["DecayTime"] * p["RoomSize"] / 20.;
        let mut power = 0.;
        for line in &mut self.lines {
            let d = line.delay.len();
            line.gain = (-1000f64.ln() * d as f64 / (2. * 48000. * time)).exp();
            power += line.gain * line.gain;
            line.low.tune(
                d,
                p["DecayLow"],
                p["FreqLow"],
                time,
                false,
                p["LowCut"] != 0.,
            );
            line.high.tune(
                d,
                p["DecayHigh"],
                p["FreqHigh"],
                time,
                true,
                p["HiCut"] != 0.,
            );
        }
        let n = self.lines.len() as f64;
        self.input_gain = ((n - power) / (n + power)).sqrt();
        self.rolloff_coefficient = if p["Rolloff"] == 20000. {
            1.
        } else {
            1. - (-std::f64::consts::TAU * p["Rolloff"] / 48000.).exp()
        };
    }
    pub fn output_channels(&self) -> usize {
        2
    }
    /// Retained heap DSP state; the caller accounts for the boxed Self.
    pub fn memory_bytes(&self) -> usize {
        self.lines.capacity() * std::mem::size_of::<Line>()
            + self
                .lines
                .iter()
                .map(|line| line.delay.capacity() * std::mem::size_of::<f64>())
                .sum::<usize>()
            + self.predelay.capacity() * std::mem::size_of::<[f64; 2]>()
            + self
                .diffusion
                .iter()
                .flatten()
                .map(|line| line.capacity() * std::mem::size_of::<f64>())
                .sum::<usize>()
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        self.parameters
            .get(name)
            .copied()
            .map(ParameterValue::Number)
            .context("Unknown SparkVerb parameter")
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let value = checked(name, value)?;
        let old = self.parameters[name];
        // Retiming a live FDN has an unmeasured state contract. Do not silently
        // clear its retained tail when the requested layout changes.
        ensure!(
            !matches!(
                name,
                "RoomSize" | "Shape" | "Quality" | "PreDelay" | "DiffusionStart" | "DiffusionOnOff"
            ) || old == value,
            "Live SparkVerb delay-layout updates are not implemented"
        );
        let mut p = self.parameters.clone();
        p.insert(name.into(), value);
        delays(&p)?;
        self.parameters = p;
        self.tune();
        Ok(())
    }
    pub fn clear(&mut self) {
        for line in &mut self.lines {
            line.delay.fill(0.);
            line.position = 0;
            line.low.state = 0.;
            line.high.state = 0.;
            line.dc = 0.;
        }
        for line in self.diffusion.iter_mut().flatten() {
            line.fill(0.);
        }
        self.diffusion_positions = [[0; 4]; 2];
        self.predelay.fill([0.; 2]);
        self.pre_position = 0;
        self.rolloff = [0.; 2];
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames.iter().all(|f| f[..2].iter().all(|x| x.is_finite())),
            "Nonfinite SparkVerb input"
        );
        if self.parameters["Bypass"] != 0. {
            return Ok(());
        }
        let n = self.lines.len();
        let normalization = 1. / (n as f64).sqrt();
        let dc_coefficient = 1. - (-std::f64::consts::TAU / 48000.).exp();
        let angle = self.parameters["Width"] * std::f64::consts::FRAC_PI_4;
        let (side, mid) = angle.sin_cos();
        let mix = self.parameters["Mix"];
        let [dry_gain, wet_gain] = if self.parameters["MixMode"] == 0. {
            [1. - mix, mix]
        } else {
            [(1. - mix).sqrt(), mix.sqrt()]
        };
        for frame in frames {
            let dry = [f64::from(frame[0]), f64::from(frame[1])];
            let mut input = dry;
            if self.parameters["DiffusionOnOff"] != 0. {
                let a = self.parameters["Diffusion"];
                for ch in 0..2 {
                    for stage in 0..4 {
                        let line = &mut self.diffusion[ch][stage];
                        let position = &mut self.diffusion_positions[ch][stage];
                        let y = a * input[ch] + line[*position];
                        line[*position] = input[ch] - a * y;
                        *position = (*position + 1) % line.len();
                        input[ch] = y;
                    }
                }
            }
            let mut delayed = [0.; 16];
            for (i, line) in self.lines.iter().enumerate() {
                delayed[i] = line.delay[line.position] * line.gain;
            }
            for (i, line) in self.lines.iter_mut().enumerate() {
                let feedback = delayed[..n]
                    .iter()
                    .enumerate()
                    .map(|(j, &v)| {
                        if (i & j).count_ones().is_multiple_of(2) {
                            v
                        } else {
                            -v
                        }
                    })
                    .sum::<f64>()
                    * normalization;
                let filtered = line.high.step(line.low.step(feedback));
                line.dc += dc_coefficient * (filtered - line.dc);
                line.delay[line.position] = filtered - line.dc + input[i % 2] * self.input_gain;
                line.position = (line.position + 1) % line.delay.len();
            }
            let raw = [
                mid * delayed[0] + side * delayed[1],
                mid * delayed[0] - side * delayed[1],
            ];
            let mut wet = [0.; 2];
            for ch in 0..2 {
                self.rolloff[ch] += self.rolloff_coefficient * (raw[ch] - self.rolloff[ch]);
                wet[ch] = self.rolloff[ch];
            }
            if !self.predelay.is_empty() {
                std::mem::swap(&mut wet, &mut self.predelay[self.pre_position]);
                self.pre_position = (self.pre_position + 1) % self.predelay.len();
            }
            for ch in 0..2 {
                frame[ch] = (dry_gain * dry[ch] + wet_gain * wet[ch]) as f32;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(attrs: &[(&str, f64)]) -> ProgramNode {
        ProgramNode {
            parent: None,
            kind: "SparkVerb".into(),
            name: None,
            attributes: [("ModDepth", 0.), ("Mix", 1.)]
                .into_iter()
                .chain(attrs.iter().copied())
                .map(|(k, v)| (k.into(), v.to_string()))
                .collect(),
            text: String::new(),
        }
    }
    #[test]
    fn uvi_sparkverb_matches_native_static_network_and_output() {
        // Original PCM16 .25 impulse, native centered mono bus=.125 per side.
        for (attrs, first, want, second, want2) in [
            (
                vec![],
                1811,
                0.03334677591919899,
                3804,
                0.020425494760274887,
            ),
            (
                vec![("Shape", 0.5)],
                1223,
                0.03483111038804054,
                2446,
                0.01127578318119049,
            ),
            (
                vec![("Shape", 1.)],
                787,
                0.03552202880382538,
                1574,
                0.011865940876305103,
            ),
            (
                vec![("DiffusionOnOff", 1.)],
                1811,
                0.004864158108830452,
                2051,
                0.00486475694924593,
            ),
            (
                vec![("DiffusionOnOff", 1.), ("Diffusion", 0.)],
                3996,
                0.03334677591919899,
                5807,
                0.010348043404519558,
            ),
            (
                vec![("RoomSize", 4.)],
                367,
                0.03337910398840904,
                734,
                0.01034020259976387,
            ),
            (
                vec![("Quality", 2.)],
                1709,
                0.03271888196468353,
                3418,
                0.014464563690125942,
            ),
            (
                vec![("Quality", 4.)],
                1877,
                0.033790603280067444,
                3754,
                0.007379430346190929,
            ),
            (
                vec![("DecayHigh", 0.5)],
                1811,
                0.03334677591919899,
                3622,
                0.00969530176371336,
            ),
            (
                vec![("HiCut", 1.)],
                1811,
                0.03334677591919899,
                3622,
                0.007487914990633726,
            ),
            (
                vec![("PreDelay", 0.03125)],
                1812,
                0.03334677591919899,
                3805,
                0.020425494760274887,
            ),
        ] {
            let mut fx = SparkVerb::new(&node(&attrs), 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 7000];
            frames[0][0] = 0.125;
            frames[0][1] = 0.125;
            fx.process(&mut frames).unwrap();
            assert!(frames[..first].iter().all(|f| f[0] == 0.));
            for (i, v) in [(first, want), (second, want2)] {
                assert!(
                    (f64::from(frames[i][0]) - v).abs() < 2e-8,
                    "{attrs:?} {i}: {}",
                    frames[i][0]
                );
            }
            assert!(
                fx.set_parameter("ModDepth", &ParameterValue::Number(1.))
                    .is_err()
            );
            assert_eq!(
                fx.parameter("ModDepth").unwrap(),
                ParameterValue::Number(0.)
            );
            fx.clear();
            assert!(
                fx.set_parameter("RoomSize", &ParameterValue::Number(12.))
                    .is_err()
            );
        }
        let mut fx = SparkVerb::new(&node(&[("Mix", 0.25), ("MixMode", 1.)]), 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 2000];
        frames[0][0] = 0.125;
        frames[0][1] = 0.125;
        fx.process(&mut frames).unwrap();
        assert!((frames[0][0] - 0.10825317353010178).abs() < 1e-8);
        assert!((frames[1811][0] - 0.016673387959599495).abs() < 1e-8);
        assert!(SparkVerb::new(&node(&[("ModDepth", 1.)]), 2, 48000.).is_err());
        assert!(SparkVerb::new(&node(&[]), 2, 44100.).is_err());
    }
}
