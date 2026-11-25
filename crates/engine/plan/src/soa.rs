//! Compact Structure-of-Arrays (SoA) zone table for cache-hot note-on
//! matching, compact cold playback metadata, and zero-duplication script zone
//! queries (IR vNext Phase 2 / Lane L3).
//!
//! Hot note-on scans touch only the 8-byte [`ZoneMatch`] records (8 zones per
//! 64-byte cache line — 50,000 zones occupy just 400 KB and fit in L2 cache).
//! Cold playback fields live in 40-byte [`ZonePlay`] records, with zone IDs
//! (`4 B`), deduplicated sample indices (`4 B`), flat loop arenas, crossfades,
//! zone envelopes, start rules, and optional sampleless zone names stored
//! sparsely so instruments with tens of thousands of plain zones pay zero
//! per-zone heap allocations.

use std::ops::RangeInclusive;
use std::sync::Arc;

use kontra_ir::{self as ir, ZonePar};

use crate::zones::{PAN_RANGE, TUNE_RANGE, VOLUME_RANGE, zone_name};
use crate::{Atom, Crossfade, Trigger, Zone};

/// Sentinel in [`ZoneTable::sample_idx`] indicating a sampleless (e.g. oscillator) zone.
pub const NO_SAMPLE: u32 = u32::MAX;

/// 8-byte hot note-on scan record (8 zones per 64-byte cache line).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneMatch {
    pub key_lo: u8,
    pub key_hi: u8,
    pub vel_lo: u8,
    pub vel_hi: u8,
    /// Group index (saturated to `u16::MAX` if a plan exceeds 65,535 groups).
    pub group: u16,
    /// Low 2 bits: [`Trigger`]; bit 2: has non-empty start rules.
    pub flags: u8,
    /// Reserved for alignment/future rule tag.
    pub _pad: u8,
}

const _: () = assert!(std::mem::size_of::<ZoneMatch>() == 8);

impl ZoneMatch {
    const TRIGGER_MASK: u8 = 0b11;
    const HAS_START_RULES: u8 = 1 << 2;

    /// Builds a hot [`ZoneMatch`] entry from a prepared [`Zone`].
    #[must_use]
    pub fn from_zone(z: &Zone) -> Self {
        let trig = match z.trigger {
            Trigger::Attack => 0,
            Trigger::Release => 1,
            Trigger::First => 2,
            Trigger::Legato => 3,
        };
        let has_rules = if z.start.is_empty() {
            0
        } else {
            Self::HAS_START_RULES
        };
        Self {
            key_lo: *z.keys.start(),
            key_hi: *z.keys.end(),
            vel_lo: *z.velocities.start(),
            vel_hi: *z.velocities.end(),
            group: u16::try_from(z.group).unwrap_or(u16::MAX),
            flags: trig | has_rules,
            _pad: 0,
        }
    }

    /// Decoded [`Trigger`] stored in the low 2 flag bits.
    #[must_use]
    pub const fn trigger(self) -> Trigger {
        match self.flags & Self::TRIGGER_MASK {
            1 => Trigger::Release,
            2 => Trigger::First,
            3 => Trigger::Legato,
            _ => Trigger::Attack,
        }
    }

    /// Whether this zone has start conditions to evaluate in `Behaviour::starts`.
    #[must_use]
    pub const fn has_start_rules(self) -> bool {
        (self.flags & Self::HAS_START_RULES) != 0
    }

    /// Fast key-range check.
    #[must_use]
    pub const fn holds_key(self, key: u8) -> bool {
        key >= self.key_lo && key <= self.key_hi
    }

    /// Fast key + velocity + trigger check without touching cold [`Zone`] data.
    #[must_use]
    pub fn matches(self, key: u8, velocity: u8, accept: impl Fn(Trigger) -> bool) -> bool {
        key >= self.key_lo
            && key <= self.key_hi
            && velocity >= self.vel_lo
            && velocity <= self.vel_hi
            && accept(self.trigger())
    }
}

