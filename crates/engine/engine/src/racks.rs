//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use std::sync::Arc;

use kontra_dsp::core::Levels;
use kontra_ir as ir;

use crate::effects;
use crate::filters;
use crate::ledger::{self, Ramp};
use crate::{Engine, Response, SampleTime};
use kontra_behaviour::Behaviour;
pub(crate) use kontra_effects::processors::{Dsp, Responses, convolver, processor};
use kontra_effects::stem;

pub(crate) struct Rack {
    /// The plan's slots as scripts have changed them.
    pub(crate) slots: Box<[Option<ir::Effect>]>,
    dsp: Box<[Dsp]>,
    /// Routes moving the slots ([`ir::Bus::mods`]), the blends they read,
    /// and each route's amount for this span.
    mods: Box<[ir::ModKind]>,
    blends: Box<[ir::Blend]>,
    amounts: Box<[f32]>,
    /// Each slot's levels, gliding where its settings move.
    levels: Box<[Levels]>,
    rate: u32,
}

/// The sources a rack's routes read: those no note owns.
pub(crate) struct Live<'a> {
    pub(crate) behaviour: &'a Behaviour,
    pub(crate) scripts: &'a [Option<(u16, Ramp)>],
    pub(crate) now: SampleTime,
}

impl Live<'_> {
    /// Controllers and pitch bend come from the channel last played.
    fn read(&self, source: ir::Source, blends: &[ir::Blend]) -> Option<f32> {
        let (b, channel) = (self.behaviour, self.behaviour.channel);
        Some(match source {
            ir::Source::Controller(cc) => f32::from(b.cc(channel, cc)) / 127.0,
            ir::Source::ControllerFrom { cc, start } => {
                let arrived = b.cc_arrived(channel, cc);
                f32::from(if arrived { b.cc(channel, cc) } else { start }) / 127.0
            }
            ir::Source::PitchBend => b.bend(channel),
            ir::Source::Script(i) => {
                ledger::ramp_at(self.scripts, i, self.now).unwrap_or(0) as f32 / 1_000_000.0
            }
            ir::Source::Constant => 1.0,
            ir::Source::Blend(n) => {
                let b = blends.get(usize::from(n))?;
                let mut sum = b.base;
                for t in &b.terms {
                    let mut x = t.amount * self.through(&t.input)?;
                    for f in &t.factors {
                        x *= f.at(self.through(&f.input)?);
                    }
                    sum += x;
                }
                sum
            }
            _ => return None,
        })
    }

    /// `t`'s source through its table; a bipolar source spans a table as
    /// `(x + 1)/2`. No blend: no recursion.
    fn through(&self, t: &ir::Transfer) -> Option<f32> {
        let x = self.read(t.source, &[])?;
        let bipolar = matches!(t.source, ir::Source::PitchBend) && !t.table.is_empty();
        Some(t.apply(if bipolar { (x + 1.0) / 2.0 } else { x }))
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

impl Rack {
    /// `impulses` are the plan's convolution responses. Not real-time.
    pub(crate) fn new(
        slots: &[Option<ir::Effect>],
        rate: u32,
        impulses: &[Option<Response>],
    ) -> Self {
        Self::routed(slots, rate, Responses(impulses, &[]), &[], &[])
    }

    /// As [`Self::new`], its slots moved by `mods` reading `blends`.
    pub(crate) fn routed(
        slots: &[Option<ir::Effect>],
        rate: u32,
        impulses: Responses<'_>,
        mods: &[ir::ModKind],
        blends: &[ir::Blend],
    ) -> Self {
        let moved = |n: usize| {
            mods.iter().any(
                |m| matches!(m, ir::ModKind::EffectParam { insert, .. } if usize::from(*insert) == n),
            )
        };
        let dsp = slots
            .iter()
            .enumerate()
            .map(|(n, slot)| processor(*slot, moved(n), rate, &impulses))
            .collect();
        Self {
            slots: slots.into(),
            dsp,
            mods: mods.into(),
            blends: blends.into(),
            amounts: vec![0.0; mods.len()].into(),
            levels: vec![Levels::new(rate); slots.len()].into(),
            rate,
        }
    }

    /// Puts `effect` and its processor in slot `n`; the old pair comes back.
    pub(crate) fn install(
        &mut self,
        n: usize,
        effect: Option<ir::Effect>,
        dsp: Dsp,
    ) -> Result<(Option<ir::Effect>, Dsp), Dsp> {
        let (Some(slot), Some(old)) = (self.slots.get_mut(n), self.dsp.get_mut(n)) else {
            return Err(dsp);
        };
        Ok((std::mem::replace(slot, effect), std::mem::replace(old, dsp)))
    }

    /// Reads its routes' sources for the next span.
    pub(crate) fn follow(&mut self, live: &Live) {
        for (m, amount) in self.mods.iter().zip(self.amounts.iter_mut()) {
            *amount = match *m {
                ir::ModKind::EffectParam { source, depth, .. } => {
                    live.read(source, &self.blends).unwrap_or(0.0) * depth
                }
                ir::ModKind::Send { source, .. } => live.read(source, &self.blends).unwrap_or(1.0),
                _ => continue,
            };
        }
    }

    /// Effect configurations with this rack's last applied route amounts.
    pub(crate) fn inspect_slots(&self) -> Vec<Option<ir::Effect>> {
        self.slots
            .iter()
            .copied()
            .enumerate()
            .map(|(n, slot)| {
                let mut effect = slot?;
                for (m, &amount) in self.mods.iter().zip(self.amounts.iter()) {
                    if let ir::ModKind::EffectParam { insert, param, .. } = *m
                        && usize::from(insert) == n
                    {
                        param.add_to(&mut effect, amount);
                    }
                }
                Some(effect)
            })
            .collect()
    }

    /// Route values last applied to this rack, for offline inspection.
    pub(crate) fn route_amounts(&self) -> impl Iterator<Item = (ir::ModKind, f32)> + '_ {
        self.mods.iter().copied().zip(self.amounts.iter().copied())
    }

    /// What its routes scale send `n`'s level by this span.
    pub(crate) fn send_scale(&self, n: usize) -> f32 {
        self.mods
            .iter()
            .zip(self.amounts.iter())
            .filter(|(m, _)| matches!(m, ir::ModKind::Send { send, .. } if usize::from(*send) == n))
            .map(|(_, a)| a)
            .product()
    }

    /// Processes `io`; a send slot adds it to `taps` (one buffer per send slot).
    pub(crate) fn process(
        &mut self,
        io: &mut [[f32; 2]],
        taps: &mut [Box<[[f32; 2]]>],
        tempo: f32,
    ) {
        let slots = self.slots.iter().zip(self.dsp.iter_mut());
        for (n, ((slot, dsp), levels)) in slots.zip(self.levels.iter_mut()).enumerate() {
            let Some(mut e) = slot.filter(|e| !e.bypass) else {
                continue;
            };
            for (m, &amount) in self.mods.iter().zip(self.amounts.iter()) {
                if let ir::ModKind::EffectParam { insert, param, .. } = *m
                    && usize::from(insert) == n
                {
                    param.add_to(&mut e, amount);
                }
            }
            match (dsp, e.kind) {
                (Dsp::Eq(f), _) => filters::run(f, io),
                (Dsp::Filter(svf), ir::EffectKind::Filter(f)) => {
                    svf.retune(filters::shape(&f), self.rate);
                    svf.process(io);
                }
                // ponytail: retuned once a span; per-sample if a sweep zippers.
                (Dsp::Bells(eq), ir::EffectKind::Eq(bells)) => {
                    eq.retune(bells, self.rate);
                    eq.process(io);
                }
                // The wet path from the input, then the dry level, then the wet at the output gain.
                (Dsp::Wet(processor, wet), kind) => {
                    let Some(wet) = wet.get_mut(..io.len()) else {
                        continue;
                    };
                    // A script or the tempo may have moved it since the last block.
                    processor.set(kind, tempo);
                    wet.copy_from_slice(io);
                    processor.process(wet);
                    levels.mix(io, wet, e.dry, e.output);
                    continue;
                }
                (_, ir::EffectKind::Sends(levels)) => {
                    for (tap, level) in taps.iter_mut().zip(levels) {
                        for (t, x) in tap.iter_mut().zip(io.iter()) {
                            t[0] += x[0] * level;
                            t[1] += x[1] * level;
                        }
                    }
                }
                _ => {}
            }
            levels.matrix(io, effects::matrix(&e));
        }
    }

    /// Whether any slot is active (a send slot with nothing in it returns nothing).
    pub(crate) fn active(&self) -> bool {
        self.slots.iter().flatten().any(|e| !e.bypass)
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
            let (mods, blends) = (
                std::mem::take(&mut rack.mods),
                std::mem::take(&mut rack.blends),
            );
            let impulses = Responses(&plan.impulses, &self.loaded);
            *rack = Rack::routed(&rack.slots, rate, impulses, &mods, &blends);
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
