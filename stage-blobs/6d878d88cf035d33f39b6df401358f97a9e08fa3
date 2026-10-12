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
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    rate: u32,
    b: [f64; 3],
    a: [f64; 2],
}
impl Biquad {
    pub(crate) fn trace_coefficients(&self) -> [f64; 5] { [self.b[0],self.b[1],self.b[2],self.a[0],self.a[1]] }
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
    ControlGain(ControlRange),
    Delay(Delay),
    /// Stereo compressor; the smoothed reduction lives in the stage's state.
    Compressor(CompressorSettings),
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
    /// Kontakt Gainer (DSP_SYSTEM_INVENTORY "Gainer", KONTAKT_REFERENCE s.25):
    /// `x * (dry + g)` where `g` follows `gain` through a one-pole using the
    /// native per-frame coefficient, starting at its first target. Constant
    /// or control targets only; per-voice scalar path.
    Gainer {
        dry: f64,
        gain: Parameter,
    },
    /// Native Stereo Modeller width/balance smoothing and optional right delay.
    StereoModeller(StereoSettings),
    /// Kontakt Daft filter; per-voice scalar path.
    Daft(DaftSettings),
    /// Pinned v1 native Ladder LP4, with separate preallocated family state.
    LadderLP4(LadderSettings),
    /// WaveShaper rectification (stateless).
    Rectify(Rectifier),
    /// Formant Crusher decimation; per-voice scalar path.
    Decimate(Decimator),
    StateVariable(StateVariableFilter),
    /// Stereo reverb; bus scope only (it owns megabytes of state).
    Reverb(ReverbSettings),
    /// `dry * x + wet * (x * impulse)`; bus scope only. `impulse` indexes
    /// the table given to [`crate::Prepared::with_impulses`].
    Convolution {
        impulse: usize,
        dry: f64,
        wet: f64,
    },
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

impl Processor {
    fn valid(&self) -> bool {
        match self {
            Processor::Gain(gain) => gain.is_finite(),
            Processor::StereoMatrix(matrix) => matrix.iter().flatten().all(|v| v.is_finite()),
            Processor::ControlGain(binding) => binding.valid(),
            Processor::StateVariable(filter) => filter.valid(),
            Processor::Reverb(settings) => settings.valid(),
            Processor::Convolution { dry, wet, .. } => dry.is_finite() && wet.is_finite(),
            Processor::Mix {
                dry, wet, bypass, ..
            } => dry.valid() && wet.valid() && bypass.valid(),
            Processor::Compressor(settings) => settings.valid(),
            Processor::Decimate(decimator) => decimator.valid(),
            Processor::Daft(settings) => settings.valid(),
            Processor::LadderLP4(settings) => settings.valid(),
            Processor::StereoModeller(settings) => settings.valid(),
            Processor::Branch { gain, .. } => gain.is_finite(),
            Processor::Rectify(_) => true,
            Processor::Gainer { dry, gain } => {
                dry.is_finite() && gain.valid() && !matches!(gain, Parameter::Expression { .. })
            }
            Processor::Biquad(_) | Processor::Delay(_) => true,
        }
    }
}

mod compressor;
pub(super) mod control;
mod convolution;
mod daft;
mod ladder_kernel;
mod ladder;
pub use ladder::LadderSettings;
mod delay;
mod stereo;
mod taps;
pub use stereo::StereoSettings;
pub use taps::{VoiceSendPosition, VoiceSendTap};
pub(super) mod lanes;
mod reverb;
mod shaping;
pub(super) mod svf;
pub use compressor::CompressorSettings;
pub(super) use control::ControlRamp;
pub use control::{ControlRange, Parameter};
pub use convolution::ConvolutionUpload;
pub(super) use convolution::{Convolution, tail_frames as impulse_tail_frames};
pub use convolution::{Impulse, MAX_IMPULSE_FRAMES};
pub use daft::DaftSettings;
pub use delay::Delay;
pub(super) use reverb::Reverb;
pub use reverb::ReverbSettings;
pub use shaping::{Decimator, Rectifier};
pub use svf::{StateVariableFilter, SvfMode};

pub(super) enum PreparedProcessor {
    Gain(f64),
    StereoMatrix([[f64; 2]; 2]),
    Biquad(Biquad),
    ControlGain(usize),
    Delay {
        delay: Delay,
        offset: usize,
    },
    Compressor(compressor::Compressor),
    Rectify(Rectifier),
    Gainer {
        dry: f64,
        gain: control::PreparedParameter,
        /// Per-sample one-pole coefficient.
        k: f64,
    },
    Decimate(Decimator),
    Daft(daft::Daft),
    LadderLP4 { ladder: ladder::Ladder, offset: usize },
    StereoModeller {
        stereo: stereo::Stereo,
        offset: usize,
    },
    Branch {
        count: u16,
        gain: f64,
        first: bool,
        last: bool,
    },
    StateVariable(usize),
    /// Index into the bus graph's reverbs.
    Reverb(usize),
    /// Index into the bus graph's convolutions.
    Convolution(usize),
    /// `count` following stages in parallel with the dry signal; the lanes are
    /// dry, wet and bypass.
    Mix {
        count: u16,
        lanes: [usize; 3],
    },
}

pub(super) struct PreparedVoiceChain {
    pre: Box<[PreparedProcessor]>,
    post: Box<[PreparedProcessor]>,
    tail_frames: u32,
    pub delay_frames: usize,
    taps: Box<[taps::PreparedTap]>,
    pub tap_buses: Box<[usize]>,
}

pub(super) struct RenderContext<'a> {
    pub amplifier: Option<crate::voice_mod::Ramp>,
    pub expression: Frame,
    pub delay: &'a mut [[f64; 2]],
    pub parameters: &'a [ControlRamp],
    pub filters: svf::FilterContext<'a>,
    pub at: u64,
    pub feeds: &'a mut [taps::TapFeed],
    pub trace: Option<crate::trace::VoiceTrace<'a>>,
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
        filters: &mut Vec<svf::PreparedFilter>,
    ) -> Result<PreparedVoiceChain, Error> {
        let mut delay_frames = 0;
        let mut tap_buses: Vec<_> = self.taps.iter().map(|tap| tap.bus).collect();
        tap_buses.sort_unstable();
        tap_buses.dedup();
        Ok(PreparedVoiceChain {
            pre: compile_processors(
                self.pre,
                rate,
                bindings,
                &mut delay_frames,
                filters,
                None,
                None,
            )?,
            post: compile_processors(
                self.post,
                rate,
                bindings,
                &mut delay_frames,
                filters,
                None,
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
        })
    }
}
pub(super) fn compile_processors(
    stages: Box<[Processor]>,
    rate: u32,
    bindings: &mut Vec<ControlRange>,
    delay_frames: &mut usize,
    filters: &mut Vec<svf::PreparedFilter>,
    mut reverbs: Option<&mut Vec<(ReverbSettings, u32)>>,
    mut convolutions: Option<&mut Vec<(usize, f64, f64)>>,
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
                    let index = filters.len();
                    filters.push(filter.compile(rate, bindings)?);
                    PreparedProcessor::StateVariable(index)
                }
                Processor::Delay(delay) => {
                    let offset = *delay_frames;
                    *delay_frames = offset
                        .checked_add(delay.frames as usize)
                        .ok_or(Error::Capacity)?;
                    PreparedProcessor::Delay { delay, offset }
                }
                Processor::Compressor(settings) => {
                    PreparedProcessor::Compressor(settings.prepare(rate))
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
                Processor::Daft(settings) => {
                    PreparedProcessor::Daft(settings.compile(rate, bindings))
                }
                Processor::LadderLP4(settings) => {
                    let offset = *delay_frames;
                    *delay_frames = offset.checked_add(ladder::CELLS).ok_or(Error::Capacity)?;
                    PreparedProcessor::LadderLP4 { ladder: settings.compile(rate, bindings), offset }
                }
                Processor::Rectify(mode) => PreparedProcessor::Rectify(mode),
                Processor::Gainer { dry, gain } => PreparedProcessor::Gainer {
                    dry,
                    gain: gain.compile(bindings),
                    k: f64::from(f32::from_bits(0x3a11a2b4)),
                },
                Processor::StereoModeller(settings) => {
                    let offset = *delay_frames;
                    if settings.pseudo {
                        *delay_frames = offset.checked_add(1024).ok_or(Error::Capacity)?;
                    }
                    PreparedProcessor::StereoModeller {
                        stereo: settings.compile(rate, bindings),
                        offset,
                    }
                }
                Processor::Decimate(decimator) => PreparedProcessor::Decimate(decimator),
                Processor::Reverb(settings) => {
                    let reverbs = reverbs.as_deref_mut().ok_or(Error::InvalidInput)?;
                    reverbs.push((settings, 0));
                    PreparedProcessor::Reverb(reverbs.len() - 1)
                }
                Processor::Convolution { impulse, dry, wet } => {
                    let all = convolutions.as_deref_mut().ok_or(Error::InvalidInput)?;
                    all.push((impulse, dry, wet));
                    PreparedProcessor::Convolution(all.len() - 1)
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
    pub(crate) fn filter_indices(&self, stages: std::ops::Range<usize>) -> Vec<u32> {
        self.pre
            .iter()
            .chain(&self.post)
            .enumerate()
            .filter_map(|(i, p)| match p {
                PreparedProcessor::StateVariable(filter) if stages.contains(&i) => {
                    u32::try_from(*filter).ok()
                }
                _ => None,
            })
            .collect()
    }
    pub(super) fn stages(&self) -> usize {
        self.pre.len() + self.post.len()
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
            let Some(begun) = self.begin(voice, pcm, chunk.len(), kernel, &mut block) else {
                break;
            };
            let len = begun.len;
            if begun.held == len {
                rendered += len;
                continue;
            }
            if TRACE { if let Some(t) = context.trace.as_mut() {
                t.identity.source_metrics=crate::trace::metrics(&block,len);
                t.record(t.nodes.source, &block, &block, len, [1.; 2], context.parameters);
            } }
            let at = context.at + (chunk_index * BLOCK) as u64;
            let (pre, post) = states.split_at_mut(self.pre.len());
            let mut fault = self.process_section::<TRACE>(true, pre, &mut block, len, begun.held, at, &mut context);
            let levels = levels(voice, len, begun.held);
            let before_amp = if TRACE { block } else { [[0.; BLOCK]; 2] };
            let [left, right] = &mut block;
            for (i, ((l, r), level)) in left[..len].iter_mut().zip(&mut right[..len]).zip(&levels).enumerate() {
                let gains = context.amplifier.map_or([1.0; 2], |r| r.gains_at(at + i as u64 + 1));
                *l *= level * f64::from(gains[0]);
                *r *= level * f64::from(gains[1]);
            }
            if TRACE { if let Some(t) = context.trace.as_mut() {
                t.record(t.nodes.amp, &before_amp, &block, len,
                    std::array::from_fn(|c| levels[..len].iter().enumerate().map(|(i,l)| l * f64::from(context.amplifier.map_or([1.;2],|r|r.gains_at(at+i as u64+1))[c])).sum::<f64>() / len.max(1) as f64), context.parameters);
            } }
            fault |= self.process_section::<TRACE>(false, post, &mut block, len, begun.held, at, &mut context);
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
            if TRACE { if let Some(t) = context.trace.as_mut() {
                let mut final_block = block;
                for c in 0..2 { for i in begun.held..len {
                    let fade = voice.dsp_fade.map_or(1., |(total, initial)| {
                        // Held startup frames do not consume the fade clock.
                        let remaining = voice.tail_remaining.unwrap_or(0) + (len - i) as u32;
                        f64::from(initial) * f64::from(remaining) / f64::from(total)
                    });
                    final_block[c][i] *= f64::from(context.expression[c]) * fade;
                    if finish_fault { final_block[c][i] = 0.; }
                } }
                t.record(t.nodes.output, &block, &final_block, len, context.expression.map(f64::from), context.parameters);
                if t.graph.nodes[t.nodes.output].bus.is_none() {
                    t.record(t.graph.master, &final_block, &final_block, len, [1.; 2], context.parameters);
                }
            } }
            rendered += len;
        }
        (rendered, faults)
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
    ) -> Option<Begun> {
        if self.done(voice) {
            return None;
        }
        let mut raw = [[0.; 2]; BLOCK];
        let waiting = voice.cursor.waiting();
        let active = voice.envelope.remaining()
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
        } else {
            voice.cursor.render(
                pcm,
                &mut raw[..count],
                &mut unity,
                1.0,
                [1.; 2],
                kernel,
            )
        };
        let held = if waiting { produced - (remaining - unity.remaining()) } else { 0 };
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
            || !states.iter().all(ProcessorState::finite);
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

    /// Whether every stage has a lane kernel (delay lines and compressors stay per voice).
    pub(super) fn batches(&self) -> bool {
        self.taps.is_empty()
            && !self.pre.iter().chain(&self.post).any(|stage| {
                matches!(
                    stage,
                    PreparedProcessor::Delay { .. }
                        | PreparedProcessor::Compressor(_)
                        | PreparedProcessor::Decimate(_)
                        | PreparedProcessor::Daft(_)
                        | PreparedProcessor::LadderLP4 { .. }
                        | PreparedProcessor::Branch { .. }
                ) || matches!(stage, PreparedProcessor::StereoModeller { stereo, .. } if !stereo.batches())
            })
    }

    pub(super) fn pre(&self) -> &[PreparedProcessor] {
        &self.pre
    }

    pub(super) fn post(&self) -> &[PreparedProcessor] {
        &self.post
    }

    pub(super) fn done(&self, voice: &Voice) -> bool {
        voice.tail_remaining == Some(0)
            || (self.tail_frames == 0 && (voice.cursor.done() || voice.envelope.done()))
    }
}

