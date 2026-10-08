//! Control-thread compilation of immutable assets and native mappings.
mod predicates;
mod selection;
use super::{Envelope, Error, Frame, NotePitch, Playback};
use predicates::Matching;
pub use predicates::{AXIS_BASE, ControllerCondition, MAX_AXES, PREVIOUS_KEY};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering::Relaxed};
use sampler_pool::{Snapshot, SnapshotRead};

/// Immutable decoded-asset metadata with optional resident PCM. Construct, clone and drop on the
/// control side. Clones share the original sample buffer; rendering only borrows.
#[derive(Clone, Debug)]
pub struct Pcm(std::sync::Arc<PcmData>);

/// Process-local identity of one immutable decoded asset revision. Clones share it;
/// a separately constructed revision never reuses an old identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(u64);

type StoreEntry = ([i32; super::STORE_KEY], i64);

#[derive(Debug)]
struct PcmData {
    id: AssetId,
    rate: u32,
    frames: Option<Box<[Frame]>>,
    // Pre-decimated octave levels (level 1 first), built by `Pcm::mipmapped`
    // or `service_mipmaps`. Audio borrows an immutable published generation.
    levels: Levels,
    // Deepest octave voices asked for, and the runtime clock of the last
    // render that read this asset.
    wanted: AtomicU8,
    used: AtomicU64,
    // A streamed asset's resident ranges (where voices start); empty when purged.
    /// Readers (every render thread) share it; only a control-side swap writes.
    head: Snapshot<Ranges>,
    // A start was refused because its pages were not resident.
    cold: AtomicBool,
    length: usize,
}
type LevelData = Box<[Box<[Frame]>]>;
/// Resident ranges of a streamed asset: (first frame, frames), ascending and
/// disjoint.
pub type Ranges = Box<[(usize, crate::Packed)]>;

/// Calls `f` with each part of `range` outside `ranges`, in order, until it
/// returns false; returns whether every call returned true.
pub(crate) fn uncovered(
    ranges: &[(usize, crate::Packed)],
    range: std::ops::Range<usize>,
    mut f: impl FnMut(std::ops::Range<usize>) -> bool,
) -> bool {
    let mut at = range.start;
    let mut i = ranges.partition_point(|(start, frames)| start + frames.len() <= at);
    while at < range.end {
        match ranges.get(i) {
            Some((start, frames)) if *start <= at => at = start + frames.len(),
            next => {
                let end = next.map_or(range.end, |(start, _)| (*start).min(range.end));
                if !f(at..end) {
                    return false;
                }
                at = end;
            }
        }
        i += usize::from(ranges.get(i).is_some_and(|(s, f)| s + f.len() <= at));
    }
    true
}
type Levels = Snapshot<LevelData>;
impl Pcm {
    /// Validate once without copying the owned frame buffer. All public access is
    /// immutable, so subsequent prepared plans need not rescan sample contents.
    pub fn new(rate: u32, frames: Box<[Frame]>) -> Result<Self, Error> {
        if rate == 0 || frames.is_empty() || frames.iter().flatten().any(|x| !x.is_finite()) {
            return Err(Error::InvalidInput);
        }
        Self::create(rate, frames.len(), Some(frames))
    }
    /// `new` plus every octave level, built here (about +100% memory). Prefer
    /// `service_mipmaps`, which builds levels only for assets played an octave
    /// or more up, within a budget.
    /// Voices pitched up by an octave or more resample from the coarsest level
    /// at or below their step, so the kernel stays under two octaves wide
    /// instead of stretching to the full step. Windows that touch a loop seam,
    /// crossfade or view edge read the original frames.
    pub fn mipmapped(rate: u32, frames: Box<[Frame]>) -> Result<Self, Error> {
        let pcm = Self::new(rate, frames)?;
        let depth = crate::resample::OCTAVES;
        pcm.0.levels.replace(crate::resample::octaves(pcm.0.frames.as_deref().unwrap(), depth), |_| ());
        pcm.0.wanted.store(depth as u8, Relaxed);
        Ok(pcm)
    }
    /// Metadata for worker-decoded pages. The worker registry must resolve this
    /// revision's ID to the corresponding immutable decoded source.
    pub fn streamed(rate: u32, frames: usize) -> Result<Self, Error> {
        if rate == 0 || frames == 0 {
            return Err(Error::InvalidInput);
        }
        Self::create(rate, frames, None)
    }
    /// `streamed` with its first frames resident, so starts need no page.
    pub fn headed(rate: u32, frames: usize, head: &[Frame]) -> Result<Self, Error> {
        let pcm = Self::streamed(rate, frames)?;
        pcm.set_ranges(vec![(0, head.into())])?;
        Ok(pcm)
    }
    /// Control side: replace a streamed asset's resident frame ranges,
    /// typically the first frames of each zone start, so starts need no cache
    /// page; empty purges them. Ranges ascend, disjoint and non-empty. Returns
    /// a borrowed old generation; only subsequent control-side collection frees it. A start that finds its frames
    /// missing fails `NotReady` and marks the asset cold (`take_cold`).
    /// Frames are kept packed ([`crate::Packed`]): 16/24-bit, mono when both
    /// channels match, whenever that reads back bit-exactly.
    pub fn set_ranges(&self, ranges: Vec<(usize, Box<[Frame]>)>) -> Result<SnapshotRead<'_, Ranges>, Error> {
        let valid = self.0.frames.is_none()
            && ranges.windows(2).all(|w| w[0].0 + w[0].1.len() <= w[1].0)
            && ranges.iter().all(|(start, frames)| {
                !frames.is_empty()
                    && start
                        .checked_add(frames.len())
                        .is_some_and(|end| end <= self.0.length)
                    && frames.iter().flatten().all(|x| x.is_finite())
            });
        if !valid {
            return Err(Error::InvalidInput);
        }
        let ranges = ranges
            .into_iter()
            .map(|(start, frames)| (start, crate::Packed::new(&frames)))
            .collect();
        self.0.head.collect();
        Ok(self.0.head.swap(ranges))
    }
    /// Control side: frames in resident ranges, collecting retired generations.
    pub fn head_frames(&self) -> usize {
        self.0.head.collect();
        self.0.head.read().iter().map(|(_, f)| f.len()).sum()
    }
    /// Control side: packed bytes held by the current resident ranges.
    pub fn head_bytes(&self) -> usize {
        self.0.head.collect();
        self.0.head.read().iter().map(|(_, f)| f.bytes()).sum()
    }
    /// Whether a start was refused for a missing head since the last call.
    pub fn take_cold(&self) -> bool {
        self.0.cold.swap(false, Relaxed)
    }
    /// Runtime clock at the end of the last render that read this asset (0 if
    /// never).
    pub fn last_played(&self) -> u64 {
        self.0.used.load(Relaxed)
    }
    /// Audio side: an immutable generation, never missing due to publication.
    pub(crate) fn try_head(&self) -> Option<SnapshotRead<'_, Ranges>> {
        Some(self.0.head.read())
    }
    pub fn mark_cold(&self) {
        self.0.cold.store(true, Relaxed);
    }
    pub(crate) fn touch(&self, now: u64) {
        self.0.used.store(now, Relaxed);
    }
    fn create(rate: u32, length: usize, frames: Option<Box<[Frame]>>) -> Result<Self, Error> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ASSET: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let id = NEXT_ASSET
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        Ok(Self(std::sync::Arc::new(PcmData {
            id: AssetId(id),
            rate,
            frames,
            levels: Levels::default(),
            wanted: AtomicU8::new(0),
            used: AtomicU64::new(0),
            head: Snapshot::default(),
            cold: AtomicBool::new(false),
            length,
        })))
    }
    pub fn asset_id(&self) -> AssetId {
        self.0.id
    }
    pub fn sample_rate(&self) -> u32 {
        self.0.rate
    }
    pub fn frame_count(&self) -> usize {
        self.0.length
    }
    pub fn resident_frames(&self) -> Option<&[Frame]> {
        self.0.frames.as_deref()
    }
    /// Audio side: an immutable octave generation, even during publication.
    pub(crate) fn try_levels(&self) -> Option<SnapshotRead<'_, LevelData>> {
        Some(self.0.levels.read())
    }
    /// Audio side: record that a voice reads this asset at `step` at `now`.
    pub(crate) fn want_levels(&self, step: f64, now: u64) {
        if step >= 2.0 && self.0.frames.is_some() {
            let depth = (step.log2() as usize).min(crate::resample::OCTAVES);
            self.0.wanted.fetch_max(depth as u8, Relaxed);
            self.0.used.store(now, Relaxed);
        }
    }
    fn level_bytes(&self) -> usize {
        self.0.levels.collect();
        let levels = self.0.levels.read();
        levels.iter().map(|l| l.len()).sum::<usize>() * size_of::<Frame>()
    }
    /// Control side: bytes of resident frames, octave levels and any streamed head.
    pub fn resident_bytes(&self) -> usize {
        (self.0.frames.as_ref().map_or(0, |f| f.len()) + self.head_frames()) * size_of::<Frame>()
            + self.level_bytes()
    }
}

