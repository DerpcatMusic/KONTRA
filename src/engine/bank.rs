//! Immutable instrument data prepared for playback.

use super::{
    map::{LoopMap, PlayMap},
    filter::GroupFilter,
    params::ModTable,
    residency::{self, Residency},
    stream::Streamer,
    voice::{Ahdsr, Flex, FlexPoint},
};
use crate::{
    audio::{self, Frame, Pcm, Sample, SampleReader, Source},
    import::{Group, Instrument, VoiceLimit, Zone},
};
use anyhow::{Result, bail};
use std::{
    collections::HashMap,
    num::NonZero,
    ops::Range,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};

/// Default resident sample memory budget per bank. A bank never fails on
/// it: [`Builder::plan`] trades preload and resident start-offset range for
/// streaming until the bank fits.
pub const MEMORY_LIMIT: usize = 1 << 30;
/// Frames of every sample kept in RAM past the furthest start offset
/// (Kontakt's DFD preload). Voices start from this instantly while the
/// streamer fetches the rest; 2048 frames is ≈43 ms at 48 kHz (12 KiB per
/// 24-bit stereo sample, a fifth of Kontakt's 60 KB default). Measured on
/// NVMe with a cold page cache (audits/PERFORMANCE.md): no underruns at
/// 1000 streaming voices and 256 note starts per second; 1024 frames
/// underruns. Shrinks toward [`MIN_PRELOAD`] to fit the budget.
pub const PRELOAD_FRAMES: u64 = 2048;
/// Smallest preload the budget forces before start-offset ranges start to
/// stream: ≈21 ms at 48 kHz.
pub const MIN_PRELOAD: u64 = 1024;
/// Last-resort preload for banks that still do not fit: ≈5 ms at 48 kHz.
/// Streams may underrun under heavy load; the bank warns.
pub const FLOOR_PRELOAD: u64 = 256;
/// Memory left to the rest of the system when samples load into RAM only:
/// this much plus an eighth of physical memory.
pub(super) const RAM_HEADROOM: usize = 1 << 30;

/// Where sample data plays from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Streaming {
    /// Preload sample starts and stream the rest from disk.
    #[default]
    Auto,
    /// Load every sample whole: no disk reads or streaming while playing.
    /// Samples that do not fit the free RAM stream as with `Auto`.
    RamOnly,
}

/// Voices per instrument when the program stores no limit.
const DEFAULT_POLYPHONY: usize = 512;

/// Per-group playback parameters, derived from import data and then changed
/// by scripts (`set_engine_par`) on the audio thread; the rest of a bank
/// never changes.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSettings {
    /// Amplitude envelope; `None` uses the engine's attack/release defaults.
    pub envelope: Option<Ahdsr>,
    /// Flex amplitude envelope, multiplied with `envelope`.
    pub flex: Option<Flex>,
    /// Linear group volume.
    pub gain: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    /// Semitones.
    pub tune: f32,
    /// Instrument bus the group renders into; `None` is the instrument output.
    pub bus: Option<u8>,
    /// External modulation (velocity, controllers, pitch bend...).
    pub mods: ModTable,
    /// Insert filters and EQs; `None` costs voices nothing.
    pub filter: Option<Box<GroupFilter>>,
    /// Kontakt interpolation quality; every setting currently uses 4-point Hermite.
    pub interp_quality: i32,
    /// Index into the instrument's voice groups.
    pub voice_group: Option<u16>,
}

impl From<&Group> for GroupSettings {
    /// Kontakt applies velocity and pitch bend only through modulation
    /// assignments: none stored means velocity and bend do nothing.
    fn from(group: &Group) -> Self {
        Self {
            envelope: group.volume_env.as_ref().map(Ahdsr::from),
            flex: group.flex_env.as_ref().map(Flex::from),
            gain: group.gain,
            pan: group.pan,
            tune: 12.0 * group.tune.log2() as f32,
            bus: None,
            mods: ModTable::from(group),
            filter: GroupFilter::new(group),
            interp_quality: group.interp_quality,
            voice_group: None,
        }
    }
}

impl From<&crate::import::Ahdsr> for Ahdsr {
    fn from(env: &crate::import::Ahdsr) -> Self {
        Self {
            attack: env.attack_ms / 1000.0,
            curve: env.attack_curve,
            hold: env.hold_ms / 1000.0,
            decay: env.decay_ms / 1000.0,
            sustain: env.sustain.clamp(0.0, 1.0),
            release: env.release_ms / 1000.0,
        }
    }
}

impl From<&crate::import::FlexEnvelope> for Flex {
    /// Stored curves bulge above (> 0.5) or below the straight segment;
    /// the engine's curve is signed by direction (see `audits/MODULATION.md`).
    fn from(env: &crate::import::FlexEnvelope) -> Self {
        let mut from = 0.0;
        let points = env
            .points
            .iter()
            .map(|p| {
                let bulge = 2.0 * p.curve - 1.0;
                let point = FlexPoint {
                    seconds: p.time_ms / 1000.0,
                    level: p.level,
                    curve: if p.level < from { -bulge } else { bulge },
                };
                from = p.level;
                point
            })
            .collect();
        Self {
            points,
            sustain: env.sustain as usize,
        }
    }
}

/// A sample's resident data: one or more spans of decoded frames.
pub(crate) struct SampleData {
    pub rate: u32,
    pub(crate) spans: Vec<Span>,
    /// Some zone path leaves the resident spans and must stream.
    pub(crate) streamed: bool,
}

