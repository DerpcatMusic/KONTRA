//! Prepared per-voice modulation: LFOs, envelopes and note/controller sources
//! routed to gain, pan, pitch, filter cutoff/Q, a per-voice tone filter and
//! sample start.
//!
//! Rate: sources and routes are evaluated once per render chunk, at most
//! [`crate::dsp::BLOCK`] frames, at the chunk's end time. Gain and pan ramp
//! linearly across the chunk from the previous control point; pitch, cutoff,
//! Q and tone hold the chunk's midpoint value. Each kind of work is dispatched
//! once per chunk and voice, never per sample.
//!
//! Layout: every prepared program is flat arrays (sources, routes, shapes);
//! runtime state is structure-of-arrays sized at runtime construction for the
//! largest program, so rendering never allocates.
pub mod laws;

use crate::control::{RealControlIndex, RealValuesRef};
use crate::{Envelope, Error, blocks::envelope::EnvelopeState};
pub use crate::blocks::envelope::{Breakpoint, Breakpoints};
use crate::blocks::envelope::Segment;
pub use crate::blocks::lfo::{Lfo, LfoInputs, LfoRate, LfoShape};
use crate::blocks::lfo::{hash, uniform, wave};

struct Shape {
    points: Box<[(f32, f32)]>,
    table: bool,
}

impl Shape {
    fn new(points: Box<[(f32, f32)]>) -> Self {
        let last = points.len().saturating_sub(1);
        let table = last > 0
            && points
                .iter()
                .enumerate()
                .all(|(i, p)| p.0 == (i as f64 / last as f64) as f32);
        Self { points, table }
    }

    fn evaluate(&self, x: f32) -> f32 {
        let points = &self.points;
        // Port 0cb7a8a0:src/engine/params.rs::Mod::shape table indexing.
        // Check rounded brackets, then retain v2's original interpolation bits.
        let before = |i: usize| {
            #[cfg(test)]
            SHAPE_PROBES.set(SHAPE_PROBES.get() + 1);
            points[i].0 < x
        };
        let search = || {
            points.partition_point(|p| {
                #[cfg(test)]
                SHAPE_PROBES.set(SHAPE_PROBES.get() + 1);
                p.0 < x
            })
        };
        let after = if self.table && x.is_finite() {
            let last = points.len() - 1;
            let position = x * last as f32;
            let index = (position as usize).min(last);
            if before(index) {
                let next = index + 1;
                if next < points.len() && before(next) {
                    search()
                } else {
                    next
                }
            } else if index > 0 && !before(index - 1) {
                search()
            } else {
                index
            }
        } else {
            search()
        };
        interpolate(points, x, after)
    }
}

/// What a route reads. Lfo is bipolar (-1..=1); every other source is 0..=1.
#[derive(Clone, Copy, Debug)]
pub enum ModSource {
    Lfo(Lfo),
    /// Stored envelope-follower fields (attack, decay, gain, adaption), held
    /// per voice; follows the note's group audio, 0..=1.
    Follower([f32; 4]),
    /// Kontakt glide ramp (stored mode and time, ms): starts at the note's
    /// interval from the previous note, octave-normalized, bipolar.
    Glide { mode: u32, time_ms: f32 },
    /// Gated by the voice's family: releases with it.
    Envelope(Envelope),
    /// Index into [`ModProgram::breakpoints`]; gated like `Envelope`.
    Breakpoints(usize),
    Velocity,
    /// Note number / 127.
    Key,
    /// Current effective value of a controller in the note's performance domain.
    Controller(u8),
    /// The note's expression pressure (MPE channel pressure, poly pressure).
    Pressure,
    /// The note's expression timbre (MPE CC74).
    Timbre,
    /// A uniform value drawn once per voice.
    Random,
    Constant,
    /// `clamp(1 − held / frames, 0, 1)`: the share of a `frames` countdown
    /// left when the key was released (now, while it is down).
    ReleaseCounter {
        frames: u32,
    },
    /// The note's script value for "from script" modulator `id`
    /// ([`crate::Instruction::WriteModValue`]), `clamp(v / 1e6, -1, 1)`.
    Script(u16),
    /// [`Self::Script`] read as a bipolar source (-1..=1).
    ScriptBipolar(u16),
    ConstantProperty{node:u32,value:crate::ControlId,style:crate::ControlId,bipolar:bool},
    /// The note's raw pitch bend ([`crate::Expression::bend`]), -1..=1.
    PitchBend,
}

impl ModSource {
    fn bipolar(&self) -> bool {
        matches!(self, Self::Lfo(_) | Self::Glide { .. } | Self::PitchBend | Self::ScriptBipolar(_) | Self::ConstantProperty{bipolar:true,..})
    }
}

/// Destination and law; `v` is the route's transformed source value and `u`
/// its unipolar view ((v + 1) / 2 for bipolar sources).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompiledTarget {
    /// gain × (1 − depth·(1 − u)).
    Attenuate,
    /// gain × 10^(depth·v / 20); depth in decibels.
    Decibels,
    /// pan + depth·v, saturated to -1..=1.
    Pan,
    /// pitch + depth·v semitones.
    Pitch,
    /// Every voice-chain state-variable filter's cutoff × 2^(depth·v / 12).
    Cutoff,
    /// Every voice-chain state-variable filter's Q × 10^(depth·v / 20).
    Resonance,
    /// Actual compiled filter index, preserving the addressed source stage.
    ProcessorCutoff(u32),
    ProcessingParameter { scope: u32, binding: u32 },
    ProcessorResonance(u32),
    /// Add normalized depth to the native filter's saved cutoff knob.
    ProcessorNativeCutoff(u32),
    ProcessorNativeResonance(u32),
    ProcessorNativeGain(u32),
    /// A per-voice low-pass, open (bypassed) at 0, closing by depth·v semitones
    /// below the open cutoff when the sum is negative.
    Tone,
    /// Start offset of depth·u × the region's start range, at voice start only.
    SampleStart,
    Control(usize),
    SourcePhase(u32),
    /// Rate of LFO source `n`: the normalized frequency lane (ID26, additive
    /// set of FUN_140873040: `$out := $in + E * $intensity`) moves by
    /// depth·v each control cell; the LFO integrates the resulting rate.
    SourceRate(u32),
    /// Offset of normalized position, first form amount, or second form amount.
    Wavetable(u8),
    /// A stage time of the voice's amplitude envelope (Kontakt ids 184..187).
    /// The stage index is the core's `EnvelopeStage` ordinal.
    VoiceEnvelopeStage(u8),
    LoopStart,
    LoopLength,
}

/// Open, plan-local destination identity. Legacy constructors preserve native laws.
pub type ModTarget = crate::ParameterAddress;

impl crate::ParameterAddress {
    pub(crate) fn compiled(self) -> Option<CompiledTarget> {
        if let crate::ParameterScope::ProcessingScope(scope)=self.scope {
            return (self.parameter==0).then_some(CompiledTarget::ProcessingParameter{scope,binding:self.node});
        }
        if self.scope != crate::ParameterScope::Voice {
            return None;
        }
        use CompiledTarget as T;
        Some(match (self.node, u32::MAX - self.parameter) {
            (0, 0) => T::Attenuate,
            (0, 1) => T::Decibels,
            (0, 2) => T::Pan,
            (0, 3) => T::Pitch,
            (0, 4) => T::Cutoff,
            (0, 5) => T::Resonance,
            (i, 6) => T::ProcessorCutoff(i),
            (i, 7) => T::ProcessorResonance(i),
            (i, 8) => T::ProcessorNativeCutoff(i),
            (i, 9) => T::ProcessorNativeResonance(i),
            (i, 10) => T::ProcessorNativeGain(i),
            (0, 11) => T::Tone,
            (0, 12) => T::SampleStart,
            (i, 14) => T::SourcePhase(i),
            (i, 17) => T::SourceRate(i),
            (0, 18) => T::Wavetable(0),
            (0, 19) => T::Wavetable(1),
            (0, 20) => T::Wavetable(2),
            (0, 21) => T::VoiceEnvelopeStage(0),
            (0, 22) => T::VoiceEnvelopeStage(1),
            (0, 23) => T::VoiceEnvelopeStage(2),
            (0, 24) => T::VoiceEnvelopeStage(3),
            (0, 25) => T::VoiceEnvelopeStage(4),
            (0, 15) => T::LoopStart,
            (0, 16) => T::LoopLength,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModRoute {
    /// Index into [`ModProgram::sources`].
    pub source: usize,
    pub target: ModTarget,
    pub depth: f64,
    /// Unipolar `1 - v`, bipolar `-v`, before the shape.
    pub invert: bool,
    /// Index into [`ModProgram::shapes`].
    pub shape: Option<usize>,
    /// One-pole lag frames to reach 99% of a step (Kontakt's lag law).
    pub lag: u32,
    /// Depth modifier from a second source, using its selected combination law.
    pub scale: Option<ModScale>,
    /// Multiplies a live depth control's value (a saved `depth` is already in
    /// destination units). A lane's range span: Normalized depth is a fraction of it.
    pub live_gain: f64,
    /// The target's law reads the source's unipolar view (FUN_140873040 `need_uni`).
    pub unipolar: bool,
    /// Further intensity assignments to the same route, applied in order after
    /// `scale`: each template's `$in` is the previous result.
    pub more_scales: [Option<ModScale>; MORE_SCALES],
    /// Native identity of a Kontakt external route; see [`ModNative`].
    pub native: Option<ModNative>,
}

/// Native identity a Kontakt external route carries from its record: the
/// serialized target name's native id, the target flag byte (bit 0x04 selects
/// the ID25 combination) and the external source type (+0x544). With a proved
/// identity the router prepares the source and combines it with
/// [`laws`] (`prepare_source` / `combine`); otherwise the route keeps the
/// generic path ([`laws::external_source_q`] names the reason).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModNative {
    pub target: u16,
    pub flags: u8,
    pub source_type: u8,
}

/// Intensity assignments a route takes beyond its first.
pub const MORE_SCALES: usize = 8;
const SCALE_LAGS: usize = MORE_SCALES + 1;

/// Changes an outgoing route's depth using another source. The default law
/// multiplies by its unipolar view (or shaped value). Evaluated with the route.
#[derive(Clone, Copy, Debug)]
pub struct ModScale {
    /// Index into [`ModProgram::sources`].
    pub source: usize,
    /// Index into [`ModProgram::shapes`]; its output is used unmapped.
    pub shape: Option<usize>,
    /// Generic product or checked native per-target intensity combination.
    pub law: ModScaleLaw,
    /// One-pole lag frames on the processed source (99% of a step; FUN_140873040
    /// smooths the complete expression E, template 0x144f1d950). 0: none.
    pub lag: u32,
}

/// How a second source changes one outgoing target's saved depth.
#[derive(Clone, Copy, Debug, Default)]
pub enum ModScaleLaw {
    #[default]
    Multiply,
    /// Kontakt ID25; `depth` belongs to the intensity assignment, not the
    /// outgoing target. Only flag 0x04 selects the combination expression.
    /// `unit` is outgoing depth units per normalized target intensity; the
    /// evaluator converts to normalized intensity before applying this law.
    /// Preparation admits saved flags 0x10/0x14, unshaped/unlagged non-live
    /// sources only. Native initialization and live writes stay unsupported.
    KontaktIntensity { depth: f64, flags: u8, unit: f64 },
}

impl ModScaleLaw {
    /// `source` is already conditionally inverted; `source_bipolar` selects
    /// its unipolar view independently of the target flags. `base` is normalized
    /// intensity; unit conversion belongs to the caller. No clamping. This is
    /// the derived expression law, not native fcinv_* or callback execution.
    pub fn apply(self, base: f64, source: f64, source_bipolar: bool) -> f64 {
        let source = unipolar(source, source_bipolar);
        match self {
            Self::Multiply => base * source,
            Self::KontaktIntensity { depth, flags, .. } if flags & 4 == 0 =>
                base * (1. - (1. - source) * depth),
            Self::KontaktIntensity { depth, .. } =>
                1. - (1. - base) * (1. - source * depth),
        }
    }
}

impl ModRoute {
    pub fn new(source: usize, target: ModTarget, depth: f64) -> Self {
        Self {
            source,
            target,
            depth,
            invert: false,
            shape: None,
            lag: 0,
            scale: None,
            live_gain: 1.,
            unipolar: false,
            more_scales: [None; MORE_SCALES],
            native: None,
        }
    }

    /// Every intensity assignment of the route, in application order.
    pub fn scales(&self) -> impl Iterator<Item = ModScale> + '_ {
        self.scale.iter().chain(self.more_scales.iter().flatten()).copied()
    }

    /// [`Self::scales`], to edit in place.
    pub fn scales_mut(&mut self) -> impl Iterator<Item = &mut ModScale> + '_ {
        self.scale.iter_mut().chain(self.more_scales.iter_mut().flatten())
    }
}

/// One voice modulation program, shared by the regions bound to it.
#[derive(Clone, Debug, Default)]
pub struct ModProgram {
    /// Sparse outgoing route index -> live depth and source bypass controls.
    pub controls: Vec<(usize, [crate::ControlId; 2])>,
    pub sources: Vec<ModSource>,
    pub breakpoints: Vec<Breakpoints>,
    pub routes: Vec<ModRoute>,
    /// Piecewise-linear transfer curves over 0..=1 as ascending (x, y) points.
    pub shapes: Vec<Vec<(f64, f64)>>,
    /// Envelope source index -> its stage controls (attack, hold, decay,
    /// sustain, release, attack curve; KSP `ENGINE_PAR_ATTACK`.. on the
    /// modulator's slot). Read when a voice starts, like the amplitude envelope.
    pub envelope_controls: Vec<(usize, [Option<crate::ControlId>; 6])>,
}

/// Declarative source row of an actual compiled per-activation scope. Runtime
/// reads must additionally validate its real ProcessingOwner/Plan generation.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ProcessingSourceRef {pub scope:usize,pub source:usize}
/// Bind an existing physical program source slot to that real owner source.
/// The original source descriptor is preparation metadata, not another state.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ModProvider {pub program:usize,pub source:usize,pub provider:ProcessingSourceRef}

#[derive(Clone, Copy)]
enum Prepared {
    Provider(ProcessingSourceRef),
    Lfo(Lfo),
    Envelope(usize),
    /// Index into the program's breakpoint envelopes and the voice's segments.
    Breakpoints(usize),
    /// Slot among the program's followers, and its stored fields.
    Follower{slot:usize,spec:[f32;4]},
    /// Slot among the program's glides, and its stored fields.
    Glide{slot:usize,mode:u32,time_ms:f32},
    Input(InputSource),
}

/// Inputs with no per-voice oscillator/envelope state. Only this type can be
/// projected directly from a note/controller snapshot.
#[derive(Clone, Copy)]
enum InputSource {
    Velocity,
    Key,
    Controller(u8),
    Pressure,
    Timbre,
    Random,
    Constant,
    ReleaseCounter(u32),
    Script(u16),
    ConstantProperty{node:u32,value:crate::ControlId,style:crate::ControlId,bipolar:bool},
    PitchBend,
}

/// A route whose source preparation and combination follow `laws`.
struct NativeLaw {
    target: u16,
    flags: u8,
    /// Source-domain argument (bipolar source).
    q: bool,
    table: Option<laws::Table>,
    /// The external source's type selects the compiler's `fcubic` step
    /// (types 7..=13, 0x140873120).
    cubic: bool,
}

struct Program {
    native: Option<crate::whole_voice::Program>,
    native_controls: Box<[Option<([RealControlIndex; 2], f64)>]>,
    native_flex_control: Option<(f64, Option<[RealControlIndex; 2]>)>,
    sources: Box<[Prepared]>,
    provider_scope:Option<usize>,
    fades: Box<[Option<Box<[f32]>>]>,
    bipolar: Box<[bool]>,
    envelopes: Box<[Envelope]>,
    /// Per envelope: control indices of its stages ([`ModProgram::envelope_controls`]).
    envelope_controls: Box<[[Option<RealControlIndex>; 6]]>,
    breakpoints: Box<[Breakpoints]>,
    routes: Box<[ModRoute]>,
    /// Per route: the native law when its identity and table are proved.
    laws: Box<[Option<NativeLaw>]>,
    targets: Box<[CompiledTarget]>,
    controls: Box<[(usize, [RealControlIndex; 2])]>,
    filter_indices: Box<[usize]>,
    /// Prepared source order and incoming phase-route spans; no callback graph scans.
    source_order: Box<[usize]>,
    phase_routes: Box<[Box<[usize]>]>,
    /// Incoming rate routes per LFO source, and that LFO's saved rate as a point
    /// of the normalized frequency lane (NaN: unrouted).
    rate_routes: Box<[Box<[usize]>]>,
    rate_lane: Box<[f64]>,
    loop_routes: Box<[(usize, Option<[RealControlIndex; 2]>)]>,
    shapes: Box<[Shape]>,
    /// Whether any route reaches each kind of output, so unused work is skipped.
    filter: bool,
    tone: bool,
    start: bool,
    /// v1 settled-result reuse, restricted to sources without clocks or lag.
    cacheable: bool,
    control: bool,
}

/// Immutable per-plan programs and region bindings.
#[derive(Default)]
pub(crate) struct VoiceModulation {
    programs: Box<[Program]>,
    regions: Box<[Option<u32>]>,
    start_ranges: Box<[u32]>,
    sources: usize,
    envelopes: usize,
    breakpoints: usize,
    followers: usize,
    glides: usize,
    routes: usize,
}

/// What sizes a [`VoiceModState`], so it can be built without the plan.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModShape {
    native: bool,
    empty: bool,
    sources: usize,
    envelopes: usize,
    breakpoints: usize,
    followers: usize,
    glides: usize,
    routes: usize,
}

