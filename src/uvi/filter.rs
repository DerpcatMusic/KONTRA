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

pub const FIDELITY_DIAGNOSTIC: &str = "UVI Xpander shapes, both solvers and saturation have authored native comparisons at 48 kHz; quiet rate paths are compared at 8/32/44.1/48/64/96/192 kHz; scalar cutoff transitions are compared at 32/48/96 kHz; Q/Fat transitions, scalar KeyTracking with the AlgorithmII direct path, scalar soft/hardDrive with both solvers, and dynamic softDrive with AlgorithmII direct/oversampled paths are compared at 48 kHz, with native rounding residuals up to 3e-5; matrix-connected Freq/Q/Fat/Drive/KeyTracking and Bypass-only cold starts are compared with AlgorithmII direct at 48 kHz; plain scalar cold starts are compared at 32/48/96 kHz with both solvers and at 32/256-frame blocks at 48 kHz; other cold configurations, other live transitions, partial control-block timing, cross-rate saturation and exact float-rounding parity remain unverified";
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
    // Scaled DFII memories carry raw saturation. Normalization after the
    // output preserves the independently measured moving-drive state.
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

// Scalar controls share the measured .33 law but keep independent clocks.
// Only solver coefficients interpolate; gain/shaper controls hold each point.
#[derive(Clone, Copy, Default)]
struct Control {
    future: f32,
    dynamic: bool,
    settle_begin: Option<usize>,
}

impl Control {
    // Freq/Q/Tracking near-target coefficient ramps span a host block. Held gain
    // controls reach the same normalized target at the next control point.
    fn point(
        &mut self,
        target: f32,
        alpha: f32,
        frame: usize,
        block: usize,
        interpolate: bool,
    ) -> Option<(f32, usize)> {
        if self.dynamic {
            self.future = target;
            self.settle_begin = None;
            return Some((target, 32));
        }
        if let Some(begin) = self.settle_begin {
            if frame < begin + block {
                return None;
            }
            self.settle_begin = None;
        }
        let increment = (target - self.future) * alpha;
        if frame % block == 0 && target != self.future && increment.abs() < 1e-6 {
            self.future = target;
            if interpolate {
                self.settle_begin = Some(frame);
            }
            return Some((target, if interpolate { block } else { 32 }));
        }
        let point = self.future;
        self.future += increment;
        Some((point, 32))
    }
}