/// One voice block's source output, settled by [`PreparedVoiceChain::begin`].
#[derive(Clone, Copy)]
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
        *level = f64::from(voice.gain) * f64::from(
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
}
impl ProcessorState {
    pub(super) fn finite(&self) -> bool {
        self.z
            .iter()
            .flatten()
            .chain(&self.aux)
            .all(|v| v.is_finite())
    }
}

/// Run `len` frames of `block` through each stage in turn. Returns whether a
/// stage observed a nonfinite value it does not keep in visible state.
#[allow(clippy::too_many_arguments)]
pub(super) fn process<const TRACE: bool>(
    stages: &[PreparedProcessor],
    states: &mut [ProcessorState],
    block: &mut Planar,
    len: usize,
    parameters: &[ControlRamp],
    at: u64,
    delay_samples: &mut [[f64; 2]],
    filters: &mut svf::FilterContext<'_>,
    mut trace: Option<crate::trace::Section<'_>>,
) -> bool {
    let mut fault = false;
    let mut next = 0;
    // The entering signal and the running sum of the rack branch in progress.
    let mut split: Option<(Planar, Planar)> = None;
    while next < stages.len() {
        let (stage, index) = (&stages[next], next);
        next += 1;
        let state = &mut states[index];
        let input = if TRACE { *block } else { [[0.; BLOCK]; 2] };
        let mut applied = [1.; 2];
        let mut trace_output = None;
        let mut enabled = true;
        match stage {
            PreparedProcessor::Branch {
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
                    &stages[inner.clone()],
                    &mut states[inner.clone()],
                    block,
                    len,
                    parameters,
                    at,
                    delay_samples,
                    filters,
                    if TRACE { trace.as_mut().map(|t| crate::trace::Section {
                        recorder: &mut *t.recorder, graph: t.graph, nodes: &t.nodes[inner.clone()], identity: t.identity }) } else { None },
                );
                if let Some((entering, sum)) = split.as_mut() {
                    for c in 0..2 {
                        for i in 0..len {
                            sum[c][i] += gain * block[c][i];
                        }
                    }
                    if TRACE { trace_output = Some(*sum); applied = [*gain; 2]; }
                    *block = if *last { *sum } else { *entering };
                }
                if *last {
                    split = None;
                }
            }
            PreparedProcessor::Mix { count, lanes } => {
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
                if TRACE && off { if let Some(t) = trace.as_mut() {
                    for skipped in inner.clone() { t.record(skipped, &dry_block, &dry_block, len, [1.; 2], false, parameters); }
                } }
                if !off {
                    fault |= process::<TRACE>(
                        &stages[inner.clone()],
                        &mut states[inner.clone()],
                        block,
                        len,
                        parameters,
                        at,
                        delay_samples,
                        filters,
                        if TRACE { trace.as_mut().map(|t| crate::trace::Section {
                            recorder: &mut *t.recorder, graph: t.graph, nodes: &t.nodes[inner.clone()], identity: t.identity }) } else { None },
                    );
                }
                for c in 0..2 {
                    for i in 0..len {
                        let t = at + i as u64;
                        let b = bypass.value(t);
                        let wet_part = if off {
                            0.
                        } else {
                            wet.value(t) * (1. - b) * block[c][i]
                        };
                        block[c][i] = (dry.value(t) * (1. - b) + b) * dry_block[c][i] + wet_part;
                    }
                }
            }
            PreparedProcessor::StateVariable(index) => {
                filters.process(*index, &mut state.z, block, len, parameters, at);
            }
            PreparedProcessor::Reverb(index) => {
                let mut wet = [[0f32; BLOCK]; 2];
                for (w, v) in wet.iter_mut().zip(block.iter()) {
                    w[..len]
                        .iter_mut()
                        .zip(&v[..len])
                        .for_each(|(w, v)| *w = *v as f32);
                }
                let [left, right] = &mut wet;
                filters.reverbs[*index].process(&mut left[..len], &mut right[..len]);
                for (v, w) in block.iter_mut().zip(&wet) {
                    v[..len]
                        .iter_mut()
                        .zip(&w[..len])
                        .for_each(|(v, w)| *v = f64::from(*w));
                }
            }
            PreparedProcessor::Convolution(index) => {
                let mut wet = [[0f32; BLOCK]; 2];
                for (w, v) in wet.iter_mut().zip(block.iter()) {
                    w[..len]
                        .iter_mut()
                        .zip(&v[..len])
                        .for_each(|(w, v)| *w = *v as f32);
                }
                let [left, right] = &mut wet;
                filters.convolutions[*index].process([&mut left[..len], &mut right[..len]]);
                for (v, w) in block.iter_mut().zip(&wet) {
                    v[..len]
                        .iter_mut()
                        .zip(&w[..len])
                        .for_each(|(v, w)| *v = f64::from(*w));
                }
            }
            PreparedProcessor::Delay { delay, offset } => {
                fault |= delay.process(
                    state,
                    &mut delay_samples[*offset..*offset + delay.frames as usize],
                    block,
                    len,
                );
            }
            PreparedProcessor::Compressor(compressor) => compressor.process(state, block, len),
            PreparedProcessor::Decimate(decimator) => decimator.process(state, block, len),
            PreparedProcessor::Daft(daft) => daft.process(state, parameters, block, len, at),
            PreparedProcessor::LadderLP4 { ladder, offset } => {
                fault |= ladder.process(state, &mut delay_samples[*offset..*offset + ladder::CELLS], parameters, block, len, at);
            }
            PreparedProcessor::StereoModeller { stereo, offset } => {
                stereo.process(
                    state,
                    parameters,
                    block,
                    len,
                    at,
                    &mut delay_samples[*offset..],
                );
            }
            PreparedProcessor::Rectify(mode) => {
                for channel in block.iter_mut() {
                    channel[..len].iter_mut().for_each(|v| *v = mode.apply(*v));
                }
            }
            PreparedProcessor::Gainer { dry, gain, k } => {
                // z[0][0] is the smoothed gain; aux[0] marks it initialised
                // (a new state starts at the first target, not at zero).
                let [left, right] = block;
                let mut current = state.z[0][0] as f32;
                for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
                    let target = gain.value(parameters, at + i as u64, None) as f32;
                    if state.aux[0] == 0. {
                        (current, state.aux[0]) = (target, 1.);
                    }
                    let m = dry + f64::from(current);
                    *l *= m;
                    *r *= m;
                    current += (target - current) * *k as f32;
                }
                state.z[0][0] = f64::from(current);
            }
            PreparedProcessor::Gain(gain) => {
                applied = [*gain; 2];
                for channel in block.iter_mut() {
                    channel[..len].iter_mut().for_each(|v| *v *= gain);
                }
            }
            PreparedProcessor::StereoMatrix(m) => {
                let [left, right] = block;
                for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
                    (*l, *r) = (m[0][0] * *l + m[0][1] * *r, m[1][0] * *l + m[1][1] * *r);
                }
            }
            PreparedProcessor::ControlGain(lane) => {
                let ramp = parameters[*lane];
                if TRACE { applied = [0.; 2]; }
                let [left, right] = block;
                for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
                    let gain = ramp.value(at + i as u64);
                    if TRACE { for mean in &mut applied { *mean += gain / len.max(1) as f64; } }
                    *l *= gain;
                    *r *= gain;
                }
            }
            PreparedProcessor::Biquad(filter) => {
                let ([b0, b1, b2], [a1, a2]) = (filter.b, filter.a);
                let [mut zl, mut zr] = state.z;
                let [left, right] = block;
                for (l, r) in left[..len].iter_mut().zip(&mut right[..len]) {
                    let (x, y) = (*l, b0 * *l + zl[0]);
                    zl = [b1 * x - a1 * y + zl[1], b2 * x - a2 * y];
                    *l = y;
                    let (x, y) = (*r, b0 * *r + zr[0]);
                    zr = [b1 * x - a1 * y + zr[1], b2 * x - a2 * y];
                    *r = y;
                }
                state.z = [zl.map(flush), zr.map(flush)];
            }
        }
        if TRACE { if let Some(t) = trace.as_mut() {
            if !matches!(stage, PreparedProcessor::Gain(_) | PreparedProcessor::ControlGain(_) | PreparedProcessor::Mix { .. } | PreparedProcessor::Branch { .. }) {
                for c in 0..2 {
                    let a: f64 = input[c][..len].iter().map(|v| v*v).sum();
                    let b: f64 = block[c][..len].iter().map(|v| v*v).sum();
                    applied[c] = if a > 0. { (b/a).sqrt() } else { 0. };
                }
            }
            t.record(index, &input, trace_output.as_ref().unwrap_or(block), len, applied, enabled, parameters);
        } }
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
    pub parameters: Box<[ControlRamp]>,
    pub delay_samples: Slab<[f64; 2]>,
    /// Filter coefficient caches, one per render lane. Lane 0 is the audio
    /// thread's (and the whole of single-threaded rendering).
    pub filters: Slab<svf::FilterBank>,
    pub buses: crate::bus::BusState,
    pub feeds: Box<[taps::TapFeed]>,
    pub trace: Option<crate::trace::Recorder>,
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
            filters: Slab::new(
                (0..lanes.clamp(1, MAX_LANES))
                    .map(|_| svf::FilterBank::new(&plan.filters, expressions))
                    .collect::<Result<_, _>>()?,
                1,
            ),
            delay_samples: Slab::new(allocate(delay_count)?, delay_stride),
            parameters: control::initial_parameters(plan, &plan.dsp_bindings),
            buses: crate::bus::BusState::new(plan)?,
            feeds: allocate(feeds)?,
            trace: plan.signal_trace.as_ref().map(|t| t.recorder()).transpose()?,
        })
    }
    /// Per-voice chain state and delay line sizes (`stride`, `delay_stride`).
    pub fn shape(plan: &Prepared) -> (usize, usize) {
        let stride = plan
            .voice_chains
            .iter()
            .map(PreparedVoiceChain::stages)
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

    #[test]
    fn native_gain_and_non_pseudo_stereo_have_lane_kernels() {
        for pseudo in [false, true] {
            let chain = VoiceChain::new(vec![
                Processor::Gainer { dry: 0.1, gain: Parameter::Constant(0.8) },
                Processor::StereoModeller(StereoSettings {
                    width: Parameter::Constant(0.7), pan: Parameter::Constant(-0.2), pseudo,
                }),
            ], Vec::new(), 0).unwrap().compile(48000, &mut Vec::new(), &mut Vec::new()).unwrap();
            assert_eq!(chain.batches(), !pseudo);
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
                let mut filters = svf::FilterBank::new(&[], 0).unwrap();
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
                            bank: &mut filters,
                            reverbs: &mut [],
                            convolutions: &mut [],
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
                            let mut filters = svf::FilterBank::new(&[], 0).unwrap();
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
                                        bank: &mut filters,
                                        reverbs: &mut [],
                                        convolutions: &mut [],
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
    pub(crate) fn trace_graph(&self, plan: &Prepared, graph: &mut crate::trace::TraceGraph,
        zone: u32, group: Option<u32>, bus: Option<usize>, initial: &[ControlRamp]) -> crate::trace::VoiceNodes {
        let source = graph.node("sample_source", "resampler", Some(zone), group, bus, vec![], 0);
        let amp = graph.node("amplifier", "envelope_velocity_gain", Some(zone), group, bus, vec![], 0);
        let output = graph.node("voice_output", "expression_fade", Some(zone), group, bus, vec![], 0);
        let pre: Vec<_> = self.pre.iter().map(|s| graph.stage(s, "group_fx_pre", Some(zone), group, bus, plan, &plan.dsp_bindings, initial)).collect();
        let post: Vec<_> = self.post.iter().map(|s| graph.stage(s, "group_fx_post", Some(zone), group, bus, plan, &plan.dsp_bindings, initial)).collect();
        let parent = graph.connect(&self.pre, &pre, source);
        graph.edge(parent, amp, "serial");
        let parent = graph.connect(&self.post, &post, amp);
        graph.edge(parent, output, "serial");
        graph.edge(output, bus.map_or(graph.master, |b| graph.buses[b].input), "sum");
        let taps = self.taps.iter().map(|tap| {
            let (kind, parent) = match tap.position {
                VoiceSendPosition::BeforeAmplitude(n) => ("send_pre", if n == 0 { source } else { pre[n-1] }),
                VoiceSendPosition::AfterAmplitude(n) => ("send_post", if n == 0 { amp } else { post[n-1] }),
            };
            let parameters=vec![graph.parameter("level",tap.gain,plan,&plan.dsp_bindings,initial),graph.parameter("bypass",tap.bypass,plan,&plan.dsp_bindings,initial)];
            let id = graph.node(kind, "send_level_bypass", Some(zone), group, Some(tap.bus), parameters, 0);
            graph.edge(parent, id, "tap"); graph.edge(id, graph.buses[tap.bus].input, "send"); id
        }).collect();
        crate::trace::VoiceNodes { source, amp, output, pre, post, taps, region:0 }
    }
}
