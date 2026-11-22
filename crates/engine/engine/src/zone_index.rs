//! Which zones a key can start, without reading every cold [`Zone`] record.
//!
//! Uses 8-byte [`ZoneMatch`] records (8 zones per 64-byte cache line) so key,
//! velocity, and trigger filtering stays inside L1/L2 cache before touching
//! cold playback state.

use kontra_plan::soa::ZoneMatch;
use kontra_plan::{Trigger, Zone};

/// Most (key, zone) pairs a per-key list may hold before the index falls
/// back to scanning the packed [`ZoneMatch`] table (a 32 MB list).
const MAX_LISTED: usize = 8 << 20;

/// Each zone's hot 8-byte match record, plus per-key zone lists.
#[derive(Clone)]
pub(crate) struct ZoneIndex {
    matches: Vec<ZoneMatch>,
    /// Zone numbers for key `k`: `flat[starts[k]..starts[k + 1]]`, ascending.
    flat: Vec<u32>,
    starts: [u32; 129],
    listed: bool,
    /// Whether any zone starts by how many keys are held (first or legato).
    held: bool,
}

impl ZoneIndex {
    /// Not real-time.
    pub(crate) fn new(zones: &[Zone]) -> Self {
        let matches: Vec<ZoneMatch> = zones.iter().map(ZoneMatch::from_zone).collect();
        let mut starts = [0u32; 129];
        let mut total = 0usize;
        for m in &matches {
            for key in m.key_lo..=m.key_hi.min(127) {
                if let Some(n) = starts.get_mut(usize::from(key) + 1) {
                    *n += 1;
                    total += 1;
                }
            }
        }
        let listed = total <= MAX_LISTED;
        let mut flat = Vec::new();
        if listed {
            let mut run = 0;
            for n in &mut starts {
                run += *n;
                *n = run;
            }
            flat = vec![0u32; total];
            let mut fill = starts;
            for (zone, m) in matches.iter().enumerate() {
                for key in m.key_lo..=m.key_hi.min(127) {
                    if let Some(at) = fill.get_mut(usize::from(key)) {
                        if let Some(slot) = flat.get_mut(*at as usize) {
                            *slot = zone as u32;
                        }
                        *at += 1;
                    }
                }
            }
        }
        Self {
            held: uses_held(zones),
            matches,
            flat,
            starts,
            listed,
        }
    }

    /// Takes the match metadata `zones` now have, in place (no allocation); the
    /// per-key lists are then stale, so keys scan the packed [`ZoneMatch`] slice.
    pub(crate) fn refresh(&mut self, zones: &[Zone]) {
        for (entry, zone) in self.matches.iter_mut().zip(zones) {
            *entry = ZoneMatch::from_zone(zone);
        }
        self.listed = false;
        self.held = uses_held(zones);
    }

    /// Whether a note's start needs the count of keys held.
    pub(crate) fn uses_held(&self) -> bool {
        self.held
    }

    /// Zone numbers that may start on `key`, ascending, when listed.
    pub(crate) fn listed(&self, key: u8) -> Option<&[u32]> {
        if !self.listed {
            return None;
        }
        let k = usize::from(key);
        self.flat
            .get(*self.starts.get(k)? as usize..*self.starts.get(k + 1)? as usize)
    }

    /// Fast 8-byte SoA pre-filter on key, velocity, and trigger before reading
    /// the cold [`Zone`] record.
    pub(crate) fn pre_matches(
        &self,
        index: usize,
        key: u8,
        velocity: u8,
        accept: &impl Fn(Trigger) -> bool,
    ) -> bool {
        self.matches
            .get(index)
            .is_none_or(|m| m.matches(key, velocity, accept))
    }
}

fn uses_held(zones: &[Zone]) -> bool {
    zones
        .iter()
        .any(|z| matches!(z.trigger, Trigger::First | Trigger::Legato))
}
