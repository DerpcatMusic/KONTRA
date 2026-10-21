//! Format-neutral semantic instrument description.
//!
//! Source translators (Kontakt, SFZ, ...) emit an [`Instrument`]; the native
//! runtime lowers it to prepared execution tables. Plain data: no I/O, no
//! audio-thread state, no dependencies. Quantities keep source units
//! ([`units`]), every modulator and processor names its [`Scope`], and meaning
//! a translator cannot express is listed in [`Instrument::unsupported`] rather
//! than dropped.
#![forbid(unsafe_code)]

pub mod fx;
/// Authored zones (or scope members) one source-processing owner can address.
/// This is an engine capacity, not a native UVI law: lowering and the core
/// topology use the same bound. ponytail: raise if a real keygroup exceeds it.
pub const MAX_PROCESSING_MEMBERS: usize = 64;
pub mod kontakt;
pub mod units;
mod validate;

pub use units::{Frequency, Gain, Pan, PanLaw, Pitch, Resonance, SourceFrames, Span, Time};
pub use validate::{Reference, ValidationError};

/// Upper bound on source processing scopes in one instrument. A scope is one
/// authored source owner, so large multi-keygroup libraries legitimately need
/// one per keygroup; this only bounds the definition roster, not live voices.
pub const MAX_PROCESSING_SCOPES: usize = 4096;

macro_rules! reference {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub usize);
    )*};
}

reference! {
    /// Index into [`Instrument::assets`].
    AssetRef,
    /// Index into [`Instrument::groups`].
    GroupRef,
    /// Index into [`Instrument::zones`].
    ZoneRef,
    /// Index into [`Instrument::sequences`].
    SequenceRef,
    /// Index into [`Instrument::articulations`].
    ArticulationRef,
    /// Index into [`Instrument::modulators`].
    ModulatorRef,
    /// Index into [`Instrument::chains`].
    ChainRef,
    /// Index into [`Instrument::buses`].
    BusRef,
    /// Index into [`Instrument::controls`].
    ControlRef,
    /// Index into [`Instrument::routes`].
    RouteRef,
    /// Index into [`Instrument::shapes`].
    ShapeRef,
}

/// Shared with the loaded script owner; opening Scripts is the first report build.
#[derive(Clone, PartialEq)]
pub struct UviScriptSources {
    pub xml: std::sync::Arc<str>,
    pub files: std::sync::Arc<Vec<(String, String)>>,
}

