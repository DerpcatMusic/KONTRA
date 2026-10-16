//! Prepared voice-local processing. No vendor objects or mutable shared filter state.
use crate::{Envelope, EnvelopeState, Error, Frame, Prepared, Voice};
use sampler_pool::Slab;

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
    /// First-order (6 dB/octave) shelves; the Q is ignored.
    FirstOrderLowShelf { gain_db: f64 },
    FirstOrderHighShelf { gain_db: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    rate: u32,
    b: [f64; 3],
    a: [f64; 2],
}
impl Biquad {
    // Keep the operation order identical in scalar and batched render paths.
    #[inline(always)]
    fn sample(&self, input: f64, [z0, z1]: [f64; 2]) -> (f64, [f64; 2]) {
        let ([b0, b1, b2], [a1, a2]) = (self.b, self.a);
        let y = b0 * input + z0;
        (y, [b1 * input - a1 * y + z1, b2 * input - a2 * y])
    }

    pub(crate) fn trace_coefficients(&self) -> [f64; 5] {
        [self.b[0], self.b[1], self.b[2], self.a[0], self.a[1]]
    }
    /// Static response from the coefficients playback consumes. Port from v1
    /// filter::magnitude, for the sound editor; no second filter kernel.
    pub fn magnitude(self, hz: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / f64::from(self.rate);
        let norm = |c: [f64; 3]| {
            let re = c[0] + c[1] * w.cos() + c[2] * (2. * w).cos();
            let im = -c[1] * w.sin() - c[2] * (2. * w).sin();
            re * re + im * im
        };
        (norm(self.b) / norm([1., self.a[0], self.a[1]])).sqrt()
    }
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
            FilterKind::FirstOrderLowShelf { gain_db } | FilterKind::FirstOrderHighShelf { gain_db } => {
                if !gain_db.is_finite() {
                    return Err(Error::InvalidInput);
                }
                let amplitude = 10_f64.powf(gain_db / 40.);
                let g = (omega * 0.5).tan();
                if matches!(kind, FilterKind::FirstOrderLowShelf { .. }) {
                    denominator = [1. + g / amplitude, g / amplitude - 1., 0.];
                    [1. + amplitude * g, amplitude * g - 1., 0.]
                } else {
                    denominator = [1. + amplitude * g, amplitude * g - 1., 0.];
                    [
                        amplitude * (amplitude + g),
                        amplitude * (g - amplitude),
                        0.,
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
    ControlGain(ControlRange),
    Delay(Delay),
    /// Universal delay / modulation effect (delays, chorus, flanger, phaser, ...).
    TimeMod(sampler_ir::fx::timemod::Config),
    /// A time-modulation unit whose stored knobs follow lanes (`Config::set`
    /// at block rate; KSP `ENGINE_PAR_DL_*`, `CH_*`, `FL_*`, `PH_*`).
    LiveTimeMod(sampler_ir::fx::timemod::Config, LiveKnobs),
    /// Every compressor, limiter, gate and expander (`dsp/dynamics/`).
    Dynamics(DynamicsSettings),
    /// One parallel branch of an effect rack: the next `count` processors run
    /// on the signal that entered the first branch, and `gain` times their
    /// output joins the sum. The last branch leaves the sum as the signal.
    /// Per-voice scalar path.
    Branch {
        count: u16,
        gain: f64,
        first: bool,
        last: bool,
    },
    /// Drive, character and utility: the universal color unit (see [`color`]).
    Color(color::Settings),
    /// The filter engine (`dsp/filter/`): native-knob cores (legacy cascade, ladder, AR, Daft).
    Filter(FilterSettings),
    /// Native three-parallel-bandpass Formant I (Talk, Sharp, Size).
    Formant([f32; 3]),
    /// Formant core table 2 (rows 5..=9): Kontakt Formant II.
    FormantII([f32; 3]),
    /// Kontakt native four-stage Phaser filter.
    Phaser([f32; 2]),
    /// Formant I with Talk, Sharp, Size as event-rate lanes (KSP ENGINE_PAR_FORMANT_*).
    LiveFormant([Parameter; 3]),
    StateVariable(StateVariableFilter),
    /// One band of the universal EQ superset (Kontakt legacy, Solid, generic RBJ, scripted UVI).
    Eq(eq_family::EqBand),
    /// Rows are output L/R, columns input L/R; each cell follows a control.
    ControlMatrix([Parameter; 4]),
    /// Reverb and/or convolution; bus scope only (it owns megabytes of
    /// state). `impulse` indexes the table given to
    /// [`crate::Prepared::with_impulses`].
    Space(sampler_ir::fx::space::Config),
    /// A reverb whose stored knobs follow lanes (`Config::set`, `ENGINE_PAR_RV_*`).
    LiveSpace(sampler_ir::fx::space::Config, LiveKnobs),
    /// The next `count` processors run in parallel with the unprocessed
    /// signal: `(dry·(1-b) + b)·x + wet·(1-b)·inner(x)`, where `b` is the
    /// bypass control (0..=1). All three are ramped controls.
    Mix {
        count: u16,
        dry: ControlRange,
        wet: ControlRange,
        bypass: ControlRange,
    },
}


/// A normalized effect knob: a constant or a lane without a ramp, within 0..=1.
/// Stored-value knobs of a unit whose family applies its own setter: each
/// lane's role and the parameter it follows.
pub type LiveKnobs = [Option<(sampler_ir::ProcessorParameter, Parameter)>; LIVE_KNOBS];
/// UVI SparkVerb has the most: wet, dry, size, three decays, predelay, width.
pub const LIVE_KNOBS: usize = 8;
type PreparedKnobs = [Option<(sampler_ir::ProcessorParameter, control::PreparedParameter)>; LIVE_KNOBS];

fn live_valid(knobs: &LiveKnobs) -> bool {
    knobs.iter().flatten().all(|(_, p)| p.valid() && !matches!(p, Parameter::Expression { .. }))
}

fn live_knob(p: &Parameter) -> bool {
    p.valid()
        && matches!(p, Parameter::Constant(_) | Parameter::Control(ControlRange { ramp_frames: 0, .. }))
        && p.bounds().iter().all(|v| (0.0..=1.0).contains(v))
}

impl Processor {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Processor::Gain(gain) => gain.is_finite(),
            Processor::StereoMatrix(matrix) => matrix.iter().flatten().all(|v| v.is_finite()),
            Processor::ControlGain(binding) => binding.valid(),
            Processor::StateVariable(filter) => filter.valid(),
            Processor::Eq(band) => band.valid(),
            Processor::ControlMatrix(cells) => cells
                .iter()
                .all(|c| c.valid() && !matches!(c, Parameter::Expression { .. })),
            Processor::Space(config) => space::valid(config),
            Processor::LiveSpace(config, knobs) => space::valid(config) && live_valid(knobs),
            Processor::LiveTimeMod(config, knobs) => config.check().is_ok() && live_valid(knobs),
            Processor::Mix {
                dry, wet, bypass, ..
            } => dry.valid() && wet.valid() && bypass.valid(),
            Processor::Dynamics(settings) => settings.valid(),
            Processor::Color(settings) => settings.valid(),
            Processor::Filter(settings) => settings.valid(),
            Processor::TimeMod(config) => config.check().is_ok(),
            Processor::Phaser(knobs) => knobs.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            Processor::Formant(knobs) | Processor::FormantII(knobs) => knobs.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            Processor::Branch { gain, .. } => gain.is_finite(),
            Processor::LiveFormant(knobs) => knobs.iter().all(live_knob),
            Processor::Biquad(_) | Processor::Delay(_) => true,
        }
    }
}

// Universal effect families (wave 4).
pub(crate) mod color;
pub(crate) mod dynamics;
pub(crate) mod eq_family;
pub(crate) mod filter;
pub(crate) mod hiir;
pub(crate) mod kaiser;
pub(crate) mod space;
pub(crate) mod timemod;
pub(super) mod control;
pub use ar::ArSettings;
pub use filter::Settings as FilterSettings;
pub use filter::cascade::{CascadeSettings, Legacy as LegacyFilter, Sv as SvFilter};
#[cfg(test)]
mod kernel_tests;
mod kernels;
pub use eq_family::{EqBand, KontaktLegacyEqSettings};
mod live;
pub use ladder::LadderSettings;
mod taps;
pub use taps::{VoiceSendPosition, VoiceSendTap};
pub(super) mod lanes;
use filter::{ar, daft, formant, ladder};
pub(crate) use filter::Knob as FilterKnob;
pub(super) use filter::svf;
pub use dynamics::DynamicsSettings;
pub(super) use control::ControlRamp;
pub use control::{ControlRange, Parameter};
pub(super) use space::Space;
pub use space::{ConvolutionUpload, Impulse, MAX_IMPULSE_FRAMES, neutral_impulse_shape, shape_impulse};
pub use daft::DaftSettings;
pub use timemod::Delay;
pub use color::{
    CurveSettings, Decimator, GainerSettings, HoldSettings, NoiseSettings, QuantizerSettings, Rectifier,
    Settings as ColorSettings, StereoSettings,
};
pub use svf::{OutputLowPass, StateVariableFilter, SvfMode};

pub(super) enum PreparedProcessor {
    Gain(f64),
    StereoMatrix([[f64; 2]; 2]),
    Biquad(Biquad),
    Eq(eq_family::Stage),
    ControlMatrix([control::PreparedParameter; 4]),
    ControlGain(usize),
    TimeMod {
        timemod: timemod::TimeMod,
        offset: usize,
        live: PreparedKnobs,
    },
    Dynamics(dynamics::Dynamics),
    LiveFormant { knobs: [control::PreparedParameter; 3], rate: u32 },
    Formant(formant::Formant),
    Color {
        color: color::Color,
        /// Pseudo-stereo ring cells in the stage's delay storage.
        offset: usize,
    },
    /// The filter engine with its cells at `offset` of the delay storage.
    Filter {
        engine: filter::Engine,
        offset: usize,
    },
    Branch {
        count: u16,
        gain: f64,
        first: bool,
        last: bool,
    },
    StateVariable(svf::SvfHandle),
    /// Index into the bus graph's spaces.
    Space(usize, PreparedKnobs),
    /// `count` following stages in parallel with the dry signal; the lanes are
    /// dry, wet and bypass.
    Mix {
        count: u16,
        lanes: [usize; 3],
    },
}

pub(super) struct CompiledVoiceChain {
    pub parameter_span: std::ops::Range<usize>,
    pre: Box<[PreparedProcessor]>,
    post: Box<[PreparedProcessor]>,
    tail_frames: u32,
    pub delay_frames: usize,
    taps: Box<[taps::PreparedTap]>,
    pub tap_buses: Box<[usize]>,
}

/// The exact operation domain accepted by voice lanes. Payloads move here;
/// scalar-only processors and bus spaces have no representation in this owner.
pub(super) enum BatchOp {
    Gain(f64),
    StereoMatrix([[f64; 2]; 2]),
    Biquad(Biquad),
    Eq(eq_family::ConstantNativeEq),
    Color { color: color::BatchColor, offset: usize },
    ControlGain(usize),
    StateVariable(svf::SvfHandle),
    Mix { count: u16, lanes: [usize; 3] },
}
/// Preparation classifies a valid operation; scalar ownership is not an error.
enum OperationOwner { Batch(BatchOp), Scalar(PreparedProcessor) }
impl PreparedProcessor {
    fn into_batch(self) -> OperationOwner {
        match self {
            Self::Gain(x) => OperationOwner::Batch(BatchOp::Gain(x)),
            Self::StereoMatrix(x) => OperationOwner::Batch(BatchOp::StereoMatrix(x)),
            Self::Biquad(x) => OperationOwner::Batch(BatchOp::Biquad(x)),
            Self::ControlGain(x) => OperationOwner::Batch(BatchOp::ControlGain(x)),
            Self::StateVariable(x) => OperationOwner::Batch(BatchOp::StateVariable(x)),
            Self::Mix { count, lanes } => OperationOwner::Batch(BatchOp::Mix { count, lanes }),
            Self::Eq(eq) => match eq.constant() {
                Some(eq) => OperationOwner::Batch(BatchOp::Eq(eq)),
                None => OperationOwner::Scalar(Self::Eq(eq)),
            },
            Self::Color { color, offset } => match color::BatchColor::classify(color) {
                color::ColorOwner::Batch(color) => OperationOwner::Batch(BatchOp::Color { color, offset }),
                color::ColorOwner::Scalar(color) => OperationOwner::Scalar(Self::Color { color, offset }),
            },
            scalar @ (Self::ControlMatrix(_) | Self::TimeMod { .. } | Self::Dynamics(_)
                | Self::LiveFormant { .. } | Self::Formant(_) | Self::Filter { .. }
                | Self::Branch { .. } | Self::Space(..)) => OperationOwner::Scalar(scalar),
        }
    }
    pub(super) fn view(&self) -> ProcessorRef<'_> {
        match self {
            Self::Gain(x) => ProcessorRef::Gain(x),
            Self::StereoMatrix(x) => ProcessorRef::StereoMatrix(x),
            Self::Biquad(x) => ProcessorRef::Biquad(x),
            Self::Eq(x) => ProcessorRef::Eq(eq_family::EqRef::Scalar(x)),
            Self::ControlGain(x) => ProcessorRef::ControlGain(x),
            Self::ControlMatrix(x) => ProcessorRef::ControlMatrix(x),
            Self::TimeMod { timemod, offset, live } => ProcessorRef::TimeMod { timemod, offset, live },
            Self::Dynamics(x) => ProcessorRef::Dynamics(x),
            Self::LiveFormant { knobs, rate } => ProcessorRef::LiveFormant { knobs, rate },
            Self::Formant(x) => ProcessorRef::Formant(x),
            Self::Color { color, offset } => ProcessorRef::Color { color, offset },
            Self::Filter { engine, offset } => ProcessorRef::Filter { engine, offset },
            Self::Branch { count, gain, first, last } => ProcessorRef::Branch { count, gain, first, last },
            Self::StateVariable(x) => ProcessorRef::StateVariable(x),
            Self::Space(index, live) => ProcessorRef::Space(index, live),
            Self::Mix { count, lanes } => ProcessorRef::Mix { count, lanes },
        }
    }
}
impl BatchOp {
    fn into_scalar(self) -> PreparedProcessor {
        match self {
            Self::Gain(x) => PreparedProcessor::Gain(x),
            Self::StereoMatrix(x) => PreparedProcessor::StereoMatrix(x),
            Self::Biquad(x) => PreparedProcessor::Biquad(x),
            Self::Eq(x) => PreparedProcessor::Eq(x.into_stage()),
            Self::Color { color, offset } => PreparedProcessor::Color { color: color.into_color(), offset },
            Self::ControlGain(x) => PreparedProcessor::ControlGain(x),
            Self::StateVariable(x) => PreparedProcessor::StateVariable(x),
            Self::Mix { count, lanes } => PreparedProcessor::Mix { count, lanes },
        }
    }
    fn view(&self) -> ProcessorRef<'_> {
        match self {
            Self::Gain(x) => ProcessorRef::Gain(x),
            Self::StereoMatrix(x) => ProcessorRef::StereoMatrix(x),
            Self::Biquad(x) => ProcessorRef::Biquad(x),
            Self::Eq(x) => ProcessorRef::Eq(eq_family::EqRef::Constant(x)),
            Self::Color { color, offset } => ProcessorRef::Color { color: color.color(), offset },
            Self::ControlGain(x) => ProcessorRef::ControlGain(x),
            Self::StateVariable(x) => ProcessorRef::StateVariable(x),
            Self::Mix { count, lanes } => ProcessorRef::Mix { count, lanes },
        }
    }
}