/// 40-byte cold playback record (read only for matched zones and script queries).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZonePlay {
    pub sample_rate: u32,
    pub start: u32,
    pub end: u32,
    pub start_mod: u32,
    pub gain: f32,
    pub pan: f32,
    pub tune_semitones: f32,
    pub root: u8,
    /// Bit 0: `one_shot`, bit 1: `reverse`, bit 2: `repeat_cuts`, bit 3: `offset_limited`.
    pub flags: u8,
    pub vel_fade: [u8; 2],
    pub key_fade: [u8; 2],
    pub slot: u16,
    pub crossfade_offset: u16,
    pub crossfade_len: u8,
    pub env_len: u8,
}

const _: () = assert!(std::mem::size_of::<ZonePlay>() == 40);

impl ZonePlay {
    pub const ONE_SHOT: u8 = 1 << 0;
    pub const REVERSE: u8 = 1 << 1;
    pub const REPEAT_CUTS: u8 = 1 << 2;
    pub const OFFSET_LIMITED: u8 = 1 << 3;
}

/// Parameters for inserting a saved zone row into [`ZoneTable`].
#[derive(Clone, Debug)]
pub struct SavedZoneDesc<'a> {
    pub id: u32,
    pub group: u32,
    pub keys: RangeInclusive<u8>,
    pub velocities: RangeInclusive<u8>,
    pub root: u8,
    pub start: u64,
    pub end: u64,
    pub start_mod: u64,
    pub sample_rate: u32,
    pub volume_db: f32,
    pub pan: f32,
    pub tune_semitones: f32,
    pub key_fade: (u8, u8),
    pub velocity_fade: (u8, u8),
    pub sample_idx: Option<u32>,
    /// Stored only when `sample_idx` is `None` (e.g. oscillator or test stub).
    pub custom_name: Option<&'a str>,
    pub loops: &'a [ir::Loop],
}

/// Compact SoA view of every saved zone (active, purged, or oscillator) plus
/// sparse side-tables, shared by scripts and host with zero duplication.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZoneTable {
    pub matches: Vec<ZoneMatch>,
    pub play: Vec<ZonePlay>,
    /// Saved zone IDs parallel to `matches` and `play` (4 B/zone).
    pub ids: Vec<u32>,
    /// Deduplicated sample index per zone, or [`NO_SAMPLE`] (4 B/zone).
    pub sample_idx: Vec<u32>,
    /// Shared deduplicated sample descriptors for saved zones.
    pub samples: Arc<[Arc<ir::Sample>]>,
    /// Flat loop arena shared across all saved zones (zero per-zone heap allocs).
    pub loops: Vec<ir::Loop>,
    /// Sparse `(zone_row, offset, len)` sorted by `zone_row` (0 B for unlooped zones).
    pub loop_spans: Vec<(u32, u32, u16)>,
    /// Sparse `(zone_row, name)` sorted by `zone_row` for sampleless zones only.
    pub custom_names: Vec<(u32, Box<str>)>,
    pub crossfades: Vec<Crossfade>,
    pub envelopes: Vec<(u32, ir::ZoneEnvelope)>,
    pub start_rules: Vec<(u32, Vec<(ir::Join, Atom)>)>,
}

impl ZoneTable {
    /// Pre-allocates columns for `cap` saved zones sharing `samples`.
    #[must_use]
    pub fn with_capacity(cap: usize, samples: Arc<[Arc<ir::Sample>]>) -> Self {
        Self {
            matches: Vec::with_capacity(cap),
            play: Vec::with_capacity(cap),
            ids: Vec::with_capacity(cap),
            sample_idx: Vec::with_capacity(cap),
            samples,
            loops: Vec::new(),
            loop_spans: Vec::new(),
            custom_names: Vec::new(),
            crossfades: Vec::new(),
            envelopes: Vec::new(),
            start_rules: Vec::new(),
        }
    }

