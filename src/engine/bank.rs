//! Immutable instrument data prepared for playback.

use super::{
    map::{LoopMap, PlayMap},
    stream::Streamer,
    voice::Ahdsr,
};
use crate::{
    audio::{self, Frame, Sample, SampleReader, Source},
    import::{Group, Instrument, VoiceLimit, Zone},
};
use anyhow::{Result, bail, ensure};
use std::{collections::HashMap, ops::Range, path::PathBuf};

/// Resident sample memory ceiling per bank.
pub const MEMORY_LIMIT: usize = 1 << 30;
/// Frames of every sample kept in RAM (Kontakt's DFD preload). Voices start
/// from this instantly while the streamer fetches the rest; 8192 frames is
/// ≈170 ms at 48 kHz (64 KiB per sample as f32 stereo).
pub const PRELOAD_FRAMES: u64 = 8192;
/// Loops ending within this many frames of the zone start stay resident, so
/// short sustain loops never touch the disk.
const RESIDENT_LOOP: u64 = 4 * PRELOAD_FRAMES;
/// Voices per instrument when the program stores no limit.
const DEFAULT_POLYPHONY: usize = 512;

/// Per-group playback parameters the engine derives from import data.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSettings {
    /// Amplitude envelope; `None` uses the engine's attack/release defaults.
    pub envelope: Option<Ahdsr>,
    /// Velocity→volume depth: 0 ignores velocity, 1 is linear `velocity / 127`.
    pub velocity_intensity: f32,
    /// Pitch-bend range in semitones.
    pub bend_range: f32,
    /// Kontakt interpolation quality; every setting currently uses 4-point Hermite.
    pub interp_quality: i32,
    /// Index into the instrument's voice groups.
    pub voice_group: Option<u16>,
}

