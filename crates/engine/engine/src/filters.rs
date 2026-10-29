//! EQ bands and filters on summed audio. A section with fixed settings is
//! linear and time invariant, so it commutes with each voice's gain matrix:
//! voices whose groups carry the same sections on the same bus share one
//! chain, run once per span instead of once per voice. Bus and instrument
//! racks filter their own sums.
//!
//! ponytail: settings are fixed at load (after the init callback). Inserts
//! a route moves, and those up to a group's last drive effect, run on each
//! voice instead (voice_rack.rs).

use std::sync::Arc;

use kontra_ir as ir;

pub(crate) use kontra_effects::summed::{Stage, VoiceEq, run, sections, shape, stages};

use crate::racks::Rack;
use crate::{Engine, MAX_SPAN, group_racks};

pub(crate) struct Chain {
    bus: usize,
    sections: Vec<ir::EffectKind>,
    filters: Box<[Stage]>,
    /// A group's own chain, never shared: its rack runs the slots its
    /// voices leave to the sum (group_racks.rs).
    group: Option<(usize, Rack)>,
    pub(crate) mix: Box<[[f32; 2]]>,
    pub(crate) live: bool,
}

impl Chain {
    /// The group whose own chain this is, if any.
    fn owner(&self) -> Option<usize> {
        self.group.as_ref().map(|(g, _)| *g)
    }

    fn new(
        bus: usize,
        sections: Vec<ir::EffectKind>,
        group: Option<(usize, Rack)>,
        rate: u32,
    ) -> Self {
        Self {
            bus,
            filters: stages(&sections, rate),
            sections,
            group,
            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            live: false,
        }
    }
}

/// Chain `c`, or its twin on `bus` when a script moved the group there.
fn on_bus(chains: &[Chain], c: usize, bus: usize) -> Option<usize> {
    let chain = chains.get(c)?;
    if chain.bus == bus {
        return Some(c);
    }
    chains
        .iter()
        .position(|t| t.bus == bus && t.sections == chain.sections && t.owner() == chain.owner())
}

impl Engine {
    /// One chain per distinct (sections, bus) the zones' groups carry, less
    /// the inserts that run per voice.
    pub(crate) fn build_chains(&mut self) {
        self.build_voice_racks();
        self.group_split = self
            .groups
            .iter()
            .zip(&self.voice_units)
            .map(|(g, units)| group_racks::split(g, units))
            .collect();
        let shared: Vec<Vec<ir::EffectKind>> = self
            .groups
            .iter()
            .zip(&self.voice_units)
            .zip(&self.group_split)
            .map(|((g, units), split)| {
                let mut inserts = g.inserts.clone();
                for u in units.iter() {
                    if let Some(slot) = inserts.get_mut(usize::from(u.insert)) {
                        *slot = None;
                    }
                }
                // The group's rack runs these.
                inserts.truncate(split.unwrap_or(usize::MAX));
                sections(&inserts)
            })
            .collect();
        let mut chains: Vec<Chain> = Vec::new();
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        self.zone_chain = self
            .plan
            .zones
            .iter()
            .map(|z| {
                let g = z.group as usize;
                let sections = shared.get(g)?.clone();
                let split = self.group_split.get(g).copied().flatten();
                if sections.is_empty() && split.is_none() {
                    return None;
                }
                let bus = crate::plan::bus(&self.groups, z);
                let own = split.map(|_| g);
                let at = chains
                    .iter()
                    .position(|c| c.bus == bus && c.sections == sections && c.owner() == own)
                    .unwrap_or_else(|| {
                        let group = split.zip(self.groups.get(g)).map(|(from, group)| {
                            let slots = group_racks::slots(group, from);
                            (g, Rack::new(&slots, rate, &plan.impulses))
                        });
                        chains.push(Chain::new(bus, sections, group, rate));
                        chains.len() - 1
                    });
                u16::try_from(at).ok()
            })
            .collect();
        // A twin of each chain on every other bus, so a script that moves a
        // group to another bus allocates nothing on the audio thread.
        for n in 0..chains.len() {
            let Some((sections, own)) = chains.get(n).map(|c| (c.sections.clone(), c.owner()))
            else {
                continue;
            };
            for bus in 0..self.buses.len() {
                if !chains
                    .iter()
                    .any(|c| c.bus == bus && c.sections == sections && c.owner() == own)
                {
                    let group = own.and_then(|g| {
                        let from = self.group_split.get(g).copied().flatten()?;
                        let slots = group_racks::slots(self.groups.get(g)?, from);
                        Some((g, Rack::new(&slots, rate, &plan.impulses)))
                    });
                    chains.push(Chain::new(bus, sections.clone(), group, rate));
                }
            }
        }
        self.chains = chains;
    }

