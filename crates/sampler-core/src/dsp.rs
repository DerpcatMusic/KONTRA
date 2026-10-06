//! Prepared voice-local processing. No vendor objects or mutable shared filter state.
use crate::{Envelope, EnvelopeState, Error, Frame, Prepared, Voice};

/// Native RBJ biquad responses. Band-pass has unity peak gain.
#[derive(Clone, Copy, Debug)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    AllPass,
    Peak { gain_db: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    rate: u32,
    b: [f64; 3],
    a: [f64; 2],
}
impl Biquad {
    /// Prepare coefficients off audio. Frequency is strictly between DC and Nyquist;
    /// Q is positive. Reject numerically unstable rounded coefficients.
    pub fn new(rate: u32, kind: FilterKind, frequency_hz: f64, q: f64) -> Result<Self, Error> {
        if rate == 0
            || !frequency_hz.is_finite()
            || frequency_hz <= 0.
            || frequency_hz >= f64::from(rate) * 0.5
            || !q.is_finite()
            || q <= 0.
        {
            return Err(Error::InvalidInput);
        }
        let omega = std::f64::consts::TAU * (frequency_hz / f64::from(rate));
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2. * q);
        let mut denominator = [1. + alpha, -2. * cos, 1. - alpha];
        let numerator = match kind {
            FilterKind::LowPass => {
                // Half-angle form avoids subtractive cancellation near DC.
                let half = (omega * 0.5).sin().powi(2);
                [half, 2. * half, half]
            }
            FilterKind::HighPass => {
                let half = (omega * 0.5).cos().powi(2);
                [half, -2. * half, half]
            }
            FilterKind::BandPass => [alpha, 0., -alpha],
            FilterKind::Notch => [1., -2. * cos, 1.],
            FilterKind::AllPass => [1. - alpha, -2. * cos, 1. + alpha],
            FilterKind::Peak { gain_db } => {
                if !gain_db.is_finite() {
                    return Err(Error::InvalidInput);
                }
                let amplitude = 10_f64.powf(gain_db / 40.);
                denominator = [1. + alpha / amplitude, -2. * cos, 1. - alpha / amplitude];
                [1. + alpha * amplitude, -2. * cos, 1. - alpha * amplitude]
            }
        };
        let b = numerator.map(|value| value / denominator[0]);
        let a = [
            denominator[1] / denominator[0],
            denominator[2] / denominator[0],
        ];
        if !b.iter().chain(&a).all(|v| v.is_finite())
            || a[1].abs() >= 1.
            || 1. + a[0] + a[1] <= 0.
            || 1. - a[0] + a[1] <= 0.
        {
            return Err(Error::InvalidInput);
        }
        Ok(Self { rate, b, a })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum VoiceProcessor {
    /// Linear amplitude, including polarity inversion. Must be finite.
    Gain(f64),
    Biquad(Biquad),
}

/// Serial, stereo-independent processing with an explicit envelope boundary.
/// Tail frames are an authored maximum after source/envelope completion, not a
/// guessed silence threshold or a claim that an IIR has a mathematically finite tail.
pub struct VoiceChain {
    pre: Box<[VoiceProcessor]>,
    post: Box<[VoiceProcessor]>,
    tail_frames: u32,
}
impl VoiceChain {
    pub fn new(
        pre_envelope: Vec<VoiceProcessor>,
        post_envelope: Vec<VoiceProcessor>,
        tail_frames: u32,
    ) -> Result<Self, Error> {
        if pre_envelope
            .iter()
            .chain(&post_envelope)
            .any(|stage| matches!(stage, VoiceProcessor::Gain(gain) if !gain.is_finite()))
        {
            return Err(Error::InvalidInput);
        }
        pre_envelope
            .len()
            .checked_add(post_envelope.len())
            .ok_or(Error::Capacity)?;
        Ok(Self {
            pre: pre_envelope.into_boxed_slice(),
            post: post_envelope.into_boxed_slice(),
            tail_frames,
        })
    }
    pub(super) fn valid_for(&self, rate: u32) -> bool {
        self.pre
            .iter()
            .chain(&self.post)
            .all(|stage| !matches!(stage, VoiceProcessor::Biquad(filter) if filter.rate != rate))
    }
    pub(super) fn stages(&self) -> usize {
        self.pre.len() + self.post.len()
    }

