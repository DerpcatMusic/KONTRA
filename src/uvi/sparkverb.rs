//! Original, bounded SparkVerb feedback-network mathematics.
//!
//! Public parameter facts: https://lua.uvi.net/_elements.html#sparkverb.
//! Authored PCM16 impulses through official Workstation 4.0.9 Windows/Wine,
//! 2026-10-03, established the Hadamard topology, input normalization, shelves,
//! cut filters, stereo matrix, width, mix and predelay. No vendor source
//! expression, SDK source, commercial audio or extracted IR is included.
//! Static numerical models predict 54 complete 48-kHz authored native tails
//! within 1.3e-8. Delay vectors below are measured mathematical quantities;
//! unmeasured Shape/Quality combinations and moving Mode0 remain unsupported.
//! Interval-bounded RoomSize mapping rejects uncertain prime-delay boundaries.
//! Mode1/2 moving-delay models additionally match nine full native tails
//! within 5e-6; native floating-point parity remains unverified.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "SparkVerb currently supports measured 48-kHz delay layouts; moving Mode0, unmeasured Shape/Quality combinations and uncertain prime-delay boundaries, mono insert promotion, live control smoothing and bypass transitions remain unsupported or native-unverified; measured Mode1/2 modulation tails agree within 5e-6 but native float parity is unverified";

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

fn next_prime(mut value: usize) -> usize {
    value = value.max(2);
    while (2..=(value as f64).sqrt() as usize).any(|divisor| value.is_multiple_of(divisor)) {
        value += 1;
    }
    value
}