/// Bytes of octave levels 1..=depth for `frames` frames.
fn levels_size(frames: usize, depth: usize) -> usize {
    (1..=depth).map(|k| frames.div_ceil(1 << k)).sum::<usize>() * size_of::<Frame>()
}

/// Control side, off the audio thread (each UI tick, say): build octave levels
/// for resident assets that voices played an octave or more up, most recently
/// played first, within `budget` bytes of levels across `assets`. Room comes
/// from evicting the least recently played levels, only those idle at least
/// `idle` frames (runtime clock) longer than the asset needing room, so two
/// busy assets never evict each other back and forth. Without levels a voice
/// is still correct, through a wider kernel. Returns the level bytes held.
pub fn service_mipmaps(assets: &[Pcm], budget: usize, idle: u64) -> usize {
    let mut held: usize = assets.iter().map(Pcm::level_bytes).sum();
    // ponytail: quadratic victim search; an ordered index if asset counts and
    // pass rates make it show up.
    let mut wanting: Vec<&Pcm> = assets
        .iter()
        .filter(|p| {
            let depth = p.0.levels.read().len();
            usize::from(p.0.wanted.load(Relaxed)) > depth
        })
        .collect();
    wanting.sort_by_key(|p| std::cmp::Reverse(p.0.used.load(Relaxed)));
    for pcm in wanting {
        let (frames, depth) = (pcm.0.frames.as_deref().unwrap(), pcm.0.wanted.load(Relaxed));
        let need = levels_size(frames.len(), depth.into()).saturating_sub(pcm.level_bytes());
        let used = pcm.0.used.load(Relaxed);
        while held + need > budget {
            let victim = assets
                .iter()
                .filter(|p| p.0.used.load(Relaxed).saturating_add(idle) <= used)
                .filter(|p| p.level_bytes() > 0)
                .min_by_key(|p| p.0.used.load(Relaxed));
            let Some(victim) = victim else { break };
            held -= victim.level_bytes();
            victim.0.levels.replace(Box::new([]), |_| ());
        }
        if held + need <= budget {
            let built = crate::resample::octaves(frames, depth.into());
            pcm.0.levels.replace(built, |_| ());
            held += need;
        }
    }
    // A lowered budget evicts least recently played levels regardless of idle.
    while held > budget {
        let victim = assets
            .iter()
            .filter(|p| p.level_bytes() > 0)
            .min_by_key(|p| p.0.used.load(Relaxed))
            .expect("held bytes belong to some asset");
        held -= victim.level_bytes();
        victim.0.levels.replace(Box::new([]), |_| ());
    }
    held
}

/// Native tuning offsets in semitones from the nominal 12-tone equal-tempered
/// pitch of each logical key. Zero preserves authored tuning. Compile off audio;
/// adoption affects new roots, while existing notes and children keep their plan.
#[derive(Clone, Debug)]
pub struct Tuning([f64; 128]);

impl Tuning {
    pub fn new(offsets_semitones: [f64; 128]) -> Result<Self, Error> {
        if offsets_semitones.iter().any(|offset| !offset.is_finite()) {
            return Err(Error::InvalidInput);
        }
        Ok(Self(offsets_semitones))
    }

    pub fn offsets_semitones(&self) -> &[f64; 128] {
        &self.0
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self([0.0; 128])
    }
}

/// Native resident region with optional tuned root-key tracking.
/// PCM can be shared by regions with independent ranges, directions and loops.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub sample: usize,
    pub key_low: u8,
    pub key_high: u8,
    /// Nominal recorded root key. Tracking includes the played key's tuning offset;
    /// None keeps the source at its authored pitch and bypasses the tuning table.
    pub root_key: Option<u8>,
    pub velocity_low: f64,
    pub velocity_high: f64,
    pub gain: f32,
    pub envelope: Envelope,
    pub playback: Playback,
}