    /// Appends one saved zone row (active, purged, or oscillator) with zero
    /// per-zone string or loop `Vec` allocations when backed by a sample.
    pub fn push_saved(&mut self, desc: SavedZoneDesc<'_>) {
        let row = u32::try_from(self.ids.len()).unwrap_or(u32::MAX);
        self.ids.push(desc.id);
        self.sample_idx.push(desc.sample_idx.unwrap_or(NO_SAMPLE));
        if desc.sample_idx.is_none()
            && let Some(name) = desc.custom_name
            && !name.is_empty()
        {
            self.custom_names.push((row, name.into()));
        }
        if !desc.loops.is_empty() {
            let offset = u32::try_from(self.loops.len()).unwrap_or(u32::MAX);
            let len = u16::try_from(desc.loops.len()).unwrap_or(u16::MAX);
            self.loops.extend_from_slice(desc.loops);
            self.loop_spans.push((row, offset, len));
        }
        self.matches.push(ZoneMatch {
            key_lo: *desc.keys.start(),
            key_hi: *desc.keys.end(),
            vel_lo: *desc.velocities.start(),
            vel_hi: *desc.velocities.end(),
            group: u16::try_from(desc.group).unwrap_or(u16::MAX),
            flags: 0,
            _pad: 0,
        });
        self.play.push(ZonePlay {
            sample_rate: desc.sample_rate,
            start: u32::try_from(desc.start).unwrap_or(u32::MAX),
            end: u32::try_from(desc.end).unwrap_or(u32::MAX),
            start_mod: u32::try_from(desc.start_mod).unwrap_or(u32::MAX),
            gain: 10f32.powf(desc.volume_db / 20.0),
            pan: desc.pan,
            tune_semitones: desc.tune_semitones,
            root: desc.root,
            flags: 0,
            vel_fade: [desc.velocity_fade.0, desc.velocity_fade.1],
            key_fade: [desc.key_fade.0, desc.key_fade.1],
            slot: 0,
            crossfade_offset: 0,
            crossfade_len: 0,
            env_len: 0,
        });
    }

    /// Packs a slice of [`Zone`]s into compact SoA arrays + sparse side-tables.
    #[must_use]
    pub fn from_zones(zones: &[Zone]) -> Self {
        let mut matches = Vec::with_capacity(zones.len());
        let mut play = Vec::with_capacity(zones.len());
        let mut ids = Vec::with_capacity(zones.len());
        let mut sample_idx = Vec::with_capacity(zones.len());
        let mut crossfades = Vec::new();
        let mut envelopes = Vec::new();
        let mut start_rules = Vec::new();

        for (idx, z) in zones.iter().enumerate() {
            let zone_idx = u32::try_from(idx).unwrap_or(u32::MAX);
            matches.push(ZoneMatch::from_zone(z));
            ids.push(z.id);
            sample_idx.push(NO_SAMPLE);

            let xf_offset = u16::try_from(crossfades.len()).unwrap_or(u16::MAX);
            let xf_len = u8::try_from(z.crossfades.len()).unwrap_or(u8::MAX);
            crossfades.extend_from_slice(&z.crossfades);

            for env in &z.envelopes {
                envelopes.push((zone_idx, env.clone()));
            }
            if !z.start.is_empty() {
                start_rules.push((zone_idx, z.start.clone()));
            }

            let mut flags = 0u8;
            if z.one_shot {
                flags |= ZonePlay::ONE_SHOT;
            }
            if z.region.reverse {
                flags |= ZonePlay::REVERSE;
            }
            if z.repeat_cuts {
                flags |= ZonePlay::REPEAT_CUTS;
            }
            if z.offset_limited {
                flags |= ZonePlay::OFFSET_LIMITED;
            }

            play.push(ZonePlay {
                sample_rate: z.sample_rate,
                start: u32::try_from(z.region.start).unwrap_or(u32::MAX),
                end: u32::try_from(z.region.end).unwrap_or(u32::MAX),
                start_mod: u32::try_from(z.start_mod).unwrap_or(u32::MAX),
                gain: z.gain,
                pan: z.pan,
                tune_semitones: z.tune as f32,
                root: z.root,
                flags,
                vel_fade: [z.fades[0].0, z.fades[0].1],
                key_fade: [z.fades[1].0, z.fades[1].1],
                slot: z.slot,
                crossfade_offset: xf_offset,
                crossfade_len: xf_len,
                env_len: u8::try_from(z.envelopes.len()).unwrap_or(u8::MAX),
            });
        }

        Self {
            matches,
            play,
            ids,
            sample_idx,
            samples: Arc::from([]),
            loops: Vec::new(),
            loop_spans: Vec::new(),
            custom_names: Vec::new(),
            crossfades,
            envelopes,
            start_rules,
        }
    }

