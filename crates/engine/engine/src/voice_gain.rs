//! A voice's level and pitch step from its zone, its note and the group's
//! modulation; re-read when a controller or expression moves.

use std::cell::Cell;

use kontra_ir as ir;

use crate::ledger::Note;
use ir::TableValue::{Inharmonic, ModAmount, ModTune, Phase, PhaseRandom, Position, Warp};
use kontra_stream::Mix;
use kontra_voice::{Ahdsr, Gain, Lag, Read};

use crate::effects::{self, product};
use crate::modulators::{self, Age};
use crate::{Control, Engine, Level, Zone};

/// A voice's lagging pitch routes: their semitones and longest time constant.
/// The voice glides toward them a control step at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bend {
    pub(crate) semitones: f32,
    pub(crate) tau_ms: f32,
    /// The routes that do not lag.
    pub(crate) instant: f32,
}

impl Bend {
    pub(crate) const NONE: Self = Self {
        semitones: 0.0,
        tau_ms: 0.0,
        instant: 0.0,
    };
}

/// A script's group pan glides as a one-pole of this time constant
/// (measured natively: τ 37.5 ms, as the instrument's volume and pan).
pub(crate) const GROUP_PAN_TAU_MS: f32 = 37.5;

pub(crate) use kontra_plan::modulation::{bent, glide};

/// Active dependencies during a single modulation evaluation. Fixed storage
/// keeps this usable on the audio thread, including when a route is skipped.
#[derive(Default)]
pub(crate) struct Evaluation {
    active: [Cell<u64>; 12],
    depth: Cell<u8>,
    /// Dependencies entered since the outermost one: siblings re-read nested
    /// LFOs uncached, so a branching chain would grow exponentially.
    work: Cell<u16>,
}

#[derive(Clone, Copy)]
enum Dependency {
    LfoMotion,
    LfoIntensity,
    EnvelopeIntensity,
}

struct Active<'a> {
    word: &'a Cell<u64>,
    bit: u64,
    depth: &'a Cell<u8>,
}

impl Evaluation {
    fn enter(&self, dependency: Dependency, index: u8) -> Option<Active<'_>> {
        // Also bound the stack for long acyclic dependency chains.
        const MAX_DEPTH: u8 = 32;
        // ponytail: a flat budget skips the excess like a cycle; memoize
        // nested reads if a real library ever hits it.
        const MAX_WORK: u16 = 1024;
        let depth = self.depth.get();
        let work = if depth == 0 { 0 } else { self.work.get() };
        if work == MAX_WORK {
            return None;
        }
        self.work.set(work + 1);
        let word = self
            .active
            .get(dependency as usize * 4 + usize::from(index / 64))?;
        let bit = 1 << (index % 64);
        if depth == MAX_DEPTH || word.get() & bit != 0 {
            return None;
        }
        word.set(word.get() | bit);
        self.depth.set(depth + 1);
        Some(Active {
            word,
            bit,
            depth: &self.depth,
        })
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.word.set(self.word.get() & !self.bit);
        self.depth.set(self.depth.get() - 1);
    }
}