/// Velocity-to-amplitude response, independent of velocity selection and note data.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VelocityCurve {
    Constant,
    #[default]
    Linear,
    /// Positive finite exponent of normalized velocity. Applied once per voice.
    Power(f64),
}

impl PreparedRegion {
    /// Crossfade gain for a note at `key` and normalised `velocity`.
    pub(super) fn fade_gain(&self, key: u8, velocity: f64) -> f32 {
        let f = self.fades;
        if f == ZoneFades::default() {
            return 1.0;
        }
        let v = (velocity * 127.0).round().clamp(0.0, 127.0) as u8;
        let [kl, kh, vl, vh] = self.bounds;
        ramp(key, kl, kh, f.key_in, f.key_out) * ramp(v, vl, vh, f.velocity_in, f.velocity_out)
    }
}

impl VelocityCurve {
    fn amplitude(self, velocity: f64) -> f32 {
        match self {
            Self::Constant => 1.0,
            Self::Linear => velocity as f32,
            Self::Power(exponent) => velocity.powf(exponent) as f32,
        }
    }
}

/// Linear zone crossfades inside a region's key and velocity ranges (see
/// `sampler_ir::Fades`): gain `(v - low + 1) / (fade + 1)` rising over `fade`
/// steps from the low edge and the mirror falling to the high edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZoneFades {
    pub velocity_in: u8,
    pub velocity_out: u8,
    pub key_in: u8,
    pub key_out: u8,
}

fn ramp(v: u8, low: u8, high: u8, fade_in: u8, fade_out: u8) -> f32 {
    let (v, low, high) = (i32::from(v), i32::from(low), i32::from(high));
    let mut gain = 1.0;
    let (fade_in, fade_out) = (i32::from(fade_in), i32::from(fade_out));
    if fade_in > 0 && v - low < fade_in {
        gain = ((v - low + 1).max(0) as f32) / (fade_in + 1) as f32;
    }
    if fade_out > 0 && high - v < fade_out {
        gain *= ((high - v + 1).max(0) as f32) / (fade_out + 1) as f32;
    }
    gain
}

/// Render-ready region: authoring bounds and playback metadata have been compiled.
#[derive(Clone, Copy)]
struct PreparedRegion {
    sample: usize,
    velocity_low: f64,
    velocity_high: f64,
    gain: f32,
    velocity_curve: VelocityCurve,
    fades: ZoneFades,
    /// key low, key high, velocity low, velocity high.
    bounds: [u8; 4],
    chain: Option<usize>,
    bus: Option<usize>,
    envelope: Envelope,
    cursor: super::source::Cursor,
    root_key: Option<u8>,
    transpose_semitones: f64,
    take: Option<super::Take>,
    trigger: super::Trigger,
    articulation: Option<u32>,
    conditions: Option<usize>,
}

#[derive(Clone, Copy)]
struct Candidate {
    region: usize,
    step: f64,
}

pub struct Prepared {
    pub(super) rate: u32,
    pub(super) pcm: Box<[Pcm]>,
    pub(super) buses: super::bus::PreparedBuses,
    pub(super) impulses: Vec<std::sync::Arc<super::dsp::Impulse>>,
    pub(super) voice_chains: Box<[super::dsp::PreparedVoiceChain]>,
    pub(super) filters: Box<[super::dsp::svf::PreparedFilter]>,
    pub(super) dsp_bindings: Box<[super::ControlRange]>,
    pub(super) dsp_controls: Box<[(super::ControlId, usize)]>,
    regions: Box<[PreparedRegion]>,
    pub(super) group_count: u32,
    pub(super) source_event_limit: i32,
    /// CC64 holds no gate: a behavior implements sustain itself.
    pub(super) script_sustain: bool,
    pub(super) script_release_triggers: bool,
    /// Controller values a script set in `on init`, before any input.
    pub(super) initial_controllers: Vec<(u8, u32)>,
    pub(super) region_groups: Box<[Option<u32>]>,
    pub(super) region_zone_ids: Box<[u32]>,
    pub(super) native_start: Box<[Box<[sampler_ir::GroupStart]>]>,
    pub(super) native_articulation_keys: Box<[Option<u8>]>,
    pub(super) native_default_key: Option<u8>,
    pub(super) native_rr_length: u32,
    pub(super) native_random_groups: Box<[u32]>,
    pub(super) voice_limit: Option<super::VoiceLimit>,
    pub(super) voice_limits: Box<[super::VoiceLimit]>,
    pub(super) group_voice_limits: Box<[Option<usize>]>,
    pub(super) monophonic_release: Box<[bool]>,
    pub(super) engine_parameters: Box<[super::EngineParameterBinding]>,
    pub(super) envelope_controls: Box<[[Option<super::ControlId>; 6]]>,
    pub(super) engine_lookups: Box<[super::EngineLookup]>,
    pub(super) group_params: Box<[super::GroupParams]>,
    pub(super) group_faders: Box<[Option<super::GroupFader>]>,
    /// Bus index by source address, for script group routing.
    pub(super) bus_addresses: Box<[(i32, usize)]>,
    // Boxed: a Prepared moves by value through every builder, and inline
    // tables of this size made each debug frame hundreds of kilobytes.
    offsets: Box<[usize; 129]>,
    phase_offsets: Box<[[usize; 2]; 128]>,
    pub(super) release_options: [super::ReleaseOptions; 2],
    pub(super) release_reserves: Box<[[super::ReleaseReserve; 2]; 128]>,
    candidates: Box<[Candidate]>,
    pub(super) programs: Box<[super::Program]>,
    pub(super) script_initial: Box<[super::ops::ScriptBank]>,
    pub(super) stages: Box<[super::Stage]>,
    pub(super) note_cells: usize,
    pub(super) automation: Box<[super::AutomationBinding]>,
    pub(super) widgets: Box<[super::WidgetDefinition]>,
    pub(super) controls: Box<[super::ControlDefinition]>,
    pub(super) control_programs: Box<[super::ControlCallback]>,
    pub(super) plan_programs: Box<[super::PlanProgram]>,
    pub(super) signal_programs: Box<[super::SignalProgram]>,
    pub(super) shared_store: (Box<[StoreEntry]>, usize),
    pub(super) keyswitches: Box<[Option<u32>; 128]>,
    articulated: bool,
    pub(super) switching: super::Switching,
    bend_range: f64,
    conditions: Box<[Box<[ControllerCondition]>]>,
    /// Some condition reads [`PREVIOUS_KEY`], so attacks record it.
    tracks_previous: bool,
    condition_ends: Box<[usize]>,
    pub(super) release_selection: [super::SelectionPolicy; 2],
    pub(super) modulation: super::Modulation,
    pub(super) voice_modulation: super::voice_mod::VoiceModulation,
    pub(super) sequences: Box<[super::variation::PreparedSequence]>,
    pub(super) sequence_cells: usize,
    pub(super) shuffle_entries: usize,
}