impl VoiceModulation {
    /// Conservative checked preparation-window price for the scoped subset.
    /// Includes owned caller vector capacities plus prepared arrays and compiler
    /// temporary rows. Native whole-voice programs are not part of this path.
    pub(crate) fn scoped_preparation_bytes(programs:&[ModProgram],program_capacity:usize,bindings:&[Option<usize>],binding_capacity:usize)->Result<usize,Error>{
        fn add<T>(n:&mut usize,count:usize)->Result<(),Error>{
            *n=n.checked_add(std::alloc::Layout::array::<T>(count).map_err(|_|Error::Capacity)?.size()).ok_or(Error::Capacity)?;Ok(())
        }
        let mut n=0;
        add::<ModProgram>(&mut n,program_capacity)?;add::<Option<usize>>(&mut n,binding_capacity)?;
        add::<Program>(&mut n,programs.len())?;add::<Option<u32>>(&mut n,bindings.len())?;add::<u32>(&mut n,bindings.len())?;
        for p in programs {
            // Bound preparation work before dependency-order / curve compiling.
            if p.sources.len()>1024||p.routes.len()>4096||p.controls.len()>4096||p.shapes.len()>4096||p.breakpoints.len()>1024{return Err(Error::Capacity);}
            add::<ModSource>(&mut n,p.sources.capacity())?;add::<ModRoute>(&mut n,p.routes.capacity())?;
            add::<(usize,[crate::ControlId;2])>(&mut n,p.controls.capacity())?;
            add::<(usize,[Option<crate::ControlId>;6])>(&mut n,p.envelope_controls.capacity())?;
            add::<Vec<(f64,f64)>>(&mut n,p.shapes.capacity())?;add::<Breakpoints>(&mut n,p.breakpoints.capacity())?;
            for curve in &p.shapes{add::<(f64,f64)>(&mut n,curve.capacity())?;add::<(f32,f32)>(&mut n,curve.len())?;}
            for envelope in &p.breakpoints{add::<Breakpoint>(&mut n,envelope.points.capacity())?;}
            let source=p.sources.len();let routes=p.routes.len();
            add::<Prepared>(&mut n,source)?;add::<bool>(&mut n,source.checked_mul(2).ok_or(Error::Capacity)?)?;
            add::<Option<Box<[f32]>>>(&mut n,source)?;add::<Envelope>(&mut n,source)?;
            add::<[Option<RealControlIndex>;6]>(&mut n,source)?;add::<Shape>(&mut n,p.shapes.len())?;
            // Owned source-rate/phase dependencies retain their incoming index
            // rows. The compiler reserves exact per-target counts; each route
            // contributes to at most one row, including box-conversion overlap.
            add::<Vec<usize>>(&mut n,source.checked_mul(2).ok_or(Error::Capacity)?)?;
            add::<Box<[usize]>>(&mut n,source.checked_mul(2).ok_or(Error::Capacity)?)?;
            let dependencies=p.routes.iter().filter(|route|matches!(route.target.compiled(),Some(CompiledTarget::SourceRate(_)|CompiledTarget::SourcePhase(_)))).count();
            add::<usize>(&mut n,dependencies.checked_mul(2).ok_or(Error::Capacity)?)?;
            add::<usize>(&mut n,source)?;add::<f64>(&mut n,source)?;
            add::<CompiledTarget>(&mut n,routes)?;add::<Option<NativeLaw>>(&mut n,routes)?;
            add::<ModRoute>(&mut n,routes)?;add::<(usize,[RealControlIndex;2])>(&mut n,p.controls.len())?;
            add::<Breakpoints>(&mut n,p.breakpoints.len())?;
            add::<Option<usize>>(&mut n,p.breakpoints.len())?;add::<Breakpoints>(&mut n,p.breakpoints.len())?;
            for source in &p.sources {
                if matches!(source,ModSource::Lfo(Lfo{shape:LfoShape::DigitalSine{..}|LfoShape::NativeFaded{..},..})) {
                    // Public KontaktLfoFade::from_saved enforces <=5000ms at
                    // <=384kHz; its native /32 table has at most60001 values.
                    add::<f32>(&mut n,60001)?;
                }
            }
        }
        // Covers prepared-box conversion and actual Vec builder overlap, not
        // allocator headers/RSS or the caller's unrelated bank/source owners.
        n.checked_mul(2).ok_or(Error::Capacity)
    }
    /// Required scalar homes are a compiled-program invariant, not merely an
    /// attachment-time check. Roster/schema/topology replacement must preserve
    /// the SAME actual scope home for every compiled scalar output.
    pub(crate) fn validate_scoped_bindings(&self,scopes:usize,
        mut home:impl FnMut(usize,crate::SourceOwnerControl)->bool,
        mut parameter:impl FnMut(usize,usize)->bool,
        mut gate:impl FnMut(usize)->Option<crate::ProcessingModulationGate>)->Result<(),Error>{
        if (!self.programs.is_empty()||!self.regions.is_empty())&&self.regions.len()!=scopes{return Err(Error::InvalidInput);}
        for scope in 0..scopes {
            let program=self.program(scope).map(|index|self.programs.get(index as usize).ok_or(Error::InvalidInput)).transpose()?;
            if let Some(program)=program {
                for target in &program.targets {
                    let field=match target {
                        CompiledTarget::Attenuate|CompiledTarget::Decibels=>crate::SourceOwnerControl::Gain,
                        CompiledTarget::Pan=>crate::SourceOwnerControl::Pan,
                        CompiledTarget::SourceRate(_)|CompiledTarget::SourcePhase(_)=>continue,
                        CompiledTarget::ProcessingParameter{scope:actual,binding}=>{
                            if *actual as usize!=scope||!parameter(scope,*binding as usize){return Err(Error::InvalidInput);}continue;
                        },
                        _=>return Err(Error::InvalidInput),
                    };
                    if !home(scope,field){return Err(Error::InvalidInput);}
                }
            }
            if let Some(gate)=gate(scope){
                let program=program.ok_or(Error::InvalidInput)?;
                let route=program.routes.get(gate.route).ok_or(Error::InvalidInput)?;
                if !matches!(program.sources.get(gate.source),Some(Prepared::Envelope(_)))
                    ||route.source!=gate.source||route.target!=ModTarget::Attenuate||route.depth!=1.
                    ||route.invert||route.shape.is_some()||route.scale.is_some()||route.more_scales.iter().any(Option::is_some)
                    ||route.lag!=0||route.live_gain!=1.||route.unipolar
                    ||program.controls.iter().any(|(row,_)|*row==gate.route)
                    ||matches!(gate.velocity,crate::VelocityCurve::Power(p)if !p.is_finite()||p<=0.){return Err(Error::InvalidInput);}
            }
        }Ok(())
    }
    pub(crate) fn validate_constant_properties(&self,valid:impl Fn(u32,crate::ControlId,crate::ControlId,bool)->bool)->Result<(),Error>{
        for program in &self.programs{for source in &program.sources{
            if let Prepared::Input(InputSource::ConstantProperty{node,value,style,bipolar})=*source{
                if !valid(node,value,style,bipolar){return Err(Error::InvalidInput);}
            }
        }}Ok(())
    }
    /// Element input programs have a real controller-source producer and no
    /// note-context/native source. Validate again after any program/schema edit.
    pub(crate) fn validate_element_program(&self,index:u32)->Result<usize,Error>{
        let p=self.programs.get(index as usize).ok_or(Error::InvalidInput)?;
        if p.native.is_some()||p.provider_scope.is_some()
            ||p.sources.iter().any(|s|!matches!(s,Prepared::Input(InputSource::Controller(cc)) if *cc<128) && !matches!(s,Prepared::Input(InputSource::Constant|InputSource::ConstantProperty{..}|InputSource::Script(_))))
            ||p.targets.iter().any(|t|!matches!(t,CompiledTarget::Attenuate|CompiledTarget::Decibels|CompiledTarget::Pan))
        {return Err(Error::InvalidInput);}
        Ok(p.sources.len())
    }

    pub(crate) fn element_uses(&self,index:usize,field:crate::SourceOwnerControl)->Result<bool,Error>{
        self.validate_element_program(u32::try_from(index).map_err(|_|Error::Capacity)?)?;
        Ok(self.programs[index].targets.iter().any(|target|match field{
            crate::SourceOwnerControl::Gain=>matches!(target,CompiledTarget::Attenuate|CompiledTarget::Decibels),
            crate::SourceOwnerControl::Pan=>matches!(target,CompiledTarget::Pan),
            crate::SourceOwnerControl::OutputMute|crate::SourceOwnerControl::MidiMute=>false,
        }))
    }

