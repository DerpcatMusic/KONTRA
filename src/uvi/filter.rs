//! Original Xpander constant-Q mathematics, measured against official UVI
//! Workstation 4.0.9 with authored impulses and DC signals on 2026-10-03.
//! Parameter ranges: https://lua.uvi.net/_elements.html#xpander-filter
//! Shape/topology: https://www.uvi.net/falcon and the official Falcon manual,
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf
//! Native numerical observations establish the 37 tap combinations, frequency
//! and resonance polynomials, rational saturation and two-phase allpass path.
//! Algorithm II uses bilinear TPT stages with a measured zero-delay solve.
//! AGM and Jacobi identities: https://dlmf.nist.gov/22.20 and 19.8.
//! No vendor source expression was used.

use super::{
    dsp::{Frame, MAX_CHANNELS},
    host::ParameterValue,
    program::ProgramNode,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI Xpander shapes, both solvers and saturation have authored native comparisons at 48 kHz; quiet rate paths are compared at 8/32/44.1/48/64/96/192 kHz; scalar cutoff transitions are compared at 32/48/96 kHz and dynamic cutoff points at 48 kHz, with native rounding residuals up to 3e-5; other live transitions, partial control-block timing, cross-rate saturation and exact float-rounding parity remain unverified";
const DEFAULTS: [(&str, f64); 10] = [
    ("Bypass", 0.),
    ("Freq", 1000.),
    ("Q", 0.),
    ("KeyTracking", 0.),
    ("Mode", 3.),
    ("DistortionType", 0.),
    ("Drive", 0.),
    ("Fat", 1.),
    ("Algorithm", 0.),
    ("Oversampling", 1.),
];

// Original elliptic halfband mathematics, confirmed by native common-path
// impulses at 8/32/40/44.1/48/64/96/192kHz. Coefficients depend on rate;
// arbitrary-rate tables or nearest-rate aliases would change the native phase.
fn elliptic_agm(m: f64) -> (f64, [(f64, f64); 16], usize) {
    let (mut a, mut b) = (1., (1. - m).sqrt());
    let mut levels = [(0., 0.); 16];
    let mut count = 0;
    for level in &mut levels {
        let next = (a + b) * 0.5;
        let c = (a - b) * 0.5;
        *level = (next, c);
        count += 1;
        b = (a * b).sqrt();
        a = next;
        if c < 1e-15 {
            break;
        }
    }
    (std::f64::consts::PI / (2. * a), levels, count)
}

pub(super) fn allpass_coefficients(rate: f64, attenuation_db: f64) -> ([f64; 8], usize) {
    let edge = (20_000. / rate).min(0.48);
    let k = (std::f64::consts::PI * edge * 0.5).tan().powi(2);
    let m = k * k;
    let (complete, levels, depth) = elliptic_agm(m);
    let log_nome = -std::f64::consts::PI * elliptic_agm(1. - m).0 / complete;
    let mut count = 2;
    while count < 8
        && -10. / 10f64.ln() * (4f64.ln() + (2 * count + 1) as f64 * log_nome * 0.5)
            < attenuation_db
    {
        count += 2;
    }
    let mut coefficients = [0.; 8];
    for (i, coefficient) in coefficients[..count].iter_mut().enumerate() {
        // Descending AGM gives the Jacobi amplitude at u=2*i*K/(2*M+1).
        let mut phi = (1usize << depth) as f64 * (i + 1) as f64 * std::f64::consts::PI
            / (2 * count + 1) as f64;
        for &(a, c) in levels[..depth].iter().rev() {
            phi = (phi + (c / a * phi.sin()).asin()) * 0.5;
        }
        let sn = phi.sin();
        let x = phi.cos() * (1. - m * sn * sn).sqrt() / (1. + k * sn * sn);
        *coefficient = (1. - x) / (1. + x);
    }
    (coefficients, count)
}

// Input, LP1, LP2, LP3, LP4. Each row was independently identified from an
// authored native impulse, including mixed/twin shapes; none is an alias.
const MIX: [[f64; 5]; 37] = [
    [0., 1., 0., 0., 0.],
    [0., 0., 1., 0., 0.],
    [0., 0., 0., 1., 0.],
    [0., 0., 0., 0., 1.],
    [1., -1., 0., 0., 0.],
    [1., -2., 1., 0., 0.],
    [1., -3., 3., -1., 0.],
    [1., -4., 6., -4., 1.],
    [0., 2., -2., 0., 0.],
    [0., 0., 4., -8., 4.],
    [1., -2., 2., 0., 0.],
    [1., -4., 8., -8., 4.],
    [1., -3., 6., -4., 0.],
    [0., 1., -3., 6., -4.],
    [-1., 2., 0., 0., 0.],
    [1., -4., 4., 0., 0.],
    [0., 0., 0., 1., -1.],
    [0., 1., -3., 3., -1.],
    [0., 1., -2., 1., 0.],
    [0., 0., 1., -1., 0.],
    [0., -1., 2., 0., 0.],
    [0., 0., 1., -2., 2.],
    [1., -4., 7., -6., 2.],
    [0., 4., -12., 16., -8.],
    [1., -1., 1., 0., 0.],
    [1., 0., -1., 2., -1.],
    [-1., 4., -5.25, 2.5, 0.],
    [0., 0., 4., -8., 5.],
    [1., -4., 6.25, -4.5, 1.25],
    [0., 2., -6., 6.5, -2.5],
    [-0.25, 1., -2.25, 2.5, 0.],
    [0., 0., 0.25, -0.5, 1.25],
    [1., -4., 10., -12., 5.],
    [0., 0.5, -1.5, 3.5, -2.5],
    [1., -4., 10.25, -12.5, 6.25],
    [0.25, -1., 2.75, -3.5, 2.5],
    [1., -4., 7.25, -6.5, 2.5],
];

pub fn supports(kind: &str) -> bool {
    kind == "XpanderFilter"
}

fn check(name: &str, value: f64) -> Result<()> {
    let (low, high, integer) = match name {
        "Bypass" | "Algorithm" | "Oversampling" => (0., 1., true),
        "Freq" => (20., 20_000., false),
        "Q" | "KeyTracking" | "Fat" => (0., 1., false),
        "Mode" => (0., 36., true),
        "DistortionType" => (0., 2., true),
        "Drive" => (-20., 20., false),
        _ => bail!("Unsupported XpanderFilter parameter {name}"),
    };
    ensure!(
        value.is_finite() && (low..=high).contains(&value) && (!integer || value.fract() == 0.),
        "Invalid XpanderFilter parameter {name}"
    );
    Ok(())
}

fn parameters(node: &ProgramNode) -> Result<BTreeMap<String, f64>> {
    ensure!(supports(&node.kind), "Unsupported UVI filter {}", node.kind);
    let mut p: BTreeMap<String, f64> = DEFAULTS
        .into_iter()
        .map(|(name, value)| (name.into(), value))
        .collect();
    for (name, text) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let value = text
            .parse()
            .with_context(|| format!("Invalid XpanderFilter parameter {name}"))?;
        check(name, value)?;
        p.insert(name.clone(), value);
    }
    Ok(p)
}

pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

#[derive(Clone, Copy, Default)]
struct State {
    // First-order allpasses y=a*x+z; z=x-a*y, independently per phase.
    up: [f64; 8],
    down: [f64; 8],
    // Scaled DFII memories, measured from three independent cutoff jumps.
    offsets: [f64; 4],
    feedback_output: f64,
}

pub(super) fn allpass(mut x: f64, state: &mut [f64; 8], phase: usize, coefficients: &[f64]) -> f64 {
    for i in (phase..coefficients.len()).step_by(2) {
        let y = coefficients[i] * x + state[i];
        state[i] = x - coefficients[i] * y;
        x = y;
    }
    x
}

fn rational(x: f64) -> f64 {
    x * (27. + x * x) / (27. + 9. * x * x)
}

pub struct XpanderFilter {
    channels: usize,
    rate: f64,
    parameters: BTreeMap<String, f64>,
    note: u8,
    phases: usize,
    allpass: [f64; 8],
    allpass_count: usize,
    stage: f64,
    pole: f64,
    feedback: f64,
    compensation: f64,
    denominator: f64,
    drive: f64,
    normalization: f64,
    feedback_gain: f64,
    state: [State; MAX_CHANNELS],
    control_phase: usize,
    frame: usize,
    block_frames: usize,
    settle_begin: Option<usize>,
    ramp: [f32; 5],
    increment: [f32; 5],
    processed: bool,
    dynamic_frequency: bool,
    future_log: f32,
    control_start: [f64; 5],
    control_end: [f64; 5],
    control_queue: [f64; 5],
}

