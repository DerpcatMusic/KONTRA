//! MIDI before a part's scripts: articulations picked by channel or by
//! velocity, remapped keyswitches, and MPE.
//!
//! A library switches articulations by keyswitch (or by a click on its
//! list). To play several at once without pressing them, each note-on is
//! given the articulation its channel or velocity maps to; when that is not
//! the one the scripts last switched to, its keyswitch (a note-on and
//! note-off, or the list row's control) goes in first, at the same sample.
//! The scripts switch, then play the note with it. A chord of mixed
//! articulations switches before each of its notes, in order. Channel mode
//! plays every channel on the part's own, so notes from several channels on
//! one key share it: it is released when the last of them is.
//!
//! MPE: notes on a zone's member channels take their channel's pitch bend
//! (48 semitones by default) as their own tuning, their channel pressure as
//! polyphonic aftertouch (`on poly_at`, `%POLY_AT`) and CC74 on their
//! channel; host note expressions (CLAP tuning, pressure, volume, pan,
//! brightness) do the same by key. Pressure turns into the note's volume and
//! brightness into the part's tone when the instrument has no use for them.
//!
//! Configuration lives in the host state ([`Articulate`], [`Mpe`] in each
//! part); the audio thread gets it as a fixed-size [`Route`] and keeps its
//! own [`Router`] per rack slot.

use crate::engine::{Engine, Expression, PartControls, Rack};
#[cfg(feature = "plugin")]
use moose::core::{
    EventBody,
    custom_state::{StateCursor, StateField},
    midi,
};
use serde::{Deserialize, Serialize};

/// Articulations a [`Route`] holds; lists longer than this route the rest as keyswitches.
pub const MAX_ARTICULATIONS: usize = 64;
const NONE: u8 = u8::MAX;
/// A remap to no key: the articulation's keyswitch no longer switches.
pub const CLEARED: u8 = u8::MAX;
/// Velocity of an injected keyswitch.
const SWITCH_VELOCITY: u8 = 100;

/// An articulation as an instrument's panel names it: name, keyswitch, and
/// the script slot and control that pick it.
pub type Found = (String, Option<u8>, Option<(u16, u16)>);

/// How a part's notes pick their articulation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Mode {
    /// The player's own keyswitches, as the library ships.
    #[default]
    Keyswitch,
    /// Each articulation listens on its own MIDI channel.
    Channel,
    /// The velocity range is split across the articulations.
    Velocity,
}

/// One row of an instrument's articulation list and how it is played.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Articulation {
    pub name: String,
    /// The script's keyswitch for it.
    pub key: Option<u8>,
    /// Script slot and control that pick it when it has no keyswitch.
    pub control: Option<(u16, u16)>,
    /// Channel mode: the MIDI channel (0..=15) that plays it.
    pub channel: u8,
    /// Velocity mode: the velocities that play it.
    pub low: u8,
    pub high: u8,
    /// Takes part in channel and velocity mode.
    pub enabled: bool,
    /// The key the player uses for it instead of `key`; [`CLEARED`]: none.
    pub remap: Option<u8>,
}

impl Default for Articulation {
    fn default() -> Self {
        Self {
            name: String::new(),
            key: None,
            control: None,
            channel: 0,
            low: 1,
            high: 127,
            enabled: true,
            remap: None,
        }
    }
}

/// A part's articulation setup, saved with the host state.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Articulate {
    /// The instrument the list was read from; another instrument's is ignored.
    pub source: String,
    pub mode: Mode,
    pub articulations: Vec<Articulation>,
    /// Velocity mode plays each note at this velocity; 0 rescales the note's
    /// place in its articulation's range to the whole 1..=127.
    pub fixed_velocity: u8,
    /// A remapped keyswitch's original key keeps switching too.
    pub keep_original: bool,
}

impl Default for Articulate {
    fn default() -> Self {
        Self {
            source: String::new(),
            mode: Mode::Keyswitch,
            articulations: Vec::new(),
            fixed_velocity: 0,
            keep_original: true,
        }
    }
}

impl Articulate {
    /// Take `found` (name, keyswitch, control) as `source`'s list, keeping
    /// what the player set for rows of the same name. Channels default to
    /// the row's place, velocity ranges to an even split. True when changed.
    pub fn sync(&mut self, source: &str, found: &[Found]) -> bool {
        let same = self.source == source
            && self.articulations.len() == found.len()
            && (self.articulations.iter().zip(found))
                .all(|(a, (name, key, control))| (&a.name, a.key, a.control) == (name, *key, *control));
        if same {
            return false;
        }
        let old = if self.source == source { std::mem::take(&mut self.articulations) } else { Vec::new() };
        self.source = source.to_owned();
        self.articulations = (found.iter().enumerate())
            .map(|(n, (name, key, control))| {
                let kept = old.iter().find(|a| &a.name == name);
                Articulation {
                    name: name.clone(),
                    key: *key,
                    control: *control,
                    ..kept.cloned().unwrap_or(Articulation {
                        channel: (n % 16) as u8,
                        ..Articulation::default()
                    })
                }
            })
            .collect();
        if old.is_empty() {
            self.split_velocities();
        }
        true
    }

    /// Spread 1..=127 evenly over the enabled articulations, soft to hard.
    pub fn split_velocities(&mut self) {
        let count = self.articulations.iter().filter(|a| a.enabled).count().max(1);
        for (n, a) in self.articulations.iter_mut().filter(|a| a.enabled).enumerate() {
            a.low = (1 + n * 127 / count) as u8;
            a.high = ((n + 1) * 127 / count) as u8;
        }
    }
}

/// MPE setup, saved with the host state.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Mpe {
    pub zone: Zone,
    /// Member channels, 1..=15.
    pub members: u8,
    /// Member channels' pitch bend range in semitones.
    pub bend_range: u8,
}

impl Default for Mpe {
    fn default() -> Self {
        Self {
            zone: Zone::Off,
            members: 15,
            bend_range: 48,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Zone {
    #[default]
    Off,
    /// Master channel 1, members 2 up.
    Lower,
    /// Master channel 16, members 15 down.
    Upper,
}

/// Kept in the host state as JSON, so fields added later load as their defaults.
#[cfg(feature = "plugin")]
macro_rules! json_state {
    ($($t:ty),*) => {$(
        impl StateField for $t {
            fn write_field(&self, buf: &mut Vec<u8>) {
                serde_json::to_string(self).unwrap_or_default().write_field(buf);
            }
            fn read_field(cursor: &mut StateCursor) -> Option<Self> {
                Some(serde_json::from_str(&String::read_field(cursor)?).unwrap_or_default())
            }
        }
    )*};
}
#[cfg(feature = "plugin")]
json_state!(Articulate, Mpe);

/// One articulation as the audio thread routes it.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Art {
    key: u8,
    slot: u16,
    control: u16,
    channel: u8,
    low: u8,
    high: u8,
    enabled: bool,
}

/// A part's [`Articulate`] and [`Mpe`] in fixed-size form for the audio thread.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Route {
    mode: Mode,
    count: usize,
    arts: [Art; MAX_ARTICULATIONS],
    fixed_velocity: u8,
    /// Input key to the key the scripts get; [`NONE`] drops it.
    keys: [u8; 128],
    mpe: Mpe,
}

impl Default for Route {
    fn default() -> Self {
        Self::new("", &Articulate::default(), &Mpe::default())
    }
}

impl Route {
    /// The route for the instrument at `path`; a list read from another
    /// instrument routes nothing.
    pub fn new(path: &str, a: &Articulate, mpe: &Mpe) -> Self {
        let none = Art {
            key: NONE,
            slot: 0,
            control: u16::MAX,
            channel: 0,
            low: 1,
            high: 127,
            enabled: false,
        };
        let mut route = Self {
            mode: a.mode,
            count: 0,
            arts: [none; MAX_ARTICULATIONS],
            fixed_velocity: a.fixed_velocity.min(127),
            keys: std::array::from_fn(|n| n as u8),
            mpe: Mpe {
                members: mpe.members.clamp(1, 15),
                bend_range: mpe.bend_range.clamp(1, 96),
                ..*mpe
            },
        };
        if a.source != path || path.is_empty() {
            route.mode = Mode::Keyswitch;
            return route;
        }
        for (art, a) in route.arts.iter_mut().zip(&a.articulations) {
            let (slot, control) = a.control.unwrap_or((0, u16::MAX));
            *art = Art {
                key: a.key.filter(|&k| k < 128).unwrap_or(NONE),
                slot,
                control,
                channel: a.channel.min(15),
                low: a.low.clamp(1, 127),
                high: a.high.clamp(1, 127),
                enabled: a.enabled,
            };
        }
        route.count = a.articulations.len().min(MAX_ARTICULATIONS);
        // Originals first, so a remap onto another original key wins.
        for a in a.articulations.iter().filter(|r| !a.keep_original || r.remap == Some(CLEARED)) {
            if let (Some(key), Some(to)) = (a.key, a.remap)
                && key < 128
                && to != key
            {
                route.keys[key as usize] = NONE;
            }
        }
        for a in &a.articulations {
            if let (Some(key), Some(to)) = (a.key, a.remap)
                && key < 128
                && to < 128
            {
                route.keys[to as usize] = key;
            }
        }
        route
    }

    /// The zone's master channel and member channels, when MPE is on.
    fn zone(&self) -> Option<(u8, std::ops::RangeInclusive<u8>)> {
        let m = self.mpe.members;
        match self.mpe.zone {
            Zone::Off => None,
            Zone::Lower => Some((0, 1..=m)),
            Zone::Upper => Some((15, 15 - m..=14)),
        }
    }

    /// Channel mode, unless the authored MPE route owns the channels.
    fn by_channel(&self) -> bool {
        self.mode == Mode::Channel && self.mpe.zone == Zone::Off
    }

    fn arts(&self) -> &[Art] {
        &self.arts[..self.count]
    }
}

/// What reaches a part.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum In {
    /// Exact owner, velocity and initial tuning in semitones.
    HostOn(crate::engine::HostNote, crate::midi::Velocity, f32),
    HostOff(crate::engine::HostPattern),
    HostOffVelocity(crate::engine::HostPattern, crate::midi::Velocity),
    HostChoke(crate::engine::HostPattern),
    HostExpression(crate::engine::HostPattern, crate::engine::HostExpression),
    /// Native exact poly pressure retains its authored callback before acceptance.
    HostPressure(crate::engine::HostPattern, crate::midi::ControlValue),
    NoteOn(u8, u8, u8),
    Midi1NoteOn(u8, u8, u8),
    NoteOnVelocity(u8, u8, crate::midi::Velocity),
    /// Native NoteOn attribute3 absolute pitch (Q7.9 at the boundary).
    NoteOnVelocityPitch(u8, u8, crate::midi::Velocity, f64),
    NoteOff(u8, u8),
    /// Actual MIDI1 wire releases alone consume the velocity prefix.
    Midi1NoteOff(u8, u8, u8),
    Midi1ZeroNoteOff(u8, u8),
    NoteOffVelocity(u8, u8, crate::midi::Velocity),
    Cc(u8, u8, u8),
    Midi1Cc(u8, u8, u8),
    MidiControl(u8, crate::midi::Control),
    Parameter(u8, crate::midi::NativeParameter),
    PerNoteControl(u8, u8, crate::midi::NoteControl),
    PerNotePressure(u8, u8, crate::midi::ControlValue),
    PerNoteManagement { channel: u8, note: u8, detach: bool, reset: bool },
    /// 0..=16383, centre 8192.
    Bend(u8, u16),
    Pressure(u8, u8),
    PolyAt(u8, u8, u8),
    Midi1PolyAt(u8, u8, u8),
    /// Synthetic compatibility expressions: semitones, MIDI1 pressure,
    /// linear gain, pan (−1..=1), normalized brightness.
    NoteTune(u8, u8, f32),
    NotePressure(u8, u8, u8),
    NoteGain(u8, u8, f32),
    NotePan(u8, u8, f32),
    NoteBrightness(u8, u8, crate::midi::ControlValue),
}

impl In {
    /// Native wire input. Per-note values and normalized velocities retain
    /// their precision; explicit host expressions use the exact-owner lane.
    #[cfg(feature = "plugin")]
    pub fn from_event(body: &EventBody) -> Option<Self> {
        use crate::midi::{Bend, Control, ControlValue, NativeParameter, NativeParameterData, NoteControl, Velocity};
        Some(match *body {
            EventBody::NoteOn2 { channel, note, velocity, attribute_type: 3, attribute, .. }
                if channel < 16 && note < 128 =>
                Self::NoteOnVelocityPitch(channel, note, Velocity::from_midi2(velocity), f64::from(attribute) / 512.),
            EventBody::NoteOn2 { channel, note, velocity, .. }
                if channel < 16 && note < 128 =>
                Self::NoteOnVelocity(channel, note, Velocity::from_midi2(velocity)),
            EventBody::NoteOff2 { channel, note, velocity, .. }
                if channel < 16 && note < 128 =>
                Self::NoteOffVelocity(channel, note, Velocity::from_midi2(velocity)),
            EventBody::NoteOn2 { .. } | EventBody::NoteOff2 { .. } => return None,
            // UMP v1.1.2 §7.4.6 recommends ignoring MIDI1 bank/parameter
            // compounds in native MIDI2 CC; unified parameter messages carry
            // their own address and full-width value below.
            EventBody::ControlChange2 { channel, cc, value, .. }
                if channel < 16 && cc < 128 && !crate::midi::ignored_midi2_cc(cc) =>
                Self::MidiControl(channel, Control::Cc(cc, ControlValue::from_midi2(value))),
            EventBody::ChannelPressure2 { channel, pressure, .. } if channel < 16 =>
                Self::MidiControl(channel, Control::Pressure(ControlValue::from_midi2(pressure))),
            EventBody::PitchBend2 { channel, value, .. } if channel < 16 =>
                Self::MidiControl(channel, Control::Bend(Bend::from_midi2(value))),
            EventBody::ControlChange2 { .. } | EventBody::ChannelPressure2 { .. } | EventBody::PitchBend2 { .. } => return None,
            EventBody::RegisteredController { channel, bank, index, value, .. }
                if channel < 16 && bank < 128 && index < 128 =>
                Self::Parameter(channel, NativeParameter { registered: true, address: u16::from(bank) << 7 | u16::from(index), data: NativeParameterData::Absolute(value) }),
            EventBody::AssignableController { channel, bank, index, value, .. }
                if channel < 16 && bank < 128 && index < 128 =>
                Self::Parameter(channel, NativeParameter { registered: false, address: u16::from(bank) << 7 | u16::from(index), data: NativeParameterData::Absolute(value) }),
            EventBody::RelativeRegisteredController { channel, bank, index, delta, .. }
                if channel < 16 && bank < 128 && index < 128 =>
                Self::Parameter(channel, NativeParameter { registered: true, address: u16::from(bank) << 7 | u16::from(index), data: NativeParameterData::Relative(delta) }),
            EventBody::RelativeAssignableController { channel, bank, index, delta, .. }
                if channel < 16 && bank < 128 && index < 128 =>
                Self::Parameter(channel, NativeParameter { registered: false, address: u16::from(bank) << 7 | u16::from(index), data: NativeParameterData::Relative(delta) }),
            EventBody::RegisteredController { .. } | EventBody::AssignableController { .. }
            | EventBody::RelativeRegisteredController { .. } | EventBody::RelativeAssignableController { .. } => return None,
            EventBody::PerNotePitchBend { channel, note, value, .. } if channel < 16 && note < 128 =>
                Self::PerNoteControl(channel, note, NoteControl::Pitch(Bend::from_midi2(value))),
            EventBody::PolyPressure2 { channel, note, pressure, .. } if channel < 16 && note < 128 =>
                Self::PerNotePressure(channel, note, ControlValue::from_midi2(pressure)),
            EventBody::PerNoteCC { channel, note, cc: index, value, registered, .. }
                if channel < 16 && note < 128 =>
                Self::PerNoteControl(channel, note, if registered { NoteControl::Registered { index, value } }
                    else { NoteControl::Assignable { index, value } }),
            EventBody::PerNoteManagement { channel, note, flags, .. }
                if channel < 16 && note < 128 =>
                // UMP v1.1.2 Figure51: D is bit1, S is bit0; reserved
                // fields do not change the defined action (§2.1.3).
                Self::PerNoteManagement { channel, note, detach: flags & 2 != 0, reset: flags & 1 != 0 },
            // Native unsupported controls never fall back to channel MIDI1.
            EventBody::PerNoteCC { .. } | EventBody::PerNotePitchBend { .. }
            | EventBody::PolyPressure2 { .. } | EventBody::PerNoteManagement { .. } => return None,
            EventBody::NoteOn { channel, note, velocity: 0, .. } if channel < 16 && note < 128 => {
                Self::Midi1ZeroNoteOff(channel, note)
            }
            EventBody::NoteOn { channel, note, velocity, .. } if channel < 16 && note < 128 && velocity < 128 => Self::Midi1NoteOn(channel, note, velocity),
            EventBody::NoteOn { .. } => return None,
            EventBody::NoteOff { channel, note, velocity, .. } if channel < 16 && note < 128 && velocity <= 127 => {
                Self::Midi1NoteOff(channel, note, velocity)
            }
            EventBody::NoteOff { .. } => return None,
            EventBody::ControlChange { channel, cc, value, .. } if channel < 16 && cc < 128 && value < 128 => Self::Midi1Cc(channel, cc, value),
            EventBody::ControlChange { .. } => return None,
            EventBody::PitchBend { channel, value, .. } if channel < 16 && value < 16384 => Self::Bend(channel, value),
            EventBody::PitchBend { .. } => return None,
            EventBody::ChannelPressure { channel, pressure, .. } if channel < 16 && pressure < 128 => Self::Pressure(channel, pressure),
            EventBody::ChannelPressure { .. } => return None,
            EventBody::Aftertouch { channel, note, pressure, .. } if channel < 16 && note < 128 && pressure < 128 => {
                Self::Midi1PolyAt(channel, note, pressure)
            }
            EventBody::Aftertouch { .. } => return None,
            _ => return Self::from_event(&midi::downconvert_to_midi1(body)?),
        })
    }

    pub(crate) fn channel(self) -> u8 {
        match self {
            Self::HostOn(note, ..) => note.channel,
            Self::PerNoteManagement { channel, .. } => channel,
            Self::HostOff(p) | Self::HostOffVelocity(p, _) | Self::HostChoke(p) | Self::HostExpression(p, _) | Self::HostPressure(p, _) => p.channel.max(0).min(15) as u8,
            Self::NoteOn(c, ..)
            | Self::Midi1NoteOn(c, ..)
            | Self::Midi1Cc(c, ..)
            | Self::NoteOnVelocity(c, ..)
            | Self::NoteOnVelocityPitch(c, ..)
            | Self::Parameter(c, _)
            | Self::PerNoteControl(c, ..)
            | Self::PerNotePressure(c, ..)
            | Self::Midi1PolyAt(c, ..)
            | Self::NoteOffVelocity(c, ..)
            | Self::NoteOff(c, _)
            | Self::Midi1NoteOff(c, ..)
            | Self::Midi1ZeroNoteOff(c, _)
            | Self::MidiControl(c, _)
            | Self::Cc(c, ..)
            | Self::Bend(c, _)
            | Self::Pressure(c, _)
            | Self::PolyAt(c, ..)
            | Self::NoteTune(c, ..)
            | Self::NotePressure(c, ..)
            | Self::NoteGain(c, ..)
            | Self::NotePan(c, ..)
            | Self::NoteBrightness(c, ..) => c & 15,
        }
    }
}

/// What a [`Router`] sends the part's engine.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Out {
    /// Actual wire attack, distinct from generated articulation keyswitches.
    InputNoteOn { physical: u8, raw_key: u8, logical: u8, key: u8, velocity: crate::midi::Velocity },
    /// The delivered original host input; generated articulation keyswitches
    /// remain ordinary notes, even when their pitch equals this input.
    HostNoteOn(crate::engine::HostNote, u8, u8, crate::midi::Velocity, f32),
    NoteOn(u8, u8, u8),
    NoteOnVelocity(u8, u8, crate::midi::Velocity),
    NoteOff(u8, u8),
    NoteOffVelocity(u8, u8, crate::midi::Velocity),
    /// Script channel, physical input channel, key, velocity.
    NoteOnFrom(u8, u8, u8, u8),
    NoteOnVelocityFrom(u8, u8, u8, crate::midi::Velocity),
    NoteOffFrom(u8, u8, u8),
    NoteOffVelocityFrom(u8, u8, u8, crate::midi::Velocity),
    Cc(u8, u8, u8),
    CcFrom(u8, u8, u8, u8),
    MidiControl(u8, crate::midi::Control),
    MidiControlFrom(u8, u8, crate::midi::Control),
    SoundOffFrom(u8, u16),
    BendFrom(u8, u8, u16),
    PressureFrom(u8, u8, u8),
    PolyAtFrom(u8, u8, u8, u8),
    /// Compatibility callback only; canonical channel pressure owns the sound.
    MpePolyAt(u8, u8, u8, crate::midi::ControlValue),
    Bend(u8, u16),
    Pressure(u8, u8),
    PolyAt(u8, u8, u8),
    /// A script control set (script slot, control, value), as a click on it.
    Control(usize, usize, i32),
    Expression(u8, u8, Expression),
    FreezeExpression(u8, u8),
}

/// One rack slot's pre-script MIDI layer on the audio thread.
pub struct Router {
    pub route: Route,
    current: Option<usize>,
    held: [[(u8, u8); 128]; 16],
    held_from: [u128; 16],
    member_pressure: [Option<crate::midi::ControlValue>; 16],
    /// Read-only projection of the engine's effective negotiated zones.
    mpe: crate::midi::MpeZones,
    /// Authored configuration already installed in the engine.
    configured_mpe: Option<(Mpe, bool)>,
    expression: Box<[[Expression; 128]; 16]>,
    brightness: [u8; 128],
    scripted: bool,
    handles_pressure: bool,
    script: usize,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            route: Route::default(),
            current: None,
            held: [[(NONE, NONE); 128]; 16],
            held_from: [0; 16],
            member_pressure: [None; 16],
            mpe: crate::midi::MpeZones::default(),
            configured_mpe: None,
            expression: Box::new([[Expression::default(); 128]; 16]),
            brightness: [NONE; 128],
            scripted: false,
            handles_pressure: false,
            script: 0,
        }
    }
}

fn pressure_gain(pressure: u8) -> f32 {
    10f32.powf((f32::from(pressure.min(127)) / 127.0 - 1.0) * 12.0 / 20.0)
}

impl Router {
    pub(crate) fn mpe_enabled(&self) -> bool {
        self.route.mpe.zone != Zone::Off
    }
    pub(crate) fn external_input_supported(&self) -> bool {
        self.route.mode == Mode::Keyswitch && self.route.count == 0
            && self.route.keys.iter().enumerate().all(|(key, &to)| usize::from(to) == key)
    }
    /// Take a new route; held notes retain their captured destination.
    pub fn set_route(&mut self, route: Route) {
        if route != self.route {
            if route.mpe != self.route.mpe || route.by_channel() != self.route.by_channel() {
                self.configured_mpe = None;
                let mut mpe = crate::midi::MpeZones::default();
                if !route.by_channel() && let Some((manager, _)) = route.zone() {
                    mpe.configure(manager, route.mpe.members, route.mpe.bend_range);
                }
                self.mpe = mpe;
            }
            self.route = route;
            self.forget();
        }
    }

    /// Each channel plays its own articulation.
    /// Project the same static setup as real ingress, without installing it.
    pub(crate) fn source_topology(&self, current: crate::midi::MpeZones) -> Option<crate::midi::MpeZones> {
        if self.route.count != 0 || self.route.by_channel()
            || !self.route.keys.iter().enumerate().all(|(key, &mapped)| key == usize::from(mapped)) {
            return None;
        }
        if self.configured_mpe == Some((self.route.mpe, self.route.by_channel())) {
            return Some(current);
        }
        let mut projected = crate::midi::MpeZones::default();
        if let Some((manager, members, range)) = self.mpe_setup() {
            projected.configure(manager, members, range)?;
        }
        Some(projected)
    }

