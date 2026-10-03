//! Original time-effect mathematics, characterized using authored PCM impulses
//! against official UVI Workstation 4.0.9 (Windows/Wine), 44.1/48/96 kHz,
//! 2026-10-03.
//! Parameter facts: https://lua.uvi.net/_elements.html and UVI Falcon manual
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf.
//! No vendor source or sample-bank content is included. DualDelay and DualDelayX
//! uses measured RC filters, feedback rotation, fractional time and sqrt mixing.
//! X tape uses measured transposed shelves, normalized tanh and post-delay gain;
//! its unproved diffusion, dispersion, grit and ducking sections are rejected.
//! WhiteChorus uses measured uint32 phase, a 256-entry sine table, RC crossover,
//! and a common Tone feedback line feeding parallel voices. Its Speed/Depth
//! startup clock and DualDelay modulation amplitude remain calibrated models.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI DualDelay/DualDelayX peak EQ uses an RBJ approximation and modulation amplitude is empirically calibrated; DualDelayX tape has measured finite-precision sine residuals up to 2.3e-7, live tape/switch, reflection, filtering and modulation enable transitions are rejected, and scalar clocks outside Mix/Feedback/Rotation/TapeDrive/TapeWarmth remain native-unverified; WhiteChorus Speed/Depth startup is calibrated and phase-offset/read rounding remains approximate, with live NumVoices/Mode/Bypass changes and rates outside 48 kHz native-unverified; legacy DualDelay control smoothing outside Mix/Feedback/Rotation, rates outside 44.1/48/96 kHz and bypass transitions remain native-unverified";
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

const DELAY_X: &[(&str, f64, f64, f64, bool)] = &[
    ("TapeSaturation", 0., 0., 1., true),
    ("TapeDrive", 0., 0., 1., false),
    ("TapeWarmth", 0.5, 0., 1., false),
    ("Reflection", 0., 0., 1., true),
    ("PeakCompensation", 1., 0., 1., false),
    ("Modulation", 1., 0., 1., true),
    ("DispersionSpread", 20., 1., 100., false),
    ("DispersionFreq", 200., 0., 20000., false),
    ("Dispersion", 0., 0., 1., true),
    ("DiffusionSpread", 20., 1., 100., false),
    ("DiffusionAmount", 0.2, 0., 1., false),
    ("Diffusion", 0., 0., 1., true),
    ("CrusherFreq", 9., 0., 9., true),
    ("CrusherBits", 16., 4., 16., true),
    ("Crusher", 0., 0., 1., true),
    ("Filtering", 1., 0., 1., true),
    ("DuckerThreshold", -10., -100., 0., false),
    ("DuckerAttack", 1., 1., 100., false),
    ("DuckerHold", 1., 1., 1000., false),
    ("DuckerDecay", 200., 10., 1000., false),
    ("DuckerAttenuation", 20., 0., 50., false),
    ("DuckerBypass", 1., 0., 1., true),
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
    matches!(kind, "DualDelay" | "DualDelayX" | "WhiteChorus")
}

fn controls(kind: &str) -> impl Iterator<Item = &'static (&'static str, f64, f64, f64, bool)> + '_ {
    let base = if kind == "WhiteChorus" { CHORUS } else { DELAY };
    let extra = if kind == "DualDelayX" { DELAY_X } else { &[] };
    base.iter()
        .filter(move |p| kind != "DualDelayX" || p.0 != "DualDelayVersion")
        .chain(extra.iter())
}

fn validate_delay_x(p: &BTreeMap<String, f64>) -> Result<()> {
    for name in ["Dispersion", "Diffusion", "Crusher"] {
        ensure!(p[name] == 0., "Unsupported active DualDelayX {name}");
    }
    ensure!(
        p["DuckerBypass"] == 1.,
        "Unsupported active DualDelayX ducking"
    );
    Ok(())
}

fn scalar(value: &ParameterValue) -> Result<f64> {
    match value {
        ParameterValue::Number(v) if v.is_finite() => Ok(*v),
        ParameterValue::Boolean(v) => Ok(f64::from(u8::from(*v))),
        _ => bail!("UVI time effect requires a finite numeric parameter"),
    }
}

