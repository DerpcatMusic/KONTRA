//! Lower a semantic [`sampler_ir::Instrument`] to a [`Prepared`] plan through
//! the public builders. Control-thread only. Anything the runtime cannot
//! execute exactly is rejected with [`LowerError::Unsupported`], never
//! approximated silently.
use crate::{
    Biquad, Bus, BusSend, ControllerCondition, Direction, Driver, Envelope, EnvelopeCurve, Error,
    FilterKind, Keyswitch, Lfo, LfoRate, LfoShape, Loop, LoopMode, LoopShape, ModProgram, ModRoute,
    ModScale, ModSource, ModTarget, Parameter, Pcm, Playback, Prepared, Processor, Region,
    SelectionPolicy, Selector, Sequence, SequenceScope, StateVariableFilter, SvfMode, Switch,
    SwitchKeys, Switching, Take, TakePolicy, Trigger, VelocityCurve, VoiceChain,
};
use sampler_ir as ir;
use std::fmt;

/// Seed for random sequences; fixed so renders are reproducible.
const SEED: u64 = 0x5eed_1a7e;
/// Decay allowance for bus filters after their input stops, in seconds.
const BUS_TAIL_SECONDS: f64 = 0.1;

/// The builder step that refused lowered data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Regions,
    Envelope,
    Filter,
    VoiceChains,
    Buses,
    Variation,
    Releases,
    Articulations,
    Controllers,
    Modulation,
}

/// IR meaning the runtime cannot yet execute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Feature {
    Trigger(ir::Trigger),
    KeyScaling {
        cents_per_key: i32,
    },
    /// Probability takes that do not split [0, 1) into equal parts.
    UnevenProbabilities,
    MixedTakeKinds,
    ModulationRoute(ir::Target),
    /// Only envelopes drive amplitude.
    AmplitudeSource,
    ModulatorScope(ir::Scope),
    ChainScope(ir::Scope),
    GroupChain,
    FilterPoles(u8),
    TempoSync,
    VendorResonance,
    Delay,
    PreChainSend,
    Controls,
    /// Pitch bend routed anywhere but pitch (where it is native expression).
    PitchBendSource,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LowerError {
    Invalid(ir::ValidationError),
    AssetCount {
        assets: usize,
        supplied: usize,
    },
    Unsupported {
        owner: String,
        feature: Feature,
    },
    Core {
        stage: Stage,
        owner: String,
        error: Error,
    },
    /// A behavior binder rejected a module.
    Behavior {
        module: String,
        message: String,
    },
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => write!(f, "invalid instrument: {error}"),
            Self::AssetCount { assets, supplied } => {
                write!(
                    f,
                    "instrument has {assets} assets but {supplied} were supplied"
                )
            }
            Self::Unsupported { owner, feature } => {
                write!(
                    f,
                    "{owner}: {feature:?} is not supported by the native runtime"
                )
            }
            Self::Core {
                stage,
                owner,
                error,
            } => write!(
                f,
                "{owner}: {stage:?} rejected by the native runtime: {error}"
            ),
            Self::Behavior { module, message } => write!(f, "behavior {module}: {message}"),
        }
    }
}

impl std::error::Error for LowerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Invalid(error) => Some(error),
            Self::Core { error, .. } => Some(error),
            _ => None,
        }
    }
}

fn unsupported(owner: impl Into<String>, feature: Feature) -> LowerError {
    LowerError::Unsupported {
        owner: owner.into(),
        feature,
    }
}

/// Playback-rate ratios (source frames per output frame) the runtime can
/// resample. A translator narrows zones whose keys would pitch past them.
pub const PITCH_STEPS: std::ops::RangeInclusive<f64> =
    crate::resample::MIN_STEP..=crate::resample::MAX_STEP;

fn core(stage: Stage, owner: impl Into<String>) -> impl FnOnce(Error) -> LowerError {
    let owner = owner.into();
    move |error| LowerError::Core {
        stage,
        owner,
        error,
    }
}

/// Native per-note expression every lowered zone receives on top of its
/// authored modulation. Pitch bend needs no route: the note's expression bend
/// (sampler-midi's MPE member-channel bend, or a host's per-note tuning) is
/// always native. Both laws are identity at rest (zero pressure, centre
/// timbre), so non-MPE playing is unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MpeDefaults {
    /// Gain boost at full pressure in decibels; 0 disables.
    pub pressure_db: f64,
    /// How far a per-voice low-pass closes, in semitones below fully open, as
    /// timbre falls from centre (CC74 64) to 0; 0 disables.
    pub timbre_semitones: f64,
}

