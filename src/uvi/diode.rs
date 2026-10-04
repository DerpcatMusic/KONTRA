//! Original coupled capacitor/diode circuit mathematics for a measured channel
//! helper. This boundary precedes the native DC blocker and OutputGain stage;
//! it is not registered with Program playback.
//! Official controls: https://lua.uvi.net/_elements.html#diode-clipper
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{ensure, Result};

pub const FIDELITY_DIAGNOSTIC: &str = "DiodeClipper channel circuit with Asymmetry=0 only, before the native fixed DC blocker and OutputGain. 432 cold/warm, 24 stress, 6 long-tail and 48 bypass/resume authored comparisons are float32-identical, with native math executing without arithmetic hooks. Hosted controls, full callback, voice lifecycle and whole-program audio remain unverified; Program playback does not admit this leaf";

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
        ensure!(
            asymmetry == 0.,
            "DiodeClipper circuit helper only proves Asymmetry=0"
        );
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
        let sum = 1. + self.tone + self.high_pass;
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
                    let positive = (output * slope).exp();
                    let negative = (-output * slope * self.asymmetry).exp();
                    let residual =
                        (output * sum - target + high * diode_scale * (positive - negative)) / sum;
                    let derivative = 1.
                        + high / sum * diode_scale * slope * (positive + self.asymmetry * negative);
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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(circuit.configure(20_000., 1., 3., 1.).is_err());
        assert!(circuit.configure(f64::NAN, 1., 3., 0.).is_err());
        assert!(DiodeCircuit::new(13, 48_000.).is_err());
        circuit.reset();
        let mut reset = [[0.; MAX_CHANNELS]; 16];
        reset[0][0] = 1.;
        circuit.process(&mut reset, false).unwrap();
        assert_eq!(reset, frames);
    }
}