    fn mpe_setup(&self) -> Option<(u8, u8, u8)> {
        if self.route.by_channel() { None } else { self.route.zone().map(|(manager, _)|
            (manager, self.route.mpe.members, self.route.mpe.bend_range)) }
    }

    pub fn by_channel(&self) -> bool {
        self.route.mode == Mode::Channel && self.mpe.zones().iter().all(|zone| zone.members == 0)
    }

    /// Physical input channels stopped by a channel-mode message.
    pub(crate) fn stop_channels(&self, channel: u8) -> u16 {
        (1u16 << (channel & 15)) | self.mpe.manager(channel).map_or(0, |zone| zone.member_mask())
    }

    /// The scripts' articulation may have changed some other way (a click):
    /// switch again before the next note.
    pub fn forget(&mut self) {
        self.current = None;
    }

    pub(crate) fn reset_midi(&mut self) {
        self.forget();
        self.configured_mpe = None;
        self.held.fill([(NONE, NONE); 128]);
        self.held_from.fill(0);
        self.member_pressure.fill(None);
        self.expression.fill([Expression::default(); 128]);
        self.brightness.fill(NONE);
    }

    /// Whether the part takes `channel` on `port`: its own channel, a zone's,
    /// or one an articulation listens on.
    pub fn hears(&self, c: &PartControls, port: u8, channel: u8) -> bool {
        let r = &self.route;
        c.port == port
            && (c.channel < 0
                || c.channel == i16::from(channel)
                || self.mpe.zone(channel).is_some()
                || r.by_channel() && r.arts().iter().any(|a| a.enabled && a.channel == channel))
    }

    /// The part's tone as a share of its setting: brightness below the
    /// middle closes it, when the instrument has no use for brightness.
    pub fn cutoff_scale(&self) -> f32 {
        let b = self.brightness.iter().filter(|&&b| b != NONE).max().copied();
        match b {
            Some(b) if b < 64 && !self.scripted => 2f32.powf((f32::from(b) - 64.0) / 64.0 * 6.6),
            _ => 1.0,
        }
    }

    /// The articulation a note on `channel` at `velocity` plays, if the mode picks one.
    fn pick(&self, channel: u8, velocity: u8) -> Option<usize> {
        let r = &self.route;
        let wanted = |a: &Art| match r.mode {
            _ if !a.enabled || a.key == NONE && a.control == u16::MAX => false,
            Mode::Channel => r.by_channel() && a.channel == channel,
            Mode::Velocity => (a.low..=a.high).contains(&velocity),
            Mode::Keyswitch => false,
        };
        r.arts().iter().position(wanted)
    }

    /// The articulation a note-on would play, as far as the route knows,
    /// and whether it is a keyswitch rather than a note.
    pub fn articulation_of(&self, channel: u8, note: u8, velocity: u8) -> (Option<usize>, bool) {
        let key = self.route.keys[note as usize & 127];
        if key == NONE {
            return (None, false);
        }
        match self.route.arts().iter().position(|a| a.key == key) {
            Some(ks) => (Some(ks), true),
            None => (self.pick(channel & 15, velocity), false),
        }
    }

    /// The articulation script control `control` of script slot `slot` picks.
    pub fn articulation_of_control(&self, slot: usize, control: usize) -> Option<usize> {
        (self.route.arts()).iter().position(|a| a.control != u16::MAX && usize::from(a.slot) == slot && usize::from(a.control) == control)
    }

    /// Switch the scripts to articulation `to` before a note on `channel`,
    /// unless they are there already.
    pub fn select(&mut self, to: usize, channel: u8, e: &mut Engine) {
        self.follow(e);
        if to < self.route.count {
            self.switch(to, channel & 15, &mut |o| apply(e, o));
        }
    }

    /// Take in what `e`'s scripts are; replaced ones are switched again.
    fn follow(&mut self, e: &Engine) {
        let rt = e.script();
        let script = rt.map_or(0, |rt| std::ptr::from_ref(rt) as usize);
        if script != self.script {
            self.script = script;
            self.forget();
        }
        self.scripted = rt.is_some();
        self.handles_pressure = rt.is_some_and(|rt| rt.handles_poly_at());
    }

    fn switch(&mut self, to: usize, channel: u8, out: &mut impl FnMut(Out)) {
        if self.current == Some(to) {
            return;
        }
        let a = self.route.arts[to];
        if a.key != NONE {
            out(Out::NoteOn(channel, a.key, SWITCH_VELOCITY));
            out(Out::NoteOff(channel, a.key));
        } else {
            out(Out::Control(usize::from(a.slot), usize::from(a.control), 1));
        }
        self.current = Some(to);
    }

    fn set_expression(&mut self, channel: u8, key: u8, f: impl FnOnce(&mut Expression), out: &mut impl FnMut(Out)) {
        let x = &mut self.expression[channel as usize & 15][key as usize & 127];
        let was = *x;
        f(x);
        if *x != was {
            out(Out::Expression(channel, key, *x));
        }
    }

    /// The channel and key the held attack received, or the current route.
    fn note_route(&self, channel: u8, note: u8, to: u8) -> (u8, u8) {
        match self.held[channel as usize & 15][note as usize & 127] {
            (_, NONE) => (to, self.route.keys[note as usize & 127]),
            held => held,
        }
    }

    fn pressure(&mut self, channel: u8, key: u8, value: u8, out: &mut impl FnMut(Out)) {
        out(Out::PolyAt(channel, key, value));
        if !self.handles_pressure {
            self.set_expression(channel, key, |x| x.gain = pressure_gain(value), out);
        }
    }

    /// Route one input to the part's engine: `home` is the part's channel,
    /// where channel mode plays everything.
    pub fn input(&mut self, ev: In, home: u8, output: &mut impl FnMut(Out)) {
        self.input_impl(ev, home, false, None, output);
    }

    fn input_impl(&mut self, ev: In, home: u8, bind_input: bool, precise_onset: Option<crate::midi::Velocity>, output: &mut impl FnMut(Out)) {
        use crate::midi::Velocity;
        let canonical = if let In::MidiControl(_, control) = ev { Some(control) } else { None };
        let ev = match ev {
            In::Midi1Cc(c, cc, raw) => In::Cc(c, cc, raw),
            In::Midi1PolyAt(c, n, raw) => In::PolyAt(c, n, raw),
            In::MidiControl(c, crate::midi::Control::Cc(cc, value)) => In::Cc(c, cc, value.midi1()),
            In::MidiControl(c, crate::midi::Control::Cc14 { cc, raw, .. }) => In::Cc(c, cc, raw),
            In::MidiControl(c, crate::midi::Control::Bend(value)) => In::Bend(c, value.midi1()),
            In::MidiControl(c, crate::midi::Control::Pressure(value)) => In::Pressure(c, value.midi1()),
            In::MidiControl(c, crate::midi::Control::ResetPressure) => In::Pressure(c, 0),
            ev => ev,
        };
        let (ev, host_note, mut onset_velocity, release_velocity) = match ev {
            In::Midi1NoteOn(c, n, v) => (In::NoteOn(c, n, v), None, precise_onset, None),
            In::Midi1NoteOff(c, n, v) => (In::NoteOff(c, n), None, None, Some(Velocity::from_midi1(v))),
            In::Midi1ZeroNoteOff(c, n) => (In::NoteOff(c, n), None, None, Some(Velocity::from_midi1(64))),
            In::HostOn(note, velocity, tune) => (In::NoteOn(note.channel, note.key, velocity.midi1()), Some((note,tune)), Some(velocity), None),
            In::NoteOnVelocity(c, n, v) | In::NoteOnVelocityPitch(c, n, v, _) => (In::NoteOn(c, n, v.midi1().max(1)), None, Some(v), None),
            In::NoteOffVelocity(c, n, v) => (In::NoteOff(c, n), None, None, Some(v)),
            ev => (ev, None, None, None),
        };
        let r = self.route;
        let by_channel = self.by_channel();
        let mpe = self.mpe;
        let channel = ev.channel();
        let to = if by_channel { home & 15 } else { channel };
        let out = &mut |o| {
            let o = match (canonical, o) {
                (Some(control), Out::Cc(c, ..) | Out::Bend(c, _) | Out::Pressure(c, _)) => Out::MidiControl(c, control),
                (_, o) => o,
            };
            output(if by_channel {
            match o {
                Out::NoteOn(c, n, v) => Out::NoteOnFrom(c, channel, n, v),
                Out::NoteOff(c, n) => Out::NoteOffFrom(c, channel, n),
                Out::NoteOnVelocity(c, n, v) => Out::NoteOnVelocityFrom(c, channel, n, v),
                Out::NoteOffVelocity(c, n, v) => Out::NoteOffVelocityFrom(c, channel, n, v),
                Out::MidiControl(c, control) => Out::MidiControlFrom(c, channel, control),
                Out::Cc(c, cc, v) => Out::CcFrom(c, channel, cc, v),
                Out::Bend(c, v) => Out::BendFrom(c, channel, v),
                Out::Pressure(c, v) => Out::PressureFrom(c, channel, v),
                Out::PolyAt(c, n, v) => Out::PolyAtFrom(c, channel, n, v),
                o => o,
            }
        } else { o });
        };
        match ev {
            // HostOn is normalized above with its identity retained for final
            // delivery; the other exact events resolve engine-owned tuples.
            In::HostOn(..) | In::HostOff(..) | In::HostOffVelocity(..) | In::HostChoke(..) | In::HostExpression(..) | In::HostPressure(..)
            | In::NoteOnVelocity(..) | In::NoteOnVelocityPitch(..) | In::NoteOffVelocity(..) | In::Midi1NoteOn(..) | In::Midi1Cc(..) | In::Midi1PolyAt(..) | In::Midi1NoteOff(..) | In::Midi1ZeroNoteOff(..) | In::MidiControl(..)
            | In::Parameter(..) | In::PerNoteControl(..) | In::PerNotePressure(..) | In::PerNoteManagement { .. } => {},
            In::NoteOn(_, note, velocity) => {
                let key = r.keys[note as usize & 127];
                if key == NONE {
                    return;
                }
                let mut velocity = velocity;
                if let Some(ks) = r.arts().iter().position(|a| a.key == key) {
                    self.current = Some(ks);
                } else if let Some(a) = self.pick(channel, velocity) {
                    if r.mode == Mode::Velocity {
                        let art = r.arts[a];
                        velocity = match r.fixed_velocity {
                            0 if art.high > art.low => {
                                let span = u16::from(art.high - art.low);
                                (1 + u16::from(velocity.saturating_sub(art.low)) * 126 / span) as u8
                            }
                            0 => velocity,
                            fixed => fixed,
                        };
                        // Authored articulation rescaling deliberately writes a
                        // new MIDI1 velocity; normal routing retains host precision.
                        if onset_velocity.is_some() { onset_velocity = Some(Velocity::from_midi1(velocity)); }
                    }
                    self.switch(a, to, out);
                }
                // A new note on a key starts from its channel's expression.
                if mpe.member(channel).is_some() {
                    out(Out::FreezeExpression(to, key));
                }
                self.brightness[key as usize] = NONE;
                self.set_expression(to, key, |x| *x = Expression::default(), out);
                self.held[channel as usize][note as usize & 127] = (to, key);
                let bit = 1u128 << (note & 127);
                if by_channel { self.held_from[channel as usize] |= bit; }
                else { self.held_from[channel as usize] &= !bit; }
                if let Some((note,tune)) = host_note {
                    out(Out::HostNoteOn(note, to, key, onset_velocity.unwrap_or_else(|| Velocity::from_midi1(velocity)), tune));
                } else if bind_input {
                    out(Out::InputNoteOn { physical: channel, raw_key: note, logical: to, key,
                        velocity: onset_velocity.unwrap_or_else(|| Velocity::from_midi1(velocity)) });
                } else if let Some(velocity) = onset_velocity {
                    if by_channel { out(Out::NoteOnVelocityFrom(to, channel, key, velocity)); }
                    else { out(Out::NoteOnVelocity(to, key, velocity)); }
                } else if by_channel {
                    out(Out::NoteOnFrom(to, channel, key, velocity));
                } else {
                    out(Out::NoteOn(to, key, velocity));
                }
                if mpe.member(channel).is_some() && let Some(value) = self.member_pressure[channel as usize] {
                    out(Out::MpePolyAt(to, channel, key, value));
                }
            }
            In::NoteOff(_, note) => {
                let bit = 1u128 << (note & 127);
                let from = self.held_from[channel as usize] & bit != 0;
                self.held_from[channel as usize] &= !bit;
                let (to, key) = match std::mem::replace(&mut self.held[channel as usize][note as usize & 127], (NONE, NONE)) {
                    (_, NONE) => (to, r.keys[note as usize & 127]),
                    held => held,
                };
                if key == NONE {
                    return;
                }
                if from || by_channel {
                    if let Some(velocity) = release_velocity { out(Out::NoteOffVelocityFrom(to, channel, key, velocity)); }
                    else { out(Out::NoteOffFrom(to, channel, key)); }
                    return;
                }
                let shared = (0..16).any(|c| c != channel as usize && self.held[c][note as usize & 127] == (to, key));
                if !shared {
                    if let Some(velocity) = release_velocity { out(Out::NoteOffVelocity(to, key, velocity)); }
                    else { out(Out::NoteOff(to, key)); }
                }
            }
            In::Bend(_, value) => {
                out(Out::Bend(to, value));
            }
            In::Pressure(_, value) => {
                let precise = match canonical {
                    Some(crate::midi::Control::Pressure(value)) => value,
                    _ => crate::midi::ControlValue::from_midi1(value),
                };
                self.member_pressure[channel as usize] = Some(precise);
                if mpe.member(channel).is_some() {
                    for (logical,key) in self.held[channel as usize].into_iter().filter(|held| held.1 != NONE) {
                        out(Out::MpePolyAt(logical, channel, key, precise));
                    }
                }
                out(Out::Pressure(to, value));
            }
            In::PolyAt(_, note, value) => {
                let (to, key) = self.note_route(channel, note, to);
                if key != NONE { out(Out::PolyAt(to, key, value)); }
            }
            In::Cc(_, 121, value) => {
                out(Out::Cc(to, 121, value));
                let channels = self.stop_channels(channel);
                for c in (0..16u8).filter(|&c| channels & (1 << c) != 0) {
                    self.member_pressure[c as usize] = None;
                }
            }
            In::Cc(_, cc @ (120 | 123..=127), value) => {
                let channels = self.stop_channels(channel);
                // Sound-off retains physical routing until key-up; a remap
                // made meanwhile must not change where its release goes.
                for owner in (0..16).filter(|owner| cc != 120 && channels & (1 << owner) != 0) {
                    let row = std::mem::replace(&mut self.held[owner], [(NONE, NONE); 128]);
                    let from = std::mem::take(&mut self.held_from[owner]);
                    // A mode change does not change who owns existing notes.
                    for (note, (to, key)) in row.into_iter().enumerate() {
                        if key != NONE && (by_channel || from & (1u128 << note) != 0) {
                            out(Out::NoteOffFrom(to, owner as u8, key));
                        }
                    }
                }
                if cc == 120 {
                    out(Out::SoundOffFrom(to, channels));
                } else {
                    // Retain the received mode number; its implicit release
                    // does not enable unsupported Omni/Mono mode switching.
                    out(Out::Cc(to, cc, value));
                }
            }
            In::Cc(_, cc, value) => {
                if cc == 74 && mpe.member(channel).is_some() {
                    let row = self.held[channel as usize];
                    for (_, key) in row.into_iter().filter(|h| h.1 != NONE) {
                        self.brightness[key as usize] = value;
                    }
                }
                out(Out::Cc(to, cc, value));
            }
            In::NoteTune(_, note, semitones) => {
                let (to, key) = self.note_route(channel, note, to);
                if key != NONE {
                    self.set_expression(to, key, |x| x.tune = semitones, out);
                }
            }
            In::NotePressure(_, note, value) => {
                let (to, key) = self.note_route(channel, note, to);
                if key != NONE {
                    self.pressure(to, key, value, out);
                }
            }
            In::NoteGain(_, note, gain) => {
                let (to, key) = self.note_route(channel, note, to);
                if key != NONE {
                    self.set_expression(to, key, |x| x.gain = gain.clamp(0.0, 4.0), out);
                }
            }
            In::NotePan(_, note, pan) => {
                let (to, key) = self.note_route(channel, note, to);
                if key != NONE {
                    self.set_expression(to, key, |x| x.pan = pan.clamp(-1.0, 1.0), out);
                }
            }
            In::NoteBrightness(_, note, value) => {
                // Retain the note-on route, including its logical channel, even
                // when articulation mapping changes while the key is held.
                let (to, key) = self.note_route(channel, note, to);
                if key != NONE {
                    self.set_expression(to, key, |x| x.note_cc74 = Some(value), out);
                }
            }
        }
    }

    /// Follow RPN 0 (pitch bend range) on member channels and the MPE
    /// configuration message (RPN 6) on the master channel.
    /// Reconcile routing caches only after Engine reconciles musical owners.
    fn sync_mpe(&mut self, mpe: crate::midi::MpeZones, _out: &mut impl FnMut(Out)) {
        let previous = self.mpe;
        self.mpe = mpe;
        for channel in 0..16u8 {
            let old = previous.zone(channel);
            let new = mpe.zone(channel);
            if old.map(|zone| (zone.manager, zone.manager == channel))
                != new.map(|zone| (zone.manager, zone.manager == channel))
            {
                self.member_pressure[channel as usize] = None;
                self.held[channel as usize].fill((NONE, NONE));
                self.held_from[channel as usize] = 0;
                // Expression is indexed by the delivered route, not this
                // physical input. Each new note resets its own destination.
            }
        }
    }

}

/// Apply what a router sends to its engine.
pub(crate) fn apply(e: &mut Engine, o: Out) {
    match o {
        Out::InputNoteOn { physical, logical, key, velocity, .. } => e.note_on_from_velocity(logical, physical, key, velocity),
        Out::HostNoteOn(note, c, n, v, tune) => { e.host_note_on_velocity(note, c, n, v, tune); }
        Out::NoteOn(c, n, v) => e.note_on(c, n, v),
        Out::NoteOff(c, n) => e.note_off(c, n),
        Out::NoteOnFrom(c, owner, n, v) => e.note_on_from(c, owner, n, v),
        Out::NoteOffFrom(c, owner, n) => e.note_off_from(c, owner, n),
        Out::NoteOnVelocity(c, n, v) => e.note_on_velocity(c, n, v),
        Out::NoteOffVelocity(c, n, v) => e.note_off_velocity(c, n, v),
        Out::NoteOnVelocityFrom(c, owner, n, v) => e.note_on_from_velocity(c, owner, n, v),
        Out::NoteOffVelocityFrom(c, owner, n, v) => e.note_off_from_velocity(c, owner, n, v),
        Out::MidiControl(c, control) => e.control_from(c, c, control),
        Out::MidiControlFrom(c, input, control) => e.control_from(c, input, control),
        Out::Cc(c, cc, v) => e.cc(c, cc, v),
        Out::CcFrom(c, input, cc, v) => e.cc_from(c, input, cc, v),
        Out::SoundOffFrom(c, mask) => e.all_sound_off_from(c, mask),
        Out::BendFrom(c, input, v) => e.pitch_bend_from(c, input, v),
        Out::PressureFrom(c, input, v) => e.channel_pressure_from(c, input, v),
        Out::PolyAtFrom(c, input, n, v) => e.poly_pressure_from(c, input, n, v),
        Out::MpePolyAt(c, input, n, v) => e.mpe_poly_pressure_from(c, input, n, v),
        Out::Bend(c, v) => e.pitch_bend(c, v),
        Out::Pressure(c, v) => e.channel_pressure(c, v),
        Out::PolyAt(c, n, v) => e.poly_pressure(c, n, v),
        Out::Control(slot, control, value) => e.ui_control(slot, control, value),
        Out::Expression(c, n, x) => e.set_expression_on(c, n, x),
        Out::FreezeExpression(c, n) => e.freeze_released_expression(c, n),
    }
}

/// Send `ev` through `r` to its part's engine `e`; notes in channel mode
/// arrive on `home`.
#[cfg(test)]
pub(crate) fn feed(r: &mut Router, e: &mut Engine, ev: In, home: u8) {
    feed_on_port(r,e,ev,home,0);
}