impl Default for MpeDefaults {
    fn default() -> Self {
        Self {
            pressure_db: 6.0,
            timbre_semitones: 60.0,
        }
    }
}

/// Lowering choices that are not part of the instrument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// `None` leaves pressure and timbre to authored routes only.
    pub mpe: Option<MpeDefaults>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mpe: Some(MpeDefaults::default()),
        }
    }
}

/// [`lower_with`] default [`Options`]: native MPE on every zone.
pub fn lower(
    instrument: &ir::Instrument,
    rate: u32,
    pcm: Vec<Pcm>,
    bind_behaviors: impl FnOnce(&[ir::Behavior], Prepared) -> Result<Prepared, LowerError>,
) -> Result<Prepared, LowerError> {
    lower_with(instrument, rate, pcm, &Options::default(), bind_behaviors)
}

/// Lower `instrument` for output at `rate`. `pcm[i]` is the decoded audio of
/// `instrument.assets[i]`. `bind_behaviors` receives every behavior module and
/// the plan built so far; it compiles them with its language frontend (for
/// KSP, `sampler_ksp`) and returns the bound plan. It is not called when the
/// instrument has no behaviors.
pub fn lower_with(
    instrument: &ir::Instrument,
    rate: u32,
    pcm: Vec<Pcm>,
    options: &Options,
    bind_behaviors: impl FnOnce(&[ir::Behavior], Prepared) -> Result<Prepared, LowerError>,
) -> Result<Prepared, LowerError> {
    instrument.validate().map_err(LowerError::Invalid)?;
    if pcm.len() != instrument.assets.len() {
        return Err(LowerError::AssetCount {
            assets: instrument.assets.len(),
            supplied: pcm.len(),
        });
    }
    if !instrument.controls.is_empty() {
        return Err(unsupported("controls", Feature::Controls));
    }
    let lowering = Lowering {
        ir: instrument,
        rate,
        pcm: &pcm,
        mpe: options
            .mpe
            .filter(|m| m.pressure_db != 0.0 || m.timbre_semitones != 0.0),
    };
    let mut regions = Vec::with_capacity(instrument.zones.len());
    let mut chains = Vec::new();
    let mut chain_of = Vec::with_capacity(instrument.zones.len());
    let mut candidates = 0usize;
    for (i, zone) in instrument.zones.iter().enumerate() {
        let (region, chain) = lowering.zone(i, zone)?;
        // Name the zone whose geometry the runtime would reject as a whole.
        let source = &pcm[region.sample];
        region
            .playback
            .cursor(source.frame_count(), source.sample_rate(), rate)
            .map_err(core(Stage::Regions, format!("zone {i}")))?;
        candidates += usize::from(zone.keys.high - zone.keys.low) + 1;
        regions.push(region);
        chain_of.push(chain.map(|chain| {
            chains.push(chain);
            chains.len() - 1
        }));
    }
    let mut plan = Prepared::new(rate, pcm.clone(), regions, candidates)
        .map_err(core(Stage::Regions, "zones"))?;
    if !chains.is_empty() {
        plan = plan
            .with_voice_chains(chains, chain_of)
            .map_err(core(Stage::VoiceChains, "zones"))?;
    }
    if instrument
        .zones
        .iter()
        .any(|z| z.velocity != ir::VelocityResponse::Linear)
    {
        let curves = instrument
            .zones
            .iter()
            .map(|z| match z.velocity {
                ir::VelocityResponse::None => VelocityCurve::Constant,
                ir::VelocityResponse::Linear => VelocityCurve::Linear,
                ir::VelocityResponse::Power(exponent) => VelocityCurve::Power(exponent),
            })
            .collect();
        plan = plan
            .with_velocity_curves(curves)
            .map_err(core(Stage::Regions, "velocity responses"))?;
    }
    plan = lowering.buses(plan)?;
    plan = lowering.modulation(plan)?;
    plan = lowering.variation(plan)?;
    plan = lowering.releases(plan)?;
    plan = lowering.articulations(plan)?;
    plan = lowering.controllers(plan)?;
    if instrument.behaviors.is_empty() {
        Ok(plan)
    } else {
        bind_behaviors(&instrument.behaviors, plan)
    }
}

struct Lowering<'a> {
    ir: &'a ir::Instrument,
    rate: u32,
    pcm: &'a [Pcm],
    mpe: Option<MpeDefaults>,
}

