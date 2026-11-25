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

/// The most private chains one instrument builds for its shared groups: a
/// sanity ceiling (about 10 MB) far past the largest library's groups.
const PRIVATE_CHAINS: usize = 4096;

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
                    .filter_map(|(n, e)| Some((u8::try_from(n).ok()?, e.clone()?)))
                    .filter(|(_, e)| summed(e))
                    .collect()
            })
            .collect();
        let mut chains: Vec<Chain> = Vec::new();
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        let has_graph = self.graph_groups();
        self.group_chain = (0..self.groups.len())
            .map(|g| {
                let sections = shared.get(g)?.clone();
                let split = self.group_split.get(g).copied().flatten();
                if !sections.iter().any(|(_, e)| sounds(e)) && split.is_none() {
                    return None;
                }
                if has_graph {
                    // In graph mode every group with inserts owns a dedicated chain
                    // feeding `group_mix[g]` (`bus = usize::MAX`), so groups stay
                    // isolated before `GroupInput { group: g }` and `effect_mut`
                    // updates follow in place.
                    let group = split.zip(self.groups.get(g)).map(|(from, group)| {
                        let slots = group_racks::slots(group, from);
                        (g, Rack::new(&slots, rate, &plan.impulses))
                    });
                    let mut chain = Chain::new(usize::MAX, sections, group, rate);
                    chain.graph_group = Some(g);
                    chain.to_graph_group = true;
                    chains.push(chain);
                    return u16::try_from(chains.len() - 1).ok();
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
        // group (or redirects a note) to another bus allocates nothing on the audio thread.
        for n in 0..chains.len() {
            let Some((sections, own, graph_group)) = chains
                .get(n)
                .map(|c| (c.sections.clone(), c.owner(), c.graph_group))
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
                    let mut twin = Chain::new(bus, sections.clone(), group, rate);
                    twin.graph_group = graph_group;
                    chains.push(twin);
                }
            }
        }
        // Each group on a shared chain (or none) gets its private one, up to
        // PRIVATE_CHAINS per instrument (about 2.5 KB each), built here at
        // load so a write after init allocates nothing.
        let mut left = PRIVATE_CHAINS;
        self.group_private = (0..self.groups.len())
            .map(|g| {
                if has_graph {
                    return None;
                }
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
        let (rate, graph_groups) = (self.rate, self.graph_groups());
        for (g, moved) in self.group_moved.iter_mut().enumerate() {
            if !std::mem::take(moved) {
                continue;
            }
            let Some(group) = self.groups.get(g) else {
                continue;
            };
            let inserts = &group.inserts[..];
            // Its own racks take the written slots; they run them every span.
            for chain in &mut self.chains {
                if let Some((owner, rack)) = &mut chain.group
                    && *owner == g
                {
                    for (slot, now) in rack.slots.iter_mut().zip(inserts) {
                        if slot.is_some() {
                            slot.clone_from(now);
                        }
                    }
                }
            }
            if self.group_split.get(g).copied().flatten().is_some() || graph_groups {
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
        for gmix in &mut self.group_mix {
            if let Some(mix) = gmix.get_mut(..len) {
                mix.fill([0.0; 2]);
            }
        }
        self.graph_redirected = false;
        let has_graph = self.graph_groups();
        let live_end = self.live_end.min(self.voices.len());
        for (_index, slot) in self
            .voices
            .iter_mut()
            .take(live_end)
            .enumerate()
            .filter(|(_, s)| s.voice.is_active())
        {
            let zone = self.plan.zones.get(slot.zone);
            let redirect = (slot.note.and_then(|n| self.notes.get(n)))
                .and_then(|n| n.redirect)
                .map(usize::from)
                .filter(|&b| b < self.buses.len());
            let bus =
                redirect.unwrap_or_else(|| zone.map_or(0, |z| crate::plan::bus(&self.groups, z)));
            let mix = if has_graph {
                if let Some(target_bus) = redirect {
                    self.graph_redirected = true;
                    let chain = zone
                        .and_then(|z| self.group_chain.get(z.group as usize).copied().flatten())
                        .and_then(|c| on_bus(&self.chains, usize::from(c), target_bus));
                    match chain.and_then(|c| self.chains.get_mut(c)) {
                        Some(chain) => {
                            chain.live = true;
                            chain.mix.get_mut(..len)
                        }
                        None => self
                            .buses
                            .get_mut(target_bus)
                            .and_then(|b| b.mix.get_mut(..len)),
                    }
                } else if let Some(g) = zone.map(|z| z.group as usize) {
                    let chain = self.group_chain.get(g).copied().flatten();
                    match chain.and_then(|c| self.chains.get_mut(usize::from(c))) {
                        Some(chain) => {
                            chain.live = true;
                            chain.mix.get_mut(..len)
                        }
                        None => self.group_mix.get_mut(g).and_then(|m| m.get_mut(..len)),
                    }
                } else {
                    None
                }
            } else {
                let chain =
                    zone.and_then(|z| self.group_chain.get(z.group as usize).copied().flatten());
                let chain = chain.and_then(|c| on_bus(&self.chains, usize::from(c), bus));
                match chain.and_then(|c| self.chains.get_mut(c)) {
                    Some(chain) => {
                        if chain.private {
                            chain.bus = bus;
                        }
                        chain.live = true;
                        chain.mix.get_mut(..len)
                    }
                    None => self.buses.get_mut(bus).and_then(|b| b.mix.get_mut(..len)),
                }
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
                let (inserts, amp_split) = group
                    .and_then(|g| self.groups.get(g))
                    .map_or((&[][..], u8::MAX), |g| (&g.inserts[..], g.amp_split));
                let split_at = slot.rack.partition_point(|u| u.insert < amp_split);
                let (pre_rack, post_rack) = slot.rack.split_at_mut(split_at);
                let (spare, rate) = (&mut *self.scratch, self.rate);
                let lines = &mut slot.comb_lines;
                if post_rack.is_empty() {
                    slot.voice.render_through(
                        &mut self.voice_scratch,
                        &self.stream,
                        self.now.0,
                        mix,
                        |io| {
                            crate::voice_rack::process(pre_rack, inserts, io, spare, lines, rate);
                        },
                    );
                } else {
                    slot.voice.render_through_stages(
                        &mut self.voice_scratch,
                        &self.stream,
                        self.now.0,
                        mix,
                        |stage, io| match stage {
                            kontra_voice::VoiceStage::PreAmp if !pre_rack.is_empty() => {
                                crate::voice_rack::process(
                                    pre_rack, inserts, io, spare, lines, rate,
                                );
                            }
                            kontra_voice::VoiceStage::PostAmp => {
                                crate::voice_rack::process(
                                    post_rack, inserts, io, spare, lines, rate,
                                );
                            }
                            _ => {}
                        },
                    );
                }
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
        let ended = self
            .voices
            .iter()
            .take(live_end)
            .rposition(|s| s.voice.is_active());
        self.live_end = ended.map_or(0, |i| i + 1);
        self.mix_chains(len);
    }

    /// Filters each chain that sounded or still rings, into its bus (or group mix in graph mode).
    pub(crate) fn mix_chains(&mut self, len: usize) {
        let tempo = self.tempo;
        let has_graph = self.graph_groups();
        for chain in &mut self.chains {
            // A group rack may ring on (a reverb's tail): it always runs.
            if !chain.live && chain.group.is_none() && !chain.ringing {
                continue;
            }
            chain.live = false;
            chain.run(len);
            let owner = chain.owner();
            let to_graph_group = chain.to_graph_group;
            let chain_bus = chain.bus;
            let Some(mix) = chain.mix.get_mut(..len) else {
                continue;
            };
            if let Some((_, rack)) = &mut chain.group {
                // A script's writes reached its slots in `follow_groups`.
                rack.process(mix, &mut [], tempo);
            }
            if has_graph && to_graph_group {
                if let Some(out) = owner
                    .and_then(|g| self.group_mix.get_mut(g))
                    .and_then(|m| m.get_mut(..len))
                {
                    for (o, m) in out.iter_mut().zip(mix.iter()) {
                        o[0] += m[0];
                        o[1] += m[1];
                    }
                }
            } else if let Some(out) = self
                .buses
                .get_mut(chain_bus)
                .and_then(|b| b.mix.get_mut(..len))
            {
                if has_graph {
                    self.graph_redirected = true;
                }
                for (o, m) in out.iter_mut().zip(mix.iter()) {
                    o[0] += m[0];
                    o[1] += m[1];
                }
            }
            mix.fill([0.0; 2]);
        }
    }
}

impl Engine {
    /// Whether groups enter the audio graph through their own inputs (a
    /// `.kontra` graph) rather than through the instrument buses.
    pub(crate) fn graph_groups(&self) -> bool {
        self.graph.has_group_inputs()
    }
}