    /// Number of saved zones in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    /// Whether the table contains zero saved zones.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Row index for saved zone `id`, if present. Zero allocation.
    #[must_use]
    pub fn row_of(&self, id: u32) -> Option<usize> {
        self.ids.iter().position(|&z| z == id)
    }

    /// Saved zone's stem name (derived on the fly from `samples` with zero allocation).
    #[must_use]
    pub fn name_at(&self, row: usize) -> &str {
        if let Some(sample) = self.sample_at(row) {
            return zone_name(&sample.name);
        }
        let Ok(key) = u32::try_from(row) else {
            return "";
        };
        self.custom_names
            .binary_search_by_key(&key, |(r, _)| *r)
            .ok()
            .and_then(|i| self.custom_names.get(i))
            .map_or("", |(_, name)| name.as_ref())
    }

    /// Saved zone's sample descriptor, if sample-backed.
    #[must_use]
    pub fn sample_at(&self, row: usize) -> Option<&Arc<ir::Sample>> {
        let idx = *self.sample_idx.get(row)?;
        if idx == NO_SAMPLE {
            return None;
        }
        self.samples.get(idx as usize)
    }

    /// Saved zone's loops slice in the flat loop arena (zero allocation).
    #[must_use]
    pub fn loops_at(&self, row: usize) -> &[ir::Loop] {
        let Ok(key) = u32::try_from(row) else {
            return &[];
        };
        let Ok(pos) = self.loop_spans.binary_search_by_key(&key, |(r, _, _)| *r) else {
            return &[];
        };
        let Some(&(_, off, len)) = self.loop_spans.get(pos) else {
            return &[];
        };
        let start = off as usize;
        let end = start.saturating_add(usize::from(len));
        self.loops.get(start..end).unwrap_or_default()
    }

    /// Finds the first saved zone whose name matches `name` exactly.
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<u32> {
        (0..self.len())
            .find(|&row| self.name_at(row) == name)
            .and_then(|row| self.ids.get(row).copied())
    }

    /// Reads a KSP `$ZONE_PAR_*` value directly from the SoA columns.
    #[must_use]
    pub fn get_par(&self, row: usize, par: ZonePar) -> Option<i32> {
        let m = *self.matches.get(row)?;
        let p = *self.play.get(row)?;
        let i32_u32 = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
        Some(match par {
            ZonePar::LowKey => i32::from(m.key_lo),
            ZonePar::HighKey => i32::from(m.key_hi),
            ZonePar::LowVelocity => i32::from(m.vel_lo),
            ZonePar::HighVelocity => i32::from(m.vel_hi),
            ZonePar::RootKey => i32::from(p.root),
            ZonePar::Group => i32::from(m.group),
            ZonePar::SampleStart => i32_u32(p.start),
            ZonePar::SampleEnd => i32_u32(p.end),
            ZonePar::StartRange => i32_u32(p.start_mod),
            ZonePar::SampleRate => i32_u32(p.sample_rate),
            ZonePar::Selected | ZonePar::Bpm => 0,
            ZonePar::Volume => (p.gain.max(1e-6).log10() * 2000.0).round() as i32,
            ZonePar::Pan => (p.pan * 1000.0).round() as i32,
            ZonePar::Tune => (p.tune_semitones * 100.0).round() as i32,
            ZonePar::FadeLowKey => i32::from(p.key_fade[0]),
            ZonePar::FadeHighKey => i32::from(p.key_fade[1]),
            ZonePar::FadeLowVelocity => i32::from(p.vel_fade[0]),
            ZonePar::FadeHighVelocity => i32::from(p.vel_fade[1]),
        })
    }