pub(crate) struct Span {
    pub start: u64,
    /// Shared with every bank in the process holding the same frames
    /// (see [`resident`]).
    pub data: Arc<Frames>,
}

/// Decoded frames, counted in [`resident_bytes`] while they live.
pub(crate) struct Frames(Pcm);

static RESIDENT_BYTES: AtomicUsize = AtomicUsize::new(0);

impl Frames {
    pub fn new(pcm: Pcm) -> Arc<Self> {
        RESIDENT_BYTES.fetch_add(pcm.bytes(), Ordering::Relaxed);
        Arc::new(Self(pcm))
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        RESIDENT_BYTES.fetch_sub(self.0.bytes(), Ordering::Relaxed);
    }
}

impl std::ops::Deref for Frames {
    type Target = Pcm;
    fn deref(&self) -> &Pcm {
        &self.0
    }
}

/// Decoded sample frames shared across banks: parts and plugin instances
/// in one process that load the same sample hold one copy. Banks own the
/// data; the registry only finds it, and forgets it when the last bank
/// drops. Touched only while loading, never on the audio thread.
pub(super) mod resident {
    use super::{Arc, Frames, Mutex, Source, Span};
    use std::{collections::HashMap, path::PathBuf, sync::Weak};

    /// By sample path and packing (loops pack uncompressed): each span's
    /// first frame and data.
    type Registry = HashMap<(PathBuf, bool), Vec<(u64, Weak<Frames>)>>;
    static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);
    static SOURCES: Mutex<Option<HashMap<PathBuf, Weak<Source>>>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
        let mut lock = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        f(lock.get_or_insert_default())
    }

    /// A live span of `key` holding all of `range`, nothing before `from`,
    /// where the bank's previous span ends, and nothing past `limit`.
    pub fn find(key: &(PathBuf, bool), range: &std::ops::Range<u64>, from: u64, limit: u64) -> Option<Span> {
        with(|r| {
            r.get(key)?.iter().find_map(|(start, data)| {
                let data = data.upgrade()?;
                let end = start + data.len() as u64;
                (*start >= from && *start <= range.start && end >= range.end && end <= limit)
                    .then_some(Span { start: *start, data })
            })
        })
    }

    /// Forget spans and sources every bank has dropped.
    pub fn sweep() {
        with(|r| {
            r.retain(|_, spans| {
                spans.retain(|(_, d)| d.strong_count() > 0);
                !spans.is_empty()
            })
        });
        let mut sources = SOURCES.lock().unwrap_or_else(|e| e.into_inner());
        sources.get_or_insert_default().retain(|_, s| s.strong_count() > 0);
    }

    /// Where `path` streams from, one copy per process.
    pub fn source((source, path): (Source, &PathBuf)) -> Arc<Source> {
        let mut lock = SOURCES.lock().unwrap_or_else(|e| e.into_inner());
        let sources = lock.get_or_insert_default();
        if let Some(shared) = sources.get(path).and_then(Weak::upgrade) {
            return shared;
        }
        let shared = Arc::new(source);
        sources.insert(path.clone(), Arc::downgrade(&shared));
        shared
    }

    pub fn insert(key: (PathBuf, bool), span: &Span) {
        with(|r| {
            let spans = r.entry(key).or_default();
            spans.retain(|(_, d)| d.strong_count() > 0);
            spans.push((span.start, Arc::downgrade(&span.data)));
        })
    }
}

/// Live slices banks share, by content.
type Pool<T> = Mutex<Vec<std::sync::Weak<[T]>>>;
static ZONES: Pool<Zone> = Mutex::new(Vec::new());
static PLAYS: Pool<ZonePlay> = Mutex::new(Vec::new());
static KEY_ZONES: Pool<u32> = Mutex::new(Vec::new());

/// A live slice of `pool` equal to `items`, or `items` shared from now on.
fn intern<T: PartialEq>(pool: &Pool<T>, items: Vec<T>) -> Arc<[T]> {
    let mut pool = pool.lock().unwrap_or_else(|e| e.into_inner());
    pool.retain(|w| w.strong_count() > 0);
    if let Some(shared) = pool.iter().filter_map(std::sync::Weak::upgrade).find(|s| **s == *items) {
        return shared;
    }
    let shared: Arc<[T]> = items.into();
    pool.push(Arc::downgrade(&shared));
    shared
}

/// Bytes of decoded sample data resident in this process, shared spans
/// counted once.
pub fn resident_bytes() -> usize {
    RESIDENT_BYTES.load(Ordering::Relaxed)
}

/// Resident sample memory every part in the process may use together: a
/// quarter of physical RAM, and no more than half of what is free now on
/// top of what is already held, so a busy machine gets smaller preloads
/// rather than swapping. Without `/proc/meminfo`, twice [`MEMORY_LIMIT`].
pub fn memory_budget() -> usize {
    ram_free().map_or(2 * MEMORY_LIMIT, |(free, total)| {
        (total / 4).min(resident_bytes() + free / 2).max(MEMORY_LIMIT / 4)
    })
}

impl SampleData {
    /// The resident span holding `frame`.
    pub(crate) fn span_at(&self, frame: u64) -> Option<u32> {
        let i = self.spans.partition_point(|s| s.end() <= frame);
        self.spans
            .get(i)
            .filter(|s| s.start <= frame)
            .map(|_| i as u32)
    }
}

impl Span {
    pub fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }
}

/// Zone data resolved for playback.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ZonePlay {
    pub sample: u32,
    pub map: PlayMap,
    /// Maximum start offset in frames.
    pub start_mod: u64,
}

