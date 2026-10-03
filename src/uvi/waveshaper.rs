//! Original WaveShaper equations measured against official UVI Workstation 4.0.9
//! using authored PCM16 steps/impulses at 48 kHz and positive cross-rate probes
//! (256-frame blocks, 2026-10-03).
//! Public control facts: https://lua.uvi.net/_elements.html#WaveShaper and
//! https://s3.amazonaws.com/uvi/UVIFC/falcon_manual.pdf. No vendor source copied.
//! Unsupported modes and oversampling reject rather than substituting another DSP.

use super::{
    dsp::Frame,
    filter::{allpass, allpass_coefficients},
    host::ParameterValue,
    program::ProgramNode,
};
use anyhow::{Context, Result, bail, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "UVI WaveShaper uses an ideal triangle instead of native lookup corner smoothing; high-drive float rounding, control transitions and remaining curve/rate combinations remain native-unverified";
// Published name,default,min,max,integer.
const CONTROLS: [(&str, f32, f64, f64, bool); 10] = [
    ("Bypass", 0., 0., 1., true),
    ("InputGain", 0., -40., 40., false),
    ("OutputGain", 0., -40., 40., false),
    ("Amount", 0., 0., 1., false),
    ("Knee", 0., -10., 10., false),
    ("Mix", 1., 0., 1., false),
    ("PreFreq", 20000., 20., 22000., false),
    ("PostFreq", 20., 2., 20000., false),
    ("Oversampling", 0., 0., 4., true),
    ("Mode", 0., 0., 11., true),
];
fn index(name: &str) -> Result<usize> {
    CONTROLS
        .iter()
        .position(|p| p.0 == name)
        .with_context(|| format!("Unsupported WaveShaper parameter {name}"))
}
fn checked(name: &str, value: &ParameterValue) -> Result<f32> {
    let i = index(name)?;
    let v = match value {
        ParameterValue::Number(v) if v.is_finite() => *v,
        ParameterValue::Boolean(v) => f64::from(u8::from(*v)),
        _ => bail!("WaveShaper needs a finite numeric parameter"),
    };
    let (_, _, min, max, integer) = CONTROLS[i];
    ensure!(
        (min..=max).contains(&v) && (!integer || v.fract() == 0.),
        "Invalid WaveShaper parameter {name}"
    );
    Ok(v as f32)
}
fn admissible(p: &[f32; 10]) -> Result<()> {
    ensure!(
        p[8] <= 1.,
        "WaveShaper oversampling above 2x is not implemented"
    );
    ensure!(
        matches!(p[9] as u8, 0 | 1 | 2 | 3 | 5 | 6 | 7 | 8 | 9 | 10 | 11),
        "WaveShaper mode is not implemented"
    );
    Ok(())
}
fn parameters(node: &ProgramNode) -> Result<[f32; 10]> {
    ensure!(
        node.kind == "WaveShaper",
        "Unsupported UVI waveshaper {}",
        node.kind
    );
    let mut p = CONTROLS.map(|c| c.1);
    for (name, raw) in &node.attributes {
        if name == "Name" {
            continue;
        }
        let v = ParameterValue::Number(
            raw.parse()
                .with_context(|| format!("Invalid WaveShaper parameter {name}"))?,
        );
        p[index(name)?] = checked(name, &v)?;
    }
    admissible(&p)?;
    Ok(p)
}
pub fn supports(kind: &str) -> bool {
    kind == "WaveShaper"
}
pub fn validate(node: &ProgramNode) -> Result<()> {
    parameters(node).map(|_| ())
}

fn shape(x: f32, mode: u8, amount: f32, knee: f32) -> f32 {
    let drive = (8. * amount).exp2();
    let y = f64::from(x) * f64::from(drive);
    match mode {
        0 => (y * std::f64::consts::FRAC_PI_2).sin() as f32,
        1 => (1. - ((y + 1.).rem_euclid(4.) - 2.).abs()) as f32,
        2 | 3 => {
            let p = 10f64.powf(f64::from(knee) / 10.);
            let a = y.abs().powf(p);
            let n = (2. * f64::from(drive)).powf(p);
            let (a, n) = if mode == 2 {
                (a.tanh(), n.tanh())
            } else {
                (a.atan(), n.atan())
            };
            ((a / n).powf(1. / p).copysign(f64::from(x))) as f32
        }
        5 => ((f64::from(x) * 2f64.powf(2. * f64::from(amount))).sinh()
            * std::f64::consts::FRAC_PI_2)
            .sin() as f32,
        6 => x.abs(),
        7 => x.max(0.),
        8 => {
            let z = f64::from(x)
                .abs()
                .powf(0.5 * 10f64.powf(f64::from(knee) / 10.));
            (z * (1. - z).exp()).copysign(f64::from(x)) as f32
        }
        10 => {
            let p = 10f64.powf(f64::from(knee) / 10.);
            let k = 0.1 * f64::from(drive);
            (4. * ((k * p * f64::from(x)).exp_m1() / (4. * k * p.sinh())).tanh()) as f32
        }
        11 => (2. * (f64::from(x).tanh().cosh() - 1.)) as f32,
        9 => {
            if x < 0. {
                x
            } else {
                (y.tanh() / f64::from(drive)) as f32
            }
        }
        _ => unreachable!(),
    }
}

pub struct WaveShaper {
    channels: usize,
    rate: f32,
    p: [f32; 10],
    low: [f32; 2],
    high: [f32; 2],
    input: f32,
    output: f32,
    pre: f32,
    post: f32,
    coefficients: [f64; 8],
    sections: usize,
    up: [[f64; 8]; 2],
    down: [[f64; 8]; 2],
}
impl WaveShaper {
    pub fn new(node: &ProgramNode, channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=2).contains(&channels),
            "WaveShaper requires a mono or stereo bus"
        );
        ensure!(
            rate.is_finite() && (8000. ..=192000.).contains(&rate),
            "Invalid WaveShaper sample rate"
        );
        // Native section count changes at 44066 -> 44067 Hz: 78 dB, not Xpander's 80.
        let (coefficients, sections) = allpass_coefficients(rate, 78.);
        let mut s = Self {
            channels,
            rate: rate as f32,
            p: parameters(node)?,
            low: [0.; 2],
            high: [0.; 2],
            input: 0.,
            output: 0.,
            pre: 0.,
            post: 0.,
            coefficients,
            sections,
            up: [[0.; 8]; 2],
            down: [[0.; 8]; 2],
        };
        s.tune();
        Ok(s)
    }
    fn tune(&mut self) {
        self.input = 10f32.powf(self.p[1] / 20.);
        self.output = 10f32.powf(self.p[2] / 20.);
        let rate = self.rate * if self.p[8] == 0. { 1. } else { 2. };
        self.pre = 1. - (-std::f32::consts::TAU * self.p[6] / rate).exp();
        self.post = 1. - (-std::f32::consts::TAU * self.p[7] / rate).exp();
    }
    pub fn output_channels(&self) -> usize {
        self.channels
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    pub fn parameter(&self, name: &str) -> Result<ParameterValue> {
        Ok(ParameterValue::Number(f64::from(self.p[index(name)?])))
    }
    pub fn set_parameter(&mut self, name: &str, value: &ParameterValue) -> Result<()> {
        let i = index(name)?;
        let mut p = self.p;
        p[i] = checked(name, value)?;
        admissible(&p)?;
        self.p = p;
        self.tune();
        Ok(())
    }
    pub fn clear(&mut self) {
        self.low.fill(0.);
        self.high.fill(0.);
        self.up.fill([0.; 8]);
        self.down.fill([0.; 8]);
    }
    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|f| f[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite WaveShaper input"
        );
        if self.p[0] != 0. {
            return Ok(());
        }
        for frame in frames {
            for (c, x) in frame[..self.channels].iter_mut().enumerate() {
                let dry = *x;
                let input = dry * self.input;
                ensure!(input.is_finite(), "WaveShaper input overflow");
                let mut wet = 0.;
                let oversampled = self.p[8] != 0.;
                for phase in 0..if oversampled { 2 } else { 1 } {
                    let input = if oversampled {
                        allpass(
                            f64::from(input),
                            &mut self.up[c],
                            phase,
                            &self.coefficients[..self.sections],
                        ) as f32
                    } else {
                        input
                    };
                    self.low[c] += self.pre * (input - self.low[c]);
                    let y = shape(self.low[c], self.p[9] as u8, self.p[3], self.p[4]);
                    ensure!(y.is_finite(), "WaveShaper curve overflow");
                    self.high[c] += self.post * (y - self.high[c]);
                    let y = y - self.high[c];
                    wet += if oversampled {
                        (allpass(
                            f64::from(y),
                            &mut self.down[c],
                            1 - phase,
                            &self.coefficients[..self.sections],
                        ) * 0.5) as f32
                    } else {
                        y
                    };
                }
                wet *= self.output;
                *x = dry * (1. - self.p[5]) + wet * self.p[5];
                ensure!(x.is_finite(), "WaveShaper output overflow");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::dsp::MAX_CHANNELS;
    use super::*;
    #[test]
    fn authored_native_wave_shaper_curve_and_filter() {
        // Original stationary native transfer points, not preset/sample data.
        for (mode, amount, knee, x, want, tolerance) in [
            (0, 0., 0., 0.25, 0.382683432, 1e-7),
            (1, 0.5, 0., 0.0625, 1., 1e-7),
            (2, 0., 0., 0.25, 0.254057733892, 1e-6),
            (2, 0.5, 0., 0.0625, 0.761594156, 1e-7),
            (3, 0., 0., 0.25, 0.221269879276, 1e-7),
            (3, 0.2, 2., 0.0625, 0.1457213, 1e-5),
            (5, 0., 0., 0.5, 0.730145312, 1e-7),
            (6, 1., -10., -0.25, 0.25, 0.),
            (7, 1., 10., -0.25, 0., 0.),
            (9, 0.5, 0., 0.0625, 0.047599634, 1e-7),
            (8, 1., 0., 0.25, 0.82436063535, 1e-7),
            (8, 0., 10., 0.5, 0.08233133597, 1e-5),
            (10, 0.5, 0., 0.5, 0.64606393, 1e-6),
            (11, 1., -8., 0.5, 0.21737983, 1e-7),
        ] {
            assert!(
                (f64::from(shape(x, mode, amount, knee)) - want).abs() <= tolerance,
                "mode{mode}"
            );
        }
    }
    #[test]
    fn authored_native_impulse_dry_and_validation() {
        let node = ProgramNode {
            parent: None,
            kind: "WaveShaper".into(),
            name: None,
            attributes: [
                ("PreFreq".into(), "22000".into()),
                ("PostFreq".into(), "2".into()),
            ]
            .into(),
            text: String::new(),
        };
        let mut effect = WaveShaper::new(&node, 1, 48000.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 4];
        frames[0][0] = 0.25;
        effect.process(&mut frames).unwrap();
        for (frame, native_left) in frames.iter().zip([
            0.1810634881258011,
            0.010354464873671532,
            0.0005339590716175735,
            -0.000017456488421885297,
        ]) {
            assert!((f64::from(frame[0]) * 0.5 - native_left).abs() < 2e-7);
        }
        effect
            .set_parameter("Mix", &ParameterValue::Number(0.))
            .unwrap();
        effect
            .set_parameter("InputGain", &ParameterValue::Number(6.))
            .unwrap();
        let mut dry = [[0.25; MAX_CHANNELS]; 1];
        effect.process(&mut dry).unwrap();
        assert_eq!(dry[0][0], 0.25);
        assert!(
            effect
                .set_parameter("Oversampling", &ParameterValue::Number(2.))
                .is_err()
        );
        assert!(
            effect
                .set_parameter("Mode", &ParameterValue::Number(4.))
                .is_err()
        );
        assert!(
            effect
                .set_parameter("Amount", &ParameterValue::Number(f64::NAN))
                .is_err()
        );
        assert!(WaveShaper::new(&node, 3, 48000.).is_err());
        assert!(WaveShaper::new(&node, 1, 0.).is_err());
    }
    #[test]
    fn authored_native_twofold_oversampling_history() {
        let node = ProgramNode {
            parent: None,
            kind: "WaveShaper".into(),
            name: None,
            attributes: [
                ("PreFreq".into(), "22000".into()),
                ("PostFreq".into(), "2".into()),
                ("Oversampling".into(), "1".into()),
            ]
            .into(),
            text: String::new(),
        };
        let mut effect = WaveShaper::new(&node, 1, 48000.).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 12];
        frames[0][0] = 0.25;
        effect.process(&mut frames[..5]).unwrap();
        effect.process(&mut frames[5..]).unwrap();
        for (frame, native_left) in frames.iter().zip([
            0.00011817346967291087,
            0.004105924628674984,
            0.03593229502439499,
            0.10866370797157288,
            0.08956358581781387,
            -0.05086439847946167,
            -0.008961625397205353,
            0.03720388561487198,
            -0.039785318076610565,
            0.029928723350167274,
            -0.01716247946023941,
            0.005438457243144512,
        ]) {
            assert!((f64::from(frame[0]) * 0.5 - native_left).abs() < 1e-7);
        }
        assert!(effect.memory_bytes() < 4096);
        // Original paired native pulse vectors straddle the 78 dB section-count
        // boundary by 1 Hz. Xpander's separate 80 dB threshold needs eight here.
        for (rate, first, native) in [
            (
                44066.,
                0.08733749389648438,
                [
                    0.000023677037461311556,
                    0.0010654672514647245,
                    0.014186117798089981,
                    0.08397278189659119,
                    0.25524020195007324,
                    0.42163920402526855,
                    0.4105815291404724,
                    0.35077226161956787,
                    0.3940199017524719,
                    0.38692227005958557,
                    0.36938875913619995,
                    0.39472639560699463,
                ],
            ),
            (
                44067.,
                0.04086494445800781,
                [
                    0.0001398046442773193,
                    0.004275340586900711,
                    0.04268199950456619,
                    0.18946129083633423,
                    0.4030710458755493,
                    0.4294762909412384,
                    0.34047332406044006,
                    0.3990269899368286,
                    0.38573381304740906,
                    0.3667721152305603,
                    0.4009332060813904,
                    0.36346423625946045,
                ],
            ),
        ] {
            let mut effect = WaveShaper::new(&node, 1, rate).unwrap();
            let mut frames = [[0.; MAX_CHANNELS]; 12];
            for frame in &mut frames {
                frame[0] = 0.25;
            }
            frames[0][0] = first;
            effect.process(&mut frames).unwrap();
            for (frame, want) in frames.iter().zip(native) {
                assert!((f64::from(frame[0]) - want).abs() < 2e-7, "rate {rate}");
            }
        }
    }
}