impl Engine {
    /// The voice's gain, and the slower gain of its routes that lag (their
    /// product, at the longest time constant among them).
    /// `group_pan` is where the voice's glide of its group's pan is.
    pub(crate) fn voice_gain(
        &self,
        zone: &Zone,
        note: &Note,
        factor: f32,
        age: Age,
        group_pan: f32,
    ) -> (Gain, Lag) {
        let channel = note.host.port_channel();
        let crossfade: f32 = zone
            .crossfades
            .iter()
            .map(|x| {
                x.gain(
                    self.behaviour
                        .control(x.control, channel, note.velocity_byte, note.key),
                )
            })
            .product();
        // ponytail: one lag per voice, the longest of its routes'; a
        // route of its own when a group mixes two lags audibly.
        let (mut mods, mut lagged, mut tau) = (1.0f32, 1.0f32, 0.0f32);
        let groups = self.groups.get(zone.group as usize);
        for m in groups.iter().flat_map(|g| &g.mods).filter(|m| !m.bypass) {
            if let ir::ModKind::Volume {
                source,
                intensity,
                curve,
                tau_ms,
            } = m.kind
            {
                let intensity = intensity * self.scale(zone, source, note, age);
                let input = self.unit(zone, source, note, age);
                let g = input.map_or(1.0, |x| curve.gain(intensity, x));
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, g, crate::RouteUnit::GainMultiplier);
                if tau_ms > 0.0 {
                    (lagged, tau) = (lagged * g, tau.max(tau_ms));
                } else {
                    mods *= g;
                }
            }
        }
        let pan: f32 = self.groups.get(zone.group as usize).map_or(0.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .filter_map(|m| match m.kind {
                    ir::ModKind::Pan { source, amount } => {
                        let input = self.routed(zone, source, note, age)?;
                        let value = input * amount;
                        #[cfg(feature = "trace")]
                        self.trace_route(zone, m, Some(input), value, crate::RouteUnit::Pan);
                        Some(value)
                    }
                    _ => None,
                })
                .sum()
        });
        let level = Level {
            pan: group_pan,
            ..self.level(zone)
        };
        let [l, r] = gain(zone, note, level, pan);
        let g = crossfade * mods * factor;
        let group = self
            .groups
            .get(zone.group as usize)
            .map_or(effects::IDENTITY, |g| {
                // The group's sum runs the slots from its split on.
                let split = self.group_split.get(zone.group as usize).copied().flatten();
                effects::rack(
                    g.inserts
                        .get(..split.unwrap_or(g.inserts.len()))
                        .unwrap_or_default(),
                )
            });
        let amp = [[l * g, 0.0], [0.0, r * g]];
        (product(group, amp), Lag::new(lagged, tau, self.rate))
    }

    /// The zone's channel matrix with its live cells ([`ir::ModKind::Mix`])
    /// read for this note; `None` when no route moves it.
    pub(crate) fn voice_mix(&self, zone: &Zone, note: &Note, age: Age) -> Option<Mix> {
        let g = self.groups.get(zone.group as usize)?;
        let mut mix = *zone.region.mix.as_deref()?;
        let mut moved = false;
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::Mix {
                source,
                zone: z,
                row,
                side,
            } = m.kind
                && z == zone.member
                && let Some(cell) = mix
                    .get_mut(usize::from(row))
                    .and_then(|r| r.get_mut(usize::from(side)))
            {
                let input = self.source(zone, source, note, age);
                let value = input.unwrap_or(1.0);
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, value, crate::RouteUnit::MixMultiplier);
                *cell *= value;
                moved = true;
            }
        }
        moved.then_some(mix)
    }

    /// Where this note's voice reads its group's wavetable: each value as
    /// the group (or a script) set it plus its routes' `source × depth`,
    /// held within 0..=1. `None` outside a wavetable group.
    pub(crate) fn voice_read(&self, zone: &Zone, note: &Note, age: Age) -> Option<Read> {
        let g = self.groups.get(zone.group as usize)?;
        let mut read = g.wavetable?;
        read.phase = (read.phase + note.draw.unit() * read.phase_random).fract();
        read.rate = self.rate as f32;
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::Wavetable {
                source,
                value,
                depth,
            } = m.kind
            {
                let input = self.routed(zone, source, note, age);
                let moved = input.unwrap_or(0.0) * depth;
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, moved, crate::RouteUnit::TableFraction);
                if let Some(v) = table_value(&mut read, value) {
                    *v += moved;
                }
            }
        }
        for v in [Position, Warp(0), Warp(1), Inharmonic, ModAmount, ModTune] {
            if let Some(v) = table_value(&mut read, v) {
                *v = v.clamp(0.0, 1.0);
            }
        }
        Some(read)
    }

    /// The pan the zone's group is set to.
    pub(crate) fn group_pan(&self, zone: &Zone) -> f32 {
        self.level(zone).pan
    }

    /// The group's level, with the instrument's tune (its level and pan
    /// are the output fader's).
    fn level(&self, zone: &Zone) -> Level {
        let group = self
            .groups
            .get(zone.group as usize)
            .map_or(Level::UNITY, |g| g.level);
        Level {
            tune: group.tune + self.instrument.tune,
            ..group
        }
    }

    /// LFO `n`'s value for this note at `age` (see [`Self::source`]).
    fn lfo_value(&self, zone: &Zone, n: u8, note: &Note, age: Age) -> Option<f32> {
        let lfo = self.moved_lfo(zone, n, note, age)?;
        if let ir::Wave::Chaos { .. } = lfo.wave {
            return age.chaos.get(usize::from(n)).copied();
        }
        let seed = match lfo.clock {
            ir::Clock::Voice => note.draw.seed(),
            ir::Clock::Free => zone.group.wrapping_mul(32).wrapping_add(u32::from(n)),
        };
        Some(modulators::lfo(&lfo, age, seed, self.tempo))
    }

    /// LFO `n` as this note plays it: its group's, with the note-on routes
    /// on phase and rate and the live phase warp applied.
    fn moved_lfo(&self, zone: &Zone, n: u8, note: &Note, age: Age) -> Option<ir::Lfo> {
        let group = || self.groups.get(zone.group as usize);
        let g = group()?;
        let mut lfo = *g.lfos.get(usize::from(n))?;
        let _active = self.evaluation.enter(Dependency::LfoMotion, n)?;
        let moving = (self.reshaped.get(zone.group as usize).into_iter())
            .flat_map(|r| r.moving(n))
            .filter_map(|i| g.mods.get(i));
        // ponytail: phase and rate routes read their source at
        // note-on (exact for velocity and note-on values); a live
        // controller moving the rate needs a per-voice phase accumulator.
        let onset = Age::at_start(age.start_frame);
        for m in moving.filter(|m| !m.bypass) {
            match m.kind {
                // An LFO source here would recurse.
                ir::ModKind::LfoPhase {
                    source,
                    lfo: l,
                    depth,
                } if l == n && !matches!(source, ir::Source::Lfo(_)) => {
                    let input = self.source(zone, source, note, onset);
                    let value = input.unwrap_or(0.0) * depth;
                    #[cfg(feature = "trace")]
                    self.trace_route(zone, m, input, value, crate::RouteUnit::PhaseCycles);
                    lfo.phase += value;
                }
                ir::ModKind::LfoRate {
                    source,
                    lfo: l,
                    depth,
                    octaves,
                    range_hz: [lo, hi],
                } if l == n && !matches!(source, ir::Source::Lfo(_)) => {
                    let input = self.source(zone, source, note, onset);
                    let f = (input.unwrap_or(0.0) * depth * octaves).exp2();
                    #[cfg(feature = "trace")]
                    self.trace_route(zone, m, input, f, crate::RouteUnit::RateMultiplier);
                    lfo.rate = match lfo.rate {
                        ir::Rate::Hz(hz) => ir::Rate::Hz((hz * f).clamp(lo, hi.max(lo))),
                        ir::Rate::Beats(b) => ir::Rate::Beats(b / f),
                        ir::Rate::Steps { beats, count } => ir::Rate::Beats(beats * count / f),
                    };
                }
                _ => {}
            }
        }
        lfo.phase += age.warp.get(usize::from(n)).copied().unwrap_or(0.0);
        Some(lfo)
    }

    fn envelope_value(&self, zone: &Zone, n: u8, age: Age) -> Option<f32> {
        let group = || self.groups.get(zone.group as usize);
        let e = group()?.envelopes.get(usize::from(n))?;
        let stretch = age
            .stretch
            .get(usize::from(n))
            .copied()
            .unwrap_or(modulators::EnvScale::UNITY);
        Some(modulators::stretched(e, age, stretch))
    }

    /// A live source's value for this note; `None` for one fixed at the voice's start.
    pub(crate) fn source(
        &self,
        zone: &Zone,
        source: ir::Source,
        note: &Note,
        age: Age,
    ) -> Option<f32> {
        let channel = note.host.port_channel();
        let group = || self.groups.get(zone.group as usize);
        match source {
            ir::Source::Shaped(n) => {
                let s = group()?.shapers.get(usize::from(n))?;
                self.through(zone, s, note, age, true)
            }
            ir::Source::Blend(n) => {
                let b = group()?.blends.get(usize::from(n))?;
                let mut sum = b.base;
                for t in &b.terms {
                    let mut x = t.amount * self.through(zone, &t.input, note, age, false)?;
                    for f in &t.factors {
                        x *= f.at(self.through(zone, &f.input, note, age, false)?);
                    }
                    sum += x;
                }
                Some(sum)
            }
            ir::Source::Envelope(n) => self
                .memo
                .envelope(n, age, || self.envelope_value(zone, n, age)),
            // A nested evaluation may skip an active dependency. Its value
            // must not populate or reuse the top-level cache.
            ir::Source::Lfo(n) if self.evaluation.depth.get() != 0 => {
                self.lfo_value(zone, n, note, age)
            }
            ir::Source::Lfo(n) => self.memo.lfo(n, age, || self.lfo_value(zone, n, note, age)),
            ir::Source::Velocity => Some(note.velocity as f32),
            ir::Source::ReleaseVelocity => Some(note.release_velocity),
            // A note's own brightness is its controller 74 (MPE's timbre).
            ir::Source::Controller(74) | ir::Source::ControllerFrom { cc: 74, .. }
                if note.expression_brightness.is_some() =>
            {
                note.expression_brightness
            }
            ir::Source::Controller(cc) => Some(f32::from(self.behaviour.cc(channel, cc)) / 127.0),
            ir::Source::ControllerFrom { cc, start } => {
                let v = if self.behaviour.cc_arrived(channel, cc) {
                    self.behaviour.cc(channel, cc)
                } else {
                    start
                };
                Some(f32::from(v) / 127.0)
            }
            ir::Source::PitchBend => Some(self.behaviour.bend(channel)),
            ir::Source::Pressure => {
                Some((note.expression_pressure).unwrap_or_else(|| self.behaviour.pressure(channel)))
            }
            ir::Source::Key => Some(f32::from(note.key) / 127.0),
            ir::Source::Constant => Some(1.0),
            ir::Source::Script(index) => Some(
                note.mod_value(index, self.now)
                    .unwrap_or_else(|| self.script_mod(index)) as f32
                    / 1_000_000.0,
            ),
            ir::Source::HeldTime { span_ms } => Some(kontra_plan::held_time(span_ms, note.held)),
            ir::Source::Random { bipolar } => {
                let u = note.draw.unit();
                Some(if bipolar { 2.0 * u - 1.0 } else { u })
            }
            ir::Source::ZoneEnvelope(slot) => self.memo.zone_envelope(slot, age, || {
                let env = zone.envelopes.iter().find(|e| e.slot == slot)?;
                let pos = if env.speed_lock {
                    age.sample_pos
                } else {
                    age.unpitched_pos
                };
                Some(kontra_plan::modulation::zone_envelope(env, pos))
            }),
        }
    }

    /// `t`'s source through its table. A shaper may read a blend; a blend's
    /// inputs read neither (`None`: no recursion).
    fn through(
        &self,
        zone: &Zone,
        t: &ir::Transfer,
        note: &Note,
        age: Age,
        shaper: bool,
    ) -> Option<f32> {
        // A smoothed step wave reads its own past, so the glide follows the
        // tempo through the LFO's rate with no state.
        if t.smooth_s > 0.0
            && let ir::Source::Lfo(n) = t.source
        {
            let scale = if shaper {
                self.intensity_scale(zone, t.source, note, age)?
            } else {
                1.0
            };
            let smoothed = |back| {
                let past = Age {
                    held: age.held - back,
                    now: age.now - back,
                    ..age
                };
                let x = self.lfo_value(zone, n, note, past)? * scale;
                Some(t.apply((x + 1.0) / 2.0))
            };
            // The wave is the group's unless a note-on route moved it: voices
            // that play the same wave read the same smoothed value.
            return match self.moved_lfo(zone, n, note, age) {
                Some(lfo) => {
                    modulators::smoothed_shared(t, &lfo, age, self.tempo, scale.to_bits(), || {
                        t.smoothed(smoothed)
                    })
                }
                None => t.smoothed(smoothed),
            };
        }
        let x = match t.source {
            ir::Source::Shaped(_) => return None,
            ir::Source::Blend(_) if !shaper => return None,
            // A shaper reads an LFO at the depth its Intensity routes give it.
            s if shaper => {
                self.source(zone, s, note, age)? * self.intensity_scale(zone, s, note, age)?
            }
            s => self.source(zone, s, note, age)?,
        };
        // An LFO spans the table as `(x + 1)/2`, and bipolar sources a table.
        let bend = matches!(
            t.source,
            ir::Source::PitchBend
                | ir::Source::Random { bipolar: true }
                | ir::Source::ZoneEnvelope(_)
        ) && !t.table.is_empty();
        let x = if bend || matches!(t.source, ir::Source::Lfo(_)) {
            (x + 1.0) / 2.0
        } else {
            x
        };
        Some(t.apply(x))
    }

    /// A route's source scaled by the [`ir::ModKind::Intensity`] routes on it.
    pub(crate) fn routed(
        &self,
        zone: &Zone,
        source: ir::Source,
        note: &Note,
        age: Age,
    ) -> Option<f32> {
        Some(self.source(zone, source, note, age)? * self.scale(zone, source, note, age))
    }

    /// `s` on 0..=1: a bipolar source read as `(x + 1)/2`.
    pub(crate) fn unit(&self, zone: &Zone, s: ir::Source, note: &Note, age: Age) -> Option<f32> {
        let x = self.source(zone, s, note, age)?;
        Some(match s {
            ir::Source::Lfo(_)
            | ir::Source::PitchBend
            | ir::Source::Random { bipolar: true }
            | ir::Source::ZoneEnvelope(_) => (x + 1.0) * 0.5,
            _ => x,
        })
    }

    /// The product of the [`ir::ModKind::Intensity`] routes on the timed source `of`.
    pub(crate) fn scale(&self, zone: &Zone, of: ir::Source, note: &Note, age: Age) -> f32 {
        self.intensity_scale(zone, of, note, age).unwrap_or(1.0)
    }

    fn intensity_scale(&self, zone: &Zone, of: ir::Source, note: &Note, age: Age) -> Option<f32> {
        let (dependency, index) = match of {
            ir::Source::Lfo(n) => (Dependency::LfoIntensity, n),
            ir::Source::Envelope(n) => (Dependency::EnvelopeIntensity, n),
            _ => return Some(1.0),
        };
        let g = zone.group as usize;
        let (Some(group), Some(reshaped)) = (self.groups.get(g), self.reshaped.get(g)) else {
            return Some(1.0);
        };
        let _active = self.evaluation.enter(dependency, index)?;
        Some(
            reshaped
                .scaling(of)
                .filter_map(|i| group.mods.get(i))
                .filter(|m| !m.bypass)
                .filter_map(|m| match m.kind {
                    ir::ModKind::Intensity {
                        source,
                        of: o,
                        depth,
                    } if o == of => {
                        let input = self.unit(zone, source, note, age)?;
                        let x = if depth < 0.0 { 1.0 - input } else { input };
                        let value = (1.0 - (1.0 - x.clamp(0.0, 1.0)) * depth.abs()).max(0.0);
                        #[cfg(feature = "trace")]
                        self.trace_route(
                            zone,
                            m,
                            Some(input),
                            value,
                            crate::RouteUnit::IntensityMultiplier,
                        );
                        Some(value)
                    }
                    _ => None,
                })
                .product(),
        )
    }

    /// The voice's step without its pitch routes, and those routes: the
    /// voice glides toward the lagging ones ([`Bend`]).
    pub(crate) fn step(&self, zone: &Zone, note: &Note, age: Age) -> (f64, Bend) {
        let mut bend = Bend::NONE;
        let groups = self.groups.get(zone.group as usize);
        for m in groups.iter().flat_map(|g| &g.mods).filter(|m| !m.bypass) {
            if let ir::ModKind::Pitch {
                source,
                semitones,
                depth,
                tau_ms,
            } = m.kind
            {
                let input = self.routed(zone, source, note, age);
                let st = input.unwrap_or(0.0) * semitones * depth;
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, st, crate::RouteUnit::Semitones);
                if tau_ms > 0.0 {
                    bend.semitones += st;
                    bend.tau_ms = bend.tau_ms.max(tau_ms);
                } else {
                    bend.instant += st;
                }
            }
        }
        let semitones = (f64::from(note.key) - f64::from(zone.root)) * zone.keytrack
            + zone.tune
            + self.level(zone).tune
            + note.tune
            + note.expression_tune
            + f64::from(note.script_tune) / 100_000.0;
        let step = 2f64.powf(semitones / 12.0) * f64::from(zone.sample_rate) / f64::from(self.rate);
        (step, bend)
    }

    /// The fractions of the sample its loops' start and end move for a
    /// voice, read at note-on.
    pub(crate) fn loop_shift(&self, zone: &Zone, note: &Note) -> [f32; 2] {
        let mut shift = [0.0; 2];
        let Some(g) = self.groups.get(zone.group as usize) else {
            return shift;
        };
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::LoopShift {
                source,
                length,
                depth,
            } = m.kind
                && let Some(x) = self.source(zone, source, note, Age::START)
                && let Some(s) = shift.get_mut(usize::from(length))
            {
                let value = x * depth;
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, Some(x), value, crate::RouteUnit::LoopFraction);
                *s += value;
            }
        }
        shift
    }

    /// Frames the zone's start moves, within its start range either way.
    pub(crate) fn start_offset(&self, zone: &Zone, note: &Note) -> i64 {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return 0;
        };
        let x: f32 = g
            .mods
            .iter()
            .filter(|m| !m.bypass)
            .filter_map(|m| match m.kind {
                ir::ModKind::SampleStart { source, depth } => {
                    let x = self.source(zone, source, note, Age::START)?;
                    let value = x * depth;
                    #[cfg(feature = "trace")]
                    self.trace_route(
                        zone,
                        m,
                        Some(x),
                        value,
                        crate::RouteUnit::SampleStartFraction,
                    );
                    Some(value)
                }
                _ => None,
            })
            .sum();
        let range = zone.start_mod as f64;
        (f64::from(x) * range).round().clamp(-range, range) as i64
    }

    /// How much envelope modulation routes scale each of the group's first
    /// [`modulators::WARPS`] breakpoint envelopes for a voice starting now:
    /// attack, hold, decay, sustain (and initial release) are latched at
    /// note-on; release is refreshed live at note-off via
    /// [`Self::release_stretches`].
    // ponytail: envelopes past the eighth keep their drawn times; widen
    // WARPS if a group routes one.
    pub(crate) fn stretches(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
    ) -> [modulators::EnvScale; modulators::WARPS] {
        let mut out = [modulators::EnvScale::UNITY; modulators::WARPS];
        let Some(g) = self.groups.get(zone.group as usize) else {
            return out;
        };
        let reshaped = self.reshaped.get(zone.group as usize);
        for ((n, scale), e) in (0u8..).zip(&mut out).zip(&g.envelopes) {
            let drawn = |i: usize| e.points.get(i).map_or(0.0, |p| p.ms.max(0.0));
            let mut attack_ms = drawn(0);
            let mut hold_ms = if e.stages { drawn(1) } else { 0.0 };
            let mut decay_ms = if e.stages { drawn(2) } else { 0.0 };
            let rel_idx = e.sustain.map_or(usize::MAX, |s| s + 1);
            let mut release_ms = drawn(rel_idx);
            let mut sustain = 1.0f32;
            let routes = reshaped.into_iter().flat_map(|r| r.enveloping(n));
            for m in routes.filter_map(|i| g.mods.get(i)).filter(|m| !m.bypass) {
                // Sources are read at note-on, as Kontakt reads them. An
                // envelope source is then its start level: its arm reads
                // the voice's latched stretch, never these routes, so an
                // envelope timing itself (or another) cannot recurse.
                match m.kind {
                    ir::ModKind::Attack {
                        source,
                        intensity,
                        scale: ts,
                        ..
                    } if drawn(0) > 0.0 => {
                        if let Some(x) = self.unit(zone, source, note, age) {
                            attack_ms = time_route(attack_ms, x, intensity, ts);
                        }
                    }
                    ir::ModKind::Hold {
                        source,
                        intensity,
                        scale: ts,
                        ..
                    } if e.stages && drawn(1) > 0.0 => {
                        if let Some(x) = self.unit(zone, source, note, age) {
                            hold_ms = time_route(hold_ms, x, intensity, ts);
                        }
                    }
                    ir::ModKind::Decay {
                        source,
                        intensity,
                        scale: ts,
                        ..
                    } if e.stages && drawn(2) > 0.0 => {
                        if let Some(x) = self.unit(zone, source, note, age) {
                            decay_ms = time_route(decay_ms, x, intensity, ts);
                        }
                    }
                    ir::ModKind::Sustain {
                        source,
                        intensity,
                        curve,
                        ..
                    } if e.stages => {
                        if let Some(x) = self.unit(zone, source, note, age) {
                            sustain *= curve.gain(intensity, x);
                        }
                    }
                    ir::ModKind::Release {
                        source,
                        intensity,
                        scale: ts,
                        ..
                    } if drawn(rel_idx) > 0.0 => {
                        if let Some(x) = self.unit(zone, source, note, age) {
                            release_ms = time_route(release_ms, x, intensity, ts);
                        }
                    }
                    _ => {}
                }
            }
            if drawn(0) > 0.0 {
                scale.attack = attack_ms / drawn(0);
            }
            if e.stages && drawn(1) > 0.0 {
                scale.hold = hold_ms / drawn(1);
            }
            if e.stages && drawn(2) > 0.0 {
                scale.decay = decay_ms / drawn(2);
            }
            if drawn(rel_idx) > 0.0 {
                scale.release = release_ms / drawn(rel_idx);
            }
            scale.sustain = sustain;
        }
        out
    }

    /// Re-evaluates breakpoint envelope release stretches live at note-off.
    /// Returns `None` when the group has no active breakpoint release routes.
    pub(crate) fn release_stretches(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
    ) -> Option<[Option<f32>; modulators::WARPS]> {
        let g = self.groups.get(zone.group as usize)?;
        let reshaped = self.reshaped.get(zone.group as usize)?;
        let mut any = false;
        let mut out = [None; modulators::WARPS];
        for ((n, rel_scale), e) in (0u8..).zip(&mut out).zip(&g.envelopes) {
            let rel_idx = e.sustain.map_or(usize::MAX, |s| s + 1);
            let drawn = e.points.get(rel_idx).map_or(0.0, |p| p.ms.max(0.0));
            if drawn <= 0.0 {
                continue;
            }
            let mut ms = drawn;
            let mut env_hit = false;
            for m in reshaped
                .enveloping(n)
                .filter_map(|i| g.mods.get(i))
                .filter(|m| !m.bypass)
            {
                if let ir::ModKind::Release {
                    source,
                    intensity,
                    scale,
                    ..
                } = m.kind
                    && let Some(x) = self.unit(zone, source, note, age)
                {
                    ms = time_route(ms, x, intensity, scale);
                    env_hit = true;
                }
            }
            if env_hit {
                *rel_scale = Some(ms / drawn);
                any = true;
            }
        }
        any.then_some(out)
    }

    /// Whether the group's primary amplitude envelope is bypassed.
    fn amp_envelope_bypassed(g: &crate::Group) -> bool {
        let mut has_env = false;
        for m in &g.mods {
            if m.kind == ir::ModKind::Envelope {
                has_env = true;
                if !m.bypass {
                    return false;
                }
            }
        }
        has_env
    }

    /// The group's amplitude envelope for a voice, its attack, hold, decay,
    /// sustain, and initial release scaled by their routes.
    pub(crate) fn envelope(&self, zone: &Zone, note: &Note, age: Age) -> Ahdsr {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return Ahdsr::GATE;
        };
        if Self::amp_envelope_bypassed(g) {
            return Ahdsr::GATE;
        }
        let mut e = g.envelope;
        let rate = self.rate as f32;
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModKind::Sustain {
                source,
                intensity,
                curve,
                envelope: None,
            } = m.kind
            {
                let Some(x) = self.unit(zone, source, note, age) else {
                    continue;
                };
                e.sustain = (e.sustain * curve.gain(intensity, x)).clamp(0.0, 1.0);
                continue;
            }
            let (source, intensity, scale, frames) = match m.kind {
                ir::ModKind::Attack {
                    source,
                    intensity,
                    scale,
                    envelope: None,
                } => (source, intensity, scale, &mut e.attack),
                ir::ModKind::Hold {
                    source,
                    intensity,
                    scale,
                    envelope: None,
                } => (source, intensity, scale, &mut e.hold),
                ir::ModKind::Decay {
                    source,
                    intensity,
                    scale,
                    envelope: None,
                } => (source, intensity, scale, &mut e.decay),
                ir::ModKind::Release {
                    source,
                    intensity,
                    scale,
                    envelope: None,
                } => (source, intensity, scale, &mut e.release),
                _ => continue,
            };
            let Some(x) = self.unit(zone, source, note, age) else {
                continue;
            };
            let ms = time_route(*frames as f32 * 1000.0 / rate, x, intensity, scale);
            *frames = (ms * rate / 1000.0).round() as u32;
            #[cfg(feature = "trace")]
            self.trace_route(
                zone,
                m,
                Some(x),
                *frames as f32,
                crate::RouteUnit::EnvelopeFrames,
            );
        }
        e
    }

    /// Evaluates active [`ir::ModKind::Release`] routes on the amplitude
    /// envelope live at note-off (so CCs, pitch bend, LFOs, and release
    /// velocity moved while the key was held take effect). Returns `None`
    /// when the amplitude envelope is bypassed or has no active release routes.
    pub(crate) fn release_frames(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
        release_base: u32,
    ) -> Option<u32> {
        let g = self.groups.get(zone.group as usize)?;
        if Self::amp_envelope_bypassed(g) {
            return None;
        }
        let rate = self.rate as f32;
        let mut frames = release_base;
        let mut hit = false;
        for m in g.mods.iter().filter(|m| !m.bypass) {
            let ir::ModKind::Release {
                source,
                intensity,
                scale,
                envelope: None,
            } = m.kind
            else {
                continue;
            };
            let Some(x) = self.unit(zone, source, note, age) else {
                continue;
            };
            let ms = time_route(frames as f32 * 1000.0 / rate, x, intensity, scale);
            frames = (ms * rate / 1000.0).round() as u32;
            hit = true;
        }
        hit.then_some(frames)
    }

    /// Whether moving `cc` changes this zone's level.
    pub(crate) fn follows(&self, zone: &Zone, cc: u8) -> bool {
        zone.crossfades.iter().any(|x| x.control == Control::Cc(cc))
            || self.groups.get(zone.group as usize).is_some_and(|g| {
                g.mods.iter().any(|m| match m.kind {
                    ir::ModKind::Volume { source, .. }
                    | ir::ModKind::Pitch { source, .. }
                    | ir::ModKind::Pan { source, .. }
                    | ir::ModKind::Cutoff { source, .. }
                    | ir::ModKind::CutoffHz { source, .. }
                    | ir::ModKind::Resonance { source, .. }
                    | ir::ModKind::EqGain { source, .. }
                    | ir::ModKind::EffectParam { source, .. }
                    | ir::ModKind::Mix { source, .. }
                    | ir::ModKind::Intensity { source, .. }
                    | ir::ModKind::Wavetable { source, .. } => g.reads(source, cc),
                    ir::ModKind::Envelope
                    | ir::ModKind::SampleStart { .. }
                    | ir::ModKind::Attack { .. }
                    | ir::ModKind::Hold { .. }
                    | ir::ModKind::Decay { .. }
                    | ir::ModKind::Sustain { .. }
                    | ir::ModKind::Release { .. }
                    | ir::ModKind::LfoPhase { .. }
                    | ir::ModKind::LfoRate { .. }
                    | ir::ModKind::LfoHz { .. }
                    | ir::ModKind::LfoDivision { .. }
                    | ir::ModKind::Send { .. }
                    | ir::ModKind::LoopShift { .. } => false,
                })
            })
    }

    /// Whether moving `cc` changes this zone's pitch.
    pub(crate) fn pitch_follows(&self, zone: &Zone, cc: u8) -> bool {
        self.groups.get(zone.group as usize).is_some_and(|g| {
            g.mods.iter().any(|m| {
                matches!(m.kind, ir::ModKind::Pitch { source, .. }
                    | ir::ModKind::Intensity { source, .. } if g.reads(source, cc))
            })
        })
    }
}

