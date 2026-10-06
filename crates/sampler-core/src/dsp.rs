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
    LowShelf { gain_db: f64 },
    HighShelf { gain_db: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    rate: u32,
    b: [f64; 3],
    a: [f64; 2],
}
impl Biquad {
    /// Prepare coefficients off audio. Frequency is strictly between DC and Nyquist;
    /// Q is positive, including for shelves: Q = 1/sqrt(2) gives RBJ shelf slope S=1.
    /// Larger Q permits resonant overshoot. Reject numerically unstable coefficients.
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
            FilterKind::LowShelf { gain_db } | FilterKind::HighShelf { gain_db } => {
                if !gain_db.is_finite() {
                    return Err(Error::InvalidInput);
                }
                let amplitude = 10_f64.powf(gain_db / 40.);
                let beta = 2. * amplitude.sqrt() * alpha;
                let plus = amplitude + 1.;
                let minus = amplitude - 1.;
                if matches!(kind, FilterKind::LowShelf { .. }) {
                    denominator = [
                        plus + minus * cos + beta,
                        -2. * (minus + plus * cos),
                        plus + minus * cos - beta,
                    ];
                    [
                        amplitude * (plus - minus * cos + beta),
                        2. * amplitude * (minus - plus * cos),
                        amplitude * (plus - minus * cos - beta),
                    ]
                } else {
                    denominator = [
                        plus - minus * cos + beta,
                        2. * (minus - plus * cos),
                        plus - minus * cos - beta,
                    ];
                    [
                        amplitude * (plus + minus * cos + beta),
                        -2. * amplitude * (minus + plus * cos),
                        amplitude * (plus + minus * cos - beta),
                    ]
                }
            }
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
pub enum Processor {
    /// Linear amplitude, including polarity inversion. Must be finite.
    Gain(f64),
    /// Rows are output L/R, columns input L/R. Coefficients must be finite.
    /// Source profiles own pan/width laws; this stage does not clamp or normalize.
    StereoMatrix([[f64; 2]; 2]),
    Biquad(Biquad),
    ControlGain(GainControl),
}

impl Processor {
    fn valid(&self) -> bool {
        match self {
            Processor::Gain(gain) => gain.is_finite(),
            Processor::StereoMatrix(matrix) => matrix.iter().flatten().all(|v| v.is_finite()),
            Processor::ControlGain(binding) => binding.valid(),
            Processor::Biquad(_) => true,
        }
    }
}

pub(super) mod control;
pub use control::GainControl;
pub(super) use control::GainRamp;

pub(super) enum PreparedProcessor {
    Gain(f64),
    StereoMatrix([[f64; 2]; 2]),
    Biquad(Biquad),
    ControlGain(usize),
}

pub(super) struct PreparedVoiceChain {
    pre: Box<[PreparedProcessor]>,
    post: Box<[PreparedProcessor]>,
    tail_frames: u32,
}

pub(super) struct RenderContext<'a> {
    pub expression: Frame,
    pub gains: &'a [GainRamp],
    pub at: u64,
}

/// Serial stereo processing with an explicit envelope boundary.
/// Tail frames are an authored maximum after source/envelope completion, not a
/// guessed silence threshold or a claim that an IIR has a mathematically finite tail.
pub struct VoiceChain {
    pre: Box<[Processor]>,
    post: Box<[Processor]>,
    tail_frames: u32,
}
impl VoiceChain {
    pub fn new(
        pre_envelope: Vec<Processor>,
        post_envelope: Vec<Processor>,
        tail_frames: u32,
    ) -> Result<Self, Error> {
        if pre_envelope
            .iter()
            .chain(&post_envelope)
            .any(|stage| !stage.valid())
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
    pub(super) fn compile(
        self,
        rate: u32,
        bindings: &mut Vec<GainControl>,
    ) -> Result<PreparedVoiceChain, Error> {
        Ok(PreparedVoiceChain {
            pre: compile_processors(self.pre, rate, bindings)?,
            post: compile_processors(self.post, rate, bindings)?,
            tail_frames: self.tail_frames,
        })
    }
}
pub(super) fn compile_processors(
    stages: Box<[Processor]>,
    rate: u32,
    bindings: &mut Vec<GainControl>,
) -> Result<Box<[PreparedProcessor]>, Error> {
    if stages.iter().any(|stage| !stage.valid()) {
        return Err(Error::InvalidInput);
    }
    stages
        .into_vec()
        .into_iter()
        .map(|stage| {
            Ok(match stage {
                Processor::Gain(gain) => PreparedProcessor::Gain(gain),
                Processor::StereoMatrix(matrix) => PreparedProcessor::StereoMatrix(matrix),
                Processor::Biquad(filter) => {
                    if filter.rate != rate {
                        return Err(Error::InvalidInput);
                    }
                    PreparedProcessor::Biquad(filter)
                }
                Processor::ControlGain(binding) => {
                    let lane = bindings.len();
                    bindings.push(binding);
                    PreparedProcessor::ControlGain(lane)
                }
            })
        })
        .collect()
}

impl PreparedVoiceChain {
    pub(super) fn stages(&self) -> usize {
        self.pre.len() + self.post.len()
    }

