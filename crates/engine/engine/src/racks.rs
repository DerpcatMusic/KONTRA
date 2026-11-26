//! The engine's side of its racks (kontra-racks): response loads and the
//! mix through the audio graph, then the instrument send and main racks.

use std::sync::Arc;

use kontra_ir as ir;

use crate::Engine;
pub(crate) use kontra_effects::processors::{Dsp, Responses, convolver, processor};
use kontra_effects::stem;
pub(crate) use kontra_racks::rack::{Live, Rack};

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