pub(crate) fn feed_on_port(r: &mut Router, e: &mut Engine, ev: In, home: u8, port: u8) {
    // Empty slots still receive setup/controller input, but cannot own a note.
    if matches!(ev, In::HostOn(..)) && e.bank().is_none() && e.script().is_none() { return; }
    // Wire protocol decoding happens once. Native/exact velocities never
    // consume a MIDI1 prefix, and selectors retain the original attack byte.
    let precise_onset = if let In::Midi1NoteOn(channel, _, raw) = ev { Some(e.midi1_velocity(channel, raw)) } else { None };
    let ev = match ev {
        In::Midi1NoteOff(channel, key, raw) => In::NoteOffVelocity(channel, key, e.midi1_release_velocity(channel, raw)),
        In::Midi1ZeroNoteOff(channel, key) => {
            e.midi1_velocity(channel, 0);
            In::NoteOffVelocity(channel, key, crate::midi::Velocity::from_midi1(64))
        },
        ev => ev,
    };
    let address = match ev {
        In::Midi1NoteOn(channel, key, _) => Some(crate::engine::InputAddress { port, channel, key }),
        In::NoteOnVelocity(channel, key, _) | In::NoteOnVelocityPitch(channel, key, ..) => Some(crate::engine::InputAddress { port, channel, key }),
        _ => None,
    };
    if let Some(address) = address && !e.admit_input_note(address) { return; }
    if let In::HostOn(note, ..) = ev && !e.admit_host_note(note) { return; }
    r.follow(e);
    let configured = (r.route.mpe, r.route.by_channel());
    if r.configured_mpe != Some(configured) {
        e.configure_mpe(r.mpe_setup());
        r.configured_mpe = Some(configured);
    }
    let (ev, mut received) = match ev {
        In::Midi1Cc(input, cc, raw) => {
            let Some(received) = e.receive_midi1_cc(input, cc, raw) else { return; };
            (In::MidiControl(input, received.control), Some(received))
        },
        In::Cc(input, cc, raw) => (ev, e.receive_controller(input, cc, raw)),
        // Canonical native MIDI2 controls do not participate in the MIDI1
        // compound decoder, including ordinary authored CC96/97 delivery.
        In::MidiControl(_, control @ (crate::midi::Control::Cc(..) | crate::midi::Control::Cc14 { .. })) =>
            (ev, Some(crate::midi::ReceivedControl::without_parameter(control))),
        ev => (ev, None),
    };
    r.sync_mpe(e.mpe_zones(), &mut |output| apply(e, output));
    // Channel modes can be consumed by ownership handling rather than ordinary
    // controller callbacks; their raw library receiver slot still changes first.
    let raw_mode = match ev {
        In::Cc(input, cc @ 120..=127, raw) => Some((input,
            crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(raw)))),
        In::MidiControl(input, control @ crate::midi::Control::Cc(120..=127, _)) => Some((input, control)),
        _ => None,
    };
    if let Some((input, control)) = raw_mode {
        e.receive_library_control(if r.by_channel() { home & 15 } else { input }, control);
    }
    // Disabled native stop messages remain controls: do not remove captured
    // note routes or emit physical key-ups before the instrument sees them.
    let disabled_stop = match ev {
        In::Cc(input, cc @ (120 | 123), raw) => Some((input,
            crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(raw)))),
        In::MidiControl(input, control @ crate::midi::Control::Cc(120 | 123, _)) => Some((input, control)),
        _ => None,
    };
    if let Some((input, control)) = disabled_stop
        && !e.native_note_action_enabled(control.legacy().0)
    {
        let logical = if r.by_channel() { home & 15 } else { input };
        e.control_from_received_on_port(logical, input,
            received.take().unwrap_or_else(|| crate::midi::ReceivedControl::without_parameter(control)), port);
        return;
    }
    match ev {
        In::HostOff(pattern) | In::HostOffVelocity(pattern, _) | In::HostChoke(pattern) => {
            match ev {
                In::HostChoke(_) => e.host_note_choke(pattern),
                In::HostOffVelocity(_, velocity) => e.host_note_off_velocity(pattern, velocity),
                _ => e.host_note_off(pattern),
            }
            let mut index = 0;
            while let Some((note,_)) = e.host_note_at(index) {
                let (channel,key)=r.held[note.channel as usize][note.key as usize];
                if pattern.matches(note) && !e.host_key_held(note.channel,note.key)
                    && !e.input_key_down_from(note.channel,channel,key) {
                    r.held[note.channel as usize][note.key as usize] = (NONE,NONE);
                    r.held_from[note.channel as usize] &= !(1u128 << note.key);
                }
                index += 1;
            }
        },
        In::Parameter(input, value) => {
            let logical = if r.by_channel() { home & 15 } else { input };
            e.native_parameter_from(logical, input, value);
        },
        In::PerNoteControl(channel, raw_key, value) => {
            let to = if r.by_channel() { home & 15 } else { channel };
            let (logical, key) = r.note_route(channel, raw_key, to);
            // A filtered key may still carry pre-On protocol state for a later
            // route. Callback note remains a valid MIDI key in that case.
            e.note_controller_from(logical, if key == NONE { raw_key } else { key },
                crate::engine::InputAddress { port, channel, key: raw_key }, value);
        },
        In::PerNoteManagement { channel, note: key, detach, reset } => {
            e.per_note_manage(crate::engine::InputAddress { port, channel, key }, detach, reset);
        },
        In::PerNotePressure(channel, raw_key, value) => {
            let to = if r.by_channel() { home & 15 } else { channel };
            let (logical, key) = r.note_route(channel, raw_key, to);
            e.per_note_pressure_from(logical, if key == NONE { raw_key } else { key },
                crate::engine::InputAddress { port, channel, key: raw_key }, value);
        },
        In::Midi1PolyAt(channel, raw_key, value) => {
            let to = if r.by_channel() { home & 15 } else { channel };
            let (logical, key) = r.note_route(channel, raw_key, to);
            e.per_note_pressure_from(logical, if key == NONE { raw_key } else { key },
                crate::engine::InputAddress { port, channel, key: raw_key }, crate::midi::ControlValue::from_midi1(value));
        },
        In::HostExpression(pattern, expression) => e.host_expression(pattern, expression),
        In::HostPressure(pattern, pressure) => e.host_pressure(pattern, pressure),
        ev => r.input_impl(ev, home, matches!(ev, In::Midi1NoteOn(..) | In::NoteOnVelocity(..) | In::NoteOnVelocityPitch(..)), precise_onset, &mut |output| match output {
            Out::InputNoteOn { physical, raw_key, logical, key, velocity } => {
                let pitch = if let In::NoteOnVelocityPitch(_, _, _, pitch) = ev { Some(pitch) } else { None };
                e.note_on_input_velocity_pitch(crate::engine::InputAddress { port, channel: physical, key: raw_key }, logical, key, velocity, pitch);
            },
            Out::MidiControl(channel, control) => match control {
                crate::midi::Control::Cc(..) | crate::midi::Control::Cc14 { .. } => e.control_from_received_on_port(channel, channel,
                    received.take().unwrap_or_else(|| crate::midi::ReceivedControl::without_parameter(control)), port),
                _ => e.control_from(channel, channel, control),
            },
            Out::MidiControlFrom(channel, input, control) => match control {
                crate::midi::Control::Cc(..) | crate::midi::Control::Cc14 { .. } => e.control_from_received_on_port(channel, input,
                    received.take().unwrap_or_else(|| crate::midi::ReceivedControl::without_parameter(control)), port),
                _ => e.control_from(channel, input, control),
            },
            Out::Cc(channel, cc, value) => e.control_from_received_on_port(channel, channel,
                received.take().unwrap_or_else(|| crate::midi::ReceivedControl::without_parameter(
                    crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(value)))), port),
            Out::CcFrom(channel, input, cc, value) => e.control_from_received_on_port(channel, input,
                received.take().unwrap_or_else(|| crate::midi::ReceivedControl::without_parameter(
                    crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(value)))), port),
            output => apply(e, output),
        }),
    }
    if let In::NoteOff(input,key) | In::NoteOffVelocity(input,key,_)=ev
        && let Some(route)=e.held_host_route(input,key) {
        r.held[input as usize][key as usize]=route;
        if input!=route.0 { r.held_from[input as usize] |= 1u128<<key; }
    }

}

/// Whether a part receives host input. Releases, bend and controllers other
/// than volume and pan reach the whole port so routing changes never stick notes.
pub fn reaches(c: &PartControls, r: &Router, port: u8, ev: In) -> bool {
    // Old owners may outlive a part's routing change. Their immutable tuple,
    // rather than the current channel/port selector, resolves these events.
    if matches!(ev, In::HostOff(..) | In::HostOffVelocity(..) | In::HostChoke(..) | In::HostExpression(..) | In::HostPressure(..)) { return true; }
    let wide = match ev {
        In::NoteOff(..) | In::NoteOffVelocity(..) | In::Midi1NoteOff(..) | In::Midi1ZeroNoteOff(..) | In::Bend(..)
        | In::Parameter(..) | In::PerNoteControl(..) | In::PerNoteManagement { .. }
        | In::PerNotePressure(..) | In::Midi1PolyAt(..) => true,
        In::MidiControl(_, crate::midi::Control::Bend(_)) => true,
        In::MidiControl(_, crate::midi::Control::Cc(cc, _)) => !matches!(cc, 7 | 10),
        In::MidiControl(_, crate::midi::Control::Cc14 { coarse, .. }) => !matches!(coarse, 7 | 10),
        In::Cc(_, cc, _) => !matches!(cc, 7 | 10),
        In::Midi1Cc(_, cc, _) => !matches!(cc, 7 | 10 | 39 | 42),
        _ => false,
    };
    if wide { c.port == port } else { r.hears(c, port, ev.channel()) }
}

/// A bank keeps raw saved-channel delivery and never lends its selected engine.
pub(crate) fn feed_part(r: &mut Router, part: &mut crate::engine::RackPart, ev: In, home: u8, port: u8) {
    if part.is_instrument_bank() {
        if r.external_input_supported() && !r.mpe_enabled() {
            part.bank_input(port, ev);
        } else if let Some(bank) = part.program_bank_mut() {
            bank.reject_input();
        }
    } else if let Some(engine) = part.instrument_mut() {
        feed_on_port(r, engine, ev, home, port);
    }
}

/// Send host input through every part's router that accepts it.
pub fn dispatch(rack: &mut Rack, routers: &mut [Router], port: u8, ev: In) {
    dispatch_with(rack, routers, port, ev, &mut |_, _, _, _, _| false);
}

pub fn dispatch_with(rack: &mut Rack, routers: &mut [Router], port: u8, ev: In,
    external: &mut impl FnMut(usize, u8, In, &Router, bool) -> bool) {
    for slot in 0..rack.parts.len().min(rack.controls.len()).min(routers.len()) {
        let c = rack.controls[slot];
        let r = &mut routers[slot];
        let reached = if rack.parts[slot].is_instrument_bank() {
            rack.parts[slot].matches_input(port, ev.channel())
        } else { reaches(&c, r, port, ev) };
        if !external(slot, port, ev, r, reached) && reached {
            rack.with_part(slot, |e| feed_part(r, e, ev, u8::try_from(c.channel).unwrap_or(0), port));
        }
    }
}

/// Dispatch and record its targets in caller-prepared storage for later key-up.
/// Every flag is overwritten; there is no fixed-width part mask.
pub fn dispatch_record(rack: &mut Rack, routers: &mut [Router], port: u8, ev: In, reached: &mut [bool]) {
    dispatch_record_with(rack, routers, port, ev, reached, &mut |_, _, _, _, _| false);
}

pub fn dispatch_record_with(rack: &mut Rack, routers: &mut [Router], port: u8, ev: In, reached: &mut [bool],
    external: &mut impl FnMut(usize, u8, In, &Router, bool) -> bool) {
    reached.fill(false);
    for slot in 0..rack.parts.len().min(rack.controls.len()).min(routers.len()).min(reached.len()) {
        let c = rack.controls[slot];
        let r = &mut routers[slot];
        reached[slot] = if rack.parts[slot].is_instrument_bank() {
            rack.parts[slot].matches_input(port, ev.channel())
        } else { reaches(&c, r, port, ev) };
        if !external(slot, port, ev, r, reached[slot]) && reached[slot] {
            rack.with_part(slot, |e| feed_part(r, e, ev, u8::try_from(c.channel).unwrap_or(0), port));
        }
    }
}

/// Send input to previously recorded targets, even if routing has changed.
pub fn dispatch_to(rack: &mut Rack, routers: &mut [Router], slots: impl IntoIterator<Item = usize>, ev: In) {
    dispatch_to_on_port(rack,routers,slots,ev,0);
}

pub fn dispatch_to_on_port(rack: &mut Rack, routers: &mut [Router], slots: impl IntoIterator<Item = usize>, ev: In, port: u8) {
    dispatch_to_with(rack, routers, slots, port, ev, &mut |_, _, _, _, _| false);
}

pub fn dispatch_to_with(rack: &mut Rack, routers: &mut [Router], slots: impl IntoIterator<Item = usize>, port: u8, ev: In,
    external: &mut impl FnMut(usize, u8, In, &Router, bool) -> bool) {
    for slot in slots {
        if let (Some(c), Some(r)) = (rack.controls.get(slot).copied(), routers.get_mut(slot)) {
            if !external(slot, port, ev, r, true) {
                rack.with_part(slot, |e| feed_part(r, e, ev, u8::try_from(c.channel).unwrap_or(0), port));
            }
        }
    }
}

/// Send input to one rack part (the on-screen keyboard).
pub fn play(rack: &mut Rack, routers: &mut [Router], slot: usize, ev: In) {
    play_with(rack, routers, slot, 0, ev, &mut |_, _, _, _, _| false);
}

pub fn play_with(rack: &mut Rack, routers: &mut [Router], slot: usize, port: u8, ev: In,
    external: &mut impl FnMut(usize, u8, In, &Router, bool) -> bool) {
    if let Some(r) = routers.get_mut(slot) {
        if !external(slot, port, ev, r, true) {
            rack.with_part(slot, |e| feed_part(r, e, ev, ev.channel(), port));
        }
    }
}