    pub(super) fn render(
        &self,
        voice: &mut Voice,
        pcm: &[Frame],
        output: &mut [Frame],
        states: &mut [FilterState],
        gains: Frame,
        kernel: &crate::resample::Kernel,
    ) -> u64 {
        let mut faults = 0;
        let mut unity = EnvelopeState::new(Envelope::default());
        for chunk in output.chunks_mut(64) {
            if self.done(voice) {
                break;
            }
            let mut raw = [[0.; 2]; 64];
            let count = chunk
                .len()
                .min(voice.envelope.remaining())
                .min(voice.tail_remaining.map_or(usize::MAX, |n| n as usize));
            let produced = if voice.cursor.done() || voice.envelope.done() {
                0
            } else {
                voice.cursor.render(
                    pcm,
                    &mut raw[..count],
                    &mut unity,
                    voice.gain,
                    [1.; 2],
                    kernel,
                )
            };
            for (index, frame) in chunk.iter_mut().enumerate() {
                let ended = index >= produced || voice.envelope.done();
                if ended && voice.tail_remaining.is_none() {
                    voice.tail_remaining = Some(self.tail_frames);
                }
                if voice.tail_remaining == Some(0) {
                    break;
                }
                let (pre, post) = states.split_at_mut(self.pre.len());
                let mut value = process(
                    &self.pre,
                    pre,
                    if ended {
                        [0.; 2]
                    } else {
                        raw[index].map(f64::from)
                    },
                );
                let level = voice
                    .envelope
                    .constant_level()
                    .unwrap_or_else(|| voice.envelope.next());
                value = value.map(|v| v * f64::from(level));
                value = process(&self.post, post, value);
                let fade = voice.dsp_fade.map_or(1., |(total, initial)| {
                    f64::from(initial) * f64::from(voice.tail_remaining.unwrap()) / f64::from(total)
                });
                let result = std::array::from_fn::<_, 2, _>(|channel| {
                    (value[channel] * f64::from(gains[channel]) * fade) as f32
                });
                if result.iter().all(|v| v.is_finite()) && states.iter().all(FilterState::finite) {
                    for channel in 0..2 {
                        frame[channel] += if result[channel].is_subnormal() {
                            0.
                        } else {
                            result[channel]
                        };
                    }
                } else {
                    states.fill(FilterState::default());
                    faults += 1;
                }
                if let Some(remaining) = &mut voice.tail_remaining {
                    *remaining -= 1;
                }
            }
        }
        faults
    }

