//! Bounded stationary Exciter model from original Workstation PCM16 probes.
//! Public ranges: https://lua.uvi.net/_elements.html#exciter.
//! Mode0 sine law and Mode1 envelope-normalized quadratic/cubic terms were
//! independently checked against steps and stereo chirps. Mode1's hidden
//! 48-kHz constants below are measured fits, not native bit-exact constants.

use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "Exciter admits stationary 48-kHz stereo Modes0/1 at Oversampling0; Mode1 envelope and harmonic coefficients are fitted from authored native responses and float bit parity is unverified. Other rates/channel layouts, oversampling and live control/bypass transitions remain unimplemented.";
const PARAMETERS: [(&str, f64, f64, f64, bool); 7] = [
    ("Bypass", 0., 0., 1., true),
    ("InputGain", 0., -30., 20., false),
    ("Amount", 0., 0., 15., false),
    ("Mix", 100., 0., 100., false),
    ("Mode", 1., 0., 1., true),
    ("OutputGain", 0., -40., 10., false),
    ("Oversampling", 0., 0., 4., true),
];
const ENVELOPE_POLE_48K: f64 = 0.953549793;
const SECOND_HARMONIC: f64 = 0.008933677;
const THIRD_HARMONIC: f64 = 0.0059707525;

pub fn supports(kind: &str) -> bool {
    kind == "Exciter"
}
fn checked(name: &str, value: &ParameterValue) -> Result<(usize, f64)> {
    let index = PARAMETERS
        .iter()
        .position(|p| p.0 == name)
        .context("Unknown Exciter parameter")?;
    let (_, _, low, high, integer) = PARAMETERS[index];
    let value = match value {
        ParameterValue::Number(value) => *value,
        ParameterValue::Boolean(value) => f64::from(u8::from(*value)),
        ParameterValue::Text(_) => anyhow::bail!("Exciter requires finite numeric parameters"),
    };
    ensure!(
        value.is_finite() && (low..=high).contains(&value) && (!integer || value.fract() == 0.),
        "Invalid Exciter parameter {name}"
    );
    Ok((index, f64::from(value as f32)))
}
fn parameters(node: &ProgramNode) -> Result<[f64; 7]> {
    ensure!(supports(&node.kind), "Unsupported Exciter processor kind");
    let mut values = PARAMETERS.map(|p| p.1);
    for (name, value) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let value = ParameterValue::Number(
            value
                .parse()
                .with_context(|| format!("Invalid Exciter parameter {name}"))?,
        );
        let (index, value) = checked(name, &value)?;
        values[index] = value;
    }
    ensure!(
        values[6] == 0.,
        "Exciter oversampling is not native-measured"
    );
    Ok(values)
}
pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