pub(crate) struct VoiceGroup {
    pub max_voices: usize,
    pub kill_mode: i16,
    pub prefer_released: bool,
    pub fade: f32,
    pub exclusion: i32,
}

impl From<&VoiceLimit> for VoiceGroup {
    fn from(v: &VoiceLimit) -> Self {
        Self {
            max_voices: v.max_voices.max(1) as usize,
            kill_mode: v.kill_mode,
            prefer_released: v.prefer_released,
            fade: v.fade_ms as f32 / 1000.0,
            exclusion: v.exclusion_group,
        }
    }
}

/// Everything a part needs to play, immutable once handed to an engine.
pub struct Bank {
    groups: Vec<Group>,
    /// Zones, their playback and the zones on each key are the same for
    /// every bank of one instrument: parts and instances share one copy
    /// (17 MiB a part on Areia).
    zones: Arc<[Zone]>,
    /// What voices play: [`Bank::base`] with the player's overrides on top.
    pub settings: Vec<GroupSettings>,
    /// The library's values as its scripts have set them, under the
    /// player's overrides (see `overrides.rs`).
    pub base: Vec<GroupSettings>,
    pub(crate) plays: Arc<[ZonePlay]>,
    /// Per group: not muted and, when any group is soloed, soloed.
    pub(crate) playable: Vec<bool>,
    /// Zones mapped to key `k` are `key_zones[key_start[k]..key_start[k + 1]]`.
    key_start: [u32; 129],
    key_zones: Arc<[u32]>,
    pub(crate) samples: Vec<SampleData>,
    pub(crate) voice_groups: Vec<Option<VoiceGroup>>,
    pub(crate) polyphony: usize,
    pub(crate) streamer: Option<Streamer>,
    /// Resident sample and stream-buffer bytes.
    pub bytes: usize,
    /// Preload frames per sample the memory budget allowed.
    pub preload: u64,
    /// Resident bytes the preload was planned for; [`Bank::bytes`] is less
    /// where samples pack.
    pub planned: usize,
    /// Frames of each zone's start-offset range kept resident; later offsets
    /// start from the stream.
    pub cover: u64,
    /// How the budget degraded streaming, when it may not keep up.
    pub warning: Option<String>,
    /// Zones dropped because their sample is missing, unreadable or out of bounds.
    pub skipped_zones: usize,
    /// First few reasons for skipped zones.
    pub issues: Vec<String>,
    /// Note starts per sample, counted by the audio thread for [`Residency`].
    pub(crate) usage: Arc<[AtomicU32]>,
    /// Smart memory for this bank's streamed samples, for the loader to take
    /// ([`Bank::take_residency`]).
    residency: Option<Box<Residency>>,
}

/// [`Bank::load_counting`]'s progress once every sample is read.
pub const LOAD_DONE: u32 = 1000;

impl Bank {
    /// Load every group of `instrument` within [`MEMORY_LIMIT`], streaming
    /// long samples from disk.
    pub fn load(instrument: &Instrument) -> Result<Self> {
        Self::load_within(instrument, MEMORY_LIMIT)
    }

    /// Load every group of `instrument` with at most `budget` bytes of
    /// resident samples and stream buffers. The preload shrinks from
    /// [`PRELOAD_FRAMES`] toward [`MIN_PRELOAD`] until the bank fits.
    pub fn load_within(instrument: &Instrument, budget: usize) -> Result<Self> {
        Self::load_counting(instrument, budget, Streaming::Auto, &[], &AtomicU32::new(0))
    }

