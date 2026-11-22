//! Compact Structure-of-Arrays (SoA) zone table for cache-hot note-on
//! matching and compact cold playback metadata (IR vNext Phase 2).
//!
//! Hot note-on scans touch only the 8-byte [`ZoneMatch`] records (8 zones per
//! 64-byte cache line — 50,000 zones occupy just 400 KB and fit in L2 cache).
//! Cold playback fields live in 40-byte [`ZonePlay`] records, with variable-
//! length crossfades, zone envelopes, and start rules stored sparsely so
//! instruments with tens of thousands of plain zones pay zero per-zone heap
//! overhead for unused features.

use kontra_ir as ir;

use crate::{Atom, Crossfade, Trigger, Zone};

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

/// 40-byte cold playback record (read only for matched zones).
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

/// Compact SoA view of a plan's zones plus sparse side-tables.
#[derive(Clone, Debug, Default)]
pub struct ZoneTable {
    pub matches: Vec<ZoneMatch>,
    pub play: Vec<ZonePlay>,
    pub crossfades: Vec<Crossfade>,
    pub envelopes: Vec<(u32, ir::ZoneEnvelope)>,
    pub start_rules: Vec<(u32, Vec<(ir::Join, Atom)>)>,
}

impl ZoneTable {
    /// Packs a slice of [`Zone`]s into compact SoA arrays + sparse side-tables.
    #[must_use]
    pub fn from_zones(zones: &[Zone]) -> Self {
        let mut matches = Vec::with_capacity(zones.len());
        let mut play = Vec::with_capacity(zones.len());
        let mut crossfades = Vec::new();
        let mut envelopes = Vec::new();
        let mut start_rules = Vec::new();

        for (idx, z) in zones.iter().enumerate() {
            let zone_idx = u32::try_from(idx).unwrap_or(u32::MAX);
            matches.push(ZoneMatch::from_zone(z));

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
            crossfades,
            envelopes,
            start_rules,
        }
    }

    /// Number of zones in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    /// Whether the table contains zero zones.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// Total heap bytes held by the SoA arrays and sparse side-tables.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.matches.len() * std::mem::size_of::<ZoneMatch>()
            + self.play.len() * std::mem::size_of::<ZonePlay>()
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
        // 42,636 * (8 + 40) = 2,046,528 bytes (~1.95 MiB) — well under 2.5 MB!
        assert!(
            table.heap_bytes() < 2_500_000,
            "Areia-scale SoA table took {} bytes (limit 2.5 MB)",
            table.heap_bytes()
        );
        assert!(table.matches[0].matches(21, 64, |t| t == Trigger::Attack));
        assert!(!table.matches[0].matches(22, 64, |t| t == Trigger::Attack));
        assert!(!table.matches[0].matches(21, 64, |t| t == Trigger::Release));
    }
}