    pub(super) fn done(&self, voice: &Voice) -> bool {
        voice.tail_remaining == Some(0)
            || (self.tail_frames == 0 && (voice.cursor.done() || voice.envelope.done()))
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct FilterState {
    z: [[f64; 2]; 2],
}
impl FilterState {
    fn finite(&self) -> bool {
        self.z.iter().flatten().all(|v| v.is_finite())
    }
}
fn process(stages: &[VoiceProcessor], states: &mut [FilterState], mut value: [f64; 2]) -> [f64; 2] {
    for (stage, state) in stages.iter().zip(states) {
        match stage {
            VoiceProcessor::Gain(gain) => value = value.map(|v| v * gain),
            VoiceProcessor::Biquad(filter) => {
                for (sample, z) in value.iter_mut().zip(&mut state.z) {
                    let output = filter.b[0] * *sample + z[0];
                    z[0] = filter.b[1] * *sample - filter.a[0] * output + z[1];
                    z[1] = filter.b[2] * *sample - filter.a[1] * output;
                    for cell in z {
                        if cell.is_subnormal() {
                            *cell = 0.;
                        }
                    }
                    *sample = output;
                }
            }
        }
    }
    value
}

pub(super) struct VoiceDspState {
    pub stride: usize,
    pub cells: Box<[FilterState]>,
}
impl VoiceDspState {
    pub fn new(plan: &Prepared, voices: usize) -> Result<Self, Error> {
        let stride = plan
            .voice_chains
            .iter()
            .map(VoiceChain::stages)
            .max()
            .unwrap_or(0);
        let cells = stride.checked_mul(voices).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<FilterState>(cells).map_err(|_| Error::Capacity)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(cells)
            .map_err(|_| Error::Capacity)?;
        values.resize(cells, FilterState::default());
        Ok(Self {
            stride,
            cells: values.into_boxed_slice(),
        })
    }
    pub fn reset(&mut self, voice: usize) {
        self.cells[voice * self.stride..(voice + 1) * self.stride].fill(FilterState::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biquad_impulses_match_independent_difference_equations_and_named_responses() {
        for rate in [44100, 48000, 96000] {
            let frequency = f64::from(rate) / 8.;
            for (kind, dc, center, nyquist) in [
                (FilterKind::LowPass, 1., 0.7, 0.),
                (FilterKind::HighPass, 0., 0.7, 1.),
                (FilterKind::BandPass, 0., 1., 0.),
                (FilterKind::Notch, 1., 0., 1.),
                (FilterKind::AllPass, 1., 1., 1.),
                (FilterKind::Peak { gain_db: 12. }, 1., 10_f64.powf(0.6), 1.),
                (
                    FilterKind::Peak { gain_db: -12. },
                    1.,
                    10_f64.powf(-0.6),
                    1.,
                ),
            ] {
                let filter = Biquad::new(rate, kind, frequency, 0.7).unwrap();
                let mut state = [FilterState::default()];
                let (mut x, mut y) = ([0.; 2], [0.; 2]);
                let mut response = [[0.; 2]; 3];
                for sample in 0..4096 {
                    let input = if sample == 0 { 1. } else { 0. };
                    let expected = filter.b[0] * input + filter.b[1] * x[0] + filter.b[2] * x[1]
                        - filter.a[0] * y[0]
                        - filter.a[1] * y[1];
                    x = [input, x[0]];
                    y = [expected, y[0]];
                    let actual =
                        process(&[VoiceProcessor::Biquad(filter)], &mut state, [input, 0.]);
                    assert!((actual[0] - expected).abs() < 1e-13);
                    assert_eq!(actual[1], 0., "stereo channels must not share state");
                    for (index, omega) in [0., std::f64::consts::TAU / 8., std::f64::consts::PI]
                        .into_iter()
                        .enumerate()
                    {
                        let (sin, cos) = (omega * f64::from(sample)).sin_cos();
                        response[index][0] += actual[0] * cos;
                        response[index][1] -= actual[0] * sin;
                    }
                }
                for (actual, expected) in response.into_iter().zip([dc, center, nyquist]) {
                    assert!(
                        (actual[0].hypot(actual[1]) - expected).abs() < 1e-12,
                        "{kind:?}, {rate}, {actual:?}, expected {expected}"
                    );
                }
            }
        }
        for (rate, hz, q) in [
            (0, 1000., 1.),
            (48000, 0., 1.),
            (48000, 24000., 1.),
            (48000, f64::NAN, 1.),
            (48000, 1000., 0.),
            (48000, 1000., f64::INFINITY),
            (48000, 1000., f64::MAX),
            (48000, f64::MIN_POSITIVE, 1.),
        ] {
            assert!(Biquad::new(rate, FilterKind::LowPass, hz, q).is_err());
        }
        for gain_db in [f64::NAN, f64::INFINITY, f64::MAX, -f64::MAX] {
            assert!(Biquad::new(48000, FilterKind::Peak { gain_db }, 1000., 1.).is_err());
        }
    }
}
