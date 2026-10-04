//! Original lookahead-limiter mathematics, measured against official UVI
//! Workstation 4.0.9 using authored PCM16 impulses, steps and sine bursts.
//! Parameter facts: https://lua.uvi.net/_elements.html and
//! https://s3.amazonaws.com/uvi/UVIFC/falcon_manual.pdf (2026-10-03).
//! No vendor source, scripts or sample-bank material is included.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeMap, VecDeque};

pub const FIDELITY_DIAGNOSTIC: &str = "UVI Maximizer release after varying peaks and SlewRate use an empirical model; live controls other than Threshold/Ceiling, bypass reentry and rates outside 44.1/48/96 kHz remain native-unverified";

const CONTROLS: &[(&str, f64, f64, f64, bool)] = &[
    ("Bypass", 0., 0., 1., true),
    ("Threshold", -6., -40., 0., false),
    ("Attack", 0., 0., 20., false),
    ("Lookahead", 2., 1., 20., false),
    ("Release", 10., 0.1, 1000., false),
    ("Ceiling", -0.1, -20., 0., false),
    ("Knee", 0., 0., 10., false),
    ("ReleaseBlend", 0.1, 0., 1., false),
    ("SlewRate", 20., 0., 200., false),
    ("CeilingMode", 0., 0., 0., true),
    ("TruePeak", 0., 0., 0., true),
];

pub fn supports(kind: &str) -> bool {
    kind == "Maximizer"
}

fn checked(name: &str, value: &ParameterValue) -> Result<f64> {
    let (_, _, low, high, integer) = CONTROLS
        .iter()
        .find(|control| control.0 == name)
        .with_context(|| format!("Unsupported Maximizer parameter {name}"))?;
    let value = match value {
        ParameterValue::Number(value) if value.is_finite() => *value,
        ParameterValue::Boolean(value) => f64::from(u8::from(*value)),
        _ => bail!("Maximizer requires a finite numeric parameter"),
    };
    ensure!(
        (*low..=*high).contains(&value) && (!integer || value.fract() == 0.),
        "Invalid Maximizer parameter {name}"
    );
    ensure!(
        name != "Attack" || value == 0.,
        "Maximizer nonzero Attack is not native-characterized"
    );
    Ok(f64::from(value as f32))
}

fn parameters(node: &ProgramNode) -> Result<BTreeMap<String, f64>> {
    ensure!(
        supports(&node.kind),
        "Unsupported UVI limiter {}",
        node.kind
    );
    let mut parameters: BTreeMap<String, f64> = CONTROLS
        .iter()
        .map(|&(name, value, ..)| (name.into(), f64::from(value as f32)))
        .collect();
    for (name, raw) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let value = ParameterValue::Number(
            raw.parse()
                .with_context(|| format!("Invalid Maximizer parameter {name}"))?,
        );
        parameters.insert(name.clone(), checked(name, &value)?);
    }
    Ok(parameters)
}

pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

// Fixed-capacity means. Epoch tags make a new gain minimum reset the slew
// history in O(1), including during continuously increasing input peaks.
struct Mean {
    samples: Vec<(u64, f64)>,
    position: usize,
    length: usize,
    epoch: u64,
    base: f64,
    sum: f64,
}
impl Mean {
    fn new(capacity: usize, length: usize, value: f64) -> Self {
        let mut result = Self {
            samples: vec![(0, 0.); capacity],
            position: 0,
            length: 1,
            epoch: 0,
            base: 0.,
            sum: 0.,
        };
        result.reset(length, value);
        result
    }
    fn reset(&mut self, length: usize, value: f64) {
        assert!((1..=self.samples.len()).contains(&length));
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.samples.fill((0, 0.));
            self.epoch = 1;
        }
        self.position = 0;
        self.length = length;
        self.base = value;
        self.sum = value * length as f64;
    }
    fn value(&self) -> f64 {
        self.sum / self.length as f64
    }
    fn process(&mut self, value: f64) -> f64 {
        let (epoch, old) = self.samples[self.position];
        self.sum += value - if epoch == self.epoch { old } else { self.base };
        self.samples[self.position] = (self.epoch, value);
        self.position = (self.position + 1) % self.length;
        self.value()
    }
    fn memory_bytes(&self) -> usize {
        self.samples.capacity() * std::mem::size_of::<(u64, f64)>()
    }
}

