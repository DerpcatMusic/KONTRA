//! The engine's side of its racks (kontra-racks): response loads and the
//! mix through the audio graph, then the instrument send and main racks.

use std::sync::Arc;

use kontra_ir as ir;

use crate::{Engine, Plan};
pub(crate) use kontra_effects::processors::{Dsp, Responses, convolver, processor};
use kontra_effects::stem;
pub(crate) use kontra_racks::rack::{Live, Rack};

/// Pre-built per-group chain and voice-rack buffers prepared off the audio thread.
struct PreparedGroupRacks {
    mutated: Vec<(usize, Vec<Option<ir::Effect>>)>,
    voice_units: Box<[Arc<[crate::voice_rack::Unit]>]>,
    group_split: Box<[Option<usize>]>,
    chains: Vec<crate::filters::Chain>,
    group_chain: Box<[Option<u16>]>,
    group_private: Box<[Option<u16>]>,
    group_moved: Box<[bool]>,
    voice_racks: Vec<Vec<crate::voice_rack::Unit>>,
    voice_comb_lines: Vec<Vec<[f32; 2]>>,
    most_units: usize,
    comb_frames: usize,
}

/// Off-thread pre-built rack replacements for live insert-chain edits (insert,
/// remove, reorder, or parameter update) on program, bus, or group chains.
///
/// [`Engine::swap_racks`] swaps these into the live engine with zero
/// allocations on the audio thread; replaced racks, chains, and buffers move
/// back into this struct so they drop on the control thread.
#[derive(Default)]
pub struct PreparedRacks {
    program: Option<Rack>,
    buses: Vec<(usize, Rack)>,
    groups: Option<PreparedGroupRacks>,
    knobs: Option<Vec<(ir::RackAt, u8, kontra_behaviour::Positions)>>,
    dirty_knobs: Vec<ir::RackAt>,
}

impl PreparedRacks {
    /// Builds replacement racks and chains from `plan` at `rate` for the dirty
    /// targets (`program`, `dirty_buses`, `dirty_groups`). Not real-time.
    #[must_use]
    pub fn build(
        plan: &Plan,
        rate: u32,
        program: bool,
        dirty_buses: &[usize],
        dirty_groups: &[usize],
    ) -> Self {
        let rate = rate.max(1);
        let impulses = Responses(&plan.impulses, &[]);
        let mut dirty_knobs = Vec::new();

        let prog_rack = if program {
            dirty_knobs.push(ir::RackAt::Inserts);
            Some(Rack::new(&plan.inserts, rate, &plan.impulses))
        } else {
            None
        };

        let mut bus_racks = Vec::new();
        for &b in dirty_buses {
            if bus_racks.iter().any(|(idx, _)| *idx == b) {
                continue;
            }
            if let Some(bus) = plan.buses.get(b) {
                if let Some(bus_slot) = b.checked_sub(1).and_then(|v| u8::try_from(v).ok()) {
                    dirty_knobs.push(ir::RackAt::Bus(bus_slot));
                }
                let rack = Rack::routed(&bus.inserts, rate, impulses, &bus.mods, &bus.blends)
                    .with_lfos(&bus.lfos);
                bus_racks.push((b, rack));
            }
        }

        let groups = if dirty_groups.is_empty() {
            None
        } else {
            let mut mutated = Vec::new();
            for &g in dirty_groups {
                if mutated.iter().any(|(idx, _)| *idx == g) {
                    continue;
                }
                if let Some(group) = plan.groups.get(g) {
                    if let Ok(gid) = u32::try_from(g) {
                        dirty_knobs.push(ir::RackAt::Group(gid));
                    }
                    mutated.push((g, group.inserts.clone()));
                }
            }
            if mutated.is_empty() {
                None
            } else {
                let voice_units: Box<[Arc<[crate::voice_rack::Unit]>]> = plan
                    .groups
                    .iter()
                    .map(|g| crate::voice_rack::template(g, rate))
                    .collect();
                let has_graph = plan.graph.as_ref().is_some_and(|g| {
                    g.nodes
                        .iter()
                        .any(|n| matches!(n.kind, ir::GraphNodeKind::GroupInput { .. }))
                });
                let built = crate::filters::build_chains_for(
                    &plan.groups,
                    &voice_units,
                    &plan.impulses,
                    plan.buses.len(),
                    has_graph,
                    rate,
                );
                let most_units = voice_units.iter().map(|u| u.len()).max().unwrap_or(0);
                let comb_frames = voice_units
                    .iter()
                    .map(|u| crate::voice_rack::combs(u))
                    .max()
                    .unwrap_or(0)
                    * crate::voice_rack::comb_line(rate);
                let pool = 2 * plan.voices + 1;
                let voice_racks = if most_units > 0 {
                    (0..pool).map(|_| Vec::with_capacity(most_units)).collect()
                } else {
                    Vec::new()
                };
                let voice_comb_lines = if comb_frames > 0 {
                    (0..pool).map(|_| vec![[0.0; 2]; comb_frames]).collect()
                } else {
                    Vec::new()
                };
                Some(PreparedGroupRacks {
                    mutated,
                    voice_units,
                    group_split: built.group_split,
                    chains: built.chains,
                    group_chain: built.group_chain,
                    group_private: built.group_private,
                    group_moved: built.group_moved,
                    voice_racks,
                    voice_comb_lines,
                    most_units,
                    comb_frames,
                })
            }
        };

        let mut knobs = Vec::with_capacity(plan.slot_models.len() + crate::fx_types::REQUESTS * 2);
        for (at, slot, model) in &plan.slot_models {
            knobs.push((*at, *slot, kontra_behaviour::positions(model)));
        }

        Self {
            program: prog_rack,
            buses: bus_racks,
            groups,
            knobs: Some(knobs),
            dirty_knobs,
        }
    }

