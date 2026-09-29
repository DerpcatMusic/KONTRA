//! Sampler engine.
//!
//! Ownership: a [`Bank`] and its [`FxProcessor`] are built off the audio
//! thread. The bank is never mutated once handed to an [`Engine`];
//! [`Engine::set_bank`] and [`Engine::set_fx`] return what they replace so the
//! caller frees it off the audio thread. Everything reachable from
//! [`Engine::render`] and the event methods is free of allocation, locks, I/O
//! and panics; storage is preallocated in [`Engine::default`].
//!
//! A KSP [`Runtime`] is initialized off the audio thread (against a
//! [`ScriptSetup`]) and installed with [`Engine::set_script`]. From then on
//! MIDI input reaches voices only through it: its engine calls become
//! time-stamped commands that [`Engine::render`] applies at their frame.

mod bank;
mod map;
mod params;
mod rack;
mod script;
mod stream;
mod voice;

pub(crate) use bank::parallel;
pub use bank::{Bank, GroupSettings, LOAD_DONE, MEMORY_LIMIT, PRELOAD_FRAMES};
pub use params::{MAX_WRITES, Mod, ModTable, VOICE_MODS};
pub use rack::{BUSES, Block, PartControls, RACK_SLOTS, Rack};
pub use script::{MAX_COMMANDS, ScriptSetup, load_scripts};
pub use voice::{Ahdsr, Flex, FlexPoint};

use crate::fx::FxProcessor;
use crate::ksp::Runtime;
use map::FOREVER;
use params::{Address, GroupPar, Write};
use script::{Command, Host};
use stream::Slot;
use voice::{Context, Envelope, Fade, Scratch, Stream, Voice, balance};

/// Voice storage per engine. Polyphony limits steal before this is reached;
/// only a full store forces a hard cut.
pub const MAX_VOICES: usize = 1024;
/// Largest block rendered in one pass; longer requests are split.
pub const MAX_BLOCK: usize = 128;
/// Groups addressable by [`GroupMask`]: Kontakt's per-instrument ceiling.
pub const MAX_GROUPS: usize = 4096;
/// Fade applied to voices stolen by the instrument polyphony limit.
const STEAL_FADE: f32 = 0.005;

/// Identifies one note event and every voice it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize)]
pub struct EventId(pub u32);

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Groups a note may start, like KSP's allow_group/disallow_group.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GroupMask([u64; MAX_GROUPS / 64]);

impl GroupMask {
    pub const fn all() -> Self {
        Self([u64::MAX; MAX_GROUPS / 64])
    }

    pub const fn none() -> Self {
        Self([0; MAX_GROUPS / 64])
    }

    pub fn set(&mut self, group: usize, allowed: bool) {
        if let Some(word) = self.0.get_mut(group / 64) {
            let bit = 1 << (group % 64);
            if allowed { *word |= bit } else { *word &= !bit }
        }
    }

    pub fn contains(&self, group: usize) -> bool {
        self.0
            .get(group / 64)
            .is_some_and(|w| w & (1 << (group % 64)) != 0)
    }

    /// Allowed group indices below `count`.
    pub fn iter(&self, count: usize) -> impl Iterator<Item = usize> + '_ {
        (0..count.min(MAX_GROUPS)).filter(|&g| self.contains(g))
    }
}

impl std::fmt::Debug for GroupMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter(MAX_GROUPS)).finish()
    }
}

/// A note start with scripted parameters (KSP play_note and friends).
#[derive(Clone, Copy)]
pub struct NoteEvent<'a> {
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
    /// Groups to consider; `None` uses the engine's allow mask.
    pub groups: Option<&'a GroupMask>,
    /// Start offset in microseconds, bounded by each zone's start-mod range.
    pub offset_us: u64,
    /// Linear gain.
    pub volume: f32,
    /// Semitones.
    pub tune: f64,
    /// −1 (left) to 1 (right), added to zone and group pan.
    pub pan: f32,
}

impl NoteEvent<'_> {
    pub fn new(channel: u8, note: u8, velocity: u8) -> Self {
        Self {
            channel,
            note,
            velocity,
            groups: None,
            offset_us: 0,
            volume: 1.0,
            tune: 0.0,
            pan: 0.0,
        }
    }
}

/// Absolute parameter changes for a running event.
#[derive(Clone, Copy, Debug)]
pub enum EventChange {
    /// Linear gain.
    Volume(f32),
    /// Semitones.
    Tune(f64),
    Pan(f32),
}

