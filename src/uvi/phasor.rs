//! Original Phasor mathematics measured with authored PCM16 impulse trains against
//! official UVI Workstation 4.0.9 (2026-10-03), including positive 8/32/48/96 kHz
//! comparisons. Public parameters: https://lua.uvi.net/_elements.html#Phasor.
//! No vendor source or preset/sample contents are included.
use super::{dsp::Frame, host::ParameterValue, program::ProgramNode};
use anyhow::{Context, Result, bail, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "UVI Phasor stationary filters, triangle/sine sweeps, stereo spread and synced rates have authored native comparisons; live controls and bypass/resume have authored comparisons; other tempos, LFO-mode/sync transitions and high-feedback float rounding remain unverified";
// Name, default, min, max, integral. Synced Speed=12 is independently measured
// from an authored fixture; the public unsynced limit remains 10 Hz.
const CONTROLS: [(&str, f32, f32, f32, bool); 10] = [
    ("Bypass", 0., 0., 1., true),
    ("SyncToHost", 0., 0., 1., true),
    ("Speed", 0.3, 0.01, 12., false),
    ("MinFreq", 200., 20., 20000., false),
    ("MaxFreq", 3000., 20., 20000., false),
    ("Feedback", 0.7, -0.99, 0.99, false),
    ("Depth", 1., 0., 1., false),
    ("Spread", 1., 0., 1., false),
    ("Order", 3., 1., 12., true),
    ("LFOSHape", 0., 0., 3., true),
];
fn index(name: &str) -> Result<usize> {
    CONTROLS
        .iter()
        .position(|p| p.0 == name)
        .with_context(|| format!("Unsupported Phasor parameter {name}"))
}
fn checked(name: &str, value: &ParameterValue) -> Result<f32> {
    let i = index(name)?;
    let raw = match value {
        ParameterValue::Number(v) if v.is_finite() => *v,
        ParameterValue::Boolean(v) => f64::from(u8::from(*v)),
        _ => bail!("Phasor requires a finite numeric parameter"),
    };
    let v = raw as f32;
    let (_, _, min, max, integral) = CONTROLS[i];
    ensure!(
        v.is_finite() && (min..=max).contains(&v) && (!integral || raw.fract() == 0.),
        "Invalid Phasor parameter {name}"
    );
    Ok(v)
}
fn admissible(p: &[f32; 10]) -> Result<()> {
    ensure!(
        p[9] <= 1.,
        "Phasor stochastic LFO shapes are not implemented"
    );
    ensure!(
        p[1] != 0. || p[2] <= 10.,
        "Phasor unsynced speed exceeds 10 Hz"
    );
    Ok(())
}
fn parameters(node: &ProgramNode) -> Result<[f32; 10]> {
    ensure!(
        node.kind == "Phasor",
        "Unsupported UVI phasor {}",
        node.kind
    );
    let mut p = CONTROLS.map(|c| c.1);
    for (name, raw) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let v = ParameterValue::Number(
            raw.parse()
                .with_context(|| format!("Invalid Phasor parameter {name}"))?,
        );
        p[index(name)?] = checked(name, &v)?;
    }
    admissible(&p)?;
    Ok(p)
}
pub fn supports(kind: &str) -> bool {
    kind == "Phasor"
}
pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