/// "C-1", "D#3", "Eb2" (Kontakt's names, C3 = 60) as a MIDI key.
pub fn parse_note(text: &str) -> Option<u8> {
    let text = text.trim();
    let mut chars = text.chars();
    let base = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, rest) = match rest.chars().next()? {
        '#' => (1, &rest[1..]),
        'b' => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i16 = rest.parse().ok()?;
    u8::try_from((octave + 2) * 12 + base + shift).ok().filter(|&n| n < 128)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn areia() -> Articulate {
        let names = ["Sustained", "Spiccato", "Pizzicato", "Tremolo"];
        let found: Vec<_> = (names.iter().enumerate())
            .map(|(n, name)| (name.to_string(), Some(12 + n as u8), None))
            .collect();
        let mut a = Articulate::default();
        a.sync("lib.nki", &found);
        a
    }

    fn run(r: &mut Router, input: &[In]) -> Vec<Out> {
        let mut out = Vec::new();
        for &ev in input {
            r.input(ev, 0, &mut |o| out.push(o));
        }
        out.retain(|o| !matches!(o, Out::Expression(..)));
        out
    }

    fn router(a: &Articulate, mpe: &Mpe) -> Router {
        let mut r = Router::default();
        r.set_route(Route::new("lib.nki", a, mpe));
        r
    }

    #[test]
    fn sound_off_keeps_the_original_noteoff_route_after_a_remap() {
        let mut r = Router::default();
        let mut before = Route::default();
        before.keys[60] = 62;
        r.set_route(before);
        assert_eq!(run(&mut r,&[In::NoteOn(0,60,100)]),[Out::NoteOn(0,62,100)]);
        assert_eq!(run(&mut r,&[In::Cc(0,120,0)]),[Out::SoundOffFrom(0,1)]);
        let mut after = Route::default();
        after.keys[60] = 65;
        r.set_route(after);
        assert_eq!(run(&mut r,&[In::NoteOff(0,60)]),[Out::NoteOff(0,62)]);
    }

    #[test]
    fn notes_parse_as_kontakt_names_them() {
        assert_eq!(parse_note("C-1"), Some(12));
        assert_eq!(parse_note("D#0"), Some(27));
        assert_eq!(parse_note("C3"), Some(60));
        assert_eq!(parse_note("Bb-2"), Some(10));
        assert_eq!(parse_note("G8"), Some(127));
        assert_eq!(parse_note("H3"), None);
    }

    #[test]
    fn channels_switch_before_each_note_of_a_mixed_chord() {
        let mut a = areia();
        a.mode = Mode::Channel;
        let mut r = router(&a, &Mpe::default());
        // Channel 2 (spiccato) and channel 3 (pizzicato) at once, then channel 2 again.
        let out = run(&mut r, &[In::NoteOn(1, 60, 90), In::NoteOn(2, 64, 80), In::NoteOn(1, 67, 70)]);
        assert_eq!(
            out,
            [
                Out::NoteOnFrom(0, 1, 13, SWITCH_VELOCITY),
                Out::NoteOffFrom(0, 1, 13),
                Out::NoteOnFrom(0, 1, 60, 90),
                Out::NoteOnFrom(0, 2, 14, SWITCH_VELOCITY),
                Out::NoteOffFrom(0, 2, 14),
                Out::NoteOnFrom(0, 2, 64, 80),
                Out::NoteOnFrom(0, 1, 13, SWITCH_VELOCITY),
                Out::NoteOffFrom(0, 1, 13),
                Out::NoteOnFrom(0, 1, 67, 70),
            ]
        );
        // Releases go where their notes went.
        assert_eq!(run(&mut r, &[In::NoteOff(2, 64)]), [Out::NoteOffFrom(0, 2, 64)]);
        // The same articulation again needs no switch.
        assert_eq!(run(&mut r, &[In::NoteOn(1, 62, 50)]), [Out::NoteOnFrom(0, 1, 62, 50)]);
        // Something else may have switched: switch again.
        r.forget();
        assert_eq!(run(&mut r, &[In::NoteOn(1, 62, 50)])[0], Out::NoteOnFrom(0, 1, 13, SWITCH_VELOCITY));
    }

    #[test]
    fn parameter_reset_preserves_other_physical_inputs_on_the_same_home_channel() {
        use crate::ksp::{LogEngine, Runtime, Value};
        let (mut engine, mut router) = three_articulation_part();
        let controls = "on init\ndeclare ui_switch $go\nend on\non ui_control($go)\nset_rpn(9,640)\nend on";
        let receiver = "on init
            make_perfview
            declare ui_slider $two(0,100)
            declare ui_slider $five(0,100)
            declare ui_slider $authored(0,100)
            declare ui_slider $channel(0,15)
            end on
            on rpn
            wait(1000)
            if ($RPN_ADDRESS = 2)
                inc($two)
            end if
            if ($RPN_ADDRESS = 5)
                inc($five)
            end if
            if ($RPN_ADDRESS = 9)
                inc($authored)
            end if
            $channel := $MIDI_CHANNEL
            end on";
        let (runtime, errors) = Runtime::with_scripts(&[controls, receiver],
            &mut LogEngine::new(Vec::new(), 48_000.), 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        engine.set_script(Some(Box::new(runtime)));
        let mut exercise = || {
            engine.begin_audio_block(128, 1, true);
            engine.ui_control(0, 0, 1);
            for input in [2, 5] {
                for (cc, value) in [(101, 0), (100, input), (6, 2)] {
                    feed(&mut router, &mut engine, In::Cc(input, cc, value), 0);
                }
            }
            feed(&mut router, &mut engine, In::Cc(2, 121, 0), 0);
            engine.render(&mut [0.; 128], &mut [0.; 128]);
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
        #[cfg(not(feature = "plugin"))]
        exercise();
        let ui = engine.script().unwrap().interface(1);
        for (control, value) in [(0, 0), (1, 1), (2, 1), (3, 0)] {
            assert_eq!(ui.controls[control].properties["$CONTROL_PAR_VALUE"], Value::Int(value));
        }
        assert!(engine.script().unwrap().diagnostics().is_empty());
    }

    #[test]
    fn controller_reset_preserves_other_physical_inputs_on_the_same_home_channel() {
        use crate::ksp::{LogEngine, Runtime, Value};
        let (mut engine, mut router) = three_articulation_part();
        let controls = "on init\ndeclare ui_switch $go\nend on\non ui_control($go)\nset_controller(1,9)\nend on";
        let receiver = "on init
            make_perfview
            declare ui_slider $cc_two(0,100)
            declare ui_slider $cc_five(0,100)
            declare ui_slider $poly_two(0,100)
            declare ui_slider $poly_five(0,100)
            declare ui_slider $authored(0,100)
            declare ui_slider $channel(0,15)
            end on
            on controller
            if ($CC_NUM = 1)
                if (%CC[1] = 2)
                    wait(1000)
                    inc($cc_two)
                else
                    if (%CC[1] = 5)
                        wait(1000)
                        inc($cc_five)
                    else
                        if (%CC[1] = 9)
                            wait(1000)
                            inc($authored)
                        end if
                    end if
                end if
                $channel := $MIDI_CHANNEL
            end if
            end on
            on poly_at
            if ($POLY_AT_NUM = 60)
                wait(1000)
                inc($poly_two)
            else
                if ($POLY_AT_NUM = 64)
                    wait(1000)
                    inc($poly_five)
                end if
            end if
            $channel := $MIDI_CHANNEL
            end on";
        let (runtime, errors) = Runtime::with_scripts(&[controls, receiver],
            &mut LogEngine::new(Vec::new(), 48_000.), 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        engine.set_script(Some(Box::new(runtime)));
        let mut exercise = || {
            engine.begin_audio_block(128, 1, true);
            engine.ui_control(0, 0, 1);
            for (input, key) in [(2, 60), (5, 64)] {
                feed(&mut router, &mut engine, In::Cc(input, 1, input), 0);
                feed(&mut router, &mut engine, In::PolyAt(input, key, 50), 0);
            }
            feed(&mut router, &mut engine, In::Cc(2, 121, 0), 0);
            engine.render(&mut [0.; 128], &mut [0.; 128]);
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
        #[cfg(not(feature = "plugin"))]
        exercise();
        let ui = engine.script().unwrap().interface(1);
        for (control, value) in [(0, 0), (1, 1), (2, 0), (3, 1), (4, 1), (5, 0)] {
            assert_eq!(ui.controls[control].properties["$CONTROL_PAR_VALUE"], Value::Int(value),
                "control {control}; controls={:?}; diagnostics={:?}",
                ui.controls, engine.script().unwrap().diagnostics());
        }
        assert!(engine.script().unwrap().diagnostics().is_empty());
    }

    /// A script that keeps one global articulation that keyswitches C-1,
    /// C#-1 and D-1 set, as libraries do, and starts each note in that
    /// articulation's group.
    const GLOBAL_ARTICULATION: &str = "on init\ndeclare $art := 0\nend on\non note\nif ($EVENT_NOTE < 24)\n$art := $EVENT_NOTE - 12\nignore_event($EVENT_ID)\nelse\ndisallow_group($ALL_GROUPS)\nallow_group($art)\nend if\nend on";

    fn runtime(script: &str) -> crate::ksp::Runtime {
        use crate::ksp::{LogEngine, Runtime};
        let (rt, errors) = Runtime::with_scripts(&[script], &mut LogEngine::new(Vec::new(), 48000.0), 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        rt
    }

    /// A scripted part with one group per articulation, playing [`GLOBAL_ARTICULATION`].
    fn three_articulation_part() -> (Engine, Router) {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone}};
        let groups = ["Pizzicato", "Staccato", "Legato"].map(|name| Group { name: name.into(), ..Group::default() });
        let path = std::path::PathBuf::from("tone");
        let zones = (0..3)
            .map(|group| Zone { group, sample: path.clone(), low_key: 48, high_key: 72, low_velocity: 1, high_velocity: 127, ..Zone::default() })
            .collect();
        let tone = Sample { rate: 48000, frames: vec![[0.5; 2]; 48000] };
        let bank = Bank::from_samples(groups.to_vec(), zones, vec![(path, tone)]).unwrap();
        let rt = runtime(GLOBAL_ARTICULATION);
        let mut e = Engine::default();
        e.reset(48000.0);
        e.set_bank(Some(Box::new(bank)));
        e.set_script(Some(Box::new(rt)));
        let found: Vec<Found> = (groups.iter().enumerate()).map(|(n, g)| (g.name.clone(), Some(12 + n as u8), None)).collect();
        let mut a = Articulate::default();
        a.sync("lib.nki", &found);
        a.mode = Mode::Channel;
        (e, router(&a, &Mpe::default()))
    }

    fn render(e: &mut Engine) {
        let (mut l, mut r) = ([0f32; 64], [0f32; 64]);
        e.render(&mut l, &mut r);
    }

    /// The groups of the voices playing `note`, by whether they were released.
    fn voices(e: &Engine, note: u8, released: bool) -> Vec<u32> {
        let mut g: Vec<u32> = (e.voice_census().iter()).filter(|v| v.note == note && v.released == released).map(|v| v.group).collect();
        g.sort_unstable();
        g
    }

    /// Pizzicato, staccato and legato on channels 1, 2 and 3 at the same
    /// sample: each note plays its own articulation's group, and a release
    /// on one channel never ends another channel's note on the same key.
    #[test]
    fn simultaneous_notes_on_three_channels_keep_their_articulations() {
        let (mut e, mut r) = three_articulation_part();
        for (channel, note) in [(0, 60), (1, 62), (2, 64)] {
            feed(&mut r, &mut e, In::NoteOn(channel, note, 100), 0);
        }
        render(&mut e);
        assert_eq!([voices(&e, 60, false), voices(&e, 62, false), voices(&e, 64, false)], [[0], [1], [2]]);

        // The same line doubled on all three: one key, three articulations.
        let (mut e, mut r) = three_articulation_part();
        for channel in 0..3 {
            feed(&mut r, &mut e, In::NoteOn(channel, 60, 100), 0);
        }
        render(&mut e);
        assert_eq!(voices(&e, 60, false), [0, 1, 2]);
        // The short pizzicato's release cuts neither staccato nor legato.
        feed(&mut r, &mut e, In::NoteOff(0, 60), 0);
        feed(&mut r, &mut e, In::NoteOff(1, 60), 0);
        render(&mut e);
        assert_eq!(voices(&e, 60, false), [2], "each channel must release its own articulation");
        assert!(e.key_down(0, 60), "the remaining legato note keeps the shared script key held");
        // The last one lets the key go.
        feed(&mut r, &mut e, In::NoteOff(2, 60), 0);
        render(&mut e);
        assert_eq!((voices(&e, 60, false), voices(&e, 60, true)), (vec![], vec![0, 1, 2]));
        assert!(!e.key_down(0, 60));
    }

    /// A part whose scripts were replaced (an instrument or its saved
    /// values reloaded) starts at the library's own articulation: the next
    /// note switches again, even on the channel switched to last.
    #[test]
    fn replaced_scripts_are_switched_again() {
        let (mut e, mut r) = three_articulation_part();
        feed(&mut r, &mut e, In::NoteOn(2, 60, 100), 0);
        feed(&mut r, &mut e, In::NoteOff(2, 60), 0);
        e.set_script(Some(Box::new(runtime(GLOBAL_ARTICULATION))));
        feed(&mut r, &mut e, In::NoteOn(2, 62, 100), 0);
        render(&mut e);
        assert_eq!(voices(&e, 62, false), [2]);
    }

    #[test]
    fn all_notes_off_releases_only_its_physical_channel() {
        let (mut e, mut r) = three_articulation_part();
        for channel in 0..3 {
            feed(&mut r, &mut e, In::NoteOn(channel, 60, 100), 0);
        }
        render(&mut e);
        feed(&mut r, &mut e, In::Cc(1, 123, 0), 0);
        render(&mut e);
        assert_eq!(voices(&e, 60, false), [0, 2], "channel 2 stop must leave channels 1 and 3 held");
        assert!(e.key_down(0, 60));

        // The engine's home-channel stop (audition/host cleanup) releases all
        // physical owners routed to it, including repeated notes after a stop.
        feed(&mut r, &mut e, In::NoteOn(1, 60, 100), 0);
        render(&mut e);
        e.cc(0, 123, 0);
        render(&mut e);
        assert!(voices(&e, 60, false).is_empty());
        assert!(!e.key_down(0, 60));
    }

    #[test]
    fn authored_home_stop_preserves_each_inputs_sustain_and_other_homes() {
        for scripted in [false, true] {
            for cc in 123..=127 {
                let (mut e, _) = three_articulation_part();
                if !scripted { e.set_script(None); }
                e.cc_from(0, 1, 64, 127);
                e.note_on_from(0, 1, 60, 100);
                e.note_on_from(0, 2, 60, 100);
                e.note_on_from(7, 1, 62, 100);
                assert!(e.host_note_on(crate::engine::HostNote { port:0, channel:1, key:64, id:77, clap:true },0,64,100,0.));
                render(&mut e);
                assert_eq!(crate::plugin::tests::allocations(|| {
                    e.cc(0, cc, 0);
                    render(&mut e);
                }), 0);
                assert!(!e.input_key_down_from(1,0,60));
                assert!(!e.input_key_down_from(2,0,60));
                assert!(!e.host_key_held(1,64));
                assert!(!voices(&e,60,false).is_empty(), "physical input1 sustain retains its released key");
                assert!(!voices(&e,64,false).is_empty(), "the exact owner shares its physical sustain");
                assert!(e.input_key_down_from(1,7,62), "another logical home on the same input remains held");
                e.cc_from(0,1,64,0);
                render(&mut e);
                assert!(voices(&e,60,false).is_empty());
                assert!(voices(&e,64,false).is_empty());
                assert!(!voices(&e,62,false).is_empty());
            }
        }
    }

    #[test]
    fn channel_noteoffs_keep_physical_owners_after_switching_to_keys() {
        fn no_heap(f: impl FnOnce()) {
            #[cfg(feature="plugin")]
            assert_eq!(crate::plugin::tests::allocations(f),0);
            #[cfg(not(feature="plugin"))]
            f();
        }
        for controller in [false,true] {
            for order in [[1,2],[2,1]] {
                let (mut e,mut r) = three_articulation_part();
                no_heap(|| {
                    for channel in [1,2] { feed(&mut r,&mut e,In::NoteOn(channel,60,100),7); }
                    render(&mut e);
                });
                assert_eq!(e.voice_census().iter().filter(|v|!v.released).count(),2);
                // Existing notes keep their logical route and physical owner.
                r.set_route(Route::default());
                for (i,channel) in order.into_iter().enumerate() {
                    no_heap(|| {
                        feed(&mut r,&mut e,if controller { In::Cc(channel,123,0) } else { In::NoteOff(channel,60) },7);
                        render(&mut e);
                    });
                    assert!(!e.script().unwrap().key_down_from(channel,7,60),"controller={controller}, channel={channel}");
                    assert!(e.voice_census().iter().filter(|v|v.input_channel==Some(channel)).all(|v|v.released));
                    if i==0 { assert!(e.script().unwrap().key_down_from(order[1],7,60)); }
                }
                assert_eq!(e.dropped_commands(),0);
                assert!(e.script().unwrap().diagnostics().is_empty());
            }
        }
    }

    #[cfg(feature="plugin")]
    #[test]
    fn anonymous_key_up_keeps_exact_captured_route_across_mode_change() {
        use crate::engine::{HostNote,HostPattern};
        let (mut e,mut r)=three_articulation_part();
        let note=HostNote { port:0,channel:2,key:60,id:10,clap:true };
        let pattern=HostPattern { port:0,channel:2,key:60,id:10,clap:true };
        assert_eq!(crate::plugin::tests::allocations(|| {
        feed(&mut r,&mut e,In::HostOn(note,crate::midi::Velocity::from_midi1(100),0.),7);
            assert_eq!(e.held_host_route(2,60),Some((7,60)),"the articulation keyswitch must not acquire the host tuple");
            assert_eq!(e.host_note_drops(),0);
            feed(&mut r,&mut e,In::NoteOn(2,60,100),7); render(&mut e);
            assert_eq!(e.active_voices(),2);
            r.set_route(Route::default());
            feed(&mut r,&mut e,In::NoteOff(2,60),7); render(&mut e);
            assert!(e.key_down(7,60)); assert_eq!(r.held[2][60],(7,60));
            assert_ne!(r.held_from[2] & (1u128<<60),0);
            feed(&mut r,&mut e,In::HostOff(pattern),7); render(&mut e);
            assert!(!e.key_down(7,60)); assert_eq!(r.held[2][60],(NONE,NONE));
        }),0);

        // Selection and original input can have the same pitch. The output
        // role, rather than a key comparison, must distinguish their owners.
        let (mut e,mut r)=three_articulation_part();
        e.set_script(None);
        r.route.arts[2].key=60;
        assert_eq!(crate::plugin::tests::allocations(|| {
            r.switch(2,7,&mut |o| apply(&mut e,o));
            assert!(e.host_note_at(0).is_none());
        feed(&mut r,&mut e,In::HostOn(note,crate::midi::Velocity::from_midi1(100),0.),7);
            assert_eq!(e.held_host_route(2,60),Some((7,60)));
            assert_eq!(e.host_note_drops(),0);
            assert!(e.key_down(7,60));
            feed(&mut r,&mut e,In::HostOff(pattern),7);
            assert!(!e.key_down(7,60));
        }),0);
    }

    #[test]
    fn exact_choke_preserves_anonymous_key_and_route_ownership() {
        use crate::engine::{HostNote,HostPattern};
        for scripted in [false,true] {
            let (mut e,mut r)=three_articulation_part();
            if !scripted { e.set_script(None); }
            let note=HostNote { port:0,channel:2,key:60,id:10,clap:true };
            let pattern=HostPattern { port:0,channel:2,key:60,id:10,clap:true };
            let actions=|| {
        feed(&mut r,&mut e,In::HostOn(note,crate::midi::Velocity::from_midi1(100),0.),7);
                assert_eq!(e.held_host_route(2,60),Some((7,60)));
                assert_eq!(e.host_note_drops(),0);
                feed(&mut r,&mut e,In::NoteOn(2,60,100),7);
                render(&mut e);
                feed(&mut r,&mut e,In::HostChoke(pattern),7);
                assert!(e.key_down(7,60),"exact choke cleared the anonymous held key");
                assert!(e.input_key_down_from(2,7,60));
                assert_ne!(r.held[2][60],(NONE,NONE));
                assert_ne!(r.held_from[2] & (1u128<<60),0);
                r.set_route(Route::default());
                feed(&mut r,&mut e,In::NoteOff(2,60),7);
                render(&mut e);
                assert!(!e.input_key_down_from(2,7,60),"anonymous key-up lost its captured route");
                assert!(!e.key_down(7,60));
            };
            #[cfg(feature="plugin")]
            assert_eq!(crate::plugin::tests::allocations(actions),0);
            #[cfg(not(feature="plugin"))]
            {
                let mut actions = actions;
                actions();
            }
        }
    }

    #[test]
    fn sound_off_and_reset_cancel_waiting_script_notes() {
        for stop in 0..4 {
            let (mut e, _) = three_articulation_part();
            e.set_script(Some(Box::new(runtime("on init\ndeclare ui_switch $go\nend on\non note\nignore_event($EVENT_ID)\nwait(10000)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,-1)\nend on\non ui_control($go)\nset_controller(7,63)\nset_controller(64,127)\nend on"))));
            e.note_on(1, 60, 100);
            e.note_on(2, 62, 100);
            e.ui_control(0, 0, 1);
            match stop {
                0 => e.cc(1, 120, 0),
                1 => {
                    e.set_mpe_zone(Some((0, (1 << 1) | (1 << 2))));
                    e.cc(0, 120, 0);
                }
                2 => e.reset(48000.),
                _ => e.panic(),
            }
            let (mut left, mut right) = ([0.; 1024], [0.; 1024]);
            e.render(&mut left, &mut right);
            if stop != 2 {
                assert_eq!(e.cc_state()[0][7], 63, "stop={stop}: a queued UI volume edit was lost");
                assert_eq!(e.cc_state()[0][64], if stop == 3 { 0 } else { 127 }, "stop={stop}: wrong sustain reset");
            }
            assert!(voices(&e, 60, false).is_empty(), "stop={stop}: a canceled callback restarted its note");
            assert!(!e.key_down(1, 60), "stop={stop}: canceled input remained held");
            assert_eq!(voices(&e, 62, false).len(), if stop == 0 { 3 } else { 0 }, "stop={stop}: wrong channel scope");
            e.note_on(1, 64, 100);
            e.render(&mut left, &mut right);
            assert_eq!(voices(&e, 64, false).len(), 3, "stop={stop}: fresh input stopped working");
            assert!(e.script().unwrap().diagnostics().is_empty());
        }
    }

    #[test]
    fn channel_sound_off_cancels_all_generated_lifetimes_on_only_its_physical_input() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone}, ksp::{LogEngine, Runtime}};
        let script = r#"on init
declare $art := 0
declare %ids[2]
declare ui_switch $go
end on
on note
if ($EVENT_NOTE < 24)
$art := $EVENT_NOTE - 12
ignore_event($EVENT_ID)
else
disallow_group($ALL_GROUPS)
allow_group($art)
allow_group($art + 2)
if ($EVENT_NOTE = 60)
%ids[$art] := $EVENT_ID
play_note(61,100,0,1000000)
play_note(62,100,0,0)
end if
if ($EVENT_NOTE = 70)
ignore_event($EVENT_ID)
wait(40000)
play_note(70,100,0,-1)
end if
end if
end on
on release
if ($EVENT_NOTE = 64)
play_note(67,100,0,0)
end if
if ($EVENT_NOTE = 66)
wait(40000)
play_note(68,100,0,0)
end if
end on
on controller
if ($CC_NUM = 20)
set_rpn(1,100)
wait(40000)
play_note(72,100,0,0)
end if
if ($CC_NUM = $VCC_PITCH_BEND)
wait(40000)
play_note(76,100,0,0)
end if
if ($CC_NUM = $VCC_MONO_AT)
wait(40000)
play_note(75,100,0,0)
end if
end on
on poly_at
wait(40000)
play_note(77,100,0,0)
end on
on ui_control($go)
if ($go = 1)
wait(40000)
set_controller(7,63)
play_note(74,100,0,0)
else
fade_in(%ids[0],2000)
fade_in(%ids[1],2000)
end if
end on"#;
        let groups: Vec<_> = (0..4).map(|g| Group { name: format!("group{g}"), release_trigger: g >= 2, ..Group::default() }).collect();
        let path = std::path::PathBuf::from("tone");
        let zones = (0..4).map(|group| Zone { group, sample: path.clone(), low_key: 48, high_key: 80, low_velocity: 1, high_velocity: 127, ..Zone::default() }).collect();
        let bank = Bank::from_samples(groups, zones, vec![(path, Sample { rate: 48000, frames: vec![[0.5; 2]; 48000] })]).unwrap();
        let (rt, errors) = Runtime::with_scripts(&[script, "on note\nif ($EVENT_NOTE = 61)\nset_event_par($EVENT_ID,$EVENT_PAR_MIDI_CHANNEL,5)\nend if\nend on\non rpn\nwait(40000)\nplay_note(78,100,0,0)\nend on"], &mut LogEngine::new(Vec::new(), 48000.0), 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let mut e = Engine::default();
        e.reset(48000.0);
        e.set_bank(Some(Box::new(bank)));
        e.set_script(Some(Box::new(rt)));
        let mut a = Articulate::default();
        a.sync("lib.nki", &[("first".into(), Some(12), None), ("second".into(), Some(13), None)]);
        a.mode = Mode::Channel;
        let mut r = router(&a, &Mpe::default());
        e.cc(0, 64, 127);
        for input in 0..2 {
            for note in [60, 64, 66, 70] { feed(&mut r, &mut e, In::NoteOn(input, note, 100), 0); }
            feed(&mut r, &mut e, In::Cc(input, 20, 100), 0);
            feed(&mut r, &mut e, In::Bend(input, 10000), 0);
            feed(&mut r, &mut e, In::Pressure(input, 100), 0);
            feed(&mut r, &mut e, In::PolyAt(input, 60, 100), 0);
        }
        e.ui_control(0, 0, 1);
        let advance = |e: &mut Engine, frames| { let (mut l, mut r) = (vec![0.; frames], vec![0.; frames]); e.render(&mut l, &mut r); };
        advance(&mut e, 512);
        for input in 0..2 {
            for note in [64, 66] { feed(&mut r, &mut e, In::NoteOff(input, note), 0); }
        }
        advance(&mut e, 512); // Native note 64 releases are now pedal-deferred.
        assert_eq!(voices(&e, 61, false), [0, 1], "timed children sound before abort");
        assert!(e.voice_census().iter().filter(|v| v.note == 61).all(|v| v.channel == 5), "script reroutes the timed child away from home");
        assert!(e.voice_census().iter().any(|v| v.note == 62 && v.group == 2 && v.release_trigger));
        // UI-created FadeIn commands still carry the target event's origin.
        // Otherwise a queued ramp can overwrite the selected voice's cut fade.
        e.ui_control(0, 0, 0);
        feed(&mut r, &mut e, In::Cc(0, 120, 0), 0);
        advance(&mut e, 4096); // Pass every canceled wait and the click-free fade.
        for note in [60, 61, 62, 64, 66, 67, 68, 70] {
            let groups: Vec<_> = e.voice_census().iter().filter(|v| v.note == note).map(|v| v.group).collect();
            assert!(!groups.is_empty(), "unrelated input lost note {note}");
            assert!(groups.iter().all(|g| *g == 1 || *g == 3), "selected input restarted note {note}: {groups:?}");
        }
        for note in [72, 75, 76, 77, 78] {
            assert_eq!(e.voice_census().iter().filter(|v| v.note == note).count(), 4, "only unrelated performance callback may start {note}");
        }
        assert_eq!(e.voice_census().iter().filter(|v| v.note == 74).count(), 4, "UI callback has no physical origin");
        assert_eq!(e.cc_state()[0][7], 63);
        assert_eq!(r.held[0][60], (0,60), "sound-off keeps the physical release route");
        assert!(e.script().unwrap().key_down_from(0, 0, 60));
        assert!(e.script().unwrap().key_down_from(1, 0, 60));
        e.cc(0, 64, 0);
        advance(&mut e, 512);
        let release_groups: Vec<_> = e.voice_census().iter().filter(|v| v.note == 64 && v.release_trigger).map(|v| v.group).collect();
        assert_eq!(release_groups, [3], "pedal-up must not revive canceled native releases");
        feed(&mut r, &mut e, In::NoteOn(0, 71, 100), 0);
        advance(&mut e, 512);
        assert_eq!(voices(&e, 71, false), [0], "fresh physical input remains usable");
        assert!(e.script().unwrap().diagnostics().is_empty());
        assert_eq!(e.dropped_commands(), 0);
        e.cc(0, 120, 0);
        advance(&mut e, 512);
        assert!(e.voice_census().iter().all(|v| v.channel == 5 && v.note == 61 && v.group == 1),
            "engine-channel stop cuts every origin on home, including UI, preserving rerouted channels");
        assert!(!voices(&e, 61, false).is_empty());
        e.panic();
        advance(&mut e, 512);
        assert!(e.voice_census().is_empty(), "Panic cuts every logical channel");
    }

    #[test]
    fn unscripted_channel_notes_release_their_own_same_pitch_voices() {
        for stop in [false,true] {
            let (mut e,mut r)=three_articulation_part();
            e.set_script(None);
            for channel in 0..2 { feed(&mut r,&mut e,In::NoteOn(channel,60,100),0); }
            render(&mut e);
            assert_eq!(voices(&e,60,false).len(),6);
            feed(&mut r,&mut e,if stop { In::Cc(0,123,0) } else { In::NoteOff(0,60) },0);
            render(&mut e);
            assert_eq!(voices(&e,60,false).len(),3,"one channel's release must leave only the other's voices held");
            assert!(e.key_down(0,60));
            feed(&mut r,&mut e,In::NoteOff(1,60),0);
            render(&mut e);
            assert!(voices(&e,60,false).is_empty());
            assert!(!e.key_down(0,60));
        }
    }

    #[test]
    fn shared_pitch_retriggers_release_the_sustained_note() {
        let (mut e, mut r) = three_articulation_part();
        feed(&mut r, &mut e, In::NoteOn(2, 60, 100), 0);
        for channel in [0, 1].into_iter().cycle().take(12) {
            feed(&mut r, &mut e, In::NoteOn(channel, 60, 100), 0);
            render(&mut e);
            feed(&mut r, &mut e, In::NoteOff(channel, 60), 0);
        }
        assert!(!voices(&e, 60, false).is_empty(), "the sustained note must keep playing");
        feed(&mut r, &mut e, In::NoteOff(2, 60), 0);
        render(&mut e);
        assert!(voices(&e, 60, false).is_empty(), "every same-pitch parent must receive its release");
    }

    #[test]
    fn velocity_splits_and_rescales() {
        let mut a = areia();
        a.mode = Mode::Velocity;
        assert_eq!(
            a.articulations.iter().map(|a| (a.low, a.high)).collect::<Vec<_>>(),
            [(1, 31), (32, 63), (64, 95), (96, 127)]
        );
        let mut r = router(&a, &Mpe::default());
        // Soft plays sustained, rescaled: 31 is the top of its range.
        let out = run(&mut r, &[In::NoteOn(0, 60, 31), In::NoteOn(0, 64, 96)]);
        assert_eq!(out[..3], [Out::NoteOn(0, 12, SWITCH_VELOCITY), Out::NoteOff(0, 12), Out::NoteOn(0, 60, 127)]);
        assert_eq!(out[3..], [Out::NoteOn(0, 15, SWITCH_VELOCITY), Out::NoteOff(0, 15), Out::NoteOn(0, 64, 1)]);
        a.fixed_velocity = 100;
        let mut r = router(&a, &Mpe::default());
        assert_eq!(run(&mut r, &[In::NoteOn(0, 60, 40)])[2], Out::NoteOn(0, 60, 100));
    }

    #[test]
    fn disabled_rows_and_control_switches() {
        let mut a = areia();
        a.mode = Mode::Channel;
        a.articulations[1].enabled = false;
        a.articulations[2].key = None;
        a.articulations[2].control = Some((1, 40));
        let mut r = router(&a, &Mpe::default());
        // Channel 2's articulation is off: the note plays as it is.
        assert_eq!(run(&mut r, &[In::NoteOn(1, 60, 90)]), [Out::NoteOnFrom(0, 1, 60, 90)]);
        assert_eq!(run(&mut r, &[In::NoteOn(2, 62, 90)]), [Out::Control(1, 40, 1), Out::NoteOnFrom(0, 2, 62, 90)]);
        // Channel mode hears the articulations' channels on an omni or set part.
        let c = PartControls { channel: 0, ..PartControls::default() };
        assert!(r.hears(&c, 0, 2) && !r.hears(&c, 0, 1) && !r.hears(&c, 1, 2));
    }

    #[test]
    fn remapped_keyswitches_reach_the_original() {
        let mut a = areia();
        a.articulations[0].remap = Some(100);
        let mut r = router(&a, &Mpe::default());
        let out = run(&mut r, &[In::NoteOn(0, 100, 90), In::NoteOff(0, 100), In::NoteOn(0, 12, 90)]);
        assert_eq!(out, [Out::NoteOn(0, 12, 90), Out::NoteOff(0, 12), Out::NoteOn(0, 12, 90)]);
        a.keep_original = false;
        let mut r = router(&a, &Mpe::default());
        assert_eq!(run(&mut r, &[In::NoteOn(0, 12, 90), In::NoteOff(0, 12)]), []);
        // A cleared keyswitch no longer switches, original or not.
        let mut cleared = areia();
        cleared.articulations[1].remap = Some(CLEARED);
        let mut r = router(&cleared, &Mpe::default());
        assert_eq!(run(&mut r, &[In::NoteOn(0, 13, 90), In::NoteOn(0, 12, 90)]), [Out::NoteOn(0, 12, 90)]);
        // A list read from another instrument routes nothing.
        let mut r = Router::default();
        r.set_route(Route::new("other.nki", &a, &Mpe::default()));
        assert_eq!(run(&mut r, &[In::NoteOn(0, 100, 90)]), [Out::NoteOn(0, 100, 90)]);
    }

    #[test]
    fn sync_keeps_player_settings_by_name() {
        let mut a = areia();
        a.articulations[2].channel = 9;
        a.articulations[2].remap = Some(90);
        let found: Vec<_> = a.articulations.iter().rev().map(|a| (a.name.clone(), a.key, a.control)).collect();
        assert!(a.sync("lib.nki", &found));
        assert_eq!((a.articulations[1].channel, a.articulations[1].remap), (9, Some(90)));
        assert!(!a.sync("lib.nki", &found));
        assert!(a.sync("other.nki", &found));
        assert_eq!(a.articulations[1].remap, None);
    }

    #[test]
    fn mpe_master_mod_wheel_reaches_members_after_one_script_callback() {
        use crate::{
            audio::Sample,
            engine::Bank,
            import::{Group, Zone as SampleZone},
            modulation::{ModAssignment, ModSource, ModTarget},
        };
        let engine = |scripted: bool| {
            let group = Group {
                mods: vec![ModAssignment { cc_default: None,
                    name: "CC1_VOLUME".into(),
                    source: ModSource::MidiCc(1),
                    target: ModTarget::Volume,
                    intensity: 1.,
                    invert: false,
                    lag_ms: 0,
                    shaper: None,
                }],
                ..Group::default()
            };
            let sample = Sample {
                rate: 48000,
                frames: vec![[0.25; 2]; 24000],
            };
            let bank = Bank::from_samples(
                vec![group],
                vec![SampleZone::default()],
                vec![(std::path::PathBuf::new(), sample)],
            )
            .unwrap();
            let mut e = Engine::default();
            e.set_bank(Some(Box::new(bank)));
            // Duplicated callbacks change pan, making the rendered comparison fail.
            if scripted {
                e.set_script(Some(Box::new(runtime("on init\ndeclare $calls\nend on\non controller\nif ($CC_NUM = 1)\ninc($calls)\nset_engine_par($ENGINE_PAR_PAN,500000+$calls*10000,-1,-1,-1)\nend if\nend on"))));
            }
            e
        };
        for scripted in [false, true] {
            for (zone, master, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
                let mut actual = engine(scripted);
                let mut r = router(
                    &Articulate::default(),
                    &Mpe {
                        zone,
                        ..Mpe::default()
                    },
                );
                feed(&mut r, &mut actual, In::Cc(master, 1, 96), 0);
                feed(&mut r, &mut actual, In::NoteOn(member, 60, 100), 0);
                let mut expected = engine(scripted);
                expected.cc(member, 1, 96);
                expected.note_on(member, 60, 100);
                let (mut left, mut right, mut ref_left, mut ref_right) =
                    ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                actual.render(&mut left, &mut right);
                expected.render(&mut ref_left, &mut ref_right);
                assert!(
                    left.iter()
                        .zip(ref_left)
                        .chain(right.iter().zip(ref_right))
                        .all(|(a, b)| (a - b).abs() < 1e-5),
                    "{zone:?}, scripted={scripted}: manager modulation or callback count differs"
                );
            }
        }
    }

    /// Remembered member pressure sets a new note's initial expression.
    #[test]
    fn mpe_pressure_before_note_on_sets_the_same_initial_expression() {
        for scripted in [false, true] {
            for (zone, member) in [(Zone::Lower, 1), (Zone::Upper, 14)] {
                let play = |before: bool| {
                    let (mut e, _) = three_articulation_part();
                    if !scripted { e.set_script(None); }
                    let mut r = router(&Articulate::default(), &Mpe { zone, ..Mpe::default() });
                    if before { feed(&mut r, &mut e, In::Pressure(member, 37), 0); }
                    feed(&mut r, &mut e, In::NoteOn(member, 60, 100), 0);
                    if !before { feed(&mut r, &mut e, In::Pressure(member, 37), 0); }
                    let (mut left, mut right) = ([0.; 128], [0.; 128]);
                    e.render(&mut left, &mut right);
                    (left, right)
                };
                let (before, after) = (play(true), play(false));
                for i in 0..128 {
                    assert!((before.0[i] - after.0[i]).abs() < 1e-5 && (before.1[i] - after.1[i]).abs() < 1e-5, "{zone:?}, scripted={scripted}: initial pressure lost at frame {i}");
                }
            }
        }
    }

    #[test]
    fn mpe_initial_pressure_reaches_raw_modulation_with_scripts() {
        use crate::{
            audio::Sample,
            engine::Bank,
            import::{Group, Zone as SampleZone},
            modulation::{ModAssignment, ModSource, ModTarget},
        };
        let setup = |scripted| {
            let group = Group {
                mods: vec![ModAssignment { cc_default: None,
                    name: "PRESSURE_VOLUME".into(),
                    source: ModSource::MonoAftertouch,
                    target: ModTarget::Volume,
                    intensity: 1.,
                    invert: false,
                    lag_ms: 0,
                    shaper: None,
                }],
                ..Group::default()
            };
            let sample = Sample {
                rate: 48000,
                frames: vec![[0.25; 2]; 24000],
            };
            let bank = Bank::from_samples(
                vec![group],
                vec![SampleZone::default()],
                vec![(std::path::PathBuf::new(), sample)],
            )
            .unwrap();
            let mut e = Engine::default();
            e.set_bank(Some(Box::new(bank)));
            if scripted {
                e.set_script(Some(Box::new(runtime("on init\nend on"))));
            }
            e
        };
        for scripted in [false, true] {
            for (zone, master, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
                let mut actual = setup(scripted);
                let mut ar = router(
                    &Articulate::default(),
                    &Mpe {
                        zone,
                        ..Mpe::default()
                    },
                );
                feed(&mut ar, &mut actual, In::Pressure(member, 37), 0);
                feed(&mut ar, &mut actual, In::Pressure(master, 64), 0);
                feed(&mut ar, &mut actual, In::NoteOn(member, 60, 100), 0);
                let mut expected = setup(scripted);
                // Independent non-MPE reference: pressure source is max(37,64),
                // while the member's initial native gain uses its own pressure37.
                expected.channel_pressure(member, 64);
                expected.set_expression_on(member,60,Expression { gain:pressure_gain(37),..Expression::default() });
                expected.note_on(member,60,100);
                let (mut a, mut b, mut c, mut d) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                actual.render(&mut a, &mut b);
                expected.render(&mut c, &mut d);
                assert!(
                    a.iter()
                        .zip(c)
                        .chain(b.iter().zip(d))
                        .all(|(a, b)| (a - b).abs() < 1e-5),
                    "{zone:?}, scripted={scripted}: initial member/manager pressure was lost"
                );
            }
        }
    }

    #[test]
    fn mpe_rpn_range_changes_update_active_notes_and_nrpn_does_not_change_range() {
        use crate::{audio::Sample,engine::Bank,import::{Group,Zone as SampleZone}};
        let engine=|| {
            let bank=Bank::from_samples(vec![Group::default()],vec![SampleZone::default()],vec![(
                Default::default(),Sample {rate:48000,frames:(0..32000).map(|i| [i as f32/32000.;2]).collect()})]).unwrap();
            let mut e=Engine::default();e.set_bank(Some(Box::new(bank)));e
        };
        for (zone,members) in [(Zone::Lower,[1,2]),(Zone::Upper,[14,13])] {
            let (mut e,mut reference)=(engine(),engine());
            let mut r=router(&Articulate::default(),&Mpe {zone,..Mpe::default()});
            for (channel,note) in members.into_iter().zip([60,62]) {
                feed(&mut r,&mut e,In::Bend(channel,12288),0);
                feed(&mut r,&mut e,In::NoteOn(channel,note,100),0);
                reference.set_expression_on(channel,note,Expression {tune:24.,..Default::default()});
                reference.note_on(channel,note,100);
            }
            let check=|e:&mut Engine,reference:&mut Engine| {
                let (mut a,mut b,mut x,mut y)=([0.;128],[0.;128],[0.;128],[0.;128]);
                e.render(&mut a,&mut b);reference.render(&mut x,&mut y);
                assert!(a.iter().zip(x).chain(b.iter().zip(y)).all(|(a,b)| (a-b).abs()<1e-5),"{zone:?}: live range PCM differs from independent note tune");
            };
            check(&mut e,&mut reference);
            for (range,cents) in [(12,50),(0,0)] {
                for (cc,value) in [(101,0),(100,0),(6,range),(38,cents)] {feed(&mut r,&mut e,In::Cc(members[0],cc,value),0);}
                let tune=(f32::from(range)+f32::from(cents)/100.)/2.;
                for (channel,note) in members.into_iter().zip([60,62]) {reference.set_expression_on(channel,note,Expression {tune,..Default::default()});}
                check(&mut e,&mut reference);
            }
            for (cc,value) in [(99,0),(98,0),(6,96)] {feed(&mut r,&mut e,In::Cc(members[0],cc,value),0);}
            feed(&mut r,&mut e,In::Bend(members[0],0),0);
            check(&mut e,&mut reference);
            assert_eq!(e.mpe_zones().member(members[0]).unwrap().member_range,crate::midi::BendRange {coarse:0,fine:0},"NRPN changed pitch sensitivity");
        }
    }

    #[test]
    fn mpe_release_samples_preserve_the_originating_event_expression() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Loop, Zone as SampleZone}, modulation::{ModAssignment, ModSource, ModTarget}};
        for (delay, attack_rendered) in [(0, true), (1000, true), (1000, false)] {
            let setup = || {
                let group = Group { mods: vec![ModAssignment { cc_default: None,
                    name: "PB_PITCH".into(), source: ModSource::PitchBend, target: ModTarget::Pitch,
                    intensity: 1., invert: false, lag_ms: 0, shaper: None,
                }], ..Group::default() };
                let groups = vec![group.clone(), Group { release_trigger: true, ..group }];
                let path = std::path::PathBuf::from("release-tone");
                let zones = (0..2).map(|group| SampleZone { group, sample: path.clone(),
                    loop_range: Some(Loop { start: 0, end: 1024, alternating: false, until_release: false, crossfade: 0 }),
                    ..SampleZone::default() }).collect();
                let tone = Sample { rate: 48000, frames: (0..1024).map(|i| [(i as f32 * 0.07).sin() * 0.2; 2]).collect() };
                let bank = Bank::from_samples(groups, zones, vec![(path, tone)]).unwrap();
                let wait = if delay > 0 { "ignore_event($EVENT_ID)\nwait(1000)\n" } else { "" };
                let off = if delay > 0 { "note_off($EVENT_ID)\n" } else { "" };
                let rt = runtime(&format!("on note\ndisallow_group($ALL_GROUPS)\nallow_group(0)\nend on\non release\n{wait}disallow_group($ALL_GROUPS)\nallow_group(1)\n{off}end on"));
                let mut e = Engine::default();
                e.reset(48000.);
                e.attack = 0.0001;
                e.release = 0.001;
                e.set_bank(Some(Box::new(bank)));
                e.set_script(Some(Box::new(rt)));
                (e, router(&Articulate::default(), &Mpe { zone: Zone::Lower, ..Mpe::default() }))
            };
            let start = |e: &mut Engine, r: &mut Router, gain, pan, tune| {
                feed(r, e, In::NoteOn(1, 60, 100), 0);
                feed(r, e, In::NoteGain(1, 60, gain), 0);
                feed(r, e, In::NotePan(1, 60, pan), 0);
                feed(r, e, In::NoteTune(1, 60, tune), 0);
            };
            let (mut actual, mut ar) = setup();
            let (mut old, mut or) = setup();
            let (mut new, mut nr) = setup();
            for (e, r) in [(&mut actual, &mut ar), (&mut old, &mut or)] {
                start(e, r, 0.25, -1., 7.);
                if attack_rendered { render(e); }
                feed(r, e, In::Cc(0, 64, 127), 0);
                feed(r, e, In::NoteOff(1, 60), 0);
                assert!(!e.key_down(1, 60), "the physical key must be up before reuse");
            }
            // Old Release and new Start are queued at the same frame; pedal-up
            // later creates the old release sample after this member has been reused.
            start(&mut actual, &mut ar, 0.75, 1., -5.);
            start(&mut new, &mut nr, 0.75, 1., -5.);
            for e in [&mut actual, &mut old, &mut new] { render(e); }
            for (e, r) in [(&mut actual, &mut ar), (&mut old, &mut or), (&mut new, &mut nr)] {
                feed(r, e, In::Cc(0, 64, 0), 0);
            }
            let audio = |e: &mut Engine| {
                let (mut l, mut r) = ([0.; 128], [0.; 128]);
                e.render(&mut l, &mut r);
                (l, r)
            };
            let mut old_release_step = None;
            for master_bend in [8192, 12288] {
                for (e, r) in [(&mut actual, &mut ar), (&mut old, &mut or), (&mut new, &mut nr)] {
                    feed(r, e, In::Bend(0, master_bend), 0);
                }
                let (a, o, n) = (audio(&mut actual), audio(&mut old), audio(&mut new));
                let voices = actual.voice_census();
                let release_step = voices.iter().find(|v| v.release_trigger)
                    .unwrap_or_else(|| panic!("no release sample: delay={delay}, attack_rendered={attack_rendered}, master={master_bend}, voices={voices:?}, diagnostics={:?}", actual.script().unwrap().diagnostics())).step;
                if let Some(before) = old_release_step { assert!(release_step > before, "master bend must remain live on a frozen release tail"); }
                old_release_step = Some(release_step);
                for i in 0..128 {
                    assert!((a.0[i] - o.0[i] - n.0[i]).abs() < 1e-5 && (a.1[i] - o.1[i] - n.1[i]).abs() < 1e-5,
                        "frame {i}: release sample followed the reused member's expression (master bend {master_bend})");
                }
            }
        }
    }

    #[test]
    fn mpe_member_controls_stop_modulating_released_notes_on_channel_reuse() {
        use crate::{
            audio::Sample,
            engine::Bank,
            import::{Group, Zone as SampleZone},
            modulation::{ModAssignment, ModSource, ModTarget},
        };
        for source in [ModSource::MidiCc(74), ModSource::MonoAftertouch] {
            for scripted in [false, true] {
                for (zone, master, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
                    let setup = || {
                        let group = Group {
                            mods: vec![ModAssignment { cc_default: None,
                                name: "CC74_VOLUME".into(),
                                source,
                                target: ModTarget::Volume,
                                intensity: 1.,
                                invert: false,
                                lag_ms: 0,
                                shaper: None,
                            }],
                            ..Group::default()
                        };
                        let sample = Sample {
                            rate: 48000,
                            frames: vec![[0.25; 2]; 24000],
                        };
                        let bank = Bank::from_samples(
                            vec![group],
                            vec![SampleZone::default()],
                            vec![(std::path::PathBuf::new(), sample)],
                        )
                        .unwrap();
                        let mut e = Engine::default();
                        e.reset(48000.);
                        e.attack = 0.0001;
                        e.set_bank(Some(Box::new(bank)));
                        if scripted {
                            e.set_script(Some(Box::new(runtime(
                                "on init\ndeclare $ready := 1\nend on",
                            ))));
                        }
                        (
                            e,
                            router(
                                &Articulate::default(),
                                &Mpe {
                                    zone,
                                    ..Mpe::default()
                                },
                            ),
                        )
                    };
                    let start = |e: &mut Engine, r: &mut Router, brightness| {
                        let control = if source == ModSource::MonoAftertouch {
                            In::Pressure(member, brightness)
                        } else {
                            In::Cc(member, 74, brightness)
                        };
                        feed(r, e, control, 0);
                        feed(r, e, In::NoteOn(member, 60, 100), 0);
                        // Isolate raw modulation from KONTRA's pressure-volume fallback.
                        feed(r, e, In::NoteGain(member, 60, 1.), 0);
                    };
                    let (mut actual, mut ar) = setup();
                    let (mut old, mut or) = setup();
                    let (mut new, mut nr) = setup();
                    for (e, r) in [(&mut actual, &mut ar), (&mut old, &mut or)] {
                        start(e, r, 32);
                        render(e);
                        feed(r, e, In::Cc(master, 64, 127), 0);
                        feed(r, e, In::NoteOff(member, 60), 0);
                    }
                    start(&mut actual, &mut ar, 96);
                    start(&mut new, &mut nr, 96);
                    let audio = |e: &mut Engine| {
                        let (mut l, mut r) = ([0.; 128], [0.; 128]);
                        e.render(&mut l, &mut r);
                        (l, r)
                    };
                    for manager_value in [0, 64] {
                        for (e, r) in [
                            (&mut actual, &mut ar),
                            (&mut old, &mut or),
                            (&mut new, &mut nr),
                        ] {
                            let control = if source == ModSource::MonoAftertouch {
                                In::Pressure(master, manager_value)
                            } else {
                                In::Cc(master, 74, manager_value)
                            };
                            feed(r, e, control, 0);
                        }
                        let (a, o, n) = (audio(&mut actual), audio(&mut old), audio(&mut new));
                        for i in 0..128 {
                            assert!(
                                (a.0[i] - o.0[i] - n.0[i]).abs() < 1e-5
                                    && (a.1[i] - o.1[i] - n.1[i]).abs() < 1e-5,
                                "{zone:?} scripted={scripted}, {source:?}, frame {i}: member control changed an older released note"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn registered_note_brightness_modulates_only_its_retained_transposed_owner_without_heap() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone as SampleZone},
            modulation::{ModAssignment, ModSource, ModTarget}};
        for mpe in [false, true] {
            let group = Group { mods: vec![ModAssignment { cc_default: None, name: "CC74_VOLUME".into(),
                source: ModSource::MidiCc(74), target: ModTarget::Volume, intensity: 1.,
                invert: false, lag_ms: 0, shaper: None }, ModAssignment { cc_default: None, name:"CC74_START".into(),
                source:ModSource::MidiCc(74), target:ModTarget::SampleStart, intensity:1.,
                invert:false, lag_ms:0, shaper:None }], ..Default::default() };
            let zones = [(60, -1.), (61, 1.)].map(|(key, pan)| SampleZone {
                root:key, low_key:key, high_key:key, pan, start_mod:Some(1000), ..Default::default() });
            let bank = Bank::from_samples(vec![group], zones.to_vec(),
                vec![(std::path::PathBuf::new(), Sample { rate:48000, frames:vec![[0.25;2];24000] })]).unwrap();
            let mut e = Engine::default();
            e.reset(48000.);
            e.attack = 0.0001;
            e.set_bank(Some(Box::new(bank)));
            e.set_script(Some(Box::new(runtime("on init\nend on\non note\nchange_note($EVENT_ID,$EVENT_NOTE+12)\nend on"))));
            let mut r = router(&Articulate::default(), &Mpe { zone:if mpe { Zone::Lower } else { Zone::Off }, ..Default::default() });
            r.route.mode = if mpe { Mode::Keyswitch } else { Mode::Channel };
            r.route.keys[60] = 48;
            r.route.keys[61] = 49;
            let (first, second, controller) = if mpe { (1,2,0) } else { (4,4,4) };
            let (mut left, mut right) = ([0.;512], [0.;512]);
            assert_eq!(crate::plugin::tests::allocations(|| {
                feed(&mut r,&mut e,In::Cc(controller,74,32),7);
                if mpe {
                    // Isolate the manager's source from the default centered member timbre.
                    for member in [first,second] { feed(&mut r,&mut e,In::Cc(member,74,0),7); }
                }
                feed(&mut r,&mut e,In::NoteOn(first,60,100),7);
                feed(&mut r,&mut e,In::NoteOn(second,61,100),7);
                // Start-only sample-offset modulation must see the same-sample
                // override before queued voices have ever been rendered.
                feed(&mut r,&mut e,In::NoteBrightness(first,60,crate::midi::ControlValue::from_midi1(127)),7);
                e.render(&mut left,&mut right);
                assert!((left[511]/right[511]-127./32.).abs()<0.01);
                r.set_expression(if mpe { first } else { 7 },48,|x| x.note_cc74=None,&mut |o| apply(&mut e,o));
                e.render(&mut left,&mut right);
                let low = [left[511],right[511]];
                assert!(low.iter().all(|v| *v > 0.001));
                feed(&mut r,&mut e,In::Cc(controller,74,64),7);
                e.render(&mut left,&mut right);
                let common = [left[511],right[511]];
                for i in 0..2 { assert!((common[i]/low[i]-2.).abs()<0.01,"mpe={mpe}: channel CC74 must reach both held notes"); }
                // Current routing differs from the retained attack owner.
                if !mpe { r.route.mode = Mode::Keyswitch; }
                feed(&mut r,&mut e,In::NoteBrightness(first,60,crate::midi::ControlValue::from_midi1(127)),7);
                e.render(&mut left,&mut right);
                assert!((left[511]/common[0]-127./64.).abs()<0.01,"mpe={mpe}: settled native modulation did not see its note override");
                assert!((right[511]-common[1]).abs()<1e-6,"mpe={mpe}: per-note brightness changed the other voice");
                assert_eq!(r.brightness[48],NONE,"per-note brightness must not enter the part-wide fallback filter cache");
                // Repeating an absolute value neither adds to it nor emits another edit.
                r.input(In::NoteBrightness(first,60,crate::midi::ControlValue::from_midi1(127)),7,&mut |_| panic!("equal note expression emitted twice"));
                // Channel controls still reach the note without an override.
                if !mpe { r.route.mode = Mode::Channel; }
                feed(&mut r,&mut e,In::Cc(controller,74,16),7);
                e.render(&mut left,&mut right);
                assert!((left[511]/common[0]-127./64.).abs()<0.01,"mpe={mpe}: channel CC must not be added to an absolute note override");
                assert!((right[511]/common[1]-0.25).abs()<0.01,"mpe={mpe}: channel fallback stopped working");
                feed(&mut r,&mut e,In::NoteBrightness(first,60,crate::midi::ControlValue::from_midi1(8)),7);
                e.render(&mut left,&mut right);
                assert!((left[511]/common[0]-8./64.).abs()<0.01);
                assert!((right[511]/common[1]-0.25).abs()<0.01);
                r.scripted = false;
                assert_eq!(r.cutoff_scale(),1.,"a low note override must not darken the whole part");
            }),0);
            assert_eq!(e.cc_state()[if mpe { 1 } else { 4 }][74],if mpe { 0 } else { 16 },"note expression must never become physical channel CC74");
            if !mpe { assert_eq!(e.cc_state()[7][74],0,"delivery channel must not receive physical controller state"); }
            assert_eq!(e.active_voices(),2);
            let voices = e.voice_census();
            let position = |note| voices.iter().find(|v| v.note==note).unwrap().pos;
            assert!(position(60)-position(61)>500.,"mpe={mpe}: start-only modulation missed the initial note override");
        }
    }

    #[test]
    fn same_frame_note_expression_sets_initial_gain_and_pan() {
        use crate::{
            audio::Sample,
            engine::{Bank, NoteEvent},
            import::{Group, Zone as SampleZone},
        };
        let setup = |scripted| {
            let sample = Sample {
                rate: 48000,
                frames: vec![[0.25; 2]; 24000],
            };
            let bank = Bank::from_samples(
                vec![Group::default()],
                vec![SampleZone::default()],
                vec![(std::path::PathBuf::new(), sample)],
            )
            .unwrap();
            let mut e = Engine::default();
            e.reset(48000.);
            e.attack = 0.0001;
            e.set_bank(Some(Box::new(bank)));
            if scripted {
                e.set_script(Some(Box::new(runtime("on init\nend on"))));
            }
            e
        };
        for scripted in [false, true] {
            let mut actual = setup(scripted);
            let mut r = router(&Articulate::default(), &Mpe::default());
            feed(&mut r, &mut actual, In::NoteOn(0, 60, 100), 0);
            feed(&mut r, &mut actual, In::NoteGain(0, 60, 0.25), 0);
            feed(&mut r, &mut actual, In::NotePan(0, 60, 1.), 0);
            let mut expected = setup(false);
            expected.start_event(&NoteEvent {
                volume: 0.25,
                pan: 1.,
                ..NoteEvent::new(0, 60, 100)
            });
            let (mut a, mut b, mut c, mut d) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
            actual.render(&mut a, &mut b);
            expected.render(&mut c, &mut d);
            assert!(
                a.iter()
                    .zip(c)
                    .chain(b.iter().zip(d))
                    .all(|(a, b)| (a - b).abs() < 1e-5),
                "scripted={scripted}: initial expression briefly used default gain/pan"
            );
            // Later edits still interpolate, preserving the existing click prevention.
            feed(&mut r, &mut actual, In::NoteGain(0, 60, 0.75), 0);
            actual.render(&mut a, &mut b);
            assert!(
                (b[0] - d[127]).abs() < 1e-5 && b[127] > b[0] * 2.,
                "scripted={scripted}: live gain changed without its ramp"
            );
        }
    }

    #[test]
    fn mpe_script_generated_release_inherits_its_parent_expression() {
        use crate::{
            audio::Sample,
            engine::Bank,
            import::{Group, Zone as SampleZone},
            modulation::{ModAssignment, ModSource, ModTarget},
        };
        for (delay, attack_rendered) in [(0, true), (1000, true), (1000, false)] {
            let setup = || {
                let release = Group {
                    mods: vec![ModAssignment { cc_default: None,
                        name: "CC74_VOLUME".into(),
                        source: ModSource::MidiCc(74),
                        target: ModTarget::Volume,
                        intensity: 1.,
                        invert: false,
                        lag_ms: 0,
                        shaper: None,
                    }],
                    ..Group::default()
                };
                let zones = (0..2)
                    .map(|group| SampleZone {
                        group,
                        ..SampleZone::default()
                    })
                    .collect();
                let sample = Sample {
                    rate: 48000,
                    frames: vec![[0.25; 2]; 24000],
                };
                let bank = Bank::from_samples(
                    vec![Group::default(), release],
                    zones,
                    vec![(std::path::PathBuf::new(), sample)],
                )
                .unwrap();
                let wait = if delay > 0 { "wait(1000)\n" } else { "" };
                let rt = runtime(&format!(
                    "on note\ndisallow_group($ALL_GROUPS)\nallow_group(0)\nend on\non release\nignore_event($EVENT_ID)\n{wait}disallow_group($ALL_GROUPS)\nallow_group(1)\nplay_note($EVENT_NOTE,100,0,0)\nnote_off($EVENT_ID)\nend on"
                ));
                let mut e = Engine::default();
                e.reset(48000.);
                e.attack = 0.0001;
                e.release = 0.001;
                e.set_bank(Some(Box::new(bank)));
                e.set_script(Some(Box::new(rt)));
                (
                    e,
                    router(
                        &Articulate::default(),
                        &Mpe {
                            zone: Zone::Lower,
                            ..Mpe::default()
                        },
                    ),
                )
            };
            let start = |e: &mut Engine, r: &mut Router, brightness, gain, pan| {
                feed(r, e, In::Cc(1, 74, brightness), 0);
                feed(r, e, In::NoteOn(1, 60, 100), 0);
                feed(r, e, In::NoteGain(1, 60, gain), 0);
                feed(r, e, In::NotePan(1, 60, pan), 0);
            };
            let (mut actual, mut ar) = setup();
            let (mut old, mut or) = setup();
            let (mut new, mut nr) = setup();
            for (e, r) in [(&mut actual, &mut ar), (&mut old, &mut or)] {
                start(e, r, 32, 0.25, -1.);
                if attack_rendered {
                    render(e);
                }
                feed(r, e, In::Cc(0, 64, 127), 0);
                feed(r, e, In::NoteOff(1, 60), 0);
            }
            start(&mut actual, &mut ar, 96, 0.75, 1.);
            start(&mut new, &mut nr, 96, 0.75, 1.);
            for _ in 0..2 {
                let audio = |e: &mut Engine| {
                    let (mut l, mut r) = ([0.; 128], [0.; 128]);
                    e.render(&mut l, &mut r);
                    (l, r)
                };
                let (a, o, n) = (audio(&mut actual), audio(&mut old), audio(&mut new));
                for i in 0..128 {
                    assert!(
                        (a.0[i] - o.0[i] - n.0[i]).abs() < 1e-5
                            && (a.1[i] - o.1[i] - n.1[i]).abs() < 1e-5,
                        "delay={delay}, attack_rendered={attack_rendered}, frame {i}: generated release lost its parent expression; actual={:?}, old={:?}, new={:?}; voices={:?} vs {:?} + {:?}",
                        (a.0[i], a.1[i]),
                        (o.0[i], o.1[i]),
                        (n.0[i], n.1[i]),
                        actual.voice_census(),
                        old.voice_census(),
                        new.voice_census()
                    );
                }
            }
        }
    }

    #[test]
    fn mpe_same_pitch_channel_reuse_preserves_pedal_held_expression() {
        let setup = || {
            let (e, _) = three_articulation_part();
            (e, router(&Articulate::default(), &Mpe { zone: Zone::Lower, ..Mpe::default() }))
        };
        let start = |e: &mut Engine, r: &mut Router, gain, pan| {
            feed(r, e, In::NoteOn(1, 60, 100), 0);
            feed(r, e, In::NoteGain(1, 60, gain), 0);
            feed(r, e, In::NotePan(1, 60, pan), 0);
        };
        let (mut actual, mut ar) = setup();
        let (mut old, mut or) = setup();
        for (e, r) in [(&mut actual, &mut ar), (&mut old, &mut or)] {
            start(e, r, 0.25, -1.);
            render(e);
            feed(r, e, In::Cc(0, 64, 127), 0);
            feed(r, e, In::NoteOff(1, 60), 0);
        }
        // The old release is queued for the same frame as the new attack.
        start(&mut actual, &mut ar, 0.75, 1.);
        let (mut new, mut nr) = setup();
        start(&mut new, &mut nr, 0.75, 1.);
        let audio = |e: &mut Engine| {
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            e.render(&mut l, &mut r);
            (l, r)
        };
        let (a, o, n) = (audio(&mut actual), audio(&mut old), audio(&mut new));
        for i in 0..128 {
            assert!((a.0[i] - o.0[i] - n.0[i]).abs() < 1e-5 && (a.1[i] - o.1[i] - n.1[i]).abs() < 1e-5,
                "frame {i}: reused member changed the older pedal-held note");
        }
    }

    #[test]
    fn mpe_master_and_member_bends_combine_with_scripts() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone as SampleZone}, modulation::{ModAssignment, ModSource, ModTarget}};
        let engine = |scripted: bool| {
            let group = Group { mods: vec![ModAssignment { cc_default: None,
                name: "PB_PITCH".into(), source: ModSource::PitchBend, target: ModTarget::Pitch,
                intensity: 1.0, invert: false, lag_ms: 0, shaper: None,
            }], ..Group::default() };
            let sample = Sample { rate: 48000, frames: (0..24000).map(|i| [i as f32 / 24000.; 2]).collect() };
            let bank = Bank::from_samples(vec![group], vec![SampleZone::default()], vec![(std::path::PathBuf::new(), sample)]).unwrap();
            let mut e = Engine::default();
            e.set_bank(Some(Box::new(bank)));
            if scripted { e.set_script(Some(Box::new(runtime("on init\nend on")))); }
            e
        };
        for scripted in [false, true] {
            for (zone, master, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
                let mut e = engine(scripted);
                let mut r = router(&Articulate::default(), &Mpe { zone, ..Mpe::default() });
                feed(&mut r, &mut e, In::Bend(master, 12288), 0);
                feed(&mut r, &mut e, In::Bend(member, 12288), 0);
                feed(&mut r, &mut e, In::NoteOn(member, 60, 100), 0);
                let mut reference = engine(scripted);
                reference.pitch_bend(member, 12288);
                reference.note_on(member, 60, 100);
                reference.set_expression_on(member, 60, Expression { tune: 24., ..Expression::default() });
                let (mut actual, mut right, mut expected) = ([0.; 128], [0.; 128], [0.; 128]);
                e.render(&mut actual, &mut right);
                reference.render(&mut expected, &mut right);
                assert!(actual.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5), "{zone:?}, scripted={scripted}: master bend was lost or duplicated");
            }
        }
    }

    #[test]
    fn mpe_negotiated_master_and_member_ranges_render_exact_pitch() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone as SampleZone}, modulation::{ModAssignment, ModSource, ModTarget}};
        let engine = |scripted: bool, pitch_mod: bool| {
            let mut mods = vec![ModAssignment { cc_default: None,
                name: "PB_VOLUME".into(), source: ModSource::PitchBend, target: ModTarget::Volume,
                intensity: 0.25, invert: false, lag_ms: 0, shaper: None,
            }];
            if pitch_mod { mods.push(ModAssignment { cc_default: None,
                name: "PB_PITCH".into(), source: ModSource::PitchBend, target: ModTarget::Pitch,
                intensity: 1., invert: false, lag_ms: 0, shaper: None,
            }); }
            let group = Group { mods, ..Group::default() };
            let sample = Sample { rate: 48000, frames: (0..24000).map(|i| [i as f32 / 24000.; 2]).collect() };
            let bank = Bank::from_samples(vec![group], vec![SampleZone::default()], vec![(std::path::PathBuf::new(), sample)]).unwrap();
            let mut e = Engine::default();
            e.set_bank(Some(Box::new(bank)));
            if scripted { e.set_script(Some(Box::new(runtime("on init\nend on")))); }
            e
        };
        for scripted in [false, true] {
            for pitch_mod in [false, true] {
                for (zone, master, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
                    for on_master in [false, true] {
                        let note_channel = if on_master { master } else { member };
                        let mut e = engine(scripted, pitch_mod);
                        let mut r = router(&Articulate::default(), &Mpe { zone, bend_range: 24, ..Mpe::default() });
                        for channel in [master, member] { feed(&mut r, &mut e, In::Bend(channel, 12288), 0); }
                        for (channel, semitones, cents) in [(master, 6, 25), (member, 12, 50)] {
                            for (cc, value) in [(101, 0), (100, 0), (6, semitones), (38, cents)] {
                                feed(&mut r, &mut e, In::Cc(channel, cc, value), 0);
                            }
                        }
                        feed(&mut r, &mut e, In::NoteOn(note_channel, 60, 100), 0);
                        let mut reference = engine(scripted, pitch_mod);
                        // Keep the same bend-to-volume source in the reference;
                        // remove its library pitch offset from the explicit tune.
                        let source_bend = if on_master { 0.5 } else { 1. };
                        reference.control_from(note_channel,note_channel,crate::midi::Control::Bend(crate::midi::Bend::new(source_bend).unwrap()));
                        reference.note_on(note_channel, 60, 100);
                        let phases = [
                            (master, vec![], 9.375, 3.125),
                            // Reselecting RPN 0 without CC38 infers zero cents.
                            (master, vec![(101, 0), (100, 0), (6, 0)], 6.25, 0.),
                            (master, vec![(101, 127), (100, 127), (38, 99), (6, 96)], 6.25, 0.),
                            (member, vec![(99, 0), (98, 0), (38, 99), (6, 96)], 6.25, 0.),
                            // A new coarse byte clears a fine byte sent beforehand.
                            (member, vec![(100, 0), (101, 0), (38, 25), (6, 7)], 3.5, 0.),
                            (member, vec![(101, 0), (100, 0), (6, 0)], 0., 0.),
                            // An explicit MCM restores ±2/±48, even after manual ranges.
                            (master, vec![(101, 0), (100, 6), (6, 2)], 25., 1.),
                        ];
                        for (channel, controls, tune, master_tune) in phases {
                            let tune = if on_master { master_tune } else { tune };
                            for (cc, value) in controls { feed(&mut r, &mut e, In::Cc(channel, cc, value), 0); }
                            reference.set_expression_on(note_channel, 60, Expression { tune: tune - if pitch_mod { source_bend as f32 * 12. } else { 0. }, ..Expression::default() });
                            let (mut actual, mut right, mut expected) = ([0.; 128], [0.; 128], [0.; 128]);
                            e.render(&mut actual, &mut right);
                            reference.render(&mut expected, &mut right);
                            assert!(actual.iter().any(|v| *v != 0.));
                            assert!(actual.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5),
                                "{zone:?}, scripted={scripted}, pitch_mod={pitch_mod}, on_master={on_master}, tune={tune}: negotiated pitch lost or doubled");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn reset_all_controllers_clears_cached_pedals_and_mpe_without_stopping_keys() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone as SampleZone}};
        let engine = |scripted: bool| {
            let sample = Sample { rate: 48000, frames: (0..24000).map(|i| [i as f32 / 24000.; 2]).collect() };
            let bank = Bank::from_samples(vec![Group::default()], vec![SampleZone::default()],
                vec![(std::path::PathBuf::new(), sample)]).unwrap();
            let mut e = Engine::default();
            e.attack = 0.0001;
            e.release = 0.001;
            e.set_bank(Some(Box::new(bank)));
            if scripted { e.set_script(Some(Box::new(runtime("on init\nend on")))); }
            e
        };
        let mut e = engine(false);
        e.set_script(Some(Box::new(runtime(r#"on init
            SET_CONDITION(NO_SYS_SCRIPT_PEDAL)
            declare %pedal[16]
            declare %events[2048]
            declare %held[2048]
            declare $i
            declare ui_knob $setting (0, 100, 1)
            declare ui_knob $registered (0, 127, 1)
            declare ui_knob $resumed_zero (0, 100, 1)
            declare ui_knob $resumed_one (0, 100, 1)
        end on
        on note
            %events[128 * $MIDI_CHANNEL + $EVENT_NOTE] := $EVENT_ID
            %held[128 * $MIDI_CHANNEL + $EVENT_NOTE] := 1
        end on
        on release
            %held[128 * $MIDI_CHANNEL + $EVENT_NOTE] := 0
            if (%pedal[$MIDI_CHANNEL] >= 64)
                ignore_event($EVENT_ID)
            end if
        end on
        on controller
            if ($CC_NUM = 6 and %CC[100] < 127 and %CC[101] < 127)
                $registered := %CC[6]
            end if
            if ($CC_NUM = 1 and %CC[1] = 99)
                wait(10000)
                if ($MIDI_CHANNEL = 0)
                    inc($resumed_zero)
                else
                    inc($resumed_one)
                end if
                set_controller(64, 127)
            end if
            if ($CC_NUM = 64 or $CC_NUM = 66)
                %pedal[$MIDI_CHANNEL] := %CC[$CC_NUM]
                if (%CC[$CC_NUM] < 64)
                    $i := 0
                    while ($i < 128)
                        if (%held[128 * $MIDI_CHANNEL + $i] = 0 and %events[128 * $MIDI_CHANNEL + $i] # 0)
                            note_off(%events[128 * $MIDI_CHANNEL + $i])
                        end if
                        inc($i)
                    end while
                end if
            end if
        end on
        on ui_control($setting)
            wait(20000)
            $setting := 42 + %POLY_AT[61]
        end on"#))));
        for (cc, value) in [(0, 23), (32, 14), (7, 90), (10, 37), (74, 81), (91, 77),
                            (2, 88), (65, 127), (67, 127), (98, 0), (99, 0), (100, 0), (101, 0)] {
            e.cc(0, cc, value);
        }
        e.cc(0, 6, 31);
        e.poly_pressure(0, 61, 92);
        e.render(&mut [0.; 128], &mut [0.; 128]);
        e.cc(0, 64, 127);
        e.note_on(0, 60, 100);
        e.note_off(0, 60);
        e.note_on(0, 61, 100);
        e.note_on(1, 64, 100);
        e.cc(0, 1, 99);
        e.cc(1, 1, 99);
        e.ui_control(0, 0, 1);
        e.cc(0, 121, 0);
        assert!(e.key_down(0, 61) && e.key_down(1, 64));
        let (mut left, mut right) = ([0.; 128], [0.; 128]);
        for _ in 0..12 { e.render(&mut left, &mut right); }
        let ui = e.script().unwrap().interface(0);
        assert_eq!(ui.controls[2].properties["$CONTROL_PAR_VALUE"],crate::ksp::Value::Int(0),"CC121 must cancel the older physical0 callback");
        assert_eq!(ui.controls[3].properties["$CONTROL_PAR_VALUE"],crate::ksp::Value::Int(1),"unrelated physical1 callback must survive");
        // The surviving callback explicitly authors a part-wide controller edit.
        assert_eq!(e.cc_state()[0][64], 127, "accepted authored pedal must broadcast to every performance row");
        for (cc, value) in [(0, 23), (32, 14), (7, 90), (10, 37), (74, 81), (91, 77),
                            (2, 0), (65, 0), (67, 0), (98, 127), (99, 127), (100, 127), (101, 127)] {
            assert_eq!(e.cc_state()[0][cc], value, "CC{cc}: incorrect RP-015 reset");
            assert_eq!(e.script().unwrap().env.input.cc[cc], i32::from(value));
        }
        assert_eq!(e.cc_state()[1][64], 127, "unrelated channel callbacks survive");
        assert_eq!(e.script().unwrap().interface(0).controls[1].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(31), "registered parameter changed while resetting its data-entry CC");
        assert!(!e.voice_census().iter().any(|v| v.note == 60));
        assert!(e.voice_census().iter().any(|v| v.note == 61));
        assert_eq!(e.script().unwrap().interface(0).controls[0].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(42));
        e.note_off(0, 61);
        e.cc(1, 64, 0);
        e.note_off(1, 64);
        for _ in 0..12 { e.render(&mut left, &mut right); }
        assert_eq!(e.active_voices(), 0, "fresh releases see the reset script cache");
        e.set_mpe_zone(Some((0, 1 << 1)));
        e.cc(1, 64, 127);
        e.note_on(1, 60, 100);
        e.note_off(1, 60);
        e.cc(0, 121, 0);
        for _ in 0..12 { e.render(&mut left, &mut right); }
        assert_eq!(e.active_voices(), 0, "MPE master reset also clears a member's authored pedal cache");
        assert!(e.script().unwrap().diagnostics().is_empty());

        for scripted in [false, true] {
            for (zone, master, member, other) in [(Zone::Lower, 0, 1, 2), (Zone::Upper, 15, 14, 13)] {
                let (mut e, mut reference) = (engine(scripted), engine(scripted));
                let mut r = router(&Articulate::default(), &Mpe { zone, ..Mpe::default() });
                for (channel, semitones, cents) in [(master, 6, 25), (member, 12, 50)] {
                    for (cc, value) in [(101, 0), (100, 0), (6, semitones), (38, cents)] {
                        feed(&mut r, &mut e, In::Cc(channel, cc, value), 0);
                    }
                }
                for channel in [master, member, other] { feed(&mut r, &mut e, In::Bend(channel, 12288), 0); }
                for (channel, note, pressure) in [(member, 60, 100), (other, 64, 80)] {
                    feed(&mut r, &mut e, In::Pressure(channel, pressure), 0);
                    feed(&mut r, &mut e, In::NoteOn(channel, note, 100), 0);
                    reference.note_on(channel, note, 100);
                    reference.set_expression_on(channel, note, Expression { tune: 9.375, gain: pressure_gain(pressure), ..Expression::default() });
                }
                let mut expected = [0.; 128];
                e.render(&mut left, &mut right);
                reference.render(&mut expected, &mut right);
                feed(&mut r, &mut e, In::Cc(member, 121, 0), 0);
                reference.set_expression_on(member, 60, Expression { tune: 3.125, ..Expression::default() });
                assert!(e.key_down(member, 60) && e.key_down(other, 64));
                feed(&mut r, &mut e, In::NoteOn(member, 67, 100), 0);
                reference.note_on(member, 67, 100);
                reference.set_expression_on(member, 67, Expression { tune: 3.125, ..Expression::default() });
                e.render(&mut left, &mut right);
                reference.render(&mut expected, &mut right);
                assert!(left.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5), "member reset changed another member or restored stale expression");
                feed(&mut r, &mut e, In::Cc(master, 121, 0), 0);
                for (channel, note) in [(member, 60), (member, 67), (other, 64)] {
                    reference.set_expression_on(channel, note, Expression::default());
                }
                assert_eq!(r.mpe.member(member).unwrap().member_range, crate::midi::BendRange { coarse: 12, fine: 50 });
                assert_eq!(r.mpe.manager(master).unwrap().manager_range, Some(crate::midi::BendRange { coarse: 6, fine: 25 }));
                feed(&mut r, &mut e, In::Cc(member, 6, 96), 0);
                assert_eq!(r.mpe.member(member).unwrap().member_range, crate::midi::BendRange { coarse: 12, fine: 50 }, "data entry after reset has no selected RPN");
                e.render(&mut left, &mut right);
                reference.render(&mut expected, &mut right);
                assert!(left.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5), "master reset failed to clear its whole MPE zone");
                feed(&mut r, &mut e, In::Bend(member, 12288), 0);
                for note in [60, 67] { reference.set_expression_on(member, note, Expression { tune: 6.25, ..Expression::default() }); }
                e.render(&mut left, &mut right);
                reference.render(&mut expected, &mut right);
                assert!(left.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5), "reset lost negotiated cents/range");
            }
        }
    }

    #[test]
    fn mpe_master_pedals_hold_scripted_member_notes() {
        for (zone, master, member) in [(Zone::Lower, 0, 1), (Zone::Upper, 15, 14)] {
            let (mut e, _) = three_articulation_part();
            let mut r = router(&Articulate::default(), &Mpe { zone, ..Mpe::default() });
            feed(&mut r, &mut e, In::NoteOn(member, 60, 100), 0);
            feed(&mut r, &mut e, In::Cc(master, 66, 127), 0);
            feed(&mut r, &mut e, In::NoteOn(member, 62, 100), 0);
            feed(&mut r, &mut e, In::Cc(master, 66, 100), 0);
            feed(&mut r, &mut e, In::Cc(master, 64, 127), 0);
            for note in [60, 62] {
                feed(&mut r, &mut e, In::NoteOff(member, note), 0);
            }
            render(&mut e);
            assert!(!voices(&e, 60, false).is_empty() && !voices(&e, 62, false).is_empty(), "{zone:?}: master sustain must hold both member notes");
            feed(&mut r, &mut e, In::Cc(master, 64, 0), 0);
            render(&mut e);
            assert!(!voices(&e, 60, false).is_empty(), "sostenuto retains only its captured note");
            assert!(voices(&e, 62, false).is_empty());
            feed(&mut r, &mut e, In::Cc(master, 66, 0), 0);
            render(&mut e);
            assert!(voices(&e, 60, false).is_empty());
        }
    }

    #[test]
    fn mpe_same_pitch_members_keep_their_own_expression() {
        let mpe = Mpe { zone: Zone::Lower, ..Mpe::default() };
        let play = |channels: &[u8]| {
            let (mut e, _) = three_articulation_part();
            let mut r = router(&Articulate::default(), &mpe);
            for &channel in channels {
                feed(&mut r, &mut e, In::NoteOn(channel, 60, 100), 0);
                feed(&mut r, &mut e, In::NotePan(channel, 60, if channel == 1 { -1.0 } else { 1.0 }), 0);
                feed(&mut r, &mut e, In::NoteGain(channel, 60, if channel == 1 { 0.25 } else { 0.75 }), 0);
            }
            let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
            e.render(&mut l, &mut r);
            (l, r)
        };
        let a = play(&[1]);
        let b = play(&[2]);
        let both = play(&[1, 2]);
        for i in 0..128 {
            assert!((both.0[i] - a.0[i] - b.0[i]).abs() < 1e-5, "left frame {i}: another member changed this note");
            assert!((both.1[i] - a.1[i] - b.1[i]).abs() < 1e-5, "right frame {i}: another member changed this note");
        }
    }

    #[test]
    fn mpe_pressure_zero_is_active_but_cc121_restores_unity_after_waits_without_heap() {
        use crate::{audio::Sample,engine::Bank,import::{Group,Zone as SampleZone}};
        let engine=|| {
            let bank=Bank::from_samples(vec![Group::default()],vec![SampleZone::default()],vec![(
                Default::default(),Sample {rate:48000,frames:vec![[0.25;2];48000]})]).unwrap();
            let mut e=Engine::default();e.set_bank(Some(Box::new(bank)));e.attack=0.0001;e
        };
        for script in [None,Some("on init\nend on"),Some("on init\nend on\non controller\nif ($CC_NUM = $VCC_MONO_AT)\nwait(1000)\nend if\nend on")] {
            for sizes in [&[128][..],&[1,7,31,128][..]] {
                let (mut e,mut reference)=(engine(),engine());
                if let Some(source)=script {e.set_script(Some(Box::new(runtime(source))));}
                let mut r=router(&Articulate::default(),&Mpe {zone:Zone::Lower,..Mpe::default()});
                let (mut a,mut b,mut x,mut y)=([0.;512],[0.;512],[0.;512],[0.;512]);
                let render=|e:&mut Engine,l:&mut [f32],r:&mut [f32]| {
                    let (mut at,mut block)=(0,0);
                    while at<l.len() {let n=sizes[block%sizes.len()].min(l.len()-at);
                        e.render(&mut l[at..at+n],&mut r[at..at+n]);at+=n;block+=1;}
                };
                let mut exercise=|| {
                    feed(&mut r,&mut e,In::NoteOn(1,60,100),0);
                    reference.note_on(1,60,100);
                    render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);
                    let full=a[511];assert!(full>0.01,"fixture must sound without a pressure modulation route");
                    for (event,gain) in [(In::Pressure(1,0),pressure_gain(0)),(In::Cc(1,121,0),1.),(In::Pressure(1,0),pressure_gain(0))] {
                        feed(&mut r,&mut e,event,0);
                        reference.set_expression_on(1,60,Expression {gain,..Default::default()});
                        // Includes the callback wait and the native gain ramp;
                        // a stale reset-default forward cannot hide afterward.
                        for _ in 0..4 {render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);}
                        assert!(a.iter().zip(x).chain(b.iter().zip(y)).all(|(a,b)| (a-b).abs()<1e-6),"script={script:?} sizes={sizes:?} event={event:?}: reset pressure lost its accepted intent");
                        assert!((a[511]/full-gain).abs()<1e-5,"explicit zero must use the native floor; CC121 must restore unity");
                        assert!(e.key_down(1,60));
                    }
                    if let Some(rt)=e.script() {assert!(rt.diagnostics().is_empty(),"{:?}",rt.diagnostics());}
                };
                #[cfg(feature="plugin")]
                assert_eq!(crate::plugin::tests::allocations(||exercise()),0);
                #[cfg(not(feature="plugin"))]
                exercise();
            }
        }
    }

    #[test]
    fn physical_pedals_keep_inputs_isolated_on_one_delivery_channel_at_the_midi2_boundary() {
        use crate::{audio::Sample,engine::{Bank,HostNote,HostPattern},import::{Group,Zone as SampleZone},midi::{Control,ControlValue,Velocity}};
        for pedal in [64,66] {
            for scripted in [false,true] {
                for sizes in [&[512][..],&[1,7,31,128][..]] {
                    let zones=[(60,-1.),(61,1.)].map(|(key,pan)| SampleZone {
                        root:key,low_key:key,high_key:key,pan,..Default::default() });
                    let bank=Bank::from_samples(vec![Group::default()],zones.to_vec(),vec![(
                        Default::default(),Sample {rate:48000,frames:vec![[0.25;2];48000]})]).unwrap();
                    let mut e=Engine::default(); e.set_bank(Some(Box::new(bank)));e.attack=0.0001;e.release=0.002;
                    if scripted { e.set_script(Some(Box::new(runtime("on init\nend on\non controller\nwait(1000)\nend on")))); }
                    let mut r=router(&Articulate::default(),&Mpe::default());r.route.mode=Mode::Channel;
                    let owners=[HostNote {port:0,channel:4,key:60,id:104,clap:true},
                        HostNote {port:0,channel:7,key:61,id:107,clap:true}];
                    let (mut left,mut right)=([0.;512],[0.;512]);
                    let render=|e:&mut Engine,l:&mut [f32],r:&mut [f32]| {
                        let (mut at,mut block)=(0,0);
                        while at<l.len() {let n=sizes[block%sizes.len()].min(l.len()-at);
                            e.render(&mut l[at..at+n],&mut r[at..at+n]);at+=n;block+=1;}
                        e.mark_host_notes();
                    };
                    let mut exercise=|| {
                        for on in [false,true] {
                            for owner in owners {feed(&mut r,&mut e,In::HostOn(owner,Velocity::from_midi1(100),0.),0);}
                            render(&mut e,&mut left,&mut right);
                            assert!(left[511]>0.01 && right[511]>0.01,"fixture must sound on both physical inputs");
                            let word=if on {0x8000_0000} else {0x7fff_ffff};
                            feed(&mut r,&mut e,In::MidiControl(4,Control::Cc(pedal,ControlValue::from_midi2(word))),0);
                            render(&mut e,&mut left,&mut right);
                            for owner in owners {feed(&mut r,&mut e,In::HostOff(HostPattern {
                                port:0,channel:i32::from(owner.channel),key:i32::from(owner.key),id:owner.id,clap:true}),0);}
                            for _ in 0..4 {render(&mut e,&mut left,&mut right);}
                            if on {
                                assert!(left[511]>0.01,"pedal={pedal} scripted={scripted}: physical4 not held");
                                assert!(right.iter().all(|x| x.abs()<1e-6),"pedal={pedal} scripted={scripted}: physical4 pedal leaked to physical7");
                                assert!(e.host_note_pending(owners[0]),"pedal-held exact owner ended early");
                                assert!(!e.host_note_pending(owners[1]),"unrelated physical owner's NOTE_END eligibility delayed");
                                feed(&mut r,&mut e,In::MidiControl(4,Control::Cc(pedal,ControlValue::from_midi2(0x7fff_ffff))),0);
                                for _ in 0..4 {render(&mut e,&mut left,&mut right);}
                            }
                            assert!(left.iter().chain(&right).all(|x| x.abs()<1e-6),"pedal={pedal} scripted={scripted}: below-half value must release");
                            assert_eq!(e.active_voices(),0);
                            for owner in owners {
                                assert!(!e.host_note_pending(owner),"exact address remained ineligible for NOTE_END after pedal release");
                                e.retire_host_note(owner);
                            }
                        }
                    };
                    #[cfg(feature="plugin")]
                    assert_eq!(crate::plugin::tests::allocations(||exercise()),0);
                    #[cfg(not(feature="plugin"))]
                    exercise();
                }
            }
        }
    }

    #[test]
    fn mode_messages_release_only_selected_midi_owners_and_respect_pedals_without_heap() {
        use crate::{audio::Sample,engine::{Bank,HostNote,HostPattern},import::{Group,Zone as SampleZone},midi::{Control,ControlValue,Velocity}};
        let engine=|scripted| {
            let zones=[(60,-1.),(61,1.)].map(|(key,pan)| SampleZone {
                root:key,low_key:key,high_key:key,pan,..Default::default() });
            let bank=Bank::from_samples(vec![Group::default()],zones.to_vec(),vec![(
                Default::default(),Sample {rate:48000,frames:vec![[0.25;2];48000]})]).unwrap();
            let mut e=Engine::default();e.set_bank(Some(Box::new(bank)));e.attack=0.0001;e.release=0.002;
            if scripted {e.set_script(Some(Box::new(runtime("on init\nend on\non note\nwait(1000)\nend on\non release\nwait(1000)\nend on\non controller\nwait(1000)\nend on"))));}
            e
        };
        for cc in 123..=127 {
            for exact in [false,true] {
                for scripted in [false,true] {
                    for sizes in [&[512][..],&[1,7,31,128][..]] {
                        let (mut e,mut reference)=(engine(scripted),engine(scripted));
                        let (mut r,mut rr)=(router(&Articulate::default(),&Mpe::default()),router(&Articulate::default(),&Mpe::default()));
                        r.route.mode=Mode::Channel;rr.route.mode=Mode::Channel;
                        let owners=[HostNote {port:0,channel:4,key:60,id:104,clap:true},HostNote {port:0,channel:7,key:61,id:107,clap:true}];
                        let off=|owner:HostNote| if exact {In::HostOff(HostPattern {port:0,channel:i32::from(owner.channel),key:i32::from(owner.key),id:owner.id,clap:true})} else {In::NoteOff(owner.channel,owner.key)};
                        let (mut a,mut b,mut x,mut y)=([0.;512],[0.;512],[0.;512],[0.;512]);
                        let render=|e:&mut Engine,l:&mut [f32],r:&mut [f32]| {
                            let (mut at,mut block)=(0,0);
                            while at<l.len() {let n=sizes[block%sizes.len()].min(l.len()-at);
                                e.render(&mut l[at..at+n],&mut r[at..at+n]);at+=n;block+=1;}
                            e.mark_host_notes();
                        };
                        let mut exercise=|| {
                            for owner in owners {
                                let on=if exact {In::HostOn(owner,Velocity::from_midi1(100),0.)} else {In::NoteOn(owner.channel,owner.key,100)};
                                feed(&mut r,&mut e,on,0);feed(&mut rr,&mut reference,on,0);
                            }
                            render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);
                            assert!(a[511]>0.01 && b[511]>0.01,"fixture must sound on both inputs");
                            let pedal=In::MidiControl(4,Control::Cc(64,ControlValue::FULL));
                            feed(&mut r,&mut e,pedal,0);feed(&mut rr,&mut reference,pedal,0);
                            render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);
                            let value=if cc==126 {2} else {0};
                            feed(&mut r,&mut e,In::Cc(4,cc,value),0);
                            feed(&mut rr,&mut reference,off(owners[0]),0);
                            for _ in 0..4 {render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);}
                            assert!(a.iter().zip(x).chain(b.iter().zip(y)).all(|(a,b)| (a-b).abs()<1e-6),"CC{cc} exact={exact} scripted={scripted}: implicit release differs from NoteOff");
                            assert!(a[511]>0.01 && b[511]>0.01,"All Notes Off must respect sustain and leave other input held");
                            assert_eq!(e.cc_state()[4][cc as usize],value,"original mode CC value must be retained");
                            assert_eq!(e.cc_state()[7][cc as usize],0,"mode cache leaked to unrelated input");
                            let up=In::MidiControl(4,Control::Cc(64,ControlValue::ZERO));
                            feed(&mut r,&mut e,up,0);feed(&mut rr,&mut reference,up,0);
                            for _ in 0..4 {render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);}
                            assert!(a.iter().all(|v| v.abs()<1e-6) && b[511]>0.01,"CC{cc}: pedal up must end only selected input");
                            assert!(a.iter().zip(x).chain(b.iter().zip(y)).all(|(a,b)| (a-b).abs()<1e-6));
                            if exact {assert!(!e.host_note_pending(owners[0]) && e.host_note_pending(owners[1]));e.retire_host_note(owners[0]);}
                            feed(&mut r,&mut e,off(owners[1]),0);feed(&mut rr,&mut reference,off(owners[1]),0);
                            for _ in 0..4 {render(&mut e,&mut a,&mut b);render(&mut reference,&mut x,&mut y);}
                            assert!(a.iter().chain(&b).all(|v| v.abs()<1e-6));
                            if exact {assert!(!e.host_note_pending(owners[1]));e.retire_host_note(owners[1]);}
                            if let Some(rt)=e.script() {assert!(rt.diagnostics().is_empty(),"{:?}",rt.diagnostics());}
                        };
                        #[cfg(feature="plugin")]
                        assert_eq!(crate::plugin::tests::allocations(||exercise()),0);
                        #[cfg(not(feature="plugin"))]
                        exercise();
                    }
                }
            }
        }
    }

    #[test]
    fn mpe_lower_zone_expresses_each_note() {
        let (mut actual, _) = three_articulation_part();
        let (mut expected, _) = three_articulation_part();
        actual.set_script(None); expected.set_script(None);
        let mut r = router(&Articulate::default(), &Mpe { zone:Zone::Lower, ..Mpe::default() });
        let mut reference = router(&Articulate::default(), &Mpe::default());
        feed(&mut r,&mut actual,In::Bend(1,12288),0);
        for (channel,key,tune) in [(1,60,24.),(2,64,-48.)] {
            feed(&mut r,&mut actual,In::NoteOn(channel,key,100),0);
            feed(&mut reference,&mut expected,In::NoteOn(channel,key,100),0);
            feed(&mut reference,&mut expected,In::NoteTune(channel,key,tune),0);
        }
        feed(&mut r,&mut actual,In::Bend(2,0),0);
        feed(&mut r,&mut actual,In::Pressure(1,127),0);
        let check = |a:&mut Engine,b:&mut Engine| {
            let (mut l,mut r,mut x,mut y)=([0.;128],[0.;128],[0.;128],[0.;128]);
            a.render(&mut l,&mut r); b.render(&mut x,&mut y);
            assert!(l.iter().zip(x).chain(r.iter().zip(y)).all(|(a,b)| (a-b).abs()<1e-5));
        };
        check(&mut actual,&mut expected);
        // The shared received decoder changes both members in this zone.
        for (cc,value) in [(101,0),(100,0),(6,12)] { feed(&mut r,&mut actual,In::Cc(3,cc,value),0); }
        feed(&mut r,&mut actual,In::Bend(1,0),0);
        for (channel,key) in [(1,60),(2,64)] { feed(&mut reference,&mut expected,In::NoteTune(channel,key,-12.),0); }
        check(&mut actual,&mut expected);
        let c = PartControls { channel:0, ..PartControls::default() };
        assert!(r.hears(&c,0,15) && r.hears(&c,0,3));
    }

    #[test]
    fn mpe_pressure_is_volume_only_when_unhandled() {
        for handled in [false,true] {
            let (mut actual, _) = three_articulation_part();
            let (mut expected, _) = three_articulation_part();
            actual.set_script(if handled { Some(Box::new(runtime("on init\nend on\non poly_at\nend on"))) } else { None });
            expected.set_script(None);
            let mut r=router(&Articulate::default(),&Mpe { zone:Zone::Upper,members:4,..Mpe::default() });
            let mut reference=router(&Articulate::default(),&Mpe::default());
            feed(&mut r,&mut actual,In::NoteOn(14,60,100),0);
            feed(&mut r,&mut actual,In::Pressure(14,0),0);
            feed(&mut reference,&mut expected,In::NoteOn(14,60,100),0);
            feed(&mut reference,&mut expected,In::NoteGain(14,60,if handled {1.} else {pressure_gain(0)}),0);
            let (mut l,mut rr,mut x,mut y)=([0.;512],[0.;512],[0.;512],[0.;512]);
            actual.render(&mut l,&mut rr); expected.render(&mut x,&mut y);
            assert!(l.iter().zip(x).chain(rr.iter().zip(y)).all(|(a,b)| (a-b).abs()<1e-5),"handled={handled}");
            let c=PartControls {channel:15,..PartControls::default()};
            assert!(!r.hears(&c,0,10) && r.hears(&c,0,11));
        }
    }

    #[test]
    fn host_note_expressions_follow_the_key() {
        let mut r = Router::default();
        let mut out = Vec::new();
        r.input(In::NoteOn(0, 60, 100), 0, &mut |o| out.push(o));
        r.input(In::NoteTune(0, 60, -3.5), 0, &mut |o| out.push(o));
        r.input(In::NoteGain(0, 60, 0.5), 0, &mut |o| out.push(o));
        r.input(In::NotePan(0, 60, 0.25), 0, &mut |o| out.push(o));
        assert_eq!(out.last(), Some(&Out::Expression(0, 60, Expression { tune: -3.5, gain: 0.5, pan: 0.25, ..Expression::default() })));
        // The next note on the key starts plain.
        r.input(In::NoteOff(0, 60), 0, &mut |o| out.push(o));
        r.input(In::NoteOn(0, 60, 100), 0, &mut |o| out.push(o));
        assert!(out.contains(&Out::Expression(0, 60, Expression::default())));
    }

    #[test]
    fn anonymous_note_expressions_keep_the_captured_channel_after_mode_and_key_remap() {
        let mut r = router(&Articulate::default(), &Mpe::default());
        r.route.mode = Mode::Channel;
        r.route.keys[60] = 48;
        let check = || {
            r.input(In::NoteOn(4, 60, 100), 7, &mut |_| {});
            r.route.mode = Mode::Keyswitch;
            // A same-pitch neighbour now belongs to a different logical channel.
            r.input(In::NoteOn(2, 60, 100), 7, &mut |_| {});
            r.route.keys[60] = 49;
            for ev in [In::NoteTune(4, 60, 12.), In::NoteGain(4, 60, 0.25),
                In::NotePan(4, 60, 1.), In::PolyAt(4, 60, 32), In::NotePressure(4, 60, 127)]
            {
                let mut delivered = 0;
                r.input(ev, 7, &mut |out| {
                    match out {
                        Out::Expression(channel, key, _) | Out::PolyAt(channel, key, _) =>
                            assert_eq!((channel, key), (7, 48), "{ev:?}: held expression must target the attack's channel and key"),
                        other => panic!("unexpected expression delivery {other:?}"),
                    }
                    delivered += 1;
                });
                assert!(delivered > 0, "{ev:?}: expression disappeared");
            }
            assert_eq!(r.expression[7][48], Expression { tune:12., gain:1., pan:1., ..Default::default() });
            assert_eq!(r.expression[2][48], Expression::default(), "the neighbour's expression changed");
            // Key-up still closes exactly the same captured route.
            r.input(In::NoteOff(4, 60), 7, &mut |out| assert_eq!(out, Out::NoteOffFrom(7, 4, 48)));
            assert_eq!(r.held[2][60], (2, 48));
        };
        #[cfg(feature="plugin")]
        assert_eq!(crate::plugin::tests::allocations(check), 0);
        #[cfg(not(feature="plugin"))]
        { let mut check = check; check(); }
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn host_events_read_per_note_and_narrow_the_rest() {
        let tune = midi::per_note_bend_from_semitones(-3.5);
        let ev = |body| In::from_event(&body);
        assert_eq!(
            ev(EventBody::PerNotePitchBend { group: 0, channel: 1, note: 60, value: tune }),
            Some(In::PerNoteControl(1, 60, crate::midi::NoteControl::Pitch(crate::midi::Bend::from_midi2(tune))))
        );
        // CLAP volume: unity gain is the wire's quarter point.
        let gain = ev(EventBody::PerNoteCC { group: 0, channel: 0, note: 60, cc: 7, value: u32::MAX / 4, registered: true });
        assert!(matches!(gain, Some(In::PerNoteControl(0, 60, crate::midi::NoteControl::Registered { index:7, value })) if value == u32::MAX / 4));
        let pan = ev(EventBody::PerNoteCC { group: 0, channel: 0, note: 60, cc: 10, value: u32::MAX, registered: true });
        assert!(matches!(pan, Some(In::PerNoteControl(0, 60, crate::midi::NoteControl::Registered { index:10, value:u32::MAX }))));
        assert_eq!(
            ev(EventBody::PolyPressure2 { group: 0, channel: 2, note: 61, pressure: u32::MAX }),
            Some(In::PerNotePressure(2, 61, crate::midi::ControlValue::FULL))
        );
        assert_eq!(
            ev(EventBody::NoteOn2 { group: 0, channel: 0, note: 60, velocity: u16::MAX, attribute_type: 0, attribute: 0 }),
            Some(In::NoteOnVelocity(0, 60, crate::midi::Velocity::from_midi2(u16::MAX)))
        );
        assert_eq!(ev(EventBody::NoteOn { group: 0, channel: 3, note: 60, velocity: 0 }), Some(In::Midi1ZeroNoteOff(3, 60)));
        assert_eq!(ev(EventBody::PerNoteCC { group: 0, channel: 0, note: 60, cc: 7, value: 0, registered: false }), Some(In::PerNoteControl(0, 60, crate::midi::NoteControl::Assignable { index:7, value:0 })));
    }

    #[test]
    fn mpe_following_transposed_child_freezes_before_its_release_wait() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Loop, Zone as SampleZone},
            ksp::{LogEngine, Runtime}, modulation::{ModAssignment, ModSource, ModTarget}};
        let setup = |generated| {
            let group = Group { mods: vec![ModAssignment { cc_default: None, name: "CC74_VOLUME".into(),
                source: ModSource::MidiCc(74), target: ModTarget::Volume, intensity: 0.5,
                invert: false, lag_ms: 0, shaper: None }], ..Group::default() };
            let groups = vec![group.clone(), Group { release_trigger: true, ..group }];
            let path = std::path::PathBuf::from("child-tone");
            let zones = (0..2).map(|group| SampleZone { group, sample: path.clone(),
                loop_range: Some(Loop { start: 0, end: 1024, alternating: false, until_release: false, crossfade: 0 }),
                ..SampleZone::default() }).collect();
            let bank = Bank::from_samples(groups, zones, vec![(path, Sample { rate: 48000,
                frames: (0..1024).map(|i| [(i as f32 * 0.07).sin() * 0.2; 2]).collect() })]).unwrap();
            let scripts = [if generated { "on note\nignore_event($EVENT_ID)\nplay_note($EVENT_NOTE+12,$EVENT_VELOCITY,0,-1)\nend on" }
                else { "on note\nchange_note($EVENT_ID,$EVENT_NOTE+12)\nend on" },
                "on note\ndisallow_group($ALL_GROUPS)\nallow_group(0)\nend on\non release\nignore_event($EVENT_ID)\nwait(40000)\ndisallow_group($ALL_GROUPS)\nallow_group(1)\nnote_off($EVENT_ID)\nend on"];
            let (rt, errors) = Runtime::with_scripts(&scripts, &mut LogEngine::new(Vec::new(),48000.),8,Vec::new());
            assert!(errors.iter().all(Option::is_none),"{errors:?}");
            let mut e = Engine::default(); e.reset(48000.); e.attack=0.0001; e.release=0.001;
            e.set_bank(Some(Box::new(bank))); e.set_script(Some(Box::new(rt)));
            (e,router(&Articulate::default(),&Mpe { zone: Zone::Lower,..Mpe::default() }))
        };
        let start = |e: &mut Engine,r: &mut Router,cc,gain,pan,tune| {
            feed(r,e,In::Cc(1,74,cc),0); feed(r,e,In::NoteOn(1,60,100),0);
            feed(r,e,In::NoteGain(1,60,gain),0); feed(r,e,In::NotePan(1,60,pan),0);
            feed(r,e,In::NoteTune(1,60,tune),0);
        };
        let compare = |a: &mut Engine,o: &mut Engine,n: &mut Engine,phase| {
            let audio = |e: &mut Engine| { let (mut l,mut r)=([0.;128],[0.;128]); e.render(&mut l,&mut r); (l,r) };
            let (a,o,n)=(audio(a),audio(o),audio(n));
            for i in 0..128 { assert!((a.0[i]-o.0[i]-n.0[i]).abs()<1e-5 && (a.1[i]-o.1[i]-n.1[i]).abs()<1e-5,
                "{phase}, frame {i}: following transposed child inherited a reused member"); }
        };
        for rendered in [false,true] {
            let (mut a,mut ar)=setup(true); let (mut o,mut or)=setup(false); let (mut n,mut nr)=setup(false);
            start(&mut a,&mut ar,32,0.25,-1.,7.); start(&mut o,&mut or,32,0.25,-1.,7.);
            if rendered {
                compare(&mut a,&mut o,&mut n,"child initial physical expression");
                for (e,r) in [(&mut a,&mut ar),(&mut o,&mut or)] {
                    feed(r,e,In::NoteGain(1,60,0.5),0); feed(r,e,In::NotePan(1,60,-0.5),0);
                    feed(r,e,In::NoteTune(1,60,3.),0);
                }
                compare(&mut a,&mut o,&mut n,"child live physical expression");
            }
            for (e,r) in [(&mut a,&mut ar),(&mut o,&mut or)] {
                feed(r,e,In::Cc(0,64,127),0); feed(r,e,In::NoteOff(1,60),0);
            }
            start(&mut a,&mut ar,96,0.75,1.,-5.); start(&mut n,&mut nr,96,0.75,1.,-5.);
            compare(&mut a,&mut o,&mut n,"child waits after physical-off");
            for _ in 0..16 { compare(&mut a,&mut o,&mut n,"child delayed release under sustain"); }
            for (e,r) in [(&mut a,&mut ar),(&mut o,&mut or),(&mut n,&mut nr)] { feed(r,e,In::Cc(0,64,0),0); }
            compare(&mut a,&mut o,&mut n,"child release sample");
            assert!(a.voice_census().iter().any(|v|v.release_trigger),"child release sample must start");
        }
    }

    #[test]
    fn mpe_script_transposition_keeps_physical_expression_and_release_counter() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Loop, Zone as SampleZone}, modulation::{ModAssignment, ModSource, ModTarget}};
        let setup = |transpose| {
            let raw = ModAssignment { cc_default: None, name: "CC74_VOLUME".into(), source: ModSource::MidiCc(74),
                target: ModTarget::Volume, intensity: 0.5, invert: false, lag_ms: 0, shaper: None };
            let groups = vec![Group { mods: vec![raw.clone()], ..Group::default() },
                Group { release_trigger: true, release_counter_ms: 1000, mods: vec![raw,
                    ModAssignment { cc_default: None, name: "RTC_VOLUME".into(), source: ModSource::ReleaseTriggerCounter,
                        target: ModTarget::Volume, intensity: 1., invert: false, lag_ms: 0, shaper: None }], ..Group::default() }];
            let path = std::path::PathBuf::from("tone");
            let zones = (0..2).map(|group| SampleZone { group, sample: path.clone(),
                loop_range: Some(Loop { start: 0, end: 1024, alternating: false, until_release: false, crossfade: 0 }),
                ..SampleZone::default() }).collect();
            let bank = Bank::from_samples(groups, zones, vec![(path, Sample { rate: 48000,
                frames: (0..1024).map(|i| [(i as f32 * 0.07).sin() * 0.2; 2]).collect() })]).unwrap();
            let rt = runtime(&format!("on note\nchange_note($EVENT_ID,$EVENT_NOTE+{transpose})\ndisallow_group($ALL_GROUPS)\nallow_group(0)\nend on\non release\nignore_event($EVENT_ID)\nwait(40000)\ndisallow_group($ALL_GROUPS)\nallow_group(1)\nnote_off($EVENT_ID)\nend on"));
            let mut e = Engine::default(); e.reset(48000.); e.attack = 0.0001; e.release = 0.001;
            e.set_bank(Some(Box::new(bank))); e.set_script(Some(Box::new(rt)));
            (e, router(&Articulate::default(), &Mpe { zone: Zone::Lower, ..Mpe::default() }))
        };
        let start = |e: &mut Engine, r: &mut Router, key, cc, gain, pan, tune| {
            feed(r, e, In::Cc(1, 74, cc), 0);
            feed(r, e, In::NoteOn(1, key, 100), 0);
            feed(r, e, In::NoteGain(1, key, gain), 0);
            feed(r, e, In::NotePan(1, key, pan), 0);
            feed(r, e, In::NoteTune(1, key, tune), 0);
        };
        let advance = |e: &mut Engine, frames: usize| {
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            let mut left = frames;
            while left > 0 { let n = left.min(128); e.render(&mut l[..n], &mut r[..n]); left -= n; }
        };
        let compare = |a: &mut Engine, o: &mut Engine, n: &mut Engine, phase| {
            let audio = |e: &mut Engine| { let (mut l, mut r) = ([0.;128],[0.;128]); e.render(&mut l,&mut r); (l,r) };
            let (a,o,n) = (audio(a),audio(o),audio(n));
            for i in 0..128 { assert!((a.0[i]-o.0[i]-n.0[i]).abs()<1e-5 && (a.1[i]-o.1[i]-n.1[i]).abs()<1e-5,
                "{phase}, frame {i}: script transposition lost physical expression/counter identity"); }
        };
        for held in [0, 4800] {
            let (mut a, mut ar) = setup(12);
            let (mut o, mut or) = setup(0);
            let (mut n, mut nr) = setup(0);
            start(&mut a,&mut ar,60,32,0.25,-1.,7.);
            start(&mut o,&mut or,72,32,0.25,-1.,7.);
            for e in [&mut a,&mut o,&mut n] { advance(e,held); }
            if held > 0 { compare(&mut a,&mut o,&mut n,"held"); }
            for (e,r,key) in [(&mut a,&mut ar,60),(&mut o,&mut or,72)] {
                feed(r,e,In::Cc(0,64,127),0); feed(r,e,In::NoteOff(1,key),0);
            }
            start(&mut a,&mut ar,60,96,0.75,1.,-5.);
            start(&mut n,&mut nr,72,96,0.75,1.,-5.);
            for e in [&mut a,&mut o,&mut n] { advance(e,2400); }
            compare(&mut a,&mut o,&mut n,"pedal held after delayed callback");
            for (e,r) in [(&mut a,&mut ar),(&mut o,&mut or),(&mut n,&mut nr)] { feed(r,e,In::Cc(0,64,0),0); }
            compare(&mut a,&mut o,&mut n,"release sample");
            let gain = |e: &Engine| e.voice_census().into_iter().find(|v|v.release_trigger).expect("release sample").gain;
            assert!((gain(&a)-gain(&o)).abs()<1e-5,"delayed script release retimed transposed note's counter");
        }
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn settings_survive_the_host_state() {
        let mut a = areia();
        a.mode = Mode::Velocity;
        a.articulations[2].remap = Some(100);
        let mpe = Mpe { zone: Zone::Upper, ..Mpe::default() };
        let mut buf = Vec::new();
        a.write_field(&mut buf);
        mpe.write_field(&mut buf);
        let mut c = StateCursor::new(&buf);
        assert_eq!(Articulate::read_field(&mut c), Some(a));
        assert_eq!(Mpe::read_field(&mut c), Some(mpe));
    }
}