/// One instrument's playback state around an immutable [`Bank`].
pub struct Engine {
    bank: Option<Box<Bank>>,
    /// Program effects, applied to the whole output after the output stage.
    fx: FxProcessor,
    player: Player,
    /// Instrument scripts; when present, MIDI reaches voices only through them.
    script: Option<Box<Runtime>>,
    /// Script engine calls for the next render, ordered by frame.
    commands: Vec<Command>,
    /// Script engine parameter changes for the next render, ordered by frame.
    writes: Vec<Write>,
    /// MIDI channel of the latest input routed to the script; its notes play there.
    script_channel: u8,
    /// Envelope attack (s) for groups without their own envelope.
    pub attack: f32,
    /// Envelope release (s) for groups without their own envelope.
    pub release: f32,
    /// Output low-pass ("Tone") cutoff in Hz; 20 kHz or more bypasses it.
    pub cutoff: f32,
    /// Offline rendering: wait for streamed data instead of playing silence.
    pub blocking_streams: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            bank: None,
            fx: FxProcessor::default(),
            player: Player::new(48000.0),
            script: None,
            commands: Vec::with_capacity(MAX_COMMANDS),
            writes: Vec::with_capacity(MAX_WRITES),
            script_channel: 0,
            attack: 0.002,
            release: 0.15,
            cutoff: 20000.0,
            blocking_streams: false,
        }
    }
}

impl Engine {
    /// Install `bank`, stopping every voice; returns the previous bank for
    /// disposal off the audio thread.
    pub fn set_bank(&mut self, bank: Option<Box<Bank>>) -> Option<Box<Bank>> {
        self.player.clear_voices(self.bank.as_deref());
        self.commands.clear();
        self.writes.clear();
        let old = std::mem::replace(&mut self.bank, bank);
        self.player.free.clear();
        let slots = self.bank.as_deref().map_or(0, |b| b.slots().len());
        self.player.free.extend((0..slots as u16).rev());
        self.replay(Address::is_group);
        old
    }

    /// Install the program effects, built for [`rate`](Self::rate) with
    /// blocks of [`MAX_BLOCK`]; returns the previous processor for disposal
    /// off the audio thread.
    pub fn set_fx(&mut self, fx: FxProcessor) -> FxProcessor {
        let old = std::mem::replace(&mut self.fx, fx);
        self.replay(|a| matches!(a, Address::Fx(..)));
        old
    }

    /// Install the instrument scripts, initialized off the audio thread; returns
    /// the previous runtime for disposal there. `None` plays MIDI directly.
    /// Engine parameters the scripts set in `on init` apply now, and again to
    /// banks and effects installed later.
    pub fn set_script(&mut self, mut script: Option<Box<Runtime>>) -> Option<Box<Runtime>> {
        self.commands.clear();
        self.writes.clear();
        if let Some(rt) = script.as_deref_mut() {
            rt.set_sample_rate(self.player.rate);
        }
        let old = std::mem::replace(&mut self.script, script);
        self.replay(|_| true);
        self.apply_init_controllers();
        old
    }

    /// Set the controllers the scripts set while loading, on every channel.
    fn apply_init_controllers(&mut self) {
        let Some(rt) = self.script.as_deref() else {
            return;
        };
        let defaults = self.defaults();
        for &(cc, value) in &rt.init_controllers {
            for channel in 0..16 {
                self.player
                    .cc(self.bank.as_deref(), channel, cc, value, defaults);
            }
        }
    }

    /// Apply the scripts' `on init` engine parameters whose address `only` accepts.
    fn replay(&mut self, only: fn(&Address) -> bool) {
        let Some(rt) = self.script.as_deref_mut() else {
            return;
        };
        // Moved out and back: no allocation.
        let pars = std::mem::take(&mut rt.init_engine_pars);
        for &(par, value) in &pars {
            let groups = self.bank.as_deref().map_or(&[][..], Bank::groups);
            if let Some(address) = Address::resolve(par, groups).filter(only) {
                self.write(address, address.decode(value));
            }
        }
        if let Some(rt) = self.script.as_deref_mut() {
            rt.init_engine_pars = pars;
        }
    }

    /// Apply one engine parameter; false when nothing installed holds it.
    fn write(&mut self, address: Address, value: f32) -> bool {
        match address {
            Address::Fx(rack, slot, param) => self.fx.set_param(rack, slot, param, value),
            Address::Instrument(p) => {
                let (volume, pan, tune) = &mut self.player.instrument;
                match p {
                    GroupPar::Volume => *volume = value.max(0.0),
                    GroupPar::Pan => *pan = value.clamp(-1.0, 1.0),
                    GroupPar::Tune => *tune = value,
                    GroupPar::Output => return false,
                }
                true
            }
            _ => self
                .bank
                .as_deref_mut()
                .is_some_and(|bank| params::write(&mut bank.settings, address, value)),
        }
    }

    pub fn script(&self) -> Option<&Runtime> {
        self.script.as_deref()
    }

    pub fn bank(&self) -> Option<&Bank> {
        self.bank.as_deref()
    }

    pub fn rate(&self) -> f64 {
        self.player.rate
    }