impl Lowering<'_> {
    fn frames(&self, time: ir::Time) -> u32 {
        (time.seconds() * f64::from(self.rate))
            .round()
            .min(f64::from(u32::MAX)) as u32
    }

    fn group(&self, zone: &ir::Zone) -> Option<&ir::Group> {
        zone.group.map(|g| &self.ir.groups[g.0])
    }

    fn zone(&self, i: usize, zone: &ir::Zone) -> Result<(Region, Option<VoiceChain>), LowerError> {
        let owner = format!("zone {i}");
        if let ir::Trigger::First | ir::Trigger::Legato = zone.trigger {
            return Err(unsupported(owner, Feature::Trigger(zone.trigger)));
        }
        let root_key = match zone.pitch {
            ir::KeyTracking::Tracked { root }
            | ir::KeyTracking::Scaled {
                root,
                cents_per_key: 100,
            } => Some(root),
            ir::KeyTracking::Fixed
            | ir::KeyTracking::Scaled {
                cents_per_key: 0, ..
            } => None,
            ir::KeyTracking::Scaled { cents_per_key, .. } => {
                return Err(unsupported(owner, Feature::KeyScaling { cents_per_key }));
            }
        };
        let group = self.group(zone);
        if group.is_some_and(|g| g.chain.is_some()) {
            return Err(unsupported(owner, Feature::GroupChain));
        }
        let gain = zone.gain.linear() * group.map_or(1.0, |g| g.gain.linear());
        let tune = zone.tune.semitones() + group.map_or(0.0, |g| g.tune.semitones());
        let pan = zone.pan.position + group.map_or(0.0, |g| g.pan.position);
        let asset_rate = f64::from(self.pcm[zone.asset.0].sample_rate());
        let envelope = self.envelope(&owner, zone)?;
        let mut pre = Vec::new();
        let mut post = Vec::new();
        if gain > 1.0 {
            // Region gain is an attenuator; boost belongs to the voice chain.
            pre.push(Processor::Gain(gain));
        }
        if pan != 0.0 {
            post.push(Processor::StereoMatrix(stereo(ir::Pan {
                position: pan.clamp(-1.0, 1.0),
                law: zone.pan.law,
            })));
        }
        if let Some(chain) = zone.chain {
            let chain = &self.ir.chains[chain.0];
            if chain.scope != ir::Scope::Voice {
                return Err(unsupported(owner, Feature::ChainScope(chain.scope)));
            }
            for p in &chain.pre_amplitude {
                pre.push(self.processor(&owner, *p)?);
            }
            for p in &chain.post_amplitude {
                post.push(self.processor(&owner, *p)?);
            }
        }
        let chain = if pre.is_empty() && post.is_empty() {
            None
        } else {
            Some(VoiceChain::new(pre, post, 0).map_err(core(Stage::VoiceChains, owner.clone()))?)
        };
        let loop_range = |range: ir::LoopRange, mode| {
            let crossfade = range.crossfade.frames(asset_rate) as usize;
            Loop {
                start: range.start as usize,
                end: range.end as usize,
                mode,
                shape: if range.alternating {
                    LoopShape::PingPong
                } else if crossfade > 0 {
                    LoopShape::Crossfade { frames: crossfade }
                } else {
                    LoopShape::Wrap
                },
                passes: None,
            }
        };
        let playback = Playback {
            start: zone.playback.start as usize,
            end: zone.playback.end.map(|end| end as usize),
            direction: if zone.playback.reverse {
                Direction::Reverse
            } else {
                Direction::Forward
            },
            loop_range: match zone.playback.looping {
                ir::Looping::None | ir::Looping::OneShot => None,
                ir::Looping::Continuous(range) => Some(loop_range(range, LoopMode::Continuous)),
                ir::Looping::UntilRelease(range) => Some(loop_range(range, LoopMode::UntilRelease)),
            },
            transpose_semitones: tune,
        };
        let region = Region {
            sample: zone.asset.0,
            key_low: zone.keys.low,
            key_high: zone.keys.high,
            root_key,
            velocity_low: f64::from(zone.velocities.low) / 127.0,
            velocity_high: f64::from(zone.velocities.high) / 127.0,
            gain: gain.min(1.0) as f32,
            envelope,
            playback,
        };
        Ok((region, chain))
    }

    fn envelope(&self, owner: &str, zone: &ir::Zone) -> Result<Envelope, LowerError> {
        let one_shot = zone.playback.looping == ir::Looping::OneShot;
        let Some(modulator) = zone.amplitude else {
            return Ok(if one_shot {
                Envelope::one_shot(0, u32::MAX, 0)
            } else {
                Envelope::default()
            });
        };
        let modulator = &self.ir.modulators[modulator.0];
        if modulator.scope != ir::Scope::Voice {
            return Err(unsupported(owner, Feature::ModulatorScope(modulator.scope)));
        }
        let ir::ModulationSource::Envelope(e) = &modulator.source else {
            return Err(unsupported(owner, Feature::AmplitudeSource));
        };
        self.adsr(owner, e, one_shot)
    }

    fn adsr(&self, owner: &str, e: &ir::Envelope, one_shot: bool) -> Result<Envelope, LowerError> {
        let curve = |curve: ir::Curve| match curve {
            ir::Curve::Linear => Ok(EnvelopeCurve::default()),
            ir::Curve::Exponential(k) => {
                EnvelopeCurve::exponential(k).map_err(core(Stage::Envelope, owner))
            }
        };
        let envelope = if one_shot {
            // Plays to the end of the audio whatever the gate does.
            Envelope::one_shot(self.frames(e.attack), u32::MAX, 0)
        } else if e.one_shot {
            Envelope::one_shot(
                self.frames(e.attack),
                self.frames(e.hold),
                self.frames(e.decay),
            )
        } else {
            Envelope::new(
                self.frames(e.attack),
                self.frames(e.hold),
                self.frames(e.decay),
                e.sustain as f32,
                self.frames(e.release),
            )
            .map_err(core(Stage::Envelope, owner))?
        };
        Ok(envelope.with_delay(self.frames(e.delay)).with_curves(
            curve(e.attack_shape)?,
            curve(e.decay_shape)?,
            curve(e.release_shape)?,
        ))
    }

    /// One voice modulation program per distinct zone route list.
    fn modulation(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        if self.mpe.is_none() && self.ir.zones.iter().all(|z| z.routes.is_empty()) {
            return Ok(plan);
        }
        let mut programs: Vec<ModProgram> = Vec::new();
        let mut known = std::collections::HashMap::new();
        let mut bindings = Vec::with_capacity(self.ir.zones.len());
        for (i, zone) in self.ir.zones.iter().enumerate() {
            if self.mpe.is_none() && zone.routes.is_empty() {
                bindings.push(None);
                continue;
            }
            let index = match known.get(&(&zone.routes, zone.chain)) {
                Some(&index) => index,
                None => {
                    let program = self.program(&format!("zone {i}"), zone)?;
                    let index = programs.len();
                    programs.push(program);
                    known.insert((&zone.routes, zone.chain), index);
                    index
                }
            };
            // A program whose routes all belong to native expression is no program.
            bindings.push((!programs[index].routes.is_empty()).then_some(index));
        }
        let ranges = self
            .ir
            .zones
            .iter()
            .map(|z| z.playback.start_range.min(u64::from(u32::MAX)) as u32)
            .collect();
        plan.with_voice_modulation(programs, bindings, ranges)
            .map_err(core(Stage::Modulation, "zones"))
    }

    fn program(&self, owner: &str, zone: &ir::Zone) -> Result<ModProgram, LowerError> {
        let mut program = ModProgram::default();
        let mut sources = std::collections::HashMap::new();
        let mut shapes = std::collections::HashMap::new();
        for &route_ref in &zone.routes {
            let route = &self.ir.routes[route_ref.0];
            let owner = format!("{owner} route {}", route_ref.0);
            let modulator = &self.ir.modulators[route.source.0];
            if modulator.scope != ir::Scope::Voice {
                return Err(unsupported(owner, Feature::ModulatorScope(modulator.scope)));
            }
            let target = match (route.target, route.depth) {
                // Pitch bend to pitch is the note's native expression bend.
                (ir::Target::Pitch, _) if modulator.source == ir::ModulationSource::PitchBend => {
                    continue;
                }
                (ir::Target::Amplitude, ir::Depth::Normalized(i)) => (ModTarget::Attenuate, i),
                (ir::Target::Amplitude, ir::Depth::Gain(g)) => {
                    (ModTarget::Decibels, 20.0 * g.linear().log10())
                }
                (ir::Target::Pitch, ir::Depth::Pitch(p)) => (ModTarget::Pitch, p.semitones()),
                (ir::Target::Pan, ir::Depth::Normalized(d)) => (ModTarget::Pan, d),
                (ir::Target::SampleStart, ir::Depth::Normalized(d)) => (ModTarget::SampleStart, d),
                (
                    ir::Target::Processor {
                        chain,
                        index,
                        parameter,
                    },
                    depth,
                ) if Some(chain) == zone.chain && self.modulable_filter(chain, index) => {
                    match (parameter, depth) {
                        (ir::ProcessorParameter::Cutoff, ir::Depth::Pitch(p)) => {
                            (ModTarget::Cutoff, p.semitones())
                        }
                        (ir::ProcessorParameter::Resonance, ir::Depth::Gain(g)) => {
                            (ModTarget::Resonance, 20.0 * g.linear().log10())
                        }
                        _ => {
                            return Err(unsupported(owner, Feature::ModulationRoute(route.target)));
                        }
                    }
                }
                _ => return Err(unsupported(owner, Feature::ModulationRoute(route.target))),
            };
            let mut source_of = |modulator: ir::ModulatorRef,
                                 program: &mut ModProgram|
             -> Result<usize, LowerError> {
                if let Some(&index) = sources.get(&modulator) {
                    return Ok(index);
                }
                let m = &self.ir.modulators[modulator.0];
                if m.scope != ir::Scope::Voice {
                    return Err(unsupported(owner.clone(), Feature::ModulatorScope(m.scope)));
                }
                program.sources.push(self.mod_source(&owner, &m.source)?);
                sources.insert(modulator, program.sources.len() - 1);
                Ok(program.sources.len() - 1)
            };
            let source = source_of(route.source, &mut program)?;
            let scale = match route.scale {
                None => None,
                Some(scale) => Some(ModScale {
                    source: source_of(scale.source, &mut program)?,
                    shape: None,
                }),
            };
            let mut shape_of = |shape: ir::ShapeRef, program: &mut ModProgram| {
                *shapes.entry(shape).or_insert_with(|| {
                    program.shapes.push(self.ir.shapes[shape.0].points.clone());
                    program.shapes.len() - 1
                })
            };
            let shape = route.shape.map(|shape| shape_of(shape, &mut program));
            let scale = scale.map(|s| ModScale {
                shape: route
                    .scale
                    .and_then(|r| r.shape)
                    .map(|shape| shape_of(shape, &mut program)),
                ..s
            });
            program.routes.push(ModRoute {
                source,
                target: target.0,
                depth: target.1,
                invert: route.invert,
                shape,
                lag: self.frames(route.smoothing),
                scale,
            });
        }
        if let Some(mpe) = self.mpe {
            if mpe.pressure_db != 0.0 {
                program.sources.push(ModSource::Pressure);
                program.routes.push(ModRoute::new(
                    program.sources.len() - 1,
                    ModTarget::Decibels,
                    mpe.pressure_db,
                ));
            }
            if mpe.timbre_semitones != 0.0 {
                // -1 at timbre 0, 0 from centre up: only darker than centre.
                program
                    .shapes
                    .push(vec![(0.0, -1.0), (0.5, 0.0), (1.0, 0.0)]);
                program.sources.push(ModSource::Timbre);
                program.routes.push(ModRoute {
                    shape: Some(program.shapes.len() - 1),
                    ..ModRoute::new(
                        program.sources.len() - 1,
                        ModTarget::Tone,
                        mpe.timbre_semitones,
                    )
                });
            }
        }
        Ok(program)
    }

    /// Voice cutoff/Q modulation scales every state-variable filter in the
    /// chain, so it is exact only when the addressed filter is the only one.
    fn modulable_filter(&self, chain: ir::ChainRef, index: usize) -> bool {
        let chain = &self.ir.chains[chain.0];
        let processors: Vec<_> = chain
            .pre_amplitude
            .iter()
            .chain(&chain.post_amplitude)
            .collect();
        let svf = |p: &ir::Processor| {
            matches!(
                p,
                ir::Processor::Filter(ir::Filter {
                    kind: ir::FilterKind::LowPass { poles: 2 }
                        | ir::FilterKind::HighPass { poles: 2 }
                        | ir::FilterKind::BandPass { poles: 2 }
                        | ir::FilterKind::Notch { poles: 2 }
                        | ir::FilterKind::AllPass,
                    ..
                })
            )
        };
        svf(processors[index]) && processors.iter().filter(|p| svf(p)).count() == 1
    }

    fn mod_source(
        &self,
        owner: &str,
        source: &ir::ModulationSource,
    ) -> Result<ModSource, LowerError> {
        Ok(match source {
            ir::ModulationSource::Envelope(e) => ModSource::Envelope(self.adsr(owner, e, false)?),
            ir::ModulationSource::Lfo(lfo) => ModSource::Lfo(Lfo {
                shape: match lfo.shape {
                    ir::LfoShape::Sine => LfoShape::Sine,
                    ir::LfoShape::Triangle => LfoShape::Triangle,
                    ir::LfoShape::Square => LfoShape::Square,
                    ir::LfoShape::SawUp => LfoShape::SawUp,
                    ir::LfoShape::SawDown => LfoShape::SawDown,
                    ir::LfoShape::SampleAndHold => LfoShape::SampleAndHold,
                    ir::LfoShape::Random => LfoShape::Random,
                },
                rate: match lfo.rate {
                    ir::Frequency::Hertz(hz) => LfoRate::Hertz(hz),
                    ir::Frequency::Beats(beats) => LfoRate::Beats(beats),
                },
                phase: lfo.phase,
                delay: self.frames(lfo.delay),
                fade: self.frames(lfo.fade_in),
                retrigger: lfo.retrigger,
            }),
            ir::ModulationSource::Controller(cc) => ModSource::Controller(*cc),
            ir::ModulationSource::Velocity => ModSource::Velocity,
            ir::ModulationSource::Key => ModSource::Key,
            // Pressure arrives as the note's expression pressure: MPE channel
            // pressure or polyphonic aftertouch.
            ir::ModulationSource::ChannelPressure | ir::ModulationSource::PolyPressure => {
                ModSource::Pressure
            }
            ir::ModulationSource::Timbre => ModSource::Timbre,
            ir::ModulationSource::Random => ModSource::Random,
            ir::ModulationSource::Constant => ModSource::Constant,
            ir::ModulationSource::PitchBend => {
                return Err(unsupported(owner, Feature::PitchBendSource));
            }
        })
    }

    fn processor(&self, owner: &str, processor: ir::Processor) -> Result<Processor, LowerError> {
        Ok(match processor {
            ir::Processor::Gain(gain) => Processor::Gain(gain.linear()),
            ir::Processor::Pan(pan) => Processor::StereoMatrix(stereo(pan)),
            ir::Processor::Filter(filter) => self.filter(owner, filter)?,
            ir::Processor::Delay { .. } => return Err(unsupported(owner, Feature::Delay)),
        })
    }

    fn filter(&self, owner: &str, filter: ir::Filter) -> Result<Processor, LowerError> {
        let ir::Frequency::Hertz(cutoff) = filter.cutoff else {
            return Err(unsupported(owner, Feature::TempoSync));
        };
        // Below Nyquist with headroom; the source may author up to 20 kHz at 44.1 kHz.
        let cutoff = cutoff.min(f64::from(self.rate) * 0.49);
        let q = match filter.resonance {
            ir::Resonance::Q(q) => q,
            // A 2-pole peak of Q·(flat) sits at the cutoff; 0 dB is Butterworth.
            ir::Resonance::Decibels(db) => std::f64::consts::FRAC_1_SQRT_2 * 10f64.powf(db / 20.0),
            ir::Resonance::Normalized(_) => {
                return Err(unsupported(owner, Feature::VendorResonance));
            }
        };
        let svf = |mode| {
            Ok(Processor::StateVariable(StateVariableFilter {
                mode,
                cutoff_hz: Parameter::Constant(cutoff),
                q: Parameter::Constant(q),
            }))
        };
        let biquad = |kind| {
            Biquad::new(self.rate, kind, cutoff, q)
                .map(Processor::Biquad)
                .map_err(core(Stage::Filter, owner))
        };
        match filter.kind {
            ir::FilterKind::LowPass { poles: 2 } => svf(SvfMode::LowPass),
            ir::FilterKind::HighPass { poles: 2 } => svf(SvfMode::HighPass),
            ir::FilterKind::BandPass { poles: 2 } => svf(SvfMode::BandPass),
            ir::FilterKind::Notch { poles: 2 } => svf(SvfMode::Notch),
            ir::FilterKind::AllPass => svf(SvfMode::AllPass),
            ir::FilterKind::LowPass { poles }
            | ir::FilterKind::HighPass { poles }
            | ir::FilterKind::BandPass { poles }
            | ir::FilterKind::Notch { poles } => {
                Err(unsupported(owner, Feature::FilterPoles(poles)))
            }
            ir::FilterKind::Peak { gain } => biquad(FilterKind::Peak {
                gain_db: 20.0 * gain.linear().log10(),
            }),
            ir::FilterKind::LowShelf { gain } => biquad(FilterKind::LowShelf {
                gain_db: 20.0 * gain.linear().log10(),
            }),
            ir::FilterKind::HighShelf { gain } => biquad(FilterKind::HighShelf {
                gain_db: 20.0 * gain.linear().log10(),
            }),
        }
    }

    fn buses(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        let routed = self.ir.zones.iter().any(|z| {
            self.group(z)
                .is_some_and(|g| g.output != ir::Output::Master)
        });
        if self.ir.buses.is_empty() && !routed {
            return Ok(plan);
        }
        let target = |output: ir::Output| match output {
            ir::Output::Master => None,
            ir::Output::Bus(bus) => Some(bus.0),
        };
        let mut buses = Vec::with_capacity(self.ir.buses.len());
        for (i, bus) in self.ir.buses.iter().enumerate() {
            let owner = format!("bus {i}");
            let mut processors = Vec::new();
            if let Some(chain) = bus.chain {
                let chain = &self.ir.chains[chain.0];
                if chain.scope != ir::Scope::Bus(ir::BusRef(i)) {
                    return Err(unsupported(owner, Feature::ChainScope(chain.scope)));
                }
                for p in chain.pre_amplitude.iter().chain(&chain.post_amplitude) {
                    processors.push(self.processor(&owner, *p)?);
                }
            }
            let mut sends = vec![BusSend {
                bus: target(bus.output),
                gain: 1.0,
            }];
            for send in &bus.sends {
                if send.position == ir::SendPosition::PreChain {
                    return Err(unsupported(owner, Feature::PreChainSend));
                }
                sends.push(BusSend {
                    bus: target(send.to),
                    gain: send.gain.linear(),
                });
            }
            let tail_frames = if processors.is_empty() {
                0
            } else {
                (BUS_TAIL_SECONDS * f64::from(self.rate)) as u32
            };
            buses.push(Bus {
                processors,
                sends,
                tail_frames,
            });
        }
        let bindings = self
            .ir
            .zones
            .iter()
            .map(|z| self.group(z).and_then(|g| target(g.output)))
            .collect();
        plan.with_buses(buses, bindings)
            .map_err(core(Stage::Buses, "buses"))
    }

    fn variation(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        if self.ir.sequences.is_empty() {
            return Ok(plan);
        }
        let mut sequences = Vec::with_capacity(self.ir.sequences.len());
        // Probability takes become uniform random takes, indexed by interval order.
        let mut intervals: Vec<Vec<(f64, f64)>> = vec![Vec::new(); self.ir.sequences.len()];
        let mut kinds = vec![None; self.ir.sequences.len()];
        for (i, zone) in self.ir.zones.iter().enumerate() {
            let Some(selection) = zone.selection else {
                continue;
            };
            let s = selection.sequence.0;
            let probability = matches!(selection.take, ir::Take::Probability { .. });
            if kinds[s]
                .replace(probability)
                .is_some_and(|k| k != probability)
            {
                return Err(unsupported(format!("zone {i}"), Feature::MixedTakeKinds));
            }
            if let ir::Take::Probability { low, high } = selection.take
                && !intervals[s].contains(&(low, high))
            {
                intervals[s].push((low, high));
            }
        }
        for (s, sequence) in self.ir.sequences.iter().enumerate() {
            let owner = format!("sequence {s}");
            let (takes, policy) = if kinds[s] == Some(true) {
                let parts = &mut intervals[s];
                parts.sort_by(|a, b| a.0.total_cmp(&b.0));
                let width = 1.0 / parts.len() as f64;
                let even = parts.iter().enumerate().all(|(n, (low, high))| {
                    (low - n as f64 * width).abs() < 1e-6
                        && (high - (n + 1) as f64 * width).abs() < 1e-6
                });
                if !even {
                    return Err(unsupported(owner, Feature::UnevenProbabilities));
                }
                (parts.len() as u32, TakePolicy::Random { seed: SEED })
            } else {
                let policy = match sequence.policy {
                    ir::SequencePolicy::RoundRobin => TakePolicy::Sequential,
                    ir::SequencePolicy::Random => TakePolicy::Random { seed: SEED },
                    ir::SequencePolicy::RandomNoRepeat => TakePolicy::NoRepeat { seed: SEED },
                };
                (sequence.takes, policy)
            };
            let (scope, capacity) = match sequence.counter {
                ir::CounterScope::Instrument => (SequenceScope::Global, 1),
                ir::CounterScope::Key => (SequenceScope::Key, 128),
                ir::CounterScope::Channel => (SequenceScope::Channel, 16),
                ir::CounterScope::ChannelKey => (SequenceScope::ChannelKey, 16 * 128),
            };
            sequences.push(Sequence {
                takes,
                policy,
                scope,
                capacity,
            });
        }
        let takes = self
            .ir
            .zones
            .iter()
            .map(|zone| {
                zone.selection.map(|selection| Take {
                    sequence: selection.sequence.0,
                    index: match selection.take {
                        ir::Take::Index(index) => index,
                        ir::Take::Probability { low, high } => intervals[selection.sequence.0]
                            .iter()
                            .position(|&part| part == (low, high))
                            .unwrap_or_default()
                            as u32,
                    },
                })
            })
            .collect();
        let states = sequences.iter().map(|s| s.capacity).sum();
        plan.with_variation(sequences, takes, states, 0)
            .map_err(core(Stage::Variation, "sequences"))
    }

    fn releases(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        if self
            .ir
            .zones
            .iter()
            .all(|z| z.trigger == ir::Trigger::Attack)
        {
            return Ok(plan);
        }
        let triggers = self
            .ir
            .zones
            .iter()
            .map(|z| match z.trigger {
                ir::Trigger::KeyRelease => Trigger::KeyRelease,
                ir::Trigger::GateRelease => Trigger::GateRelease,
                _ => Trigger::Attack,
            })
            .collect();
        plan.with_releases(triggers, Default::default(), Default::default())
            .map_err(core(Stage::Releases, "release zones"))
    }

    fn articulations(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        let articulations = &self.ir.articulations;
        if articulations.is_empty() {
            return Ok(plan);
        }
        // The runtime starts in articulation 0: give that number to the default.
        let default = articulations.iter().position(|a| a.default).unwrap_or(0);
        let id = |index: usize| match index {
            i if i == default => 0,
            i if i < default => i as u32 + 1,
            i => i as u32,
        };
        let switching = self.ir.switching;
        let behavior = switching.owner == ir::SwitchOwner::Behavior;
        // A behavior reads its own switch keys; freed keys play notes.
        let native_keys = !behavior
            && (switching.driver == ir::Driver::Keys || switching.keys != ir::SwitchKeys::Play);
        let switches = articulations
            .iter()
            .enumerate()
            .filter(|_| native_keys)
            .flat_map(|(i, a)| {
                a.switch_keys.iter().map(move |&key| Keyswitch {
                    key,
                    articulation: id(i),
                })
            })
            .collect();
        let tags = self
            .ir
            .zones
            .iter()
            .map(|z| z.articulation.map(|a| id(a.0)))
            .collect();
        let selectors = articulations
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let alt = a.alternatives;
                let (controller, low, high) = match switching.driver {
                    ir::Driver::Keys => return None,
                    ir::Driver::Velocity => alt.velocities.map(|v| (0, v.low, v.high))?,
                    ir::Driver::Channel => alt.channel.map(|c| (0, c, c))?,
                    ir::Driver::Controller => {
                        alt.controller.map(|c| (c.controller, c.low, c.high))?
                    }
                    ir::Driver::Program => alt.program.map(|p| (0, p, p))?,
                };
                let switch = if behavior {
                    Switch::Tap(*a.switch_keys.first()?)
                } else {
                    Switch::Articulation(id(i))
                };
                Some(Selector {
                    controller,
                    low,
                    high,
                    switch,
                })
            })
            .collect();
        let switching = Switching::new(
            match switching.driver {
                ir::Driver::Keys => Driver::Keys,
                ir::Driver::Velocity => Driver::Velocity,
                ir::Driver::Channel => Driver::Channel,
                ir::Driver::Controller => Driver::Controller,
                ir::Driver::Program => Driver::Program,
            },
            match switching.keys {
                ir::SwitchKeys::Keep => SwitchKeys::Keep,
                ir::SwitchKeys::Play => SwitchKeys::Play,
                ir::SwitchKeys::Swallow => SwitchKeys::Swallow,
            },
            articulations
                .iter()
                .flat_map(|a| a.switch_keys.iter().copied()),
            selectors,
        )
        .map_err(core(Stage::Articulations, "articulation drivers"))?;
        plan.with_articulations(
            tags,
            switches,
            SelectionPolicy::Onset,
            SelectionPolicy::Onset,
        )
        .map(|plan| plan.with_switching(switching))
        .map_err(core(Stage::Articulations, "articulations"))
    }

    fn controllers(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        if self.ir.zones.iter().all(|z| z.conditions.is_empty()) {
            return Ok(plan);
        }
        // A 7-bit value covers every 32-bit value that scales down to it.
        let conditions: Vec<Vec<_>> = self
            .ir
            .zones
            .iter()
            .map(|z| {
                z.conditions
                    .iter()
                    .map(|c| ControllerCondition {
                        controller: c.controller,
                        low: u32::from(c.low) << 25,
                        high: (u32::from(c.high) << 25) | 0x01ff_ffff,
                    })
                    .collect()
            })
            .collect();
        let count = conditions.iter().map(Vec::len).sum();
        plan.with_controllers(conditions, count)
            .map_err(core(Stage::Controllers, "controller ranges"))
    }
}

/// Per-channel gains for a stereo source.
fn stereo(pan: ir::Pan) -> [[f64; 2]; 2] {
    let p = pan.position;
    let (left, right) = match pan.law {
        ir::PanLaw::Balance => (1.0 - p.max(0.0), 1.0 + p.min(0.0)),
        ir::PanLaw::EqualPower => {
            let angle = (p + 1.0) * std::f64::consts::FRAC_PI_4;
            (angle.cos(), angle.sin())
        }
    };
    [[left, 0.0], [0.0, right]]
}
