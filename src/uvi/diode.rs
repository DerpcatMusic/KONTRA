//! Original coupled capacitor/diode circuit mathematics for a measured channel
//! helper and fixed DC rejection. The composed boundary precedes OutputGain;
//! it is not registered with Program playback.
//! Official controls: https://lua.uvi.net/_elements.html#diode-clipper
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "DiodeClipper circuit and fixed DC blocker before OutputGain, limited to six measured rates and direct physical controls. 432 circuit, 24 asymmetric stress, 624 DC/pre-gain and 576 pre-gain lifecycle authored comparisons are float32-identical, with unchanged native math and passive capture before gain. OutputGain magnitude conversion matches 513 native cases; its multiplication, complete native reset, hosted controls, full callback, voice lifecycle and whole-program audio remain unverified; Program playback does not admit this leaf";

/// Measured magnitude passed into native OutputGain dispatch. This conversion
/// does not apply gain to audio or establish the unavailable native multiply.
pub fn output_gain_multiplier(gain_db: f64) -> Result<f32> {
    ensure!(
        gain_db.is_finite() && (-40.0..=10.0).contains(&gain_db),
        "Invalid DiodeClipper OutputGain"
    );
    Ok(10f64.powf(f64::from(gain_db as f32) * 0.05) as f32)
}

#[derive(Clone, Copy, Default)]
struct State {
    capacitor: f64,
    output: f64,
    conditioned_input: f64,
}

pub struct DiodeCircuit {
    channels: usize,
    rate: f64,
    tone: f64,
    high_pass: f64,
    drive: f64,
    asymmetry: f64,
    states: [State; MAX_CHANNELS],
}

impl DiodeCircuit {
    pub fn new(channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "DiodeClipper needs 1..12 channels"
        );
        ensure!(
            [8_000., 32_000., 44_100., 48_000., 96_000., 192_000.].contains(&rate),
            "DiodeClipper circuit helper rate has no authored native comparison"
        );
        let mut result = Self {
            channels,
            rate,
            tone: 0.,
            high_pass: 0.,
            drive: 1.,
            asymmetry: 1.,
            states: [State::default(); MAX_CHANNELS],
        };
        result.configure(20_000., 1., 0., 0.)?;
        Ok(result)
    }

    /// Direct circuit fields. Preserve capacitor and output history on edits.
    /// OutputGain is outside this helper boundary.
    pub fn configure(
        &mut self,
        tone: f64,
        high_pass: f64,
        drive: f64,
        asymmetry: f64,
    ) -> Result<()> {
        for (name, value, low, high) in [
            ("Tone", tone, 100., 20_000.),
            ("HighPass", high_pass, 1., 20_000.),
            ("Drive", drive, 0., 30.),
            ("Asymmetry", asymmetry, 0., 1.),
        ] {
            ensure!(
                value.is_finite() && (low..=high).contains(&value),
                "Invalid DiodeClipper {name}"
            );
        }
        // The measured circuit widens a float32-rounded tau to double.
        let tau = f64::from(std::f32::consts::TAU);
        self.tone = f64::from(tone as f32) * tau * (1. / self.rate);
        self.high_pass = f64::from(high_pass as f32) * tau * (1. / self.rate);
        self.drive = 10f64.powf(f64::from(drive as f32) * 0.05);
        self.asymmetry = 10f64.powf(f64::from(asymmetry as f32));
        Ok(())
    }

    pub fn reset(&mut self) {
        self.states.fill(State::default());
    }

    pub fn process(&mut self, frames: &mut [Frame], bypass: bool) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|f| f[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite DiodeClipper input"
        );
        if bypass {
            return Ok(());
        }
        let slope = 22.075055187637968;
        let diode_scale = self.tone * 5.5461999999999995e-6;
        let sum = self.tone + self.high_pass + 1.;
        let high = 1. + self.high_pass;
        for frame in frames {
            for (sample, state) in frame[..self.channels].iter_mut().zip(&mut self.states) {
                let input = (f64::from(*sample) * self.drive * (1. / 4.5)).tanh() * 4.5;
                let target = ((input * self.tone + state.output * self.high_pass)
                    - state.capacitor * self.tone)
                    + state.output;
                let mut output = state.output;
                let mut capacitor = state.capacitor;
                for _ in 0..10 {
                    // Scale the implicit equation by the reverse exponential.
                    // This preserves the measured finite-iteration trajectory
                    // for stiff asymmetric transitions; an algebraically
                    // simplified residual amplifies rounding differences there.
                    let reverse_factor = self.asymmetry * slope;
                    let exponential = (output * reverse_factor).exp();
                    let forward = (output * slope * (self.asymmetry + 1.)).exp();
                    let weight = (-output * reverse_factor).exp() * (1. / sum);
                    let diode = diode_scale * (forward - 1.);
                    let forward_slope = diode_scale * slope * forward;
                    let reverse_slope = diode_scale * reverse_factor;
                    let residual = (((output * sum * exponential + diode * self.high_pass)
                        - target * exponential)
                        + diode)
                        * weight;
                    let derivative = ((reverse_slope + exponential + forward_slope)
                        * self.high_pass
                        + exponential * self.tone
                        + (forward_slope + reverse_slope + exponential))
                        * weight;
                    if derivative.abs() < 1e-10 {
                        break;
                    }
                    let delta = (-residual / derivative).clamp(-0.5, 0.5);
                    output = (output + delta).clamp(-1.5, 1.5);
                    capacitor = ((input - output) * self.high_pass + state.capacitor) / high;
                    if delta.abs() < 0.001 {
                        break;
                    }
                }
                state.capacitor = capacitor;
                state.output = output;
                state.conditioned_input = input;
                *sample = output as f32;
                ensure!(sample.is_finite(), "Nonfinite DiodeClipper output");
            }
        }
        Ok(())
    }
}