impl std::fmt::Debug for UviScriptSources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UviScriptSources (source omitted)")
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instrument {
    pub name: String,
    pub source: SourceFormat,
    pub assets: Vec<Asset>,
    pub groups: Vec<Group>,
    pub default_keyswitch: Option<u8>,
    /// Physical source indices, retained across omissions and key-range filtering.
    pub source_indices: SourceIndices,
    pub zones: Vec<Zone>,
    /// Native source identity -> fully translated destination recipes.
    #[cfg_attr(feature = "cache", serde(default))]
    pub zone_routing: Vec<ZoneRouting>,
    #[cfg_attr(feature = "cache", serde(default))]
    pub zone_routing_mode: Option<u8>,
    pub sequences: Vec<Sequence>,
    pub articulations: Vec<Articulation>,
    /// Independent nested selectors (see [`Axis`]), beside the articulation.
    pub axes: Vec<Axis>,
    /// Which input selects among [`Instrument::articulations`].
    pub switching: Switching,
    pub modulators: Vec<Modulator>,
    pub routes: Vec<Route>,
    /// Transfer curves routes apply to their source value.
    pub shapes: Vec<Shape>,
    pub chains: Vec<Chain>,
    /// Explicit per-selection-activation processor owners. This namespace is
    /// independent of GroupRef, selection families and source storage packs.
    /// Empty retains the existing singleton source execution exactly.
    #[cfg_attr(feature = "cache", serde(default))]
    pub processing_scopes: Vec<ProcessingScope>,
    pub buses: Vec<Bus>,
    /// Explicit channel programs on existing GLOBAL bus owners, never voice activations.
    #[cfg_attr(feature="cache",serde(default))]
    pub processing_buses:Vec<ProcessingBus>,
    /// Sum entering the instrument/program inserts, after upstream group buses.
    /// Direct outputs before this point bypass instrument-level processing.
    pub input_bus: Option<BusRef>,
    /// Source addresses of buses (Kontakt's `$NI_BUS_OFFSET` + n), which a
    /// script's output-channel writes name.
    pub bus_addresses: Vec<(i32, BusRef)>,
    /// Impulse responses bus convolutions refer to.
    pub impulses: Vec<Impulse>,
    /// How the importing profile's loader shapes each of [`Instrument::impulses`]
    /// once its audio is decoded (parallel; missing or `None` entries stay as decoded).
    pub impulse_shapes: Vec<Option<ImpulseShape>>,
    pub controls: Vec<Control>,
    /// Actual processor lanes driven by authored controls; indices span pre then post.
    pub processor_controls: Vec<ProcessorControl>,
    /// Ordered per-voice sends at authored processor boundaries.
    pub voice_send_taps: Vec<VoiceSendTap>,
    pub behaviors: Vec<Behavior>,
    /// In-memory UVI inputs for lazy script inspection; never serialized.
    #[cfg_attr(feature = "cache", serde(skip))]
    pub uvi_script_sources: Option<UviScriptSources>,
    /// Saved source automation, addressed by slider declaration ordinal per slot.
    pub script_automation: Vec<ScriptAutomation>,
    /// Polyphony of the whole instrument.
    pub voice_limit: Option<VoiceLimit>,
    /// Polyphony of voice groups; [`Group::voice_limit`] indexes this.
    pub voice_limits: Vec<VoiceLimit>,
    /// A host controller that sets the instrument volume once it arrives.
    pub host_volume: Option<HostVolume>,
    /// Original Kontakt settings, including values not yet admitted by playback.
    pub kontakt_objects: Option<Box<kontakt::Objects>>,
    /// Authored source modules and fields retained for format/DSP handoff.
    /// Includes bypassed modules and unmodeled fields. Lowering does not execute
    /// these records; executable semantics remain in processors/routes above.
    // Source handoff records can contain opaque payloads; keep them in RAM only.
    #[cfg_attr(feature = "cache", serde(skip))]
    pub source_parameters: Vec<SourceParameterRecord>,
    /// Source meaning this description does not carry. Lowering never reads it;
    /// it exists so a caller can show or reject what was not translated.
    pub unsupported: Vec<Unsupported>,
    /// Complete authored DSP slot inventory, collected only by diagnostic builds.
    pub dsp_slots: Option<Vec<DspSlot>>,
    /// Native XML records for the independent audition oracle, never consumed by playback.
    pub native_family: Option<NativeFamily>,
    /// Native groups with an active sample-start modulation target.
    pub native_start_mod_groups: Option<Vec<u32>>,
    /// A snapshot load: each behavior's own saved state before the snapshot's
    /// values replaced it (`behaviors[i].state`); empty otherwise. Scripts run
    /// `on persistence_changed` with it first, as Kontakt loads the instrument
    /// and then applies the snapshot.
    #[cfg_attr(feature = "cache", serde(skip))]
    pub snapshot_base_state: Vec<Vec<(String, Saved)>>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamily {
    pub zones: Vec<NativeFamilyZone>,
    pub unknown: Option<String>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamilyZone {
    pub id: u32,
    pub group: u32,
    pub keys: KeyRange,
    pub velocities: VelocityRange,
    pub muted: bool,
    pub start: i64,
    pub end: i64,
    pub frames: i64,
    pub reverse: bool,
    pub loops: Vec<NativeFamilyLoop>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamilyLoop {
    pub slot: usize,
    pub mode: i32,
    pub start: i64,
    pub length: i64,
    pub count: i32,
    pub alternating: bool,
    pub crossfade: i64,
    pub tuning: f64,
}

/// One occupied native slot; addresses never compact around holes.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DspSlot {
    pub kind: DspSlotKind,
    pub scope: String,
    pub slot: usize,
    pub module: String,
    pub enabled: bool,
    pub disposition: DspDisposition,
    pub targets: Vec<DspTarget>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DspTarget {
    pub ordinal: usize,
    pub parameter: String,
    pub module_slot: Option<usize>,
    /// Physical insert lookup or XML owner; source-control slot namespaces are unverified.
    pub module: Option<String>,
    pub enabled: bool,
    pub disposition: DspDisposition,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspSlotKind {
    Fx,
    Filter,
    Mod,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspDisposition {
    Implemented,
    Approximated(DspSlotReason),
    Dropped(DspSlotReason),
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspSlotReason {
    NotModeled,
    Malformed,
    NativeLawUnverified,
    TargetsDropped,
    SourceNotExecuted,
    ResourceUnavailable,
    SavedBypassNotInstantiated,
    MutedScopeNotInstantiated,
    ScopeNotInstantiated,
}

/// `members[k]` is member `k`'s `[left, right]` gain. Length equals the bundle width.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct BundlePlacement {
    pub location: String,
    pub members: Vec<[f64; 2]>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceIndices {
    pub groups: Vec<Option<GroupRef>>,
    pub zones: Vec<Option<ZoneRef>>,
    /// Original source-owner hierarchy. Identities are document-local native
    /// addresses, not runtime handles, display names, or processing-owner IDs.
    /// The compiled plan/actual runtime owner supplies generation fencing.
    /// Empty is legacy metadata absence, never proof that all scopes coincide.
    #[cfg_attr(feature = "cache", serde(default))]
    pub owners: Vec<SourceOwner>,
    /// Source-defined shared processing recipes. No width is guessed during
    /// XML/container translation; the normal loader binds actual source ports.
    #[cfg_attr(feature = "cache", serde(default))]
    pub processing: Vec<SourceProcessingRecipe>,
    /// Exact original processor operands on a real per-activation owner. These
    /// are not output Gain/Pan roles or a cache of current parameter values.
    #[cfg_attr(feature = "cache", serde(default))]
    pub processing_parameters: Vec<SourceProcessingParameter>,
    /// Full authored matrices, before actual source geometry is known. These
    /// cells are not an early decoder fold or a guessed two-channel graph.
    #[cfg_attr(feature = "cache", serde(default))]
    pub processing_matrices: Vec<SourceProcessingMatrix>,
    /// Explicit caller-supplied conversion at the source-owner output port.
    /// Absent is not permission to drop wide channels or infer native enums.
    #[cfg_attr(feature = "cache", serde(default))]
    pub processing_ports: Vec<SourceProcessingPort>,
    /// Stereo placement of each member of a channel bundle wider than two,
    /// keyed by asset location. Read by the source reader in place of an
    /// implicit first-two-members fold. Absent means no authored placement.
    #[cfg_attr(feature = "cache", serde(default))]
    pub bundle_placements: Vec<BundlePlacement>,
    /// Current UVI native source/header and primitive profile. Pending geometry
    /// is resolved by the ordinary reader, never from physical Stereo packing.
    #[cfg_attr(feature = "cache", serde(default))]
    pub uvi_native_processing: Vec<UviNativeProcessingProfile>,
    /// Explicit actual output-field roles, never inferred from processor kind.
    #[cfg_attr(feature = "cache", serde(default))]
    pub output_controls:Vec<SourceOwnerOutputControl>,
    /// Executable modulation at a real Program/Layer output, supplied by the
    /// resolved element MIDI source domain. Never copied to member zones.
    #[cfg_attr(feature = "cache", serde(default))]
    pub global_modulation:Vec<SourceGlobalModulation>,
    /// Writable Constant definitions; property controls are not listener inputs.
    #[cfg_attr(feature = "cache", serde(default))]
    pub constant_properties:Vec<SourceConstantProperty>,
    /// Original physical order. Empty for caches without native identity metadata.
    #[cfg_attr(feature = "cache", serde(default))]
    pub zone_catalog: Vec<SourceZone>,
    /// Authored sample-player identities, independent of asset renumbering.
    /// An initially empty player retains complete dormant zone recipes.
    #[cfg_attr(feature = "cache", serde(default))]
    pub sample_targets: Vec<SourceSampleTarget>,
    /// SamplePlayer zones without program-owned PlaybackOptions. Initial source
    /// loading inherits file playback for these zones, never root/tune/gain.
    /// Presence is explicit: an empty stored PlaybackOptions suppresses inheritance.
    #[cfg_attr(feature = "cache", serde(default))]
    pub file_playback_zones: Vec<ZoneRef>,
    /// One logical six-channel source, stored as three ordinary stereo assets.
    /// Empty in legacy translations. These dependencies must survive narrowing;
    /// their presence never authorizes a consumer to fall back to the first pack.
    #[cfg_attr(feature = "cache", serde(default))]
    pub source_bundles: Vec<SourceBundle>,
    pub modulators: Vec<SourceModulator>,
    /// Original Kontakt AHDSR setter values; normalized curves cannot recover v1 bits.
    pub ahdsrs: Vec<SourceAhdsr>,
    /// Authored native mod/target names and physical lookup addresses, including omitted DSP.
    pub engine_lookups: Vec<SourceEngineLookup>,
    /// Authored normalized getter values before script initialization, at physical addresses.
    pub engine_values: Vec<SourceEngineValue>,
    /// Native addresses aliasing real continuous processor controls.
    pub control_aliases: Vec<SourceControlAlias>,
    /// Admitted outgoing route -> live depth and internal-source bypass controls.
    pub route_controls: Vec<(RouteRef, ControlRef, ControlRef)>,
    /// Intensity assignments beyond a route's first (`Route::scale`), in
    /// application order; each combination's input is the previous result.
    #[cfg_attr(feature = "cache", serde(default))]
    pub chained_scales: Vec<(RouteRef, RouteScale)>,
    /// Routes whose target law reads the source's unipolar view ((v + 1) / 2 of a
    /// bipolar source): FUN_140873040's `need_uni` targets outside its signed sets.
    #[cfg_attr(feature = "cache", serde(default))]
    pub unipolar_routes: Vec<RouteRef>,
    /// Native identity of Kontakt external routes with a proved source type.
    #[cfg_attr(feature = "cache", serde(default))]
    pub native_routes: Vec<(RouteRef, NativeRoute)>,
    pub slots: Vec<Option<usize>>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceZoneHome {
    pub group: u32,
    pub low_key: u8,
    pub high_key: u8,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceZone {
    pub id: Option<u32>,
    pub home: SourceZoneHome,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoneRouting {
    pub source_zone: u32,
    pub group: u32,
    pub low_key: u8,
    pub high_key: u8,
    pub variants: Vec<ZoneRouteVariant>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoneRouteVariant {
    pub group: u32,
    /// Unavailable destinations retain their identity and never become a fallback.
    pub zone: Option<ZoneRef>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSampleTarget {
    pub node: u32,
    pub zones: Vec<ZoneRef>,
}

/// A real source address in the importing instrument's document namespace.
/// Kontakt's physical group/zone indices are deliberately not UVI XML node IDs.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceOwnerId {
    UviNode(u32),
    KontaktInstrument,
    KontaktGroup(u32),
    KontaktZone(u32),
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOwnerKind {
    Instrument,
    Layer,
    Keygroup,
    Player,
    Aux,
    KontaktGroup,
    KontaktZone,
}

/// A retained source property. A binding is real only when it names a typed
/// control; the saved value alone does not pretend to implement live updates.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSwitch {
    pub saved: bool,
    pub control: Option<ControlRef>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceEventProcessorId {
    UviNode(u32),
    KontaktScriptSlot(u32),
}

/// EVERY direct authored event-list member, including unsupported processors.
/// `ordinal` counts the original full list, not only ScriptProcessor entries.
/// This row is source metadata, not a VM, callback token, or execution admission.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEventProcessor {
    pub id: SourceEventProcessorId,
    pub owner: SourceOwnerId,
    pub ordinal: u32,
    pub kind: String,
    pub bypass: SourceSwitch,
}

/// Known native input predicates. An unsupported predicate remains explicit;
/// its absence from an adapter cannot silently mean "accept every event".
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceInputPredicate {
    Keys(KeyRange),
    Velocities(VelocityRange),
    Channel(u8),
    Unsupported { field: String },
}

/// Source topology and semantic membership shared by the audio and event
/// producers. `children` is direct original order. `zones`/`groups` are actual
/// translated references within this owner's subtree, not guessed names.
/// The kind does NOT choose a DSP lifetime: UVI KG activation processing,
/// Kontakt per-voice groups and instrument buses compile their own proven law.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceOwner {
    pub id: SourceOwnerId,
    pub parent: Option<SourceOwnerId>,
    pub kind: SourceOwnerKind,
    pub children: Vec<SourceOwnerId>,
    pub groups: Vec<GroupRef>,
    pub zones: Vec<ZoneRef>,
    pub predicates: Vec<SourceInputPredicate>,
    pub bypass: Option<SourceSwitch>,
    pub mute: Option<SourceSwitch>,
    pub event_processors: Vec<SourceEventProcessor>,
}

/// Initial storage recipe for ONE source/zone, not three selectable voices.
/// Pack order is fixed: input channels (1,2), (3,4), (5,6). Geometry applies
/// identically to all packs; source pitch and Playback remain on the zone.
/// This semantic record does not assert native oscillator grouping or width.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceBundle {
    pub zone: ZoneRef,
    pub packs: [AssetRef; 3],
    pub geometry: SourceBundleGeometry,
    /// Original logical player/resource identities, independent of asset compaction.
    pub player: u32,
    pub logical_asset: u32,
    /// Immutable resource identity. No PCM, URI or decoder state is retained here.
    pub origin: [u8; 32],
    pub boundary: SourceMatrixBoundary,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceBundleGeometry {
    pub rate: u32,
    pub frames: u64,
}

/// Authored position of the first width-reducing matrix. Coordinates and control
/// identities remain explicit so deleting a prefix cannot renumber later slots.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceMatrixBoundary {
    pub node: u32,
    pub chain: ChainRef,
    pub section: SourceBoundarySection,
    pub index: usize,
    /// Twelve independent signed cells. Coordinates are zero-based; XML's
    /// Gain_i_j has input i and output j, not row-major XML field order.
    pub cells: [SourceMatrixCell; 12],
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceBoundarySection {
    BeforeAmplitude,
    AfterAmplitude,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceMatrixCell {
    pub input: u8,
    pub output: u8,
    pub coefficient: f64,
    pub control: Option<ControlRef>,
    pub ramp: Time,
}

/// What a Kontakt external route record says about its law: the native target
/// id (registry id of the serialized target name), the target flag byte and
/// the external source type (+0x544). Consumed by sampler-core
/// `voice_mod::laws`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeRoute {
    pub target: u16,
    pub flags: u8,
    pub source_type: u8,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceModulator {
    pub group: usize,
    pub slot: usize,
    pub external: bool,
    pub name: String,
    pub runtime: Option<ModulatorRef>,
    /// Original Kontakt settings, including bypassed and unmodeled targets.
    /// None for synthetic/non-Kontakt registries; never playback admission.
    pub settings: Option<kontakt::Modulation>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceAhdsr {
    pub group: usize,
    pub slot: usize,
    pub attack_ms: f32,
    pub attack_curve: f32,
    pub hold_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub ahd_only: bool,
    /// Exact pinned primary amplitude geometry, validated while the raw wrapper exists.
    pub native_amplitude: bool,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEngineLookup {
    pub group: i32,
    /// -1 for a modulator; physical modulator slot for a target.
    pub owner: i32,
    pub target: bool,
    pub name: String,
    /// Physical modulator slot or authored target ordinal, never a runtime DSP index.
    pub index: i32,
}

/// The native 0..1M knob maps linearly to the named control's Continuous range.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceControlAlias {
    pub control: ControlRef,
    pub parameter: String,
    pub address: SlotAddress,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEngineValue {
    pub parameter: u16,
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
    pub value: i32,
}

/// A version-pinned module record in the source format, identified by its
/// original group/rack/physical slot path (not an index into prepared chains).
#[derive(Clone, Debug, PartialEq)]
pub struct SourceParameterRecord {
    pub location: String,
    pub object_id: u16,
    pub version: u16,
    pub fields: Vec<SourceParameter>,
}

/// Names carry explicit units where established (`*_ms`, `*_hz`, `*_db`).
/// Unnamed storage fields stay `unknown_*`; they are never presented as
/// normalized engine controls or fabricated coefficients.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceParameter {
    pub name: &'static str,
    pub value: SourceParameterValue,
}

#[derive(Clone, PartialEq)]
pub enum SourceParameterValue {
    Number(f64),
    Integer(i64),
    Boolean(bool),
    Text(String),
    Numbers(Vec<f64>),
    Records(Vec<Vec<SourceParameter>>),
    /// Undecoded source bytes, retained in memory only. No playback meaning.
    Opaque(Vec<u8>),
}

impl std::fmt::Debug for SourceParameterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(v) => v.fmt(f),
            Self::Integer(v) => v.fmt(f),
            Self::Boolean(v) => v.fmt(f),
            Self::Text(v) => v.fmt(f),
            Self::Numbers(v) => v.fmt(f),
            Self::Records(v) => v.fmt(f),
            Self::Opaque(v) => write!(f, "<{} opaque bytes>", v.len()),
        }
    }
}

/// Instrument volume as a host parameter (Kontakt's CC7): it starts at the
/// saved value and, when `controller` is received, becomes `(cc/127)^3`,
/// replacing the saved value (measured, not multiplied in).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostVolume {
    pub controller: u8,
    /// Linear gain saved with the instrument, before any controller.
    pub saved: f64,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SourceFormat {
    #[default]
    Native,
    Kontakt {
        /// Serialized object version of the program records.
        version: u16,
    },
    Sfz,
    /// UVI Falcon / Workstation program XML.
    Uvi,
}

// ---------------------------------------------------------------- assets

/// Minimum storage required by an authored sample user. Shared immutable
/// sources must retain the strongest requirement of all their users.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetResidency {
    /// The source may use resident attacks with disk-streamed tails.
    #[default]
    Streamable,
    /// The complete base PCM must be resident before publishing the part.
    Resident,
}

impl AssetResidency {
    pub const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Resident, _) | (_, Self::Resident) => Self::Resident,
            _ => Self::Streamable,
        }
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub location: AssetLocation,
    pub encoding: Encoding,
    /// A source-level minimum, independent of the host's preferred cache mode.
    #[cfg_attr(feature = "cache", serde(default))]
    pub residency: AssetResidency,
    /// Recorded pitch stored with the audio, if any.
    pub root_key: Option<u8>,
    /// Loops stored with the audio (e.g. a WAV `smpl` chunk).
    pub loops: Vec<LoopRange>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetLocation {
    /// Path as authored, relative to the instrument file unless absolute.
    Path(String),
    /// Entry of a Kontakt file table, resolved by the container profile.
    KontaktFile { id: i32 },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Unknown,
    Wav,
    Aiff,
    Flac,
    Ogg,
    /// Native Instruments compressed wave.
    Ncw,
}

// ---------------------------------------------------------------- mapping

/// A Kontakt group or other source-level layer: shared settings for its zones.
/// SFZ `<group>`/`<master>` headers are inheritance, not scopes, so the SFZ
/// translator resolves them into zones and emits no groups.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// Saved mute preserves the complete definition for live unmute.
    #[cfg_attr(feature = "cache", serde(default))]
    pub muted: bool,
    /// Strictly admitted Kontakt cycle oscillator, independent of sample root/rate.
    #[cfg_attr(feature = "cache", serde(default))]
    pub wavetable: Option<Wavetable>,
    /// Independent time/pitch source (Kontakt Time Machine family). Absent for
    /// plain sampler playback, where transposition also changes tempo.
    #[cfg_attr(feature = "cache", serde(default))]
    pub time_stretch: Option<TimeStretch>,
    /// Kontakt machine source identified but not yet voiced (Beat Machine,
    /// S1200, MP60). Such zones play as a sampler; the stored controls are kept
    /// here so the engine can voice them without another load.
    #[cfg_attr(feature = "cache", serde(default))]
    pub machine: Option<Machine>,
    /// Beat Machine crossfade controls (stored mode 4); `None` for every other source.
    #[cfg_attr(feature = "cache", serde(default))]
    pub beat_slices: Option<BeatSlices>,
    pub name: String,
    pub start: Vec<GroupStart>,
    pub gain: Gain,
    pub pan: Pan,
    pub tune: Pitch,
    /// Group-scope chain; its processors see the sum of this group's voices.
    pub chain: Option<ChainRef>,
    pub output: Output,
    /// Index into [`Instrument::voice_limits`] shared by this group's voices.
    pub voice_limit: Option<usize>,
    /// Additional source resource domain (e.g. standard/HQ Time Machine Pro).
    /// Indexes Instrument::voice_limits independently of the voice group.
    #[cfg_attr(feature = "cache", serde(default))]
    pub source_voice_limit: Option<usize>,
    /// A release-trigger group where playing a note again cuts that note's
    /// release samples still sounding (Kontakt manual, Release Trigger
    /// "Monophonic").
    pub monophonic_release: bool,
    /// Extra routes of the group's sound to buses (aux sends), beside `output`.
    pub sends: Vec<GroupSend>,
    /// Set by [`Instrument::tap_group`]: the bus that carries this group's
    /// fader and sends.
    pub tap: Option<GroupTap>,
}

/// Kontakt machine source modules whose DSP the engine does not yet model.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    BeatMachine,
    /// `filter_mode` is the stored S1200 filter selector (engine parameter
    /// 0x318); valid values are `0..=S1200_FILTER_MODES`.
    S1200 { filter_mode: Option<u8> },
    Mp60,
}

/// Beat Machine slice controls of a stored mode-4 group. `attack_ms` and `release_ms`
/// are Kontakt's "Slice Atk" and "Slice Rel" (stored `slice_float_1`, `slice_float_2`,
/// milliseconds); `flag` is the stored `slice_flag_1`, whose UI meaning is not recovered.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatSlices {
    pub attack_ms: f32,
    pub release_ms: f32,
    pub flag: bool,
}

/// Most slice boundaries one Beat Machine table holds: eight slices, as in the native
/// `BSlice` table (`beat_slices::SLOTS`).
pub const BEAT_GRID_MARKERS: usize = 9;

/// Slice boundaries of one zone of a Beat Machine group, in source frames: slice `i` spans
/// `markers[i]..markers[i + 1]` (strictly increasing). `tempo_scale` is the zone's stored
/// playback rate (private float at byte 12; 0.25 for the captured fixed 1/16 grid).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatGrid {
    pub markers: [u32; BEAT_GRID_MARKERS],
    /// Number of used entries in `markers`; slices are `count - 1`.
    pub count: u8,
    pub tempo_scale: f32,
}

impl BeatGrid {
    /// `Ok(None)` below two markers (no slice), `Err` above the table capacity.
    pub fn from_markers(markers: &[u32], tempo_scale: f32) -> Result<Option<Self>, String> {
        if markers.len() < 2 {
            return Ok(None);
        }
        if markers.len() > BEAT_GRID_MARKERS {
            return Err(format!(
                "{} slice markers exceed the Beat Machine table ({} slices)",
                markers.len(),
                BEAT_GRID_MARKERS - 1
            ));
        }
        let mut grid = [0; BEAT_GRID_MARKERS];
        grid[..markers.len()].copy_from_slice(markers);
        Ok(Some(Self {
            markers: grid,
            count: markers.len() as u8,
            tempo_scale,
        }))
    }
}

/// Largest stored S1200 filter selector; `set_engine_par` rejects larger values.
pub const S1200_FILTER_MODE_MAX: u8 = 5;

/// Which time-stretch engine configuration a group's source uses. The engine
/// is shared (`sampler-core` `StretchEngine`); the kind only picks its
/// configuration, so formats add kinds without new DSP code paths.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StretchKind {
    TimeMachine1,
    TimeMachine2,
    TimeMachinePro,
    /// Tone Machine (stored mode 1). Shares the Time Machine 1 record fields.
    ToneMachine,
}

/// Saved controls of a time-stretch source. Pitch is the group/zone tuning
/// plus key tracking, exactly as for sampler playback; these controls only
/// decouple tempo from it.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeStretch {
    pub kind: StretchKind,
    /// Playback speed as a ratio of the original; 0 freezes the position.
    pub speed: f32,
    /// Time Machine 1: Smooth as stored (0..=1) and Grain in milliseconds.
    pub smooth: f32,
    pub grain_ms: f32,
    /// Time Machine 1 HQ analysis / Time Machine Pro resource class.
    pub high_quality: bool,
    /// Time Machine 2 controls as stored: Transient Copy, Transient Size (in
    /// source positions) and the analysis Grain profile index (0..=4).
    /// Carried for the TM2 engine; the stand-in engine does not use them.
    pub transient_copy: bool,
    pub transient_size: u32,
    pub grain_profile: u32,
    /// Time Machine Pro controls as stored (Keep Formants, Frm Shift, Env
    /// Order); carried, unused until a Pro backend exists.
    pub keep_formants: bool,
    pub formant_shift: f32,
    pub env_order: f32,
    /// Tempo-synced cycle length in beats (stored `timing_value`, set by the
    /// `SPEED_UNIT` write). `None` when the timing is free. The synced speed law
    /// itself is not modelled yet, so the engine ignores this.
    pub sync_beats: Option<f32>,
}
impl TimeStretch {
    /// Neutral controls (authored defaults are not established).
    pub fn new(kind: StretchKind, speed: f32) -> Self {
        Self {
            kind,
            speed,
            smooth: 0.5,
            grain_ms: 20.,
            high_quality: false,
            transient_copy: false,
            transient_size: 0,
            grain_profile: 0,
            keep_formants: false,
            formant_shift: 0.,
            env_order: 0.25,
            sync_beats: None,
        }
    }
    pub fn valid(self) -> bool {
        self.speed.is_finite() && (0. ..=64.).contains(&self.speed)
    }
}