    /// [`Bank::load_within`], counting `progress` up to [`LOAD_DONE`] as
    /// samples are opened and then read. `controllers` are the values the
    /// scripts set in `on init` ([`crate::ksp::Runtime::init_controllers`]):
    /// a tight budget keeps resident only the start offsets they select.
    /// [`Streaming::RamOnly`] loads samples whole past `budget`, as far as
    /// free RAM allows.
    pub fn load_counting(
        instrument: &Instrument,
        budget: usize,
        streaming: Streaming,
        controllers: &[(u8, u8)],
        progress: &AtomicU32,
    ) -> Result<Self> {
        let mut issues = Issues::default();
        resident::sweep();
        // Resolve each distinct sample once, then open them all in parallel.
        // By the path's bytes: a `Path` hashes one component at a time.
        let mut ids: HashMap<&std::ffi::OsStr, usize> = HashMap::new();
        let mut paths = Vec::new();
        let zone_ids: Vec<_> = instrument
            .zones
            .iter()
            .map(|zone| {
                zone.available.then(|| {
                    *ids.entry(zone.sample.as_os_str()).or_insert_with(|| {
                        paths.push(&zone.sample);
                        paths.len() - 1
                    })
                })
            })
            .collect();
        // Resolved on this thread: sources outlive loading, and allocating
        // them on workers fragments their heap arenas (18 MiB more RSS on
        // Vista 5 Violins). Import just read each member header, so these
        // reads come from the page cache.
        let mut sources = audio::Sources::default();
        let resolved: Vec<_> = paths.iter().map(|path| sources.source(path)).collect();
        // Progress runs on from where the caller left it: opening every
        // sample takes the first tenth of the rest, reading them the others
        // by frames read. Only ever rises.
        let base = progress.load(Ordering::Relaxed).min(LOAD_DONE) as usize;
        let reads_from = base + (LOAD_DONE as usize - base) / 10;
        let advance = |done: &AtomicUsize, of: usize, add: usize, (from, to): (usize, usize)| {
            let n = done.fetch_add(add, Ordering::Relaxed) + add;
            let at = from + (to - from) * n.min(of) / of.max(1);
            progress.fetch_max(at as u32, Ordering::Relaxed);
        };
        let opens = AtomicUsize::new(0);
        let opened = parallel(resolved, |_: &mut (), source| {
            advance(&opens, paths.len(), 1, (base, reads_from));
            let source = source?;
            anyhow::Ok((source.open()?, source))
        });
        let mut readers: Vec<(Source, SampleReader, &PathBuf)> = Vec::new();
        let opened: Vec<Option<u32>> = opened
            .into_iter()
            .zip(&paths)
            .map(|(result, &path)| match result {
                Ok((reader, source)) => {
                    readers.push((source, reader, path));
                    Some(readers.len() as u32 - 1)
                }
                Err(e) => {
                    issues.note(format_args!("{e:#}"));
                    None
                }
            })
            .collect();
        let mut zones = Vec::new();
        let mut zone_samples = Vec::new();
        for (zone, id) in instrument.zones.iter().zip(zone_ids) {
            match id.map(|id| opened[id]) {
                None => issues.skip(format_args!("unavailable {}", zone.sample.display())),
                Some(None) => issues.skip(format_args!("unreadable {}", zone.sample.display())),
                Some(Some(id)) => {
                    zones.push(zone.clone());
                    zone_samples.push(id);
                }
            }
        }
        let info = readers.iter().map(|(_, r, _)| (r.rate, r.frames)).collect();
        let mut builder =
            Builder::new(instrument.groups.clone(), zones, zone_samples, info, issues)?;
        builder.limits(instrument);

        let frame_bytes: Vec<_> = readers
            .iter()
            .map(|(_, r, _)| Pcm::frame_bytes(r.bits))
            .collect();
        // ponytail: a plan wider than another bank's spans (a roomier budget)
        // reads its own copy; growing the shared spans in place, and moving
        // the other banks onto them, would keep one.
        let mut layout = builder.plan(&frame_bytes, budget, controllers);
        let ram_only = (streaming == Streaming::RamOnly).then(|| {
            // ponytail: /proc/meminfo only; other systems get MEMORY_LIMIT until they have a probe.
            let room = ram_free().map_or(MEMORY_LIMIT, |(free, total)| {
                free.saturating_sub(RAM_HEADROOM + total / 8)
            });
            builder.keep_whole(&mut layout, &frame_bytes, room)
        });
        let (preload, whole, margin, planned) =
            (layout.preload, layout.whole, layout.margin, layout.bytes);
        let (cover, max_cover) = (layout.cover.min(layout.width), layout.width);
        // Before a sample fails to read drops its zones, while the plays line
        // up with the plan's.
        let tracked = ram_only.is_none().then(|| {
            residency::by_sample(&builder.plays, &builder.zones, &layout.reach, frame_bytes.len())
        });
        let frames_to_read = (layout.plan.iter())
            .map(|(spans, ..)| spans.iter().map(|r| r.end - r.start).sum::<u64>() as usize)
            .sum();
        let read = AtomicUsize::new(0);
        let jobs = readers.into_iter().zip(layout.plan).collect();
        let decoded = parallel(
            jobs,
            |(ints, buf): &mut (Vec<[i32; 2]>, Vec<Frame>),
             ((source, mut reader, path), (spans, streamed, looping))| {
                // Frames another bank holds are shared, not read again. A
                // shared span may reach past the planned range: more is resident.
                let key = (path.clone(), !looping);
                let mut kept: Vec<Span> = Vec::with_capacity(spans.len());
                let read_spans = || -> Result<()> {
                    for range in spans {
                        let len = (range.end - range.start) as usize;
                        advance(&read, frames_to_read, len, (reads_from, LOAD_DONE as usize));
                        let from = kept.last().map_or(0, Span::end);
                        if !kept.is_empty() && range.end <= from {
                            continue;
                        }
                        if let Some(span) = resident::find(&key, &range, from, u64::MAX) {
                            kept.push(span);
                            continue;
                        }
                        let range = range.start.max(from)..range.end;
                        let data = Frames::new(reader.read_pcm(range.clone(), !looping, ints, buf)?);
                        let span = Span { start: range.start, data };
                        resident::insert(key.clone(), &span);
                        kept.push(span);
                    }
                    Ok(())
                };
                let spans = read_spans().map(|()| kept);
                (spans, streamed, reader.rate, (source, path))
            },
        );
        progress.store(LOAD_DONE, Ordering::Relaxed);
        let mut samples = Vec::with_capacity(decoded.len());
        let mut streamed = Vec::with_capacity(decoded.len());
        let mut bytes = 0;
        for (id, (spans, streamed_sample, rate, source)) in decoded.into_iter().enumerate() {
            let (spans, streamed_sample) = match spans {
                Ok(spans) => (spans, streamed_sample),
                Err(e) => {
                    builder.drop_sample(id, &e);
                    (Vec::new(), false)
                }
            };
            bytes += spans.iter().map(|s| s.data.bytes()).sum::<usize>();
            streamed.push(streamed_sample.then(|| resident::source(source)));
            samples.push(SampleData {
                rate,
                spans,
                streamed: streamed_sample,
            });
        }
        let streamer = if streamed.iter().any(Option::is_some) {
            bytes += Streamer::BYTES;
            Some(Streamer::spawn(streamed)?)
        } else {
            None
        };
        let mut bank = builder.finish(samples, streamer, bytes)?;
        (bank.preload, bank.planned, bank.cover) = (preload, planned, cover);
        bank.residency = tracked
            .and_then(|plays| Residency::new(&bank, plays, &frame_bytes, cover))
            .map(Box::new);
        let mib = budget >> 20;
        let still = bank.streamed_samples();
        bank.warning = if let Some(needed) = ram_only.filter(|_| still > 0) {
            Some(format!(
                "Not enough free RAM to load every sample: {still} of {} still stream ({} MiB more needed)",
                bank.samples.len(),
                needed >> 20
            ))
        } else if ram_only.is_some() {
            None
        } else if planned > budget {
            Some(format!(
                "Needs {} MiB resident at the smallest preload, over the {mib} MiB budget",
                planned >> 20
            ))
        } else if preload < MIN_PRELOAD {
            Some(format!(
                "Preload cut to {preload} frames to fit {mib} MiB: streams may underrun under heavy load"
            ))
        } else if cover < max_cover {
            Some(format!(
                "Sample-start offsets more than {cover} frames past the scripts' setting stream from disk and may start late"
            ))
        } else if !whole {
            Some(format!(
                "Sample-start offsets more than {margin} frames from the scripts' controller settings stream from disk and may start late"
            ))
        } else {
            None
        };
        audio::trim_heap();
        Ok(bank)
    }