/// Fixed DC rejection after the circuit, preserving the native stereo phase
/// history and four-frame arithmetic. OutputGain is outside this helper.
pub struct DiodeDcBlocker {
    channels: usize,
    coefficient: f32,
    state: [[f32; 4]; MAX_CHANNELS],
}

impl DiodeDcBlocker {
    pub fn new(channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Diode DC blocker needs 1..12 channels"
        );
        ensure!(
            [8_000., 32_000., 44_100., 48_000., 96_000., 192_000.].contains(&rate),
            "Diode DC blocker rate has no authored native comparison"
        );
        let inverse = f64::from(1f32 / rate as f32);
        let coefficient = (-std::f64::consts::TAU * inverse).exp() as f32;
        Ok(Self {
            channels,
            coefficient,
            state: [[0.; 4]; MAX_CHANNELS],
        })
    }

    pub fn reset(&mut self) {
        self.state.fill([0.; 4]);
    }

    pub fn process(&mut self, frames: &mut [Frame], bypass: bool) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|f| f[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite Diode DC input"
        );
        if bypass {
            return Ok(());
        }
        let p = self.coefficient;
        let a = 1. - p;
        if self.channels == 2 {
            let square = p * p;
            let cross = p * a;
            for channel in 0..2 {
                let state = &mut self.state[channel];
                let mut groups = frames.chunks_exact_mut(4);
                for group in &mut groups {
                    let [x0, x1, x2, x3] = std::array::from_fn(|i| group[i][channel]);
                    // Two-step one-pole recurrence, evaluated in native order.
                    let z0 = x0 * a + state[3] * cross + state[0] * square;
                    let z1 = x1 * a + x0 * cross + state[1] * square;
                    let z2 = x1 * cross + x2 * a + z0 * square;
                    let z3 = x2 * cross + x3 * a + z1 * square;
                    for (frame, value) in group.iter_mut().zip([x0 - z0, x1 - z1, x2 - z2, x3 - z3])
                    {
                        frame[channel] = value;
                    }
                    *state = [z2, z3, x2, x3];
                }
                for frame in groups.into_remainder() {
                    let input = frame[channel];
                    let low_pass = (input - state[1]) * a + state[1];
                    frame[channel] = input - low_pass;
                    *state = [state[1], low_pass, state[3], input];
                }
            }
        } else {
            for frame in frames {
                for (sample, state) in frame[..self.channels].iter_mut().zip(&mut self.state) {
                    state[1] = state[1] + (*sample - state[1]) * a;
                    *sample -= state[1];
                }
            }
        }
        Ok(())
    }
}

/// Measured callback boundary before OutputGain; not a complete DiodeClipper.
pub struct DiodePreGain {
    circuit: DiodeCircuit,
    dc: DiodeDcBlocker,
}

