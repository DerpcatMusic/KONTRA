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

mod audit;
mod bank;
pub(crate) mod filter;
mod map;
pub mod overrides;
mod params;
mod rack;
mod residency;
mod script;
mod stream;
mod voice;

pub(crate) use bank::parallel;
pub use audit::audit_dsp;
pub use bank::{Bank, GroupSettings, LOAD_DONE, MEMORY_LIMIT, PRELOAD_FRAMES, Streaming, memory_budget, resident_bytes};
pub use params::{Disp, MAX_WRITES, Mod, ModTable, VOICE_MODS, display as engine_par_display, id as engine_par};
pub use residency::{Heads, Residency};
pub use rack::{
    BUSES, Block, BusControls, Mix, NO_AUX, PartControls, Peaks, RACK_SLOTS, Rack, TUNE_RANGE,
};
pub use script::{IrRequest, MAX_COMMANDS, ScriptSetup, effects, load_scripts, load_scripts_with_ir};
pub use voice::{Ahdsr, Flex, FlexPoint, Phase};

use crate::fx::FxProcessor;
use crate::ksp::Runtime;
use map::FOREVER;
use std::sync::atomic::Ordering;
use params::{Address, GroupPar, Write};
use script::{Command, Host};
use stream::Slot;
use filter::VoiceFilter;
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
/// Load (render time over block time) from which released voices too quiet
/// to hear under the rest end early: at −80 dBFS here, rising to −50 dBFS
/// at full load.
const SHED_FROM: f32 = 0.7;
/// Load from which the quietest released voices end, [`STEAL_PER_BLOCK`]
/// a block, however loud.
const STEAL_FROM: f32 = 0.9;
const STEAL_PER_BLOCK: usize = 16;
/// Seconds of ramp where a voice starts or ends mid-waveform.
pub(crate) const DECLICK: f32 = 0.001;

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
    /// Physical channel and original engine input key, before KSP transposition.
    pub owner: Option<(u8, u8)>,
    /// Physical input provenance, independent of key-follow lifetime.
    pub input_channel: Option<u8>,
    /// Physical key-up may precede application of a queued attack.
    pub counter_stop: Option<u64>,
    /// A release sample's frozen originating event duration in milliseconds.
    pub release_held_ms: Option<f32>,
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
    /// An originating MPE event's member expression, independent of channel reuse.
    pub frozen_expression: Option<Expression>,
}

impl NoteEvent<'_> {
    pub fn new(channel: u8, note: u8, velocity: u8) -> Self {
        Self {
            channel,
            note,
            velocity,
            owner: None,
            input_channel: None,
            counter_stop: None,
            release_held_ms: None,
            groups: None,
            offset_us: 0,
            volume: 1.0,
            tune: 0.0,
            pan: 0.0,
            frozen_expression: None,
        }
    }
}

/// Per-channel/key expression from MPE or host note expressions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Expression {
    /// Semitones.
    pub tune: f32,
    /// Linear gain.
    pub gain: f32,
    /// −1..=1, added to the voice's pan.
    pub pan: f32,
    /// Raw member controls frozen at physical key-up; manager controls stay live.
    pub member_cc74: Option<u8>,
    pub member_pressure: Option<u8>,
}

impl Default for Expression {
    fn default() -> Self {
        Self { tune: 0.0, gain: 1.0, pan: 0.0, member_cc74: None, member_pressure: None }
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
    ir_requests: Vec<IrRequest>,
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
    /// The part's own tune in semitones (cents as the fraction), over the
    /// instrument's and the groups'.
    pub tune: f32,
    /// The player's edits over the bank's values (see `overrides.rs`).
    overrides: overrides::Edits,
    /// Recent render time over block time, set by the host: near the
    /// deadline, released voices end early (see [`SHED_FROM`]).
    pub load: f32,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            bank: None,
            fx: FxProcessor::default(),
            player: Player::new(48000.0),
            script: None,
            commands: Vec::with_capacity(script::COMMAND_CAPACITY),
            writes: Vec::with_capacity(MAX_WRITES),
            ir_requests: Vec::with_capacity(32),
            script_channel: 0,
            attack: 0.002,
            release: 0.15,
            cutoff: 20000.0,
            blocking_streams: false,
            tune: 0.0,
            overrides: overrides::Edits(Vec::with_capacity(overrides::MAX_OVERRIDES)),
            load: 0.0,
        }
    }
}

impl Engine {
    /// Install `bank`, stopping every voice; returns the previous bank for
    /// disposal off the audio thread.
    pub fn set_bank(&mut self, bank: Option<Box<Bank>>) -> Option<Box<Bank>> {
        self.player.clear_voices(self.bank.as_deref());
        self.player.touch();
        self.commands.clear();
        self.writes.clear();
        self.ir_requests.clear();
        let old = std::mem::replace(&mut self.bank, bank);
        self.player.free.clear();
        let slots = self.bank.as_deref().map_or(0, |b| b.slots().len());
        self.player.free.extend((0..slots as u16).rev());
        self.replay(Address::is_group);
        self.refresh_overrides();
        old
    }

    /// Replace the bank with `bank`, built from the same instrument with
    /// other residency (the RAM-only fill finishing): playing voices carry
    /// on from the new bank's data, and script-set group parameters stay.
    /// A bank with other zones (a sample read in one load only) installs as
    /// [`Engine::set_bank`] does. Returns the old bank for disposal off the
    /// audio thread.
    pub fn upgrade_bank(&mut self, mut bank: Box<Bank>) -> Option<Box<Bank>> {
        let Some(old) = self.bank.as_deref_mut().filter(|old| {
            old.zones().len() == bank.zones().len() && old.samples.len() == bank.samples.len()
        }) else {
            return self.set_bank(Some(bank));
        };
        // Swapped, not cloned: no allocation here. The base too: scripts
        // write it, and overrides recompute what plays from it.
        std::mem::swap(&mut old.settings, &mut bank.settings);
        std::mem::swap(&mut old.base, &mut bank.base);
        self.player.touch();
        self.player.rebind(old, &bank);
        self.bank.replace(bank)
    }