/// Exhaustive borrowed scalar/trace/finiteness view. No payload or history is
/// copied into a general operation array to render a typed batch owner.
#[derive(Clone, Copy)]
pub(super) enum ProcessorRef<'a> {
    Gain(&'a f64), StereoMatrix(&'a [[f64; 2]; 2]), Biquad(&'a Biquad),
    Eq(eq_family::EqRef<'a>), ControlGain(&'a usize),
    ControlMatrix(&'a [control::PreparedParameter; 4]),
    TimeMod { timemod: &'a timemod::TimeMod, offset: &'a usize, live: &'a PreparedKnobs },
    Dynamics(&'a dynamics::Dynamics),
    LiveFormant { knobs: &'a [control::PreparedParameter; 3], rate: &'a u32 },
    Formant(&'a formant::Formant),
    Color { color: &'a color::Color, offset: &'a usize },
    Filter { engine: &'a filter::Engine, offset: &'a usize },
    Branch { count: &'a u16, gain: &'a f64, first: &'a bool, last: &'a bool },
    StateVariable(&'a svf::SvfHandle), Space(&'a usize, &'a PreparedKnobs),
    Mix { count: &'a u16, lanes: &'a [usize; 3] },
}
impl<'a> From<&'a PreparedProcessor> for ProcessorRef<'a> {
    fn from(stage: &'a PreparedProcessor) -> Self { stage.view() }
}
#[derive(Clone, Copy)]
pub(super) enum ProcessorSlice<'a> {
    Scalar(&'a [PreparedProcessor]),
    Batch(&'a [BatchOp]),
}
impl<'a> ProcessorSlice<'a> {
    pub(super) fn len(self) -> usize { match self { Self::Scalar(x) => x.len(), Self::Batch(x) => x.len() } }
    pub(super) fn get(self, index: usize) -> ProcessorRef<'a> {
        match self { Self::Scalar(x) => x[index].view(), Self::Batch(x) => x[index].view() }
    }
    pub(super) fn slice(self, range: std::ops::Range<usize>) -> Self {
        match self { Self::Scalar(x) => Self::Scalar(&x[range]), Self::Batch(x) => Self::Batch(&x[range]) }
    }
    pub(super) fn iter(self) -> impl Iterator<Item = ProcessorRef<'a>> {
        (0..self.len()).map(move |index| self.get(index))
    }
}
impl<'a> From<&'a [BatchOp]> for ProcessorSlice<'a> {
    fn from(stages: &'a [BatchOp]) -> Self { Self::Batch(stages) }
}
impl<'a, const N: usize> From<&'a [BatchOp; N]> for ProcessorSlice<'a> {
    fn from(stages: &'a [BatchOp; N]) -> Self { Self::Batch(stages) }
}
impl<'a> From<&'a [PreparedProcessor]> for ProcessorSlice<'a> {
    fn from(stages: &'a [PreparedProcessor]) -> Self { Self::Scalar(stages) }
}
impl<'a, const N: usize> From<&'a [PreparedProcessor; N]> for ProcessorSlice<'a> {
    fn from(stages: &'a [PreparedProcessor; N]) -> Self { Self::Scalar(stages) }
}

struct ScalarOps { pre: Box<[PreparedProcessor]>, post: Box<[PreparedProcessor]> }
struct BatchOps { logical_chain: usize, pre: Box<[BatchOp]>, post: Box<[BatchOp]> }
#[derive(Clone, Copy)]
enum ChainOwner { Scalar(usize), Batch(BatchIndex) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BatchIndex(usize);
/// Stable source/tail/tap metadata keeps the same identity for source reuse.
/// It owns no parallel copy of its processor lists.
pub(super) struct PreparedVoiceChain {
    pub parameter_span: std::ops::Range<usize>,
    owner: ChainOwner,
    stage_count: usize,
    tail_frames: u32,
    pub delay_frames: usize,
    taps: Box<[taps::PreparedTap]>,
    pub tap_buses: Box<[usize]>,
}
impl PreparedVoiceChain {
    pub(super) fn batch_index(&self) -> Option<BatchIndex> {
        match self.owner { ChainOwner::Scalar(_) => None, ChainOwner::Batch(index) => Some(index) }
    }
}
#[derive(Default)]
pub(super) struct PreparedChains {
    chains: Box<[PreparedVoiceChain]>,
    scalar: Box<[ScalarOps]>,
    batch: Box<[BatchOps]>,
}
impl std::ops::Deref for PreparedChains {
    type Target = [PreparedVoiceChain];
    fn deref(&self) -> &Self::Target { &self.chains }
}
#[derive(Clone, Copy)]
pub(super) struct PreparedChainRef<'a> {
    common: &'a PreparedVoiceChain,
    pre: ProcessorSlice<'a>,
    post: ProcessorSlice<'a>,
}
impl std::ops::Deref for PreparedChainRef<'_> {
    type Target = PreparedVoiceChain;
    fn deref(&self) -> &Self::Target { self.common }
}
pub(super) struct PreparedBatchRef<'a> {
    view: PreparedChainRef<'a>,
    ops: &'a BatchOps,
}
impl<'a> std::ops::Deref for PreparedBatchRef<'a> {
    type Target = PreparedChainRef<'a>;
    fn deref(&self) -> &Self::Target { &self.view }
}
impl PreparedBatchRef<'_> {
    pub(super) fn pre(&self) -> &[BatchOp] { &self.ops.pre }
    pub(super) fn post(&self) -> &[BatchOp] { &self.ops.post }
}

/// A one-pass off-audio move. If any stage is scalar-only, earlier batch values
/// move back without losing f64 controls, offsets or backing owners. No graph is
/// retained twice, and there is no rejection/fallback in a render operation.
fn batch_slice(stages: Box<[PreparedProcessor]>) -> Result<Box<[BatchOp]>, Box<[PreparedProcessor]>> {
    let count = stages.len();
    let mut batch = Vec::with_capacity(count);
    let mut stages = stages.into_vec().into_iter();
    while let Some(stage) = stages.next() {
        match stage.into_batch() {
            OperationOwner::Batch(stage) => batch.push(stage),
            OperationOwner::Scalar(stage) => {
                let mut scalar = Vec::with_capacity(count);
                scalar.extend(batch.into_iter().map(BatchOp::into_scalar));
                scalar.push(stage);
                scalar.extend(stages);
                return Err(scalar.into_boxed_slice());
            }
        }
    }
    Ok(batch.into_boxed_slice())
}
impl PreparedChains {
    pub(super) fn from_compiled(chains: Vec<CompiledVoiceChain>) -> Self {
        let mut common = Vec::with_capacity(chains.len());
        let mut scalar = Vec::new();
        let mut batch = Vec::new();
        for CompiledVoiceChain { parameter_span, pre, post, tail_frames, delay_frames, taps, tap_buses } in chains {
            let stage_count = pre.len() + post.len();
            let batchable = if taps.is_empty() {
                match batch_slice(pre) {
                    Ok(pre) => match batch_slice(post) {
                        Ok(post) => Ok((pre, post)),
                        Err(post) => Err((pre.into_vec().into_iter().map(BatchOp::into_scalar).collect(), post)),
                    },
                    Err(pre) => Err((pre, post)),
                }
            } else { Err((pre, post)) };
            let owner = match batchable {
                Ok((pre, post)) => {
                    let index = BatchIndex(batch.len());
                    batch.push(BatchOps { logical_chain: common.len(), pre, post });
                    ChainOwner::Batch(index)
                }
                Err((pre, post)) => {
                    let index = scalar.len(); scalar.push(ScalarOps { pre, post }); ChainOwner::Scalar(index)
                }
            };
            common.push(PreparedVoiceChain { parameter_span, owner, stage_count, tail_frames,
                delay_frames, taps, tap_buses });
        }
        Self { chains: common.into_boxed_slice(), scalar: scalar.into_boxed_slice(), batch: batch.into_boxed_slice() }
    }
    pub(super) fn view(&self, index: usize) -> PreparedChainRef<'_> {
        let common = &self.chains[index];
        let (pre, post) = match common.owner {
            ChainOwner::Scalar(i) => (ProcessorSlice::Scalar(&self.scalar[i].pre), ProcessorSlice::Scalar(&self.scalar[i].post)),
            ChainOwner::Batch(i) => (ProcessorSlice::Batch(&self.batch[i.0].pre), ProcessorSlice::Batch(&self.batch[i.0].post)),
        };
        PreparedChainRef { common, pre, post }
    }
    pub(super) fn batch(&self, index: BatchIndex) -> PreparedBatchRef<'_> {
        let ops = &self.batch[index.0];
        PreparedBatchRef { view: PreparedChainRef { common: &self.chains[ops.logical_chain],
            pre: ProcessorSlice::Batch(&ops.pre), post: ProcessorSlice::Batch(&ops.post) }, ops }
    }
    pub(super) fn push_empty(&mut self, chain: CompiledVoiceChain) {
        // Trace inserts an actually empty chain. Rebuild only these immutable
        // descriptors/lists off audio, moving all existing operation owners.
        let mut chains = std::mem::take(&mut self.chains).into_vec();
        let mut batch = std::mem::take(&mut self.batch).into_vec();
        let CompiledVoiceChain { parameter_span, pre, post, tail_frames, delay_frames, taps, tap_buses } = chain;
        // The caller constructs an empty VoiceChain; classify its actual lists
        // with the same preparation path rather than assert that shape here.
        let appended = Self::from_compiled(vec![CompiledVoiceChain { parameter_span, pre, post,
            tail_frames, delay_frames, taps, tap_buses }]);
        let mut scalar = std::mem::take(&mut self.scalar).into_vec();
        let scalar_offset = scalar.len(); let batch_offset = batch.len(); let logical_offset = chains.len();
        scalar.extend(appended.scalar);
        batch.extend(appended.batch.into_vec().into_iter().map(|mut ops| { ops.logical_chain += logical_offset; ops }));
        chains.extend(appended.chains.into_vec().into_iter().map(|mut chain| {
            chain.owner = match chain.owner { ChainOwner::Scalar(i) => ChainOwner::Scalar(i + scalar_offset),
                ChainOwner::Batch(i) => ChainOwner::Batch(BatchIndex(i.0 + batch_offset)) };
            chain
        }));
        self.chains = chains.into_boxed_slice(); self.scalar = scalar.into_boxed_slice(); self.batch = batch.into_boxed_slice();
    }
}
#[cfg(test)]
impl CompiledVoiceChain {
    fn stages(&self) -> usize { self.pre.len() + self.post.len() }

}