/// Saved controls for the measured 2048-frame Kontakt wavetable subset.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wavetable {
    pub position: f32,
    pub phase: f32,
    pub form1: f32,
    pub form2: f32,
    /// Native phase-form IDs, independent of UI menu order.
    pub form1_type: u8,
    pub form2_type: u8,
}
impl Wavetable {
    /// Phase-only forms verified against Kontakt's 0x140567aa0 instructions.
    /// Amplitude forms and the unmeasured phase forms remain outside admission.
    pub const fn supports_form(kind: u8) -> bool {
        matches!(kind, 0..=7 | 8..=13 | 14..=17 | 24..=26 | 28..=33)
    }
    pub fn valid(self) -> bool {
        [self.position, self.phase, self.form1, self.form2]
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && Self::supports_form(self.form1_type)
            && Self::supports_form(self.form2_type)
    }
}

/// Physical row and the logical operator connecting it to the following row.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupStart {
    pub slot: u8,
    pub test: StartTest,
    pub next: StartJoin,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartJoin {
    And,
    AndNot,
    Or,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartTest {
    Key {
        low: u8,
        high: u8,
    },
    Controller {
        controller: u8,
        low: u8,
        high: u8,
    },
    /// One-based authored position in the instrument's native RR cycle.
    RoundRobin(u32),
    Random,
}

/// Where a group's fader lives once it is tapped: `bus` outputs at the bus's
/// `gain`, and so do its `sends` listed in `post` (the post-fader ones); the
/// rest leave the bus before the fader.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct GroupTap {
    pub bus: BusRef,
    pub post: Vec<usize>,
}

/// A group's send to a bus.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupSend {
    pub to: BusRef,
    pub gain: Gain,
    /// Taken before the group's own gain (fader), else after it.
    pub pre_fader: bool,
}

/// Past `voices` sounding voices, starting another fades one out over
/// `fade`: a released one first when `prefer_released`, else by `kill`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceLimit {
    pub voices: u32,
    pub kill: Kill,
    pub prefer_released: bool,
    pub fade: Time,
    /// Voice domains in the same set cut other domains in that set.
    #[cfg_attr(feature = "cache", serde(default))]
    pub exclusive_set: Option<u32>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kill {
    /// The quietest.
    Any,
    #[default]
    Oldest,
    Newest,
    /// Highest note.
    Highest,
    Lowest,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// None is an authored, dormant sample player, not a missing decoded file.
    /// Its complete mapping remains available for a future sample transaction.
    pub asset: Option<AssetRef>,
    pub group: Option<GroupRef>,
    pub keys: KeyRange,
    pub velocities: VelocityRange,
    pub conditions: Vec<ControllerRange>,
    pub trigger: Trigger,
    pub selection: Option<Selection>,
    pub articulation: Option<ArticulationRef>,
    /// Nested selector choices this zone sounds under, all of which must hold.
    pub axes: Vec<AxisPick>,
    pub pitch: KeyTracking,
    pub tune: Pitch,
    pub gain: Gain,
    /// How note velocity scales this zone's amplitude.
    pub velocity: VelocityResponse,
    /// Linear ramps at the edges of the key and velocity ranges.
    pub fades: Fades,
    pub pan: Pan,
    pub playback: Playback,
    /// Voice-scope processing for this zone, before group/bus processing.
    pub chain: Option<ChainRef>,
    /// Voice-scope amplitude envelope; `None` is a gate (instant on/off).
    pub amplitude: Option<ModulatorRef>,
    /// Modulation routes that act on this zone's voices, in authored order.
    pub routes: Vec<RouteRef>,
}

impl Instrument {
    /// Register physical compressor owners for editors, without claiming native
    /// normalized laws. Existing authored bindings keep their identities.
    pub fn register_compressor_controls(&mut self) {
        let existing: std::collections::BTreeSet<_> = self
            .processor_controls
            .iter()
            .map(|b| (b.chain.0, b.index, b.parameter as u8))
            .collect();
        for (chain, processors) in self.chains.iter().enumerate() {
            for (index, processor) in processors
                .pre_amplitude
                .iter()
                .chain(&processors.post_amplitude)
                .enumerate()
            {
                let Processor::Dynamics(c) = *processor else {
                    continue;
                };
                use fx::dynamics::Law;
                // These are physical editor controls, in the DSP law's units.
                // Native is not a domain: Feedback takes linear input drive and
                // ratio 1..20, unlike the original compressor's dB / 0..1 knobs.
                let (input_label, input_low, input_high, input_unit, ratio_low, ratio_high, attack_low, release_low) = match c.law {
                    Law::Generic(_) => ("Threshold", -60., 0., ControlUnit::Decibels, 1., 20., 0., 0.),
                    Law::KontaktClassic(_) | Law::KontaktPro(_) =>
                        ("Threshold", -68., -3., ControlUnit::Decibels, 0., 1., 0., 0.),
                    Law::KontaktEnhanced(_) =>
                        ("Threshold", -68., -3., ControlUnit::Decibels, 0., 1., 0.000_001, 0.),
                    Law::KontaktFeedback { .. } =>
                        ("Input gain", 10_f64.powf(-0.5), 100., ControlUnit::None, 1., 20., 0.000_1, 0.05),
                    Law::KontaktLimiter(_) =>
                        ("Input gain", -24., 24., ControlUnit::Decibels, 0., 0., 0., 0.),
                    // These cores either ignore the four generic knobs or use
                    // different controls. Do not invent a compressor editor.
                    _ => continue,
                };
                for (parameter, label, default, low, high, unit) in [
                    (
                        ProcessorParameter::Threshold,
                        input_label,
                        c.threshold(),
                        input_low,
                        input_high,
                        input_unit,
                    ),
                    (
                        ProcessorParameter::Ratio,
                        "Ratio",
                        c.ratio(),
                        ratio_low,
                        ratio_high,
                        ControlUnit::None,
                    ),
                    (
                        ProcessorParameter::Attack,
                        "Attack",
                        c.attack.seconds(),
                        attack_low,
                        1_f64,
                        ControlUnit::Seconds,
                    ),
                    (
                        ProcessorParameter::Release,
                        "Release",
                        c.release.seconds(),
                        release_low,
                        5_f64,
                        ControlUnit::Seconds,
                    ),
                ] {
                    if matches!(c.law, Law::KontaktLimiter(_))
                        && matches!(parameter, ProcessorParameter::Ratio | ProcessorParameter::Attack) {
                        continue;
                    }
                    if existing.contains(&(chain, index, parameter as u8)) {
                        continue;
                    }
                    let control = ControlRef(self.controls.len());
                    // Labels can become more precise without changing a saved
                    // editor control's identity (the historical key was Threshold).
                    let key_label = if parameter == ProcessorParameter::Threshold { "Threshold" } else { label };
                    self.controls.push(Control {
                        key: format!("compressor:{chain}:{index}:{key_label}"),
                        label: label.into(),
                        value: ControlValue::Continuous {
                            min: low.min(default),
                            max: high.max(default),
                            default,
                            unit,
                        },
                        automation: Automation::None,
                    });
                    self.processor_controls.push(ProcessorControl {
                        control,
                        chain: ChainRef(chain),
                        index,
                        parameter,
                        ramp: Time::Milliseconds(10.),
                    });
                }
            }
        }
    }

    /// The controllers that scale zone amplitude, most zones first (ties by
    /// number): the instrument's dynamics or volume sources, so a host can
    /// show "dynamics: CC1" and an expression layer can target it. Counts zones
    /// carrying a route from `Controller(cc)` to [`Target::Amplitude`] with a
    /// nonzero depth that raises the gain as the controller rises (sign or
    /// gain above unity, `invert`, and the shape's ends; a shape that rises and
    /// falls back reads as its ends): a controller that turns a zone down is
    /// not its dynamics.
    pub fn amplitude_controllers(&self) -> Vec<u8> {
        let mut zones = [0usize; 128];
        for zone in &self.zones {
            let mut seen = [false; 128];
            for route in zone.routes.iter().map(|r| &self.routes[r.0]) {
                let ModulationSource::Controller(cc) = self.modulators[route.source.0].source
                else {
                    continue;
                };
                let falls = route.shape.and_then(|s| self.shapes.get(s.0)).is_some_and(|s| {
                    matches!((s.points.first(), s.points.last()), (Some(a), Some(b)) if b.1 < a.1)
                });
                let live = match route.depth {
                    Depth::Normalized(d) => d != 0.0 && (d > 0.0) != (route.invert != falls),
                    // gain × g^v rises with the controller when g is above unity.
                    Depth::Gain(g) => g.linear() != 1.0 && (g.linear() > 1.0) != (route.invert != falls),
                    Depth::Pitch(_) => true,
                };
                if route.target == Target::Amplitude && live && cc < 128 {
                    seen[usize::from(cc)] = true;
                }
            }
            for (n, seen) in seen.iter().enumerate() {
                zones[n] += usize::from(*seen);
            }
        }
        let mut ccs: Vec<u8> = (0..128u8).filter(|&n| zones[usize::from(n)] > 0).collect();
        ccs.sort_by_key(|&n| std::cmp::Reverse(zones[usize::from(n)]));
        ccs
    }

    /// Make `bus` the tap point of group `index`: the group's fader moves onto
    /// the bus's output and its sends leave from the signal before it (pre-fader)
    /// or scaled by it (post-fader, see [`GroupTap`]). The group's voices then
    /// feed `bus` unscaled.
    pub fn tap_group(&mut self, index: usize, bus: BusRef) {
        let group = &mut self.groups[index];
        let target = &mut self.buses[bus.0];
        target.gain = group.gain;
        let mut post = Vec::new();
        for send in group.sends.drain(..) {
            if !send.pre_fader {
                post.push(target.sends.len());
            }
            target.sends.push(Send {
                to: Output::Bus(send.to),
                gain: send.gain,
                position: SendPosition::PostChain,
            });
        }
        group.tap = Some(GroupTap { bus, post });
        group.gain = Gain::UNITY;
    }