    /// Whether any rack or chain update is carried.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.program.is_none() && self.buses.is_empty() && self.groups.is_none()
    }
}

impl Engine {
    /// Atomically installs off-thread pre-built rack/chain replacements with
    /// zero allocations on the audio thread. Replaced processors and buffers
    /// swap back into `racks` to be dropped on the control thread.
    pub fn swap_racks(&mut self, racks: &mut PreparedRacks) {
        if let Some(prog) = &mut racks.program {
            std::mem::swap(&mut self.rack, prog);
        }
        for (b, rack) in &mut racks.buses {
            if let Some(bus) = self.buses.get_mut(*b) {
                std::mem::swap(&mut bus.rack, rack);
            }
        }
        if let Some(g) = &mut racks.groups {
            for (idx, inserts) in &mut g.mutated {
                if let Some(group) = self.groups.get_mut(*idx) {
                    std::mem::swap(&mut group.inserts, inserts);
                }
            }
            std::mem::swap(&mut self.voice_units, &mut g.voice_units);
            std::mem::swap(&mut self.group_split, &mut g.group_split);
            std::mem::swap(&mut self.chains, &mut g.chains);
            std::mem::swap(&mut self.group_chain, &mut g.group_chain);
            std::mem::swap(&mut self.group_private, &mut g.group_private);
            std::mem::swap(&mut self.group_moved, &mut g.group_moved);

            // Preserve any prior live edits on groups not in `g.mutated` by
            // flagging them for `follow_groups` on the next span.
            for gi in 0..self.groups.len() {
                if g.mutated.iter().any(|(m, _)| *m == gi) {
                    continue;
                }
                let differs = self
                    .groups
                    .get(gi)
                    .zip(self.plan.groups.get(gi))
                    .is_some_and(|(live, planned)| live.inserts != planned.inserts);
                if differs {
                    self.group_written(gi);
                }
            }

            // Grow per-voice scratch capacity using pre-allocated buffers when needed.
            for i in 0..self.voices.len() {
                let Some(slot) = self.voices.get_mut(i) else {
                    continue;
                };
                if g.most_units > slot.rack.capacity()
                    && let Some(buf) = g.voice_racks.get_mut(i)
                {
                    buf.clear();
                    buf.extend_from_slice(&slot.rack);
                    std::mem::swap(&mut slot.rack, buf);
                }
                if g.comb_frames > slot.comb_lines.len()
                    && let Some(buf) = g.voice_comb_lines.get_mut(i)
                {
                    let n = slot.comb_lines.len().min(buf.len());
                    if let (Some(dst), Some(src)) = (buf.get_mut(..n), slot.comb_lines.get(..n)) {
                        dst.copy_from_slice(src);
                    }
                    std::mem::swap(&mut slot.comb_lines, buf);
                }
            }

            // Re-sync currently sounding voices in mutated groups so live notes
            // immediately run the updated per-voice effect chain.
            let plan = Arc::clone(&self.plan);
            for i in 0..self.voices.len() {
                let active_in_mutated = self
                    .voices
                    .get(i)
                    .filter(|s| s.voice.is_active())
                    .and_then(|s| {
                        let zone = plan.zones.get(s.zone)?;
                        let group = zone.group as usize;
                        g.mutated.iter().any(|(m, _)| *m == group).then_some((
                            s.zone,
                            s.note?,
                            self.slot_timing(s),
                        ))
                    });
                if let Some((zone_idx, note_ref, age)) = active_in_mutated
                    && let (Some(zone), Some(note)) =
                        (plan.zones.get(zone_idx), self.notes.get(note_ref).copied())
                {
                    self.start_rack(i, zone, &note, age);
                }
            }
        }

        if let Some(knobs) = &mut racks.knobs {
            for (at, slot, pos) in knobs.iter_mut() {
                if !racks.dirty_knobs.contains(at)
                    && let Some((_, _, live)) = self
                        .behaviour
                        .knobs
                        .iter()
                        .find(|(a, s, _)| a == at && s == slot)
                {
                    *pos = *live;
                }
            }
            std::mem::swap(&mut self.behaviour.knobs, knobs);
        }

        self.refresh_all(None);
    }
}