    /// Revalidate compiled processor references after a control-side topology edit.
    /// The target kind and index must still belong to this prepared generation.
    pub(crate) fn validate_dsp_targets(
        &self,
        filters: &crate::dsp::filter::svf::PreparedFilters,
        parameters: usize,
    ) -> Result<(), Error> {
        use crate::dsp::filter::svf::FilterKind;
        let invalid = self
            .programs
            .iter()
            .flat_map(|program| program.targets.iter())
            .any(|target| match *target {
                CompiledTarget::ProcessorCutoff(index)
                | CompiledTarget::ProcessorResonance(index) => !matches!(
                    filters.kind(index as usize),
                    Some(FilterKind::StateVariable)
                ),
                CompiledTarget::ProcessorNativeCutoff(index)
                | CompiledTarget::ProcessorNativeResonance(index)
                | CompiledTarget::ProcessorNativeGain(index) => !matches!(
                    filters.kind(index as usize),
                    Some(FilterKind::NativeControl)
                ),
                CompiledTarget::Control(index) => index >= parameters,
                _ => false,
            });
        if invalid {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }

    /// Re-resolve sparse input cells off audio after a complete schema replacement.
    pub(crate) fn remap_controls(
        &mut self,
        resolve: impl Fn(RealControlIndex) -> Result<RealControlIndex, Error>,
    ) -> Result<(), Error> {
        for program in &self.programs {
            for (_, indices) in &program.controls {
                resolve(indices[0])?;
                resolve(indices[1])?;
            }
            for (indices, _) in program.native_controls.iter().flatten() {
                resolve(indices[0])?;
                resolve(indices[1])?;
            }
            for (_, indices) in &program.loop_routes {
                if let Some(indices)=indices { resolve(indices[0])?; resolve(indices[1])?; }
            }
            for index in program.envelope_controls.iter().flatten().flatten() {
                resolve(*index)?;
            }
            if let Some((_,Some(indices))) = program.native_flex_control {
                resolve(indices[0])?;
                resolve(indices[1])?;
            }
        }
        for program in &mut self.programs {
            for (_, indices) in &mut program.controls {
                *indices = [resolve(indices[0])?, resolve(indices[1])?];
            }
            for (indices, _) in program.native_controls.iter_mut().flatten() {
                *indices = [resolve(indices[0])?, resolve(indices[1])?];
            }
            for (_, indices) in &mut program.loop_routes {
                if let Some(indices)=indices { *indices=[resolve(indices[0])?,resolve(indices[1])?]; }
            }
            for index in program.envelope_controls.iter_mut().flatten().flatten() {
                *index = resolve(*index)?;
            }
            if let Some((_,Some(indices))) = &mut program.native_flex_control {
                *indices = [resolve(indices[0])?,resolve(indices[1])?];
            }
        }
        Ok(())
    }

    pub(crate) fn has_control_targets(&self) -> bool {
        self.programs.iter().any(|p| p.control)
    }

    pub fn shape(&self) -> ModShape {
        ModShape {
            native: self.programs.iter().any(|p| p.native.is_some()),
            empty: self.is_empty(),
            sources: self.sources,
            envelopes: self.envelopes,
            breakpoints: self.breakpoints,
            followers: self.followers,
            glides: self.glides,
            routes: self.routes,
        }
    }
    /// Bytes of [`VoiceModState`] one voice slot needs.
    pub fn bytes_per_voice(&self) -> usize {
        VoiceModState::checked_storage_bytes(self.shape(),1,0).unwrap_or(usize::MAX)
    }
    #[cfg(test)]
    pub fn new(
        programs: Vec<ModProgram>,
        regions: Vec<Option<usize>>,
        start_ranges: Vec<u32>,
    ) -> Result<Self, Error> {
        Self::new_resolved(
            programs,
            regions,
            start_ranges,
            Vec::new(),
            |_| None,
            |_| Err(Error::InvalidInput),
        )
    }

    pub(crate) fn new_resolved(
        programs: Vec<ModProgram>,
        regions: Vec<Option<usize>>,
        start_ranges: Vec<u32>,
        native: Vec<Option<crate::whole_voice::Program>>,
        resolve: impl Fn(crate::ParameterAddress) -> Option<usize>,
        control: impl Fn(crate::ControlId) -> Result<RealControlIndex, Error>,
    ) -> Result<Self, Error> {
        Self::new_resolved_providers(programs,regions,start_ranges,native,Vec::new(),resolve,control)
    }
    pub(crate) fn new_resolved_providers(
        programs:Vec<ModProgram>,regions:Vec<Option<usize>>,start_ranges:Vec<u32>,
        native:Vec<Option<crate::whole_voice::Program>>,providers:Vec<(ModProvider,bool)>,
        resolve:impl Fn(crate::ParameterAddress)->Option<usize>,
        control:impl Fn(crate::ControlId)->Result<RealControlIndex,Error>,
    )->Result<Self,Error>{
        for (i,(binding,_))in providers.iter().enumerate(){
            if programs.get(binding.program).and_then(|p|p.sources.get(binding.source)).is_none()
                ||providers[..i].iter().any(|(old,_)|old.program==binding.program&&old.source==binding.source){return Err(Error::InvalidInput);}
        }
        if start_ranges.len() != regions.len()
            || regions.iter().flatten().any(|p| *p >= programs.len())
            || programs.len() > u32::MAX as usize
        {
            return Err(Error::InvalidInput);
        }
        let mut native = native.into_iter();
        let mut compiled = Vec::with_capacity(programs.len());
        for (program_index,program) in programs.into_iter().enumerate() {
            let mut provider_scope=None;
            for (binding,_)in providers.iter().filter(|(binding,_)|binding.program==program_index){
                if provider_scope.is_some_and(|scope|scope!=binding.provider.scope){return Err(Error::InvalidInput);}
                provider_scope=Some(binding.provider.scope);
            }
            let mut envelopes = Vec::with_capacity(program.sources.iter().filter(|source|matches!(source,ModSource::Envelope(_))).count());
            std::alloc::Layout::array::<Option<usize>>(program.breakpoints.len()).map_err(|_|Error::Capacity)?;
            let mut breakpoint_indices=vec![None;program.breakpoints.len()];
            let mut active_breakpoints=Vec::new();
            let sources = program
                .sources
                .iter()
                .enumerate().map(|(source_index,source)| {
                    if let Some((binding,_))=providers.iter().find(|(binding,_)|binding.program==program_index&&binding.source==source_index){
                        return Ok(Prepared::Provider(binding.provider));
                    }
                    Ok(match *source {
                        ModSource::Lfo(lfo) => {
                            if let LfoShape::DigitalSine { level, .. }
                            | LfoShape::SineScaled(level) = lfo.shape
                                && !(-1.0..=1.0).contains(&level)
                            {
                                return Err(Error::InvalidInput);
                            }
                            if !lfo.valid() {
                                return Err(Error::InvalidInput);
                            }
                            if matches!(lfo.shape, LfoShape::DigitalSine { .. })
                                && (lfo.delay != 0
                                    || lfo.fade != 0
                                    || !lfo.retrigger
                                    || lfo.shared
                                    || matches!(lfo.rate, LfoRate::Beats(_)))
                            {
                                return Err(Error::InvalidInput);
                            }
                            let rate = match lfo.rate {
                                LfoRate::Hertz(r) | LfoRate::Beats(r) => r,
                            };
                            if !(rate.is_finite() && rate > 0.0)
                                || !(0.0..=1.0).contains(&lfo.phase)
                            {
                                return Err(Error::InvalidInput);
                            }
                            Prepared::Lfo(lfo)
                        }
                        ModSource::Envelope(envelope) => {
                            envelopes.push(envelope);
                            Prepared::Envelope(envelopes.len() - 1)
                        }
                        ModSource::Follower(spec) => {
                            if crate::blocks::follower::AdaptiveFollower::from_stored(1.0, spec).is_none() {
                                return Err(Error::InvalidInput);
                            }
                            let slot = program.sources[..source_index]
                                .iter()
                                .filter(|s| matches!(s, ModSource::Follower(_)))
                                .count();
                            Prepared::Follower { slot, spec }
                        }
                        ModSource::Glide { mode, time_ms } => {
                            if !matches!(mode, 1 | 2) || !time_ms.is_finite() || time_ms < 0.0 {
                                return Err(Error::InvalidInput);
                            }
                            let slot = program.sources[..source_index]
                                .iter()
                                .filter(|s| matches!(s, ModSource::Glide { .. }))
                                .count();
                            Prepared::Glide { slot, mode, time_ms }
                        }
                        ModSource::Breakpoints(index) => {
                            let b = program.breakpoints.get(index).ok_or(Error::InvalidInput)?;
                            if !b.valid() {
                                return Err(Error::InvalidInput);
                            }
                            let actual=match breakpoint_indices[index]{Some(actual)=>actual,None=>{
                                let actual=active_breakpoints.len();active_breakpoints.push(b.clone());breakpoint_indices[index]=Some(actual);actual
                            }};
                            Prepared::Breakpoints(actual)
                        }
                        ModSource::Velocity => Prepared::Input(InputSource::Velocity),
                        ModSource::Key => Prepared::Input(InputSource::Key),
                        ModSource::Controller(cc) if cc < 128 => Prepared::Input(InputSource::Controller(cc)),
                        ModSource::Controller(_) => return Err(Error::InvalidInput),
                        ModSource::Pressure => Prepared::Input(InputSource::Pressure),
                        ModSource::Timbre => Prepared::Input(InputSource::Timbre),
                        ModSource::Random => Prepared::Input(InputSource::Random),
                        ModSource::Constant => Prepared::Input(InputSource::Constant),
                        ModSource::ReleaseCounter { frames: 0 } => {
                            return Err(Error::InvalidInput);
                        }
                        ModSource::ReleaseCounter { frames } => Prepared::Input(InputSource::ReleaseCounter(frames)),
                        ModSource::Script(id) | ModSource::ScriptBipolar(id) => Prepared::Input(InputSource::Script(id)),
                        ModSource::ConstantProperty{node,value,style,bipolar}=>Prepared::Input(InputSource::ConstantProperty{node,value,style,bipolar}),
                        ModSource::PitchBend => Prepared::Input(InputSource::PitchBend),
                    })
                })
                .collect::<Result<Box<[_]>, Error>>()?;
            let mut shapes = Vec::with_capacity(program.shapes.len());
            for shape in &program.shapes {
                if shape.is_empty()
                    || shape.windows(2).any(|w| w[1].0 < w[0].0)
                    || shape
                        .iter()
                        .any(|&(x, y)| !(0.0..=1.0).contains(&x) || !y.is_finite())
                {
                    return Err(Error::InvalidInput);
                }
                shapes.push(Shape::new(
                    shape
                        .iter()
                        .map(|&(x, y)| (x as f32, y as f32))
                        .collect::<Box<[_]>>(),
                ));
            }
            for route in &program.routes {
                if route.source >= sources.len()
                    || !route.depth.is_finite()
                    || (matches!(
                        route.target.compiled(),
                        Some(CompiledTarget::LoopStart | CompiledTarget::LoopLength)
                    ) && (!matches!(sources.get(route.source), Some(Prepared::Input(InputSource::Constant)))
                        || route.invert
                        || route.shape.is_some()
                        || route.scale.is_some()
                        || route.lag != 0))
                    || route.shape.is_some_and(|s| s >= shapes.len())
                    || route.scales().any(|s| {
                        s.source >= sources.len() || s.shape.is_some_and(|s| s >= shapes.len())
                            // Native lag/init/inversion/live paths have no numeric proof.
                            // The target's own shape and lag act on its source
                            // expression; `$intensity` is a separate variable
                            // (FUN_140873040 templates), so they stay admissible.
                            || matches!(s.law, ModScaleLaw::KontaktIntensity { depth, flags, unit }
                                if !depth.is_finite() || !(-1. ..=1.).contains(&depth)
                                || !unit.is_finite() || unit == 0.
                                || !(0. ..=1.).contains(&(route.depth / unit))
                                || !matches!(flags & !0x02, 0x10 | 0x14)
                                || route.target == ModTarget::SampleStart)
                    })
                {
                    return Err(Error::InvalidInput);
                }
            }
            // Reserve exact incoming dependency rows before filling them, so
            // scoped preparation can price actual backing without Vec growth.
            let mut phase_routes:Vec<Vec<usize>>=(0..sources.len()).map(|target|Vec::with_capacity(program.routes.iter().filter(|route|matches!(route.target.compiled(),Some(CompiledTarget::SourcePhase(index))if index as usize==target)).count())).collect();
            for (index, route) in program.routes.iter().enumerate() {
                if let Some(CompiledTarget::SourcePhase(target)) = route.target.compiled() {
                    let target = target as usize;
                    if !matches!(sources.get(target), Some(Prepared::Lfo(l)) if !l.shared)
                        || route.lag != 0
                        || route.scales().next().is_some()
                    {
                        return Err(Error::InvalidInput);
                    }
                    phase_routes[target].push(index);
                }
            }
            let mut rate_routes:Vec<Vec<usize>>=(0..sources.len()).map(|target|Vec::with_capacity(program.routes.iter().filter(|route|matches!(route.target.compiled(),Some(CompiledTarget::SourceRate(index))if index as usize==target)).count())).collect();
            let mut rate_lane = vec![f64::NAN; sources.len()];
            for (index, route) in program.routes.iter().enumerate() {
                if let Some(CompiledTarget::SourceRate(target)) = route.target.compiled() {
                    let target = target as usize;
                    // A shared clock or a beat-length cycle has no per-voice frequency lane.
                    let Some(Prepared::Lfo(Lfo { rate: LfoRate::Hertz(hz), shared: false, .. })) =
                        sources.get(target)
                    else {
                        return Err(Error::InvalidInput);
                    };
                    let lane = crate::v1_voice_controls::LfoFrequency::lane(*hz).ok_or(Error::InvalidInput)?;
                    rate_lane[target] = lane;
                    rate_routes[target].push(index);
                }
            }
            let mut source_order = Vec::with_capacity(sources.len());
            let mut emitted = vec![false; sources.len()];
            while source_order.len() < sources.len() {
                let next = (0..sources.len()).find(|&i| {
                    !emitted[i]
                        && phase_routes[i]
                            .iter()
                            .chain(&rate_routes[i])
                            .all(|&r| {
                                let route = &program.routes[r];
                                emitted[route.source]
                                    && route.scales().all(|scale| emitted[scale.source])
                            })
                });
                let Some(next) = next else {
                    return Err(Error::InvalidInput);
                };
                emitted[next] = true;
                source_order.push(next);
            }
            let reaches =
                |targets: &[ModTarget]| program.routes.iter().any(|r| targets.contains(&r.target));
            let mut filter_indices: Vec<_> = program
                .routes
                .iter()
                .filter_map(|route| match route.target.compiled() {
                    Some(
                        CompiledTarget::ProcessorCutoff(i)
                        | CompiledTarget::ProcessorResonance(i)
                        | CompiledTarget::ProcessorNativeCutoff(i)
                        | CompiledTarget::ProcessorNativeResonance(i)
                        | CompiledTarget::ProcessorNativeGain(i),
                    ) => Some(i as usize),
                    _ => None,
                })
                .collect();
            filter_indices.sort_unstable();
            filter_indices.dedup();
            let mut envelope_controls = vec![[None; 6]; envelopes.len()];
            for &(source, ids) in &program.envelope_controls {
                if matches!(sources.get(source),Some(Prepared::Provider(_))){continue;}
                let Some(Prepared::Envelope(e)) = sources.get(source) else {
                    return Err(Error::InvalidInput);
                };
                for (slot, id) in envelope_controls[*e].iter_mut().zip(ids) {
                    *slot = id.map(&control).transpose()?;
                }
            }
            let mut controls = program
                .controls
                .iter()
                .map(|&(route, ids)| {
                    if route >= program.routes.len() {
                        return Err(Error::InvalidInput);
                    }
                    Ok((route, [control(ids[0])?, control(ids[1])?]))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            controls.sort_unstable_by_key(|b| b.0);
            let loop_routes = program
                .routes
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    matches!(
                        r.target.compiled(),
                        Some(CompiledTarget::LoopStart | CompiledTarget::LoopLength)
                    )
                    .then(|| (i, controls.iter().find(|b| b.0 == i).map(|b| b.1)))
                })
                .collect();
            if controls.windows(2).any(|b| b[0].0 == b[1].0) {
                return Err(Error::InvalidInput);
            }
            let fades = sources.iter().map(|s| match s {
                Prepared::Lfo(Lfo {
                    shape: LfoShape::DigitalSine { fade, .. } | LfoShape::NativeFaded { fade, .. },
                    ..
                }) => Some(fade.gains()),
                _ => None,
            }).collect();
            let native_program = native.next().flatten();
            if provider_scope.is_some()&&native_program.is_some(){return Err(Error::InvalidInput);}
            let mut native_controls = native_program.as_ref().map_or_else(Vec::new, |n| {
                vec![None; n.plan.description().mods.assignments().len()]
            });
            if let Some(n) = &native_program {
                for &(route, ids) in &controls {
                    if let crate::whole_voice::Route::External(assignment) = n.routes[route] {
                        let target = n.plan.description().mods.assignments()[assignment as usize].route.unwrap().1;
                        let scale = if target == crate::v1_voice_controls::Target::Pitch { 12. } else { 1. };
                        native_controls[assignment as usize] = Some((ids, scale));
                    }
                }
            }
            let native_flex_control = native_program.as_ref().and_then(|native| {
                let route = native.routes.iter().position(|r| matches!(r,crate::whole_voice::Route::Flex))?;
                Some((program.routes[route].depth,controls.iter().find(|(index,_)| *index == route).map(|(_,ids)| *ids)))
            });
            let native_laws = program
                .routes
                .iter()
                .map(|route| {
                    let n = route.native?;
                    let q = laws::external_source_q(n.source_type).ok()?;
                    // The source must arrive in the domain the law prepares.
                    if q != program.sources.get(route.source)?.bipolar() {
                        return None;
                    }
                    let compiled = route.target.compiled()?;
                    let attenuate = matches!(compiled, CompiledTarget::Attenuate);
                    // Only targets whose native family the router applies exactly:
                    // H as the gain product, everything else as plain addition.
                    // Target scales (eq 0.5, 2/3) are proved only as emitted source
                    // factors; the dB/Hz consumer is open, so those stay generic.
                    if laws::target_scale(n.target, false) != 1.0
                        || attenuate != laws::is_h(n.target, n.flags)
                        || (!attenuate && !laws::is_add(n.target, n.flags))
                        || !matches!(compiled, CompiledTarget::Attenuate | CompiledTarget::Pan
                            | CompiledTarget::Pitch | CompiledTarget::Cutoff
                            | CompiledTarget::Resonance | CompiledTarget::ProcessorCutoff(_)
                            | CompiledTarget::ProcessorResonance(_)
                            | CompiledTarget::ProcessorNativeCutoff(_)
                            | CompiledTarget::ProcessorNativeResonance(_)
                            | CompiledTarget::ProcessorNativeGain(_) | CompiledTarget::Wavetable(_))
                    {
                        return None;
                    }
                    // A stored table is 128 samples (T[128] repeats T[127]);
                    // curve shapers are open, so they stay on the generic path.
                    let table = match route.shape {
                        None => None,
                        Some(shape) => {
                            let shape = &shapes[shape];
                            if !shape.table || shape.points.len() != 128 {
                                return None;
                            }
                            let samples: [f32; 128] = std::array::from_fn(|k| shape.points[k].1);
                            Some(laws::Table::from_curve_samples(&samples))
                        }
                    };
                    Some(NativeLaw {
                        target: n.target,
                        flags: n.flags,
                        q,
                        table,
                        // The compiler cubes the inner expression for an
                        // external source of type 7..=13 (0x140873120).
                        cubic: (7..=13).contains(&n.source_type),
                    })
                })
                .collect::<Box<[_]>>();
            compiled.push(Program {
                provider_scope,
                laws: native_laws,
                controls: controls.into_boxed_slice(),
                fades,
                native: native_program,
                native_controls: native_controls.into_boxed_slice(),
                native_flex_control,
                bipolar: program.sources.iter().enumerate().map(|(source,value)|providers.iter().find(|(binding,_)|binding.program==program_index&&binding.source==source).map_or_else(||value.bipolar(),|(_,bipolar)|*bipolar)).collect(),
                cacheable: sources.iter().all(|s| {
                    matches!(
                        s,
                        Prepared::Input(InputSource::Velocity)
                            | Prepared::Input(InputSource::Key)
                            | Prepared::Input(InputSource::Controller(_))
                            | Prepared::Input(InputSource::Pressure)
                            | Prepared::Input(InputSource::Timbre)
                            | Prepared::Input(InputSource::Random)
                            | Prepared::Input(InputSource::Constant)
                            | Prepared::Input(InputSource::Script(_))
                            | Prepared::Input(InputSource::ConstantProperty{..})
                            | Prepared::Input(InputSource::PitchBend)
                    )
                }) && program.routes.iter().all(|r| r.lag == 0)
                    && program.controls.is_empty(),
                sources,
                filter_indices: filter_indices.into_boxed_slice(),
                source_order: source_order.into_boxed_slice(),
                loop_routes,
                phase_routes: phase_routes
                    .into_iter()
                    .map(Vec::into_boxed_slice)
                    .collect(),
                rate_routes: rate_routes.into_iter().map(Vec::into_boxed_slice).collect(),
                rate_lane: rate_lane.into_boxed_slice(),
                envelopes: envelopes.into_boxed_slice(),
                envelope_controls: envelope_controls.into_boxed_slice(),
                breakpoints: active_breakpoints.into_boxed_slice(),
                filter: reaches(&[ModTarget::Cutoff, ModTarget::Resonance]),
                tone: reaches(&[ModTarget::Tone]),
                start: reaches(&[ModTarget::SampleStart]),
                targets: program
                    .routes
                    .iter()
                    .map(|r| {
                        r.target
                            .compiled()
                            .or_else(|| resolve(r.target).map(CompiledTarget::Control))
                            .ok_or(Error::InvalidInput)
                    })
                    .collect::<Result<_, _>>()?,
                control: program.routes.iter().any(|r| r.target.compiled().is_none()),
                routes: program.routes.into_boxed_slice(),
                shapes: shapes.into_boxed_slice(),
            });
        }
        Ok(Self {
            sources: compiled.iter().map(|p| p.sources.len()).max().unwrap_or(0),
            envelopes: compiled
                .iter()
                .map(|p| p.envelopes.len())
                .max()
                .unwrap_or(0),
            glides: compiled
                .iter()
                .map(|p| p.sources.iter().filter(|s| matches!(s, Prepared::Glide { .. })).count())
                .max()
                .unwrap_or(0),
            followers: compiled
                .iter()
                .map(|p| p.sources.iter().filter(|s| matches!(s, Prepared::Follower { .. })).count())
                .max()
                .unwrap_or(0),
            breakpoints: compiled
                .iter()
                .map(|p| p.breakpoints.len())
                .max()
                .unwrap_or(0),
            routes: compiled.iter().map(|p| p.routes.len()).max().unwrap_or(0),
            programs: compiled.into_boxed_slice(),
            regions: regions.into_iter().map(|p| p.map(|p| p as u32)).collect(),
            start_ranges: start_ranges.into_boxed_slice(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    pub fn program(&self, region: usize) -> Option<u32> {
        self.regions.get(region).copied().flatten()
    }

    /// The ordinary onset consumer. A provider-dependent onset must use the
    /// explicitly borrowed real owner row, never a per-player approximation.
    pub fn start_offset(&self, region:usize, inputs:&Inputs<'_>, seed:u64)->Result<u32,Error>{
        self.start_offset_provided(region,inputs,seed,0,ProviderView::empty())
    }
    pub(crate) fn start_offset_provided(&self,region:usize,inputs:&Inputs<'_>,seed:u64,frame:u64,provider:ProviderView<'_>)->Result<u32,Error>{
        let Some(index)=self.program(region)else{return Ok(0);};
        let program=&self.programs[index as usize];
        if program.provider_scope!=provider.scope{return Err(Error::InvalidInput);}
        if program.provider_scope.is_some(){
            if frame<provider.begin||frame>provider.end{return Err(Error::InvalidInput);}
            for source in &program.sources{if let Prepared::Provider(source)=source{
                if Some(source.scope)!=provider.scope||source.source>=provider.from.len()||source.source>=provider.to.len(){return Err(Error::InvalidInput);}
            }}
        }
        if !program.start{return Ok(0);}
        Ok(self.offset(program,region,|index|match program.sources[index]{
            Prepared::Lfo(lfo)if lfo.delay==0&&lfo.fade==0=>{
                let signal=wave(lfo.shape,lfo.phase,seed^index as u64);
                program.fades[index].as_ref().map_or(signal,|gains|f64::from(signal as f32*gains[0]))
            }
            Prepared::Lfo(_)|Prepared::Envelope(_)|Prepared::Breakpoints(_)|Prepared::Follower{..}|Prepared::Glide{..}=>0.,
            Prepared::Input(input)=>input.input(inputs,seed,index),
            Prepared::Provider(source)=>provider.at(source.source,frame),
        }))
    }

    /// Control-side proof, using the exact onset consumer below. A dependent
    /// route makes the entire result unknown, even when its saved depth is zero.
    pub fn constant_start_offset(&self, region: usize) -> Option<u32> {
        self.start_ranges.get(region)?;
        let Some(index) = self.program(region) else {
            return Some(0);
        };
        let program = &self.programs[index as usize];
        if program.routes.iter().filter(|r| r.target == ModTarget::SampleStart).any(|route| {
            !matches!(program.sources[route.source], Prepared::Input(InputSource::Constant))
                || route.scales().any(|scale| !matches!(program.sources[scale.source], Prepared::Input(InputSource::Constant)))
        }) {
            return None;
        }
        Some(self.offset(program, region, |_| 1.0))
    }

    /// Prediction for explicitly initialized controllers, using the same
    /// compiled offset consumer as voice admission. Unknown dependencies make
    /// the whole prediction unknown, including zero-depth assignments.
    pub fn initialized_start_offset(
        &self,
        region: usize,
        controller: impl Fn(u8) -> Option<u32>,
    ) -> Option<u32> {
        self.start_ranges.get(region)?;
        let Some(index) = self.program(region) else { return Some(0) };
        let program = &self.programs[index as usize];
        let at_start = |index: usize| match program.sources[index] {
            Prepared::Input(InputSource::Constant) => Some(1.0),
            Prepared::Input(InputSource::Controller(cc)) => controller(cc).map(|v| f64::from(v) * FULL_SCALE),
            _ => None,
        };
        for route in program.routes.iter().filter(|r| r.target == ModTarget::SampleStart) {
            at_start(route.source)?;
            for scale in route.scales() { at_start(scale.source)?; }
        }
        Some(self.offset(program, region, |index| {
            at_start(index).expect("every contributing onset source was checked")
        }))
    }

    pub fn has_loop_targets(&self, region: usize) -> bool {
        self.program(region).is_some_and(|index| self.programs[index as usize].targets.iter()
            .any(|target| matches!(target, CompiledTarget::LoopStart | CompiledTarget::LoopLength)))
    }

    fn offset(&self, program: &Program, region: usize, at_start: impl Fn(usize) -> f64) -> u32 {
        let mut fraction = 0.0;
        for route in program.routes.iter().filter(|r| r.target == ModTarget::SampleStart) {
            let bipolar = program.bipolar[route.source];
            let v = program.transform(route, at_start(route.source), bipolar);
            let depth = route.scales().fold(route.depth,
                |d, s| program.depth(d, s, at_start(s.source), &mut 0., 0, true));
            fraction += depth * unipolar(v, bipolar);
        }
        (fraction.clamp(0.0, 1.0) * f64::from(self.start_ranges[region])) as u32
    }
}

impl InputSource {
    fn input(self, inputs: &Inputs<'_>, seed: u64, index: usize) -> f64 {
        match self {
            Self::Velocity => inputs.velocity,
            Self::Key => inputs.key,
            Self::Controller(cc) => f64::from(inputs.controllers[usize::from(cc)]) * FULL_SCALE,
            Self::Pressure => f64::from(inputs.pressure) * FULL_SCALE,
            Self::Timbre => f64::from(inputs.timbre) * FULL_SCALE,
            Self::Random => uniform(hash(seed, index as u64, u64::MAX)),
            Self::Constant => 1.0,
            Self::ConstantProperty{node,value,style,bipolar}=>inputs.controls.constant_property(node,value,style,bipolar),
            Self::PitchBend => inputs.bend,
            Self::Script(id) => (f64::from(inputs.script.get(id)) / 1_000_000.0).clamp(-1.0, 1.0),
            Self::ReleaseCounter(frames) => {
                (1.0 - inputs.held as f64 / f64::from(frames)).clamp(0.0, 1.0)
            }
        }
    }
}

const FULL_SCALE: f64 = 1.0 / u32::MAX as f64;

impl Program {
    fn depth(
        &self,
        base: f64,
        scale: ModScale,
        raw: f64,
        lagged: &mut f64,
        frames: u32,
        onset: bool,
    ) -> f64 {
        let (mut source, bipolar) = match scale.shape {
            Some(shape) => {
                let x = unipolar(raw, self.bipolar[scale.source]);
                (f64::from(self.shapes[shape].evaluate(x as f32)), false)
            }
            None => (raw, self.bipolar[scale.source]),
        };
        if scale.lag != 0 && !onset {
            // Same one-pole law as a target's lag, on the complete expression.
            let alpha = 1.0
                - (-2.0 * std::f64::consts::LN_10 * f64::from(frames) / f64::from(scale.lag)).exp();
            source = *lagged + (source - *lagged) * alpha;
        }
        *lagged = source;
        match scale.law {
            ModScaleLaw::Multiply => base * unipolar(source, bipolar),
            ModScaleLaw::KontaktIntensity { unit, .. } =>
                scale.law.apply(base / unit, source, bipolar) * unit,
        }
    }

    /// Live depth control of route `i`, else its saved depth.
    fn base_depth(&self, i: usize, inputs: &Inputs<'_>) -> f64 {
        self.base_depth_controls(i,inputs.controls)
    }
    fn base_depth_controls(&self,i:usize,controls:RealValuesRef<'_>)->f64{
        match self.controls.binary_search_by_key(&i, |c| c.0) {
            Ok(at) => {
                let [depth, bypass] = self.controls[at].1;
                if controls.get(bypass) == 0. { controls.get(depth) * self.routes[i].live_gain } else { 0. }
            }
            Err(_) => self.routes[i].depth,
        }
    }

    /// One residual route at a control point: its processed source value
    /// (inversion, shape, lag) and effective depth (second-source scale).
    fn route_point(
        &self,
        index: usize,
        route: &ModRoute,
        values: &[f64],
        lagged: &mut f64,
        scale_lagged: &mut [f64],
        base: f64,
        frames: u32,
        onset: bool,
    ) -> (f64, f64) {
        let mut v = match &self.laws[index] {
            // Prepared source E (Kontakt FUN_140873040): shaping, target
            // scale, then conditional inversion, in binary32.
            Some(law) => f64::from(laws::prepare_source(
                &laws::Prep {
                    target: law.target,
                    q: law.q,
                    table: law.table.as_ref(),
                    external_type7: false,
                    omit_range: false,
                    special3: false,
                    invert: route.invert,
                },
                values[route.source] as f32,
            )),
            None => {
                let mut v = self.transform(route, values[route.source], self.bipolar[route.source]);
                if route.unipolar {
                    v = unipolar(v, self.bipolar[route.source]);
                }
                v
            }
        };
        if route.lag != 0 && !onset {
            // 99% of a step in `lag` frames: Kontakt's one-pole lag law.
            let alpha = 1.0
                - (-2.0 * std::f64::consts::LN_10 * f64::from(frames) / f64::from(route.lag)).exp();
            v = *lagged + (v - *lagged) * alpha;
        }
        *lagged = v;
        let d = route
            .scales()
            .zip(scale_lagged)
            .fold(base, |d, (s, lag)| self.depth(d, s, values[s.source], lag, frames, onset));
        (v, d)
    }

    fn transform(&self, route: &ModRoute, raw: f64, bipolar: bool) -> f64 {
        let mut v = if route.invert {
            if bipolar { -raw } else { 1.0 - raw }
        } else {
            raw
        };
        if let Some(shape) = route.shape {
            let x = unipolar(v, bipolar);
            let y = self.shapes[shape].evaluate(x as f32) as f64;
            v = if bipolar { 2.0 * y - 1.0 } else { y };
        }
        v
    }
}

fn unipolar(v: f64, bipolar: bool) -> f64 {
    if bipolar { (v + 1.0) * 0.5 } else { v }
}

#[cfg(test)]
fn evaluate(points: &[(f32, f32)], x: f32) -> f32 {
    let after = points.partition_point(|p| {
        #[cfg(test)]
        SHAPE_PROBES.set(SHAPE_PROBES.get() + 1);
        p.0 < x
    });
    interpolate(points, x, after)
}

fn interpolate(points: &[(f32, f32)], x: f32, after: usize) -> f32 {
    match after {
        0 => points[0].1,
        n if n == points.len() => points[n - 1].1,
        n => {
            let (a, b) = (points[n - 1], points[n]);
            let span = b.0 - a.0;
            if span > 0.0 {
                a.1 + (b.1 - a.1) * (x - a.0) / span
            } else {
                b.1
            }
        }
    }
}


/// Note and controller inputs read by a voice's sources.
pub(crate) struct Inputs<'a> {
    pub velocity: f64,
    pub key: f64,
    pub pressure: u32,
    pub timbre: u32,
    pub controllers: &'a [u32; 128],
    /// Frames from the note's admission to its key release (or now).
    pub held: u64,
    pub script: &'a crate::script_params::ModValues,
    /// The note's raw pitch bend, -1..=1.
    pub bend: f64,
    pub controls: RealValuesRef<'a>,
    /// Group audio for the span being advanced, for audio-driven sources
    /// (envelope follower). Empty when the caller has none: those sources hold.
    pub audio: &'a [f32],
    /// Semitones from the previous note of the group to this note (glide).
    /// 0 when the caller has none.
    pub interval: i32,
}

impl<'a> Inputs<'a> {
    pub fn new(
        note: &crate::Note,
        expression: crate::Expression,
        controllers: &'a [u32; 128],
        held: u64,
        script: &'a crate::script_params::ModValues,
        controls: RealValuesRef<'a>,
    ) -> Self {
        Self {
            velocity: note.velocity,
            key: f64::from(note.pitch.key()) / 127.0,
            pressure: expression.pressure,
            timbre: expression.timbre,
            controllers,
            held,
            script,
            controls,
            bend: expression.bend,
            audio: &[],
            interval: 0,
        }
    }
}

/// Actual source-owner points in this program's immutable source order.
/// The owner produces them once; destination programs only borrow the view.
/// Unlike Inputs this type cannot contain manufactured note context.
pub(crate) struct ElementInputs<'a>{
    pub points:&'a[f64],
    pub controls:RealValuesRef<'a>,
}

/// Per-chunk results a voice renders with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Outputs {
    pub gains: [f32; 2],
    /// Already evaluated owner scalar operands, before the final Pan clamp/law.
    pub scalar_gain:f64,
    pub scalar_pan:f64,
    pub pitch: f64,
    /// Cutoff and Q factors for the voice chain's state-variable filters.
    pub filter: [f64; 2],
    /// Semitones below the tone filter's open cutoff; 0 bypasses it.
    pub tone: f64,
    pub wavetable: [f64; 3],
    /// The family H factor of each amplitude envelope stage: the stage's
    /// normalized knob is multiplied by it. 1.0 leaves the stage unchanged.
    pub envelope_stage: [f64; 5],
}

impl Default for Outputs {
    fn default() -> Self {
        Self {
            gains: [1.0; 2], scalar_gain:1., scalar_pan:0.,
            pitch: 0.0,
            filter: [1.0; 2],
            tone: 0.0,
            wavetable: [0.; 3],
            envelope_stage: [1.0; 5],
        }
    }
}

/// Mutable per-voice state for one plan generation, sized at construction.
pub(crate) struct VoiceModState {
    pub(crate) native: sampler_pool::Slab<Option<crate::v1_voice_controls::ControlState>>,
    pub(crate) native_starts: u64,
    pub(crate) native_blocks: std::sync::atomic::AtomicU64,
    sources: usize,
    envelopes: usize,
    breakpoints: usize,
    routes: usize,
    program: Box<[Option<u32>]>,
    seed: Box<[u64]>,
    age: Box<[u32]>,
    outputs: Box<[Outputs]>,
    /// The control point before `outputs`.
    previous: Box<[Outputs]>,
    /// Runtime frames of `previous` and `outputs`.
    times: Box<[[u64; 2]]>,
    /// Tone filter state: two integrator pairs (left/right).
    tone: Box<[[[f64; 2]; 2]]>,
    phase: Box<[f64]>,
    values: Box<[f64]>,
    previous_values:Box<[f64]>,
    pending_start:Box<[bool]>,
    lagged: Box<[f64]>,
    /// Lag state of each route's intensity assignments, `SCALE_LAGS` per route.
    scale_lagged: Box<[f64]>,
    processor_values: Box<[f64]>,
    previous_processor_values: Box<[f64]>,
    envelope: Box<[EnvelopeState]>,
    segments: Box<[Segment]>,
    followers: usize,
    follower: Box<[crate::blocks::follower::AdaptiveFollower]>,
    glides: usize,
    glide: Box<[crate::blocks::glide::GlideRamp]>,
    /// Key of the last note this state started: the origin of the next glide.
    last_key: Option<i32>,
    /// Runtime frame and seed of the latest voice start: the restart of every
    /// shared retriggered LFO.
    restart: (u64, u64),
    // Only the scoped evaluator has per-program shared restart domains. A
    // pending row is invisible to every other row until publication commits.
    restart_domains:Box<[(u64,u64)]>,
    pending_restart:Box<[Option<(u64,u64)>]>,
}

/// Borrowed current/future source endpoints of one actual scoped state row.
/// It carries no independent mutable value or clock and cannot outlive the row.
#[derive(Clone,Copy)]
pub(crate) struct ProviderView<'a>{scope:Option<usize>,from:&'a[f64],to:&'a[f64],begin:u64,end:u64}
impl ProviderView<'_>{
    fn empty()->Self{Self{scope:None,from:&[],to:&[],begin:0,end:0}}
    fn at(&self,source:usize,frame:u64)->f64{
        let from=self.from[source];let to=self.to[source];
        if self.end==self.begin||frame>=self.end{return to;}
        if frame<=self.begin{return from;}
        let part=(frame-self.begin)as f64/(self.end-self.begin)as f64;
        from+(to-from)*part
    }
}
impl VoiceModState {
    pub fn new(modulation: &VoiceModulation, voices: usize) -> Result<Self, Error> {
        Self::with_shape(modulation.shape(), voices)
    }

