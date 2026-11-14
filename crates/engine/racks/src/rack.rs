//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use kontra_dsp::core::Levels;
use kontra_effects::matrix as effects;
use kontra_effects::summed as filters;
use kontra_ir as ir;
use kontra_ledger::ledger::{self, Ramp};
use kontra_plan::Response;
use kontra_plan::modulation::{Age, lfo};
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
    /// The oscillators `Source::Lfo` reads, free-running.
    lfos: Box<[ir::Lfo]>,
    /// Per oscillator, the time it is read at this span's start (a synced
    /// one's accumulated cycles over its rate, so a tempo change moves its
    /// rate, not its phase), the cycles a synced one has run, and when the
    /// rack last followed.
    lfo_t: Box<[f32]>,
    lfo_cycles: Box<[f64]>,
    last_seconds: f32,
    /// Per route, whether it reads an oscillator (a send's level then moves
    /// every frame: [`Self::send_scales`]).
    live: Box<[bool]>,
    amounts: Box<[f32]>,
    /// Each slot's levels, gliding where its settings move.
    levels: Box<[Levels]>,
    /// Per slot, frames its input and output have both been silent.
    quiet: Box<[usize]>,
    rate: u32,
}

/// Whether `source` reads an oscillator, directly or in its blend.
fn reads_lfo(source: ir::Source, blends: &[ir::Blend]) -> bool {
    let is = |t: &ir::Transfer| matches!(t.source, ir::Source::Lfo(_));
    match source {
        ir::Source::Lfo(_) => true,
        ir::Source::Blend(n) => blends.get(usize::from(n)).is_some_and(|b| {
            b.terms
                .iter()
                .any(|t| is(&t.input) || t.factors.iter().any(|f| is(&f.input)))
        }),
        _ => false,
    }
}

/// The oscillators a rack's routes read, each at its own time plus `offset`.
#[derive(Clone, Copy)]
struct Clock<'a> {
    lfos: &'a [ir::Lfo],
    t: &'a [f32],
    offset: f32,
}

/// The sources a rack's routes read: those no note owns.
pub struct Live<'a> {
    pub behaviour: &'a Behaviour,
    pub scripts: &'a [Option<(u16, Ramp)>],
    pub now: SampleTime,
    /// Seconds since the engine started, and the host tempo (free LFOs).
    pub seconds: f32,
    pub tempo: f32,
}

