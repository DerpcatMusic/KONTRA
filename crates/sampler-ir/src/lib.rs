//! Format-neutral semantic instrument description.
//!
//! Source translators (Kontakt, SFZ, ...) emit an [`Instrument`]; the native
//! runtime lowers it to prepared execution tables. Plain data: no I/O, no
//! audio-thread state, no dependencies. Quantities keep source units
//! ([`units`]), every modulator and processor names its [`Scope`], and meaning
//! a translator cannot express is listed in [`Instrument::unsupported`] rather
//! than dropped.
#![forbid(unsafe_code)]

pub mod kontakt;
pub mod units;
mod validate;

pub use units::{Frequency, Gain, Pan, PanLaw, Pitch, Resonance, SourceFrames, Span, Time};
pub use validate::{Reference, ValidationError};

macro_rules! reference {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
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
    pub buses: Vec<Bus>,
    /// Sum entering the instrument/program inserts, after upstream group buses.
    /// Direct outputs before this point bypass instrument-level processing.
    pub input_bus: Option<BusRef>,
    /// Source addresses of buses (Kontakt's `$NI_BUS_OFFSET` + n), which a
    /// script's output-channel writes name.
    pub bus_addresses: Vec<(i32, BusRef)>,
    /// Impulse responses bus convolutions refer to.
    pub impulses: Vec<Impulse>,
    pub controls: Vec<Control>,
    /// Actual processor lanes driven by authored controls; indices span pre then post.
    pub processor_controls: Vec<ProcessorControl>,
    /// Ordered per-voice sends at authored processor boundaries.
    pub voice_send_taps: Vec<VoiceSendTap>,
    pub behaviors: Vec<Behavior>,
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
    /// Source meaning this description does not carry. Lowering never reads it;
    /// it exists so a caller can show or reject what was not translated.
    pub unsupported: Vec<Unsupported>,
    /// Complete authored DSP slot inventory, collected only by diagnostic builds.
    pub dsp_slots: Option<Vec<DspSlot>>,
    /// Native XML records for the independent audition oracle, never consumed by playback.
    pub native_family: Option<NativeFamily>,
    /// Native groups with an active sample-start modulation target.
    pub native_start_mod_groups: Option<Vec<u32>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeFamily {
    pub zones: Vec<NativeFamilyZone>,
    pub unknown: Option<&'static str>,
}
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspSlotKind { Fx, Filter, Mod }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DspDisposition {
    Implemented,
    Approximated(DspSlotReason),
    Dropped(DspSlotReason),
}

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceIndices {
    pub groups: Vec<Option<GroupRef>>,
    pub zones: Vec<Option<ZoneRef>>,
    pub modulators: Vec<SourceModulator>,
    /// Authored native mod/target names and physical lookup addresses, including omitted DSP.
    pub engine_lookups: Vec<SourceEngineLookup>,
    /// Authored normalized getter values before script initialization, at physical addresses.
    pub engine_values: Vec<SourceEngineValue>,
    /// Native addresses aliasing real continuous processor controls.
    pub control_aliases: Vec<SourceControlAlias>,
    pub slots: Vec<Option<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceModulator {
    pub group: usize,
    pub slot: usize,
    pub external: bool,
    pub name: String,
    pub runtime: Option<ModulatorRef>,
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceControlAlias {
    pub control: ControlRef,
    pub parameter: String,
    pub address: SlotAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEngineValue {
    pub parameter: u16,
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
    pub value: i32,
}

/// Instrument volume as a host parameter (Kontakt's CC7): it starts at the
/// saved value and, when `controller` is received, becomes `(cc/127)^3`,
/// replacing the saved value (measured, not multiplied in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostVolume {
    pub controller: u8,
    /// Linear gain saved with the instrument, before any controller.
    pub saved: f64,
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub location: AssetLocation,
    pub encoding: Encoding,
    /// Recorded pitch stored with the audio, if any.
    pub root_key: Option<u8>,
    /// Loops stored with the audio (e.g. a WAV `smpl` chunk).
    pub loops: Vec<LoopRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetLocation {
    /// Path as authored, relative to the instrument file unless absolute.
    Path(String),
    /// Entry of a Kontakt file table, resolved by the container profile.
    KontaktFile { id: i32 },
}

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// Strictly admitted Kontakt cycle oscillator, independent of sample root/rate.
    pub wavetable: Option<Wavetable>,
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

/// Saved controls for v1's admitted 2048-frame Kontakt wavetable subset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wavetable {
    pub position: f32,
    pub phase: f32,
    pub form1: f32,
    pub form2: f32,
    /// Native phase-form IDs: 0 (identity) or 16 (ASYM2MP).
    pub form1_type: u8,
    pub form2_type: u8,
}
impl Wavetable {
    pub fn valid(self) -> bool {
        [self.position, self.phase, self.form1, self.form2].iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && matches!(self.form1_type, 0 | 16)
            && matches!(self.form2_type, 0 | 16)
    }
}

/// Physical row and the logical operator connecting it to the following row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupStart {
    pub slot: u8,
    pub test: StartTest,
    pub next: StartJoin,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartJoin {
    And,
    AndNot,
    Or,
}
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
#[derive(Clone, Debug, PartialEq)]
pub struct GroupTap {
    pub bus: BusRef,
    pub post: Vec<usize>,
}

/// A group's send to a bus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupSend {
    pub to: BusRef,
    pub gain: Gain,
    /// Taken before the group's own gain (fader), else after it.
    pub pre_fader: bool,
}

/// Past `voices` sounding voices, starting another fades one out over
/// `fade`: a released one first when `prefer_released`, else by `kill`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceLimit {
    pub voices: u32,
    pub kill: Kill,
    pub prefer_released: bool,
    pub fade: Time,
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub asset: AssetRef,
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
                let Processor::Compressor(c) = *processor else {
                    continue;
                };
                for (parameter, label, default, low, high, unit) in [
                    (
                        ProcessorParameter::Threshold,
                        "Threshold",
                        c.threshold_db,
                        -60_f64,
                        0_f64,
                        ControlUnit::Decibels,
                    ),
                    (
                        ProcessorParameter::Ratio,
                        "Ratio",
                        c.ratio,
                        1_f64,
                        20_f64,
                        ControlUnit::None,
                    ),
                    (
                        ProcessorParameter::Attack,
                        "Attack",
                        c.attack.seconds(),
                        0_f64,
                        1_f64,
                        ControlUnit::Seconds,
                    ),
                    (
                        ProcessorParameter::Release,
                        "Release",
                        c.release.seconds(),
                        0_f64,
                        5_f64,
                        ControlUnit::Seconds,
                    ),
                ] {
                    if existing.contains(&(chain, index, parameter as u8)) {
                        continue;
                    }
                    let control = ControlRef(self.controls.len());
                    self.controls.push(Control {
                        key: format!("compressor:{chain}:{index}:{label}"),
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
    /// nonzero depth.
    pub fn amplitude_controllers(&self) -> Vec<u8> {
        let mut zones = [0usize; 128];
        for zone in &self.zones {
            let mut seen = [false; 128];
            for route in zone.routes.iter().map(|r| &self.routes[r.0]) {
                let ModulationSource::Controller(cc) = self.modulators[route.source.0].source
                else {
                    continue;
                };
                let live = match route.depth {
                    Depth::Normalized(d) => d != 0.0,
                    Depth::Gain(_) | Depth::Pitch(_) => true,
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
        for zone in &mut self.source_indices.zones {
            *zone = zone.and_then(|z| remap[z.0]);
        }
        let mut used = vec![false; self.assets.len()];
        for zone in &self.zones {
            if let Some(used) = used.get_mut(zone.asset.0) {
                *used = true;
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
            zone.asset = AssetRef(
                renumbered
                    .get(zone.asset.0)
                    .copied()
                    .unwrap_or(zone.asset.0),
            );
        }
        kept
    }
}

impl Zone {
    /// Full key and velocity range, root key 60, no processing.
    pub fn new(asset: AssetRef) -> Self {
        Self {
            asset,
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
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VelocityResponse {
    /// Velocity does not change amplitude.
    None,
    /// Amplitude is velocity / 127.
    #[default]
    Linear,
    /// Amplitude is (velocity / 127) ^ exponent.
    Power(f64),
}

/// Zone crossfades, in key and velocity steps inside the zone's own ranges.
/// A fade-in of `F` over a low edge `L` has gain `(v - L + 1) / (F + 1)` for
/// `L <= v <= L + F`; a fade-out over a high edge `H` mirrors it,
/// `(H - v + 1) / (F + 1)`. Zero is no fade. The key and velocity gains
/// multiply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fades {
    pub velocity_in: u8,
    pub velocity_out: u8,
    pub key_in: u8,
    pub key_out: u8,
}

/// Inclusive MIDI key range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRange {
    pub low: u8,
    pub high: u8,
}

impl KeyRange {
    pub const FULL: Self = Self { low: 0, high: 127 };
}

/// Inclusive MIDI 1.0 velocity range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VelocityRange {
    pub low: u8,
    pub high: u8,
}

impl VelocityRange {
    pub const FULL: Self = Self { low: 1, high: 127 };
}

/// Plays only while a 7-bit controller is within an inclusive range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerRange {
    pub controller: u8,
    pub low: u8,
    pub high: u8,
}

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub sequence: SequenceRef,
    pub take: Take,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Take {
    /// Zero-based position in a sequential or uniform-random sequence.
    Index(u32),
    /// Plays when a uniform draw in `[low, high)` lands here (SFZ lorand/hirand).
    Probability { low: f64, high: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub policy: SequencePolicy,
    /// Number of alternatives; `Take::Index` values are below this.
    pub takes: u32,
    /// What owns the position counter.
    pub counter: CounterScope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequencePolicy {
    RoundRobin,
    Random,
    RandomNoRepeat,
}

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    pub name: String,
    pub choices: Vec<AxisChoice>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisChoice {
    pub name: String,
    /// Keys that select this choice.
    pub switch_keys: Vec<u8>,
}

/// A zone's requirement that `axes[axis]` has `choice` active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisPick {
    pub axis: usize,
    pub choice: usize,
}

/// A selectable articulation, switched by keys and/or an alternative driver.
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchOwner {
    /// The runtime holds the articulation; zones name the one they belong to.
    #[default]
    Native,
    /// A behavior reads the switch keys and selects groups itself; drivers
    /// select by tapping an articulation's first switch key into it.
    Behavior,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Driver {
    #[default]
    Keys,
    Velocity,
    Channel,
    Controller,
    Program,
}

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
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopRange {
    pub start: SourceFrames,
    /// Exclusive end.
    pub end: SourceFrames,
    pub crossfade: Span,
    pub alternating: bool,
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Modulator {
    pub scope: Scope,
    pub source: ModulationSource,
}

/// What a modulator reads. Unipolar sources produce 0..=1, bipolar ones -1..=1:
/// envelopes, controllers, velocity, key, pressure, timbre, random and constant
/// are unipolar; LFOs and pitch bend are bipolar.
#[derive(Clone, Debug, PartialEq)]
pub enum ModulationSource {
    Envelope(Envelope),
    /// A multi-segment envelope (Kontakt flex), gated like `Envelope`.
    Breakpoints(Breakpoints),
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
}

impl ModulationSource {
    /// Whether values span -1..=1 rather than 0..=1.
    pub fn bipolar(&self) -> bool {
        matches!(self, Self::Lfo(_) | Self::PitchBend)
    }
}

/// Glides from 0 through `points`, each reached `time` after the previous;
/// holds at `points[sustain]` while gated; a release glides from the current
/// level through the points after `sustain` (jumping there when not yet
/// reached) and the last level holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakpoints {
    pub points: Vec<Breakpoint>,
    pub sustain: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breakpoint {
    pub time: Time,
    /// 0.0..=1.0 of full scale.
    pub level: f64,
    pub shape: Curve,
}

/// Delay-attack-hold-decay-sustain-release.
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Curve {
    #[default]
    Linear,
    /// expm1(k·t)/expm1(k); positive starts slowly.
    Exponential(f64),
    /// Holds the starting level until the stage ends, then steps.
    Step,
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoShape {
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
    /// -1 to +1.
    SawUp,
    /// +1 to -1.
    SawDown,
    /// A new uniform value each cycle, held.
    SampleAndHold,
    /// A new uniform value each cycle, reached linearly by the cycle's end.
    Random,
}

/// A piecewise-linear transfer curve over 0..=1, as ascending `(input, output)`
/// points. Bipolar values are mapped through `(v + 1) / 2` and back.
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteScale {
    pub source: ModulatorRef,
    pub shape: Option<ShapeRef>,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Depth {
    Gain(Gain),
    Pitch(Pitch),
    /// Fraction of the target's full range.
    Normalized(f64),
}

// ---------------------------------------------------------------- DSP

/// Serial processors in one scope. A voice chain splits around the amplitude
/// envelope because nonlinear stages sound different on each side of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub scope: Scope,
    pub pre_amplitude: Vec<Processor>,
    pub post_amplitude: Vec<Processor>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Processor {
    Gain(Gain),
    /// Native smoothed gain with a dry input contribution.
    Gainer {
        gain: Gain,
        dry: f64,
    },
    /// Native width 0..1 (.5 identity), balance -1..1 and optional pseudo stereo.
    StereoModeller {
        width: f64,
        pan: f64,
        pseudo: bool,
    },
    Pan(Pan),
    Filter(Filter),
    Delay {
        time: Time,
        feedback: f64,
        mix: f64,
    },
    /// Linear stereo mix: rows are output left/right, columns input
    /// left/right. Width, balance, polarity and channel swaps.
    StereoMatrix([[f64; 2]; 2]),
    /// Algorithmic stereo reverb over a summed signal: bus and master scope.
    Reverb(Reverb),
    /// Feed-forward compressor on the stereo pair.
    Compressor(Compressor),
    /// Memoryless rectification of both channels.
    Rectify(Rectifier),
    /// Pinned v1 Lo-Fi approximation; saved normalized controls.
    LoFi { bits: f32, frequency: f32, noise: f32, color: f32 },
    /// Kontakt's Daft filter: normalized controls, laws in the engine.
    Daft(Daft),
    /// Native Ladder LP4. Values are normalized; the engine owns the laws.
    LadderLP4(LadderLP4),
    /// Mute a send return when its paired Mix slot is bypassed. Uses that
    /// slot's existing bypass control; insert bypass still passes dry audio.
    SendReturnGate { address: SlotAddress },
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
    /// `dry * input + wet * (input * impulse)` over a summed signal: bus and
    /// master scope. Convolution adds no latency.
    Convolution {
        impulse: ImpulseRef,
        dry: f64,
        wet: f64,
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
}

/// Kontakt's Daft low or high pass. Every value is the stored 0..=1 control;
/// the native engine owns the gain, cutoff and resonance laws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Daft {
    pub gain: f64,
    pub cutoff: f64,
    pub resonance: f64,
    pub highpass: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LadderLP4 {
    /// Physical native owner; absent for an authored standalone processor.
    pub address: Option<SlotAddress>,
    pub gain: f64,
    pub cutoff: f64,
    pub resonance: f64,
    pub record_version: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rectifier {
    /// `|x|`.
    Full,
    /// `max(x, 0)`.
    Half,
}

/// A feed-forward compressor. The level law is the textbook hard-knee one;
/// only the linked detector (signed channel mean) is recovered from Kontakt.
// ponytail: unverified threshold/ratio/time laws, pending a rendering reference.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compressor {
    pub threshold_db: f64,
    /// At least 1.
    pub ratio: f64,
    pub attack: Time,
    pub release: Time,
    pub makeup: Gain,
    /// Detect on the signed mean of both channels and apply one gain to both.
    pub link: bool,
}

/// Where a script finds an effect slot: `set_engine_par`'s group, slot and
/// generic arguments (-1 where they do not apply).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotAddress {
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpulseRef(pub usize);

/// A stereo impulse response, already shaped (reversed, predelayed, enveloped,
/// gain-scaled) by the importing profile. A mono response repeats in both
/// channels; the channels have the same length.
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

/// Physical reverb settings; an importing profile maps its own controls
/// here. Wet signal only: the dry path is the bus's other send.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reverb {
    /// Seconds for the tail to fall 60 dB.
    pub decay_seconds: f64,
    /// Room size as a scale of the reference line lengths, 0.05..=1.5.
    pub size: f64,
    pub damping_hz: f64,
    pub modulation_seconds: f64,
    /// Input diffusion, 0..=0.75.
    pub diffusion: f64,
    pub predelay_seconds: f64,
    pub input_cutoff_hz: f64,
    /// Wet low-frequency change in dB (zero or negative).
    pub low_shelf_db: f64,
    /// 0 mono .. 1 full width.
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    pub cutoff: Frequency,
    pub resonance: Resonance,
}

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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Output {
    #[default]
    Master,
    Bus(BusRef),
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Send {
    pub to: Output,
    pub gain: Gain,
    pub position: SendPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendPosition {
    PreChain,
    PostChain,
}

/// Format-neutral binding of one authored control to an actual processor field.
/// The control's Continuous range is in the field's native units (Hz, linear
/// gain, dB, Q or normalized knobs). A bound Peak uses dB Gain and 0..1
/// Cutoff/Resonance knobs (20 Hz * 10^(3*x), 0.3 + 2.7*x octaves).
/// Ramping uses the engine sample clock.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceSendPosition {
    BeforeAmplitude(usize),
    AfterAmplitude(usize),
}
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

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// Stable identity from the source (script variable, opcode, parameter ID).
    pub key: String,
    pub label: String,
    pub value: ControlValue,
    pub automation: Automation,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlUnit {
    None,
    Decibels,
    Hertz,
    Seconds,
    Semitones,
    Percent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Automation {
    #[default]
    None,
    Host,
    Controller(u8),
}

/// Saved Kontakt script-slider assignment. Ordinals count only sliders, not all UI controls.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptAutomation {
    pub source: ScriptAutomationSource,
    pub source_slot: u8,
    pub slider: u32,
    pub low: f64,
    pub high: f64,
    pub soft_takeover: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptAutomationSource { Controller(u8), HostParameter(u16) }

// ---------------------------------------------------------------- behavior

/// A script that reacts to events. The IR carries it in its source language;
/// lowering compiles it to the native behavior instruction set.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Ksp,
    Lua,
}

/// A runtime service a behavior needs; lowering rejects modules whose
/// requirements the runtime cannot provide instead of running them partially.
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
    let base = match chars.next()?.to_ascii_uppercase() { 'C' => 0, 'D' => 2, 'E' => 4, 'F' => 5, 'G' => 7, 'A' => 9, 'B' => 11, _ => return None };
    let rest = chars.as_str();
    let (offset, octave) = match rest.as_bytes().first() { Some(b'#') => (1, &rest[1..]), Some(b'b' | b'B') => (-1, &rest[1..]), _ => (0, rest) };
    let note = (i32::from(octave.parse::<i16>().ok()?) + 2) * 12 + base + offset;
    u8::try_from(note).ok().filter(|n| *n < 128)
}