    pub(crate) fn new_scoped(modulation:&VoiceModulation, owners:usize)->Result<Self,Error>{
        Self::checked_storage_bytes(modulation.shape(),owners,modulation.programs.len())?;
        let mut state=Self::new(modulation,owners)?;
        if !modulation.is_empty(){
            state.restart_domains=crate::dsp::allocate(modulation.programs.len())?;
            state.pending_restart=crate::dsp::allocate(owners)?;
            state.previous_values=crate::dsp::allocate(modulation.sources.checked_mul(owners).ok_or(Error::Capacity)?)?;
        }Ok(state)
    }
    /// Full backing-array geometry before allocation, including row projections,
    /// all lag/processor/segment arrays and scoped restart transactions.
    pub(crate) fn checked_storage_bytes(shape:ModShape,rows:usize,domains:usize)->Result<usize,Error>{
        if shape.empty{return Ok(0);}
        fn add<T>(total:&mut usize,count:usize)->Result<(),Error>{
            let bytes=std::alloc::Layout::array::<T>(count).map_err(|_|Error::Capacity)?.size();
            *total=total.checked_add(bytes).ok_or(Error::Capacity)?;Ok(())
        }
        let slots=|count:usize|count.checked_mul(rows).ok_or(Error::Capacity);
        let mut n=0;
        add::<Option<crate::v1_voice_controls::ControlState>>(&mut n,if shape.native{rows}else{0})?;
        add::<std::sync::atomic::AtomicBool>(&mut n,if shape.native{rows}else{0})?;
        add::<Option<u32>>(&mut n,rows)?;add::<u64>(&mut n,rows)?;add::<u32>(&mut n,rows)?;add::<bool>(&mut n,rows)?;
        add::<f64>(&mut n,if domains==0{0}else{slots(shape.sources)?})?;
        add::<Outputs>(&mut n,slots(2)?)?;add::<[u64;2]>(&mut n,rows)?;add::<[[f64;2];2]>(&mut n,rows)?;
        add::<f64>(&mut n,slots(shape.sources.checked_mul(2).ok_or(Error::Capacity)?)?)?;
        add::<f64>(&mut n,slots(shape.routes.checked_mul(3+SCALE_LAGS).ok_or(Error::Capacity)?)?)?;
        add::<EnvelopeState>(&mut n,slots(shape.envelopes)?)?;add::<Segment>(&mut n,slots(shape.breakpoints)?)?;
        add::<crate::blocks::follower::AdaptiveFollower>(&mut n,slots(shape.followers)?)?;
        add::<crate::blocks::glide::GlideRamp>(&mut n,slots(shape.glides)?)?;
        add::<(u64,u64)>(&mut n,domains)?;
        add::<Option<(u64,u64)>>(&mut n,if domains==0{0}else{rows})?;Ok(n)
    }
    pub fn with_shape(modulation: ModShape, voices: usize) -> Result<Self, Error> {
        Self::checked_storage_bytes(modulation,voices,0)?;
        let slots = |n: usize| n.checked_mul(voices).ok_or(Error::Capacity);
        if modulation.empty {
            return Ok(Self::empty());
        }
        Ok(Self {
            native: sampler_pool::Slab::new(crate::dsp::allocate(if modulation.native { voices } else { 0 })?, 1),
            native_starts: 0,
            native_blocks: std::sync::atomic::AtomicU64::new(0),
            sources: modulation.sources,
            envelopes: modulation.envelopes,
            breakpoints: modulation.breakpoints,
            followers: modulation.followers,
            glides: modulation.glides,
            glide: std::iter::repeat_n(crate::blocks::glide::GlideRamp { start: 0.0, step: 0.0, gain: 0.0 }, slots(modulation.glides)?).collect(),
            last_key: None,
            routes: modulation.routes,
            program: crate::dsp::allocate(voices)?,
            seed: crate::dsp::allocate(voices)?,
            age: crate::dsp::allocate(voices)?,
            outputs: crate::dsp::allocate(voices)?,
            previous: crate::dsp::allocate(voices)?,
            times: crate::dsp::allocate(voices)?,
            tone: crate::dsp::allocate(voices)?,
            phase: crate::dsp::allocate(slots(modulation.sources)?)?,
            values: crate::dsp::allocate(slots(modulation.sources)?)?,
            previous_values:Box::new([]),pending_start:crate::dsp::allocate(voices)?,
            lagged: crate::dsp::allocate(slots(modulation.routes)?)?,
            scale_lagged: crate::dsp::allocate(slots(modulation.routes * SCALE_LAGS)?)?,
            processor_values: crate::dsp::allocate(slots(modulation.routes)?)?,
            previous_processor_values: crate::dsp::allocate(slots(modulation.routes)?)?,
            envelope: std::iter::repeat_n(
                EnvelopeState::new(Envelope::default()),
                slots(modulation.envelopes)?,
            )
            .collect(),
            segments: crate::dsp::allocate(slots(modulation.breakpoints)?)?,
            follower: std::iter::repeat_n(crate::blocks::follower::AdaptiveFollower::new(0., 0., 0.), slots(modulation.followers)?).collect(),
            restart: (0, 0),restart_domains:Box::new([]),pending_restart:Box::new([]),
        })
    }

    /// Take the state of `larger` (same shape, more voices), moving every
    /// voice's state across; the old arrays are left in `larger`. No allocation.
    pub fn adopt(&mut self, larger: &mut Self) {
        fn take<T>(old: &mut Box<[T]>, new: &mut Box<[T]>) {
            let n = old.len();
            new[..n].swap_with_slice(old);
            std::mem::swap(old, new);
        }
        if !self.native.is_empty() {
            let n = self.native.len();
            larger.native.as_mut_slice()[..n].swap_with_slice(self.native.as_mut_slice());
            std::mem::swap(&mut self.native, &mut larger.native);
        }
        take(&mut self.program, &mut larger.program);
        take(&mut self.seed, &mut larger.seed);
        take(&mut self.age, &mut larger.age);
        take(&mut self.outputs, &mut larger.outputs);
        take(&mut self.previous, &mut larger.previous);
        take(&mut self.times, &mut larger.times);
        take(&mut self.tone, &mut larger.tone);
        take(&mut self.phase, &mut larger.phase);
        take(&mut self.values, &mut larger.values);
        take(&mut self.pending_start,&mut larger.pending_start);
        // This adopt path grows physical rows only. Scoped owner arrays retain
        // their independent fixed capacity and are never moved/reseeded here.
        take(&mut self.lagged, &mut larger.lagged);
        take(&mut self.scale_lagged, &mut larger.scale_lagged);
        take(&mut self.processor_values, &mut larger.processor_values);
        take(
            &mut self.previous_processor_values,
            &mut larger.previous_processor_values,
        );
        take(&mut self.envelope, &mut larger.envelope);
        take(&mut self.follower, &mut larger.follower);
        take(&mut self.glide, &mut larger.glide);
        larger.last_key = self.last_key;
    }

    fn empty() -> Self {
        Self {
            native: sampler_pool::Slab::new(Box::new([]), 1),
            native_starts: 0,
            native_blocks: std::sync::atomic::AtomicU64::new(0),
            sources: 0,
            envelopes: 0,
            breakpoints: 0,
            followers: 0,
            glides: 0,
            glide: Box::new([]),
            last_key: None,
            routes: 0,
            program: Box::new([]),
            seed: Box::new([]),
            age: Box::new([]),
            outputs: Box::new([]),
            previous: Box::new([]),
            times: Box::new([]),
            tone: Box::new([]),
            phase: Box::new([]),
            values: Box::new([]),previous_values:Box::new([]),pending_start:Box::new([]),
            lagged: Box::new([]),
            scale_lagged: Box::new([]),
            processor_values: Box::new([]),
            previous_processor_values: Box::new([]),
            envelope: Box::new([]),
            segments: Box::new([]),
            follower: Box::new([]),
            restart: (0, 0),restart_domains:Box::new([]),pending_restart:Box::new([]),
        }
    }

    pub fn program(&self, voice: usize) -> Option<u32> {
        self.program.get(voice).copied().flatten()
    }

    pub(crate) fn fill_filter_factors(
        &self,
        modulation: &VoiceModulation,
        voice: usize,
        // Native: cutoff/Q/Gain deltas and enabled bits; SVF: cutoff/Q factors.
        factors: &mut [[f64; 4]],
        previous_program: &mut Option<u32>,
        normalized: impl Fn(usize) -> bool,
    ) {
        // v1 filter.rs projects group-local rows; reset only the last voice's rows.
        if let Some(previous) = *previous_program {
            for &index in &modulation.programs[previous as usize].filter_indices {
                factors[index] = if normalized(index) {
                    [0.; 4]
                } else {
                    [1., 1., 0., 0.]
                };
            }
        }
        *previous_program = self.program(voice);
        if let Some(program) = *previous_program {
            let p = &modulation.programs[program as usize];
            for &index in &p.filter_indices {
                factors[index] = [0.; 4];
            }
            let offset = voice * self.routes;
            for (i, target) in p.targets.iter().enumerate() {
                let value = (self.processor_values[offset + i]
                    + self.previous_processor_values[offset + i])
                    * 0.5;
                match *target {
                    CompiledTarget::ProcessorCutoff(index) => factors[index as usize][0] += value,
                    CompiledTarget::ProcessorResonance(index) => {
                        factors[index as usize][1] += value
                    }
                    CompiledTarget::ProcessorNativeCutoff(index) => {
                        factors[index as usize][0] += value;
                        factors[index as usize][3] =
                            ((factors[index as usize][3] as u8) | 1) as f64;
                    }
                    CompiledTarget::ProcessorNativeResonance(index) => {
                        factors[index as usize][1] += value;
                        factors[index as usize][3] =
                            ((factors[index as usize][3] as u8) | 2) as f64;
                    }
                    CompiledTarget::ProcessorNativeGain(index) => {
                        factors[index as usize][2] += value;
                        factors[index as usize][3] =
                            ((factors[index as usize][3] as u8) | 4) as f64;
                    }
                    _ => {}
                }
            }
        }
        let indices = previous_program.map_or(&[][..], |p| {
            &*modulation.programs[p as usize].filter_indices
        });
        for &index in indices {
            let factor = &mut factors[index];
            if normalized(index) {
                continue;
            }
            let deltas = *factor;
            *factor = [1., 1., 0., 0.];
            // v1 0cb7a8a0:src/engine/filter.rs skips neutral modulation deltas.
            for (n, (v, d)) in factor
                .iter_mut()
                .take(2)
                .zip(&deltas)
                .enumerate()
                .filter(|(_, (_, d))| **d != 0.0)
            {
                *v = if n == 0 {
                    (d / 12.0).exp2()
                } else {
                    10f64.powf(d / 20.0)
                };
            }
        }
    }

    /// Bind a starting voice and compute its first control point, so the first
    /// chunk ramps from the onset values rather than from unity.
    /// A lane-local copy retains the shared base ramp and every voice's own offsets.
    pub(crate) fn project_parameters(
        &self,
        modulation: &VoiceModulation,
        voice: usize,
        base: &[crate::dsp::control::ControlRamp],
        bindings: &[crate::ControlRange],
        span: std::ops::Range<usize>,
        scratch: &mut [crate::dsp::control::ControlRamp],
    ) -> bool {
        let Some(program) = self.program(voice) else {
            return false;
        };
        let p = &modulation.programs[program as usize];
        if !p.control {
            return false;
        }
        scratch[span.clone()].copy_from_slice(&base[span.clone()]);
        let offset = voice * self.routes;
        for (i, target) in p.targets.iter().enumerate() {
            if let CompiledTarget::Control(lane) = *target {
                if !span.contains(&lane) {
                    continue;
                }
                let delta = (self.previous_processor_values[offset + i]
                    + self.processor_values[offset + i])
                    * 0.5;
                scratch[lane].add_modulation(delta, bindings[lane]);
            }
        }
        true
    }

    pub(crate) fn project_processing_parameters(
        &self, modulation:&VoiceModulation, owner:usize, scope:usize,
        bindings:&[crate::topology::CompiledProcessingParameter], begin:u64,end:u64,rate:u32,
        parameters:&mut[crate::dsp::control::ControlRamp],
    )->Result<(),Error>{
        if bindings.is_empty(){return Ok(());}
        let program=self.program(owner).and_then(|p|modulation.programs.get(p as usize)).ok_or(Error::InvalidInput)?;
        let offset=owner.checked_mul(self.routes).ok_or(Error::Capacity)?;
        // Preflight every actual target/operand before mutating transient scratch.
        for(binding,row)in bindings.iter().enumerate(){
            if row.lane>=parameters.len(){return Err(Error::InvalidInput);}
            for &route in &row.routes {
                if !matches!(program.targets.get(route),Some(CompiledTarget::ProcessingParameter{scope:actual,binding:index})if *actual as usize==scope&&*index as usize==binding){return Err(Error::InvalidInput);}
                if offset.checked_add(route).is_none_or(|index|index>=self.processor_values.len()||index>=self.previous_processor_values.len()){return Err(Error::InvalidInput);}
            }
        }
        let projection=|row:&crate::topology::CompiledProcessingParameter|{
            let mut semitones=0.;
            for &route in &row.routes {semitones+=(self.previous_processor_values[offset+route]+self.processor_values[offset+route])*0.5;}
            parameters[row.lane].project_processor((semitones/12.).exp2(),&row.definition.fixed_factors,row.definition.law,begin,end,rate)
        };
        // The first pass validates all values before ANY scratch publication.
        for row in bindings {let _=projection(row)?;}
        for row in bindings {
            let mut semitones=0.;for &route in &row.routes{semitones+=(self.previous_processor_values[offset+route]+self.processor_values[offset+route])*0.5;}
            parameters[row.lane]=parameters[row.lane].project_processor((semitones/12.).exp2(),&row.definition.fixed_factors,row.definition.law,begin,end,rate)?;
        }
        Ok(())
    }

    pub fn start(
        &mut self, modulation: &VoiceModulation, voice: usize, region: usize,
        inputs: &Inputs<'_>, clock: Clock, seed: u64,
    ) { self.start_with_controls(modulation, voice, region, inputs, clock, seed, true); }
    pub(crate) fn start_scoped(&mut self,modulation:&VoiceModulation,voice:usize,scope:usize,inputs:&Inputs<'_>,clock:Clock,seed:u64,closed:bool){
        self.start_initial(modulation,voice,scope,inputs,clock,seed,false,closed);
    }
    pub(crate) fn start_with_controls(&mut self,modulation:&VoiceModulation,voice:usize,region:usize,inputs:&Inputs<'_>,clock:Clock,seed:u64,native_allowed:bool){
        self.start_initial(modulation,voice,region,inputs,clock,seed,native_allowed,false);
    }
    fn start_initial(&mut self,modulation:&VoiceModulation,voice:usize,region:usize,inputs:&Inputs<'_>,clock:Clock,seed:u64,native_allowed:bool,initially_released:bool){
        if let Some(program)=modulation.program(region)&&modulation.programs[program as usize].provider_scope.is_some(){
            if let Some(slot)=self.program.get_mut(voice){*slot=Some(program);self.seed[voice]=seed;self.pending_start[voice]=true;}return;
        }
        self.start_initial_view(modulation,voice,region,inputs,clock,seed,native_allowed,initially_released,ProviderView::empty());
    }
    fn start_initial_view(
        &mut self,
        modulation: &VoiceModulation,
        voice: usize,
        region: usize,
        inputs: &Inputs<'_>,
        clock: Clock,
        seed: u64, native_allowed: bool,initially_released:bool,provider:ProviderView<'_>,
    ) {
        self.start_initial_program(modulation,voice,modulation.program(region),inputs,clock,seed,native_allowed,initially_released,provider);
    }
    fn start_initial_program(&mut self,modulation:&VoiceModulation,voice:usize,index:Option<u32>,inputs:&Inputs<'_>,clock:Clock,seed:u64,native_allowed:bool,initially_released:bool,provider:ProviderView<'_>){
        if let Some(state) = self.native.as_mut_slice().get_mut(voice) { *state = None; }
        let Some(program) = self.program.get_mut(voice) else {
            return;
        };
        *program = index;
        let Some(index) = *program else {
            return;
        };
        let p = &modulation.programs[index as usize];
        if native_allowed && let Some(native) = &p.native {
            let mut state = crate::v1_voice_controls::ControlState::new(&native.plan);
            state.reset_read(&native.plan, |s,t| crate::whole_voice::read(inputs,s,t), clock.now);
            self.native.as_mut_slice()[voice] = Some(state);
            self.native_starts += 1;
        }
        self.seed[voice] = seed;
        self.pending_start[voice]=false;
        self.age[voice] = 0;
        if self.restart_domains.is_empty(){self.restart=(clock.now,seed);}
        else{self.pending_restart[voice]=Some((clock.now,seed));}
        self.tone[voice] = [[0.0; 2]; 2];
        for (i, source) in p.sources.iter().enumerate() {
            if let Prepared::Lfo(lfo) = source {
                self.phase[voice * self.sources + i] = lfo.phase;
            }
        }
        for (i, (envelope, controls)) in p.envelopes.iter().zip(&p.envelope_controls).enumerate() {
            // A stage moved off its authored value by a lane write (host or script).
            let mut live = *envelope;
            for (stage, index) in crate::engine_parameters::ENVELOPE_STAGES.into_iter().zip(controls) {
                if let Some(index) = index {
                    let value=inputs.controls.get(*index);
                    if value != envelope.control_value(stage) { live=live.with_control(stage,value); }
                }
            }
            self.envelope[voice * self.envelopes + i] = EnvelopeState::new(live);
        }
        for segment in &mut self.segments[voice * self.breakpoints..][..p.breakpoints.len()] {
            *segment = Segment::default();
        }
        let (mut slot, mut glide_slot) = (0, 0);
        for source in p.sources.iter() {
            match *source {
                Prepared::Follower { spec, .. } => {
                    if let Some(follower) = crate::blocks::follower::AdaptiveFollower::from_stored(clock.rate as f32, spec) {
                        self.follower[voice * self.followers + slot] = follower;
                    }
                    slot += 1;
                }
                Prepared::Glide { mode, time_ms, .. } => {
                    self.glide[voice * self.glides + glide_slot] = crate::blocks::glide::GlideRamp::kontakt_glide(
                        mode,
                        time_ms,
                        inputs.interval,
                        clock.rate as f32,
                    );
                    glide_slot += 1;
                }
                _ => {}
            }
        }
        self.last_key = Some((inputs.key * 127.0).round() as i32);
        if initially_released{self.release(modulation,voice);}
        let restart=self.restart_point(voice,index as usize);
        self.outputs[voice] = self.evaluate(p, voice, inputs, clock, 0, true,restart,provider);
        self.previous[voice] = self.outputs[voice];
        if !self.previous_values.is_empty(){let span=voice*self.sources..voice*self.sources+p.sources.len();self.previous_values[span.clone()].copy_from_slice(&self.values[span]);}
        self.times[voice] = [clock.now; 2];
    }