fn delays(p: &Parameters) -> Result<Vec<usize>> {
    ensure!(
        p["ModDepth"] == 0. || p["Mode"] != 0.,
        "SparkVerb moving Mode0 native rounding is not implemented"
    );
    // Preserve individually verified native layouts, including their boundaries.
    let measured: Option<&[usize]> = match (p["Quality"] as u8, p["RoomSize"], p["Shape"]) {
        (2, 20., 0.) => Some(&[1709, 2143, 2699, 3407]),
        (3, 4., 0.) => Some(&[367, 401, 443, 487, 541, 593, 659, 727]),
        (3, 10., 0.) => Some(&[907, 997, 1103, 1217, 1361, 1481, 1637, 1811]),
        (3, 20., 0.) => Some(&[1811, 1993, 2203, 2437, 2683, 2963, 3271, 3613]),
        (3, 50., 0.) => Some(&[4513, 4987, 5501, 6079, 6709, 7411, 8179, 9029]),
        (4, 20., 0.) => Some(&[
            1877, 1973, 2063, 2153, 2267, 2371, 2473, 2591, 2713, 2843, 2999, 3119, 3271, 3433,
            3581, 3761,
        ]),
        (3, 20., 0.5) => Some(&[1223, 1487, 1823, 2213, 2693, 3299, 4003, 4889]),
        (3, 20., 1.) => Some(&[787, 1049, 1423, 1901, 2557, 3449, 4637, 6247]),
        (4, 20., 0.5) => Some(&[
            1013, 1151, 1277, 1433, 1609, 1811, 2027, 2281, 2557, 2879, 3217, 3613, 4057, 4561,
            5107, 5737,
        ]),
        (4, 20., 1.0) => Some(&[
            499, 599, 727, 877, 1049, 1259, 1511, 1823, 2203, 2633, 3163, 3821, 4583, 5507, 6637,
            7993,
        ]),
        _ => None,
    };
    if let Some(measured) = measured {
        return Ok(measured.to_vec());
    }
    // Authored native threshold probes bound the unrounded seconds per RoomSize unit
    // coefficient at these fixed Shape/Quality pairs. No Shape interpolation.
    let (lower, upper) = match (p["Quality"] as u8, p["Shape"]) {
        (2, 0.) => (0.001771519600861144, 0.0017715204377366583),
        (3, 0.) => (0.0018804209110257252, 0.0018804226402518885),
        (4, 0.) => (0.0019527193467307564, 0.0019527200874251624),
        (3, 0.5) => (0.0012706153921700843, 0.0012706163608489076),
        (3, 1.) => (0.0008119173683025765, 0.0008119182878511134),
        (4, 0.5) => (0.0010557298735631917, 0.0010557303800642363),
        (4, 1.0) => (0.0005187613563812313, 0.0005187625887079876),
        _ => bail!("SparkVerb Shape/Quality coefficient is not measured"),
    };
    let count = 1usize << p["Quality"] as u32;
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let factor = p["RoomSize"]
            * 48000.
            * 2f64.powf((1. + (p["Quality"] - 1.) * p["Shape"]) * i as f64 / (count - 1) as f64);
        // A conservative .05-frame envelope also excludes uncertain native
        // float rounding. Floor first, then choose the next prime inclusively.
        let a = next_prime((lower * factor - 0.05).floor() as usize);
        let b = next_prime((upper * factor + 0.05).floor() as usize);
        ensure!(
            a == b,
            "SparkVerb RoomSize is too close to an uncertain prime-delay boundary"
        );
        result.push(a);
    }
    Ok(result)
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
    nominal_delay: usize,
    modulation_amplitude: f64,
    phase_step: u32,
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
    clock: u64,
    sine_table: [f32; 257],
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
            .enumerate()
            .map(|(i, &d)| {
                let frequency = parameters["ModRate"] / (16. * d as f64);
                let amplitude = parameters["ModDepth"] * std::f64::consts::LN_2
                    / (1200. * std::f64::consts::TAU * frequency);
                Line {
                    nominal_delay: d,
                    modulation_amplitude: if i % 2 == 0 { amplitude } else { -amplitude },
                    phase_step: (frequency * 4294967296.).floor() as u32,
                    delay: vec![
                        0.;
                        d + if amplitude == 0. {
                            0
                        } else {
                            amplitude.ceil() as usize + 3
                        }
                    ],
                    position: 0,
                    gain: 0.,
                    low: Filter::default(),
                    high: Filter::default(),
                    dc: 0.,
                }
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
            clock: 0,
            sine_table: std::array::from_fn(|i| {
                if i == 256 {
                    0.
                } else {
                    (std::f64::consts::TAU * i as f64 / 256.).sin() as f32
                }
            }),
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
            let d = line.nominal_delay;
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
                "RoomSize"
                    | "Shape"
                    | "Quality"
                    | "PreDelay"
                    | "DiffusionStart"
                    | "DiffusionOnOff"
                    | "ModDepth"
                    | "ModRate"
                    | "Mode"
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
        self.clock = 0;
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
        let mode = self.parameters["Mode"] as u8;
        for frame in frames {
            let dry = [f64::from(frame[0]), f64::from(frame[1])];
            let mut input = dry;
            if !self.predelay.is_empty() {
                std::mem::swap(&mut input, &mut self.predelay[self.pre_position]);
                self.pre_position = (self.pre_position + 1) % self.predelay.len();
            }
            if self.parameters["DiffusionOnOff"] != 0. {
                let a = self.parameters["Diffusion"];
                for (ch, channel) in input.iter_mut().enumerate() {
                    for stage in 0..4 {
                        let line = &mut self.diffusion[ch][stage];
                        let position = &mut self.diffusion_positions[ch][stage];
                        let y = a * *channel + line[*position];
                        line[*position] = *channel - a * y;
                        *position = (*position + 1) % line.len();
                        *channel = y;
                    }
                }
            }
            let mut delayed = [0.; 16];
            for (i, line) in self.lines.iter().enumerate() {
                let value = if line.modulation_amplitude == 0. {
                    line.delay[line.position]
                } else {
                    // Native table endpoints are interpolated over 64 host frames.
                    let block = self.clock & !63;
                    let fraction = (self.clock & 63) as f64 / 64.;
                    let a = table_sine(&self.sine_table, block, line.phase_step);
                    let b = table_sine(&self.sine_table, block.wrapping_add(64), line.phase_step);
                    let delay = line.nominal_delay as f64
                        + line.modulation_amplitude * (a * (1. - fraction) + b * fraction);
                    let position =
                        (line.position as f64 - delay).rem_euclid(line.delay.len() as f64);
                    let index = position.floor() as usize;
                    let f = position - index as f64;
                    let at = |offset: usize| {
                        line.delay[(index + line.delay.len() + offset - 1) % line.delay.len()]
                    };
                    if mode == 1 {
                        at(1) * (1. - f) + at(2) * f
                    } else {
                        // Four-point Lagrange support [-1, 0, 1, 2].
                        at(0) * (-f * (f - 1.) * (f - 2.) / 6.)
                            + at(1) * ((f + 1.) * (f - 1.) * (f - 2.) / 2.)
                            + at(2) * (-(f + 1.) * f * (f - 2.) / 2.)
                            + at(3) * ((f + 1.) * f * (f - 1.) / 6.)
                    }
                };
                delayed[i] = value * line.gain;
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
            for (ch, state) in self.rolloff.iter_mut().enumerate() {
                *state += self.rolloff_coefficient * (raw[ch] - *state);
            }
            let wet = self.rolloff;
            for ch in 0..2 {
                frame[ch] = (dry_gain * dry[ch] + wet_gain * wet[ch]) as f32;
            }
            self.clock = self.clock.wrapping_add(1);
        }
        Ok(())
    }
}

fn table_sine(table: &[f32; 257], clock: u64, step: u32) -> f64 {
    let phase = clock.wrapping_mul(u64::from(step)) as u32;
    let index = (phase >> 24) as usize;
    let fraction = (phase & 0x00ff_ffff) as f32 / 16777216.;
    f64::from(table[index] + (table[index + 1] - table[index]) * fraction)
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
        assert!((frames[0][0] - 0.108_253_17).abs() < 1e-8);
        assert!((frames[1811][0] - 0.016_673_388).abs() < 1e-8);
        assert!(SparkVerb::new(&node(&[("ModDepth", 1.), ("Mode", 0.)]), 2, 48000.).is_err());
        assert!(SparkVerb::new(&node(&[]), 2, 44100.).is_err());
    }
    #[test]
    fn uvi_sparkverb_matches_native_moving_delay_clock() {
        // Original stereo PCM16 impulse at host frame8192, native Workstation4.0.9.
        type Attributes<'a> = &'a [(&'a str, f64)];
        type NativeTaps<'a> = &'a [(usize, f32)];
        let cases: &[(Attributes<'_>, NativeTaps<'_>)] = &[
            (
                &[("ModDepth", 1.)],
                &[(1813, 0.026664546), (1990, 0.021647312)],
            ),
            (
                &[("ModDepth", 4.), ("Mode", 2.)],
                &[(1818, -0.0010920132), (1820, 0.028593825)],
            ),
            (
                &[("ModDepth", 4.), ("Quality", 2.)],
                &[(1716, 0.011389901), (2131, 0.030591099)],
            ),
            (
                &[("ModDepth", 4.), ("Quality", 4.)],
                &[(1886, 0.0174198), (1962, 0.015131718)],
            ),
            (
                &[("ModDepth", 4.), ("DiffusionOnOff", 1.), ("Diffusion", 0.5)],
                &[(1819, 0.00043021297), (1982, 0.001314224)],
            ),
            (
                &[("ModDepth", 4.), ("PreDelay", 10.)],
                &[(2299, 0.029308943), (2462, 0.0041384716)],
            ),
        ];
        for &(attrs, expected) in cases {
            let node = node(attrs);
            let mut whole = SparkVerb::new(&node, 2, 48000.).unwrap();
            let mut split = SparkVerb::new(&node, 2, 48000.).unwrap();
            let mut a = vec![[0.; 12]; 13000];
            a[8192][..2].fill(0.125);
            let mut b = a.clone();
            whole.process(&mut a).unwrap();
            for chunk in b.chunks_mut(17) {
                split.process(chunk).unwrap();
            }
            assert_eq!(a, b, "host block partition changed the modulation clock");
            for &(offset, native) in expected {
                assert!(
                    (a[8192 + offset][0] - native).abs() < 5e-6,
                    "{attrs:?} frame{offset}"
                );
            }
            whole.clear();
            let mut reset = vec![[0.; 12]; 13000];
            reset[8192][..2].fill(0.125);
            whole.process(&mut reset).unwrap();
            assert_eq!(a, reset, "clear did not reset the moving-delay clock");
        }
    }
    #[test]
    fn uvi_sparkverb_rejects_native_uncertain_prime_boundaries() {
        // Native: first delay1811 below this transition,1823 above it.
        for room in [20.0752735, 20.0752926] {
            let error = SparkVerb::new(&node(&[("RoomSize", room)]), 2, 48000.)
                .err()
                .unwrap();
            assert!(error.to_string().contains("prime-delay boundary"));
        }
        for (shape, lower, upper) in [(0.5, 20.0098438, 20.0098534), (1., 20.079834, 20.0798817)] {
            for room in [lower, upper] {
                let error = SparkVerb::new(
                    &node(&[("RoomSize", room), ("Quality", 4.), ("Shape", shape)]),
                    2,
                    48000.,
                )
                .err()
                .unwrap();
                assert!(error.to_string().contains("prime-delay boundary"));
            }
        }
        // A later internal line is also only .0028 frames from a boundary.
        assert!(SparkVerb::new(&node(&[("RoomSize", 33.3), ("Shape", 1.)]), 2, 48000.).is_err());
        for (room, shape, quality, first) in [
            (7.25, 1., 3., 283),
            (7.25, 0., 3., 659),
            (33.3, 0., 3., 3011),
            (7.25, 0.5, 3., 443),
            (7.25, 0., 2., 617),
            (33.3, 0., 4., 3121),
            (4., 0., 4., 379),
            (50., 0., 2., 4253),
        ] {
            let fx = SparkVerb::new(
                &node(&[("RoomSize", room), ("Shape", shape), ("Quality", quality)]),
                2,
                48000.,
            )
            .unwrap();
            assert_eq!(fx.lines[0].nominal_delay, first);
        }
    }
}
