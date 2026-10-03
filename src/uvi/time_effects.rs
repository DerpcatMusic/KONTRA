//! Original time-effect mathematics, characterized using authored PCM impulses
//! against official UVI Workstation 4.0.9 (Windows/Wine), 44.1/48/96 kHz,
//! 2026-10-03.
//! Parameter facts: https://lua.uvi.net/_elements.html and UVI Falcon manual
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf.
//! No vendor source or sample-bank content is included. Stationary DualDelay
//! uses measured RC filters, feedback rotation, fractional time and sqrt mixing.
//! WhiteChorus uses measured uint32 phase, a 256-entry sine table, RC crossover,
//! and a common Tone feedback line feeding parallel voices. Its Speed/Depth
//! startup clock and DualDelay modulation amplitude remain calibrated models.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI DualDelay peak EQ uses an RBJ approximation and modulation amplitude is empirically calibrated; WhiteChorus Speed/Depth startup is calibrated and phase-offset/read rounding remains approximate, with live controls outside Mix and rates outside 48 kHz native-unverified; DualDelay control smoothing outside Mix/Feedback/Rotation, rates outside 44.1/48/96 kHz and bypass transitions remain native-unverified";
const MAX_SECONDS: f64 = 5.;
const MODULATION_DETUNE: f64 = 0.00057425;
const MEMORY_LIMIT: usize = 64 << 20;

// name, default, minimum, maximum, integer: published native-unit controls.
const DELAY: &[(&str, f64, f64, f64, bool)] = &[
    ("Bypass", 0., 0., 1., true),
    ("SyncToHost", 0., 0., 1., true),
    ("DelayTime", 0.125, 0.001, 5., false),
    ("Feedback", 0.3, 0., 1., false),
    ("LowCut", 20., 20., 4000., false),
    ("HighCut", 20000., 1000., 20000., false),
    ("PeakFreq", 1000., 20., 20000., false),
    ("PeakGain", 0., -20., 20., false),
    ("PeakQ", 1., 0.1, 10., false),
    ("Mix", 0.5, 0., 1., false),
    ("Rotation", 0., -180., 180., false),
    ("InputWidth", 1., 0., 1., false),
    ("OutputWidth", 1., 0., 1., false),
    ("DelayRatio", 0., -0.9, 0.9, false),
    ("FeedbackRatio", 0., -0.9, 0.9, false),
    ("InputRotation", 0., -1., 1., false),
    ("OutputRotation", 0., -1., 1., false),
    ("ModDepth", 0., 0., 20., false),
    ("ModRate", 1., 0.1, 10., false),
    ("ModChannelOffset", 1., 0., 1., false),
    ("Interpolation", 1., 0., 2., true),
    ("DualDelayVersion", 1., 0., 1., true),
];

const CHORUS: &[(&str, f64, f64, f64, bool)] = &[
    ("Bypass", 0., 0., 1., true),
    ("Speed", 0.2, 0.1, 1., false),
    ("Depth", 5., 1., 40., false),
    ("Edge", 0., -1., 1., false),
    ("Tone", 22050., 2000., 22050., false),
    ("Crossover", 20., 20., 5000., false),
    ("LowGain", 1., 0., 2., false),
    ("Mix", 1., 0., 1., false),
    ("Trim", 0., -12., 0., false),
    ("NumVoices", 4., 2., 8., true),
    ("Mode", 0., 0., 1., true),
];

pub fn supports(kind: &str) -> bool {
    matches!(kind, "DualDelay" | "WhiteChorus")
}

fn scalar(value: &ParameterValue) -> Result<f64> {
    match value {
        ParameterValue::Number(v) if v.is_finite() => Ok(*v),
        ParameterValue::Boolean(v) => Ok(f64::from(u8::from(*v))),
        _ => bail!("UVI time effect requires a finite numeric parameter"),
    }
}

fn checked(kind: &str, name: &str, value: &ParameterValue) -> Result<f64> {
    let controls = if kind == "DualDelay" { DELAY } else { CHORUS };
    let (_, _, low, high, integer) = controls
        .iter()
        .find(|p| p.0 == name)
        .with_context(|| format!("Unsupported {kind} parameter {name}"))?;
    let value = scalar(value)?;
    ensure!(
        (*low..=*high).contains(&value) && (!integer || value.fract() == 0.),
        "Invalid {kind} parameter {name}"
    );
    Ok(f64::from(value as f32))
}

fn parameters(node: &ProgramNode) -> Result<BTreeMap<String, f64>> {
    ensure!(
        supports(&node.kind),
        "Unsupported UVI time effect {}",
        node.kind
    );
    let kind = &node.kind;
    let controls = if kind == "DualDelay" { DELAY } else { CHORUS };
    let mut p: BTreeMap<String, f64> = controls.iter().map(|&(n, v, ..)| (n.into(), v)).collect();
    for (name, raw) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let value = ParameterValue::Number(
            raw.parse()
                .with_context(|| format!("Invalid {kind} parameter {name}"))?,
        );
        p.insert(name.clone(), checked(kind, name, &value)?);
    }
    // Old serialized versions store angular modulation depth. The native
    // loader migrates it to the version-1 value, clamped to the public range.
    if kind == "DualDelay"
        && (!node.attributes.contains_key("DualDelayVersion") || p["DualDelayVersion"] == 0.)
    {
        p.insert(
            "ModDepth".into(),
            (p["ModDepth"] * std::f64::consts::TAU).min(20.),
        );
        p.insert("DualDelayVersion".into(), 1.);
    }
    Ok(p)
}

pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

#[derive(Clone, Copy, Default)]
struct Peak {
    c: [f32; 5],
    z: [[f32; 2]; 2],
}
impl Peak {
    fn tune(&mut self, frequency: f64, gain: f64, q: f64, rate: f64) {
        let w = std::f64::consts::TAU * frequency.min(rate * 0.49) / rate;
        let a = 10f64.powf(gain / 40.);
        let alpha = w.sin() / (2. * q);
        let a0 = 1. + alpha / a;
        // Native feedback EQ normalizes boosts by the maximum band gain.
        let scale = 10f64.powf(-gain.max(0.) / 20.);
        self.c = [
            ((1. + alpha * a) / a0 * scale) as f32,
            (-2. * w.cos() / a0 * scale) as f32,
            ((1. - alpha * a) / a0 * scale) as f32,
            (-2. * w.cos() / a0) as f32,
            ((1. - alpha / a) / a0) as f32,
        ];
    }
    fn step(&mut self, x: f32, channel: usize) -> f32 {
        let [b0, b1, b2, a1, a2] = self.c;
        let y = b0 * x + self.z[channel][0];
        self.z[channel] = [b1 * x - a1 * y + self.z[channel][1], b2 * x - a2 * y];
        y
    }
}

fn rotation(angle: f64) -> [f32; 2] {
    [angle.cos() as f32, angle.sin() as f32]
}
fn rotate([l, r]: [f32; 2], [c, s]: [f32; 2]) -> [f32; 2] {
    [c * l - s * r, s * l + c * r]
}
fn width([l, r]: [f32; 2], w: f32) -> [f32; 2] {
    let (m, s) = ((l + r) * 0.5, (l - r) * 0.5 * w);
    [m + s, m - s]
}
fn ratio(value: f64, r: f64) -> [f32; 2] {
    [
        (value * (1. + r.min(0.))) as f32,
        (value * (1. - r.max(0.))) as f32,
    ]
}

pub struct TimeEffect(TimeProcessor);
enum TimeProcessor {
    Delay(Box<DualDelay>),
    Chorus(Box<WhiteChorus>),
}
impl TimeEffect {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            channels == 2,
            "UVI time effect requires a stereo bus; mono insert promotion is unverified"
        );
        ensure!(
            rate.is_finite() && (8000. ..=192000.).contains(&rate),
            "Invalid UVI time-effect sample rate"
        );
        Ok(Self(match node.kind.as_str() {
            "DualDelay" => TimeProcessor::Delay(Box::new(DualDelay::new(node, channels, rate)?)),
            "WhiteChorus" => TimeProcessor::Chorus(Box::new(WhiteChorus::new(node, rate)?)),
            _ => bail!("Unsupported UVI time effect {}", node.kind),
        }))
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        match &self.0 {
            TimeProcessor::Delay(p) => p.parameter(name),
            TimeProcessor::Chorus(p) => p
                .parameters
                .get(name)
                .copied()
                .map(ParameterValue::Number)
                .context("Unknown WhiteChorus parameter"),
        }
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        match &mut self.0 {
            TimeProcessor::Delay(p) => p.set_parameter(name, value),
            TimeProcessor::Chorus(p) => p.set_parameter(name, value),
        }
    }
    pub fn set_tempo(&mut self, tempo: f64) -> Result<()> {
        ensure!(
            tempo.is_finite() && (1. ..=1000.).contains(&tempo),
            "Invalid UVI time-effect tempo"
        );
        if let TimeProcessor::Delay(p) = &mut self.0 {
            p.set_tempo(tempo)?;
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        match &mut self.0 {
            TimeProcessor::Delay(p) => p.clear(),
            TimeProcessor::Chorus(p) => p.clear(),
        }
    }
    pub fn output_channels(&self) -> usize {
        2
    }
    pub fn memory_bytes(&self) -> usize {
        match &self.0 {
            TimeProcessor::Delay(p) => std::mem::size_of::<DualDelay>() + p.memory_bytes(),
            TimeProcessor::Chorus(p) => {
                std::mem::size_of::<WhiteChorus>()
                    + p.lines.capacity() * std::mem::size_of::<[f32; 2]>()
            }
        }
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        match &mut self.0 {
            TimeProcessor::Delay(p) => p.process(frames),
            TimeProcessor::Chorus(p) => p.process(frames),
        }
    }
}

