//! Original time-effect mathematics, characterized using authored PCM impulses
//! against official UVI Workstation 4.0.9 (Windows/Wine), 44.1/48/96 kHz,
//! 2026-10-03.
//! Parameter facts: https://lua.uvi.net/_elements.html and UVI Falcon manual
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf.
//! No vendor source or sample-bank content is included. DualDelay and DualDelayX
//! use measured RC filters, feedback rotation, fractional time and sqrt mixing.
//! X tape uses measured transposed shelves, normalized tanh and post-delay gain;
//! X diffusion uses a measured nested lattice and 500-frame coefficient clock
//! only at 48 kHz/Spread 20/readers 0/1; dispersion, grit and ducking are rejected.
//! WhiteChorus uses measured uint32 phase, a 256-entry sine table, RC crossover,
//! and a common Tone feedback line feeding parallel voices. Its Speed/Depth
//! startup clock and DualDelay modulation amplitude remain calibrated models.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI DualDelay/DualDelayX peak EQ uses an RBJ approximation and modulation amplitude is empirically calibrated; DualDelayX tape has measured finite-precision sine residuals up to 2.3e-7, live tape/switch, reflection, filtering and modulation enable transitions are rejected, and diffusion supports only 48 kHz/Spread20/readers0/1, with live enable transitions rejected and modulated read residuals up to 4.0e-5; scalar clocks outside Mix/Feedback/Rotation/TapeDrive/TapeWarmth/DiffusionAmount remain native-unverified; WhiteChorus Speed/Depth startup is calibrated and phase-offset/read rounding remains approximate, with live NumVoices/Mode/Bypass changes and rates outside 48 kHz native-unverified; legacy DualDelay control smoothing outside Mix/Feedback/Rotation, rates outside 44.1/48/96 kHz and bypass transitions remain native-unverified";
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
    for name in ["Dispersion", "Crusher"] {
        ensure!(p[name] == 0., "Unsupported active DualDelayX {name}");
    }
    ensure!(
        p["Diffusion"] == 0. || p["DiffusionSpread"] == 20.,
        "DualDelayX diffusion Spread outside 20 is native-unverified"
    );
    ensure!(
        p["Diffusion"] == 0. || p["Interpolation"] != 2.,
        "DualDelayX diffusion cubic reader is native-unverified"
    );
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

