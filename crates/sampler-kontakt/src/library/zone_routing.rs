//! Control-thread physical-zone preparation; no library-specific range guesses.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

// A retained bank is finite. Core independently checks candidate/state/job bytes.
const MAX_VARIANTS: usize = 8192;
pub(super) struct Routes {
    pub enabled: bool,
    pub mode: Option<u8>,
    homes: BTreeMap<u32, [i32; 3]>,
    groups: BTreeSet<usize>,
}
impl Routes {
    pub fn new(
        initialized: &crate::load::ScriptInit,
        original: &BTreeMap<u32, [i32; 3]>,
        groups: usize,
    ) -> Result<Self, String> {
        let mut homes = original.clone();
        let mut destinations = BTreeSet::new();
        let mut dynamic = false;
        let mut writes = false;
        let mut mode = sampler_ksp::model::SnapshotMode::default();
        for state in initialized
            .states
            .iter()
            .flatten()
            .filter_map(|s| s.as_ref().ok())
        {
            let need = state.zone_routing_requirements();
            writes |= need.writes;
            dynamic |= need.dynamic_groups;
            destinations.extend(
                need.destination_groups
                    .into_iter()
                    .filter_map(|g| usize::try_from(g).ok()),
            );
            homes.extend(
                state
                    .zone_parameters()
                    .iter()
                    .map(|(&id, &home)| (id, home)),
            );
            if let Some(write) = state.snapshot_mode_write() {
                mode = write.mode;
            }
        }
        if homes.len() != original.len()
            || homes.iter().any(|(id, h)| {
                !original.contains_key(id)
                    || usize::try_from(h[0]).ok().is_none_or(|g| g >= groups)
                    || !(0..=127).contains(&h[1])
                    || !(0..=127).contains(&h[2])
                    || h[1] > h[2]
            })
        {
            return Err("initializer zone homes exceed their physical source authority".into());
        }
        let enabled = writes && !mode.saves_engine();
        if enabled {
            if dynamic {
                destinations.extend(0..groups);
            }
            // Invalid literal targets stay unavailable and the actual call
            // reports failure. They cannot invent another group/template.
            destinations.retain(|g| *g < groups);
            let count = homes
                .values()
                .try_fold(0usize, |n, h| {
                    n.checked_add(
                        destinations.len() + usize::from(!destinations.contains(&(h[0] as usize))),
                    )
                })
                .ok_or("zone routing bank size overflow")?;
            if count > MAX_VARIANTS {
                return Err("zone routing bank exceeds its 8192-template preparation limit".into());
            }
        }
        Ok(Self {
            enabled,
            mode: enabled.then_some(if mode.reruns_init() { 2 } else { 3 }),
            homes,
            groups: destinations,
        })
    }
    pub fn home(&self, id: u32) -> Option<[i32; 3]> {
        self.homes.get(&id).copied()
    }
    pub fn apply_initial(&self, source_zone: u32, zone: &mut RawZone) -> Result<(), String> {
        let Some(&[group, low, high]) = self.homes.get(&source_zone) else {
            return Err("physical zone home missing before translation".into());
        };
        zone.group = usize::try_from(group).map_err(|_| "negative physical group")?;
        zone.keys = [low as u8, high as u8];
        zone.source.group = group as u32;
        zone.source.low_key = low as i16;
        zone.source.high_key = high as i16;
        Ok(())
    }
    pub fn destinations(&self, current: usize) -> impl Iterator<Item = usize> + '_ {
        self.groups
            .iter()
            .copied()
            .filter(move |group| *group != current)
    }
}