fn checked(kind: &str, name: &str, value: &ParameterValue) -> Result<f64> {
    let (_, _, low, high, integer) = controls(kind)
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
    let mut p: BTreeMap<String, f64> = controls(kind).map(|&(n, v, ..)| (n.into(), v)).collect();
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
    if kind == "DualDelayX" {
        validate_delay_x(&p)?;
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
    fn tune(&mut self, frequency: f64, gain: f64, q: f64, compensation: f64, rate: f64) {
        let w = std::f64::consts::TAU * frequency.min(rate * 0.49) / rate;
        let a = 10f64.powf(gain / 40.);
        let alpha = w.sin() / (2. * q);
        let a0 = 1. + alpha / a;
        // Native feedback EQ normalizes boosts by the maximum band gain.
        let scale = 10f64.powf(-gain.max(0.) * compensation / 20.);
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
            "DualDelay" | "DualDelayX" => {
                TimeProcessor::Delay(Box::new(DualDelay::new(node, channels, rate)?))
            }
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
    /// Signal targets have a separate native Mix domain from script setters:
    /// smooth the finite raw target, then clamp the audible mixing gains.
    pub fn set_effective(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        if name == "Mix"
            && let TimeProcessor::Delay(p) = &mut self.0
        {
            let target = scalar(value)? as f32;
            ensure!(target.is_finite(), "Nonfinite UVI delay Mix target");
            p.set_mix_target(target);
            return Ok(());
        }
        self.set_parameter(name, value)
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

// X stores normalized saturated samples. The inverse shelf and compensation
// belong after the raw delay: live Drive/Warmth probes distinguish this from
// a stationary-equivalent arrangement with both shelves in the writer.
struct Tape {
    controls: [f32; 2],
    targets: [f32; 2],
    previous_targets: [f32; 2],
    changed_at: [u64; 2],
    drive_gain: f32,
    output_gain: f32,
    pre: [f32; 3],
    post: [f32; 3],
    pre_state: [f32; 2],
    post_state: [f32; 2],
}
impl Tape {
    fn new(drive: f32, warmth: f32, rate: f64) -> Self {
        let mut result = Self {
            controls: [drive, warmth],
            targets: [drive, warmth],
            previous_targets: [drive, warmth],
            changed_at: [0; 2],
            drive_gain: 1.,
            output_gain: 1.,
            pre: [0.; 3],
            post: [0.; 3],
            pre_state: [0.; 2],
            post_state: [0.; 2],
        };
        result.tune(rate);
        result
    }
    fn tune(&mut self, rate: f64) {
        self.drive_gain = 10f64.powf(1.5 * f64::from(self.controls[0])) as f32;
        let a = f64::from(self.drive_gain) * 0.3f64.sqrt();
        self.output_gain = (0.1f64.sqrt() / (1. - a.tanh() / a).sqrt()) as f32;
        let gain = 10f64.powf(f64::from(self.controls[1]));
        let c = (std::f64::consts::PI * 2500. / rate).tan() * gain.sqrt();
        let b0 = ((gain + c) / (1. + c)) as f32;
        let b1 = ((-gain + c) / (1. + c)) as f32;
        let a1 = ((-1. + c) / (1. + c)) as f32;
        self.pre = [b0, b1, a1];
        self.post = [1. / b0, a1 / b0, b1 / b0];
    }
    fn set_target(&mut self, i: usize, value: f32, elapsed: u64) {
        if value != self.targets[i] {
            if self.changed_at[i] != elapsed {
                self.previous_targets[i] = self.targets[i];
            }
            self.targets[i] = value;
            self.changed_at[i] = elapsed;
        }
    }
    fn advance(&mut self, elapsed: u64, alpha: f32, rate: f64) {
        for i in 0..2 {
            let target = if elapsed == self.changed_at[i] {
                self.previous_targets[i]
            } else {
                self.targets[i]
            };
            self.controls[i] += alpha * (target - self.controls[i]);
            if elapsed.is_multiple_of(256) && (alpha * (target - self.controls[i])).abs() < 1e-6 {
                self.controls[i] = target;
            }
        }
        self.tune(rate);
    }
    fn encode(&mut self, x: f32, ch: usize) -> f32 {
        let [b0, b1, a1] = self.pre;
        let y = b0 * x + self.pre_state[ch];
        self.pre_state[ch] = b1 * x - a1 * y;
        y.tanh()
    }
    fn decode(&mut self, x: [f32; 2]) -> [f32; 2] {
        let [b0, b1, a1] = self.post;
        std::array::from_fn(|ch| {
            let y = b0 * x[ch] + self.post_state[ch];
            self.post_state[ch] = b1 * x[ch] - a1 * y;
            y
        })
    }
}

struct DualDelay {
    is_x: bool,
    tape: Option<Tape>,
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
    mix_target: f32,
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
        let is_x = node.kind == "DualDelayX";
        ensure!(
            !is_x || [44100., 48000., 96000.].contains(&rate),
            "DualDelayX sample rate is native-unverified"
        );
        let tape = (is_x && parameters["TapeSaturation"] != 0.).then(|| {
            Tape::new(
                parameters["TapeDrive"] as f32,
                parameters["TapeWarmth"] as f32,
                rate,
            )
        });
        let mix = parameters["Mix"] as f32;
        // Native maximum-time modulation can read beyond five seconds. Retain
        // the full published Depth20/Rate.1 detune envelope to avoid overwrite
        // and a wrapped-index underflow, while keeping preparation bounded.
        let max_detune_seconds = MODULATION_DETUNE * 20. / (std::f64::consts::TAU * 0.1);
        let length = ((MAX_SECONDS + max_detune_seconds) * rate).ceil() as usize + 2;
        ensure!(length * 8 <= MEMORY_LIMIT, "UVI delay exceeds memory bound");
        let mut result = Self {
            is_x,
            tape,
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
            mix_target: mix,
            mix_gains: [(1. - mix).sqrt(), mix.sqrt()],
            mix_smoothing: 1. - 0.33f32.powf(3200. / rate as f32),
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
        if self.is_x {
            validate_delay_x(p)?;
        }
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
        let detune = if self.is_x && p["Modulation"] == 0. {
            0.
        } else {
            self.rate * MODULATION_DETUNE * p["ModDepth"] / (std::f64::consts::TAU * p["ModRate"])
        };
        self.modulation = self
            .frames
            .map(|n| detune.min(f64::from(n) * 0.005 * p["ModDepth"]) as f32);
        self.input_rotation = rotation(p["InputRotation"] * std::f64::consts::FRAC_PI_4);
        self.output_rotation = rotation(p["OutputRotation"] * std::f64::consts::FRAC_PI_4);
        self.feedback_rotation = rotation(f64::from(self.rotation) * std::f64::consts::PI / 180.);
        self.poles = [p["HighCut"], p["LowCut"]]
            .map(|f| 1. - (-std::f32::consts::TAU * f as f32 / self.rate as f32).exp());
        self.peak.tune(
            p["PeakFreq"],
            p["PeakGain"],
            p["PeakQ"],
            if self.is_x { p["PeakCompensation"] } else { 1. },
            self.rate,
        );
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
        let kind = if self.is_x { "DualDelayX" } else { "DualDelay" };
        let value = checked(kind, name, value)?;
        if self.is_x
            && matches!(
                name,
                "TapeSaturation" | "Reflection" | "Filtering" | "Modulation"
            )
        {
            ensure!(
                self.parameters[name] == value,
                "DualDelayX {name} live transition is native-unverified"
            );
        }
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
        if name == "Mix" {
            self.set_mix_target(value as f32);
        } else if let Some(tape) = &mut self.tape
            && matches!(name, "TapeDrive" | "TapeWarmth")
        {
            tape.set_target(
                usize::from(name == "TapeWarmth"),
                value as f32,
                self.elapsed,
            );
        } else if value != old
            && let Some(control) = match name {
                "Feedback" | "FeedbackRatio" => Some(1),
                "Rotation" => Some(2),
                _ => None,
            }
        {
            if self.control_changed_at[control] != self.elapsed {
                match control {
                    1 => self.previous_targets[1..3].copy_from_slice(&old_feedback),
                    2 => self.previous_targets[3] = old as f32,
                    _ => unreachable!(),
                }
            }
            self.control_changed_at[control] = self.elapsed;
        }
        Ok(())
    }
    fn set_mix_target(&mut self, target: f32) {
        if target != self.mix_target {
            if self.control_changed_at[0] != self.elapsed {
                self.previous_targets[0] = self.mix_target;
            }
            self.control_changed_at[0] = self.elapsed;
            self.mix_target = target;
        }
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
        self.mix_target = self.mix;
        self.mix_gains = [(1. - self.mix).sqrt(), self.mix.sqrt()];
        self.low = [0.; 2];
        self.high = [0.; 2];
        self.peak.z = [[0.; 2]; 2];
        if let Some(tape) = &mut self.tape {
            *tape = Tape::new(
                self.parameters["TapeDrive"] as f32,
                self.parameters["TapeWarmth"] as f32,
                self.rate,
            );
        }
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
                    self.mix_target
                };
                let next_mix = self.mix + self.mix_smoothing * (mix_target - self.mix);
                ensure!(next_mix.is_finite(), "Nonfinite UVI delay Mix control");
                self.mix = next_mix;
                // Native Mix snaps only at the host's 256-frame boundary,
                // after the 32-frame update, using its own measured epsilon.
                if self.elapsed.is_multiple_of(256)
                    && (self.mix_smoothing * (mix_target - self.mix)).abs()
                        < if self.is_x { 1e-6 } else { 1e-7 }
                {
                    self.mix = mix_target;
                }
                let audible_mix = self.mix.clamp(0., 1.);
                self.mix_gains = [(1. - audible_mix).sqrt(), audible_mix.sqrt()];
                for ch in 0..2 {
                    let target = if self.elapsed == self.control_changed_at[1] {
                        self.previous_targets[1 + ch]
                    } else {
                        self.feedback_target[ch]
                    };
                    self.feedback[ch] += self.mix_smoothing * (target - self.feedback[ch]);
                    if self.is_x
                        && self.elapsed.is_multiple_of(256)
                        && (self.mix_smoothing * (target - self.feedback[ch])).abs() < 1e-6
                    {
                        self.feedback[ch] = target;
                    }
                }
                let rotation_target = if self.elapsed == self.control_changed_at[2] {
                    self.previous_targets[3]
                } else {
                    self.parameters["Rotation"] as f32
                };
                self.rotation += self.mix_smoothing * (rotation_target - self.rotation);
                self.feedback_rotation =
                    rotation(f64::from(self.rotation) * std::f64::consts::PI / 180.);
                if let Some(tape) = &mut self.tape {
                    tape.advance(self.elapsed, self.mix_smoothing, self.rate);
                }
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
            if let Some(tape) = &mut self.tape {
                delayed = tape.decode(delayed);
            }
            let back = if self.is_x && self.parameters["Reflection"] != 0. {
                let [c, s] = self.feedback_rotation;
                [
                    s * delayed[0] + c * delayed[1],
                    c * delayed[0] - s * delayed[1],
                ]
            } else {
                rotate(delayed, self.feedback_rotation)
            };
            let mut source = rotate(
                width(input, self.parameters["InputWidth"] as f32),
                self.input_rotation,
            );
            if let Some(tape) = &self.tape {
                source = source.map(|x| x * tape.drive_gain);
            }
            let mut next = [0.; 2];
            for ch in 0..2 {
                // Legacy scales the input before filter state; X scales the
                // filtered/saturated writer. Live steps distinguish the order.
                let x = (source[ch] + back[ch]) * if self.is_x { 1. } else { self.feedback[ch] };
                let mut y = x;
                if !self.is_x || self.parameters["Filtering"] != 0. {
                    self.low[ch] += self.poles[0] * (x - self.low[ch]);
                    self.high[ch] += self.poles[1] * (self.low[ch] - self.high[ch]);
                    y = self.low[ch] - self.high[ch];
                    if self.parameters["PeakGain"] != 0. {
                        y = self.peak.step(y, ch);
                    }
                }
                if let Some(tape) = &mut self.tape {
                    y = tape.encode(y, ch);
                }
                next[ch] = y * if self.is_x { self.feedback[ch] } else { 1. };
            }
            self.lines[self.position] = next;
            self.position = (self.position + 1) % self.lines.len();
            self.elapsed = self.elapsed.wrapping_add(1);
            if let Some(tape) = &self.tape {
                delayed = delayed.map(|x| x * tape.output_gain);
            }
            // Native applies output rotation before width; these operations
            // do not commute when both controls differ from their defaults.
            let out = width(
                rotate(delayed, self.output_rotation),
                self.parameters["OutputWidth"] as f32,
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
    phases: [[u32; 2]; 8],
    sine: [f32; 257],
    oscillator_endpoints: [[[f32; 2]; 2]; 8],
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
    previous_targets: [f64; 8],
    controls: [f64; 5],
    controls_changed_at: [u64; 5],
    low_gain: f32,
    low: [f32; 2],
    tone: [f32; 2],
    poles: [f32; 2],
    gain: f32,
    trim: f32,
    voices: usize,
}
impl WhiteChorus {
    const CLOCK_PARAMETERS: [&str; 5] = ["Crossover", "Tone", "Edge", "LowGain", "Trim"];
    fn control_targets(parameters: &BTreeMap<String, f64>) -> [f64; 5] {
        std::array::from_fn(|i| {
            let value = parameters[Self::CLOCK_PARAMETERS[i]];
            if i < 2 { value.ln() } else { value }
        })
    }
    fn new(node: &ProgramNode, rate: f64) -> Result<Self> {
        let parameters = parameters(node)?;
        let mix = parameters["Mix"] as f32;
        let speed_target = parameters["Speed"].ln();
        let depth_target = parameters["Depth"];
        let controls = Self::control_targets(&parameters);
        // Depth40/Speed.1 reaches less than 75ms, including all voice extrema.
        let length = (rate * 0.1).ceil() as usize + 2;
        let mut result = Self {
            rate,
            parameters,
            lines: vec![[0.; 2]; length],
            position: 0,
            elapsed: 0,
            phase: 0,
            phases: [[0; 2]; 8],
            sine: std::array::from_fn(|i| {
                if i == 256 {
                    0.
                } else {
                    (std::f64::consts::TAU * i as f64 / 256.).sin() as f32
                }
            }),
            oscillator_endpoints: [[[0.; 2]; 2]; 8],
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
            mix_smoothing: 1. - 0.33f32.powf(3200. / rate as f32),
            mix_changed_at: 0,
            previous_targets: [
                f64::from(mix),
                speed_target,
                depth_target,
                controls[0],
                controls[1],
                controls[2],
                controls[3],
                controls[4],
            ],
            controls,
            controls_changed_at: [0; 5],
            low_gain: 1.,
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
        let edge = self.controls[2];
        self.gain = if edge <= 0. {
            ((1. + edge) / self.voices as f64) as f32
        } else {
            (1. / self.voices as f64 + (0.95 - 1. / self.voices as f64) * edge) as f32
        };
        self.poles = [self.controls[0].exp(), self.controls[1].exp()]
            .map(|f| 1. - (-std::f32::consts::TAU * f as f32 / self.rate as f32).exp());
        self.low_gain = self.controls[3] as f32;
        self.trim = 10f64.powf(self.controls[4] / 20.) as f32;
        for (i, phase) in self.phases.iter_mut().enumerate() {
            let denominator = if p["Mode"] == 0. {
                self.voices
            } else {
                2 * self.voices
            };
            *phase = std::array::from_fn(|ch| {
                // Native stereo Mode0 interleaves half-offset banks; Mode1
                // assigns each channel half of a bank twice the voice count.
                let index = i as f64
                    + if ch == 0 {
                        0.
                    } else if p["Mode"] == 0. {
                        0.5
                    } else {
                        self.voices as f64
                    };
                ((2f64.powf(index / denominator as f64) - 1.) * 4294967296.) as u32
            });
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
                _ => {
                    if let Some(i) = Self::CLOCK_PARAMETERS.iter().position(|&p| p == name) {
                        if self.controls_changed_at[i] != self.elapsed {
                            self.previous_targets[i + 3] = if i < 2 { old.ln() } else { old };
                        }
                        self.controls_changed_at[i] = self.elapsed;
                    }
                }
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
        self.oscillator_endpoints = [[[0.; 2]; 2]; 8];
        self.speed = 0.2;
        self.depth = 5.;
        self.mix = self.parameters["Mix"] as f32;
        self.mix_changed_at = 0;
        self.speed_changed_at = 0;
        self.depth_changed_at = 0;
        self.speed_target = self.parameters["Speed"].ln();
        self.depth_target = self.parameters["Depth"];
        self.previous_targets[..3].copy_from_slice(&[
            f64::from(self.mix),
            self.speed_target,
            self.depth_target,
        ]);
        self.controls = Self::control_targets(&self.parameters);
        self.controls_changed_at = [0; 5];
        self.previous_targets[3..].copy_from_slice(&self.controls);
        self.low = [0.; 2];
        self.tone = [0.; 2];
        self.tune();
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
                if self.elapsed.is_multiple_of(256)
                    && (self.mix_smoothing * (mix_target - self.mix)).abs() < 1e-7
                {
                    self.mix = mix_target;
                }
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
                let targets = Self::control_targets(&self.parameters);
                for (i, target) in targets.into_iter().enumerate() {
                    let target = if self.elapsed == self.controls_changed_at[i] {
                        self.previous_targets[i + 3]
                    } else {
                        target
                    };
                    self.controls[i] += f64::from(self.mix_smoothing) * (target - self.controls[i]);
                }
                self.tune();
            }
            let speed = self.speed as f32;
            let phase_step = (f64::from(speed) / self.rate * 4294967296.) as u32;
            if self.elapsed.is_multiple_of(64) {
                // Native interpolates sine-table endpoints every64 frames,
                // including across the lookup table's slope knots.
                let next = self.phase.wrapping_add(phase_step.wrapping_mul(64));
                self.oscillator_endpoints = std::array::from_fn(|i| {
                    std::array::from_fn(|ch| {
                        [
                            self.oscillator(self.phase.wrapping_add(self.phases[i][ch])),
                            self.oscillator(next.wrapping_add(self.phases[i][ch])),
                        ]
                    })
                });
            }
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
                for ch in 0..2 {
                    let [start, end] = self.oscillator_endpoints[i][ch];
                    let oscillator = start + (end - start) * (self.elapsed % 64) as f32 / 64.;
                    let delay = base + amplitude * oscillator;
                    wet[ch] += voice_gain * self.read(delay.max(1.))[ch];
                }
            }
            self.lines[self.position] = self.tone;
            self.position = (self.position + 1) % self.lines.len();
            let dry_gain = (1. - self.mix).sqrt();
            let wet_gain = self.mix.sqrt();
            for ch in 0..2 {
                f[ch] = self.trim
                    * (dry_gain * input[ch] + wet_gain * (wet[ch] + self.low_gain * self.low[ch]));
            }
            self.phase = self.phase.wrapping_add(phase_step);
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
    fn delay_x(attrs: &str) -> ProgramNode {
        let mut node = effect(attrs);
        node.kind = "DualDelayX".into();
        node
    }
    #[test]
    fn uvi_dual_delay_x_typed_controls_and_unproved_sections() {
        let node = delay_x("");
        let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
        assert_eq!(controls("DualDelayX").count(), 43);
        assert_eq!(
            fx.parameter("TapeWarmth").unwrap(),
            ParameterValue::Number(0.5)
        );
        assert!(fx.parameter("DualDelayVersion").is_err());
        assert!(validate(&delay_x("DualDelayVersion='1'")).is_err());
        for attrs in [
            "Diffusion='1'",
            "Dispersion='1'",
            "Crusher='1'",
            "DuckerBypass='0'",
        ] {
            assert!(validate(&delay_x(attrs)).is_err(), "{attrs}");
            assert!(TimeEffect::new(&delay_x(attrs), 2, 48000.).is_err());
        }
        assert!(
            fx.set_parameter("Diffusion", &ParameterValue::Boolean(true))
                .is_err()
        );
        assert_eq!(
            fx.parameter("Diffusion").unwrap(),
            ParameterValue::Number(0.)
        );
        assert!(
            fx.set_parameter("TapeDrive", &ParameterValue::Number(1.01))
                .is_err()
        );
        assert!(
            fx.set_parameter("TapeSaturation", &ParameterValue::Boolean(true))
                .is_err()
        );
        assert!(TimeEffect::new(&node, 2, 32000.).is_err());
    }
    #[test]
    fn uvi_dual_delay_x_native_reflection_and_filter_gates() {
        let mut fx = TimeEffect::new(
            &delay_x(
                "DelayTime='.001' Feedback='.5' Mix='1' Filtering='0' Reflection='1' Rotation='45'",
            ),
            2,
            48000.,
        )
        .unwrap();
        let mut frames = vec![[0.; 12]; 1000];
        frames[0][1] = 0.25;
        fx.process(&mut frames).unwrap();
        for (i, expected) in [
            (48, [0., 0.1249995083]),
            (96, [0.04419383034, -0.04419383407]),
            (144, [0., 0.03124964051]),
        ] {
            for ch in 0..2 {
                assert!(
                    (frames[i][ch] - expected[ch]).abs() < 3e-8,
                    "{i}/{ch}: {}",
                    frames[i][ch]
                );
            }
        }
        let mut fx = TimeEffect::new(
            &delay_x("DelayTime='.01' Feedback='.5' Mix='1' Filtering='0' PeakGain='6'"),
            2,
            48000.,
        )
        .unwrap();
        let mut frames = vec![[0.; 12]; 700];
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        assert!((frames[480][0] - 0.1249999851).abs() < 3e-8);
        assert!(frames[481..488].iter().all(|f| f[0] == 0.));
        let mut fx = TimeEffect::new(
            &delay_x(
                "DelayTime='.01' Feedback='.5' Mix='1' ModDepth='4' ModRate='.3' Modulation='0'",
            ),
            2,
            48000.,
        )
        .unwrap();
        let mut frames = vec![[0.; 12]; 700];
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        assert!((frames[480][0] - 0.1155783758).abs() < 3e-8);
    }
    #[test]
    fn uvi_dual_delay_x_native_tape_static() {
        for (drive, warmth, samples) in [
            (0., 0., [0.1295884699, 0., 0.]),
            (0., 0.5, [0.1155816615, -0.003736798884, -0.002602618420]),
            (0.2, 0.5, [0.09910318255, -0.01154534239, -0.008080410771]),
            (0.5, 0.5, [0.07186803222, -0.03689695895, -0.02698104829]),
            (1., 0.5, [0.06097087264, -0.06373913586, -0.07856545597]),
        ] {
            let node = delay_x(&format!(
                "DelayTime='.001' Feedback='.5' Mix='1' Filtering='0' TapeSaturation='1' TapeDrive='{drive}' TapeWarmth='{warmth}'"
            ));
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 1000];
            frames[0][0] = 0.25;
            fx.process(&mut frames).unwrap();
            for (i, expected) in [48, 50, 52].into_iter().zip(samples) {
                assert!(
                    (frames[i][0] - expected).abs() < 3e-8,
                    "Drive{drive}/Warmth{warmth}/{i}: {} vs {expected}",
                    frames[i][0]
                );
            }
        }
    }
    #[test]
    fn uvi_dual_delay_x_native_tape_cross_rate() {
        for (rate, drive, warmth, i, expected) in [
            (44100., 0.2, 0.5, 45, -0.002058978425),
            (44100., 0.5, 1., 46, -0.02649354935),
            (96000., 0.2, 0.5, 96, 0.09421595931),
            (96000., 0.5, 1., 212, -0.007202530280),
        ] {
            let node = delay_x(&format!(
                "DelayTime='.001' Feedback='.5' Mix='1' Filtering='0' TapeSaturation='1' TapeDrive='{drive}' TapeWarmth='{warmth}'"
            ));
            let mut fx = TimeEffect::new(&node, 2, rate).unwrap();
            let mut frames = vec![[0.; 12]; 1000];
            frames[0][0] = 0.25;
            fx.process(&mut frames).unwrap();
            assert!(
                (frames[i][0] - expected).abs() < 3e-8,
                "{rate}/{i}: {} vs {expected}",
                frames[i][0]
            );
        }
    }
    #[test]
    fn uvi_dual_delay_x_native_tape_live_controls() {
        for (name, target, samples) in [
            (
                "TapeDrive",
                0.5,
                [
                    0.002694309456,
                    0.002738363342,
                    0.003171588993,
                    0.003356239991,
                    0.003415464424,
                ],
            ),
            (
                "TapeWarmth",
                0.8,
                [
                    0.002786492463,
                    0.002918473911,
                    0.002823625226,
                    0.002828329569,
                    0.002829542384,
                ],
            ),
        ] {
            let node = delay_x(
                "DelayTime='.001' Feedback='.01' Mix='1' Filtering='0' TapeSaturation='1' TapeDrive='.2' TapeWarmth='.5'",
            );
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 16384];
            for frame in &mut frames {
                frame[0] = 0.25;
            }
            fx.process(&mut frames).unwrap();
            fx.set_parameter(name, &ParameterValue::Number(target))
                .unwrap();
            let mut frames = vec![[0.; 12]; 4700];
            for frame in &mut frames {
                frame[0] = 0.25;
            }
            fx.process(&mut frames).unwrap();
            for (i, expected) in [32, 80, 512, 1024, 4352].into_iter().zip(samples) {
                assert!(
                    (frames[i][0] - expected).abs() < 1e-8,
                    "{name}/{i}: {} vs {expected}",
                    frames[i][0]
                );
            }
        }
    }
    #[test]
    fn uvi_dual_delay_x_native_feedback_order_and_mix_snap() {
        let mut fx = TimeEffect::new(
            &delay_x("DelayTime='.001' Feedback='.3' Mix='1'"),
            2,
            48000.,
        )
        .unwrap();
        let mut frames = vec![[0.; 12]; 16384];
        for i in [4096, 8192, 12288] {
            frames[i][0] = 0.25;
        }
        fx.process(&mut frames).unwrap();
        fx.set_parameter("Feedback", &ParameterValue::Number(0.6))
            .unwrap();
        let mut frames = vec![[0.; 12]; 600];
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        // X gain follows the filter state. Legacy's superficially equivalent
        // stationary ordering produces .02043339424 at96 instead.
        for (i, expected) in [
            (96, 0.02042107284),
            (128, -0.0003028174688),
            (256, -0.0003457333660),
            (512, -0.0001492632728),
        ] {
            assert!(
                (frames[i][0] - expected).abs() < 3e-8,
                "{i}: {} vs {expected}",
                frames[i][0]
            );
        }
        let mut fx = TimeEffect::new(&delay_x("Feedback='0' Mix='0'"), 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 16384];
        for frame in &mut frames {
            frame[0] = 0.25;
        }
        fx.process(&mut frames).unwrap();
        fx.set_parameter("Mix", &ParameterValue::Number(1.))
            .unwrap();
        let mut frames = vec![[0.; 12]; 6000];
        for frame in &mut frames {
            frame[0] = 0.25;
        }
        fx.process(&mut frames).unwrap();
        assert!((frames[4608][0] - 0.001220703125).abs() < 3e-8);
        assert_eq!(frames[4863][0], frames[4832][0]);
        assert_eq!(frames[4864][0], 0.);
    }
    #[test]
    fn uvi_dual_delay_x_native_tape_independent_routing_and_sine() {
        let node = delay_x(
            "DelayTime='.001' Feedback='.4' Mix='.7' LowCut='230' HighCut='7300' Rotation='73' InputWidth='.37' OutputWidth='.61' InputRotation='-.7' OutputRotation='.8' DelayRatio='.33' FeedbackRatio='-.21' TapeSaturation='1' TapeDrive='.2' TapeWarmth='.5'",
        );
        let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 1000];
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        for (i, expected) in [
            (32, [0.001433747122, -0.002439395757]),
            (48, [0.02458427660, 0.02038822696]),
            (80, [-0.001876554452, 0.003431939753]),
            (96, [0.001156237558, 0.0004797187285]),
            (256, [-0.000001415771749, -0.000003861824098]),
        ] {
            for ch in 0..2 {
                assert!(
                    (frames[i][ch] - expected[ch]).abs() < 3e-8,
                    "{i}/{ch}: {} vs {}",
                    frames[i][ch],
                    expected[ch]
                );
            }
        }
        let node = delay_x(
            "DelayTime='.001' Feedback='.72' Mix='1' LowCut='342.28' HighCut='9068.97' TapeSaturation='1' TapeDrive='.172' TapeWarmth='.58'",
        );
        let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 40001];
        for (i, frame) in frames.iter_mut().enumerate() {
            // Same original 16-bit quantized 1 kHz source as the native probe.
            frame[0] = (8192. * (std::f64::consts::TAU * 1000. * i as f64 / 48000.).sin()).round()
                as f32
                / 32768.;
            frame[0] *= 0.99999988;
        }
        fx.process(&mut frames).unwrap();
        // Full independent native sine render measured max2.24e-7, unlike
        // sparse impulses; this precision ceiling is explicitly diagnosed.
        for (i, expected) in [
            (1000, -0.1672999561),
            (8192, -0.2997734249),
            (16384, 0.1672971100),
            (24576, 0.1794879735),
            (40000, 0.1672971100),
        ] {
            assert!(
                (frames[i][0] - expected).abs() < 3e-7,
                "{i}: {} vs {expected}",
                frames[i][0]
            );
        }
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
            assert!(fx.memory_bytes() < 41000);
        }
    }
    #[test]
    fn uvi_white_chorus_native_modes_voice_counts_and_levels() {
        // Additional voice banks retain a measured float-rounding
        // residual below 6e-6 at these early checkpoints; diagnosed above.
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
                    (f[8192 + i][0] - value).abs() < 6e-6,
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
    fn uvi_white_chorus_native_six_and_eight_voice_mode_spacing() {
        // Native reached Starter configurations: Mode1 uses half of a bank
        // twice the selected voice count, rather than a fixed eight-voice bank.
        for (voices, mode, checkpoints) in [
            (
                6,
                0,
                [
                    (57, 0.05907764658),
                    (82, 0.07886858284),
                    (250, 0.06871520728),
                ],
            ),
            (
                6,
                1,
                [
                    (220, 0.07728394866),
                    (238, 0.04999915138),
                    (264, 0.08675110340),
                ],
            ),
            (
                8,
                1,
                [
                    (211, 0.06661188602),
                    (229, 0.06103919819),
                    (267, 0.13516837358),
                ],
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus NumVoices='{voices}' Mode='{mode}' Edge='-1' LowGain='0'/></Inserts></Program>"
            )).unwrap().nodes.remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 10000];
            frames[8192][0] = 0.25;
            fx.process(&mut frames).unwrap();
            for (delay, expected) in checkpoints {
                assert!(
                    (frames[8192 + delay][0] - expected).abs() < 6e-6,
                    "voices{voices}/mode{mode}/{delay}: {} vs {expected}",
                    frames[8192 + delay][0]
                );
            }
        }
    }
    #[test]
    fn uvi_white_chorus_native_stereo_phase_banks() {
        // Identical authored stereo impulses independently identify each
        // channel's bank. The measured early read-rounding residual is <4e-6.
        for (voices, mode, checkpoints) in [
            (
                4,
                0,
                [
                    (77, 0.10163776577),
                    (107, 0.11132773012),
                    (254, 0.11599827558),
                ],
            ),
            (
                6,
                0,
                [
                    (51, 0.04981407523),
                    (220, 0.07694233954),
                    (267, 0.08553870022),
                ],
            ),
            (
                8,
                0,
                [
                    (52, 0.04956490919),
                    (124, 0.08080752939),
                    (256, 0.08101453632),
                ],
            ),
            (
                6,
                1,
                [
                    (57, 0.05881531909),
                    (107, 0.05063379183),
                    (193, 0.07324445248),
                ],
            ),
            (
                8,
                1,
                [
                    (71, 0.07002160698),
                    (124, 0.08019130677),
                    (193, 0.06306712329),
                ],
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus NumVoices='{voices}' Mode='{mode}' Edge='-1' LowGain='0'/></Inserts></Program>"
            )).unwrap().nodes.remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 10000];
            frames[8192][..2].fill(0.25);
            fx.process(&mut frames).unwrap();
            for (delay, expected) in checkpoints {
                assert!(
                    (frames[8192 + delay][1] - expected).abs() < 4e-6,
                    "voices{voices}/mode{mode}/right{delay}: {} vs {expected}",
                    frames[8192 + delay][1]
                );
            }
            assert!(frames.iter().any(|f| (f[0] - f[1]).abs() > 0.05));
        }
    }
    #[test]
    fn uvi_white_chorus_native_long_oscillator_clock() {
        // Independent authored native impulse trains distinguish the 64-frame
        // sine-endpoint clock from per-frame or 16/32/128-frame alternatives.
        // The 1e-5 ceiling covers measured long-train read-rounding residuals
        // below8e-6; the diagnostic deliberately retains that precision limit.
        for (voices, mode, checkpoints) in [
            (4, 0, [(135437, 0.03740978241), (299119, 0.01225477085)]),
            (6, 1, [(213114, 0.05583680421), (446698, 0.08409113437)]),
            (8, 1, [(110640, 0.07209462672), (397437, 0.00357553712)]),
        ] {
            let node=parse_program(&format!("<Program><Inserts><WhiteChorus NumVoices='{voices}' Mode='{mode}' Edge='-1' LowGain='0'/></Inserts></Program>")).unwrap().nodes.remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let count = checkpoints.iter().map(|(i, _)| *i).max().unwrap() + 1;
            let mut frames = vec![[0.; 12]; count];
            for i in (4096..count).step_by(4096) {
                frames[i][0] = 0.25;
            }
            fx.process(&mut frames).unwrap();
            for (i, value) in checkpoints {
                assert!(
                    (frames[i][0] - value).abs() < 1e-5,
                    "voices{voices}/mode{mode}/{i}: {} vs {value}",
                    frames[i][0]
                );
            }
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
    fn uvi_white_chorus_native_direct_control_clocks() {
        // Original Lua setters at host frame16384; the first wet voice arrives
        // at48, so these earlier samples identify each consumer's control law.
        for (name, start, target, checkpoints) in [
            (
                "Crossover",
                20.,
                1000.,
                [
                    (0, 0.05951308832),
                    (31, 0.00045199221),
                    (32, 0.00045042421),
                    (47, 0.00042764642),
                ],
            ),
            (
                "Tone",
                22050.,
                2000.,
                [
                    (0, 0.05951308832),
                    (31, 0.00045199221),
                    (32, 0.00045079712),
                    (47, 0.00043343627),
                ],
            ),
            (
                "Edge",
                -0.5,
                0.5,
                [
                    (0, 0.03008336388),
                    (31, 0.00052733981),
                    (32, 0.00051525282),
                    (47, 0.00049541111),
                ],
            ),
            (
                "LowGain",
                1.,
                2.,
                [
                    (0, 0.05951308832),
                    (31, 0.00045199221),
                    (32, 0.00049363694),
                    (47, 0.00047462762),
                ],
            ),
            (
                "Trim",
                0.,
                -6.,
                [
                    (0, 0.05951308832),
                    (31, 0.00045199221),
                    (32, 0.00042916110),
                    (47, 0.00041263469),
                ],
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus {name}='{start}'/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            fx.process(&mut vec![[0.; 12]; 16384]).unwrap();
            fx.set_parameter(name, &ParameterValue::Number(target))
                .unwrap();
            assert_eq!(fx.parameter(name).unwrap(), ParameterValue::Number(target));
            let mut frames = [[0.; 12]; 48];
            frames[0][0] = 0.25;
            fx.process(&mut frames).unwrap();
            for (i, expected) in checkpoints {
                assert!(
                    (frames[i][0] - expected).abs() < 3e-8,
                    "{name}/{i}: {} vs {expected}",
                    frames[i][0]
                );
            }
        }
    }
    #[test]
    fn uvi_white_chorus_native_serialized_control_initialization() {
        // Native load-time controls take effect immediately. They must not
        // share the runtime setter's first-quantum hold or factory target.
        for (name, value, first, at32) in [
            ("Crossover", 1000., 0.08244666457, 0.00034789566),
            ("Tone", 2000., 0.01501191128, 0.00045284559),
            ("Edge", -0.5, 0.03008336388, 0.00052596111),
            ("LowGain", 2., 0.06016672775, 0.00105192221),
            ("Trim", -6., 0.02982719801, 0.00022594044),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><WhiteChorus {name}='{value}'/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut frames = [[0.; 12]; 48];
            frames[0][0] = 0.25;
            fx.process(&mut frames).unwrap();
            assert!(
                (frames[0][0] - first).abs() < 3e-8,
                "{name}: {}",
                frames[0][0]
            );
            assert!(
                (frames[32][0] - at32).abs() < 3e-8,
                "{name}/32: {}",
                frames[32][0]
            );
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
    fn uvi_dual_delay_native_raw_mix_target_and_reversal() {
        // Original DC + CC connection: Mix .2, Ratio -.5, CC127 at16384,
        // then CC0 at32768. Metadata remains .2 while the raw goal is -.3.
        let mut fx = TimeEffect::new(&effect("Feedback='0' Mix='0.2'"), 2, 48000.).unwrap();
        let metadata = fx.parameter("Mix").unwrap();
        let dc = |n| vec![[0.25, 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]; n];
        fx.process(&mut dc(16384)).unwrap();
        fx.set_effective("Mix", &ParameterValue::Number(-0.3))
            .unwrap();
        assert_eq!(fx.parameter("Mix").unwrap(), metadata);
        assert!(
            fx.set_parameter("Mix", &ParameterValue::Number(-0.3))
                .is_err()
        );
        assert!(
            fx.set_effective("Mix", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
        assert!(
            fx.set_effective("Mix", &ParameterValue::Number(f64::MAX))
                .is_err()
        );
        let mut down = dc(16384);
        fx.process(&mut down).unwrap();
        for (i, expected) in [
            (0, 0.2236067951),
            (31, 0.2236067951),
            (32, 0.2285310030),
            (64, 0.2330112010),
            (128, 0.2408284694),
            (512, 0.25),
        ] {
            assert!((down[i][0] - expected).abs() < 3e-8);
        }
        fx.set_effective("Mix", &ParameterValue::Number(0.2))
            .unwrap();
        let mut up = dc(1100);
        fx.process(&mut up).unwrap();
        for (i, expected) in [
            (32, 0.25),
            (128, 0.25),
            (384, 0.25),
            (512, 0.2440856099),
            (768, 0.2351646274),
            (1024, 0.2300771326),
        ] {
            assert!((up[i][0] - expected).abs() < 3e-8, "{i}: {}", up[i][0]);
        }
        assert_eq!(fx.parameter("Mix").unwrap(), metadata);
        // An explicit script write also resets an effective target when the
        // metadata value already equals the script value.
        fx.set_effective("Mix", &ParameterValue::Number(-0.3))
            .unwrap();
        fx.set_parameter("Mix", &metadata).unwrap();
        let TimeProcessor::Delay(delay) = fx.0 else {
            unreachable!()
        };
        assert_eq!(delay.mix_target, 0.2f32);
    }
    #[test]
    fn uvi_dual_delay_native_combined_stereo_routing() {
        // Original stereo-host impulse, independently characterized against
        // native legacy DualDelay and DualDelayX with shaping disabled.
        let mut fx = TimeEffect::new(
            &effect("DelayTime='0.001' Feedback='0.4' Mix='0.7' LowCut='230' HighCut='7300' Rotation='73' InputWidth='0.37' OutputWidth='0.61' InputRotation='-0.7' OutputRotation='0.8' DelayRatio='0.33' FeedbackRatio='-0.21'"),
            2,
            48000.,
        )
        .unwrap();
        let mut frames = vec![[0.; 12]; 300];
        frames[0][0] = 0.25;
        fx.process(&mut frames).unwrap();
        for (i, expected) in [
            (32, [0.00118245697, -0.00201184768]),
            (33, [0.00064492557, -0.00109728472]),
            (48, [0.02258636430, 0.01871825568]),
            (49, [0.00798956025, 0.00669017108]),
            (64, [-0.00066578778, -0.00065462047]),
            (80, [-0.00173245405, 0.00315187522]),
            (96, [0.00109349343, 0.00048162483]),
            (128, [-0.00095924834, -0.00068177789]),
            (256, [-0.00000186548, -0.00000575619]),
        ] {
            for ch in 0..2 {
                assert!(
                    (frames[i][ch] - expected[ch]).abs() < 3e-8,
                    "{i}/{ch}: {} vs {}",
                    frames[i][ch],
                    expected[ch]
                );
            }
        }
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
    fn uvi_time_effect_mix_native_control_smoothing() {
        for (kind, attrs, values) in [
            (
                "DualDelay",
                "Mix='0' Feedback='0'",
                [0.24092978239, 0.21564623713, 0.13840363920],
            ),
            (
                "WhiteChorus",
                "Mix='0' Edge='-1' LowGain='0' Crossover='5000'",
                [0.24092979729, 0.21564625204, 0.13840366900],
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><{kind} {attrs}/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut warmup = vec![[0.; 12]; 16384];
            for frame in &mut warmup {
                frame[0] = 0.25;
            }
            fx.process(&mut warmup).unwrap();
            fx.set_parameter("Mix", &ParameterValue::Number(1.))
                .unwrap();
            let mut frames = vec![[0.; 12]; 600];
            for frame in &mut frames {
                frame[0] = 0.25;
            }
            fx.process(&mut frames).unwrap();
            assert!(frames[..32].iter().all(|f| f[0] == 0.25));
            for (i, value) in [32, 128, 512].into_iter().zip(values) {
                assert!(
                    (frames[i][0] - value).abs() < 3e-8,
                    "{kind}/{i}: {} vs {value}",
                    frames[i][0]
                );
            }
            assert_eq!(frames[32][0], frames[63][0]);
        }
    }
    #[test]
    fn uvi_time_effect_native_late_mix_snap() {
        // Independent native DC captures distinguish a 256-frame snap from
        // floating-point convergence or snapping every control quantum.
        for (kind, attrs, residual) in [
            ("DualDelay", "Feedback='0'", 0.),
            (
                "WhiteChorus",
                "Edge='-1' LowGain='0' Crossover='5000'",
                2.98023224e-8,
            ),
        ] {
            let node = parse_program(&format!(
                "<Program><Inserts><{kind} Mix='0' {attrs}/></Inserts></Program>"
            ))
            .unwrap()
            .nodes
            .remove(2);
            let mut fx = TimeEffect::new(&node, 2, 48000.).unwrap();
            let mut warmup = vec![[0.; 12]; 16384];
            for frame in &mut warmup {
                frame[0] = 0.25;
            }
            fx.process(&mut warmup).unwrap();
            fx.set_parameter("Mix", &ParameterValue::Number(1.))
                .unwrap();
            let mut frames = vec![[0.; 12]; 9000];
            for frame in &mut frames {
                frame[0] = 0.25;
            }
            // An arbitrary caller chunk boundary must not reset the clock.
            fx.process(&mut frames[..5749]).unwrap();
            fx.process(&mut frames[5749..]).unwrap();
            for (i, expected) in [
                (5632, 0.000371262373),
                (5856, 0.000286280265),
                (5887, 0.000286280265),
                (5888, 0.),
                (8192, 0.),
            ] {
                assert!(
                    (frames[i][0] - expected - residual).abs() < 3e-8,
                    "{kind}/{i}: {} vs {}",
                    frames[i][0],
                    expected + residual
                );
            }
        }
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
