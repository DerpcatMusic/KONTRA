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

use crate::engine::{Engine, Expression, PartControls, RACK_SLOTS, Rack};
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

    fn member(&self, channel: u8) -> bool {
        self.zone().is_some_and(|(_, members)| members.contains(&channel))
    }

    fn master(&self, channel: u8) -> bool {
        self.zone().is_some_and(|(master, _)| master == channel)
    }

    /// Channel mode, unless MPE owns the channels.
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
    NoteOn(u8, u8, u8),
    NoteOff(u8, u8),
    Cc(u8, u8, u8),
    /// 0..=16383, centre 8192.
    Bend(u8, u16),
    Pressure(u8, u8),
    PolyAt(u8, u8, u8),
    /// Host note expressions, by channel and key: semitones, pressure
    /// (0..=127), linear gain, pan (−1..=1), brightness (0..=127).
    NoteTune(u8, u8, f32),
    NotePressure(u8, u8, u8),
    NoteGain(u8, u8, f32),
    NotePan(u8, u8, f32),
    NoteBrightness(u8, u8, u8),
}

impl In {
    /// A host event as a part takes it. Per-note MIDI 2.0 bodies (CLAP note
    /// expressions) stay per key; the rest of MIDI 2.0 narrows to MIDI 1.0.
    #[cfg(feature = "plugin")]
    pub fn from_event(body: &EventBody) -> Option<Self> {
        let unit = |v: u32| (f64::from(v) / f64::from(u32::MAX)) as f32;
        let hi7 = |v: u32| (v >> 25) as u8;
        Some(match *body {
            EventBody::PerNotePitchBend { channel, note, value, .. } => {
                Self::NoteTune(channel, note, midi::per_note_bend_semitones(value) as f32)
            }
            EventBody::PolyPressure2 { channel, note, pressure, .. } => {
                Self::NotePressure(channel, note, hi7(pressure))
            }
            EventBody::PerNoteCC { channel, note, cc: 7, value, registered: true, .. } => {
                Self::NoteGain(channel, note, unit(value) * midi::PER_NOTE_VOLUME_MAX_GAIN as f32)
            }
            EventBody::PerNoteCC { channel, note, cc: 10, value, registered: true, .. } => {
                Self::NotePan(channel, note, unit(value) * 2.0 - 1.0)
            }
            EventBody::PerNoteCC { channel, note, cc: 74, value, registered: true, .. } => {
                Self::NoteBrightness(channel, note, hi7(value))
            }
            EventBody::NoteOn { channel, note, velocity: 0, .. } => Self::NoteOff(channel, note),
            EventBody::NoteOn { channel, note, velocity, .. } => Self::NoteOn(channel, note, velocity),
            EventBody::NoteOff { channel, note, .. } => Self::NoteOff(channel, note),
            EventBody::ControlChange { channel, cc, value, .. } => Self::Cc(channel, cc, value),
            EventBody::PitchBend { channel, value, .. } => Self::Bend(channel, value),
            EventBody::ChannelPressure { channel, pressure, .. } => Self::Pressure(channel, pressure),
            EventBody::Aftertouch { channel, note, pressure, .. } => {
                Self::PolyAt(channel, note, pressure)
            }
            _ => return Self::from_event(&midi::downconvert_to_midi1(body)?),
        })
    }