impl Prepared {
    /// One response per authored region, in its original order. Selection always
    /// uses the unmodified velocity; this only changes the admitted voice gain.
    pub fn with_velocity_curves(mut self, curves: Vec<VelocityCurve>) -> Result<Self, Error> {
        if curves.len() != self.regions.len()
            || curves.iter().any(|curve| {
                matches!(curve, VelocityCurve::Power(exponent) if !exponent.is_finite() || *exponent <= 0.0)
            })
        {
            return Err(Error::InvalidInput);
        }
        for (region, curve) in self.regions.iter_mut().zip(curves) {
            region.velocity_curve = curve;
        }
        Ok(self)
    }

    /// One crossfade set per authored region, in its original order.
    pub fn with_zone_fades(mut self, fades: Vec<ZoneFades>) -> Result<Self, Error> {
        if fades.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        for (region, fades) in self.regions.iter_mut().zip(fades) {
            region.fades = fades;
        }
        Ok(self)
    }

    /// Takes immutable PCM handles without copying/rescanning their frames. Region
    /// validation and candidate construction happen off audio. max_candidates
    /// bounds the expanded key index, not file IO.
    pub fn new(
        rate: u32,
        pcm: Vec<Pcm>,
        regions: Vec<Region>,
        max_candidates: usize,
    ) -> Result<Self, Error> {
        Self::new_tuned(rate, pcm, regions, max_candidates, &Tuning::default())
    }