pub(super) struct RenderContext<'a> {
    pub controls: Option<crate::whole_voice::Render<'a>>,
    pub amplifier: Option<crate::voice_mod::Ramp>,
    pub expression: Frame,
    pub delay: &'a mut [[f64; 2]],
    pub parameters: &'a [ControlRamp],
    pub filters: svf::FilterContext<'a>,
    pub at: u64,
    pub feeds: &'a mut [taps::TapFeed],
    pub trace: Option<crate::trace::VoiceTrace<'a>>,
    /// This voice's time-stretch engine, if it has one.
    pub stretch: Option<&'a mut crate::source::StretchEngine>,
}

/// Serial stereo processing with an explicit envelope boundary.
/// Tail frames are an authored maximum after source/envelope completion, not a
/// guessed silence threshold or a claim that an IIR has a mathematically finite tail.
pub struct VoiceChain {
    pre: Box<[Processor]>,
    post: Box<[Processor]>,
    tail_frames: u32,
    taps: Box<[VoiceSendTap]>,
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
            taps: Box::new([]),
        })
    }
    pub(super) fn compile(
        self,
        rate: u32,
        bindings: &mut Vec<ControlRange>,
        filters: &mut svf::FilterRegistryBuilder,
    ) -> Result<CompiledVoiceChain, Error> {
        let first_parameter = bindings.len();
        let mut delay_frames = 0;
        let mut tap_buses: Vec<_> = self.taps.iter().map(|tap| tap.bus).collect();
        tap_buses.sort_unstable();
        tap_buses.dedup();
        Ok(CompiledVoiceChain {
            pre: compile_processors(
                self.pre,
                rate,
                bindings,
                &mut delay_frames,
                filters,
                None,
            )?,
            post: compile_processors(
                self.post,
                rate,
                bindings,
                &mut delay_frames,
                filters,
                None,
            )?,
            tail_frames: self.tail_frames,
            delay_frames,
            taps: self
                .taps
                .into_iter()
                .map(|tap| tap.compile(bindings))
                .collect(),
            tap_buses: tap_buses.into_boxed_slice(),
            parameter_span: first_parameter..bindings.len(),
        })
    }
}
fn prepare_timemod(
    timemod: timemod::TimeMod,
    live: Option<LiveKnobs>,
    bindings: &mut Vec<ControlRange>,
    delay_frames: &mut usize,
) -> Result<PreparedProcessor, Error> {
    let offset = *delay_frames;
    *delay_frames = offset.checked_add(timemod.frames()).ok_or(Error::Capacity)?;
    let live = live.map_or([None; LIVE_KNOBS], |knobs| {
        knobs.map(|k| k.map(|(role, parameter)| (role, parameter.compile(bindings))))
    });
    Ok(PreparedProcessor::TimeMod { timemod, offset, live })
}