    /// Give every group that has sends a bus of its own to tap (what a host's
    /// mixer already does for every group), so lowering hears the same mix.
    pub fn with_group_taps(&self) -> Self {
        let mut routed = self.clone();
        for index in 0..routed.groups.len() {
            if routed.groups[index].sends.is_empty() {
                continue;
            }
            let group = &routed.groups[index];
            let bus = BusRef(routed.buses.len());
            routed.buses.push(Bus {
                name: group.name.clone(),
                chain: None,
                sends: Vec::new(),
                output: group.output,
                gain: Gain::UNITY,
            });
            routed.groups[index].output = Output::Bus(bus);
            routed.tap_group(index, bus);
        }
        routed
    }

    /// Keep only the zones `keep` accepts and the assets they use, renumbering
    /// assets in their original order. Returns the original index of each
    /// remaining asset, so a caller can load just those.
    pub fn retain_zones(&mut self, mut keep: impl FnMut(&Zone) -> bool) -> Vec<usize> {
        let mut remap = vec![None; self.zones.len()];
        let mut old = 0;
        let mut next = 0;
        self.zones.retain(|zone| {
            let retained = keep(zone);
            if retained {
                remap[old] = Some(ZoneRef(next));
                next += 1;
            }
            old += 1;
            retained
        });
        for scope in &mut self.processing_scopes {
            if let Some(signal)=&mut scope.signal {
                if let ProcessingSourceLaw::UviC997Layout{members}=&mut signal.source_law {
                    let old=*members;*members=[None;16];let mut output=0;
                    for (ordinal,zone)in scope.members.iter().enumerate(){
                        if remap.get(zone.0).copied().unwrap_or(Some(*zone)).is_some(){if output<16{members[output]=old.get(ordinal).copied().flatten();}output+=1;}
                    }
                }
            }
            scope.members = scope.members.iter().filter_map(|z| {
                // Preserve dangling public refs for validate(), never drop them.
                remap.get(z.0).copied().unwrap_or(Some(*z))
            }).collect();
        }
        let mut processing_remap = vec![None; self.processing_scopes.len()];
        let mut processing_old = 0;
        let mut processing_next = 0;
        self.processing_scopes.retain(|scope| {
            let keep = !scope.members.is_empty();
            if keep { processing_remap[processing_old] = Some(processing_next); processing_next += 1; }
            processing_old += 1;
            keep
        });
        self.source_indices.output_controls.retain_mut(|home| {
            if let SourceOwnerControlTarget::ProcessingScope{scope,..}=&mut home.target {
                match processing_remap.get(*scope).copied().unwrap_or(Some(*scope)) {
                    Some(next)=>*scope=next, None=>return false,
                }
            } true
        });
        for recipe in &mut self.source_indices.processing {
            recipe.members = recipe.members.iter().filter_map(|zone| {
                remap.get(zone.0).copied().unwrap_or(Some(*zone))
            }).collect();
            recipe.compiled = recipe.compiled.and_then(|old| {
                processing_remap.get(old).copied().unwrap_or(Some(old))
            });
        }
        self.source_indices.processing.retain(|recipe| !recipe.members.is_empty());
        self.source_indices.processing_parameters.retain_mut(|parameter| {
            if !self.source_indices.processing.iter().any(|recipe|recipe.owner==parameter.owner&&recipe.chain==Some(parameter.chain)){return false;}
            if let Some(compiled)=&mut parameter.compiled {
                match processing_remap.get(compiled.scope).copied().unwrap_or(Some(compiled.scope)) {
                    Some(next)=>compiled.scope=next,None=>parameter.compiled=None,
                }
            }true
        });
        for profile in &mut self.source_indices.uvi_native_processing {
            profile.members=profile.members.iter().filter_map(|(zone,input)|remap.get(zone.0).copied().unwrap_or(Some(*zone)).map(|zone|(zone,*input))).collect();
        }
        self.source_indices.uvi_native_processing.retain(|profile|!profile.members.is_empty());
        for matrix in &mut self.source_indices.processing_matrices {
            if let Some(binding)=&mut matrix.compiled {
                if let Some(scope)=processing_remap.get(binding.scope).copied().flatten() {
                    binding.scope=scope;
                } else { matrix.compiled=None; }
            }
        }
        // Keep every native owner and event-list identity after narrowing.
        // Only actual translated zone references change. Dangling public refs
        // remain visible for validate(), rather than becoming a missing owner.
        for owner in &mut self.source_indices.owners {
            owner.zones = owner.zones.iter().filter_map(|zone| {
                remap.get(zone.0).copied().unwrap_or(Some(*zone))
            }).collect();
        }
        for zone in &mut self.source_indices.zones {
            *zone = zone.and_then(|z| remap[z.0]);
        }
        for route in &mut self.zone_routing {
            for variant in &mut route.variants {
                variant.zone = variant.zone.and_then(|z| remap[z.0]);
            }
        }
        for target in &mut self.source_indices.sample_targets {
            target.zones = target.zones.iter().filter_map(|z| remap[z.0]).collect();
        }
        self.source_indices.sample_targets.retain(|t| !t.zones.is_empty());
        self.source_indices.file_playback_zones=self.source_indices.file_playback_zones.iter()
            .filter_map(|z|remap[z.0]).collect();
        // Malformed public recipes remain malformed for validate(); never turn
        // a dangling owner into a silently removed wide-source declaration.
        self.source_indices.source_bundles.retain_mut(|bundle| {
            match remap.get(bundle.zone.0) {
                Some(Some(zone)) => { bundle.zone = *zone; true }
                Some(None) => false,
                None => true,
            }
        });
        let mut used = vec![false; self.assets.len()];
        for zone in &self.zones {
            if let Some(used) = zone.asset.and_then(|a| used.get_mut(a.0)) {
                *used = true;
            }
        }
        for bundle in &self.source_indices.source_bundles {
            for asset in bundle.packs {
                if let Some(used) = used.get_mut(asset.0) {
                    *used = true;
                }
            }
        }
        let kept: Vec<usize> = (0..self.assets.len()).filter(|&i| used[i]).collect();
        let mut renumbered = vec![0; self.assets.len()];
        for (new, &old) in kept.iter().enumerate() {
            renumbered[old] = new;
        }
        let mut index = 0;
        self.assets.retain(|_| {
            index += 1;
            used[index - 1]
        });
        for zone in &mut self.zones {
            zone.asset = zone.asset.map(|asset| AssetRef(
                renumbered
                    .get(asset.0)
                    .copied()
                    .unwrap_or(asset.0),
            ));
        }
        for bundle in &mut self.source_indices.source_bundles {
            for asset in &mut bundle.packs {
                // Every valid dependency was marked above; dangling refs stay
                // outside the compacted table and fail structural validation.
                if let Some(index) = renumbered.get(asset.0) {
                    *asset = AssetRef(*index);
                }
            }
        }
        kept
    }
}

impl Zone {
    /// An authored mapping with no sample bound yet. Requires an explicit
    /// source sample-target identity before structural validation/lowering.
    pub fn dormant() -> Self {
        Self { asset: None, ..Self::new(AssetRef(0)) }
    }
    /// Full key and velocity range, root key 60, no processing.
    pub fn new(asset: AssetRef) -> Self {
        Self {
            asset: Some(asset),
            group: None,
            keys: KeyRange::FULL,
            velocities: VelocityRange::FULL,
            conditions: Vec::new(),
            trigger: Trigger::Attack,
            selection: None,
            articulation: None,
            axes: Vec::new(),
            pitch: KeyTracking::Tracked { root: 60 },
            tune: Pitch::NONE,
            gain: Gain::UNITY,
            velocity: VelocityResponse::Linear,
            fades: Fades::default(),
            pan: Pan::CENTER,
            playback: Playback::default(),
            chain: None,
            amplitude: None,
            routes: Vec::new(),
        }
    }
}

/// Velocity-to-amplitude response; selection always uses the raw velocity.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VelocityResponse {
    /// Velocity does not change amplitude.
    None,
    /// Amplitude is velocity / 127.
    #[default]
    Linear,
    /// Kontakt's velocity-to-volume route at the default intensity (50 of 127, the Beat Machine
    /// route). Amplitude follows Kontakt's measured relative-dB table (`sampler-core` prepare).
    VolumeTable,
    /// Amplitude is (velocity / 127) ^ exponent.
    Power(f64),
}

/// Zone crossfades, in key and velocity steps inside the zone's own ranges.
/// A fade-in of `F` over a low edge `L` has gain `(v - L + 1) / (F + 1)` for
/// `L <= v <= L + F`; a fade-out over a high edge `H` mirrors it,
/// `(H - v + 1) / (F + 1)`. Zero is no fade. The key and velocity gains
/// multiply.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fades {
    pub velocity_in: u8,
    pub velocity_out: u8,
    pub key_in: u8,
    pub key_out: u8,
}

/// Inclusive MIDI key range.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRange {
    pub low: u8,
    pub high: u8,
}

impl KeyRange {
    pub const FULL: Self = Self { low: 0, high: 127 };
}

/// Inclusive MIDI 1.0 velocity range.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VelocityRange {
    pub low: u8,
    pub high: u8,
}

impl VelocityRange {
    pub const FULL: Self = Self { low: 1, high: 127 };
}

/// Plays only while a 7-bit controller is within an inclusive range.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerRange {
    pub controller: u8,
    pub low: u8,
    pub high: u8,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    #[default]
    Attack,
    /// On key-up, regardless of sustain pedal.
    KeyRelease,
    /// When the note's gate closes: key-up, or pedal-up for a sustained key.
    GateRelease,
    /// Only when no other key is held.
    First,
    /// Only when another key is already held.
    Legato,
    /// A recorded transition: only when this key minus the most recent
    /// still-held key is `low..=high` semitones (negative: stepping down), so a
    /// legato instrument can place one sample per interval.
    Transition { low: i8, high: i8 },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyTracking {
    /// Pitch follows the played key relative to `root`, 100 cents per key.
    Tracked { root: u8 },
    /// Pitch follows the key by `cents_per_key` relative to `root`.
    Scaled { root: u8, cents_per_key: i32 },
    /// Every key plays the recorded pitch.
    Fixed,
}

/// The zone's place in a sequence of alternatives.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub sequence: SequenceRef,
    pub take: Take,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Take {
    /// Zero-based position in a sequential or uniform-random sequence.
    Index(u32),
    /// Plays when a uniform draw in `[low, high)` lands here (SFZ lorand/hirand).
    Probability { low: f64, high: f64 },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub policy: SequencePolicy,
    /// Number of alternatives; `Take::Index` values are below this.
    pub takes: u32,
    /// What owns the position counter.
    pub counter: CounterScope,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequencePolicy {
    RoundRobin,
    Random,
    RandomNoRepeat,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterScope {
    Instrument,
    Key,
    Channel,
    ChannelKey,
}

/// One independent selector: exactly one choice is active, switched by the
/// choices' keys, and it keeps its choice while other selectors change. A
/// zone lists the choices it sounds under ([`Zone::axes`]), so a tree (outer
/// articulation, inner variant) is a zone naming one choice per level.
/// Choice 0 is active before any switch is played. At most four axes.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    pub name: String,
    pub choices: Vec<AxisChoice>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisChoice {
    pub name: String,
    /// Keys that select this choice.
    pub switch_keys: Vec<u8>,
}

/// A zone's requirement that `axes[axis]` has `choice` active.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisPick {
    pub axis: usize,
    pub choice: usize,
}

/// A selectable articulation, switched by keys and/or an alternative driver.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Articulation {
    /// Stable source axis/group/control identity, independent of display order.
    pub source: String,
    /// Optional authored selection control (stable engine control ID).
    pub control: Option<u128>,
    pub name: String,
    /// The source's keyswitch keys. Under [`SwitchOwner::Behavior`] a behavior
    /// reads them, and the first is the key a driver taps to select this.
    pub switch_keys: Vec<u8>,
    /// Active before any switch is played.
    pub default: bool,
    /// What selects it when [`Switching::driver`] is not [`Driver::Keys`].
    pub alternatives: Alternatives,
}

/// Non-key inputs that select one articulation. Only the family named by
/// [`Switching::driver`] is live; the others are kept for switching modes.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Alternatives {
    /// Note-on velocities that select it, then play the note.
    pub velocities: Option<VelocityRange>,
    /// Zero-based MIDI channel whose notes select it, then play.
    pub channel: Option<u8>,
    /// A controller value range that selects it.
    pub controller: Option<ControllerRange>,
    pub program: Option<u8>,
}

/// How articulations are selected: who interprets a switch and which input
/// drives it.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Switching {
    pub owner: SwitchOwner,
    pub driver: Driver,
    /// What played switch keys do while another driver is active.
    pub keys: SwitchKeys,
}

impl Switching {
    /// One byte for a saved part: owner, driver and key policy. A remap
    /// survives reload exactly through `from_bits(to_bits())`.
    pub fn to_bits(self) -> u8 {
        self.owner as u8 | (self.driver as u8) << 1 | (self.keys as u8) << 4
    }