    fn channel(self) -> u8 {
        match self {
            Self::NoteOn(c, ..)
            | Self::NoteOff(c, _)
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
    NoteOn(u8, u8, u8),
    NoteOff(u8, u8),
    /// Script channel, physical input channel, key, velocity.
    NoteOnFrom(u8, u8, u8, u8),
    NoteOffFrom(u8, u8, u8),
    Cc(u8, u8, u8),
    Bend(u8, u16),
    Pressure(u8, u8),
    PolyAt(u8, u8, u8),
    /// A script control set (script slot, control, value), as a click on it.
    Control(usize, usize, i32),
    Expression(u8, u8, Expression),
}

/// MIDI state of the member channels' RPNs: parameter number, being set.
#[derive(Clone, Copy, Default)]
struct Rpn {
    msb: u8,
    lsb: u8,
}

/// One rack slot's pre-script MIDI layer on the audio thread.
pub struct Router {
    pub route: Route,
    /// The articulation the scripts were last switched to, as far as known.
    current: Option<usize>,
    /// Per input channel and key: the channel and key the engine got.
    held: [[(u8, u8); 128]; 16],
    /// MPE: member channels' pitch bend and the live bend range.
    bend: [u16; 16],
    bend_range: u8,
    rpn: [Rpn; 16],
    /// Per channel and key as the engine got them.
    expression: Box<[[Expression; 128]; 16]>,
    brightness: [u8; 128],
    /// The part's scripts: they take pressure per note (`on poly_at`), and
    /// channel controllers once, not per member channel.
    scripted: bool,
    handles_pressure: bool,
    /// The scripts [`Self::current`] is known for, by address: new ones
    /// (a reload, restored values) start at the library's own articulation.
    script: usize,
}

impl Default for Router {
    fn default() -> Self {
        let route = Route::default();
        Self {
            bend_range: route.mpe.bend_range,
            route,
            current: None,
            held: [[(NONE, NONE); 128]; 16],
            bend: [8192; 16],
            rpn: [Rpn { msb: 127, lsb: 127 }; 16],
            expression: Box::new([[Expression::default(); 128]; 16]),
            brightness: [NONE; 128],
            scripted: false,
            handles_pressure: false,
            script: 0,
        }
    }
}

/// Pressure as volume: −12 dB at none, unity at full.
fn pressure_gain(pressure: u8) -> f32 {
    10f32.powf((f32::from(pressure.min(127)) / 127.0 - 1.0) * 12.0 / 20.0)
}

impl Router {
    /// Take a new route; held notes keep where they went.
    pub fn set_route(&mut self, route: Route) {
        if route != self.route {
            if route.mpe.bend_range != self.route.mpe.bend_range {
                self.bend_range = route.mpe.bend_range;
            }
            self.route = route;
            self.forget();
        }
    }

    /// Each channel plays its own articulation.
    pub fn by_channel(&self) -> bool {
        self.route.by_channel()
    }

    /// The scripts' articulation may have changed some other way (a click):
    /// switch again before the next note.
    pub fn forget(&mut self) {
        self.current = None;
    }

    /// Whether the part takes `channel` on `port`: its own channel, a zone's,
    /// or one an articulation listens on.
    pub fn hears(&self, c: &PartControls, port: u8, channel: u8) -> bool {
        let r = &self.route;
        c.port == port
            && (c.channel < 0
                || c.channel == i16::from(channel)
                || r.zone().is_some_and(|(master, members)| master == channel || members.contains(&channel))
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

    /// The key the engine got for `note` held on `channel`, or its remap.
    fn key_of(&self, channel: u8, note: u8) -> u8 {
        match self.held[channel as usize & 15][note as usize & 127] {
            (_, NONE) => self.route.keys[note as usize & 127],
            (_, key) => key,
        }
    }

    fn member_tune(&self, channel: u8) -> f32 {
        (f32::from(self.bend[channel as usize & 15]) - 8192.0) / 8192.0 * f32::from(self.bend_range)
    }

    fn pressure(&mut self, channel: u8, key: u8, value: u8, out: &mut impl FnMut(Out)) {
        out(Out::PolyAt(channel, key, value));
        if !self.handles_pressure {
            self.set_expression(channel, key, |x| x.gain = pressure_gain(value), out);
        }
    }

    /// Route one input to the part's engine: `home` is the part's channel,
    /// where channel mode plays everything.
    pub fn input(&mut self, ev: In, home: u8, out: &mut impl FnMut(Out)) {
        let r = self.route;
        let channel = ev.channel();
        let to = if r.by_channel() { home & 15 } else { channel };
        match ev {
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
                    }
                    self.switch(a, to, out);
                }
                // A new note on a key starts from its channel's expression.
                let tune = if r.member(channel) { self.member_tune(channel) } else { 0.0 };
                self.brightness[key as usize] = NONE;
                self.set_expression(to, key, |x| *x = Expression { tune, ..Expression::default() }, out);
                self.held[channel as usize][note as usize & 127] = (to, key);
                if r.by_channel() && self.scripted {
                    out(Out::NoteOnFrom(to, channel, key, velocity));
                } else {
                    out(Out::NoteOn(to, key, velocity));
                }
            }
            In::NoteOff(_, note) => {
                let (to, key) = match std::mem::replace(&mut self.held[channel as usize][note as usize & 127], (NONE, NONE)) {
                    (_, NONE) => (to, r.keys[note as usize & 127]),
                    held => held,
                };
                if key == NONE {
                    return;
                }
                if r.by_channel() && self.scripted {
                    out(Out::NoteOffFrom(to, channel, key));
                    return;
                }
                // Unscripted notes still share the engine's channel/key pair.
                // ponytail: use event ownership for unscripted channel mode too.
                let shared = (0..16).any(|c| c != channel as usize && self.held[c][note as usize & 127] == (to, key));
                if !shared {
                    out(Out::NoteOff(to, key));
                }
            }
            In::Bend(_, value) if r.member(channel) => {
                self.bend[channel as usize] = value.min(16383);
                let tune = self.member_tune(channel);
                let row = self.held[channel as usize];
                for (to, key) in row.into_iter().filter(|h| h.1 != NONE) {
                    self.set_expression(to, key, |x| x.tune = tune, out);
                }
            }
            In::Bend(_, value) => {
                out(Out::Bend(to, value));
                // Unscripted voices bend by their own channel: the master's
                // bend reaches every member.
                if r.master(channel) && !self.scripted {
                    for m in r.zone().map(|z| z.1).into_iter().flatten() {
                        out(Out::Bend(m, value));
                    }
                }
            }
            In::Pressure(_, value) if r.member(channel) => {
                let row = self.held[channel as usize];
                for (to, key) in row.into_iter().filter(|h| h.1 != NONE) {
                    self.pressure(to, key, value, out);
                }
                if !self.scripted {
                    out(Out::Pressure(channel, value));
                }
            }
            In::Pressure(_, value) => out(Out::Pressure(to, value)),
            In::PolyAt(_, note, value) => out(Out::PolyAt(to, self.key_of(channel, note), value)),
            In::Cc(_, 123, _) if r.by_channel() && self.scripted => {
                // Channel-mode scripts share a home channel, but a host stop
                // belongs to the physical channel that sent it.
                for (to, key) in std::mem::replace(&mut self.held[channel as usize], [(NONE, NONE); 128]) {
                    if key != NONE {
                        out(Out::NoteOffFrom(to, channel, key));
                    }
                }
            }
            In::Cc(_, cc, value) => {
                if r.zone().is_some() {
                    self.rpn_cc(channel, cc, value);
                }
                if cc == 74 && r.member(channel) {
                    let row = self.held[channel as usize];
                    for (_, key) in row.into_iter().filter(|h| h.1 != NONE) {
                        self.brightness[key as usize] = value;
                    }
                }
                out(Out::Cc(to, cc, value));
                if r.master(channel) && !self.scripted && !matches!(cc, 64 | 66 | 120 | 121 | 123) {
                    for m in r.zone().map(|z| z.1).into_iter().flatten() {
                        out(Out::Cc(m, cc, value));
                    }
                }
            }
            In::NoteTune(_, note, semitones) => {
                let key = self.key_of(channel, note);
                if key != NONE {
                    self.set_expression(to, key, |x| x.tune = semitones, out);
                }
            }
            In::NotePressure(_, note, value) => {
                let key = self.key_of(channel, note);
                if key != NONE {
                    self.pressure(to, key, value, out);
                }
            }
            In::NoteGain(_, note, gain) => {
                let key = self.key_of(channel, note);
                if key != NONE {
                    self.set_expression(to, key, |x| x.gain = gain.clamp(0.0, 4.0), out);
                }
            }
            In::NotePan(_, note, pan) => {
                let key = self.key_of(channel, note);
                if key != NONE {
                    self.set_expression(to, key, |x| x.pan = pan.clamp(-1.0, 1.0), out);
                }
            }
            In::NoteBrightness(_, note, value) => {
                let key = self.key_of(channel, note);
                if key != NONE {
                    self.brightness[key as usize] = value;
                    out(Out::Cc(to, 74, value));
                }
            }
        }
    }

    /// Follow RPN 0 (pitch bend range) on member channels and the MPE
    /// configuration message (RPN 6) on the master channel.
    fn rpn_cc(&mut self, channel: u8, cc: u8, value: u8) {
        let rpn = &mut self.rpn[channel as usize];
        match cc {
            101 => rpn.msb = value,
            100 => rpn.lsb = value,
            6 => match (rpn.msb, rpn.lsb) {
                (0, 0) if self.route.member(channel) => self.bend_range = value.clamp(1, 96),
                (0, 6) if self.route.master(channel) => {
                    if value == 0 {
                        self.route.mpe.zone = Zone::Off;
                    } else {
                        self.route.mpe.members = value.min(15);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// Apply what a router sends to its engine.
pub(crate) fn apply(e: &mut Engine, o: Out) {
    match o {
        Out::NoteOn(c, n, v) => e.note_on(c, n, v),
        Out::NoteOff(c, n) => e.note_off(c, n),
        Out::NoteOnFrom(c, owner, n, v) => e.note_on_from(c, owner, n, v),
        Out::NoteOffFrom(c, owner, n) => e.note_off_from(c, owner, n),
        Out::Cc(c, cc, v) => e.cc(c, cc, v),
        Out::Bend(c, v) => e.pitch_bend(c, v),
        Out::Pressure(c, v) => e.channel_pressure(c, v),
        Out::PolyAt(c, n, v) => e.poly_pressure(c, n, v),
        Out::Control(slot, control, value) => e.ui_control(slot, control, value),
        Out::Expression(c, n, x) => e.set_expression_on(c, n, x),
    }
}

/// Send `ev` through `r` to its part's engine `e`; notes in channel mode
/// arrive on `home`.
pub(crate) fn feed(r: &mut Router, e: &mut Engine, ev: In, home: u8) {
    r.follow(e);
    let zone = if r.by_channel() { None } else { r.route.zone() };
    e.set_mpe_zone(zone.map(|(master, members)| (master, members.fold(0u16, |mask, member| mask | (1 << member)))));
    r.input(ev, home, &mut |o| apply(e, o));
}

/// Send `ev`, from host MIDI port `port`, through each rack part's router
/// that takes it. Releases, pitch bend and controllers other than volume and
/// pan reach every part on the port, so a changed routing never sticks a
/// note or a pedal. Returns the slots it reached, one bit each.
pub fn dispatch(rack: &mut Rack, routers: &mut [Router; RACK_SLOTS], port: u8, ev: In) -> u32 {
    let reached = reach(rack, routers, port, ev);
    dispatch_to(rack, routers, reached, ev);
    reached
}

/// The slots [`dispatch`] sends `ev` to, one bit each.
pub fn reach(rack: &Rack, routers: &[Router; RACK_SLOTS], port: u8, ev: In) -> u32 {
    let wide = match ev {
        In::NoteOff(..) | In::Bend(..) => true,
        In::Cc(_, cc, _) => !matches!(cc, 7 | 10),
        _ => false,
    };
    let mut reached = 0;
    for (slot, (c, r)) in rack.controls.iter().zip(routers.iter()).enumerate() {
        if if wide { c.port == port } else { r.hears(c, port, ev.channel()) } {
            reached |= 1 << slot;
        }
    }
    reached
}

/// Send `ev` through the routers of the slots in `slots` (one bit each), as
/// [`dispatch`] would: a note-off reaches exactly the parts its note-on did.
pub fn dispatch_to(rack: &mut Rack, routers: &mut [Router; RACK_SLOTS], slots: u32, ev: In) {
    let Rack { parts, controls, .. } = rack;
    for (slot, ((e, c), r)) in parts.iter_mut().zip(controls.iter()).zip(routers.iter_mut()).enumerate() {
        if slots & 1 << slot != 0 {
            feed(r, e, ev, u8::try_from(c.channel).unwrap_or(0));
        }
    }
}

/// Send `ev` to rack slot `slot` alone (the on-screen keyboard), through its router.
pub fn play(rack: &mut Rack, routers: &mut [Router; RACK_SLOTS], slot: usize, ev: In) {
    let slot = slot.min(RACK_SLOTS - 1);
    feed(&mut routers[slot], &mut rack.parts[slot], ev, ev.channel());
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
                Out::NoteOn(0, 13, SWITCH_VELOCITY),
                Out::NoteOff(0, 13),
                Out::NoteOn(0, 60, 90),
                Out::NoteOn(0, 14, SWITCH_VELOCITY),
                Out::NoteOff(0, 14),
                Out::NoteOn(0, 64, 80),
                Out::NoteOn(0, 13, SWITCH_VELOCITY),
                Out::NoteOff(0, 13),
                Out::NoteOn(0, 67, 70),
            ]
        );
        // Releases go where their notes went.
        assert_eq!(run(&mut r, &[In::NoteOff(2, 64)]), [Out::NoteOff(0, 64)]);
        // The same articulation again needs no switch.
        assert_eq!(run(&mut r, &[In::NoteOn(1, 62, 50)]), [Out::NoteOn(0, 62, 50)]);
        // Something else may have switched: switch again.
        r.forget();
        assert_eq!(run(&mut r, &[In::NoteOn(1, 62, 50)])[0], Out::NoteOn(0, 13, SWITCH_VELOCITY));
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
        assert_eq!(run(&mut r, &[In::NoteOn(1, 60, 90)]), [Out::NoteOn(0, 60, 90)]);
        assert_eq!(run(&mut r, &[In::NoteOn(2, 62, 90)]), [Out::Control(1, 40, 1), Out::NoteOn(0, 62, 90)]);
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

    /// A synthetic MPE stream: lower zone, two notes on members 2 and 3,
    /// each bent, pressed and brightened on its own channel.
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
    fn mpe_lower_zone_expresses_each_note() {
        let mpe = Mpe { zone: Zone::Lower, ..Mpe::default() };
        let mut r = router(&Articulate::default(), &mpe);
        let mut out = Vec::new();
        let mut send = |r: &mut Router, ev| r.input(ev, 0, &mut |o| out.push(o));
        // Bend before the note, as MPE controllers do: the note starts bent.
        send(&mut r, In::Bend(1, 8192 + 4096));
        send(&mut r, In::NoteOn(1, 60, 100));
        send(&mut r, In::NoteOn(2, 64, 100));
        send(&mut r, In::Bend(2, 0));
        send(&mut r, In::Pressure(1, 127));
        send(&mut r, In::Cc(2, 74, 20));
        let x = |key| out.iter().rev().find_map(|o| match o {
            Out::Expression(_, k, x) if *k == key => Some(*x),
            _ => None,
        });
        assert_eq!(x(60).unwrap().tune, 24.0);
        assert_eq!(x(64).unwrap().tune, -48.0);
        // Pressure is poly aftertouch for the scripts and, unused by them, volume.
        assert!(out.contains(&Out::PolyAt(1, 60, 127)));
        assert!(!out.iter().any(|o| matches!(o, Out::PolyAt(_, 64, _))));
        assert_eq!(x(60).unwrap().gain, 1.0);
        assert!(out.contains(&Out::Cc(2, 74, 20)));
        // Member bends never reach the engine as channel bends.
        assert!(!out.iter().any(|o| matches!(o, Out::Bend(..))));
        assert!(r.cutoff_scale() < 0.1);
        // RPN 0 on a member sets the bend range.
        for (cc, v) in [(101, 0), (100, 0), (6, 12)] {
            r.input(In::Cc(3, cc, v), 0, &mut |_| {});
        }
        let mut last = None;
        r.input(In::Bend(1, 0), 0, &mut |o| last = Some(o));
        assert_eq!(last, Some(Out::Expression(1, 60, Expression { tune: -12.0, gain: 1.0, pan: 0.0 })));
        // Members are heard, other channels not, on a part set to channel 1.
        let c = PartControls { channel: 0, ..PartControls::default() };
        assert!(r.hears(&c, 0, 15) && r.hears(&c, 0, 3));
    }

    #[test]
    fn mpe_pressure_is_volume_only_when_unhandled() {
        let mpe = Mpe { zone: Zone::Upper, members: 4, ..Mpe::default() };
        let mut r = router(&Articulate::default(), &mpe);
        let mut out = Vec::new();
        r.input(In::NoteOn(14, 60, 100), 0, &mut |o| out.push(o));
        r.input(In::Pressure(14, 0), 0, &mut |o| out.push(o));
        let gain = out.iter().rev().find_map(|o| match o {
            Out::Expression(14, 60, x) => Some(x.gain),
            _ => None,
        });
        assert!((gain.unwrap() - 0.251).abs() < 0.01);
        // Channel 10 is outside a four-member upper zone.
        let c = PartControls { channel: 15, ..PartControls::default() };
        assert!(!r.hears(&c, 0, 10) && r.hears(&c, 0, 11));
        r.handles_pressure = true;
        out.clear();
        r.input(In::Pressure(14, 50), 0, &mut |o| out.push(o));
        assert_eq!(out, [Out::PolyAt(14, 60, 50), Out::Pressure(14, 50)]);
    }

    #[test]
    fn host_note_expressions_follow_the_key() {
        let mut r = Router::default();
        let mut out = Vec::new();
        r.input(In::NoteOn(0, 60, 100), 0, &mut |o| out.push(o));
        r.input(In::NoteTune(0, 60, -3.5), 0, &mut |o| out.push(o));
        r.input(In::NoteGain(0, 60, 0.5), 0, &mut |o| out.push(o));
        r.input(In::NotePan(0, 60, 0.25), 0, &mut |o| out.push(o));
        assert_eq!(out.last(), Some(&Out::Expression(0, 60, Expression { tune: -3.5, gain: 0.5, pan: 0.25 })));
        // The next note on the key starts plain.
        r.input(In::NoteOff(0, 60), 0, &mut |o| out.push(o));
        r.input(In::NoteOn(0, 60, 100), 0, &mut |o| out.push(o));
        assert!(out.contains(&Out::Expression(0, 60, Expression::default())));
    }

    #[test]
    #[cfg(feature = "plugin")]
    fn host_events_read_per_note_and_narrow_the_rest() {
        let tune = midi::per_note_bend_from_semitones(-3.5);
        let ev = |body| In::from_event(&body);
        assert_eq!(
            ev(EventBody::PerNotePitchBend { group: 0, channel: 1, note: 60, value: tune }),
            Some(In::NoteTune(1, 60, -3.5))
        );
        // CLAP volume: unity gain is the wire's quarter point.
        let gain = ev(EventBody::PerNoteCC { group: 0, channel: 0, note: 60, cc: 7, value: u32::MAX / 4, registered: true });
        assert!(matches!(gain, Some(In::NoteGain(0, 60, g)) if (g - 1.0).abs() < 1e-3));
        let pan = ev(EventBody::PerNoteCC { group: 0, channel: 0, note: 60, cc: 10, value: u32::MAX, registered: true });
        assert!(matches!(pan, Some(In::NotePan(0, 60, p)) if (p - 1.0).abs() < 1e-3));
        assert_eq!(
            ev(EventBody::PolyPressure2 { group: 0, channel: 2, note: 61, pressure: u32::MAX }),
            Some(In::NotePressure(2, 61, 127))
        );
        assert_eq!(
            ev(EventBody::NoteOn2 { group: 0, channel: 0, note: 60, velocity: u16::MAX, attribute_type: 0, attribute: 0 }),
            Some(In::NoteOn(0, 60, 127))
        );
        assert_eq!(ev(EventBody::NoteOn { group: 0, channel: 3, note: 60, velocity: 0 }), Some(In::NoteOff(3, 60)));
        assert_eq!(ev(EventBody::PerNoteCC { group: 0, channel: 0, note: 60, cc: 7, value: 0, registered: false }), None);
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
