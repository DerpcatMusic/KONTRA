//! The immutable instrument the engine plays, built off the audio thread by a
//! format translator (02-ENGINE.md §6). Sampler behaviour is data here
//! (13-FRESH-CORE.md §2.1): scripts are optional, not where round robin lives.

use crate::ScriptPlan;
use std::ops::RangeInclusive;

use kontra_ir as ir;
use kontra_voice::{Ahdsr, Region};

/// When a zone sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trigger {
    /// On note-on.
    #[default]
    Attack,
    /// On note release (key up, or the pedal lifting), from the original velocity.
    Release,
    /// On note-on with no other key down.
    First,
    /// On note-on while another key is down.
    Legato,
}

/// Which counter a round robin steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrCounter {
    /// One counter for the instrument.
    Global,
    /// One counter per key.
    Key,
    /// One counter per voice group ([`Zone::group`]).
    Group,
}

/// The zone plays on step `position` (0-based) of a cycle of `length`.
/// Every zone of one note reads the same step, so mic layers always agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundRobin {
    pub counter: RrCounter,
    pub position: u8,
    pub length: u8,
    /// Random step, never the same twice in a row; else sequential.
    pub random: bool,
}

/// What a crossfade or a range reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Velocity,
    /// A MIDI CC, live: moving it changes sounding voices.
    Cc(u8),
}

/// An equal-power crossfade over 0..=127 values of `control`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crossfade {
    pub control: Control,
    /// Silent at or below `.0`, full at or above `.1`.
    pub fade_in: Option<(u8, u8)>,
    /// Full at or below `.0`, silent at or above `.1`.
    pub fade_out: Option<(u8, u8)>,
}

impl Crossfade {
    #[must_use]
    pub fn gain(&self, value: u8) -> f32 {
        let ramp = |(lo, hi): (u8, u8)| -> f32 {
            if value <= lo {
                0.0
            } else if value >= hi {
                1.0
            } else {
                f32::from(value - lo) / f32::from(hi - lo)
            }
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        let fade_in = self.fade_in.map_or(1.0, |r| (ramp(r) * quarter).sin());
        let fade_out = self.fade_out.map_or(1.0, |r| (ramp(r) * quarter).cos());
        fade_in * fade_out
    }
}

/// What a group's voices share and a script may change while they play.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub mods: Vec<ir::Mod>,
    pub envelope: Ahdsr,
}

impl Group {
    /// Velocity 24 dB down at zero, a gate envelope.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            mods: vec![ir::Mod {
                name: String::new(),
                slot: 0,
                kind: ir::ModKind::Volume {
                    source: ir::Source::Velocity,
                    intensity: 1.0,
                    curve: ir::Curve::Decibels(24.0),
                },
            }],
            envelope: Ahdsr::GATE,
        }
    }

    /// The mod a script addresses by `slot`.
    pub fn mod_mut(&mut self, slot: i32) -> Option<&mut ir::Mod> {
        self.mods.iter_mut().find(|m| i32::from(m.slot) == slot)
    }
}

/// One playable zone, resolved: no format knowledge is left here.
#[derive(Clone)]
pub struct Zone {
    // Selection.
    pub keys: RangeInclusive<u8>,
    /// MIDI velocities 1..=127 the zone answers.
    pub velocities: RangeInclusive<u8>,
    /// Every listed CC must be inside its range at note-on.
    pub cc_ranges: Vec<(u8, RangeInclusive<u8>)>,
    /// The keyswitch (articulation) that must be active, if any.
    pub keyswitch: Option<u8>,
    pub trigger: Trigger,
    pub round_robin: Option<RoundRobin>,
    pub crossfades: Vec<Crossfade>,
    // Voice group.
    /// Voice group, for polyphony limits, choke and group round robins.
    pub group: u32,
    /// A voice of this zone fades when a voice of group `off_by` starts (hi-hat choke).
    pub off_by: Option<u32>,
    // Playback.
    /// The key at which the sample plays at its recorded pitch.
    pub root: u8,
    /// Semitones of pitch per key away from `root`: 1 tracks the keyboard, 0 is fixed.
    pub keytrack: f64,
    /// Fine tune in semitones.
    pub tune: f64,
    /// The sample's own rate, for the step against the engine rate.
    pub sample_rate: u32,
    pub region: Region,
    /// Linear gain at full velocity.
    pub gain: f32,
    /// −1 (left) ..= 1 (right).
    pub pan: f32,
    /// Release triggers: dB lost per second the key was held.
    pub release_decay_db: f32,
    /// Ignores note-off and plays to the end (drums, one-shots).
    pub one_shot: bool,
    /// Output bus (mic position) index into [`Plan::buses`].
    pub bus: usize,
}

