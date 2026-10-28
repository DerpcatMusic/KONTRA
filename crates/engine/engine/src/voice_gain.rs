//! A voice's level and pitch step from its zone, its note and the group's
//! modulation; re-read when a controller or expression moves.

use kontra_ir as ir;

use crate::ledger::Note;
use kontra_voice::{Ahdsr, Gain};

use crate::effects::{self, product};
use crate::modulators::{self, Age};
use crate::{Control, Engine, Level, Zone};

impl Engine {
    pub(crate) fn voice_gain(&self, zone: &Zone, note: &Note, factor: f32, age: Age) -> Gain {
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
        let mods: f32 = self.groups.get(zone.group as usize).map_or(1.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .map(|m| match m.kind {
                    ir::ModKind::Volume {
                        source,
                        intensity,
                        curve,
                    } => {
                        // HeldTime is fixed at the voice's start: see `release_counter`.
                        let x = self.source(zone, source, note, age);
                        let x = match source {
                            ir::Source::Lfo(_) | ir::Source::PitchBend => {
                                x.map(|x| (x + 1.0) * 0.5)
                            }
                            _ => x,
                        };
                        let intensity = intensity * self.scale(zone, source, note, age);
                        x.map_or(1.0, |x| curve.gain(intensity, x))
                    }
                    _ => 1.0,
                })
                .product()
        });
        let pan: f32 = self.groups.get(zone.group as usize).map_or(0.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .filter_map(|m| match m.kind {
                    ir::ModKind::Pan { source, amount } => {
                        Some(self.routed(zone, source, note, age)? * amount)
                    }
                    _ => None,
                })
                .sum()
        });
        let [l, r] = gain(zone, note, self.level(zone), pan);
        let g = crossfade * mods * factor;
        let group = self
            .groups
            .get(zone.group as usize)
            .map_or(effects::IDENTITY, |g| effects::rack(&g.inserts));
        let amp = [[l * g, 0.0], [0.0, r * g]];
        product(group, amp)
    }

    /// The group's and the instrument's level, combined.
    fn level(&self, zone: &Zone) -> Level {
        let group = self
            .groups
            .get(zone.group as usize)
            .map_or(Level::UNITY, |g| g.level);
        Level {
            gain: group.gain * self.instrument.gain,
            pan: group.pan + self.instrument.pan,
            tune: group.tune + self.instrument.tune,
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
            ir::Source::Envelope(n) => Some(modulators::envelope(
                group()?.envelopes.get(usize::from(n))?,
                age,
            )),
            ir::Source::Lfo(n) => {
                let g = group()?;
                let mut lfo = *g.lfos.get(usize::from(n))?;
                for m in g.mods.iter().filter(|m| !m.bypass) {
                    if let ir::ModKind::LfoPhase { source, lfo: l, depth } = m.kind
                        && l == n
                        // An LFO source here would recurse.
                        && !matches!(source, ir::Source::Lfo(_))
                    {
                        let x = self.source(zone, source, note, Age::START).unwrap_or(0.0);
                        lfo.phase += x * depth;
                    }
                }
                Some(modulators::lfo(&lfo, age, u32::from(note.key), self.tempo))
            }
            ir::Source::Velocity => Some(note.velocity as f32),
            ir::Source::Controller(cc) => Some(f32::from(self.behaviour.cc(channel, cc)) / 127.0),
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
                        let x = self.source(zone, source, note, age)?;
                        let x = match source {
                            ir::Source::Lfo(_) | ir::Source::PitchBend => (x + 1.0) * 0.5,
                            _ => x,
                        };
                        Some(1.0 - (1.0 - x) * depth)
                    }
                    _ => None,
                })
                .product()
        })
    }

    pub(crate) fn step(&self, zone: &Zone, note: &Note, age: Age) -> f64 {
        // ponytail: pitch moves at once; Kontakt lags pitch routes (250 ms
        // default). Add a per-voice step glide when a library's bend steps audibly.
        let mods: f32 = self.groups.get(zone.group as usize).map_or(0.0, |g| {
            g.mods
                .iter()
                .filter(|m| !m.bypass)
                .filter_map(|m| match m.kind {
                    ir::ModKind::Pitch {
                        source,
                        semitones,
                        depth,
                    } => Some(self.routed(zone, source, note, age)? * semitones * depth),
                    _ => None,
                })
                .sum()
        });
        let semitones = f64::from(mods)
            + (f64::from(note.key) - f64::from(zone.root)) * zone.keytrack
            + zone.tune
            + self.level(zone).tune
            + note.tune
            + note.expression_tune
            + f64::from(note.script_tune) / 100_000.0;
        2f64.powf(semitones / 12.0) * f64::from(zone.sample_rate) / f64::from(self.rate)
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
                *s += x * depth;
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
                        ir::Source::HeldTime { span_ms } => held_time(span_ms, held),
                        source => self.source(zone, source, note, Age::START)?,
                    };
                    Some(x * depth)
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
                ir::Source::HeldTime { span_ms } => Some(held_time(span_ms, held)),
                // Outside the signed families the law compiler (FUN_140873040)
                // reads a bipolar source without `frange_bi`: 0..=1.
                ir::Source::PitchBend | ir::Source::Lfo(_) => self
                    .source(zone, source, note, Age::START)
                    .map(|x| (x + 1.0) * 0.5),
                source => self.source(zone, source, note, Age::START),
            };
            let Some(x) = x else {
                continue;
            };
            let x = if intensity < 0.0 { 1.0 - x } else { x };
            let m = 1.0 - (1.0 - x.clamp(0.0, 1.0)) * intensity.abs();
            let ms = *frames as f32 * 1000.0 / rate;
            let ms = if floor_ms > 0.0 {
                floor_ms * ((ms + floor_ms) / floor_ms).powf(m * m * m) - floor_ms
            } else {
                ms * m
            };
            *frames = (ms.max(0.0) * rate / 1000.0).round() as u32;
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
                    } => curve.gain(intensity, held_time(ms, held)),
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
                    | ir::ModKind::Intensity { source, .. } => source == ir::Source::Controller(cc),
                    ir::ModKind::Envelope
                    | ir::ModKind::SampleStart { .. }
                    | ir::ModKind::Attack { .. }
                    | ir::ModKind::Release { .. }
                    | ir::ModKind::LfoPhase { .. }
                    | ir::ModKind::LoopShift { .. } => false,
                })
            })
    }

    /// Whether moving `cc` changes this zone's pitch.
    pub(crate) fn pitch_follows(&self, zone: &Zone, cc: u8) -> bool {
        self.groups.get(zone.group as usize).is_some_and(|g| {
            g.mods.iter().any(|m| {
                matches!(m.kind, ir::ModKind::Pitch { source: ir::Source::Controller(c), .. }
                    | ir::ModKind::Intensity { source: ir::Source::Controller(c), .. } if c == cc)
            })
        })
    }
}

// ponytail: balance pan law only; equal-power and Kontakt's raised cosine
// when a library's measured stereo image needs them.
fn gain(zone: &Zone, note: &Note, level: Level, modulated: f32) -> [f32; 2] {
    let script = 10f32.powf(note.script_volume as f32 / 20_000.0);
    let g = zone.gain * level.gain * note.expression_gain * script;
    let pan =
        (zone.pan + level.pan + modulated + note.expression_pan + note.script_pan as f32 / 1000.0)
            .clamp(-1.0, 1.0);
    [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)]
}

/// 1 at the key's press, falling to 0 once it has been held `span_ms`.
fn held_time(span_ms: u32, held: f32) -> f32 {
    if span_ms == 0 {
        return 1.0;
    }
    let t = span_ms as f32;
    ((t - held * 1000.0) / t).max(0.0)
}