impl XpanderFilter {
    /// Call set_note before processing a keygroup voice. Note 60 is UVI C3,
    /// the documented reference for the KeyTracking control.
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "UVI filter needs 1..12 channels"
        );
        ensure!(
            rate.is_finite() && (8_000. ..=192_000.).contains(&rate),
            "Invalid UVI filter rate"
        );
        let (allpass, allpass_count) = allpass_coefficients(rate, 80.);
        let mut result = Self {
            channels,
            rate,
            parameters: parameters(node)?,
            note: 60,
            phases: 1,
            allpass,
            allpass_count,
            stage: 0.,
            pole: 0.,
            feedback: 0.,
            compensation: 0.,
            denominator: 1.,
            drive: 1.,
            normalization: 1.,
            feedback_gain: 1.,
            state: [State::default(); MAX_CHANNELS],
            control_phase: 0,
            frame: 0,
            block_frames: 256,
            settle_begin: None,
            ramp: [0.; 5],
            increment: [0.; 5],
            processed: false,
            dynamic_frequency: false,
            future_log: 0.,
            control_start: [0.; 5],
            control_end: [0.; 5],
            control_queue: [0.; 5],
        };
        result.configure()?;
        result.reset_control();
        Ok(result)
    }

    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        self.parameters
            .get(name)
            .copied()
            .map(|value| {
                if name == "Bypass" {
                    ParameterValue::Boolean(value != 0.)
                } else {
                    ParameterValue::Number(value)
                }
            })
            .context("Unknown UVI filter parameter")
    }

    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        self.set_effective_parameter(name, value, false)
    }

    /// Dynamic sources supply raw cutoff points at the 32-frame control clock.
    /// Scalar controls (including CC and Constant) retain the cutoff's own RC.
    /// The renderer may call this every output frame; coefficient points are
    /// sampled here, so repeated values must not restart an in-flight ramp.
    pub fn set_effective_parameter(
        &mut self,
        name: &str,
        value: &ParameterValue,
        dynamic: bool,
    ) -> Result<()> {
        let value = match value {
            ParameterValue::Number(n) if name != "Bypass" => *n,
            ParameterValue::Boolean(b) if name == "Bypass" => f64::from(u8::from(*b)),
            _ => bail!("UVI filter Bypass requires Boolean; other parameters require Number"),
        };
        check(name, value)?;
        if name == "Freq" {
            self.dynamic_frequency = dynamic;
        }
        if self.parameters.get(name).copied() == Some(value) {
            return Ok(());
        }
        let previous = self
            .parameters
            .insert(name.into(), value)
            .expect("validated key");
        if self.processed && name == "Freq" {
            self.settle_begin = None;
            return Ok(());
        }
        // Bypass freezes the filter, resampler and cutoff clock together.
        if name == "Bypass" || name == "Mode" {
            return Ok(());
        }
        let frequency = if self.processed {
            f64::from(20. * 1000f32.powf(self.future_log))
        } else {
            self.parameters["Freq"]
        };
        if let Err(error) = self.configure_frequency(frequency) {
            self.parameters.insert(name.into(), previous);
            return Err(error);
        }
        self.reset_coefficients();
        if !self.processed {
            self.future_log = Self::frequency_position(self.parameters["Freq"]);
        }
        Ok(())
    }

    pub fn set_note(&mut self, note: u8) -> Result<()> {
        ensure!(note <= 127, "Invalid UVI filter MIDI note");
        if self.note == note {
            return Ok(());
        }
        let previous = self.note;
        self.note = note;
        if let Err(error) = self.configure() {
            self.note = previous;
            return Err(error);
        }
        self.reset_control();
        Ok(())
    }

    /// The host's processing block controls the final near-target ramp.
    /// Native authored probes cover 32 and 256 frames; playback uses 256.
    pub fn set_control_block_frames(&mut self, frames: usize) -> Result<()> {
        ensure!(
            matches!(frames, 32 | 256),
            "Unmeasured Xpander host block size"
        );
        ensure!(!self.processed, "Set Xpander host block before processing");
        self.block_frames = frames;
        Ok(())
    }

    fn frequency_position(frequency: f64) -> f32 {
        (frequency as f32 / 20.).ln() / 1000f32.ln()
    }

    fn coefficients(&self) -> [f64; 5] {
        [
            self.stage,
            self.pole,
            self.feedback,
            self.compensation,
            self.denominator,
        ]
    }

    fn reset_coefficients(&mut self) {
        self.control_start = self.coefficients();
        self.control_end = self.control_start;
        self.control_queue = self.control_start;
        self.ramp = self.control_start.map(|v| v as f32);
        self.increment = [0.; 5];
        self.settle_begin = None;
    }

    fn reset_control(&mut self) {
        self.future_log = Self::frequency_position(self.parameters["Freq"]);
        self.reset_coefficients();
    }

    fn control_tick(&mut self) -> Result<()> {
        if self.control_phase != 0 {
            return Ok(());
        }
        if let Some(begin) = self.settle_begin {
            if self.frame < begin + self.block_frames {
                return Ok(());
            }
            self.settle_begin = None;
            self.control_start = self.control_end;
            self.control_queue = self.control_end;
        }
        let target = Self::frequency_position(self.parameters["Freq"]);
        if self.dynamic_frequency {
            self.control_start = self.control_end;
            self.future_log = target;
            self.configure_frequency(self.parameters["Freq"])?;
            self.control_end = self.coefficients();
            self.control_queue = self.control_end;
            return Ok(());
        }
        let alpha = 1f32 - 0.33f32.powf(3200. / self.rate as f32);
        // Independently bracketed with native 5331/5332Hz transitions: the
        // threshold is the normalized increment, not an absolute Hz epsilon.
        if self.frame % self.block_frames == 0
            && target != self.future_log
            && ((target - self.future_log) * alpha).abs() < 1e-6
        {
            self.future_log = target;
            self.control_start = self.control_end;
            self.configure_frequency(self.parameters["Freq"])?;
            self.control_end = self.coefficients();
            self.control_queue = self.control_end;
            self.settle_begin = Some(self.frame);
            return Ok(());
        }
        self.control_start = self.control_end;
        self.control_end = self.control_queue;
        self.future_log += (target - self.future_log) * alpha;
        let frequency = if self.future_log == target {
            self.parameters["Freq"]
        } else {
            f64::from(20. * 1000f32.powf(self.future_log))
        };
        self.configure_frequency(frequency)?;
        self.control_queue = self.coefficients();
        Ok(())
    }
    fn configure(&mut self) -> Result<()> {
        self.configure_frequency(self.parameters["Freq"])
    }
    fn configure_frequency(&mut self, base_frequency: f64) -> Result<()> {
        let p = &self.parameters;
        // Native tracking extends below/above the stored 20..20000Hz range.
        // Each solver applies its own internal-rate ceiling after tracking.
        let frequency =
            base_frequency * ((f64::from(self.note) - 60.) * p["KeyTracking"] / 12.).exp2();
        self.phases = if p["Oversampling"] != 0. || (p["Algorithm"] == 0. && self.rate <= 48_000.) {
            2
        } else {
            1
        };
        let rate = self.rate * self.phases as f64;
        if p["Algorithm"] == 0. {
            // Algorithm I caps at internal Nyquist; Algorithm II at .499.
            let w = 2. * std::f64::consts::PI * frequency.min(0.5 * rate) / rate;
            let g = w * (0.9892 + w * (-0.4242 + w * (0.1381 - 0.0202 * w)));
            ensure!(
                g > 0. && g < 2.,
                "XpanderFilter cutoff exceeds the measured stable solver range at this sample rate"
            );
            self.stage = g / 1.3;
            self.pole = 1. - g;
            self.feedback = 4. * p["Q"] * (1.0029 + w * (0.0526 + w * (-0.0926 + 0.0218 * w)));
            self.denominator = 1.;
        } else {
            // Native positive 8/32kHz probes keep the stored Freq=20000,
            // while the effective cutoff is capped at .499*internalRate.
            let g = (std::f64::consts::PI * frequency.min(0.499 * rate) / rate).tan();
            self.stage = g / (1. + g);
            self.pole = 0.;
            self.feedback = 4. * p["Q"];
            self.denominator = 1. + self.feedback * self.stage.powi(4);
        }
        self.compensation = 1. + p["Fat"] * self.feedback;
        self.drive = 10f64.powf(p["Drive"] / 20.);
        match p["DistortionType"] as u8 {
            0 => {
                self.normalization = rational(self.drive);
                let d2 = self.drive * self.drive;
                self.feedback_gain = (27. + 9. * d2) / (27. + d2);
            }
            1 => {
                self.normalization = self.drive.min(1.);
                self.feedback_gain = self.drive / self.normalization;
            }
            2 => {
                self.drive = 1.;
                self.normalization = 1.;
                self.feedback_gain = 1.;
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    pub fn process(&mut self, io: &mut [Frame]) -> Result<()> {
        ensure!(
            io.iter()
                .flat_map(|f| &f[..self.channels])
                .all(|x| x.is_finite()),
            "Nonfinite UVI filter input"
        );
        if self.parameters["Bypass"] != 0. {
            return Ok(());
        }
        let mix = MIX[self.parameters["Mode"] as usize];
        let distortion = self.parameters["DistortionType"] as u8;
        let zero_delay = self.parameters["Algorithm"] != 0.;
        let phases = self.phases;
        let allpass_coeffs = self.allpass;
        let coefficients = &allpass_coeffs[..self.allpass_count];
        for frame in io {
            self.control_tick()?;
            self.processed = true;
            if self.control_phase == 0 && self.settle_begin.is_none_or(|begin| begin == self.frame)
            {
                self.ramp = self.control_start.map(|v| v as f32);
                let frames = if self.settle_begin.is_some() {
                    self.block_frames
                } else {
                    32
                };
                self.increment = std::array::from_fn(|i| {
                    (self.control_end[i] as f32 - self.ramp[i]) / (frames * phases) as f32
                });
            }
            // Native coefficient slopes accumulate in f32 at each internal
            // phase. Compute once per frame and reuse for every channel.
            let mut phase_coeffs = [[0.; 5]; 2];
            for c in &mut phase_coeffs[..phases] {
                *c = self.ramp.map(f64::from);
                for i in 0..5 {
                    self.ramp[i] += self.increment[i];
                }
            }
            for (channel, x) in frame[..self.channels].iter_mut().enumerate() {
                let state = &mut self.state[channel];
                let mut output = 0.;
                // Algorithm I forces 2x through 48 kHz, honors the toggle above;
                // Algorithm II honors the toggle at every measured rate.
                for phase in 0..phases {
                    let coeff = phase_coeffs[phase];
                    let [stage_gain, pole, feedback, compensation, denominator] = coeff;
                    let up = if phases == 2 {
                        allpass(f64::from(*x), &mut state.up, phase, coefficients)
                    } else {
                        f64::from(*x)
                    };
                    let offsets = state.offsets;
                    let feedback_offset = if zero_delay {
                        offsets
                            .iter()
                            .fold(0., |sum, offset| stage_gain * sum + offset)
                    } else {
                        state.feedback_output
                    };
                    let input = (up * compensation
                        - feedback * feedback_offset / self.feedback_gain)
                        * self.drive;
                    let mut tap = match distortion {
                        0 => rational(input) / self.normalization,
                        1 => input.clamp(-1., 1.) / self.normalization,
                        2 => input,
                        _ => unreachable!(),
                    } / denominator;
                    let mut result = mix[0] * tap;
                    for (stage, offset) in offsets.into_iter().enumerate() {
                        let y = stage_gain * tap + offset;
                        state.offsets[stage] = if zero_delay {
                            stage_gain * tap + (1. - 2. * stage_gain) * y
                        } else {
                            0.3 * stage_gain * tap + pole * y
                        };
                        tap = y;
                        result += mix[stage + 1] * y;
                    }
                    state.feedback_output = tap;
                    output += if phases == 2 {
                        allpass(result, &mut state.down, 1 - phase, coefficients)
                    } else {
                        result
                    };
                }
                *x = (output / phases as f64) as f32;
                ensure!(x.is_finite(), "Nonfinite UVI filter output");
            }
            self.control_phase = (self.control_phase + 1) % 32;
            self.frame += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::program::parse_program;
    use super::*;

    fn filter(attributes: &str, channels: usize) -> XpanderFilter {
        let p = parse_program(&format!(
            "<Program><Inserts><XpanderFilter {attributes}/></Inserts></Program>"
        ))
        .unwrap();
        XpanderFilter::new(&p.nodes[2], channels, 48000.).unwrap()
    }

    #[test]
    fn authored_native_xpander_shapes_saturation_and_fragmentation() {
        // Original native authored 32/32768 impulse, normalized to unity.
        // Independent output samples at frames 3, 7, 15 and 31. No bank assets.
        #[rustfmt::skip]
        const SHAPES: [[f32; 4]; 37] = [
            [7.367460430e-02, 8.607654274e-02, 2.080189995e-02, 3.559282981e-03, ],
            [7.381628267e-03, 4.266845435e-02, 4.140250012e-02, 1.180872042e-02, ],
            [6.666701520e-04, 1.317025535e-02, 3.402062133e-02, 2.167098597e-02, ],
            [5.604675971e-05, 2.891889773e-03, 1.899895817e-02, 2.687584981e-02, ],
            [5.446836352e-01, 9.556870908e-02, -9.997791797e-02, -3.517939523e-02, ],
            [4.783906639e-01, 5.216058716e-02, -7.937732339e-02, -2.692995779e-02, ],
            [4.188126624e-01, 3.825064003e-02, -5.139485747e-02, -2.854279242e-02, ],
            [3.653389812e-01, 4.356060922e-02, -3.105217218e-02, -3.481302783e-02, ],
            [1.325859278e-01, 8.681619167e-02, -4.120119661e-02, -1.649886928e-02, ],
            [2.441733889e-02, 7.687934488e-02, -3.055914864e-02, -1.862961799e-02, ],
            [4.857723117e-01, 9.482901543e-02, -3.797482327e-02, -1.512124389e-02, ],
            [3.776037395e-01, 8.489221334e-02, -2.733276971e-02, -1.725198328e-02, ],
            [4.389576018e-01, 1.267452836e-01, -2.924920619e-02, -5.812959746e-02, ],
            [5.530554429e-02, 2.552515455e-02, 2.472230978e-02, -9.344350547e-03, ],
            [-4.710090756e-01, -9.492128156e-03, 1.207797974e-01, 3.873867542e-02, ],
            [3.531863689e-01, 8.012813516e-03, 3.226355882e-03, 1.377628185e-03, ],
            [6.106234505e-04, 1.027836557e-02, 1.502166502e-02, -5.204863381e-03, ],
            [5.347367376e-02, -5.309940316e-03, -2.034268528e-02, 6.270238664e-03, ],
            [5.957800895e-02, 1.390989404e-02, -2.798247524e-02, 1.612832304e-03, ],
            [6.714958232e-03, 2.949820086e-02, 7.381877862e-03, -9.862268344e-03, ],
            [-5.891133845e-02, -7.396430010e-04, 6.200310215e-02, 2.005815320e-02, ],
            [6.160381716e-03, 2.211172320e-02, 1.135916915e-02, 2.221844532e-02, ],
            [3.714433312e-01, 6.278043985e-02, -3.869194537e-02, -3.947043791e-02, ],
            [2.163371891e-01, 1.987371594e-02, -2.128407732e-02, 4.261495080e-03, ],
            [5.520652533e-01, 1.382371336e-01, -5.857542902e-02, -2.337067574e-02, ],
            [6.122539043e-01, 1.624253839e-01, -7.153622061e-02, -2.696270309e-02, ],
            [-3.607467413e-01, -2.842273191e-02, 3.007207625e-02, 3.803894669e-02, ],
            [2.447338589e-02, 7.977122813e-02, -1.156018488e-02, 8.246230893e-03, ],
            [3.668650985e-01, 4.836562276e-02, -3.296208382e-02, -3.597737104e-02, ],
            [1.072526649e-01, -5.480698776e-03, -3.317454085e-02, 9.938044474e-03, ],
            [-9.585695714e-02, -2.241313271e-02, 3.249183670e-02, 3.907216340e-02, ],
            [1.582130441e-03, 7.696848363e-03, 1.708901115e-02, 2.571150102e-02, ],
            [3.897563517e-01, 1.204399914e-01, -6.161129102e-02, -5.344264954e-02, ],
            [2.795808390e-02, 1.790175773e-02, 1.987198554e-02, -7.274606265e-03, ],
            [3.913384974e-01, 1.281368285e-01, -4.452225938e-02, -2.773113176e-02, ],
            [9.902121872e-02, 3.780684248e-02, 1.686188974e-03, 1.235083397e-02, ],
            [3.729834259e-01, 6.830838323e-02, -3.585213050e-02, -3.391582146e-02, ],
        ];
        for (mode, expected) in SHAPES.iter().enumerate() {
            let mut fx = filter(&format!("Mode=\"{mode}\" DistortionType=\"2\""), 2);
            let mut io = [[0.; MAX_CHANNELS]; 128];
            io[0][0] = 1. / 1024.;
            io[0][1] = -1. / 2048.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [3, 7, 15, 31].into_iter().zip(expected) {
                assert!(
                    (io[index][0] * 1024. - expected).abs() < 1e-6,
                    "mode {mode}, frame {index}"
                );
                assert!((io[index][1] * 2048. + expected).abs() < 1e-6);
            }
        }
        // The frequency and compensation controls were separately measured;
        // a single cutoff cannot distinguish their polynomial coefficients.
        #[rustfmt::skip]
        const FREQUENCIES: [(f64, [f32; 3]); 5] = [
            (100., [5.109531571e-07, 7.757826097e-05, 1.812436385e-03]),
            (500., [2.478336392e-04, 1.090902276e-02, 1.743830740e-03]),
            (5000., [1.475795209e-01, 1.416064060e-05, -3.334358345e-09]),
            (10000., [8.135783672e-02, 5.588732311e-04, 1.067719069e-08]),
            (20000., [5.733842775e-02, 6.801758893e-03, 1.000533189e-06]),
        ];
        for (freq, expected) in FREQUENCIES {
            let mut fx = filter(&format!("Freq=\"{freq}\" DistortionType=\"2\""), 1);
            let mut io = [[0.; MAX_CHANNELS]; 256];
            io[0][0] = 1. / 1024.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [7, 31, 127].into_iter().zip(expected) {
                assert!(
                    (io[index][0] * 1024. - expected).abs() < 1e-6,
                    "freq {freq}, frame {index}"
                );
            }
        }
        #[rustfmt::skip]
        const COMPENSATION: [(f64, f64, f32); 9] = [
            (0.25, 0., 2.064287104e-02),
            (0.25, 0.5, 3.102572635e-02),
            (0.25, 1., 4.140860587e-02),
            (0.5, 0., 1.473129075e-02),
            (0.5, 0.5, 2.955027297e-02),
            (0.5, 1., 4.436921328e-02),
            (0.75, 0., 9.134986438e-03),
            (0.75, 0.5, 2.291902341e-02),
            (0.75, 1., 3.670306504e-02),
        ];
        for (q, fat, expected) in COMPENSATION {
            let mut fx = filter(&format!("Q=\"{q}\" Fat=\"{fat}\" DistortionType=\"2\""), 1);
            let mut io = [[0.; MAX_CHANNELS]; 128];
            io[0][0] = 1. / 1024.;
            fx.process(&mut io).unwrap();
            assert!(
                (io[31][0] * 1024. - expected).abs() < 1e-6,
                "Q {q}, Fat {fat}"
            );
        }
        // Large impulse (32767/32768), Q=.75, three distinct shapers;
        // values are native outputs normalized to remove the mono pan gain.
        #[rustfmt::skip]
        const SATURATION: [[f32; 4]; 3] = [
            [1.893573499e-04, 7.173242047e-03, 4.259800911e-02, 2.153839171e-02, ],
            [1.617650269e-04, 5.923940800e-03, 3.397165239e-02, 1.485211961e-02, ],
            [2.251745755e-04, 1.161030121e-02, 7.414852083e-02, 3.670186177e-02, ],
        ];
        for (distortion, expected) in SATURATION.iter().enumerate() {
            let attributes = format!("Q=\"0.75\" DistortionType=\"{distortion}\"");
            let mut whole = filter(&attributes, 1);
            let mut split = filter(&attributes, 1);
            let mut a = [[0.; MAX_CHANNELS]; 128];
            a[0][0] = 32767. / 32768.;
            let mut b = a;
            whole.process(&mut a).unwrap();
            for chunk in b.chunks_mut(7) {
                split.process(chunk).unwrap();
            }
            assert_eq!(a, b);
            for (index, expected) in [3, 7, 15, 31].into_iter().zip(expected) {
                assert!(
                    (a[index][0] - expected).abs() < 1e-6,
                    "distortion {distortion}, frame {index}"
                );
            }
        }
        // Original DC .125, native Drive sweep establishes soft normalization,
        // hardclip's zero-dB lower bound and Linear's ignored drive control.
        #[rustfmt::skip]
        const DRIVES: [(f64, [f32; 3]); 8] = [
            (-20., [1.253645122e-01, 1.250000298e-01, 1.250000298e-01]),
            (-12., [1.272943169e-01, 1.250000000e-01, 1.250000000e-01]),
            (-6., [1.340616643e-01, 1.250000149e-01, 1.250000149e-01]),
            (0., [1.599741876e-01, 1.250000149e-01, 1.250000894e-01]),
            (6., [2.489228100e-01, 2.494079918e-01, 1.250002384e-01]),
            (12., [4.613366425e-01, 4.976340234e-01, 1.250006109e-01]),
            (18., [6.438658834e-01, 9.929110408e-01, 1.250023097e-01]),
            (20., [6.346541047e-01, 1.000000119e+00, 1.250037104e-01]),
        ];
        for (drive, expected) in DRIVES {
            for (distortion, expected) in expected.into_iter().enumerate() {
                let mut fx = filter(
                    &format!("Mode=\"0\" Drive=\"{drive}\" DistortionType=\"{distortion}\""),
                    1,
                );
                let mut io = [[0.; MAX_CHANNELS]; 4096];
                for frame in &mut io {
                    frame[0] = 0.125;
                }
                fx.process(&mut io).unwrap();
                assert!(
                    (io[4095][0] - expected).abs() < 5e-6,
                    "drive {drive}, distortion {distortion}"
                );
            }
        }
        // Independent Algorithm II probes distinguish the TPT solver, its
        // nonlinear solve order and both states of the oversampling toggle.
        #[rustfmt::skip]
        const ZERO_DELAY: [(f64, u8, u8, [f32; 4]); 24] = [
            (1000., 0, 0, [1.851358320e-05, 1.282612560e-03, 8.696346544e-03, 2.833032981e-02, ], ),
            (1000., 0, 1, [1.431576129e-05, 9.917893913e-04, 6.724512205e-03, 2.190632559e-02, ], ),
            (1000., 0, 2, [5.726105519e-05, 3.967019264e-03, 2.689711377e-02, 8.762222528e-02, ], ),
            (1000., 1, 0, [6.092974569e-09, 8.404527034e-05, 5.658171140e-03, 4.102279618e-02, ], ),
            (1000., 1, 1, [4.781889995e-09, 7.184060814e-05, 4.690510686e-03, 3.279198706e-02, ], ),
            (1000., 1, 2, [4.781891327e-09, 9.442205192e-05, 8.994814008e-03, 7.108838111e-02, ], ),
            (5000., 0, 0, [5.269241985e-03, 1.334217191e-01, -4.037205875e-02, 7.876557112e-02, ], ),
            (5000., 0, 1, [4.074478988e-03, 1.030734479e-01, -3.394839168e-02, 6.143784150e-02, ], ),
            (5000., 0, 2, [1.629734971e-02, 4.122793972e-01, -1.357887238e-01, 2.457427979e-01, ], ),
            (5000., 1, 0, [2.333658358e-06, 2.141584642e-02, 2.914600670e-01, -9.739783593e-03, ], ),
            (5000., 1, 1, [1.832091698e-06, 1.835690066e-02, 2.377878129e-01, 1.574965753e-02, ], ),
            (5000., 1, 2, [1.832092380e-06, 2.497757599e-02, 4.550974667e-01, 1.311173886e-01, ], ),
            (10000., 0, 0, [4.152768850e-02, 7.396913320e-02, 1.617197990e-01, -8.176570386e-02, ], ),
            (10000., 0, 1, [3.211158887e-02, 4.463705793e-02, 1.361468136e-01, -6.739572436e-02, ], ),
            (10000., 0, 2, [1.284418702e-01, 1.785428226e-01, 5.445675254e-01, -2.695733011e-01, ], ),
            (10000., 1, 0, [2.255894105e-05, 1.340845525e-01, -1.284386814e-01, 3.184337169e-02, ], ),
            (10000., 1, 1, [1.771341522e-05, 1.157507971e-01, -1.170769930e-01, 5.810539052e-02, ], ),
            (10000., 1, 2, [1.771342068e-05, 1.562394500e-01, -7.318277955e-01, 4.000999928e-01, ], ),
            (20000., 0, 0, [2.315673828e-01, 5.392458662e-02, -9.113463014e-02, -1.067782100e-02, ], ),
            (20000., 0, 1, [1.790611595e-01, 7.030776143e-02, -8.976514637e-02, 9.141474962e-03, ], ),
            (20000., 0, 2, [7.162196636e-01, 2.812186480e-01, -3.590466976e-01, 3.656211495e-02, ], ),
            (20000., 1, 0, [1.623057469e-04, 4.514210224e-01, 6.777848303e-02, 1.481978297e-01, ], ),
            (20000., 1, 1, [1.273745875e-04, 3.878555894e-01, 4.268448055e-02, 1.396763772e-01, ], ),
            (20000., 1, 2, [1.273746311e-04, 4.596724212e-01, 6.881806850e-01, 7.403402925e-01, ], ),
        ];
        for (freq, oversampling, distortion, expected) in ZERO_DELAY {
            let mut fx = filter(
                &format!(
                    "Algorithm=\"1\" Oversampling=\"{oversampling}\" Q=\"0.75\" Freq=\"{freq}\" DistortionType=\"{distortion}\""
                ),
                1,
            );
            let mut io = [[0.; MAX_CHANNELS]; 128];
            io[0][0] = 32767. / 32768.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [0, 3, 7, 15].into_iter().zip(expected) {
                assert!(
                    (io[index][0] - expected).abs() < 2e-5,
                    "Algorithm II freq {freq}, sampling {oversampling}, distortion {distortion}, frame {index}"
                );
            }
        }
        #[rustfmt::skip]
        const ZERO_DELAY_SHAPES: [(u8, [f32; 4]); 6] = [
            (0, [6.150990352e-02, 8.879322559e-02, 5.252085254e-02, 1.837535016e-02, ], ),
            (4, [9.384596348e-01, -8.879322559e-02, -5.252085254e-02, -1.837535016e-02, ], ),
            (8, [1.154526398e-01, 1.089797243e-01, 9.145069867e-03, -3.550702333e-02, ], ),
            (13, [5.149829760e-02, 2.308242023e-02, 1.312654931e-02, 2.991638519e-02, ], ),
            (26, [-7.732119560e-01, 1.922334433e-01, 1.312138606e-02, -2.757279761e-02, ], ),
            (36, [7.798839808e-01, -1.485915929e-01, 1.201692689e-02, 1.601552777e-02, ], ),
        ];
        for (mode, expected) in ZERO_DELAY_SHAPES {
            let mut fx = filter(
                &format!("Algorithm=\"1\" Oversampling=\"0\" Mode=\"{mode}\" DistortionType=\"2\""),
                1,
            );
            let mut io = [[0.; MAX_CHANNELS]; 128];
            io[0][0] = 32767. / 32768.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [0, 3, 7, 15].into_iter().zip(expected) {
                assert!(
                    (io[index][0] - expected).abs() < 2e-6,
                    "Algorithm II mode {mode}, frame {index}"
                );
            }
        }
        // Coupled Drive/Q probes distinguish negative hardclip normalization
        // from clamping at unity; quiet DC alone cannot distinguish them.
        #[rustfmt::skip]
        const DRIVE_FEEDBACK: [(u8, u8, f64, u8, [f32; 2]); 24] = [
            (0, 0, -20., 0, [7.291509211e-02, 3.605150059e-02]),
            (0, 0, 6., 0, [4.310429469e-02, 2.668718994e-02]),
            (0, 0, 20., 0, [7.922478765e-02, 8.400911093e-02]),
            (0, 0, -20., 1, [7.414863259e-02, 3.670191392e-02]),
            (0, 0, -12., 1, [7.414865494e-02, 3.670194373e-02]),
            (0, 0, -6., 1, [5.681229755e-02, 2.662664466e-02]),
            (0, 0, 6., 1, [4.325014353e-02, 2.233894356e-02]),
            (0, 0, 20., 1, [5.786622316e-02, 6.228123233e-02]),
            (1, 0, -20., 0, [8.392719924e-02, 1.238979492e-02]),
            (1, 0, 6., 0, [2.684996836e-02, 3.997239750e-03]),
            (1, 0, 20., 0, [7.214827091e-02, 1.200974826e-02]),
            (1, 0, -20., 1, [8.762261271e-02, 1.292310748e-02]),
            (1, 0, -12., 1, [8.721065521e-02, 1.286235545e-02]),
            (1, 0, -6., 1, [4.370886832e-02, 6.446448620e-03]),
            (1, 0, 6., 1, [2.190632559e-02, 3.230877686e-03]),
            (1, 0, 20., 1, [2.190632559e-02, 3.230877686e-03]),
            (1, 1, -20., 0, [6.992179900e-02, 4.005253315e-02]),
            (1, 1, 6., 0, [4.139041528e-02, 2.902526967e-02]),
            (1, 1, 20., 0, [7.568947226e-02, 8.733263612e-02]),
            (1, 1, -20., 1, [7.108850032e-02, 4.076562077e-02]),
            (1, 1, -12., 1, [7.108853757e-02, 4.076564312e-02]),
            (1, 1, -6., 1, [5.477229133e-02, 2.985592745e-02]),
            (1, 1, 6., 1, [4.153827205e-02, 2.470047027e-02]),
            (1, 1, 20., 1, [5.537178367e-02, 6.487253308e-02]),
        ];
        for (algorithm, oversampling, drive, distortion, expected) in DRIVE_FEEDBACK {
            let mut fx = filter(
                &format!(
                    "Algorithm=\"{algorithm}\" Oversampling=\"{oversampling}\" Q=\"0.75\" Drive=\"{drive}\" DistortionType=\"{distortion}\""
                ),
                1,
            );
            let mut io = [[0.; MAX_CHANNELS]; 128];
            io[0][0] = 32767. / 32768.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [15, 31].into_iter().zip(expected) {
                assert!(
                    (io[index][0] - expected).abs() < 1e-6,
                    "coupled drive {drive}, distortion {distortion}, Algorithm {algorithm}, sampling {oversampling}, frame {index}"
                );
            }
        }
        // Original large delta/Q=.75 probes verify the input tap belongs to
        // the feedback-shaped signal in highpass, allpass and mixed shapes.
        #[rustfmt::skip]
        const MIXED_FEEDBACK: [(u8, u8, u8, [f32; 4]); 18] = [
            (0, 0, 4, [3.748537274e-03, 1.182188034e+00, -1.303091496e-01, 6.289122254e-02]),
            (0, 0, 14, [-3.540632082e-03, -9.857723117e-01, 2.193879187e-01, 1.818162575e-02]),
            (0, 0, 36, [3.199949628e-03, 7.381969690e-01, -2.821130119e-02, 2.906479500e-02]),
            (0, 1, 4, [2.955713309e-03, 9.682009816e-01, -7.691083848e-02, 4.689553753e-02]),
            (0, 1, 14, [-2.791957464e-03, -8.024293184e-01, 1.452713311e-01, 1.917634159e-02]),
            (0, 1, 36, [2.523570089e-03, 5.945191383e-01, -4.090666771e-04, 2.370541170e-02]),
            (0, 2, 4, [2.955714474e-03, 2.188153028e+00, -3.495830894e-01, 8.762288094e-02]),
            (0, 2, 14, [-2.791958395e-03, -1.892167211e+00, 4.818724394e-01, 5.905896425e-02]),
            (0, 2, 36, [2.523571020e-03, 1.498355269e+00, -8.838701248e-02, 2.690687217e-02]),
            (1, 0, 4, [3.797834506e-03, 1.201423645e+00, -1.328774393e-01, 6.180604175e-02]),
            (1, 0, 14, [-3.656357760e-03, -1.025218844e+00, 2.238986194e-01, 1.929570176e-02]),
            (1, 0, 36, [3.408608260e-03, 7.821110487e-01, -2.934472635e-02, 3.098369017e-02]),
            (1, 1, 4, [2.994230017e-03, 9.845187664e-01, -7.835574448e-02, 4.598508403e-02]),
            (1, 1, 14, [-2.882864326e-03, -8.351407051e-01, 1.478171051e-01, 2.011217922e-02]),
            (1, 1, 36, [2.687812317e-03, 6.297965050e-01, 3.408193588e-04, 2.531714551e-02]),
            (1, 2, 4, [2.994230948e-03, 2.215538263e+00, -3.545993567e-01, 8.512677252e-02]),
            (1, 2, 14, [-2.882865258e-03, -1.958007097e+00, 4.919820428e-01, 6.152367592e-02]),
            (1, 2, 36, [2.687813248e-03, 1.585525870e+00, -9.091910720e-02, 2.851120941e-02]),
        ];
        for (algorithm, distortion, mode, expected) in MIXED_FEEDBACK {
            let mut fx = filter(
                &format!(
                    "Algorithm=\"{algorithm}\" Oversampling=\"1\" Q=\"0.75\" DistortionType=\"{distortion}\" Mode=\"{mode}\""
                ),
                1,
            );
            let mut io = [[0.; MAX_CHANNELS]; 128];
            io[0][0] = 32767. / 32768.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [0, 3, 12, 50].into_iter().zip(expected) {
                assert!(
                    (io[index][0] - expected).abs() < 2e-6 * (1. + expected.abs()),
                    "mixed feedback Algorithm {algorithm}, distortion {distortion}, mode {mode}, frame {index}"
                );
            }
        }
        // The native direct Algorithm II path caps cutoff at .499*rate.
        // The public control remains 20000 at both measured low rates.
        const LOW_RATE_CUTOFF: [f32; 4] = [
            9.875326157e-01,
            2.474094182e-02,
            2.248827368e-02,
            1.974010840e-02,
        ];
        for rate in [8000., 32000.] {
            let p = parse_program("<Program><Inserts><XpanderFilter Algorithm=\"1\" Oversampling=\"0\" Freq=\"20000\" DistortionType=\"2\"/></Inserts></Program>").unwrap();
            let mut fx = XpanderFilter::new(&p.nodes[2], 1, rate).unwrap();
            assert_eq!(
                fx.parameter("Freq").unwrap(),
                ParameterValue::Number(20000.)
            );
            let mut io = [[0.; MAX_CHANNELS]; 32];
            io[0][0] = 1. / 1024.;
            fx.process(&mut io).unwrap();
            for (index, expected) in [0, 1, 7, 15].into_iter().zip(LOW_RATE_CUTOFF) {
                assert!(
                    (io[index][0] * 1024. - expected).abs() < 2e-6,
                    "low-rate cutoff {rate}, frame {index}"
                );
            }
        }
        // An original 7Hz PCM16 sine and native controller toggles at frames
        // 8192/16384 established transparent bypass with frozen filter state.
        #[rustfmt::skip]
        const BYPASS_RESUME: [(u8, u8, [f32; 4]); 3] = [
            (0, 0, [1.471140832e-01, 1.483296156e-01, 1.497111619e-01, 1.524470150e-01]),
            (1, 0, [1.487162411e-01, 1.499252319e-01, 1.512991041e-01, 1.540183872e-01]),
            (1, 1, [1.482522786e-01, 1.494636089e-01, 1.508403420e-01, 1.535664499e-01]),
        ];
        for (algorithm, oversampling, expected) in BYPASS_RESUME {
            let mut fx = filter(
                &format!(
                    "Freq=\"50\" Algorithm=\"{algorithm}\" Oversampling=\"{oversampling}\" DistortionType=\"2\""
                ),
                1,
            );
            let mut io: Vec<Frame> = (0..16448)
                .map(|i| {
                    let mut frame = [0.; MAX_CHANNELS];
                    frame[0] = ((0.25
                        * (2. * std::f64::consts::PI * 7. * i as f64 / 48000.).sin()
                        * 32768.)
                        .round()
                        / 32768.) as f32;
                    frame
                })
                .collect();
            fx.process(&mut io[..8192]).unwrap();
            fx.set_parameter("Bypass", &ParameterValue::Boolean(true))
                .unwrap();
            let dry = io[8192..16384].to_vec();
            fx.process(&mut io[8192..16384]).unwrap();
            assert_eq!(&io[8192..16384], dry);
            fx.set_parameter("Bypass", &ParameterValue::Boolean(false))
                .unwrap();
            fx.process(&mut io[16384..]).unwrap();
            for (index, expected) in [0, 7, 15, 31].into_iter().zip(expected) {
                assert!(
                    (io[16384 + index][0] - expected).abs() < 1e-5,
                    "bypass resume Algorithm {algorithm}, sampling {oversampling}, frame {index}"
                );
            }
        }
        // Original matching-rate PCM16 delta32 measurements; these distinguish
        // low-rate section count, Algorithm I cutoff cap and high-rate direct path.
        #[rustfmt::skip]
        const RATE_IMPULSE: [(f64, u8, u8, f64, [f32; 4]); 18] = [
            (8000., 0, 0, 1000., [7.445945812e-06, 2.819475718e-02, 1.595221162e-01, -1.066613913e-04]),
            (8000., 0, 0, 20000., [1.132947742e-03, 5.956187248e-01, 2.204459459e-01, 4.343529791e-02]),
            (8000., 1, 0, 1000., [7.359317038e-03, 1.635637283e-01, 8.527667820e-02, 6.241804851e-09]),
            (8000., 1, 0, 20000., [9.875326157e-01, 2.397099324e-02, 2.248827368e-02, 1.502786856e-02]),
            (8000., 1, 1, 1000., [1.568311177e-06, 1.313836966e-02, 1.751604676e-01, 1.132875113e-04]),
            (8000., 1, 1, 20000., [1.347887097e-03, 6.209264994e-01, 2.012058944e-01, 5.662286654e-02]),
            (32000., 0, 0, 1000., [4.941405862e-08, 3.051189706e-04, 1.112581044e-02, 2.060638741e-02]),
            (32000., 0, 0, 20000., [6.293460028e-04, 4.956775308e-01, 2.058652490e-01, 6.312857848e-03]),
            (32000., 1, 0, 1000., [6.462593592e-05, 3.986877389e-03, 2.148096263e-02, 1.669347845e-02]),
            (32000., 1, 0, 20000., [9.875326157e-01, 2.397099324e-02, 2.248827368e-02, 1.502786856e-02]),
            (32000., 1, 1, 1000., [1.043744824e-08, 1.330382947e-04, 8.930972777e-03, 2.145482600e-02]),
            (32000., 1, 1, 20000., [2.197859139e-04, 3.813278377e-01, 1.445104033e-01, -3.011485934e-02]),
            (96000., 0, 0, 1000., [5.503025022e-06, 1.602671255e-04, 9.313444607e-04, 1.220588200e-02]),
            (96000., 0, 0, 20000., [1.571040899e-01, 1.595209390e-01, 2.424335806e-03, 3.943208496e-19]),
            (96000., 1, 0, 1000., [1.009663379e-06, 7.875332085e-05, 6.754809292e-04, 1.197878085e-02]),
            (96000., 1, 0, 20000., [3.553483263e-02, 2.806751728e-01, 3.653521650e-03, 4.168694938e-22]),
            (96000., 1, 1, 1000., [1.651198622e-10, 3.320537189e-06, 2.414509217e-04, 1.106418855e-02]),
            (96000., 1, 1, 20000., [9.332592526e-06, 7.965664566e-02, 9.594684839e-02, -9.640034477e-06]),
        ];
        for (rate, algorithm, oversampling, frequency, expected) in RATE_IMPULSE {
            let p = parse_program(&format!(
                "<Program><Inserts><XpanderFilter Freq=\"{frequency}\" Algorithm=\"{algorithm}\" Oversampling=\"{oversampling}\" DistortionType=\"2\"/></Inserts></Program>"
            )).unwrap();
            let mut fx = XpanderFilter::new(&p.nodes[2], 1, rate).unwrap();
            let mut io = vec![[0.; MAX_CHANNELS]; 128];
            io[0][0] = 1.;
            fx.process(&mut io).unwrap();
            for (i, expected) in [0, 3, 7, 31].into_iter().zip(expected) {
                assert!(
                    (io[i][0] - expected).abs() < 2e-6,
                    "rate {rate}, Algorithm {algorithm}, sampling {oversampling}, Freq {frequency}, frame {i}"
                );
            }
        }
        // Original quiet native LP1 controls establish tracked frequencies of
        // 10Hz, 40kHz and the solver ceiling, beyond the stored control range.
        #[rustfmt::skip]
        const TRACKED_IMPULSE: [(u8, u8, f64, f64, [f32; 4]); 12] = [
            (0, 48, 1., 20., [4.206134747e-07, 7.995342021e-04, 1.415344188e-03, 1.251988579e-03]),
            (0, 60, 0., 20., [8.409569432e-07, 1.597714610e-03, 2.816081513e-03, 2.413784154e-03]),
            (0, 72, 1., 10000., [5.047720042e-04, 5.301831365e-01, 2.042247206e-01, -9.929586202e-03]),
            (0, 72, 1., 20000., [7.278415142e-04, 6.075282693e-01, 1.927149743e-01, -2.786644362e-02]),
            (0, 60, 0., 20000., [5.047720042e-04, 5.301831365e-01, 2.042247206e-01, -9.929586202e-03]),
            (0, 84, 1., 20000., [7.410088438e-04, 6.106308699e-01, 1.898769885e-01, -2.891639620e-02]),
            (1, 48, 1., 20., [6.540708127e-04, 1.303868019e-03, 1.297058887e-03, 1.256943797e-03]),
            (1, 60, 0., 20., [1.307286904e-03, 2.597519662e-03, 2.570460783e-03, 2.413924318e-03]),
            (1, 72, 1., 10000., [7.886754274e-01, 1.111111492e-01, 1.234568190e-02, 2.323056414e-08]),
            (1, 72, 1., 20000., [9.968687296e-01, 6.165740546e-03, 6.012714002e-03, 5.171092693e-03]),
            (1, 60, 0., 20000., [7.886754274e-01, 1.111111492e-01, 1.234568190e-02, 2.323056414e-08]),
            (1, 84, 1., 20000., [9.968687296e-01, 6.165740546e-03, 6.012714002e-03, 5.171092693e-03]),
        ];
        for (algorithm, note, tracking, frequency, expected) in TRACKED_IMPULSE {
            let mut fx = filter(
                &format!(
                    "Mode=\"0\" Freq=\"{frequency}\" Algorithm=\"{algorithm}\" Oversampling=\"0\" KeyTracking=\"{tracking}\" DistortionType=\"2\""
                ),
                1,
            );
            fx.set_note(note).unwrap();
            let mut io = vec![[0.; MAX_CHANNELS]; 128];
            io[0][0] = 1.;
            fx.process(&mut io).unwrap();
            for (i, expected) in [0, 3, 7, 31].into_iter().zip(expected) {
                assert!(
                    (io[i][0] - expected).abs() < 2e-6,
                    "tracking Algorithm {algorithm}, note {note}, Freq {frequency}, frame {i}"
                );
            }
        }
        // Native LP1+HP1 is dry above 48 kHz with Oversampling0.
        for rate in [48001., 64000., 96000.] {
            let mut sum = vec![[0.; MAX_CHANNELS]; 32];
            for mode in [0, 4] {
                let p = parse_program(&format!(
                    "<Program><Inserts><XpanderFilter Mode=\"{mode}\" Oversampling=\"0\" DistortionType=\"2\"/></Inserts></Program>"
                )).unwrap();
                let mut fx = XpanderFilter::new(&p.nodes[2], 1, rate).unwrap();
                let mut io = vec![[0.; MAX_CHANNELS]; 32];
                io[0][0] = 1.;
                fx.process(&mut io).unwrap();
                for (out, value) in sum.iter_mut().zip(io) {
                    out[0] += value[0];
                }
            }
            assert!((sum[0][0] - 1.).abs() < 1e-7);
            assert!(sum[1..].iter().all(|frame| frame[0].abs() < 1e-7));
        }
        let mut fx = filter("DistortionType=\"2\"", 1);
        let previous = fx.parameter("Algorithm").unwrap();
        assert!(
            fx.set_parameter("Algorithm", &ParameterValue::Number(2.))
                .is_err()
        );
        assert_eq!(fx.parameter("Algorithm").unwrap(), previous);
        assert!(
            fx.set_parameter("Algorithm", &ParameterValue::Boolean(true))
                .is_err()
        );
        assert!(
            fx.set_parameter("Bypass", &ParameterValue::Number(1.))
                .is_err()
        );
        fx.set_parameter("Bypass", &ParameterValue::Boolean(true))
            .unwrap();
        assert_eq!(
            fx.parameter("Bypass").unwrap(),
            ParameterValue::Boolean(true)
        );
        let mut io = [[1.; MAX_CHANNELS]];
        fx.process(&mut io).unwrap();
        assert_eq!(io, [[1.; MAX_CHANNELS]]);
        io[0][0] = f32::NAN;
        assert!(fx.process(&mut io).is_err());
        fx.set_parameter("Freq", &ParameterValue::Number(1000.))
            .unwrap();
        fx.set_parameter("KeyTracking", &ParameterValue::Number(1.))
            .unwrap();
        let reference = fx.stage;
        fx.set_note(72).unwrap();
        assert!(fx.stage > reference);
        let p = parse_program("<Program><Inserts><XpanderFilter/></Inserts></Program>").unwrap();
        let mut low_rate = XpanderFilter::new(&p.nodes[2], 1, 8000.).unwrap();
        low_rate
            .set_parameter("Freq", &ParameterValue::Number(20000.))
            .unwrap();
        assert_eq!(
            low_rate.parameter("Freq").unwrap(),
            ParameterValue::Number(20000.)
        );
    }
    fn authored_sine(frame: usize, rate: f64) -> f32 {
        ((0.25 * (std::f64::consts::TAU * 1379. * frame as f64 / rate).sin() * 32768.).round()
            / 32768.) as f32
    }

    #[test]
    fn authored_native_cutoff_scalar_clock_and_settling() {
        // Original PCM16 1379Hz sine, direct API cutoff 1000->5000 at
        // frame8192 and back at16384. Native observations include both solvers,
        // both AlgorithmII rate paths, three rates and resonant soft shaping.
        // Static gain startup is excluded. Remaining native f32 rounding is
        // bounded, including the independently observed final host-block ramp.
        const POINTS: [usize; 17] = [
            1024, 8192, 8223, 8224, 8256, 8320, 8960, 10000, 11264, 12544, 12672, 16128, 16384,
            16416, 16512, 17408, 21000,
        ];
        #[rustfmt::skip]
        const CASES: [(f64, u8, u8, bool, [f32; 17]); 8] = [
            (48000., 0, 0, false, [-2.914619818e-02, -2.673291601e-02, -1.328595262e-02, -1.772941090e-02, -1.376130339e-02, 1.330240909e-02, 1.372344047e-01, 7.283777744e-02, 1.550025940e-01, 1.723558754e-01, -1.936173588e-01, 1.399706453e-01, 4.520146176e-02, 1.423129439e-01, 1.005557925e-01, -8.294066065e-04, -2.335441485e-02]),
            (48000., 1, 0, false, [-2.695612982e-02, -2.958914638e-02, -2.267988957e-02, -2.572439052e-02, -2.562059276e-02, -9.522412904e-03, 1.812171340e-01, 1.491718292e-01, 8.118621260e-02, 2.119849175e-01, -1.387775838e-01, 1.960761845e-01, -4.459050298e-02, 6.455665082e-02, 1.581518054e-01, -1.822778024e-02, -2.873082086e-02]),
            (48000., 1, 1, false, [-2.961795777e-02, -2.611744963e-02, -1.126209367e-02, -1.600690931e-02, -1.108972263e-02, 1.818878762e-02, 1.236543655e-01, 5.327572301e-02, 1.685453206e-01, 1.590399146e-01, -2.018356174e-01, 1.235791296e-01, 6.506456435e-02, 1.570720375e-01, 8.476133645e-02, 2.936375095e-03, -2.219420299e-02]),
            (32000., 1, 0, false, [-4.891168326e-03, 1.377853751e-02, -2.942547016e-02, -2.793532610e-02, 4.051455855e-02, 3.711301088e-02, -8.123973012e-02, -2.168188095e-01, 2.204321772e-01, 1.191123277e-01, -9.990086406e-02, -1.723491997e-01, -1.414272040e-01, 2.190142125e-01, 1.681780666e-01, -1.818583533e-02, 2.182248794e-02]),
            (96000., 1, 0, false, [1.871669851e-02, 1.330250967e-02, -3.587396117e-03, -6.227198523e-03, 1.563376398e-03, -8.261200972e-03, 9.566827863e-02, 7.610698044e-02, -1.503905207e-01, 2.371023409e-02, -1.693166494e-01, -2.720024902e-03, 1.954396516e-01, -1.659001410e-01, -1.212815475e-02, -6.201712042e-02, 1.080587134e-02]),
            (48000., 0, 0, true, [-1.387927830e-01, -1.206795350e-01, -4.833376035e-02, -7.135377824e-02, -4.815973714e-02, 7.477380335e-02, 5.567879081e-01, 4.692532718e-01, -3.227325156e-03, 5.364906788e-01, -1.674908996e-01, 5.270934701e-01, -2.824700475e-01, -4.568889737e-02, 5.612660646e-01, 1.968392171e-02, -1.014690921e-01]),
            (48000., 1, 0, true, [-1.351801157e-01, -1.402986199e-01, -9.722124040e-02, -1.141466349e-01, -1.083790287e-01, -3.460383043e-02, 4.770365357e-01, 5.290631056e-01, -2.028498352e-01, 4.807024896e-01, 5.103003606e-02, 5.215846300e-01, -4.209110141e-01, -2.379729450e-01, 5.357634425e-01, -6.819412112e-02, -1.325182319e-01]),
            (48000., 1, 1, true, [-1.396245211e-01, -1.163613945e-01, -3.751552105e-02, -6.186177954e-02, -3.436037898e-02, 9.723789990e-02, 5.633789897e-01, 4.460301101e-01, 4.787544534e-02, 5.350935459e-01, -2.199341655e-01, 5.151938796e-01, -2.446179241e-01, 3.943162505e-03, 5.566776395e-01, 3.823124990e-02, -9.457526356e-02]),
        ];
        for (rate, algorithm, oversampling, coupled, expected) in CASES {
            let attributes = if coupled {
                "Q=\"0.75\" Fat=\"1\" Drive=\"6\" DistortionType=\"0\""
            } else {
                "DistortionType=\"2\""
            };
            let p = parse_program(&format!(
                "<Program><XpanderFilter Freq=\"1000\" Mode=\"3\" Algorithm=\"{algorithm}\" Oversampling=\"{oversampling}\" {attributes}/></Program>"
            )).unwrap();
            let mut fx = XpanderFilter::new(&p.nodes[1], 1, rate).unwrap();
            let mut io: Vec<Frame> = (0..24576)
                .map(|n| {
                    let mut frame = [0.; MAX_CHANNELS];
                    frame[0] = authored_sine(n, rate);
                    frame
                })
                .collect();
            let mut settled = None;
            for n in (0..io.len()).step_by(32) {
                if n == 8192 {
                    fx.set_parameter("Freq", &ParameterValue::Number(5000.))
                        .unwrap();
                }
                if n == 16384 {
                    fx.set_parameter("Freq", &ParameterValue::Number(1000.))
                        .unwrap();
                }
                fx.process(&mut io[n..n + 32]).unwrap();
                if settled.is_none() {
                    settled = fx.settle_begin;
                }
            }
            assert_eq!(
                settled,
                Some(match rate as u32 {
                    32000 => 11264,
                    96000 => 16128,
                    _ => 12544,
                })
            );
            let bound = if coupled { 3e-5 } else { 1e-5 };
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (io[n][0] - native).abs() < bound,
                    "cutoff rate{rate}, Algorithm{algorithm}, OS{oversampling}, resonant{coupled}, frame{n}: {} vs {native}",
                    io[n][0]
                );
            }
        }
        // Native 1Hz bracket establishes normalized increment epsilon1e-6.
        // A target-Hz epsilon or the Constant producer's epsilon1e-7 gives
        // different native final-ramp boundaries for these adjacent targets.
        #[rustfmt::skip]
        let brackets = [
            (5331., 12416, [-6.331995875e-02, -2.187654227e-01, 4.775892198e-02, -1.005152985e-01, 1.468517780e-01, 2.090162188e-01, 8.434538543e-02]),
            (5332., 12448, [-6.336339563e-02, -2.187682092e-01, 4.773806036e-02, -1.004932970e-01, 1.468297541e-01, 2.090121657e-01, 8.438850939e-02]),
        ];
        for (target, native_begin, expected) in brackets {
            let mut fx = filter("Algorithm=\"1\" Oversampling=\"0\" DistortionType=\"2\"", 1);
            fx.set_control_block_frames(32).unwrap();
            let mut io: Vec<Frame> = (0..12800)
                .map(|n| {
                    let mut frame = [0.; MAX_CHANNELS];
                    frame[0] = authored_sine(n, 48000.);
                    frame
                })
                .collect();
            let mut begin = None;
            for n in (0..io.len()).step_by(32) {
                if n == 8192 {
                    fx.set_parameter("Freq", &ParameterValue::Number(target))
                        .unwrap();
                }
                fx.process(&mut io[n..n + 32]).unwrap();
                if begin.is_none() {
                    begin = fx.settle_begin;
                }
            }
            assert_eq!(begin, Some(native_begin));
            for (n, native) in [12416, 12424, 12448, 12464, 12480, 12512, 12608]
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (io[n][0] - native).abs() < 3e-6,
                    "native normalized cutoff bracket {target}, frame{n}"
                );
            }
        }
    }

    #[test]
    fn authored_native_cutoff_dynamic_and_scalar_source_routes() {
        // Authored native sine LFO2Hz->Freq, ratio0.1; original dynamic source
        // points bypass the scalar cutoff RC but retain 32-frame interpolation.
        // Bound also includes the native LFO's observed float phase drift.
        let mut fx = filter("Algorithm=\"1\" Oversampling=\"0\" DistortionType=\"2\"", 1);
        let mut dynamic = Vec::new();
        for n in 0..24576 {
            let source = (1. + (std::f64::consts::TAU * 2. * n as f64 / 48000.).sin()) * 0.5;
            let frequency = 1000. * 1000f64.powf(0.1 * source);
            fx.set_effective_parameter("Freq", &ParameterValue::Number(frequency), true)
                .unwrap();
            let mut io = [[0.; MAX_CHANNELS]];
            io[0][0] = authored_sine(n, 48000.);
            fx.process(&mut io).unwrap();
            dynamic.push(io[0][0]);
        }
        #[rustfmt::skip]
        let observed_dynamic = [
            (1024, -2.172172815e-02),
            (2048, 5.239425600e-02),
            (4096, 1.067570969e-01),
            (6144, 8.269789815e-02),
            (8192, -3.372157365e-02),
            (10000, -7.092898339e-02),
            (12288, -9.548679926e-03),
            (14336, 4.036397859e-02),
            (16384, 2.008865029e-02),
            (19000, 3.041182272e-02),
            (21000, -3.794048727e-02),
            (23040, 3.222490475e-02),
        ];
        for (n, native) in observed_dynamic {
            assert!(
                (dynamic[n] - native).abs() < 8e-6,
                "dynamic native cutoff frame{n}"
            );
        }
        // Constant modulation has a producer RC as well as this cutoff's RC.
        // CC is raw and has just the cutoff RC. Both used the same original
        // controller events and graph ratio ln(5)/ln(1000) in the native host.
        for constant in [false, true] {
            let mut fx = filter("Algorithm=\"1\" Oversampling=\"0\" DistortionType=\"2\"", 1);
            let mut source = 0f32;
            let mut target = 0f32;
            let alpha = 1f32 - 0.33f32.powf(3200. / 48000.);
            let mut io: Vec<Frame> = (0..24576)
                .map(|n| {
                    let mut frame = [0.; MAX_CHANNELS];
                    frame[0] = authored_sine(n, 48000.);
                    frame
                })
                .collect();
            for n in (0..io.len()).step_by(32) {
                if n > 0 {
                    source += (target - source) * alpha;
                }
                if n == 8192 {
                    target = 1.;
                }
                if n == 16384 {
                    target = 0.;
                }
                if n % 256 == 0 && ((target - source) * alpha).abs() < 1e-7 {
                    source = target;
                }
                let value = if constant { source } else { target };
                let frequency = 1000. * 5f64.powf(f64::from(value));
                fx.set_effective_parameter("Freq", &ParameterValue::Number(frequency), false)
                    .unwrap();
                fx.process(&mut io[n..n + 32]).unwrap();
            }
            #[rustfmt::skip]
            let observed = if constant { [
                (8192, -2.958914638e-02),
                (8224, -2.572439052e-02),
                (8256, -1.538763754e-02),
                (8320, 1.250610966e-02),
                (8448, 3.183722496e-02),
                (8960, 4.665406793e-02),
                (10000, 1.227641553e-01),
                (12544, 2.118970156e-01),
                (16384, -4.459050298e-02),
                (16416, 6.455665082e-02),
                (16512, 2.084397227e-01),
                (17408, -8.913652599e-02),
                (19000, 3.193709999e-02),
                (21000, -2.877480350e-02),
            ] } else { [
                (8192, -2.958914638e-02),
                (8224, -2.572439052e-02),
                (8256, -2.562059276e-02),
                (8320, -9.522412904e-03),
                (8448, 9.845993668e-02),
                (8960, 1.812169850e-01),
                (10000, 1.491719633e-01),
                (12544, 2.119850367e-01),
                (16384, -4.459050298e-02),
                (16416, 6.455665082e-02),
                (16512, 1.581519246e-01),
                (17408, -1.822773367e-02),
                (19000, 2.991260774e-02),
                (21000, -2.873411961e-02),
            ] };
            for (n, native) in observed {
                let bound = if constant {
                    2e-5
                } else if n <= 10000 {
                    3e-6
                } else {
                    1e-5
                };
                assert!(
                    (io[n][0] - native).abs() < bound,
                    "native scalar cutoff source Constant{constant}, frame{n}"
                );
            }
        }
    }
}