struct DualDelay {
    rate: f64,
    tempo: f64,
    parameters: BTreeMap<String, f64>,
    lines: Vec<[f32; 2]>,
    position: usize,
    frames: [f32; 2],
    feedback: [f32; 2],
    feedback_target: [f32; 2],
    rotation: f32,
    modulation: [f32; 2],
    elapsed: u64,
    mix: f32,
    mix_gains: [f32; 2],
    mix_smoothing: f32,
    control_changed_at: [u64; 3],
    previous_targets: [f32; 4],
    input_rotation: [f32; 2],
    output_rotation: [f32; 2],
    feedback_rotation: [f32; 2],
    poles: [f32; 2],
    low: [f32; 2],
    high: [f32; 2],
    peak: Peak,
}
impl DualDelay {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            channels == 2,
            "DualDelay requires a stereo bus; mono insert promotion is unverified"
        );
        ensure!(
            rate.is_finite() && (8000. ..=192000.).contains(&rate),
            "Invalid UVI delay sample rate"
        );
        let parameters = parameters(node)?;
        let mix = parameters["Mix"] as f32;
        // Native maximum-time modulation can read beyond five seconds. Retain
        // the full published Depth20/Rate.1 detune envelope to avoid overwrite
        // and a wrapped-index underflow, while keeping preparation bounded.
        let max_detune_seconds = MODULATION_DETUNE * 20. / (std::f64::consts::TAU * 0.1);
        let length = ((MAX_SECONDS + max_detune_seconds) * rate).ceil() as usize + 2;
        ensure!(length * 8 <= MEMORY_LIMIT, "UVI delay exceeds memory bound");
        let mut result = Self {
            rate,
            tempo: 120.,
            parameters,
            lines: vec![[0.; 2]; length],
            position: 0,
            frames: [0.; 2],
            feedback: [0.; 2],
            feedback_target: [0.; 2],
            rotation: 0.,
            modulation: [0.; 2],
            elapsed: 0,
            mix,
            mix_gains: [(1. - mix).sqrt(), mix.sqrt()],
            mix_smoothing: 1. - (-32. / (rate * (433. / 48000.))).exp() as f32,
            control_changed_at: [0; 3],
            previous_targets: [mix, 0., 0., 0.],
            input_rotation: [1., 0.],
            output_rotation: [1., 0.],
            feedback_rotation: [1., 0.],
            poles: [0.; 2],
            low: [0.; 2],
            high: [0.; 2],
            peak: Peak::default(),
        };
        result.tune()?;
        result.feedback = result.feedback_target;
        result.rotation = result.parameters["Rotation"] as f32;
        result.previous_targets = [mix, result.feedback[0], result.feedback[1], result.rotation];
        result.feedback_rotation =
            rotation(f64::from(result.rotation) * std::f64::consts::PI / 180.);
        Ok(result)
    }
    fn tune(&mut self) -> Result<()> {
        let p = &self.parameters;
        let seconds = p["DelayTime"]
            * if p["SyncToHost"] != 0. {
                60. / self.tempo
            } else {
                1.
            };
        ensure!(
            seconds <= MAX_SECONDS,
            "Synced DualDelay exceeds five-second capacity"
        );
        self.frames = ratio(seconds * self.rate, p["DelayRatio"]);
        self.feedback_target = ratio(p["Feedback"], p["FeedbackRatio"]);
        // ponytail: this detune calibration was measured at 44.1/48/96 kHz, not a
        // vendor formula. Refit against other rates before claiming parity.
        let detune =
            self.rate * MODULATION_DETUNE * p["ModDepth"] / (std::f64::consts::TAU * p["ModRate"]);
        self.modulation = self
            .frames
            .map(|n| detune.min(f64::from(n) * 0.005 * p["ModDepth"]) as f32);
        self.input_rotation = rotation(p["InputRotation"] * std::f64::consts::FRAC_PI_4);
        self.output_rotation = rotation(p["OutputRotation"] * std::f64::consts::FRAC_PI_4);
        self.feedback_rotation = rotation(f64::from(self.rotation) * std::f64::consts::PI / 180.);
        self.poles = [p["HighCut"], p["LowCut"]]
            .map(|f| 1. - (-std::f32::consts::TAU * f as f32 / self.rate as f32).exp());
        self.peak
            .tune(p["PeakFreq"], p["PeakGain"], p["PeakQ"], self.rate);
        Ok(())
    }
    /// Retained delay-line bytes for the renderer's aggregate preparation bound.
    pub fn memory_bytes(&self) -> usize {
        self.lines.capacity() * std::mem::size_of::<[f32; 2]>()
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        self.parameters
            .get(name)
            .copied()
            .map(ParameterValue::Number)
            .context("Unknown UVI delay parameter")
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let value = checked("DualDelay", name, value)?;
        let old_feedback = self.feedback_target;
        let old = self
            .parameters
            .insert(name.into(), value)
            .context("Unknown UVI delay parameter")?;
        if let Err(error) = self.tune() {
            self.parameters.insert(name.into(), old);
            self.tune()?;
            return Err(error);
        }
        if value != old
            && let Some(control) = match name {
                "Mix" => Some(0),
                "Feedback" | "FeedbackRatio" => Some(1),
                "Rotation" => Some(2),
                _ => None,
            }
        {
            if self.control_changed_at[control] != self.elapsed {
                match control {
                    0 => self.previous_targets[0] = old as f32,
                    1 => self.previous_targets[1..3].copy_from_slice(&old_feedback),
                    2 => self.previous_targets[3] = old as f32,
                    _ => unreachable!(),
                }
            }
            self.control_changed_at[control] = self.elapsed;
        }
        Ok(())
    }
    pub fn set_tempo(&mut self, tempo: f64) -> Result<()> {
        ensure!(
            tempo.is_finite() && (1. ..=1000.).contains(&tempo),
            "Invalid UVI delay tempo"
        );
        let old = self.tempo;
        self.tempo = tempo;
        if let Err(error) = self.tune() {
            self.tempo = old;
            self.tune()?;
            return Err(error);
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.lines.fill([0.; 2]);
        self.position = 0;
        self.elapsed = 0;
        self.control_changed_at = [0; 3];
        self.mix = self.parameters["Mix"] as f32;
        self.mix_gains = [(1. - self.mix).sqrt(), self.mix.sqrt()];
        self.low = [0.; 2];
        self.high = [0.; 2];
        self.peak.z = [[0.; 2]; 2];
        self.feedback = self.feedback_target;
        self.rotation = self.parameters["Rotation"] as f32;
        self.previous_targets = [self.mix, self.feedback[0], self.feedback[1], self.rotation];
        self.feedback_rotation = rotation(f64::from(self.rotation) * std::f64::consts::PI / 180.);
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames.iter().all(|f| f[..2].iter().all(|x| x.is_finite())),
            "Nonfinite UVI delay input"
        );
        if self.parameters["Bypass"] != 0. {
            return Ok(());
        }
        for f in frames {
            // Native Lua setters and CC connections both leave the old Mix for
            // the first control quantum, then advance this RC law every 32 frames.
            if self.elapsed != 0 && self.elapsed.is_multiple_of(32) {
                // A setter on this boundary supplies the next quantum's
                // target. Advance toward the previous frame's target so a
                // continuously changing signal cannot starve the smoother.
                let mix_target = if self.elapsed == self.control_changed_at[0] {
                    self.previous_targets[0]
                } else {
                    self.parameters["Mix"] as f32
                };
                self.mix += self.mix_smoothing * (mix_target - self.mix);
                self.mix_gains = [(1. - self.mix).sqrt(), self.mix.sqrt()];
                for ch in 0..2 {
                    let target = if self.elapsed == self.control_changed_at[1] {
                        self.previous_targets[1 + ch]
                    } else {
                        self.feedback_target[ch]
                    };
                    self.feedback[ch] += self.mix_smoothing * (target - self.feedback[ch]);
                }
                let rotation_target = if self.elapsed == self.control_changed_at[2] {
                    self.previous_targets[3]
                } else {
                    self.parameters["Rotation"] as f32
                };
                self.rotation += self.mix_smoothing * (rotation_target - self.rotation);
                self.feedback_rotation =
                    rotation(f64::from(self.rotation) * std::f64::consts::PI / 180.);
            }
            let [dry, wet] = self.mix_gains;
            let input = [f[0], f[1]];
            let mut delayed = [0.; 2];
            for (ch, y) in delayed.iter_mut().enumerate() {
                let phase =
                    std::f64::consts::TAU * self.parameters["ModRate"] * self.elapsed as f64
                        / self.rate
                        + ch as f64 * std::f64::consts::PI * self.parameters["ModChannelOffset"];
                let delay = (self.frames[ch] + self.modulation[ch] * phase.sin() as f32).max(1.);
                let whole = if self.parameters["Interpolation"] == 0. {
                    delay.ceil()
                } else {
                    delay.floor()
                } as usize;
                let frac = if self.parameters["Interpolation"] == 0. {
                    0.
                } else {
                    delay - whole as f32
                };
                let length = self.lines.len();
                let i = (self.position + length - whole) % length;
                let at = |offset: usize| self.lines[offset % length][ch];
                let a = at(i);
                let b = at(i + length - 1);
                let sample = if self.parameters["Interpolation"] == 2. {
                    // Four-point Lagrange: native negative pre/post impulse taps
                    // distinguish this from Catmull-Rom cubic interpolation.
                    let t = frac;
                    -t * (1. - t) * (2. - t) / 6. * at(i + 1)
                        + (1. + t) * (1. - t) * (2. - t) / 2. * a
                        + (1. + t) * t * (2. - t) / 2. * b
                        - (1. + t) * t * (1. - t) / 6. * at(i + length - 2)
                } else {
                    a + (b - a) * frac
                };
                *y = sample;
            }
            let back = rotate(delayed, self.feedback_rotation);
            let source = rotate(
                width(input, self.parameters["InputWidth"] as f32),
                self.input_rotation,
            );
            let mut next = [0.; 2];
            for ch in 0..2 {
                // Native feedback gain is stored with the input: a live gain step
                // leaves the first delayed echo unchanged, then affects repeats.
                let x = (source[ch] + back[ch]) * self.feedback[ch];
                self.low[ch] += self.poles[0] * (x - self.low[ch]);
                self.high[ch] += self.poles[1] * (self.low[ch] - self.high[ch]);
                let filtered = self.low[ch] - self.high[ch];
                next[ch] = if self.parameters["PeakGain"] == 0. {
                    filtered
                } else {
                    self.peak.step(filtered, ch)
                };
            }
            self.lines[self.position] = next;
            self.position = (self.position + 1) % self.lines.len();
            self.elapsed = self.elapsed.wrapping_add(1);
            let out = rotate(
                width(delayed, self.parameters["OutputWidth"] as f32),
                self.output_rotation,
            );
            for ch in 0..2 {
                f[ch] = dry * input[ch] + wet * out[ch];
            }
            ensure!(
                f[..2].iter().all(|x| x.is_finite()),
                "Nonfinite UVI delay output"
            );
        }
        Ok(())
    }
}