    /// Compile a tuning table into key candidates, with no table lookup or pitch
    /// calculation added to rendering. The recorded root remains nominal: tuning
    /// a played root key changes its pitch too. Fixed-pitch regions are exempt.
    pub fn new_tuned(
        rate: u32,
        pcm: Vec<Pcm>,
        regions: Vec<Region>,
        max_candidates: usize,
        tuning: &Tuning,
    ) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let mut count = 0usize;
        let mut prepared_regions = Vec::new();
        for r in &regions {
            if r.sample >= pcm.len()
                || r.key_low > r.key_high
                || r.key_high >= 128
                || r.root_key.is_some_and(|key| key >= 128)
                || !r.velocity_low.is_finite()
                || !r.velocity_high.is_finite()
                || r.velocity_low < 0.0
                || r.velocity_high > 1.0
                || r.velocity_low > r.velocity_high
                || !r.gain.is_finite()
                || !(0.0..=1.0).contains(&r.gain)
            {
                return Err(Error::InvalidInput);
            }
            let mut playback = r.playback;
            if let Some(root) = r.root_key {
                playback.transpose_semitones +=
                    f64::from(r.key_low) - f64::from(root) + tuning.0[r.key_low as usize];
            }
            let cursor = playback.cursor(
                pcm[r.sample].frame_count(),
                pcm[r.sample].sample_rate(),
                rate,
            )?;
            count = count
                .checked_add(usize::from(r.key_high - r.key_low) + 1)
                .ok_or(Error::Capacity)?;
            if count > max_candidates {
                return Err(Error::Capacity);
            }
            prepared_regions.push(PreparedRegion {
                sample: r.sample,
                velocity_low: r.velocity_low,
                velocity_high: r.velocity_high,
                gain: r.gain,
                velocity_curve: VelocityCurve::Linear,
                fades: ZoneFades::default(),
                bounds: [
                    r.key_low,
                    r.key_high,
                    (r.velocity_low * 127.0).round() as u8,
                    (r.velocity_high * 127.0).round() as u8,
                ],
                chain: None,
                bus: None,
                envelope: r.envelope,
                cursor,
                root_key: r.root_key,
                transpose_semitones: r.playback.transpose_semitones,
                take: None,
                trigger: super::Trigger::Attack,
                articulation: None,
                conditions: None,
            });
        }
        let mut offsets = [0; 129];
        let mut candidates = Vec::with_capacity(count);
        for key in 0..128 {
            offsets[key as usize] = candidates.len();
            for (region, r) in regions.iter().enumerate() {
                if r.key_low <= key && key <= r.key_high {
                    let step = if let Some(root) = r.root_key {
                        let mut playback = r.playback;
                        playback.transpose_semitones +=
                            f64::from(key) - f64::from(root) + tuning.0[key as usize];
                        playback.step(pcm[r.sample].sample_rate(), rate)
                    } else {
                        prepared_regions[region].cursor.step()
                    };
                    if !(super::resample::MIN_STEP..=super::resample::MAX_STEP).contains(&step) {
                        return Err(Error::InvalidInput);
                    }
                    candidates.push(Candidate { region, step });
                }
            }
        }
        offsets[128] = candidates.len();
        Ok(Self {
            rate,
            pcm: pcm.into_boxed_slice(),
            buses: super::bus::PreparedBuses::default(),
            impulses: Vec::new(),
            voice_chains: Box::new([]),
            dsp_bindings: Box::new([]),
            filters: Box::new([]),
            dsp_controls: Box::new([]),
            regions: prepared_regions.into_boxed_slice(),
            group_count: 0,
            source_event_limit: i32::MAX,
            script_sustain: false,
            script_release_triggers: false,
            initial_controllers: Vec::new(),
            region_groups: Box::new([]),
            region_zone_ids: Box::new([]),
            native_start: Box::new([]),
            native_articulation_keys: Box::new([]),
            native_default_key: None,
            native_rr_length: 0,
            native_random_groups: Box::new([]),
            voice_limit: None,
            voice_limits: Box::new([]),
            group_voice_limits: Box::new([]),
            monophonic_release: Box::new([]),
            engine_parameters: Box::new([]),
            envelope_controls: Box::new([]),
            engine_lookups: Box::new([]),
            group_params: Box::new([]),
            group_faders: Box::new([]),
            bus_addresses: Box::new([]),
            phase_offsets: Box::new(std::array::from_fn(|key| [offsets[key + 1]; 2])),
            release_options: [super::ReleaseOptions::default(); 2],
            release_reserves: Box::new([[super::ReleaseReserve::default(); 2]; 128]),
            offsets: Box::new(offsets),
            candidates: candidates.into_boxed_slice(),
            programs: Box::new([]),
            script_initial: Box::new([]),
            stages: Box::new([]),
            note_cells: 0,
            automation: Box::new([]),
            widgets: Box::new([]),
            controls: Box::new([]),
            control_programs: Box::new([]),
            plan_programs: Box::new([]),
            signal_programs: Box::new([]),
            shared_store: (Box::new([]), 0),
            keyswitches: Box::new([None; 128]),
            articulated: false,
            switching: Default::default(),
            bend_range: 2.0,
            conditions: Box::new([]),
            tracks_previous: false,
            condition_ends: Box::new([]),
            release_selection: [super::SelectionPolicy::Onset; 2],
            modulation: super::Modulation::default(),
            voice_modulation: Default::default(),
            sequences: Box::new([]),
            sequence_cells: 0,
            shuffle_entries: 0,
        })
    }

    /// Replace the complete program table and optional note binding atomically on
    /// the control thread. Generated children select regions without re-entry.
    pub fn with_programs(
        mut self,
        mut programs: Vec<super::Program>,
        note_program: Option<usize>,
    ) -> Result<Self, Error> {
        if note_program
            .is_some_and(|index| programs.get(index).is_none_or(|p| p.requires_controller))
        {
            return Err(Error::InvalidInput);
        }
        self.validate_program_controls(&programs)?;
        self.validate_program_scripts(&programs)?;
        super::plan_programs::validate_starts(&programs)?;
        // One polyphonic bank per script instance, plus the unbound native bank.
        // Resolve offsets off audio; callback execution never scans other scripts.
        let bank = |p: &super::Program| p.script_instance.map_or(0, |id| usize::from(id.0) + 1);
        let mut bases = vec![0usize; self.script_initial.len() + 1];
        for p in &programs {
            bases[bank(p)] = bases[bank(p)].max(p.note_cells);
        }
        let mut cells = 0usize;
        for base in &mut bases {
            let width = *base;
            *base = cells;
            cells = cells.checked_add(width).ok_or(Error::Capacity)?;
        }
        for p in &mut programs {
            p.note_base = bases[bank(p)];
        }
        self.note_cells = cells;
        self.programs = programs.into_boxed_slice();
        self.stages = Box::new([super::Stage {
            note: note_program,
            ..super::Stage::default()
        }]);
        self.control_programs = Box::new([]);
        self.plan_programs = Box::new([]);
        self.signal_programs = Box::new([]);
        Ok(self)
    }

    /// Bind a physical-key release callback to triggered external inputs. It has
    /// an independently reserved continuation and may wait beyond gate closure.
    /// Replacing the complete program table clears this binding.
    pub fn with_release_program(mut self, program: usize) -> Result<Self, Error> {
        if !self.programs.get(program).is_some_and(|p| {
            !p.requires_controller && p.wait_lifetime == super::WaitLifetime::Callback
        }) {
            return Err(Error::InvalidInput);
        }
        if self.stages.is_empty() {
            self.stages = Box::new([super::Stage::default()]);
        }
        self.stages[0].release = Some(program);
        Ok(self)
    }

    /// Bind per-voice modulation programs: one optional program per authored
    /// region, and per region the source frames a full sample-start route spans.
    pub fn with_voice_modulation(
        mut self,
        programs: Vec<super::ModProgram>,
        regions: Vec<Option<usize>>,
        start_ranges: Vec<u32>,
    ) -> Result<Self, Error> {
        if regions.len() != self.regions.len()
            || programs
                .iter()
                .flat_map(|p| &p.routes)
                .any(|r| match r.target {
                    super::ModTarget::ProcessorCutoff(i)
                    | super::ModTarget::ProcessorResonance(i) => i as usize >= self.filters.len(),
                    _ => false,
                })
        {
            return Err(Error::InvalidInput);
        }
        self.voice_modulation =
            super::voice_mod::VoiceModulation::new(programs, regions, start_ranges)?;
        Ok(self)
    }

    /// Bind a validated note-scoped modulation program on the control thread.
    pub fn with_modulation(mut self, modulation: super::Modulation) -> Self {
        self.modulation = modulation;
        self
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub(crate) fn region_chain(&self, region: usize) -> Option<usize> {
        self.regions[region].chain
    }

    /// Required cells per logical note across all script-instance namespaces.
    pub fn note_cell_count(&self) -> usize {
        self.note_cells
    }
    /// Maximum integer register count required by any callback in this plan.
    pub fn behavior_local_count(&self) -> usize {
        self.programs
            .iter()
            .map(|program| program.locals)
            .max()
            .unwrap_or(0)
    }
    /// Most voices one key's release phase can start together.
    pub fn release_voices(&self) -> usize {
        self.release_reserves
            .iter()
            .flatten()
            .map(|r| r.voices)
            .max()
            .unwrap_or(0)
    }
    /// Convolution processors across the bus chains, in bus then processor
    /// order: the slots [`crate::Runtime::swap_convolution`] addresses.
    pub fn convolution_slots(&self) -> usize {
        self.buses.convolution_slots()
    }
    /// Script modules a note passes through, in order.
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
    /// Bytes one voice slot costs under this plan: the voice itself, its chain
    /// and delay state, and its modulation state. Control side, for sizing
    /// `Limits::voices` against a memory budget.
    pub fn voice_state_bytes(&self) -> usize {
        use std::mem::size_of;
        let stages = self
            .voice_chains
            .iter()
            .map(|c| c.stages())
            .max()
            .unwrap_or(0);
        let delay = self
            .voice_chains
            .iter()
            .map(|c| c.delay_frames)
            .max()
            .unwrap_or(0);
        size_of::<crate::Slot<crate::Voice>>()
            + stages * size_of::<crate::dsp::ProcessorState>()
            + delay * size_of::<[f64; 2]>()
            + self.voice_modulation.bytes_per_voice()
    }
    pub fn sample_count(&self) -> usize {
        self.pcm.len()
    }
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
    /// Resolve the positive source zone ID used by EventInfo::ZoneId, retaining source holes.
    /// Control-side lookup; zero, omitted zones and absent source maps return None.
    /// No fallback to runtime region ordinals.
    pub fn source_zone_region(&self, zone_id: u32) -> Option<usize> {
        if zone_id == 0 {
            return None;
        }
        self.region_zone_ids.iter().position(|&id| id == zone_id)
    }

    /// The immutable prepared sample asset behind a region, for control-side peak work.
    pub fn region_asset(&self, region: usize) -> Option<&Pcm> {
        self.regions.get(region).and_then(|r| self.pcm.get(r.sample))
    }

    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    fn step(&self, candidate: Candidate, pitch: NotePitch) -> f64 {
        let r = &self.regions[candidate.region];
        match (pitch, r.root_key) {
            (NotePitch::Absolute(pitch), Some(root)) => {
                let semitones = r.transpose_semitones + (pitch - f64::from(root));
                f64::from(self.pcm[r.sample].sample_rate()) / f64::from(self.rate)
                    * super::pitch::ratio(semitones)
            }
            _ => candidate.step,
        }
    }

    /// Attach one optional take to each region in its original authoring order.
    /// Candidates are grouped by sequence without expanding a key/channel/take
    /// Cartesian product. Every sequence advances once per accepted eligible note,
    /// including a deliberately unmapped take; completely unmatched notes do not.
    /// `max_states` bounds reserved scope cells; `max_shuffle_entries` bounds the
    /// total u32 bag entries (reserved owners times takes for shuffle sequences).
    pub fn with_variation(
        mut self,
        sequences: Vec<super::Sequence>,
        region_takes: Vec<Option<super::Take>>,
        max_states: usize,
        max_shuffle_entries: usize,
    ) -> Result<Self, Error> {
        if region_takes.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        let mut prepared = Vec::with_capacity(sequences.len());
        let mut cells = 0usize;
        let mut entries = 0usize;
        for mut spec in sequences {
            if spec.takes == 0
                || spec.capacity == 0
                || (matches!(spec.policy, super::TakePolicy::NoRepeat { .. }) && spec.takes < 2)
            {
                return Err(Error::InvalidInput);
            }
            spec.capacity = match spec.scope {
                super::SequenceScope::Global => 1,
                super::SequenceScope::Key => spec.capacity.min(128),
                _ => spec.capacity,
            };
            let offset = cells;
            cells = cells.checked_add(spec.capacity).ok_or(Error::Capacity)?;
            if cells > max_states {
                return Err(Error::Capacity);
            }
            let shuffle_offset = entries;
            if matches!(spec.policy, super::TakePolicy::Shuffle { .. }) {
                let count = spec
                    .capacity
                    .checked_mul(spec.takes as usize)
                    .ok_or(Error::Capacity)?;
                entries = entries.checked_add(count).ok_or(Error::Capacity)?;
                if entries > max_shuffle_entries {
                    return Err(Error::Capacity);
                }
            }
            prepared.push(super::variation::PreparedSequence {
                spec,
                offset,
                shuffle_offset,
            });
        }
        super::variation::SequenceState::check_size(cells, entries)?;
        for (r, take) in self.regions.iter_mut().zip(region_takes) {
            if let Some(take) = take
                && prepared
                    .get(take.sequence)
                    .is_none_or(|s| take.index >= s.spec.takes)
            {
                return Err(Error::InvalidInput);
            }
            r.take = take;
        }
        self.sequences = prepared.into_boxed_slice();
        self.sequence_cells = cells;
        self.shuffle_entries = entries;
        self.compile_selection();
        Ok(self)
    }

    /// Attach native attack/key-release/gate-release phases in authoring order.
    /// Release families choose their own takes and have independent source lifetimes.
    pub fn with_releases(
        mut self,
        triggers: Vec<super::Trigger>,
        key_release: super::ReleaseOptions,
        gate_release: super::ReleaseOptions,
    ) -> Result<Self, Error> {
        if triggers.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        for options in [key_release, gate_release] {
            if let super::ReleaseVelocity::KeyUp { fallback } = options.velocity {
                super::release::validate_velocity(Some(fallback))?;
            }
        }
        self.release_options = [key_release, gate_release];
        for (region, trigger) in self.regions.iter_mut().zip(triggers) {
            if let Some(index) = trigger.release_index()
                && self.release_options[index].duration.is_none()
                && region.cursor.unbounded_loop()
                && !region.envelope.finite()
            {
                return Err(Error::InvalidInput);
            }
            region.trigger = trigger;
        }
        self.compile_selection();
        Ok(self)
    }

    /// Tag regions in original authoring order. None is an unconditional layer.
    /// Policies independently choose onset/current articulation for key/gate releases.
    pub fn with_articulations(
        mut self,
        regions: Vec<Option<u32>>,
        switches: Vec<super::Keyswitch>,
        key_release: super::SelectionPolicy,
        gate_release: super::SelectionPolicy,
    ) -> Result<Self, Error> {
        if regions.len() != self.regions.len() {
            return Err(Error::InvalidInput);
        }
        let mut keyswitches = [None; 128];
        for switch in switches {
            let slot = keyswitches
                .get_mut(usize::from(switch.key))
                .ok_or(Error::InvalidInput)?;
            if slot.replace(switch.articulation).is_some() {
                return Err(Error::InvalidInput);
            }
        }
        self.articulated = regions.iter().any(Option::is_some);
        for (region, value) in self.regions.iter_mut().zip(regions) {
            region.articulation = value;
        }
        self.keyswitches = Box::new(keyswitches);
        self.release_selection = [key_release, gate_release];
        self.compile_selection();
        Ok(self)
    }

    /// Current release state is unknown until its actual transition.
    pub(super) fn pending_selection<'a>(
        &self,
        trigger: super::Trigger,
        onset: &'a super::performance::State,
    ) -> Option<&'a super::performance::State> {
        match self.release_selection[trigger.release_index().unwrap()] {
            super::SelectionPolicy::Onset => Some(onset),
            super::SelectionPolicy::Current => None,
        }
    }

    // Two sparse ranges per sequence: unconditional regions plus the selected tag.
    // Do not expand keys x articulations x takes x microphones into another table.
    fn active_ranges(
        &self,
        range: std::ops::Range<usize>,
        articulation: u32,
    ) -> [std::ops::Range<usize>; 2] {
        if !self.articulated {
            return [range, 0..0];
        }
        let candidates = &self.candidates[range.clone()];
        let common = candidates.partition_point(|c| self.regions[c.region].articulation.is_none());
        let begin = candidates
            .partition_point(|c| self.regions[c.region].articulation < Some(articulation));
        let end = candidates
            .partition_point(|c| self.regions[c.region].articulation <= Some(articulation));
        [
            range.start..range.start + common,
            range.start + begin..range.start + end,
        ]
    }

    fn compile_selection(&mut self) {
        for key in 0..128 {
            let begin = self.offsets[key];
            let end = self.offsets[key + 1];
            let candidates = &mut self.candidates[begin..end];
            candidates.sort_by_key(|c| {
                let r = self.regions[c.region];
                (
                    r.trigger,
                    r.take.map(|t| t.sequence),
                    r.articulation,
                    r.conditions,
                    c.region,
                )
            });
            self.phase_offsets[key] = [
                begin
                    + candidates.partition_point(|c| {
                        self.regions[c.region].trigger == super::Trigger::Attack
                    }),
                begin
                    + candidates.partition_point(|c| {
                        self.regions[c.region].trigger != super::Trigger::GateRelease
                    }),
            ];
            for trigger in [super::Trigger::KeyRelease, super::Trigger::GateRelease] {
                self.release_reserves[key][trigger.release_index().unwrap()] =
                    self.release_bound(key as u8, trigger);
            }
        }
        if self.conditions.is_empty() {
            self.condition_ends = Box::new([]);
        } else {
            let mut ends = vec![0; self.candidates.len()];
            let mut end = ends.len();
            let mut previous = None;
            for i in (0..ends.len()).rev() {
                let condition = self.regions[self.candidates[i].region].conditions;
                if previous != Some(condition) {
                    end = i + 1;
                    previous = Some(condition);
                }
                ends[i] = end;
            }
            self.condition_ends = ends.into_boxed_slice();
        }
    }

    fn range(&self, key: u8, trigger: super::Trigger) -> std::ops::Range<usize> {
        let key = usize::from(key);
        match trigger {
            super::Trigger::Attack => self.offsets[key]..self.phase_offsets[key][0],
            super::Trigger::KeyRelease => self.phase_offsets[key][0]..self.phase_offsets[key][1],
            super::Trigger::GateRelease => self.phase_offsets[key][1]..self.offsets[key + 1],
        }
    }

    fn release_bound(&self, key: u8, trigger: super::Trigger) -> super::ReleaseReserve {
        use std::collections::BTreeMap;
        // Control-only sweep: closed velocity intervals overlap at shared endpoints.
        // Track each take's overlap and each group's maximum, without expanding
        // key x velocity x take x microphone products or scanning every velocity.
        #[derive(Default)]
        struct TakeOverlap {
            common: usize,
            counts: BTreeMap<u32, usize>,
            levels: BTreeMap<usize, usize>,
        }
        fn change(levels: &mut BTreeMap<usize, usize>, old: usize, new: usize) {
            if old > 0 {
                let frequency = levels.get_mut(&old).unwrap();
                *frequency -= 1;
                if *frequency == 0 {
                    levels.remove(&old);
                }
            }
            if new > 0 {
                *levels.entry(new).or_default() += 1;
            }
        }
        struct Group {
            counts: BTreeMap<u32, TakeOverlap>,
            levels: BTreeMap<usize, usize>,
            sequenced: bool,
        }
        let range = self.range(key, trigger);
        let mut events = Vec::with_capacity(range.len() * 2);
        let mut groups = Vec::new();
        let mut from = range.start;
        while from < range.end {
            let until = self.group_end(from, range.end);
            for candidate in &self.candidates[from..until] {
                let r = self.regions[candidate.region];
                events.push((
                    r.velocity_low,
                    true,
                    groups.len(),
                    r.take.map_or(0, |t| t.index),
                    r.articulation,
                ));
                events.push((
                    r.velocity_high,
                    false,
                    groups.len(),
                    r.take.map_or(0, |t| t.index),
                    r.articulation,
                ));
            }
            groups.push(Group {
                counts: BTreeMap::new(),
                levels: BTreeMap::new(),
                sequenced: self.regions[self.candidates[from].region].take.is_some(),
            });
            from = until;
        }
        events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then_with(|| b.1.cmp(&a.1)));
        let mut active = super::ReleaseReserve::default();
        let mut maximum = active;
        for (_, start, group, take, articulation) in events {
            let group = &mut groups[group];
            let old_maximum = group.levels.last_key_value().map_or(0, |(&level, _)| level);
            let take = group.counts.entry(take).or_default();
            let old = take.common + take.levels.last_key_value().map_or(0, |(&n, _)| n);
            if let Some(articulation) = articulation {
                let count = take.counts.entry(articulation).or_default();
                let before = *count;
                *count = if start { before + 1 } else { before - 1 };
                change(&mut take.levels, before, *count);
            } else {
                take.common = if start {
                    take.common + 1
                } else {
                    take.common - 1
                };
            }
            let new = take.common + take.levels.last_key_value().map_or(0, |(&n, _)| n);
            change(&mut group.levels, old, new);
            let new_maximum = group.levels.last_key_value().map_or(0, |(&level, _)| level);
            active.voices = active.voices - old_maximum + new_maximum;
            if old_maximum == 0 && new_maximum != 0 {
                active.families += 1;
                active.decisions += usize::from(group.sequenced);
            } else if new_maximum == 0 && old_maximum != 0 {
                active.families -= 1;
                active.decisions -= usize::from(group.sequenced);
            }
            active.commands = if self.release_options[trigger.release_index().unwrap()]
                .duration
                .is_some_and(|n| n > 0)
            {
                active.families
            } else {
                0
            };
            maximum = maximum.maximum(active);
        }
        if !self.conditions.is_empty() {
            maximum.voices = maximum
                .voices
                .min(self.controller_release_bound(key, trigger));
        }
        maximum
    }

    pub(super) fn validate_release_pitch(
        &self,
        pitch: NotePitch,
        trigger: super::Trigger,
        range: super::pitch::PitchRange,
        velocity: Option<f64>,
        state: Option<&super::performance::State>,
    ) -> Result<(), Error> {
        let candidates = self.range(pitch.key(), trigger);
        let mut from = candidates.start;
        while from < candidates.end {
            let until = self.group_end(from, candidates.end);
            let ranges = state.map_or([from..until, 0..0], |state| {
                self.active_ranges(from..until, state.articulation)
            });
            let mut matching = Matching::new(ranges);
            while let Some(candidate) = matching.next(self, state, velocity) {
                range.apply(self.step(candidate, pitch))?;
            }
            from = until;
        }
        Ok(())
    }

    // None means physical release velocity is not known yet.
    pub(super) fn release_velocity(
        &self,
        trigger: super::Trigger,
        onset: f64,
        key_released: bool,
        velocity: Option<f64>,
    ) -> Option<f64> {
        match self.release_options[trigger.release_index().unwrap()].velocity {
            super::ReleaseVelocity::Onset => Some(onset),
            super::ReleaseVelocity::KeyUp { fallback } => {
                key_released.then_some(velocity.unwrap_or(fallback))
            }
        }
    }

    fn sequence_groups<'a>(
        &'a self,
        key: u8,
        trigger: super::Trigger,
        state: Option<&'a super::performance::State>,
        velocity: Option<f64>,
    ) -> impl Iterator<Item = usize> + 'a {
        let mut range = self.range(key, trigger);
        std::iter::from_fn(move || {
            if range.is_empty() {
                return None;
            }
            let until = self.group_end(range.start, range.end);
            let ranges = state.map_or([range.start..until, 0..0], |state| {
                self.active_ranges(range.start..until, state.articulation)
            });
            let eligible = Matching::new(ranges).next(self, state, velocity).is_some();
            let sequence = self.regions[self.candidates[range.start].region]
                .take
                .map(|t| t.sequence)
                .filter(|_| eligible);
            range.start = until;
            Some(sequence)
        })
        .flatten()
    }

    fn group_end(&self, begin: usize, end: usize) -> usize {
        if self.sequences.is_empty() {
            return end;
        }
        let sequence = self.regions[self.candidates[begin].region]
            .take
            .map(|t| t.sequence);
        begin
            + self.candidates[begin..end]
                .partition_point(|c| self.regions[c.region].take.map(|t| t.sequence) == sequence)
    }

    fn choose(
        &self,
        first: Option<Candidate>,
        address: super::ChannelAddress,
        key: u8,
        state: &super::variation::SequenceState,
    ) -> Result<Option<super::variation::PendingTake>, Error> {
        let Some(take) = first.and_then(|c| self.regions[c.region].take) else {
            return Ok(None);
        };
        state
            .choose(take.sequence, &self.sequences[take.sequence], address, key)
            .map(Some)
    }
}

