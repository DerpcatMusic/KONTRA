//! Lower a semantic [`sampler_ir::Instrument`] to a [`Prepared`] plan through
//! the public builders. Control-thread only. Anything the runtime cannot
//! execute exactly is rejected with [`LowerError::Unsupported`], never
//! approximated silently.
use crate::{
    Biquad, Breakpoint, Breakpoints, Bus, BusSend, ControlDefinition, ControlDomain, ControlRange,
    CompressorSettings, DaftSettings, ControlValue, ControllerCondition, Direction, Driver, Envelope, EnvelopeCurve, Error,
    FilterKind, GroupParams, Impulse, Keyswitch, Lfo, LfoRate, LfoShape, Loop, LoopMode, LoopShape,
    ModProgram, ModRoute, ModScale, ModSource, ModTarget, Parameter, Pcm, Playback, Prepared,
    Processor, Rectifier, Region, ReverbSettings, SelectionPolicy, Selector, Sequence, SequenceScope,
    SlotKind, StateVariableFilter, SvfMode, Switch, SwitchKeys, Switching, Take, TakePolicy,
    Trigger, VelocityCurve, VoiceChain, ZoneFades, slot_control,
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
    /// A reverb in a voice or group chain; it is a bus processor.
    VoiceReverb,
    PreChainSend,
    Controls,
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
    /// What per-note timbre moves.
    pub timbre: TimbreTarget,
}

/// What an MPE note's timbre (Y, CC74) drives. Kontakt gives CC74 no meaning
/// of its own, so the choice is ours: the instrument's own loudness-by-dynamics
/// position when it has one, else its filter's cutoff, else a plain tone filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimbreTarget {
    /// A per-voice low-pass that closes below centre.
    #[default]
    Tone,
    /// The cutoff of the zone's own filter, both ways from centre; zones
    /// without a modulable filter fall back to [`Self::Tone`].
    Cutoff,
    /// The note's own value of this dynamics controller (the host side writes
    /// it per member channel); lowering adds no route.
    Controller(u8),
}

/// Semitones the cutoff moves each way from centre when timbre drives it.
const TIMBRE_CUTOFF_SEMITONES: f64 = 36.0;

impl MpeDefaults {
    /// The defaults with timbre aimed at `instrument`'s dynamics controller if
    /// it has one (the host volume's own controller excluded), else its filter
    /// cutoff, else the tone filter.
    pub fn for_instrument(instrument: &ir::Instrument) -> Self {
        let volume = instrument.host_volume.map(|v| v.controller);
        let timbre = if let Some(cc) = instrument
            .amplitude_controllers()
            .into_iter()
            .find(|&cc| Some(cc) != volume)
        {
            TimbreTarget::Controller(cc)
        } else if instrument.zones.iter().filter_map(|z| z.chain).any(|c| {
            let chain = &instrument.chains[c.0];
            (0..chain.pre_amplitude.len() + chain.post_amplitude.len())
                .any(|i| modulable_filter(instrument, c, i))
        }) {
            TimbreTarget::Cutoff
        } else {
            TimbreTarget::Tone
        };
        Self {
            timbre,
            ..Self::default()
        }
    }
}