    pub(super) fn render(
        &self,
        voice: &mut Voice,
        pcm: &[Frame],
        output: &mut [Frame],
        states: &mut [FilterState],
        context: RenderContext<'_>,
        kernel: &crate::resample::Kernel,
    ) -> (usize, u64) {
        let mut faults = 0;
        let mut rendered = 0;
        let mut unity = EnvelopeState::new(Envelope::default());
        for (chunk_index, chunk) in output.chunks_mut(64).enumerate() {
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
                let at = context.at + (chunk_index * 64 + index) as u64;
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
                    context.gains,
                    at,
                );
                let level = voice
                    .envelope
                    .constant_level()
                    .unwrap_or_else(|| voice.envelope.next());
                value = value.map(|v| v * f64::from(level));
                value = process(&self.post, post, value, context.gains, at);
                let fade = voice.dsp_fade.map_or(1., |(total, initial)| {
                    f64::from(initial) * f64::from(voice.tail_remaining.unwrap()) / f64::from(total)
                });
                let result = std::array::from_fn::<_, 2, _>(|channel| {
                    (value[channel] * f64::from(context.expression[channel]) * fade) as f32
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
                rendered += 1;
                if let Some(remaining) = &mut voice.tail_remaining {
                    *remaining -= 1;
                }
            }
        }
        (rendered, faults)
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
    pub(super) fn finite(&self) -> bool {
        self.z.iter().flatten().all(|v| v.is_finite())
    }
}
pub(super) fn process(
    stages: &[PreparedProcessor],
    states: &mut [FilterState],
    mut value: [f64; 2],
    gains: &[GainRamp],
    at: u64,
) -> [f64; 2] {
    for (stage, state) in stages.iter().zip(states) {
        match stage {
            PreparedProcessor::Gain(gain) => value = value.map(|v| v * gain),
            PreparedProcessor::StereoMatrix(matrix) => {
                value = matrix.map(|row| row[0] * value[0] + row[1] * value[1]);
            }
            PreparedProcessor::ControlGain(lane) => {
                let gain = gains[*lane].value(at);
                value = value.map(|v| v * gain);
            }
            PreparedProcessor::Biquad(filter) => {
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

pub(super) struct DspState {
    pub stride: usize,
    pub cells: Box<[FilterState]>,
    pub gains: Box<[GainRamp]>,
    pub buses: crate::bus::BusState,
}
impl DspState {
    pub fn new(plan: &Prepared, voices: usize) -> Result<Self, Error> {
        let stride = plan
            .voice_chains
            .iter()
            .map(PreparedVoiceChain::stages)
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
            gains: control::initial_gains(plan, &plan.gain_bindings),
            buses: crate::bus::BusState::new(plan)?,
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
                (
                    FilterKind::LowShelf { gain_db: 12. },
                    10_f64.powf(0.6),
                    10_f64.powf(0.3),
                    1.,
                ),
                (
                    FilterKind::LowShelf { gain_db: -12. },
                    10_f64.powf(-0.6),
                    10_f64.powf(-0.3),
                    1.,
                ),
                (
                    FilterKind::HighShelf { gain_db: 12. },
                    1.,
                    10_f64.powf(0.3),
                    10_f64.powf(0.6),
                ),
                (
                    FilterKind::HighShelf { gain_db: -12. },
                    1.,
                    10_f64.powf(-0.3),
                    10_f64.powf(-0.6),
                ),
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
                    let actual = process(
                        &[PreparedProcessor::Biquad(filter)],
                        &mut state,
                        [input, 0.],
                        &[],
                        0,
                    );
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
            for kind in [
                FilterKind::Peak { gain_db },
                FilterKind::LowShelf { gain_db },
                FilterKind::HighShelf { gain_db },
            ] {
                assert!(Biquad::new(48000, kind, 1000., 1.).is_err());
            }
        }
    }

    #[test]
    fn shelf_boost_cut_pairs_cancel_and_unit_slope_responses_are_monotonic() {
        for rate in [44100, 48000, 96000] {
            for frequency in [40., 1000., f64::from(rate) * 0.49] {
                for q in [0.2, std::f64::consts::FRAC_1_SQRT_2, 4.] {
                    for gain_db in [0., 6., 24.] {
                        for high in [false, true] {
                            let kind = |gain_db| {
                                if high {
                                    FilterKind::HighShelf { gain_db }
                                } else {
                                    FilterKind::LowShelf { gain_db }
                                }
                            };
                            let boost = Biquad::new(rate, kind(gain_db), frequency, q).unwrap();
                            let cut = Biquad::new(rate, kind(-gain_db), frequency, q).unwrap();
                            let mut state = [FilterState::default(); 2];
                            for i in 0..4096 {
                                let input =
                                    [(f64::from(i) * 0.017).sin(), if i == 0 { 1. } else { 0. }];
                                let output = process(
                                    &[
                                        PreparedProcessor::Biquad(boost),
                                        PreparedProcessor::Biquad(cut),
                                    ],
                                    &mut state,
                                    input,
                                    &[],
                                    0,
                                );
                                for (actual, expected) in output.into_iter().zip(input) {
                                    assert!(
                                        (actual - expected).abs() < 2e-9,
                                        "{rate}, {frequency}, {q}, {gain_db}, {high}, {i}: {actual} != {expected}"
                                    );
                                }
                            }
                            if q != std::f64::consts::FRAC_1_SQRT_2 {
                                continue;
                            }
                            let mut previous = if high { 1. } else { 10_f64.powf(gain_db / 20.) };
                            for i in 0..=512 {
                                let omega = std::f64::consts::PI * f64::from(i) / 512.;
                                let (s, c) = omega.sin_cos();
                                let (s2, c2) = (2. * omega).sin_cos();
                                let magnitude = (boost.b[0] + boost.b[1] * c + boost.b[2] * c2)
                                    .hypot(boost.b[1] * s + boost.b[2] * s2)
                                    / (1. + boost.a[0] * c + boost.a[1] * c2)
                                        .hypot(boost.a[0] * s + boost.a[1] * s2);
                                let delta = if high {
                                    magnitude - previous
                                } else {
                                    previous - magnitude
                                };
                                assert!(delta >= -2e-8, "nonmonotonic unit-slope shelf: {delta}");
                                previous = magnitude;
                            }
                        }
                    }
                }
            }
        }
    }
}