    /// Semitones from the last note this state started to `key` (0 before any).
    pub(crate) fn glide_interval(&self, key: i32) -> i32 {
        self.last_key.map_or(0, |last| key - last)
    }

    pub(crate) fn validate_element_start(&self,modulation:&VoiceModulation,row:usize,region:usize,points:usize)->Result<(),Error>{
        let index=modulation.program(region).ok_or(Error::InvalidInput)?;
        let sources=modulation.validate_element_program(index)?;
        if row>=self.program.len()||row>=self.pending_restart.len()||points!=sources
            ||self.previous_values.len()<self.sources.checked_mul(self.program.len()).ok_or(Error::Capacity)?
        {return Err(Error::InvalidInput);}Ok(())
    }

    /// Initialize a real element output row at its installation frame. All
    /// validation precedes state writes; the same priced scoped arrays are used.
    pub(crate) fn start_element(&mut self,modulation:&VoiceModulation,row:usize,region:usize,
        inputs:&ElementInputs<'_>,clock:Clock)->Result<(),Error>{
        let index=modulation.program(region).ok_or(Error::InvalidInput)?;
        self.validate_element_start(modulation,row,region,inputs.points.len())?;
        if inputs.points.iter().any(|v|!v.is_finite()){return Err(Error::InvalidInput);}
        let sources=inputs.points.len();
        let p=&modulation.programs[index as usize];
        self.program[row]=Some(index);self.seed[row]=0;self.age[row]=0;self.pending_start[row]=false;
        self.pending_restart[row]=None;
        let point=self.evaluate_points(p,row,inputs.controls,None,clock,0,true,(clock.now,0),ProviderView::empty(),
            |source,index|match source{InputSource::ConstantProperty{node,value,style,bipolar}=>inputs.controls.constant_property(node,value,style,bipolar),_=>inputs.points[index]});
        self.outputs[row]=point;self.previous[row]=point;self.times[row]=[clock.now;2];
        let span=row*self.sources..row*self.sources+sources;
        self.previous_values[span.clone()].copy_from_slice(&self.values[span]);
        Ok(())
    }

    pub(crate) fn element_ramp(&self,row:usize)->Result<Ramp,Error>{
        self.program(row).ok_or(Error::InvalidInput)?;
        let [begin,end]=*self.times.get(row).ok_or(Error::InvalidInput)?;
        Ok(Ramp{from:self.previous[row],to:self.outputs[row],begin,end,script_fade:None})
    }

    /// Forecast exactly once at the absolute shared control boundary. Source
    /// points must be provided for that target by the canonical element owner.
    pub(crate) fn advance_element(&mut self,modulation:&VoiceModulation,row:usize,
        inputs:&ElementInputs<'_>,clock:Clock)->Result<Ramp,Error>{
        let index=self.program(row).ok_or(Error::InvalidInput)?;
        let sources=modulation.validate_element_program(index)?;
        if inputs.points.len()!=sources||inputs.points.iter().any(|v|!v.is_finite())
            ||self.previous_values.len()<self.sources.checked_mul(self.program.len()).ok_or(Error::Capacity)?
        {return Err(Error::InvalidInput);}
        let [_,end]=self.times[row];
        if clock.now>=end{
            let target=(clock.now/CELL).checked_add(1).and_then(|v|v.checked_mul(CELL)).ok_or(Error::Capacity)?;
            let frames=u32::try_from(target.checked_sub(end).ok_or(Error::InvalidInput)?).map_err(|_|Error::Capacity)?;
            let p=&modulation.programs[index as usize];
            let span=row*self.sources..row*self.sources+sources;
            self.previous_values[span.clone()].copy_from_slice(&self.values[span]);
            let next=self.evaluate_points(p,row,inputs.controls,None,Clock{now:target,..clock},frames,false,(0,0),ProviderView::empty(),
                |source,index|match source{InputSource::ConstantProperty{node,value,style,bipolar}=>inputs.controls.constant_property(node,value,style,bipolar),_=>inputs.points[index]});
            self.previous[row]=std::mem::replace(&mut self.outputs[row],next);self.times[row]=[end,target];
        }
        let [begin,end]=self.times[row];
        Ok(Ramp{from:self.previous[row],to:self.outputs[row],begin,end,script_fade:None})
    }

    /// Constant native loop lanes respond at event fragments, independently of the source clock.
    pub fn loop_offsets(
        &self,
        modulation: &VoiceModulation,
        voice: usize,
        controls: RealValuesRef<'_>,
    ) -> Option<[f32; 2]> {
        let p = &modulation.programs[self.program(voice)? as usize];
        if p.loop_routes.is_empty() {
            return None;
        }
        let mut offsets = [0.; 2];
        for &(i, ids) in &p.loop_routes {
            let route = &p.routes[i];
            let depth = match ids {
                Some([depth, bypass]) => if controls.get(bypass) == 0. { controls.get(depth) } else { 0. },
                None => route.depth,
            };
            let lane = usize::from(route.target == ModTarget::LoopLength);
            offsets[lane] += depth as f32;
        }
        Some(offsets)
    }

    pub(crate) fn native_render<'a>(&'a self, modulation: &'a VoiceModulation, voice: usize,
        state: &'a mut Option<crate::v1_voice_controls::ControlState>, controls: RealValuesRef<'_>) -> Option<crate::whole_voice::Render<'a>> {
        let program = &modulation.programs[self.program(voice)? as usize];
        let native = program.native.as_ref()?;
        let flex_depth = program.native_flex_control.map_or(1., |(base,ids)| {
            ids.map_or(base, |[depth,bypass]| if controls.get(bypass) == 0. { controls.get(depth) } else { 0. })
        });
        Some(crate::whole_voice::Render { plan: &native.plan, state: state.as_mut()?, blocks: &self.native_blocks, flex_depth })
    }
    pub fn release(&mut self, modulation: &VoiceModulation, voice: usize) {
        if let Some(program) = self.program(voice) {
            let p = &modulation.programs[program as usize];
            if let (Some(native), Some(Some(state))) = (&p.native, self.native.as_mut_slice().get_mut(voice)) {
                state.release(&native.plan);
            }
            for state in &mut self.envelope[voice * self.envelopes..][..p.envelopes.len()] {
                state.release();
            }
            let segments = &mut self.segments[voice * self.breakpoints..][..p.breakpoints.len()];
            for (segment, envelope) in segments.iter_mut().zip(&p.breakpoints) {
                segment.release(envelope);
            }
        }
    }

    pub(crate) fn scoped_has_forecast(&self,voice:usize,at:u64)->Result<bool,Error>{
        let times=self.times.get(voice).ok_or(Error::InvalidInput)?;Ok(times[1]>at)
    }
    pub(crate) fn scope_gate_end(&self,modulation:&VoiceModulation,voice:usize,source:usize,releasing:bool)->Option<u64>{
        let p=&modulation.programs[self.program(voice)? as usize];
        let Prepared::Envelope(envelope)=*p.sources.get(source)? else{return None;};
        let remaining=self.envelope[voice*self.envelopes+envelope].remaining();
        if remaining==usize::MAX{return None;}
        let anchor=self.times[voice][1];
        if remaining==0&&!releasing{return Some(anchor);}
        // On release an already prepared endpoint still has its old value;
        // the ended phase first appears at the next evaluated control point.
        let duration=if releasing{remaining.max(1)}else{remaining};
        let end=anchor.saturating_add(u64::try_from(duration).unwrap_or(u64::MAX));
        Some(end.checked_add(CELL-1).map_or(u64::MAX,|rounded|rounded/CELL*CELL))
    }
    pub(crate) fn commit_scoped_start(&mut self,voice:usize){
        if let Some(pending)=self.pending_restart.get_mut(voice).and_then(Option::take)
            && let Some(program)=self.program(voice){self.restart_domains[program as usize]=pending;}
    }
    pub fn stop(&mut self, voice: usize) {
        if let Some(pending)=self.pending_restart.get_mut(voice){*pending=None;}
        if let Some(program) = self.program.get_mut(voice) {
            *program = None;
        }
        if let Some(pending)=self.pending_start.get_mut(voice){*pending=false;}
    }

    pub(crate) fn wavetable_offsets(&self, voice: usize) -> [f32; 3] {
        if self.program(voice).is_none() { return [0.; 3]; }
        std::array::from_fn(|lane| {
            ((self.previous[voice].wavetable[lane] + self.outputs[voice].wavetable[lane]) * 0.5) as f32
        })
    }

    /// The control ramp covering a segment starting at `clock.now`. Points
    /// sit on the absolute `CELL` grid (and the voice onset), so a segment
    /// entering a new cell evaluates its end once and segments inside a cell
    /// reuse it: output does not depend on how the host splits blocks.
    /// Segments must not cross a grid line.
    pub fn advance(&mut self,modulation:&VoiceModulation,voice:usize,inputs:&Inputs<'_>,clock:Clock)->Ramp{
        self.advance_view(modulation,voice,inputs,clock,ProviderView::empty())
    }
    fn advance_view(
        &mut self,
        modulation: &VoiceModulation,
        voice: usize,
        inputs: &Inputs<'_>,
        mut clock: Clock,provider:ProviderView<'_>,
    ) -> Ramp {
        let [begin, end] = self.times[voice];
        if let (Some(program), true) = (self.program(voice), clock.now >= end) {
            let p = &modulation.programs[program as usize];
            let target = (clock.now / CELL + 1) * CELL;
            clock.now = target;
            let restart=self.restart_point(voice,program as usize);
            if !self.previous_values.is_empty(){let span=voice*self.sources..voice*self.sources+p.sources.len();self.previous_values[span.clone()].copy_from_slice(&self.values[span]);}
            let next = self.evaluate(p, voice, inputs, clock, (target - end) as u32, false,restart,provider);
            self.previous[voice] = std::mem::replace(&mut self.outputs[voice], next);
            self.times[voice] = [end, target];
        } else if clock.now >= end {
            return Ramp {
                from: self.outputs[voice],
                to: self.outputs[voice],
                begin,
                end,
                script_fade: None,
            };
        }
        let [begin, end] = self.times[voice];
        Ramp {
            from: self.previous[voice],
            to: self.outputs[voice],
            begin,
            end,
            script_fade: None,
        }
    }

    /// The caller already selected this actual program. Pending scoped starts
    /// use their own unpublished point; other owners see the committed domain.
    fn restart_point(&self,voice:usize,program:usize)->(u64,u64){
        if self.restart_domains.is_empty(){self.restart}
        else{match self.pending_restart[voice]{Some(point)=>point,None=>self.restart_domains[program]}}
    }
    fn evaluate(
        &mut self,
        p: &Program,
        voice: usize,
        inputs: &Inputs<'_>,
        clock: Clock,
        frames: u32,
        onset: bool,
        restart:(u64,u64),provider:ProviderView<'_>,
    ) -> Outputs {
        let seed=self.seed[voice];
        self.evaluate_points(p,voice,inputs.controls,Some(inputs),clock,frames,onset,restart,provider,
            |source,index|source.input(inputs,seed,index))
    }

    // Both voice and element programs use this exact route/shape/lag/scalar
    // evaluator. Element points are supplied by their real source owner; no
    // Note, velocity, key, expression or arbitrary channel snapshot is made.
    fn evaluate_points(
        &mut self,p:&Program,voice:usize,controls:RealValuesRef<'_>,note_inputs:Option<&Inputs<'_>>,
        clock:Clock,frames:u32,onset:bool,restart:(u64,u64),provider:ProviderView<'_>,
        mut input_point:impl FnMut(InputSource,usize)->f64,
    )->Outputs {
        let native_context=p.native.as_ref().zip(note_inputs)
            .filter(|_|self.native.as_mut_slice().get(voice).is_some_and(Option::is_some));
        let native=native_context.map(|(program,_)|program);
        let seed = self.seed[voice];
        let age = self.age[voice].saturating_add(frames);
        self.age[voice] = age;
        let values = &mut self.values[voice * self.sources..][..p.sources.len()];
        let phases = &mut self.phase[voice * self.sources..][..p.sources.len()];
        let processor_values = &mut self.processor_values[voice * self.routes..][..p.routes.len()];
        let previous_processor_values =
            &mut self.previous_processor_values[voice * self.routes..][..p.routes.len()];
        let lagged = &mut self.lagged[voice * self.routes..][..p.routes.len()];
        let scale_lagged = &mut self.scale_lagged[voice * self.routes * SCALE_LAGS..][..p.routes.len() * SCALE_LAGS];
        previous_processor_values.copy_from_slice(processor_values);
        let mut changed = onset;
        for &i in &p.source_order {
            let source = p.sources[i];
            if onset && !p.phase_routes[i].is_empty() {
                // The native phase lane is read once after reset, never each control cell.
                // start_with_controls already seeded the owning source's saved base.
                let mut phase = phases[i];
                for &r in &p.phase_routes[i] {
                    let route = &p.routes[r];
                    phase += p.base_depth_controls(r,controls)
                        * p.transform(route, values[route.source], p.bipolar[route.source]);
                }
                phases[i] = phase;
            }
            let value = match source {
                _ if native.is_some_and(|n| n.flex_source == Some(i)) => 0.,
                Prepared::Lfo(lfo) if lfo.shape == LfoShape::Zero => 0.,
                Prepared::Lfo(lfo) => {
                    let mut inputs_lfo = LfoInputs::UNITY;
                    if !p.rate_routes[i].is_empty() {
                        // Rate routes settle first (source_order); the lane
                        // moves, the cycle integrates it, so no phase jumps.
                        // LfoInputs.rate is relative to the saved rate.
                        let mut lane = p.rate_lane[i];
                        for &r in &p.rate_routes[i] {
                            let base = p.base_depth_controls(r,controls);
                            let (v, d) = p.route_point(r, &p.routes[r], values, &mut lagged[r], &mut scale_lagged[r * SCALE_LAGS..][..SCALE_LAGS], base, frames, onset);
                            processor_values[r] = d * v;
                            if onset {
                                previous_processor_values[r] = processor_values[r];
                            }
                            lane += d * v;
                        }
                        let saved = lfo.hertz(clock.tempo, LfoInputs::UNITY);
                        inputs_lfo.rate = crate::v1_voice_controls::LfoFrequency::hertz(lane as f32) / saved;
                    }
                    let hz = lfo.hertz(clock.tempo, inputs_lfo);
                    let since = clock.now.saturating_sub(restart.0);
                    let age = if lfo.shared && lfo.retrigger {
                        u32::try_from(since).unwrap_or(u32::MAX)
                    } else {
                        age
                    };
                    let (phase, seed) = if lfo.shared && lfo.retrigger {
                        let t = since as f64 / clock.rate;
                        (lfo.phase + hz * t, restart.1 ^ i as u64)
                    } else if lfo.retrigger
                        || !p.phase_routes[i].is_empty()
                        || !p.rate_routes[i].is_empty()
                    {
                        // An absolute onset lane also replaces a private free-cycle phase.
                        phases[i] += hz * f64::from(frames) / clock.rate;
                        (phases[i], seed ^ i as u64)
                    } else {
                        // ponytail: free cycles read absolute time, so a tempo change jumps phase.
                        (phases[i] + hz * clock.now as f64 / clock.rate, i as u64)
                    };
                    // shortcut: gains use the shared 64-frame clock; W9 must certify native host timing.
                    if let Some(gains) = &p.fades[i] {
                        let gain = gains.get((age / 32) as usize).copied().unwrap_or(1.);
                        f64::from(wave(lfo.shape, phase, seed) as f32 * gain)
                    } else {
                        lfo.value(phase, seed, age, LfoInputs::UNITY)
                    }
                }
                Prepared::Envelope(e) => {
                    f64::from(self.envelope[voice * self.envelopes + e].advance(frames))
                }
                Prepared::Breakpoints(b) => f64::from(
                    self.segments[voice * self.breakpoints + b].advance(&p.breakpoints[b], frames),
                ),
                Prepared::Glide { slot, .. } => {
                    let ramp = &mut self.glide[voice * self.glides + slot];
                    let n = (frames as usize).clamp(1, crate::dsp::BLOCK);
                    let mut buf = [0f32; crate::dsp::BLOCK];
                    ramp.render(&mut buf[..n]);
                    f64::from(buf[n - 1])
                }
                Prepared::Follower { slot, .. } => {
                    let follower = &mut self.follower[voice * self.followers + slot];
                    if let Some(audio) = note_inputs.map(|inputs| inputs.audio).filter(|a| !a.is_empty()) {
                        follower.process(audio);
                    }
                    f64::from(follower.level())
                }
                Prepared::Input(input) => input_point(input,i),
                Prepared::Provider(source)=>provider.at(source.source,clock.now),
            };
            if p.cacheable || native.is_some() {
                changed |= values[i].to_bits() != value.to_bits();
            }
            values[i] = value;
        }
        let native_point = native_context.map(|(native,inputs)| {
            let state = self.native.as_mut_slice()[voice].as_mut().unwrap();
            // The pinned owner observes raw inputs and live intensity/bypass
            // values. Unrelated residual sources must not invalidate its cache.
            state.plan_controls_read_with_intensity(&native.plan,
                |s,t| crate::whole_voice::read(inputs,s,t), 0, frames as usize,
                |assignment, m| match p.native_controls[assignment] {
                    Some(([depth, bypass], scale)) => if controls.get(bypass) == 0. { (controls.get(depth) / scale) as f32 } else { 0. },
                    None => m.intensity,
                })
        });
        if p.cacheable && !changed && native.is_none() {
            return self.outputs[voice];
        }
        let mut gain = 1.0;
        let mut decibels = 0.0;
        let mut pan = 0.0;
        let mut out = Outputs::default();
        out.pitch = native_point.map_or(0., |p| f64::from(p.1));
        let mut resonance = 0.0;
        let mut cutoff = 0.0;
        let mut route_controls = p.controls.iter().peekable();
        for (i, (route, lagged)) in p.routes.iter().zip(lagged).enumerate() {
            let scale_lagged = &mut scale_lagged[i * SCALE_LAGS..][..SCALE_LAGS];
            // Bypass masks the route, not the source clock/lag; resume never restarts it.
            let base = match route_controls.peek().copied() {
                Some(&(index, [depth, bypass])) if index == i => {
                    route_controls.next();
                    if controls.get(bypass) == 0. { controls.get(depth) * route.live_gain } else { 0. }
                }
                _ => route.depth,
            };
            let control = native.map_or(crate::whole_voice::Route::Residual, |n| n.routes[i]);
            match control {
                crate::whole_voice::Route::Flex => continue,
                crate::whole_voice::Route::External(assignment) => {
                    let Some((native,inputs))=native_context else {continue;};
                    let m = &native.plan.description().mods.assignments()[assignment as usize];
                    let (source,target) = m.route.unwrap();
                    if target != crate::v1_voice_controls::Target::Module { continue; }
                    let input = crate::whole_voice::read(inputs, source, target);
                    let mut value = *lagged as f32;
                    if onset { value = m.shape(input); }
                    else { m.follow_value(&mut value, input, frames as usize, clock.rate as f32); }
                    *lagged = f64::from(value);
                    processor_values[i] = f64::from(base as f32 * value);
                    if onset { previous_processor_values[i] = processor_values[i]; }
                    continue;
                }
                crate::whole_voice::Route::Residual => {}
            }
            // Rate routes were settled with their LFO, before it ran.
            if matches!(p.targets[i], CompiledTarget::SourceRate(_)) {
                continue;
            }
            let bipolar = p.bipolar[route.source];
            let (v, d) = p.route_point(i, route, values, lagged, scale_lagged, base, frames, onset);
            processor_values[i] = d * v;
            if onset {
                previous_processor_values[i] = processor_values[i];
            }
            match p.targets[i] {
                CompiledTarget::Attenuate => match &p.laws[i] {
                    // Family H: x * C(1 - (1 - E) * i), where C is the
                    // compiler's `fcubic` for an external source of type
                    // 7..=13 and the identity otherwise.
                    Some(law) => {
                        let ctx = laws::CombineCtx {
                            target: law.target,
                            route_flags: law.flags,
                            env_special: false,
                            direct_special: false,
                            cubic_inner: law.cubic,
                        };
                        gain = f64::from(laws::combine(&ctx, gain as f32, v as f32, d as f32));
                    }
                    None => gain *= 1.0 - d * (1.0 - unipolar(v, bipolar)),
                },
                CompiledTarget::Decibels => decibels += d * v,
                CompiledTarget::Pan => pan += d * v,
                CompiledTarget::Pitch => out.pitch += d * v,
                CompiledTarget::Cutoff => cutoff += d * v,
                CompiledTarget::Resonance => resonance += d * v,
                CompiledTarget::Tone => out.tone += d * v,
                CompiledTarget::Wavetable(lane) => out.wavetable[usize::from(lane)] += d * v,
                CompiledTarget::SampleStart
                | CompiledTarget::SourcePhase(_)
                | CompiledTarget::SourceRate(_) => {}
                CompiledTarget::VoiceEnvelopeStage(stage) => {
                    // Family H: the stage's normalized knob is multiplied by
                    // C(1 - (1 - E) * i). C is the compiler's `fcubic` for an
                    // external source of type 7..=13 (0x140876753) and the
                    // identity otherwise; the source is read in its unipolar
                    // view (`need_uni`).
                    let e = unipolar(v, bipolar);
                    let inner = 1.0 - (1.0 - e) * d;
                    let cubic = route
                        .native
                        .is_some_and(|n| (7..=13).contains(&n.source_type));
                    out.envelope_stage[usize::from(stage)] = if cubic {
                        f64::from(laws::fcubic(inner as f32))
                    } else {
                        inner
                    };
                }
                CompiledTarget::LoopStart | CompiledTarget::LoopLength => {}
                CompiledTarget::ProcessingParameter { .. }
                | CompiledTarget::ProcessorCutoff(_)
                | CompiledTarget::ProcessorResonance(_)
                | CompiledTarget::ProcessorNativeCutoff(_)
                | CompiledTarget::ProcessorNativeResonance(_)
                | CompiledTarget::ProcessorNativeGain(_)
                | CompiledTarget::Control(_) => {}
            }
        }
        let gain = (gain * 10f64.powf(decibels / 20.0)).max(0.0);
        out.scalar_gain=gain;out.scalar_pan=pan;
        let pan = pan.clamp(-1.0, 1.0);
        out.gains = [
            (gain * (1.0 - pan.max(0.0))) as f32,
            (gain * (1.0 + pan.min(0.0))) as f32,
        ];
        if p.filter {
            out.filter = [(cutoff / 12.0).exp2(), 10f64.powf(resonance / 20.0)];
        }
        if let Some((gain,_,_)) = native_point { out.gains = out.gains.map(|g| g * gain); }
        out.tone = if p.tone { out.tone.min(0.0) } else { 0.0 };
        out
    }

    /// Mix one rendered segment starting at runtime frame `at` into `output`,
    /// ramping gains along `ramp` and running the voice's tone filter at the
    /// ramp's midpoint when closed.
    pub fn mix(
        &mut self,
        voice: usize,
        chunk: &mut [crate::Frame],
        output: &mut [crate::Frame],
        ramp: Ramp,
        at: u64,
        rate: f64,
    ) {
        let Ramp {
            from,
            to,
            begin,
            end,
            script_fade,
        } = ramp;
        let tone = (from.tone + to.tone) * 0.5;
        if tone < 0.0 {
            let hz = (TONE_OPEN * rate * (tone / 12.0).exp2()).max(10.0);
            let g = (std::f64::consts::PI * hz / rate).tan();
            // Butterworth TPT state-variable low-pass.
            let k = std::f64::consts::SQRT_2;
            let a1 = 1.0 / (1.0 + g * (g + k));
            let (a2, a3) = (g * a1, g * g * a1);
            let state = &mut self.tone[voice];
            for frame in chunk.iter_mut() {
                for (channel, sample) in frame.iter_mut().enumerate() {
                    let [ic1, ic2] = &mut state[channel];
                    let v3 = f64::from(*sample) - *ic2;
                    let v1 = a1 * *ic1 + a2 * v3;
                    let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                    *ic1 = 2.0 * v1 - *ic1;
                    *ic2 = 2.0 * v2 - *ic2;
                    *sample = v2 as f32;
                }
            }
            for channel in state.iter_mut() {
                *channel = channel.map(crate::dsp::flush);
            }
        } else {
            // Open: the filter passes its input, so its integrators follow it.
            self.tone[voice] = [[0.0; 2]; 2];
        }
        let len = end.saturating_sub(begin).max(1) as f32;
        let step = [
            (to.gains[0] - from.gains[0]) / len,
            (to.gains[1] - from.gains[1]) / len,
        ];
        let offset = at.saturating_sub(begin) as f32;
        if let Some(fade) = script_fade {
            for (i, (out, frame)) in output.iter_mut().zip(chunk.iter()).enumerate() {
                let position = offset + (i + 1) as f32;
                let gain = fade.at(at + i as u64 + 1) as f32;
                out[0] += frame[0] * (from.gains[0] + step[0] * position) * gain;
                out[1] += frame[1] * (from.gains[1] + step[1] * position) * gain;
            }
        } else {
            for (i, (out, frame)) in output.iter_mut().zip(chunk.iter()).enumerate() {
                let at = offset + (i + 1) as f32;
                out[0] += frame[0] * (from.gains[0] + step[0] * at);
                out[1] += frame[1] * (from.gains[1] + step[1] * at);
            }
        }
    }
}

