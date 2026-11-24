//! A voice's level and pitch step from its zone, its note and the group's
//! modulation; re-read when a controller or expression moves.

use std::cell::Cell;

use ir::{EnvStage as S, RouteLaw as L};
use kontra_ir as ir;

use crate::ledger::Note;
use ir::TableValue::{Inharmonic, ModAmount, ModTune, Phase, PhaseRandom, Position, Warp};
use kontra_stream::Mix;
use kontra_voice::{Ahdsr, Gain, LAGS, Lag, Lags, Read};

use crate::effects::{self, product};
use crate::modulators::{self, Age};
use crate::{Control, Engine, Level, Zone};

/// A voice's lagging pitch routes, each its semitones and time constant
/// ([`lag_into`]); the voice glides toward each a control step at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bend {
    pub(crate) lagging: [(f32, f32); LAGS],
    /// The routes that do not lag.
    pub(crate) instant: f32,
}

impl Bend {
    pub(crate) const NONE: Self = Self {
        lagging: [(0.0, 0.0); LAGS],
        instant: 0.0,
    };

    /// Every lagging route's semitones, as they will be once glided.
    pub(crate) fn lagged(&self) -> f32 {
        self.lagging.iter().map(|&(st, _)| st).sum()
    }
}

/// The `k`th lagging route's `value` and time constant into `lags`, each
/// route its own one-pole; past the last, folded into it (`fold`) at the
/// longer time constant.
fn lag_into(
    lags: &mut [(f32, f32); LAGS],
    k: &mut usize,
    value: f32,
    tau_ms: f32,
    fold: fn(f32, f32) -> f32,
) {
    let fresh = *k < LAGS;
    if let Some(slot) = lags.get_mut((*k).min(LAGS - 1)) {
        let (v, tau) = *slot;
        *slot = if fresh {
            (value, tau_ms)
        } else {
            (fold(v, value), tau.max(tau_ms))
        };
    }
    *k += 1;
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
    /// The voice's gain, and the slower gains of its routes that lag, each
    /// at its own time constant ([`lag_into`]).
    /// `group_pan` is where the voice's glide of its group's pan is.
    pub(crate) fn voice_gain(
        &self,
        zone: &Zone,
        note: &Note,
        factor: f32,
        age: Age,
        group_pan: f32,
    ) -> (Gain, Lags) {
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
        let (mut mods, mut lags, mut k) = (1.0f32, [(1.0f32, 0.0f32); LAGS], 0);
        let groups = self.groups.get(zone.group as usize);
        for m in groups.iter().flat_map(|g| &g.mods).filter(|m| !m.bypass) {
            if let ir::ModRoute {
                source,
                target: ir::ParamAddr::Volume,
                depth: intensity,
                law: ir::RouteLaw::Gain(curve),
                smooth_ms: tau_ms,
                ..
            } = m.route
            {
                let intensity = intensity * self.scale(zone, source, note, age);
                let input = self.unit(zone, source, note, age);
                let g = input.map_or(1.0, |x| curve.gain(intensity, x));
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, g, crate::RouteUnit::GainMultiplier);
                if tau_ms > 0.0 {
                    lag_into(&mut lags, &mut k, g, tau_ms, |a, b| a * b);
                } else {
                    mods *= g;
                }
            }
        }
        let pan: f32 = self.groups.get(zone.group as usize).map_or(0.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .filter_map(|m| match m.route {
                    ir::ModRoute {
                        source,
                        target: ir::ParamAddr::Pan,
                        depth: amount,
                        ..
                    } => {
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
        let [l, r] = gain(zone, note, level, pan, self.plan.output.balance);
        let g = crossfade * mods * factor;
        let amp = [[l * g, 0.0], [0.0, r * g]];
        let gain = self.groups.get(zone.group as usize).map_or(amp, |g| {
            // The group's sum runs the slots from its split on; matrix slots
            // already placed in the per-voice rack (`Kind::Matrix`) run there
            // in exact slot order around drives rather than commuting here.
            let group_idx = zone.group as usize;
            let group_split = self
                .group_split
                .get(group_idx)
                .copied()
                .flatten()
                .unwrap_or(g.inserts.len())
                .min(g.inserts.len());
            let amp_split = g.amp_split_index().min(group_split);
            let units = self
                .voice_units
                .get(group_idx)
                .map_or(&[][..], |u| u.as_ref());
            let in_voice_rack = |slot: usize| {
                units.iter().any(|u| {
                    usize::from(u.insert) == slot
                        && matches!(u.kind, crate::voice_rack::Kind::Matrix)
                })
            };
            let fold_range = |start: usize, end: usize| {
                g.inserts
                    .get(start..end)
                    .unwrap_or_default()
                    .iter()
                    .enumerate()
                    .filter(|&(rel, _)| !in_voice_rack(start + rel))
                    .filter_map(|(_, e)| e.as_ref())
                    .filter(|e| !e.bypass && !effects::rack_only(&e.kind))
                    .fold(effects::IDENTITY, |m, e| product(effects::matrix(e), m))
            };
            let pre = fold_range(0, amp_split);
            let post = fold_range(amp_split, group_split);
            product(post, product(amp, pre))
        });
        let lags = lags.map(|(g, tau)| Lag::new(g, tau, self.rate));
        (gain, lags)
    }

    /// The zone's channel matrix with its live cells ([`ir::ParamAddr::ZoneMix`])
    /// read for this note; `None` when no route moves it.
    pub(crate) fn voice_mix(&self, zone: &Zone, note: &Note, age: Age) -> Option<Mix> {
        let g = self.groups.get(zone.group as usize)?;
        let mut mix = *zone.region.mix.as_deref()?;
        let mut moved = false;
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModRoute {
                source,
                target: ir::ParamAddr::ZoneMix { zone: z, row, side },
                ..
            } = m.route
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
            if let ir::ModRoute {
                source,
                target: ir::ParamAddr::Wavetable(value),
                depth,
                ..
            } = m.route
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
            return age.chaos.get(usize::from(n)).map(|c| c[3]);
        }
        let seed = match lfo.clock {
            ir::Clock::Voice => note.draw.seed(),
            ir::Clock::Free => zone.group.wrapping_mul(32).wrapping_add(u32::from(n)),
        };
        Some(modulators::lfo(&lfo, age, seed, self.tempo))
    }

    /// Latches note-on LFO phase offsets from [`ir::LfoParam::Phase`] routes
    /// into `out` (one per LFO), so moving a controller mid-note does not shift
    /// the phase of an already-sounding voice.
    pub(crate) fn lfo_phase_offsets(&self, zone: &Zone, note: &Note, onset: Age, out: &mut [f32]) {
        out.fill(0.0);
        let Some((g, reshaped)) = self
            .groups
            .get(zone.group as usize)
            .zip(self.reshaped.get(zone.group as usize))
        else {
            return;
        };
        for (n, offset) in (0u8..=u8::MAX).zip(out.iter_mut()).take(g.lfos.len()) {
            let Some(_active) = self.evaluation.enter(Dependency::LfoMotion, n) else {
                continue;
            };
            for m in reshaped
                .moving(n)
                .filter_map(|i| g.mods.get(i))
                .filter(|m| !m.bypass)
            {
                let ir::ModRoute {
                    source,
                    target:
                        ir::ParamAddr::LfoParam {
                            lfo: l,
                            param: ir::LfoParam::Phase,
                        },
                    depth,
                    ..
                } = m.route
                else {
                    continue;
                };
                if l != n || matches!(source, ir::Source::Lfo(_)) {
                    continue;
                }
                let input = self.source(zone, source, note, onset);
                let value = input.unwrap_or(0.0) * depth;
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, value, crate::RouteUnit::PhaseCycles);
                *offset += value;
            }
        }
    }

    /// LFO `n` as this note plays it: its group's, with the latched note-on
    /// phase offset, fixed note-on rate routes, and live phase warp applied.
    fn moved_lfo(&self, zone: &Zone, n: u8, note: &Note, age: Age) -> Option<ir::Lfo> {
        let group = || self.groups.get(zone.group as usize);
        let g = group()?;
        let mut lfo = *g.lfos.get(usize::from(n))?;
        let _active = self.evaluation.enter(Dependency::LfoMotion, n)?;
        let moving = (self.reshaped.get(zone.group as usize).into_iter())
            .flat_map(|r| r.moving(n))
            .filter_map(|i| g.mods.get(i));
        // Phase offsets are latched at note-on in `age.lfo_phase`; rate routes
        // whose source is fixed for the note apply here, while live rate routes
        // integrate into `age.warp` from note-on on (`Engine::warp`).
        let onset = Age::at_start(age.start_frame);
        for m in moving.filter(|m| !m.bypass) {
            let ir::ModRoute {
                source,
                target: ir::ParamAddr::LfoParam { lfo: l, param },
                depth,
                span: octaves,
                law,
                ..
            } = m.route
            else {
                continue;
            };
            // An LFO source here would recurse.
            if l != n || matches!(source, ir::Source::Lfo(_)) {
                continue;
            }
            if let (ir::LfoParam::Rate, ir::RouteLaw::Range([lo, hi])) = (param, law)
                && source.fixed_for_note()
            {
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
        }
        lfo.phase += age.lfo_phase.get(usize::from(n)).copied().unwrap_or(0.0);
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
            ir::Source::Controller(cc) => Some(f32::from(self.behaviour.cc(channel, cc)) / 127.0),
            ir::Source::ControllerFrom { cc, start } => {
                let v = if self.behaviour.cc_arrived(channel, cc) {
                    self.behaviour.cc(channel, cc)
                } else {
                    start
                };
                Some(f32::from(v) / 127.0)
            }
            ir::Source::PitchBend => Some(self.bend(note, channel)),
            ir::Source::Pressure => {
                Some((note.expression_pressure).unwrap_or_else(|| self.behaviour.pressure(channel)))
            }
            ir::Source::NoteExpr(expr) => Some(match expr {
                ir::NoteExpr::Pressure => note
                    .expression_pressure
                    .unwrap_or_else(|| self.behaviour.pressure(channel)),
                ir::NoteExpr::Brightness => note
                    .expression_brightness
                    .unwrap_or_else(|| f32::from(self.behaviour.cc(channel, 74)) / 127.0),
                ir::NoteExpr::Vibrato => note
                    .expression_vibrato
                    .unwrap_or_else(|| f32::from(self.behaviour.cc(channel, 1)) / 127.0),
                ir::NoteExpr::Expression => note
                    .expression_expression
                    .unwrap_or_else(|| f32::from(self.behaviour.cc(channel, 11)) / 127.0),
                ir::NoteExpr::Tune => ((note.expression_tune as f32) / 12.0).clamp(-1.0, 1.0),
                ir::NoteExpr::Gain => note.expression_gain,
                ir::NoteExpr::Pan => note.expression_pan,
            }),
            ir::Source::PerNoteCc { registered, index } => {
                Some(note.per_note_cc(registered, index).unwrap_or(0.0))
            }
            ir::Source::Key => Some(f32::from(note.key) / 127.0),
            ir::Source::Constant => Some(1.0),
            ir::Source::Script(_) | ir::Source::ScriptBipolar(_) | ir::Source::Macro(_) => {
                let index = source.script_channel()?;
                Some(
                    note.mod_value(index, self.now)
                        .unwrap_or_else(|| self.script_mod(index)) as f32
                        / 1_000_000.0,
                )
            }
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

    /// `note`'s pitch bend, −1..=1: its channel's wheel, and its own bend
    /// (per-note MIDI 2.0 or MPE, a host's tune expression) as a wheel of
    /// [`PER_NOTE_BEND_SEMITONES`](kontra_events::PER_NOTE_BEND_SEMITONES).
    fn bend(&self, note: &Note, channel: u8) -> f32 {
        let own = note.expression_tune / kontra_events::PER_NOTE_BEND_SEMITONES;
        (self.behaviour.bend(channel) + own as f32).clamp(-1.0, 1.0)
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
        let normalize = t.source.is_bipolar()
            && (!t.table.is_empty() || matches!(t.source, ir::Source::Lfo(_)));
        let x = if normalize { (x + 1.0) * 0.5 } else { x };
        Some(t.apply(x))
    }

    /// A route's source scaled by the [`ir::ParamAddr::RouteIntensity`] routes on it.
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
        Some(if s.is_bipolar() { (x + 1.0) * 0.5 } else { x })
    }

    /// The product of the [`ir::ParamAddr::RouteIntensity`] routes on the timed source `of`.
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
                .filter_map(|m| match m.route {
                    ir::ModRoute {
                        source,
                        target: ir::ParamAddr::RouteIntensity { of: o },
                        depth,
                        ..
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
        let (mut bend, mut k) = (Bend::NONE, 0);
        let groups = self.groups.get(zone.group as usize);
        for m in groups.iter().flat_map(|g| &g.mods).filter(|m| !m.bypass) {
            if let ir::ModRoute {
                source,
                target: ir::ParamAddr::Pitch,
                depth,
                span: semitones,
                smooth_ms: tau_ms,
                ..
            } = m.route
            {
                // A note's own bend tunes it at once (`expression_tune`
                // below), not again through its wheel's route.
                let input = match source {
                    ir::Source::PitchBend => {
                        let wheel = self.behaviour.bend(note.host.port_channel());
                        Some(wheel * self.scale(zone, source, note, age))
                    }
                    _ => self.routed(zone, source, note, age),
                };
                let st = input.unwrap_or(0.0) * semitones * depth;
                #[cfg(feature = "trace")]
                self.trace_route(zone, m, input, st, crate::RouteUnit::Semitones);
                if tau_ms > 0.0 {
                    lag_into(&mut bend.lagging, &mut k, st, tau_ms, |a, b| a + b);
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
    pub(crate) fn loop_shift(&self, zone: &Zone, note: &Note, age: Age) -> [f32; 2] {
        let mut shift = [0.0; 2];
        let Some(g) = self.groups.get(zone.group as usize) else {
            return shift;
        };
        for m in g.mods.iter().filter(|m| !m.bypass) {
            if let ir::ModRoute {
                source,
                target: ir::ParamAddr::LoopShift { length },
                depth,
                ..
            } = m.route
                && let Some(x) = self.source(zone, source, note, age)
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
    pub(crate) fn start_offset(&self, zone: &Zone, note: &Note, age: Age) -> i64 {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return 0;
        };
        let x: f32 = g
            .mods
            .iter()
            .filter(|m| !m.bypass)
            .filter_map(|m| match m.route {
                ir::ModRoute {
                    source,
                    target: ir::ParamAddr::SampleStart,
                    depth,
                    ..
                } => {
                    let x = self.source(zone, source, note, age)?;
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

    /// How much envelope modulation routes scale each of the group's
    /// breakpoint envelopes for a voice starting now, into `out` (one per
    /// envelope): attack, hold, decay, sustain (and initial release) are
    /// latched at note-on; release is refreshed live at note-off via
    /// [`Self::release_stretches`].
    pub(crate) fn stretches(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
        out: &mut [modulators::EnvScale],
    ) {
        out.fill(modulators::EnvScale::UNITY);
        let Some(g) = self.groups.get(zone.group as usize) else {
            return;
        };
        let reshaped = self.reshaped.get(zone.group as usize);
        for ((n, scale), e) in (0u8..=u8::MAX).zip(out).zip(&g.envelopes) {
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
                let r = m.route;
                let ir::ParamAddr::EnvelopeStage { stage, .. } = r.target else {
                    continue;
                };
                let Some(x) = self.unit(zone, r.source, note, age) else {
                    continue;
                };
                let intensity = r.depth;
                match (stage, r.law) {
                    (S::Attack, L::Time(ts)) if drawn(0) > 0.0 => {
                        attack_ms = time_route(attack_ms, x, intensity, ts);
                    }
                    (S::Hold, L::Time(ts)) if e.stages && drawn(1) > 0.0 => {
                        hold_ms = time_route(hold_ms, x, intensity, ts);
                    }
                    (S::Decay, L::Time(ts)) if e.stages && drawn(2) > 0.0 => {
                        decay_ms = time_route(decay_ms, x, intensity, ts);
                    }
                    (S::Sustain, L::Gain(curve)) if e.stages => {
                        sustain *= curve.gain(intensity, x);
                    }
                    (S::Release, L::Time(ts)) if drawn(rel_idx) > 0.0 => {
                        release_ms = time_route(release_ms, x, intensity, ts);
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
    }

    /// Re-evaluates breakpoint envelope release stretches live at note-off,
    /// into `out`'s releases (one per envelope; the rest as they were).
    /// False when the group has no active breakpoint release routes.
    pub(crate) fn release_stretches(
        &self,
        zone: &Zone,
        note: &Note,
        age: Age,
        out: &mut [modulators::EnvScale],
    ) -> bool {
        let Some((g, reshaped)) = self
            .groups
            .get(zone.group as usize)
            .zip(self.reshaped.get(zone.group as usize))
        else {
            return false;
        };
        let mut any = false;
        for ((n, rel_scale), e) in (0u8..=u8::MAX).zip(out).zip(&g.envelopes) {
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
                if let ir::ModRoute {
                    source,
                    target:
                        ir::ParamAddr::EnvelopeStage {
                            stage: S::Release, ..
                        },
                    depth: intensity,
                    law: L::Time(scale),
                    ..
                } = m.route
                    && let Some(x) = self.unit(zone, source, note, age)
                {
                    ms = time_route(ms, x, intensity, scale);
                    env_hit = true;
                }
            }
            if env_hit {
                rel_scale.release = ms / drawn;
                any = true;
            }
        }
        any
    }

    /// Whether the group's primary amplitude envelope is bypassed.
    fn amp_envelope_bypassed(g: &crate::Group) -> bool {
        let mut has_env = false;
        for m in &g.mods {
            if m.route.target == ir::ParamAddr::AmpEnvelope {
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
            let r = m.route;
            let ir::ParamAddr::EnvelopeStage {
                envelope: None,
                stage,
            } = r.target
            else {
                continue;
            };
            let (source, intensity) = (r.source, r.depth);
            if let (S::Sustain, L::Gain(curve)) = (stage, r.law) {
                let Some(x) = self.unit(zone, source, note, age) else {
                    continue;
                };
                e.sustain = (e.sustain * curve.gain(intensity, x)).clamp(0.0, 1.0);
                continue;
            }
            let L::Time(scale) = r.law else {
                continue;
            };
            let frames = match stage {
                S::Attack => &mut e.attack,
                S::Hold => &mut e.hold,
                S::Decay => &mut e.decay,
                S::Release => &mut e.release,
                S::Sustain => continue,
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

    /// Evaluates active release routes on the amplitude
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
            let ir::ModRoute {
                source,
                target:
                    ir::ParamAddr::EnvelopeStage {
                        envelope: None,
                        stage: S::Release,
                    },
                depth: intensity,
                law: L::Time(scale),
                ..
            } = m.route
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
                g.mods.iter().any(|m| {
                    let r = m.route;
                    let live = match r.target {
                        ir::ParamAddr::Volume
                        | ir::ParamAddr::Pitch
                        | ir::ParamAddr::Pan
                        | ir::ParamAddr::FilterCutoff { .. }
                        | ir::ParamAddr::FilterResonance { .. }
                        | ir::ParamAddr::EqBandGain { .. }
                        | ir::ParamAddr::EffectParam { .. }
                        | ir::ParamAddr::ZoneMix { .. }
                        | ir::ParamAddr::RouteIntensity { .. }
                        | ir::ParamAddr::Wavetable(_) => true,
                        ir::ParamAddr::AmpEnvelope
                        | ir::ParamAddr::SampleStart
                        | ir::ParamAddr::EnvelopeStage { .. }
                        | ir::ParamAddr::LfoParam { .. }
                        | ir::ParamAddr::BusSend { .. }
                        | ir::ParamAddr::LoopShift { .. } => false,
                    };
                    live && g.reads(r.source, cc)
                })
            })
    }

    /// Whether moving `cc` changes this zone's pitch.
    pub(crate) fn pitch_follows(&self, zone: &Zone, cc: u8) -> bool {
        self.groups.get(zone.group as usize).is_some_and(|g| {
            g.mods.iter().any(|m| {
                let r = m.route;
                matches!(
                    r.target,
                    ir::ParamAddr::Pitch | ir::ParamAddr::RouteIntensity { .. }
                ) && g.reads(r.source, cc)
            })
        })
    }
}

/// A voice's left and right gains: its levels, and its pans summed and
/// balanced by `law`.
fn gain(zone: &Zone, note: &Note, level: Level, modulated: f32, law: ir::Balance) -> [f32; 2] {
    let script = ir::common::MILLIBEL_GAIN.eval_unclamped(note.script_volume as f32);
    let g = zone.gain * level.gain * note.expression_gain * script;
    let pan =
        (zone.pan + level.pan + modulated + note.expression_pan + note.script_pan as f32 / 1000.0)
            .clamp(-1.0, 1.0);
    law.gains(pan).map(|side| g * side)
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

    #[test]
    fn kt01_ahdsr_time_route_and_sustain_match_binary_fcubic_values() {
        // Binary 0x140873120 / 0x140876753: 1000 ms attack at x = 0.5, i = 1.0:
        // - types 7..=13 (power 3): 2 * 501^(0.5^3) - 2 = 2.35023 ms
        // - types 1..=5  (power 1): 2 * 501^0.5 - 2     = 42.76606 ms
        let cubed = time_route(
            1000.0,
            0.5,
            1.0,
            ir::TimeScale::LogKnob {
                floor_ms: 2.0,
                power: 3,
            },
        );
        let expected_cubed = 2.0 * 501.0f32.powf(0.125) - 2.0;
        assert!(
            (cubed - expected_cubed).abs() < 1e-4,
            "got {cubed}, want {expected_cubed}"
        );
        let plain = time_route(
            1000.0,
            0.5,
            1.0,
            ir::TimeScale::LogKnob {
                floor_ms: 2.0,
                power: 1,
            },
        );
        let expected_plain = 2.0 * 501.0f32.sqrt() - 2.0;
        assert!(
            (plain - expected_plain).abs() < 1e-3,
            "got {plain}, want {expected_plain}"
        );

        // Sustain with cubed source: S' = (u * m^3)^3 = S * m^9 (0.5^9 = 1/512).
        let sustain_cubed = ir::Curve::Depth { power: 9 }.gain(1.0, 0.5);
        assert!(
            (sustain_cubed - (1.0 / 512.0)).abs() < 1e-6,
            "got {sustain_cubed}, want {}",
            1.0 / 512.0
        );
        let sustain_plain = ir::Curve::Depth { power: 3 }.gain(1.0, 0.5);
        assert!(
            (sustain_plain - 0.125).abs() < 1e-6,
            "got {sustain_plain}, want 0.125"
        );
    }

    #[test]
    fn kt04_bipolar_script_source_normalizes_through_shaper_and_unit() {
        use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};
        use std::sync::Arc;

        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let fmt = SampleFmt {
            encoding: Encoding::F32,
            channels: 1,
        };
        let key = BlobKey {
            file: "kt04".into(),
            offset: 0,
            fmt,
        };
        let sample = store
            .load(
                &key,
                Arc::new(MemSource::new(fmt, vec![0u8; 4 * 64])),
                64,
                std::slice::from_ref(&(0..64)),
            )
            .unwrap();
        let region = kontra_voice::Region {
            mix: None,
            table: None,
            sample,
            start: 0,
            end: 64,
            loops: Default::default(),
            reverse: false,
        };
        let zone = Zone::new(region, 60, 48_000);
        let mut plan = crate::Plan::new(vec![zone.clone()], 8);
        let shaper = ir::Transfer {
            source: ir::Source::ScriptBipolar(0),
            table: vec![-1.0, 0.0, 1.0],
            smooth_s: 0.0,
            stepped: false,
        };
        plan.groups[0].shapers.push(shaper.clone());
        let engine = Engine::new(Arc::new(plan), store.client(), 48_000);
        let mut note = Note::new(
            kontra_events::HostNote {
                port: 0,
                channel: 0,
                key: 60,
                id: 1,
                clap: false,
            },
            1.0,
            0.0,
            kontra_time::SampleTime(0),
            kontra_behaviour::RrDraw::default(),
        );

        // At rest (0), bipolar script maps to midpoint 0.5 in unit() and 0.0 through [-1, 0, 1].
        let u_zero = engine
            .unit(&zone, shaper.source, &note, Age::START)
            .unwrap();
        let t_zero = engine
            .through(&zone, &shaper, &note, Age::START, false)
            .unwrap();
        assert!(
            (u_zero - 0.5).abs() < 1e-6,
            "unit(0): got {u_zero}, want 0.5"
        );
        assert!(t_zero.abs() < 1e-6, "through(0): got {t_zero}, want 0.0");

        // Negative script value (-500_000 -> -0.5) must not clamp to 0.0.
        note.set_mod_value(0, -500_000, kontra_time::SampleTime(0), 0, 0);
        let u_neg = engine
            .unit(&zone, shaper.source, &note, Age::START)
            .unwrap();
        let t_neg = engine
            .through(&zone, &shaper, &note, Age::START, false)
            .unwrap();
        assert!(
            (u_neg - 0.25).abs() < 1e-6,
            "unit(-0.5): got {u_neg}, want 0.25"
        );
        assert!(
            (t_neg - -0.5).abs() < 1e-6,
            "through(-0.5): got {t_neg}, want -0.5"
        );

        // Unipolar Script(0) stays 0..=1 so volume routes keep 250_000 -> 0.25.
        note.set_mod_value(0, 250_000, kontra_time::SampleTime(0), 0, 0);
        let u_uni = engine
            .unit(&zone, ir::Source::Script(0), &note, Age::START)
            .unwrap();
        assert!(
            (u_uni - 0.25).abs() < 1e-6,
            "unipolar unit(0.25): got {u_uni}, want 0.25"
        );
    }
}
