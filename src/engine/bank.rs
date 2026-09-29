//! Immutable instrument data prepared for playback.

use super::{
    map::{LoopMap, PlayMap},
    params::ModTable,
    stream::Streamer,
    voice::Ahdsr,
};
use crate::{
    audio::{self, Frame, Pcm, Sample, SampleReader, Source},
    import::{Group, Instrument, VoiceLimit, Zone},
};
use anyhow::{Result, bail, ensure};
use std::{collections::HashMap, num::NonZero, ops::Range, path::PathBuf, sync::Mutex};

/// Default resident sample memory budget per bank.
pub const MEMORY_LIMIT: usize = 1 << 30;
/// Frames of every sample kept in RAM past the furthest start offset
/// (Kontakt's DFD preload). Voices start from this instantly while the
/// streamer fetches the rest; 8192 frames is ≈170 ms at 48 kHz (48 KiB per
/// 24-bit stereo sample). Shrinks toward [`MIN_PRELOAD`] to fit the budget.
pub const PRELOAD_FRAMES: u64 = 8192;
/// Smallest preload the budget may force: ≈43 ms at 48 kHz, several times
/// the streamer's time to first data on an SSD.
pub const MIN_PRELOAD: u64 = 2048;
/// Voices per instrument when the program stores no limit.
const DEFAULT_POLYPHONY: usize = 512;

/// Per-group playback parameters, derived from import data and then changed
/// by scripts (`set_engine_par`) on the audio thread; the rest of a bank
/// never changes.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSettings {
    /// Amplitude envelope; `None` uses the engine's attack/release defaults.
    pub envelope: Option<Ahdsr>,
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
            gain: group.gain,
            pan: group.pan,
            tune: 12.0 * group.tune.log2() as f32,
            bus: None,
            mods: ModTable::from(group),
            interp_quality: group.interp_quality,
            voice_group: None,
        }
    }
}

impl From<&crate::import::Ahdsr> for Ahdsr {
    // ponytail: attack curve is decoded but the voice envelope has one fixed shape.
    fn from(env: &crate::import::Ahdsr) -> Self {
        Self {
            attack: env.attack_ms / 1000.0,
            hold: env.hold_ms / 1000.0,
            decay: env.decay_ms / 1000.0,
            sustain: env.sustain.clamp(0.0, 1.0),
            release: env.release_ms / 1000.0,
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
    pub data: Pcm,
}

impl Span {
    pub fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }
}

/// Zone data resolved for playback.
pub(crate) struct ZonePlay {
    pub sample: u32,
    /// Resident span holding the zone's first frames.
    pub span: u32,
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
    zones: Vec<Zone>,
    pub settings: Vec<GroupSettings>,
    pub(crate) plays: Vec<ZonePlay>,
    /// Per group: not muted and, when any group is soloed, soloed.
    pub(crate) playable: Vec<bool>,
    /// Zones mapped to key `k` are `key_zones[key_start[k]..key_start[k + 1]]`.
    key_start: [u32; 129],
    key_zones: Box<[u32]>,
    pub(crate) samples: Vec<SampleData>,
    pub(crate) voice_groups: Vec<Option<VoiceGroup>>,
    pub(crate) polyphony: usize,
    pub(crate) streamer: Option<Streamer>,
    /// Resident sample and stream-buffer bytes.
    pub bytes: usize,
    /// Preload frames per sample the memory budget allowed.
    pub preload: u64,
    /// Zones dropped because their sample is missing, unreadable or out of bounds.
    pub skipped_zones: usize,
    /// First few reasons for skipped zones.
    pub issues: Vec<String>,
}

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
        let mut issues = Issues::default();
        // Resolve each distinct sample once, then open them all in parallel.
        let mut ids: HashMap<&PathBuf, usize> = HashMap::new();
        let mut paths = Vec::new();
        let zone_ids: Vec<_> = instrument
            .zones
            .iter()
            .map(|zone| {
                zone.available.then(|| {
                    *ids.entry(&zone.sample).or_insert_with(|| {
                        paths.push(&zone.sample);
                        paths.len() - 1
                    })
                })
            })
            .collect();
        let mut sources = audio::Sources::default();
        let resolved: Vec<_> = paths.iter().map(|path| sources.source(path)).collect();
        let opened = parallel(resolved, |_: &mut (), source| {
            let source = source?;
            anyhow::Ok((source.open()?, source))
        });
        let mut readers: Vec<(Source, SampleReader)> = Vec::new();
        let opened: Vec<Option<u32>> = opened
            .into_iter()
            .map(|result| match result {
                Ok((reader, source)) => {
                    readers.push((source, reader));
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
        let info = readers.iter().map(|(_, r)| (r.rate, r.frames)).collect();
        let mut builder =
            Builder::new(instrument.groups.clone(), zones, zone_samples, info, issues)?;
        builder.limits(instrument);

        let frame_bytes: Vec<_> = readers
            .iter()
            .map(|(_, r)| Pcm::frame_bytes(r.bits))
            .collect();
        let (preload, plan) = builder.plan(&frame_bytes, budget)?;
        let jobs = readers.into_iter().zip(plan).collect();
        let decoded = parallel(
            jobs,
            |buf: &mut Vec<Frame>, ((source, mut reader), (spans, streamed))| {
                let spans = spans
                    .into_iter()
                    .map(|range| {
                        buf.clear();
                        buf.resize((range.end - range.start) as usize, [0.0; 2]);
                        reader.read(range.start, buf)?;
                        Ok(Span {
                            start: range.start,
                            data: Pcm::pack(buf),
                        })
                    })
                    .collect::<Result<Vec<_>>>();
                (spans, streamed, reader.rate, source)
            },
        );
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
            streamed.push(streamed_sample.then_some(source));
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
        ensure!(
            bytes <= budget,
            "Resident sample data exceeds the {} MiB bank limit",
            budget >> 20
        );
        let mut bank = builder.finish(samples, streamer, bytes)?;
        bank.preload = preload;
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
                    data: Pcm::pack(&s.frames),
                }],
                streamed: false,
            })
            .collect();
        let bytes = samples.iter().map(|s| s.spans[0].data.bytes()).sum();
        builder.finish(samples, None, bytes)
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