#[cfg(test)]
mod real {
    use super::*;

    #[test]
    #[ignore = "requires the owner's local Areia library"]
    #[cfg(feature = "plugin")]
    fn areia_same_pitch_channel_retriggers_leave_no_stuck_sustain() {
        let path = format!("{}/Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/07 Areia - Full Ens - Core Techniques.nki", crate::import::LIBRARY_ROOT);
        let i = crate::import::read(std::path::Path::new(&path)).unwrap();
        let mut e = crate::timing::engine_for(&i, 48000.0, crate::engine::MEMORY_LIMIT).unwrap();
        let found = crate::timing::found(&i, &e);
        let mut a = Articulate::default();
        a.sync(&path, &found);
        a.mode = Mode::Channel;
        let row = |name: &str| a.articulations.iter().position(|a| a.name.contains(name)).unwrap();
        let channels = [row("Sustained (NV-V)"), row("Spiccato Fast"), row("Spiccato Slow")].map(|n| a.articulations[n].channel);
        let mut r = Router::default();
        r.set_route(Route::new(&path, &a, &Mpe::default()));
        e.cc(0, 1, 100);
        for note in [65, 64, 62] {
            feed(&mut r, &mut e, In::NoteOn(channels[0], note, 100), 0);
            step(&mut e, 0.1);
            let sustained = held(&e, note);
            assert!(!sustained.is_empty(), "note {note} must sound before testing its release");
            for channel in channels[1..].iter().copied().cycle().take(12) {
                feed(&mut r, &mut e, In::NoteOn(channel, note, 100), 0);
                step(&mut e, 0.04);
                feed(&mut r, &mut e, In::NoteOff(channel, note), 0);
                step(&mut e, 0.04);
            }
            assert!(held(&e, note).iter().any(|g| sustained.contains(g)), "the short notes must leave sustain playing");
            feed(&mut r, &mut e, In::NoteOff(channels[0], note), 0);
            step(&mut e, 0.3);
            let stuck: Vec<_> = e.voice_census().into_iter().filter(|v| v.note == note && !v.released && !v.release_trigger && sustained.contains(&v.group)).collect();
            assert!(stuck.is_empty(), "note {note}: sustain still held after every input released: {stuck:?}");
        }
        let diagnostics = e.script().unwrap().diagnostics();
        println!("Areia runtime diagnostics: {diagnostics:?}");
        assert!(!diagnostics.iter().any(|d| d.contains("exhausted") || d.contains("instruction budget")), "{diagnostics:?}");
    }