pub struct Maximizer {
    rate: f64,
    channels: usize,
    parameters: BTreeMap<String, f64>,
    smoothed: [f64; 2],
    previous_targets: [f64; 2],
    smoothing: f64,
    delay: Vec<Frame>,
    position: usize,
    elapsed: u64,
    peaks: VecDeque<(u64, f64)>,
    half_lookahead: usize,
    fast: f64,
    slow: f64,
    slew: Mean,
    means: [Mean; 2],
}
impl Maximizer {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=2).contains(&channels),
            "Maximizer requires a mono or stereo bus; wider linking is unverified"
        );
        ensure!(
            rate.is_finite() && (8000. ..=192000.).contains(&rate),
            "Invalid Maximizer sample rate"
        );
        let parameters = parameters(node)?;
        let smoothed = [parameters["Threshold"], parameters["Ceiling"]];
        let half_lookahead = (parameters["Lookahead"] * rate / 2000.).floor() as usize;
        let max_half = (20. * rate / 2000.).ceil() as usize;
        let slew_capacity = (0.35 * 200. * rate / 1000.).ceil() as usize;
        let gain = 10f64.powf((smoothed[1] - smoothed[0]) / 20.);
        Ok(Self {
            rate,
            channels,
            parameters,
            smoothed,
            previous_targets: smoothed,
            // Native 32-frame control clock, independently measured by the
            // limiter, delay, chorus and control-source probes.
            smoothing: f64::from(1. - 0.33f32.powf(3200. / rate as f32)),
            delay: vec![[0.; 12]; max_half * 2 + 1],
            position: 0,
            elapsed: 0,
            peaks: VecDeque::with_capacity(max_half * 2 + 1),
            half_lookahead,
            fast: 0.,
            slow: 0.,
            slew: Mean::new(slew_capacity, 1, 0.),
            means: [
                Mean::new(max_half, half_lookahead, gain),
                Mean::new(max_half, half_lookahead, gain),
            ],
        })
    }
    fn slew_length(&self) -> usize {
        // ponytail: fitted finite-window release shape, not a vendor formula.
        // Replace this calibration after varying-peak native parity is proven.
        ((0.35 * self.parameters["SlewRate"] * self.rate / 1000.).ceil() as usize)
            .saturating_sub(2)
            .max(1)
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        self.parameters
            .get(name)
            .copied()
            .map(ParameterValue::Number)
            .context("Unknown Maximizer parameter")
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let value = checked(name, value)?;
        let old = self
            .parameters
            .get_mut(name)
            .context("Unknown Maximizer parameter")?;
        if *old == value {
            return Ok(());
        }
        *old = value;
        match name {
            "Lookahead" => {
                self.half_lookahead = (value * self.rate / 2000.).floor() as usize;
                let gain = self.means[1].value();
                for mean in &mut self.means {
                    mean.reset(self.half_lookahead, gain);
                }
            }
            "SlewRate" => self.slew.reset(self.slew_length(), self.slew.value()),
            _ => {}
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.delay.fill([0.; 12]);
        self.position = 0;
        self.elapsed = 0;
        self.peaks.clear();
        self.fast = 0.;
        self.slow = 0.;
        self.smoothed = [self.parameters["Threshold"], self.parameters["Ceiling"]];
        self.previous_targets = self.smoothed;
        self.slew.reset(self.slew_length(), 0.);
        let gain = 10f64.powf((self.smoothed[1] - self.smoothed[0]) / 20.);
        for mean in &mut self.means {
            mean.reset(self.half_lookahead, gain);
        }
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.delay.capacity() * std::mem::size_of::<Frame>()
            + self.peaks.capacity() * std::mem::size_of::<(u64, f64)>()
            + self.slew.memory_bytes()
            + self.means.iter().map(Mean::memory_bytes).sum::<usize>()
    }
    pub fn diagnostics(&self) -> &'static str {
        FIDELITY_DIAGNOSTIC
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|frame| frame[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite Maximizer input"
        );
        if self.parameters["Bypass"] != 0. {
            return Ok(());
        }
        let lookahead = self.half_lookahead * 2;
        let release = self.parameters["Release"] * self.rate / 1000.;
        let fast_coefficient = (-3f64.ln() / release).exp();
        let slow_coefficient = (-3f64.ln() / (100. * release)).exp();
        let blend = self.parameters["ReleaseBlend"];
        let knee = self.parameters["Knee"];
        let slew_length = self.slew_length();
        let targets = [self.parameters["Threshold"], self.parameters["Ceiling"]];
        for frame in frames {
            if self.elapsed != 0 && self.elapsed.is_multiple_of(32) {
                for (smoothed, target) in self.smoothed.iter_mut().zip(self.previous_targets) {
                    *smoothed += self.smoothing * (target - *smoothed);
                }
            }
            // Publish setters after the control tick: an aligned step keeps
            // its first quantum, while a continuously changing target advances.
            self.previous_targets = targets;
            let peak = frame[..self.channels]
                .iter()
                .map(|x| f64::from(x.abs()))
                .fold(0., f64::max);
            let over = if peak > 0. {
                20. * peak.log10() - self.smoothed[0]
            } else {
                f64::NEG_INFINITY
            };
            let reduction = if knee == 0. || over >= knee {
                -over.max(0.)
            } else if over <= -knee {
                0.
            } else {
                -(over + knee).powi(2) / (4. * knee)
            } * std::f64::consts::LN_10
                / 20.;
            // Release approaches the current weaker reduction, not always
            // zero. Apply it before the lookahead minimum hold: paired peaks
            // and changing-amplitude sine bursts distinguish the ordering.
            if reduction < self.fast * (1. - blend) + self.slow * blend {
                self.fast = reduction;
                self.slow = reduction;
            } else {
                self.fast = reduction + (self.fast - reduction) * fast_coefficient;
                self.slow = reduction + (self.slow - reduction) * slow_coefficient;
            }
            let reduction = self.fast * (1. - blend) + self.slow * blend;
            while self
                .peaks
                .front()
                .is_some_and(|&(time, _)| self.elapsed.wrapping_sub(time) >= lookahead as u64)
            {
                self.peaks.pop_front();
            }
            while self
                .peaks
                .back()
                .is_some_and(|&(_, value)| value >= reduction)
            {
                self.peaks.pop_back();
            }
            self.peaks.push_back((self.elapsed, reduction));
            let mut reduction = self.peaks.front().expect("current envelope minimum").1;
            if reduction < self.slew.value() || self.slew.length != slew_length {
                self.slew.reset(slew_length, reduction);
            } else {
                reduction = self.slew.process(reduction);
            }
            let gain = reduction.exp() * 10f64.powf((self.smoothed[1] - self.smoothed[0]) / 20.);
            let gain = self.means[0].process(gain);
            let gain = self.means[1].process(gain) as f32;
            self.delay[self.position] = *frame;
            let delayed =
                self.delay[(self.position + self.delay.len() - lookahead) % self.delay.len()];
            for channel in 0..self.channels {
                frame[channel] = delayed[channel] * gain;
            }
            self.position = (self.position + 1) % self.delay.len();
            self.elapsed = self.elapsed.wrapping_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(values: &[(&str, &str)]) -> ProgramNode {
        ProgramNode {
            parent: None,
            kind: "Maximizer".into(),
            name: None,
            attributes: values.iter().map(|&(k, v)| (k.into(), v.into())).collect(),
            text: String::new(),
        }
    }
    fn frame(left: f32, right: f32) -> Frame {
        let mut frame = [0.; 12];
        frame[..2].copy_from_slice(&[left, right]);
        frame
    }
    #[test]
    fn native_maximizer_gain_timing_controls_and_bounds() -> Result<()> {
        let mut effect = Maximizer::new(&node(&[]), 2, 48000.)?;
        let mut constant = vec![frame(0.25, 0.125); 16384];
        effect.process(&mut constant)?;
        assert!(constant[..96].iter().all(|frame| frame[0] == 0.));
        assert!((constant[96][0] - 0.49310568).abs() < 1e-7);
        assert_eq!(constant[96][0], 2. * constant[96][1]);
        effect.set_parameter("Threshold", &ParameterValue::Number(-12.))?;
        let mut changed = vec![frame(0.25, 0.125); 4096];
        effect.process(&mut changed)?;
        for (i, native) in [
            (0, 0.49310568),
            (32, 0.49311647),
            (64, 0.49917296),
            (256, 0.632_216),
            (512, 0.77015686),
            (1024, 0.91271424),
        ] {
            assert!(
                (changed[i][0] - native).abs() < 4e-5,
                "live threshold frame {i}"
            );
        }
        let mut continuous = Maximizer::new(&node(&[]), 2, 48000.)?;
        for i in 0..256 {
            continuous.set_parameter("Threshold", &ParameterValue::Number(-6. - i as f64 / 64.))?;
            continuous.set_parameter("Ceiling", &ParameterValue::Number(-0.1 - i as f64 / 128.))?;
            continuous.process(&mut [frame(0.25, 0.125)])?;
        }
        assert!(
            continuous.smoothed[0] < -6.5 && continuous.smoothed[1] < -0.3,
            "continuous targets must not starve the 32-frame control clock"
        );
        let mut effect = Maximizer::new(
            &node(&[("SlewRate", "0"), ("ReleaseBlend", "0")]),
            2,
            48000.,
        )?;
        let mut step = (0..33000)
            .map(|i| {
                frame(
                    if (8192..16384).contains(&i) {
                        0.75
                    } else {
                        328. / 32768.
                    },
                    164. / 32768.,
                )
            })
            .collect::<Vec<_>>();
        effect.process(&mut step)?;
        for (i, native) in [
            (8192, 0.019740647),
            (8240, 0.016266692),
            (8288, 0.98855305),
            (16480, 0.013_193_6),
            (17504, 0.018913947),
        ] {
            assert!(
                (step[i][0] - native).abs() < 3e-6,
                "native step frame {i}: {}",
                step[i][0]
            );
        }
        let mut varying = Maximizer::new(
            &node(&[("SlewRate", "0"), ("ReleaseBlend", "0")]),
            2,
            48000.,
        )?;
        let input = (0..20000)
            .map(|i| {
                let amplitude = if i % 1000 < 300 { 0.74 } else { 0.24 };
                let sample = (amplitude * (i as f64 * 0.074).sin() * 32768.).round();
                frame(sample as f32 / 32768., 1638. / 32768.)
            })
            .collect::<Vec<_>>();
        let mut bursts = input.clone();
        varying.process(&mut bursts)?;
        for (i, native) in [
            (15400, 0.066_993_3),
            (15450, 0.068_893_49),
            (15500, 0.071_563_7),
        ] {
            assert!(
                (bursts[i][1] - native).abs() < 5e-7,
                "varying peaks frame {i}"
            );
        }
        let mut blended = Maximizer::new(&node(&[("SlewRate", "0")]), 2, 48000.)?;
        let mut bursts = input;
        blended.process(&mut bursts)?;
        for (i, native) in [(3062, 0.076_910_69), (7995, 0.086_165_55)] {
            assert!(
                (bursts[i][1] - native).abs() < 5e-7,
                "blended release frame {i}"
            );
        }
        let mut calibrated = Maximizer::new(&node(&[]), 2, 48000.)?;
        let mut step = (0..33000)
            .map(|i| {
                frame(
                    if (8192..16384).contains(&i) {
                        0.75
                    } else {
                        328. / 32768.
                    },
                    164. / 32768.,
                )
            })
            .collect::<Vec<_>>();
        calibrated.process(&mut step)?;
        for (i, native) in [
            (16480, 0.013193579),
            (17504, 0.017906412),
            (20576, 0.019027537),
            (32864, 0.019201135),
        ] {
            assert!(
                (step[i][0] - native).abs() < 5e-6,
                "calibrated slew frame {i}"
            );
        }
        let mut knee = Maximizer::new(&node(&[("Knee", "6")]), 2, 48000.)?;
        let mut hot = vec![frame(0.75, 0.25); 2048];
        knee.process(&mut hot)?;
        assert!((hot[1024][0] - 0.959_382_8).abs() < 2e-7);
        for (rate, delay) in [(44100., 88), (48000., 96), (96000., 192)] {
            let mut effect = Maximizer::new(&node(&[]), 1, rate)?;
            let mut impulse = vec![frame(0., 0.); delay + 4];
            impulse[0][0] = 0.25;
            effect.process(&mut impulse)?;
            assert!(impulse[..delay].iter().all(|frame| frame[0] == 0.));
            assert!((impulse[delay][0] - 0.49310568).abs() < 1e-7);
        }
        let mut effect = Maximizer::new(&node(&[("Bypass", "1")]), 2, 192000.)?;
        let bytes = effect.memory_bytes();
        assert!(bytes < 1 << 20);
        let mut dry = vec![frame(0.25, 0.125); 128];
        effect.process(&mut dry)?;
        assert_eq!(dry[0], frame(0.25, 0.125));
        assert_eq!(bytes, effect.memory_bytes());
        assert!(
            effect
                .set_parameter("Threshold", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
        assert!(
            effect
                .set_parameter("Attack", &ParameterValue::Number(1.))
                .is_err()
        );
        assert!(validate(&node(&[("TruePeak", "1")])).is_err());
        assert!(Maximizer::new(&node(&[]), 3, 48000.).is_err());
        assert!(!effect.diagnostics().is_empty());
        Ok(())
    }
}