/// Control grid on the runtime clock, in frames.
pub(crate) const CELL: u64 = crate::dsp::BLOCK as u64;

/// Control points around a segment: `from` at runtime frame `begin`, `to` at
/// `end`. Gains ramp between them; pitch, cutoff, Q and tone hold the midpoint.
#[derive(Clone, Copy)]
pub(crate) struct Ramp {
    pub from: Outputs,
    pub to: Outputs,
    pub begin: u64,
    pub end: u64,
    /// Active nonlinear fade, applied at the physical amplifier sample clock.
    pub script_fade: Option<crate::script_params::Fade>,
}

/// Cell slopes prepared once; preserve the original F32 operation order and
/// physical sample-clock fade. Port the v1 0cb7a8a0:src/engine/voice.rs block
/// delta preparation, retaining v2's absolute offset law. NI group amplifier
/// scope and Kontakt topology B11 effect-owned cadence remain unchanged.
/// No recursive accumulation or cadence change.
pub(crate) struct GainRamp {
    from: [f32; 2],
    step: [f32; 2],
    begin: u64,
    script_fade: Option<crate::script_params::Fade>,
}
impl GainRamp {
    pub(crate) fn at(&self, at: u64) -> [f32; 2] {
        let offset = at.saturating_sub(self.begin) as f32;
        let gains = std::array::from_fn(|c| self.from[c] + self.step[c] * offset);
        match self.script_fade {
            Some(fade) => {
                let factor = fade.at(at) as f32;
                gains.map(|gain| gain * factor)
            },
            None => gains,
        }
    }
}

