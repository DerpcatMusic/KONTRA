//! The engine's side of its racks (kontra-racks): response loads and the
//! mix through the buses, sends and main rack.

use std::sync::Arc;

use kontra_ir as ir;

use crate::Engine;
use crate::effects;
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
        let found = (plan.responses.iter()).position(|(n, _)| n.eq_ignore_ascii_case(stem))?;
        let (_, sample) = plan.responses.get(found)?;
        let (playing, law) = self.playing(slot, generic)?;
        if playing.map(usize::from) == Some(found) {
            return Some(0);
        }
        self.init_impulses.retain(|p| (p.0, p.1) != (slot, generic));
        if self.running.kind != crate::script::Kind::Init {
            let index = u16::try_from(found).ok()?;
            return self.request_response(slot, generic, index, law);
        }
        let known =
            (self.loaded.iter()).position(|(n, l, _)| n.eq_ignore_ascii_case(stem) && *l == law);
        let index = match known {
            Some(j) => j,
            None => {
                let r = crate::response(sample, &law, self.rate).ok()?;
                self.loaded.push((stem.to_lowercase(), law, r));
                // Room for loads after init, which must not allocate.
                self.loaded.reserve(crate::fx_types::REQUESTS);
                self.loaded.len() - 1
            }
        };
        let index = u16::try_from(plan.impulses.len() + index).ok()?;
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
        self.prepare_init_impulses();
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        let racks = std::iter::once(&mut self.rack)
            .chain(self.sends.iter_mut())
            .chain(std::iter::once(&mut self.main))
            .chain(self.buses.iter_mut().map(|b| &mut b.rack));
        for rack in racks {
            rack.rebuild(rate, Responses(&plan.impulses, &self.loaded));
        }
    }

    /// Adds the buses into `span` through their racks, then runs the
    /// instrument's insert, send and main racks on it.
    pub(crate) fn mix_out(&mut self, span: &mut [[f32; 2]]) {
        let (len, tempo) = (span.len(), self.tempo);
        for tap in &mut self.taps {
            tap.fill([0.0; 2]);
        }
        for at in 0..self.buses.len() {
            let (done, later) = self.buses.split_at_mut(at + 1);
            let Some(bus) = done.last_mut() else {
                continue;
            };
            if let Some(mix) = bus.mix.get_mut(..len) {
                let live = Live {
                    behaviour: &self.behaviour,
                    scripts: &self.script_mods,
                    now: self.now,
                };
                bus.rack.follow(&live);
                bus.rack.process(mix, &mut self.taps, tempo);
                // A send to an earlier bus would come a span late: none.
                // Pre-fader sends, the fader's glide, post-fader sends, the pan.
                for post in [false, true] {
                    if post {
                        bus.fader.process(mix, bus.gain, [1.0; 2]);
                    }
                    let sends = bus.sends.iter().enumerate();
                    for (n, send) in sends.filter(|(_, s)| s.post_fader == post) {
                        let Some(to) = send.bus.checked_sub(at + 1).and_then(|k| later.get_mut(k))
                        else {
                            continue;
                        };
                        // ponytail: a moved level steps once a span; ramp it if a ride zippers.
                        let level = send.level * bus.rack.send_scale(n);
                        for (o, m) in to.mix.iter_mut().zip(mix.iter()) {
                            o[0] += m[0] * level;
                            o[1] += m[1] * level;
                        }
                    }
                }
                let [[l, _], [_, r]] = effects::balance(bus.pan);
                bus.balance.process(mix, 1.0, [l, r]);
                for m in mix.iter() {
                    bus.peak = [bus.peak[0].max(m[0].abs()), bus.peak[1].max(m[1].abs())];
                }
            }
            if !bus.to_main {
                continue;
            }
            for (o, m) in span.iter_mut().zip(bus.mix.iter()) {
                o[0] += m[0];
                o[1] += m[1];
            }
        }
        self.rack.process(span, &mut self.taps, tempo);
        for (send, tap) in self.sends.iter_mut().zip(self.taps.iter_mut()) {
            if let Some(tap) = tap.get_mut(..len).filter(|_| send.active()) {
                send.process(tap, &mut [], tempo);
                for (o, t) in span.iter_mut().zip(tap.iter()) {
                    o[0] += t[0];
                    o[1] += t[1];
                }
            }
        }
        self.main.process(span, &mut [], tempo);
        let (gain, pan) = (self.instrument.gain * self.expression, self.instrument.pan);
        self.output.process(span, gain, self.balance.gains(pan));
    }
}