    /// Stop all voices, silence effect tails and reset MIDI state; keeps the
    /// bank, effects, scripts and group mask. Effects stay built for their own
    /// rate: replace them with [`set_fx`](Self::set_fx) when `rate` changes.
    pub fn reset(&mut self, rate: f64) {
        self.player.clear_voices(self.bank.as_deref());
        self.commands.clear();
        self.writes.clear();
        self.fx.clear();
        self.player.reset_midi();
        self.player.rate = rate;
        if let Some(rt) = self.script.as_deref_mut() {
            rt.set_sample_rate(rate);
        }
        self.apply_init_controllers();
    }

    pub fn active_voices(&self) -> usize {
        self.player.voices.len()
    }

    /// Streamed frames that were not ready in time (played as silence).
    pub fn underruns(&self) -> u64 {
        self.player.underruns
    }

    /// Script engine calls dropped because the command queue was full.
    pub fn dropped_commands(&self) -> u64 {
        self.player.dropped_commands
    }

    /// The runtime and its engine view, borrowed apart so scripts can drive voices.
    fn scripted(&mut self, channel: u8) -> Option<(&mut Runtime, Host<'_>)> {
        let rt = self.script.as_deref_mut()?;
        self.script_channel = channel;
        let host = Host {
            bank: self.bank.as_deref(),
            fx: &self.fx,
            player: &mut self.player,
            commands: &mut self.commands,
            writes: &mut self.writes,
        };
        Some((rt, host))
    }