// Original native-measured 48 kHz / Spread20 nested allpass. The startup
// blend uses the forward lattice's allpole signal, not the dry input.
const DIFFUSION_NOMINALS: [[usize; 15]; 2] = [
    [
        100, 194, 208, 309, 333, 373, 413, 481, 575, 660, 692, 745, 790, 814, 868,
    ],
    [
        21, 125, 231, 286, 322, 340, 418, 432, 487, 558, 650, 727, 783, 849, 930,
    ],
];
struct Diffusion {
    lines: [Vec<f32>; 2],
    positions: [[usize; 16]; 2],
    current: f32,
    target: f32,
    previous_target: f32,
    changed_at: u64,
    endpoints: [f32; 2],
    phase: usize,
    interpolating: bool,
    startup: usize,
}
impl Diffusion {
    fn new(amount: f32) -> Self {
        Self {
            lines: std::array::from_fn(|_| vec![0.; 959]),
            positions: [[0; 16]; 2],
            current: amount,
            target: amount,
            previous_target: amount,
            changed_at: 0,
            endpoints: [0., amount],
            phase: 0,
            interpolating: true,
            startup: 0,
        }
    }
    fn clear(&mut self, amount: f32) {
        for line in &mut self.lines {
            line.fill(0.);
        }
        self.positions = [[0; 16]; 2];
        self.current = amount;
        self.target = amount;
        self.previous_target = amount;
        self.changed_at = 0;
        self.endpoints = [0., amount];
        self.phase = 0;
        self.interpolating = true;
        self.startup = 0;
    }
    fn set_target(&mut self, value: f32, elapsed: u64) {
        if value == self.target {
            return;
        }
        if self.changed_at != elapsed {
            self.previous_target = self.target;
        }
        self.changed_at = elapsed;
        self.target = value;
        if !self.interpolating {
            self.interpolating = true;
            self.phase = 0;
            self.endpoints = [self.current; 2];
        }
    }
    fn advance(&mut self, elapsed: u64, alpha: f32) {
        let target = if self.changed_at == elapsed {
            self.previous_target
        } else {
            self.target
        };
        self.current += alpha * (target - self.current);
        if elapsed.is_multiple_of(256) && (alpha * (target - self.current)).abs() < 1e-6 {
            self.current = target;
        }
    }
    fn process(&mut self, input: f32, ch: usize) -> f32 {
        let nominal = DIFFUSION_NOMINALS[ch];
        let fraction = self.phase as f32 / 500.;
        let amount = self.endpoints[0] + fraction * (self.endpoints[1] - self.endpoints[0]);
        let coefficients = nominal.map(|n| amount * 0.618 * n as f32 / 960.);
        let mut forward = [0.; 15];
        let mut read = [0.; 15];
        let mut x = input;
        for (i, &n) in nominal.iter().enumerate() {
            read[i] = self.lines[ch][n - 1 + self.positions[ch][i + 1]];
            forward[i] = x - coefficients[i] * read[i];
            x = forward[i];
        }
        let allpole = x;
        let mut child = x;
        for (i, &n) in nominal.iter().enumerate().rev() {
            let offset = n - 1 + self.positions[ch][i + 1];
            self.lines[ch][offset] = child;
            child = coefficients[i] * forward[i] + read[i];
            let gap = nominal.get(i + 1).copied().unwrap_or(960) - n;
            self.positions[ch][i + 1] = (self.positions[ch][i + 1] + 1) % gap;
        }
        let prefix = self.positions[ch][0];
        let delayed = self.lines[ch][prefix];
        self.lines[ch][prefix] = child;
        self.positions[ch][0] = (prefix + 1) % (nominal[0] - 1);
        let blend = (self.startup + 1).min(500) as f32 / 500.;
        blend * delayed + (1. - blend) * allpole
    }
    fn finish_frame(&mut self) {
        self.startup = (self.startup + 1).min(500);
        if self.interpolating {
            self.phase += 1;
            if self.phase == 500 {
                self.endpoints = [self.endpoints[1], self.current];
                self.phase = 0;
                self.interpolating =
                    self.endpoints[0] != self.endpoints[1] || self.current != self.target;
            }
        }
    }
    fn memory_bytes(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.capacity() * std::mem::size_of::<f32>())
            .sum()
    }
}

