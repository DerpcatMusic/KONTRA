//! Static, unconnected Compressor Expander. Public physical controls:
//! https://lua.uvi.net/_elements.html#compressor-expander . Authored comparisons
//! execute the original native DSP core constructor, fresh clone, preparation and
//! complete audio wrapper; the initialized metadata factory/host is not proved.
//! GateThreshold=-130 stays below the native detector's 1e-5 amplitude floor,
//! so gate gain remains unity. Automatic makeup and live controls stay gated.
use super::{
    dsp::{Frame, MAX_CHANNELS},
    host::ParameterValue,
    program::ProgramNode,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

pub const FIDELITY_DIAGNOSTIC: &str = "UVI CompExp shared static unconnected controls with GateThreshold=-130 and AutoMakeUp=0 produced bit-identical measured PCM and reset/split histories in 288 authored native DSP wrapper cases at 32/44.1/48kHz and 1/2/6/12 channels (observed, not a universal numerical bound); linked float32 RMS, smoothing and lookahead including Mix=0 delay are measured; initialized host, connected/changed live controls, bypass, automatic makeup, active gate, keygroup voice-tail lifetime, short positive time controls and unmeasured rates/channel counts remain unsupported";
// Name, default, physical minimum, physical maximum.
const PARAMETERS: [(&str, f64, f64, f64); 12] = [
    ("Bypass", 0., 0., 1.),
    ("AutoMakeUp", 0., 0., 1.),
    ("MakeUpGain", 0., -30., 30.),
    ("CompThreshold", 0., -130., 0.),
    ("CompRatio", 10., 1., 30.),
    ("CompAttack", 10., 0., 500.),
    ("CompRelease", 100., 0., 5000.),
    ("GateThreshold", -130., -130., 0.),
    ("GateRatio", 1., 0.1, 30.),
    ("GateAttack", 10., 0., 500.),
    ("GateRelease", 100., 0., 5000.),
    ("Mix", 1., 0., 1.),
];
fn index(name: &str) -> Result<usize> {
    PARAMETERS
        .iter()
        .position(|p| p.0 == name)
        .with_context(|| format!("Unsupported CompExp parameter {name}"))
}
fn values(attributes: &BTreeMap<String, String>) -> Result<[f64; 12]> {
    let mut p = PARAMETERS.map(|p| p.1);
    for (name, raw) in attributes {
        if name == "Name" {
            continue;
        }
        p[index(name)?] = raw
            .parse()
            .with_context(|| format!("Invalid CompExp parameter {name}"))?;
    }
    for (value, (_, _, min, max)) in p.iter().zip(PARAMETERS) {
        ensure!(
            value.is_finite() && (min..=max).contains(value),
            "Invalid CompExp physical parameter"
        );
    }
    for i in [5, 9] {
        ensure!(
            p[i] == 0. || p[i] >= 1.,
            "CompExp sub-millisecond positive attack is unverified"
        );
    }
    for i in [6, 10] {
        ensure!(
            p[i] == 0. || p[i] >= 2.,
            "CompExp short positive release is unverified"
        );
    }
    ensure!(p[0] == 0., "CompExp bypass lifecycle is unverified");
    ensure!(p[1] == 0., "CompExp automatic makeup is unverified");
    ensure!(p[7] == -130., "CompExp active gate threshold is unverified");
    Ok(p)
}
fn parameters(node: &ProgramNode) -> Result<[f64; 12]> {
    ensure!(
        supports(&node.kind),
        "Unsupported UVI compressor {}",
        node.kind
    );
    values(&node.attributes)
}
pub fn supports(kind: &str) -> bool {
    kind == "CompExp"
}
pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}
fn number(value: &ParameterValue) -> Result<f64> {
    match value {
        ParameterValue::Number(v) if v.is_finite() => Ok(*v),
        ParameterValue::Boolean(v) => Ok(f64::from(u8::from(*v))),
        _ => bail!("CompExp requires finite numeric parameters"),
    }
}
fn check_write(p: &[f64; 12], name: &str, value: &ParameterValue) -> Result<()> {
    ensure!(
        number(value)? == p[index(name)?],
        "CompExp live controls and bypass transitions are unverified"
    );
    Ok(())
}
/// Enforce the static boundary before any inactive or active owner is changed.
pub fn validate_static_write(
    attributes: &BTreeMap<String, String>,
    name: &str,
    value: &ParameterValue,
) -> Result<()> {
    check_write(&values(attributes)?, name, value)
}

