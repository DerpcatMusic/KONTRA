//! Original Xpander constant-Q mathematics, measured against official UVI
//! Workstation 4.0.9 with authored impulses and DC signals on 2026-10-03.
//! Parameter ranges: https://lua.uvi.net/_elements.html#xpander-filter
//! Shape/topology: https://www.uvi.net/falcon and the official Falcon manual,
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf
//! Native numerical observations establish the 37 tap combinations, frequency
//! and resonance polynomials, rational saturation and two-phase allpass path.
//! Algorithm II uses bilinear TPT stages with a measured zero-delay solve.
//! No vendor source expression was used.

use super::{
    dsp::{Frame, MAX_CHANNELS},
    host::ParameterValue,
    program::ProgramNode,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI Xpander shapes, both solvers and saturation have authored native comparisons at 48 kHz; other rates, live parameter transitions and exact float-rounding parity remain unverified";
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

// Independently fitted from the authored LP1 + HP1 impulse. These mathematical
// coefficients describe the native path; they are not copied implementation.
const ALLPASS: [f64; 6] = [
    0.04418161, 0.16418989, 0.33074303, 0.51508740, 0.70205269, 0.89488971,
];

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
    up: [f64; 6],
    down: [f64; 6],
    previous_input: [f64; 4],
    previous_output: [f64; 4],
}

fn allpass(mut x: f64, state: &mut [f64; 6], phase: usize) -> f64 {
    for i in (phase..6).step_by(2) {
        let y = ALLPASS[i] * x + state[i];
        state[i] = x - ALLPASS[i] * y;
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
    stage: f64,
    pole: f64,
    feedback: f64,
    compensation: f64,
    denominator: f64,
    drive: f64,
    normalization: f64,
    feedback_gain: f64,
    state: [State; MAX_CHANNELS],
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
        let mut result = Self {
            channels,
            rate,
            parameters: parameters(node)?,
            note: 60,
            stage: 0.,
            pole: 0.,
            feedback: 0.,
            compensation: 0.,
            denominator: 1.,
            drive: 1.,
            normalization: 1.,
            feedback_gain: 1.,
            state: [State::default(); MAX_CHANNELS],
        };
        result.configure()?;
        Ok(result)
    }

    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        self.parameters
            .get(name)
            .copied()
            .map(ParameterValue::Number)
            .context("Unknown UVI filter parameter")
    }

    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let value = match value {
            ParameterValue::Number(n) => *n,
            ParameterValue::Boolean(b) if name == "Bypass" => f64::from(u8::from(*b)),
            _ => bail!("Numeric UVI filter parameter required"),
        };
        check(name, value)?;
        if self.parameters.get(name).copied() != Some(value) {
            let previous = self
                .parameters
                .insert(name.into(), value)
                .expect("validated key");
            if let Err(error) = self.configure() {
                self.parameters.insert(name.into(), previous);
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn set_note(&mut self, note: u8) -> Result<()> {
        ensure!(note <= 127, "Invalid UVI filter MIDI note");
        if self.note != note {
            let previous = self.note;
            self.note = note;
            if let Err(error) = self.configure() {
                self.note = previous;
                return Err(error);
            }
        }
        Ok(())
    }

    fn configure(&mut self) -> Result<()> {
        let p = &self.parameters;
        let frequency = (p["Freq"]
            * ((f64::from(self.note) - 60.) * p["KeyTracking"] / 12.).exp2())
        .clamp(20., 20_000.);
        if p["Algorithm"] == 0. {
            let w = std::f64::consts::PI * frequency / self.rate;
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
            let rate = self.rate * if p["Oversampling"] == 0. { 1. } else { 2. };
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
        let phases = if !zero_delay || self.parameters["Oversampling"] != 0. {
            2
        } else {
            1
        };
        for frame in io {
            for (channel, x) in frame[..self.channels].iter_mut().enumerate() {
                let state = &mut self.state[channel];
                let mut output = 0.;
                // Algorithm I uses the measured 2x path for both toggle values.
                // Algorithm II honors Oversampling, including its direct path.
                for phase in 0..phases {
                    let up = if phases == 2 {
                        allpass(f64::from(*x), &mut state.up, phase)
                    } else {
                        f64::from(*x)
                    };
                    let offsets = state.previous_output.map(|y| (1. - self.stage) * y);
                    let feedback_offset = if zero_delay {
                        offsets
                            .iter()
                            .fold(0., |sum, offset| self.stage * sum + offset)
                    } else {
                        state.previous_output[3]
                    };
                    let input = (up * self.compensation
                        - self.feedback * feedback_offset / self.feedback_gain)
                        * self.drive;
                    let mut tap = match distortion {
                        0 => rational(input) / self.normalization,
                        1 => input.clamp(-1., 1.) / self.normalization,
                        2 => input,
                        _ => unreachable!(),
                    } / self.denominator;
                    let mut result = mix[0] * tap;
                    for (stage, offset) in offsets.into_iter().enumerate() {
                        let y = if zero_delay {
                            self.stage * tap + offset
                        } else {
                            self.stage * (tap + 0.3 * state.previous_input[stage])
                                + self.pole * state.previous_output[stage]
                        };
                        state.previous_input[stage] = tap;
                        state.previous_output[stage] = if zero_delay {
                            2. * y - state.previous_output[stage]
                        } else {
                            y
                        };
                        tap = y;
                        result += mix[stage + 1] * y;
                    }
                    output += if phases == 2 {
                        allpass(result, &mut state.down, 1 - phase)
                    } else {
                        result
                    };
                }
                *x = (output / phases as f64) as f32;
                ensure!(x.is_finite(), "Nonfinite UVI filter output");
            }
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
        let mut fx = filter("DistortionType=\"2\"", 1);
        let previous = fx.parameter("Algorithm").unwrap();
        assert!(
            fx.set_parameter("Algorithm", &ParameterValue::Number(2.))
                .is_err()
        );
        assert_eq!(fx.parameter("Algorithm").unwrap(), previous);
        fx.set_parameter("Bypass", &ParameterValue::Boolean(true))
            .unwrap();
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
        assert!(
            low_rate
                .set_parameter("Freq", &ParameterValue::Number(20000.))
                .is_err()
        );
        assert_eq!(
            low_rate.parameter("Freq").unwrap(),
            ParameterValue::Number(1000.)
        );
    }
}