pub struct Exciter {
    parameters: [f64; 7],
    envelope: [f64; 2],
    input_gain: f64,
    output_gain: f64,
    second: f64,
    third: f64,
    sine_drive: f64,
}
impl Exciter {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            channels == 2 && rate == 48000.,
            "Exciter requires a measured 48-kHz stereo bus"
        );
        let parameters = parameters(node)?;
        let amount = parameters[2];
        Ok(Self {
            parameters,
            envelope: [0.; 2],
            input_gain: 10f64.powf(parameters[1] / 20.),
            output_gain: 10f64.powf(parameters[5] / 20.),
            second: SECOND_HARMONIC * 10f64.powf(amount / 20.),
            third: THIRD_HARMONIC * 10f64.powf(0.075 * amount),
            sine_drive: std::f64::consts::TAU / 5. * 10f64.powf(amount / 30.),
        })
    }
    pub fn output_channels(&self) -> usize {
        2
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    pub fn clear(&mut self) {
        self.envelope.fill(0.);
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        let index = PARAMETERS
            .iter()
            .position(|p| p.0 == name)
            .context("Unknown Exciter parameter")?;
        Ok(ParameterValue::Number(self.parameters[index]))
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let (index, value) = checked(name, value)?;
        ensure!(
            value == self.parameters[index],
            "Live Exciter parameter transitions are not native-measured"
        );
        Ok(())
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|frame| frame[..2].iter().all(|v| v.is_finite())),
            "Nonfinite Exciter input"
        );
        if self.parameters[0] != 0. || self.parameters[3] == 0. {
            return Ok(());
        }
        let mix = self.parameters[3] / 100.;
        for frame in frames.iter_mut() {
            for (ch, sample) in frame[..2].iter_mut().enumerate() {
                let dry = f64::from(*sample);
                let input = dry * self.input_gain;
                let wet = if self.parameters[4] == 0. {
                    (input * self.sine_drive).sin() / (std::f64::consts::TAU / 5.).sin()
                } else {
                    let envelope = &mut self.envelope[ch];
                    *envelope =
                        ENVELOPE_POLE_48K * *envelope + (1. - ENVELOPE_POLE_48K) * input.abs();
                    let square = *envelope * *envelope;
                    // Independent native low-level captures establish separate
                    // square/cubic denominator floors; a shared floor fails.
                    input * (1. + self.parameters[2] / 5.)
                        + self.second * input * input * *envelope / square.max(1e-6)
                        + self.third * input * input * input * *envelope
                            / (square * *envelope).max(1e-9)
                };
                *sample = ((1. - mix) * dry + mix * self.output_gain * wet) as f32;
            }
        }
        ensure!(
            frames
                .iter()
                .all(|frame| frame[..2].iter().all(|v| v.is_finite())),
            "Nonfinite Exciter output"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(mode: u8, amount: f64) -> ProgramNode {
        ProgramNode {
            parent: None,
            kind: "Exciter".into(),
            name: None,
            attributes: [
                ("Mode".into(), mode.to_string()),
                ("Amount".into(), amount.to_string()),
            ]
            .into(),
            text: String::new(),
        }
    }
    #[test]
    fn authored_native_static_curves_envelope_fragmentation_and_channel_isolation() {
        let mut n = node(1, 2.);
        let mut fx = Exciter::new(&n, 2, 48000.).unwrap();
        let mut first = [[0.; 12]; 1];
        first[0][0] = -31129. / 32768.;
        first[0][1] = -29490. / 32768.;
        fx.process(&mut first).unwrap();
        assert!((f64::from(first[0][0]) - -4.813335418701172).abs() < 2e-6);
        assert!((f64::from(first[0][1]) - -4.5599045753479).abs() < 2e-6);
        for mode in [0, 1] {
            n.attributes.insert("Mode".into(), mode.to_string());
            let mut whole = Exciter::new(&n, 2, 48000.).unwrap();
            let mut split = Exciter::new(&n, 2, 48000.).unwrap();
            let mut input: Vec<Frame> = (0..1024)
                .map(|i| {
                    let mut f = [0.; 12];
                    f[0] = (i as f32 * 0.017).sin() * 0.4;
                    f[1] = (i as f32 * 0.13).cos() * 0.1;
                    f[7] = 0.25;
                    f
                })
                .collect();
            let mut fragmented = input.clone();
            whole.process(&mut input).unwrap();
            for block in fragmented.chunks_mut(7) {
                split.process(block).unwrap();
            }
            assert_eq!(input, fragmented);
            assert!(
                input
                    .iter()
                    .all(|f| f.iter().all(|v| v.is_finite()) && f[7] == 0.25)
            );
        }
    }
    #[test]
    fn unsupported_settings_and_failed_updates_remain_atomic() {
        let n = node(1, 2.);
        let mut fx = Exciter::new(&n, 2, 48000.).unwrap();
        assert!(
            fx.set_parameter("Amount", &ParameterValue::Number(3.))
                .is_err()
        );
        assert!(
            fx.set_parameter("Amount", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
        assert_eq!(fx.parameter("Amount").unwrap(), ParameterValue::Number(2.));
        let mut n = n;
        n.attributes.insert("Oversampling".into(), "1".into());
        assert!(validate(&n).is_err());
        n.attributes.insert("Oversampling".into(), "0".into());
        n.attributes.insert("Unknown".into(), "0".into());
        assert!(validate(&n).is_err());
        assert!(Exciter::new(&node(1, 2.), 1, 48000.).is_err());
        assert!(Exciter::new(&node(1, 2.), 2, 44100.).is_err());
    }
}