    fn step(e: &mut Engine, seconds: f64) {
        let (mut l, mut r) = ([0f32; 256], [0f32; 256]);
        for _ in 0..(seconds * e.rate()) as usize / 256 {
            e.render(&mut l, &mut r);
        }
    }

    /// Groups of the voices on `note` still held.
    fn held(e: &Engine, note: u8) -> Vec<u32> {
        let mut g: Vec<u32> = (e.voice_census().iter()).filter(|v| v.note == note && !v.released && !v.release_trigger).map(|v| v.group).collect();
        g.sort_unstable();
        g
    }

    /// Solo Cello in channel mode: pizzicato, spiccato and legato at once on
    /// one key each play their own groups, and the short notes' releases
    /// leave the legato alone.
    #[test]
    #[ignore = "requires the owner's local Solo library"]
    #[cfg(feature = "plugin")]
    fn solo_cello_plays_three_articulations_on_three_channels() {
        let path = format!("{}/Solo/Instruments/01 Multi Patches/Solo - 03 Solo Cello.nki", crate::import::LIBRARY_ROOT);
        let i = crate::import::read(std::path::Path::new(&path)).unwrap();
        let part = |found: &[Found]| {
            let mut e = crate::timing::engine_for(&i, 48000.0, crate::engine::MEMORY_LIMIT).unwrap();
            step(&mut e, 0.3);
            let mut a = Articulate::default();
            a.sync(&path, found);
            a.mode = Mode::Channel;
            let mut r = Router::default();
            r.set_route(Route::new(&path, &a, &Mpe::default()));
            (e, r, a)
        };
        let found = crate::timing::found(&i, &part(&[]).0);
        let row = |name: &str| found.iter().position(|f| f.0.contains(name)).unwrap();
        let arts = [row("Pizzicato"), row("Spiccato"), row("Legato")];
        // Each alone, as a reference.
        let alone: Vec<Vec<u32>> = (arts.iter())
            .map(|&n| {
                let (mut e, mut r, a) = part(&found);
                feed(&mut r, &mut e, In::NoteOn(a.articulations[n].channel, 50, 100), 0);
                step(&mut e, 0.1);
                held(&e, 50)
            })
            .collect();
        assert!(alone.iter().all(|g| !g.is_empty()));
        // Round robins move a group by one or two: compare by fives.
        let family = |g: &[u32]| g.iter().map(|g| g / 5).collect::<std::collections::BTreeSet<_>>();
        let (mut e, mut r, a) = part(&found);
        for &n in &arts {
            feed(&mut r, &mut e, In::NoteOn(a.articulations[n].channel, 50, 100), 0);
        }
        step(&mut e, 0.1);
        assert_eq!(family(&held(&e, 50)), family(&alone.concat()));
        // Pizzicato and spiccato end; the legato plays on.
        for &n in &arts[..2] {
            feed(&mut r, &mut e, In::NoteOff(a.articulations[n].channel, 50), 0);
        }
        step(&mut e, 0.1);
        assert!(alone[2].iter().all(|g| held(&e, 50).contains(g)), "the legato was cut");
    }