impl Live<'_> {
    /// Controllers and pitch bend come from the channel last played.
    fn read(&self, source: ir::Source, blends: &[ir::Blend], clock: Clock) -> Option<f32> {
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
            ir::Source::Lfo(n) => {
                let t = clock.t.get(usize::from(n)).copied().unwrap_or(self.seconds) + clock.offset;
                let age = Age {
                    held: t,
                    now: t,
                    ..Age::START
                };
                lfo(clock.lfos.get(usize::from(n))?, age, 0, self.tempo)
            }
            ir::Source::Blend(n) => {
                let b = blends.get(usize::from(n))?;
                let mut sum = b.base;
                for t in &b.terms {
                    let mut x = t.amount * self.through(&t.input, clock)?;
                    for f in &t.factors {
                        x *= f.at(self.through(&f.input, clock)?);
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
    fn through(&self, t: &ir::Transfer, clock: Clock) -> Option<f32> {
        if t.smooth_s > 0.0 && matches!(t.source, ir::Source::Lfo(_)) {
            let smoothed = |back| {
                let past = Clock {
                    offset: clock.offset - back,
                    ..clock
                };
                Some(t.apply((self.read(t.source, &[], past)? + 1.0) / 2.0))
            };
            // Every rack that reads the same wave and table at the same time
            // reads the same value (a voice's rack, a bus's): evaluate once.
            if let ir::Source::Lfo(n) = t.source
                && let Some(l) = clock.lfos.get(usize::from(n))
            {
                let at =
                    clock.t.get(usize::from(n)).copied().unwrap_or(self.seconds) + clock.offset;
                let age = Age {
                    held: at,
                    now: at,
                    ..Age::START
                };
                // A rack scales by nothing: a NaN pattern no voice's scale can be,
                // so a voice at scale 0 (or 1) never shares this key.
                return kontra_plan::modulation::smoothed_shared(
                    t,
                    l,
                    age,
                    self.tempo,
                    u32::MAX,
                    || t.smoothed(smoothed),
                );
            }
            return t.smoothed(smoothed);
        }
        let x = self.read(t.source, &[], clock)?;
        // An LFO is always read on 0..=1; pitch bend only through a table.
        let bipolar = matches!(t.source, ir::Source::Lfo(_))
            || (matches!(t.source, ir::Source::PitchBend) && !t.table.is_empty());
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
        let dsp = slots
            .iter()
            .map(|slot| processor(*slot, rate, &impulses))
            .collect();
        Self {
            slots: slots.into(),
            dsp,
            mods: mods.into(),
            blends: blends.into(),
            lfos: Box::default(),
            lfo_t: Box::default(),
            lfo_cycles: Box::default(),
            last_seconds: 0.0,
            live: mods
                .iter()
                .map(
                    |m| matches!(m, ir::ModKind::Send { source, .. } if reads_lfo(*source, blends)),
                )
                .collect(),
            amounts: vec![0.0; mods.len()].into(),
            levels: vec![Levels::new(rate); slots.len()].into(),
            quiet: vec![0; slots.len()].into(),
            rate,
        }
    }

    /// A rack with no slots that only reads `blends` and `lfos`: [`Self::follow`]
    /// then [`Self::read`] give a route's value at control rate.
    pub fn reader(blends: &[ir::Blend], lfos: &[ir::Lfo], rate: u32) -> Self {
        Self::routed(&[], rate, Responses(&[], &[]), &[], blends).with_lfos(lfos)
    }

    /// `source` as of the last [`Self::follow`].
    pub fn read(&self, live: &Live, source: ir::Source) -> Option<f32> {
        live.read(source, &self.blends, self.clock(0.0))
    }

    /// Sets the oscillators its routes read ([`ir::Bus::lfos`]).
    #[must_use]
    pub fn with_lfos(mut self, lfos: &[ir::Lfo]) -> Self {
        self.lfos = lfos.into();
        self.lfo_t = vec![0.0; lfos.len()].into();
        self.lfo_cycles = vec![0.0; lfos.len()].into();
        self
    }

    /// Rebuilds its processors from its slots and routes. Not real-time.
    pub fn rebuild(&mut self, rate: u32, impulses: Responses<'_>) {
        let (mods, blends, lfos) = (
            std::mem::take(&mut self.mods),
            std::mem::take(&mut self.blends),
            std::mem::take(&mut self.lfos),
        );
        *self = Self::routed(&self.slots, rate, impulses, &mods, &blends).with_lfos(&lfos);
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
        let dt = f64::from((live.seconds - self.last_seconds).max(0.0));
        self.last_seconds = live.seconds;
        // Places each synced LFO's rate is moved along the note ladder.
        let mut places = [0.0f32; kontra_plan::modulation::WARPS];
        for m in &self.mods {
            if let ir::ModKind::LfoDivision { source, lfo, depth } = *m
                && let Some(p) = places.get_mut(usize::from(lfo))
            {
                let clock = Clock {
                    lfos: &self.lfos,
                    t: &self.lfo_t,
                    offset: 0.0,
                };
                *p += live.read(source, &self.blends, clock).unwrap_or(0.0) * depth * 29.0;
            }
        }
        for (((l, cycles), t), p) in self
            .lfos
            .iter()
            .zip(self.lfo_cycles.iter_mut())
            .zip(self.lfo_t.iter_mut())
            .zip(places.into_iter().chain(std::iter::repeat(0.0)))
        {
            let moved = match l.rate {
                ir::Rate::Beats(b) if p != 0.0 => ir::Lfo {
                    rate: ir::Rate::Beats(ir::ladder_beats(b, p)),
                    ..*l
                },
                _ => *l,
            };
            let hz = kontra_plan::modulation::hz(&moved, live.tempo);
            let nominal = kontra_plan::modulation::hz(l, live.tempo);
            *t = match l.rate {
                ir::Rate::Hz(_) => live.seconds,
                _ => {
                    *cycles += f64::from(hz) * dt;
                    (*cycles / f64::from(nominal.max(1e-6))) as f32
                }
            };
        }
        let clock = Clock {
            lfos: &self.lfos,
            t: &self.lfo_t,
            offset: 0.0,
        };
        for (m, amount) in self.mods.iter().zip(self.amounts.iter_mut()) {
            *amount = match *m {
                ir::ModKind::EffectParam { source, depth, .. } => {
                    live.read(source, &self.blends, clock).unwrap_or(0.0) * depth
                }
                ir::ModKind::Send { source, .. } => {
                    live.read(source, &self.blends, clock).unwrap_or(1.0)
                }
                _ => continue,
            };
        }
    }

    fn clock(&self, offset: f32) -> Clock<'_> {
        Clock {
            lfos: &self.lfos,
            t: &self.lfo_t,
            offset,
        }
    }

    /// Whether send `n`'s level reads an oscillator, so it moves within a span.
    pub fn send_moves(&self, n: usize) -> bool {
        self.mods.iter().zip(self.live.iter()).any(|(m, &l)| {
            l && matches!(m, ir::ModKind::Send { send, .. } if usize::from(*send) == n)
        })
    }

    /// Send `n`'s scale at each frame of the span `live` was followed for,
    /// `step` seconds apart, so it does not depend on where blocks split.
    pub fn send_scales(&self, live: &Live, n: usize, step: f32, out: &mut [f32]) {
        for (k, o) in out.iter_mut().enumerate() {
            let clock = self.clock(k as f32 * step);
            let routes = self
                .mods
                .iter()
                .zip(self.live.iter())
                .zip(self.amounts.iter());
            *o = routes
                .filter_map(|((m, &moves), &held)| match *m {
                    ir::ModKind::Send { source, send } if usize::from(send) == n => {
                        Some(if moves {
                            live.read(source, &self.blends, clock).unwrap_or(1.0)
                        } else {
                            held
                        })
                    }
                    _ => None,
                })
                .product();
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
                    // Silence in (or an input it cannot hear) and nothing but
                    // silence stored for longer than it remembers: its state is
                    // silent, so its clocks move on alone.
                    let deaf = processor.deaf();
                    let still = deaf || is_silent(io);
                    if still && processor.memory().is_some_and(|m| *quiet >= m) {
                        processor.idle(wet);
                        if deaf {
                            // The input still passes at the dry level.
                            wet.fill([0.0; 2]);
                            levels.mix(io, wet, e.dry, e.output);
                        } else {
                            levels.idle(e.dry, e.output, io.len());
                        }
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

    /// A notch phaser at its swept, fed-back defaults (its input mixed in).
    fn notches() -> ir::Effect {
        ir::Effect {
            kind: ir::EffectKind::Notches(ir::Notches {
                stages: 6,
                hz: 622.0,
                octaves: 4.6,
                spread: 0.0,
                spread_swing: 1.0,
                spread_limit: 2.7,
                feedback: 0.5,
                loop_width: 8.0,
                loop_top: 0.1,
                rate: ir::Rate::Hz(0.9),
                rounding: 0.0,
                depth: 1.0,
                warp: 0.1,
                phase: 0.25,
                polarity: ir::Polarity::Normal,
                mix: 1.0,
                low_cut_hz: 5.0,
                internal_hz: 90_000.0,
            }),
            ..chorus()
        }
    }

    /// Seven swept voices fed back through a scatter, the last silent
    /// (heard through the loop), and a straight-through share.
    fn ensemble() -> ir::Effect {
        static VOICES: ir::EnsembleVoices = ir::EnsembleVoices {
            delay: [0.003, 0.003, 0.007, 0.007, 0.011, 0.011, 0.013],
            rate: [1.0, 1.0, 1.4, 1.4, 1.7, 1.7, 0.7],
            start: [0.0, 0.5, 0.0, 0.5, 0.0, 0.5, 0.0],
            bend: [0.2; 7],
            wobble_rate: [6.0; 7],
            wobble: [0.0, 0.0, 0.0, 0.0, 0.1, 0.1, 0.0],
            smooth_hz: [3000.0; 7],
            filter: [ir::Section {
                hz: 5_000.0,
                num: [1.0, 0.0, 0.0],
                den: [1.0, 1.414, 1.0],
            }; 7],
        };
        ir::Effect {
            kind: ir::EffectKind::Ensemble(ir::Ensemble {
                voices: &VOICES,
                span: 1.0,
                hz: 0.25,
                sweep: 0.004,
                levels: [1.0, 1.0, 0.7, 0.7, 0.0, 0.0, 0.0],
                angles: [-0.1, 0.1, -0.1, 0.1, 0.0, 0.0, 0.0],
                scatter: [0.5, -0.2, -0.2, 0.3, -0.2, 0.3, 0.3],
                feedback: 0.5,
                polarity: ir::Polarity::Normal,
                input: 0.9,
                wet: 0.3,
                direct: 0.7,
                shelf: ir::Section {
                    hz: 30.0,
                    num: [0.5, 1.0, 1.0],
                    den: [1.0, 1.414, 1.0],
                },
            }),
            ..chorus()
        }
    }

    /// A comb bank half way into its scan: swept heads, the window moving,
    /// three voices fed back hard, wide, through zero off.
    fn comb_bank() -> ir::Effect {
        ir::Effect {
            kind: ir::EffectKind::CombBank(ir::CombBank {
                root_hz: 110.0,
                ratio: [1.0, 1.25, 1.5, 2.0],
                detune: [1.01, 0.99, 0.98, 1.02],
                lowest_hz: 50.0,
                feedback: 0.9,
                voices: 2.0,
                damping: 0.1,
                depth: 0.5,
                rate: ir::Rate::Hz(0.9),
                offset: 0.1,
                polarity: ir::Polarity::Normal,
                mode: ir::CombMode::Scan,
                scan_depth: 0.5,
                scan_shape: ir::ScanShape::SawUp,
                width: 0.5,
                mix: 0.7,
            }),
            ..chorus()
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
        for effect in [chorus(), notches(), ensemble(), comb_bank()] {
            sleeps_and_wakes_in_step(effect);
        }
    }

    fn sleeps_and_wakes_in_step(effect: ir::Effect) {
        let make = || Rack::new(&[Some(effect)], RATE, &[]);
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