impl Prepared {
    /// Bind shared immutable processor chains in authored region order. Runtime
    /// state remains per voice, per channel and per retained generation.
    pub fn with_voice_chains(
        mut self,
        chains: Vec<super::VoiceChain>,
        bindings: Vec<Option<usize>>,
    ) -> Result<Self, Error> {
        if bindings.len() != self.regions.len()
            || bindings
                .iter()
                .flatten()
                .any(|index| *index >= chains.len())
        {
            return Err(Error::InvalidInput);
        }
        for (region, chain) in self.regions.iter_mut().zip(bindings) {
            region.chain = chain;
        }
        let mut parameters = Vec::new();
        let mut filters = Vec::new();
        self.voice_chains = chains
            .into_iter()
            .map(|chain| chain.compile(self.rate, &mut parameters, &mut filters))
            .collect::<Result<_, _>>()?;
        let mut controls: Vec<_> = parameters
            .iter()
            .enumerate()
            .map(|(lane, binding)| (binding.control, lane))
            .collect();
        controls.sort_unstable();
        self.dsp_controls = controls.into_boxed_slice();
        self.dsp_bindings = parameters.into_boxed_slice();
        self.filters = filters.into_boxed_slice();
        self.validate_dsp_controls()?;
        Ok(self)
    }
}