pub(super) fn compile_processors(
    stages: Box<[Processor]>,
    rate: u32,
    bindings: &mut Vec<ControlRange>,
    delay_frames: &mut usize,
    filters: &mut svf::FilterRegistryBuilder,
    mut spaces: Option<&mut Vec<sampler_ir::fx::space::Config>>,
) -> Result<Box<[PreparedProcessor]>, Error> {
    if stages.iter().any(|stage| !stage.valid()) {
        return Err(Error::InvalidInput);
    }
    stages
        .into_vec()
        .into_iter()
        .map(|stage| {
            Ok(match stage {
                Processor::StateVariable(filter) => {
                    let index = filters.push_svf(filter.compile(rate, bindings)?);
                    PreparedProcessor::StateVariable(index)
                }
                Processor::Delay(delay) => prepare_timemod(
                    timemod::TimeMod::compile(delay.config(), rate)?, None, bindings, delay_frames,
                )?,
                Processor::TimeMod(config) => prepare_timemod(
                    timemod::TimeMod::compile(config, rate)?, None, bindings, delay_frames,
                )?,
                Processor::LiveTimeMod(config, knobs) => prepare_timemod(
                    timemod::TimeMod::compile(config, rate)?, Some(knobs), bindings, delay_frames,
                )?,
                Processor::Phaser(knobs) => prepare_timemod(
                    timemod::TimeMod::phaser_filter(knobs)?, None, bindings, delay_frames,
                )?,
                Processor::Dynamics(settings) => {
                    PreparedProcessor::Dynamics(settings.prepare(rate, bindings, delay_frames)?)
                }
                Processor::Branch {
                    count,
                    gain,
                    first,
                    last,
                } => PreparedProcessor::Branch {
                    count,
                    gain,
                    first,
                    last,
                },
                Processor::Filter(settings) => {
                    let offset = *delay_frames;
                    let mut engine = settings.compile(rate, bindings);
                    *delay_frames = offset.checked_add(engine.cells()).ok_or(Error::Capacity)?;
                    engine.modulation_index = filters.push_native();
                    PreparedProcessor::Filter { engine, offset }
                }
                Processor::LiveFormant(knobs) => PreparedProcessor::LiveFormant { knobs: knobs.map(|k| k.compile(bindings)), rate },
            Processor::Formant(knobs) => PreparedProcessor::Formant(formant::Formant::compile(knobs, false, rate)),
            Processor::FormantII(knobs) => PreparedProcessor::Formant(formant::Formant::compile(knobs, true, rate)),
                Processor::Color(settings) => {
                    let offset = *delay_frames;
                    *delay_frames = offset.checked_add(settings.ring_frames(rate)).ok_or(Error::Capacity)?;
                    PreparedProcessor::Color {
                        color: settings.compile(rate, bindings),
                        offset,
                    }
                }
                Processor::Space(config) | Processor::LiveSpace(config, _) => {
                    let spaces = spaces.as_deref_mut().ok_or(Error::InvalidInput)?;
                    spaces.push(config);
                    let live = match stage {
                        Processor::LiveSpace(_, knobs) => knobs.map(|k| k.map(|(r, p)| (r, p.compile(bindings)))),
                        _ => [None; LIVE_KNOBS],
                    };
                    PreparedProcessor::Space(spaces.len() - 1, live)
                }
                Processor::Mix {
                    count,
                    dry,
                    wet,
                    bypass,
                } => {
                    let lanes = [dry, wet, bypass].map(|binding| {
                        bindings.push(binding);
                        bindings.len() - 1
                    });
                    PreparedProcessor::Mix { count, lanes }
                }
                Processor::Gain(gain) => PreparedProcessor::Gain(gain),
                Processor::StereoMatrix(matrix) => PreparedProcessor::StereoMatrix(matrix),
                Processor::Biquad(filter) => {
                    if filter.rate != rate {
                        return Err(Error::InvalidInput);
                    }
                    PreparedProcessor::Biquad(filter)
                }
                Processor::Eq(band) => PreparedProcessor::Eq(band.compile(rate, bindings)?),
                Processor::ControlMatrix(cells) => {
                    PreparedProcessor::ControlMatrix(cells.map(|c| c.compile(bindings)))
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
        self.stage_count
    }



    /// Render the source for one block into `block` and settle its length:
    /// None once the chain is done. Frames past the source's output are its
    /// zero-input tail, which starts at the first of them and runs for at most
    /// `tail_frames`.
    pub(super) fn begin(
        &self,
        voice: &mut Voice,
        pcm: &(impl crate::source::ReadFrames + ?Sized),
        frames: usize,
        kernel: &crate::resample::Kernel,
        block: &mut Planar,
        stretch: Option<&mut crate::source::StretchEngine>,
    ) -> Option<Begun> {
        if self.done(voice) {
            return None;
        }
        let mut raw = [[0.; 2]; BLOCK];
        let waiting = voice.cursor.waiting();
        let active = voice
            .envelope
            .remaining()
            .min(voice.tail_remaining.map_or(usize::MAX, |n| n as usize));
        // A finite unity hold clocks only frames the cursor advances, including silent misses.
        let mut unity = EnvelopeState::new(if waiting {
            Envelope::one_shot(0, active.min(BLOCK + 1) as u32, 0)
        } else {
            Envelope::default()
        });
        let remaining = unity.remaining();
        let count = if waiting { frames } else { frames.min(active) };
        let produced = if voice.cursor.done() || voice.envelope.done() {
            0
        } else if let (Some(engine), false) = (stretch, waiting) {
            crate::source::stretch_block(engine, voice, pcm, &mut raw[..count])
        } else {
            voice
                .cursor
                .render(pcm, &mut raw[..count], &mut unity, 1.0, [1.; 2], kernel)
        };
        let held = if waiting {
            produced - (remaining - unity.remaining())
        } else {
            0
        };
        let tail = voice
            .tail_remaining
            .or_else(|| (produced < frames).then_some(self.tail_frames));
        let len = match (voice.tail_remaining, tail) {
            (Some(t), _) => held + (frames - held).min(t as usize),
            (None, Some(t)) => produced + (frames - produced).min(t as usize),
            (None, None) => frames,
        };
        for (i, frame) in raw[..produced.min(len)].iter().enumerate() {
            block[0][i] = f64::from(frame[0]);
            block[1][i] = f64::from(frame[1]);
        }
        Some(Begun {
            produced,
            len,
            held,
            tail,
        })
    }



    pub(super) fn done(&self, voice: &Voice) -> bool {
        voice.tail_remaining == Some(0)
            || (self.tail_frames == 0 && (voice.cursor.done() || voice.envelope.done()))
    }
}

impl PreparedChainRef<'_> {
    pub(super) fn pre_len(&self) -> usize { self.pre.len() }
    pub(super) fn post_len(&self) -> usize { self.post.len() }
    pub(crate) fn filter_indices(&self, stages: std::ops::Range<usize>) -> Vec<u32> {
        self.pre
            .iter()
            .chain(self.post.iter())
            .enumerate()
            .filter_map(|(i, p)| match p {
                ProcessorRef::StateVariable(filter) if stages.contains(&i) => {
                    u32::try_from(filter.addressed()).ok()
                }
                ProcessorRef::Filter { engine, .. } if stages.contains(&i) => {
                    u32::try_from(engine.modulation_index).ok()
                }
                _ => None,
            })
            .collect()
    }
    /// Render one voice block by block: every stage runs over the whole block
    /// before the next, with its coefficients and response chosen once. A
    /// nonfinite block drops that voice's block and resets its processor state.
    pub(super) fn render<const TRACE: bool>(
        &self,
        voice: &mut Voice,
        pcm: &(impl crate::source::ReadFrames + ?Sized),
        output: &mut [Frame],
        states: &mut [ProcessorState],
        mut context: RenderContext<'_>,
        kernel: &crate::resample::Kernel,
    ) -> (usize, u64) {
        let mut faults = 0;
        let mut rendered = 0;
        for (chunk_index, chunk) in output.chunks_mut(BLOCK).enumerate() {
            let mut block = [[0.; BLOCK]; 2];
            let Some(begun) = self.begin(
                voice,
                pcm,
                chunk.len(),
                kernel,
                &mut block,
                context.stretch.as_deref_mut(),
            ) else {
                break;
            };
            let len = begun.len;
            if begun.held == len {
                rendered += len;
                continue;
            }
            if TRACE {
                if let Some(t) = context.trace.as_mut() {
                    t.identity.source_metrics = crate::trace::metrics(&block, len);
                    t.record(
                        t.nodes.source,
                        &block,
                        &block,
                        len,
                        [1.; 2],
                        context.parameters,
                    );
                }
            }
            let at = context.at + (chunk_index * BLOCK) as u64;
            let (pre, post) = states.split_at_mut(self.pre.len());
            let mut fault = self.process_section::<TRACE>(
                true,
                pre,
                &mut block,
                len,
                begun.held,
                at,
                &mut context,
            );
            let levels = if let Some(controls) = context.controls.as_mut() {
                controls.levels(voice, len, begun.held)
            } else { levels(voice, len, begun.held) };

            let before_amp = if TRACE { block } else { [[0.; BLOCK]; 2] };
            let amplifier = context.amplifier.map(|r| r.prepare_gains());
            let [left, right] = &mut block;
            for (i, ((l, r), level)) in left[..len]
                .iter_mut()
                .zip(&mut right[..len])
                .zip(&levels)
                .enumerate()
            {
                let gains = amplifier.as_ref().map_or([1.0; 2], |r| r.at(at + i as u64 + 1));
                *l *= level * f64::from(gains[0]);
                *r *= level * f64::from(gains[1]);
            }
            if TRACE {
                if let Some(t) = context.trace.as_mut() {
                    t.record(
                        t.nodes.amp,
                        &before_amp,
                        &block,
                        len,
                        std::array::from_fn(|c| {
                            levels[..len]
                                .iter()
                                .enumerate()
                                .map(|(i, l)| {
                                    l * f64::from(
                                        context
                                            .amplifier
                                            .map_or([1.; 2], |r| r.gains_at(at + i as u64 + 1))[c],
                                    )
                                })
                                .sum::<f64>()
                                / len.max(1) as f64
                        }),
                        context.parameters,
                    );
                }
            }
            fault |= self.process_section::<TRACE>(
                false,
                post,
                &mut block,
                len,
                begun.held,
                at,
                &mut context,
            );
            let finish_fault = self.finish(
                voice,
                begun,
                &block,
                fault,
                states,
                context.expression,
                chunk,
            );
            faults += u64::from(finish_fault);
            if TRACE {
                if let Some(t) = context.trace.as_mut() {
                    let mut final_block = block;
                    for c in 0..2 {
                        for i in begun.held..len {
                            let fade = voice.dsp_fade.map_or(1., |(total, initial)| {
                                // Held startup frames do not consume the fade clock.
                                let remaining =
                                    voice.tail_remaining.unwrap_or(0) + (len - i) as u32;
                                f64::from(initial) * f64::from(remaining) / f64::from(total)
                            });
                            final_block[c][i] *= f64::from(context.expression[c]) * fade;
                            if finish_fault {
                                final_block[c][i] = 0.;
                            }
                        }
                    }
                    t.record(
                        t.nodes.output,
                        &block,
                        &final_block,
                        len,
                        context.expression.map(f64::from),
                        context.parameters,
                    );
                    if t.graph.nodes[t.nodes.output].bus.is_none() {
                        t.record(
                            t.graph.master,
                            &final_block,
                            &final_block,
                            len,
                            [1.; 2],
                            context.parameters,
                        );
                    }
                }
            }
            rendered += len;
        }
        (rendered, faults)
    }
    /// Scale, check and mix one processed block, then advance the tail.
    /// Returns whether the block faulted (and was dropped).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish(
        &self,
        voice: &mut Voice,
        Begun {
            produced,
            len,
            held,
            tail,
        }: Begun,
        block: &Planar,
        fault: bool,
        states: &mut [ProcessorState],
        expression: Frame,
        output: &mut [Frame],
    ) -> bool {
        let mut result = [[0f32; 2]; BLOCK];
        for (i, frame) in result[held..len].iter_mut().enumerate() {
            // A DSP fade exists only with a running tail: its count at frame i.
            let fade = voice.dsp_fade.map_or(1., |(total, initial)| {
                let remaining = voice.tail_remaining.expect("fading tail") - i as u32;
                f64::from(initial) * f64::from(remaining) / f64::from(total)
            });
            *frame = std::array::from_fn(|channel| {
                (block[channel][held + i] * f64::from(expression[channel]) * fade) as f32
            });
        }
        let fault = fault
            || !result[..len].iter().flatten().all(|v| v.is_finite())
            || !finite_states(self.pre.iter().chain(self.post.iter()), states);
        if fault {
            states.fill(ProcessorState::default());
        } else {
            for (frame, result) in output.iter_mut().zip(&result[..len]) {
                for channel in 0..2 {
                    frame[channel] += flush32(result[channel]);
                }
            }
        }
        voice.tail_remaining = match voice.tail_remaining {
            Some(t) => Some(t - (len - held) as u32),
            None => tail.map(|t| t - (len - produced) as u32),
        };
        fault
    }
}

/// One voice block's source output, settled by [`PreparedVoiceChain::begin`].
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(super) struct Begun {
    pub produced: usize,
    pub len: usize,
    /// Silent startup prefix whose source, envelope and processor state rest.
    pub held: usize,
    tail: Option<u32>,
}

/// The voice envelope's level for each of `len` frames.
pub(super) fn levels(voice: &mut Voice, len: usize, held: usize) -> [f64; BLOCK] {
    let mut levels = [0.; BLOCK];
    for level in &mut levels[held..len] {
        *level = f64::from(voice.gain)
            * f64::from(
                voice
                    .envelope
                    .constant_level()
                    .unwrap_or_else(|| voice.envelope.next()),
            );
    }
    levels
}

/// Frames per processing block. Render segmentation never presents a voice or
/// bus more than this many frames from one start.
pub(super) const BLOCK: usize = 64;
/// Left and right channels of one block, in double precision.
pub(super) type Planar = [[f64; BLOCK]; 2];

/// Zero a subnormal state or sample: run once per block on retained state.
#[inline(always)]
pub(super) fn flush(v: f64) -> f64 {
    if v.is_subnormal() { 0. } else { v }
}
#[inline(always)]
fn flush32(v: f32) -> f32 {
    if v.is_subnormal() { 0. } else { v }
}

#[derive(Clone, Copy, Default)]
pub(super) struct ProcessorState {
    z: [[f64; 2]; 2],
    /// Further state for stages that need more than `z` (the decimator).
    aux: [f64; 16],
    delay_position: u32,
    delay_filled: u32,
    /// The color unit's stage histories.
    color: color::State,
}
impl ProcessorState {
    fn finite_for(&self, stage: ProcessorRef<'_>) -> bool {
        let common = self.z
            .iter()
            .flatten()
            .chain(&self.aux)
            .all(|v| v.is_finite());
        common && match stage {
            ProcessorRef::Color { color, .. } => color.finite_state(&self.color),
            // Native KONTAKTFX filters keep their history in the color slot.
            ProcessorRef::Filter { .. } => self.color.finite(),
            ProcessorRef::Gain(_)
            | ProcessorRef::StereoMatrix(_)
            | ProcessorRef::Biquad(_)
            | ProcessorRef::ControlGain(_)
            | ProcessorRef::TimeMod { .. }
            | ProcessorRef::Dynamics(_)
            | ProcessorRef::Formant(_)
            | ProcessorRef::LiveFormant { .. }
            | ProcessorRef::Eq(_)
            | ProcessorRef::ControlMatrix(_)
            | ProcessorRef::Branch { .. }
            | ProcessorRef::StateVariable(_)
            | ProcessorRef::Space(..)
            | ProcessorRef::Mix { .. } => true,
        }
    }
}

/// Prepared stage identities cannot change while their state cells are owned.
/// A reused voice resets its cells before a different chain can claim them.
/// Validate all potentially live state, including bypassed stages, without
/// scanning color histories that this immutable processor cannot touch.
pub(super) fn finite_states<'a, T: Into<ProcessorRef<'a>>>(
    stages: impl IntoIterator<Item = T>,
    states: &[ProcessorState],
) -> bool {
    let mut stages = stages.into_iter();
    for state in states {
        let Some(stage) = stages.next() else { return false; };
        if !state.finite_for(stage.into()) {
            return false;
        }
    }
    stages.next().is_none()
}

/// Run `len` frames of `block` through each stage in turn. Returns whether a
/// stage observed a nonfinite value it does not keep in visible state.
#[allow(clippy::too_many_arguments)]
pub(super) fn process<'a, const TRACE: bool>(
    stages: impl Into<ProcessorSlice<'a>>,
    states: &mut [ProcessorState],
    block: &mut Planar,
    len: usize,
    parameters: &[ControlRamp],
    at: u64,
    delay_samples: &mut [[f64; 2]],
    filters: &mut svf::FilterContext<'_>,
    mut trace: Option<crate::trace::Section<'_>>,
) -> bool {
    let stages = stages.into();
    let mut fault = false;
    let mut next = 0;
    // The entering signal and the running sum of the rack branch in progress.
    let mut split: Option<(Planar, Planar)> = None;
    while next < stages.len() {
        let (stage, index) = (stages.get(next), next);
        next += 1;
        let state = &mut states[index];
        let input = if TRACE { *block } else { [[0.; BLOCK]; 2] };
        let mut applied = [1.; 2];
        let mut trace_output = None;
        let mut enabled = true;
        match stage {
            ProcessorRef::Branch {
                count,
                gain,
                first,
                last,
            } => {
                let inner = next..next + usize::from(*count);
                next = inner.end;
                if *first || split.is_none() {
                    split = Some((*block, [[0.; BLOCK]; 2]));
                }
                fault |= process::<TRACE>(
                    stages.slice(inner.clone()),
                    &mut states[inner.clone()],
                    block,
                    len,
                    parameters,
                    at,
                    delay_samples,
                    filters,
                    if TRACE {
                        trace.as_mut().map(|t| crate::trace::Section {
                            recorder: &mut *t.recorder,
                            graph: t.graph,
                            nodes: &t.nodes[inner.clone()],
                            identity: t.identity,
                        })
                    } else {
                        None
                    },
                );
                if let Some((entering, sum)) = split.as_mut() {
                    for c in 0..2 {
                        for i in 0..len {
                            sum[c][i] += gain * block[c][i];
                        }
                    }
                    if TRACE {
                        trace_output = Some(*sum);
                        applied = [*gain; 2];
                    }
                    *block = if *last { *sum } else { *entering };
                }
                if *last {
                    split = None;
                }
            }
            ProcessorRef::Mix { count, lanes } => {
                let inner = next..next + usize::from(*count);
                next = inner.end;
                let [dry, wet, bypass] = lanes.map(|lane| parameters[lane]);
                let last = at + len.saturating_sub(1) as u64;
                // A fully bypassed block skips the inner processors (their
                // state, such as a reverb tail, rests until the bypass lifts).
                let off = bypass.value(at) >= 1. && bypass.value(last) >= 1.;
                let dry_block = *block;
                enabled = !off;
                applied = [if off { 1. } else { wet.value(at) }; 2];
                if TRACE && off {
                    if let Some(t) = trace.as_mut() {
                        for skipped in inner.clone() {
                            t.record(
                                skipped, &dry_block, &dry_block, len, [1.; 2], false, parameters,
                            );
                        }
                    }
                }
                if !off {
                    fault |= process::<TRACE>(
                        stages.slice(inner.clone()),
                        &mut states[inner.clone()],
                        block,
                        len,
                        parameters,
                        at,
                        delay_samples,
                        filters,
                        if TRACE {
                            trace.as_mut().map(|t| crate::trace::Section {
                                recorder: &mut *t.recorder,
                                graph: t.graph,
                                nodes: &t.nodes[inner.clone()],
                                identity: t.identity,
                            })
                        } else {
                            None
                        },
                    );
                }
                let held = dry
                    .held(at, len)
                    .zip(wet.held(at, len))
                    .zip(bypass.held(at, len));
                if let Some(((dry, wet), bypass)) = held {
                    let gains = kernels::mix_gains(dry, wet, bypass);
                    sampler_simd::dispatch(
                        #[inline(always)]
                        || {
                            for (channel, input) in block.iter_mut().zip(&dry_block) {
                                for (out, dry) in channel[..len].iter_mut().zip(input) {
                                    *out = kernels::mix(*dry, *out, gains, off);
                                }
                            }
                        },
                    );
                } else {
                    for c in 0..2 {
                        for i in 0..len {
                            let t = at + i as u64;
                            let b = bypass.value(t);
                            let gains = kernels::mix_gains(dry.value(t), wet.value(t), b);
                            block[c][i] = kernels::mix(dry_block[c][i], block[c][i], gains, off);
                        }
                    }
                }
            }
            ProcessorRef::StateVariable(index) => {
                filters.process(*index, &mut state.z, block, len, parameters, at);
            }
            ProcessorRef::Space(index, live) => {
                if live[0].is_some() {
                    let space = &mut filters.spaces[*index];
                    let mut config = space.config();
                    for (role, knob) in live.iter().flatten() {
                        config.set(*role, knob.value(parameters, at, None));
                    }
                    space.retune(config);
                }
                let mut wet = [[0f32; BLOCK]; 2];
                for (w, v) in wet.iter_mut().zip(block.iter()) {
                    w[..len]
                        .iter_mut()
                        .zip(&v[..len])
                        .for_each(|(w, v)| *w = *v as f32);
                }
                let [left, right] = &mut wet;
                filters.spaces[*index].process([&mut left[..len], &mut right[..len]]);
                for (v, w) in block.iter_mut().zip(&wet) {
                    v[..len]
                        .iter_mut()
                        .zip(&w[..len])
                        .for_each(|(v, w)| *v = f64::from(*w));
                }
            }
            ProcessorRef::TimeMod { timemod, offset, live } => {
                // Event-rate lanes: the unit re-derives from its stored knobs; the ring persists.
                let retuned;
                let timemod = if live[0].is_some() {
                    let mut config = timemod.config();
                    for (role, knob) in live.iter().flatten() {
                        config.set(*role, knob.value(parameters, at, None));
                    }
                    retuned = timemod.retuned(config);
                    &retuned
                } else {
                    timemod
                };
                fault |= timemod.process(
                    state,
                    &mut delay_samples[*offset..*offset + timemod.frames()],
                    block,
                    len,
                    filters.tempo,
                );
            }
            ProcessorRef::Dynamics(dynamics) => dynamics.process(
                state,
                &mut delay_samples[dynamics.cells()],
                parameters,
                block,
                len,
                at,
            ),
            ProcessorRef::Formant(formant) => formant.process(state, block, len),
            // Event-rate lanes: constant within a fragment (as LiveDistortion); the
            // coefficients follow them while the filter and noise states persist.
            ProcessorRef::LiveFormant { knobs, rate } => {
                let [talk, sharp, size] = knobs.map(|k| k.value(parameters, at, None) as f32);
                formant::Formant::compile([talk, sharp, size], false, *rate).process(state, block, len)
            }
            ProcessorRef::Filter { engine, offset } => {
                fault |= engine.process(
                    state,
                    &mut delay_samples[*offset..*offset + engine.cells()],
                    parameters,
                    block,
                    len,
                    at,
                    filters.bank.native_knobs(engine.modulation_index),
                );
            }
            ProcessorRef::Color { color, offset } => {
                let ring = &mut delay_samples[*offset..];
                color.process(&mut state.color, parameters, block, len, at, ring);
            }
            ProcessorRef::Gain(gain) => {
                applied = [*gain; 2];
                for channel in block.iter_mut() {
                    channel[..len]
                        .iter_mut()
                        .for_each(|v| *v = kernels::gain(*v, *gain));
                }
            }
            ProcessorRef::StereoMatrix(m) => {
                let [left, right] = block;
                for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
                    [*l, *r] = kernels::matrix([*l, *r], *m);
                }
            }
            ProcessorRef::ControlGain(lane) => {
                let ramp = parameters[*lane];
                if TRACE {
                    applied = [0.; 2];
                }
                let [left, right] = block;
                for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
                    let gain = ramp.value(at + i as u64);
                    if TRACE {
                        for mean in &mut applied {
                            *mean += gain / len.max(1) as f64;
                        }
                    }
                    *l = kernels::gain(*l, gain);
                    *r = kernels::gain(*r, gain);
                }
            }
            ProcessorRef::Eq(eq) => eq.process(state, parameters, block, len, at),
            ProcessorRef::ControlMatrix(cells) => {
                live::matrix(cells, parameters, block, len, at)
            }
            ProcessorRef::Biquad(filter) => {
                let [mut zl, mut zr] = state.z;
                let [left, right] = block;
                for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
                    (*l, zl) = filter.sample(*l, zl);
                    (*r, zr) = filter.sample(*r, zr);
                }
                state.z = [zl.map(flush), zr.map(flush)];
            }
        }
        if TRACE {
            if let Some(t) = trace.as_mut() {
                if !matches!(
                    stage,
                    ProcessorRef::Gain(_)
                        | ProcessorRef::ControlGain(_)
                        | ProcessorRef::Mix { .. }
                        | ProcessorRef::Branch { .. }
                ) {
                    for c in 0..2 {
                        let a: f64 = input[c][..len].iter().map(|v| v * v).sum();
                        let b: f64 = block[c][..len].iter().map(|v| v * v).sum();
                        applied[c] = if a > 0. { (b / a).sqrt() } else { 0. };
                    }
                }
                t.record(
                    index,
                    &input,
                    trace_output.as_ref().unwrap_or(block),
                    len,
                    applied,
                    enabled,
                    parameters,
                );
            }
        }
    }
    fault
}