    /// Note input takes effect at the start of the next [`render`](Self::render).
    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if channel >= 16 || note >= 128 {
            return;
        }
        if velocity == 0 {
            return self.note_off(channel, note);
        }
        self.player.keys[channel as usize][note as usize] = velocity.min(127);
        if let Some((rt, mut host)) = self.scripted(channel) {
            return rt.note_on(&mut host, 0, note, velocity.min(127));
        }
        self.player.pedal_releases[channel as usize][note as usize] = 0;
        self.start_event(&NoteEvent::new(channel, note, velocity));
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        if channel >= 16 || note >= 128 {
            return;
        }
        if self.script.is_some() {
            self.player.keys[channel as usize][note as usize] = 0;
            if let Some((rt, mut host)) = self.scripted(channel) {
                rt.note_off(&mut host, 0, note);
            }
            return;
        }
        let defaults = self.defaults();
        if let Some(bank) = self.bank.as_deref() {
            self.player.note_off(bank, channel, note, defaults);
        } else {
            self.player.keys[channel as usize][note as usize] = 0;
        }
    }

    /// Controllers pass through the scripts; channel mode messages (120 and up)
    /// act on the engine directly so a script can never swallow a panic.
    pub fn cc(&mut self, channel: u8, cc: u8, value: u8) {
        if channel >= 16 || cc >= 128 {
            return;
        }
        if self.script.is_some() {
            if cc < 120 {
                if let Some((rt, mut host)) = self.scripted(channel) {
                    rt.controller(&mut host, 0, cc, value.min(127));
                }
                return;
            }
            if cc == 123 {
                for note in 0..128 {
                    if self.key_down(channel, note) {
                        self.note_off(channel, note);
                    }
                }
                return;
            }
        }
        let defaults = self.defaults();
        self.player
            .cc(self.bank.as_deref(), channel, cc, value, defaults);
    }

    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        if channel >= 16 {
            return;
        }
        let value = value.min(16383);
        if let Some((rt, mut host)) = self.scripted(channel) {
            return rt.pitch_bend(&mut host, 0, i32::from(value) - 8192);
        }
        self.player.bend[channel as usize] = (f32::from(value) - 8192.0) / 8192.0;
    }

    /// Channel pressure (mono aftertouch modulation), through the scripts.
    pub fn channel_pressure(&mut self, channel: u8, value: u8) {
        let (channel, value) = (channel.min(15), value.min(127));
        if let Some((rt, mut host)) = self.scripted(channel) {
            return rt.channel_pressure(&mut host, 0, value);
        }
        self.player.pressure[channel as usize] = value;
    }

    /// A host edit of script control `control` in script slot `slot`: sets its
    /// value and runs the script's `on ui_control`.
    pub fn ui_control(&mut self, slot: usize, control: usize, value: i32) {
        let channel = self.script_channel;
        if let Some((rt, mut host)) = self.scripted(channel) {
            rt.ui_control(&mut host, slot, control, value);
        }
    }

    /// Polyphonic key pressure; only scripts react to it.
    pub fn poly_pressure(&mut self, channel: u8, note: u8, value: u8) {
        if let Some((rt, mut host)) = self.scripted(channel.min(15)) {
            rt.poly_pressure(&mut host, 0, note, value.min(127));
        }
    }

    /// Start every eligible zone for `event`; the id addresses all its voices.
    /// Returns `None` only for invalid input or when no bank is loaded.
    pub fn start_event(&mut self, event: &NoteEvent) -> Option<EventId> {
        let defaults = self.defaults();
        let bank = self.bank.as_deref()?;
        let id = self.player.next_id();
        self.player.start(bank, event, id, false, defaults)
    }

    /// Release the event's voices (note-off by id), firing release triggers.
    pub fn release_event(&mut self, id: EventId) {
        let defaults = self.defaults();
        let Some(bank) = self.bank.as_deref() else {
            return;
        };
        if let Some((channel, note, velocity, false)) = self.player.release_voices(bank, id) {
            let allowed = self.player.allowed;
            let key = (channel, note, velocity);
            self.player.trigger_release(bank, key, &allowed, defaults);
        }
    }

    /// Ramp the event's voices to `level` over `seconds`; `stop` ends them at silence.
    pub fn fade_event(&mut self, id: EventId, seconds: f32, level: f32, stop: bool) {
        let frames = self.player.frames(seconds);
        for v in self.player.voices.iter_mut().filter(|v| v.event == id) {
            v.fade.start(level.max(0.0), frames, stop);
        }
    }

    pub fn change_event(&mut self, id: EventId, change: EventChange) {
        self.player.change_event(id, change);
    }

    pub fn event_active(&self, id: EventId) -> bool {
        self.player.voices.iter().any(|v| v.event == id)
    }

    /// Allow or disallow one group for notes without an explicit mask.
    pub fn set_group_allowed(&mut self, group: usize, allowed: bool) {
        self.player.allowed.set(group, allowed);
    }

    pub fn set_all_groups_allowed(&mut self, allowed: bool) {
        self.player.allowed = if allowed {
            GroupMask::all()
        } else {
            GroupMask::none()
        };
    }

    pub fn allowed_groups(&self) -> &GroupMask {
        &self.player.allowed
    }

    /// Last value of every controller per channel (KSP `%CC`).
    pub fn cc_state(&self) -> &[[u8; 128]; 16] {
        &self.player.cc
    }

    /// Whether a key is physically down (KSP `%KEY_DOWN`).
    pub fn key_down(&self, channel: u8, note: u8) -> bool {
        self.player
            .keys
            .get(channel as usize)
            .and_then(|k| k.get(note as usize))
            .is_some_and(|&v| v > 0)
    }

    /// Render `left.len()` frames, overwriting both buffers. Scripts advance
    /// first; their commands then split voice rendering at their exact frames.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        left.fill(0.0);
        right.fill(0.0);
        let channel = self.script_channel;
        if let Some((rt, mut host)) = self.scripted(channel) {
            rt.process(&mut host, n as u32);
        }
        let defaults = self.defaults();
        let (mut next, mut written) = (0, 0);
        for (block, (l, r)) in left[..n]
            .chunks_mut(MAX_BLOCK)
            .zip(right[..n].chunks_mut(MAX_BLOCK))
            .enumerate()
        {
            let (base, len) = (block * MAX_BLOCK, l.len());
            let mut pos = 0;
            loop {
                // Parameters first: they configure notes started at the same frame.
                while let Some(&w) = self
                    .writes
                    .get(written)
                    .filter(|w| w.at as usize <= base + pos)
                {
                    self.write(w.address, w.value);
                    written += 1;
                }
                while let Some(c) = self
                    .commands
                    .get(next)
                    .filter(|c| c.at as usize <= base + pos)
                {
                    if let Some(bank) = self.bank.as_deref() {
                        self.player.apply(bank, c, channel, defaults);
                    }
                    next += 1;
                }
                if pos == len {
                    break;
                }
                let due = [
                    self.commands.get(next).map(|c| c.at),
                    self.writes.get(written).map(|w| w.at),
                ];
                let end = due
                    .into_iter()
                    .flatten()
                    .fold(len, |end, at| end.min(at as usize - base));
                if let Some(bank) = self.bank.as_deref() {
                    let blocking = self.blocking_streams;
                    let out = (&mut l[pos..end], &mut r[pos..end]);
                    self.player.render(bank, out, &mut self.fx, pos, blocking);
                }
                pos = end;
            }
            self.fx.mix_buses(l, r);
            self.player.output(l, r, self.cutoff);
            // Runs without voices too, so reverb and convolution tails ring out.
            self.fx.process(l, r);
        }
        // Only an empty render leaves changes behind: apply them now.
        for i in written..self.writes.len() {
            let w = self.writes[i];
            self.write(w.address, w.value);
        }
        if let Some(bank) = self.bank.as_deref() {
            for c in &self.commands[next..] {
                self.player.apply(bank, c, channel, defaults);
            }
        }
        self.commands.clear();
        self.writes.clear();
    }

    fn defaults(&self) -> Ahdsr {
        Ahdsr {
            attack: self.attack,
            curve: 0.0,
            hold: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: self.release,
        }
    }
}