    /// Swap in heads the smart memory resized (see `residency.rs`), each
    /// only while no voice plays its sample: a voice reads its sample's
    /// head by index and limit. Applied heads then hold the replaced data,
    /// to free off the audio thread; the rest wait for a later block.
    pub fn swap_heads(&mut self, heads: &mut [residency::Head]) {
        let Some(bank) = self.bank.as_deref_mut() else {
            return;
        };
        let in_use = &mut self.player.in_use;
        in_use.clear();
        in_use.extend(self.player.voices.iter().map(|v| v.sample));
        in_use.sort_unstable();
        for head in heads {
            let sample = head.sample();
            if let Some(data) = bank.samples.get_mut(sample as usize)
                && data.streamed
                && in_use.binary_search(&sample).is_err()
            {
                std::mem::swap(&mut data.spans, &mut head.spans);
                head.applied();
            }
        }
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
        self.ir_requests.clear();
        if let Some(rt) = script.as_deref_mut() {
            rt.set_sample_rate(self.player.rate);
        }
        let old = std::mem::replace(&mut self.script, script);
        self.player.native_sustain = !self.script.as_ref().is_some_and(|rt| rt.condition("NO_SYS_SCRIPT_PEDAL"));
        self.player.native_release_triggers = !self.script.as_ref().is_some_and(|rt| rt.condition("NO_SYS_SCRIPT_RLS_TRIG"));
        if !self.player.native_release_triggers { self.player.pending_releases.clear(); }
        // Reconcile held notes when replacing a script with the pedal down.
        // CC64 remains visible to scripts/modulation even when native hold is off.
        let defaults = self.defaults();
        for channel in 0..16 {
            let on = self.player.native_sustain && self.player.cc[channel][64] >= 64;
            let was_on = std::mem::replace(&mut self.player.sustain[channel], on);
            if was_on && !on && let Some(bank) = self.bank.as_deref() {
                self.player.pedal_up(bank, channel as u8, defaults);
            }
        }
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
        self.player.touch();
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
            _ => self.write_group(address, value),

        }
    }

    pub fn script(&self) -> Option<&Runtime> {
        self.script.as_deref()
    }

    pub fn begin_audio_block(&mut self, frames: usize, parts: usize, offline: bool) {
        if let Some(rt) = &mut self.script {
            rt.begin_audio_block(frames, parts, offline);
        }
    }

    /// The instrument's effects, and what it played past its own output.
    pub fn fx(&self) -> &FxProcessor {
        &self.fx
    }

    pub fn bank(&self) -> Option<&Bank> {
        self.bank.as_deref()
    }

    pub fn rate(&self) -> f64 {
        self.player.rate
    }

    /// Mix voices that resample alike together (on by default; off renders
    /// every voice alone, for comparison).
    pub fn set_lanes(&mut self, on: bool) {
        self.player.shared = on;
    }

    /// Stop all voices, silence effect tails and reset MIDI state; keeps the
    /// bank, effects, scripts and group mask. Effects stay built for their own
    /// rate: replace them with [`set_fx`](Self::set_fx) when `rate` changes.
    pub fn reset(&mut self, rate: f64) {
        self.cancel_notes(u16::MAX);
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
        if let Some(rt) = self.script.as_deref_mut() { rt.reset_controllers(Some(&self.player.cc[0])); }
    }

    fn cancel_notes(&mut self, channels: u16) {
        if let Some(rt) = self.script.as_deref_mut() { rt.all_sound_off(channels); }
        // Controller commands can carry UI edits; stopping notes must not lose them.
        self.commands.retain(|command| channels & (1 << command.channel) == 0
            || matches!(command.kind, script::Kind::Controller { .. }));
    }

    /// Stop every performance context in one pass, preserving script/UI setup.
    pub fn panic(&mut self) {
        self.cancel_notes(u16::MAX);
        self.commands.retain(|command| !matches!(command.kind,
            script::Kind::Controller { cc: 1 | 11 | 64 | 66 | 128 | 129, .. }));
        let defaults = self.defaults();
        for channel in 0..16 {
            self.player.cc(self.bank.as_deref(), channel, 120, 0, defaults);
            self.player.cc(self.bank.as_deref(), channel, 121, 0, defaults);
        }
        if let Some(rt) = self.script.as_deref_mut() { rt.reset_controllers(None); }
    }

    pub fn active_voices(&self) -> usize {
        self.player.voices.len()
    }

    /// Voices not muted: those rendered, and heard.
    pub fn audible_voices(&self) -> usize {
        self.player.voices.iter().filter(|v| v.gains != [0.0; 2]).count()
    }

    /// What each playing voice is, for diagnostics: group, whether it was
    /// released or release-triggered, streams, and its channel gain and
    /// envelope level at the end of the last block.
    pub fn voice_census(&self) -> Vec<VoiceInfo> {
        self.player
            .voices
            .iter()
            .map(|v| VoiceInfo {
                group: v.group,
                event: v.event,
                input_channel: v.input_channel,
                owner: v.owner,
                held: v.held,
                channel: v.channel,
                note: v.note,
                released: v.released,
                release_trigger: v.release_trigger,
                streams: v.stream.is_some_and(|s| !s.paused),
                gain: v.gains[0].abs().max(v.gains[1].abs()),
                envelope: v.env.level() * v.flex.as_ref().map_or(1.0, |f| f.level()) * v.fade.value(),
                sample: v.sample,
                pos: v.pos,
                step: v.step * v.tune * v.pitch.1,
                filtered: self.bank.as_ref().is_some_and(|b| b.settings[v.group as usize].filter.is_some()),
                phase: v.env.phase(),
            })
            .collect()
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
    fn scripted_from(&mut self, channel: u8, input_channel: u8) -> Option<(&mut Runtime, Host<'_>)> {
        let (rt, host) = self.scripted(channel)?;
        rt.set_input_channel(input_channel);
        Some((rt, host))
    }

    fn scripted(&mut self, channel: u8) -> Option<(&mut Runtime, Host<'_>)> {
        let rt = self.script.as_deref_mut()?;
        self.script_channel = channel;
        rt.set_midi_channel(channel);
        let host = Host {
            channel,
            bank: self.bank.as_deref(),
            fx: &self.fx,
            player: &mut self.player,
            commands: &mut self.commands,
            writes: &mut self.writes,
            ir_requests: &mut self.ir_requests,
        };
        Some((rt, host))
    }

    /// Note input takes effect at the start of the next [`render`](Self::render).
    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        self.note_on_from(channel, channel, note, velocity);
    }

    pub(crate) fn note_on_from(&mut self, channel: u8, owner: u8, note: u8, velocity: u8) {
        if channel >= 16 || owner >= 16 || note >= 128 {
            return;
        }
        if velocity == 0 {
            return self.note_off_from(channel, owner, note);
        }
        self.player.keys[channel as usize][note as usize] = velocity.min(127);
        self.player.key_on[channel as usize][note as usize] = self.player.now;
        if let Some((rt, mut host)) = self.scripted(channel) {
            return rt.note_on_from(&mut host, 0, owner, note, velocity.min(127));
        }
        self.player.input_keys[owner as usize][note as usize] = (channel, velocity.min(127));
        self.start_event(&NoteEvent { owner: Some((owner, note)), input_channel: Some(owner), ..NoteEvent::new(channel, note, velocity) });
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        self.note_off_from(channel, channel, note);
    }

    pub(crate) fn note_off_from(&mut self, channel: u8, owner: u8, note: u8) {
        if channel >= 16 || owner >= 16 || note >= 128 {
            return;
        }
        for v in self.player.voices.iter_mut().filter(|v| v.channel == channel && v.owner == Some((owner, note)) && v.held && !v.release_trigger) {
            v.counter_stop.get_or_insert(self.player.now);
        }
        for c in self.commands.iter_mut().filter(|c| c.channel == channel) {
            if let script::Kind::Start { owner: input, counter_stop, .. } = &mut c.kind
                && *input == Some((owner, note)) { counter_stop.get_or_insert(self.player.now); }
        }
        // Capture at the physical key-up, before a delayed KSP release or
        // member reuse can replace this event's channel/key expression.
        if self.player.mpe_zone.is_some_and(|(_, members)| members & (1 << channel) != 0) {
            let x = self.release_snapshot(channel, note);
            for c in self.commands.iter_mut().filter(|c| c.channel == channel) {
                if let script::Kind::Start { owner: input, expression, .. } = &mut c.kind
                    && *input == Some((owner, note)) { expression.get_or_insert(x); }
            }
            for v in self.player.voices.iter_mut().filter(|v| v.channel == channel && v.owner == Some((owner, note)) && v.held && !v.release_trigger) {
                v.frozen_expression.get_or_insert(x);
            }
        }
        if self.script.is_some() {
            if !self.script.as_ref().unwrap().key_down_except(owner, channel, note) {
                self.player.keys[channel as usize][note as usize] = 0;
                self.player.key_up[channel as usize][note as usize] = self.player.now;
            }
            if let Some((rt, mut host)) = self.scripted(channel) {
                rt.note_off_from(&mut host, 0, owner, note);
            }
            return;
        }
        let defaults = self.defaults();
        if let Some(bank) = self.bank.as_deref() {
            self.player.note_off(bank, channel, note, Some(owner), defaults);
        } else {
            self.player.keys[channel as usize][note as usize] = 0;
        }
    }

    /// Controllers pass through the scripts; channel mode messages (120 and up)
    /// act on the engine directly so a script can never swallow a panic.
    pub fn cc(&mut self, channel: u8, cc: u8, value: u8) {
        self.cc_from(channel, channel, cc, value);
    }

    pub(crate) fn cc_from(&mut self, channel: u8, input_channel: u8, cc: u8, value: u8) {
        if channel >= 16 || cc >= 128 {
            return;
        }
        if cc == 120 {
            let members = self.player.mpe_zone.filter(|(master, _)| *master == channel).map_or(0, |(_, members)| members);
            let channels = (1 << channel) | members;
            self.cancel_notes(channels);
        }
        if cc == 123 && let Some((master, members)) = self.player.mpe_zone && channel == master {
            for member in (0..16).filter(|m| members & (1 << m) != 0) {
                self.cc(member, cc, value);
            }
        }
        if self.script.is_some() {
            if cc < 120 {
                if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
                    rt.controller(&mut host, 0, cc, value.min(127));
                }
                return;
            }
            if cc == 123 {
                for owner in 0..16 {
                    for note in 0..128 {
                        if self.script.as_ref().is_some_and(|rt| rt.key_down_from(owner, channel, note)) {
                            self.note_off_from(channel, owner, note);
                        }
                    }
                }
                return;
            }
        }
        let defaults = self.defaults();
        self.player
            .cc(self.bank.as_deref(), channel, cc, value, defaults);
    }

    /// Cut a physical performance context, including notes a script rerouted.
    pub(crate) fn all_sound_off_from(&mut self, channel: u8, input_mask: u16) {
        if channel >= 16 || input_mask == 0 { return; }
        let selected = |input_channel: Option<u8>| input_channel.is_some_and(|input| input_mask & (1 << input.min(15)) != 0);
        let mut affected = 1 << channel;
        if let Some(rt) = self.script.as_deref_mut() { affected |= rt.all_sound_off_from(input_mask); }
        self.commands.retain(|c| {
            if !selected(c.input_channel) { return true; }
            affected |= 1 << c.channel;
            false
        });
        let fade = self.player.fade_frames(STEAL_FADE);
        for v in self.player.voices.iter_mut().filter(|v| selected(v.input_channel)) {
            affected |= 1 << v.channel;
            v.fade.start(0.0, fade, true);
        }
        self.player.pending_releases.retain(|r| {
            if !selected(r.input_channel) { return true; }
            affected |= 1 << r.channel;
            false
        });
        for input in (0..16).filter(|input| input_mask & (1 << input) != 0) {
            for key in self.player.input_keys[input].iter_mut().filter(|key| key.1 > 0) {
                affected |= 1 << key.0;
                *key = (0, 0);
            }
        }
        for channel in (0..16).filter(|channel| affected & (1 << channel) != 0) {
            for note in 0..128 {
                let held = if let Some(rt) = self.script.as_deref() {
                    (0..16).any(|input| rt.key_down_from(input, channel as u8, note as u8))
                } else {
                    self.player.input_keys.iter().any(|row| row[note].0 == channel as u8 && row[note].1 > 0)
                };
                if !held {
                    self.player.keys[channel][note] = 0;
                    self.player.key_up[channel][note] = self.player.now;
                }
            }
        }
    }

    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        self.pitch_bend_from(channel, channel, value);
    }

    pub(crate) fn pitch_bend_from(&mut self, channel: u8, input_channel: u8, value: u16) {
        if channel >= 16 {
            return;
        }
        let value = value.min(16383);
        if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
            return rt.pitch_bend(&mut host, 0, i32::from(value) - 8192);
        }
        self.player.bend[channel as usize] = (f32::from(value) - 8192.0) / 8192.0;
        self.player.touch();
    }

    /// Channel pressure (mono aftertouch modulation), through the scripts.
    pub fn channel_pressure(&mut self, channel: u8, value: u8) {
        self.channel_pressure_from(channel, channel, value);
    }

    pub(crate) fn channel_pressure_from(&mut self, channel: u8, input_channel: u8, value: u8) {
        let (channel, value) = (channel.min(15), value.min(127));
        if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
            return rt.channel_pressure(&mut host, 0, value);
        }
        self.player.pressure[channel as usize] = value;
        self.player.touch();
    }

    /// A host edit of script control `control` in script slot `slot`: sets its
    /// value and runs the script's `on ui_control`.
    pub fn ui_control(&mut self, slot: usize, control: usize, value: i32) {
        let channel = self.script_channel;
        if let Some((rt, mut host)) = self.scripted(channel) {
            rt.ui_control(&mut host, slot, control, value);
        }
    }

    pub fn pop_ir_request(&mut self) -> Option<IrRequest> {
        if !self.ir_requests.is_empty() {
            let mut request = self.ir_requests.remove(0);
            if let Some(settings) = self.fx.ir_request_settings(request.rack, request.slot) { request.settings = settings; }
            Some(request)
        } else {
            self.fx.take_ir_change().map(|(rack, slot, settings)| IrRequest::rebuild(rack, slot, settings))
        }
    }

    pub fn retry_ir_request(&mut self, mut request: IrRequest) -> Result<(), IrRequest> {
        if request.id < 0 {
            self.fx.mark_ir_changed(request.rack, request.slot);
            return Ok(());
        }
        if self.ir_requests.len() == self.ir_requests.capacity() { return Err(request) }
        if let Some(settings) = self.fx.ir_request_settings(request.rack, request.slot) { request.settings = settings; }
        self.ir_requests.push(request);
        Ok(())
    }

    /// Install an off-thread-built IR, retaining slot gains and returning the
    /// old DSP for disposal off the audio thread. Failures keep the old IR.
    pub fn finish_ir(&mut self, slot: u8, id: i32, ir: Option<crate::fx::PreparedIr>) -> Option<crate::fx::PreparedIr> {
        let (loaded, retired) = match ir {
            Some(ir) => match self.fx.replace_ir(ir) {
                Ok(old) => (true, Some(old)),
                Err(ir) => (false, Some(ir)),
            },
            None => (false, None),
        };
        if id >= 0 && let Some((rt, mut host)) = self.scripted(self.script_channel) {
            rt.async_complete(&mut host, slot, id, loaded);
        }
        retired
    }

    /// Polyphonic key pressure; only scripts react to it.
    pub fn poly_pressure(&mut self, channel: u8, note: u8, value: u8) {
        self.poly_pressure_from(channel, channel, note, value);
    }

    pub(crate) fn poly_pressure_from(&mut self, channel: u8, input_channel: u8, note: u8, value: u8) {
        if let Some((rt, mut host)) = self.scripted_from(channel.min(15), input_channel) {
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
        if let Some((channel, note, velocity, false, latched, input_channel)) = self.player.release_voices(bank, id) {
            let allowed = self.player.allowed;
            let key = (channel, note, velocity);
            let expression = self.player.release_expression(id, channel, note);
            self.player.trigger_release(bank, id, key, &allowed, latched, expression, input_channel, defaults);
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

    /// Set live expression on `note`; released MPE snapshots remain independent.
    pub fn set_expression(&mut self, note: u8, expression: Expression) {
        for channel in 0..16 {
            self.set_expression_on(channel, note, expression);
        }
    }

    pub fn set_expression_on(&mut self, channel: u8, note: u8, expression: Expression) {
        if channel < 16 && note < 128 {
            self.player.expression[channel as usize][note as usize] = expression;
        }
    }

    pub(crate) fn set_mpe_zone(&mut self, zone: Option<(u8, u16)>) {
        self.player.mpe_zone = zone.map(|(master, members)| {
            let master = master.min(15);
            (master, members & !(1 << master))
        });
    }

    pub(crate) fn set_mpe_master_bend_range(&mut self, range: Option<f32>) {
        if self.player.mpe_master_bend_range != range {
            self.player.mpe_master_bend_range = range;
            self.player.touch();
        }
    }

    /// Applied state plus earlier same-frame script commands, before the
    /// release callback or reused member can replace these controller values.
    fn release_snapshot(&self, channel: u8, note: u8) -> Expression {
        self.player.release_snapshot(&self.commands, 0, channel, note)
    }

    pub(crate) fn freeze_released_expression(&mut self, channel: u8, note: u8) {
        if channel >= 16 || note >= 128 { return; }
        let expression = self.release_snapshot(channel, note);
        for v in &mut self.player.voices {
            if v.channel == channel && v.owner.map_or(v.note, |(_, key)| key) == note && v.frozen_expression.is_none()
                && (!v.held || self.commands.iter().any(|c| c.at == 0 && c.id == v.event
                    && matches!(c.kind, script::Kind::Release { .. }))) {
                v.frozen_expression = Some(expression);
            }
        }
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

    /// Bounded pending work for worker-side support reports: commands, writes, releases.
    pub fn pending_work(&self) -> [usize; 3] {
        [self.commands.len(), self.writes.len(), self.player.pending_releases.len()]
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
        let _ftz = FlushDenormals::new();
        let n = left.len().min(right.len());
        left.fill(0.0);
        right.fill(0.0);
        // An empty slot: nothing to play, run or ring out.
        if self.bank.is_none() && self.script.is_none() && self.fx.is_empty() {
            self.commands.clear();
            self.writes.clear();
            return;
        }
        let channel = self.script_channel;
        if let Some((rt, mut host)) = self.scripted(channel) {
            rt.process(&mut host, n as u32);
        }
        self.player.shed(self.load);
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
                        self.player.apply(bank, c, defaults);
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
                    let (blocking, tune) = (self.blocking_streams, self.tune);
                    let out = (&mut l[pos..end], &mut r[pos..end]);
                    // No writes left in the block: bus faders hold until it ends.
                    let steady = written == self.writes.len();
                    self.player.render(bank, out, &mut self.fx, pos, blocking, tune, steady);
                }
                pos = end;
            }
            self.fx.mix_buses(l, r);
            self.player.output(l, r, self.cutoff);
            // Runs without voices too, so reverb and convolution tails ring out.
            self.fx.process(l, r);
            // Backstop: a non-finite sample would deafen the host's whole bus.
            if !l.iter().chain(r.iter()).all(|x| x.is_finite()) {
                l.fill(0.0);
                r.fill(0.0);
                self.fx.clear();
                self.player.tone = [0.0; 2];
            }
        }
        // Only an empty render leaves changes behind: apply them now.
        for i in written..self.writes.len() {
            let w = self.writes[i];
            self.write(w.address, w.value);
        }
        if let Some(bank) = self.bank.as_deref() {
            for c in &self.commands[next..] {
                self.player.apply(bank, c, defaults);
            }
        }
        // Recovery runs after every queued command: a pending pedal-down must
        // not undo it. Extreme stop pressure cuts this channel rather than
        // leaving an unreleaseable voice or pedal; the drop counter reports it.
        let overflow = std::mem::take(&mut self.player.stop_overflow);
        for channel in (0..16).filter(|c| overflow & (1 << c) != 0) {
            self.player.pending_releases.retain(|r| r.channel != channel);
            self.player.cc(self.bank.as_deref(), channel, 121, 0, defaults);
            self.player.cc(self.bank.as_deref(), channel, 120, 0, defaults);
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

/// One voice as [`Engine::voice_census`] reports it.
#[derive(Clone, Copy, Debug)]
pub struct VoiceInfo {
    pub group: u32,
    pub event: EventId,
    /// Physical input provenance, retained through script rerouting.
    pub input_channel: Option<u8>,
    pub owner: Option<(u8, u8)>,
    pub held: bool,
    pub channel: u8,
    pub note: u8,
    pub released: bool,
    pub release_trigger: bool,
    pub streams: bool,
    pub gain: f32,
    pub envelope: f32,
    /// Sample, virtual position and source frames per output frame before
    /// modulation: voices sharing a step and a position's fraction resample alike.
    pub sample: u32,
    pub pos: f64,
    pub step: f64,
    /// The group runs a per-voice filter or EQ.
    pub filtered: bool,
    pub phase: voice::Phase,
}

/// One released event, retaining the script's release-sample selection until
/// its pedals let go. Prepared storage also covers events with no attack zones.
#[derive(Clone, Copy)]
struct PendingRelease {
    source: EventId,
    input_channel: Option<u8>,
    channel: u8,
    note: u8,
    velocity: u8,
    groups: GroupMask,
    captured: bool,
    expression: Option<Expression>,
    held_ms: f32,
}

/// Engine state apart from the bank, so voices can mutate while the bank is borrowed.
struct Player {
    voices: Vec<Voice>,
    /// Free stream slots of the current bank.
    free: Vec<u16>,
    /// Samples voices play, gathered to swap heads safely.
    in_use: Vec<u32>,
    /// Zones matched by the current note start, with crossfade gains.
    pending: Vec<(u32, f32)>,
    scratch: Scratch,
    /// The block's voices by lane.
    lanes: voice::Lanes,
    /// The group filter of the lanes being rendered, and the filtered
    /// lanes by filter: `(class, lane)`.
    lane_filter: filter::LaneFilter,
    order: Vec<(u64, u16)>,
    /// A filter's summed input and a voice's own: left, right, left, right.
    filtered: Box<[[f32; MAX_BLOCK]; 4]>,
    /// Voices that ended in the block.
    dead: Vec<u16>,
    /// Voices may share lanes.
    shared: bool,
    rate: f64,
    /// KSP preprocessor flags bypass only these native system-script actions.
    native_sustain: bool,
    native_release_triggers: bool,
    sustain: [bool; 16],
    /// Pedals and stop messages on an MPE master affect its member channels.
    mpe_zone: Option<(u8, u16)>,
    mpe_master_bend_range: Option<f32>,
    /// Release samples deferred by sustain or their own sostenuto capture.
    pending_releases: Vec<PendingRelease>,
    sostenuto_down: [bool; 16],
    bend: [f32; 16],
    cc: [[u8; 128]; 16],
    /// Channel pressure.
    pressure: [u8; 16],
    /// Per-channel/key expression, by the voices' MIDI channel and note.
    expression: Box<[[Expression; 128]; 16]>,
    /// Velocity of keys that are down.
    keys: [[u8; 128]; 16],
    /// Unscripted physical inputs: engine channel and velocity. Prepared off-thread.
    input_keys: Box<[[(u8, u8); 128]; 16]>,
    /// Frames rendered so far.
    now: u64,
    /// Frame each key went down (or its release counter was reset) and up:
    /// the release-trigger counter's start and stop. Boxed: a rack's engines
    /// are built on the stack.
    key_on: Box<[[u64; 128]; 16]>,
    key_up: Box<[[u64; 128]; 16]>,
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
    /// Channels requiring click-free recovery after the bounded stop quota fills.
    stop_overflow: u16,
    /// Stamp of what modulation reads (see [`voice::Context::inputs`]):
    /// bumped by every controller, bend, pressure and group write.
    inputs: u32,
}

impl Player {
    fn new(rate: f64) -> Self {
        let mut player = Self {
            voices: Vec::with_capacity(MAX_VOICES),
            free: Vec::with_capacity(stream::SLOTS),
            in_use: Vec::with_capacity(MAX_VOICES),
            pending: Vec::with_capacity(MAX_VOICES),
            scratch: Scratch::default(),
            lanes: voice::Lanes::new(MAX_VOICES),
            lane_filter: filter::LaneFilter::new(voice::WINDOW),
            order: Vec::with_capacity(MAX_VOICES),
            filtered: Box::new([[0.0; MAX_BLOCK]; 4]),
            dead: Vec::with_capacity(MAX_VOICES),
            shared: true,
            rate,
            native_sustain: true,
            native_release_triggers: true,
            sustain: [false; 16],
            mpe_zone: None,
            mpe_master_bend_range: None,
            pending_releases: Vec::with_capacity(crate::ksp::EVENT_CAPACITY),
            sostenuto_down: [false; 16],
            bend: [0.0; 16],
            cc: [[0; 128]; 16],
            pressure: [0; 16],
            expression: Box::new([[Expression::default(); 128]; 16]),
            keys: [[0; 128]; 16],
            input_keys: Box::new([[(0, 0); 128]; 16]),
            now: 0,
            key_on: Box::new([[0; 128]; 16]),
            key_up: Box::new([[0; 128]; 16]),
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
            stop_overflow: 0,
            inputs: 0,
        };
        player.reset_midi();
        player
    }

    fn reset_midi(&mut self) {
        self.stop_overflow = 0;
        self.sustain = [false; 16];
        self.pending_releases.clear();
        self.sostenuto_down = [false; 16];
        self.bend = [0.0; 16];
        self.pressure = [0; 16];
        self.expression.fill([Expression::default(); 128]);
        self.cc = [[0; 128]; 16];
        for cc in &mut self.cc {
            (cc[7], cc[10], cc[11]) = (127, 64, 127);
        }
        self.keys = [[0; 128]; 16];
        self.input_keys.fill([(0, 0); 128]);
        (self.volume, self.pan) = (1.0, 0.0);
        self.tone = [0.0; 2];
        self.touch();
    }

    /// Something modulation reads changed: settled voices modulate again.
    fn touch(&mut self) {
        self.inputs = self.inputs.wrapping_add(1);
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

    /// Move playing voices from `old` to `new`, which holds the same samples
    /// with other residency: each finds its resident span and limit as at a
    /// start, and streams through a slot of `new` from where it is.
    fn rebind(&mut self, old: &Bank, new: &Bank) {
        let slots = old.slots();
        self.free.clear();
        self.free.extend((0..new.slots().len() as u16).rev());
        for v in &mut self.voices {
            if let Some(stream) = v.stream.take() {
                slots[stream.slot as usize].stop();
            }
            let sample = &new.samples[v.sample as usize];
            let first = (v.pos as u64).saturating_sub(1);
            v.span = (v.map.run(first, v.wraps))
                .and_then(|run| sample.span_at(run.frame))
                .unwrap_or(0);
            let span = &sample.spans[v.span as usize];
            v.limit = v.map.resident_limit(first, v.wraps, span.start, span.end());
            // Paused, the next render configures it from the position, as
            // when a muted voice returns; a loop reserves it as at a start.
            let slot = if sample.streamed { self.free.pop() } else { None };
            v.stream = slot.map(|slot| voice::Stream {
                slot,
                tag: 0,
                trusted: 0,
                paused: v.limit != FOREVER,
            });
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

    /// How long the key was held when its release-trigger counter stopped:
    /// until now while it is down, else until it went up.
    fn held_ms(&self, channel: u8, note: u8) -> f32 {
        let (c, n) = (channel as usize & 15, note as usize & 127);
        let end = if self.keys[c][n] > 0 { self.now } else { self.key_up[c][n] };
        end.saturating_sub(self.key_on[c][n]) as f32 * 1000.0 / self.rate as f32
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
        for z in bank.matching_zones(ev.channel, ev.note, ev.velocity, release_trigger, mask)
            .take(self.pending.capacity()) {
            let zone = &bank.zones()[z as usize];
            let gain = edge_gain(ev.velocity, zone.low_velocity, zone.high_velocity,
                zone.fade_low_velocity, zone.fade_high_velocity)
                * edge_gain(ev.note, zone.low_key, zone.high_key, zone.fade_low_key, zone.fade_high_key);
            self.pending.push((z, gain));
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
        // One writer, the audio thread: no read-modify-write needed.
        let played = &bank.usage[play.sample as usize];
        played.store(played.load(Ordering::Relaxed).wrapping_add(1), Ordering::Relaxed);
        self.make_room(bank, settings.voice_group);

        let key = if group.key_tracking {
            2f64.powf((f64::from(ev.note) - f64::from(zone.root)) / 12.0)
        } else {
            1.0
        };
        let step = f64::from(sample.rate) / self.rate * zone.tune * key;
        let c = ev.channel as usize;
        let master = self.mpe_zone.filter(|(_, members)| members & (1 << c) != 0).map(|(master, _)| master as usize);
        let expression_key = ev.owner.map_or(ev.note, |(_, key)| key);
        let expression = ev.frozen_expression.unwrap_or(self.expression[c][expression_key as usize & 127]);
        let inputs = params::Inputs {
            cc: &self.cc[c],
            cc74: master.map(|m| expression.member_cc74.unwrap_or(self.cc[c][74]).saturating_add(self.cc[m][74]).min(127)),
            bend: self.bend[c] + master.map_or(0., |m| self.bend[m]),
            pressure: master.map_or(self.pressure[c], |m| expression.member_pressure.unwrap_or(self.pressure[c]).max(self.pressure[m])),
            note: ev.note,
            velocity: ev.velocity,
            counter: if release_trigger {
                params::release_counter(group.release_counter_ms, ev.release_held_ms.unwrap_or_else(|| self.held_ms(ev.channel, ev.note)))
            } else {
                0.0
            },
        };
        let bend_pitch = self.mpe_zone.and_then(|(manager, members)| self.mpe_master_bend_range
            .filter(|_| manager == ev.channel || members & (1 << c) != 0)
            .map(|_| if manager == ev.channel { 0. } else { self.bend[c] }));
        let mods = settings.mods.start(&inputs, bend_pitch);
        let modulated = (settings.mods.start_offset(&inputs) * play.start_mod as f32) as u64;
        let offset = ((ev.offset_us as f64 * f64::from(sample.rate) / 1e6) as u64 + modulated)
            .min(play.start_mod);
        // A release-triggered voice starts with its key already up.
        let wraps = play
            .map
            .wraps(if release_trigger { offset } else { FOREVER });
        // The resident span holding the voice's first window frame (one
        // before the start, for the cubic's left tap), if any does.
        let first = offset.saturating_sub(1);
        let span_index = play
            .map
            .run(first, wraps)
            .and_then(|run| sample.span_at(run.frame))
            .unwrap_or(0);
        let span = &sample.spans[span_index as usize];
        let limit = play.map.resident_limit(first, wraps, span.start, span.end());
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
                    paused: false,
                }
            } else {
                // From the voice's start on (`limit >= first`): an offset past
                // the resident range streams from there, not from the zone start.
                let tag = bank.slots()[slot as usize].configure(
                    play.sample,
                    &play.map,
                    wraps,
                    limit,
                    first,
                );
                Stream {
                    slot,
                    tag,
                    trusted: limit,
                    paused: false,
                }
            }
        });
        let base_level = zone.gain * gain;
        let mut envelope = settings.envelope.unwrap_or(if settings.flex.is_some() {
            Ahdsr::UNITY
        } else {
            defaults
        });
        settings.mods.scale_envelope(&mut envelope, &inputs);
        let mut voice = Voice {
            event,
            zone_id: play.zone_id,
            group: zone.group as u32,
            voice_group: settings.voice_group,
            channel: ev.channel,
            note: ev.note,
            velocity: ev.velocity,
            counter_start: self.now,
            counter_stop: ev.counter_stop,
            owner: ev.owner,
            input_channel: ev.input_channel,
            held: !release_trigger,
            sostenuto: false,
            released: false,
            release_trigger,
            frozen_expression: ev.frozen_expression,
            age: self.clock,
            sample: play.sample,
            span: span_index,
            map: play.map,
            wraps,
            length: play.map.len(wraps),
            limit,
            pos: offset as f64,
            step,
            tune: 2f64.powf(ev.tune / 12.0),
            pitch: (f32::NAN, 1.0),
            mods,
            modulated: (1.0, 0.0),
            settled: None,
            stream,
            env: Envelope::new(&envelope, self.rate as f32),
            flex: settings.flex.as_ref().map(|_| Envelope::flex()),
            fade: Fade::FULL,
            base_level,
            volume: ev.volume.max(0.0),
            base_pan: zone.pan,
            pan: ev.pan,
            gains: [0.0; 2],
            muted: 0,
            hold: if limit <= first && stream.is_some() {
                (voice::START_HOLD * self.rate as f32) as u32
            } else {
                0
            },
            filter: VoiceFilter::new(settings.filter.as_deref(), &settings.mods, &inputs, self.rate as f32),
            plan: Default::default(),
        };
        // Start at the voice's first-block gains, so it does not ramp in.
        let (modulation, ..) = settings.mods.modulate(&mut voice.mods, &inputs, 0, 1.0, bend_pitch);
        let pan = (zone.pan + settings.pan + ev.pan + expression.pan).clamp(-1.0, 1.0);
        voice.gains = balance(base_level * settings.gain * modulation * voice.volume * expression.gain, pan);
        if offset > 0 {
            voice.fade.fade_in(self.fade_frames(DECLICK));
        }
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
            // Live voices are at most all voices: below the limit, no scan.
            if self.voices.len() >= rule.max_voices {
                let (live, victim) = victim(
                    &self.voices,
                    |v| v.voice_group == Some(group),
                    rule.kill_mode,
                    rule.prefer_released,
                );
                if live >= rule.max_voices && let Some(i) = victim {
                    let fade = self.fade_frames(rule.fade);
                    self.voices[i].fade.start(0.0, fade, true);
                }
            }
        }
        if self.voices.len() >= bank.polyphony {
            let (live, victim) = victim(&self.voices, |_| true, 1, true);
            if live >= bank.polyphony && let Some(i) = victim {
                let fade = self.fade_frames(STEAL_FADE);
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

    fn note_off(&mut self, bank: &Bank, channel: u8, note: u8, owner: Option<u8>, defaults: Ahdsr) {
        if channel >= 16 || note >= 128 {
            return;
        }
        let (c, n) = (channel as usize, note as usize);
        let velocity = if let Some(owner) = owner {
            let input = &mut self.input_keys[owner as usize][n];
            if input.0 == channel { std::mem::take(&mut input.1) } else { 0 }
        } else {
            for row in self.input_keys.iter_mut().filter(|r| r[n].0 == channel) { row[n].1 = 0; }
            self.keys[c][n]
        };
        let remaining = self.input_keys.iter().filter(|r| r[n].0 == channel).map(|r| r[n].1).max().unwrap_or(0);
        // Direct start_event callers may have no physical-input record.
        let velocity = if velocity == 0 && remaining == 0 { self.keys[c][n] } else { velocity };
        self.keys[c][n] = remaining;
        if velocity > 0 && remaining == 0 {
            self.key_up[c][n] = self.now;
        }
        let allowed = self.allowed;
        let mut found = false;
        // Layered voices share an event. Releasing that event clears held on
        // every layer, so each distinct physical retrigger fires exactly once.
        // ponytail: scan per event; index event voices if dense retrigger releases dominate.
        while let Some(source) = self.voices.iter().find(|v| v.channel == channel
            && owner.map_or(v.note == note, |owner| v.owner.map_or(v.note == note, |input| input == (owner, note)))
            && v.held && !v.released && !v.release_trigger).map(|v| v.event) {
            if let Some((channel, note, velocity, false, captured, input_channel)) = self.release_voices(bank, source) {
                found = true;
                let expression = self.release_expression(source, channel, note);
                self.trigger_release(bank, source, (channel, note, velocity), &allowed, captured, expression, input_channel, defaults);
            }
        }
        // Release-only instruments can have a key but no attack voice.
        if !found && velocity > 0 {
            let source = self.next_id();
            let expression = self.release_expression(source, channel, note);
            self.trigger_release(bank, source, (channel, note, velocity), &allowed, false, expression, owner, defaults);
        }
    }

    /// Release one event's voices; a held sustain pedal defers them like a key
    /// release. Returns the first voice's channel, note, velocity and whether
    /// it is itself a release trigger.
    fn release_voices(&mut self, bank: &Bank, id: EventId) -> Option<(u8, u8, u8, bool, bool, Option<u8>)> {
        let mut first = None;
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.event == id && !v.released)
        {
            let event = first.get_or_insert((v.channel, v.note, v.velocity, v.release_trigger, false, v.input_channel));
            event.4 |= v.sostenuto;
            v.counter_stop.get_or_insert(self.now);
            let c = v.channel as usize & 15;
            if self.sustain[c] || v.sostenuto {
                v.held = false;
            } else {
                v.release(bank, &mut self.free);
            }
        }
        first
    }

    // ponytail: release-only or exhausted attack events use the key clock; retain
    // event timing outside voices if those libraries need independent retrigger clocks.
    fn event_held_ms(&self, source: EventId, channel: u8, note: u8) -> f32 {
        self.voices.iter().find(|v| v.event == source).map_or_else(
            || self.held_ms(channel, note),
            |v| v.counter_stop.unwrap_or(self.now).saturating_sub(v.counter_start) as f32 * 1000. / self.rate as f32,
        )
    }

    fn release_snapshot(&self, commands: &[script::Command], at: u32, channel: u8, note: u8) -> Expression {
        let mut x = self.member_snapshot(channel, note);
        for c in commands.iter().filter(|c| c.channel == channel && c.at <= at) {
            match c.kind {
                script::Kind::Controller { cc: 74, value } => x.member_cc74 = Some(value.clamp(0, 127) as u8),
                script::Kind::Controller { cc: 129, value } => x.member_pressure = Some(value.clamp(0, 127) as u8),
                _ => {}
            }
        }
        x
    }

    fn member_snapshot(&self, channel: u8, note: u8) -> Expression {
        Expression {
            member_cc74: Some(self.cc[channel as usize & 15][74]),
            member_pressure: Some(self.pressure[channel as usize & 15]),
            ..self.expression[channel as usize & 15][note as usize & 127]
        }
    }

    /// The originating voice may have been frozen at an earlier physical
    /// key-up while the script delayed its release callback.
    fn release_expression(&self, source: EventId, channel: u8, note: u8) -> Option<Expression> {
        let voice = self.voices.iter().find(|v| v.event == source);
        let key = voice.and_then(|v| v.owner).map_or(note, |(_, key)| key);
        voice.and_then(|v| v.frozen_expression)
            .or_else(|| self.mpe_zone.filter(|(_, members)| channel < 16 && members & (1 << channel) != 0)
                .map(|_| self.member_snapshot(channel, key)))
    }

    /// Start the release-trigger zones for a key release, limited to `groups`;
    /// pedals defer the exact event, velocity and group mask until pedal-up.
    fn trigger_release(
        &mut self,
        bank: &Bank,
        source: EventId,
        (channel, note, velocity): (u8, u8, u8),
        groups: &GroupMask,
        latched: bool,
        expression: Option<Expression>,
        input_channel: Option<u8>,
        defaults: Ahdsr,
    ) {
        if !self.native_release_triggers { return; }
        let held_ms = self.event_held_ms(source, channel, note);
        if channel < 16 && note < 128 && (latched || self.sustain[channel as usize]) {
            if self.pending_releases.iter().any(|r| r.source == source) { return; }
            if self.pending_releases.len() == crate::ksp::EVENT_CAPACITY {
                // Only the excess release sample is lost; voice and pedal
                // lifetimes have already advanced, and the loss is counted.
                self.dropped_commands += 1;
                return;
            }
            self.pending_releases.push(PendingRelease {
                source, input_channel, channel, note, velocity, groups: *groups, captured: latched, expression, held_ms,
            });
            return;
        }
        let id = self.next_id();
        let event = NoteEvent {
            groups: Some(groups),
            frozen_expression: expression,
            release_held_ms: Some(held_ms),
            input_channel,
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
        self.touch();
        match cc {
            // General MIDI volume curve: 127 is unity.
            7 => self.volume = (f32::from(value) / 127.0).powi(2),
            10 => self.pan = ((f32::from(value) - 64.0) / 63.0).clamp(-1.0, 1.0),
            64 if self.native_sustain => {
                let on = value >= 64;
                if self.sustain[c] && !on {
                    self.sustain[c] = false;
                    if let Some(bank) = bank {
                        self.pedal_up(bank, channel, defaults);
                    }
                }
                self.sustain[c] = on;
            }
            // Sostenuto latches the keys down now; its release lets go of
            // those no longer held by key or sustain.
            66 => {
                let on = value >= 64;
                if std::mem::replace(&mut self.sostenuto_down[c], on) == on {
                    return;
                }
                if on {
                    // Scripts may queue these commands while input keys already
                    // reflect later note-offs. Capture the voices sounding at
                    // this command's time, only on the pedal's down edge.
                    for v in self.voices.iter_mut().filter(|v| v.channel == channel && v.held && !v.released && !v.release_trigger) {
                        v.sostenuto = true;
                    }
                } else {
                    for v in self.voices.iter_mut().filter(|v| v.channel == channel) { v.sostenuto = false; }
                    for r in self.pending_releases.iter_mut().filter(|r| r.channel == channel) { r.captured = false; }
                    if let Some(bank) = bank { self.pedal_up(bank, channel, defaults); }
                }
            }
            // All sound off: a click-free cut, well short of any release.
            120 => {
                let fade = self.fade_frames(STEAL_FADE);
                for v in self.voices.iter_mut().filter(|v| v.channel == channel) {
                    v.fade.start(0.0, fade, true);
                }
                self.pending_releases.retain(|r| r.channel != channel);
                self.keys[c].fill(0);
                self.key_up[c].fill(self.now);
                for row in self.input_keys.iter_mut() {
                    for input in row.iter_mut().filter(|input| input.0 == channel) { *input = (0, 0); }
                }
            }
            121 => {
                self.cc(bank, channel, 64, 0, defaults);
                self.cc(bank, channel, 66, 0, defaults);
                self.bend[c] = 0.0;
                self.pressure[c] = 0;
                self.cc[c][1] = 0;
                self.cc[c][11] = 127;
            }
            123 => {
                if let Some(bank) = bank {
                    for note in 0..128 {
                        self.note_off(bank, channel, note, None, defaults);
                    }
                }
            }
            _ => {}
        }
        // Forward once at the engine boundary, after the script's controller
        // callback. A script may consume or delay a controller; member channels
        // must not run copies of that callback.
        // Expressive CC74 is separate; data-entry/RPN selectors stay local.
        // Engine::cc handles CC123 through the runtime's physical note owners.
        if !matches!(cc, 6 | 38 | 74 | 96..=101 | 123)
            && let Some((master, members)) = self.mpe_zone
            && channel == master
        {
            for member in (0..16).filter(|m| members & (1 << m) != 0) {
                self.cc(bank, member, cc, value, defaults);
            }
        }
    }

    /// A pedal let go: release voices whose own key and pedal holds are gone.
    fn pedal_up(&mut self, bank: &Bank, channel: u8, defaults: Ahdsr) {
        let c = channel as usize;
        if self.sustain[c] { return }
        for v in &mut self.voices {
            if v.channel == channel
                && !v.sostenuto
                && !v.held
                && !v.released
                && !v.release_trigger
            {
                v.release(bank, &mut self.free);
            }
        }
        // Compact survivors in place, preserving release order without a
        // temporary allocation or quadratic shifts for a full pedal history.
        let mut keep = 0;
        for i in 0..self.pending_releases.len() {
            let r = self.pending_releases[i];
            if r.channel == channel && !r.captured {
                let id = self.next_id();
                let event = NoteEvent { input_channel: r.input_channel, groups: Some(&r.groups), frozen_expression: r.expression, release_held_ms: Some(r.held_ms), ..NoteEvent::new(r.channel, r.note, r.velocity) };
                self.start(bank, &event, id, true, defaults);
            } else {
                self.pending_releases[keep] = r;
                keep += 1;
            }
        }
        self.pending_releases.truncate(keep);
    }

    /// Under `load`, fade out released voices: first those below a floor
    /// that rises with the load, then near the deadline the quietest. Held
    /// notes and muted voices (which cost next to nothing) are left alone.
    fn shed(&mut self, load: f32) {
        if load < SHED_FROM {
            return;
        }
        let over = ((load - SHED_FROM) / (1.0 - SHED_FROM)).min(1.0);
        let floor = 10f32.powf((-80.0 + 30.0 * over) / 20.0);
        let steal = if load >= STEAL_FROM { STEAL_PER_BLOCK } else { 0 };
        let fade = self.fade_frames(STEAL_FADE);
        // The quietest released voices above the floor, loudest first.
        let mut quietest = [(f32::INFINITY, 0); STEAL_PER_BLOCK];
        for (i, v) in self.voices.iter_mut().enumerate() {
            if !v.released || v.fade.dying() || v.gains == [0.0; 2] {
                continue;
            }
            let level = v.level();
            if level < floor {
                v.fade.start(0.0, fade, true);
            } else if steal > 0 && level < quietest[0].0 {
                quietest[0] = (level, i);
                quietest.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
            }
        }
        for &(level, i) in &quietest {
            if level.is_finite() {
                self.voices[i].fade.start(0.0, fade, true);
            }
        }
    }

    /// Render every voice into `left`/`right`, or into its group's bus input
    /// at frame `offset` of the current block. `steady`: no bus fader moves
    /// before the block ends.
    fn render(
        &mut self,
        bank: &Bank,
        (left, right): (&mut [f32], &mut [f32]),
        fx: &mut FxProcessor,
        offset: usize,
        blocking: bool,
        tune: f32,
        steady: bool,
    ) {
        let cx = Context {
            bank,
            slots: bank.slots(),
            cc: &self.cc,
            bend: &self.bend,
            mpe_zone: self.mpe_zone,
            mpe_master_bend_range: self.mpe_master_bend_range,
            pressure: &self.pressure,
            expression: &self.expression,
            tune: self.instrument.2 + tune,
            rate: self.rate as f32,
            blocking,
            inputs: self.inputs,
        };
        let n = left.len();
        // Plan every voice, then take them lane by lane: voices sharing one
        // sum into its window, the rest (and lanes of one) render alone.
        self.lanes.clear();
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if voice.waits(&cx, n) {
                self.lanes.skip(i as u16);
                continue;
            }
            let bus = bank.settings[voice.group as usize].bus;
            let through = bus.filter(|_| steady).and_then(|b| fx.bus_gains(b));
            match voice.plan(&cx, n, bus, through).filter(|_| self.shared) {
                Some(lane) => self.lanes.add(lane, voice.plan.class, &voice.filter.held, i as u16),
                None => self.lanes.skip(i as u16),
            }
        }
        self.dead.clear();
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if self.lanes.shared(i as u16) || voice.hold > 0 {
                continue;
            }
            let bus = bank.settings[voice.group as usize].bus;
            let (alive, underrun) = match bus.and_then(|b| fx.bus_input(b, offset..offset + n)) {
                Some((l, r)) => voice.render(&cx, &mut self.scratch, l, r, false),
                None => voice.render(&cx, &mut self.scratch, left, right, false),
            };
            self.underruns += u64::from(underrun);
            if !alive {
                self.dead.push(i as u16);
            }
        }
        for &(lane, _, first, _, count) in self.lanes.lanes.iter().filter(|l| l.1 == 0 && l.4 > 1) {
            let Scratch { window, acc, amp, .. } = &mut self.scratch;
            let acc = &mut acc[..lane.count(n)];
            acc.fill([0.0; 2]);
            for i in self.lanes.members(first, count) {
                let (alive, underrun) = self.voices[i as usize].accumulate(&cx, window, acc, None);
                self.underruns += u64::from(underrun);
                if !alive {
                    self.dead.push(i);
                }
            }
            match lane.bus().and_then(|b| fx.bus_input(b, offset..offset + n)) {
                Some((l, r)) => lane.mix(acc, amp, l, r),
                None => lane.mix(acc, amp, left, right),
            }
        }
        // Filtered lanes, even of one voice, by filter: each filter runs
        // once, on the sum of its lanes.
        self.order.clear();
        let filtered = self.lanes.lanes.iter().enumerate().filter(|(_, l)| l.1 != 0);
        self.order.extend(filtered.map(|(i, l)| (l.1, i as u16)));
        self.order.sort_unstable();
        let mut at = 0;
        while let Some(&(class, head)) = self.order.get(at) {
            let key = self.lanes.keys[head as usize];
            let bus = self.lanes.lanes[head as usize].0.bus();
            let lf = &mut self.lane_filter;
            lf.prepare(&key, n);
            let [out_l, out_r, solo_l, solo_r] = &mut *self.filtered;
            let (ol, or) = (&mut out_l[..n], &mut out_r[..n]);
            ol.fill(0.0);
            or.fill(0.0);
            let (sl, sr) = (&mut solo_l[..n], &mut solo_r[..n]);
            // Equal hashes of unequal filters (or buses) split into runs.
            while let Some(&(c, index)) = self.order.get(at) {
                let (lane, _, first, _, count) = self.lanes.lanes[index as usize];
                if c != class || self.lanes.keys[index as usize] != key || lane.bus() != bus {
                    break;
                }
                at += 1;
                if count == 1 {
                    // Alone, the voice's own output moves its state on.
                    let voice = &mut self.voices[first as usize];
                    lf.begin(&voice.filter);
                    sl.fill(0.0);
                    sr.fill(0.0);
                    let (alive, underrun) = match lane.is_solo() {
                        true => voice.render(&cx, &mut self.scratch, sl, sr, true),
                        false => {
                            let Scratch { window, acc, amp, .. } = &mut self.scratch;
                            let acc = &mut acc[..lane.count(n)];
                            acc.fill([0.0; 2]);
                            let rendered = voice.accumulate(&cx, window, acc, None);
                            lane.mix(acc, amp, sl, sr);
                            rendered
                        }
                    };
                    lf.dots_out(sl, sr, [&mut *ol, &mut *or]);
                    lf.end(&mut voice.filter, [1.0; 2]);
                    self.underruns += u64::from(underrun);
                    if !alive {
                        self.dead.push(first);
                    }
                    continue;
                }
                let Scratch { window, acc, amp, .. } = &mut self.scratch;
                let acc = &mut acc[..lane.count(n)];
                acc.fill([0.0; 2]);
                lane.curve(&mut amp[..n]);
                lf.kernel(&amp[..n], lane.base(), lane.step(), acc.len());
                for i in self.lanes.members(first, count) {
                    let (alive, underrun) = self.voices[i as usize].accumulate(&cx, window, acc, Some(&mut *lf));
                    self.underruns += u64::from(underrun);
                    if !alive {
                        self.dead.push(i);
                    }
                }
                lane.mix(acc, amp, ol, or);
            }
            lf.finish(ol, or);
            let (l, r) = match bus.and_then(|b| fx.bus_input(b, offset..offset + n)) {
                Some((l, r)) => (l, r),
                None => (&mut *left, &mut *right),
            };
            l.iter_mut().zip(ol.iter()).for_each(|(o, x)| *o += x);
            r.iter_mut().zip(or.iter()).for_each(|(o, x)| *o += x);
        }
        // Highest first, so each swap brings in a voice that lives.
        self.dead.sort_unstable_by(|a, b| b.cmp(a));
        for &i in &self.dead {
            let v = self.voices.swap_remove(i as usize);
            if let Some(stream) = v.stream {
                cx.slots[stream.slot as usize].stop();
                self.free.push(stream.slot);
            }
        }
        self.now += n as u64;
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

/// Flush-to-zero and denormals-are-zero while alive, restoring the caller's
/// mode after: decaying tails (tone filter, effects, releases) otherwise
/// crawl through subnormals at many times the cost per sample.
struct FlushDenormals {
    #[cfg(target_arch = "x86_64")]
    saved: u32,
}

impl FlushDenormals {
    #[allow(deprecated)]
    fn new() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
            const FTZ_DAZ: u32 = 0x8040;
            // SAFETY: SSE is baseline on x86_64; only the FTZ and DAZ bits change.
            let saved = unsafe { _mm_getcsr() };
            unsafe { _mm_setcsr(saved | FTZ_DAZ) };
            Self { saved }
        }
        #[cfg(not(target_arch = "x86_64"))]
        Self {}
    }
}

impl Drop for FlushDenormals {
    #[allow(deprecated)]
    fn drop(&mut self) {
        // SAFETY: restores the mode read in `new`.
        #[cfg(target_arch = "x86_64")]
        unsafe {
            std::arch::x86_64::_mm_setcsr(self.saved)
        };
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

/// Voices `filter` keeps that are not fading out, and the one to steal:
/// kill modes are Kontakt's (0 any, 1 oldest, 2 newest, 3 highest, 4
/// lowest); released voices go first when preferred.
fn victim(
    voices: &[Voice],
    filter: impl Fn(&Voice) -> bool,
    mode: i16,
    prefer_released: bool,
) -> (usize, Option<usize>) {
    // One pass counts and picks: at the voice limit, every note start
    // scans every voice.
    let mut live = 0;
    let mut best: Option<((u8, u64), usize)> = None;
    for (i, v) in voices.iter().enumerate() {
        if v.fade.dying() || !filter(v) {
            continue;
        }
        live += 1;
        let rank = u8::from(prefer_released && !v.released);
        let key = match mode {
            2 => u64::MAX - v.age,
            3 => u64::from(127 - v.note),
            4 => u64::from(v.note),
            _ => v.age,
        };
        if best.is_none_or(|(b, _)| (rank, key) < b) {
            best = Some(((rank, key), i));
        }
    }
    (live, best.map(|(_, i)| i))
}

impl Bank {
    pub(crate) fn slots(&self) -> &[Slot] {
        self.streamer.as_ref().map_or(&[], |s| s.slots())
    }
}

/// Real-time pacing for the offline harnesses (`audit-patch`,
/// `bench-stream`, `render --realtime`, the plugin bench), which play
/// through the engine on a plain thread standing in for a host's audio
/// callback. A host catches up after a late callback by at most its device
/// buffer, the rest being a dropout; a harness that caught up on every
/// missed block would, after the thread was descheduled a while on a busy
/// machine, render back to back faster than real time, outrun the streams
/// and count the machine's stall as the engine's underruns.
pub struct Pace {
    start: std::time::Instant,
}

impl Pace {
    /// Lateness caught up by rendering back to back, as a device buffer
    /// would absorb; beyond it the schedule moves on.
    pub const CATCH_UP: std::time::Duration = std::time::Duration::from_millis(20);

    pub fn start() -> Self {
        Self { start: std::time::Instant::now() }
    }

    /// Sleep until audio time `at` (from the start) is due and return how
    /// late it already was. Later than [`Pace::CATCH_UP`], the schedule
    /// shifts by the lateness: playback resumes at real-time pace from now.
    pub fn until(&mut self, at: std::time::Duration) -> std::time::Duration {
        let (due, now) = (self.start + at, std::time::Instant::now());
        if let Some(wait) = due.checked_duration_since(now) {
            std::thread::sleep(wait);
            return std::time::Duration::ZERO;
        }
        let late = now - due;
        if late > Self::CATCH_UP {
            self.start += late;
        }
        late
    }
}

#[cfg(test)]
mod pace_tests {
    use super::Pace;
    use std::time::Duration;

    /// A stall longer than a device buffer is not caught up faster than
    /// real time: the next block is due a block after the stall, not at once.
    #[test]
    fn a_stall_is_not_caught_up_back_to_back() {
        let block = Duration::from_millis(3);
        let mut pace = Pace::start();
        std::thread::sleep(Duration::from_millis(200));
        assert!(pace.until(block) >= Duration::from_millis(190));
        let resumed = std::time::Instant::now();
        assert_eq!(pace.until(block * 2), Duration::ZERO);
        assert!(resumed.elapsed() >= Duration::from_millis(2));
        // Short lateness is caught up, as a device buffer absorbs it.
        let mut pace = Pace::start();
        std::thread::sleep(Duration::from_millis(10));
        assert!(pace.until(block) > Duration::ZERO);
        assert!(pace.until(block * 2) > Duration::ZERO);
    }
}