pub(super) fn allocate<T: Default>(count: usize) -> Result<Box<[T]>, Error> {
    std::alloc::Layout::array::<T>(count).map_err(|_| Error::Capacity)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| Error::Capacity)?;
    values.resize_with(count, T::default);
    Ok(values.into_boxed_slice())
}

/// Most render lanes: the audio thread and its workers.
pub(crate) const MAX_LANES: usize = 8;

pub(super) struct DspState {
    /// Voice slots `cells` and `delay_samples` are sized for.
    pub voices: usize,
    pub stride: usize,
    /// One `stride` of chain state per voice slot; claimed per voice.
    pub cells: Slab<ProcessorState>,
    pub owner_cells: Slab<ProcessorState>,
    pub owner_delays: Slab<[f64; 2]>,
    pub parameters: Box<[ControlRamp]>,
    pub modulated_parameters: Slab<ControlRamp>,
    pub delay_samples: Slab<[f64; 2]>,
    /// Filter coefficient caches, one per render lane. Lane 0 is the audio
    /// thread's (and the whole of single-threaded rendering).
    pub filters: Slab<svf::FilterBank>,
    pub buses: crate::bus::BusState,
    pub feeds: Box<[taps::TapFeed]>,
    pub trace: Option<crate::trace::Recorder>,
    /// Time-stretch engines, allocated only for a plan with stretch groups.
    pub stretch: Slab<crate::source::StretchEngine>,
    /// Bit per engine in use; touched by the audio writer at voice start/end.
    pub stretch_busy: u64,
}
impl DspState {
    pub fn new(
        plan: &Prepared,
        voices: usize,
        expressions: usize,
        lanes: usize,
    ) -> Result<Self, Error> {
        let (stride, delay_stride) = Self::shape(plan);
        let cells = stride.checked_mul(voices).ok_or(Error::Capacity)?;
        let delay_count = delay_stride.checked_mul(voices).ok_or(Error::Capacity)?;
        if plan
            .voice_chains
            .iter()
            .flat_map(|chain| &chain.tap_buses)
            .any(|bus| *bus >= plan.buses.len())
        {
            return Err(Error::InvalidInput);
        }
        let feeds = if plan.voice_chains.iter().any(|chain| !chain.taps.is_empty()) {
            plan.buses.len()
        } else {
            0
        };
        Ok(Self {
            voices,
            stride,
            cells: Slab::new(allocate(cells)?, stride),
            owner_cells: {
                let n = if plan.processing_scopes.is_empty() { 0 } else { voices };
                Slab::new(allocate(stride.checked_mul(n).ok_or(Error::Capacity)?)?, stride)
            },
            owner_delays: {
                let n = if plan.processing_scopes.is_empty() { 0 } else { voices };
                Slab::new(allocate(delay_stride.checked_mul(n).ok_or(Error::Capacity)?)?, delay_stride)
            },
            filters: Slab::new(
                (0..lanes.clamp(1, MAX_LANES))
                    .map(|_| svf::FilterBank::new(&plan.filters, expressions))
                    .collect::<Result<_, _>>()?,
                1,
            ),
            delay_samples: Slab::new(allocate(delay_count)?, delay_stride),
            modulated_parameters: {
                let stride = if plan.voice_modulation.has_control_targets() {
                    plan.dsp_bindings.len()
                } else {
                    0
                };
                Slab::new(
                    allocate(
                        stride
                            .checked_mul(lanes.clamp(1, MAX_LANES))
                            .ok_or(Error::Capacity)?,
                    )?,
                    stride,
                )
            },
            parameters: control::initial_parameters(plan, &plan.dsp_bindings),
            buses: crate::bus::BusState::new(plan)?,
            feeds: allocate(feeds)?,
            trace: plan
                .signal_trace
                .as_ref()
                .map(|t| t.recorder())
                .transpose()?,
            stretch: Slab::new(
                if plan.stretch_groups.iter().any(Option::is_some) {
                    (0..crate::source::STRETCH_VOICES)
                        .map(|_| crate::source::StretchEngine::with_capacity(plan.rate))
                        .collect()
                } else {
                    Box::new([])
                },
                1,
            ),
            stretch_busy: 0,
        })
    }
    /// Per-voice chain state and delay line sizes (`stride`, `delay_stride`).
    pub fn shape(plan: &Prepared) -> (usize, usize) {
        let stride = plan
            .voice_chains
            .iter()
            .map(|chain| chain.stages())
            .max()
            .unwrap_or(0);
        let delay = plan
            .voice_chains
            .iter()
            .map(|chain| chain.delay_frames)
            .max()
            .unwrap_or(0);
        (stride, delay)
    }
    /// Per-voice storage for `voices` slots of a plan of this `shape`.
    pub fn voice_storage(
        (stride, delay): (usize, usize),
        voices: usize,
    ) -> Result<(Slab<ProcessorState>, Slab<[f64; 2]>), Error> {
        let cells = stride.checked_mul(voices).ok_or(Error::Capacity)?;
        let delays = delay.checked_mul(voices).ok_or(Error::Capacity)?;
        Ok((
            Slab::new(allocate(cells)?, stride),
            Slab::new(allocate(delays)?, delay),
        ))
    }
    /// Take the larger per-voice storage `cells`/`delays` (from
    /// `voice_storage`), moving every live voice's state across; the old
    /// storage is left in the arguments. No allocation.
    pub fn adopt(
        &mut self,
        voices: usize,
        cells: &mut Slab<ProcessorState>,
        delays: &mut Slab<[f64; 2]>,
    ) {
        let n = self.cells.len();
        cells.as_mut_slice()[..n].swap_with_slice(self.cells.as_mut_slice());
        std::mem::swap(&mut self.cells, cells);
        let n = self.delay_samples.len();
        delays.as_mut_slice()[..n].swap_with_slice(self.delay_samples.as_mut_slice());
        std::mem::swap(&mut self.delay_samples, delays);
        self.voices = voices;
    }
    /// Make sure there is a filter cache for each of `lanes` render lanes.
    /// Control side: allocates.
    pub fn ensure_lanes(
        &mut self,
        plan: &Prepared,
        expressions: usize,
        lanes: usize,
    ) -> Result<(), Error> {
        let lanes = lanes.clamp(1, MAX_LANES);
        if plan.voice_modulation.has_control_targets() {
            let stride = plan.dsp_bindings.len();
            let size = stride.checked_mul(lanes).ok_or(Error::Capacity)?;
            if self.modulated_parameters.len() < size {
                self.modulated_parameters = Slab::new(allocate(size)?, stride);
            }
        }
        if self.filters.len() >= lanes {
            return Ok(());
        }
        let empty = Slab::new(Box::new([]), 1);
        let mut banks = std::mem::replace(&mut self.filters, empty)
            .into_items()
            .into_vec();
        while banks.len() < lanes {
            banks.push(svf::FilterBank::new(&plan.filters, expressions)?);
        }
        self.filters = Slab::new(banks.into_boxed_slice(), 1);
        Ok(())
    }
    pub fn reset(&mut self, voice: usize) {
        let stride = self.stride;
        self.cells.as_mut_slice()[voice * stride..(voice + 1) * stride]
            .fill(ProcessorState::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Attribution prototype only: production admission still uses DspState::reset.
    fn reset_owned_candidate(stage: &PreparedProcessor, state: &mut ProcessorState) {
        state.z = [[0.; 2]; 2];
        state.aux = [0.; 16];
        state.delay_position = 0;
        state.delay_filled = 0;
        if matches!(stage, PreparedProcessor::Color { .. } | PreparedProcessor::Filter { .. }) {
            state.color = Default::default();
        }
    }

    #[test]
    #[ignore = "optimized voice-admission reset footprint attribution, not callback timing"]
    fn voice_reset_stage_evidence() {
        use std::{hint::black_box, time::Instant};
        let mut processors = Vec::new();
        for (frequency, gain) in [(0.3, 6.), (0.55, -4.), (0.7, 3.)] {
            let mut band = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, frequency, 0.5, gain);
            band.law = sampler_ir::fx::eq::Law::KontaktLegacy;
            processors.push(Processor::Eq(band));
        }
        processors.push(Processor::Color(ColorSettings::stereo(StereoSettings {
            width: Parameter::Constant(0.7), pan: Parameter::Constant(0.2), pseudo: false,
        })));
        let chain = VoiceChain::new(Vec::new(), processors, 128).unwrap()
            .compile(48_000, &mut Vec::new(), &mut svf::FilterRegistryBuilder::default()).unwrap();
        let stages: Vec<_> = chain.pre.iter().chain(&chain.post).collect();
        eprintln!("voice_reset ProcessorState_bytes={} owned_stages={}",
            size_of::<ProcessorState>(), stages.len());
        for stride in [stages.len(), stages.len() * 4] {
            let mut cells = vec![ProcessorState::default(); 300 * stride];
            let loops = ((128 << 20) / (cells.len() * size_of::<ProcessorState>())).clamp(16, 2000);
            for pair in 0..4 {
                for candidate in if pair % 2 == 0 { [0, 1, 2] } else { [2, 1, 0] } {
                    let start = Instant::now();
                    for _ in 0..loops {
                        for states in black_box(&mut cells[..]).chunks_exact_mut(stride) {
                            match candidate {
                                0 => states.fill(ProcessorState::default()),
                                1 => states[..stages.len()].fill(ProcessorState::default()),
                                _ => for (stage, state) in stages.iter().zip(states) {
                                    reset_owned_candidate(stage, state);
                                },
                            }
                        }
                    }
                    black_box(&mut cells);
                    eprintln!("voice_reset stride={stride} pair={pair} candidate={candidate} voices=300 loops={loops} us={}",
                        start.elapsed().as_micros());
                }
            }
        }
    }

    #[test]
    #[ignore = "optimized, isolated validation-stage evidence; not a full callback benchmark"]
    fn finite_guard_stage_evidence() {
        use std::{hint::black_box, time::Instant};
        let mut processors = Vec::new();
        for (frequency, gain) in [(0.3, 6.), (0.55, -4.), (0.7, 3.)] {
            let mut band = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, frequency, 0.5, gain);
            band.law = sampler_ir::fx::eq::Law::KontaktLegacy;
            processors.push(Processor::Eq(band));
        }
        processors.push(Processor::Color(ColorSettings::stereo(StereoSettings {
            width: Parameter::Constant(0.7), pan: Parameter::Constant(0.2), pseudo: false,
        })));
        let chain = VoiceChain::new(Vec::new(), processors, 128).unwrap()
            .compile(48_000, &mut Vec::new(), &mut svf::FilterRegistryBuilder::default()).unwrap();
        let mut cells = vec![ProcessorState::default(); 300 * chain.stages()];
        for (i, cell) in cells.iter_mut().enumerate() {
            cell.z = [[(i as f64 * 0.37).sin() * 0.8, 0.13]; 2];
        }
        let original = |states: &[ProcessorState]| states.iter().all(|s| {
            s.z.iter().flatten().chain(&s.aux).all(|v| v.is_finite()) && s.color.finite()
        });
        assert!(original(&cells));
        cells[17].aux[3] = f64::NAN;
        assert!(!original(&cells));
        assert!(!finite_states(chain.pre.iter().chain(&chain.post), &cells[16..20]));
        cells[17].aux[3] = 0.;
        for repeat in 0..4 {
            for scoped in if repeat % 2 == 0 { [false, true] } else { [true, false] } {
                let start = Instant::now();
                for _ in 0..2_000 {
                    for states in black_box(&cells).chunks_exact(chain.stages()) {
                        let valid = if scoped {
                            finite_states(black_box(chain.pre.iter().chain(&chain.post)), black_box(states))
                        } else { original(black_box(states)) };
                        assert!(black_box(valid));
                    }
                }
                eprintln!("FINITE_GUARD_EVIDENCE repeat={repeat} scoped={scoped} voices=300 stages=4 checks=600000 elapsed_us={}", start.elapsed().as_micros());
            }
        }
    }

