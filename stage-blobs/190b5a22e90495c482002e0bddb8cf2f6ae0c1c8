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

pub const FIDELITY_DIAGNOSTIC: &str = "UVI Xpander shapes, both solvers and saturation have authored native comparisons at 48 kHz; quiet rate paths are compared at 8/32/44.1/48/64/96/192 kHz; scalar cutoff transitions are compared at 32/48/96 kHz; Q/Fat transitions, scalar KeyTracking with the AlgorithmII direct path, scalar soft/hardDrive with both solvers, and dynamic softDrive with AlgorithmII direct/oversampled paths are compared at 48 kHz, with native rounding residuals up to 3e-5; matrix-connected Freq/Q/Fat/Drive/KeyTracking and Bypass-only cold starts are compared with AlgorithmII direct at 48 kHz; plain scalar cold starts are compared at 32/48/96 kHz with both solvers and at 32/256-frame blocks at 48 kHz; scalar pending-control Bypass resumes are compared at 48 kHz with AlgorithmII direct and AlgorithmI forced2x; other cold configurations, matrix Bypass resumes, other live transitions, partial control-block timing, cross-rate saturation and exact float-rounding parity remain unverified";
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
        if frame.is_multiple_of(block) && target != self.future && increment.abs() < 1e-6 {
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
    scalar_resume: Option<(usize, [f64; 3], f64)>,
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
            scalar_resume: None,
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
        if name == "Bypass" && value == 0. && self.processed && !self.matrix_initialization {
            self.scalar_resume = Some((
                self.frame,
                self.frequency_ramp.map(f64::from),
                f64::from(self.q_ramp),
            ));
            for index in 0..5 {
                self.controls[index].future = self.position(index);
                self.controls[index].settle_begin = None;
            }
            self.fat = self.parameters["Fat"];
            self.tracking = self.parameters["KeyTracking"];
            self.cutoff_point = self.parameters["Freq"];
            self.pending_drive = Some(self.parameters["Drive"]);
        }
        if let Some(index) = control && (self.processed || matrix) {
            self.controls[index].settle_begin = None;
            return Ok(());
        }
        // Bypass freezes audio state; scalar re-enable synchronizes pending values.
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
        let (begin, initial, initial_q) = self.scalar_resume.unwrap_or((
            0,
            [
                0.,
                if self.parameters["Algorithm"] == 0. {
                    1.
                } else {
                    0.
                },
                self.feedback_factor,
            ],
            0.,
        ));
        let elapsed = self.frame - begin;
        if !elapsed.is_multiple_of(span) {
            return Ok(());
        }
        self.configure_frequency(self.parameters["Freq"])?;
        if elapsed == 0 {
            self.frequency_end = initial;
            self.q_end = initial_q;
        }
        let start = self.frequency_end;
        let start_q = self.q_end;
        let proportion = ((elapsed + span) as f64 / self.block_frames as f64).min(1.);
        let (stage, pole) = if self.parameters["Algorithm"] == 0. {
            (
                initial[0] + (self.stage - initial[0]) * proportion,
                initial[1] + (self.pole - initial[1]) * proportion,
            )
        } else {
            let raw_begin = initial[0] / (1. - initial[0]);
            let raw = raw_begin + (self.stage / (1. - self.stage) - raw_begin) * proportion;
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
        let feedback_begin = 4. * initial_q * initial[2];
        let feedback_end = 4. * self.parameters["Q"] * self.feedback_factor;
        self.q_end = (feedback_begin + (feedback_end - feedback_begin) * proportion)
            / (4. * self.feedback_factor);
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
        if let Some((begin, _, _)) = self.scalar_resume {
            if self.frame - begin < self.block_frames {
                return self.scalar_startup_tick();
            }
            self.scalar_resume = None;
        }
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
            for (next, increment) in next_frequency.iter_mut().zip(self.frequency_increment) {
                *next += increment;
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
                for (phase, &coeff) in phase_coeffs[..phases].iter().enumerate() {
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
            [7.367_460_4e-2, 8.607_654e-2, 2.080_19e-2, 3.559_283e-3, ],
            [7.381_628_3e-3, 4.266_845_4e-2, 4.140_25e-2, 1.180_872e-2, ],
            [6.666_701_5e-4, 1.317_025_5e-2, 3.402_062e-2, 2.167_098_6e-2, ],
            [5.604_676e-5, 2.891_889_8e-3, 1.899_895_8e-2, 2.687_585e-2, ],
            [5.446_836_4e-1, 9.556_871e-2, -9.997_792e-2, -3.517_939_5e-2, ],
            [4.783_906_6e-1, 5.216_058_7e-2, -7.937_732e-2, -2.692_995_8e-2, ],
            [4.188_126_6e-1, 3.825_064e-2, -5.139_485_7e-2, -2.854_279_2e-2, ],
            [3.653_389_8e-1, 4.356_061e-2, -3.105_217_2e-2, -3.481_302_8e-2, ],
            [1.325_859_3e-1, 8.681_619e-2, -4.120_119_7e-2, -1.649_887e-2, ],
            [2.441_733_9e-2, 7.687_934_5e-2, -3.055_914_9e-2, -1.862_961_8e-2, ],
            [4.857_723e-1, 9.482_901_5e-2, -3.797_482_3e-2, -1.512_124_4e-2, ],
            [3.776_037_4e-1, 8.489_221e-2, -2.733_277e-2, -1.725_198_3e-2, ],
            [4.389_576e-1, 1.267_452_8e-1, -2.924_920_6e-2, -5.812_959_7e-2, ],
            [5.530_554_4e-2, 2.552_515_5e-2, 2.472_231e-2, -9.344_351e-3, ],
            [-4.710_090_8e-1, -9.492_128e-3, 1.207_798e-1, 3.873_867_5e-2, ],
            [3.531_863_7e-1, 8.012_813_5e-3, 3.226_355_9e-3, 1.377_628_2e-3, ],
            [6.106_234_5e-4, 1.027_836_6e-2, 1.502_166_5e-2, -5.204_863_4e-3, ],
            [5.347_367_4e-2, -5.309_940_3e-3, -2.034_268_5e-2, 6.270_238_7e-3, ],
            [5.957_801e-2, 1.390_989_4e-2, -2.798_247_5e-2, 1.612_832_3e-3, ],
            [6.714_958e-3, 2.949_82e-2, 7.381_878e-3, -9.862_268e-3, ],
            [-5.891_134e-2, -7.396_43e-4, 6.200_310_2e-2, 2.005_815_3e-2, ],
            [6.160_381_7e-3, 2.211_172_3e-2, 1.135_916_9e-2, 2.221_844_5e-2, ],
            [3.714_433_3e-1, 6.278_044e-2, -3.869_194_5e-2, -3.947_043_8e-2, ],
            [2.163_371_9e-1, 1.987_371_6e-2, -2.128_407_7e-2, 4.261_495e-3, ],
            [5.520_652_5e-1, 1.382_371_3e-1, -5.857_543e-2, -2.337_067_6e-2, ],
            [6.122_539e-1, 1.624_253_8e-1, -7.153_622e-2, -2.696_270_3e-2, ],
            [-3.607_467_4e-1, -2.842_273_2e-2, 3.007_207_6e-2, 3.803_894_7e-2, ],
            [2.447_338_6e-2, 7.977_123e-2, -1.156_018_5e-2, 8.246_231e-3, ],
            [3.668_651e-1, 4.836_562_3e-2, -3.296_208_4e-2, -3.597_737e-2, ],
            [1.072_526_65e-1, -5.480_699e-3, -3.317_454e-2, 9.938_044_5e-3, ],
            [-9.585_696e-2, -2.241_313_3e-2, 3.249_183_7e-2, 3.907_216_3e-2, ],
            [1.582_130_4e-3, 7.696_848_4e-3, 1.708_901_1e-2, 2.571_150_1e-2, ],
            [3.897_563_5e-1, 1.204_399_9e-1, -6.161_129e-2, -5.344_265e-2, ],
            [2.795_808_4e-2, 1.790_175_8e-2, 1.987_198_6e-2, -7.274_606_3e-3, ],
            [3.913_385e-1, 1.281_368_3e-1, -4.452_226e-2, -2.773_113_2e-2, ],
            [9.902_122e-2, 3.780_684_2e-2, 1.686_189e-3, 1.235_083_4e-2, ],
            [3.729_834_3e-1, 6.830_838e-2, -3.585_213e-2, -3.391_582e-2, ],
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
            (100., [5.109_531_6e-7, 7.757_826e-5, 1.812_436_4e-3]),
            (500., [2.478_336_4e-4, 1.090_902_3e-2, 1.743_830_7e-3]),
            (5000., [1.475_795_2e-1, 1.416_064_1e-5, -3.334_358_3e-9]),
            (10000., [8.135_784e-2, 5.588_732_3e-4, 1.067_719_1e-8]),
            (20000., [5.733_842_8e-2, 6.801_759e-3, 1.000_533_2e-6]),
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
            (0.25, 0., 2.064_287_1e-2),
            (0.25, 0.5, 3.102_572_6e-2),
            (0.25, 1., 4.140_860_6e-2),
            (0.5, 0., 1.473_129_1e-2),
            (0.5, 0.5, 2.955_027_3e-2),
            (0.5, 1., 4.436_921_3e-2),
            (0.75, 0., 9.134_986e-3),
            (0.75, 0.5, 2.291_902_3e-2),
            (0.75, 1., 3.670_306_5e-2),
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
            [1.893_573_5e-4, 7.173_242e-3, 4.259_801e-2, 2.153_839_2e-2, ],
            [1.617_650_3e-4, 5.923_941e-3, 3.397_165_2e-2, 1.485_212e-2, ],
            [2.251_745_8e-4, 1.161_030_1e-2, 7.414_852e-2, 3.670_186e-2, ],
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
            (-20., [1.253_645_1e-1, 1.250_000_3e-1, 1.250_000_3e-1]),
            (-12., [1.272_943_2e-1, 1.25e-1, 1.25e-1]),
            (-6., [1.340_616_6e-1, 1.250_000_1e-1, 1.250_000_1e-1]),
            (0., [1.599_741_9e-1, 1.250_000_1e-1, 1.250_000_9e-1]),
            (6., [2.489_228_1e-1, 2.494_079_9e-1, 1.250_002_4e-1]),
            (12., [4.613_366_4e-1, 4.976_340_2e-1, 1.250_006_1e-1]),
            (18., [6.438_659e-1, 9.929_110_4e-1, 1.250_023_1e-1]),
            (20., [6.346_541e-1, 1.000_000_1, 1.250_037_1e-1]),
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
            (1000., 0, 0, [1.851_358_3e-5, 1.282_612_6e-3, 8.696_347e-3, 2.833_033e-2, ], ),
            (1000., 0, 1, [1.431_576_1e-5, 9.917_894e-4, 6.724_512e-3, 2.190_632_6e-2, ], ),
            (1000., 0, 2, [5.726_105_5e-5, 3.967_019_3e-3, 2.689_711_4e-2, 8.762_222_5e-2, ], ),
            (1000., 1, 0, [6.092_974_6e-9, 8.404_527e-5, 5.658_171e-3, 4.102_279_6e-2, ], ),
            (1000., 1, 1, [4.781_89e-9, 7.184_061e-5, 4.690_510_7e-3, 3.279_198_7e-2, ], ),
            (1000., 1, 2, [4.781_891_3e-9, 9.442_205e-5, 8.994_814e-3, 7.108_838e-2, ], ),
            (5000., 0, 0, [5.269_242e-3, 1.334_217_2e-1, -4.037_206e-2, 7.876_557e-2, ], ),
            (5000., 0, 1, [4.074_479e-3, 1.030_734_5e-1, -3.394_839e-2, 6.143_784e-2, ], ),
            (5000., 0, 2, [1.629_735e-2, 4.122_794e-1, -1.357_887_2e-1, 2.457_428e-1, ], ),
            (5000., 1, 0, [2.333_658_4e-6, 2.141_584_6e-2, 2.914_600_7e-1, -9.739_784e-3, ], ),
            (5000., 1, 1, [1.832_091_7e-6, 1.835_69e-2, 2.377_878_1e-1, 1.574_965_8e-2, ], ),
            (5000., 1, 2, [1.832_092_4e-6, 2.497_757_6e-2, 4.550_974_7e-1, 1.311_173_9e-1, ], ),
            (10000., 0, 0, [4.152_769e-2, 7.396_913e-2, 1.617_198e-1, -8.176_570_4e-2, ], ),
            (10000., 0, 1, [3.211_159e-2, 4.463_705_8e-2, 1.361_468_1e-1, -6.739_572_4e-2, ], ),
            (10000., 0, 2, [1.284_418_7e-1, 1.785_428_2e-1, 5.445_675e-1, -2.695_733e-1, ], ),
            (10000., 1, 0, [2.255_894_1e-5, 1.340_845_5e-1, -1.284_386_8e-1, 3.184_337e-2, ], ),
            (10000., 1, 1, [1.771_341_5e-5, 1.157_508e-1, -1.170_769_9e-1, 5.810_539e-2, ], ),
            (10000., 1, 2, [1.771_342e-5, 1.562_394_5e-1, -7.318_278e-1, 4.001e-1, ], ),
            (20000., 0, 0, [2.315_673_8e-1, 5.392_458_7e-2, -9.113_463e-2, -1.067_782_1e-2, ], ),
            (20000., 0, 1, [1.790_611_6e-1, 7.030_776e-2, -8.976_515e-2, 9.141_475e-3, ], ),
            (20000., 0, 2, [7.162_196_6e-1, 2.812_186_5e-1, -3.590_467e-1, 3.656_211_5e-2, ], ),
            (20000., 1, 0, [1.623_057_5e-4, 4.514_210_2e-1, 6.777_848e-2, 1.481_978_3e-1, ], ),
            (20000., 1, 1, [1.273_745_9e-4, 3.878_556e-1, 4.268_448e-2, 1.396_763_8e-1, ], ),
            (20000., 1, 2, [1.273_746_3e-4, 4.596_724_2e-1, 6.881_807e-1, 7.403_403e-1, ], ),
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
            (0, [6.150_990_4e-2, 8.879_322_6e-2, 5.252_085_3e-2, 1.837_535e-2, ], ),
            (4, [9.384_596_3e-1, -8.879_322_6e-2, -5.252_085_3e-2, -1.837_535e-2, ], ),
            (8, [1.154_526_4e-1, 1.089_797_24e-1, 9.145_07e-3, -3.550_702_3e-2, ], ),
            (13, [5.149_829_8e-2, 2.308_242e-2, 1.312_654_9e-2, 2.991_638_5e-2, ], ),
            (26, [-7.732_119_6e-1, 1.922_334_4e-1, 1.312_138_6e-2, -2.757_279_8e-2, ], ),
            (36, [7.798_84e-1, -1.485_916e-1, 1.201_692_7e-2, 1.601_552_8e-2, ], ),
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
            (0, 0, -20., 0, [7.291_509e-2, 3.605_15e-2]),
            (0, 0, 6., 0, [4.310_429_5e-2, 2.668_719e-2]),
            (0, 0, 20., 0, [7.922_479e-2, 8.400_911e-2]),
            (0, 0, -20., 1, [7.414_863e-2, 3.670_191_4e-2]),
            (0, 0, -12., 1, [7.414_865_5e-2, 3.670_194_4e-2]),
            (0, 0, -6., 1, [5.681_229_8e-2, 2.662_664_5e-2]),
            (0, 0, 6., 1, [4.325_014_4e-2, 2.233_894_4e-2]),
            (0, 0, 20., 1, [5.786_622_3e-2, 6.228_123_2e-2]),
            (1, 0, -20., 0, [8.392_72e-2, 1.238_979_5e-2]),
            (1, 0, 6., 0, [2.684_996_8e-2, 3.997_239_7e-3]),
            (1, 0, 20., 0, [7.214_827e-2, 1.200_974_8e-2]),
            (1, 0, -20., 1, [8.762_261e-2, 1.292_310_75e-2]),
            (1, 0, -12., 1, [8.721_065_5e-2, 1.286_235_5e-2]),
            (1, 0, -6., 1, [4.370_887e-2, 6.446_448_6e-3]),
            (1, 0, 6., 1, [2.190_632_6e-2, 3.230_877_7e-3]),
            (1, 0, 20., 1, [2.190_632_6e-2, 3.230_877_7e-3]),
            (1, 1, -20., 0, [6.992_18e-2, 4.005_253_3e-2]),
            (1, 1, 6., 0, [4.139_041_5e-2, 2.902_527e-2]),
            (1, 1, 20., 0, [7.568_947e-2, 8.733_263_6e-2]),
            (1, 1, -20., 1, [7.108_85e-2, 4.076_562e-2]),
            (1, 1, -12., 1, [7.108_854e-2, 4.076_564_3e-2]),
            (1, 1, -6., 1, [5.477_229e-2, 2.985_592_7e-2]),
            (1, 1, 6., 1, [4.153_827_2e-2, 2.470_047e-2]),
            (1, 1, 20., 1, [5.537_178_4e-2, 6.487_253e-2]),
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
            (0, 0, 4, [3.748_537_3e-3, 1.182_188, -1.303_091_5e-1, 6.289_122e-2]),
            (0, 0, 14, [-3.540_632e-3, -9.857_723e-1, 2.193_879_2e-1, 1.818_162_6e-2]),
            (0, 0, 36, [3.199_949_6e-3, 7.381_969_7e-1, -2.821_130_1e-2, 2.906_479_5e-2]),
            (0, 1, 4, [2.955_713_3e-3, 9.682_01e-1, -7.691_084e-2, 4.689_553_8e-2]),
            (0, 1, 14, [-2.791_957_5e-3, -8.024_293e-1, 1.452_713_3e-1, 1.917_634_2e-2]),
            (0, 1, 36, [2.523_57e-3, 5.945_191_4e-1, -4.090_666_8e-4, 2.370_541_2e-2]),
            (0, 2, 4, [2.955_714_5e-3, 2.188_153, -3.495_831e-1, 8.762_288e-2]),
            (0, 2, 14, [-2.791_958_4e-3, -1.892_167_2, 4.818_724_4e-1, 5.905_896_4e-2]),
            (0, 2, 36, [2.523_571e-3, 1.498_355_3, -8.838_701e-2, 2.690_687_2e-2]),
            (1, 0, 4, [3.797_834_5e-3, 1.201_423_6, -1.328_774_4e-1, 6.180_604e-2]),
            (1, 0, 14, [-3.656_357_8e-3, -1.025_218_8, 2.238_986_2e-1, 1.929_570_2e-2]),
            (1, 0, 36, [3.408_608_3e-3, 7.821_110_5e-1, -2.934_472_6e-2, 3.098_369e-2]),
            (1, 1, 4, [2.994_23e-3, 9.845_187_7e-1, -7.835_574_4e-2, 4.598_508_4e-2]),
            (1, 1, 14, [-2.882_864_3e-3, -8.351_407e-1, 1.478_171e-1, 2.011_218e-2]),
            (1, 1, 36, [2.687_812_3e-3, 6.297_965e-1, 3.408_193_6e-4, 2.531_714_6e-2]),
            (1, 2, 4, [2.994_231e-3, 2.215_538_3, -3.545_993_6e-1, 8.512_677e-2]),
            (1, 2, 14, [-2.882_865_3e-3, -1.958_007_1, 4.919_820_4e-1, 6.152_367_6e-2]),
            (1, 2, 36, [2.687_813_2e-3, 1.585_525_9, -9.091_911e-2, 2.851_121e-2]),
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
            9.875_326e-1,
            2.474_094_2e-2,
            2.248_827_4e-2,
            1.974_010_8e-2,
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
            (0, 0, [1.471_140_8e-1, 1.483_296_2e-1, 1.497_111_6e-1, 1.524_470_2e-1]),
            (1, 0, [1.487_162_4e-1, 1.499_252_3e-1, 1.512_991e-1, 1.540_183_9e-1]),
            (1, 1, [1.482_522_8e-1, 1.494_636_1e-1, 1.508_403_4e-1, 1.535_664_5e-1]),
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
            (8000., 0, 0, 1000., [7.445_946e-6, 2.819_475_7e-2, 1.595_221_2e-1, -1.066_613_9e-4]),
            (8000., 0, 0, 20000., [1.132_947_7e-3, 5.956_187e-1, 2.204_459_5e-1, 4.343_529_8e-2]),
            (8000., 1, 0, 1000., [7.359_317e-3, 1.635_637_3e-1, 8.527_668e-2, 6.241_805e-9]),
            (8000., 1, 0, 20000., [9.875_326e-1, 2.397_099_3e-2, 2.248_827_4e-2, 1.502_786_9e-2]),
            (8000., 1, 1, 1000., [1.568_311_2e-6, 1.313_837e-2, 1.751_604_7e-1, 1.132_875_1e-4]),
            (8000., 1, 1, 20000., [1.347_887_1e-3, 6.209_265e-1, 2.012_059e-1, 5.662_286_7e-2]),
            (32000., 0, 0, 1000., [4.941_406e-8, 3.051_189_7e-4, 1.112_581e-2, 2.060_638_7e-2]),
            (32000., 0, 0, 20000., [6.293_46e-4, 4.956_775_3e-1, 2.058_652_5e-1, 6.312_858e-3]),
            (32000., 1, 0, 1000., [6.462_593_6e-5, 3.986_877_4e-3, 2.148_096_3e-2, 1.669_347_8e-2]),
            (32000., 1, 0, 20000., [9.875_326e-1, 2.397_099_3e-2, 2.248_827_4e-2, 1.502_786_9e-2]),
            (32000., 1, 1, 1000., [1.043_744_8e-8, 1.330_383e-4, 8.930_973e-3, 2.145_482_6e-2]),
            (32000., 1, 1, 20000., [2.197_859_1e-4, 3.813_278_4e-1, 1.445_104e-1, -3.011_486e-2]),
            (96000., 0, 0, 1000., [5.503_025e-6, 1.602_671_3e-4, 9.313_444_6e-4, 1.220_588_2e-2]),
            (96000., 0, 0, 20000., [1.571_040_9e-1, 1.595_209_4e-1, 2.424_335_8e-3, 3.943_208_5e-19]),
            (96000., 1, 0, 1000., [1.009_663_4e-6, 7.875_332e-5, 6.754_809_3e-4, 1.197_878_1e-2]),
            (96000., 1, 0, 20000., [3.553_483_3e-2, 2.806_751_7e-1, 3.653_521_6e-3, 4.168_695e-22]),
            (96000., 1, 1, 1000., [1.651_198_6e-10, 3.320_537_2e-6, 2.414_509_2e-4, 1.106_418_9e-2]),
            (96000., 1, 1, 20000., [9.332_592_5e-6, 7.965_664_6e-2, 9.594_685e-2, -9.640_034_5e-6]),
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
            (0, 48, 1., 20., [4.206_134_7e-7, 7.995_342e-4, 1.415_344_2e-3, 1.251_988_6e-3]),
            (0, 60, 0., 20., [8.409_569_4e-7, 1.597_714_6e-3, 2.816_081_5e-3, 2.413_784_2e-3]),
            (0, 72, 1., 10000., [5.047_72e-4, 5.301_831_4e-1, 2.042_247_2e-1, -9.929_586e-3]),
            (0, 72, 1., 20000., [7.278_415e-4, 6.075_282_7e-1, 1.927_149_7e-1, -2.786_644_4e-2]),
            (0, 60, 0., 20000., [5.047_72e-4, 5.301_831_4e-1, 2.042_247_2e-1, -9.929_586e-3]),
            (0, 84, 1., 20000., [7.410_088_4e-4, 6.106_308_7e-1, 1.898_769_9e-1, -2.891_639_6e-2]),
            (1, 48, 1., 20., [6.540_708e-4, 1.303_868e-3, 1.297_058_9e-3, 1.256_943_8e-3]),
            (1, 60, 0., 20., [1.307_286_9e-3, 2.597_519_7e-3, 2.570_460_8e-3, 2.413_924_3e-3]),
            (1, 72, 1., 10000., [7.886_754e-1, 1.111_111_5e-1, 1.234_568_2e-2, 2.323_056_4e-8]),
            (1, 72, 1., 20000., [9.968_687e-1, 6.165_740_5e-3, 6.012_714e-3, 5.171_092_7e-3]),
            (1, 60, 0., 20000., [7.886_754e-1, 1.111_111_5e-1, 1.234_568_2e-2, 2.323_056_4e-8]),
            (1, 84, 1., 20000., [9.968_687e-1, 6.165_740_5e-3, 6.012_714e-3, 5.171_092_7e-3]),
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
            (48000., 0, 0, false, [-2.914_619_8e-2, -2.673_291_6e-2, -1.328_595_3e-2, -1.772_941e-2, -1.376_130_3e-2, 1.330_240_9e-2, 1.372_344e-1, 7.283_778e-2, 1.550_026e-1, 1.723_558_8e-1, -1.936_173_6e-1, 1.399_706_5e-1, 4.520_146e-2, 1.423_129_4e-1, 1.005_557_9e-1, -8.294_066e-4, -2.335_441_5e-2]),
            (48000., 1, 0, false, [-2.695_613e-2, -2.958_914_6e-2, -2.267_989e-2, -2.572_439e-2, -2.562_059_3e-2, -9.522_413e-3, 1.812_171_3e-1, 1.491_718_3e-1, 8.118_621e-2, 2.119_849_2e-1, -1.387_775_8e-1, 1.960_761_8e-1, -4.459_050_3e-2, 6.455_665e-2, 1.581_518e-1, -1.822_778e-2, -2.873_082e-2]),
            (48000., 1, 1, false, [-2.961_795_8e-2, -2.611_745e-2, -1.126_209_4e-2, -1.600_691e-2, -1.108_972_3e-2, 1.818_878_8e-2, 1.236_543_66e-1, 5.327_572_3e-2, 1.685_453_2e-1, 1.590_399_1e-1, -2.018_356_2e-1, 1.235_791_3e-1, 6.506_456_4e-2, 1.570_720_4e-1, 8.476_134e-2, 2.936_375e-3, -2.219_420_3e-2]),
            (32000., 1, 0, false, [-4.891_168_3e-3, 1.377_853_75e-2, -2.942_547e-2, -2.793_532_6e-2, 4.051_456e-2, 3.711_301e-2, -8.123_973e-2, -2.168_188_1e-1, 2.204_321_8e-1, 1.191_123_3e-1, -9.990_086_4e-2, -1.723_492e-1, -1.414_272e-1, 2.190_142_1e-1, 1.681_780_7e-1, -1.818_583_5e-2, 2.182_248_8e-2]),
            (96000., 1, 0, false, [1.871_669_9e-2, 1.330_251e-2, -3.587_396_1e-3, -6.227_198_5e-3, 1.563_376_4e-3, -8.261_201e-3, 9.566_828e-2, 7.610_698e-2, -1.503_905_2e-1, 2.371_023_4e-2, -1.693_166_5e-1, -2.720_025e-3, 1.954_396_5e-1, -1.659_001_4e-1, -1.212_815_5e-2, -6.201_712e-2, 1.080_587_1e-2]),
            (48000., 0, 0, true, [-1.387_927_8e-1, -1.206_795_35e-1, -4.833_376e-2, -7.135_378e-2, -4.815_973_7e-2, 7.477_38e-2, 5.567_879e-1, 4.692_532_7e-1, -3.227_325_2e-3, 5.364_907e-1, -1.674_909e-1, 5.270_934_7e-1, -2.824_700_5e-1, -4.568_889_7e-2, 5.612_660_6e-1, 1.968_392_2e-2, -1.014_690_9e-1]),
            (48000., 1, 0, true, [-1.351_801_2e-1, -1.402_986_2e-1, -9.722_124e-2, -1.141_466_35e-1, -1.083_790_3e-1, -3.460_383e-2, 4.770_365_4e-1, 5.290_631e-1, -2.028_498_4e-1, 4.807_025e-1, 5.103_003_6e-2, 5.215_846_3e-1, -4.209_11e-1, -2.379_729_4e-1, 5.357_634_4e-1, -6.819_412e-2, -1.325_182_3e-1]),
            (48000., 1, 1, true, [-1.396_245_2e-1, -1.163_613_95e-1, -3.751_552e-2, -6.186_178e-2, -3.436_038e-2, 9.723_79e-2, 5.633_79e-1, 4.460_301e-1, 4.787_544_5e-2, 5.350_935_5e-1, -2.199_341_7e-1, 5.151_939e-1, -2.446_179_2e-1, 3.943_162_5e-3, 5.566_776_4e-1, 3.823_125e-2, -9.457_526e-2]),
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
            (5331., 12416, [-6.331_996e-2, -2.187_654_2e-1, 4.775_892_2e-2, -1.005_153e-1, 1.468_517_8e-1, 2.090_162_2e-1, 8.434_538_5e-2]),
            (5332., 12448, [-6.336_339_6e-2, -2.187_682_1e-1, 4.773_806e-2, -1.004_933e-1, 1.468_297_5e-1, 2.090_121_7e-1, 8.438_851e-2]),
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
            (1024, -2.172_172_8e-2),
            (2048, 5.239_425_6e-2),
            (4096, 1.067_571e-1),
            (6144, 8.269_79e-2),
            (8192, -3.372_157_4e-2),
            (10000, -7.092_898e-2),
            (12288, -9.548_68e-3),
            (14336, 4.036_398e-2),
            (16384, 2.008_865e-2),
            (19000, 3.041_182_3e-2),
            (21000, -3.794_048_7e-2),
            (23040, 3.222_490_5e-2),
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
                (8192, -2.958_914_6e-2),
                (8224, -2.572_439e-2),
                (8256, -1.538_763_8e-2),
                (8320, 1.250_611e-2),
                (8448, 3.183_722_5e-2),
                (8960, 4.665_406_8e-2),
                (10000, 1.227_641_55e-1),
                (12544, 2.118_970_2e-1),
                (16384, -4.459_050_3e-2),
                (16416, 6.455_665e-2),
                (16512, 2.084_397_2e-1),
                (17408, -8.913_652_6e-2),
                (19000, 3.193_71e-2),
                (21000, -2.877_480_4e-2),
            ] } else { [
                (8192, -2.958_914_6e-2),
                (8224, -2.572_439e-2),
                (8256, -2.562_059_3e-2),
                (8320, -9.522_413e-3),
                (8448, 9.845_994e-2),
                (8960, 1.812_169_8e-1),
                (10000, 1.491_719_6e-1),
                (12544, 2.119_850_4e-1),
                (16384, -4.459_050_3e-2),
                (16416, 6.455_665e-2),
                (16512, 1.581_519_2e-1),
                (17408, -1.822_773_4e-2),
                (19000, 2.991_260_8e-2),
                (21000, -2.873_412e-2),
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
            ("Q", r#"Algorithm="0" Oversampling="0" DistortionType="2""#, 0.75, [-2.673_291_6e-2, -1.772_938_9e-2, -2.159_671_5e-2, -9.252_018e-3, -1.502_750_3e-2, 1.243_800_8e-2, 3.537_386_3e-2, 1.397_730_9e-2, -6.118_206_3e-2, -1.251_025_5e-1, -7.663_056e-2, -1.367_002_9e-1, -1.557_724_3e-1, -1.108_024_5e-2, -8.555_718e-2, -6.059_896_2e-2, -1.111_536_5e-1, 1.272_685_1e-2, 2.730_012_1e-2, -2.335_460_3e-2]),
            ("Q", r#"Algorithm="1" Oversampling="0" DistortionType="2""#, 0.75, [-2.958_914_6e-2, -2.572_439e-2, -2.793_294_8e-2, -2.155_292_4e-2, -2.628_033_4e-2, -3.761_815e-3, 2.054_419_4e-2, 4.068_473_7e-2, -3.582_415_7e-2, -1.277_585_2e-1, -1.242_450_5e-1, -1.580_065e-1, -1.524_104_9e-1, 5.111_044_6e-2, -2.841_062_3e-2, 1.417_613_1e-4, -1.266_413_3e-1, -2.836_447_4e-3, 2.984_105_8e-2, -2.873_290_9e-2]),
            ("Fat", r#"Q=".75" Fat="0" Algorithm="1" Oversampling="0" DistortionType="2""#, 1.0, [-3.801_539_5e-2, -2.759_129_7e-2, -3.227_608e-2, -1.990_851_8e-2, -2.770_158e-2, 9.742_305e-3, 4.502_347e-2, 3.382_733_5e-2, -5.895_797_5e-2, -1.370_958_2e-1, -1.242_189_3e-1, -1.580_085e-1, -1.524_105_8e-1, 5.111_044_6e-2, -2.841_010_5e-2, 1.462_326e-4, -1.312_584_7e-1, 9.868_446e-3, 3.863_156e-2, -3.433_506e-2]),
            ("Drive", r#"Q=".75" Algorithm="1" Oversampling="0" DistortionType="0""#, 12.0, [-1.405_938_1e-1, -1.086_835_2e-1, -1.173_743_6e-1, -5.544_375e-2, -7.506_777e-2, 1.836_471_4e-2, 8.393_317_5e-2, 7.058_915e-2, -6.727_292_4e-2, -1.389_325_3e-1, -1.336_054_5e-1, -1.459_614_5e-1, -1.286_227e-1, 7.779_941e-2, 5.774_398e-3, 3.285_177_4e-2, -1.436_189_1e-1, 1.232_924_1e-2, 1.413_975_4e-1, -1.301_238_2e-1]),
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
            (0.24219, 12416, [3.377_709e-2, 5.820_241_6e-2, 3.139_715_9e-3, 1.299_026_1e-2, -2.828_970_2e-2, -5.260_721_6e-2, -3.906_217_2e-2]),
            (0.24221, 12448, [3.377_837_7e-2, 5.820_549_3e-2, 3.138_023_2e-3, 1.299_108e-2, -2.828_899_8e-2, -5.261_089_7e-2, -3.906_372_6e-2]),
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
            ("Q", r#"Algorithm="1" Oversampling="0" DistortionType="2""#, 0.75, [-1.410_977_1e-1, -1.350_710_1e-1, -1.012_159_3e-1, 4.606_700_7e-2, 5.206_27e-2, 1.933_692_4e-2, 3.111_593_6e-2, -4.244_329_8e-2, 6.540_554e-2]),
            ("Fat", r#"Q=".75" Fat="0" Algorithm="1" Oversampling="0" DistortionType="2""#, 1.0, [-1.430_379_2e-1, -1.414_418_4e-1, -1.022_356_45e-1, 6.647_135e-2, 6.374_888e-2, 1.440_275_3e-2, 4.027_266e-2, -4.925_64e-2, 8.332_927_5e-2]),
            ("Drive", r#"Q=".75" Algorithm="1" Oversampling="0" DistortionType="0""#, 12.0, [-1.459_977_6e-1, -1.387_987_3e-1, -1.300_856e-1, 8.225_924_5e-2, 1.403_312e-1, 5.858_716e-2, 1.412_469_3e-1, -1.295_411_9e-1, 1.350_655e-1]),
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
            5E-1, 5.041_883_6E-1, 5.083_767E-1, 5.125_648_4E-1, 5.167_507E-1, 5.209_365E-1,
            5.251_216E-1, 5.293_024E-1, 5.334_832E-1, 5.376_624E-1, 5.418_356E-1, 5.460_088E-1,
            5.501_792E-1, 5.543_423E-1, 5.585_055E-1, 5.626_642E-1, 5.668_147E-1, 5.709_653E-1,
            5.751_095E-1, 5.792_45E-1, 5.833_804_6E-1, 5.875_072_5E-1, 5.916_251_5E-1, 5.957_430_6E-1,
            5.998_497E-1, 6.039_475E-1, 6.080_453_4E-1, 6.121_29E-1, 6.162_042_6E-1, 6.202_796E-1,
            6.243_374E-1, 6.283_877E-1, 6.324_380_6E-1, 6.364_671_6E-1, 6.404_900_6E-1, 6.445_13E-1,
            6.485_107_5E-1, 6.525_038_5E-1, 6.564_969E-1, 6.604_605E-1, 6.644_213E-1, 6.683_821_7E-1,
            6.723_089E-1, 6.762_351E-1, 6.801_593_3E-1, 6.840_485E-1, 6.879_377E-1, 6.918_22E-1,
            6.956_718_6E-1, 6.995_216_6E-1, 7.033_634E-1, 7.071_716E-1, 7.109_798E-1, 7.147_763_4E-1,
            7.185_406E-1, 7.223_048E-1, 7.260_536E-1, 7.297_716E-1, 7.334_896_3E-1, 7.371_88E-1,
            7.408_575_4E-1, 7.445_271E-1, 7.481_725E-1, 7.517_914E-1, 7.554_103E-1, 7.590_003E-1,
            7.625_663E-1, 7.661_323_5E-1, 7.696_645E-1, 7.731_755_4E-1, 7.766_865_5E-1, 7.801_584E-1,
            7.836_123E-1, 7.870_662E-1, 7.904_753_7E-1, 7.938_701E-1, 7.972_648E-1, 8.006_089E-1,
            8.039_423E-1, 8.072_758_3E-1, 8.105_525_4E-1, 8.138_227_5E-1, 8.170_93E-1, 8.203_001E-1,
            8.235_051_6E-1, 8.267_075_4E-1, 8.298_454E-1, 8.329_833E-1, 8.361_137E-1, 8.391_825E-1,
            8.422_513_6E-1, 8.453_074_7E-1, 8.483_054_6E-1, 8.513_034E-1, 8.542_832E-1, 8.572_085E-1,
            8.601_337_7E-1, 8.630_352E-1, 8.658_860_3E-1, 8.687_368_6E-1, 8.715_579_5E-1, 8.743_325_5E-1,
            8.771_072E-1, 8.798_46E-1, 8.825_428E-1, 8.852_396E-1, 8.878_941_5E-1, 8.905_115E-1,
            8.931_288E-1, 8.956_974E-1, 8.982_337E-1, 9.007_7E-1, 9.032_509E-1, 9.057_045_6E-1,
            9.081_583E-1, 9.105_497E-1, 9.129_193_4E-1, 9.152_890_4E-1, 9.175_892_5E-1, 9.198_734E-1,
            9.221_576E-1, 9.243_651_6E-1, 9.265_625_5E-1, 9.287_598E-1, 9.308_731_6E-1, 9.329_823E-1,
            9.350_894_7E-1, 9.371_092E-1, 9.391_289E-1, 9.411_402E-1, 9.430_691_6E-1, 9.449_982E-1,
            9.469_123E-1, 9.487_495E-1, 9.505_867E-1, 9.524_022_3E-1, 9.541_465E-1, 9.558_908E-1,
            9.576_064_3E-1, 9.592_567_7E-1, 9.609_070_4E-1, 9.625_218E-1, 9.640_771E-1, 9.656_324_4E-1,
            9.671_452E-1, 9.686_045_6E-1, 9.700_64E-1, 9.714_736E-1, 9.728_361_4E-1, 9.741_988E-1,
            9.755_043E-1, 9.767_692_7E-1, 9.780_342_6E-1, 9.792_348E-1, 9.804_014E-1, 9.815_680_4E-1,
            9.826_627_4E-1, 9.837_303E-1, 9.847_978_4E-1, 9.857_86E-1, 9.867_538E-1, 9.877_217E-1,
            9.886_026E-1, 9.894_701E-1, 9.903_376E-1, 9.911_107_4E-1, 9.918_774_4E-1, 9.926_434E-1,
            9.933_088E-1, 9.939_742E-1, 9.946_317_7E-1, 9.951_955E-1, 9.957_592E-1, 9.963_078_5E-1,
            9.967_695_5E-1, 9.972_312_5E-1, 9.976_706_5E-1, 9.980_3E-1, 9.983_893_6E-1, 9.987_192E-1,
            9.989_760_5E-1, 9.992_329E-1, 9.994_529_5E-1, 9.996_071_5E-1, 9.997_613_4E-1, 9.998_714_3E-1,
            9.999_228E-1, 9.999_742_5E-1, 9.999_743E-1, 9.999_229_3E-1, 9.998_715_5E-1, 9.997_616_4E-1,
            9.996_074_4E-1, 9.994_533E-1, 9.992_335E-1, 9.989_766E-1, 9.987_197_5E-1, 9.983_901E-1,
            9.980_307E-1, 9.976_713_7E-1, 9.972_321_4E-1, 9.967_704_4E-1, 9.963_087_4E-1, 9.957_603E-1,
            9.951_966E-1, 9.946_328_4E-1, 9.939_755E-1, 9.933_101_5E-1, 9.926_448E-1, 9.918_79E-1,
            9.911_123E-1, 9.903_394E-1, 9.894_718_5E-1, 9.886_043_7E-1, 9.877_236_5E-1, 9.867_558_5E-1,
            9.857_88E-1, 9.848_000_4E-1, 9.837_324_6E-1, 9.826_649_4E-1, 9.815_703_6E-1, 9.804_038E-1,
            9.792_371_4E-1, 9.780_368E-1, 9.767_718_3E-1, 9.755_069E-1, 9.742_015_6E-1, 9.728_389_4E-1,
            9.714_763E-1, 9.700_669E-1, 9.686_075_4E-1, 9.671_481E-1, 9.656_356_6E-1, 9.640_803_3E-1,
            9.625_25E-1, 9.609_104_4E-1, 9.592_601E-1, 9.576_098E-1, 9.558_943E-1, 9.541_5E-1,
            9.524_057_5E-1, 9.505_904_3E-1, 9.487_532_4E-1, 9.469_16E-1, 9.450_021E-1, 9.430_730_3E-1,
            9.411_44E-1, 9.391_329E-1, 9.371_133E-1, 9.350_935E-1, 9.329_866E-1, 9.308_774_5E-1,
            9.287_643_4E-1, 9.265_669_6E-1, 9.243_696_3E-1, 9.221_622_3E-1,
            9.198_780_7e-1, 9.175_938_4e-1, 9.152_938e-1, 9.129_241e-1, 9.105_544_7e-1, 9.081_633e-1,
            9.057_095_6e-1, 9.032_558e-1, 9.007_752e-1, 8.982_389e-1, 8.957_026e-1, 8.931_341e-1,
            8.905_168e-1, 8.878_995e-1, 8.852_451e-1, 8.825_483e-1, 8.798_514_6e-1, 8.771_128e-1,
            8.743_381_5e-1, 8.715_635e-1, 8.687_426e-1, 8.658_917_5e-1, 8.630_409e-1, 8.601_396_7e-1,
            8.572_144e-1, 8.542_892e-1, 8.513_095e-1, 8.483_115_4e-1, 8.453_135_5e-1, 8.422_575_6e-1,
            8.391_887e-1, 8.361_199e-1, 8.329_897e-1, 8.298_518e-1, 8.267_139e-1, 8.235_116e-1,
            8.203_066e-1, 8.170_996_3e-1, 8.138_294e-1, 8.105_591_5e-1, 8.072_825_7e-1, 8.039_490_6e-1,
            8.006_155_5e-1, 7.972_716e-1, 7.938_769_5e-1, 7.904_822e-1, 7.870_732_5e-1, 7.836_193e-1,
            7.801_654e-1, 7.766_937e-1, 7.731_826_3e-1, 7.696_715_6e-1, 7.661_396e-1, 7.625_736e-1,
            7.590_076e-1, 7.554_177e-1, 7.517_987_5e-1, 7.481_798_5e-1, 7.445_345e-1, 7.408_65e-1,
            7.371_954e-1, 7.334_971_4e-1, 7.297_791e-1, 7.260_611e-1, 7.223_125e-1, 7.185_482_4e-1,
            7.147_839_7e-1, 7.109_875_7e-1, 7.071_793e-1, 7.033_711e-1, 6.995_294_7e-1, 6.956_796e-1,
            6.918_297_4e-1, 6.879_455_4e-1, 6.840_564e-1, 6.801_672e-1, 6.762_430_7e-1, 6.723_169e-1,
            6.683_901_5e-1, 6.644_293e-1, 6.604_684_6e-1, 6.565_049e-1, 6.525_119e-1, 6.485_188_6e-1,
            6.445_212e-1, 6.404_983e-1, 6.364_753e-1, 6.324_462e-1, 6.283_959e-1, 6.243_455_4e-1,
            6.202_878_4e-1, 6.162_125e-1, 6.121_371_4e-1, 6.080_536e-1, 6.039_558e-1, 5.998_58e-1,
            5.957_514e-1, 5.916_335e-1, 5.875_156e-1, 5.833_888e-1, 5.792_533e-1, 5.751_178e-1,
            5.709_737e-1, 5.668_231e-1, 5.626_726e-1, 5.585_139_4e-1, 5.543_508e-1, 5.501_876_5e-1,
            5.460_173e-1, 5.418_44e-1, 5.376_708e-1, 5.334_916e-1, 5.293_108e-1, 5.251_300_3e-1,
            5.209_449_5e-1, 5.167_591_6e-1, 5.125_733_6e-1, 5.083_852e-1, 5.041_968e-1, 5.000_084_6e-1,
            4.958_201e-1, 4.916_317_5e-1, 4.874_435_7e-1, 4.832_577_7e-1, 4.790_719_7e-1, 4.748_869e-1,
            4.707_061e-1, 4.665_253e-1, 4.623_460_8e-1, 4.581_728_6e-1, 4.539_996_4e-1, 4.498_292_2e-1,
            4.456_660_7e-1, 4.415_029e-1, 4.373_441_6e-1, 4.331_936_2e-1, 4.290_431e-1, 4.248_989e-1,
            4.207_634e-1, 4.166_279_4e-1, 4.125_010_4e-1, 4.083_831_3e-1, 4.042_652_2e-1, 4.001_585_8e-1,
            3.960_607_6e-1, 3.919_629_8e-1, 3.878_793_4e-1, 3.838_040_2e-1, 3.797_286_7e-1, 3.756_708_5e-1,
            3.716_205e-1, 3.675_701_6e-1, 3.635_409_5e-1, 3.595_180_2e-1, 3.554_951e-1, 3.514_973e-1,
            3.475_042_6e-1, 3.435_112_2e-1, 3.395_475_1e-1, 3.355_866_7e-1, 3.316_258_5e-1, 3.276_99e-1,
            3.237_728_2e-1, 3.198_485_4e-1, 3.159_593_6e-1, 3.120_702e-1, 3.081_858_5e-1, 3.043_359_8e-1,
            3.004_861_2e-1, 2.966_442_7e-1, 2.928_360_7e-1, 2.890_278_7e-1, 2.852_312_3e-1, 2.814_67e-1,
            2.777_027_5e-1, 2.739_539e-1, 2.702_359e-1, 2.665_179e-1, 2.628_194_7e-1, 2.591_499e-1,
            2.554_803_5e-1, 2.518_348e-1, 2.482_159_1e-1, 2.445_969_9e-1, 2.410_069_1e-1, 2.374_408_8e-1,
            2.338_748_6e-1, 2.303_426_3e-1, 2.268_315_9e-1, 2.233_205_4e-1, 2.198_486_3e-1, 2.163_947e-1,
            2.129_407_5e-1, 2.095_315_5e-1, 2.061_368_5e-1, 2.027_421_6e-1, 1.993_979_8e-1, 1.960_644_7e-1,
            1.927_309_6e-1, 1.894_541_1e-1, 1.861_838_7e-1, 1.829_136e-1, 1.797_064e-1, 1.765_013_3e-1,
            1.732_987_8e-1, 1.701_608_9e-1, 1.670_230_3e-1, 1.638_925_7e-1, 1.608_237_3e-1, 1.577_549e-1,
            1.546_985_8e-1, 1.517_006_2e-1, 1.487_026_5e-1, 1.457_226_9e-1, 1.427_974_7e-1, 1.398_722_2e-1,
            1.369_706_4e-1, 1.341_198_1e-1, 1.312_689_5e-1, 1.284_477_4e-1, 1.256_731_2e-1, 1.228_984_9e-1,
            1.201_595_4e-1, 1.174_627_54e-1, 1.147_659_7e-1, 1.121_111_8e-1, 1.094_938_5e-1, 1.068_764_9e-1,
            1.043_077_1e-1, 1.017_714_14e-1, 9.923_512e-2, 9.675_407_4e-2, 9.430_039e-2, 9.184_667_5e-2,
            8.945_51e-2, 8.708_545_6e-2, 8.471_575_4e-2, 8.241_531e-2, 8.013_111e-2, 7.784_694_4e-2,
            7.563_922e-2, 7.344_192e-2, 7.124_466e-2, 6.913_110_6e-2, 6.702_193_6e-2, 6.491_464_4e-2,
            6.289_494e-2, 6.087_523_7e-2, 5.886_376e-2, 5.693_471_4e-2, 5.500_567e-2, 5.309_143_7e-2,
            5.125_424_3e-2, 4.941_702e-2, 4.760_134_2e-2, 4.585_704_2e-2, 4.411_274_2e-2, 4.239_684_3e-2,
            4.074_654e-2, 3.909_620_6e-2, 3.748_128e-2, 3.592_598_4e-2, 3.437_066e-2, 3.285_780_5e-2,
            3.139_841_6e-2, 2.993_899_6e-2, 2.852_922_7e-2, 2.716_660_5e-2, 2.580_398_3e-2, 2.449_828_4e-2,
            2.323_329_4e-2, 2.196_827_5e-2, 2.076_757e-2, 1.960_092_8e-2, 1.843_431_6e-2, 1.733_937_9e-2,
            1.627_183e-2, 1.520_428_1e-2, 1.421_591_6e-2, 1.324_808_6e-2, 1.228_025_6e-2, 1.139_915e-2,
            1.053_163_4e-2, 9.664_118e-3, 8.890_837e-3, 8.124_143e-3, 7.357_925e-3, 6.692_528_7e-3,
            6.027_102_5e-3, 5.369_365e-3, 4.805_654_3e-3, 4.241_943_4e-3, 3.693_074e-3, 3.231_406_2e-3,
            2.769_708_6e-3, 2.330_094_6e-3, 1.970_708_4e-3, 1.611_352e-3, 1.281_291_2e-3, 1.024_454_8e-3,
            7.675_588e-4, 5.473_196_5e-4, 3.931_522_4e-4, 2.389_848_2e-4, 1.286_566_3e-4, 7.724_762e-5,
            2.589_821_8e-5, 2.557_039_3e-5, 7.694_96e-5, 1.283_586e-4, 2.380_609_5e-4, 3.922_283_6e-4,
            5.463_958e-4, 7.660_389e-4, 1.022_875_3e-3, 1.279_741_5e-3, 1.609_176_4e-3, 1.968_532_8e-3,
            2.327_948_8e-3, 2.766_937e-3, 3.228_604_8e-3, 3.690_302_4e-3, 4.238_546e-3, 4.802_227e-3,
            5.365_938e-3, 6.023_079e-3, 6.688_475_6e-3, 7.353_872e-3, 8.119_464e-3, 8.886_158_5e-3,
            9.658_843e-3, 1.052_635_9e-2, 1.139_390_5e-2, 1.227_438_45e-2, 1.324_221_5e-2, 1.421_004_5e-2,
            1.519_781_35e-2, 1.626_539_2e-2, 1.733_291_1e-2, 1.842_722_3e-2, 1.959_386_5e-2, 2.076_050_6e-2,
            2.196_061_6e-2, 2.322_560_5e-2, 2.449_056_5e-2, 2.579_566_8e-2, 2.715_826e-2, 2.852_091_2e-2,
            2.993_011_5e-2, 3.138_950_5e-2, 3.284_889_5e-2, 3.436_118_4e-2, 3.591_650_7e-2, 3.747_186e-2,
            3.908_622_3e-2, 4.073_655_6e-2, 4.238_686e-2, 4.410_219_2e-2, 4.584_649_2e-2, 4.759_079_2e-2,
            4.940_593_2e-2, 5.124_312_6e-2, 5.308_038e-2, 5.499_398_7e-2, 5.692_303e-2, 5.885_198_7e-2,
            6.086_29e-2, 6.288_26e-2, 6.490_230_6e-2, 6.700_909e-2, 6.911_826e-2, 7.123_121_6e-2,
            7.342_857e-2, 7.562_59e-2, 7.783_311_6e-2, 8.011_728_5e-2, 8.240_151_4e-2, 8.470_142e-2,
            8.707_112e-2, 8.944_076e-2, 9.183_18e-2, 9.428_549e-2, 9.673_917e-2, 9.921_965e-2,
            1.017_559_2e-1, 1.042_922_4e-1, 1.068_605_2e-1, 1.094_778_8e-1, 1.120_952_1e-1, 1.147_494_9e-1,
            1.174_463e-1, 1.201_430_86e-1, 1.228_815_6e-1, 1.256_562_2e-1, 1.284_308_7e-1, 1.312_516_6e-1,
            1.341_024_6e-1, 1.369_533e-1, 1.398_544_9e-1, 1.427_797_4e-1, 1.457_049_8e-1, 1.486_845e-1,
            1.516_824e-1, 1.546_803_1e-1, 1.577_361_5e-1, 1.608_049_9e-1, 1.638_738_5e-1, 1.670_039_3e-1,
            1.701_418_2e-1, 1.732_797e-1, 1.764_819e-1, 1.796_869e-1, 1.828_937_5e-1, 1.861_640_2e-1,
            1.894_342_9e-1, 1.927_107_3e-1, 1.960_442_4e-1, 1.993_777_2e-1, 2.027_215_4e-1, 2.061_162_6e-1,
            2.095_110_1e-1, 2.129_198_3e-1, 2.163_737_1e-1, 2.198_275_9e-1, 2.232_990_9e-1, 2.268_101_6e-1,
            2.303_212e-1, 2.338_531e-1, 2.374_191_6e-1, 2.409_852_1e-1, 2.445_750_2e-1, 2.481_939_2e-1,
            2.518_128_2e-1, 2.554_580_6e-1, 2.591_276_2e-1, 2.627_972e-1, 2.664_953_8e-1, 2.702_134e-1,
            2.739_314_4e-1, 2.776_799_8e-1, 2.814_442_5e-1, 2.852_085e-1, 2.890_048e-1, 2.928_129_4e-1,
            2.966_210_8e-1, 3.004_626_3e-1, 3.043_125e-1, 3.081_624e-1, 3.120_465_3e-1, 3.159_357_3e-1,
            3.198_249_3e-1, 3.237_489_8e-1, 3.276_752e-1, 3.316_018e-1, 3.355_626_5e-1, 3.395_235e-1,
            3.434_869_6e-1, 3.474_800_6e-1, 3.514_731_5e-1, 3.554_707_8e-1, 3.594_937_3e-1, 3.635_166_6e-1,
            3.675_456_6e-1, 3.715_959_5e-1, 3.756_462e-1, 3.797_038_2e-1, 3.837_791_7e-1, 3.878_545e-1,
            3.919_38e-1, 3.960_358_5e-1, 4.001_337e-1, 4.042_402_5e-1, 4.083_581_6e-1, 4.124_760_6e-1,
            4.166_028_2e-1, 4.207_383e-1, 4.248_738e-1, 4.290_179_3e-1, 4.331_685e-1, 4.373_190_7e-1,
            4.414_777_5e-1, 4.456_409e-1, 4.498_040_7e-1, 4.539_743_7e-1, 4.581_475e-1, 4.623_206_6e-1,
            4.664_998e-1, 4.706_806e-1, 4.748_613_8e-1, 4.790_465e-1, 4.832_323_2e-1, 4.874_181_7e-1,
            4.916_063_2e-1, 4.957_946_8e-1, 4.999_830_4e-1, 5.041_713e-1, 5.083_597e-1, 5.125_478_5e-1,
            5.167_336_5e-1, 5.209_195e-1, 5.251_046_4e-1, 5.292_854_3e-1, 5.334_661_6e-1, 5.376_454e-1,
            5.418_186e-1, 5.459_918_4e-1, 5.501_622_6e-1, 5.543_254e-1, 5.584_885_5e-1, 5.626_473e-1,
            5.667_978_5e-1, 5.709_484e-1, 5.750_927e-1,
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
            (8960, 2.397_818e-1),
            (10000, 2.714_936_7e-1),
            (12288, -8.358_175e-2),
            (12544, 2.567_042e-1),
            (12576, 2.659_919_6e-1),
            (12608, 2.076_792e-1),
            (12672, -3.891_954_6e-2),
            (12768, -2.688_043_4e-1),
            (12800, -2.210_325_4e-1),
            (13056, 5.603_232_4e-3),
            (16384, -6.965_818e-2),
            (16416, 4.759_488_3e-2),
            (17408, -1.858_420_3e-2),
            (19000, 3.146_197e-2),
            (20736, 2.912_761e-2),
            (20768, 1.109_132_6e-2),
            (20800, -9.996_548e-3),
            (20960, -1.557_556_45e-2),
            (21000, -4.244_329_8e-2),
            (23040, 6.540_554e-2),
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
            (0, false, [-1.165_166_94e-1, -6.257_658_5e-2, -8.011_335e-2, 2.771_522_6e-4, -2.401_392_2e-2, -1.430_923_5e-1, -9.798_542_4e-2, -6.302_177e-2, -1.390_511_5e-1, 1.339_614_1e-1, 2.161_836e-2, -5.231_790_6e-2, -2.680_671e-2, 6.581_33e-2, 1.189_036_74e-1, 2.813_984_8e-2, -9.491_347e-2, 1.397_602_9e-1]),
            (1, false, [-1.111_556_1e-1, -5.234_142e-2, -7.182_751_6e-2, 1.251_022_05e-2, -1.274_843e-2, -1.441_088_2e-1, -9.012_802e-2, -7.511_376e-2, -1.376_330_7e-1, 1.312_063e-1, 9.271_116e-3, -6.473_710_4e-2, -3.970_158_5e-2, 7.744_530_6e-2, 1.138_947_1e-1, 1.638_618_1e-2, -8.707_927e-2, 1.397_311e-1]),
            (1, true, [-1.248_657_26e-1, -7.114_398_5e-2, -9.335_831_6e-2, 2.037_829_3e-3, -2.507_062_3e-2, -1.442_053_6e-1, -8.586_926e-2, -7.804_582e-2, -1.279_922e-1, 1.182_227e-1, -9.471_935e-3, -7.810_513e-2, -5.524_032_6e-2, 7.948_618_4e-2, 1.139_412e-1, 1.887_58e-2, -8.837_162e-2, 1.388_316_5e-1]),
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
            (0, 1, [-1.052_096_6e-1, -5.354_831e-2, -7.443_312e-2, 6.965_268_4e-3, -1.732_345e-2, -1.453_541_2e-1, -9.899_063_4e-2, -6.397_016e-2, -1.405_297_8e-1, 1.353_277_9e-1, 2.198_125_4e-2, -5.312_265_5e-2, -2.719_915_8e-2, 6.711_285e-2, 1.078_912_54e-1, 2.106_494_5e-2, -8.447_771_5e-2, 1.289_259_6e-1]),
            (1, 0, [-1.294_018e-1, -9.766_169e-2, -1.124_989_2e-1, -4.634_798e-2, -6.876_165_4e-2, -1.396_205_6e-1, -1.371_048_4e-1, -9.448_093e-3, -1.500_396_7e-1, 1.460_657_1e-1, 7.801_356e-2, 7.504_975_4e-3, 3.486_957_8e-2, 1.569_488_3e-2, 1.307_258_8e-1, 7.105_180_6e-2, -1.187_813_7e-1, 1.311_337_8e-1]),
            (1, 1, [-9.980_61e-2, -4.371_003e-2, -6.593_142e-2, 1.877_218_7e-2, -5.873_966e-3, -1.464_065e-1, -9.104_042e-2, -7.620_691e-2, -1.391_181_8e-1, 1.325_763_9e-1, 9.515_906e-3, -6.570_208e-2, -4.022_68e-2, 7.826_038e-2, 1.028_034_7e-1, 9.943_817e-3, -7.679_711e-2, 1.284_838_5e-1]),
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
            ("only", 1000.0, 1.0, false, 1e-06, [1.392_019_2e-1, 9.931_978_6e-2, 3.904_127_3e-2, 1.840_819_4e-1, 1.930_874_9e-1, -9.176_774e-2, -8.988_073_5e-2, 2.016_528_7e-1, -1.566_388_3e-1, -1.781_161_4e-1, 7.354_922e-2, -7.199_079_5e-2, -1.054_768_7e-1, 3.075_519_2e-2, -1.015_951_1e-2, 1.690_591_6e-1, -1.240_283_2e-1, -2.858_594e-2, -1.407_382_8e-1, 1.445_538_8e-1, -6.882_883_6e-2, 1.041_812_5e-1, -1.140_090_15e-1, -1.455_102_1e-1]),
            ("cancel", 500.0, 1.0, false, 1e-06, [1.392_019_2e-1, 9.931_978_6e-2, 3.446_416e-2, 1.461_426_3e-1, 1.138_233_8e-1, -1.754_883e-2, -1.044_956e-1, 1.452_916_1e-1, -7.312_262e-2, -9.475_282e-2, 9.522_99e-2, -2.230_965_3e-3, -2.851_834_3e-2, 6.921_859e-2, -5.589_948_6e-2, 1.414_595_1e-1, -4.393_297_8e-2, 2.949_998_5e-2, -1.405_370_7e-1, 1.445_477_8e-1, -6.882_883_6e-2, 1.041_812_5e-1, -1.140_090_15e-1, -1.455_102_1e-1]),
            ("add", 1500.0, 0.75, false, 1e-06, [1.392_019_2e-1, 9.931_978_6e-2, 4.060_704_6e-2, 1.888_070_6e-1, 2.125_008_1e-1, -1.178_704e-1, -7.466_407e-2, 2.091_575_4e-1, -1.810_277_4e-1, -2.004_290_8e-1, 5.655_802e-2, -9.785_499_4e-2, -1.315_782_4e-1, 9.919_257e-3, 1.215_253_2e-2, 1.661_713e-1, -1.498_628e-1, -5.304_811_5e-2, -1.408_056_5e-1, 1.445_522_5e-1, -6.882_792e-2, 1.041_813_1e-1, -1.140_093_1e-1, -1.455_102_1e-1]),
            ("overlap", 20000.0, 0.05, true, 2e-06, [1.392_019_2e-1, 9.931_978_6e-2, 5.523_375_8e-2, 1.487_274_8e-1, 2.432_465e-1, -2.154_917_4e-1, 3.155_081_3e-2, 1.769_453e-1, -2.478_511_3e-1, -2.495_804_6e-1, -5.282_136_8e-2, -2.010_877_3e-1, -2.244_860_4e-1, -1.038_495_4e-1, 1.264_514e-1, 9.307_311_5e-2, -2.354_491_4e-1, -1.651_065_5e-1, -2.019_059_8e-1, 1.827_553_2e-1, -2.288_174_2e-1, 1.624_545_8e-1, -1.513_438_1e-1, -1.462_782e-1]),
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
            ("Freq",r#"DistortionType="2""#,0.1,[0.000000000e+00,1.190_295_1e-4,7.057_778e-4,4.934_406e-3,3.135_957_6e-2,8.878_513e-2,-4.344_717_4e-2,-1.604_389_4e-1,-1.574_005_2e-1,-1.477_883_8e-1,-1.652_471e-1,-1.749_614_2e-1,-1.789_862_2e-1,9.347_144e-3,-6.078_448e-2,1.796_222_8e-1,-9.757_539e-2,1.763_360_4e-1,2.021_695_1e-1,-1.048_069e-1,-4.701_037_3e-2,-1.655_779_6e-1]),
            ("Q",r#"DistortionType="2""#,0.75,[0.000000000e+00,9.033_701e-5,5.516_947e-4,4.102_432_6e-3,2.948_021e-2,1.052_622_4e-1,-7.034_59e-2,-3.145_911_4e-1,-3.199_325_8e-1,-3.118_176_8e-1,-3.695_587e-1,-4.148_411e-1,-4.474_622e-1,1.604_536_2e-1,-2.788_140_4e-2,4.054_080_8e-1,-1.072_212_4e-1,5.087_45e-1,6.072_319e-1,-3.179_932_8e-1,-4.897_114_3e-2,-3.453_055_3e-1]),
            ("KeyTracking",r#"DistortionType="2""#,0.75,[0.000000000e+00,1.099_992_5e-4,6.522_777e-4,4.563_058_7e-3,2.909_689e-2,8.411_154e-2,-3.777_113_6e-2,-1.532_173_9e-1,-1.515_066_5e-1,-1.435_478_6e-1,-1.551_044_7e-1,-1.656_694_9e-1,-1.708_463_7e-1,1.639_947_9e-2,-5.097_339_3e-2,1.706_322_3e-1,-8.584_198e-2,1.704_588_8e-1,1.895_819_3e-1,-1.059_208_7e-1,-4.623_983_4e-2,-1.611_101_9e-1]),
            ("Fat",r#"Q="0.75" Fat="0" DistortionType="2""#,1.0,[0.000000000e+00,9.033_696e-5,5.516_941_3e-4,4.102_424e-3,2.948_008e-2,1.052_275_15e-1,-7.204_568_4e-2,-3.220_185_3e-1,-3.274_537_3e-1,-3.186_823_7e-1,-4.963_803e-1,-5.601_983_7e-1,-6.089_756_5e-1,3.788_838_4e-1,4.212_828e-2,4.376_962e-1,-4.397_104_3e-2,5.611_38e-1,6.090_657e-1,-4.162_817_3e-1,-3.723_109_6e-3,-4.121_481_8e-1]),
            ("Drive",r#"Q="0.75" DistortionType="0""#,12.0,[0.000000000e+00,1.908_639_5e-4,1.188_225_7e-3,9.024_747e-3,6.383_013e-2,2.420_393e-1,-1.530_187_9e-1,-5.954_233_4e-1,-6.265_035e-1,-6.339_635e-1,-5.088_337_7e-1,-5.705_874e-1,-6.236_269e-1,2.101_472_9e-1,-8.439_113e-2,5.972_206_6e-1,-2.153_246_7e-1,6.967_823_5e-1,6.064_583e-1,-6.184_144e-1,-1.029_847_9e-1,-6.882_558_5e-1]),
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
            (0.0,[0.000000000e+00,8.629_213e-5,5.117_894_6e-4,3.585_818_7e-3,2.306_760_5e-2,7.044_615e-2,-2.355_593_3e-2,-1.302_671_9e-1,-1.316_603_3e-1,-1.278_581_2e-1,-1.234_239_6e-1,-1.355_924_6e-1,-1.433_449_2e-1,3.279_042_2e-2,-2.483_948_3e-2,1.406_688_1e-1,-5.320_595_6e-2,1.456_556_3e-1,1.392_019_2e-1,-1.044_956e-1,-4.393_297_4e-2,-1.455_102_1e-1]),
            (0.75,[0.000000000e+00,8.629_213e-5,5.117_894_6e-4,3.585_818_7e-3,2.306_760_5e-2,7.044_615e-2,-2.355_593_3e-2,-1.302_671_9e-1,-1.316_603_3e-1,-1.278_635_7e-1,-1.273_344_5e-1,-1.394_374_2e-1,-1.469_869e-1,2.924_316_7e-2,-3.417_472e-2,1.634_898_8e-1,-9.252_796e-2,1.778_681_7e-1,1.930_532_6e-1,-9.867_251e-2,-1.030_933_04e-1,-1.794_820_3e-1]),
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
            (0.2,0.5,0.,[3.502_391_7e-2,6.514_113_4e-2,5.601_514_5e-2,2.633_638_7e-2,7.873_311e-3,7.036_548_3e-4]),
            (0.2,0.5,63.,[4.108_368_6e-2,7.541_589_4e-2,6.302_244e-2,2.568_366_8e-2,6.108_238e-3,3.454_886e-4]),
            (0.2,0.5,127.,[4.821_703e-2,8.713_452_5e-2,7.032_905_5e-2,2.409_114_9e-2,4.339_241e-3,1.407_752_7e-4]),
            (0.8,0.5,127.,[5.816_827_3e-2,1.028_023_6e-1,7.888_304e-2,2.098_39e-2,2.521_914_4e-3,3.642_660_5e-5]),
            (0.2,-0.5,127.,[3.075_589_6e-2,5.772_809_7e-2,5.062_618_5e-2,2.626_123e-2,9.187_955e-3,1.124_673_6e-3]),
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
            (1,[2.080_611e-1,1.798_464_7e-1,2.485_674_9e-1,2.490_397_1e-1,-2.502_437_5e-1,2.091_641_7e-1,2.305_141_7e-1,1.576_830_4e-1,4.636_129e-2,-2.434_368_1e-1,1.048_971_9e-1,-1.118_588_9e-1,-2.501_869_5e-1,-2.431_959_5e-1,-1.827_916_5e-1,1.637_150_8e-1,9.623_237e-2,-2.306_54e-1,1.292_134_1e-1,1.720_395_2e-1,-2.459_781_5e-1,-1.284_771_6e-1]),
            (2,[2.080_611_7e-1,1.798_465_3e-1,2.485_675_8e-1,2.490_398e-1,-2.502_438e-1,2.091_642_5e-1,2.305_142_6e-1,1.576_831e-1,4.636_130_5e-2,-2.434_369e-1,1.048_972_4e-1,-1.118_589_4e-1,-2.501_870_4e-1,-2.431_959_4e-1,-1.827_916_5e-1,1.637_151_2e-1,9.623_239e-2,-2.306_540_9e-1,1.292_134_5e-1,1.720_395_8e-1,-2.459_782_4e-1,-1.284_772e-1]),
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
            ("a1-q0",r#"Algorithm="1" Oversampling="0" DistortionType="2""#,256,1e-06,[0.000000000e+00,1.130_820_4e-5,2.089_247_6e-4,2.213_424_8e-3,1.081_344_9e-2,-1.942_267_3e-2,-2.133_641_4e-2,-2.528_970_7e-2,-3.159_736e-2,-1.118_486_6e-2,2.144_810_6e-2,6.649_165_4e-3,1.062_234e-1,9.607_444e-2,5.699_444_6e-2,1.730_920_4e-3,9.698_454e-2,1.290_640_7e-1,1.390_244e-1,1.443_761e-1,3.329_179_8e-2,-4.493_279_8e-2,1.166_495_1e-1,-5.320_595_6e-2,1.456_556_3e-1,3.966_958_7e-3]),
            ("a1-q75",r#"Algorithm="1" Oversampling="0" Q=".75" DistortionType="2""#,256,8e-06,[0.000000000e+00,1.144_072_2e-5,2.146_980_4e-4,2.348_225_6e-3,1.206_641e-2,-2.709_918_8e-2,-2.971_001_5e-2,-4.580_548_4e-2,-5.675_443_6e-2,-2.661_543_9e-2,5.142_482e-2,1.485_951_6e-2,3.687_176e-1,3.398_414e-1,1.816_517_1e-1,-8.579_823_4e-2,4.133_111_5e-1,6.131_519_7e-1,6.873_100_4e-1,7.398_243e-1,3.090_188_8e-1,-1.167_431_5e-1,8.179_454e-1,-6.146_780_4e-2,7.766_121_6e-1,2.431_763_3e-1]),
            ("quiet-soft",r#"Algorithm="1" Oversampling="0" Freq="100" Q=".5" Fat=".5" Drive="6" DistortionType="0""#,256,3e-07,[0.000000000e+00,2.327_287_8e-6,4.276_351e-5,4.440_114_4e-4,2.236_753_2e-3,-4.177_334_7e-3,-4.667_209_5e-3,-4.888_162e-3,-6.545_061_7e-3,4.604_017e-4,1.227_247_5e-2,8.439_659e-3,4.155_541_6e-2,4.106_065e-2,-2.040_073_6e-5,-2.704_642_9e-2,1.229_383_8e-2,3.153_993_2e-2,4.030_380_4e-2,4.810_310_5e-2,4.309_380_4e-2,1.473_191_1e-2,6.209_146_2e-2,9.952_809e-3,5.473_988e-2,3.206_234_8e-2]),
            ("highq-soft",r#"Algorithm="1" Oversampling="0" Freq="5000" Q=".8" Drive="12" DistortionType="0""#,256,2e-05,[0.000000000e+00,2.178_337e-4,3.908_612_8e-3,3.704_645e-2,1.697_802_1e-1,-3.528_306_8e-1,-3.765_156_3e-1,-6.057_613_5e-1,-6.601_653e-1,-7.022_142_4e-1,-7.510_363e-1,-8.219_976_4e-1,-4.570_471_3e-1,-6.500_541e-1,9.511_040_4e-1,9.010_380_5e-1,9.177_131e-1,8.263_369e-1,6.868_516e-1,4.414_256_5e-1,-8.485_567_6e-1,-9.130_077_4e-1,-6.573_822e-1,-9.167_744e-1,4.407_577_2e-2,-8.767_448e-1]),
            ("highcut-hard",r#"Algorithm="1" Oversampling="0" Freq="18000" Q=".5" DistortionType="1""#,256,2e-06,[0.000000000e+00,2.660_280_2e-4,4.927_044_3e-3,4.961_400_5e-2,1.552_467_5e-1,-2.361_543_2e-1,-2.019_187_8e-1,-2.386_945_2e-1,-2.146_816e-1,-2.538_543_3e-1,-2.141_696_2e-1,-2.349_108e-1,2.359_044_7e-3,-4.270_569e-2,2.505_858e-1,2.146_402_6e-1,2.465_842_7e-1,2.149_505_9e-1,1.883_746_4e-1,1.559_405e-1,-1.754_101_7e-1,-2.427_880_6e-1,-2.168_103_5e-2,-2.462_493_5e-1,1.088_679_1e-1,-2.082_774_8e-1]),
            ("highq-linear",r#"Algorithm="1" Oversampling="0" Q=".8" DistortionType="2""#,256,9e-06,[0.000000000e+00,1.144_955_9e-5,2.150_829_7e-4,2.357_212_8e-3,1.214_994_8e-2,-2.761_099e-2,-3.026_829_1e-2,-4.717_257_6e-2,-5.843_106_3e-2,-2.762_557_8e-2,5.354_713_3e-2,1.553_086_4e-2,3.877_734_8e-1,3.578_449_8e-1,1.883_656e-1,-9.914_479_4e-2,4.352_785_3e-1,6.515_299_7e-1,7.325_415e-1,7.906_012e-1,3.417_334_6e-1,-1.151_836_1e-1,8.993_622_7e-1,-4.954_123e-2,8.267_725e-1,2.808_782e-1]),
            ("a1-os-q0",r#"Algorithm="1" Oversampling="1" DistortionType="2""#,256,2e-06,[0.000000000e+00,6.150_371e-9,6.643_802e-6,8.983_419e-4,9.967_972_5e-3,-1.189_457_5e-2,-1.510_919_8e-2,-5.683_737_3e-3,-1.334_178_3e-2,1.914_879_1e-2,5.595_577e-2,4.443_844_4e-2,1.160_743_4e-1,1.158_885_2e-1,-3.536_332_6e-3,-6.474_26e-2,3.610_101e-2,8.361_152e-2,1.037_622_2e-1,1.205_664_1e-1,9.606_419e-2,2.542_665e-2,1.447_176_8e-1,1.668_018_8e-2,1.368_299e-1,7.192_03e-2]),
            ("a1-os-hard",r#"Algorithm="1" Oversampling="1" Freq="18000" Q=".5" DistortionType="1""#,256,2e-06,[0.000000000e+00,1.174_596_7e-7,1.274_847_7e-4,1.687_064e-2,1.440_618_6e-1,-2.565_132_4e-1,-2.614_123_8e-1,-2.952_323_6e-1,-2.926_279_3e-1,-2.380_715_8e-1,-1.437_421_9e-1,-1.805_858_9e-1,1.085_112_54e-1,6.531_966_5e-2,2.271_476_4e-1,1.414_592e-1,2.445_652_8e-1,2.514_538_2e-1,2.425_341_3e-1,2.259_183_2e-1,-8.381_736e-2,-1.949_053_3e-1,8.661_325e-2,-2.045_492_2e-1,1.957_048_5e-1,-1.308_392e-1]),
            ("a0-q0",r#"Algorithm="0" Oversampling="0" DistortionType="2""#,256,8e-06,[0.000000000e+00,8.823_884e-9,8.319_6e-6,9.352_095e-4,9.809_46e-3,-1.187_651_9e-2,-1.495_414_8e-2,-6.306_321_4e-3,-1.373_313_3e-2,1.764_408e-2,5.383_489e-2,4.237_821_3e-2,1.148_824_1e-1,1.143_236_1e-1,-1.043_746_6e-3,-6.180_530_8e-2,3.881_926e-2,8.568_369_6e-2,1.054_027_45e-1,1.217_186_15e-1,9.331_792_6e-2,2.229_475_2e-2,1.435_513_6e-1,1.356_372_1e-2,1.373_095_7e-1,6.892_423e-2]),
            ("a0-hard",r#"Algorithm="0" Oversampling="0" Freq="18000" Q=".5" DistortionType="1""#,256,2e-06,[0.000000000e+00,1.080_254_46e-7,1.024_596e-4,1.144_418_7e-2,1.034_537_26e-1,-1.940_816_6e-1,-2.082_259_7e-1,-3.313_027_6e-1,-3.433_621e-1,-2.709_571e-1,-1.656_388_2e-1,-2.018_701_4e-1,9.787_444e-2,5.357_766e-2,2.326_23e-1,1.477_819_5e-1,2.463_334_2e-1,2.505_766_2e-1,2.403_927_1e-1,2.226_004_3e-1,-9.028_415_4e-2,-1.994_251_9e-1,8.023_406_6e-2,-2.087_270_8e-1,1.915_316_3e-1,-1.367_844_6e-1]),
            ("b32-q0",r#"Algorithm="1" Oversampling="0" DistortionType="2""#,32,3e-07,[0.000000000e+00,8.629_213e-5,1.590_582_9e-3,1.649_166_3e-2,6.949_541e-2,-1.302_671_9e-1,-1.316_603_3e-1,-1.234_239_6e-1,-1.355_924_6e-1,-6.985_714e-2,1.510_655_2e-3,-2.483_948_3e-2,1.252_555_5e-1,1.095_308_14e-1,7.384_254e-2,3.060_763e-3,9.919_524e-2,1.309_484e-1,1.406_688_1e-1,1.458_182e-1,3.379_636e-2,-4.459_248_5e-2,1.166_581_3e-1,-5.320_595_6e-2,1.456_556_3e-1,3.966_958_7e-3]),
            ("b32-q75",r#"Algorithm="1" Oversampling="0" Q=".75" DistortionType="2""#,32,3e-06,[0.000000000e+00,9.438_189_6e-5,1.942_460_9e-3,2.457_013_2e-2,1.358_272_4e-1,-5.018_182_4e-1,-5.100_779e-1,-7.998_852e-1,-9.011_882_5e-1,2.200_774_8e-2,1.861_204_8e-1,6.657_888e-2,8.471_71e-1,8.005_856e-1,1.198_337_7e-1,-1.587_008e-1,3.778_213_6e-1,5.940_016_5e-1,6.754_853e-1,7.353_627e-1,3.568_49e-1,-5.955_226_7e-2,7.711_324e-1,-6.428_297e-2,7.765_625e-1,2.431_763_3e-1]),
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
            (32000.,r#"Algorithm="0" Oversampling="0" Freq="1000" DistortionType="2""#,1.2e-05,[0.000000000e+00,3.444_921e-8,2.299_368e-5,1.987_774_2e-3,9.153_586e-3,2.164_603_4e-3,7.627_828_5e-3,2.553_758_2e-2,1.619_651e-2,-5.987_991_4e-2,7.134_255e-2,7.938_984e-2,-3.335_522_5e-2,-2.339_848_3e-3,6.330_68e-2,2.524_745_7e-2,-1.133_534_4e-1,-1.432_621_5e-1,-1.427_749e-1,-1.317_036_3e-1,1.252_099_1e-1,1.412_752e-1,1.003_832e-3,-1.369_242_8e-1,-1.058_824_7e-1,1.421_139_7e-1]),
            (32000.,r#"Algorithm="1" Oversampling="0" Freq="1000" DistortionType="2""#,1e-06,[0.000000000e+00,2.510_654_5e-5,4.479_426e-4,3.956_916_3e-3,3.778_569_2e-3,1.437_014_5e-2,1.860_173_8e-2,-3.911_221e-4,-1.214_405_1e-2,-4.036_504_8e-2,8.152_237_5e-2,7.472_382_5e-2,4.375_435_4e-2,7.175_463_4e-2,1.199_352_44e-1,-6.729_389e-2,-1.440_480_8e-1,-1.229_590_6e-1,-9.807_015e-2,-6.582_065e-2,1.455_746e-1,8.712_377e-2,-9.006_436e-2,-7.564_469e-2,-2.051_620_4e-2,1.331_056_5e-1]),
            (32000.,r#"Algorithm="1" Oversampling="1" Freq="1000" DistortionType="2""#,1.5e-06,[0.000000000e+00,2.442_012_3e-8,1.877_187e-5,1.944_174_2e-3,9.658_543e-3,1.578_804e-3,7.283_537_6e-3,2.748_468e-2,1.798_586_5e-2,-6.213_094_7e-2,7.204_965_5e-2,8.086_95e-2,-3.706_839e-2,-5.824_822e-3,6.035_079_8e-2,2.999_788_3e-2,-1.107_043_55e-1,-1.430_670_3e-1,-1.438_914_1e-1,-1.340_635_6e-1,1.229_394_7e-1,1.429_234_6e-1,5.968_707e-3,-1.390_025_8e-1,-1.095_638_35e-1,1.412_893_1e-1]),
            (32000.,r#"Algorithm="1" Oversampling="1" Freq="12000" Q=".5" DistortionType="1""#,3e-06,[0.000000000e+00,3.113_956_4e-7,2.404_732_8e-4,2.434_142_5e-2,7.722_477_6e-2,1.466_919_3e-1,1.942_598_8e-1,-1.767_302_8e-1,-2.510_733e-1,9.671_9e-3,1.761_194_5e-1,1.119_123_25e-1,2.108_804_9e-1,2.448_125_5e-1,2.577_050_3e-1,-2.035_662_8e-1,-2.339_137e-1,-1.458_893_7e-1,-8.420_370_5e-2,-1.636_403_8e-2,2.214_385_1e-1,5.798_053_7e-2,-2.253_479e-1,-3.435_566_3e-2,6.798_22e-2,1.715_847_4e-1]),
            (96000.,r#"Algorithm="0" Oversampling="0" Freq="1000" DistortionType="2""#,6.5e-06,[0.000000000e+00,4.261_053e-6,6.556_075e-5,6.824_753_4e-4,5.365_895_6e-3,2.126_106_6e-2,2.162_667_7e-2,-3.993_261_6e-2,-4.128_613_7e-2,5.404_496_6e-2,-6.187_672e-2,-6.669_186e-2,-5.452_191_5e-2,-6.373_907_6e-2,-3.073_339_7e-2,5.979_112_5e-2,1.050_671_3e-2,-1.552_939_4e-2,-2.851_443_7e-2,-4.128_399e-2,-1.236_271_7e-1,-1.400_518_9e-1,8.955_042e-2,1.409_956_4e-1,-5.590_795_4e-2,-1.306_407_5e-1]),
            (96000.,r#"Algorithm="1" Oversampling="0" Freq="1000" DistortionType="2""#,2e-06,[0.000000000e+00,2.856_672_4e-6,5.392_286_8e-5,6.334_506_3e-4,5.286_063e-3,2.173_827_2e-2,2.215_293_2e-2,-4.071_767_6e-2,-4.217_54e-2,5.459_990_7e-2,-6.205_981_6e-2,-6.706_104e-2,-5.318_897e-2,-6.259_163_5e-2,-3.343_421e-2,6.272_235e-2,1.355_289_2e-2,-1.256_755_2e-2,-2.562_984_6e-2,-3.851_654_4e-2,-1.224_962_4e-1,-1.397_412e-1,8.749_467e-2,1.407_762_2e-1,-5.330_055_6e-2,-1.298_071_7e-1]),
            (96000.,r#"Algorithm="1" Oversampling="1" Freq="1000" DistortionType="2""#,3e-06,[0.000000000e+00,3.257_742_7e-9,3.450_642_7e-6,2.881_340_6e-4,4.004_747e-3,2.195_967_4e-2,2.272_849_2e-2,-3.658_633_3e-2,-3.873_056_5e-2,4.740_198_3e-2,-4.967_938_7e-2,-5.568_589e-2,-3.114_877_3e-2,-4.132_160_5e-2,-5.751_305e-2,8.679_797e-2,4.198_285e-2,1.665_199e-2,3.616_502_5e-3,-9.517_195e-3,-1.037_884_9e-1,-1.278_487_1e-1,6.173_394e-2,1.294_665_2e-1,-2.444_276_8e-2,-1.132_333_3e-1]),
            (96000.,r#"Algorithm="0" Oversampling="0" Freq="18000" Q=".5" DistortionType="1""#,3e-06,[0.000000000e+00,5.206_274e-5,8.084_049e-4,8.455_299e-3,6.331_722e-2,1.594_073e-1,1.522_250_3e-1,-2.438_932_8e-1,-2.195_954_8e-1,1.887_577_8e-1,-2.135_767_3e-1,-1.968_407_3e-1,-2.544_927e-1,-2.501_664_2e-1,1.991_100_3e-1,-1.482_406e-1,-2.103_506_5e-1,-2.317_064_1e-1,-2.396_470_3e-1,-2.450_191e-1,-2.230_780_7e-1,-1.824_758e-1,2.503_724e-1,1.769_981_4e-1,-2.515_680_8e-1,-2.093_729e-1]),
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

    #[test]
    fn authored_native_scalar_bypass_resume_synchronizes_pending_controls() {
        // Original 48k PCM16 sine. Bypass8192..16384 freezes audio memories;
        // pending scalar solver values use a host-block raw parameter ramp
        // on resume, Fat applies immediately, Drive one output frame later.
        // Initially bypassed filters retain their zero-coefficient cold start.
        #[rustfmt::skip]
        const POINTS:[usize;28]=[16384,16385,16386,16391,16415,16416,16447,16448,16511,16512,16546,16548,16575,16576,16604,16622,16637,16639,16640,16699,16701,16896,17000,18000,19442,21845,22212,23495];
        type ResumeCase<'a> = (u8, &'a str, &'a str, &'a [(&'a str, f64)], f32, [f32; 28]);
        #[rustfmt::skip]
        let cases:[ResumeCase<'_>;14]=[
            (1,"Freq",r#"Freq="1000" DistortionType="2""#,&[("Freq",5000.0)],1.1e-06,[1.121_365_9e-1,6.831_220_5e-2,2.853_494_9e-2,-8.992_232e-2,3.455_404_6e-2,1.406_271_5e-3,1.067_777_3e-1,7.327_542_5e-2,2.236_816_4e-1,2.125_954_8e-1,2.244_772e-1,1.870_541_3e-1,1.768_679_8e-1,2.025_690_4e-1,-4.163_762e-2,1.205_226_3e-2,-1.095_725_6e-1,-2.608_799_7e-2,1.766_579e-2,-2.327_559_3e-1,-2.405_845_2e-1,1.796_502_6e-1,1.914_607_3e-1,1.210_507e-1,-1.667_598e-2,-7.056_327e-2,1.304_412_5e-1,-7.420_494e-2]),
            (1,"Q",r#"Freq="18000" DistortionType="2""#,&[("Q",0.75)],3e-07,[-1.030_676_44e-1,-3.007_455e-1,-2.272_285_5e-1,-1.533_425_7e-1,-1.346_374_9e-1,-1.708_671_7e-1,-2.011_485_8e-2,-6.479_671e-2,1.977_234_2e-1,1.666_947_3e-1,1.912_009_1e-1,1.211_230_75e-1,2.368_467_5e-1,2.478_765_7e-1,4.757_767_5e-2,-7.437_728e-2,-3.299_28e-2,5.706_976e-2,9.998_288e-2,-2.504_089e-1,-2.284_341_3e-1,1.209_805_46e-1,1.373_548e-1,1.901_455_7e-1,-9.875_155e-2,-1.481_934e-1,1.974_265_9e-1,5.810_762e-3]),
            (1,"FreqQ",r#"Freq="1000" DistortionType="2""#,&[("Freq",18000.0),("Q",0.5)],8e-07,[1.121_365_9e-1,6.713_136e-2,2.333_703_8e-2,-1.181_148_44e-1,-1.083_109_3e-1,-1.575_148_7e-1,-2.607_644e-2,-7.234_842e-2,1.957_907_4e-1,1.639_453_2e-1,1.887_194_7e-1,1.182_149_5e-1,2.374_200_5e-1,2.483_308_6e-1,4.663_196_6e-2,-7.231_761_5e-2,-3.582_008_6e-2,5.404_400_5e-2,9.706_103e-2,-2.506_087e-1,-2.297_626_1e-1,1.237_331_5e-1,1.400_197e-1,1.881_493_8e-1,-9.587_935e-2,-1.456_548_7e-1,1.955_040_2e-1,2.686_424_2e-3]),
            (1,"Drive",r#"Freq="1000" DistortionType="0""#,&[("Drive",12.0)],6e-07,[1.419_448_1e-1,3.484_204_4e-2,-6.404_666_6e-2,-3.600_611_4e-1,1.801_161_6e-1,9.169_935_4e-2,3.842_727_5e-1,3.197_788_6e-1,4.435_324e-1,4.682_403_8e-1,4.492_391e-1,4.799_623_8e-1,1.159_243_6e-1,1.935_682_6e-1,-3.623_424_2e-1,3.224_934_6e-1,-4.493_166_2e-1,-3.500_237_2e-1,-2.782_479e-1,-2.752_836_3e-1,-3.873_559_8e-1,4.798_422_8e-1,4.775_534e-1,-6.568_347e-2,2.805_953_3e-1,1.769_320_2e-1,-4.337_545_5e-2,-4.149_131_5e-1]),
            (1,"Fat",r#"Freq="1000" Q=".75" DistortionType="2""#,&[("Fat",0.2)],4e-06,[5.741_933_6e-1,5.130_951e-1,4.595_959e-1,2.775_690_3e-1,-2.410_010_4e-1,-2.971_554e-1,5.957_621_3e-1,5.458_559_4e-1,1.805_496_4e-1,1.893_843_9e-1,4.017_819_8e-1,4.410_296_7e-1,-1.199_517_55e-1,-5.705_242_2e-2,-2.220_256_5e-1,1.973_612_3e-1,-3.091_032_2e-1,-2.598_536_3e-1,-2.218_737e-1,-6.836_829e-2,-1.667_070_4e-1,3.074_546_5e-1,3.004_288e-1,-1.357_562_4e-1,2.471_158_5e-1,1.966_388_4e-1,-1.226_580_4e-1,-3.021_935e-1]),
            (1,"QFat",r#"Freq="1000" DistortionType="2""#,&[("Q",0.75),("Fat",0.2)],1e-06,[1.121_365_9e-1,6.851_674e-2,2.945_060_7e-2,-8.490_755e-2,6.007_921_3e-2,3.171_107_5e-2,1.425_426_9e-1,1.215_769_2e-1,1.999_621_2e-1,2.137_971_2e-1,2.222_011_5e-1,2.454_235_1e-1,1.516_239_7e-2,6.254_113e-2,-2.478_256_7e-1,2.465_939_5e-1,-3.128_018e-1,-2.753_942e-1,-2.428_910_9e-1,-9.604_589e-2,-1.944_203_7e-1,3.063_288_6e-1,2.998_043_6e-1,-1.357_563_1e-1,2.471_158_5e-1,1.966_388_4e-1,-1.226_580_4e-1,-3.021_935e-1]),
            (1,"Tracking",r#"Freq="1000" DistortionType="2""#,&[("KeyTracking",1.0)],2e-06,[1.121_365_9e-1,6.847_592_4e-2,2.926_306_4e-2,-8.580_12e-2,5.201_589e-2,2.419_563_2e-2,1.169_945_5e-1,9.387_053e-2,1.811_91e-1,1.828_426_4e-1,1.898_790_4e-1,1.816_429_6e-1,9.365_101e-2,1.232_279_5e-1,-9.930_412e-2,7.780_120_5e-2,-1.489_488_3e-1,-8.934_594e-2,-5.456_067_2e-2,-1.685_580_8e-1,-1.995_358_3e-1,1.905_079_8e-1,1.959_394_2e-1,3.752_666_3e-2,5.546_918e-2,9.320_3e-3,4.671_197_4e-2,-1.253_248_5e-1]),
            (1,"InitiallyTrue",r#"Freq="1000" Bypass="1" DistortionType="2""#,&[],1.1e-06,[0.000000000e+00,-6.234_503e-5,-2.505_172e-4,-2.530_384_8e-3,1.736_443_3e-2,1.469_383_8e-2,4.204_907_6e-2,3.956_321_6e-2,6.891_291e-2,7.659_401e-2,9.054_147e-2,1.023_624_5e-1,1.221_700_85e-2,3.322_675e-2,-1.007_672_1e-1,9.332_578e-2,-1.311_686_6e-1,-1.022_630_5e-1,-8.231_353e-2,-8.477_911e-2,-1.215_782_1e-1,1.464_656_3e-1,1.465_821_9e-1,-2.315_851_3e-2,8.452_908e-2,5.534_631_4e-2,-1.654_135_6e-2,-1.230_509_4e-1]),
            (0,"Q",r#"Freq="18000" DistortionType="2""#,&[("Q",0.75)],2e-06,[2.481_931_7e-1,2.325_310_3e-1,1.464_293_2e-1,-2.753_75e-1,-2.292_521e-2,-7.126_594e-2,8.961_432e-2,4.572_840_8e-2,2.457_771_9e-1,2.306_887_7e-1,2.422_515e-1,2.005_091_2e-1,1.867_147_4e-1,2.144_013_2e-1,-5.303_851e-2,2.502_196_5e-2,-1.278_102_2e-1,-4.227_214_7e-2,3.243_065_7e-3,-2.392_651_4e-1,-2.529_708e-1,1.983_250_2e-1,2.097_446_2e-1,1.130_849_5e-1,-1.942_372_6e-3,-5.885_344e-2,1.233_401_3e-1,-9.246_234e-2]),
            (0,"FreqQ",r#"Freq="1000" DistortionType="2""#,&[("Freq",18000.0),("Q",0.5)],6e-06,[1.034_182_7e-1,1.195_819_2e-1,1.248_280_4e-1,-6.899_112e-2,9.180_913e-2,4.296_831e-2,1.301_970_3e-1,7.362_485e-2,2.567_329_7e-1,2.374_916_8e-1,2.460_5e-1,1.992_294e-1,1.936_629_3e-1,2.207_589_1e-1,-5.273_549_3e-2,2.766_095e-2,-1.321_573_4e-1,-4.766_985_4e-2,-2.454_596_6e-3,-2.373_333e-1,-2.532_473e-1,2.021_710_4e-1,2.132_657e-1,1.076_249e-1,4.237_789e-3,-5.291_526_8e-2,1.179_738_04e-1,-9.818_929_4e-2]),
            (0,"Drive",r#"Freq="1000" DistortionType="0""#,&[("Drive",12.0)],4e-07,[1.309_297_5e-1,1.169_870_6e-1,1.210_346_9e-1,-2.494_238_5e-1,3.764_063_7e-1,3.078_838_3e-1,4.734_180_3e-1,4.519_719_2e-1,3.272_516_4e-1,3.796_915e-1,3.380_44e-1,4.292_059_5e-1,-1.050_245_3e-1,-1.700_138_5e-2,-4.675_071_8e-1,4.530_415_8e-1,-4.749_207e-1,-4.635_199_3e-1,-4.323_352_6e-1,-8.300_581_6e-2,-2.358_663e-1,4.264_221_5e-1,4.101_831_3e-1,-2.832_071e-1,4.335_518_2e-1,3.704_189_4e-1,-2.635_841_4e-1,-4.779_381_5e-1]),
            (0,"Fat",r#"Freq="1000" Q=".75" DistortionType="2""#,&[("Fat",0.2)],5e-07,[3.764_719_7e-1,4.740_654_8e-1,5.031_592e-1,3.017_545e-1,-6.039_920_4e-2,-1.171_960_4e-1,6.471_096e-1,6.385_281e-1,1.552_947_8e-1,1.798_436e-1,2.769_588e-1,3.729_752_6e-1,-2.621_857_2e-1,-2.109_334_2e-1,-2.339_044_7e-1,2.591_118_5e-1,-3.240_749e-1,-3.265_694_4e-1,-3.124_436e-1,6.564_452_5e-2,-4.196_363e-2,2.369_671_5e-1,2.225_810_3e-1,-2.469_275e-1,3.066_584_5e-1,2.842_006_4e-1,-2.378_845_5e-1,-3.086_685_2e-1]),
            (0,"QFat",r#"Freq="1000" DistortionType="2""#,&[("Q",0.75),("Fat",0.2)],2e-06,[1.034_171_06e-1,1.195_404_9e-1,1.247_849_3e-1,-4.678_093_6e-2,1.217_699_8e-1,1.004_945_2e-1,1.736_064_6e-1,1.659_190_5e-1,1.368_281_1e-1,1.655_504_4e-1,1.520_388_1e-1,2.100_613_6e-1,-1.043_035_2e-1,-5.947_012_5e-2,-2.861_685_2e-1,2.978_202_7e-1,-3.016_641_7e-1,-3.157_418_4e-1,-3.076_559_6e-1,4.547_365_8e-2,-6.589_417e-2,2.390_565_6e-1,2.233_195_2e-1,-2.469_277_7e-1,3.066_584_5e-1,2.842_006_4e-1,-2.378_845_5e-1,-3.086_685_2e-1]),
            (0,"Tracking",r#"Freq="1000" DistortionType="2""#,&[("KeyTracking",1.0)],1.6e-05,[1.034_170_9e-1,1.195_387_5e-1,1.247_547_5e-1,-4.801_463_7e-2,1.132_793_5e-1,9.171_887e-2,1.564_510_2e-1,1.447_231_3e-1,1.497_131e-1,1.660_519_5e-1,1.638_147_5e-1,1.871_316_3e-1,6.277_649e-3,4.135_422_4e-2,-1.662_679_6e-1,1.531_932_8e-1,-1.957_905_1e-1,-1.623_382_4e-1,-1.372_468_2e-1,-9.745_680_5e-2,-1.551_408_8e-1,2.050_794_5e-1,2.033_149_9e-1,-5.708_803_6e-2,1.382_165e-1,1.003_382_5e-1,-4.803_427e-2,-1.849_562_4e-1]),
        ];
        for (algorithm, tag, attrs, changes, bound, expected) in cases {
            let mut fx = filter(
                &format!(r#"Mode="0" Algorithm="{algorithm}" Oversampling="0" {attrs}"#),
                1,
            );
            fx.set_note(72).unwrap();
            let mut output = Vec::new();
            for n in 0..24576 {
                if n == 8192 || n == 16384 {
                    fx.set_parameter("Bypass", &ParameterValue::Boolean(n == 8192))
                        .unwrap();
                    if n == 8192 {
                        for (name, value) in changes {
                            fx.set_parameter(name, &ParameterValue::Number(*value))
                                .unwrap();
                        }
                    }
                }
                let mut io = [[0.; MAX_CHANNELS]];
                io[0][0] = authored_sine(n, 48000.);
                fx.process(&mut io).unwrap();
                if (8192..16384).contains(&n) {
                    assert_eq!(io[0][0], authored_sine(n, 48000.));
                }
                output.push(io[0][0]);
            }
            for (n, native) in POINTS.into_iter().zip(expected) {
                assert!(
                    (output[n] - native).abs() < bound,
                    "native scalar bypass resume algorithm{algorithm} {tag} frame{n}: {} vs {native}",
                    output[n]
                );
            }
        }
    }
}