impl Zone {
    /// A plain attack zone over the full key and velocity range on bus 0.
    #[must_use]
    pub fn new(region: Region, root: u8, sample_rate: u32) -> Self {
        Self {
            keys: 0..=127,
            velocities: 1..=127,
            cc_ranges: Vec::new(),
            keyswitch: None,
            trigger: Trigger::Attack,
            round_robin: None,
            crossfades: Vec::new(),
            group: 0,
            off_by: None,
            root,
            keytrack: 1.0,
            tune: 0.0,
            sample_rate,
            region,
            gain: 1.0,
            pan: 0.0,
            release_decay_db: 0.0,
            one_shot: false,
            bus: 0,
        }
    }
}

/// A mixer channel: one mic position, or one output of the instrument.
#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub name: String,
    pub gain: f32,
    /// A disabled bus starts no voices (purged mic): it costs no CPU.
    pub enabled: bool,
}

/// Pool sizes, zones, groups and buses. Growing a pool is a new plan, not a resize.
#[derive(Clone)]
pub struct Plan {
    pub zones: Vec<Zone>,
    /// Indexed by [`Zone::group`].
    pub groups: Vec<Group>,
    pub buses: Vec<Bus>,
    /// Voice limits per group: (group, polyphony). The oldest voice in the group fades.
    pub polyphony: Vec<(u32, u16)>,
    /// Keys that switch articulation instead of sounding.
    pub keyswitches: Option<RangeInclusive<u8>>,
    pub default_keyswitch: Option<u8>,
    pub voices: usize,
    pub notes: usize,
    /// Pending timed work; also bounds one block's input.
    pub queue: usize,
    /// The instrument script, if any.
    pub script: Option<ScriptPlan>,
}

impl Plan {
    /// One bus, no behaviour beyond the zones.
    #[must_use]
    pub fn new(zones: Vec<Zone>, voices: usize) -> Self {
        let groups = zones
            .iter()
            .map(|z| z.group as usize + 1)
            .max()
            .unwrap_or(0);
        Self {
            groups: vec![Group::standard(); groups],
            zones,
            buses: vec![Bus {
                name: "Main".into(),
                gain: 1.0,
                enabled: true,
            }],
            polyphony: Vec::new(),
            keyswitches: None,
            default_keyswitch: None,
            voices,
            notes: 128,
            queue: 1024,
            script: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_velocity_crossfade_is_equal_power() {
        let soft = Crossfade {
            control: Control::Velocity,
            fade_in: None,
            fade_out: Some((40, 80)),
        };
        let loud = Crossfade {
            control: Control::Velocity,
            fade_in: Some((40, 80)),
            fade_out: None,
        };
        for v in [0u8, 40, 50, 60, 70, 80, 127] {
            let power = soft.gain(v).powi(2) + loud.gain(v).powi(2);
            assert!((power - 1.0).abs() < 1e-6, "velocity {v}: {power}");
        }
        assert_eq!(soft.gain(30), 1.0);
        assert_eq!(loud.gain(100), 1.0);
    }
}