    #[test]
    fn stage_guards_keep_common_histories_and_reject_state_shape_mismatch() {
        let stage = PreparedProcessor::Gain(1.);
        let mut state = ProcessorState::default();
        assert!(finite_states([&stage], &[state]));
        assert!(!finite_states([&stage], &[]));
        assert!(!finite_states(std::iter::empty::<ProcessorRef<'_>>(), &[state]));
        state.z[1][0] = f64::NAN;
        assert!(!finite_states([&stage], &[state]));
        state = ProcessorState::default();
        state.aux[15] = f64::INFINITY;
        assert!(!finite_states([&stage], &[state]));
    }

    #[test]
    fn native_gain_and_non_pseudo_stereo_have_lane_kernels() {
        for pseudo in [false, true] {
            let chain = VoiceChain::new(
                vec![Processor::Color(ColorSettings {
                    gainer: Some(GainerSettings { gain: Parameter::Constant(0.8), dry: 0.1 }),
                    stereo: Some(StereoSettings {
                        width: Parameter::Constant(0.7),
                        pan: Parameter::Constant(-0.2),
                        pseudo,
                    }),
                    ..ColorSettings::IDENTITY
                })],
                Vec::new(),
                0,
            )
            .unwrap()
            .compile(48000, &mut Vec::new(), &mut svf::FilterRegistryBuilder::default())
            .unwrap();
            let chains = PreparedChains::from_compiled(vec![chain]);
            assert_eq!(chains[0].batch_index().is_some(), !pseudo);
        }
    }

    /// One frame through `stages` as a one-frame block.
    fn process(
        stages: &[PreparedProcessor],
        states: &mut [ProcessorState],
        value: [f64; 2],
        parameters: &[ControlRamp],
        at: u64,
        delay: &mut [[f64; 2]],
        filters: &mut svf::FilterContext<'_>,
    ) -> [f64; 2] {
        let mut block = [[0.; BLOCK]; 2];
        (block[0][0], block[1][0]) = (value[0], value[1]);
        assert!(!super::process::<false>(
            stages, states, &mut block, 1, parameters, at, delay, filters, None
        ));
        [block[0][0], block[1][0]]
    }

    #[test]
    fn first_order_shelves_reach_their_gain_one_band_away() {
        let gain = 10_f64.powf(0.6);
        let low = Biquad::new(48000, FilterKind::FirstOrderLowShelf { gain_db: 12. }, 1000., 0.7).unwrap();
        let high = Biquad::new(48000, FilterKind::FirstOrderHighShelf { gain_db: 12. }, 1000., 0.7).unwrap();
        assert!((low.magnitude(1.) - gain).abs() < 1e-3, "{}", low.magnitude(1.));
        assert!((low.magnitude(23999.) - 1.).abs() < 1e-2);
        assert!((high.magnitude(1.) - 1.).abs() < 1e-3);
        assert!((high.magnitude(23999.) - gain).abs() < 1e-2, "{}", high.magnitude(23999.));
        // Six dB per octave: a decade above the corner is within 1 dB of its asymptote.
        assert!((low.magnitude(10000.) - 1.).abs() < 0.2);
    }

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
                let mut state = [ProcessorState::default()];
                let mut filters = svf::FilterBank::new(&svf::PreparedFilters::default(), 0).unwrap();
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
                        &mut [],
                        &mut svf::FilterContext {
                            tempo: 120.0,
                            bank: &mut filters,
                            spaces: &mut [],
                            expression: None,
                        },
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
                            let mut state = [ProcessorState::default(); 2];
                            let mut filters = svf::FilterBank::new(&svf::PreparedFilters::default(), 0).unwrap();
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
                                    &mut [],
                                    &mut svf::FilterContext {
                                        tempo: 120.0,
                                        bank: &mut filters,
                                        spaces: &mut [],
                                        expression: None,
                                    },
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

impl PreparedVoiceChain {
    /// Three voice boundaries, one node per compiled processor and per tap.
    pub(crate) fn trace_node_count(&self) -> Option<usize> {
        3usize.checked_add(self.stage_count)?.checked_add(self.taps.len())
    }

}

impl PreparedChainRef<'_> {
    pub(crate) fn trace_graph(
        &self,
        plan: &Prepared,
        graph: &mut crate::trace::TraceGraph,
        zone: u32,
        group: Option<u32>,
        bus: Option<usize>,
        initial: &[ControlRamp],
    ) -> crate::trace::VoiceNodes {
        let source = graph.node(
            "sample_source",
            "resampler",
            Some(zone),
            group,
            bus,
            vec![],
            0,
        );
        let amp = graph.node(
            "amplifier",
            "envelope_velocity_gain",
            Some(zone),
            group,
            bus,
            vec![],
            0,
        );
        let output = graph.node(
            "voice_output",
            "expression_fade",
            Some(zone),
            group,
            bus,
            vec![],
            0,
        );
        let pre: Vec<_> = self
            .pre
            .iter()
            .map(|s| {
                graph.stage(
                    s,
                    "group_fx_pre",
                    Some(zone),
                    group,
                    bus,
                    plan,
                    &plan.dsp_bindings,
                    initial,
                )
            })
            .collect();
        let post: Vec<_> = self
            .post
            .iter()
            .map(|s| {
                graph.stage(
                    s,
                    "group_fx_post",
                    Some(zone),
                    group,
                    bus,
                    plan,
                    &plan.dsp_bindings,
                    initial,
                )
            })
            .collect();
        let parent = graph.connect(self.pre, &pre, source);
        graph.edge(parent, amp, "serial");
        let parent = graph.connect(self.post, &post, amp);
        graph.edge(parent, output, "serial");
        graph.edge(
            output,
            bus.map_or(graph.master, |b| graph.buses[b].input),
            "sum",
        );
        let taps = self
            .taps
            .iter()
            .map(|tap| {
                let (kind, parent) = match tap.position {
                    VoiceSendPosition::BeforeAmplitude(n) => {
                        ("send_pre", if n == 0 { source } else { pre[n - 1] })
                    }
                    VoiceSendPosition::AfterAmplitude(n) => {
                        ("send_post", if n == 0 { amp } else { post[n - 1] })
                    }
                };
                let parameters = vec![
                    graph.parameter("level", tap.gain, plan, &plan.dsp_bindings, initial),
                    graph.parameter("bypass", tap.bypass, plan, &plan.dsp_bindings, initial),
                ];
                let id = graph.node(
                    kind,
                    "send_level_bypass",
                    Some(zone),
                    group,
                    Some(tap.bus),
                    parameters,
                    0,
                );
                graph.edge(parent, id, "tap");
                graph.edge(id, graph.buses[tap.bus].input, "send");
                id
            })
            .collect();
        crate::trace::VoiceNodes {
            source,
            amp,
            output,
            pre,
            post,
            taps,
            region: 0,
        }
    }
}

#[cfg(test)]
mod typed_batch_owner_tests {
    use super::*;

    fn native_band() -> Processor {
        let mut band = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak,
            f64::from_bits(0x3fe0_0000_0000_0001), 0.3456789012345, 9.1234567898);
        band.law = sampler_ir::fx::eq::Law::KontaktLegacy;
        band.enabled = Parameter::Constant(f64::from_bits(0x3fe0_0000_0000_0001));
        Processor::Eq(band)
    }
    fn compile(pre: Vec<Processor>, post: Vec<Processor>) -> CompiledVoiceChain {
        VoiceChain::new(pre, post, 31).unwrap()
            .compile(48_000, &mut Vec::new(), &mut svf::FilterRegistryBuilder::default()).unwrap()
    }
    fn color() -> Processor {
        Processor::Color(ColorSettings { curve: CurveSettings::Classic(Parameter::Constant(0.3)),
            gainer: Some(GainerSettings { gain: Parameter::Constant(0.7), dry: 0.2 }),
            ..ColorSettings::IDENTITY })
    }
    fn scalar_matrix() -> Processor {
        Processor::ControlMatrix([Parameter::Constant(1.), Parameter::Constant(0.),
            Parameter::Constant(0.), Parameter::Constant(1.)])
    }
    fn eq_constants(chain: PreparedChainRef<'_>) -> Vec<u64> {
        chain.pre.iter().filter_map(|stage| match stage { ProcessorRef::Eq(eq) => Some(eq), _ => None })
            .flat_map(|eq| eq.trace_parameters()).map(|(_, p)| match p {
                control::PreparedParameter::Constant(value) => value.to_bits(),
                _ => panic!("constant fixture changed parameter kind"),
            }).collect()
    }

    #[test]
    fn dense_batch_owner_is_retained_when_logical_chain_indices_differ() {
        let chains = PreparedChains::from_compiled(vec![
            compile(vec![scalar_matrix()], vec![]),
            compile(vec![native_band(), color()], vec![]),
            compile(vec![Processor::Eq(EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, 1100., 0.7, 2.))], vec![]),
            compile(vec![Processor::Gain(0.5)], vec![]),
        ]);
        assert!(chains[0].batch_index().is_none());
        assert!(chains[2].batch_index().is_none());
        let first = chains[1].batch_index().unwrap();
        let last = chains[3].batch_index().unwrap();
        assert_eq!([first.0, last.0], [0, 1]);
        assert!(std::ptr::eq(chains.batch(first).common, &chains[1]));
        assert!(std::ptr::eq(chains.batch(last).common, &chains[3]));
        assert_eq!([chains.scalar.len(), chains.batch.len()], [2, 2]);
    }

