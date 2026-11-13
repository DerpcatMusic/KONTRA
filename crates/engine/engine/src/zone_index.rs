//! Which zones a key can start, without reading every zone.

use kontra_plan::{Trigger, Zone};

/// Most (key, zone) pairs a per-key list may hold before the index falls
/// back to scanning the packed key ranges (a 32 MB list).
const MAX_LISTED: usize = 8 << 20;

/// Each zone's key range, packed, and per key the zones whose range holds it.
#[derive(Clone)]
pub(crate) struct ZoneIndex {
    spans: Vec<(u8, u8)>,
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
        let spans: Vec<(u8, u8)> = zones
            .iter()
            .map(|z| (*z.keys.start(), *z.keys.end()))
            .collect();
        let mut starts = [0u32; 129];
        let mut total = 0usize;
        for &(low, high) in &spans {
            for key in low..=high.min(127) {
                if let Some(n) = starts.get_mut(usize::from(key) + 1) {
                    *n += 1;
                    total += 1;
                }
            }
        }
        let listed = total <= MAX_LISTED;
        let mut flat = Vec::new();
        if listed {
            for k in 0..128 {
                starts[k + 1] += starts[k];
            }
            flat = vec![0u32; total];
            let mut fill = starts;
            for (zone, &(low, high)) in spans.iter().enumerate() {
                for key in low..=high.min(127) {
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
            spans,
            flat,
            starts,
            listed,
        }
    }

    /// Takes the key ranges `zones` now have, in place (no allocation); the
    /// per-key lists are then stale, so keys scan the ranges.
    pub(crate) fn refresh(&mut self, zones: &[Zone]) {
        for (span, zone) in self.spans.iter_mut().zip(zones) {
            *span = (*zone.keys.start(), *zone.keys.end());
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

    /// Whether zone `index`'s key range holds `key`.
    pub(crate) fn holds(&self, index: usize, key: u8) -> bool {
        self.spans
            .get(index)
            .is_none_or(|&(low, high)| key >= low && key <= high)
    }
}

fn uses_held(zones: &[Zone]) -> bool {
    zones
        .iter()
        .any(|z| matches!(z.trigger, Trigger::First | Trigger::Legato))
}