pub struct CompExp {
    parameters: [f64; 12],
    channels: usize,
    delay: Vec<Frame>,
    cursor: usize,
    rms: f32,
    gain: f32,
    post: f32,
    threshold: f32,
    inverse: f32,
    attack: f32,
    release: f32,
    release_bias: f32,
    smooth: f32,
    average: f32,
    makeup: f32,
    mix: f32,
}
impl CompExp {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            [1, 2, 6, 12].contains(&channels),
            "Unmeasured CompExp channel count"
        );
        ensure!(
            [32000., 44100., 48000.].contains(&rate),
            "Unmeasured CompExp sample rate"
        );
        let p = parameters(node)?;
        let rate = rate as f32;
        let attack = p[5] as f32;
        let release = p[6] as f32;
        let common = attack.min(release);
        let alpha = |ms: f32| {
            if ms <= 0. {
                1.
            } else {
                1. - ((2. / rate) * (-1000. / ms)).exp()
            }
        };
        let release_exp = if release <= 0. {
            0.
        } else {
            ((2. / rate) * (-1000. / release)).exp()
        };
        Ok(Self {
            parameters: p,
            channels,
            delay: vec![[0.; MAX_CHANNELS]; (rate * 0.001) as usize],
            cursor: 0,
            rms: 0.,
            gain: 1.,
            post: 1.,
            threshold: 10f32.powf(p[3] as f32 * 0.05),
            inverse: 1. / p[4] as f32,
            attack: alpha(attack),
            release: release_exp,
            release_bias: 1. - release_exp,
            smooth: alpha(common * 0.5),
            average: alpha(common * 0.125),
            makeup: 10f32.powf(p[2] as f32 * 0.05),
            mix: p[11] as f32,
        })
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        Ok(ParameterValue::Number(self.parameters[index(name)?]))
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        check_write(&self.parameters, name, value)
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.delay.capacity() * std::mem::size_of::<Frame>()
    }
    /// The original clear method clears the delay and detector/envelope histories.
    pub fn clear(&mut self) {
        self.delay.fill([0.; MAX_CHANNELS]);
        self.cursor = 0;
        self.rms = 0.;
        self.gain = 1.;
        self.post = 1.;
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        for frame in frames {
            let mut squares = 0.;
            for x in &frame[..self.channels] {
                squares += *x * *x;
            }
            let mean = squares * (1. / self.channels as f32);
            self.rms = (mean - self.rms) * self.average + self.rms;
            let amplitude = self.rms.sqrt().max(1e-5);
            let target = if amplitude >= self.threshold && self.inverse != 1. {
                (amplitude / self.threshold).powf(self.inverse - 1.)
            } else {
                1.
            };
            self.gain = if target < self.gain {
                (target - self.gain) * self.attack + self.gain
            } else {
                self.gain * self.release + self.release_bias
            };
            self.post = (self.gain - self.post) * self.smooth + self.post;
            let scale = (self.post * self.makeup - 1.) * self.mix + 1.;
            let next = if self.cursor + 1 < self.delay.len() {
                self.cursor + 1
            } else {
                0
            };
            for (c, x) in frame[..self.channels].iter_mut().enumerate() {
                self.delay[self.cursor][c] = *x;
                *x = scale * self.delay[next][c];
            }
            self.cursor = next;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::program::parse_program;
    fn node(attributes: &str) -> ProgramNode {
        parse_program(&format!("<Program><CompExp {attributes}/></Program>"))
            .unwrap()
            .nodes
            .remove(1)
    }
    #[test]
    fn measured_mix_zero_retains_native_lookahead_and_channel_independence() {
        for (rate, delay) in [(32000., 31), (44100., 43), (48000., 47)] {
            for channels in [1, 2, 6, 12] {
                let mut fx = CompExp::new(&node("Mix='0'"), channels, rate).unwrap();
                let mut frames = vec![[0.; MAX_CHANNELS]; delay + 2];
                for c in 0..channels {
                    frames[0][c] = (c + 1) as f32 / 16.;
                }
                let impulse = frames[0];
                fx.process(&mut frames).unwrap();
                assert!(frames[..delay].iter().all(|f| *f == [0.; MAX_CHANNELS]));
                assert_eq!(frames[delay], impulse);
                assert_eq!(frames[delay + 1], [0.; MAX_CHANNELS]);
                assert_eq!(
                    fx.memory_bytes(),
                    std::mem::size_of::<CompExp>() + (delay + 1) * std::mem::size_of::<Frame>()
                );
            }
        }
    }
    #[test]
    fn static_writes_preserve_history_and_clear_restores_fresh_state() {
        let node = node(
            "CompThreshold='-25' CompRatio='4' CompAttack='1' CompRelease='2' MakeUpGain='12' Mix='.6'",
        );
        let input: Vec<Frame> = (0..300)
            .map(|i| {
                let mut f = [0.; MAX_CHANNELS];
                f[0] = ((i * 13 % 31) as f32 - 15.) / 16.;
                f[1] = -0.3;
                f
            })
            .collect();
        let mut reference = CompExp::new(&node, 2, 48000.).unwrap();
        let mut expected = input.clone();
        reference.process(&mut expected).unwrap();
        let mut fx = CompExp::new(&node, 2, 48000.).unwrap();
        let mut actual = input.clone();
        fx.process(&mut actual[..73]).unwrap();
        fx.set_parameter("CompThreshold", &ParameterValue::Number(-25.))
            .unwrap();
        fx.set_parameter("Bypass", &ParameterValue::Boolean(false))
            .unwrap();
        assert!(
            fx.set_parameter("CompThreshold", &ParameterValue::Number(-24.))
                .is_err()
        );
        assert!(
            fx.set_parameter("Bypass", &ParameterValue::Boolean(true))
                .is_err()
        );
        assert!(
            fx.set_parameter("Unknown", &ParameterValue::Number(0.))
                .is_err()
        );
        fx.process(&mut actual[73..]).unwrap();
        assert_eq!(actual, expected);
        fx.clear();
        let mut reset = input;
        for chunk in reset.chunks_mut(17) {
            fx.process(chunk).unwrap();
        }
        assert_eq!(reset, expected);
    }
    #[test]
    fn unmeasured_controls_rates_and_widths_stay_gated() {
        for attributes in [
            "Bypass='1'",
            "AutoMakeUp='1'",
            "GateThreshold='-129'",
            "CompAttack='.001'",
            "GateAttack='.5'",
            "CompRelease='1'",
            "GateRelease='1.5'",
            "Mix='nan'",
            "CompRatio='31'",
            "Other='0'",
        ] {
            assert!(validate(&node(attributes)).is_err(), "{attributes}");
        }
        for attributes in [
            "",
            "CompAttack='0' CompRelease='0' GateAttack='0' GateRelease='0'",
            "CompAttack='500' CompRelease='5000' GateRatio='.1'",
            "CompThreshold='-130' MakeUpGain='-30' Mix='0'",
        ] {
            validate(&node(attributes)).unwrap();
        }
        assert!(CompExp::new(&node(""), 0, 48000.).is_err());
        assert!(CompExp::new(&node(""), 13, 48000.).is_err());
        assert!(CompExp::new(&node(""), 3, 48000.).is_err());
        assert!(CompExp::new(&node(""), 2, 96000.).is_err());
        assert!(
            validate_static_write(
                &node("").attributes,
                "Mix",
                &ParameterValue::Text("1".into())
            )
            .is_err()
        );
    }
}