    /// Renders every voice into its chain or bus mix, then the chains.
    pub(crate) fn render_span(&mut self, len: usize) {
        if self.now.0.is_multiple_of(MAX_SPAN) {
            self.refresh_timed();
            self.rearm_timers();
        }
        for bus in &mut self.buses {
            if let Some(mix) = bus.mix.get_mut(..len) {
                mix.fill([0.0; 2]);
            }
        }
        for slot in self.voices.iter_mut().filter(|s| s.voice.is_active()) {
            let zone = self.plan.zones.get(slot.zone);
            let bus = zone.map_or(0, |z| crate::plan::bus(&self.groups, z));
            let chain = self.zone_chain.get(slot.zone).copied().flatten();
            let chain = chain.and_then(|c| on_bus(&self.chains, usize::from(c), bus));
            let mix = match chain.and_then(|c| self.chains.get_mut(c)) {
                Some(chain) => {
                    chain.live = true;
                    chain.mix.get_mut(..len)
                }
                None => self.buses.get_mut(bus).and_then(|b| b.mix.get_mut(..len)),
            };
            let Some(mix) = mix else {
                continue;
            };
            let before = slot.voice.missing_frames();
            if slot.rack.is_empty() {
                slot.voice.render(&self.stream, self.now.0, mix);
            } else {
                let group = self.plan.zones.get(slot.zone).map(|z| z.group as usize);
                let inserts = group
                    .and_then(|g| self.groups.get(g))
                    .map(|g| &g.inserts[..])
                    .unwrap_or_default();
                let (rack, spare, rate) = (&mut slot.rack, &mut *self.scratch, self.rate);
                slot.voice
                    .render_through(&self.stream, self.now.0, mix, |io| {
                        crate::voice_rack::process(rack, inserts, io, spare, rate);
                    });
            }
            self.stats.missing_frames += slot.voice.missing_frames() - before;
        }
        self.mix_chains(len);
    }

    /// Filters each chain that sounded or still rings, into its bus.
    pub(crate) fn mix_chains(&mut self, len: usize) {
        let tempo = self.tempo;
        for chain in &mut self.chains {
            // A group rack may ring on (a reverb's tail): it always runs.
            if !chain.live && chain.group.is_none() && chain.filters.iter().all(Stage::is_silent) {
                continue;
            }
            chain.live = false;
            let Some(mix) = chain.mix.get_mut(..len) else {
                continue;
            };
            run(&mut chain.filters, mix);
            if let Some((g, rack)) = &mut chain.group {
                // A script may have changed the group's slots since.
                if let Some(group) = self.groups.get(*g) {
                    for (slot, now) in rack.slots.iter_mut().zip(&group.inserts) {
                        if slot.is_some() {
                            *slot = *now;
                        }
                    }
                }
                rack.process(mix, &mut [], tempo);
            }
            if let Some(out) = self
                .buses
                .get_mut(chain.bus)
                .and_then(|b| b.mix.get_mut(..len))
            {
                for (o, m) in out.iter_mut().zip(mix.iter()) {
                    o[0] += m[0];
                    o[1] += m[1];
                }
            }
            mix.fill([0.0; 2]);
        }
    }
}