    /// A fully resident bank from decoded samples; zones select samples by path.
    pub fn from_samples(
        groups: Vec<Group>,
        zones: Vec<Zone>,
        samples: Vec<(PathBuf, Sample)>,
    ) -> Result<Self> {
        let index: HashMap<_, _> = samples
            .iter()
            .enumerate()
            .map(|(i, (path, _))| (path.clone(), i as u32))
            .collect();
        let mut zone_samples = Vec::new();
        for zone in &zones {
            zone_samples.push(
                *index
                    .get(&zone.sample)
                    .ok_or_else(|| anyhow::anyhow!("No sample for {}", zone.sample.display()))?,
            );
        }
        let info = samples
            .iter()
            .map(|(_, s)| (s.rate, s.frames.len() as u64))
            .collect();
        let builder = Builder::new(groups, zones, zone_samples, info, Issues::default())?;
        let samples: Vec<_> = samples
            .into_iter()
            .map(|(_, s)| SampleData {
                rate: s.rate,
                spans: vec![Span {
                    start: 0,
                    data: Frames::new(Pcm::pack(&s.frames, false)),
                }],
                streamed: false,
            })
            .collect();
        let bytes = samples.iter().map(|s| s.spans[0].data.bytes()).sum();
        builder.finish(samples, None, bytes)
    }

    /// The smart memory manager for this bank, once (see `residency.rs`).
    pub fn take_residency(&mut self) -> Option<Box<Residency>> {
        self.residency.take()
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Samples that stream from disk beyond their preload.
    pub fn streamed_samples(&self) -> usize {
        self.samples.iter().filter(|s| s.streamed).count()
    }

    /// Zones mapped to `note`.
    pub(crate) fn zones_on(&self, note: u8) -> &[u32] {
        let n = note as usize;
        &self.key_zones[self.key_start[n] as usize..self.key_start[n + 1] as usize]
    }

    /// Set the polyphony limit (clamped to the engine's voice storage).
    pub fn set_polyphony(&mut self, voices: usize) {
        self.polyphony = voices.clamp(1, super::MAX_VOICES);
    }
}

#[derive(Default)]
struct Issues {
    skipped: usize,
    notes: Vec<String>,
}

impl Issues {
    const KEPT: usize = 8;

    fn skip(&mut self, reason: std::fmt::Arguments) {
        self.skipped += 1;
        self.note(reason);
    }

    fn note(&mut self, reason: std::fmt::Arguments) {
        if self.notes.len() < Self::KEPT {
            self.notes.push(reason.to_string());
        }
    }
}

/// A sample's resident frame ranges, whether it streams beyond them, and
/// whether voices loop inside them indefinitely.
type Plan = (Vec<Range<u64>>, bool, bool);

/// [`Builder::plan`]'s choice: per-sample plans and their expected bytes.
struct Layout {
    preload: u64,
    /// Every start offset is resident, not only those reachable.
    whole: bool,
    /// Frames around the reachable offsets also resident.
    margin: u64,
    /// Resident frames of each zone's start-offset range past its lowest.
    cover: u64,
    /// The widest resident start-offset range any zone asks for.
    width: u64,
    /// Per zone, the start offsets kept resident.
    reach: Vec<(u64, u64)>,
    plan: Vec<Plan>,
    bytes: usize,
}

/// Shared validation and indexing for loaded and in-memory banks.
struct Builder {
    groups: Vec<Group>,
    zones: Vec<Zone>,
    plays: Vec<ZonePlay>,
    settings: Vec<GroupSettings>,
    voice_groups: Vec<Option<VoiceGroup>>,
    polyphony: usize,
    issues: Issues,
}

impl Builder {
    fn new(
        groups: Vec<Group>,
        zones: Vec<Zone>,
        samples: Vec<u32>,
        info: Vec<(u32, u64)>,
        mut issues: Issues,
    ) -> Result<Self> {
        // `info[sample]` is `(rate, frames)`.
        let total = zones.len();
        let mut kept = Vec::with_capacity(total);
        let mut plays = Vec::with_capacity(total);
        for (zone, sample) in zones.into_iter().zip(samples) {
            let (_, frames) = info[sample as usize];
            let Some(group) = groups.get(zone.group) else {
                issues.skip(format_args!("zone refers to missing group {}", zone.group));
                continue;
            };
            match play_map(&zone, group, frames) {
                Ok((map, clamped)) => {
                    if clamped {
                        issues.note(format_args!(
                            "loop crossfade shortened in {}",
                            zone.sample.display()
                        ));
                    }
                    let start_mod = zone
                        .start_mod
                        .map_or(0, u64::from)
                        .min(map.end - map.start - 1);
                    plays.push(ZonePlay {
                        sample,
                        map,
                        start_mod,
                    });
                    kept.push(zone);
                }
                Err(e) => issues.skip(format_args!("{e}: {}", zone.sample.display())),
            }
        }
        if kept.is_empty() && total > 0 {
            bail!(
                "No playable zones: {} skipped ({})",
                issues.skipped,
                issues.notes.join("; ")
            );
        }
        let mut settings: Vec<_> = groups.iter().map(GroupSettings::from).collect();
        super::params::share_curves(settings.iter_mut().map(|s| &mut s.mods));
        Ok(Self {
            groups,
            zones: kept,
            plays,
            settings,
            voice_groups: Vec::new(),
            polyphony: DEFAULT_POLYPHONY,
            issues,
        })
    }