impl DiodePreGain {
    pub fn new(channels: usize, rate: f64) -> Result<Self> {
        Ok(Self {
            circuit: DiodeCircuit::new(channels, rate)?,
            dc: DiodeDcBlocker::new(channels, rate)?,
        })
    }
    pub fn configure(
        &mut self,
        tone: f64,
        high_pass: f64,
        drive: f64,
        asymmetry: f64,
    ) -> Result<()> {
        self.circuit.configure(tone, high_pass, drive, asymmetry)
    }
    pub fn reset(&mut self) {
        self.circuit.reset();
        self.dc.reset();
    }
    pub fn process(&mut self, frames: &mut [Frame], bypass: bool) -> Result<()> {
        self.circuit.process(frames, bypass)?;
        self.dc.process(frames, bypass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_native_output_gain_conversion_only() {
        for (value, bits) in [
            (-40., 1008981770),
            (-3., 1060453359),
            (0., 1065353216),
            (10., 1078616770),
            (3.14159, 1069008571),
            (-17.54321, 1040703487),
        ] {
            assert_eq!(output_gain_multiplier(value).unwrap().to_bits(), bits);
        }
        for value in [f64::NAN, f64::INFINITY, -40.1, 10.1] {
            assert!(output_gain_multiplier(value).is_err());
        }
    }

    #[test]
    fn authored_native_impulse_and_retained_state() {
        let mut circuit = DiodeCircuit::new(2, 48_000.).unwrap();
        circuit.configure(20_000., 1., 3., 0.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 16];
        frames[0][0] = 1.;
        circuit.process(&mut frames[..7], false).unwrap();
        let mut dry = [[0.25; MAX_CHANNELS]; 17];
        let original = dry;
        circuit.process(&mut dry, true).unwrap();
        assert_eq!(dry, original);
        circuit.process(&mut frames[7..], false).unwrap();
        let expected: [f32; 16] = [
            0.52790403,
            0.14574495,
            0.040213335,
            0.0110519696,
            0.0029936153,
            0.00076679175,
            0.00015143739,
            -0.00001860799,
            -0.00006559787,
            -0.000078582925,
            -0.00008217118,
            -0.00008316274,
            -0.000083436746,
            -0.00008351247,
            -0.000083533385,
            -0.00008353916,
        ];
        for (frame, value) in frames.iter().zip(expected) {
            assert_eq!(frame[0].to_bits(), value.to_bits());
            assert_eq!(frame[1], 0.);
        }
        assert!(circuit.configure(20_000., 1., 3., 1.1).is_err());
        assert!(circuit.configure(f64::NAN, 1., 3., 0.).is_err());
        assert!(DiodeCircuit::new(13, 48_000.).is_err());
        circuit.reset();
        let mut reset = [[0.; MAX_CHANNELS]; 16];
        reset[0][0] = 1.;
        circuit.process(&mut reset, false).unwrap();
        assert_eq!(reset, frames);
    }

    #[test]
    fn authored_native_asymmetric_stiff_transition_before_gain() {
        let mut processor = DiodePreGain::new(2, 8_000.).unwrap();
        processor.configure(20_000., 1., 30., 1.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 24];
        for (i, frame) in frames.iter_mut().enumerate() {
            frame[0] = if i % 2 == 0 { 32. } else { -32. };
        }
        processor.process(&mut frames, false).unwrap();
        let expected = [
            1058841179, 3202888366, 3201357596, 3199828028, 3198299661, 3196772494, 3194433404,
            3191383864, 3188336718, 3182913421, 1065036188, 3179421335, 1059557347, 3178960802,
            1060615690, 3203839509, 3202307994, 3200777679, 3199248566, 3197720654, 3196193942,
            3193277209, 3190228576, 3186693636,
        ];
        for (frame, bits) in frames.iter().zip(expected) {
            assert_eq!(frame[0].to_bits(), bits);
            assert_eq!(frame[1], 0.);
        }
        processor.reset();
        let mut reset = [[0.; MAX_CHANNELS]; 24];
        for (i, frame) in reset.iter_mut().enumerate() {
            frame[0] = if i % 2 == 0 { 32. } else { -32. };
        }
        processor.process(&mut reset, false).unwrap();
        assert_eq!(reset, frames);
    }

    #[test]
    fn authored_native_stereo_dc_split_and_bypass() {
        let mut blocker = DiodeDcBlocker::new(2, 48_000.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 8];
        frames[0][0] = 1.;
        blocker.process(&mut frames[..5], false).unwrap();
        let mut dry = [[0.25; MAX_CHANNELS]; 3];
        let original = dry;
        blocker.process(&mut dry, true).unwrap();
        assert_eq!(dry, original);
        blocker.process(&mut frames[5..], false).unwrap();
        let expected = [
            1065351020, 3104389991, 3104388813, 3104387637, 3104386460, 3104385283, 3104384106,
            3104382930,
        ];
        for (frame, bits) in frames.iter().zip(expected) {
            assert_eq!(frame[0].to_bits(), bits);
            assert_eq!(frame[1], 0.);
        }
        blocker.reset();
        let mut reset = [[0.; MAX_CHANNELS]; 8];
        reset[0][0] = 1.;
        blocker.process(&mut reset[..5], false).unwrap();
        blocker.process(&mut reset[5..], false).unwrap();
        assert_eq!(reset, frames);
        assert!(DiodeDcBlocker::new(2, 22_050.).is_err());
        assert!(DiodePreGain::new(0, 48_000.).is_err());
    }
}
