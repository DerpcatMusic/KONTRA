//! Original Butterworth/bilinear cascade for the measured Brickwall IIR boundary.
//! The full callback's IPP copy and SoftBypass mixing remain outside this helper.
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "BrickwallFilter IIR cascade only, limited to six measured rates and cutoff <= min(20000 Hz, 0.45*rate). 576 scalar, 432 helper lifecycle, 324 actual-attribute and 48 signal-clone authored comparisons are float32-identical. Original signal constructor, preparation, cascade and clear execute with caller-owned allocation services; full callback reaches unavailable IPP copy. Copy, SoftBypass, hosted/connected controls, actual voice-cache lifecycle and whole-program audio remain unverified; Program playback does not admit this leaf";

#[derive(Clone, Copy, Default)]
struct History {
    first: f32,
    second: f32,
}

#[derive(Clone)]
pub struct BrickwallCascade {
    channels: usize,
    rate: f64,
    frequency: f64,
    slope: u8,
    high_pass: bool,
    dirty: bool,
    stages: usize,
    coefficients: [[f32; 5]; 8],
    history: [[History; 8]; MAX_CHANNELS],
}

impl BrickwallCascade {
    pub fn new(channels: usize, rate: f64) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Brickwall needs 1..12 channels"
        );
        ensure!(
            [8_000., 32_000., 44_100., 48_000., 96_000., 192_000.].contains(&rate),
            "Brickwall cascade rate has no authored native comparison"
        );
        Ok(Self {
            channels,
            rate,
            frequency: 1_000.,
            slope: 2,
            high_pass: false,
            dirty: true,
            stages: 8,
            coefficients: [[0.; 5]; 8],
            history: [[History::default(); 8]; MAX_CHANNELS],
        })
    }

    /// Direct physical controls; prepare on the next active process call.
    /// SoftBypass and hosted property/control clocks are outside this boundary.
    pub fn configure(&mut self, frequency: f64, slope: u8, high_pass: bool) -> Result<()> {
        ensure!(
            frequency.is_finite()
                && (20.0..=20_000.0).contains(&frequency)
                && frequency <= self.rate * 0.45,
            "Brickwall frequency is outside the measured safe range"
        );
        ensure!(slope <= 2, "Invalid Brickwall Slope");
        self.frequency = frequency;
        self.slope = slope;
        self.high_pass = high_pass;
        self.dirty = true;
        Ok(())
    }

    fn prepare(&mut self) {
        if !self.dirty {
            return;
        }
        let order = [4, 8, 16][usize::from(self.slope)];
        let stages = order / 2;
        if stages > self.stages {
            for channel in &mut self.history {
                channel[self.stages..stages].fill(History::default());
            }
        }
        // Preserve the native float32 reciprocal/product before double math.
        let fraction = f64::from((1f32 / self.rate as f32) * self.frequency as f32);
        let warp = 1. / (fraction * std::f64::consts::TAU * 0.5).tan();
        let square = warp * warp;
        for (stage, coefficients) in self.coefficients[..stages].iter_mut().enumerate() {
            let pole = ((2 * (stage + 1) + order) as f64 - 1.)
                * (std::f64::consts::FRAC_PI_2 / order as f64);
            let damping = pole.cos() * -2.;
            let normalization = 1. / (warp * damping + square + 1.);
            let numerator = if self.high_pass { square } else { 1. };
            let middle = if self.high_pass { -2. * square } else { 2. };
            *coefficients = [
                numerator * normalization,
                middle * normalization,
                numerator * normalization,
                ((1. - square) + (1. - square)) * normalization,
                ((square - warp * damping) + 1.) * normalization,
            ]
            .map(|value| value as f32);
        }
        self.stages = stages;
        self.dirty = false;
    }

    pub fn reset(&mut self) {
        self.history.fill([History::default(); 8]);
    }

    pub fn process(&mut self, frames: &mut [Frame], bypass: bool) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|frame| frame[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite Brickwall input"
        );
        if bypass {
            return Ok(());
        }
        self.prepare();
        for channel in 0..self.channels {
            for stage in 0..self.stages {
                let [b0, b1, b2, a1, a2] = self.coefficients[stage];
                let state = &mut self.history[channel][stage];
                for frame in frames.iter_mut() {
                    let input = frame[channel];
                    let output = input * b0 + state.first;
                    state.first = (input * b1 + state.second) - output * a1;
                    state.second = input * b2 - output * a2;
                    frame[channel] = output;
                    ensure!(output.is_finite(), "Nonfinite Brickwall output");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_native_lowpass_split_pending_bypass_and_clear() {
        let mut filter = BrickwallCascade::new(2, 48_000.).unwrap();
        filter.configure(1_000., 0, false).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 16];
        frames[0][0] = 1.;
        filter.process(&mut frames[..7], false).unwrap();
        filter.configure(2_000., 2, true).unwrap();
        let mut dry = [[0.25; MAX_CHANNELS]; 17];
        let original = dry;
        filter.process(&mut dry, true).unwrap();
        assert_eq!(dry, original);
        filter.configure(1_000., 0, false).unwrap();
        filter.process(&mut frames[7..], false).unwrap();
        let expected = [
            931296530, 955892530, 971788047, 983040325, 991543979, 998558772, 1003332761,
            1007882932, 1011226310, 1014923598, 1016947325, 1018994244, 1021058931, 1023088598,
            1024221962, 1025129825,
        ];
        for (frame, bits) in frames.iter().zip(expected) {
            assert_eq!(frame[0].to_bits(), bits);
            assert_eq!(frame[1], 0.);
        }
        filter.reset();
        let mut reset = [[0.; MAX_CHANNELS]; 16];
        reset[0][0] = 1.;
        filter.process(&mut reset, false).unwrap();
        assert_eq!(reset, frames);
    }

    #[test]
    fn authored_native_highpass_sixteen_order_impulse() {
        let mut filter = BrickwallCascade::new(1, 48_000.).unwrap();
        filter.configure(7_603.7886, 2, true).unwrap();
        let mut frames = [[0.; MAX_CHANNELS]; 16];
        frames[0][0] = 1.;
        filter.process(&mut frames, false).unwrap();
        let expected = [
            999790940, 3175105316, 1045127208, 3202246025, 1055636572, 3179697005, 3197172134,
            1031039442, 1046465808, 1018271972, 3190161948, 3184650622, 1032771517, 1040577086,
            1024821808, 3181811788,
        ];
        for (frame, bits) in frames.iter().zip(expected) {
            assert_eq!(frame[0].to_bits(), bits);
        }
    }

    #[test]
    fn unmeasured_controls_are_rejected() {
        assert!(BrickwallCascade::new(13, 48_000.).is_err());
        assert!(BrickwallCascade::new(1, 22_050.).is_err());
        let mut filter = BrickwallCascade::new(1, 8_000.).unwrap();
        assert!(filter.configure(5_000., 2, false).is_err());
        assert!(filter.configure(1_000., 3, false).is_err());
        assert!(filter.configure(f64::NAN, 0, false).is_err());
    }
}