    /// Solo Cello in channel mode over eight quantized beats: a legato line,
    /// spiccato and pizzicato on the beat, in either order. Each note plays
    /// the groups it plays with its channel alone (round robins aside).
    #[test]
    #[ignore = "requires the owner's local Solo library"]
    fn solo_cello_keeps_three_lines_apart() {
        let path = format!("{}/Solo/Instruments/01 Multi Patches/Solo - 03 Solo Cello.nki", crate::import::LIBRARY_ROOT);
        let i = crate::import::read(std::path::Path::new(&path)).unwrap();
        let fresh = || {
            let mut e = crate::timing::engine_for(&i, 48000.0, crate::engine::MEMORY_LIMIT).unwrap();
            step(&mut e, 0.3);
            e
        };
        let found = crate::timing::found(&i, &fresh());
        let mut a = Articulate::default();
        a.sync(&path, &found);
        a.mode = Mode::Channel;
        let row = |name: &str| found.iter().position(|f| f.0.contains(name)).unwrap();
        let groups: Vec<&str> = i.groups.iter().map(|g| g.name.split("RR").next().unwrap()).collect();
        let lines = [
            (row("Legato"), [50u8, 52, 53, 55, 57, 55, 53, 52]),
            (row("Spiccato"), [62, 62, 64, 65, 62, 62, 64, 65]),
            (row("Pizzicato"), [38, 43, 38, 43, 38, 43, 38, 43]),
        ];
        // Per beat and line: the groups its new note plays.
        let play = |order: &[usize]| {
            let mut e = fresh();
            let mut r = Router::default();
            r.set_route(Route::new(&path, &a, &Mpe::default()));
            let ch = |l: usize| a.articulations[lines[l].0].channel;
            let mut heard = vec![[const { Vec::new() }; 3]; 8];
            for beat in 0..8 {
                for &l in order.iter().filter(|&&l| l != 0 && beat > 0) {
                    feed(&mut r, &mut e, In::NoteOff(ch(l), lines[l].1[beat - 1]), 0);
                }
                for &l in order {
                    feed(&mut r, &mut e, In::NoteOn(ch(l), lines[l].1[beat], 100), 0);
                }
                // The legato's last note lets go just after the next began.
                step(&mut e, 0.03);
                if order.contains(&0) && beat > 0 {
                    feed(&mut r, &mut e, In::NoteOff(ch(0), lines[0].1[beat - 1]), 0);
                }
                step(&mut e, 0.07);
                for (v, &l) in e.voice_census().iter().flat_map(|v| order.iter().map(move |l| (v, l))) {
                    if v.note == lines[l].1[beat] && !v.released && !v.release_trigger {
                        heard[beat][l].push(groups[v.group as usize]);
                    }
                }
                for g in &mut heard[beat] {
                    g.sort_unstable();
                    g.dedup();
                }
                step(&mut e, 0.4);
            }
            heard
        };
        let alone: Vec<_> = (0..3).map(|l| play(&[l])).collect();
        for order in [[0, 1, 2], [2, 1, 0]] {
            let together = play(&order);
            for (beat, notes) in together.iter().enumerate() {
                for l in 0..3 {
                    assert!(!notes[l].is_empty(), "{order:?}: line {l} beat {beat} silent");
                    assert_eq!(notes[l], alone[l][beat][l], "{order:?}: line {l} beat {beat}");
                }
            }
        }
    }
}