/// A sample's resident frame ranges and whether it streams beyond them.
type Plan = (Vec<Range<u64>>, bool);

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
                    let start_mod = zone.start_mod.map_or(0, u64::from).min(map.end - map.start - 1);
                    plays.push(ZonePlay {
                        sample,
                        span: 0,
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
        let settings = groups.iter().map(GroupSettings::from).collect();
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
            settings.voice_group = group.voice_group.and_then(|v| u16::try_from(v).ok()).filter(|&v| {
                self.voice_groups
                    .get(v as usize)
                    .is_some_and(Option::is_some)
            });
        }
    }

    /// The largest preload in `MIN_PRELOAD..=PRELOAD_FRAMES` whose resident
    /// data fits `budget`, and per sample the resident ranges and whether it
    /// streams. `frame_bytes[sample]` is its expected storage per frame.
    fn plan(&self, frame_bytes: &[usize], budget: usize) -> Result<(u64, Vec<Plan>)> {
        let mut uses = vec![Vec::new(); frame_bytes.len()];
        for play in &self.plays {
            uses[play.sample as usize].push(play);
        }
        let plan = |preload| -> (Vec<Plan>, usize) {
            let plan: Vec<_> = uses.iter().map(|plays| spans(plays, preload)).collect();
            let mut bytes: usize = plan
                .iter()
                .zip(frame_bytes)
                .map(|((ranges, _), size)| {
                    size * ranges.iter().map(|r| r.end - r.start).sum::<u64>() as usize
                })
                .sum();
            if plan.iter().any(|(_, streamed)| *streamed) {
                bytes += Streamer::BYTES;
            }
            (plan, bytes)
        };
        let (full, bytes) = plan(PRELOAD_FRAMES);
        if bytes <= budget {
            return Ok((PRELOAD_FRAMES, full));
        }
        let (mut best, bytes) = plan(MIN_PRELOAD);
        ensure!(
            bytes <= budget,
            "Resident sample data needs {} MiB even at the minimum preload; the bank limit is {} MiB",
            bytes >> 20,
            budget >> 20
        );
        // Resident bytes grow with the preload: bisect to 256-frame precision.
        let (mut fits, mut over) = (MIN_PRELOAD, PRELOAD_FRAMES);
        while over - fits > 256 {
            let mid = (fits + over) / 2;
            let (candidate, bytes) = plan(mid);
            if bytes <= budget {
                (fits, best) = (mid, candidate);
            } else {
                over = mid;
            }
        }
        Ok((fits, best))
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
        let mut plays = self.plays;
        for play in &mut plays {
            let spans = &samples[play.sample as usize].spans;
            let first = if play.map.reverse {
                play.map.end - 1
            } else {
                play.map.start
            };
            let span = spans
                .iter()
                .position(|s| s.start <= first && first < s.end());
            play.span = span.ok_or_else(|| anyhow::anyhow!("Zone start is not resident"))? as u32;
        }
        let any_solo = self.groups.iter().any(|g| g.soloed);
        let playable = self
            .groups
            .iter()
            .map(|g| !g.muted && (!any_solo || g.soloed))
            .collect();
        let mut key_start = [0u32; 129];
        let mut key_zones = Vec::new();
        for note in 0..128u8 {
            key_start[note as usize] = key_zones.len() as u32;
            let on_key = self
                .zones
                .iter()
                .enumerate()
                .filter(|(_, z)| (z.low_key..=z.high_key).contains(&note));
            key_zones.extend(on_key.map(|(i, _)| i as u32));
        }
        key_start[128] = key_zones.len() as u32;
        Ok(Bank {
            groups: self.groups,
            zones: self.zones,
            settings: self.settings,
            plays,
            playable,
            key_start,
            key_zones: key_zones.into_boxed_slice(),
            samples,
            preload: PRELOAD_FRAMES,
            voice_groups: self.voice_groups,
            polyphony: self.polyphony,
            streamer,
            bytes,
            skipped_zones: self.issues.skipped,
            issues: self.issues.notes,
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
/// and whether any zone path extends beyond them. Each zone keeps `preload`
/// frames past its furthest start offset; loops ending within four preloads
/// of the zone start stay resident, so short sustain loops never touch the
/// disk. Data past the furthest frame any zone plays is never needed.
fn spans(plays: &[&ZonePlay], preload: u64) -> Plan {
    let mut frames = 0;
    let mut ranges = Vec::with_capacity(plays.len());
    for play in plays {
        let map = &play.map;
        frames = frames.max(map.end);
        let head = preload + play.start_mod;
        if map.reverse {
            ranges.push(map.end.saturating_sub(head)..map.end);
            continue;
        }
        let mut range = map.start..map.start + head;
        if let Some(l) = map
            .looped
            .filter(|l| l.end <= map.start + 4 * preload && map.start < l.end)
        {
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
        return (std::iter::once(0..frames).collect(), false);
    }
    merged.retain_mut(|r| {
        r.end = r.end.min(frames);
        !r.is_empty()
    });
    (merged, true)
}

/// Run `f` over `items` on every core, keeping order. Each worker owns one
/// `S` scratch value across its items.
fn parallel<T: Send, S: Default, R: Send>(
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