    /// Applies `set_zone_par` to a saved zone row in the SoA columns with
    /// Kontakt's clamps. Returns `false` for read-only parameters or invalid group.
    pub fn set_par(&mut self, row: usize, par: ZonePar, value: i32, group: Option<u32>) -> bool {
        let frames = self.sample_at(row).map_or_else(
            || u64::from(self.play.get(row).map_or(0, |p| p.end)),
            |s| s.frames,
        );
        let (Some(m), Some(p)) = (self.matches.get_mut(row), self.play.get_mut(row)) else {
            return false;
        };
        let key = u8::try_from(value.clamp(0, 127)).unwrap_or(0);
        let width = i32::from(m.key_hi) - i32::from(m.key_lo) + 1;
        let vwidth = i32::from(m.vel_hi) - i32::from(m.vel_lo) + 1;
        let bound =
            |v: i32| u32::try_from(u64::try_from(v).unwrap_or(0).min(frames)).unwrap_or(u32::MAX);
        match par {
            ZonePar::LowKey => {
                m.key_lo = key;
                m.key_hi = m.key_hi.max(key);
            }
            ZonePar::HighKey => {
                m.key_lo = m.key_lo.min(key);
                m.key_hi = key;
            }
            ZonePar::LowVelocity => {
                m.vel_lo = key;
                m.vel_hi = m.vel_hi.max(key);
            }
            ZonePar::HighVelocity => {
                m.vel_lo = m.vel_lo.min(key);
                m.vel_hi = key;
            }
            ZonePar::Group => match group {
                Some(g) => m.group = u16::try_from(g).unwrap_or(u16::MAX),
                None => return false,
            },
            ZonePar::RootKey => p.root = key,
            ZonePar::Volume => {
                let v = value.clamp(VOLUME_RANGE.0, VOLUME_RANGE.1);
                p.gain = 10f32.powf(v as f32 / 2000.0);
            }
            ZonePar::Pan => {
                let v = value.clamp(PAN_RANGE.0, PAN_RANGE.1);
                p.pan = v as f32 / 1000.0;
            }
            ZonePar::Tune => {
                let v = value.clamp(TUNE_RANGE.0, TUNE_RANGE.1);
                p.tune_semitones = v as f32 / 100.0;
            }
            ZonePar::FadeLowKey => crate::zones::share(&mut p.key_fade, 0, value, width),
            ZonePar::FadeHighKey => crate::zones::share(&mut p.key_fade, 1, value, width),
            ZonePar::FadeLowVelocity => crate::zones::share(&mut p.vel_fade, 0, value, vwidth),
            ZonePar::FadeHighVelocity => crate::zones::share(&mut p.vel_fade, 1, value, vwidth),
            ZonePar::SampleStart => {
                p.start = bound(value).min(p.end.saturating_sub(4));
            }
            ZonePar::SampleEnd => {
                let cap = u32::try_from(frames).unwrap_or(u32::MAX);
                p.end = bound(value).max(p.start.saturating_add(4).min(cap));
            }
            ZonePar::StartRange | ZonePar::SampleRate | ZonePar::Selected | ZonePar::Bpm => {
                return false;
            }
        }
        true
    }

    /// Reads `$LOOP_PAR_*` for saved zone at `row` from the flat loop arena.
    #[must_use]
    pub fn loop_par(&self, row: usize, index: i32, par: i32) -> i32 {
        let Ok(i) = usize::try_from(index) else {
            return -1;
        };
        if i >= 8 {
            return -1;
        }
        let Some(l) = self.loops_at(row).get(i) else {
            return 0;
        };
        let int = |n: u64| i32::try_from(n).unwrap_or(i32::MAX);
        match par {
            520 => 1,
            521 => int(l.start),
            522 => int(l.end.saturating_sub(l.start)),
            523 => int(l.crossfade),
            524 => int(l.count.into()),
            525 => (l.tune_ratio.max(1e-6).log2() * 1200.0).round() as i32,
            _ => 0,
        }
    }