struct DualDelay {
    is_x: bool,
    tape: Option<Tape>,
    diffusion: Option<Diffusion>,
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
        ensure!(
            !is_x || parameters["Diffusion"] == 0. || rate == 48000.,
            "DualDelayX diffusion rate outside 48 kHz is native-unverified"
        );
        let diffusion = (is_x && parameters["Diffusion"] != 0.)
            .then(|| Diffusion::new(parameters["DiffusionAmount"] as f32));
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
            diffusion,
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
            + self.diffusion.as_ref().map_or(0, Diffusion::memory_bytes)
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
                "TapeSaturation" | "Reflection" | "Filtering" | "Modulation" | "Diffusion"
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
        } else if name == "DiffusionAmount"
            && let Some(diffusion) = &mut self.diffusion
        {
            diffusion.set_target(value as f32, self.elapsed);
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
        if let Some(diffusion) = &mut self.diffusion {
            diffusion.clear(self.parameters["DiffusionAmount"] as f32);
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
                if let Some(diffusion) = &mut self.diffusion {
                    diffusion.advance(self.elapsed, self.mix_smoothing);
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
                let nominal = self.frames[ch] + self.modulation[ch] * phase.sin() as f32;
                let delay = if self.diffusion.is_some() {
                    (nominal - 959.).max(24.)
                } else {
                    nominal.max(1.)
                };
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
                if let Some(diffusion) = &mut self.diffusion {
                    y = diffusion.process(y, ch);
                }
                if let Some(tape) = &mut self.tape {
                    y = tape.encode(y, ch);
                }
                next[ch] = y * if self.is_x { self.feedback[ch] } else { 1. };
            }
            if let Some(diffusion) = &mut self.diffusion {
                diffusion.finish_frame();
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
                for (ch, value) in wet.iter_mut().enumerate() {
                    let [start, end] = self.oscillator_endpoints[i][ch];
                    let oscillator = start + (end - start) * (self.elapsed % 64) as f32 / 64.;
                    let delay = base + amplitude * oscillator;
                    *value += voice_gain * self.read(delay.max(1.))[ch];
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
            "Diffusion='1' DiffusionSpread='10'",
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
            (48, [0., 0.124_999_51]),
            (96, [0.044_193_83, -0.044_193_834]),
            (144, [0., 0.031_249_64]),
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
        assert!((frames[480][0] - 0.124_999_985).abs() < 3e-8);
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
        assert!((frames[480][0] - 0.115_578_376).abs() < 3e-8);
    }
    #[test]
    fn uvi_dual_delay_x_native_tape_static() {
        for (drive, warmth, samples) in [
            (0., 0., [0.129_588_47, 0., 0.]),
            (0., 0.5, [0.115_581_66, -0.003_736_799, -0.002_602_618_4]),
            (0.2, 0.5, [0.099_103_18, -0.011_545_342, -0.008_080_411]),
            (0.5, 0.5, [0.071_868_03, -0.036_896_96, -0.026_981_048]),
            (1., 0.5, [0.060_970_873, -0.063_739_136, -0.078_565_456]),
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
            (44100., 0.2, 0.5, 45, -0.002_058_978_4),
            (44100., 0.5, 1., 46, -0.026_493_55),
            (96000., 0.2, 0.5, 96, 0.094_215_96),
            (96000., 0.5, 1., 212, -0.007_202_530_3),
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
                    0.002_694_309_5,
                    0.002_738_363_3,
                    0.003_171_589,
                    0.003_356_24,
                    0.003_415_464_4,
                ],
            ),
            (
                "TapeWarmth",
                0.8,
                [
                    0.002_786_492_5,
                    0.002_918_474,
                    0.002_823_625_2,
                    0.002_828_329_6,
                    0.002_829_542_4,
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
            (96, 0.020_421_073),
            (128, -0.000_302_817_47),
            (256, -0.000_345_733_37),
            (512, -0.000_149_263_27),
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
        assert!((frames[4608][0] - 0.001_220_703_1).abs() < 3e-8);
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
            (32, [0.001_433_747_1, -0.002_439_395_8]),
            (48, [0.024_584_277, 0.020_388_227]),
            (80, [-0.001_876_554_5, 0.003_431_939_8]),
            (96, [0.001_156_237_6, 0.000_479_718_73]),
            (256, [-0.000_001_415_771_7, -0.000_003_861_824]),
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
            frame[0] *= 0.999_999_9;
        }
        fx.process(&mut frames).unwrap();
        // Full independent native sine render measured max2.24e-7, unlike
        // sparse impulses; this precision ceiling is explicitly diagnosed.
        for (i, expected) in [
            (1000, -0.167_299_96),
            (8192, -0.299_773_42),
            (16384, 0.167_297_11),
            (24576, 0.179_487_97),
            (40000, 0.167_297_11),
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
                    (51, 0.061_581_48),
                    (182, 0.104_285_7),
                    (193, 0.090_797_715),
                    (267, 0.105_595_775),
                ],
            ),
            (
                "Edge='0'",
                vec![
                    (0, 0.059_513_09),
                    (158, -0.013_816_567),
                    (208, -0.017_259_67),
                    (316, 0.002_665_296_2),
                ],
            ),
            (
                "Edge='1'",
                vec![
                    (0, 0.224_319_55),
                    (158, -0.200_855_4),
                    (208, -0.063_926_4),
                    (316, 0.179_688_42),
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
                    (182, 0.104_454_204),
                    (236, 0.075_905_085),
                    (254, 0.116_439_4),
                    (267, 0.105_381_97),
                ],
            ),
            (
                "NumVoices='2'",
                vec![
                    (182, 0.147_592_2),
                    (193, 0.128_514_2),
                    (316, 0.025_828_667),
                ],
            ),
            (
                "NumVoices='8'",
                vec![
                    (51, 0.043_640_696),
                    (77, 0.072_120_64),
                    (107, 0.078_953_45),
                    (236, 0.048_142_046),
                    (254, 0.081_854_4),
                ],
            ),
            (
                "Trim='-6'",
                vec![
                    (51, 0.030_792_175),
                    (182, 0.052_235_026),
                    (267, 0.052_933_06),
                ],
            ),
            (
                "Mix='0.5'",
                vec![
                    (51, 0.043_443_56),
                    (182, 0.073_696_494),
                    (267, 0.074_681_33),
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
                    (57, 0.059_077_647),
                    (82, 0.078_868_58),
                    (250, 0.068_715_21),
                ],
            ),
            (
                6,
                1,
                [
                    (220, 0.077_283_95),
                    (238, 0.049_999_15),
                    (264, 0.086_751_1),
                ],
            ),
            (
                8,
                1,
                [
                    (211, 0.066_611_886),
                    (229, 0.061_039_2),
                    (267, 0.135_168_37),
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
                    (77, 0.101_637_766),
                    (107, 0.111_327_73),
                    (254, 0.115_998_276),
                ],
            ),
            (
                6,
                0,
                [
                    (51, 0.049_814_075),
                    (220, 0.076_942_34),
                    (267, 0.085_538_7),
                ],
            ),
            (
                8,
                0,
                [
                    (52, 0.049_564_91),
                    (124, 0.080_807_53),
                    (256, 0.081_014_54),
                ],
            ),
            (
                6,
                1,
                [
                    (57, 0.058_815_32),
                    (107, 0.050_633_79),
                    (193, 0.073_244_45),
                ],
            ),
            (
                8,
                1,
                [
                    (71, 0.070_021_61),
                    (124, 0.080_191_31),
                    (193, 0.063_067_12),
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
            (4, 0, [(135437, 0.037_409_782), (299119, 0.012_254_771)]),
            (6, 1, [(213114, 0.055_836_804), (446698, 0.084_091_134)]),
            (8, 1, [(110640, 0.072_094_63), (397437, 0.003_575_537_1)]),
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
                    (0, 0.059_513_09),
                    (31, 0.000_451_992_2),
                    (32, 0.000_450_424_2),
                    (47, 0.00042764642),
                ],
            ),
            (
                "Tone",
                22050.,
                2000.,
                [
                    (0, 0.059_513_09),
                    (31, 0.000_451_992_2),
                    (32, 0.00045079712),
                    (47, 0.00043343627),
                ],
            ),
            (
                "Edge",
                -0.5,
                0.5,
                [
                    (0, 0.030_083_364),
                    (31, 0.000_527_339_8),
                    (32, 0.000_515_252_8),
                    (47, 0.000_495_411_1),
                ],
            ),
            (
                "LowGain",
                1.,
                2.,
                [
                    (0, 0.059_513_09),
                    (31, 0.000_451_992_2),
                    (32, 0.00049363694),
                    (47, 0.00047462762),
                ],
            ),
            (
                "Trim",
                0.,
                -6.,
                [
                    (0, 0.059_513_09),
                    (31, 0.000_451_992_2),
                    (32, 0.000_429_161_1),
                    (47, 0.000_412_634_7),
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
            ("Crossover", 1000., 0.082_446_665, 0.00034789566),
            ("Tone", 2000., 0.015_011_911, 0.000_452_845_6),
            ("Edge", -0.5, 0.030_083_364, 0.000_525_961_1),
            ("LowGain", 2., 0.060_166_728, 0.001_051_922_2),
            ("Trim", -6., 0.029_827_198, 0.00022594044),
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
        assert!((f[arrival][0] - 0.002_311_567_3).abs() < 3e-8);
        let integral: f32 = f[arrival..arrival + 8].iter().map(|f| f[0]).sum();
        assert!((integral - 0.002_448_689_7).abs() < 3e-8, "{integral}");
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
            (480, 0.115_578_376),
            (481, 0.008_129_147),
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
            (0, 0.223_606_8),
            (31, 0.223_606_8),
            (32, 0.228_531),
            (64, 0.233_011_2),
            (128, 0.240_828_47),
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
            (512, 0.244_085_61),
            (768, 0.235_164_63),
            (1024, 0.230_077_13),
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
            (32, [0.001_182_457, -0.002_011_847_7]),
            (33, [0.000_644_925_6, -0.001_097_284_7]),
            (48, [0.022_586_364, 0.018_718_256]),
            (49, [0.007_989_56, 0.006_690_171]),
            (64, [-0.000_665_787_8, -0.00065462047]),
            (80, [-0.001_732_454, 0.003_151_875_2]),
            (96, [0.001_093_493_4, 0.00048162483]),
            (128, [-0.00095924834, -0.000_681_777_9]),
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
                [0.240_929_78, 0.215_646_24, 0.138_403_64],
            ),
            (
                "WhiteChorus",
                "Mix='0' Edge='-1' LowGain='0' Crossover='5000'",
                [0.240_929_8, 0.215_646_25, 0.138_403_67],
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
                2.980_232_2e-8,
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
                (5632, 0.000_371_262_37),
                (5856, 0.000_286_280_26),
                (5887, 0.000_286_280_26),
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
            ("Feedback", 0.6, [0.020_433_394_f32, 0.]),
            ("Rotation", 90., [0.018_942_589, 0.002_148_239_9]),
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
                (f[48][0] - 0.069_346_756).abs() < 3e-8,
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
            (44100., 441, 0.117_430_86_f32, 0.108233504),
            (96000., 960, 0.091_119_28, 0.071_589_7),
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
    #[test]
    fn uvi_dual_delay_x_diffusion_proven_scope_and_clear() {
        let attrs = "Diffusion='1' DiffusionSpread='20' Filtering='0'";
        let mut fx = TimeEffect::new(&delay_x(attrs), 2, 48000.).unwrap();
        for rate in [44100., 96000.] {
            assert!(TimeEffect::new(&delay_x(attrs), 2, rate).is_err());
        }
        assert!(TimeEffect::new(&delay_x("Diffusion='1' Interpolation='2'"), 2, 48000.).is_err());
        for (name, value) in [
            ("DiffusionSpread", 10.),
            ("Interpolation", 2.),
            ("Diffusion", 0.),
        ] {
            let old = fx.parameter(name).unwrap();
            assert!(
                fx.set_parameter(name, &ParameterValue::Number(value))
                    .is_err()
            );
            assert_eq!(fx.parameter(name).unwrap(), old);
        }
        let bytes = fx.memory_bytes();
        assert!(
            bytes
                > TimeEffect::new(&delay_x(""), 2, 48000.)
                    .unwrap()
                    .memory_bytes()
        );
        let mut first = vec![[0.; 12]; 6000];
        first[0][0] = 0.25;
        fx.process(&mut first).unwrap();
        fx.clear();
        let mut replay = vec![[0.; 12]; 6000];
        replay[0][0] = 0.25;
        for chunk in replay.chunks_mut(173) {
            fx.process(chunk).unwrap();
        }
        assert_eq!(first, replay);
        assert_eq!(fx.memory_bytes(), bytes);
    }
    #[test]
    fn uvi_dual_delay_x_diffusion_native_stereo_and_minimum() {
        let mut fx = TimeEffect::new(&delay_x("DelayTime='.1' Feedback='.5' Mix='1' Filtering='0' Diffusion='1' DiffusionAmount='.3'"), 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 20000];
        frames[8192][0] = 0.25 * 0.999_999_9;
        frames[8192][1] = 0.125 * 0.999_999_9;
        fx.process(&mut frames).unwrap();
        for (offset, l, r) in [
            (3861, 0., 0.00025347652),
            (3940, 0.002_414_062_2, 0.),
            (3965, 0., 0.001_508_764_1),
            (4034, 0.004_681_533_7, 0.),
            (4048, 0.005_012_331, 0.),
            (4800, 0.104_388_066, 0.052_856_263),
            (9600, 0.030_452_477, 0.015_968_073),
        ] {
            for (ch, expected) in [l, r].into_iter().enumerate() {
                assert!(
                    (frames[8192 + offset][ch] - expected).abs() < 3e-8,
                    "{offset}/{ch}"
                );
            }
        }
        for reader in [0, 1] {
            let mut fx = TimeEffect::new(&delay_x(&format!("DelayTime='.001' Feedback='.2' Mix='1' Filtering='0' Diffusion='1' DiffusionAmount='.3' Interpolation='{reader}'")), 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 10000];
            frames[8192][0] = 0.25 * 0.999_999_9;
            fx.process(&mut frames).unwrap();
            assert!(frames[8192..8192 + 123].iter().all(|f| f[0] == 0.));
            for (offset, expected) in [
                (123, 0.00096562493),
                (217, 0.001_872_613_6),
                (231, 0.002_004_932_4),
            ] {
                assert!(
                    (frames[8192 + offset][0] - expected).abs() < 3e-8,
                    "{reader}/{offset}"
                );
            }
        }
    }
    #[test]
    fn uvi_dual_delay_x_diffusion_native_startup() {
        // Original stereo PCM pulse at host frame0. The early negative terms
        // distinguish allpole/allpass startup from an ordinary dry/wet fade.
        for (amount, at116, at194, at983_l, at983_r) in [
            (
                0.2,
                -0.002_093_753_3,
                -0.005_203_407,
                0.121_748_69,
                0.060_925_283,
            ),
            (
                0.31,
                -0.003_246_576_5,
                -0.008_064_085,
                0.117_322_2,
                0.058_778_048,
            ),
            (
                0.9,
                -0.009_445_139,
                -0.023_104_386,
                0.072_560_86,
                0.036_797_095,
            ),
        ] {
            let mut fx = TimeEffect::new(&delay_x(&format!("DelayTime='.01' Feedback='.5' Mix='1' Filtering='0' Diffusion='1' DiffusionAmount='{amount}'")), 2, 48000.).unwrap();
            let mut frames = vec![[0.; 12]; 5000];
            frames[0][0] = 0.25 * 0.999_999_9;
            frames[0][1] = 0.125 * 0.999_999_9;
            for chunk in frames.chunks_mut(73) {
                fx.process(chunk).unwrap();
            }
            for (at, ch, expected) in [
                (24, 0, 0.124_749_99),
                (48, 0, 0.059_256_244),
                (116, 0, at116),
                (194, 0, at194),
                (983, 0, at983_l),
                (983, 1, at983_r),
            ] {
                assert!(
                    (frames[at][ch] - expected).abs() < 3e-8,
                    "{amount}/{at}/{ch}: {} vs{expected}",
                    frames[at][ch]
                );
            }
        }
    }
    #[test]
    fn uvi_dual_delay_x_diffusion_native_repeated_amount() {
        let mut fx = TimeEffect::new(&delay_x("DelayTime='.01' Feedback='.01' Mix='1' Filtering='0' Diffusion='1' DiffusionAmount='.2'"), 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 45000];
        for offset in [61, 137, 293, 619, 1259, 2539, 5087, 10193, 20399] {
            frames[16384 + offset][0] = 0.25 * 0.999_999_9;
        }
        fx.process(&mut frames[..16384]).unwrap();
        fx.set_parameter("DiffusionAmount", &ParameterValue::Number(0.4))
            .unwrap();
        fx.process(&mut frames[16384..16640]).unwrap();
        fx.set_parameter("DiffusionAmount", &ParameterValue::Number(0.1))
            .unwrap();
        for chunk in frames[16640..].chunks_mut(127) {
            fx.process(chunk).unwrap();
        }
        // Two setters 256 frames apart preserve the running 500-frame lattice
        // interpolator. An immediate coefficient assignment fails these terms.
        for (offset, expected) in [
            (184, 0.000_032_187_494),
            (416, 0.000_032_187_494),
            (742, 0.000_032_233_21),
            (1382, 0.000_089_281_9),
            (2662, 0.000_014_461_153),
            (5210, 0.000_016_041_646),
            (10316, 0.000_016_093_867),
        ] {
            assert!(
                (frames[16384 + offset][0] - expected).abs() < 3e-8,
                "{offset}: {}",
                frames[16384 + offset][0]
            );
        }
    }
    #[test]
    fn uvi_dual_delay_x_diffusion_native_tape_and_sine() {
        let mut fx = TimeEffect::new(&delay_x("DelayTime='.01' Feedback='.5' Mix='1' Filtering='0' Diffusion='1' DiffusionAmount='.31' TapeSaturation='1' TapeDrive='.2' TapeWarmth='.5'"), 2, 48000.).unwrap();
        let mut pulse = vec![[0.; 12]; 5000];
        pulse[0][0] = 0.25 * 0.999_999_9;
        pulse[0][1] = 0.125 * 0.999_999_9;
        fx.process(&mut pulse).unwrap();
        for (at, l, r) in [
            (24, 0.099_029_52, 0.066_266_97),
            (48, 0.044_342_306, 0.030_663_889),
            (194, -0.008_771_609, -0.00001034039),
            (503, -0.000_534_711_9, 0.00010968557),
            (983, 0.096_676_27, 0.063_308_38),
            (1966, 0.033_122_648, 0.023_902_075),
        ] {
            for (ch, expected) in [l, r].into_iter().enumerate() {
                assert!((pulse[at][ch] - expected).abs() < 3e-8, "tape/{at}/{ch}");
            }
        }
        let mut fx = TimeEffect::new(&delay_x("DelayTime='.08' Feedback='.45' Mix='.63' LowCut='210' HighCut='8500' Rotation='47' InputWidth='.3' OutputWidth='.8' InputRotation='-.3' OutputRotation='.6' DelayRatio='.25' FeedbackRatio='-.1' TapeSaturation='1' TapeDrive='.22' TapeWarmth='.6' Diffusion='1' DiffusionAmount='.31'"), 2, 48000.).unwrap();
        let mut frames = vec![[0.; 12]; 24001];
        for (i, frame) in frames.iter_mut().enumerate() {
            let pcm =
                (8192. * (std::f64::consts::TAU * 1000. * i as f64 / 48000.).sin()).round() as f32;
            frame[0] = pcm / 32768. * 0.999_999_9;
        }
        fx.process(&mut frames).unwrap();
        // Complete native-vs-Rust sine max is 8.2e-8. Keep this independently
        // measured precision bound separate from strict impulse/control tests.
        for (at, l, r) in [
            (2916, -0.200_398_94, -0.047_986_377),
            (2980, 0.093_402_624, 0.022_322_372),
            (3840, 0.005_907_886, 0.004_690_357_5),
            (6000, 0.011_188_26, 0.008_007_38),
            (11270, -0.180_821_72, -0.066_809_97),
            (24000, 0.009_059_284, 0.017_086_886),
        ] {
            for (ch, expected) in [l, r].into_iter().enumerate() {
                assert!(
                    (frames[at][ch] - expected).abs() <= 1e-7,
                    "sine/{at}/{ch}: {} vs{expected}",
                    frames[at][ch]
                );
            }
        }
    }
}
