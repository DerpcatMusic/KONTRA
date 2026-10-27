//! A voice's level and pitch step from its zone, its note and the group's
//! modulation; re-read when a controller or expression moves.

use kontra_ir as ir;

use crate::ledger::Note;
use kontra_voice::Gain;

use crate::effects::{self, product};
use crate::{Control, Engine, Level, Zone};

impl Engine {
    pub(crate) fn voice_gain(&self, zone: &Zone, note: &Note, factor: f32) -> Gain {
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
                .map(|m| match m.kind {
                    ir::ModKind::Volume {
                        source,
                        intensity,
                        curve,
                    } => {
                        // HeldTime is fixed at the voice's start: see `release_counter`.
                        self.source(source, note)
                            .map_or(1.0, |x| curve.gain(intensity, x))
                    }
                    ir::ModKind::Envelope | ir::ModKind::Pitch { .. } => 1.0,
                })
                .product()
        });
        let [l, r] = gain(zone, note, self.level(zone));
        let g = crossfade * mods * factor;
        let group = self
            .groups
            .get(zone.group as usize)
            .map_or(effects::IDENTITY, |g| effects::rack(&g.inserts));
        let amp = [[l * g, 0.0], [0.0, r * g]];
        let bus = self.buses.get(zone.bus).map_or(effects::IDENTITY, |b| {
            let pan = self.plan.buses.get(zone.bus).map_or(0.0, |p| p.pan);
            product(effects::balance(pan), effects::rack(&b.inserts))
        });
        let voice = product(bus, product(group, amp));
        product(effects::rack(&self.inserts), voice)
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
    fn source(&self, source: ir::Source, note: &Note) -> Option<f32> {
        let channel = note.host.channel;
        match source {
            ir::Source::Velocity => Some(note.velocity as f32),
            ir::Source::Controller(cc) => Some(f32::from(self.behaviour.cc(channel, cc)) / 127.0),
            ir::Source::PitchBend => Some(self.behaviour.bend(channel)),
            ir::Source::HeldTime { .. } => None,
        }
    }

    pub(crate) fn step(&self, zone: &Zone, note: &Note) -> f64 {
        // ponytail: pitch moves at once; Kontakt lags pitch routes (250 ms
        // default). Add a per-voice step glide when a library's bend steps audibly.
        let mods: f32 = self.groups.get(zone.group as usize).map_or(0.0, |g| {
            g.mods
                .iter()
                .filter_map(|m| match m.kind {
                    ir::ModKind::Pitch { source, semitones } => {
                        Some(self.source(source, note)? * semitones)
                    }
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

    /// The release-trigger counter's gain for a voice started after the key
    /// was held `held` seconds.
    pub(crate) fn release_counter(&self, zone: &Zone, held: f32) -> f32 {
        self.groups.get(zone.group as usize).map_or(1.0, |g| {
            g.mods
                .iter()
                .map(|m| match m.kind {
                    ir::ModKind::Volume {
                        source: ir::Source::HeldTime { span_ms: ms },
                        intensity,
                        curve,
                    } if ms > 0 => {
                        let t = ms as f32;
                        curve.gain(intensity, ((t - held * 1000.0) / t).max(0.0))
                    }
                    _ => 1.0,
                })
                .product()
        })
    }

    /// Whether moving `cc` changes this zone's level.
    pub(crate) fn follows(&self, zone: &Zone, cc: u8) -> bool {
        zone.crossfades.iter().any(|x| x.control == Control::Cc(cc))
            || self.groups.get(zone.group as usize).is_some_and(|g| {
                g.mods.iter().any(|m| {
                    matches!(m.kind, ir::ModKind::Volume { source: ir::Source::Controller(c), .. } if c == cc)
                })
            })
    }
}

// ponytail: balance pan law only; equal-power and Kontakt's raised cosine
// when a library's measured stereo image needs them.
fn gain(zone: &Zone, note: &Note, level: Level) -> [f32; 2] {
    let script = 10f32.powf(note.script_volume as f32 / 20_000.0);
    let g = zone.gain * level.gain * note.expression_gain * script;
    let pan = (zone.pan + level.pan + note.expression_pan + note.script_pan as f32 / 1000.0)
        .clamp(-1.0, 1.0);
    [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)]
}