    fn limits(&mut self, instrument: &Instrument) {
        self.voice_groups = instrument
            .voice_groups
            .iter()
            .map(|v| v.as_ref().map(VoiceGroup::from))
            .collect();
        if let Some(limit) = &instrument.voice_limit {
            self.polyphony = (limit.max_voices as usize).clamp(1, super::MAX_VOICES);
        }
        for (settings, group) in self.settings.iter_mut().zip(&self.groups) {
            settings.voice_group = group
                .voice_group
                .and_then(|v| u16::try_from(v).ok())
                .filter(|&v| {
                    self.voice_groups
                        .get(v as usize)
                        .is_some_and(Option::is_some)
                });
        }
    }

    /// Per sample the resident ranges and whether it streams, shedding
    /// resident data until it fits `budget`, cheapest loss first:
    /// 1. every zone's whole start-offset range, the preload shrinking from
    ///    [`PRELOAD_FRAMES`] to [`MIN_PRELOAD`];
    /// 2. only the offsets reachable with the scripts' `controllers` (Areia
    ///    16 Violins keeps 12000 frames for each of 24576 samples, 1.8 GiB,
    ///    yet its script pins CC113 to one offset), widened into the spare
    ///    budget, or with the preload shrinking again;
    /// 3. the reachable ranges cut short;
    /// 4. the preload down to [`FLOOR_PRELOAD`].
    ///
    /// Past that the bank loads over budget rather than fail. Each step
    /// keeps the largest value that fits, within 64 frames.
    /// `frame_bytes[sample]` is its expected storage per frame; packing may
    /// store less than planned.
    fn plan(&self, frame_bytes: &[usize], budget: usize, controllers: &[(u8, u8)]) -> Layout {
        let mut uses = vec![Vec::new(); frame_bytes.len()];
        for (i, play) in self.plays.iter().enumerate() {
            uses[play.sample as usize].push(i);
        }
        let all: Vec<_> = self.plays.iter().map(|p| (0, p.start_mod)).collect();
        let reachable = self.reachable(controllers);
        let plan = |reach: &[(u64, u64)], preload, cover| -> Layout {
            let plan: Vec<_> = uses
                .iter()
                .map(|plays| {
                    let plays: Vec<_> = plays.iter().map(|&i| (&self.plays[i], reach[i])).collect();
                    spans(&plays, preload, cover)
                })
                .collect();
            let mut bytes: usize = plan
                .iter()
                .zip(frame_bytes)
                .map(|((ranges, ..), size)| {
                    size * ranges.iter().map(|r| r.end - r.start).sum::<u64>() as usize
                })
                .sum();
            if plan.iter().any(|(_, streamed, _)| *streamed) {
                bytes += Streamer::BYTES;
            }
            Layout {
                preload,
                whole: reach == &all[..],
                margin: 0,
                cover,
                width: reach.iter().map(|(lo, hi)| hi - lo).max().unwrap_or(0),
                reach: reach.to_vec(),
                plan,
                bytes,
            }
        };
        // The largest `x` in `lo..=hi` whose layout fits, given that `lo`'s does.
        let largest = |lo: u64, hi: u64, at: &dyn Fn(u64) -> Layout| {
            let (mut fits, mut over, mut best) = (lo, hi + 1, at(lo));
            while over - fits > 64 {
                let mid = (fits + over) / 2;
                let candidate = at(mid);
                if candidate.bytes <= budget {
                    (fits, best) = (mid, candidate);
                } else {
                    over = mid;
                }
            }
            best
        };
        let full = plan(&all, PRELOAD_FRAMES, u64::MAX);
        if full.bytes <= budget {
            return full;
        }
        if plan(&all, MIN_PRELOAD, u64::MAX).bytes <= budget {
            return largest(MIN_PRELOAD, PRELOAD_FRAMES, &|p| plan(&all, p, u64::MAX));
        }
        // The reachable offsets, widened by `margin` frames into spare budget.
        let widened = |margin: u64| {
            let reach: Vec<_> = reachable
                .iter()
                .zip(&all)
                .map(|(&(lo, hi), &(_, max))| {
                    (lo.saturating_sub(margin), hi.saturating_add(margin).min(max))
                })
                .collect();
            Layout {
                margin,
                ..plan(&reach, PRELOAD_FRAMES, u64::MAX)
            }
        };
        let reach = &reachable[..];
        if plan(reach, PRELOAD_FRAMES, u64::MAX).bytes <= budget {
            let widest = all.iter().map(|&(_, max)| max).max().unwrap_or(0);
            return largest(0, widest, &widened);
        }
        if plan(reach, MIN_PRELOAD, u64::MAX).bytes <= budget {
            return largest(MIN_PRELOAD, PRELOAD_FRAMES, &|p| plan(reach, p, u64::MAX));
        }
        if plan(reach, MIN_PRELOAD, 0).bytes <= budget {
            let width = reach.iter().map(|(lo, hi)| hi - lo).max().unwrap_or(0);
            return largest(0, width, &|c| plan(reach, MIN_PRELOAD, c));
        }
        let floor = plan(reach, FLOOR_PRELOAD, 0);
        if floor.bytes <= budget {
            return largest(FLOOR_PRELOAD, MIN_PRELOAD, &|p| plan(reach, p, 0));
        }
        floor
    }