pub struct Phasor {
    channels: usize,
    rate: f32,
    tempo: f32,
    p: [f32; 10],
    current: [f32; 10],
    previous: [f32; 10],
    clock: u8,
    alpha: f32,
    phase: u32,
    step: u32,
    offset: u32,
    states: [[f32; 24]; 2],
    last: [f32; 2],
    sine: [f32; 257],
    ratio: f64,
    mix: f32,
}
impl Phasor {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=2).contains(&channels),
            "Phasor requires a mono or stereo bus"
        );
        ensure!(
            rate.is_finite() && (8000. ..=192000.).contains(&rate),
            "Invalid Phasor sample rate"
        );
        let p = parameters(node)?;
        let mut s = Self {
            channels,
            rate: rate as f32,
            tempo: 120.,
            p,
            current: p,
            previous: p,
            clock: 0,
            alpha: 1. - 0.33f32.powf(3200. / rate as f32),
            phase: 0,
            step: 0,
            offset: 0,
            states: [[0.; 24]; 2],
            last: [0.; 2],
            sine: std::array::from_fn(|i| -(std::f64::consts::TAU * i as f64 / 256.).sin() as f32),
            ratio: 0.,
            mix: 0.,
        };
        s.tune();
        Ok(s)
    }
    fn tune(&mut self) {
        let speed = if self.p[1] == 0. {
            self.current[2]
        } else {
            self.tempo / (60. * self.current[2])
        };
        // Native Phasor's phase clock stays at 44.1 kHz even when its allpass
        // filters use another host rate; paired 8/32/48/96 kHz probes establish it.
        self.step = (f64::from(speed / 44100.) * 4294967296.) as u32;
        self.offset = (f64::from(self.current[7]) * 1073741824.) as u32;
        self.ratio = f64::from(self.current[4]) / f64::from(self.current[3]);
        self.mix = 1. / (1. + self.current[6] * self.current[6]).sqrt();
    }
    fn frequency(&self, phase: u32) -> f32 {
        let u = if self.p[9] == 0. {
            (2. * f64::from(phase.wrapping_add(1073741824)) / 4294967296. - 1.).abs()
        } else {
            let i = (phase >> 24) as usize;
            let t = (phase & 0x00ff_ffff) as f32 / 16777216.;
            let wave = self.sine[i] + (self.sine[i + 1] - self.sine[i]) * t;
            0.5 + 0.5 * f64::from(wave)
        };
        (f64::from(self.current[3]) * self.ratio.powf(u)) as f32
    }
    pub fn output_channels(&self) -> usize {
        self.channels
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        let i = index(name)?;
        Ok(if i <= 1 {
            ParameterValue::Boolean(self.p[i] != 0.)
        } else {
            ParameterValue::Number(f64::from(self.p[i]))
        })
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let i = index(name)?;
        let mut p = self.p;
        p[i] = checked(name, value)?;
        admissible(&p)?;
        self.p = p;
        if i <= 1 || i >= 8 {
            self.tune();
        }
        Ok(())
    }
    pub fn set_tempo(&mut self, tempo: f64) -> Result<()> {
        ensure!(
            tempo.is_finite() && (1. ..=1000.).contains(&tempo),
            "Invalid Phasor tempo"
        );
        self.tempo = tempo as f32;
        self.tune();
        Ok(())
    }
    pub fn clear(&mut self) {
        self.phase = 0;
        self.clock = 0;
        self.current = self.p;
        self.previous = self.p;
        self.tune();
        self.states.fill([0.; 24]);
        self.last.fill(0.);
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|f| f[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite Phasor input"
        );
        if self.p[0] != 0. {
            return Ok(());
        }
        for frame in frames {
            if self.clock == 0 {
                // Native scalar controls hold 32 frames, then apply this RC law.
                // Frequency endpoints smooth in log space; other controls are linear.
                // Using the previous frame's target preserves a setter on the boundary.
                for i in 2..=7 {
                    if self.current[i] == self.previous[i] {
                        continue;
                    }
                    if i == 3 || i == 4 {
                        let value = self.current[i].ln();
                        self.current[i] =
                            (value + self.alpha * (self.previous[i].ln() - value)).exp();
                    } else {
                        self.current[i] += self.alpha * (self.previous[i] - self.current[i]);
                    }
                }
                self.tune();
            }
            for (ch, dry) in frame[..self.channels].iter_mut().enumerate() {
                let phase = self
                    .phase
                    .wrapping_add(if ch == 0 { 0 } else { self.offset });
                let g = self.p[8] * self.frequency(phase) / self.rate;
                let a = (g - 1.) / (g + 1.);
                let mut x = *dry + self.current[5] * self.last[ch];
                for state in &mut self.states[ch][..2 * self.p[8] as usize] {
                    let y = a * x + *state;
                    *state = x - a * y;
                    x = y;
                }
                self.last[ch] = x;
                *dry = (*dry + self.current[6] * x) * self.mix;
                ensure!(dry.is_finite(), "Phasor output overflow");
            }
            self.phase = self.phase.wrapping_add(self.step);
            self.previous = self.p;
            self.clock = (self.clock + 1) & 31;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::dsp::MAX_CHANNELS;
    use super::*;
    fn node(values: &[(&str, &str)]) -> ProgramNode {
        ProgramNode {
            parent: None,
            kind: "Phasor".into(),
            name: None,
            attributes: values
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            text: String::new(),
        }
    }
    #[test]
    fn authored_native_stationary_filters_and_live_depth() {
        // Original native impulse histories; mono pan removed from expected values.
        for (order, feedback, native) in [
            (
                3.,
                0.,
                [
                    0.2601984143257141,
                    -0.12562325596809387,
                    -0.03202161192893982,
                    0.014917660504579544,
                    0.03294673189520836,
                    0.03425484523177147,
                    0.026977399364113808,
                    0.01633840799331665,
                    0.005507950205355883,
                    -0.003760534105822444,
                    -0.010662119835615158,
                    -0.015001287683844566,
                ],
            ),
            (
                1.,
                0.7,
                [
                    0.33941715955734253,
                    0.07762573659420013,
                    0.007646644487977028,
                    -0.04221654310822487,
                    -0.07114078104496002,
                    -0.08140641450881958,
                    -0.07716282457113266,
                    -0.0633060485124588,
                    -0.04458830505609512,
                    -0.025011973455548286,
                    -0.007506232243031263,
                    0.006153646390885115,
                ],
            ),
        ] {
            let mut effect = Phasor::new(
                &node(&[
                    ("MinFreq", "1000"),
                    ("MaxFreq", "1000"),
                    ("Feedback", "0"),
                    ("Order", "1"),
                ]),
                1,
                48000.,
            )
            .unwrap();
            effect
                .set_parameter("Order", &ParameterValue::Number(order))
                .unwrap();
            effect
                .set_parameter("Feedback", &ParameterValue::Number(feedback))
                .unwrap();
            effect.clear();
            let mut frames = [[0.; MAX_CHANNELS]; 12];
            frames[0][0] = 0.25;
            effect.process(&mut frames[..5]).unwrap();
            effect.process(&mut frames[5..]).unwrap();
            for (frame, want) in frames.iter().zip(native) {
                assert!((f64::from(frame[0]) - want).abs() < 2e-7);
            }
            assert!(effect.memory_bytes() < 2048);
        }
        let mut effect = Phasor::new(
            &node(&[
                ("MinFreq", "1000"),
                ("MaxFreq", "1000"),
                ("Feedback", "0"),
                ("Order", "1"),
                ("Depth", "0"),
            ]),
            1,
            48000.,
        )
        .unwrap();
        let mut frame = [[0.; MAX_CHANNELS]; 1];
        for _ in 0..9472 {
            frame[0][0] = 0.125;
            effect.process(&mut frame).unwrap();
        }
        effect
            .set_parameter("Depth", &ParameterValue::Number(1.))
            .unwrap();
        let mut elapsed = 9472;
        for (i, want) in [
            (32, 0.13356709480285645),
            (64, 0.1408531814813614),
            (96, 0.1469803899526596),
            (128, 0.1520906686782837),
        ] {
            // Preserve native 32-frame hold at a setter exactly on the boundary.
            while elapsed < 9472 + i {
                frame[0][0] = 0.125;
                effect.process(&mut frame).unwrap();
                elapsed += 1;
            }
            frame[0][0] = 0.125;
            effect.process(&mut frame).unwrap();
            elapsed += 1;
            assert!((f64::from(frame[0][0]) - want).abs() < 2e-7);
        }
    }
    #[test]
    fn authored_native_sweep_sync_stereo_and_bounds() {
        for (values, elapsed, input, native) in [
            (
                vec![("Speed", "10"), ("LFOSHape", "1"), ("MaxFreq", "20000")],
                4096,
                0.125,
                [
                    0.14456774294376373,
                    -0.05137403681874275,
                    -0.029289478436112404,
                    -0.014073573052883148,
                    -0.0038289001677185297,
                    0.002847696654498577,
                    0.006989790592342615,
                    0.00935515109449625,
                    0.01049603521823883,
                    0.010812186636030674,
                    0.010590544901788235,
                    0.010034962557256222,
                ],
            ),
            (
                vec![("SyncToHost", "1"), ("Speed", "12")],
                8192,
                0.125,
                [
                    0.17208102345466614,
                    -0.009138480760157108,
                    -0.008642978966236115,
                    -0.00816754437983036,
                    -0.007711454760283232,
                    -0.0072740125469863415,
                    -0.006854542531073093,
                    -0.006452395115047693,
                    -0.006066939793527126,
                    -0.005697570275515318,
                    -0.005343698430806398,
                    -0.005004757549613714,
                ],
            ),
        ] {
            let mut n = node(&[("Feedback", "0"), ("Order", "1"), ("Spread", "0")]);
            for (k, v) in values {
                n.attributes.insert(k.into(), v.into());
            }
            let mut effect = Phasor::new(&n, 1, 48000.).unwrap();
            effect
                .process(&mut vec![[0.; MAX_CHANNELS]; elapsed])
                .unwrap();
            let mut frames = [[0.; MAX_CHANNELS]; 12];
            frames[0][0] = input;
            effect.process(&mut frames).unwrap();
            for (f, want) in frames.iter().zip(native) {
                assert!((f64::from(f[0]) - want).abs() < 2e-7);
            }
        }
        // Native bypass freezes the phase clock and filter state, then resumes.
        let mut resumed =
            Phasor::new(&node(&[("Feedback", "0"), ("Order", "1")]), 1, 48000.).unwrap();
        resumed
            .process(&mut vec![[0.; MAX_CHANNELS]; 9472])
            .unwrap();
        resumed
            .set_parameter("Bypass", &ParameterValue::Boolean(true))
            .unwrap();
        resumed
            .process(&mut vec![[0.; MAX_CHANNELS]; 9728])
            .unwrap();
        resumed
            .set_parameter("Bypass", &ParameterValue::Boolean(false))
            .unwrap();
        resumed
            .process(&mut vec![[0.; MAX_CHANNELS]; 1280])
            .unwrap();
        let mut pulse = [[0.; MAX_CHANNELS]; 1];
        pulse[0][0] = 0.125;
        resumed.process(&mut pulse).unwrap();
        assert!((pulse[0][0] - 0.1730194688).abs() < 2e-7);
        let mut effect =
            Phasor::new(&node(&[("Feedback", "0"), ("Order", "1")]), 2, 48000.).unwrap();
        effect.process(&mut vec![[0.; MAX_CHANNELS]; 8192]).unwrap();
        let mut frame = [[0.; MAX_CHANNELS]; 1];
        frame[0][0] = 0.25;
        frame[0][1] = -0.125;
        effect.process(&mut frame).unwrap();
        assert!((frame[0][0] - 0.3453131914).abs() < 2e-7);
        assert!((frame[0][1] + 0.1748068035).abs() < 2e-7);
        assert!(matches!(
            effect.parameter("Bypass").unwrap(),
            ParameterValue::Boolean(false)
        ));
        assert!(
            effect
                .set_parameter("LFOSHape", &ParameterValue::Number(2.))
                .is_err()
        );
        assert!(
            effect
                .set_parameter("Depth", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
        assert!(Phasor::new(&node(&[]), 3, 48000.).is_err());
        assert!(Phasor::new(&node(&[]), 1, 0.).is_err());
        assert!(Phasor::new(&node(&[("Speed", "0.0099999998")]), 1, 48000.).is_ok());
    }
}