/// Engine state apart from the bank, so voices can mutate while the bank is borrowed.
struct Player {
    voices: Vec<Voice>,
    /// Free stream slots of the current bank.
    free: Vec<u16>,
    /// Zones matched by the current note start, with crossfade gains.
    pending: Vec<(u32, f32)>,
    scratch: Scratch,
    rate: f64,
    sustain: [bool; 16],
    bend: [f32; 16],
    cc: [[u8; 128]; 16],
    /// Channel pressure.
    pressure: [u8; 16],
    /// Velocity of keys that are down.
    keys: [[u8; 128]; 16],
    /// Release triggers deferred by the sustain pedal: note-on velocity.
    pedal_releases: [[u8; 128]; 16],
    allowed: GroupMask,
    next_event: u32,
    clock: u64,
    /// Instrument volume (CC7) and pan (CC10).
    volume: f32,
    pan: f32,
    /// Instrument volume (linear), pan and tune (semitones) set by scripts.
    instrument: (f32, f32, f32),
    out_gains: [f32; 2],
    tone: [f32; 2],
    underruns: u64,
    dropped_commands: u64,
}

impl Player {
    fn new(rate: f64) -> Self {
        let mut player = Self {
            voices: Vec::with_capacity(MAX_VOICES),
            free: Vec::with_capacity(stream::SLOTS),
            pending: Vec::with_capacity(MAX_VOICES),
            scratch: Scratch::default(),
            rate,
            sustain: [false; 16],
            bend: [0.0; 16],
            cc: [[0; 128]; 16],
            pressure: [0; 16],
            keys: [[0; 128]; 16],
            pedal_releases: [[0; 128]; 16],
            allowed: GroupMask::all(),
            next_event: 0,
            clock: 0,
            volume: 1.0,
            pan: 0.0,
            instrument: (1.0, 0.0, 0.0),
            out_gains: [1.0; 2],
            tone: [0.0; 2],
            underruns: 0,
            dropped_commands: 0,
        };
        player.reset_midi();
        player
    }

    fn reset_midi(&mut self) {
        self.sustain = [false; 16];
        self.bend = [0.0; 16];
        self.pressure = [0; 16];
        self.cc = [[0; 128]; 16];
        for cc in &mut self.cc {
            (cc[7], cc[10], cc[11]) = (127, 64, 127);
        }
        self.keys = [[0; 128]; 16];
        self.pedal_releases = [[0; 128]; 16];
        (self.volume, self.pan) = (1.0, 0.0);
        self.tone = [0.0; 2];
    }

    fn clear_voices(&mut self, bank: Option<&Bank>) {
        let slots = bank.map_or(&[][..], Bank::slots);
        for v in self.voices.drain(..) {
            if let Some(stream) = v.stream {
                slots[stream.slot as usize].stop();
                self.free.push(stream.slot);
            }
        }
    }

    fn remove(&mut self, bank: &Bank, index: usize) {
        let v = self.voices.swap_remove(index);
        if let Some(stream) = v.stream {
            bank.slots()[stream.slot as usize].stop();
            self.free.push(stream.slot);
        }
    }

    fn fade_frames(&self, seconds: f32) -> u32 {
        self.frames(seconds).max(1)
    }

    fn frames(&self, seconds: f32) -> u32 {
        (seconds.max(0.0) * self.rate as f32) as u32
    }

    fn next_id(&mut self) -> EventId {
        self.next_event = self.next_event.wrapping_add(1).max(1);
        EventId(self.next_event)
    }

    fn change_event(&mut self, id: EventId, change: EventChange) {
        for v in self.voices.iter_mut().filter(|v| v.event == id) {
            match change {
                EventChange::Volume(gain) => v.volume = gain.max(0.0),
                EventChange::Tune(semitones) => v.tune = 2f64.powf(semitones / 12.0),
                EventChange::Pan(pan) => v.pan = pan,
            }
        }
    }

