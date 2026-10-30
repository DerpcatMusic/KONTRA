//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use kontra_dsp::core::Levels;
use kontra_effects::matrix as effects;
use kontra_effects::summed as filters;
use kontra_ir as ir;
use kontra_ledger::ledger::{self, Ramp};
use kontra_plan::Response;
use kontra_time::SampleTime;

use kontra_behaviour::Behaviour;
use kontra_effects::processors::processor;
pub use kontra_effects::processors::{Dsp, Responses};

pub struct Rack {
    /// The plan's slots as scripts have changed them.
    pub slots: Box<[Option<ir::Effect>]>,
    dsp: Box<[Dsp]>,
    /// Routes moving the slots ([`ir::Bus::mods`]), the blends they read,
    /// and each route's amount for this span.
    mods: Box<[ir::ModKind]>,
    /// Whether each route reads its source mirrored (`ir::Mod::invert`).
    inverts: Box<[bool]>,
    blends: Box<[ir::Blend]>,
    amounts: Box<[f32]>,
    /// Each slot's levels, gliding where its settings move.
    levels: Box<[Levels]>,
    rate: u32,
}

/// The sources a rack's routes read: those no note owns.
pub struct Live<'a> {
    pub behaviour: &'a Behaviour,
    pub scripts: &'a [Option<(u16, Ramp)>],
    pub now: SampleTime,
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

impl Rack {
    /// `impulses` are the plan's convolution responses. Not real-time.
    pub fn new(slots: &[Option<ir::Effect>], rate: u32, impulses: &[Option<Response>]) -> Self {
        Self::routed(slots, rate, Responses(impulses, &[]), &[], &[], &[])
    }

    /// As [`Self::new`], its slots moved by `mods` reading `blends`.
    pub fn routed(
        slots: &[Option<ir::Effect>],
        rate: u32,
        impulses: Responses<'_>,
        mods: &[ir::ModKind],
        inverts: &[bool],
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
            inverts: inverts.into(),
            blends: blends.into(),
            amounts: vec![0.0; mods.len()].into(),
            levels: vec![Levels::new(rate); slots.len()].into(),
            rate,
        }
    }

    /// Rebuilds its processors from its slots and routes. Not real-time.
    pub fn rebuild(&mut self, rate: u32, impulses: Responses<'_>) {
        let (mods, inverts, blends) = (
            std::mem::take(&mut self.mods),
            std::mem::take(&mut self.inverts),
            std::mem::take(&mut self.blends),
        );
        *self = Self::routed(&self.slots, rate, impulses, &mods, &inverts, &blends);
    }

    /// Puts `effect` and its processor in slot `n`; the old pair comes back.
    pub fn install(
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
    pub fn follow(&mut self, live: &Live) {
        for n in 0..self.mods.len() {
            let Some(m) = self.mods.get(n).copied() else {
                break;
            };
            let invert = self.inverts.get(n).copied().unwrap_or(false);
            // A mirrored route reads its source as `1 - x`, or `-x` when the
            // source is bipolar (the same reading `Engine::unit` normalises).
            let mirror = |x: f32, source: ir::Source| {
                if !invert {
                    return x;
                }
                match source {
                    ir::Source::Lfo(_) | ir::Source::PitchBend => -x,
                    _ => 1.0 - x,
                }
            };
            let amount = match m {
                ir::ModKind::EffectParam { source, depth, .. } => {
                    let x = live.read(source, &self.blends).unwrap_or(0.0);
                    mirror(x, source) * depth
                }
                ir::ModKind::Send { source, .. } => {
                    let x = live.read(source, &self.blends).unwrap_or(1.0);
                    mirror(x, source)
                }
                _ => continue,
            };
            if let Some(a) = self.amounts.get_mut(n) {
                *a = amount;
            }
        }
    }

    /// Effect configurations with this rack's last applied route amounts.
    pub fn inspect_slots(&self) -> Vec<Option<ir::Effect>> {
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
    pub fn route_amounts(&self) -> impl Iterator<Item = (ir::ModKind, f32)> + '_ {
        self.mods.iter().copied().zip(self.amounts.iter().copied())
    }

    /// What its routes scale send `n`'s level by this span.
    pub fn send_scale(&self, n: usize) -> f32 {
        self.mods
            .iter()
            .zip(self.amounts.iter())
            .filter(|(m, _)| matches!(m, ir::ModKind::Send { send, .. } if usize::from(*send) == n))
            .map(|(_, a)| a)
            .product()
    }

    /// Processes `io`; a send slot adds it to `taps` (one buffer per send slot).
    pub fn process(&mut self, io: &mut [[f32; 2]], taps: &mut [Box<[[f32; 2]]>], tempo: f32) {
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
    pub fn active(&self) -> bool {
        self.slots.iter().flatten().any(|e| !e.bypass)
    }
}