    /// Load samples whole instead of streaming them, smallest first, while
    /// they fit `room` bytes. Returns the bytes the rest would need.
    fn keep_whole(&self, layout: &mut Layout, frame_bytes: &[usize], room: usize) -> usize {
        // Per sample: frames the zones play, and whether any loops.
        let mut extent = vec![(0, false); frame_bytes.len()];
        for play in &self.plays {
            let e = &mut extent[play.sample as usize];
            *e = (e.0.max(play.map.end), e.1 | play.map.looped.is_some());
        }
        let size = |i: usize| frame_bytes[i] * extent[i].0 as usize;
        let resident = |plan: &Plan| plan.0.iter().map(|r| r.end - r.start).sum::<u64>() as usize;
        let mut streamed: Vec<_> = (0..layout.plan.len()).filter(|&i| layout.plan[i].1).collect();
        streamed.sort_by_key(|&i| size(i));
        let mut room = room.saturating_sub(layout.bytes);
        let mut needed = 0;
        for i in streamed {
            let more = size(i).saturating_sub(frame_bytes[i] * resident(&layout.plan[i]));
            if more <= room {
                room -= more;
                layout.bytes += more;
                layout.plan[i] = (std::iter::once(0..extent[i].0).collect(), false, extent[i].1);
            } else {
                needed += more - room.min(more);
                room = 0;
            }
        }
        needed
    }

    /// Per zone, the lowest and highest start offset (frames) its voices
    /// take with `controllers` held and every other controller at 0, over
    /// its keys and velocities. Scripted `play_note` offsets are not known.
    fn reachable(&self, controllers: &[(u8, u8)]) -> Vec<(u64, u64)> {
        let mut cc = [0u8; 128];
        for &(n, value) in controllers {
            if let Some(c) = cc.get_mut(n as usize) {
                *c = value;
            }
        }
        let mut seen = HashMap::new();
        self.plays
            .iter()
            .zip(&self.zones)
            .map(|(play, z)| {
                if play.start_mod == 0 {
                    return (0, 0);
                }
                let key = (z.group, z.low_key, z.high_key, z.low_velocity, z.high_velocity);
                let (lo, hi) = *seen.entry(key).or_insert_with(|| {
                    self.settings[z.group].mods.start_offset_range(
                        &cc,
                        z.low_key..=z.high_key,
                        z.low_velocity..=z.high_velocity,
                    )
                });
                // As a voice rounds it (`Player::spawn`).
                let frames = |x: f32| ((x * play.start_mod as f32) as u64).min(play.start_mod);
                (frames(lo), frames(hi))
            })
            .collect()
    }

    /// Skip every zone of a sample whose data turned out to be unreadable.
    fn drop_sample(&mut self, id: usize, error: &anyhow::Error) {
        let mut kept = self.plays.iter().map(|p| p.sample as usize != id);
        let before = self.zones.len();
        self.zones.retain(|_| kept.next().unwrap_or(true));
        self.plays.retain(|p| p.sample as usize != id);
        self.issues.skipped += before - self.zones.len();
        self.issues.note(format_args!("{error:#}"));
    }