impl Engine {
    /// `load_ir_sample`: the library response `name` (its extension and
    /// folder dropped) into the convolution at `slot` of the `generic` rack,
    /// prepared as the response it replaces. The answer: 0 when it plays now
    /// (in the init callback, decoded at once: the racks are rebuilt after
    /// it), or the async id of a load the host decodes off the audio thread
    /// ([`Engine::request_response`]); `None` when it cannot load.
    pub(crate) fn load_ir_sample(&mut self, name: &str, slot: i32, generic: i32) -> Option<i32> {
        // Borrowed, matched without case: this may run on the audio thread.
        let stem = stem(name);
        let plan = Arc::clone(&self.plan);
        let Some(found) = (plan.responses.iter()).position(|(n, _)| n.eq_ignore_ascii_case(stem))
        else {
            return self.load_ir_file(name, slot, generic);
        };
        let (_, sample) = plan.responses.get(found)?;
        let (playing, law) = self.playing(slot, generic)?;
        // 0 only when the slot holds it now; a match still loading is
        // asked again, for an async id.
        if playing.map(usize::from) == Some(found) && !self.pending(slot, generic) {
            return Some(0);
        }
        if !self.building && !self.running.is(crate::script::Kind::Init) {
            let index = u16::try_from(found).ok()?;
            return self.ask(slot, generic, index, law);
        }
        self.wanted.retain(|p| (p.0, p.1) != (slot, generic));
        self.install_response(stem, sample, (slot, generic), law)
    }

    /// `load_ir_sample` of an absolute path (the manual's first form), in the
    /// init callback only: the file is read here, on the control thread. After
    /// init the audio thread cannot read it, and no response of that name is in
    /// the plan, so it stays unanswered.
    fn load_ir_file(&mut self, path: &str, slot: i32, generic: i32) -> Option<i32> {
        if (!self.building && !self.running.is(crate::script::Kind::Init))
            || !path.contains(['/', '\\'])
        {
            return None;
        }
        let sample = crate::zone_edits::describe(path)?;
        let (_, law) = self.playing(slot, generic)?;
        self.wanted.retain(|p| (p.0, p.1) != (slot, generic));
        // The whole path is the key: two folders may hold the same file name.
        self.install_response(&path.to_lowercase(), &sample, (slot, generic), law)
    }