impl Default for MpeDefaults {
    fn default() -> Self {
        Self {
            pressure_db: 6.0,
            timbre_semitones: 60.0,
            timbre: TimbreTarget::Tone,
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

/// The keyswitch map and driver table `switching` gives `instrument`'s
/// articulations, for [`crate::Runtime::set_switching`]: a remap changes how
/// articulations are selected without lowering or loading the plan again.
pub fn switching(
    instrument: &ir::Instrument,
    switching: ir::Switching,
) -> Result<(Vec<Keyswitch>, Switching), LowerError> {
    Lowering {
        ir: instrument,
        rate: 1,
        pcm: &[],
        mpe: None,
    }
    .switching(switching)
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
    let tapped;
    let instrument = if instrument.groups.iter().any(|g| !g.sends.is_empty()) {
        tapped = instrument.with_group_taps();
        &tapped
    } else {
        instrument
    };
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
    // Before the voice chains, which validate the controls they bind.
    let slots = lowering.slot_controls();
    if !slots.is_empty() {
        plan = plan
            .with_controls(slots)
            .map_err(core(Stage::Regions, "slot controls"))?;
    }
    if !instrument.groups.is_empty() {
        // Group membership and authored values for script group edits
        // (`purge_group`, `set_engine_par`); selection is unaffected.
        let count = instrument.groups.len() as u32;
        let members = instrument
            .zones
            .iter()
            .map(|z| z.group.map(|g| g.0 as u32))
            .collect();
        let params = instrument
            .groups
            .iter()
            .map(|g| GroupParams {
                decibels: 20.0
                    * g.tap
                        .as_ref()
                        .map_or(g.gain, |t| instrument.buses[t.bus.0].gain)
                        .linear()
                        .max(1e-9)
                        .log10(),
                pan: g.pan.position,
                semitones: g.tune.semitones(),
            })
            .collect();
        plan = plan
            .with_groups(count, members)
            .and_then(|p| p.with_group_params(params))
            .map_err(core(Stage::Regions, "groups"))?;
    }
    if instrument.voice_limit.is_some() || !instrument.voice_limits.is_empty() {
        let limit = |l: &ir::VoiceLimit| crate::VoiceLimit {
            voices: l.voices,
            kill: match l.kill {
                ir::Kill::Any => crate::Kill::Any,
                ir::Kill::Oldest => crate::Kill::Oldest,
                ir::Kill::Newest => crate::Kill::Newest,
                ir::Kill::Highest => crate::Kill::Highest,
                ir::Kill::Lowest => crate::Kill::Lowest,
            },
            prefer_released: l.prefer_released,
            fade: lowering.frames(l.fade),
        };
        plan = plan
            .with_voice_limits(
                instrument.voice_limit.as_ref().map(limit),
                instrument.voice_limits.iter().map(limit).collect(),
                instrument.groups.iter().map(|g| g.voice_limit).collect(),
            )
            .map_err(core(Stage::Regions, "voice limits"))?;
    }
    if instrument.groups.iter().any(|g| g.monophonic_release) {
        plan = plan
            .with_monophonic_release(
                instrument
                    .groups
                    .iter()
                    .map(|g| g.monophonic_release)
                    .collect(),
            )
            .map_err(core(Stage::Regions, "monophonic release"))?;
    }
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
    if instrument
        .zones
        .iter()
        .any(|z| z.fades != ir::Fades::default())
    {
        let fades = instrument
            .zones
            .iter()
            .map(|z| ZoneFades {
                velocity_in: z.fades.velocity_in,
                velocity_out: z.fades.velocity_out,
                key_in: z.fades.key_in,
                key_out: z.fades.key_out,
            })
            .collect();
        plan = plan
            .with_zone_fades(fades)
            .map_err(core(Stage::Regions, "zone crossfades"))?;
    }
    plan = lowering.buses(plan)?;
    plan = lowering.modulation(plan)?;
    plan = lowering.variation(plan)?;
    plan = lowering.releases(plan)?;
    plan = lowering.articulations(plan)?;
    plan = lowering.controllers(plan)?;
    plan = lowering.axes(plan)?;
    // The instrument's own bend depth (Kontakt's pitch-bend modulator) is the
    // plain MIDI default range; without one, the MIDI default of 2 semitones.
    if let Some(range) = instrument
        .routes
        .iter()
        .filter(|r| {
            r.target == ir::Target::Pitch
                && instrument.modulators[r.source.0].source == ir::ModulationSource::PitchBend
        })
        .filter_map(|r| match r.depth {
            ir::Depth::Pitch(p) => Some(p.semitones().abs()),
            _ => None,
        })
        .reduce(f64::max)
    {
        plan = plan
            .with_bend_range(range)
            .map_err(core(Stage::Modulation, "pitch-bend range"))?;
    }
    if instrument.behaviors.is_empty() {
        Ok(plan)
    } else {
        bind_behaviors(&instrument.behaviors, plan)
    }
}

/// Voice cutoff/Q modulation scales every state-variable filter in the
/// chain, so it is exact only when the addressed filter is the only one.
fn modulable_filter(ir: &ir::Instrument, chain: ir::ChainRef, index: usize) -> bool {
    let chain = &ir.chains[chain.0];
    let processors: Vec<_> = chain
        .pre_amplitude
        .iter()
        .chain(&chain.post_amplitude)
        .collect();
    let svf = |p: &ir::Processor| {
        matches!(
            p,
            ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles: 1 | 2 }
                    | ir::FilterKind::HighPass { poles: 1 | 2 }
                    | ir::FilterKind::BandPass { poles: 2 }
                    | ir::FilterKind::Notch { poles: 2 }
                    | ir::FilterKind::AllPass,
                ..
            })
        )
    };
    svf(processors[index]) && processors.iter().filter(|p| svf(p)).count() == 1
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
            if chain
                .pre_amplitude
                .iter()
                .chain(&chain.post_amplitude)
                .any(|p| {
                    matches!(
                        p,
                        ir::Processor::Reverb(_) | ir::Processor::Convolution { .. }
                    )
                })
            {
                return Err(unsupported(owner, Feature::VoiceReverb));
            }
            pre.extend(self.lower_list(&owner, &chain.pre_amplitude.iter().collect::<Vec<_>>())?);
            post.extend(self.lower_list(&owner, &chain.post_amplitude.iter().collect::<Vec<_>>())?);
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
            ir::Curve::Step => Ok(EnvelopeCurve::step()),
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
                ) if Some(chain) == zone.chain && modulable_filter(self.ir, chain, index) => {
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
                // One instrument-wide LFO: free-running, or restarted by every voice start.
                let shared = matches!(
                    (m.scope, &m.source),
                    (ir::Scope::Master, ir::ModulationSource::Lfo(_))
                );
                if m.scope != ir::Scope::Voice && !shared {
                    return Err(unsupported(owner.clone(), Feature::ModulatorScope(m.scope)));
                }
                let mut source = self.mod_source(&owner, &m.source, program)?;
                if let ModSource::Lfo(lfo) = &mut source {
                    lfo.shared = shared;
                }
                program.sources.push(source);
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
            let cutoff = mpe.timbre == TimbreTarget::Cutoff
                && zone.chain.is_some_and(|c| {
                    let chain = &self.ir.chains[c.0];
                    (0..chain.pre_amplitude.len() + chain.post_amplitude.len())
                        .any(|i| modulable_filter(self.ir, c, i))
                });
            if cutoff {
                // Both ways from centre: -1 at timbre 0, +1 at 1.
                program
                    .shapes
                    .push(vec![(0.0, -1.0), (0.5, 0.0), (1.0, 1.0)]);
                program.sources.push(ModSource::Timbre);
                program.routes.push(ModRoute {
                    shape: Some(program.shapes.len() - 1),
                    ..ModRoute::new(
                        program.sources.len() - 1,
                        ModTarget::Cutoff,
                        TIMBRE_CUTOFF_SEMITONES,
                    )
                });
            } else if matches!(mpe.timbre, TimbreTarget::Tone | TimbreTarget::Cutoff)
                && mpe.timbre_semitones != 0.0
            {
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

    fn mod_source(
        &self,
        owner: &str,
        source: &ir::ModulationSource,
        program: &mut ModProgram,
    ) -> Result<ModSource, LowerError> {
        Ok(match source {
            ir::ModulationSource::Envelope(e) => ModSource::Envelope(self.adsr(owner, e, false)?),
            ir::ModulationSource::Breakpoints(b) => {
                let mut points = Vec::with_capacity(b.points.len());
                for p in &b.points {
                    points.push(Breakpoint {
                        frames: self.frames(p.time),
                        level: p.level as f32,
                        curve: match p.shape {
                            ir::Curve::Linear => EnvelopeCurve::default(),
                            ir::Curve::Step => EnvelopeCurve::step(),
                            ir::Curve::Exponential(k) => EnvelopeCurve::exponential(k)
                                .map_err(core(Stage::Envelope, owner))?,
                        },
                    });
                }
                program.breakpoints.push(Breakpoints {
                    points,
                    sustain: b.sustain,
                });
                ModSource::Breakpoints(program.breakpoints.len() - 1)
            }
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
                shared: false,
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
            ir::ModulationSource::Script(id) => ModSource::Script(*id),
            ir::ModulationSource::ReleaseCounter(t) => ModSource::ReleaseCounter {
                frames: self.frames(*t).max(1),
            },
            // Bend to pitch never gets here (native expression bend).
            ir::ModulationSource::PitchBend => ModSource::PitchBend,
        })
    }

    /// A slot control's binding: ramped over 10 ms like a script's gain edit.
    fn slot_range(&self, kind: SlotKind, address: ir::SlotAddress) -> ControlRange {
        let control = slot_control(kind, address.group, address.slot, address.generic);
        ControlRange {
            control,
            low: 0.0,
            high: kind.max(),
            ramp_frames: self.rate / 100,
        }
    }

    /// The control behind every Mix block of every chain, once each.
    fn slot_controls(&self) -> Vec<ControlDefinition> {
        let mut all = std::collections::BTreeMap::new();
        for chain in &self.ir.chains {
            for p in chain.pre_amplitude.iter().chain(&chain.post_amplitude) {
                let ir::Processor::Mix {
                    address,
                    dry,
                    wet,
                    bypass,
                    ..
                } = *p
                else {
                    continue;
                };
                for (kind, initial) in [
                    (SlotKind::Dry, dry),
                    (SlotKind::Output, wet),
                    (SlotKind::Bypass, f64::from(bypass)),
                ] {
                    let id = slot_control(kind, address.group, address.slot, address.generic);
                    all.entry(id).or_insert(ControlDefinition {
                        id,
                        domain: ControlDomain::Real {
                            min: 0.0,
                            max: kind.max(),
                        },
                        default: ControlValue::Real(initial),
                    });
                }
            }
        }
        all.into_values().collect()
    }

    /// Lower a processor list: Mix counts are in listed processors, and a
    /// 4-pole filter inside a span lowers to two.
    fn lower_list(
        &self,
        owner: &str,
        listed: &[&ir::Processor],
    ) -> Result<Vec<Processor>, LowerError> {
        let mut processors = Vec::new();
        // Core index of each listed processor.
        let mut starts = Vec::with_capacity(listed.len() + 1);
        for p in listed {
            starts.push(processors.len());
            processors.extend(self.processors(owner, **p)?);
        }
        starts.push(processors.len());
        for (n, p) in listed.iter().enumerate() {
            if let ir::Processor::Mix { count, .. } | ir::Processor::Branch { count, .. } = **p {
                let end = starts[(n + 1 + usize::from(count)).min(listed.len())];
                if let Processor::Mix { count, .. } | Processor::Branch { count, .. } =
                    &mut processors[starts[n]]
                {
                    *count = u16::try_from(end - starts[n] - 1)
                        .map_err(|_| unsupported(owner, Feature::Controls))?;
                }
            }
        }
        Ok(processors)
    }

    /// One or more runtime stages: a 4-pole filter is two cascaded 2-pole sections.
    fn processors(
        &self,
        owner: &str,
        processor: ir::Processor,
    ) -> Result<Vec<Processor>, LowerError> {
        let two = |kind| {
            ir::Processor::Filter(match processor {
                ir::Processor::Filter(f) => ir::Filter { kind, ..f },
                _ => unreachable!(),
            })
        };
        let kind = match processor {
            ir::Processor::Filter(f) => f.kind,
            _ => return Ok(vec![self.processor(owner, processor)?]),
        };
        let half = match kind {
            ir::FilterKind::LowPass { poles: 4 } => ir::FilterKind::LowPass { poles: 2 },
            ir::FilterKind::HighPass { poles: 4 } => ir::FilterKind::HighPass { poles: 2 },
            ir::FilterKind::BandPass { poles: 4 } => ir::FilterKind::BandPass { poles: 2 },
            ir::FilterKind::Notch { poles: 4 } => ir::FilterKind::Notch { poles: 2 },
            _ => return Ok(vec![self.processor(owner, processor)?]),
        };
        Ok(vec![
            self.processor(owner, two(half))?,
            self.processor(owner, two(half))?,
        ])
    }

    fn processor(&self, owner: &str, processor: ir::Processor) -> Result<Processor, LowerError> {
        Ok(match processor {
            ir::Processor::Gain(gain) => Processor::Gain(gain.linear()),
            ir::Processor::Pan(pan) => Processor::StereoMatrix(stereo(pan)),
            ir::Processor::StereoMatrix(matrix) => Processor::StereoMatrix(matrix),
            ir::Processor::Reverb(r) => Processor::Reverb(ReverbSettings {
                decay_seconds: r.decay_seconds,
                size: r.size,
                damping_hz: r.damping_hz,
                modulation_seconds: r.modulation_seconds,
                diffusion: r.diffusion,
                predelay_seconds: r.predelay_seconds,
                input_cutoff_hz: r.input_cutoff_hz,
                low_shelf_db: r.low_shelf_db,
                width: r.width,
            }),
            ir::Processor::Compressor(c) => Processor::Compressor(CompressorSettings {
                threshold_db: c.threshold_db,
                ratio: c.ratio,
                attack_seconds: c.attack.seconds(),
                release_seconds: c.release.seconds(),
                makeup: c.makeup.linear(),
                link: c.link,
            }),
            ir::Processor::Branch { gain, first, last, .. } => Processor::Branch {
                count: 0,
                gain: gain.linear(),
                first,
                last,
            },
            ir::Processor::Daft(d) => Processor::Daft(DaftSettings {
                gain: Parameter::Constant(d.gain),
                cutoff: Parameter::Constant(d.cutoff),
                resonance: Parameter::Constant(d.resonance),
                response: Parameter::Constant(if d.highpass { 1.0 } else { 0.0 }),
            }),
            ir::Processor::Rectify(mode) => Processor::Rectify(match mode {
                ir::Rectifier::Full => Rectifier::Full,
                ir::Rectifier::Half => Rectifier::Half,
            }),
            ir::Processor::Mix { address, .. } => Processor::Mix {
                // Filled in by `lower_list`, which knows the lowered span.
                count: 0,
                dry: self.slot_range(SlotKind::Dry, address),
                wet: self.slot_range(SlotKind::Output, address),
                bypass: self.slot_range(SlotKind::Bypass, address),
            },
            ir::Processor::Convolution { impulse, dry, wet } => Processor::Convolution {
                impulse: impulse.0,
                dry,
                wet,
            },
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
            ir::FilterKind::LowPass { poles: 1 } => svf(SvfMode::OnePoleLowPass),
            ir::FilterKind::HighPass { poles: 1 } => svf(SvfMode::OnePoleHighPass),
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
        let impulses = self
            .ir
            .impulses
            .iter()
            .enumerate()
            .map(|(i, impulse)| {
                let (left, right) = (
                    resample(&impulse.left, impulse.rate, self.rate),
                    resample(&impulse.right, impulse.rate, self.rate),
                );
                Impulse::new(left, right).map_err(core(Stage::Buses, format!("impulse {i}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut buses = Vec::with_capacity(self.ir.buses.len());
        for (i, bus) in self.ir.buses.iter().enumerate() {
            let owner = format!("bus {i}");
            let mut processors = Vec::new();
            if let Some(chain) = bus.chain {
                let chain = &self.ir.chains[chain.0];
                if chain.scope != ir::Scope::Bus(ir::BusRef(i)) {
                    return Err(unsupported(owner, Feature::ChainScope(chain.scope)));
                }
                let listed: Vec<_> = chain
                    .pre_amplitude
                    .iter()
                    .chain(&chain.post_amplitude)
                    .collect();
                processors = self.lower_list(&owner, &listed)?;
            }
            let tapped = self
                .ir
                .groups
                .iter()
                .any(|g| g.tap.as_ref().is_some_and(|t| t.bus.0 == i));
            let mut sends = vec![BusSend {
                bus: target(bus.output),
                gain: if tapped { 1.0 } else { bus.gain.linear() },
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
            // Linear stages without memory leave no tail; a reverb rings for its own.
            let tail_frames = processors
                .iter()
                .map(|p| match p {
                    Processor::Gain(_)
                    | Processor::StereoMatrix(_)
                    | Processor::Compressor(_)
                    | Processor::Rectify(_)
                    | Processor::Branch { .. }
                    | Processor::Mix { .. } => 0,
                    Processor::Reverb(r) => r.tail_frames(self.rate),
                    Processor::Convolution { impulse, .. } => {
                        crate::dsp::impulse_tail_frames(&impulses[*impulse]) as u32
                    }
                    _ => (BUS_TAIL_SECONDS * f64::from(self.rate)) as u32,
                })
                .max()
                .unwrap_or(0);
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
        let faders: Vec<_> = self
            .ir
            .groups
            .iter()
            .map(|g| {
                g.tap.as_ref().map(|t| crate::GroupFader {
                    bus: t.bus.0,
                    follows: std::iter::once(0)
                        .chain(t.post.iter().map(|n| n + 1))
                        .collect(),
                    initial: self.ir.buses[t.bus.0].gain.linear(),
                })
            })
            .collect();
        let plan = plan
            .with_impulses(impulses)
            .with_buses(buses, bindings)
            .map_err(core(Stage::Buses, "buses"))?;
        let plan = plan.with_bus_addresses(
            self.ir
                .bus_addresses
                .iter()
                .map(|&(address, bus)| (address, bus.0))
                .collect(),
        );
        if faders.iter().all(Option::is_none) {
            return Ok(plan);
        }
        plan.with_group_faders(faders)
            .map_err(core(Stage::Buses, "group faders"))
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

    /// The native keyswitch map and driver table for `switching`, numbering
    /// articulations as the plan does.
    pub(crate) fn switching(
        &self,
        switching: ir::Switching,
    ) -> Result<(Vec<Keyswitch>, Switching), LowerError> {
        let articulations = &self.ir.articulations;
        let default = articulations.iter().position(|a| a.default).unwrap_or(0);
        let id = |index: usize| match index {
            i if i == default => 0,
            i if i < default => i as u32 + 1,
            i => i as u32,
        };
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
                    match a.switch_keys.first() {
                        Some(&key) => Switch::Tap(key),
                        None => Switch::Control { id: a.control?, articulation: id(i) },
                    }
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
        Ok((switches, switching))
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
        let (switches, switching) = self.switching(self.ir.switching)?;
        let tags = self
            .ir
            .zones
            .iter()
            .map(|z| z.articulation.map(|a| id(a.0)))
            .collect();
        plan.with_articulations(
            tags,
            switches,
            SelectionPolicy::Onset,
            SelectionPolicy::Onset,
        )
        .map(|plan| plan.with_switching(switching))
        .map_err(core(Stage::Articulations, "articulations"))
    }

    /// Keyswitches for the nested selectors' choices.
    fn axes(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        let keys: Vec<_> = self
            .ir
            .axes
            .iter()
            .enumerate()
            .flat_map(|(axis, a)| {
                a.choices.iter().enumerate().flat_map(move |(choice, c)| {
                    c.switch_keys.iter().map(move |&key| (key, axis, choice as u32))
                })
            })
            .collect();
        if keys.is_empty() {
            return Ok(plan);
        }
        plan.with_axis_switches(keys)
            .map_err(core(Stage::Articulations, "nested selector keys"))
    }

    fn controllers(&self, plan: Prepared) -> Result<Prepared, LowerError> {
        if self
            .ir
            .zones
            .iter()
            .all(|z| z.conditions.is_empty() && z.axes.is_empty() && previous_key(z.trigger).is_none())
        {
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
                    .chain(previous_key(z.trigger))
                    .chain(z.axes.iter().map(|p| {
                        let value = p.choice as u32;
                        ControllerCondition {
                            controller: crate::AXIS_BASE.saturating_add(p.axis as u8),
                            low: value,
                            high: value,
                        }
                    }))
                    .collect()
            })
            .collect();
        let count = conditions.iter().map(Vec::len).sum();
        plan.with_controllers(conditions, count)
            .map_err(core(Stage::Controllers, "controller ranges"))
    }
}

/// The previous-key condition a trigger kind stands for: no other key held
/// (first), any other held (legato), or a recorded interval (transition).
fn previous_key(trigger: ir::Trigger) -> Option<ControllerCondition> {
    let (low, high) = match trigger {
        ir::Trigger::First => (None, None),
        ir::Trigger::Legato => (Some(-127), Some(127)),
        ir::Trigger::Transition { low, high } => (Some(low.into()), Some(high.into())),
        _ => return None,
    };
    Some(ControllerCondition {
        controller: crate::PREVIOUS_KEY,
        low: crate::previous_key_value(low),
        high: crate::previous_key_value(high),
    })
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

/// `x` at `to` Hz instead of `from`: Blackman-windowed sinc, lowpassed below
/// the lower Nyquist. Run at load, never on the audio thread.
pub(crate) fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return x.to_vec();
    }
    const HALF: f64 = 16.0;
    let step = f64::from(from) / f64::from(to);
    let cutoff = (1.0 / step).min(1.0);
    let reach = HALF / cutoff;
    let frames = ((x.len() as f64 / step).ceil() as usize).max(1);
    (0..frames)
        .map(|i| {
            let at = i as f64 * step;
            let first = ((at - reach).ceil().max(0.0)) as usize;
            let last = ((at + reach).floor() as usize).min(x.len().saturating_sub(1));
            let mut sum = 0.0;
            for (j, v) in x.iter().enumerate().take(last + 1).skip(first) {
                let d = j as f64 - at;
                let t = d * cutoff;
                let sinc = if t == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
                };
                let w = d / reach;
                let window = 0.42
                    + 0.5 * (std::f64::consts::PI * w).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                sum += f64::from(*v) * sinc * window * cutoff;
            }
            sum as f32
        })
        .collect()
}