struct WhiteChorus {
    rate: f64,
    parameters: BTreeMap<String, f64>,
    lines: Vec<[f32; 2]>,
    position: usize,
    elapsed: u64,
    phase: u32,
    phases: [u32; 8],
    sine: [f32; 257],
    speed: f64,
    depth: f64,
    startup_smoothing: f64,
    speed_target: f64,
    depth_target: f64,
    speed_changed_at: u64,
    depth_changed_at: u64,
    mix: f32,
    mix_smoothing: f32,
    mix_changed_at: u64,
    previous_targets: [f64; 3],
    low: [f32; 2],
    tone: [f32; 2],
    poles: [f32; 2],
    gain: f32,
    trim: f32,
    voices: usize,
}
impl WhiteChorus {
    fn new(node: &ProgramNode, rate: f64) -> Result<Self> {
        let parameters = parameters(node)?;
        let mix = parameters["Mix"] as f32;
        let speed_target = parameters["Speed"].ln();
        let depth_target = parameters["Depth"];
        // Depth40/Speed.1 reaches less than 75ms, including all voice extrema.
        let length = (rate * 0.1).ceil() as usize + 2;
        let mut result = Self {
            rate,
            parameters,
            lines: vec![[0.; 2]; length],
            position: 0,
            elapsed: 0,
            phase: 0,
            phases: [0; 8],
            sine: std::array::from_fn(|i| {
                if i == 256 {
                    0.
                } else {
                    (std::f64::consts::TAU * i as f64 / 256.).sin() as f32
                }
            }),
            speed: 0.2,
            depth: 5.,
            // ponytail: native startup is approximately 300ms. This calibrated
            // clock remains diagnosed until its exact update quantum is measured.
            startup_smoothing: 1. - (-1. / (rate * 0.2997596)).exp(),
            speed_target,
            depth_target,
            speed_changed_at: 0,
            depth_changed_at: 0,
            mix,
            mix_smoothing: 1. - (-32. / (rate * (433. / 48000.))).exp() as f32,
            mix_changed_at: 0,
            previous_targets: [f64::from(mix), speed_target, depth_target],
            low: [0.; 2],
            tone: [0.; 2],
            poles: [0.; 2],
            gain: 0.,
            trim: 1.,
            voices: 4,
        };
        result.tune();
        Ok(result)
    }
    fn tune(&mut self) {
        let p = &self.parameters;
        self.voices = p["NumVoices"] as usize;
        self.gain = if p["Edge"] <= 0. {
            ((1. + p["Edge"]) / self.voices as f64) as f32
        } else {
            (1. / self.voices as f64 + (0.95 - 1. / self.voices as f64) * p["Edge"]) as f32
        };
        self.poles = [p["Crossover"], p["Tone"]]
            .map(|f| 1. - (-std::f32::consts::TAU * f as f32 / self.rate as f32).exp());
        self.trim = 10f64.powf(p["Trim"] / 20.) as f32;
        for (i, phase) in self.phases.iter_mut().enumerate() {
            let denominator = if p["Mode"] == 0. { self.voices } else { 8 };
            *phase = ((2f64.powf(i as f64 / denominator as f64) - 1.) * 4294967296.) as u32;
        }
    }
    fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let value = checked("WhiteChorus", name, value)?;
        let old = self
            .parameters
            .insert(name.into(), value)
            .context("Unknown WhiteChorus parameter")?;
        if old != value {
            match name {
                "Mix" => {
                    if self.mix_changed_at != self.elapsed {
                        self.previous_targets[0] = old;
                    }
                    self.mix_changed_at = self.elapsed;
                }
                "Speed" => {
                    if self.speed_changed_at != self.elapsed {
                        self.previous_targets[1] = old.ln();
                    }
                    self.speed_changed_at = self.elapsed;
                }
                "Depth" => {
                    if self.depth_changed_at != self.elapsed {
                        self.previous_targets[2] = old;
                    }
                    self.depth_changed_at = self.elapsed;
                }
                _ => {}
            }
        }
        self.tune();
        Ok(())
    }
    fn clear(&mut self) {
        self.lines.fill([0.; 2]);
        self.position = 0;
        self.elapsed = 0;
        self.phase = 0;
        self.speed = 0.2;
        self.depth = 5.;
        self.mix = self.parameters["Mix"] as f32;
        self.mix_changed_at = 0;
        self.speed_changed_at = 0;
        self.depth_changed_at = 0;
        self.speed_target = self.parameters["Speed"].ln();
        self.depth_target = self.parameters["Depth"];
        self.previous_targets = [f64::from(self.mix), self.speed_target, self.depth_target];
        self.low = [0.; 2];
        self.tone = [0.; 2];
    }
    fn oscillator(&self, phase: u32) -> f32 {
        // Native uint32 phase step and 256-point linear sine lookup were
        // independently identified from original ten-second impulse trains.
        let i = (phase >> 24) as usize;
        let fraction = (phase & 0x00ffffff) as f32 / 16777216.;
        self.sine[i] + (self.sine[i + 1] - self.sine[i]) * fraction
    }
    fn read(&self, delay: f32) -> [f32; 2] {
        let whole = delay.floor() as usize;
        let frac = delay - whole as f32;
        let length = self.lines.len();
        let i = (self.position + length - whole) % length;
        std::array::from_fn(|ch| {
            self.lines[i][ch]
                + (self.lines[(i + length - 1) % length][ch] - self.lines[i][ch]) * frac
        })
    }
    fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames.iter().all(|f| f[..2].iter().all(|x| x.is_finite())),
            "Nonfinite WhiteChorus input"
        );
        if self.parameters["Bypass"] != 0. {
            return Ok(());
        }
        let voice_gain = 1. / (self.voices as f32).sqrt();
        for f in frames {
            if self.elapsed != 0 && self.elapsed.is_multiple_of(32) {
                let mix_target = if self.elapsed == self.mix_changed_at {
                    self.previous_targets[0] as f32
                } else {
                    self.parameters["Mix"] as f32
                };
                self.mix += self.mix_smoothing * (mix_target - self.mix);
                // Native connected Speed eases in its logarithmic control
                // domain before the slower oscillator-rate startup clock.
                let speed_target = if self.elapsed == self.speed_changed_at {
                    self.previous_targets[1]
                } else {
                    self.parameters["Speed"].ln()
                };
                self.speed_target +=
                    f64::from(self.mix_smoothing) * (speed_target - self.speed_target);
                let depth_target = if self.elapsed == self.depth_changed_at {
                    self.previous_targets[2]
                } else {
                    self.parameters["Depth"]
                };
                self.depth_target +=
                    f64::from(self.mix_smoothing) * (depth_target - self.depth_target);
            }
            let speed = self.speed as f32;
            let amplitude = self.rate as f32 * (1. - 2f32.powf(-(self.depth as f32) / 1200.))
                / (std::f32::consts::TAU * speed);
            let base = self.rate as f32 / 1000. + amplitude;
            let feedback = self.read(base.floor());
            let input = [f[0], f[1]];
            for ch in 0..2 {
                self.low[ch] += self.poles[0] * (input[ch] - self.low[ch]);
                let high = input[ch] - self.low[ch] - self.gain * feedback[ch];
                self.tone[ch] += self.poles[1] * (high - self.tone[ch]);
            }
            let mut wet = self.tone.map(|x| self.gain * x);
            for i in 0..self.voices {
                let delay =
                    base + amplitude * self.oscillator(self.phase.wrapping_add(self.phases[i]));
                let voice = self.read(delay.max(1.));
                for ch in 0..2 {
                    wet[ch] += voice_gain * voice[ch];
                }
            }
            self.lines[self.position] = self.tone;
            self.position = (self.position + 1) % self.lines.len();
            let dry_gain = (1. - self.mix).sqrt();
            let wet_gain = self.mix.sqrt();
            for ch in 0..2 {
                f[ch] = self.trim
                    * (dry_gain * input[ch]
                        + wet_gain * (wet[ch] + self.parameters["LowGain"] as f32 * self.low[ch]));
            }
            self.phase = self
                .phase
                .wrapping_add((f64::from(speed) / self.rate * 4294967296.) as u32);
            self.speed += self.startup_smoothing * (self.speed_target.exp() - self.speed);
            self.depth += self.startup_smoothing * (self.depth_target - self.depth);
            self.elapsed = self.elapsed.wrapping_add(1);
            ensure!(
                f[..2].iter().all(|x| x.is_finite()),
                "Nonfinite WhiteChorus output"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::program::parse_program;
    use super::*;
    fn effect(attrs: &str) -> ProgramNode {
        parse_program(&format!(
            "<Program><Inserts><DualDelay {attrs}/></Inserts></Program>"
        ))
        .unwrap()
        .nodes
        .remove(2)
    }
    #[test]
    fn uvi_white_chorus_native_voices_and_feedback_network() {
        for (attrs, checkpoints) in [
            (
                "Edge='-1'",
                vec![
                    (0, 0.00065363944),
                    (51, 0.06158148125),
                    (182, 0.10428570211),
                    (193, 0.09079771489),
                    (267, 0.10559577495),
                ],
            ),
            (
                "Edge='0'",
                vec![
                    (0, 0.05951308832),
                    (158, -0.01381656714),
                    (208, -0.01725967042),
                    (316, 0.00266529620),
                ],
            ),
            (
                "Edge='1'",
                vec![
                    (0, 0.22431954741),
                    (158, -0.20085540414),
                    (208, -0.06392639875),
                    (316, 0.17968842387),
                ],
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus {attrs}/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut f = vec![[0.; 12]; 10000];
            f[8192][0] = 0.25;
            fx.process(&mut f).unwrap();
            for (i, value) in checkpoints {
                assert!(
                    (f[8192 + i][0] - value).abs() < 4e-6,
                    "{attrs}/{i}: {} vs {value}",
                    f[8192 + i][0]
                );
            }
            assert!(f.iter().all(|f| f[1] == 0.));
            assert!(fx.memory_bytes() < 40000);
        }
    }
    #[test]
    fn uvi_white_chorus_native_modes_voice_counts_and_levels() {
        // Additional odd-phase voices retain a measured float-rounding
        // residual below2e-5 at these early checkpoints; diagnosed above.
        for (attrs, checkpoints) in [
            (
                "Mode='1'",
                vec![
                    (182, 0.10445420444),
                    (236, 0.07590508461),
                    (254, 0.11643940210),
                    (267, 0.10538197309),
                ],
            ),
            (
                "NumVoices='2'",
                vec![
                    (182, 0.14759220183),
                    (193, 0.12851420045),
                    (316, 0.02582866699),
                ],
            ),
            (
                "NumVoices='8'",
                vec![
                    (51, 0.04364069551),
                    (77, 0.07212063670),
                    (107, 0.07895345241),
                    (236, 0.04814204574),
                    (254, 0.08185440302),
                ],
            ),
            (
                "Trim='-6'",
                vec![
                    (51, 0.03079217486),
                    (182, 0.05223502591),
                    (267, 0.05293305963),
                ],
            ),
            (
                "Mix='0.5'",
                vec![
                    (51, 0.04344356060),
                    (182, 0.07369649410),
                    (267, 0.07468132675),
                ],
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus {attrs}/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut f = vec![[0.; 12]; 10000];
            f[8192][0] = 0.25;
            fx.process(&mut f).unwrap();
            for (i, value) in checkpoints {
                assert!(
                    (f[8192 + i][0] - value).abs() < 2e-5,
                    "{attrs}/{i}: {} vs {value}",
                    f[8192 + i][0]
                );
            }
            assert!(
                fx.set_parameter("NumVoices", &ParameterValue::Number(9.))
                    .is_err()
            );
            assert!(
                fx.set_parameter("SpeedAlias", &ParameterValue::Number(0.2))
                    .is_err()
            );
        }
    }
    #[test]
    fn uvi_white_chorus_native_startup_arrivals() {
        // Arrival windows capture the measured startup law's one-frame
        // precision; the diagnostic retains its calibrated-clock ceiling.
        for (depth, arrivals) in [(1, [50usize, 135, 142, 190]), (40, [58, 615, 650, 987])] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus Depth='{depth}'/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut f = vec![[0.; 12]; 10000];
            f[8192][0] = 0.25;
            fx.process(&mut f).unwrap();
            for arrival in arrivals {
                let peak = f[8192 + arrival - 1..=8192 + arrival + 1]
                    .iter()
                    .map(|f| f[0])
                    .fold(0f32, f32::max);
                assert!(peak > 0.025, "Depth{depth} arrival{arrival}: {peak}");
            }
        }
    }
    #[test]
    fn uvi_dual_delay_native_maximum_time_modulation_capacity() {
        let mut fx = TimeEffect::new(&effect("DelayTime='5' ModDepth='20' ModRate='0.25' Feedback='0.01' Mix='1' DualDelayVersion='1' Interpolation='0'"), 2, 48000.).unwrap();
        let mut f = vec![[0.; 12]; 260000];
        f[8192][0] = 0.25;
        fx.process(&mut f).unwrap();
        // Independent native no-interpolation capture arrives 338 frames
        // beyond nominal 5 s. It isolates retention from the diagnosed detune
        // calibration's fractional-read residual and retains strict gain checks.
        let arrival = 8192 + 240338;
        assert!(f[..arrival].iter().all(|f| f[0] == 0.));
        assert!((f[arrival][0] - 0.00231156731).abs() < 3e-8);
        let integral: f32 = f[arrival..arrival + 8].iter().map(|f| f[0]).sum();
        assert!((integral - 0.00244868966).abs() < 3e-8, "{integral}");
        assert!(f.iter().all(|f| f[..2].iter().all(|x| x.is_finite())));
    }
    #[test]
    fn uvi_time_effect_continuous_mix_target_advances() {
        // A renderer can deliver a fresh target every frame. Before either
        // effect has a wet arrival, dry gain must already move at the first
        // native 32-frame smoothing boundary, including a setter at frame 32.
        for (kind, attributes) in [
            ("DualDelay", "Feedback='0' Mix='0'"),
            ("WhiteChorus", "Edge='-1' LowGain='0' Mix='0' NumVoices='2'"),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><{kind} {attributes}/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut frames = [[0.; 12]; 64];
            for (i, frame) in frames.iter_mut().enumerate() {
                fx.set_parameter("Mix", &ParameterValue::Number(i as f64 / 64.))
                    .unwrap();
                frame[0] = 1.;
                fx.process(std::slice::from_mut(frame)).unwrap();
            }
            assert_eq!(frames[31][0], 1.);
            assert!(frames[32][0] < 0.99, "{kind}: {}", frames[32][0]);
        }
    }
    #[test]
    fn uvi_dual_delay_matches_native_feedback_impulse() {
        let mut fx = TimeEffect::new(
            &effect("DelayTime='0.01' Feedback='0.5' Mix='1'"),
            2,
            48000.,
        )
        .unwrap();
        let mut frames = vec![[0.; 12]; 2000];
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        assert!(frames[..480].iter().all(|f| f[0] == 0.));
        for (i, want) in [
            (480, 0.11557837575674057),
            (481, 0.008129146881401539),
            (960, 0.05334042),
        ] {
            assert!((frames[i][0] - want).abs() < 3e-8, "{i}: {}", frames[i][0]);
        }
        fx.set_parameter("Rotation", &ParameterValue::Number(90.))
            .unwrap();
        // This checkpoint is a stationary XML-initialized rotation. Clear
        // commits the initial control state; live steps are checked below.
        fx.clear();
        frames.fill([0.; 12]);
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        assert!((frames[960][1] - 0.05343345).abs() < 3e-8);

        assert!(
            fx.set_parameter("DelayTime", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
    }
    #[test]
    fn uvi_dual_delay_fraction_mix_ratio_and_tempo() {
        let mut fx = TimeEffect::new(
            &effect("DelayTime='0.01001' Feedback='0.5' Mix='0.25'"),
            2,
            48000.,
        )
        .unwrap();
        let mut f = vec![[0.; 12]; 1000];
        f[0][0] = 0.25;
        fx.process(&mut f).unwrap();
        assert!((f[0][0] - 0.21650635).abs() < 3e-8);
        assert!((f[480][0] - 0.03004975).abs() < 3e-6);
        fx.clear();
        fx.set_parameter("DelayTime", &ParameterValue::Number(0.01))
            .unwrap();
        fx.set_parameter("SyncToHost", &ParameterValue::Boolean(true))
            .unwrap();
        fx.set_tempo(120.).unwrap();
        f.fill([0.; 12]);
        f[0][0] = 0.25;
        fx.process(&mut f).unwrap();
        assert!((f[240][0] - 0.05778919).abs() < 4e-6);
        fx.set_parameter("DelayRatio", &ParameterValue::Number(-0.5))
            .unwrap();
        fx.clear();
        f.fill([0.; 12]);
        f[0][0] = 0.25;
        fx.process(&mut f).unwrap();
        assert!(f[120][0] > 0.057);
    }
    #[test]
    fn uvi_dual_delay_native_cubic_and_modulation() {
        let mut fx = TimeEffect::new(
            &effect("DelayTime='0.01001' Feedback='0.5' Mix='1' Interpolation='2'"),
            2,
            48000.,
        )
        .unwrap();
        let mut f = vec![[0.; 12]; 1000];
        f[0][0] = 0.25;
        fx.process(&mut f).unwrap();
        assert!((f[479][0] + 0.00730821).abs() < 3e-6);
        assert!((f[480][0] - 0.06708589).abs() < 3e-6);
        let mut fx = TimeEffect::new(
            &effect("DelayTime='0.01' Feedback='0.5' Mix='1' ModDepth='1' DualDelayVersion='1'"),
            2,
            48000.,
        )
        .unwrap();
        let mut f = vec![[0.; 12]; 9600];
        f[8192][0] = 0.25;
        f[8192][1] = 0.25;
        fx.process(&mut f).unwrap();
        assert!((f[8674][0] - 0.09523844).abs() < 3e-5);
        assert!((f[8670][1] - 0.09672509).abs() < 3e-5);
        let legacy = TimeEffect::new(&effect("ModDepth='1'"), 2, 48000.).unwrap();
        assert_eq!(
            legacy.parameter("DualDelayVersion").unwrap(),
            ParameterValue::Number(1.)
        );
        assert!(
            (scalar(&legacy.parameter("ModDepth").unwrap()).unwrap() - std::f64::consts::TAU).abs()
                < 1e-6
        );
    }
    #[test]
    fn uvi_dual_delay_mix_native_control_smoothing() {
        let mut fx = TimeEffect::new(&effect("Mix='0' Feedback='0'"), 2, 48000.).unwrap();
        fx.set_parameter("Mix", &ParameterValue::Number(1.))
            .unwrap();
        let mut f = vec![[1.; 12]; 600];
        fx.process(&mut f).unwrap();
        for i in 0..32 {
            assert_eq!(f[i][0], 1.);
        }
        for (i, m) in [
            (32, 0.0712382197f32),
            (128, 0.2559211552),
            (512, 0.6934401393),
        ] {
            assert!((f[i][0] - (1. - m).sqrt()).abs() < 6e-5, "{i}: {}", f[i][0]);
        }
        assert_eq!(f[32][0], f[63][0]);
    }
    #[test]
    fn uvi_dual_delay_native_feedback_and_rotation_steps() {
        // Native host CC127 at frame16384, starting from XML attributes; no
        // initial CC0/Lua setter (that exercises a different init path).
        for (name, target, second) in [
            ("Feedback", 0.6, [0.02043339424f32, 0.]),
            ("Rotation", 90., [0.01894258894, 0.00214823987]),
        ] {
            let mut fx = TimeEffect::new(
                &effect("DelayTime='0.001' Feedback='0.3' Mix='1' DualDelayVersion='1'"),
                2,
                48000.,
            )
            .unwrap();
            fx.process(&mut vec![[0.; 12]; 16384]).unwrap();
            fx.set_parameter(name, &ParameterValue::Number(target))
                .unwrap();
            let mut f = vec![[0.; 12]; 150];
            f[0][0] = 0.25;
            fx.process(&mut f).unwrap();
            assert!(
                (f[48][0] - 0.06934675574).abs() < 3e-8,
                "{name} first: {}",
                f[48][0]
            );
            for ch in 0..2 {
                assert!(
                    (f[96][ch] - second[ch]).abs() < 3e-6,
                    "{name}/{ch}: {}",
                    f[96][ch]
                );
            }
        }
    }
    #[test]
    fn uvi_dual_delay_native_other_sample_rates() {
        for (rate, first, l, r) in [
            (44100., 441, 0.1174308583f32, 0.108233504),
            (96000., 960, 0.091119282, 0.071589701),
        ] {
            let mut fx = TimeEffect::new(
                &effect("DelayTime='0.01' Feedback='0.5' Mix='1' DualDelayVersion='1'"),
                2,
                rate,
            )
            .unwrap();
            let mut f = vec![[0.; 12]; 8192 + 1100];
            f[8192][0] = 0.25;
            fx.process(&mut f).unwrap();
            assert!((f[8192 + first][0] - l).abs() < 3e-7);
            fx.set_parameter("ModDepth", &ParameterValue::Number(1.))
                .unwrap();
            fx.clear();
            f.fill([0.; 12]);
            f[8192][0] = 0.25;
            fx.process(&mut f).unwrap();
            let peak = if rate == 44100. { 443 } else { 963 };
            assert!((f[8192 + peak][0] - r).abs() < 4e-5);
        }
    }
}