    /// A late scalar-only operation moves the earlier Color and constant EQ back
    /// to scalar ownership. It must retain their original f64 trace identities.
    #[test]
    fn late_scalar_classification_preserves_constant_native_eq_bits() {
        let chains = PreparedChains::from_compiled(vec![
            compile(vec![native_band(), color()], vec![]),
            compile(vec![native_band(), color()], vec![scalar_matrix()]),
        ]);
        assert!(chains[0].batch_index().is_some());
        assert!(chains[1].batch_index().is_none());
        let expected = [f64::from_bits(0x3fe0_0000_0000_0001), 0.3456789012345,
            9.1234567898, f64::from_bits(0x3fe0_0000_0000_0001)].map(f64::to_bits);
        assert_eq!(eq_constants(chains.view(0)), expected);
        assert_eq!(eq_constants(chains.view(1)), expected);
        assert!(matches!(chains.view(0).pre.get(1), ProcessorRef::Color { .. }));
        assert!(matches!(chains.view(1).pre.get(1), ProcessorRef::Color { .. }));
    }

    #[test]
    fn dynamic_native_eq_keeps_scalar_ownership() {
        const ID: crate::ControlId = crate::ControlId(0xbeef);
        let mut band = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, 0.5, 0.5, 6.);
        band.law = sampler_ir::fx::eq::Law::KontaktLegacy;
        band.frequency = Parameter::Control(ControlRange { control: ID, low: 0., high: 1., ramp_frames: 32 });
        let chains = PreparedChains::from_compiled(vec![compile(vec![Processor::Eq(band)], vec![])]);
        assert!(chains[0].batch_index().is_none());
        assert!(matches!(chains.view(0).pre.get(0), ProcessorRef::Eq(eq_family::EqRef::Scalar(_))));
    }

    #[test]
    fn typed_batch_owner_layout_receipt() {
        use std::mem::size_of;
        eprintln!("typed batch layouts: scalar_op={} batch_op={} chain={} owner={} scalar_lists={} batch_lists={} registry={} ref={} batch_index={} state={}",
            size_of::<PreparedProcessor>(), size_of::<BatchOp>(), size_of::<PreparedVoiceChain>(),
            size_of::<ChainOwner>(), size_of::<ScalarOps>(), size_of::<BatchOps>(), size_of::<PreparedChains>(),
            size_of::<PreparedChainRef<'_>>(), size_of::<BatchIndex>(), size_of::<ProcessorState>());
    }
}