const CONTROL_NAMES: [&str; 5] = ["Freq", "Q", "Fat", "Drive", "KeyTracking"];

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
    feedback_factor: f64,
    state: [State; MAX_CHANNELS],
    control_phase: usize,
    frame: usize,
    block_frames: usize,
    processed: bool,
    matrix_initialization: bool,
    controls: [Control; 5],
    cutoff_point: f64,
    tracking: f64,
    frequency_remaining: usize,
    frequency_end: [f64; 3],
    frequency_ramp: [f32; 3],
    frequency_increment: [f32; 3],
    q_end: f64,
    q_ramp: f32,
    q_increment: f32,
    other_ramp: [f32; 3],
    other_increment: [f32; 3],
    fat: f64,
    drive_db: f64,
    pending_drive: Option<f64>,
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
            feedback_factor: 1.,
            state: [State::default(); MAX_CHANNELS],
            control_phase: 0,
            frame: 0,
            block_frames: 256,
            processed: false,
            matrix_initialization: false,
            controls: [Control::default(); 5],
            cutoff_point: 1000.,
            tracking: 0.,
            frequency_remaining: 0,
            frequency_end: [0.; 3],
            frequency_ramp: [0.; 3],
            frequency_increment: [0.; 3],
            q_end: 0.,
            q_ramp: 0.,
            q_increment: 0.,
            other_ramp: [0.; 3],
            other_increment: [0.; 3],
            fat: 1.,
            drive_db: 0.,
            pending_drive: None,
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
        self.set_value(name, value, false, false)
    }

    /// This writer is for connected matrix targets, including unchanged values.
    /// Dynamic sources supply raw points at the 32-frame control clock.
    /// Scalar controls (including CC and Constant) retain independent RCs.
    /// Freq/Tracking share solver coefficient ramps; Q interpolates independently.
    /// Fat/Drive hold each point.
    /// The renderer may call this every output frame; coefficient points are
    /// sampled here, so repeated values must not restart an in-flight ramp.
    pub fn set_effective_parameter(
        &mut self,
        name: &str,
        value: &ParameterValue,
        dynamic: bool,
    ) -> Result<()> {
        self.set_value(name, value, dynamic, true)
    }
    fn set_value(
        &mut self,
        name: &str,
        value: &ParameterValue,
        dynamic: bool,
        matrix: bool,
    ) -> Result<()> {
        let value = match value {
            ParameterValue::Number(n) if name != "Bypass" => *n,
            ParameterValue::Boolean(b) if name == "Bypass" => f64::from(u8::from(*b)),
            _ => bail!("UVI filter Bypass requires Boolean; other parameters require Number"),
        };
        check(name, value)?;
        let control = CONTROL_NAMES.iter().position(|&key| key == name);
        if let Some(index) = control {
            self.controls[index].dynamic = dynamic;
        }
        if !self.processed && matrix && (control.is_some() || name == "Bypass") {
            self.matrix_initialization = true;
        }
        let current = self.parameters.get_mut(name).expect("validated key");
        if *current == value {
            return Ok(());
        }
        *current = value;
        if control.is_some() && (self.processed || matrix) {
            self.controls[control.unwrap()].settle_begin = None;
            return Ok(());
        }
        // Bypass freezes all filter and control state together.
        if name == "Bypass" || name == "Mode" {
            return Ok(());
        }
        self.configure()?;
        self.reset_control();
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

    fn position(&self, index: usize) -> f32 {
        let value = self.parameters[CONTROL_NAMES[index]] as f32;
        match index {
            0 => (value / 20.).ln() / 1000f32.ln(),
            3 => (value + 20.) / 40.,
            _ => value,
        }
    }

    fn physical(&self, index: usize, position: f32) -> f64 {
        if position == self.position(index) {
            return self.parameters[CONTROL_NAMES[index]];
        }
        match index {
            0 => f64::from(20. * 1000f32.powf(position)),
            3 => f64::from(position * 40. - 20.),
            _ => f64::from(position),
        }
    }

    fn reset_control(&mut self) {
        for i in 0..5 {
            self.controls[i].future = self.position(i);
            self.controls[i].settle_begin = None;
        }
        self.cutoff_point = self.parameters["Freq"];
        self.tracking = self.parameters["KeyTracking"];
        self.frequency_remaining = 0;
        self.frequency_end = [self.stage, self.pole, self.feedback_factor];
        self.frequency_ramp = self.frequency_end.map(|v| v as f32);
        self.frequency_increment = [0.; 3];
        self.q_end = self.parameters["Q"];
        self.q_ramp = self.q_end as f32;
        self.q_increment = 0.;
        self.fat = self.parameters["Fat"];
        self.drive_db = self.parameters["Drive"];
        self.pending_drive = None;
    }

    // Native unconnected controls ramp raw solver parameters over one host
    // block, converting endpoints in chunks of at most64 internal frames.
    fn scalar_startup_tick(&mut self) -> Result<()> {
        let span = (64 / self.phases).min(self.block_frames);
        if self.frame % span != 0 {
            return Ok(());
        }
        self.configure_frequency(self.parameters["Freq"])?;
        if self.frame == 0 {
            self.frequency_end = [
                0.,
                if self.parameters["Algorithm"] == 0. {
                    1.
                } else {
                    0.
                },
                self.feedback_factor,
            ];
            self.q_end = 0.;
        }
        let start = self.frequency_end;
        let start_q = self.q_end;
        let proportion = ((self.frame + span) as f64 / self.block_frames as f64).min(1.);
        let (stage, pole) = if self.parameters["Algorithm"] == 0. {
            (self.stage * proportion, 1. + (self.pole - 1.) * proportion)
        } else {
            let raw = self.stage / (1. - self.stage) * proportion;
            (raw / (1. + raw), 0.)
        };
        self.frequency_end = [stage, pole, self.feedback_factor];
        self.frequency_ramp = start.map(|v| v as f32);
        self.frequency_remaining = span * self.phases;
        self.frequency_increment = std::array::from_fn(|i| {
            (self.frequency_end[i] as f32 - self.frequency_ramp[i])
                / self.frequency_remaining as f32
        });
        self.q_ramp = start_q as f32;
        self.q_end = self.parameters["Q"] * proportion;
        self.q_increment = (self.q_end as f32 - self.q_ramp) / self.frequency_remaining as f32;
        let coefficients = |frequency: [f64; 3], q: f64| {
            let feedback = 4. * q * frequency[2];
            [
                feedback,
                1. + self.fat * feedback,
                if self.parameters["Algorithm"] == 0. {
                    1.
                } else {
                    1. / (1. + feedback * frequency[0].powi(4))
                },
            ]
        };
        self.other_ramp = coefficients(start, start_q).map(|v| v as f32);
        let end = coefficients(self.frequency_end, self.q_end);
        self.other_increment = std::array::from_fn(|i| {
            (end[i] as f32 - self.other_ramp[i]) / self.frequency_remaining as f32
        });
        Ok(())
    }

    fn control_tick(&mut self) -> Result<()> {
        if !self.matrix_initialization && self.frame < self.block_frames {
            return self.scalar_startup_tick();
        }
        if self.control_phase != 0 {
            return Ok(());
        }
        let alpha = 1f32 - 0.33f32.powf(3200. / self.rate as f32);
        let phases = self.phases;
        let initializing = !self.processed && self.matrix_initialization;
        if initializing {
            // Connected controls select the native zero-coefficient startup;
            // scalar futures still begin at the serialized parameter values.
            self.frequency_end = [
                0.,
                if self.parameters["Algorithm"] == 0. {
                    1.
                } else {
                    0.
                },
                self.feedback_factor,
            ];
            self.frequency_ramp = self.frequency_end.map(|v| v as f32);
            self.q_end = 0.;
            self.q_ramp = 0.;
        }
        let mut frequency_span = initializing.then_some(32);
        for index in 0..5 {
            let target = self.position(index);
            let Some((point, span)) = self.controls[index].point(
                target,
                alpha,
                self.frame,
                self.block_frames,
                index < 2 || index == 4,
            ) else {
                continue;
            };
            let value = self.physical(index, point);
            match index {
                0 | 4 => {
                    let previous = if index == 0 {
                        &mut self.cutoff_point
                    } else {
                        &mut self.tracking
                    };
                    if *previous != value {
                        *previous = value;
                        frequency_span =
                            Some(frequency_span.map_or(span, |old: usize| old.min(span)));
                    }
                }
                1 => {
                    self.q_ramp = self.q_end as f32;
                    self.q_end = value;
                    self.q_increment = (value as f32 - self.q_ramp) / (span * phases) as f32;
                }
                2 => self.fat = value,
                3 => self.pending_drive = Some(value),
                _ => unreachable!(),
            }
        }
        if let Some(span) = frequency_span {
            // Both cutoff controls feed one solver ramp. A new point arriving
            // during a long final ramp resumes from the current coefficients.
            if self.frequency_remaining == 0 {
                self.frequency_ramp = self.frequency_end.map(|v| v as f32);
            }
            self.configure_frequency(self.cutoff_point)?;
            self.frequency_end = [self.stage, self.pole, self.feedback_factor];
            self.frequency_remaining = span * phases;
            self.frequency_increment = std::array::from_fn(|i| {
                (self.frequency_end[i] as f32 - self.frequency_ramp[i])
                    / self.frequency_remaining as f32
            });
        } else if self.frequency_remaining == 0 {
            self.frequency_ramp = self.frequency_end.map(|v| v as f32);
            self.frequency_increment = [0.; 3];
        }
        // A settling cutoff must not stop another active controller. Predict
        // the next32-frame endpoints from each independent coefficient ramp.
        let mut next_frequency = self.frequency_ramp;
        let mut next_q = self.q_ramp;
        for _ in 0..32 * phases {
            for i in 0..3 {
                next_frequency[i] += self.frequency_increment[i];
            }
            next_q += self.q_increment;
        }
        let coefficients = |frequency: [f32; 3], q: f32| {
            let stage = f64::from(frequency[0]);
            let feedback = 4. * f64::from(q) * f64::from(frequency[2]);
            [
                feedback,
                1. + self.fat * feedback,
                // Native interpolates the reciprocal zero-delay solve.
                if self.parameters["Algorithm"] == 0. {
                    1.
                } else {
                    1. / (1. + feedback * stage.powi(4))
                },
            ]
        };
        let start = coefficients(self.frequency_ramp, self.q_ramp);
        let end = coefficients(next_frequency, next_q);
        self.other_ramp = start.map(|v| v as f32);
        self.other_increment =
            std::array::from_fn(|i| (end[i] as f32 - self.other_ramp[i]) / (32 * phases) as f32);
        Ok(())
    }

    fn configure(&mut self) -> Result<()> {
        self.tracking = self.parameters["KeyTracking"];
        self.configure_frequency(self.parameters["Freq"])
    }
    fn configure_frequency(&mut self, base_frequency: f64) -> Result<()> {
        let p = &self.parameters;
        // Native tracking extends below/above the stored 20..20000Hz range.
        // Each solver applies its own internal-rate ceiling after tracking.
        let frequency =
            base_frequency * ((f64::from(self.note) - 60.) * self.tracking / 12.).exp2();
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
            self.feedback_factor = 1.0029 + w * (0.0526 + w * (-0.0926 + 0.0218 * w));
        } else {
            // Native positive 8/32kHz probes keep the stored Freq=20000,
            // while the effective cutoff is capped at .499*internalRate.
            let g = (std::f64::consts::PI * frequency.min(0.499 * rate) / rate).tan();
            self.stage = g / (1. + g);
            self.pole = 0.;
            self.feedback_factor = 1.;
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
            // Native solver slopes accumulate f32 per internal phase.
            // Shaper drive/normalization holds a point, one output frame later.
            let mut phase_coeffs = [[0.; 7]; 2];
            for c in &mut phase_coeffs[..phases] {
                let drive = if distortion == 2 {
                    1.
                } else {
                    10f64.powf(self.drive_db / 20.)
                };
                let normalization = match distortion {
                    0 => rational(drive),
                    1 => drive.min(1.),
                    _ => 1.,
                };
                *c = [
                    f64::from(self.frequency_ramp[0]),
                    f64::from(self.frequency_ramp[1]),
                    f64::from(self.other_ramp[0]),
                    f64::from(self.other_ramp[1]),
                    f64::from(self.other_ramp[2]),
                    drive,
                    normalization,
                ];
                for i in 0..3 {
                    if self.frequency_remaining != 0 {
                        self.frequency_ramp[i] += self.frequency_increment[i];
                    }
                    self.other_ramp[i] += self.other_increment[i];
                }
                self.frequency_remaining = self.frequency_remaining.saturating_sub(1);
                self.q_ramp += self.q_increment;
            }
            for (channel, x) in frame[..self.channels].iter_mut().enumerate() {
                let state = &mut self.state[channel];
                let mut output = 0.;
                // Algorithm I forces 2x through 48 kHz, honors the toggle above;
                // Algorithm II honors the toggle at every measured rate.
                for phase in 0..phases {
                    let coeff = phase_coeffs[phase];
                    let [
                        stage_gain,
                        pole,
                        feedback,
                        compensation,
                        inverse_denominator,
                        drive,
                        _normalization,
                    ] = coeff;
                    // Moving Drive scales before the resampler. Static gain
                    // commutes with it; native control steps identify placement.
                    let up = if phases == 2 {
                        allpass(drive * f64::from(*x), &mut state.up, phase, coefficients)
                    } else {
                        drive * f64::from(*x)
                    };
                    let offsets = state.offsets;
                    let feedback_offset = if zero_delay {
                        offsets
                            .iter()
                            .fold(0., |sum, offset| stage_gain * sum + offset)
                    } else {
                        state.feedback_output
                    };
                    let input = up * compensation - feedback * feedback_offset;
                    let mut tap = match distortion {
                        0 => rational(input),
                        1 => input.clamp(-1., 1.),
                        2 => input,
                        _ => unreachable!(),
                    } * inverse_denominator;
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
                *x = (output / phases as f64 / phase_coeffs[0][6]) as f32;
                ensure!(x.is_finite(), "Nonfinite UVI filter output");
            }
            if let Some(db) = self.pending_drive.take() {
                self.drive_db = db;
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
        fn settled(mut fx: XpanderFilter) -> XpanderFilter {
            // These native impulses occurred after8192 silent sample frames.
            fx.process(&mut [[0.; MAX_CHANNELS]; 256]).unwrap();
            fx
        }
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
            let mut fx = settled(filter(&format!("Mode=\"{mode}\" DistortionType=\"2\""), 2));
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
            let mut fx = settled(filter(&format!("Freq=\"{freq}\" DistortionType=\"2\""), 1));
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
            let mut fx = settled(filter(
                &format!("Q=\"{q}\" Fat=\"{fat}\" DistortionType=\"2\""),
                1,
            ));
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
            let mut whole = settled(filter(&attributes, 1));
            let mut split = settled(filter(&attributes, 1));
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
                let mut fx = settled(filter(
                    &format!("Mode=\"0\" Drive=\"{drive}\" DistortionType=\"{distortion}\""),
                    1,
                ));
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
            let mut fx = settled(filter(
                &format!(
                    "Algorithm=\"1\" Oversampling=\"{oversampling}\" Q=\"0.75\" Freq=\"{freq}\" DistortionType=\"{distortion}\""
                ),
                1,
            ));
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
            let mut fx = settled(filter(
                &format!("Algorithm=\"1\" Oversampling=\"0\" Mode=\"{mode}\" DistortionType=\"2\""),
                1,
            ));
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
            let mut fx = settled(filter(
                &format!(
                    "Algorithm=\"{algorithm}\" Oversampling=\"{oversampling}\" Q=\"0.75\" Drive=\"{drive}\" DistortionType=\"{distortion}\""
                ),
                1,
            ));
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
            let mut fx = settled(filter(
                &format!(
                    "Algorithm=\"{algorithm}\" Oversampling=\"1\" Q=\"0.75\" DistortionType=\"{distortion}\" Mode=\"{mode}\""
                ),
                1,
            ));
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
            let mut fx = settled(XpanderFilter::new(&p.nodes[2], 1, rate).unwrap());
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
            let mut fx = settled(filter(
                &format!(
                    "Freq=\"50\" Algorithm=\"{algorithm}\" Oversampling=\"{oversampling}\" DistortionType=\"2\""
                ),
                1,
            ));
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
            let mut fx = settled(XpanderFilter::new(&p.nodes[2], 1, rate).unwrap());
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
            let mut fx = settled(filter(
                &format!(
                    "Mode=\"0\" Freq=\"{frequency}\" Algorithm=\"{algorithm}\" Oversampling=\"0\" KeyTracking=\"{tracking}\" DistortionType=\"2\""
                ),
                1,
            ));
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
                let mut fx = settled(XpanderFilter::new(&p.nodes[2], 1, rate).unwrap());
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
        let mut fx = settled(filter("DistortionType=\"2\"", 1));
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
                    settled = fx.controls[0].settle_begin;
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
                    begin = fx.controls[0].settle_begin;
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
    #[test]
    fn authored_native_resonance_fat_and_direct_soft_drive() {
        // Original native API Q0->.75, Fat0->1 and softDrive0->12 at8192,
        // returning at16384, using the same authored PCM16 sine. Q interpolates
        // independent coefficients; Fat holds32-frame points; direct softDrive
        // holds each point one output frame later. Raw saturation state and
        // output normalization preserve the measured moving-shaper memory.
        const POINTS: [usize; 20] = [
            8192, 8224, 8225, 8256, 8257, 8288, 8320, 8448, 8512, 8960, 10000, 12544, 13312, 16384,
            16416, 16417, 16512, 17408, 19000, 21000,
        ];
        #[rustfmt::skip]
        let cases = [
            ("Q", r#"Algorithm="0" Oversampling="0" DistortionType="2""#, 0.75, [-2.673291601e-02, -1.772938855e-02, -2.159671485e-02, -9.252018295e-03, -1.502750255e-02, 1.243800763e-02, 3.537386283e-02, 1.397730876e-02, -6.118206307e-02, -1.251025498e-01, -7.663056254e-02, -1.367002875e-01, -1.557724327e-01, -1.108024456e-02, -8.555717766e-02, -6.059896201e-02, -1.111536473e-01, 1.272685081e-02, 2.730012126e-02, -2.335460298e-02]),
            ("Q", r#"Algorithm="1" Oversampling="0" DistortionType="2""#, 0.75, [-2.958914638e-02, -2.572439052e-02, -2.793294750e-02, -2.155292407e-02, -2.628033422e-02, -3.761814907e-03, 2.054419369e-02, 4.068473727e-02, -3.582415730e-02, -1.277585179e-01, -1.242450476e-01, -1.580065042e-01, -1.524104923e-01, 5.111044645e-02, -2.841062285e-02, 1.417613094e-04, -1.266413331e-01, -2.836447442e-03, 2.984105796e-02, -2.873290889e-02]),
            ("Fat", r#"Q=".75" Fat="0" Algorithm="1" Oversampling="0" DistortionType="2""#, 1.0, [-3.801539540e-02, -2.759129740e-02, -3.227607906e-02, -1.990851760e-02, -2.770158090e-02, 9.742304683e-03, 4.502347112e-02, 3.382733464e-02, -5.895797536e-02, -1.370958239e-01, -1.242189333e-01, -1.580085009e-01, -1.524105817e-01, 5.111044645e-02, -2.841010503e-02, 1.462326036e-04, -1.312584728e-01, 9.868445806e-03, 3.863155842e-02, -3.433505818e-02]),
            ("Drive", r#"Q=".75" Algorithm="1" Oversampling="0" DistortionType="0""#, 12.0, [-1.405938119e-01, -1.086835191e-01, -1.173743606e-01, -5.544374883e-02, -7.506777346e-02, 1.836471446e-02, 8.393317461e-02, 7.058914751e-02, -6.727292389e-02, -1.389325261e-01, -1.336054504e-01, -1.459614486e-01, -1.286226958e-01, 7.779940963e-02, 5.774397869e-03, 3.285177425e-02, -1.436189115e-01, 1.232924126e-02, 1.413975358e-01, -1.301238239e-01]),
        ];
        for (name, attributes, target, expected) in cases {
            let mut fx = filter(attributes, 1);
            let mut io: Vec<Frame> = (0..24576)
                .map(|n| {
                    let mut frame = [0.; MAX_CHANNELS];
                    frame[0] = authored_sine(n, 48000.);
                    frame
                })
                .collect();
            for n in (0..io.len()).step_by(32) {
                if n == 8192 || n == 16384 {
                    fx.set_parameter(
                        name,
                        &ParameterValue::Number(if n == 8192 { target } else { 0. }),
                    )
                    .unwrap();
                }
                fx.process(&mut io[n..n + 32]).unwrap();
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (io[n][0] - native).abs() < 3e-6,
                    "native scalar {name}, frame{n}: {} vs {native}",
                    io[n][0]
                );
            }
        }
        // Adjacent original normalized-Q targets positively bracket the
        // host32 final coefficient ramp at12416 vs12448, epsilon1e-6.
        #[rustfmt::skip]
        let brackets = [
            (0.24219, 12416, [3.377709165e-02, 5.820241570e-02, 3.139715875e-03, 1.299026143e-02, -2.828970179e-02, -5.260721594e-02, -3.906217217e-02]),
            (0.24221, 12448, [3.377837688e-02, 5.820549279e-02, 3.138023196e-03, 1.299108006e-02, -2.828899771e-02, -5.261089653e-02, -3.906372562e-02]),
        ];
        for (target, native_begin, expected) in brackets {
            let mut fx = filter(r#"Algorithm="1" Oversampling="0" DistortionType="2""#, 1);
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
                    fx.set_parameter("Q", &ParameterValue::Number(target))
                        .unwrap();
                }
                fx.process(&mut io[n..n + 32]).unwrap();
                if begin.is_none() {
                    begin = fx.controls[1].settle_begin;
                }
            }
            assert_eq!(begin, Some(native_begin));
            for (n, native) in [12416, 12424, 12448, 12464, 12480, 12512, 12608]
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (io[n][0] - native).abs() < 1e-6,
                    "native Q bracket {target}, frame{n}"
                );
            }
        }
    }

    #[test]
    fn authored_native_dynamic_q_fat_and_direct_soft_drive() {
        // Original sineLFO2Hz; its separate float-phase/table fidelity is
        // unverified. Numerical observations retain that measured source
        // drift bound while distinguishing scalarRC from raw control arrays.
        const POINTS: [usize; 9] = [8192, 8960, 10000, 12288, 14336, 16384, 19000, 21000, 23040];
        #[rustfmt::skip]
        let cases = [
            ("Q", r#"Algorithm="1" Oversampling="0" DistortionType="2""#, 0.75, [-1.410977095e-01, -1.350710094e-01, -1.012159288e-01, 4.606700689e-02, 5.206270143e-02, 1.933692396e-02, 3.111593612e-02, -4.244329780e-02, 6.540554017e-02]),
            ("Fat", r#"Q=".75" Fat="0" Algorithm="1" Oversampling="0" DistortionType="2""#, 1.0, [-1.430379152e-01, -1.414418370e-01, -1.022356451e-01, 6.647135317e-02, 6.374888122e-02, 1.440275274e-02, 4.027266055e-02, -4.925639927e-02, 8.332927525e-02]),
            ("Drive", r#"Q=".75" Algorithm="1" Oversampling="0" DistortionType="0""#, 12.0, [-1.459977627e-01, -1.387987286e-01, -1.300856024e-01, 8.225924522e-02, 1.403311938e-01, 5.858715996e-02, 1.412469298e-01, -1.295411885e-01, 1.350654960e-01]),
        ];
        for (name, attributes, ratio, expected) in cases {
            let mut fx = filter(attributes, 1);
            let mut output = Vec::new();
            for n in 0..24576 {
                let source = (1. + (std::f64::consts::TAU * 2. * n as f64 / 48000.).sin()) * 0.5;
                fx.set_effective_parameter(name, &ParameterValue::Number(ratio * source), true)
                    .unwrap();
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < 1.5e-5,
                    "native dynamic {name}, frame{n}"
                );
            }
        }
    }

    fn authored_native_lfo_point(n: usize) -> f64 {
        #[rustfmt::skip]
        const SOURCE: [f32;769] = [
            5.000000000E-01, 5.041883588E-01, 5.083767176E-01, 5.125648379E-01, 5.167506933E-01, 5.209364891E-01,
            5.251216292E-01, 5.293024182E-01, 5.334832072E-01, 5.376623869E-01, 5.418356061E-01, 5.460088253E-01,
            5.501791835E-01, 5.543423295E-01, 5.585054755E-01, 5.626642108E-01, 5.668147206E-01, 5.709652901E-01,
            5.751094818E-01, 5.792449713E-01, 5.833804607E-01, 5.875072479E-01, 5.916251540E-01, 5.957430601E-01,
            5.998497009E-01, 6.039475203E-01, 6.080453396E-01, 6.121289730E-01, 6.162042618E-01, 6.202796102E-01,
            6.243373752E-01, 6.283876896E-01, 6.324380636E-01, 6.364671588E-01, 6.404900551E-01, 6.445130110E-01,
            6.485107541E-01, 6.525038481E-01, 6.564968824E-01, 6.604604721E-01, 6.644213200E-01, 6.683821678E-01,
            6.723089218E-01, 6.762350798E-01, 6.801593304E-01, 6.840484738E-01, 6.879376769E-01, 6.918219924E-01,
            6.956718564E-01, 6.995216608E-01, 7.033634186E-01, 7.071716189E-01, 7.109798193E-01, 7.147763371E-01,
            7.185406089E-01, 7.223048210E-01, 7.260535955E-01, 7.297716141E-01, 7.334896326E-01, 7.371879816E-01,
            7.408575416E-01, 7.445271015E-01, 7.481725216E-01, 7.517914176E-01, 7.554103136E-01, 7.590003014E-01,
            7.625663280E-01, 7.661323547E-01, 7.696645260E-01, 7.731755376E-01, 7.766865492E-01, 7.801584005E-01,
            7.836123109E-01, 7.870662212E-01, 7.904753685E-01, 7.938700914E-01, 7.972648144E-01, 8.006088734E-01,
            8.039423227E-01, 8.072758317E-01, 8.105525374E-01, 8.138227463E-01, 8.170930147E-01, 8.203001022E-01,
            8.235051632E-01, 8.267075419E-01, 8.298454285E-01, 8.329833150E-01, 8.361136913E-01, 8.391824961E-01,
            8.422513604E-01, 8.453074694E-01, 8.483054638E-01, 8.513033986E-01, 8.542832136E-01, 8.572084904E-01,
            8.601337671E-01, 8.630352020E-01, 8.658860326E-01, 8.687368631E-01, 8.715579510E-01, 8.743325472E-01,
            8.771072030E-01, 8.798459768E-01, 8.825427890E-01, 8.852396011E-01, 8.878941536E-01, 8.905115128E-01,
            8.931288123E-01, 8.956974149E-01, 8.982337117E-01, 9.007700086E-01, 9.032508731E-01, 9.057045579E-01,
            9.081583023E-01, 9.105497003E-01, 9.129193425E-01, 9.152890444E-01, 9.175892472E-01, 9.198734164E-01,
            9.221575856E-01, 9.243651628E-01, 9.265625477E-01, 9.287598133E-01, 9.308731556E-01, 9.329823256E-01,
            9.350894690E-01, 9.371091723E-01, 9.391288757E-01, 9.411401749E-01, 9.430691600E-01, 9.449982047E-01,
            9.469122887E-01, 9.487494826E-01, 9.505866766E-01, 9.524022341E-01, 9.541465044E-01, 9.558907747E-01,
            9.576064348E-01, 9.592567682E-01, 9.609070420E-01, 9.625217915E-01, 9.640771151E-01, 9.656324387E-01,
            9.671452045E-01, 9.686045647E-01, 9.700639844E-01, 9.714735746E-01, 9.728361368E-01, 9.741988182E-01,
            9.755042791E-01, 9.767692685E-01, 9.780342579E-01, 9.792348146E-01, 9.804013968E-01, 9.815680385E-01,
            9.826627374E-01, 9.837303162E-01, 9.847978354E-01, 9.857860208E-01, 9.867538214E-01, 9.877216816E-01,
            9.886025786E-01, 9.894701242E-01, 9.903376102E-01, 9.911107421E-01, 9.918774366E-01, 9.926434159E-01,
            9.933087826E-01, 9.939742088E-01, 9.946317673E-01, 9.951955080E-01, 9.957591891E-01, 9.963078499E-01,
            9.967695475E-01, 9.972312450E-01, 9.976706505E-01, 9.980300069E-01, 9.983893633E-01, 9.987192154E-01,
            9.989760518E-01, 9.992328882E-01, 9.994529486E-01, 9.996071458E-01, 9.997613430E-01, 9.998714328E-01,
            9.999228120E-01, 9.999742508E-01, 9.999743104E-01, 9.999229312E-01, 9.998715520E-01, 9.997616410E-01,
            9.996074438E-01, 9.994533062E-01, 9.992334843E-01, 9.989765882E-01, 9.987197518E-01, 9.983900785E-01,
            9.980307221E-01, 9.976713657E-01, 9.972321391E-01, 9.967704415E-01, 9.963087440E-01, 9.957603216E-01,
            9.951965809E-01, 9.946328402E-01, 9.939755201E-01, 9.933101535E-01, 9.926447868E-01, 9.918789864E-01,
            9.911122918E-01, 9.903393984E-01, 9.894718528E-01, 9.886043668E-01, 9.877236485E-01, 9.867558479E-01,
            9.857879877E-01, 9.848000407E-01, 9.837324619E-01, 9.826649427E-01, 9.815703630E-01, 9.804037809E-01,
            9.792371392E-01, 9.780368209E-01, 9.767718315E-01, 9.755069017E-01, 9.742015600E-01, 9.728389382E-01,
            9.714763165E-01, 9.700669050E-01, 9.686075449E-01, 9.671481252E-01, 9.656356573E-01, 9.640803337E-01,
            9.625250101E-01, 9.609104395E-01, 9.592601061E-01, 9.576097727E-01, 9.558942914E-01, 9.541500211E-01,
            9.524057508E-01, 9.505904317E-01, 9.487532377E-01, 9.469159842E-01, 9.450020790E-01, 9.430730343E-01,
            9.411439896E-01, 9.391329288E-01, 9.371132851E-01, 9.350935221E-01, 9.329866171E-01, 9.308774471E-01,
            9.287643433E-01, 9.265669584E-01, 9.243696332E-01, 9.221622348E-01,
            9.198780656e-01, 9.175938368e-01, 9.152938128e-01, 9.129241109e-01, 9.105544686e-01, 9.081633091e-01,
            9.057095647e-01, 9.032558203e-01, 9.007751942e-01, 8.982388973e-01, 8.957026005e-01, 8.931341171e-01,
            8.905168176e-01, 8.878995180e-01, 8.852450848e-01, 8.825482726e-01, 8.798514605e-01, 8.771128058e-01,
            8.743381500e-01, 8.715634942e-01, 8.687425852e-01, 8.658917546e-01, 8.630409241e-01, 8.601396680e-01,
            8.572143912e-01, 8.542891741e-01, 8.513094783e-01, 8.483115435e-01, 8.453135490e-01, 8.422575593e-01,
            8.391886950e-01, 8.361198902e-01, 8.329896927e-01, 8.298518062e-01, 8.267139196e-01, 8.235116005e-01,
            8.203065991e-01, 8.170996308e-01, 8.138294220e-01, 8.105591536e-01, 8.072825670e-01, 8.039490581e-01,
            8.006155491e-01, 7.972716093e-01, 7.938769460e-01, 7.904822230e-01, 7.870732546e-01, 7.836192846e-01,
            7.801653743e-01, 7.766937017e-01, 7.731826305e-01, 7.696715593e-01, 7.661396265e-01, 7.625735998e-01,
            7.590075731e-01, 7.554177046e-01, 7.517987490e-01, 7.481798530e-01, 7.445344925e-01, 7.408649921e-01,
            7.371953726e-01, 7.334971428e-01, 7.297791243e-01, 7.260611057e-01, 7.223125100e-01, 7.185482383e-01,
            7.147839665e-01, 7.109875679e-01, 7.071793079e-01, 7.033711076e-01, 6.995294690e-01, 6.956796050e-01,
            6.918297410e-01, 6.879455447e-01, 6.840564013e-01, 6.801671982e-01, 6.762430668e-01, 6.723169088e-01,
            6.683901548e-01, 6.644293070e-01, 6.604684591e-01, 6.565049291e-01, 6.525118947e-01, 6.485188603e-01,
            6.445211768e-01, 6.404982805e-01, 6.364753246e-01, 6.324462295e-01, 6.283959150e-01, 6.243455410e-01,
            6.202878356e-01, 6.162124872e-01, 6.121371388e-01, 6.080536246e-01, 6.039558053e-01, 5.998579860e-01,
            5.957514048e-01, 5.916334987e-01, 5.875155926e-01, 5.833888054e-01, 5.792533159e-01, 5.751178265e-01,
            5.709736943e-01, 5.668231249e-01, 5.626726151e-01, 5.585139394e-01, 5.543507934e-01, 5.501876473e-01,
            5.460172892e-01, 5.418440104e-01, 5.376707911e-01, 5.334916115e-01, 5.293108225e-01, 5.251300335e-01,
            5.209449530e-01, 5.167591572e-01, 5.125733614e-01, 5.083851814e-01, 5.041968226e-01, 5.000084639e-01,
            4.958201051e-01, 4.916317463e-01, 4.874435663e-01, 4.832577705e-01, 4.790719748e-01, 4.748868942e-01,
            4.707061052e-01, 4.665252864e-01, 4.623460770e-01, 4.581728578e-01, 4.539996386e-01, 4.498292208e-01,
            4.456660748e-01, 4.415028989e-01, 4.373441637e-01, 4.331936240e-01, 4.290431142e-01, 4.248988926e-01,
            4.207634032e-01, 4.166279435e-01, 4.125010371e-01, 4.083831310e-01, 4.042652249e-01, 4.001585841e-01,
            3.960607648e-01, 3.919629753e-01, 3.878793418e-01, 3.838040233e-01, 3.797286749e-01, 3.756708503e-01,
            3.716205060e-01, 3.675701618e-01, 3.635409474e-01, 3.595180213e-01, 3.554950953e-01, 3.514972925e-01,
            3.475042582e-01, 3.435112238e-01, 3.395475149e-01, 3.355866671e-01, 3.316258490e-01, 3.276990056e-01,
            3.237728179e-01, 3.198485374e-01, 3.159593642e-01, 3.120701909e-01, 3.081858456e-01, 3.043359816e-01,
            3.004861176e-01, 2.966442704e-01, 2.928360701e-01, 2.890278697e-01, 2.852312326e-01, 2.814669907e-01,
            2.777027488e-01, 2.739539146e-01, 2.702358961e-01, 2.665179074e-01, 2.628194690e-01, 2.591499090e-01,
            2.554803491e-01, 2.518348098e-01, 2.482159138e-01, 2.445969880e-01, 2.410069108e-01, 2.374408841e-01,
            2.338748574e-01, 2.303426266e-01, 2.268315852e-01, 2.233205438e-01, 2.198486328e-01, 2.163946927e-01,
            2.129407525e-01, 2.095315456e-01, 2.061368525e-01, 2.027421594e-01, 1.993979812e-01, 1.960644722e-01,
            1.927309632e-01, 1.894541085e-01, 1.861838698e-01, 1.829136014e-01, 1.797063947e-01, 1.765013337e-01,
            1.732987761e-01, 1.701608896e-01, 1.670230329e-01, 1.638925672e-01, 1.608237326e-01, 1.577548981e-01,
            1.546985805e-01, 1.517006159e-01, 1.487026513e-01, 1.457226872e-01, 1.427974701e-01, 1.398722231e-01,
            1.369706392e-01, 1.341198087e-01, 1.312689483e-01, 1.284477413e-01, 1.256731153e-01, 1.228984892e-01,
            1.201595366e-01, 1.174627542e-01, 1.147659719e-01, 1.121111810e-01, 1.094938517e-01, 1.068764925e-01,
            1.043077111e-01, 1.017714143e-01, 9.923511744e-02, 9.675407410e-02, 9.430038929e-02, 9.184667468e-02,
            8.945509791e-02, 8.708545566e-02, 8.471575379e-02, 8.241531253e-02, 8.013111353e-02, 7.784694433e-02,
            7.563921809e-02, 7.344192266e-02, 7.124465704e-02, 6.913110614e-02, 6.702193618e-02, 6.491464376e-02,
            6.289494038e-02, 6.087523699e-02, 5.886375904e-02, 5.693471432e-02, 5.500566959e-02, 5.309143662e-02,
            5.125424266e-02, 4.941701889e-02, 4.760134220e-02, 4.585704207e-02, 4.411274195e-02, 4.239684343e-02,
            4.074653983e-02, 3.909620643e-02, 3.748127818e-02, 3.592598438e-02, 3.437066078e-02, 3.285780549e-02,
            3.139841557e-02, 2.993899584e-02, 2.852922678e-02, 2.716660500e-02, 2.580398321e-02, 2.449828386e-02,
            2.323329449e-02, 2.196827531e-02, 2.076756954e-02, 1.960092783e-02, 1.843431592e-02, 1.733937860e-02,
            1.627182961e-02, 1.520428061e-02, 1.421591640e-02, 1.324808598e-02, 1.228025556e-02, 1.139914989e-02,
            1.053163409e-02, 9.664118290e-03, 8.890837431e-03, 8.124142885e-03, 7.357925177e-03, 6.692528725e-03,
            6.027102470e-03, 5.369365215e-03, 4.805654287e-03, 4.241943359e-03, 3.693073988e-03, 3.231406212e-03,
            2.769708633e-03, 2.330094576e-03, 1.970708370e-03, 1.611351967e-03, 1.281291246e-03, 1.024454832e-03,
            7.675588131e-04, 5.473196507e-04, 3.931522369e-04, 2.389848232e-04, 1.286566257e-04, 7.724761963e-05,
            2.589821815e-05, 2.557039261e-05, 7.694959641e-05, 1.283586025e-04, 2.380609512e-04, 3.922283649e-04,
            5.463957787e-04, 7.660388947e-04, 1.022875309e-03, 1.279741526e-03, 1.609176397e-03, 1.968532801e-03,
            2.327948809e-03, 2.766937017e-03, 3.228604794e-03, 3.690302372e-03, 4.238545895e-03, 4.802227020e-03,
            5.365937948e-03, 6.023079157e-03, 6.688475609e-03, 7.353872061e-03, 8.119463921e-03, 8.886158466e-03,
            9.658843279e-03, 1.052635908e-02, 1.139390469e-02, 1.227438450e-02, 1.324221492e-02, 1.421004534e-02,
            1.519781351e-02, 1.626539230e-02, 1.733291149e-02, 1.842722297e-02, 1.959386468e-02, 2.076050639e-02,
            2.196061611e-02, 2.322560549e-02, 2.449056506e-02, 2.579566836e-02, 2.715826035e-02, 2.852091193e-02,
            2.993011475e-02, 3.138950467e-02, 3.284889460e-02, 3.436118364e-02, 3.591650724e-02, 3.747186065e-02,
            3.908622265e-02, 4.073655605e-02, 4.238685966e-02, 4.410219193e-02, 4.584649205e-02, 4.759079218e-02,
            4.940593243e-02, 5.124312639e-02, 5.308037996e-02, 5.499398708e-02, 5.692303181e-02, 5.885198712e-02,
            6.086289883e-02, 6.288260221e-02, 6.490230560e-02, 6.700909138e-02, 6.911826134e-02, 7.123121619e-02,
            7.342857122e-02, 7.562589645e-02, 7.783311605e-02, 8.011728525e-02, 8.240151405e-02, 8.470141888e-02,
            8.707112074e-02, 8.944076300e-02, 9.183180332e-02, 9.428548813e-02, 9.673917294e-02, 9.921965003e-02,
            1.017559171e-01, 1.042922437e-01, 1.068605185e-01, 1.094778776e-01, 1.120952070e-01, 1.147494912e-01,
            1.174463034e-01, 1.201430857e-01, 1.228815615e-01, 1.256562173e-01, 1.284308732e-01, 1.312516630e-01,
            1.341024637e-01, 1.369532943e-01, 1.398544908e-01, 1.427797377e-01, 1.457049847e-01, 1.486845016e-01,
            1.516824067e-01, 1.546803117e-01, 1.577361524e-01, 1.608049870e-01, 1.638738513e-01, 1.670039296e-01,
            1.701418161e-01, 1.732797027e-01, 1.764819026e-01, 1.796869040e-01, 1.828937531e-01, 1.861640215e-01,
            1.894342899e-01, 1.927107275e-01, 1.960442364e-01, 1.993777156e-01, 2.027215362e-01, 2.061162591e-01,
            2.095110118e-01, 2.129198313e-01, 2.163737118e-01, 2.198275924e-01, 2.232990861e-01, 2.268101573e-01,
            2.303211987e-01, 2.338531017e-01, 2.374191582e-01, 2.409852147e-01, 2.445750237e-01, 2.481939197e-01,
            2.518128157e-01, 2.554580569e-01, 2.591276169e-01, 2.627972066e-01, 2.664953768e-01, 2.702133954e-01,
            2.739314437e-01, 2.776799798e-01, 2.814442515e-01, 2.852084935e-01, 2.890048027e-01, 2.928129435e-01,
            2.966210842e-01, 3.004626334e-01, 3.043124974e-01, 3.081623912e-01, 3.120465279e-01, 3.159357309e-01,
            3.198249340e-01, 3.237489760e-01, 3.276751935e-01, 3.316017985e-01, 3.355626464e-01, 3.395234942e-01,
            3.434869647e-01, 3.474800587e-01, 3.514731526e-01, 3.554707766e-01, 3.594937325e-01, 3.635166585e-01,
            3.675456643e-01, 3.715959489e-01, 3.756462038e-01, 3.797038198e-01, 3.837791681e-01, 3.878544867e-01,
            3.919380009e-01, 3.960358500e-01, 4.001336992e-01, 4.042402506e-01, 4.083581567e-01, 4.124760628e-01,
            4.166028202e-01, 4.207383096e-01, 4.248737991e-01, 4.290179312e-01, 4.331685007e-01, 4.373190701e-01,
            4.414777458e-01, 4.456408918e-01, 4.498040676e-01, 4.539743662e-01, 4.581474960e-01, 4.623206556e-01,
            4.664998055e-01, 4.706805944e-01, 4.748613834e-01, 4.790464938e-01, 4.832323194e-01, 4.874181747e-01,
            4.916063249e-01, 4.957946837e-01, 4.999830425e-01, 5.041713119e-01, 5.083596706e-01, 5.125478506e-01,
            5.167336464e-01, 5.209195018e-01, 5.251046419e-01, 5.292854309e-01, 5.334661603e-01, 5.376453996e-01,
            5.418186188e-01, 5.459918380e-01, 5.501622558e-01, 5.543254018e-01, 5.584885478e-01, 5.626472831e-01,
            5.667978525e-01, 5.709484220e-01, 5.750926733e-01,
        ];
        f64::from(SOURCE[n / 32])
    }

    #[test]
    fn authored_native_cutoff_settling_keeps_dynamic_resonance_running() {
        // Original paired LFO->Gain/DC numeric observations supply consumer
        // input, excluding the independently unverified source phase/table.
        // A cutoff's final256-frame ramp must not freeze the active Q clock.
        let mut fx = filter(r#"Algorithm="1" Oversampling="0" DistortionType="2""#, 1);
        let mut output = Vec::new();
        for n in 0..24576 {
            if n == 8192 || n == 16384 {
                fx.set_parameter(
                    "Freq",
                    &ParameterValue::Number(if n == 8192 { 5000. } else { 1000. }),
                )
                .unwrap();
            }
            let source = authored_native_lfo_point(n);
            fx.set_effective_parameter("Q", &ParameterValue::Number(0.75 * source), true)
                .unwrap();
            let mut io = [[0.; MAX_CHANNELS]];
            io[0][0] = authored_sine(n, 48000.);
            fx.process(&mut io).unwrap();
            output.push(io[0][0]);
        }
        #[rustfmt::skip]
        let observed = [
            (8960, 2.397817969e-01),
            (10000, 2.714936733e-01),
            (12288, -8.358175308e-02),
            (12544, 2.567042112e-01),
            (12576, 2.659919560e-01),
            (12608, 2.076791972e-01),
            (12672, -3.891954571e-02),
            (12768, -2.688043416e-01),
            (12800, -2.210325450e-01),
            (13056, 5.603232421e-03),
            (16384, -6.965818256e-02),
            (16416, 4.759488255e-02),
            (17408, -1.858420298e-02),
            (19000, 3.146196902e-02),
            (20736, 2.912761085e-02),
            (20768, 1.109132636e-02),
            (20800, -9.996548295e-03),
            (20960, -1.557556447e-02),
            (21000, -4.244329780e-02),
            (23040, 6.540554017e-02),
        ];
        for (n, native) in observed {
            assert!(
                (output[n] - native).abs() < 3e-5,
                "native independent Freq/Q clocks frame{n}: {} vs {native}",
                output[n]
            );
        }
    }

    #[test]
    fn authored_native_soft_drive_gain_precedes_upsampling() {
        // Original scalar steps and paired LFO->Gain/DC observations establish
        // Drive gain before upsampling and normalization after downsampling.
        // A fixed gain commutes with these linear filters, so static fixtures
        // alone cannot establish the placement. No source phase/table claim.
        const POINTS: [usize; 18] = [
            8192, 8224, 8225, 8256, 8257, 8960, 10000, 11264, 12544, 14336, 16384, 16416, 16417,
            17408, 19000, 20736, 21000, 23040,
        ];
        #[rustfmt::skip]
        let cases = [
            (0, false, [-1.165166944e-01, -6.257658452e-02, -8.011335135e-02, 2.771522559e-04, -2.401392162e-02, -1.430923492e-01, -9.798542410e-02, -6.302177161e-02, -1.390511543e-01, 1.339614093e-01, 2.161836065e-02, -5.231790617e-02, -2.680671029e-02, 6.581330299e-02, 1.189036742e-01, 2.813984826e-02, -9.491346776e-02, 1.397602856e-01]),
            (1, false, [-1.111556068e-01, -5.234142020e-02, -7.182751596e-02, 1.251022052e-02, -1.274842955e-02, -1.441088170e-01, -9.012801945e-02, -7.511375844e-02, -1.376330703e-01, 1.312063038e-01, 9.271115996e-03, -6.473710388e-02, -3.970158473e-02, 7.744530588e-02, 1.138947085e-01, 1.638618112e-02, -8.707927167e-02, 1.397310942e-01]),
            (1, true, [-1.248657256e-01, -7.114398479e-02, -9.335831553e-02, 2.037829254e-03, -2.507062256e-02, -1.442053616e-01, -8.586926013e-02, -7.804582268e-02, -1.279921979e-01, 1.182226986e-01, -9.471935220e-03, -7.810512930e-02, -5.524032563e-02, 7.948618382e-02, 1.139412001e-01, 1.887580007e-02, -8.837161958e-02, 1.388316453e-01]),
        ];
        for (algorithm, dynamic, expected) in cases {
            let mut fx = filter(
                &format!(r#"Q=".75" Algorithm="{algorithm}" Oversampling="1" DistortionType="0""#),
                1,
            );
            let mut output = Vec::new();
            for n in 0..24576 {
                if dynamic {
                    fx.set_effective_parameter(
                        "Drive",
                        &ParameterValue::Number(12. * authored_native_lfo_point(n)),
                        true,
                    )
                    .unwrap();
                } else if n == 8192 || n == 16384 {
                    fx.set_parameter(
                        "Drive",
                        &ParameterValue::Number(if n == 8192 { 12. } else { 0. }),
                    )
                    .unwrap();
                }
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < 1e-6,
                    "native oversampled softDrive Algorithm{algorithm} dynamic{dynamic}, frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
    }

    #[test]
    fn authored_native_hard_drive_control_clock_and_raw_feedback() {
        // Independent native hard clipping uses the same raw feedback state,
        // queued scalar clock and gain-before-resampler placement as soft Drive.
        const POINTS: [usize; 18] = [
            8192, 8224, 8225, 8256, 8257, 8960, 10000, 11264, 12544, 14336, 16384, 16416, 16417,
            17408, 19000, 20736, 21000, 23040,
        ];
        #[rustfmt::skip]
        let cases = [
            (0, 1, [-1.052096635e-01, -5.354830995e-02, -7.443311810e-02, 6.965268403e-03, -1.732344925e-02, -1.453541219e-01, -9.899063408e-02, -6.397016346e-02, -1.405297816e-01, 1.353277862e-01, 2.198125422e-02, -5.312265456e-02, -2.719915845e-02, 6.711284816e-02, 1.078912541e-01, 2.106494457e-02, -8.447771519e-02, 1.289259642e-01]),
            (1, 0, [-1.294018030e-01, -9.766168892e-02, -1.124989167e-01, -4.634797946e-02, -6.876165420e-02, -1.396205574e-01, -1.371048391e-01, -9.448093362e-03, -1.500396729e-01, 1.460657120e-01, 7.801356167e-02, 7.504975423e-03, 3.486957774e-02, 1.569488272e-02, 1.307258755e-01, 7.105180621e-02, -1.187813729e-01, 1.311337799e-01]),
            (1, 1, [-9.980610013e-02, -4.371003062e-02, -6.593141705e-02, 1.877218671e-02, -5.873966031e-03, -1.464065015e-01, -9.104041755e-02, -7.620690763e-02, -1.391181797e-01, 1.325763911e-01, 9.515905753e-03, -6.570208073e-02, -4.022679850e-02, 7.826037705e-02, 1.028034687e-01, 9.943816811e-03, -7.679711282e-02, 1.284838468e-01]),
        ];
        for (algorithm, oversampling, expected) in cases {
            let mut fx = filter(
                &format!(
                    r#"Q=".75" Algorithm="{algorithm}" Oversampling="{oversampling}" DistortionType="1""#
                ),
                1,
            );
            let mut output = Vec::new();
            for n in 0..24576 {
                if n == 8192 || n == 16384 {
                    fx.set_parameter(
                        "Drive",
                        &ParameterValue::Number(if n == 8192 { 12. } else { 0. }),
                    )
                    .unwrap();
                }
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < 1e-6,
                    "native hardDrive Algorithm{algorithm} OS{oversampling}, frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
    }

    #[test]
    fn authored_native_tracking_and_cutoff_keep_independent_clocks() {
        // Original note72 LP1/linear probes separate normalized Tracking RC
        // from normalized log-cutoff RC. Both feed one coefficient ramp.
        // Opposing, additive and overlapping steps distinguish this from
        // instantaneous tracking or one combined scalar RC.
        // In the native overlap host, CCdelta64 executes its Lua setter at
        // block begin12800. The leaf receives that observed setter time;
        // this does not establish general MIDI event-offset lowering.
        const POINTS: [usize; 24] = [
            8192, 8224, 8256, 8960, 10000, 12032, 12288, 12544, 12800, 12801, 12828, 12832, 12833,
            12864, 13056, 13312, 16384, 16416, 19000, 20480, 20736, 21312, 21504, 23040,
        ];
        #[rustfmt::skip]
        let cases = [
            ("only", 1000.0, 1.0, false, 1e-06, [1.392019242e-01, 9.931978583e-02, 3.904127330e-02, 1.840819418e-01, 1.930874884e-01, -9.176774323e-02, -8.988073468e-02, 2.016528696e-01, -1.566388309e-01, -1.781161427e-01, 7.354921848e-02, -7.199079543e-02, -1.054768711e-01, 3.075519204e-02, -1.015951112e-02, 1.690591574e-01, -1.240283176e-01, -2.858594060e-02, -1.407382786e-01, 1.445538849e-01, -6.882883608e-02, 1.041812524e-01, -1.140090153e-01, -1.455102116e-01]),
            ("cancel", 500.0, 1.0, false, 1e-06, [1.392019242e-01, 9.931978583e-02, 3.446416184e-02, 1.461426318e-01, 1.138233766e-01, -1.754882932e-02, -1.044955999e-01, 1.452916116e-01, -7.312262058e-02, -9.475281835e-02, 9.522990137e-02, -2.230965300e-03, -2.851834334e-02, 6.921859086e-02, -5.589948595e-02, 1.414595097e-01, -4.393297806e-02, 2.949998528e-02, -1.405370682e-01, 1.445477754e-01, -6.882883608e-02, 1.041812524e-01, -1.140090153e-01, -1.455102116e-01]),
            ("add", 1500.0, 0.75, false, 1e-06, [1.392019242e-01, 9.931978583e-02, 4.060704634e-02, 1.888070554e-01, 2.125008106e-01, -1.178703979e-01, -7.466407120e-02, 2.091575414e-01, -1.810277402e-01, -2.004290819e-01, 5.655802041e-02, -9.785499424e-02, -1.315782368e-01, 9.919256903e-03, 1.215253212e-02, 1.661712974e-01, -1.498627961e-01, -5.304811522e-02, -1.408056468e-01, 1.445522457e-01, -6.882791966e-02, 1.041813120e-01, -1.140093133e-01, -1.455102116e-01]),
            ("overlap", 20000.0, 0.05, true, 2e-06, [1.392019242e-01, 9.931978583e-02, 5.523375794e-02, 1.487274766e-01, 2.432464957e-01, -2.154917419e-01, 3.155081347e-02, 1.769452989e-01, -2.478511333e-01, -2.495804578e-01, -5.282136798e-02, -2.010877281e-01, -2.244860381e-01, -1.038495377e-01, 1.264514029e-01, 9.307311475e-02, -2.354491353e-01, -1.651065499e-01, -2.019059807e-01, 1.827553213e-01, -2.288174182e-01, 1.624545753e-01, -1.513438076e-01, -1.462782025e-01]),
        ];
        for (tag, frequency, tracking, overlap, tolerance, expected) in cases {
            let mut fx = filter(
                r#"Mode="0" Algorithm="1" Oversampling="0" DistortionType="2""#,
                1,
            );
            fx.set_note(72).unwrap();
            let mut output = Vec::new();
            for n in 0..24576 {
                if overlap {
                    if n == 8192 || n == 20480 {
                        fx.set_parameter(
                            "Freq",
                            &ParameterValue::Number(if n == 8192 { frequency } else { 1000. }),
                        )
                        .unwrap();
                    }
                    if n == 12800 || n == 16384 {
                        fx.set_parameter(
                            "KeyTracking",
                            &ParameterValue::Number(if n == 12800 { tracking } else { 0. }),
                        )
                        .unwrap();
                    }
                } else if n == 8192 || n == 16384 {
                    fx.set_parameter(
                        "Freq",
                        &ParameterValue::Number(if n == 8192 { frequency } else { 1000. }),
                    )
                    .unwrap();
                    fx.set_parameter(
                        "KeyTracking",
                        &ParameterValue::Number(if n == 8192 { tracking } else { 0. }),
                    )
                    .unwrap();
                }
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < tolerance,
                    "native independent cutoff/tracking {tag} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
    }

    #[test]
    fn authored_native_matrix_controls_start_from_zero_coefficients() {
        // Original 48k PCM16 sine and paired LFO->Gain/DC observations.
        // Matrix-connected filters ramp solver/Q coefficients from zero over
        // 32 frames, even when a scalar CC target equals its serialized base.
        // Scalar RC futures retain that base; dynamic points skip that RC.
        #[rustfmt::skip]
        const POINTS: [usize;22] = [0,1,2,4,8,16,24,31,32,33,63,64,65,91,128,256,512,1024,8192,12288,16384,23040];
        #[rustfmt::skip]
        let cases: [(&str,&str,f64,[f32;22]);5] = [
            ("Freq",r#"DistortionType="2""#,0.1,[0.000000000e+00,1.190295079e-04,7.057777839e-04,4.934405908e-03,3.135957569e-02,8.878512681e-02,-4.344717413e-02,-1.604389399e-01,-1.574005187e-01,-1.477883756e-01,-1.652470976e-01,-1.749614179e-01,-1.789862216e-01,9.347143583e-03,-6.078448147e-02,1.796222776e-01,-9.757538885e-02,1.763360351e-01,2.021695077e-01,-1.048069000e-01,-4.701037332e-02,-1.655779630e-01]),
            ("Q",r#"DistortionType="2""#,0.75,[0.000000000e+00,9.033700917e-05,5.516947131e-04,4.102432635e-03,2.948020957e-02,1.052622423e-01,-7.034590095e-02,-3.145911396e-01,-3.199325800e-01,-3.118176758e-01,-3.695586920e-01,-4.148410857e-01,-4.474622011e-01,1.604536176e-01,-2.788140438e-02,4.054080844e-01,-1.072212383e-01,5.087450147e-01,6.072319150e-01,-3.179932833e-01,-4.897114262e-02,-3.453055322e-01]),
            ("KeyTracking",r#"DistortionType="2""#,0.75,[0.000000000e+00,1.099992514e-04,6.522777257e-04,4.563058726e-03,2.909689024e-02,8.411154151e-02,-3.777113557e-02,-1.532173902e-01,-1.515066475e-01,-1.435478628e-01,-1.551044732e-01,-1.656694859e-01,-1.708463728e-01,1.639947854e-02,-5.097339302e-02,1.706322283e-01,-8.584198356e-02,1.704588830e-01,1.895819306e-01,-1.059208736e-01,-4.623983428e-02,-1.611101925e-01]),
            ("Fat",r#"Q="0.75" Fat="0" DistortionType="2""#,1.0,[0.000000000e+00,9.033695824e-05,5.516941310e-04,4.102423787e-03,2.948007919e-02,1.052275151e-01,-7.204568386e-02,-3.220185339e-01,-3.274537325e-01,-3.186823726e-01,-4.963802993e-01,-5.601983666e-01,-6.089756489e-01,3.788838387e-01,4.212827981e-02,4.376961887e-01,-4.397104308e-02,5.611379743e-01,6.090657115e-01,-4.162817299e-01,-3.723109607e-03,-4.121481776e-01]),
            ("Drive",r#"Q="0.75" DistortionType="0""#,12.0,[0.000000000e+00,1.908639533e-04,1.188225695e-03,9.024746716e-03,6.383012980e-02,2.420392931e-01,-1.530187875e-01,-5.954233408e-01,-6.265035272e-01,-6.339635253e-01,-5.088337660e-01,-5.705873966e-01,-6.236268878e-01,2.101472914e-01,-8.439113200e-02,5.972206593e-01,-2.153246701e-01,6.967823505e-01,6.064583063e-01,-6.184144020e-01,-1.029847935e-01,-6.882558465e-01]),
        ];
        for (name, attrs, ratio, expected) in cases {
            let mut fx = filter(
                &format!(r#"Mode="0" Algorithm="1" Oversampling="0" {attrs}"#),
                1,
            );
            fx.set_note(72).unwrap();
            let mut output = Vec::new();
            for n in 0..24576 {
                let source = authored_native_lfo_point(n);
                let target = if name == "Freq" {
                    1000. * 1000f64.powf(ratio * source)
                } else {
                    ratio * source
                };
                fx.set_effective_parameter(name, &ParameterValue::Number(target), true)
                    .unwrap();
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < 3e-6,
                    "native cold matrix {name} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
        #[rustfmt::skip]
        let scalar_cases: [(f64,[f32;22]);2] = [
            (0.0,[0.000000000e+00,8.629212971e-05,5.117894616e-04,3.585818689e-03,2.306760475e-02,7.044614851e-02,-2.355593257e-02,-1.302671880e-01,-1.316603273e-01,-1.278581172e-01,-1.234239563e-01,-1.355924606e-01,-1.433449239e-01,3.279042244e-02,-2.483948320e-02,1.406688094e-01,-5.320595577e-02,1.456556320e-01,1.392019242e-01,-1.044955999e-01,-4.393297434e-02,-1.455102116e-01]),
            (0.75,[0.000000000e+00,8.629212971e-05,5.117894616e-04,3.585818689e-03,2.306760475e-02,7.044614851e-02,-2.355593257e-02,-1.302671880e-01,-1.316603273e-01,-1.278635710e-01,-1.273344457e-01,-1.394374222e-01,-1.469869018e-01,2.924316749e-02,-3.417472169e-02,1.634898782e-01,-9.252796322e-02,1.778681725e-01,1.930532604e-01,-9.867250919e-02,-1.030933037e-01,-1.794820279e-01]),
        ];
        for (target, expected) in scalar_cases {
            if target == 0. {
                // Original Bypass-only CC0 capture is bit-identical to the
                // unchanged continuous-control matrix baseline over24576 frames.
                let mut bypass = filter(
                    r#"Mode="0" Algorithm="1" Oversampling="0" DistortionType="2""#,
                    1,
                );
                bypass.set_note(72).unwrap();
                let mut output = Vec::new();
                for n in 0..24576 {
                    bypass
                        .set_effective_parameter("Bypass", &ParameterValue::Boolean(false), false)
                        .unwrap();
                    let mut io = [[0.; MAX_CHANNELS]];
                    io[0][0] = authored_sine(n, 48000.);
                    bypass.process(&mut io).unwrap();
                    output.push(io[0][0]);
                }
                for (n, native) in POINTS.into_iter().zip(expected) {
                    assert!(
                        (output[n] - native).abs() < 1e-6,
                        "native Bypass-only cold matrix frame{n}"
                    );
                }
            }
            let mut fx = filter(
                r#"Mode="0" Algorithm="1" Oversampling="0" DistortionType="2""#,
                1,
            );
            fx.set_note(72).unwrap();
            let mut output = Vec::new();
            for n in 0..24576 {
                fx.set_effective_parameter("KeyTracking", &ParameterValue::Number(target), false)
                    .unwrap();
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < 1e-6,
                    "native cold scalar matrix Tracking={target} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }

        // Independent authored warmed impulses establish Mode0 KeyTracking
        // conversion, including negative Ratio and both physical clamps.
        #[rustfmt::skip]
        let tracking_cases: [(f64,f64,f64,[f32;6]);5] = [
            (0.2,0.5,0.,[3.502391651e-02,6.514113396e-02,5.601514503e-02,2.633638680e-02,7.873310708e-03,7.036548341e-04]),
            (0.2,0.5,63.,[4.108368605e-02,7.541589439e-02,6.302244216e-02,2.568366751e-02,6.108237896e-03,3.454886028e-04]),
            (0.2,0.5,127.,[4.821702838e-02,8.713452518e-02,7.032905519e-02,2.409114875e-02,4.339240957e-03,1.407752716e-04]),
            (0.8,0.5,127.,[5.816827342e-02,1.028023586e-01,7.888303697e-02,2.098390087e-02,2.521914430e-03,3.642660522e-05]),
            (0.2,-0.5,127.,[3.075589612e-02,5.772809684e-02,5.062618479e-02,2.626123093e-02,9.187955409e-03,1.124673639e-03]),
        ];
        for (base, ratio, cc, expected) in tracking_cases {
            let mut fx = filter(
                &format!(
                    r#"Mode="0" Algorithm="1" Oversampling="0" DistortionType="2" KeyTracking="{base}""#
                ),
                1,
            );
            fx.set_note(72).unwrap();
            let target = (base + ratio * cc / 127.).clamp(0., 1.);
            let mut output = Vec::new();
            for n in 0..9000 {
                fx.set_effective_parameter("KeyTracking", &ParameterValue::Number(target), false)
                    .unwrap();
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = if n == 8192 { 0.5 } else { 0. };
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in [8192, 8193, 8194, 8199, 8207, 8223]
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (output[n] - native).abs() < 1e-7,
                    "native Tracking matrix base{base} ratio{ratio} CC{cc} frame{n}"
                );
            }
        }
    }

    #[test]
    fn authored_native_high_cutoff_q_interpolates_reciprocal_feedback_solve() {
        // Original authored 1379Hz PCM16 sine, direct Q setters0->.75 at8192
        // and back at16384, 18kHz cutoff, LP1, AlgorithmII/direct at48k.
        // Native interpolates the reciprocal solve coefficient, which cannot
        // be exchanged with interpolating the denominator at this cutoff.
        #[rustfmt::skip]
        const POINTS:[usize;22]=[8192,8193,8223,8224,8241,8255,8256,8288,8320,8448,8704,9000,12000,16384,16416,16512,16640,16701,16896,18000,20736,23040];
        #[rustfmt::skip]
        let cases:[(u8,[f32;22]);2]=[
            (1,[2.080610991e-01,1.798464656e-01,2.485674918e-01,2.490397096e-01,-2.502437532e-01,2.091641724e-01,2.305141687e-01,1.576830447e-01,4.636128992e-02,-2.434368134e-01,1.048971936e-01,-1.118588895e-01,-2.501869500e-01,-2.431959510e-01,-1.827916503e-01,1.637150794e-01,9.623236954e-02,-2.306540012e-01,1.292134076e-01,1.720395237e-01,-2.459781468e-01,-1.284771562e-01]),
            (2,[2.080611736e-01,1.798465252e-01,2.485675812e-01,2.490397990e-01,-2.502438128e-01,2.091642469e-01,2.305142581e-01,1.576831043e-01,4.636130482e-02,-2.434369028e-01,1.048972383e-01,-1.118589416e-01,-2.501870394e-01,-2.431959361e-01,-1.827916503e-01,1.637151241e-01,9.623239189e-02,-2.306540906e-01,1.292134523e-01,1.720395833e-01,-2.459782362e-01,-1.284772009e-01]),
        ];
        for (distortion, expected) in cases {
            let mut fx = filter(
                &format!(
                    r#"Freq="18000" Mode="0" Algorithm="1" Oversampling="0" DistortionType="{distortion}""#
                ),
                1,
            );
            let mut output = Vec::new();
            for n in 0..24576 {
                if n == 8192 || n == 16384 {
                    fx.set_parameter(
                        "Q",
                        &ParameterValue::Number(if n == 8192 { 0.75 } else { 0. }),
                    )
                    .unwrap();
                }
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < 1e-6,
                    "native highcut Q solve mode{distortion} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
    }

    #[test]
    fn authored_native_scalar_startup_uses_host_block_and_internal_chunks() {
        // Original authored 1379Hz PCM16 sine, LP1, at48k. Unconnected scalar
        // coefficient ramps span the host block and use <=64 internal frames.
        // AlgorithmI ramps its raw polynomial g; AlgorithmII ramps rawtan(pi*f/internalRate)
        // before each endpoint's bilinear conversion. Native float residuals
        // are bounded per independently measured solver/control combination.
        #[rustfmt::skip]
        const POINTS:[usize;26]=[0,1,3,7,15,31,32,63,64,95,127,128,191,192,217,249,253,255,256,257,265,268,296,512,1024,4095];
        #[rustfmt::skip]
        let cases:[(&str,&str,usize,f32,[f32;26]);12]=[
            ("a1-q0",r#"Algorithm="1" Oversampling="0" DistortionType="2""#,256,1e-06,[0.000000000e+00,1.130820419e-05,2.089247573e-04,2.213424770e-03,1.081344858e-02,-1.942267269e-02,-2.133641392e-02,-2.528970689e-02,-3.159736097e-02,-1.118486561e-02,2.144810557e-02,6.649165414e-03,1.062233970e-01,9.607443959e-02,5.699444562e-02,1.730920398e-03,9.698454291e-02,1.290640682e-01,1.390244067e-01,1.443760991e-01,3.329179808e-02,-4.493279755e-02,1.166495085e-01,-5.320595577e-02,1.456556320e-01,3.966958728e-03]),
            ("a1-q75",r#"Algorithm="1" Oversampling="0" Q=".75" DistortionType="2""#,256,8e-06,[0.000000000e+00,1.144072212e-05,2.146980405e-04,2.348225564e-03,1.206640992e-02,-2.709918842e-02,-2.971001528e-02,-4.580548406e-02,-5.675443634e-02,-2.661543898e-02,5.142481998e-02,1.485951617e-02,3.687176108e-01,3.398413956e-01,1.816517115e-01,-8.579823375e-02,4.133111537e-01,6.131519675e-01,6.873100400e-01,7.398242950e-01,3.090188801e-01,-1.167431474e-01,8.179454207e-01,-6.146780401e-02,7.766121626e-01,2.431763262e-01]),
            ("quiet-soft",r#"Algorithm="1" Oversampling="0" Freq="100" Q=".5" Fat=".5" Drive="6" DistortionType="0""#,256,3e-07,[0.000000000e+00,2.327287802e-06,4.276350955e-05,4.440114426e-04,2.236753237e-03,-4.177334718e-03,-4.667209461e-03,-4.888162017e-03,-6.545061711e-03,4.604017013e-04,1.227247529e-02,8.439659141e-03,4.155541584e-02,4.106064886e-02,-2.040073559e-05,-2.704642899e-02,1.229383796e-02,3.153993189e-02,4.030380398e-02,4.810310528e-02,4.309380427e-02,1.473191101e-02,6.209146231e-02,9.952808730e-03,5.473988131e-02,3.206234798e-02]),
            ("highq-soft",r#"Algorithm="1" Oversampling="0" Freq="5000" Q=".8" Drive="12" DistortionType="0""#,256,2e-05,[0.000000000e+00,2.178337018e-04,3.908612765e-03,3.704645112e-02,1.697802097e-01,-3.528306782e-01,-3.765156269e-01,-6.057613492e-01,-6.601653099e-01,-7.022142410e-01,-7.510362864e-01,-8.219976425e-01,-4.570471346e-01,-6.500540972e-01,9.511040449e-01,9.010380507e-01,9.177131057e-01,8.263369203e-01,6.868516207e-01,4.414256513e-01,-8.485567570e-01,-9.130077362e-01,-6.573821902e-01,-9.167743921e-01,4.407577217e-02,-8.767448068e-01]),
            ("highcut-hard",r#"Algorithm="1" Oversampling="0" Freq="18000" Q=".5" DistortionType="1""#,256,2e-06,[0.000000000e+00,2.660280152e-04,4.927044269e-03,4.961400479e-02,1.552467495e-01,-2.361543179e-01,-2.019187808e-01,-2.386945188e-01,-2.146815956e-01,-2.538543344e-01,-2.141696215e-01,-2.349108011e-01,2.359044738e-03,-4.270568863e-02,2.505857944e-01,2.146402597e-01,2.465842664e-01,2.149505913e-01,1.883746386e-01,1.559405029e-01,-1.754101664e-01,-2.427880615e-01,-2.168103494e-02,-2.462493479e-01,1.088679135e-01,-2.082774788e-01]),
            ("highq-linear",r#"Algorithm="1" Oversampling="0" Q=".8" DistortionType="2""#,256,9e-06,[0.000000000e+00,1.144955877e-05,2.150829678e-04,2.357212827e-03,1.214994770e-02,-2.761098929e-02,-3.026829101e-02,-4.717257619e-02,-5.843106285e-02,-2.762557752e-02,5.354713276e-02,1.553086378e-02,3.877734840e-01,3.578449786e-01,1.883655936e-01,-9.914479405e-02,4.352785349e-01,6.515299678e-01,7.325415015e-01,7.906011939e-01,3.417334557e-01,-1.151836067e-01,8.993622661e-01,-4.954123124e-02,8.267725110e-01,2.808781862e-01]),
            ("a1-os-q0",r#"Algorithm="1" Oversampling="1" DistortionType="2""#,256,2e-06,[0.000000000e+00,6.150370879e-09,6.643801953e-06,8.983418811e-04,9.967972524e-03,-1.189457532e-02,-1.510919817e-02,-5.683737341e-03,-1.334178261e-02,1.914879121e-02,5.595577136e-02,4.443844408e-02,1.160743386e-01,1.158885211e-01,-3.536332631e-03,-6.474260241e-02,3.610100970e-02,8.361151814e-02,1.037622169e-01,1.205664128e-01,9.606418759e-02,2.542665042e-02,1.447176784e-01,1.668018848e-02,1.368298978e-01,7.192029804e-02]),
            ("a1-os-hard",r#"Algorithm="1" Oversampling="1" Freq="18000" Q=".5" DistortionType="1""#,256,2e-06,[0.000000000e+00,1.174596704e-07,1.274847746e-04,1.687064022e-02,1.440618634e-01,-2.565132380e-01,-2.614123821e-01,-2.952323556e-01,-2.926279306e-01,-2.380715758e-01,-1.437421888e-01,-1.805858910e-01,1.085112542e-01,6.531966478e-02,2.271476388e-01,1.414591968e-01,2.445652783e-01,2.514538169e-01,2.425341308e-01,2.259183228e-01,-8.381736279e-02,-1.949053258e-01,8.661325276e-02,-2.045492232e-01,1.957048476e-01,-1.308391988e-01]),
            ("a0-q0",r#"Algorithm="0" Oversampling="0" DistortionType="2""#,256,8e-06,[0.000000000e+00,8.823883846e-09,8.319600056e-06,9.352095076e-04,9.809459560e-03,-1.187651884e-02,-1.495414786e-02,-6.306321360e-03,-1.373313274e-02,1.764407940e-02,5.383488908e-02,4.237821326e-02,1.148824096e-01,1.143236086e-01,-1.043746597e-03,-6.180530787e-02,3.881926090e-02,8.568369597e-02,1.054027453e-01,1.217186153e-01,9.331792593e-02,2.229475230e-02,1.435513645e-01,1.356372144e-02,1.373095661e-01,6.892423332e-02]),
            ("a0-hard",r#"Algorithm="0" Oversampling="0" Freq="18000" Q=".5" DistortionType="1""#,256,2e-06,[0.000000000e+00,1.080254464e-07,1.024595986e-04,1.144418679e-02,1.034537256e-01,-1.940816641e-01,-2.082259655e-01,-3.313027620e-01,-3.433620930e-01,-2.709571123e-01,-1.656388193e-01,-2.018701434e-01,9.787444025e-02,5.357766151e-02,2.326229960e-01,1.477819532e-01,2.463334203e-01,2.505766153e-01,2.403927147e-01,2.226004303e-01,-9.028415382e-02,-1.994251907e-01,8.023406565e-02,-2.087270766e-01,1.915316284e-01,-1.367844641e-01]),
            ("b32-q0",r#"Algorithm="1" Oversampling="0" DistortionType="2""#,32,3e-07,[0.000000000e+00,8.629212971e-05,1.590582891e-03,1.649166271e-02,6.949540973e-02,-1.302671880e-01,-1.316603273e-01,-1.234239563e-01,-1.355924606e-01,-6.985714287e-02,1.510655158e-03,-2.483948320e-02,1.252555549e-01,1.095308140e-01,7.384254038e-02,3.060763003e-03,9.919524193e-02,1.309483945e-01,1.406688094e-01,1.458182037e-01,3.379635885e-02,-4.459248483e-02,1.166581288e-01,-5.320595577e-02,1.456556320e-01,3.966958728e-03]),
            ("b32-q75",r#"Algorithm="1" Oversampling="0" Q=".75" DistortionType="2""#,32,3e-06,[0.000000000e+00,9.438189591e-05,1.942460891e-03,2.457013167e-02,1.358272433e-01,-5.018182397e-01,-5.100778937e-01,-7.998852134e-01,-9.011882544e-01,2.200774848e-02,1.861204803e-01,6.657887995e-02,8.471710086e-01,8.005856276e-01,1.198337674e-01,-1.587007940e-01,3.778213561e-01,5.940016508e-01,6.754853129e-01,7.353627086e-01,3.568490148e-01,-5.955226719e-02,7.711324096e-01,-6.428296864e-02,7.765625119e-01,2.431763262e-01]),
        ];
        for (tag, attrs, block, bound, expected) in cases {
            let mut fx = filter(&format!(r#"Mode="0" {attrs}"#), 1);
            fx.set_control_block_frames(block).unwrap();
            let mut output = Vec::new();
            for n in 0..4096 {
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < bound,
                    "native scalar startup {tag} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }

        // Same-rate original PCM files and paired dry captures establish exact
        // first4096 inputs, avoiding sample interpolation at32/96k rates.
        #[rustfmt::skip]
        let rates:[(f64,&str,f32,[f32;26]);8]=[
            (32000.,r#"Algorithm="0" Oversampling="0" Freq="1000" DistortionType="2""#,1.2e-05,[0.000000000e+00,3.444921148e-08,2.299367952e-05,1.987774158e-03,9.153585881e-03,2.164603444e-03,7.627828512e-03,2.553758211e-02,1.619650982e-02,-5.987991393e-02,7.134255022e-02,7.938984036e-02,-3.335522488e-02,-2.339848317e-03,6.330680102e-02,2.524745651e-02,-1.133534387e-01,-1.432621479e-01,-1.427748948e-01,-1.317036301e-01,1.252099127e-01,1.412751973e-01,1.003831974e-03,-1.369242817e-01,-1.058824733e-01,1.421139687e-01]),
            (32000.,r#"Algorithm="1" Oversampling="0" Freq="1000" DistortionType="2""#,1e-06,[0.000000000e+00,2.510654485e-05,4.479426134e-04,3.956916276e-03,3.778569167e-03,1.437014528e-02,1.860173792e-02,-3.911220992e-04,-1.214405056e-02,-4.036504775e-02,8.152237535e-02,7.472382486e-02,4.375435412e-02,7.175463438e-02,1.199352443e-01,-6.729388982e-02,-1.440480798e-01,-1.229590625e-01,-9.807015210e-02,-6.582064927e-02,1.455745995e-01,8.712376654e-02,-9.006436169e-02,-7.564468682e-02,-2.051620372e-02,1.331056505e-01]),
            (32000.,r#"Algorithm="1" Oversampling="1" Freq="1000" DistortionType="2""#,1.5e-06,[0.000000000e+00,2.442012281e-08,1.877187060e-05,1.944174175e-03,9.658543393e-03,1.578803989e-03,7.283537649e-03,2.748467959e-02,1.798586547e-02,-6.213094667e-02,7.204965502e-02,8.086950332e-02,-3.706838936e-02,-5.824822001e-03,6.035079807e-02,2.999788336e-02,-1.107043549e-01,-1.430670321e-01,-1.438914090e-01,-1.340635568e-01,1.229394674e-01,1.429234594e-01,5.968707148e-03,-1.390025765e-01,-1.095638350e-01,1.412893087e-01]),
            (32000.,r#"Algorithm="1" Oversampling="1" Freq="12000" Q=".5" DistortionType="1""#,3e-06,[0.000000000e+00,3.113956382e-07,2.404732804e-04,2.434142493e-02,7.722477615e-02,1.466919333e-01,1.942598820e-01,-1.767302752e-01,-2.510733008e-01,9.671900421e-03,1.761194468e-01,1.119123250e-01,2.108804882e-01,2.448125482e-01,2.577050328e-01,-2.035662830e-01,-2.339137048e-01,-1.458893716e-01,-8.420370519e-02,-1.636403799e-02,2.214385122e-01,5.798053741e-02,-2.253479064e-01,-3.435566276e-02,6.798219681e-02,1.715847403e-01]),
            (96000.,r#"Algorithm="0" Oversampling="0" Freq="1000" DistortionType="2""#,6.5e-06,[0.000000000e+00,4.261053164e-06,6.556075095e-05,6.824753364e-04,5.365895573e-03,2.126106620e-02,2.162667736e-02,-3.993261606e-02,-4.128613696e-02,5.404496565e-02,-6.187672168e-02,-6.669186056e-02,-5.452191457e-02,-6.373907626e-02,-3.073339723e-02,5.979112536e-02,1.050671283e-02,-1.552939415e-02,-2.851443738e-02,-4.128399119e-02,-1.236271709e-01,-1.400518864e-01,8.955042064e-02,1.409956366e-01,-5.590795353e-02,-1.306407452e-01]),
            (96000.,r#"Algorithm="1" Oversampling="0" Freq="1000" DistortionType="2""#,2e-06,[0.000000000e+00,2.856672381e-06,5.392286766e-05,6.334506324e-04,5.286063068e-03,2.173827216e-02,2.215293236e-02,-4.071767628e-02,-4.217540100e-02,5.459990725e-02,-6.205981597e-02,-6.706103683e-02,-5.318896845e-02,-6.259163469e-02,-3.343420848e-02,6.272234768e-02,1.355289202e-02,-1.256755181e-02,-2.562984638e-02,-3.851654381e-02,-1.224962398e-01,-1.397411972e-01,8.749467134e-02,1.407762170e-01,-5.330055580e-02,-1.298071742e-01]),
            (96000.,r#"Algorithm="1" Oversampling="1" Freq="1000" DistortionType="2""#,3e-06,[0.000000000e+00,3.257742742e-09,3.450642680e-06,2.881340624e-04,4.004747141e-03,2.195967361e-02,2.272849157e-02,-3.658633307e-02,-3.873056546e-02,4.740198329e-02,-4.967938736e-02,-5.568588898e-02,-3.114877269e-02,-4.132160544e-02,-5.751305073e-02,8.679796755e-02,4.198284820e-02,1.665198989e-02,3.616502509e-03,-9.517194703e-03,-1.037884876e-01,-1.278487146e-01,6.173393875e-02,1.294665188e-01,-2.444276772e-02,-1.132333279e-01]),
            (96000.,r#"Algorithm="0" Oversampling="0" Freq="18000" Q=".5" DistortionType="1""#,3e-06,[0.000000000e+00,5.206274000e-05,8.084048750e-04,8.455298841e-03,6.331721693e-02,1.594073027e-01,1.522250324e-01,-2.438932806e-01,-2.195954770e-01,1.887577772e-01,-2.135767341e-01,-1.968407333e-01,-2.544927001e-01,-2.501664162e-01,1.991100311e-01,-1.482405961e-01,-2.103506476e-01,-2.317064106e-01,-2.396470308e-01,-2.450190932e-01,-2.230780721e-01,-1.824758053e-01,2.503724098e-01,1.769981384e-01,-2.515680790e-01,-2.093728930e-01]),
        ];
        for (rate, attrs, bound, expected) in rates {
            let p = parse_program(&format!(
                r#"<Program><XpanderFilter Mode="0" {attrs}/></Program>"#
            ))
            .unwrap();
            let mut fx = XpanderFilter::new(&p.nodes[1], 1, rate).unwrap();
            let mut output = Vec::new();
            for n in 0..4096 {
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, rate);
                fx.process(&mut io).unwrap();
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < bound,
                    "native scalar startup rate{rate} {attrs} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
    }
}
