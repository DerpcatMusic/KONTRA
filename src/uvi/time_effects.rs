//! Original time-effect mathematics, characterized using authored PCM impulses
//! against official UVI Workstation 4.0.9 (Windows/Wine), 44.1/48/96 kHz,
//! 2026-10-03.
//! Parameter facts: https://lua.uvi.net/_elements.html and UVI Falcon manual
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf.
//! No vendor source or sample-bank content is included. Stationary DualDelay
//! uses measured RC filters, feedback rotation, fractional time and sqrt mixing.
//! Sine modulation and Mix control smoothing use original calibrated models.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI DualDelay peak EQ uses an RBJ approximation and modulation amplitude is empirically calibrated; control smoothing outside Mix/Feedback/Rotation, rates outside 44.1/48/96 kHz and bypass transitions remain native-unverified";
const MAX_SECONDS: f64 = 5.;
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

pub fn supports(kind: &str) -> bool {
    kind == "DualDelay"
}

fn scalar(value: &ParameterValue) -> Result<f64> {
    match value {
        ParameterValue::Number(v) if v.is_finite() => Ok(*v),
        ParameterValue::Boolean(v) => Ok(f64::from(u8::from(*v))),
        _ => bail!("UVI delay requires a finite numeric parameter"),
    }
}

fn checked(name: &str, value: &ParameterValue) -> Result<f64> {
    let (_, _, low, high, integer) = DELAY
        .iter()
        .find(|p| p.0 == name)
        .with_context(|| format!("Unsupported DualDelay parameter {name}"))?;
    let value = scalar(value)?;
    ensure!(
        (*low..=*high).contains(&value) && (!integer || value.fract() == 0.),
        "Invalid DualDelay parameter {name}"
    );
    Ok(f64::from(value as f32))
}

fn parameters(node: &ProgramNode) -> Result<BTreeMap<String, f64>> {
    ensure!(
        supports(&node.kind),
        "Unsupported UVI time effect {}",
        node.kind
    );
    let mut p: BTreeMap<String, f64> = DELAY.iter().map(|&(n, v, ..)| (n.into(), v)).collect();
    for (name, raw) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let value = ParameterValue::Number(
            raw.parse()
                .with_context(|| format!("Invalid DualDelay parameter {name}"))?,
        );
        p.insert(name.clone(), checked(name, &value)?);
    }
    // Old serialized versions store angular modulation depth. The native
    // loader migrates it to the version-1 value, clamped to the public range.
    if !node.attributes.contains_key("DualDelayVersion") || p["DualDelayVersion"] == 0. {
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

pub struct TimeEffect {
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
    input_rotation: [f32; 2],
    output_rotation: [f32; 2],
    feedback_rotation: [f32; 2],
    poles: [f32; 2],
    low: [f32; 2],
    high: [f32; 2],
    peak: Peak,
}
impl TimeEffect {
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
        let length = (MAX_SECONDS * rate).ceil() as usize + 2;
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
            self.rate * 0.00057425 * p["ModDepth"] / (std::f64::consts::TAU * p["ModRate"]);
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
    /// Stationary and modulated stereo-bus processing; mono insert routing is unverified.
    pub fn output_channels(&self) -> usize {
        2
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
        let value = checked(name, value)?;
        let old = self
            .parameters
            .insert(name.into(), value)
            .context("Unknown UVI delay parameter")?;
        if let Err(error) = self.tune() {
            self.parameters.insert(name.into(), old);
            self.tune()?;
            return Err(error);
        }
        if value != old {
            if let Some(control) = match name {
                "Mix" => Some(0),
                "Feedback" | "FeedbackRatio" => Some(1),
                "Rotation" => Some(2),
                _ => None,
            } {
                self.control_changed_at[control] = self.elapsed;
            }
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
                if self.elapsed != self.control_changed_at[0] {
                    self.mix += self.mix_smoothing * (self.parameters["Mix"] as f32 - self.mix);
                }
                self.mix_gains = [(1. - self.mix).sqrt(), self.mix.sqrt()];
                for ch in 0..2 {
                    if self.elapsed == self.control_changed_at[1] {
                        continue;
                    }
                    self.feedback[ch] +=
                        self.mix_smoothing * (self.feedback_target[ch] - self.feedback[ch]);
                }
                if self.elapsed != self.control_changed_at[2] {
                    self.rotation +=
                        self.mix_smoothing * (self.parameters["Rotation"] as f32 - self.rotation);
                }
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