    /// Start `ev` under the caller-assigned `id`.
    fn start(
        &mut self,
        bank: &Bank,
        ev: &NoteEvent,
        id: EventId,
        release_trigger: bool,
        defaults: Ahdsr,
    ) -> Option<EventId> {
        if ev.channel >= 16 || ev.note >= 128 || !(1..=127).contains(&ev.velocity) {
            return None;
        }
        self.clock += 1;
        let mask = ev.groups.unwrap_or(&self.allowed);
        self.pending.clear();
        for &z in bank.zones_on(ev.note) {
            let zone = &bank.zones()[z as usize];
            let group = &bank.groups()[zone.group];
            let eligible = bank.playable[zone.group]
                && group.release_trigger == release_trigger
                && (group.channel < 0 || group.channel == i16::from(ev.channel))
                && mask.contains(zone.group)
                && (zone.low_velocity..=zone.high_velocity).contains(&ev.velocity);
            if eligible && self.pending.len() < self.pending.capacity() {
                let gain = edge_gain(
                    ev.velocity,
                    zone.low_velocity,
                    zone.high_velocity,
                    zone.fade_low_velocity,
                    zone.fade_high_velocity,
                ) * edge_gain(
                    ev.note,
                    zone.low_key,
                    zone.high_key,
                    zone.fade_low_key,
                    zone.fade_high_key,
                );
                self.pending.push((z, gain));
            }
        }
        for i in 0..self.pending.len() {
            let (zone, gain) = self.pending[i];
            self.spawn(bank, zone, ev, id, gain, release_trigger, defaults);
        }
        Some(id)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(
        &mut self,
        bank: &Bank,
        z: u32,
        ev: &NoteEvent,
        event: EventId,
        gain: f32,
        release_trigger: bool,
        defaults: Ahdsr,
    ) {
        let zone = &bank.zones()[z as usize];
        let play = &bank.plays[z as usize];
        let group = &bank.groups()[zone.group];
        let settings = &bank.settings[zone.group];
        let sample = &bank.samples[play.sample as usize];
        self.make_room(bank, settings.voice_group);

        let key = if group.key_tracking {
            2f64.powf((f64::from(ev.note) - f64::from(zone.root)) / 12.0)
        } else {
            1.0
        };
        let step = f64::from(sample.rate) / self.rate * zone.tune * key;
        let c = ev.channel as usize;
        let inputs = params::Inputs {
            cc: &self.cc[c],
            bend: self.bend[c],
            pressure: self.pressure[c],
            note: ev.note,
            velocity: ev.velocity,
        };
        let mods = settings.mods.start(&inputs);
        let modulated = (settings.mods.start_offset(&inputs) * play.start_mod as f32) as u64;
        let offset = ((ev.offset_us as f64 * f64::from(sample.rate) / 1e6) as u64 + modulated)
            .min(play.start_mod);
        // A release-triggered voice starts with its key already up.
        let wraps = play
            .map
            .wraps(if release_trigger { offset } else { FOREVER });
        let span = &sample.spans[play.span as usize];
        let limit = play.map.resident_limit(wraps, span.start, span.end());
        let stream = if sample.streamed {
            self.free.pop()
        } else {
            None
        }
        .map(|slot| {
            if limit == FOREVER {
                // Reserved for a loop that may end on release.
                Stream {
                    slot,
                    tag: 0,
                    trusted: 0,
                }
            } else {
                let tag = bank.slots()[slot as usize].configure(
                    play.sample,
                    &play.map,
                    wraps,
                    limit,
                    offset,
                );
                Stream {
                    slot,
                    tag,
                    trusted: limit,
                }
            }
        });
        let base_level = zone.gain * gain;
        let envelope = settings.envelope.unwrap_or(if settings.flex.is_some() {
            Ahdsr::UNITY
        } else {
            defaults
        });
        let mut voice = Voice {
            event,
            group: zone.group as u32,
            voice_group: settings.voice_group,
            channel: ev.channel,
            note: ev.note,
            velocity: ev.velocity,
            held: !release_trigger,
            released: false,
            release_trigger,
            age: self.clock,
            sample: play.sample,
            span: play.span,
            map: play.map,
            wraps,
            length: play.map.len(wraps),
            limit,
            pos: offset as f64,
            step,
            tune: 2f64.powf(ev.tune / 12.0),
            pitch: (f32::NAN, 1.0),
            mods,
            stream,
            env: Envelope::new(&envelope, self.rate as f32),
            flex: settings.flex.as_ref().map(|_| Envelope::flex()),
            fade: Fade::FULL,
            base_level,
            volume: ev.volume.max(0.0),
            base_pan: zone.pan,
            pan: ev.pan,
            gains: [0.0; 2],
        };
        // Start at the voice's first-block gains, so it does not ramp in.
        let (modulation, _) = settings.mods.modulate(&mut voice.mods, &inputs, 0, 1.0);
        let pan = (zone.pan + settings.pan + ev.pan).clamp(-1.0, 1.0);
        voice.gains = balance(base_level * settings.gain * modulation * voice.volume, pan);
        self.voices.push(voice);
    }

    /// Enforce voice-group, exclusion and instrument limits before a new voice.
    fn make_room(&mut self, bank: &Bank, voice_group: Option<u16>) {
        let limits = &bank.voice_groups;
        let rules = voice_group.and_then(|g| Some((g, limits.get(g as usize)?.as_ref()?)));
        if let Some((group, rule)) = rules {
            if rule.exclusion >= 0 {
                for v in &mut self.voices {
                    let Some(other) = v.voice_group.filter(|&o| o != group) else {
                        continue;
                    };
                    if let Some(Some(o)) = limits.get(other as usize)
                        && o.exclusion == rule.exclusion
                        && !v.fade.dying()
                    {
                        v.fade
                            .start(0.0, ((o.fade * self.rate as f32) as u32).max(1), true);
                    }
                }
            }
            let live = self
                .voices
                .iter()
                .filter(|v| v.voice_group == Some(group) && !v.fade.dying())
                .count();
            if live >= rule.max_voices {
                let fade = self.fade_frames(rule.fade);
                if let Some(i) = victim(
                    &self.voices,
                    |v| v.voice_group == Some(group),
                    rule.kill_mode,
                    rule.prefer_released,
                ) {
                    self.voices[i].fade.start(0.0, fade, true);
                }
            }
        }
        if self.voices.iter().filter(|v| !v.fade.dying()).count() >= bank.polyphony {
            let fade = self.fade_frames(STEAL_FADE);
            if let Some(i) = victim(&self.voices, |_| true, 1, true) {
                self.voices[i].fade.start(0.0, fade, true);
            }
        }
        if self.voices.len() == MAX_VOICES {
            // Storage is full of fading voices: cut the quietest.
            let quietest = (0..self.voices.len()).min_by(|&a, &b| {
                let level = |i: usize| (!self.voices[i].fade.dying(), self.voices[i].fade.value());
                level(a)
                    .partial_cmp(&level(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(i) = quietest {
                self.remove(bank, i);
            }
        }
    }

    fn note_off(&mut self, bank: &Bank, channel: u8, note: u8, defaults: Ahdsr) {
        if channel >= 16 || note >= 128 {
            return;
        }
        let (c, n) = (channel as usize, note as usize);
        let velocity = std::mem::take(&mut self.keys[c][n]);
        let sustained = self.sustain[c];
        for v in &mut self.voices {
            if v.channel == channel && v.note == note && v.held && !v.release_trigger {
                v.held = false;
                if !sustained {
                    v.release(bank, &mut self.free);
                }
            }
        }
        if velocity > 0 {
            if sustained {
                self.pedal_releases[c][n] = velocity;
            } else {
                let id = self.next_id();
                let event = NoteEvent::new(channel, note, velocity);
                self.start(bank, &event, id, true, defaults);
            }
        }
    }

    /// Release one event's voices; a held sustain pedal defers them like a key
    /// release. Returns the first voice's channel, note, velocity and whether
    /// it is itself a release trigger.
    fn release_voices(&mut self, bank: &Bank, id: EventId) -> Option<(u8, u8, u8, bool)> {
        let mut first = None;
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.event == id && !v.released)
        {
            first.get_or_insert((v.channel, v.note, v.velocity, v.release_trigger));
            if self.sustain[v.channel as usize] {
                v.held = false;
            } else {
                v.release(bank, &mut self.free);
            }
        }
        first
    }

    /// Start the release-trigger zones for a key release, limited to `groups`;
    /// a held sustain pedal defers them to pedal-up (with the engine's groups).
    fn trigger_release(
        &mut self,
        bank: &Bank,
        (channel, note, velocity): (u8, u8, u8),
        groups: &GroupMask,
        defaults: Ahdsr,
    ) {
        if self.sustain.get(channel as usize) == Some(&true) {
            self.pedal_releases[channel as usize][note as usize] = velocity;
            return;
        }
        let id = self.next_id();
        let event = NoteEvent {
            groups: Some(groups),
            ..NoteEvent::new(channel, note, velocity)
        };
        self.start(bank, &event, id, true, defaults);
    }

    fn cc(&mut self, bank: Option<&Bank>, channel: u8, cc: u8, value: u8, defaults: Ahdsr) {
        if channel >= 16 || cc >= 128 {
            return;
        }
        let c = channel as usize;
        let value = value.min(127);
        self.cc[c][cc as usize] = value;
        match cc {
            // General MIDI volume curve: 127 is unity.
            7 => self.volume = (f32::from(value) / 127.0).powi(2),
            10 => self.pan = ((f32::from(value) - 64.0) / 63.0).clamp(-1.0, 1.0),
            64 => {
                let on = value >= 64;
                if self.sustain[c] && !on {
                    self.sustain[c] = false;
                    if let Some(bank) = bank {
                        self.pedal_up(bank, channel, defaults);
                    }
                }
                self.sustain[c] = on;
            }
            120 => {
                let slots = bank.map_or(&[][..], Bank::slots);
                let free = &mut self.free;
                self.voices.retain(|v| {
                    let keep = v.channel != channel;
                    if let (false, Some(stream)) = (keep, v.stream) {
                        slots[stream.slot as usize].stop();
                        free.push(stream.slot);
                    }
                    keep
                });
            }
            121 => {
                self.cc(bank, channel, 64, 0, defaults);
                self.bend[c] = 0.0;
                self.cc[c][1] = 0;
                self.cc[c][11] = 127;
            }
            123 => {
                if let Some(bank) = bank {
                    for note in 0..128 {
                        self.note_off(bank, channel, note, defaults);
                    }
                }
            }
            _ => {}
        }
    }

    fn pedal_up(&mut self, bank: &Bank, channel: u8, defaults: Ahdsr) {
        for v in &mut self.voices {
            if v.channel == channel && !v.held && !v.released && !v.release_trigger {
                v.release(bank, &mut self.free);
            }
        }
        for note in 0..128u8 {
            let velocity =
                std::mem::take(&mut self.pedal_releases[channel as usize][note as usize]);
            if velocity > 0 {
                let id = self.next_id();
                let event = NoteEvent::new(channel, note, velocity);
                self.start(bank, &event, id, true, defaults);
            }
        }
    }

    /// Render every voice into `left`/`right`, or into its group's bus input
    /// at frame `offset` of the current block.
    fn render(
        &mut self,
        bank: &Bank,
        (left, right): (&mut [f32], &mut [f32]),
        fx: &mut FxProcessor,
        offset: usize,
        blocking: bool,
    ) {
        let cx = Context {
            bank,
            slots: bank.slots(),
            cc: &self.cc,
            bend: &self.bend,
            pressure: &self.pressure,
            tune: self.instrument.2,
            rate: self.rate as f32,
            blocking,
        };
        let n = left.len();
        let mut i = 0;
        while i < self.voices.len() {
            let voice = &mut self.voices[i];
            let bus = bank.settings[voice.group as usize].bus;
            let (alive, underrun) = match bus.and_then(|b| fx.bus_input(b, offset..offset + n)) {
                Some((l, r)) => voice.render(&cx, &mut self.scratch, l, r),
                None => voice.render(&cx, &mut self.scratch, left, right),
            };
            self.underruns += u64::from(underrun);
            if alive {
                i += 1;
            } else {
                let v = self.voices.swap_remove(i);
                if let Some(stream) = v.stream {
                    cx.slots[stream.slot as usize].stop();
                    self.free.push(stream.slot);
                }
            }
        }
    }

    /// Instrument volume/pan (ramped per block) and the Tone low-pass.
    fn output(&mut self, left: &mut [f32], right: &mut [f32], cutoff: f32) {
        let n = left.len() as f32;
        let (gain, pan, _) = self.instrument;
        let target = balance(self.volume * gain, (self.pan + pan).clamp(-1.0, 1.0));
        let start = self.out_gains;
        let delta = [(target[0] - start[0]) / n, (target[1] - start[1]) / n];
        self.out_gains = target;
        if start != [1.0; 2] || target != [1.0; 2] {
            for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                *l *= start[0] + delta[0] * i as f32;
                *r *= start[1] + delta[1] * i as f32;
            }
        }
        if cutoff >= 20000.0 {
            self.tone = [*left.last().unwrap_or(&0.0), *right.last().unwrap_or(&0.0)];
            return;
        }
        let rate = self.rate as f32;
        let a = 1.0 - (-std::f32::consts::TAU * cutoff.min(rate * 0.45) / rate).exp();
        for (channel, state) in [left, right].into_iter().zip(&mut self.tone) {
            for x in channel {
                *state += a * (*x - *state);
                *x = *state;
            }
        }
    }
}

/// Equal-power gain for a zone's crossfade edges at `x` in `[lo, hi]`.
fn edge_gain(x: u8, lo: u8, hi: u8, fade_lo: u8, fade_hi: u8) -> f32 {
    let ramp = |distance: i16, width: u8| {
        if width == 0 || distance >= i16::from(width) {
            1.0
        } else {
            let t = f32::from(distance + 1) / f32::from(width + 1);
            (t * std::f32::consts::FRAC_PI_2).sin()
        }
    };
    ramp(i16::from(x) - i16::from(lo), fade_lo) * ramp(i16::from(hi) - i16::from(x), fade_hi)
}

/// Voice to steal: kill modes are Kontakt's (0 any, 1 oldest, 2 newest,
/// 3 highest, 4 lowest); released voices go first when preferred.
fn victim(
    voices: &[Voice],
    filter: impl Fn(&Voice) -> bool,
    mode: i16,
    prefer_released: bool,
) -> Option<usize> {
    voices
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.fade.dying() && filter(v))
        .min_by_key(|(_, v)| {
            let rank = u8::from(prefer_released && !v.released);
            let key = match mode {
                2 => u64::MAX - v.age,
                3 => u64::from(127 - v.note),
                4 => u64::from(v.note),
                _ => v.age,
            };
            (rank, key)
        })
        .map(|(i, _)| i)
}

impl Bank {
    pub(crate) fn slots(&self) -> &[Slot] {
        self.streamer.as_ref().map_or(&[], |s| s.slots())
    }
}