impl Prepared {
    /// The impulse responses [`super::Processor::Convolution`] indexes; set
    /// before [`Prepared::with_buses`].
    pub fn with_impulses(mut self, impulses: Vec<super::Impulse>) -> Self {
        self.impulses = impulses.into_iter().map(std::sync::Arc::new).collect();
        self
    }

    /// Bind each region to a bus, or directly to stereo output (`None`).
    /// Graph construction, cycle validation and coefficient compilation run off audio.
    pub fn with_buses(
        mut self,
        buses: Vec<super::Bus>,
        bindings: Vec<Option<usize>>,
    ) -> Result<Self, Error> {
        if bindings.len() != self.regions.len()
            || bindings.iter().flatten().any(|i| *i >= buses.len())
        {
            return Err(Error::InvalidInput);
        }
        self.buses = super::bus::PreparedBuses::new(self.rate, buses, &self.impulses)?;
        self.validate_dsp_controls()?;
        for (region, bus) in self.regions.iter_mut().zip(bindings) {
            region.bus = bus;
        }
        Ok(self)
    }
}

impl Prepared {
    /// Default channel pitch-bend range in semitones (either direction) for
    /// plain MIDI bend; an RPN 0 on the channel overrides it.
    pub fn with_bend_range(mut self, semitones: f64) -> Result<Self, Error> {
        if !(semitones.is_finite() && (0.0..=96.0).contains(&semitones)) {
            return Err(Error::InvalidInput);
        }
        self.bend_range = semitones;
        Ok(self)
    }