    /// Decodes `sample` as the response it replaces (once per key and law) and
    /// points the slot's convolution at it. Init callback only.
    fn install_response(
        &mut self,
        key: &str,
        sample: &ir::Sample,
        (slot, generic): (i32, i32),
        law: ir::Impulse,
    ) -> Option<i32> {
        let known =
            (self.loaded.iter()).position(|(n, l, _)| n.eq_ignore_ascii_case(key) && *l == law);
        let index = match known {
            Some(j) => j,
            None => {
                let r = crate::response(sample, &law, self.rate).ok()?;
                self.loaded.push((key.to_lowercase(), law, r));
                // Room for loads after init, which must not allocate.
                self.loaded.reserve(crate::fx_types::REQUESTS);
                self.loaded.len() - 1
            }
        };
        let index = u16::try_from(self.plan.impulses.len() + index).ok()?;
        if let Some(e) = self.effect_mut(-1, slot, generic) {
            e.kind = ir::EffectKind::Convolution { impulse: index };
        }
        Some(0)
    }
}

impl Engine {
    /// Rebuilds every rack's processors from its slots as the init callback
    /// left them. Not real-time.
    pub(crate) fn rebuild_racks(&mut self) {
        self.prepare_wanted();
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        let racks = std::iter::once(&mut self.rack)
            .chain(self.sends.iter_mut())
            .chain(std::iter::once(&mut self.main))
            .chain(self.buses.iter_mut().map(|b| &mut b.rack));
        for rack in racks {
            rack.rebuild(rate, Responses(&plan.impulses, &self.loaded));
        }
        (self.graph).rebuild(rate, Responses(&plan.impulses, &self.loaded));
    }

    /// Routes the span through the compiled [`ir::AudioGraph`] (buses, sends,
    /// stems) into `outputs[..][from..from+len]`, then runs the instrument
    /// racks and output stage on the main stem.
    /// Zero heap allocations on the audio thread.
    pub(crate) fn mix_out_multi(
        &mut self,
        outputs: &mut [&mut [[f32; 2]]],
        from: usize,
        len: usize,
    ) {
        let graph = &mut self.graph;
        const MAX_STEMS: usize = 16;
        let count = outputs.len().min(MAX_STEMS);
        let mut stems: [&mut [[f32; 2]]; MAX_STEMS] = std::array::from_fn(|_| &mut [][..]);
        for (dst, out) in stems.iter_mut().zip(outputs.iter_mut().take(count)) {
            if let Some(slice) = out.get_mut(from..from + len) {
                *dst = slice;
            }
        }
        graph.clear(len);
        if graph.has_group_inputs() {
            for (gi, gmix) in self.group_mix.iter().enumerate() {
                if let Some(slice) = gmix.get(..len) {
                    graph.add_group_audio(u16::try_from(gi).unwrap_or(u16::MAX), slice);
                }
            }
            if self.graph_redirected {
                for (bi, bus) in self.buses.iter().enumerate() {
                    if let Some(slice) = bus.mix.get(..len) {
                        graph.add_bus_audio(bi, slice);
                    }
                }
            }
        }
        for tap in &mut self.taps {
            tap.fill([0.0; 2]);
        }
        let live = Live {
            behaviour: &self.behaviour,
            scripts: &self.script_mods,
            now: self.now,
            seconds: (self.now.0 as f64 / f64::from(self.rate)) as f32,
            tempo: self.tempo,
        };
        let buses = kontra_racks::graph::Buses {
            buses: &mut self.buses,
            live: &live,
            taps: &mut self.taps,
            rate: self.rate,
        };
        if let Some(active) = stems.get_mut(..count) {
            graph.process(len, self.tempo, Some(buses), active);
        }
        if let Some(main_span) = stems.first_mut() {
            if self.rack.active() || self.sends.iter().any(Rack::active) || self.main.active() {
                let tempo = self.tempo;
                self.rack.process(main_span, &mut self.taps, tempo);
                for (send, tap) in self.sends.iter_mut().zip(self.taps.iter_mut()) {
                    if let Some(tap) = tap.get_mut(..len).filter(|_| send.active()) {
                        send.process(tap, &mut [], tempo);
                        for (o, t) in main_span.iter_mut().zip(tap.iter()) {
                            o[0] += t[0];
                            o[1] += t[1];
                        }
                    }
                }
                self.main.process(main_span, &mut [], tempo);
            }
            let (gain, pan) = (self.instrument.gain * self.expression, self.instrument.pan);
            self.output
                .process(main_span, gain, self.balance.gains(pan));
        }
    }
}