// ponytail: balance pan law only; equal-power and Kontakt's raised cosine
// when a library's measured stereo image needs them.
fn gain(zone: &Zone, note: &Note, level: Level, modulated: f32) -> [f32; 2] {
    let script = ir::common::MILLIBEL_GAIN.eval_unclamped(note.script_volume as f32);
    let g = zone.gain * level.gain * note.expression_gain * script;
    let pan =
        (zone.pan + level.pan + modulated + note.expression_pan + note.script_pan as f32 / 1000.0)
            .clamp(-1.0, 1.0);
    [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)]
}

/// An envelope stage duration `ms` as a time route sets it from its source
/// `x` (0..=1): `m = 1 − (1 − x')·|intensity|` (`x' = 1 − x` for negative
/// intensity), then either `ms · m^power` ([`ir::TimeScale::Linear`]) or a
/// knob linear in `ln(ms + floor_ms)` ([`ir::TimeScale::LogKnob`]).
fn time_route(ms: f32, x: f32, intensity: f32, scale: ir::TimeScale) -> f32 {
    let x = if intensity < 0.0 { 1.0 - x } else { x };
    let multiplier = (1.0 - (1.0 - x.clamp(0.0, 1.0)) * intensity.abs()).max(0.0);
    let ms = match scale {
        ir::TimeScale::Linear { power } => ms * multiplier.powi(i32::from(power)),
        ir::TimeScale::LogKnob { floor_ms, power } if floor_ms > 0.0 => {
            let span = ir::common::geometric(floor_ms, (ms + floor_ms) / floor_ms);
            span.eval_unclamped(multiplier.powi(i32::from(power))) - floor_ms
        }
        ir::TimeScale::LogKnob { power, .. } => ms * multiplier.powi(i32::from(power)),
    };
    ms.max(0.0)
}