    /// Total heap bytes held by the SoA arrays and sparse side-tables.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.matches.len() * std::mem::size_of::<ZoneMatch>()
            + self.play.len() * std::mem::size_of::<ZonePlay>()
            + self.ids.len() * std::mem::size_of::<u32>()
            + self.sample_idx.len() * std::mem::size_of::<u32>()
            + self.loops.len() * std::mem::size_of::<ir::Loop>()
            + self.loop_spans.len() * std::mem::size_of::<(u32, u32, u16)>()
            + self.custom_names.len() * std::mem::size_of::<(u32, Box<str>)>()
            + self
                .custom_names
                .iter()
                .map(|(_, s)| s.len())
                .sum::<usize>()
            + self.crossfades.len() * std::mem::size_of::<Crossfade>()
            + self.envelopes.len() * std::mem::size_of::<(u32, ir::ZoneEnvelope)>()
            + self.start_rules.len() * std::mem::size_of::<(u32, Vec<(ir::Join, Atom)>)>()
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn areia_scale_soa_table_fits_under_two_point_five_megabytes() {
        let store = kontra_stream::Store::new(kontra_stream::StoreConfig::DEFAULT);
        assert!(store.is_ok());
        let mut store = store.unwrap_or_else(|e| panic!("store: {e}"));
        let fmt = kontra_stream::SampleFmt {
            encoding: kontra_stream::Encoding::F32,
            channels: 1,
        };
        let sample = store
            .load(
                &kontra_stream::BlobKey {
                    file: "soa_test".into(),
                    offset: 0,
                    fmt,
                },
                Arc::new(kontra_stream::MemSource::new(fmt, vec![0u8; 64])),
                16,
                std::slice::from_ref(&(0..16)),
            )
            .unwrap_or_else(|e| panic!("load: {e}"));
        let region = kontra_voice::Region {
            sample,
            start: 0,
            end: 16,
            loops: None,
            reverse: false,
            mix: None,
            table: None,
        };
        let mut zones = Vec::with_capacity(42_636);
        for i in 0..42_636u32 {
            let mut z = Zone::new(region.clone(), 60, 48_000);
            let k = (i % 88 + 21) as u8;
            z.keys = k..=k;
            z.velocities = 1..=127;
            z.group = i % 128;
            zones.push(z);
        }
        let table = ZoneTable::from_zones(&zones);
        assert_eq!(table.len(), 42_636);
        // 42,636 * (8 + 40 + 4 + 4) = 2,387,616 bytes (~2.28 MiB) — under 2.5 MB!
        assert!(
            table.heap_bytes() < 2_500_000,
            "Areia-scale SoA table took {} bytes (limit 2.5 MB)",
            table.heap_bytes()
        );
        assert!(table.matches[0].matches(21, 64, |t| t == Trigger::Attack));
        assert!(!table.matches[0].matches(22, 64, |t| t == Trigger::Attack));
        assert!(!table.matches[0].matches(21, 64, |t| t == Trigger::Release));
    }

    #[test]
    fn soa_volume_pan_tune_round_trip_exactly_across_ksp_ranges() {
        let mut table = ZoneTable::with_capacity(1, Arc::from([]));
        table.push_saved(SavedZoneDesc {
            id: 42,
            group: 0,
            keys: 0..=127,
            velocities: 1..=127,
            root: 60,
            start: 0,
            end: 1000,
            start_mod: 0,
            sample_rate: 48_000,
            volume_db: 0.0,
            pan: 0.0,
            tune_semitones: 0.0,
            key_fade: (0, 0),
            velocity_fade: (0, 0),
            sample_idx: None,
            custom_name: Some("test"),
            loops: &[],
        });
        for v in VOLUME_RANGE.0..=VOLUME_RANGE.1 {
            assert!(table.set_par(0, ZonePar::Volume, v, None));
            assert_eq!(table.get_par(0, ZonePar::Volume), Some(v), "vol {v}");
        }
        for p in PAN_RANGE.0..=PAN_RANGE.1 {
            assert!(table.set_par(0, ZonePar::Pan, p, None));
            assert_eq!(table.get_par(0, ZonePar::Pan), Some(p), "pan {p}");
        }
        for t in TUNE_RANGE.0..=TUNE_RANGE.1 {
            assert!(table.set_par(0, ZonePar::Tune, t, None));
            assert_eq!(table.get_par(0, ZonePar::Tune), Some(t), "tune {t}");
        }
    }
}