impl Default for GroupSettings {
    fn default() -> Self {
        Self {
            envelope: None,
            velocity_intensity: 1.0,
            bend_range: 2.0,
            interp_quality: 0,
            voice_group: None,
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
    pub data: Box<[Frame]>,
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
    /// Zones dropped because their sample is missing, damaged or out of bounds.
    pub skipped_zones: usize,
    /// First few reasons for skipped zones.
    pub issues: Vec<String>,
}

impl Bank {
    /// Load every group of `instrument`, streaming long samples from disk.
    pub fn load(instrument: &Instrument) -> Result<Self> {
        let mut sources = audio::Sources::default();
        let mut opened: HashMap<PathBuf, Option<u32>> = HashMap::new();
        let mut readers: Vec<(Source, SampleReader)> = Vec::new();
        let mut issues = Issues::default();
        let mut zones = Vec::new();
        let mut zone_samples = Vec::new();
        for zone in &instrument.zones {
            if !zone.available {
                issues.skip(format_args!("missing {}", zone.sample.display()));
                continue;
            }
            let id = *opened.entry(zone.sample.clone()).or_insert_with(|| {
                let opened = sources
                    .source(&zone.sample)
                    .and_then(|source| Ok((source.open()?, source)));
                match opened {
                    Ok((reader, source)) => {
                        readers.push((source, reader));
                        Some(readers.len() as u32 - 1)
                    }
                    Err(e) => {
                        issues.note(format_args!("{e:#}"));
                        None
                    }
                }
            });
            match id {
                Some(id) => {
                    zones.push(zone.clone());
                    zone_samples.push(id);
                }
                None => issues.skip(format_args!("unreadable {}", zone.sample.display())),
            }
        }
        let info = readers.iter().map(|(_, r)| (r.rate, r.frames)).collect();
        let mut builder =
            Builder::new(instrument.groups.clone(), zones, zone_samples, info, issues)?;
        builder.limits(instrument);
        let mut samples = Vec::with_capacity(readers.len());
        let mut streamed = Vec::with_capacity(readers.len());
        let mut bytes = 0;
        for (id, (source, mut reader)) in readers.into_iter().enumerate() {
            let (spans, streamed_sample) = builder.spans(id);
            let resident: u64 = spans.iter().map(|s| s.end - s.start).sum();
            bytes += resident as usize * size_of::<Frame>();
            ensure!(
                bytes <= MEMORY_LIMIT,
                "Resident sample data exceeds the {} MiB bank limit",
                MEMORY_LIMIT >> 20
            );
            let read = spans
                .into_iter()
                .map(|range| {
                    let mut data =
                        vec![[0.0; 2]; (range.end - range.start) as usize].into_boxed_slice();
                    reader.read(range.start, &mut data)?;
                    Ok(Span {
                        start: range.start,
                        data,
                    })
                })
                .collect::<Result<Vec<_>>>();
            let (spans, streamed_sample) = match read {
                Ok(spans) => (spans, streamed_sample),
                Err(e) => {
                    bytes -= resident as usize * size_of::<Frame>();
                    builder.drop_sample(id, &e);
                    (Vec::new(), false)
                }
            };
            let sample = SampleData {
                rate: reader.rate,
                spans,
                streamed: streamed_sample,
            };
            streamed.push(streamed_sample.then_some(source));
            samples.push(sample);
        }
        let streamer = if streamed.iter().any(Option::is_some) {
            bytes += Streamer::BYTES;
            Some(Streamer::spawn(streamed)?)
        } else {
            None
        };
        builder.finish(samples, streamer, bytes)
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
        let bytes = samples
            .iter()
            .map(|(_, s)| s.frames.len() * size_of::<Frame>())
            .sum();
        let samples = samples
            .into_iter()
            .map(|(_, s)| SampleData {
                rate: s.rate,
                spans: vec![Span {
                    start: 0,
                    data: s.frames.into_boxed_slice(),
                }],
                streamed: false,
            })
            .collect();
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
                    let start_mod = u64::from(zone.start_mod).min(map.end - map.start - 1);
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
        let settings = groups.iter().map(|_| GroupSettings::default()).collect();
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
            settings.interp_quality = group.interp_quality;
            settings.voice_group = u16::try_from(group.voice_group).ok().filter(|&v| {
                self.voice_groups
                    .get(v as usize)
                    .is_some_and(Option::is_some)
            });
        }
    }

    /// Resident frame ranges of sample `id`, merged and sorted, and whether
    /// any zone path extends beyond them. `frames` becomes the furthest frame
    /// any zone plays; data past it is never needed.
    fn spans(&self, id: usize) -> (Vec<Range<u64>>, bool) {
        let mut frames = 0;
        let mut ranges = Vec::new();
        for play in self.plays.iter().filter(|p| p.sample as usize == id) {
            let map = &play.map;
            frames = frames.max(map.end);
            let head = PRELOAD_FRAMES + play.start_mod;
            if map.reverse {
                ranges.push(map.end.saturating_sub(head)..map.end);
                continue;
            }
            let mut range = map.start..map.start + head;
            if let Some(l) = map
                .looped
                .filter(|l| l.end <= map.start + RESIDENT_LOOP && map.start < l.end)
            {
                range.start = range.start.min(l.start - l.xfade);
                range.end = range
                    .end
                    .max(l.end + if l.until_release { PRELOAD_FRAMES } else { 0 });
            }
            ranges.push(range);
        }
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<u64>> = Vec::new();
        for range in ranges {
            match merged.last_mut() {
                // Bridge small gaps: one span is cheaper than a stream restart.
                Some(last) if range.start <= last.end + PRELOAD_FRAMES / 2 => {
                    last.end = last.end.max(range.end)
                }
                _ => merged.push(range),
            }
        }
        let covered: u64 = merged
            .iter()
            .map(|r| r.end.min(frames) - r.start.min(frames))
            .sum();
        if frames <= 2 * PRELOAD_FRAMES || covered + PRELOAD_FRAMES >= frames {
            return (vec![0..frames], false);
        }
        (
            merged
                .iter()
                .map(|r| r.start..r.end.min(frames))
                .filter(|r| !r.is_empty())
                .collect(),
            true,
        )
    }

    /// Skip every zone of a sample whose data turned out to be damaged.
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
            if ls < start || ls >= le || le > end {
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
