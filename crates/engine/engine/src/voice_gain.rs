//! A voice's level and pitch step from its zone, its note and the group's
//! modulation; re-read when a controller or expression moves.

use kontra_ir as ir;

use crate::ledger::Note;
use kontra_stream::Mix;
use kontra_voice::{Ahdsr, Gain, Lag};

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
        let channel = note.host.channel;
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
                // HeldTime is fixed at the voice's start: see `release_counter`.
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
        let mut mix = zone.region.mix?;
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

    /// A live source's value for this note; `None` for one fixed at the voice's start.
    pub(crate) fn source(
        &self,
        zone: &Zone,
        source: ir::Source,
        note: &Note,
        age: Age,
    ) -> Option<f32> {
        let channel = note.host.channel;
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
            ir::Source::Envelope(n) => Some(modulators::envelope(
                group()?.envelopes.get(usize::from(n))?,
                age,
            )),
            ir::Source::Lfo(n) => {
                let g = group()?;
                let mut lfo = *g.lfos.get(usize::from(n))?;
                // ponytail: phase and rate routes read their source at
                // note-on (exact for velocity and note-on values); a live
                // controller moving the rate needs a per-voice phase accumulator.
                for m in g.mods.iter().filter(|m| !m.bypass) {
                    match m.kind {
                        // An LFO source here would recurse.
                        ir::ModKind::LfoPhase {
                            source,
                            lfo: l,
                            depth,
                        } if l == n && !matches!(source, ir::Source::Lfo(_)) => {
                            let input = self.source(zone, source, note, Age::START);
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
                            let input = self.source(zone, source, note, Age::START);
                            let f = (input.unwrap_or(0.0) * depth * octaves).exp2();
                            #[cfg(feature = "trace")]
                            self.trace_route(zone, m, input, f, crate::RouteUnit::RateMultiplier);
                            lfo.rate = match lfo.rate {
                                ir::Rate::Hz(hz) => ir::Rate::Hz((hz * f).clamp(lo, hi.max(lo))),
                                ir::Rate::Beats(b) => ir::Rate::Beats(b / f),
                                ir::Rate::Steps { beats, count } => {
                                    ir::Rate::Beats(beats * count / f)
                                }
                            };
                        }
                        _ => {}
                    }
                }
                if let ir::Wave::Chaos { .. } = lfo.wave {
                    return age.chaos.get(usize::from(n)).copied();
                }
                lfo.phase += age.warp.get(usize::from(n)).copied().unwrap_or(0.0);
                Some(modulators::lfo(&lfo, age, u32::from(note.key), self.tempo))
            }
            ir::Source::Velocity => Some(note.velocity as f32),
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
            ir::Source::Key => Some(f32::from(note.key) / 127.0),
            ir::Source::Constant => Some(1.0),
            ir::Source::Script(index) => Some(
                note.mod_value(index, self.now)
                    .unwrap_or_else(|| self.script_mod(index)) as f32
                    / 1_000_000.0,
            ),
            ir::Source::HeldTime { .. } => None,
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
        let x = match t.source {
            ir::Source::Shaped(_) => return None,
            ir::Source::Blend(_) if !shaper => return None,
            // A shaper reads an LFO at the depth its Intensity routes give it.
            s if shaper => self.source(zone, s, note, age)? * self.scale(zone, s, note, age),
            s => self.source(zone, s, note, age)?,
        };
        // An LFO spans the table as `(x + 1)/2`, and pitch bend a table.
        let bend = t.source == ir::Source::PitchBend && !t.table.is_empty();
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
            ir::Source::Lfo(_) | ir::Source::PitchBend => (x + 1.0) * 0.5,
            _ => x,
        })
    }

    /// The product of the [`ir::ModKind::Intensity`] routes on the timed source `of`.
    pub(crate) fn scale(&self, zone: &Zone, of: ir::Source, note: &Note, age: Age) -> f32 {
        if !matches!(of, ir::Source::Lfo(_) | ir::Source::Envelope(_)) {
            return 1.0;
        }
        self.groups.get(zone.group as usize).map_or(1.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .filter_map(|m| match m.kind {
                    ir::ModKind::Intensity {
                        source,
                        of: o,
                        depth,
                    } if o == of => {
                        let input = self.unit(zone, source, note, age)?;
                        let value = 1.0 - (1.0 - input) * depth;
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
                .product()
        })
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

    /// Frames the zone's start moves for a voice started after the key was
    /// held `held` seconds, within its start range either way.
    pub(crate) fn start_offset(&self, zone: &Zone, note: &Note, held: f32) -> i64 {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return 0;
        };
        let x: f32 = g
            .mods
            .iter()
            .filter(|m| !m.bypass)
            .filter_map(|m| match m.kind {
                ir::ModKind::SampleStart { source, depth } => {
                    let x = match source {
                        ir::Source::HeldTime { span_ms } => kontra_plan::held_time(span_ms, held),
                        source => self.source(zone, source, note, Age::START)?,
                    };
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

    /// The group's envelope for a voice started after the key was held
    /// `held` seconds, its attack and release scaled by their routes.
    /// ponytail: release routes read at note-on too; a source moving
    /// before note-off is missed, read at release if a library needs it.
    pub(crate) fn envelope(&self, zone: &Zone, note: &Note, held: f32) -> Ahdsr {
        let Some(g) = self.groups.get(zone.group as usize) else {
            return Ahdsr::GATE;
        };
        let mut e = g.envelope;
        let rate = self.rate as f32;
        for m in g.mods.iter().filter(|m| !m.bypass) {
            let (source, intensity, floor_ms, frames) = match m.kind {
                ir::ModKind::Attack {
                    source,
                    intensity,
                    floor_ms,
                } => (source, intensity, floor_ms, &mut e.attack),
                ir::ModKind::Release {
                    source,
                    intensity,
                    floor_ms,
                } => (source, intensity, floor_ms, &mut e.release),
                _ => continue,
            };
            let x = match source {
                ir::Source::HeldTime { span_ms } => Some(kontra_plan::held_time(span_ms, held)),
                // Outside the signed families the law compiler (FUN_140873040)
                // reads a bipolar source without `frange_bi`: 0..=1.
                source => self.unit(zone, source, note, Age::START),
            };
            let Some(x) = x else {
                continue;
            };
            let x = if intensity < 0.0 { 1.0 - x } else { x };
            let multiplier = 1.0 - (1.0 - x.clamp(0.0, 1.0)) * intensity.abs();
            let ms = *frames as f32 * 1000.0 / rate;
            let ms = if floor_ms > 0.0 {
                floor_ms * ((ms + floor_ms) / floor_ms).powf(multiplier * multiplier * multiplier)
                    - floor_ms
            } else {
                ms * multiplier
            };
            *frames = (ms.max(0.0) * rate / 1000.0).round() as u32;
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

    /// The release-trigger counter's gain for a voice started after the key
    /// was held `held` seconds.
    pub(crate) fn release_counter(&self, zone: &Zone, held: f32) -> f32 {
        self.groups.get(zone.group as usize).map_or(1.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .map(|m| match m.kind {
                    ir::ModKind::Volume {
                        source: ir::Source::HeldTime { span_ms: ms },
                        intensity,
                        curve,
                        ..
                    } => curve.gain(intensity, kontra_plan::held_time(ms, held)),
                    _ => 1.0,
                })
                .product()
        })
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
                    | ir::ModKind::Resonance { source, .. }
                    | ir::ModKind::EqGain { source, .. }
                    | ir::ModKind::EffectParam { source, .. }
                    | ir::ModKind::Mix { source, .. }
                    | ir::ModKind::Intensity { source, .. } => g.reads(source, cc),
                    ir::ModKind::Envelope
                    | ir::ModKind::SampleStart { .. }
                    | ir::ModKind::Attack { .. }
                    | ir::ModKind::Release { .. }
                    | ir::ModKind::LfoPhase { .. }
                    | ir::ModKind::LfoRate { .. }
                    | ir::ModKind::LfoHz { .. }
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