    /// `None` for a byte `to_bits` never produces.
    pub fn from_bits(bits: u8) -> Option<Self> {
        Some(Self {
            owner: [SwitchOwner::Native, SwitchOwner::Behavior]
                .get(usize::from(bits & 1))
                .copied()?,
            driver: [
                Driver::Keys,
                Driver::Velocity,
                Driver::Channel,
                Driver::Controller,
                Driver::Program,
            ]
            .get(usize::from(bits >> 1 & 7))
            .copied()?,
            keys: [SwitchKeys::Keep, SwitchKeys::Play, SwitchKeys::Swallow]
                .get(usize::from(bits >> 4))
                .copied()?,
        })
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchOwner {
    /// The runtime holds the articulation; zones name the one they belong to.
    #[default]
    Native,
    /// A behavior reads the switch keys and selects groups itself; drivers
    /// select by tapping an articulation's first switch key into it.
    Behavior,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Driver {
    #[default]
    Keys,
    Velocity,
    Channel,
    Controller,
    Program,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchKeys {
    /// Still switch, alongside the driver.
    #[default]
    Keep,
    /// No longer switch; they play notes. Native ownership only.
    Play,
    /// Dropped before anything sees them.
    Swallow,
}

impl Instrument {
    /// Give every articulation one value of each alternative family, in order
    /// of its lowest switch key (articulations without keys last, in list
    /// order): controller `controller` values 0.., channels 0.., programs 0..
    /// and equal velocity splits of 1..=127. Families an articulation count
    /// cannot fit (more than 16 channels, 127 velocities, 128 values) stay unset.
    pub fn assign_alternatives(&mut self, controller: u8) {
        let mut order: Vec<usize> = (0..self.articulations.len()).collect();
        order.sort_by_key(|&i| {
            let keys = &self.articulations[i].switch_keys;
            (keys.iter().min().copied().unwrap_or(u8::MAX), i)
        });
        let n = order.len();
        for (rank, &i) in order.iter().enumerate() {
            let value = u8::try_from(rank).ok().filter(|&v| v < 128);
            self.articulations[i].alternatives = Alternatives {
                velocities: (n <= 127).then(|| VelocityRange {
                    low: (1 + rank * 127 / n) as u8,
                    high: ((rank + 1) * 127 / n) as u8,
                }),
                channel: value.filter(|_| n <= 16),
                controller: value.map(|v| ControllerRange {
                    controller,
                    low: v,
                    high: v,
                }),
                program: value,
            };
        }
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Playback {
    pub start: SourceFrames,
    /// Exclusive end; `None` plays to the end of the audio.
    pub end: Option<SourceFrames>,
    pub reverse: bool,
    pub looping: Looping,
    /// Furthest frame past `start` a [`Target::SampleStart`] route can move
    /// the start to (Kontakt's zone sample-start modulation range).
    pub start_range: SourceFrames,
    /// Saved crossfade before file-boundary clamping, used by constrained loop modulation.
    #[cfg_attr(feature = "cache", serde(default))]
    pub loop_control_crossfade: Option<Span>,
    /// Beat Machine slice boundaries of this zone; `None` for a zone played as a sampler.
    #[cfg_attr(feature = "cache", serde(default))]
    pub beat_grid: Option<BeatGrid>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Looping {
    #[default]
    None,
    /// Ignores note-off; plays to the end (SFZ `one_shot`).
    OneShot,
    Continuous(LoopRange),
    /// Loops while the note is held, then plays past the loop end.
    UntilRelease(LoopRange),
    /// Eight authored physical slots, including unoccupied holes.
    Slots([Option<LoopSlot>; 8]),
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopRange {
    pub start: SourceFrames,
    /// Exclusive end.
    pub end: SourceFrames,
    pub crossfade: Span,
    pub alternating: bool,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopSlot {
    pub range: LoopRange,
    /// Zero repeats indefinitely; positive values are total outward passes.
    pub count: u32,
    /// Source-rate multiplier on repeated passes.
    pub tuning: f64,
    pub until_release: bool,
}

// ---------------------------------------------------------------- modulation

/// Where state lives and what signal a processor or modulator sees.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// One instance per playing voice.
    Voice,
    /// One instance per group, over the sum of its voices.
    Group(GroupRef),
    Bus(BusRef),
    /// The instrument output; for a modulator, one instance for the whole
    /// instrument (an LFO only; see [`Lfo::retrigger`]).
    Master,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Modulator {
    pub scope: Scope,
    pub source: ModulationSource,
}

/// What a modulator reads. Unipolar sources produce 0..=1, bipolar ones -1..=1:
/// envelopes, controllers, velocity, key, pressure, timbre, random and constant
/// are unipolar; LFOs and pitch bend are bipolar.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub enum ModulationSource {
    Envelope(Envelope),
    /// A multi-segment envelope (Kontakt flex), gated like `Envelope`.
    Breakpoints(Breakpoints),
    /// Kontakt's adaptive envelope follower, stored fields in file order.
    /// Follows the group audio; output is 0..=1.
    Follower(Follower),
    /// Kontakt's glide: a pitch ramp from the previous note, octave-normalized
    /// (see `GlideRamp::kontakt_glide`). Stored `BParGlide` mode and time.
    Glide(Glide),
    Lfo(Lfo),
    Controller(u8),
    Velocity,
    /// Note number / 127.
    Key,
    PitchBend,
    ChannelPressure,
    PolyPressure,
    /// Per-note timbre: MPE CC74 or the MIDI 2.0 per-note brightness.
    Timbre,
    /// A uniform value drawn once per voice.
    Random,
    /// Always 1.
    Constant,
    /// Kontakt's release-trigger counter: the share of `T` left when the key
    /// was released, `clamp((T − held) / T, 0, 1)`, where `held` runs from
    /// note-on to key-up (to now while the key is down).
    ReleaseCounter(Time),
    /// A script-set per-event value, `id` as KSP's "from script" modulator
    /// index: `set_event_par_arr(event, $EVENT_PAR_MOD_VALUE_ID, v, id)`,
    /// read as `clamp(v / 1_000_000, -1, 1)`; 0 until set.
    Script(u16),
    /// The same script value for a bipolar source, kept in -1..=1.
    ScriptBipolar(u16),
    /// Declared-Core target reader, never native smoothing or a listener value.
    ConstantProperty { node:u32, value:ControlRef, style:ControlRef, bipolar:bool },
}

impl ModulationSource {
    /// Whether values span -1..=1 rather than 0..=1.
    pub fn bipolar(&self) -> bool {
        matches!(self, Self::Lfo(_) | Self::PitchBend | Self::ScriptBipolar(_) | Self::ConstantProperty{bipolar:true,..})
    }
}

/// Stored glide fields: mode 1 scales time per semitone, 2 keeps it constant.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Glide {
    pub mode: u32,
    pub time_ms: f32,
}

/// Stored envelope-follower fields: attack, decay, gain and adaption speed.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Follower {
    pub attack: f32,
    pub decay: f32,
    pub gain: f32,
    pub adaption: f32,
}

/// Glides from 0 through `points`, each reached `time` after the previous;
/// holds at `points[sustain]` while gated; a release glides from the current
/// level through the points after `sustain` (jumping there when not yet
/// reached) and the last level holds.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakpoints {
    pub points: Vec<Breakpoint>,
    pub sustain: Option<usize>,
    /// Inclusive (start, end) point indices of a gated loop: reaching `end`
    /// returns to `start` instead of holding (Kontakt's Flex loop jump at the
    /// sustain row, native advance 0x140ae57dd).
    pub loop_range: Option<(usize, usize)>,
    /// Kontakt Flex release law (transition 0x140ae5450): the first release
    /// segment's duration is scaled by how far the current level has already
    /// travelled, instead of running its full length from wherever it starts.
    pub flex_release: bool,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breakpoint {
    pub time: Time,
    /// 0.0..=1.0 of full scale.
    pub level: f64,
    pub shape: Curve,
}

/// Delay-attack-hold-decay-sustain-release.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub delay: Time,
    pub attack: Time,
    pub hold: Time,
    pub decay: Time,
    /// Level reached after decay, 0.0..=1.0 of full scale.
    pub sustain: f64,
    pub release: Time,
    pub attack_shape: Curve,
    pub decay_shape: Curve,
    pub release_shape: Curve,
    /// Attack-hold-decay only: decays to zero, ignores note-off, then ends
    /// (`sustain` and `release` are unused).
    pub one_shot: bool,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            delay: Time::ZERO,
            attack: Time::ZERO,
            hold: Time::ZERO,
            decay: Time::ZERO,
            sustain: 1.0,
            release: Time::ZERO,
            attack_shape: Curve::Linear,
            decay_shape: Curve::Linear,
            release_shape: Curve::Linear,
            one_shot: false,
        }
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Curve {
    #[default]
    Linear,
    /// expm1(k·t)/expm1(k); positive starts slowly.
    Exponential(f64),
    /// Holds the starting level until the stage ends, then steps.
    Step,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: LfoShape,
    /// Cycles per second, or the cycle length in beats when tempo-synced.
    pub rate: Frequency,
    /// Silent time after the note starts.
    pub delay: Time,
    /// Linear depth ramp after the delay.
    pub fade_in: Time,
    /// Cycle position at the start, 0..1.
    pub phase: f64,
    /// Each voice starts its own cycle at `phase`; otherwise one free-running
    /// cycle, at `phase` when the instrument starts, is shared by all voices.
    /// A [`Scope::Master`] retriggered LFO is one cycle shared by all voices
    /// and restarted at `phase` (with its delay and fade) by every voice start.
    pub retrigger: bool,
}

/// Kontakt's native LFO waveforms (BLfo* processors, `modulators.md`): the
/// recovered 4096-entry tables with four-point interpolation, the width-driven
/// rectangle, and the seeded held-noise grid. Outputs are raw (they ring past
/// +-1); nothing here is an ideal waveform.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeWave {
    Sine,
    Saw,
    Triangle,
    /// Pulse width in 0..1.
    Rectangle(f32),
    /// Held noise updated twice per cycle; start phase must be below 0.5.
    Held,
    /// Kontakt Multi (`digital` false, BLfoMulti 0x140b07290) or Digital Multi
    /// (`digital` true, BLfoMultiDigital 0x140b078d0): a signed five-component
    /// mix. `weights` are in native runtime order: sine, doubled saw, triangle,
    /// pulse, held noise. `width` is the pulse width in 0..1.
    Multi {
        weights: [f32; 5],
        width: f32,
        normalize: bool,
        digital: bool,
    },
    /// A UVI Workstation LFO waveform (`WaveFormType` 0..=6, see [`UviWave`]).
    Uvi(UviWave),
}

/// UVI Workstation 4.0.9 LFO waveforms with a closed-form table or recurrence
/// (`WaveFormType` ids; registry 0x1410f5d00, process 0x1410f8ca0). The chaos
/// ids 7 and 8 are stateful Euler maps with unproved defaults, and id 9 is a
/// stored user table; neither is representable here.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UviWave {
    /// 0: 256-entry sine table, linear interpolation (table 0x1428393a0).
    Sinus,
    /// 1: +1 for entries 0..128, -1 from 128 (table 0x1428397b0).
    Square,
    /// 2: 0, +1, 0, -1, 0 at quarter cycles (table 0x142839bc0).
    Triangle,
    /// 3: -1 to +1 over entries 0..255 (table 0x142839fd0).
    RampUp,
    /// 4: +1 to -1 over entries 0..255 (table 0x14283a3e0).
    RampDown,
    /// 5: odd harmonics 1..17 (table 0x14283a7f0).
    AnalogSquare,
    /// 6: a new value from the native congruential generator at each wrap.
    RandomHold,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoShape {
    /// A native Kontakt waveform.
    Native(NativeWave),
    /// Saved retriggered Digital Multi sine, with an unsynchronized false-mode fade.
    /// `fade_ms` retains the original serialized milliseconds, not a linear ramp.
    DigitalSine { level: f64, fade_ms: f32 },
    /// Sine with a signed bipolar peak in -1..=1; phase still starts at zero.
    SineScaled(f64),
    /// A zero-weight Multi remains a bipolar source, with output 0.
    Zero,
    /// Starts at 0 rising.
    Sine,
    /// Starts at 0 rising.
    Triangle,
    /// +1 for the first half cycle.
    Square,
    /// +1 for the first `width` (0..=1) of the cycle, then -1.
    Rectangle(f64),
    /// Weighted mix of sine, rectangle(`width`), triangle, saw-up and
    /// sample-and-hold, divided by max(1, sum(abs(weights))) when `normalize`.
    Multi { weights: [f64; 5], width: f64, normalize: bool },
    /// -1 to +1.
    SawUp,
    /// +1 to -1.
    SawDown,
    /// A new uniform value each cycle, held.
    SampleAndHold,
    /// A new uniform value each cycle, reached linearly by the cycle's end.
    Random,
    /// A native waveform under Kontakt's saved LFO fade-in (`fade_ms` > 0,
    /// unsynchronized): the BLfo fade outside the waveform kernels (Multi
    /// 0x140b07290 and Digital Multi 0x140b078d0 carry none), the law of
    /// `sampler_core::KontaktLfoFade`. Replaces the linear `Lfo::fade_in`.
    NativeFaded { wave: NativeWave, fade_ms: f32 },
}

/// A piecewise-linear transfer curve over 0..=1, as ascending `(input, output)`
/// points. Bipolar values are mapped through `(v + 1) / 2` and back.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub points: Vec<(f64, f64)>,
}

/// Modulator output scaled into a target. The source value `v` passes through
/// `invert` (unipolar `1 - v`, bipolar `-v`), then `shape`, then `smoothing`
/// (a one-pole lag reaching 99% in that time). The target law:
///
/// | Target | Depth | Effect |
/// | --- | --- | --- |
/// | Amplitude | `Normalized(i)` | gain × (1 − i·(1 − u)), u the unipolar view of v |
/// | Amplitude | `Gain(g)` | gain × g^v |
/// | Pitch | `Pitch(p)` | + p·v |
/// | Pan | `Normalized(d)` | + d·v (pan in −1..=1) |
/// | Processor cutoff | `Pitch(p)` | cutoff × 2^(p·v/12) |
/// | Processor resonance | `Gain(g)` | Q × g^v |
/// | SampleStart | `Normalized(d)` | start + d·u·`Playback::start_range`, at note start |
///
/// The unipolar view of a bipolar value is (v + 1) / 2.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    pub source: ModulatorRef,
    pub target: Target,
    pub depth: Depth,
    pub invert: bool,
    pub shape: Option<ShapeRef>,
    pub smoothing: Time,
    /// Depth multiplier read from a second modulator (a modulator × modulator
    /// product, e.g. LFO depth by the mod wheel). See [`RouteScale`].
    pub scale: Option<RouteScale>,
}

/// The route's depth is multiplied by `shape(x)`, where `x` is the unipolar
/// view of `source` ((v + 1) / 2 for bipolar sources) and no shape means `x`.
/// The shape's output is used as-is, not mapped back to the source's range.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteScale {
    pub source: ModulatorRef,
    pub shape: Option<ShapeRef>,
    /// How `source` combines with the route's saved depth.
    #[cfg_attr(feature = "cache", serde(default))]
    pub law: ScaleLaw,
    /// Lag of the assignment's processed source, milliseconds to 99% of a step.
    #[cfg_attr(feature = "cache", serde(default))]
    pub lag_ms: f64,
}

/// Combination law of a [`RouteScale`].
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ScaleLaw {
    /// depth × shape(x).
    #[default]
    Multiply,
    /// Kontakt modulation of a modulator's intensity (native law ID25, see
    /// sampler-core `ModScaleLaw::KontaktIntensity`): `depth` is the scaling
    /// assignment's intensity, `flags` its stored target flags, `unit` the
    /// route's depth per unit of its saved normalized intensity.
    KontaktIntensity { depth: f64, flags: u8, unit: f64 },
}

impl Route {
    pub fn new(source: ModulatorRef, target: Target, depth: Depth) -> Self {
        Self {
            source,
            target,
            depth,
            invert: false,
            shape: None,
            smoothing: Time::ZERO,
            scale: None,
        }
    }
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Amplitude,
    Pitch,
    Pan,
    /// Playback start offset, applied when a voice starts.
    SampleStart,
    /// A processor parameter, addressed by chain and position.
    Processor {
        chain: ChainRef,
        index: usize,
        parameter: ProcessorParameter,
    },
    Control(ControlRef),
    /// Absolute source cycle position, sampled at note onset, replacing its saved phase.
    SourcePhase(ModulatorRef),
    /// Rate of a program-local LFO, added to its saved rate as `depth × source`
    /// on Kontakt's normalized frequency lane (law ID26, see sampler-core
    /// `CompiledTarget::SourceRate`). Depth is `Normalized`.
    SourceRate(ModulatorRef),
    /// Per-voice normalized wavetable source lane, added to the saved/live base.
    Wavetable(WavetableParameter),
    /// Truncated fractions of total sample frames, on a constrained forward loop.
    LoopStart,
    LoopLength,
    /// A stage time of an AHDSR envelope, addressed as a modulation target.
    /// The destination is the envelope modulator; the depth is the native
    /// intensity of the route, and the law multiplies the stage's normalized
    /// knob (see `sampler-core`'s `voice_mod::laws`).
    EnvelopeStage {
        source: ModulatorRef,
        stage: EnvelopeStage,
    },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavetableParameter {
    Position,
    Form1,
    Form2,
}

/// The stage of an AHDSR envelope a modulation target addresses (Kontakt
/// ids 184..187: `ahdsr_attack`, `ahdsr_hold`, `ahdsr_decay`,
/// `ahdsr_release`).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeStage {
    Attack,
    Hold,
    Decay,
    Release,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessorParameter {
    Response,
    Width,
    Pan,
    Cutoff,
    Resonance,
    Gain,
    Threshold,
    Ratio,
    Attack,
    Release,
    /// Hertz, on a Filter (live: scripted insert effects).
    Frequency,
    Quality,
    /// Octaves: a UVI DigitalEq band's width (band pass, notch, peak).
    Bandwidth,
    /// Decibels: a Filter's boost or a Gain's level.
    GainDecibels,
    /// Decibels subtracted from `GainDecibels` (a relative shelf).
    ReferenceDecibels,
    /// Nonzero lets the stage act; below 0.5 it passes the signal.
    Enabled,
    /// The cells of a StereoMatrix: output (row) by input (column), L or R.
    CellLL,
    CellLR,
    CellRL,
    CellRR,
    /// Normalized knobs of a script-addressed Kontakt effect module (native
    /// engine-parameter lanes): Classic Saturation shape, Distortion drive and
    /// damping, Formant I talk/sharp/size, Lo-Fi bits/frequency/noise/color.
    Shape,
    Drive,
    Damping,
    /// Kontakt Skreamer's normalized tone, bass, brightness and internal mix.
    Tone,
    Bass,
    Bright,
    Mix,
    FormantTalk,
    FormantSharp,
    FormantSize,
    LoFiBits,
    LoFiFrequency,
    LoFiNoise,
    LoFiColor,
    /// Kontakt Fuzz amount, bass and treble (live lanes of a script-addressed Fuzz).
    FuzzAmount,
    FuzzBass,
    FuzzTreble,
    /// Stored knobs of script-addressed Kontakt delay, modulation and reverb
    /// units (the family's `set` applies the native setter).
    Time,
    Feedback,
    Depth,
    Speed,
    Phase,
    Size,
    Colour,
    Predelay,
    /// Kontakt Freak's live knobs (one role each; core indices in `fx::color`).
    FreakAntifold,
    FreakFrequency,
    FreakType,
    FreakHarmonics,
    FreakContour,
    FreakRelease,
    FreakBpFreq,
    FreakStereo,
    FreakFeedback,
    FreakCarrier,
    FreakTuning,
    FreakWidth,
    FreakMix,
    /// Kontakt Bite's continuous live knobs (`fx::color::BITE_LIVE`).
    BiteSaturate,
    BitePrefilter,
    BiteFrequency,
    BiteJitter,
    BiteCrunch,
    BiteBits,
    BiteDither,
    BiteExpand,
    BitePostfilter,
    BiteMix,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    Gain(Gain),
    Pitch(Pitch),
    /// Fraction of the target's full range.
    Normalized(f64),
}

// ---------------------------------------------------------------- DSP

/// One actual XML ConstantModulation definition, independent of destinations.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct SourceConstantProperty {
    pub node:u32,pub owner:SourceOwnerId,pub value:ControlRef,pub style:ControlRef,pub bipolar:bool,
    #[cfg_attr(feature = "cache", serde(default))]
    pub law:SourceConstantPropertyLaw,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum SourceConstantPropertyLaw {
    #[default] UnqualifiedNative,
    /// Explicit caller-declared Core target reader, retaining Core route cadence.
    DeclaredCoreTargetRead,
}

/// One actual global destination and its resolved accepted-element input.
/// Identities are source/plan metadata; the Runtime must bind the real receiver
/// capability before any such outlet can execute. A destination owner is not
/// itself permission to invent a MIDI channel or per-note input context.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct SourceGlobalModulation {
    pub owner:SourceOwnerId,
    pub input_owner:SourceOwnerId,
    pub outlet:BusRef,
    pub sources:Vec<ModulatorRef>,
    pub routes:Vec<RouteRef>,
}

/// A native source-owner recipe, resolved on the ordinary loader after actual
/// SourceStorage geometry is available. This is not a second render engine.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProcessingRecipe {
    /// Authored display label only; all joins use owner/member identities.
    #[cfg_attr(feature = "cache", serde(default))]
    pub name: String,
    pub owner: SourceOwnerId,
    pub lifetime: SourceProcessingLifetime,
    /// Authored order, including saved-bypass and dormant source definitions.
    pub members: Vec<ZoneRef>,
    pub chain: Option<ChainRef>,
    pub amplitude: Option<ModulatorRef>,
    pub output: Option<BusRef>,
    /// Actual materialized ProcessingScope index; None is untranslated geometry.
    /// This index is never a source/native identity or runtime liveness token.
    pub compiled: Option<usize>,
    /// Scalar roles retain original chain coordinates until actual source layout is known.
    #[cfg_attr(feature = "cache", serde(default))]
    pub parameter_roles: Vec<SourceScopeControlRole>,
    /// Real original sources owned by this activation, including sources whose
    /// only outgoing consumer is a retained physical player. No dummy route.
    #[cfg_attr(feature = "cache", serde(default))]
    pub sources: Vec<ModulatorRef>,
    /// One program per actual activation, never one copy per physical member.
    #[cfg_attr(feature = "cache", serde(default))]
    pub routes: Vec<RouteRef>,
    /// Only an actual native plain amplitude connection grants source lifetime.
    #[cfg_attr(feature = "cache", serde(default))]
    pub source_gate:Option<SourceScopeGate>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct SourceScopeGate {pub source:ModulatorRef,pub route:RouteRef,pub velocity:VelocityResponse}

/// A separately named projection law. Historical UVI import behavior is not
/// evidence of the current native Frequency connection/cadence implementation.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceProcessingParameterLaw {
    /// Existing ordered per-route midpoint semitone sum -> exp2(sum/12).
    LegacyCutoffSemitoneMidpoint,
    /// Historical Freq connection: depth carries 12*log2(1000)*Ratio.
    /// Native dynamic attachment remains separately unqualified.
    HistoricalUviFrequencyThreeDecadeCore,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProcessingParameter {
    pub owner: SourceOwnerId,
    /// Original actual source processor node, not a VM or runtime slot.
    pub source_node: u32,
    pub chain: ChainRef,
    pub index: usize,
    pub parameter: ProcessorParameter,
    pub control: ControlRef,
    pub law: SourceProcessingParameterLaw,
    /// Ordered accepted fixed-source multipliers, each followed by the
    /// historical20..20000Hz clamp BEFORE live connection modulation.
    pub fixed_factors: Vec<SourceFrequencyFold>,
    pub compiled: Option<SourceProcessingParameterSite>,
}
/// Fixed-source historical Freq operation. Under/overflow of the positive
/// multiplier becomes a typed clamp, never a nonfinite retained value.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SourceFrequencyFold { MultiplyClamp(f64), LowerClamp, UpperClamp }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceProcessingParameterSite { pub scope: usize, pub site: ScopeControlSite }

/// The typed operand site within one real per-activation owner.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ScopeControlSite {SignalNode(usize),ChainPre(usize),ChainPost(usize)}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct SourceScopeControlRole {
    pub field: SourceOwnerOutputField,
    pub chain_index: usize,
    pub control: ControlRef,
    pub compiled: Option<ScopeControlSite>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SourceOwnerControlTarget {Bus{bus:BusRef,node:usize},ProcessingScope{scope:usize,site:ScopeControlSite}}

/// Exact author-declared source-output field role on one actual operand.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum SourceOwnerOutputField {Gain,Pan,Mute}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct SourceOwnerOutputControl {
    pub owner:SourceOwnerId,
    pub field:SourceOwnerOutputField,
    pub target:SourceOwnerControlTarget,
    pub control:ControlRef,
}

