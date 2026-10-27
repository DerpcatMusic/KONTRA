//! A voice's level and pitch step from its zone, its note and the group's
//! modulation; re-read when a controller or expression moves.

use kontra_ir as ir;

use crate::ledger::Note;
use crate::{Control, Engine, Zone};

impl Engine {
    pub(crate) fn voice_gain(&self, zone: &Zone, note: &Note, factor: f32) -> [f32; 2] {
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
                        let x = match source {
                            ir::Source::Velocity => note.velocity as f32,
                            ir::Source::Controller(cc) => {
                                f32::from(self.behaviour.cc(channel, cc)) / 127.0
                            }
                            // Fixed at the voice's start: see `release_counter`.
                            ir::Source::HeldTime { .. } => return 1.0,
                        };
                        curve.gain(intensity, x)
                    }
                    ir::ModKind::Envelope => 1.0,
                })
                .product()
        });
        let [l, r] = gain(zone, note);
        let g = crossfade * mods * factor;
        [l * g, r * g]
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

pub(crate) fn step(zone: &Zone, note: &Note, rate: u32) -> f64 {
    let semitones = (f64::from(note.key) - f64::from(zone.root)) * zone.keytrack
        + zone.tune
        + note.tune
        + note.expression_tune
        + f64::from(note.script_tune) / 100_000.0;
    2f64.powf(semitones / 12.0) * f64::from(zone.sample_rate) / f64::from(rate)
}

// ponytail: balance pan law only; equal-power and Kontakt's raised cosine
// when a library's measured stereo image needs them.
fn gain(zone: &Zone, note: &Note) -> [f32; 2] {
    let script = 10f32.powf(note.script_volume as f32 / 20_000.0);
    let g = zone.gain * note.expression_gain * script;
    let pan = (zone.pan + note.expression_pan + note.script_pan as f32 / 1000.0).clamp(-1.0, 1.0);
    [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)]
}
