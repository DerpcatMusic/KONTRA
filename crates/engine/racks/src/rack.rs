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
    blends: Box<[ir::Blend]>,
    amounts: Box<[f32]>,
    /// Each slot's levels, gliding where its settings move.
    levels: Box<[Levels]>,
    /// Per slot, frames its input and output have both been silent.
    quiet: Box<[usize]>,
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
        Self::routed(slots, rate, Responses(impulses, &[]), &[], &[])
    }

    /// As [`Self::new`], its slots moved by `mods` reading `blends`.
    pub fn routed(
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
            quiet: vec![0; slots.len()].into(),
            rate,
        }
    }

    /// Rebuilds its processors from its slots and routes. Not real-time.
    pub fn rebuild(&mut self, rate: u32, impulses: Responses<'_>) {
        let (mods, blends) = (
            std::mem::take(&mut self.mods),
            std::mem::take(&mut self.blends),
        );
        *self = Self::routed(&self.slots, rate, impulses, &mods, &blends);
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
        let slots = slots.zip(self.levels.iter_mut()).zip(self.quiet.iter_mut());
        for (n, (((slot, dsp), levels), quiet)) in slots.enumerate() {
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
                    // Silence in and nothing but silence stored for longer than
                    // it remembers: its state is silent, so its clocks move on alone.
                    let still = is_silent(io);
                    if still && processor.memory().is_some_and(|m| *quiet >= m) {
                        processor.idle(wet);
                        levels.idle(e.dry, e.output, io.len());
                        continue;
                    }
                    wet.copy_from_slice(io);
                    processor.process(wet);
                    // Its state, not only what it let out: a closed filter or
                    // a zero output may hide a tail still ringing inside.
                    let settled = processor.written() < SILENT;
                    *quiet = if still && settled && is_silent(wet) {
                        quiet.saturating_add(io.len())
                    } else {
                        0
                    };
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

/// Below this a span counts as silent: -140 dBFS, so a skipped wet path
/// stays under -120 dB after a slot's output gain.
const SILENT: f32 = 1e-7;

fn is_silent(io: &[[f32; 2]]) -> bool {
    io.iter().flatten().all(|x| x.abs() < SILENT)
}

#[cfg(test)]
mod tests {
    use super::{Rack, ir};

    const RATE: u32 = 48_000;
    const SPAN: usize = 256;

    /// A swept echo (an LFO on its head), wet only.
    fn chorus() -> ir::Effect {
        let diagonal = |g| [[g, 0.0], [0.0, g]];
        ir::Effect {
            kind: ir::EffectKind::Delay(ir::Delay {
                time: ir::DelayTime::Ms(15.0),
                depth_ms: 5.0,
                second_head: true,
                input: diagonal(1.0),
                feedback: diagonal(0.3),
                output: diagonal(1.0),
                damping_hz: 8_000.0,
                low_cut_hz: 0.0,
                steep_low_cut: false,
                high_shelf: None,
                spread_ms: 1.0,
                rate: ir::Rate::Hz(0.9),
                phase: 0.25,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }
    }

    /// A burst, seconds of silence, a burst: what the rack plays, span by
    /// span; `gate` false clears its silence count each span, so it never sleeps.
    fn play(rack: &mut Rack, gate: bool) -> Vec<[f32; 2]> {
        let burst = |n: usize| [((n * 7919 % 101) as f32 / 101.0) - 0.5; 2];
        let frames = 6 * RATE as usize;
        let mut out: Vec<[f32; 2]> = (0..frames)
            .map(|n| match n % (3 * RATE as usize) {
                k if k < 4_800 => burst(n),
                _ => [0.0; 2],
            })
            .collect();
        for span in out.chunks_mut(SPAN) {
            rack.process(span, &mut [], 120.0);
            if !gate {
                rack.quiet.fill(0);
            }
        }
        out
    }

    /// A wet slot whose input and output stay silent sleeps, its clocks
    /// moving on, and the next note plays within -120 dB of a rack that
    /// never slept.
    #[test]
    fn a_silent_slot_sleeps_and_wakes_in_step() {
        let make = || Rack::new(&[Some(chorus())], RATE, &[]);
        let (mut gated, mut always) = (make(), make());
        let a = play(&mut gated, true);
        let b = play(&mut always, false);
        // Asleep at the end: silent for longer than its ring, not counting on.
        let memory = match gated.dsp.first() {
            Some(super::Dsp::Wet(p, _)) => p.memory(),
            _ => None,
        };
        let quiet = gated.quiet.first().copied().unwrap_or(0);
        assert!(
            memory.is_some_and(|m| quiet >= m && quiet < 2 * m),
            "{quiet} of {memory:?}"
        );
        let worst = (a.iter().flatten().zip(b.iter().flatten()))
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        let loudest = b.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(
            loudest > 0.1 && worst < 1e-6 * loudest,
            "{worst} of {loudest}"
        );
    }

    /// A small line network (31 lines on an m-sequence, two units a side).
    fn law() -> &'static ir::ReverbNetwork {
        let mut sequence = [0u8; 31];
        let mut s = 1u8;
        for bit in &mut sequence {
            *bit = s & 1;
            s = (s >> 1) | ((s ^ (s >> 2)) & 1) << 4;
        }
        let pad = |pairs: &[(u8, f32)]| {
            std::array::from_fn(|i| pairs.get(i).copied().unwrap_or((0, 0.0)))
        };
        let tap = |a: u8| {
            std::array::from_fn(|i| (a + i as u8 * 3, if i % 2 == 0 { 0.4 } else { -0.4 }, 0.3))
        };
        Box::leak(Box::new(ir::ReverbNetwork {
            growth: std::array::from_fn(|i| 20.0 + i as f32 * 3.0),
            offset: std::array::from_fn(|i| 40.0 + i as f32 * 17.0),
            sequence,
            inputs: [
                pad(&[(0, 1.0), (2, -1.0), (4, 1.0)]),
                pad(&[(1, 1.0), (3, -1.0), (5, 1.0)]),
            ],
            inject_lead: 0,
            taps: [tap(6), tap(7)],
            units: [[29, 37, 41], [43, 47, 53], [59, 61, 67], [71, 73, 79]],
            unit_count: 2,
            unit_feedback: 0.9,
            hf: [0.5, 0.95, 0.27],
            level: [4.0, 0.5],
            shelf_hz: 200.0,
            rate: 44_100.0,
            max_rate: 48_000.0,
            glide_s: [0.05, 0.2],
        }))
    }

    fn wet(kind: ir::EffectKind) -> ir::Effect {
        ir::Effect {
            kind,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }
    }

    /// A burst into `hidden` (a tail rings inside, none comes out), `quiet`
    /// seconds of silence, then `open` lets the tail out: one second of what
    /// the rack plays then. `gate` false: a rack that never sleeps.
    fn reveal(hidden: ir::Effect, open: ir::Effect, quiet: f32, gate: bool) -> Vec<[f32; 2]> {
        let mut rack = Rack::new(&[Some(hidden)], RATE, &[]);
        let span = |rack: &mut Rack, io: &mut [[f32; 2]]| {
            rack.process(io, &mut [], 120.0);
            if !gate {
                rack.quiet.fill(0);
            }
        };
        let burst: Vec<[f32; 2]> = (0..4_800)
            .map(|n| [((n * 7919 % 101) as f32 / 101.0) - 0.5; 2])
            .collect();
        for chunk in burst.chunks(SPAN) {
            span(&mut rack, &mut chunk.to_vec());
        }
        for _ in 0..(quiet * RATE as f32) as usize / SPAN {
            span(&mut rack, &mut [[0.0; 2]; SPAN]);
        }
        if let Some(slot) = rack.slots.first_mut() {
            *slot = Some(open);
        }
        let mut out = vec![[0.0; 2]; RATE as usize];
        out.chunks_mut(SPAN).for_each(|c| span(&mut rack, c));
        out
    }

    /// What a later setting lets out of a hidden tail sounds as if the
    /// slot never slept: sleeping asks whether its state is silent, not
    /// whether its output was.
    fn assert_revealed(hidden: ir::Effect, open: ir::Effect, quiet: f32) {
        let a = reveal(hidden, open, quiet, true);
        let b = reveal(hidden, open, quiet, false);
        let worst = (a.iter().flatten().zip(b.iter().flatten()))
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        let loudest = b.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(
            (1e-3..1.0).contains(&loudest) && worst < 1e-6 * loudest,
            "{worst} of {loudest}"
        );
    }

    #[test]
    fn a_tail_behind_a_closed_high_cut_rings_on_when_it_opens() {
        let reverb = |high_cut_hz| {
            wet(ir::EffectKind::Reverb(ir::Reverb {
                seconds: 8.0,
                size: 1.0,
                network: Some(ir::Network::Lines(law())),
                damping: 0.2,
                diffusion: 0.5,
                predelay_ms: 0.0,
                high_cut_hz,
                low_shelf_db: 0.0,
                stereo: 1.0,
                modulation_ms: 2.0,
                modulation_hz: 0.5,
            }))
        };
        // Past the network's memory (its predelay alone is a second).
        assert_revealed(reverb(0.0), reverb(12_000.0), 3.0);
    }

    #[test]
    fn echoes_behind_a_closed_output_ring_on_when_it_opens() {
        let echo = |out| {
            let mut e = chorus();
            if let ir::EffectKind::Delay(d) = &mut e.kind {
                (d.time, d.depth_ms, d.spread_ms) = (ir::DelayTime::Ms(100.0), 1.0, 0.0);
                d.second_head = false;
                (d.feedback, d.output) = ([[0.95, 0.0], [0.0, 0.95]], [[out, 0.0], [0.0, out]]);
            }
            e
        };
        assert_revealed(echo(0.0), echo(1.0), 1.0);
    }
}