/// The wavetable value `v` of `read`.
pub(crate) fn table_value(read: &mut Read, v: ir::TableValue) -> Option<&mut f32> {
    match v {
        Position => Some(&mut read.position),
        Warp(n) => read.warps.get_mut(usize::from(n)).map(|w| &mut w.amount),
        Inharmonic => Some(&mut read.inharmonic.amount),
        ModAmount => Some(&mut read.modosc.amount),
        ModTune => Some(&mut read.modosc.tune),
        Phase => Some(&mut read.phase),
        PhaseRandom => Some(&mut read.phase_random),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modulation_dependencies_are_scoped_to_an_active_evaluation() {
        let evaluation = Evaluation::default();
        let motion = evaluation.enter(Dependency::LfoMotion, 255).unwrap();
        let intensity = evaluation.enter(Dependency::LfoIntensity, 255).unwrap();
        assert!(evaluation.enter(Dependency::LfoMotion, 255).is_none());
        assert!(
            evaluation
                .enter(Dependency::EnvelopeIntensity, 255)
                .is_some()
        );
        drop(intensity);
        drop(motion);
        assert!(evaluation.enter(Dependency::LfoMotion, 255).is_some());
        assert_eq!(evaluation.depth.get(), 0);
    }

    #[test]
    fn sibling_reads_under_one_root_share_a_work_budget() {
        let evaluation = Evaluation::default();
        let root = evaluation.enter(Dependency::LfoMotion, 0).unwrap();
        // Siblings enter and leave at depth 1: without a budget, unbounded.
        let entered = (0..5000)
            .filter(|_| evaluation.enter(Dependency::LfoMotion, 1).is_some())
            .count();
        assert!(entered < 1024, "{entered}");
        drop(root);
        // The next root read starts a fresh budget.
        assert!(evaluation.enter(Dependency::LfoMotion, 0).is_some());
    }
}