#[cfg(all(test, feature = "plugin"))]
#[path = "articulate_mpe_expression_tests.rs"]
mod mpe_expression_tests;

#[cfg(all(test, feature = "plugin"))]
mod negotiated_mpe_tests {
    use super::*;
    use crate::{audio::Sample, engine::{Bank, HostNote, HostPattern}, import::{Group, Zone as SampleZone}};

    fn engine(scripted: bool) -> Engine {
        let bank = Bank::from_samples(vec![Group::default()], vec![SampleZone::default()],
            vec![(std::path::PathBuf::new(), Sample { rate: 48000,
                frames: (0..64000).map(|i| [i as f32 / 64000.; 2]).collect() })]).unwrap();
        let mut e = Engine::default();
        e.set_bank(Some(Box::new(bank)));
        if scripted {
            let (runtime, errors) = crate::ksp::Runtime::with_scripts(&["on init\nend on"],
                &mut crate::ksp::LogEngine::new(Vec::new(), 48000.), 8, Vec::new());
            assert!(errors.iter().all(Option::is_none), "{errors:?}");
            e.set_script(Some(Box::new(runtime)));
        }
        e
    }

    fn parameter(r: &mut Router, e: &mut Engine, channel: u8, address: u8, coarse: u8, fine: Option<u8>) {
        for (cc, value) in [(101, 0), (100, address), (6, coarse)] {
            feed(r, e, In::Cc(channel, cc, value), 0);
        }
        if let Some(fine) = fine { feed(r, e, In::Cc(channel, 38, fine), 0); }
    }

    fn render(e: &mut Engine, frames: usize) {
        e.render(&mut [0.; 1024][..frames], &mut [0.; 1024][..frames]);
        e.mark_host_notes();
    }

    #[test]
    fn negotiated_dual_zones_render_independent_ranges_and_block_partitions_without_heap() {
        for scripted in [false, true] {
            let (mut actual, mut split, mut expected) = (engine(scripted), engine(scripted), engine(scripted));
            let (mut r, mut sr) = (Router::default(), Router::default());
            assert_eq!(crate::plugin::tests::allocations(|| {
                for (e, r) in [(&mut actual, &mut r), (&mut split, &mut sr)] {
                    parameter(r, e, 0, 6, 3, None);
                    parameter(r, e, 15, 6, 3, None);
                    for (channel, coarse, fine) in [(1, 12, 50), (14, 7, 25), (0, 6, 25), (15, 3, 50)] {
                        parameter(r, e, channel, 0, coarse, Some(fine));
                    }
                    for (channel, bend) in [(0, 12288), (1, 10240), (15, 4096), (14, 12288)] {
                        feed(r, e, In::Bend(channel, bend), 0);
                    }
                    for (channel, key) in [(1, 60), (14, 64)] { feed(r, e, In::NoteOn(channel, key, 100), 0); }
                }
                for (channel, key, tune) in [(1, 60, 6.25), (14, 64, 1.875)] {
                    expected.set_expression_on(channel, key, Expression { tune, ..Expression::default() });
                    expected.note_on(channel, key, 100);
                }
            }), 0);
            assert_eq!(r.route.mpe.zone, Zone::Off, "received negotiation must not rewrite the authored route");
            assert_eq!(actual.mpe_zones().member(1).unwrap().manager, 0);
            assert_eq!(actual.mpe_zones().member(14).unwrap().manager, 15);
            let (mut a, mut s, mut x, mut right) = ([0.; 1024], [0.; 1024], [0.; 1024], [0.; 1024]);
            assert_eq!(crate::plugin::tests::allocations(|| {
                actual.render(&mut a, &mut right);
                expected.render(&mut x, &mut right);
                let mut at = 0;
                for n in [1, 7, 31, 127, 13, 64].into_iter().cycle() {
                    let n = n.min(s.len() - at);
                    split.render(&mut s[at..at+n], &mut right[at..at+n]);
                    at += n;
                    if at == s.len() { break; }
                }
            }), 0);
            assert!(a.iter().any(|x| x.abs() > 0.001));
            assert!(a.iter().zip(x).all(|(a, x)| (a-x).abs() < 1e-5), "scripted={scripted}: zones lost their independent negotiated pitch");
            assert!(a.iter().zip(s).all(|(a, s)| (a-s).abs() < 1e-6), "scripted={scripted}: block partition changed MPE audio");
        }
    }

    #[test]
    fn negotiated_membership_reassignment_cleans_pedal_held_owners_and_reuses_channels() {
        for scripted in [false, true] {
            let (mut e, mut r) = (engine(scripted), Router::default());
            let old = HostNote { port: 0, channel: 12, key: 65, id: 12, clap: true };
            let pattern = HostPattern { port: 0, channel: 12, key: 65, id: 12, clap: true };
            assert_eq!(crate::plugin::tests::allocations(|| {
                parameter(&mut r, &mut e, 0, 6, 3, None);
                parameter(&mut r, &mut e, 15, 6, 3, None);
                feed(&mut r, &mut e, In::Cc(15, 64, 127), 0);
                feed(&mut r, &mut e, In::Bend(12, 12288), 0);
                feed(&mut r, &mut e, In::Pressure(12, 37), 0);
                feed(&mut r, &mut e, In::HostOn(old, crate::midi::Velocity::from_midi1(100), 0.), 0);
                for (channel, key) in [(1, 60), (14, 69)] { feed(&mut r, &mut e, In::NoteOn(channel, key, 100), 0); }
                render(&mut e, 128);
                feed(&mut r, &mut e, In::HostOff(pattern), 0);
                render(&mut e, 128);
            }), 0);
            assert!(e.host_note_pending(old), "pedal-held owner disappeared before reconfiguration");
            assert_eq!(crate::plugin::tests::allocations(|| {
                parameter(&mut r, &mut e, 0, 6, 12, None);
                render(&mut e, 1024);
            }), 0);
            assert_eq!(e.mpe_zones().member(12).unwrap().manager, 0);
            assert!(!e.host_note_pending(old), "old physical root survived MPE reassignment");
            let ended = (0..).map_while(|index| e.host_note_at(index)).find(|(note, _)| *note == old);
            assert_eq!(ended, Some((old, false)), "ended exact address must be available to plugin NOTE_END retirement");
            e.retire_host_note(old);
            assert!(!e.host_note_present(old));
            assert!(e.key_down(1, 60) && e.key_down(14, 69), "unaffected zones lost their held notes");
            assert_eq!(crate::plugin::tests::allocations(|| {
                parameter(&mut r, &mut e, 0, 6, 0, None);
                render(&mut e, 1024);
            }), 0);
            assert!(!e.key_down(1, 60) && e.key_down(14, 69), "disabling lower zone also stopped the upper zone");
            let fresh = HostNote { id: 13, ..old };
            assert_eq!(crate::plugin::tests::allocations(|| {
                parameter(&mut r, &mut e, 0, 6, 12, None);
                feed(&mut r, &mut e, In::HostOn(fresh, crate::midi::Velocity::from_midi1(100), 0.), 0);
                feed(&mut r, &mut e, In::HostOff(pattern), 0);
                render(&mut e, 128);
            }), 0);
            let voices = e.voice_census();
            assert_eq!(voices.len(), 2, "old notes or queued descendants survived reconfiguration");
            let reused = voices.iter().find(|v| v.channel == 12).unwrap();
            assert!(reused.held && !reused.released);
            assert!((reused.step - 2f64.powf(5. / 12.)).abs() < 1e-6, "reused channel inherited its previous bend");
            assert_eq!(e.cc_state()[12][64], 0, "reused member inherited old manager's pedal");
            assert_eq!(r.member_pressure[12], None);
        }
    }

    #[test]
    fn negotiated_reassignment_cancels_old_waits_but_keeps_other_physical_and_authored_work() {
        let source = "on init\ndeclare ui_slider $old_cc(0,100)\ndeclare ui_slider $other_cc(0,100)\ndeclare ui_slider $other_key(0,1)\ndeclare ui_button $authored\ndeclare ui_slider $authored_done(0,100)\nend on\non note\nif ($MIDI_CHANNEL=12)\nignore_event($EVENT_ID)\nwait(5000)\nplay_note(72,100,0,-1)\nend if\nend on\non release\nif ($MIDI_CHANNEL=12)\n$other_key := %KEY_DOWN[69]\nwait(1000)\nplay_note(73,100,0,0)\nend if\nend on\non controller\nif (($CC_NUM=1) or ($CC_NUM=74) or ($CC_NUM=7) or ($CC_NUM=10))\nif ($MIDI_CHANNEL=12)\nwait(5000)\ninc($old_cc)\nelse\nif ($MIDI_CHANNEL=14)\nwait(5000)\ninc($other_cc)\nend if\nend if\nend if\nend on\non ui_control($authored)\nwait(5000)\ninc($authored_done)\nend on";
        let (runtime, errors) = crate::ksp::Runtime::with_scripts(&[source],
            &mut crate::ksp::LogEngine::new(Vec::new(),48000.),8,Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        let (mut e, mut r) = (engine(false), Router::default());
        e.set_script(Some(Box::new(runtime)));
        let old = HostNote { port:0, channel:12, key:65, id:20, clap:true };
        assert_eq!(crate::plugin::tests::allocations(|| {
            parameter(&mut r, &mut e, 0, 6, 3, None);
            parameter(&mut r, &mut e, 15, 6, 3, None);
            feed(&mut r,&mut e,In::NoteOn(14,69,100),0);
            feed(&mut r,&mut e,In::HostOn(old,crate::midi::Velocity::from_midi1(100),0.),0);
            for input in [12,14] { feed(&mut r,&mut e,In::Cc(input,1,input),0); }
            for cc in [74,7,10] { feed(&mut r,&mut e,In::Cc(12,cc,17),0); }
            for (cc,value) in [(101,0),(100,2)] { feed(&mut r,&mut e,In::Cc(12,cc,value),0); }
            e.ui_control(0,3,1);
            render(&mut e,64);
            parameter(&mut r,&mut e,0,6,12,None);
            assert_eq!(e.receive_controller(12,6,7).and_then(|received| received.parameter),None,"reassigned member retained old parameter selection");
            render(&mut e,1024);
        }),0);
        let controls=&e.script().unwrap().interface(0).controls;
        for (index,value) in [(0,0),(1,1),(2,1),(4,1)] {
            assert_eq!(controls[index].properties["$CONTROL_PAR_VALUE"],crate::ksp::Value::Int(value),
                "control {index}: scoped MPE cleanup lost or retained the wrong callback");
        }
        assert!(!e.host_note_pending(old));
        assert!(e.key_down(14,69));
        assert!(e.voice_census().iter().all(|voice| voice.note==69),"old waiting note or cleanup release created a voice");
        assert!(e.script().unwrap().diagnostics().is_empty());
    }

    #[test]
    fn full_upper_negotiation_reassigns_lower_manager_and_channel_modes_use_the_new_zone() {
        for scripted in [false,true] {
            let (mut e,mut r)=(engine(scripted),Router::default());
            assert_eq!(crate::plugin::tests::allocations(|| {
                parameter(&mut r,&mut e,0,6,3,None);
                parameter(&mut r,&mut e,15,6,3,None);
                feed(&mut r,&mut e,In::Bend(0,12288),0);
                for (channel,key) in [(0,60),(1,62),(14,69)] {
                    feed(&mut r,&mut e,In::NoteOn(channel,key,100),0);
                }
                render(&mut e,128);
                parameter(&mut r,&mut e,15,6,15,None);
                render(&mut e,1024);
            }),0);
            assert_eq!(e.mpe_zones().manager(0),None);
            assert_eq!(e.mpe_zones().member(0).unwrap().manager,15);
            assert!(!e.key_down(0,60) && !e.key_down(1,62));
            assert!(e.key_down(14,69),"extending upper zone stopped its unchanged member");
            assert_eq!(crate::plugin::tests::allocations(|| {
                feed(&mut r,&mut e,In::NoteOn(0,65,100),0);
                render(&mut e,128);
            }),0);
            let voices=e.voice_census();
            let former_manager=voices.iter().find(|voice|voice.channel==0).unwrap();
            assert!((former_manager.step-2f64.powf(5./12.)).abs()<1e-6,"former manager retained its previous bend after becoming a member");
            assert_eq!(crate::plugin::tests::allocations(|| {
                feed(&mut r,&mut e,In::Cc(0,123,0),0);
                render(&mut e,128);
            }),0);
            assert!(!e.key_down(0,65) && e.key_down(14,69),"former manager stop still acted on its old zone");
            assert_eq!(crate::plugin::tests::allocations(|| {
                feed(&mut r,&mut e,In::Cc(15,123,0),0);
                render(&mut e,128);
            }),0);
            assert!(!e.key_down(14,69),"active upper manager stop did not reach its members");
        }
    }
}