impl PreparedChainRef<'_> {
    pub(super) fn owner_tail_frames(&self) -> u32 { self.tail_frames }

    /// Execute one shared section once over its ordered source sum. Existing
    /// typed ProcessorSlice and native history owners are retained verbatim.
    pub(super) fn render_owner(
        &self, envelope: &mut EnvelopeState, input: &mut [Frame],
        states: &mut [ProcessorState], mut context: RenderContext<'_>,
    ) -> bool {
        let len = input.len();
        let mut block = [[0.; BLOCK]; 2];
        for (i, frame) in input.iter().enumerate() {
            block[0][i] = f64::from(frame[0]); block[1][i] = f64::from(frame[1]);
        }
        let (pre, post) = states.split_at_mut(self.pre.len());
        let mut fault = self.process_section::<false>(true, pre, &mut block, len, 0, context.at, &mut context);
        for i in 0..len {
            let gain = f64::from(envelope.constant_level().unwrap_or_else(|| envelope.next()));
            block[0][i] *= gain; block[1][i] *= gain;
        }
        fault |= self.process_section::<false>(false, post, &mut block, len, 0, context.at, &mut context);
        fault |= !finite_states(self.pre.iter().chain(self.post.iter()), states);
        for (i, frame) in input.iter_mut().enumerate() {
            for c in 0..2 {
                frame[c] = (block[c][i] * f64::from(context.expression[c])) as f32;
                fault |= !frame[c].is_finite();
            }
        }
        if fault { states.fill(ProcessorState::default()); input.fill([0.; 2]); }
        fault
    }
}
