//! EQ bands and filters on summed audio. A section with fixed settings is
//! linear and time invariant, so it commutes with each voice's gain matrix:
//! voices whose groups carry the same sections on the same bus share one
//! chain, run once per span instead of once per voice. Bus and instrument
//! racks filter their own sums.
//!
//! A script, UI or editor write after init moves a group's inserts: at the
//! next span the group's own chain follows it in place, and a group on a
//! shared chain moves to its private chain (built at load) and follows there,
//! so groups nothing moves keep sharing. Inserts a route moves, and those up
//! to a group's last drive effect, run on each voice instead (voice_rack.rs).

use std::sync::Arc;

use kontra_ir as ir;

use kontra_effects::summed::{sounds, summed};

use crate::{Engine, MAX_SPAN, group_racks};

use crate::racks::Rack;
pub(crate) use kontra_racks::chain::{Chain, on_bus};

/// The most private chains one instrument builds for its shared groups.
const PRIVATE_CHAINS: usize = 256;

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
        let shared: Vec<Vec<(u8, ir::Effect)>> = self
            .groups
            .iter()
            .zip(&self.voice_units)
            .zip(&self.group_split)
            .map(|((g, units), split)| {
                let per_voice = |n: usize| units.iter().any(|u| usize::from(u.insert) == n);
                (g.inserts.iter().enumerate())
                    // The group's rack runs those from the split on.
                    .take(split.unwrap_or(usize::MAX))
                    .filter(|&(n, _)| !per_voice(n))
                    .filter_map(|(n, e)| Some((u8::try_from(n).ok()?, (*e)?)))
                    .filter(|(_, e)| summed(e))
                    .collect()
            })
            .collect();
        let mut chains: Vec<Chain> = Vec::new();
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        self.group_chain = (0..self.groups.len())
            .map(|g| {
                let sections = shared.get(g)?.clone();
                let split = self.group_split.get(g).copied().flatten();
                if !sections.iter().any(|(_, e)| sounds(e)) && split.is_none() {
                    return None;
                }
                let bus = self.groups.get(g).map_or(0, |g| g.bus);
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
        // Each group on a shared chain (or none) gets its private one, up to
        // PRIVATE_CHAINS per instrument (about 2.5 KB each).
        // ponytail: groups past the cap keep the shared chain and a write
        // after init to them is not heard; a pool taken on first write if a
        // library needs more.
        let mut left = PRIVATE_CHAINS;
        self.group_private = (0..self.groups.len())
            .map(|g| {
                let sections = shared.get(g).filter(|s| !s.is_empty())?;
                if self.group_split.get(g).copied().flatten().is_some() {
                    return None;
                }
                left = left.checked_sub(1)?;
                let bus = self.groups.get(g).map_or(0, |g| g.bus);
                let mut chain = Chain::new(bus, sections.clone(), None, rate);
                chain.private = true;
                chains.push(chain);
                u16::try_from(chains.len() - 1).ok()
            })
            .collect();
        self.group_moved = vec![false; self.groups.len()].into();
        self.chains = chains;
    }

    /// Marks group `g`'s inserts as written: the next span's chains follow.
    pub(crate) fn group_written(&mut self, g: usize) {
        if let Some(moved) = self.group_moved.get_mut(g) {
            *moved = true;
            self.groups_moved = true;
        }
    }

    /// The chains of each group written since the last span follow its
    /// inserts: its own chain (and twins) in place; a group on a shared chain
    /// whose inserts no longer match moves to its private chain first.
    fn follow_groups(&mut self) {
        if !std::mem::take(&mut self.groups_moved) {
            return;
        }
        let rate = self.rate;
        for (g, moved) in self.group_moved.iter_mut().enumerate() {
            if !std::mem::take(moved) {
                continue;
            }
            let Some(group) = self.groups.get(g) else {
                continue;
            };
            let inserts = &group.inserts[..];
            if self.group_split.get(g).copied().flatten().is_some() {
                for chain in self.chains.iter_mut().filter(|c| c.owner() == Some(g)) {
                    chain.follow(inserts, rate);
                }
                continue;
            }
            let Some(private) = self.group_private.get(g).copied().flatten() else {
                continue;
            };
            let Some(on) = self.group_chain.get_mut(g) else {
                continue;
            };
            let Some(chain) = self.chains.get_mut(usize::from(private)) else {
                continue;
            };
            // Until taken, the private chain holds the shared one's settings.
            if *on != Some(private) && chain.matches(inserts) {
                continue;
            }
            *on = Some(private);
            chain.follow(inserts, rate);
        }
    }

    /// Renders every voice into its chain or bus mix, then the chains.
    pub(crate) fn render_span(&mut self, len: usize) {
        if self.now.0.is_multiple_of(MAX_SPAN) {
            self.refresh_timed();
            self.rearm_timers();
        }
        self.follow_groups();
        for bus in &mut self.buses {
            if let Some(mix) = bus.mix.get_mut(..len) {
                mix.fill([0.0; 2]);
            }
        }
        for (_index, slot) in self
            .voices
            .iter_mut()
            .enumerate()
            .filter(|(_, s)| s.voice.is_active())
        {
            let zone = self.plan.zones.get(slot.zone);
            let bus = zone.map_or(0, |z| crate::plan::bus(&self.groups, z));
            let chain =
                zone.and_then(|z| self.group_chain.get(z.group as usize).copied().flatten());
            let chain = chain.and_then(|c| on_bus(&self.chains, usize::from(c), bus));
            let mix = match chain.and_then(|c| self.chains.get_mut(c)) {
                Some(chain) => {
                    if chain.private {
                        chain.bus = bus;
                    }
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
                slot.voice
                    .render(&mut self.voice_scratch, &self.stream, self.now.0, mix);
            } else {
                let group = self.plan.zones.get(slot.zone).map(|z| z.group as usize);
                let inserts = group
                    .and_then(|g| self.groups.get(g))
                    .map(|g| &g.inserts[..])
                    .unwrap_or_default();
                let (rack, spare, rate) = (&mut slot.rack, &mut *self.scratch, self.rate);
                let lines = &mut slot.comb_lines;
                slot.voice.render_through(
                    &mut self.voice_scratch,
                    &self.stream,
                    self.now.0,
                    mix,
                    |io| {
                        crate::voice_rack::process(rack, inserts, io, spare, lines, rate);
                    },
                );
            }
            self.stats.missing_frames += slot.voice.missing_frames() - before;
            #[cfg(feature = "trace")]
            if !slot.voice.is_active()
                && let Some(trace) = &mut self.trace
            {
                crate::trace::voice(
                    trace,
                    self.now.0 + len as u64,
                    _index,
                    slot,
                    crate::VoiceEvent::End,
                );
            }
        }
        self.mix_chains(len);
    }

    /// Filters each chain that sounded or still rings, into its bus.
    pub(crate) fn mix_chains(&mut self, len: usize) {
        let tempo = self.tempo;
        for chain in &mut self.chains {
            // A group rack may ring on (a reverb's tail): it always runs.
            if !chain.live && chain.group.is_none() && !chain.ringing {
                continue;
            }
            chain.live = false;
            chain.run(len);
            let Some(mix) = chain.mix.get_mut(..len) else {
                continue;
            };
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