    fn finish(
        self,
        samples: Vec<SampleData>,
        streamer: Option<Streamer>,
        bytes: usize,
    ) -> Result<Bank> {
        if self.zones.is_empty() && self.issues.skipped > 0 {
            bail!(
                "No playable zones: {} skipped ({})",
                self.issues.skipped,
                self.issues.notes.join("; ")
            );
        }
        let plays = self.plays;
        // Samples are resolved: a playing bank never reads zones' paths,
        // and 93k of them (Areia) are 14 MiB a part.
        let mut zones = self.zones;
        zones.iter_mut().for_each(|z| z.sample = PathBuf::new());
        let any_solo = self.groups.iter().any(|g| g.soloed);
        let playable = self
            .groups
            .iter()
            .map(|g| !g.muted && (!any_solo || g.soloed))
            .collect();
        let samples_usage = (0..samples.len()).map(|_| AtomicU32::new(0)).collect();
        let mut key_start = [0u32; 129];
        let mut key_zones = Vec::new();
        for note in 0..128u8 {
            key_start[note as usize] = key_zones.len() as u32;
            let on_key = zones
                .iter()
                .enumerate()
                .filter(|(_, z)| (z.low_key..=z.high_key).contains(&note));
            key_zones.extend(on_key.map(|(i, _)| i as u32));
        }
        key_start[128] = key_zones.len() as u32;
        Ok(Bank {
            groups: self.groups,
            zones: intern(&ZONES, zones),
            base: self.settings.clone(),
            settings: self.settings,
            plays: intern(&PLAYS, plays),
            playable,
            key_start,
            key_zones: intern(&KEY_ZONES, key_zones),
            samples,
            preload: PRELOAD_FRAMES,
            planned: bytes,
            cover: u64::MAX,
            warning: None,
            voice_groups: self.voice_groups,
            polyphony: self.polyphony,
            streamer,
            bytes,
            skipped_zones: self.issues.skipped,
            issues: self.issues.notes,
            usage: samples_usage,
            residency: None,
        })
    }
}

/// Validate a zone against its sample and build its playback path. The flag
/// reports a loop crossfade shortened to fit before the loop start.
fn play_map(zone: &Zone, group: &Group, frames: u64) -> Result<(PlayMap, bool), &'static str> {
    let start = zone.start as u64;
    let end = frames
        .checked_add_signed(i64::from(zone.end))
        .ok_or("zone end precedes sample start")?;
    if start >= end || end > frames {
        return Err("invalid sample bounds");
    }
    let mut clamped = false;
    let looped = match &zone.loop_range {
        Some(l) => {
            let (ls, le) = (l.start as u64, l.end as u64);
            // Zones may start inside or past their loop (Vista's sustains
            // do): the path enters the loop mid-cycle or plays straight through.
            if ls >= le || le > end {
                return Err("invalid loop");
            }
            // The crossfade blends toward the frames before the loop start, which must exist.
            let xfade = (l.crossfade as u64).min(ls).min(le - ls);
            clamped = xfade != l.crossfade as u64;
            Some(LoopMap {
                start: ls,
                end: le,
                xfade,
                until_release: l.until_release,
            })
        }
        None => None,
    };
    Ok((
        PlayMap {
            start,
            end,
            reverse: group.reverse,
            looped,
        },
        clamped,
    ))
}

/// Resident frame ranges of a sample played by `plays`, merged and sorted,
/// whether any zone path extends beyond them, and whether a voice may loop in
/// them. Each zone keeps its start offsets `lo..=hi` (at most `cover` past
/// `lo`; other offsets stream) and `preload` frames past them;
/// loops ending within four preloads of the zone start stay resident, so
/// short sustain loops never touch the disk. Data past the furthest frame
/// any zone plays is never needed.
pub(super) fn spans(plays: &[(&ZonePlay, (u64, u64))], preload: u64, cover: u64) -> Plan {
    let (mut frames, mut looping, mut any_loop) = (0, false, false);
    let mut ranges = Vec::with_capacity(plays.len());
    for &(play, (lo, hi)) in plays {
        let map = &play.map;
        frames = frames.max(map.end);
        any_loop |= map.looped.is_some();
        // A voice's first window frame is one before its start offset.
        let (first, head) = (lo.saturating_sub(1), hi.min(lo.saturating_add(cover)) + preload);
        if map.reverse {
            ranges.push(map.end.saturating_sub(head)..map.end - first.min(map.end));
            continue;
        }
        let mut range = map.start + first..map.start + head;
        if let Some(l) = map
            .looped
            .filter(|l| l.end <= map.start + 4 * preload && map.start < l.end)
        {
            looping = true;
            range.start = range.start.min(l.start - l.xfade);
            range.end = range
                .end
                .max(l.end + if l.until_release { preload } else { 0 });
        }
        ranges.push(range);
    }
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<u64>> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            // Bridge small gaps: one span is cheaper than a stream restart.
            Some(last) if range.start <= last.end + preload / 2 => {
                last.end = last.end.max(range.end)
            }
            _ => merged.push(range),
        }
    }
    let covered: u64 = merged
        .iter()
        .map(|r| r.end.min(frames) - r.start.min(frames))
        .sum();
    if frames <= 2 * preload || covered + preload >= frames {
        return (std::iter::once(0..frames).collect(), false, any_loop);
    }
    merged.retain_mut(|r| {
        r.end = r.end.min(frames);
        !r.is_empty()
    });
    (merged, true, looping)
}

/// Free (available) and total RAM in bytes, from `/proc/meminfo`.
pub(super) fn ram_free() -> Option<(usize, usize)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| -> Option<usize> {
        let line = info.lines().find(|l| l.starts_with(name))?;
        let kib: usize = line[name.len()..].trim().trim_end_matches("kB").trim().parse().ok()?;
        Some(kib << 10)
    };
    Some((field("MemAvailable:")?, field("MemTotal:")?))
}

/// Run `f` over `items` on every core, keeping order. Each worker owns one
/// `S` scratch value across its items.
pub(crate) fn parallel<T: Send, S: Default, R: Send>(
    items: Vec<T>,
    f: impl Fn(&mut S, T) -> R + Sync,
) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map_or(1, NonZero::get)
        .min(items.len());
    let len = items.len();
    let queue = Mutex::new(items.into_iter().enumerate());
    let next = || queue.lock().unwrap_or_else(|e| e.into_inner()).next();
    let mut out: Vec<Option<R>> = (0..len).map(|_| None).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut scratch = S::default();
                    let mut done = Vec::new();
                    while let Some((i, item)) = next() {
                        done.push((i, f(&mut scratch, item)));
                    }
                    done
                })
            })
            .collect();
        for worker in workers {
            let done = worker
                .join()
                .unwrap_or_else(|e| std::panic::resume_unwind(e));
            for (i, result) in done {
                out[i] = Some(result);
            }
        }
    });
    out.into_iter().flatten().collect()
}