    pub fn bend_range(&self) -> f64 {
        self.bend_range
    }
}

impl super::Runtime {
    /// The active plan's default channel pitch-bend range in semitones.
    pub fn bend_range(&self) -> f64 {
        self.plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .bend_range
    }
}

#[cfg(test)]
mod fade_tests {
    use super::ramp;

    /// Kontakt 8 measurements: L=1/F=100 gives v/101; L=30/F=60 gives (v-29)/61;
    /// H=100/F=60 gives (101-v)/61.
    #[test]
    fn crossfade_ramps_start_one_step_in_and_mirror() {
        assert_eq!(ramp(1, 1, 127, 100, 0), 1.0 / 101.0);
        assert_eq!(ramp(50, 30, 127, 60, 0), 21.0 / 61.0);
        assert_eq!(ramp(90, 30, 127, 60, 0), 1.0);
        assert_eq!(ramp(40, 1, 100, 0, 60), 1.0);
        assert_eq!(ramp(100, 1, 100, 0, 60), 1.0 / 61.0);
        assert_eq!(ramp(80, 1, 100, 0, 60), 21.0 / 61.0);
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;

    #[test]
    fn publishing_residency_does_not_wait_for_an_audio_reader() {
        let pcm = Pcm::headed(48000, 128, &[[0.25; 2]; 64]).unwrap();
        let old = pcm.try_head().unwrap();
        let control = pcm.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let publisher = std::thread::spawn(move || {
            control.set_ranges(vec![(0, vec![[0.5; 2]; 64].into_boxed_slice())]).unwrap();
            tx.send(()).unwrap();
        });
        let published = rx.recv_timeout(std::time::Duration::from_millis(100)).is_ok();
        assert_eq!(old[0].1.frame(0), [0.25; 2]);
        drop(old);
        publisher.join().unwrap();
        assert!(published, "publication must not block on the audio reader");
        assert_eq!(pcm.try_head().unwrap()[0].1.frame(0), [0.5; 2]);
    }
}