/// An original matrix node and its original ordered chain coordinate. Native
/// source addresses identify definitions only; PlanId/Arc owns their liveness.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProcessingMatrix {
    pub owner: SourceOwnerId,
    pub node: u32,
    pub chain: ChainRef,
    pub index: usize,
    pub matrix: ChannelMatrix,
    /// Bounded immutable schemas retained by the source adapter; only cells in
    /// the actual admitted working layout become mutable IR controls.
    pub control_definitions: Vec<SourceProcessingCellDefinition>,
    /// Actual shared IR control references, never a consumer-reconstructed native name.
    pub controls: Vec<SourceProcessingCell>,
    /// Actual postdecode binding, absent until ordinary materialization.
    pub compiled: Option<SourceProcessingMatrixBinding>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProcessingMatrixBinding {
    pub scope: usize,
    /// None denotes the retained ordinary stereo chain coordinate above.
    pub signal_node: Option<usize>,
    pub layout: SignalLayout,
    pub cells: Vec<SourceProcessingCell>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProcessingCellDefinition {
    pub input: usize,
    pub output: usize,
    pub control: Control,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceProcessingCell {
    pub input: usize,
    pub output: usize,
    pub control: ControlRef,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProcessingPort {
    pub owner: SourceOwnerId,
    /// Concrete caller matrix; its input count is checked against actual
    /// SourceStorage. Choosing this is an explicit Core graph contract, never
    /// automatic native descriptor provenance from a file channel count.
    pub conversion: ChannelMatrix,
}

/// Descriptor identities joined to the current SamplePlayer header path.
/// Names encode the ORIGINAL numeric descriptor, not inferred speaker labels.
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum UviNativeLayout { Code1, Code2, Code6 }
impl UviNativeLayout {
    pub const fn channels(self)->usize {match self {Self::Code1=>1,Self::Code2=>2,Self::Code6=>6}}
    /// Only valid plain decoded-header counts with an admitted attachment law.
    pub fn plain_header(channels:usize)->Option<Self>{match channels{1=>Some(Self::Code1),2=>Some(Self::Code2),6=>Some(Self::Code6),_=>None}}
    /// Current source-image profile. Distinct from programmable matrix cells.
    /// Unknown pairs refuse; there is no general zero-padding fallback.
    pub fn weight(self,to:Self,input:usize,output:usize)->Option<f32>{
        if input>=self.channels() || output>=to.channels(){return None;}
        let selected=match(self,to){
            (Self::Code1,Self::Code6)=>input==0&&output==2,
            (Self::Code2,Self::Code6)=>input==output,
            (Self::Code6,Self::Code6)|(Self::Code2,Self::Code2)|(Self::Code1,Self::Code1)=>input==output,
            (Self::Code6,Self::Code2)=>output==0&&(input==0||input==4)||output==1&&(input==1||input==5),
            _=>return None,
        };
        Some(if !selected{0.}else if self==Self::Code6&&to==Self::Code2{f32::from_bits(0x3f13cd3a)}else{1.})
    }
}
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct UviNativeSourceInput {pub layout:UviNativeLayout,pub storage_channels:usize,pub provenance:UviNativeSourceProvenance}
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum UviNativeSourceProvenance {PlainHeader,UnloadedDefault}
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct UviNativeOnePoleSite {pub chain:ChainRef,pub index:usize,pub frequency:f64,pub frequency_control:Option<String>,pub tracking:f64,pub mode:ChannelOnePoleMode}
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct UviNativeProcessingProfile {
    pub owner:SourceOwnerId,
    pub members:Vec<(ZoneRef,Option<UviNativeSourceInput>)>,
    pub one_poles:Vec<UviNativeOnePoleSite>,
    /// Set only by the actual application backend output contract, not XML kind.
    pub requested:Option<UviNativeLayout>,
}
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct UviNativeLayoutConvert {pub source:UviNativeLayout,pub destination:UviNativeLayout,pub pan:SignalParameter,pub pan_offset:f64}
/// Numerical primitive and control-frontier contracts are separate. This is
/// the proved native first-setup32 / prepared-whole-invocation distinction;
/// the caller supplies actual setup boundaries, never absolute clock%32.
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum UviOnePoleCadence { PreparedInvocation, FirstSetup32 }
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct UviNativeOnePole {pub frequency:SignalParameter,pub tracking:f64,pub mode:ChannelOnePoleMode,pub cadence:UviOnePoleCadence,pub tail_frames:u32}

/// Native scope lifetime evidence, separate from numerical sample profiles.
/// Kontakt Group ownership is deliberately not inferred from its display name.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceProcessingLifetime { UviKeygroupActivation }

/// One per-attack shared processing definition. Only selected members of this
/// activation contribute, in this authored order. Each member keeps its source
/// clock, local chain and local amplitude; this section runs ONCE after their sum.
/// Its optional amplitude is an additional explicit boundary, not an inferred
/// envelope instance from the declaration's ancestry. No global group sharing.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessingScope {
    pub members: Vec<ZoneRef>,
    /// Explicit precision boundary, not a format or compiler default.
    pub sum: ProcessingSumLaw,
    /// Explicit processing clock domain. This first schedule preserves core's
    /// existing absolute CELL cadence; it does not stand for UVI's native32.
    pub cadence: ProcessingCadence,
    pub chain: Option<ChainRef>,
    pub amplitude: Option<ModulatorRef>,
    /// None emits to the instrument input; Some is the existing bus identity.
    pub output: Option<BusRef>,
    /// Explicit source/sum layouts and projection before the existing stereo section.
    /// Absence retains the exact original stereo processing schedule.
    #[cfg_attr(feature = "cache", serde(default))]
    pub signal: Option<ProcessingSignal>,
}

/// Ordered discrete-channel port, separate from physical packing and native descriptors.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalLayout { pub channels: usize }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessingSignal {
    /// Existing f32 resampling, f64 source gain/amplitude sections, f32 member outlet.
    pub source_law: ProcessingSourceLaw,
    pub input: SignalLayout,
    pub nodes: Vec<ProcessingSignalNode>,
    pub controls: Vec<ProcessingCellBinding>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub enum ProcessingSignalNode {
    Matrix(ChannelMatrix),
    /// A width-preserving ordered span, sharing the ordinary slot controls and
    /// their ramp law. Fully bypassed spans hold the inner processor histories.
    Mix(ChannelMix),
    /// Existing memoryless Core rectifier applied independently to each retained channel.
    Rectifier(Rectifier),
    /// Width-preserving generic core-law stage, before the explicit projection.
    Gain(ChannelGain),
    /// One independent history per processing owner and original channel.
    OnePole(ChannelOnePole),
    /// Current original f32 N/stereo kernel. Existing CoreOnePoleF64 unchanged.
    UviOnePole(UviNativeOnePole),
    /// True descriptor conversion, not programmable GainMatrix. Its Pan operand
    /// is retained but current wide6->2 does not read it.
    UviLayout(UviNativeLayoutConvert),
    /// One scalar Pan at a declared stereo boundary. Its explicit expression
    /// home consumes canonical note pan exactly once; other Pan stages do not.
    Pan(ChannelPan),
    /// Invoke the enclosing global bus's actual existing stereo processor
    /// chain at this exact top-level point. No source or foreign-chain target.
    BusChain,
    /// Silence only this explicit signal outlet while upstream processors and
    /// source clocks continue. The canonical Toggle never overwrites Gain.
    Mute(ChannelMute),
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelMute {
    pub muted: bool,
    /// Actual canonical Toggle control; no real-valued surrogate or ramp.
    pub control: Option<String>,
}
/// A typed real stage parameter. A control binds to the existing generation's
/// value/ramp owner; the constant is used only in the absence of a binding.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct SignalParameter {
    pub constant: f64,
    pub control: Option<String>,
    /// Optional per-note factor on the retained global base. Absence preserves
    /// the original authored sound and does not install an enhancement node.
    #[cfg_attr(feature = "cache", serde(default))]
    pub expression: Option<SignalExpressionBinding>,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalExpressionSource { Pressure, Timbre }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignalExpressionBinding {
    pub source: SignalExpressionSource,
    pub law: SignalExpressionLaw,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SignalExpressionLaw {
    /// Gain base times an event-rate linear, nonnegative factor.
    LinearFactor { zero: f64, one: f64 },
    /// OnePole Hertz base times 2^mapped_octaves. No hidden destination clamp.
    Octaves { zero: f64, one: f64 },
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelMix {
    /// Number of following nodes, including nested Mix markers.
    pub count: u16,
    pub address: SlotAddress,
    pub dry: f64,
    pub wet: f64,
    pub bypass: bool,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelGain { pub gain: SignalParameter, pub arithmetic: ChannelGainArithmetic }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelPan {
    /// Immutable source connection offset, separate from the mutable scalar base.
    pub offset: f64,
    pub position: SignalParameter,
    pub law: PanLaw,
    pub expression: bool,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelGainArithmetic { CoreGainF64ThenF32 }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelOnePole {
    pub cutoff_hz: SignalParameter,
    pub mode: ChannelOnePoleMode,
    pub arithmetic: ChannelOnePoleArithmetic,
    /// Explicit bounded zero-input continuation after the last source ends.
    /// This is an authored truncation policy, not the exact finite life of an IIR.
    pub tail_frames: u32,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOnePoleMode { LowPass, HighPass }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOnePoleArithmetic {
    /// Existing core f64 recurrence/Hertz conversion/segment-end history flush.
    /// This does not select either native UVI f32 sample kernel.
    CoreOnePoleF64,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingSourceLaw {
    CoreF64GainAmplitudeF32Outlet,
    /// Same source clock/local Core sections; only these explicit member layout
    /// edges and downstream named primitives select the current native laws.
    /// Fixed existing16-member capacity is priced in the actual enum layout.
    UviC997Layout {members:[Option<UviNativeSourceInput>;16]},
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelMatrix {
    pub input: SignalLayout,
    pub output: SignalLayout,
    /// Row-major cells: output ordinal * input channels + input ordinal.
    pub cells: Vec<f64>,
    pub arithmetic: MatrixArithmetic,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixArithmetic { SeparateMulAddF32 }
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessingCellBinding {
    pub node: usize,
    pub input: usize,
    pub output: usize,
    pub control: String,
}

/// The first executable source-edge summation contract. Member blocks have
/// binary32 storage; contributions add in authored order with a binary32 store
/// after EVERY addition. Native UVI constant/vector helper equivalence remains
/// a separate adapter proof, never inferred merely from choosing this variant.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingSumLaw { OrderedF32 }

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingCadence { CoreCell64 }

/// Serial processors in one scope. A voice chain splits around the amplitude
/// envelope because nonlinear stages sound different on each side of it.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub scope: Scope,
    pub pre_amplitude: Vec<Processor>,
    pub post_amplitude: Vec<Processor>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Processor {
    Gain(Gain),
    /// Drive, character and utility: one superset configured per native unit.
    Color(fx::color::Config),
    Pan(Pan),
    Filter(Filter),
    /// The universal EQ superset: a serial cascade of bands under generic, Kontakt legacy or
    /// Solid G-EQ laws (see `fx::eq`).
    Eq(fx::eq::Config),
    Delay {
        time: Time,
        feedback: f64,
        mix: f64,
    },
    /// Linear stereo mix: rows are output left/right, columns input
    /// left/right. Width, balance, polarity and channel swaps.
    StereoMatrix([[f64; 2]; 2]),
    /// Reverb and convolution over a summed signal: bus and master scope.
    Space(fx::space::Config),
    /// Every compressor, limiter, gate and expander: one configurable superset.
    Dynamics(fx::dynamics::Config),
    /// Script- and modulation-addressed Formant I (or Formant II when `second`): saved Talk,
    /// Sharp, Size.
    LiveFormant { knobs: [f32; 3], second: bool, address: SlotAddress },
    /// Kontakt's Daft filter: normalized controls, laws in the engine.
    Daft(Daft),
    /// Native Ladder LP4. Values are normalized; the engine owns the laws.
    LadderLP4(LadderLP4),
    /// Kontakt's adaptive-resonance filter family (saved types 100..=108).
    Ar(Ar),
    /// Native Formant I: saved Talk, Sharp, Size normalized controls.
    Formant([f32; 3]),
    /// A filter-family configuration with no dedicated variant yet (Formant II).
    FilterBank(fx::filter::Config),
    /// Kontakt Phaser filter, normalized Cutoff/Resonance.
    Phaser([f32; 2]),
    /// Script- and modulation-addressed Phaser filter (type 13): the stored Cutoff and Resonance
    /// are lanes, moved by the native control clock.
    LivePhaser { knobs: [f32; 2], address: SlotAddress },
    /// Kontakt Ring Modulator: sine carrier, see `fx::ringmod`.
    RingMod(fx::ringmod::Config),
    /// Kontakt Kolor (module 0x79): the saved record. The engine owns the mode laws, see `fx::kolor`.
    Kolor(fx::kolor::Record),
    /// Kontakt Stereo Tune: detuned delay taps with cross spread, see `fx::stereo_tune`. Bus scope only.
    StereoTune(fx::stereo_tune::Config),
    /// Universal delay / modulation effect (delays, chorus, flanger, phaser, ...).
    TimeMod(fx::timemod::Config),
    /// Mute a send return when its paired Mix slot is bypassed. Uses that
    /// slot's existing bypass control; insert bypass still passes dry audio.
    SendReturnGate {
        address: SlotAddress,
    },
    /// One parallel branch of an effect rack. The next `count` processors (nested
    /// ones included) run on the signal that entered the group's first branch;
    /// `gain` times their output joins the sum, which the `last` branch leaves
    /// as the signal. Branches of one rack follow each other directly.
    Branch {
        count: u16,
        gain: Gain,
        first: bool,
        last: bool,
    },
    /// The next `count` processors of the chain run beside the unprocessed
    /// signal: `dry` of it passes around them, `wet` of their output follows,
    /// and `bypass` swaps both for the dry signal alone. A script can change
    /// all three at runtime through `address`. Bus scope only.
    Mix {
        count: u16,
        address: SlotAddress,
        dry: f64,
        wet: f64,
        bypass: bool,
    },
    /// The single explicit canonical note-Pan home of a shared source owner.
    /// Its scalar/control value is combined with projected note Pan before
    /// evaluating the declared law. Not valid on an ordinary source chain.
    ExpressionPan(OwnedPan),
}

/// Canonical shared Pan home: its base remains the public Pan/control value.
/// Constant source connections add after the base trajectory and before one clamp.
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct OwnedPan {pub pan:Pan,pub offset:f64}

impl Processor {
    /// The same unit addressed by a script slot, so [`Processor::lanes`] are
    /// live. A unit that already names its slot (an EQ band) keeps it.
    pub fn at(self, address: SlotAddress) -> Self {
        match self {
            Self::Color(c) if c.address.is_none() => Self::Color(c.at(address)),
            Self::Dynamics(c) if c.address.is_none() => Self::Dynamics(fx::dynamics::Config { address: Some(address), ..c }),
            Self::Eq(c) if c.address.is_none() => Self::Eq(fx::eq::Config { address: Some(address), ..c }),
            Self::TimeMod(c) => Self::TimeMod(fx::timemod::Config { address: Some(address), ..c }),
            Self::Space(c) => Self::Space(fx::space::Config { address: Some(address), ..c }),
            Self::Ar(a) => Self::Ar(Ar { address: Some(address), ..a }),
            Self::Daft(d) => Self::Daft(Daft { address: Some(address), ..d }),
            p => p,
        }
    }
    /// The unit's script slot and its live knobs; no knobs without a slot.
    pub fn lanes(&self) -> Option<(SlotAddress, Vec<fx::Lane>)> {
        let (address, lanes) = match self {
            Self::Color(c) => (c.address, c.lanes()),
            Self::Dynamics(c) => (c.address, c.lanes()),
            Self::Eq(c) => (c.address, c.lanes()),
            Self::TimeMod(c) => (c.address, c.lanes()),
            Self::Space(c) => (c.address, c.lanes()),
            // BParFXFilter CUTOFF and RESONANCE store the write (filters_eq.tsv identity).
            Self::Ar(Ar { address, cutoff, resonance, .. }) | Self::Daft(Daft { address, cutoff, resonance, .. }) => (
                *address,
                vec![
                    fx::Lane::new("ENGINE_PAR_CUTOFF", ProcessorParameter::Cutoff, *cutoff, fx::Law::Identity),
                    fx::Lane::new("ENGINE_PAR_RESONANCE", ProcessorParameter::Resonance, *resonance, fx::Law::Identity),
                ],
            ),
            _ => return None,
        };
        Some((address?, lanes))
    }
}

/// Kontakt's Daft low or high pass. Every value is the stored 0..=1 control;
/// the native engine owns the gain, cutoff and resonance laws.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Daft {
    /// The script slot whose `ENGINE_PAR_CUTOFF`/`RESONANCE` reach the knobs.
    #[cfg_attr(feature = "cache", serde(default))]
    pub address: Option<SlotAddress>,
    pub gain: f64,
    pub cutoff: f64,
    pub resonance: f64,
    pub highpass: bool,
}

/// Kontakt's AR filter: `mode` is the saved type minus 100 (0..=8: 2-pole,
/// 4-pole, combined 2/4; each low, band, high). Cutoff and resonance are the
/// stored 0..=1 controls; the engine owns the laws.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ar {
    /// The script slot whose `ENGINE_PAR_CUTOFF`/`RESONANCE` reach the knobs.
    #[cfg_attr(feature = "cache", serde(default))]
    pub address: Option<SlotAddress>,
    pub mode: u8,
    pub cutoff: f64,
    pub resonance: f64,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LadderLP4 {
    /// Physical native owner; absent for an authored standalone processor.
    pub address: Option<SlotAddress>,
    pub gain: f64,
    pub cutoff: f64,
    pub resonance: f64,
    /// Native alternative cutoff path (flag 0x49): the cutoff lane snaps to each new target.
    pub cutoff_path: bool,
    /// Output selector: saved Morphology type minus 29 (1..=12); feedback closes around stage 4.
    pub taps: u8,
    /// Native HQ flag: run the core at twice the rate inside a shared 2x Kaiser oversampler.
    pub high_quality: bool,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rectifier {
    /// `|x|`.
    Full,
    /// `max(x, 0)`.
    Half,
}

/// Where a script finds an effect slot: `set_engine_par`'s group, slot and
/// generic arguments (-1 where they do not apply).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotAddress {
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpulseRef(pub usize);

/// A stereo impulse response, already shaped (reversed, predelayed, enveloped,
/// gain-scaled) by the importing profile. A mono response repeats in both
/// channels; the channels have the same length.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Impulse {
    pub rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    /// A source whose loader decodes this impulse's audio from `asset` (a
    /// file of the program) and clears the field; `rate`/`left`/`right` hold a
    /// unit-impulse placeholder until then.
    pub asset: Option<AssetRef>,
}

/// The source controls of a sampled reverb that act on its impulse response.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImpulseShape {
    /// Kept fraction of the response's duration, `(0, 1]`.
    pub time: f64,
    /// Low and high frequency damping, `[-1, 1]` (negative boosts).
    pub damping_low: f64,
    pub damping_high: f64,
    pub pre_delay_ms: f64,
    /// Stereo width, `[-1, 1]`; negative swaps the channels.
    pub width: f64,
    /// Scale the response to unit mean channel energy.
    pub normalize: bool,
    /// A SampledReverb (its own gain references) rather than a Convolver.
    pub sampled_reverb: bool,
    /// Play the response backwards.
    pub reverse: bool,
    /// Scale to Kontakt's Auto Gain (see `shape_impulse`).
    pub auto_gain: bool,
    /// Volume envelope knots `[time fraction, dB]`, interpolated in amplitude.
    pub envelope: Option<[[f32; 2]; 8]>,
    /// Kontakt's early/late filters and their crossfade.
    pub filters: Option<ImpulseFilters>,
    /// Predelay frames `(int)((rate·ms)·0.001)` in f32 (Kontakt's wrapper)
    /// instead of rounded.
    pub truncate_pre_delay: bool,
}

/// Kontakt Convolution's early and late filters: `[low cut, high cut]` Hz
/// each, the early response up to `crossover` (a fraction of the length)
/// and the late one after it. `None` filters the whole response with `late`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImpulseFilters {
    pub early: [f32; 2],
    pub late: [f32; 2],
    pub crossover: Option<f32>,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    pub cutoff: Frequency,
    pub resonance: Resonance,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterKind {
    LowPass { poles: u8 },
    HighPass { poles: u8 },
    BandPass { poles: u8 },
    Notch { poles: u8 },
    AllPass,
    Peak { gain: Gain },
    LowShelf { gain: Gain },
    HighShelf { gain: Gain },
    /// A 6 dB/octave shelf: `Filter::resonance` has no effect on it.
    FirstOrderLowShelf { gain: Gain },
    FirstOrderHighShelf { gain: Gain },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Output {
    #[default]
    Master,
    Bus(BusRef),
}

/// One channel graph bound to an existing BusRef. Output layout is explicit;
/// source cardinality never implies a native descriptor or conversion matrix.
#[cfg_attr(feature="cache",derive(serde::Serialize,serde::Deserialize))]
#[derive(Clone,Debug,PartialEq)]
pub struct ProcessingBus {
    pub bus:BusRef,
    pub owner:Option<SourceOwnerId>,
    pub signal:ProcessingSignal,
    pub output:SignalLayout,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub chain: Option<ChainRef>,
    pub sends: Vec<Send>,
    pub output: Output,
    /// Level of the bus's own output; its `sends` are tapped before it, except
    /// a group tap's post-fader ones, which it scales too.
    pub gain: Gain,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Send {
    pub to: Output,
    pub gain: Gain,
    pub position: SendPosition,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendPosition {
    PreChain,
    PostChain,
}

/// Format-neutral binding of one authored control to an actual processor field.
/// The control's Continuous range is in the field's native units (Hz, linear
/// gain, dB, Q or normalized knobs). A bound Peak uses dB Gain and 0..1
/// Cutoff/Resonance knobs (20 Hz * 10^(3*x), 1/3 + x/0.375 octaves).
/// Ramping uses the engine sample clock.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessorControl {
    pub control: ControlRef,
    pub chain: ChainRef,
    pub index: usize,
    pub parameter: ProcessorParameter,
    pub ramp: Time,
}

/// Completed authored stages on the named side of the voice amplitude split.
/// Lowering preserves this boundary when one stage expands to several kernels.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceSendPosition {
    BeforeAmplitude(usize),
    AfterAmplitude(usize),
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceSendTap {
    pub chain: ChainRef,
    pub position: VoiceSendPosition,
    pub bus: BusRef,
    pub gain: Gain,
    pub bypass: bool,
    /// Optional controls use existing native Continuous ranges (linear gain,
    /// bypass 0..1), and bind real DSP lanes rather than a parameter mirror.
    pub gain_control: Option<ControlRef>,
    pub bypass_control: Option<ControlRef>,
    pub ramp: Time,
}

// ---------------------------------------------------------------- controls

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// Stable identity from the source (script variable, opcode, parameter ID).
    pub key: String,
    pub label: String,
    pub value: ControlValue,
    pub automation: Automation,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    Continuous {
        min: f64,
        max: f64,
        default: f64,
        unit: ControlUnit,
    },
    Integer {
        min: i64,
        max: i64,
        default: i64,
    },
    Toggle {
        default: bool,
    },
    Choice {
        options: Vec<String>,
        default: usize,
    },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlUnit {
    None,
    Decibels,
    Hertz,
    Seconds,
    Semitones,
    Percent,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Automation {
    #[default]
    None,
    Host,
    Controller(u8),
}

/// Saved Kontakt script-slider or -switch assignment. Ordinals count only widgets of that
/// kind (`ui_slider` or `ui_switch`), not all UI controls.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptAutomation {
    pub source: ScriptAutomationSource,
    pub source_slot: u8,
    /// A `pts_script_switch_*` target; otherwise a slider.
    #[cfg_attr(feature = "cache", serde(default))]
    pub switch: bool,
    pub slider: u32,
    pub low: f64,
    pub high: f64,
    pub soft_takeover: bool,
}
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptAutomationSource {
    Controller(u8),
    HostParameter(u16),
}

// ---------------------------------------------------------------- behavior

/// A script that reacts to events. The IR carries it in its source language;
/// lowering compiles it to the native behavior instruction set.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct Behavior {
    pub name: String,
    pub language: Language,
    pub source: String,
    /// The source's script slot, when it numbers them (Kontakt's 0..=4).
    pub slot: Option<u8>,
    /// Persisted values restored before the first callback, keyed by variable.
    pub state: Vec<(String, Saved)>,
    pub requires: Vec<Capability>,
}

/// A persisted script variable's saved value.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq)]
pub enum Saved {
    Int(i64),
    Real(f64),
    Text(String),
    /// An integer array (`%name`), in element order.
    Ints(Vec<i64>),
    /// A real array (`?name`), in element order.
    Reals(Vec<f64>),
    Texts(Vec<String>),
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Ksp,
    Lua,
}

/// A runtime service a behavior needs; lowering rejects modules whose
/// requirements the runtime cannot provide instead of running them partially.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    NoteCallbacks,
    ReleaseCallbacks,
    ControllerCallbacks,
    Waits,
    GeneratedNotes,
    EventEdits,
    GroupSelection,
    ControlCallbacks,
    UserInterface,
    PersistentState,
    FileAccess,
    EngineParameters,
}

// ---------------------------------------------------------------- report

/// Source meaning with no IR representation. Location is in source terms.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    /// e.g. `"<region> 12 (line 40)"` or `"zone 3"`.
    pub location: String,
    /// The source feature: opcode, field or header name.
    pub feature: String,
    /// The authored value, verbatim.
    pub value: String,
    pub reason: Reason,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The translator does not recognize this feature.
    Unknown,
    /// Recognized, but the IR has no equivalent yet.
    NotModeled,
    /// Recognized, but this value cannot be represented.
    InvalidValue,
    /// Recognized and representable, but how the source maps it to sound
    /// (its scaling, curve or timing law) is not established.
    UnknownLaw,
    /// The law is unverified and the feature was omitted, rather than executed
    /// by an approximation. Preserve that distinction in every host report.
    UnknownLawOmitted,
}

/// A feature name with its numbers and paths removed, so equal causes group:
/// `"Filter: filter type 12"` and `"... 13"` are one line in a report.
pub fn normalized_feature(feature: &str) -> String {
    let words: Vec<String> = feature
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .map(|word| {
            if word.contains('/') || word.contains('\\') {
                return "<path>".to_string();
            }
            let mut out = String::new();
            let mut digits = false;
            for c in word.chars() {
                if c.is_ascii_digit() {
                    if !digits {
                        out.push('N');
                    }
                    digits = true;
                } else {
                    digits = false;
                    out.push(c);
                }
            }
            out
        })
        .collect();
    words.join(" ").chars().take(140).collect()
}

/// Unsupported features grouped by [`normalized_feature`] and ranked by how
/// often they occur (ties by name). One function for the plugin's load report
/// and the corpus scoreboard, so both rank the same way.
pub fn rank_features<'a>(features: impl IntoIterator<Item = &'a str>) -> Vec<(String, usize)> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for f in features {
        *counts.entry(normalized_feature(f)).or_default() += 1;
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
}

#[cfg(test)]
mod ranking_tests {
    use super::*;

    #[test]
    fn features_group_by_cause_and_rank_by_count() {
        let ranked = rank_features([
            "Filter: type 12",
            "effect",
            "Filter: type 13",
            "Filter: type 7",
            "effect",
            "zero /mnt/a/b.wav",
        ]);
        assert_eq!(ranked[0], ("Filter: type N".to_string(), 3));
        assert_eq!(ranked[1], ("effect".to_string(), 2));
        assert_eq!(ranked[2], ("zero <path>".to_string(), 1));
    }
}

/// A Kontakt note name: MIDI 60 = C3, 0 = C-2. Strict bounds; no wrapping.
pub fn parse_note(text: &str) -> Option<u8> {
    let mut chars = text.trim().chars();
    let base = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (offset, octave) = match rest.as_bytes().first() {
        Some(b'#') => (1, &rest[1..]),
        Some(b'b' | b'B') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let note = (i32::from(octave.parse::<i16>().ok()?) + 2) * 12 + base + offset;
    u8::try_from(note).ok().filter(|n| *n < 128)
}