#[cfg(test)]
thread_local! { pub(crate) static SAMPLE_GAIN_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

impl Ramp {
    pub(crate) fn prepare_gains(self) -> GainRamp {
        let len = self.end.saturating_sub(self.begin).max(1) as f32;
        GainRamp { from:self.from.gains,
            step:std::array::from_fn(|c| (self.to.gains[c] - self.from.gains[c]) / len),
            begin:self.begin, script_fade:self.script_fade }
    }
    pub(crate) fn gains_at(self, at: u64) -> [f32; 2] {
        #[cfg(test)]
        SAMPLE_GAIN_CALLS.set(SAMPLE_GAIN_CALLS.get() + 1);
        let len = self.end.saturating_sub(self.begin).max(1) as f32;
        let offset = at.saturating_sub(self.begin) as f32;
        let gains = std::array::from_fn(|c| {
            self.from.gains[c] + (self.to.gains[c] - self.from.gains[c]) / len * offset
        });
        match self.script_fade {
            Some(fade) => {
                let factor = fade.at(at) as f32;
                gains.map(|gain| gain * factor)
            }
            None => gains,
        }
    }

    pub(crate) fn without_gains(mut self) -> Self {
        self.from.gains = [1.0; 2];
        self.to.gains = [1.0; 2];
        self.script_fade = None;
        self
    }
}

/// The tone filter's open cutoff as a fraction of the sample rate.
const TONE_OPEN: f64 = 0.45;

/// Shared time inputs: output rate, tempo and the chunk end on the runtime clock.
#[derive(Clone, Copy)]
pub(crate) struct Clock {
    pub rate: f64,
    pub tempo: f64,
    pub now: u64,
}

impl crate::Runtime {
    /// Host tempo for beat-synced LFOs, in quarter notes per minute.
    pub fn set_tempo(&mut self, bpm: f64) -> Result<(), Error> {
        if !(bpm.is_finite() && bpm > 0.0) {
            return Err(Error::InvalidInput);
        }
        self.tempo = bpm;
        Ok(())
    }
}

#[cfg(test)]
thread_local! { static SHAPE_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }


/// The existing core Tone primitive on one original source channel. This is the
/// scalar recurrence from VoiceModState::mix: its input is the already-rounded
/// f32 source outlet, integrators are f64, and output stores f32 once. The old
/// stereo implementation remains unchanged; each N-channel source owns its own
/// pair of integrators, flushed/reset at exactly the same segment boundary.
pub(crate) fn process_channel_tone(chunk: &mut [f32], state: &mut [f64; 2], tone: f64, rate: f64) {
    if tone < 0.0 {
        let hz = (TONE_OPEN * rate * (tone / 12.0).exp2()).max(10.0);
        let g = (std::f64::consts::PI * hz / rate).tan();
        let k = std::f64::consts::SQRT_2;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let (a2, a3) = (g * a1, g * g * a1);
        for sample in chunk {
            let [ic1, ic2] = &mut *state;
            let v3 = f64::from(*sample) - *ic2;
            let v1 = a1 * *ic1 + a2 * v3;
            let v2 = *ic2 + a2 * *ic1 + a3 * v3;
            *ic1 = 2.0 * v1 - *ic1;
            *ic2 = 2.0 * v2 - *ic2;
            *sample = v2 as f32;
        }
        *state = state.map(crate::dsp::flush);
    } else {
        *state = [0.0; 2];
    }
}

/// The same Tone primitive over interleaved stereo frames. Shared processing
/// activations apply it once to their summed post-chain output, so the two
/// channels keep independent integrators exactly as the mono path does.
pub(crate) fn process_frame_tone(
    frames: &mut [crate::Frame],
    state: &mut [[f64; 2]; 2],
    tone: f64,
    rate: f64,
) {
    if tone < 0.0 {
        let hz = (TONE_OPEN * rate * (tone / 12.0).exp2()).max(10.0);
        let g = (std::f64::consts::PI * hz / rate).tan();
        let k = std::f64::consts::SQRT_2;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let (a2, a3) = (g * a1, g * g * a1);
        for frame in frames {
            for (sample, state) in frame.iter_mut().zip(state.iter_mut()) {
                let [ic1, ic2] = &mut *state;
                let v3 = f64::from(*sample) - *ic2;
                let v1 = a1 * *ic1 + a2 * v3;
                let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                *ic1 = 2.0 * v1 - *ic1;
                *ic2 = 2.0 * v2 - *ic2;
                *sample = v2 as f32;
            }
        }
        for channel in state.iter_mut() {
            *channel = channel.map(crate::dsp::flush);
        }
    } else {
        *state = [[0.0; 2]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_shaper_uses_v1_index_instead_of_binary_search() {
        let points: Vec<_> = (0..128)
            .map(|i| ((i as f64 / 127.) as f32, i as f32 / 127.))
            .collect();
        let shape = Shape::new(points.into_boxed_slice());
        SHAPE_PROBES.set(0);
        std::hint::black_box(shape.evaluate(std::hint::black_box(0.43)));
        assert!(
            SHAPE_PROBES.get() <= 3,
            "uniform lookup used {} probes",
            SHAPE_PROBES.get()
        );
    }

    #[test]
    fn v1_table_index_preserves_v2_shape_bits_at_every_boundary() {
        for len in [1, 2, 3, 17, 128, 1024] {
            let points: Box<[_]> = (0..len)
                .map(|i| {
                    (
                        (i as f64 / (len - 1).max(1) as f64) as f32,
                        ((i * 7919 % 127) as f32 - 63.) / 17.,
                    )
                })
                .collect();
            for irregular in [false, true] {
                let mut points = points.clone();
                if irregular && len > 2 {
                    points[1].0 = points[0].0;
                }
                let shape = Shape::new(points);
                for x in shape
                    .points
                    .iter()
                    .flat_map(|p| [p.0.next_down(), p.0, p.0.next_up()])
                    .chain((0..=16384).map(|i| i as f32 / 16384.))
                    .chain([f32::NAN, f32::NEG_INFINITY, f32::INFINITY, -1., 2.])
                {
                    assert_eq!(
                        shape.evaluate(x).to_bits(),
                        evaluate(&shape.points, x).to_bits(),
                        "len={len} irregular={irregular} x={x}"
                    );
                }
            }
        }
    }

    #[test]
    fn scaled_sine_matches_original_digital_waveform_points_and_rejects_bad_levels() {
        // Original Digital entry 0x140b078d0, authored inputs, no helper stubs.
        let native = [
            0.,
            -0.7071070075035095,
            -1.,
            -0.7071070075035095,
            0.,
            0.7071070075035095,
            1.,
            0.7071070075035095,
            0.,
        ];
        for weight in [0.3f32, 1., -0.7] {
            for (i, point) in native.into_iter().enumerate() {
                let actual = wave(LfoShape::SineScaled(-f64::from(weight)), i as f64 / 8., 0);
                assert!((actual - f64::from(point * weight)).abs() < 1e-6);
            }
        }
        for level in [f64::NAN, f64::INFINITY, -1.01, 1.01] {
            assert!(
                VoiceModulation::new(
                    vec![ModProgram {
                        sources: vec![ModSource::Lfo(Lfo {
                            shape: LfoShape::SineScaled(level),
                            rate: LfoRate::Hertz(1.),
                            phase: 0.,
                            delay: 0,
                            fade: 0,
                            retrigger: true,
                            shared: false
                        })],
                        ..Default::default()
                    }],
                    vec![Some(0)],
                    vec![0]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn digital_fade_rejects_unproven_clock_state_at_core_boundary() {
        let valid = Lfo {
            shape: LfoShape::DigitalSine {
                level: -0.3,
                fade: crate::KontaktLfoFade::from_saved(10., 48000.).unwrap(),
            },
            rate: LfoRate::Hertz(1.),
            phase: 0.25,
            delay: 0,
            fade: 0,
            retrigger: true,
            shared: false,
        };
        let prepare = |lfo| {
            VoiceModulation::new(
                vec![ModProgram {
                    sources: vec![ModSource::Lfo(lfo)],
                    ..Default::default()
                }],
                vec![Some(0)],
                vec![0],
            )
        };
        assert!(prepare(valid).is_ok());
        for invalid in [
            Lfo { delay: 1, ..valid },
            Lfo { fade: 1, ..valid },
            Lfo {
                retrigger: false,
                ..valid
            },
            Lfo {
                shared: true,
                ..valid
            },
            Lfo {
                rate: LfoRate::Beats(1.),
                ..valid
            },
            Lfo {
                phase: -0.1,
                ..valid
            },
            Lfo {
                shape: LfoShape::DigitalSine {
                    level: f64::NAN,
                    fade: crate::KontaktLfoFade::from_saved(10., 48000.)
                        .unwrap(),
                },
                ..valid
            },
        ] {
            assert!(prepare(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn addressed_projection_averages_each_route_before_saved_order_reduction() {
        let modulation = VoiceModulation::new(
            vec![ModProgram {
                sources: vec![ModSource::Constant],
                routes: (0..3)
                    .map(|_| ModRoute::new(0, ModTarget::ProcessorNativeCutoff(0), 1.))
                    .collect(),
                ..Default::default()
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap();
        let mut state = VoiceModState::new(&modulation, 1).unwrap();
        state.program[0] = Some(0);
        let tiny = 2f64.powi(-54);
        state.previous_processor_values[..3].copy_from_slice(&[1., tiny, -1.]);
        state.processor_values[..3].copy_from_slice(&[-1., tiny, 1.]);
        let mut projected = [[0.; 4]];
        state.fill_filter_factors(&modulation, 0, &mut projected, &mut None, |_| true);
        assert_eq!(
            projected[0].map(f64::to_bits),
            [tiny, 0., 0., 1.].map(f64::to_bits)
        );
        // Reducing endpoints first loses the small contribution; this is a PCM-preservation rule.
        let sum = |values: &[f64]| values.iter().fold(0., |sum, value| sum + value);
        assert_eq!(
            (sum(&state.previous_processor_values) + sum(&state.processor_values)) * 0.5,
            0.
        );
    }

    #[test]
    fn filter_projection_work_is_bounded_by_addressed_stages() {
        use std::cell::Cell;
        let modulation = VoiceModulation::new(
            vec![ModProgram {
                sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, ModTarget::ProcessorCutoff(3), 12.)],
                ..Default::default()
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap();
        let mut state = VoiceModState::new(&modulation, 2).unwrap();
        state.program[0] = Some(0);
        state.processor_values[0] = 12.;
        state.previous_processor_values[0] = 12.;
        let mut factors = [[1., 1., 0., 0.]; 1024];
        let mut previous_program = None;
        for voice in [0, 0, 1, 0] {
            let visits = Cell::new(0);
            state.fill_filter_factors(
                &modulation,
                voice,
                &mut factors,
                &mut previous_program,
                |_| {
                    visits.set(visits.get() + 1);
                    false
                },
            );
            assert!(
                visits.get() <= 2,
                "whole-plan projection: {} stages",
                visits.get()
            );
            assert_eq!(factors[3][0], if voice == 0 { 2. } else { 1. });
            assert_eq!(factors[1023], [1., 1., 0., 0.]);
        }
    }

    #[test]
    fn sparse_projection_resets_disjoint_programs_and_keeps_native_flags() {
        let modulation = VoiceModulation::new(
            vec![
                ModProgram {
                    sources: vec![ModSource::Constant],
                    routes: vec![
                        ModRoute::new(0, ModTarget::ProcessorCutoff(2), 12.),
                        ModRoute::new(0, ModTarget::ProcessorCutoff(2), -12.),
                        ModRoute::new(0, ModTarget::ProcessorNativeCutoff(5), 0.),
                        ModRoute::new(0, ModTarget::ProcessorNativeGain(5), 0.5),
                        ModRoute::new(0, ModTarget::ProcessorNativeGain(5), -0.5),
                    ],
                    ..Default::default()
                },
                ModProgram {
                    sources: vec![ModSource::Constant],
                    routes: vec![ModRoute::new(0, ModTarget::ProcessorResonance(7), 20.)],
                    ..Default::default()
                },
            ],
            vec![Some(0), Some(1)],
            vec![0, 0],
        )
        .unwrap();
        let mut state = VoiceModState::new(&modulation, 3).unwrap();
        state.program[0] = Some(0);
        state.program[1] = Some(1);
        state.processor_values[..5].copy_from_slice(&[12., -12., 0., 0.5, -0.5]);
        state.previous_processor_values[..5].copy_from_slice(&[12., -12., 0., 0.5, -0.5]);
        state.processor_values[5] = 20.;
        state.previous_processor_values[5] = 20.;
        let neutral = |i| if i == 5 { [0.; 4] } else { [1., 1., 0., 0.] };
        let mut factors = std::array::from_fn::<_, 12, _>(neutral);
        let mut previous_program = None;
        for voice in [0, 1, 0, 2, 1, 2] {
            state.fill_filter_factors(
                &modulation,
                voice,
                &mut factors,
                &mut previous_program,
                |i| i == 5,
            );
            let mut expected = std::array::from_fn::<_, 12, _>(neutral);
            if voice == 0 {
                expected[5] = [0., 0., 0., 5.];
            }
            if voice == 1 {
                expected[7][1] = 10.;
            }
            assert_eq!(
                factors.map(|v| v.map(f64::to_bits)),
                expected.map(|v| v.map(f64::to_bits))
            );
        }
    }

    #[test]
    fn native_knob_projection_preserves_cancellation_enabled_flags_and_voice_reuse() {
        let program = ModProgram {
            sources: vec![ModSource::Constant],
            routes: vec![
                ModRoute::new(0, ModTarget::ProcessorNativeCutoff(0), 1.),
                ModRoute::new(0, ModTarget::ProcessorNativeResonance(0), 1.),
                ModRoute::new(0, ModTarget::ProcessorNativeGain(0), 1.),
                ModRoute::new(0, ModTarget::ProcessorNativeGain(0), 1.),
            ],
            ..Default::default()
        };
        let modulation = VoiceModulation::new(vec![program], vec![Some(0)], vec![0]).unwrap();
        let mut state = VoiceModState::new(&modulation, 2).unwrap();
        state.program[0] = Some(0);
        state.processor_values[..4].copy_from_slice(&[0., 0.25, 0.5, -0.5]);
        state.previous_processor_values[..4].copy_from_slice(&[0., 0.25, 0.5, -0.5]);
        let mut actual = [[0.; 4]; 2];
        let mut previous_program = None;
        state.fill_filter_factors(&modulation, 0, &mut actual, &mut previous_program, |_| true);
        assert_eq!(actual, [[0., 0.25, 0., 7.], [0.; 4]]);
        state.fill_filter_factors(&modulation, 1, &mut actual, &mut previous_program, |_| true);
        assert_eq!(actual, [[0.; 4]; 2]);
    }

    #[test]
    fn held_controller_updates_addressed_filter_ramp_after_transition() {
        let program = ModProgram {
            sources: vec![ModSource::Controller(1)],
            routes: vec![ModRoute::new(0, ModTarget::ProcessorCutoff(0), 12.0)],
            ..Default::default()
        };
        let other = ModProgram {
            routes: vec![ModRoute::new(0, ModTarget::ProcessorCutoff(0), 6.0)],
            ..program.clone()
        };
        let modulation =
            VoiceModulation::new(vec![program, other], vec![Some(0), Some(1)], vec![0, 0]).unwrap();
        let mut state = VoiceModState::new(&modulation, 1).unwrap();
        let low = [0; 128];
        let mut high = low;
        high[1] = u32::MAX;
        let script = Default::default();
        let low_inputs = Inputs {
            velocity: 1.0,
            key: 0.5,
            pressure: 0,
            timbre: 0,
            controllers: &low,
            held: 0,
            script: &script,
            bend: 0.0,
            controls: RealValuesRef::new(&[]), audio: &[], interval: 0,
        };
        let clock = |now| Clock {
            rate: 48000.0,
            tempo: 120.0,
            now,
        };
        state.start(&modulation, 0, 0, &low_inputs, clock(0), 7);
        // A CC transition moves half an octave in its first cell, then holds
        // the complete octave. A cache hit must not repeat the transition.
        for (now, controls, factor) in [
            (0, &low, 1.0),
            (64, &high, 2f64.sqrt()),
            (128, &high, 2.0),
            (192, &low, 2f64.sqrt()),
            (256, &low, 1.0),
        ] {
            let inputs = Inputs {
                controllers: controls,
                ..low_inputs
            };
            state.advance(&modulation, 0, &inputs, clock(now));
            let mut factors = [[0.0; 4]; 1];
            let mut previous_program = None;
            state.fill_filter_factors(&modulation, 0, &mut factors, &mut previous_program, |_| {
                false
            });
            assert!((factors[0][0] - factor).abs() < 1e-15, "{now}: {factors:?}");
            assert_eq!(factors[0][1], 1.0);
        }
        let high_inputs = Inputs {
            controllers: &high,
            ..low_inputs
        };
        state.start(&modulation, 0, 0, &high_inputs, clock(320), 7);
        // Reusing a voice with the same source but another route resets its result.
        state.start(&modulation, 0, 1, &high_inputs, clock(384), 7);
        let mut factors = [[0.0; 4]; 1];
        let mut previous_program = None;
        state.fill_filter_factors(&modulation, 0, &mut factors, &mut previous_program, |_| {
            false
        });
        assert!((factors[0][0] - 2f64.sqrt()).abs() < 1e-15);
    }

    #[test]
    fn factor_projection_preserves_units_cancellation_and_reused_voice_scratch() {
        let program = ModProgram {
            sources: vec![ModSource::Velocity],
            routes: vec![
                ModRoute::new(0, ModTarget::ProcessorCutoff(0), 1.),
                ModRoute::new(0, ModTarget::ProcessorCutoff(0), 1.),
                ModRoute::new(0, ModTarget::ProcessorResonance(1), 1.),
                ModRoute::new(0, ModTarget::ProcessorCutoff(2), 1.),
                ModRoute::new(0, ModTarget::Pitch, 1.),
            ],
            ..ModProgram::default()
        };
        let modulation = VoiceModulation::new(vec![program], vec![Some(0)], vec![0]).unwrap();
        let mut state = VoiceModState::new(&modulation, 2).unwrap();
        state.program[0] = Some(0);
        let mut actual = [[1., 1., 0., 0.]; 4];
        let mut previous_program = None;
        for delta in [-24., -0., 0., 6., 20.] {
            state.processor_values[..5].copy_from_slice(&[12., -12., delta, 9., 127.]);
            state.previous_processor_values[..5].copy_from_slice(&[12., -12., delta, 3., -127.]);
            state.fill_filter_factors(&modulation, 0, &mut actual, &mut previous_program, |_| {
                false
            });
            let expected = [
                [1., 1., 0., 0.],
                [1., 10f64.powf(delta / 20.), 0., 0.],
                [(6f64 / 12.).exp2(), 1., 0., 0.],
                [1., 1., 0., 0.],
            ];
            assert_eq!(
                actual.map(|v| v.map(f64::to_bits)),
                expected.map(|v| v.map(f64::to_bits))
            );
            state.fill_filter_factors(&modulation, 1, &mut actual, &mut previous_program, |_| {
                false
            });
            assert_eq!(
                actual,
                [[1., 1., 0., 0.]; 4],
                "unbound voice must clear the previous voice's factors"
            );
        }
    }

    #[test]
    fn addressed_filter_factors_reset_and_sum_at_the_control_midpoint() {
        let modulation = VoiceModulation::new(
            vec![ModProgram {
                sources: vec![ModSource::Velocity],
                routes: vec![
                    ModRoute::new(0, ModTarget::ProcessorCutoff(1), 1.0),
                    ModRoute::new(0, ModTarget::ProcessorCutoff(1), 1.0),
                    ModRoute::new(0, ModTarget::ProcessorResonance(1), 1.0),
                    ModRoute::new(0, ModTarget::ProcessorResonance(2), 1.0),
                    ModRoute::new(0, ModTarget::ProcessorResonance(2), 1.0),
                ],
                ..ModProgram::default()
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap();
        let mut state = VoiceModState::new(&modulation, 1).unwrap();
        state.program[0] = Some(0);
        state
            .previous_processor_values
            .copy_from_slice(&[4., 2., 10., -20., 20.]);
        state
            .processor_values
            .copy_from_slice(&[8., 10., 30., -40., 40.]);
        let mut previous_program = None;
        let mut factors = [[1., 1., 0., 0.]; 128];
        state.fill_filter_factors(&modulation, 0, &mut factors, &mut previous_program, |_| {
            false
        });
        assert_eq!(factors[1], [2., 10., 0., 0.]);
        assert!(
            factors
                .iter()
                .enumerate()
                .all(|(i, f)| i == 1 || *f == [1., 1., 0., 0.])
        );
        // A subsequent voice must not inherit the previous addressed factors.
        state.stop(0);
        state.fill_filter_factors(&modulation, 0, &mut factors, &mut previous_program, |_| {
            false
        });
        assert!(factors.iter().all(|f| *f == [1., 1., 0., 0.]));
    }

    #[test]
    fn waves_start_where_their_documented_shapes_do() {
        for (shape, values) in [
            (LfoShape::Sine, [0.0, 1.0, 0.0, -1.0]),
            (LfoShape::Triangle, [0.0, 1.0, 0.0, -1.0]),
            (LfoShape::Square, [1.0, 1.0, -1.0, -1.0]),
            (LfoShape::SawUp, [-1.0, -0.5, 0.0, 0.5]),
            (LfoShape::SawDown, [1.0, 0.5, 0.0, -0.5]),
        ] {
            for (i, expected) in values.into_iter().enumerate() {
                let actual = wave(shape, i as f64 * 0.25, 0);
                assert!((actual - expected).abs() < 1e-12, "{shape:?} {i}: {actual}");
            }
        }
        // Held within a cycle, new across cycles, identical for the same seed.
        let held = wave(LfoShape::SampleAndHold, 3.1, 7);
        assert_eq!(held, wave(LfoShape::SampleAndHold, 3.9, 7));
        assert_ne!(held, wave(LfoShape::SampleAndHold, 4.1, 7));
        let random = |p| wave(LfoShape::Random, p, 7);
        assert!((random(4.0) - wave(LfoShape::SampleAndHold, 4.0, 7)).abs() < 1e-12);
        assert!((random(4.5) - (random(4.0) + random(5.0)) * 0.5).abs() < 1e-12);
        for p in 0..1000 {
            let v = random(p as f64 * 0.37);
            assert!((-1.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn shapes_interpolate_and_clamp_at_their_ends() {
        let points = [(0.0, 0.2), (0.5, 1.0), (1.0, 0.0)];
        assert_eq!(evaluate(&points, 0.0), 0.2);
        assert_eq!(evaluate(&points, 0.25), 0.6);
        assert_eq!(evaluate(&points, 0.75), 0.5);
        assert_eq!(evaluate(&[(0.5, 0.3)], 0.1), 0.3);
        assert_eq!(evaluate(&[(0.5, 0.3)], 0.9), 0.3);
    }

    #[test]
    fn source_phase_keeps_native_saved_base_and_literal_onset_vectors() {
        // Original Kontakt8.exe SHA0fe6356e: phase getter0x1408efd60,
        // builder0x140977dc5, reset0x1405f45a0 and Digital0x140b078d0.
        // (saved phase, Constant1 depth, absolute lane, first native sample).
        // These are original-byte leaf outputs, not a full-host clock claim.
        let vectors: [(f64, f64, f64, u32); 7] = [
            (0.125, 0., 0.125, 0xbf3504f7),
            (0.125, 0.125, 0.25, 0xbf800000),
            (0.125, -0.25, -0.125, 0x3f3504f7),
            (0.875, 0.25, 1.125, 0xbf3504f7),
            (0.875, 0.125, 1., 0x00000000),
            (0.3700000047683716, 0., 0.3700000047683716, 0xbf3a9da7),
            (0.3700000047683716, 0.125, 0.4950000047683716, 0xbd00a885),
        ];
        let controllers = [0; 128];
        let script = Default::default();
        for (saved, depth, native_phase, native_sample_bits) in vectors {
            let modulation = VoiceModulation::new(
                vec![ModProgram {
                    sources: vec![ModSource::Lfo(Lfo {
                        shape: LfoShape::SineScaled(-1.),
                        rate: LfoRate::Hertz(3.), phase: saved,
                        delay: 0, fade: 0, retrigger: true, shared: false,
                    }), ModSource::Constant],
                    routes: vec![ModRoute::new(1, ModTarget::SourcePhase(0), depth),
                        ModRoute::new(0, ModTarget::Pitch, 1.)],
                    ..Default::default()
                }], vec![Some(0)], vec![0],
            ).unwrap();
            let inputs = Inputs {
                velocity: 1., key: 0.5, pressure: 0, timbre: 0,
                controllers: &controllers, held: 0, script: &script,
                bend: 0., controls: RealValuesRef::new(&[]), audio: &[], interval: 0,
            };
            let mut state = VoiceModState::new(&modulation, 1).unwrap();
            for now in [0, 2400, 48127] {
                // Every onset must replace reused history with the saved base.
                state.phase[0] = 123.75;
                state.values[0] = -999.;
                state.start(&modulation, 0, 0, &inputs,
                    Clock { rate: 48000., tempo: 120., now }, 7);
                assert_eq!(state.phase[0].to_bits(), native_phase.to_bits());
                // Existing waveform approximation is intentionally unchanged.
                assert!((state.values[0] - f64::from(f32::from_bits(native_sample_bits))).abs() < 2e-6);
            }
        }
    }

    #[test]
    fn source_phase_bypass_keeps_base_and_held_depth_changes_wait_for_next_onset() {
        let mut modulation = VoiceModulation::new(vec![ModProgram {
            sources: vec![ModSource::Lfo(Lfo {
                shape: LfoShape::SineScaled(-1.), rate: LfoRate::Hertz(3.),
                phase: 0.375, delay: 0, fade: 0, retrigger: true, shared: false,
            }), ModSource::Constant],
            routes: vec![ModRoute::new(1, ModTarget::SourcePhase(0), 0.125),
                ModRoute::new(0, ModTarget::Pitch, 1.)],
            ..Default::default()
        }], vec![Some(0)], vec![0]).unwrap();
        // Already resolved cells; public schema/heap behavior is tested by Runtime.
        modulation.programs[0].controls = vec![(0, [RealControlIndex::test(0), RealControlIndex::test(1)])].into_boxed_slice();
        let mut actual = VoiceModState::new(&modulation, 1).unwrap();
        let mut unchanged = VoiceModState::new(&modulation, 1).unwrap();
        let controllers = [0; 128];
        let script = Default::default();
        let old = [crate::control::RealCell::test(0.125), crate::control::RealCell::test(0.)];
        let edited = [crate::control::RealCell::test(-0.25), crate::control::RealCell::test(0.)];
        let bypass = [crate::control::RealCell::test(0.125), crate::control::RealCell::test(1.)];
        let inputs = Inputs {
            velocity: 1., key: 0.5, pressure: 0, timbre: 0,
            controllers: &controllers, held: 0, script: &script,
            bend: 0., controls: RealValuesRef::new(&old), audio: &[], interval: 0,
        };
        let clock = |now| Clock { rate: 48000., tempo: 120., now };
        actual.start(&modulation, 0, 0, &inputs, clock(2400), 7);
        unchanged.start(&modulation, 0, 0, &inputs, clock(2400), 7);
        assert_eq!(actual.phase[0].to_bits(), 0.5f64.to_bits());
        let held_inputs = Inputs { controls: RealValuesRef::new(&edited), ..inputs };
        for now in [2400, 2432, 2464, 2528, 2591] {
            actual.advance(&modulation, 0, &held_inputs, clock(now));
            unchanged.advance(&modulation, 0, &inputs, clock(now));
            assert_eq!(actual.phase[0].to_bits(), unchanged.phase[0].to_bits());
            assert_eq!(actual.values[0].to_bits(), unchanged.values[0].to_bits());
        }
        actual.start(&modulation, 0, 0, &held_inputs, clock(3000), 8);
        assert_eq!(actual.phase[0].to_bits(), 0.125f64.to_bits());
        actual.start(&modulation, 0, 0, &Inputs { controls: RealValuesRef::new(&bypass), ..inputs }, clock(6000), 9);
        assert_eq!(actual.phase[0].to_bits(), 0.375f64.to_bits());
    }

    #[test]
    fn a_shared_retriggered_lfo_restarts_for_every_voice() {
        // A 1 Hz square read at 1 kHz: +1 for the first half cycle.
        let pitch_of = |shared: bool| {
            let lfo = Lfo {
                shape: LfoShape::Square,
                rate: LfoRate::Hertz(1.0),
                phase: 0.0,
                delay: 0,
                fade: 0,
                retrigger: true,
                shared,
            };
            let program = ModProgram {
                sources: vec![ModSource::Lfo(lfo)],
                routes: vec![ModRoute::new(0, ModTarget::Pitch, 1.0)],
                ..ModProgram::default()
            };
            let modulation = VoiceModulation::new(vec![program], vec![Some(0)], vec![0]).unwrap();
            let mut state = VoiceModState::new(&modulation, 2).unwrap();
            let controllers = [0; 128];
            let inputs = Inputs {
                velocity: 1.0,
                key: 0.5,
                pressure: 0,
                timbre: 0,
                controllers: &controllers,
                held: 0,
                script: &Default::default(),
                bend: 0.0,
                controls: RealValuesRef::new(&[]), audio: &[], interval: 0,
            };
            let clock = |now| Clock {
                rate: 1000.0,
                tempo: 120.0,
                now,
            };
            state.start(&modulation, 0, 0, &inputs, clock(0), 1);
            let p = &modulation.programs[0];
            state.evaluate(p, 0, &inputs, clock(600), 600, false,state.restart_point(0,0),ProviderView::empty());
            // A second note at 0.6 s restarts the shared cycle under voice 0.
            state.start(&modulation, 1, 0, &inputs, clock(600), 2);
            state.evaluate(p, 0, &inputs, clock(700), 100, false,state.restart_point(0,0),ProviderView::empty()).pitch
        };
        assert_eq!(
            pitch_of(false),
            -1.0,
            "own cycle: 0.7 s is in its second half"
        );
        assert_eq!(pitch_of(true), 1.0, "restarted 0.1 s ago: first half");
    }
}

#[cfg(test)]
mod scoped_restart_tests {
    use super::*;
    fn program()->ModProgram {
        ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Square,rate:LfoRate::Hertz(1.),phase:0.,delay:0,fade:0,retrigger:true,shared:true})],
            routes:vec![ModRoute::new(0,ModTarget::Pan,1.)],..Default::default()}
    }
    #[test]
    fn failed_unpublished_restart_keeps_other_owner_and_distinct_program_domain() {
        let modulation=VoiceModulation::new(vec![program(),program()],vec![Some(0),Some(1)],vec![0;2]).unwrap();
        let mut state=VoiceModState::new_scoped(&modulation,3).unwrap();
        let cc=[0;128];let values=Default::default();
        let inputs=Inputs{velocity:1.,key:0.5,pressure:0,timbre:0,controllers:&cc,held:0,script:&values,bend:0.,controls:RealValuesRef::new(&[]),audio:&[],interval:0};
        let clock=|now|Clock{rate:1000.,tempo:120.,now};
        state.start(&modulation,0,0,&inputs,clock(0),1);state.commit_scoped_start(0);
        state.start(&modulation,1,0,&inputs,clock(600),2);
        state.stop(1); // rejected source family never publishes its restart
        let p=&modulation.programs[0];
        assert_eq!(state.evaluate(p,0,&inputs,clock(700),700,false,state.restart_point(0,0),ProviderView::empty()).scalar_pan,-1.);
        state.start(&modulation,2,1,&inputs,clock(700),3);state.commit_scoped_start(2);
        assert_eq!(state.evaluate(p,0,&inputs,clock(750),50,false,state.restart_point(0,0),ProviderView::empty()).scalar_pan,-1.,"other program cannot reset this domain");
        state.start(&modulation,1,0,&inputs,clock(750),4);state.commit_scoped_start(1);
        assert_eq!(state.evaluate(p,0,&inputs,clock(800),50,false,state.restart_point(0,0),ProviderView::empty()).scalar_pan,1.,"successful same-program shared retrigger is published once");
    }
    #[test]
    fn scoped_scalar_point_retains_unclamped_pan_and_gain_separately_from_legacy_balance() {
        let program=ModProgram{sources:vec![ModSource::Constant],routes:vec![ModRoute::new(0,ModTarget::Pan,2.),ModRoute::new(0,ModTarget::Decibels,6.)],..Default::default()};
        let modulation=VoiceModulation::new(vec![program],vec![Some(0)],vec![0]).unwrap();
        let mut state=VoiceModState::new_scoped(&modulation,1).unwrap();let cc=[0;128];let values=Default::default();
        let inputs=Inputs{velocity:1.,key:0.5,pressure:0,timbre:0,controllers:&cc,held:0,script:&values,bend:0.,controls:RealValuesRef::new(&[]),audio:&[],interval:0};
        state.start(&modulation,0,0,&inputs,Clock{rate:48000.,tempo:120.,now:97},1);
        let out=state.outputs[0];assert_eq!(out.scalar_pan,2.);assert!((out.scalar_gain-10f64.powf(6./20.)).abs()<1e-15);
        assert_eq!(out.gains[0],0.);assert_eq!(out.gains[1],out.scalar_gain as f32);
        assert_eq!(state.times[0],[97;2]);
    }
    #[test]
    fn scoped_storage_preflight_prices_all_array_families_and_overflow() {
        let modulation=VoiceModulation::new(vec![program()],vec![Some(0)],vec![0]).unwrap();let shape=modulation.shape();
        let row=VoiceModState::checked_storage_bytes(shape,1,0).unwrap();
        assert_eq!(modulation.bytes_per_voice(),row);
        assert_eq!(VoiceModState::checked_storage_bytes(shape,4,1).unwrap(),row*4+4*shape.sources*std::mem::size_of::<f64>()+std::mem::size_of::<(u64,u64)>()+4*std::mem::size_of::<Option<(u64,u64)>>());
        assert!(matches!(VoiceModState::checked_storage_bytes(shape,usize::MAX,1),Err(Error::Capacity)));
    }
}

impl VoiceModulation {
    pub(crate) fn source_bipolar(&self,source:ProcessingSourceRef)->Result<bool,Error>{
        let program=self.program(source.scope).ok_or(Error::InvalidInput)?;
        let program=&self.programs[program as usize];
        if matches!(program.sources.get(source.source),None|Some(Prepared::Provider(_))){return Err(Error::InvalidInput);}
        program.bipolar.get(source.source).copied().ok_or(Error::InvalidInput)
    }
    pub(crate) fn validate_provider_bindings(&self,owners:&VoiceModulation,regions:&[Option<(usize,usize)>])->Result<(),Error>{
        // Legacy Prepared::new has no shared-processing region roster. Only an
        // actual retained provider program depends on that separate metadata;
        // local source programs keep their ordinary physical-region mapping.
        let mut has_providers=false;
        for p in &self.programs {
            let provider_rows=p.sources.iter().filter(|source|matches!(source,Prepared::Provider(_))).count();
            if provider_rows==0 && p.provider_scope.is_none(){continue;}
            has_providers=true;
            let scope=p.provider_scope.ok_or(Error::InvalidInput)?;
            if provider_rows==0 || p.bipolar.len()!=p.sources.len(){return Err(Error::InvalidInput);}
            for (index,source)in p.sources.iter().enumerate(){if let Prepared::Provider(source)=source{
                if source.scope!=scope || p.bipolar[index]!=owners.source_bipolar(*source)?{return Err(Error::InvalidInput);}
            }}
        }
        if !has_providers{return Ok(());}
        if regions.len()!=self.regions.len(){return Err(Error::InvalidInput);}
        for (region,program)in self.regions.iter().enumerate(){
            let Some(program)=program else{continue;};
            let p=self.programs.get(*program as usize).ok_or(Error::InvalidInput)?;
            if let Some(scope)=p.provider_scope
                && regions[region].is_none_or(|(actual,_)|actual!=scope){return Err(Error::InvalidInput);}
        }Ok(())
    }
    pub(crate) fn region_provider_scope(&self,region:usize)->Option<usize>{self.program(region).and_then(|program|self.programs[program as usize].provider_scope)}
}
impl VoiceModState {
    /// The amplitude envelope stage factors of one voice: 1.0 leaves a stage
    /// at its authored time.
    pub(crate) fn envelope_stages(&self, voice: usize) -> [f64; 5] {
        self.outputs
            .get(voice)
            .map(|output| output.envelope_stage)
            .unwrap_or([1.0; 5])
    }
    pub(crate) fn provider_scope(&self,modulation:&VoiceModulation,voice:usize)->Option<usize>{self.program(voice).and_then(|program|modulation.programs[program as usize].provider_scope)}
    pub(crate) fn provider_view(&self,modulation:&VoiceModulation,owner:usize,scope:usize)->Result<ProviderView<'_>,Error>{
        let program=self.program(owner).ok_or(Error::NotReady)?;
        if modulation.program(scope)!=Some(program)||self.previous_values.is_empty(){return Err(Error::InvalidInput);}
        let count=modulation.programs[program as usize].sources.len();let begin=owner.checked_mul(self.sources).ok_or(Error::Capacity)?;
        let end=begin.checked_add(count).ok_or(Error::Capacity)?;let [from,to]=self.times[owner];
        Ok(ProviderView{scope:Some(scope),from:self.previous_values.get(begin..end).ok_or(Error::InvalidInput)?,to:self.values.get(begin..end).ok_or(Error::InvalidInput)?,begin:from,end:to})
    }
    fn validate_provider_view(&self,modulation:&VoiceModulation,voice:usize,provider:ProviderView<'_>,frame:u64)->Result<u32,Error>{
        let index=self.program(voice).ok_or(Error::NotReady)?;let p=&modulation.programs[index as usize];
        if p.provider_scope!=provider.scope||provider.scope.is_none()||provider.begin>frame||provider.end<frame{return Err(Error::InvalidInput);}
        for source in &p.sources{if let Prepared::Provider(source)=source{
            if Some(source.scope)!=provider.scope||source.source>=provider.from.len()||source.source>=provider.to.len(){return Err(Error::InvalidInput);}
        }}Ok(index)
    }
    pub(crate) fn defer_provider_restart(&mut self,modulation:&VoiceModulation,voice:usize){
        if self.provider_scope(modulation,voice).is_some(){self.pending_start[voice]=true;}
    }
    pub(crate) fn advance_provided(&mut self,modulation:&VoiceModulation,voice:usize,inputs:&Inputs<'_>,clock:Clock,provider:ProviderView<'_>,closed:bool)->Result<Ramp,Error>{
        let index=self.validate_provider_view(modulation,voice,provider,clock.now)?;
        let target=clock.now.checked_div(CELL).and_then(|n|n.checked_add(1)).and_then(|n|n.checked_mul(CELL)).ok_or(Error::ClockOverflow)?;
        self.validate_provider_view(modulation,voice,provider,target)?;
        if self.pending_start[voice]{let seed=self.seed[voice];self.start_initial_program(modulation,voice,Some(index),inputs,clock,seed,false,closed,provider);}
        Ok(self.advance_view(modulation,voice,inputs,clock,provider))
    }
}


#[cfg(test)]
mod provider_allocation_accounting {
    use super::*;
    #[test]
    fn provider_slots_have_no_duplicate_envelope_state_and_all_actual_arrays_are_priced(){
        let envelope=Envelope::new(0,0,0,1.,97).unwrap();
        let owner=VoiceModulation::new(vec![ModProgram{sources:vec![ModSource::Envelope(envelope)],..Default::default()}],vec![Some(0)],vec![0]).unwrap();
        let physical=VoiceModulation::new_resolved_providers(vec![ModProgram{sources:vec![ModSource::Envelope(envelope)],routes:vec![ModRoute::new(0,ModTarget::Pitch,3.)],..Default::default()}],vec![Some(0)],vec![0],vec![None],
            vec![(ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope:0,source:0}},false)],|_|None,|_|Err(Error::InvalidInput)).unwrap();
        assert_eq!(physical.envelopes,0);assert!(physical.programs[0].envelopes.is_empty());
        assert!(matches!(physical.programs[0].sources[0],Prepared::Provider(ProcessingSourceRef{scope:0,source:0})));
        let state=VoiceModState::new_scoped(&owner,7).unwrap();
        let actual=std::mem::size_of_val(&*state.program)+std::mem::size_of_val(&*state.seed)+std::mem::size_of_val(&*state.age)
            +std::mem::size_of_val(&*state.outputs)+std::mem::size_of_val(&*state.previous)+std::mem::size_of_val(&*state.times)
            +std::mem::size_of_val(&*state.tone)+std::mem::size_of_val(&*state.phase)+std::mem::size_of_val(&*state.values)
            +std::mem::size_of_val(&*state.previous_values)+std::mem::size_of_val(&*state.pending_start)
            +std::mem::size_of_val(&*state.lagged)+std::mem::size_of_val(&*state.scale_lagged)
            +std::mem::size_of_val(&*state.processor_values)+std::mem::size_of_val(&*state.previous_processor_values)
            +std::mem::size_of_val(&*state.envelope)+std::mem::size_of_val(&*state.segments)
            +std::mem::size_of_val(&*state.restart_domains)+std::mem::size_of_val(&*state.pending_restart);
        assert_eq!(actual,VoiceModState::checked_storage_bytes(owner.shape(),7,1).unwrap());
        assert!(matches!(VoiceModState::new_scoped(&owner,usize::MAX),Err(Error::Capacity)));
    }
}


#[cfg(test)]
mod provider_endpoint_precision {
    use super::ProviderView;
    #[test]
    fn borrowed_source_endpoints_preserve_their_exact_bits_before_route_math(){
        let from=[1.,-0.];let to=[f64::MIN_POSITIVE,0.];
        let points=ProviderView{scope:Some(0),from:&from,to:&to,begin:17,end:64};
        assert_eq!(points.at(0,17).to_bits(),1f64.to_bits());
        assert_eq!(points.at(0,64).to_bits(),f64::MIN_POSITIVE.to_bits(),"one exact nonzero future point is not cancellation-rounded to0");
        assert_eq!(points.at(1,17).to_bits(),(-0f64).to_bits());
        assert_eq!(points.at(1,64).to_bits(),0f64.to_bits());
        let simple_from=[0.];let simple_to=[1.];let middle=ProviderView{scope:Some(0),from:&simple_from,to:&simple_to,begin:0,end:64};
        assert_eq!(middle.at(0,16),0.25);assert_eq!(middle.at(0,32),0.5);assert_eq!(middle.at(0,48),0.75);
    }
}


#[cfg(test)]
mod provider_legacy_validation164 {
    use super::*;

    fn local() -> VoiceModulation {
        VoiceModulation::new(vec![ModProgram {
            sources: vec![ModSource::Constant],
            routes: vec![ModRoute::new(0, ModTarget::Pan, 0.25)],
            ..Default::default()
        }], vec![Some(0)], vec![0]).unwrap()
    }

    fn provider() -> (VoiceModulation, VoiceModulation) {
        let owner = local();
        let source = ProcessingSourceRef { scope: 0, source: 0 };
        let bipolar = owner.source_bipolar(source).unwrap();
        let physical = VoiceModulation::new_resolved_providers(
            vec![ModProgram {
                sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, ModTarget::Pan, 0.25)],
                ..Default::default()
            }], vec![Some(0)], vec![0], vec![None],
            vec![(ModProvider { program: 0, source: 0, provider: source }, bipolar)],
            |_| None, |_| Err(Error::InvalidInput),
        ).unwrap();
        (owner, physical)
    }

    #[test]
    fn local_programs_do_not_depend_on_unrelated_shared_region_metadata() {
        let physical = local();
        let unrelated_owner = local();
        for regions in [vec![], vec![None], vec![Some((99, 17)); 3]] {
            assert_eq!(physical.validate_provider_bindings(&VoiceModulation::default(), &regions), Ok(()));
            assert_eq!(physical.validate_provider_bindings(&unrelated_owner, &regions), Ok(()));
        }
        // The ordinary local-region/program constructor still owns its shape.
        assert!(matches!(VoiceModulation::new(vec![ModProgram::default()], vec![Some(9)], vec![0]), Err(Error::InvalidInput)));
        assert!(matches!(VoiceModulation::new(vec![ModProgram::default()], vec![Some(0)], vec![]), Err(Error::InvalidInput)));
    }

    #[test]
    fn real_provider_requires_complete_matching_shared_roster_and_source() {
        let (owner, physical) = provider();
        assert_eq!(physical.validate_provider_bindings(&owner, &[Some((0, 0))]), Ok(()));
        for regions in [vec![], vec![None], vec![Some((1, 0))], vec![Some((0, 0)); 2]] {
            assert_eq!(physical.validate_provider_bindings(&owner, &regions), Err(Error::InvalidInput));
        }
        assert_eq!(physical.validate_provider_bindings(&VoiceModulation::default(), &[Some((0, 0))]), Err(Error::InvalidInput));
        let (mut nested_owner, physical) = provider();
        nested_owner.programs[0].sources[0] = Prepared::Provider(ProcessingSourceRef { scope: 0, source: 0 });
        assert_eq!(physical.validate_provider_bindings(&nested_owner, &[Some((0, 0))]), Err(Error::InvalidInput));
    }

    #[test]
    fn malformed_provider_metadata_cannot_hide_behind_an_absent_or_unused_roster() {
        for malformed in 0..9 {
            let (owner, mut physical) = provider();
            match malformed {
                0 => physical.programs[0].provider_scope = None,
                1 => physical.programs[0].provider_scope = Some(1),
                2 => physical.programs[0].sources[0] = Prepared::Input(InputSource::Constant),
                3 => physical.programs[0].bipolar = Box::new([]),
                4 => physical.programs[0].bipolar = vec![false; 2].into_boxed_slice(),
                5 => physical.programs[0].bipolar[0] = !physical.programs[0].bipolar[0],
                6 => physical.programs[0].sources[0] = Prepared::Provider(ProcessingSourceRef { scope: 0, source: 9 }),
                7 => physical.regions[0] = Some(99),
                _ => { physical.regions[0] = None; physical.programs[0].provider_scope = None; }
            }
            assert_eq!(physical.validate_provider_bindings(&owner, &[Some((0, 0))]), Err(Error::InvalidInput), "malformed provider case {malformed}");
            assert_eq!(physical.validate_provider_bindings(&owner, &[]), Err(Error::InvalidInput), "missing metadata must not hide malformed provider case {malformed}");
        }
    }
}

#[cfg(test)]
mod follower_source_tests {
    use super::*;

    fn modulation() -> VoiceModulation {
        let program = ModProgram {
            sources: vec![ModSource::Follower([1.0, 1.0, 6.0, 0.5])],
            routes: vec![ModRoute::new(0, ModTarget::Pan, 1.)],
            ..Default::default()
        };
        VoiceModulation::new(vec![program], vec![Some(0)], vec![0]).unwrap()
    }

    fn inputs<'a>(cc: &'a [u32; 128], values: &'a crate::script_params::ModValues, audio: &'a [f32]) -> Inputs<'a> {
        Inputs {
            velocity: 1.,
            key: 0.5,
            pressure: 0,
            timbre: 0,
            controllers: cc,
            held: 0,
            script: values,
            bend: 0.,
            controls: RealValuesRef::new(&[]),
            audio,
            interval: 0,
        }
    }

    #[test]
    fn follower_source_follows_the_span_audio_and_holds_without_it() {
        let modulation = modulation();
        let mut state = VoiceModState::new(&modulation, 1).unwrap();
        let cc = [0; 128];
        let values = crate::script_params::ModValues::default();
        let clock = |now| Clock { rate: 48_000.0, tempo: 120.0, now };
        let silent: Vec<f32> = vec![0.0; 64];
        let loud: Vec<f32> = vec![1.0; 64];
        state.start(&modulation, 0, 0, &inputs(&cc, &values, &[]), clock(0), 1);
        let p = &modulation.programs[0];
        let mut run = |state: &mut VoiceModState, audio: &[f32], now: u64| {
            let restart = state.restart_point(0, 0);
            state.evaluate(p, 0, &inputs(&cc, &values, audio), clock(now), 64, false, restart, ProviderView::empty());
            state.values[0]
        };
        let first = run(&mut state, &loud, 64);
        let second = run(&mut state, &loud, 128);
        assert!(first > 0.0 && second > first, "rises towards the loud input: {first} {second}");
        // The second state recovers towards its first-state ceiling, which itself
        // decays, so the level falls over a long silence rather than at once.
        let decayed = (0..40).map(|k| run(&mut state, &silent, 192 + 64 * k)).last().unwrap();
        assert!(decayed > 0.0 && decayed < 0.5, "falls over silence, tracking the decayed ceiling: {decayed}");
        let held = run(&mut state, &[], 192 + 64 * 40);
        assert_eq!(held, decayed, "no audio supplied: the follower holds");
    }
}

#[cfg(test)]
mod glide_source_tests {
    use super::*;

    fn modulation() -> VoiceModulation {
        let program = ModProgram {
            sources: vec![ModSource::Glide { mode: 1, time_ms: 100.0 }],
            routes: vec![ModRoute::new(0, ModTarget::Pan, 1.)],
            ..Default::default()
        };
        VoiceModulation::new(vec![program], vec![Some(0)], vec![0]).unwrap()
    }

    fn inputs<'a>(cc: &'a [u32; 128], values: &'a crate::script_params::ModValues, key: u8, interval: i32) -> Inputs<'a> {
        Inputs {
            velocity: 1.,
            key: f64::from(key) / 127.0,
            pressure: 0,
            timbre: 0,
            controllers: cc,
            held: 0,
            script: values,
            bend: 0.,
            controls: RealValuesRef::new(&[]),
            audio: &[],
            interval,
        }
    }

    #[test]
    fn glide_starts_at_the_interval_from_the_previous_note_and_ramps_to_zero() {
        let modulation = modulation();
        let mut state = VoiceModState::new(&modulation, 1).unwrap();
        let (cc, values) = ([0; 128], crate::script_params::ModValues::default());
        let clock = |now| Clock { rate: 1000.0, tempo: 120.0, now };
        let p = &modulation.programs[0];
        // The first note has no previous note: nothing to glide from.
        assert_eq!(state.glide_interval(60), 0);
        state.start(&modulation, 0, 0, &inputs(&cc, &values, 60, 0), clock(0), 1);
        assert_eq!(state.values[0], 0.0, "first note");
        // The next note sees the previous key and glides seven semitones up.
        assert_eq!(state.glide_interval(67), 7);
        state.start(&modulation, 0, 0, &inputs(&cc, &values, 67, 7), clock(0), 2);
        assert!((state.values[0] + 7.0 / 12.0).abs() < 1e-6, "starts at -interval/12 octaves");
        let mut last = state.values[0];
        for k in 1..=12u64 {
            let restart = state.restart_point(0, 0);
            state.evaluate(p, 0, &inputs(&cc, &values, 67, 0), clock(64 * k), 64, false, restart, ProviderView::empty());
            assert!(state.values[0] >= last, "rises towards zero");
            last = state.values[0];
        }
        assert!(last.abs() < 1e-3, "lands on zero after 700 frames: {last}");
    }

    #[test]
    fn downward_interval_mirrors_the_glide_and_zero_interval_is_silent() {
        let modulation = modulation();
        let mut state = VoiceModState::new(&modulation, 1).unwrap();
        let (cc, values) = ([0; 128], crate::script_params::ModValues::default());
        let clock = |now| Clock { rate: 1000.0, tempo: 120.0, now };
        state.start(&modulation, 0, 0, &inputs(&cc, &values, 67, 0), clock(0), 1);
        state.start(&modulation, 0, 0, &inputs(&cc, &values, 60, -7), clock(0), 2);
        assert!((state.values[0] - 7.0 / 12.0).abs() < 1e-6, "downward starts above zero");
        state.start(&modulation, 0, 0, &inputs(&cc, &values, 60, 0), clock(0), 3);
        assert_eq!(state.values[0], 0.0, "no interval, no glide");
    }
}
