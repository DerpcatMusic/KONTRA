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
mod ahdsr;
mod ahdsr_shared;
mod envelope;
mod bank;
pub mod program_bank;
mod rack_part;
pub use rack_part::RackPart;
pub(crate) mod filter;
mod map;
mod counted_loop;
mod hq;
mod lfo;
mod lfo_volume;
mod glide;
mod host_notes;
pub(crate) mod per_note;
mod instrument;
mod group_start;
mod midi_state;
mod note_mode;
pub use note_mode::NoteMode;
mod pedals;
mod source_note;
mod release;
mod rtc;
#[cfg(all(test, feature = "plugin"))]
mod event_lifetime_tests;
mod player;
pub use host_notes::{HostNote, HostPattern, HostExpression, HostRef};
pub use per_note::{InputAddress, PerNoteInput, PerNoteRef};
pub mod overrides;
pub(crate) mod params;
mod rack;
mod source_delay;
pub use source_delay::SourceDelay;
mod residency;
mod script;
mod commands;
pub(crate) mod native_state;
mod stream;
pub use stream::{StreamError, StreamErrorKind};
mod voice;
// Pure interpolation only; raw preview does not construct an engine voice.
pub(crate) use voice::hermite;
mod wavetable;
pub(crate) mod zone;

pub(crate) use bank::parallel;
pub use audit::audit_dsp;
pub use bank::{Bank, ZoneSkipCounts, GroupSettings, LOAD_DONE, MEMORY_LIMIT, PRELOAD_FRAMES, Streaming, memory_budget, resident_bytes};
pub use params::{Disp, MAX_WRITES, Mod, ModTable, display as engine_par_display, id as engine_par};
pub use residency::{Heads, Residency};
pub use rack::{
    BUSES, Block, BusControls, Mix, NO_AUX, PartControls, Peaks, RACK_SLOTS, Rack, TUNE_RANGE,
};
pub use script::{IrRequest, MAX_COMMANDS, ScriptSetup, effects, load_scripts, load_scripts_with_ir, load_scripts_with_state, load_scripts_with_delay_state};
pub use voice::{Ahdsr, Flex, FlexPoint, Phase};

use crate::fx::FxProcessor;
use crate::ksp::Runtime;
pub use crate::midi::Velocity;
use params::{Address, Write};
use script::{Command, Host};
use stream::Slot;
use player::Player;

/// Voice storage per engine. Polyphony limits steal before this is reached;
/// only a full store forces a hard cut.
pub const MAX_VOICES: usize = 1024;
/// Largest block rendered in one pass; longer requests are split.
pub const MAX_BLOCK: usize = 128;
/// Groups addressable by [`GroupMask`]: Kontakt's per-instrument ceiling.
pub const MAX_GROUPS: usize = 4096;
/// Fade applied to voices stolen by the instrument polyphony limit.
const STEAL_FADE: f32 = 0.005;
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

/// Kontakt from-script source IDs, including ID 1000.
pub const SCRIPT_MOD_SOURCES: usize = 1001;

/// A note start with scripted parameters (KSP play_note and friends).
#[derive(Clone, Copy)]
pub struct NoteEvent<'a> {
    pub channel: u8,
    pub note: u8,
    /// Explicit 7-bit projection for zone selection and KSP.
    pub velocity: u8,
    /// Original normalized attack value, retained for continuous DSP.
    pub onset_velocity: Velocity,
    /// First physical key-up value; synthetic releases have no wire value.
    pub release_velocity: Option<Velocity>,
    /// Physical channel and original engine input key, before KSP transposition.
    pub owner: Option<(u8, u8)>,
    /// Physical input provenance, independent of key-follow lifetime.
    pub input_channel: Option<u8>,
    /// Exact host ownership survives generated children and finite attacks.
    pub host_note: Option<HostRef>,
    /// Native per-note input state, independent of the library's delivered key.
    pub per_note: Option<PerNoteInput>,
    /// Physical key-up may precede application of a queued attack.
    pub counter_stop: Option<u64>,
    /// A release sample's frozen originating event duration in milliseconds.
    pub release_held_ms: Option<f32>,
    /// Groups to consider; `None` uses the engine's allow mask.
    pub groups: Option<&'a GroupMask>,
    /// Final Group system-slot admission already ran before this queued start.
    pub group_start_applied: bool,
    /// Native release-generator provenance (private SYS3), separate from SOURCE/custom.
    pub native_release: bool,
    /// Per-event KSP from-script values in millionths.
    pub script_mods: &'a [i32],
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
        Self::new_velocity(channel, note, Velocity::from_midi1(velocity))
    }

    pub fn new_velocity(channel: u8, note: u8, velocity: Velocity) -> Self {
        Self {
            channel,
            note,
            velocity: velocity.midi1(),
            onset_velocity: velocity,
            release_velocity: None,
            owner: None,
            input_channel: None,
            host_note: None,
            per_note: None,
            counter_stop: None,
            release_held_ms: None,
            groups: None,
            group_start_applied: false,
            native_release: false,
            script_mods: &[],
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
    /// Absolute registered per-note CC74, independent of channel controllers.
    pub note_cc74: Option<crate::midi::ControlValue>,
    /// Raw member controls frozen at physical key-up; manager controls stay live.
    pub member_cc74: Option<crate::midi::ControlValue>,
    /// Frozen CC74 receipt state; `None` retains live channel lookup.
    pub member_cc74_received: Option<bool>,
    pub member_pressure: Option<crate::midi::ControlValue>,
    pub member_bend: Option<crate::midi::Bend>,
    /// Absolute exact-owner pressure, independent of channel pressure.
    pub note_pressure: Option<crate::midi::ControlValue>,
}

impl Default for Expression {
    fn default() -> Self {
        Self { tune: 0.0, gain: 1.0, pan: 0.0, note_cc74: None, member_cc74: None, member_cc74_received: None, member_pressure: None, member_bend: None, note_pressure: None }
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
    /// An authored key-up control edit; physical host capture remains separate.
    ReleaseVelocity(Velocity),
    ScriptMod { source: u16, value: i32 },
}

/// One instrument's playback, script and effect state around a prepared [`Bank`].
pub struct Engine {
    bank: Option<Box<Bank>>,
    /// The installed program’s live controls and group settings.
    state: instrument::InstrumentState,
    /// Program effects, applied to the whole output after the output stage.
    fx: FxProcessor,
    player: Player,
    /// Instrument scripts; when present, MIDI reaches voices only through them.
    script: Option<Box<Runtime>>,
    /// Script engine calls for the next render, ordered by frame.
    commands: commands::Commands,
    /// Script engine parameter changes for the next render, ordered by frame.
    writes: Vec<Write>,
    /// Current-sample parameter positions, allocated before audio processing.
    write_index: (u32, std::collections::HashMap<Address, usize>),
    ir_requests: Vec<IrRequest>,
    offline_zone: Option<(u64,Box<zone::Prepared>)>,
    zone_queued_epoch: u64,
    zone_inflight: usize,
    zone_requests: std::collections::VecDeque<crate::ksp::engine::ZoneEdit>,
    /// Configured part channel for callbacks without a MIDI input owner.
    home_channel: u8,
    /// This engine's sample clock at the beginning of the host callback.
    block_start: u64,
    /// Borrowed peer census, valid only while Rack invokes this engine.
    peer_voices: Option<usize>,
    render_state: Option<RenderState>,
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
    /// Recent render time over block time, set by the host for telemetry.
    /// This estimate must not alter audio, note lifetimes or interaction.
    pub load: f32,
}

/// One outer render owns its callback budget, native span and FX block offsets.
#[derive(Clone, Copy)]
struct RenderState {
    start: u64,
    frames: usize,
    next: usize,
    written: usize,
    block_size: usize,
    block_base: usize,
    block_len: usize,
    defaults: Ahdsr,
    empty: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            bank: None,
            state: instrument::InstrumentState::default(),
            fx: FxProcessor::default(),
            player: Player::new(48000.0),
            script: None,
            commands: commands::Commands::default(),
            writes: Vec::with_capacity(MAX_WRITES),
            write_index: (0, std::collections::HashMap::with_capacity(MAX_WRITES)),
            ir_requests: Vec::with_capacity(32),
            zone_requests: Default::default(),
            zone_inflight: 0,
            zone_queued_epoch: 0,
            offline_zone: None,
            home_channel: 0,
            block_start: 0,
            peer_voices: None,
            render_state: None,
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
        self.clear_note_modes(false);
        self.choke_host_notes();
        self.cleanup_script_input_notes();
        self.player.clear_voices(self.bank.as_deref());
        self.player.touch();
        self.observe_pending_notes();
        self.commands.clear();
        self.writes.clear();
        self.write_index.1.clear();
        self.ir_requests.clear();
        self.zone_requests.clear();
        self.zone_inflight = 0;
        self.zone_queued_epoch = 0;
        let mut bank = bank;
        if let Some(bank) = bank.as_deref_mut() {
            std::mem::swap(&mut self.zone_requests, &mut bank.zone_spares);
        }
        let state = bank.as_deref_mut().map(|b| b.initial_state.take().expect("prepared bank state"))
            .unwrap_or_default();
        let retired = std::mem::replace(&mut self.state, state);
        let mut old = std::mem::replace(&mut self.bank, bank);
        self.player.midi.clear_modulation_receipts();
        self.player.group_clock = self.bank.as_deref().map_or_else(Default::default, |b| b.group_start.state());
        if let Some(old) = old.as_deref_mut() {
            old.initial_state = Some(retired);
            std::mem::swap(&mut self.player.route_spares, &mut old.route_spares);
        }
        if let Some(bank) = self.bank.as_deref_mut() {
            std::mem::swap(&mut self.player.route_spares, &mut bank.route_spares);
        }
        self.queue_init_zone_edits();
        self.player.free.clear();
        let slots = self.bank.as_deref().map_or(0, |b| b.slots().len());
        self.player.free.extend((0..slots as u16).rev());
        self.replay(|address| address.is_group() || matches!(address, Address::Instrument(_)));
        self.refresh_overrides();
        // New source prototypes use current raw BPM, even inside the retained global threshold.
        self.state.lfo_timings.refresh(&self.state.settings, None, self.player.rate as f32, self.player.tempo);
        self.reconcile_native_sustain();
        old
    }

    /// Replace the bank with `bank`, built from the same instrument with
    /// other residency (the RAM-only fill finishing): playing voices carry
    /// on from the new bank's data, and script-set group parameters stay.
    /// A bank with other zones (a sample read in one load only) installs as
    /// [`Engine::set_bank`] does. Returns the old bank for disposal off the
    /// audio thread.
    pub fn upgrade_bank(&mut self, mut bank: Box<Bank>) -> Option<Box<Bank>> {
        if !self.zone_upgrade_ready(&bank) {
            return Some(bank);
        }
        let Some(old) = self.bank.as_deref_mut().filter(|old| {
            !bank.prepared_settings_edited
                && (self.state.note_modes.iter().all(|mode| *mode == NoteMode::Poly)
                    || note_mode::resident_sources(&bank))
                && old.zones().len() == bank.zones().len() && old.samples.len() == bank.samples.len()
                && old.samples.iter().zip(&bank.samples).all(|(old, new)|
                    old.rate == new.rate && old.frames == new.frames)
                && old.groups().len() == bank.groups().len()
                && self.state.base.len() == bank.groups().len()
                && self.state.settings.len() == bank.groups().len()
                && old.group_start == bank.group_start
                && old.counted_loops == bank.counted_loops
                && old.plays.iter().zip(bank.plays.iter()).all(|(old,new)| old.counted == new.counted)
                // Residency fills retain the original source geometry. Compare
                // immutable bank records: authored depths/bypass and editor
                // overrides can legitimately change both live settings layers.
                // Malformed NaN source records conservatively fail compatibility.
                && old.groups().iter().zip(bank.groups()).all(|(old, new)|
                    old.source_mode == new.source_mode
                    && old.interp_quality == new.interp_quality
                    && old.pitch_lfos == new.pitch_lfos
                    && old.pitch_glides == new.pitch_glides
                    && old.volume_lfos == new.volume_lfos
                    && old.volume_env == new.volume_env
                    && old.flex_env == new.flex_env
                    && old.native_flex_oneshot == new.native_flex_oneshot
                    && old.native_flex_loop == new.native_flex_loop
                    && old.native_volume_env == new.native_volume_env
                    && old.native_shared_volume_slot == new.native_shared_volume_slot
                    && old.native_primary_bypass_slot == new.native_primary_bypass_slot
                    && old.envelopes == new.envelopes
                    && old.modulators == new.modulators
                    && old.mods == new.mods)
                && bank.initial_state.as_ref().is_some_and(|prepared|
                    prepared.controller_options == self.state.controller_options
                    && prepared.controller_policy == self.state.controller_policy
                    && prepared.sampling_profile == self.state.sampling_profile
                    && prepared.settings.len() == self.state.settings.len()
                    && self.state.settings.iter().zip(&prepared.settings).all(|(live, seed)|
                        live.primary_bypass.map(|p| p.0) == seed.primary_bypass.map(|p| p.0)
                        && live.mods.same_routes(&seed.mods) && match (&live.filter, &seed.filter) {
                            (Some(live), Some(seed)) => live.same_external_routes(seed),
                            (None, None) => true,
                            _ => false,
                        }))
        }) else {
            return self.set_bank(Some(bank));
        };
        // Live state remains owned by Engine. Retire the unused prepared
        // seed with the old bank for disposal away from the audio thread.
        std::mem::swap(&mut old.initial_state, &mut bank.initial_state);
        // Live voice rows stay with the player; retire unused preparation.
        std::mem::swap(&mut old.route_spares, &mut bank.route_spares);
        // Parent proof is retired with the old bank, never released on audio.
        std::mem::swap(&mut old.zone_parent, &mut bank.zone_parent);
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
            if in_use.binary_search(&sample).is_err() {
                bank.swap_sample_head(head);
            }
        }
    }

    /// Install the program effects, built for [`rate`](Self::rate) with
    /// blocks of [`MAX_BLOCK`]; returns the previous processor for disposal
    /// off the audio thread.
    pub fn set_fx(&mut self, mut fx: FxProcessor) -> FxProcessor {
        fx.set_tempo(self.player.tempo);
        let old = std::mem::replace(&mut self.fx, fx);
        self.replay(|a| matches!(a, Address::Fx(..)));
        if let Some(rt) = self.script.as_deref() { rt.native_state.replay_fx(&mut self.fx); }
        old
    }

    /// Install the instrument scripts, initialized off the audio thread; returns
    /// the previous runtime for disposal there. `None` plays MIDI directly.
    /// Engine parameters the scripts set in `on init` apply now, and again to
    /// banks and effects installed later.
    pub fn set_script(&mut self, mut script: Option<Box<Runtime>>) -> Option<Box<Runtime>> {
        self.clear_note_modes(true);
        // An old callback/root cannot migrate into the replacement runtime.
        // Close only exact owners; anonymous MIDI behavior stays unchanged.
        self.choke_host_notes();
        self.observe_pending_notes();
        self.commands.clear();
        self.writes.clear();
        self.write_index.1.clear();
        self.ir_requests.clear();
        self.zone_requests.clear();
        self.zone_inflight = 0;
        self.zone_queued_epoch = 0;
        let channel = self.service_channel();
        if let Some(rt) = script.as_deref_mut() {
            rt.set_sample_rate(self.player.rate);
            rt.set_service_channel(channel);
        }
        let old = std::mem::replace(&mut self.script, script);
        self.player.midi.set_defaults(self.script.as_deref().map_or(&[], |rt| &rt.init_controllers));
        self.player.native_sustain = !self.script.as_ref().is_some_and(|rt| rt.condition("NO_SYS_SCRIPT_PEDAL"));
        self.player.native_mpe_pressure = !self.script.as_ref().is_some_and(|rt| rt.handles_poly_at());
        self.player.native_release_triggers = !self.script.as_ref().is_some_and(|rt| rt.condition("NO_SYS_SCRIPT_RLS_TRIG"));
        self.player.native_group_start = !self.script.as_ref().is_some_and(|rt| rt.condition("NO_SYS_SCRIPT_GROUP_START"));
        if !self.player.native_release_triggers { self.player.pending_releases.clear(); }
        self.reconcile_native_sustain();
        self.replay(|_| true);
        if let Some(rt) = self.script.as_deref() { rt.native_state.replay_delays(&mut self.fx); }
        self.apply_init_controllers();
        self.queue_init_zone_edits();
        old
    }

    fn reconcile_native_sustain(&mut self) {
        // The script override and installed instrument policy both control
        // native holds; physical CC64 remains available to authored routes.
        let defaults = self.defaults();
        for channel in 0..16 {
            let on = self.player.native_sustain && self.state.uses_sustain()
                && self.player.midi.cc[channel][64].normalized() >= 0.5;
            if self.player.pedals.set_sustain(channel as u8, on) && let Some(bank) = self.bank.as_deref() {
                self.player.pedal_up(bank, &self.state, channel as u8, defaults);
            }
        }
    }

    /// Set the controllers the scripts set while loading, on every channel.
    fn apply_init_controllers(&mut self) {
        let Some(rt) = self.script.as_deref() else {
            return;
        };
        let defaults = self.defaults();
        for &(cc, value) in &rt.init_controllers {
            if let Some(control) = crate::midi::Control::from_legacy(cc, i32::from(value)) {
                self.player.library_receiver.control(self.home_channel, control);
                self.player.control(self.bank.as_deref(), &self.state, self.home_channel,
                    crate::midi::ControlTarget::Authored, control, defaults);
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
                self.write_parameter(address, self.state.controller_policy.decode(address, value), value);
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
                if self.state.controller_policy.native_control(address).is_some() {
                    self.player.authored_instrument_control(p);
                    self.state.set_control_value(p, None);
                }
                self.state.controls.write(p, value)
            },
            _ => self.write_group(address, value),

        }
    }

    fn write_parameter(&mut self, address: Address, value: f32, native: i32) -> bool {
        if !self.write(address, value) { return false; }
        if let Some(param) = self.state.controller_policy.native_control(address) {
            self.state.set_control_value(param, Some(native.clamp(0, 1_000_000)));
        }
        true
    }

    fn apply_write(&mut self, w: params::Write, defaults: Ahdsr, next_command: usize) {
        if let Some(param) = self.state.controller_policy.native_control(w.address) {
            // Keep parameter-before-note configuration, but consume earlier
            // same-sample standard actions before the scalar that replaces
            // them. The ordinary command pass must not apply them twice.
            let mut remaining = w.standard_controls_before;
            for index in next_command..self.commands.len() {
                let c = *self.commands.get(index).unwrap();
                if c.at > w.at { break; }
                if c.standard_control(param).is_none() { continue; }
                if c.at == w.at {
                    if remaining == 0 { break; }
                    remaining -= 1;
                }
                self.apply_command(index, defaults);
            }
        }
        let physical_only = w.standard_physical.is_some() && !w.standard_authored;
        if !physical_only && self.write_parameter(w.address, w.value, w.native)
            && let Some(rt) = self.script.as_deref_mut() {
            rt.native_state.capture(w.address, w.par, w.native, w.value);
            if let Address::Fx(rack, slot, crate::fx::FxParam::Field(crate::fx::Kind::Delay, 0 | 4)) = w.address
                && let Some((fields, _)) = self.fx.delay_fields(rack, slot) {
                rt.native_state.capture_delay(crate::fx::DelayState::from_fields(rack, slot, &fields), w.par);
            }
        }
        if let Some(value) = w.standard_physical
            && let Address::Instrument(param) = w.address { self.player.native_instrument_control(param, value); }
    }

    fn apply_command(&mut self, index: usize, defaults: Ahdsr) {
        if self.commands.get(index).is_some_and(|command| !command.applied
            && matches!(command.kind, script::Kind::Control { control: crate::midi::Control::Cc(120, _), .. })) {
            self.mark_host_notes();
        }
        self.commands.apply(index, |command, note| {
            self.player.apply(self.bank.as_deref(), &self.state, command, note, defaults);
        });
    }

    /// Shared renderer/direct-admission boundary: writes configure same-frame starts.
    fn apply_pending_at(&mut self, at: u32, defaults: Ahdsr, next: &mut usize, written: &mut usize) {
        while let Some(&write) = self.writes.get(*written).filter(|write| write.at <= at) {
            self.apply_write(write, defaults, *next);
            *written += 1;
        }
        while self.commands.get(*next).is_some_and(|command| command.at <= at) {
            self.apply_command(*next, defaults);
            *next += 1;
        }
    }

    /// Direct starts belong after earlier queued arrivals at this same audio frame.
    /// Future waits retain their timestamps; applied commands retain chronology.
    fn admit_direct_start(&mut self) {
        let (mut next, mut written) = (0, 0);
        self.apply_pending_at(0, self.defaults(), &mut next, &mut written);
        if written != 0 {
            self.writes.drain(..written);
            self.write_index.1.clear();
        }
    }

    pub fn script(&self) -> Option<&Runtime> {
        self.script.as_deref()
    }

    pub fn begin_audio_block(&mut self, frames: usize, parts: usize, offline: bool) {
        if let Some(range) = &mut self.state.shared_ahdsr {
            range.invalidate("shared AHDSR awaits this host range's accepted receipts");
        }
        self.block_start = self.player.now;
        self.player.release_counter.begin(self.player.now, frames, self.player.rate);
        if let Some(rt) = &mut self.script {
            rt.begin_audio_block(frames, parts, offline);
        }
    }

    /// Apply the host's transport before processing this block's callbacks.
    pub fn set_transport(&mut self, playing: bool, tempo: f64, beats: f64, signature: (u8, u8)) {
        if tempo.is_finite() && tempo > 0.0 && (tempo as f32).is_finite() && tempo as f32 > 0.0 {
            self.player.tempo = tempo as f32;
            self.fx.set_tempo(self.player.tempo);
            if self.player.native_refresh_tempo.is_none_or(|previous| {
                let difference = self.player.tempo - previous;
                difference < -0.001f32 || difference > 0.001f32
            }) {
                self.player.native_refresh_tempo = Some(self.player.tempo);
                self.state.lfo_timings.refresh(&self.state.settings, None, self.player.rate as f32, self.player.tempo);
            }
        }
        if let Some((rt, mut host)) = self.scripted(self.service_channel()) {
            rt.set_host_transport(&mut host, playing, tempo, beats, signature);
        }
    }

    /// The instrument's effects, and what it played past its own output.
    pub fn fx(&self) -> &FxProcessor {
        &self.fx
    }

    pub fn bank(&self) -> Option<&Bank> {
        self.bank.as_deref()
    }

    /// Effective settings of the installed program.
    pub fn settings(&self) -> Option<&[GroupSettings]> {
        self.bank.as_ref().map(|_| self.state.settings.as_slice())
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
        self.reset_inner(rate, None);
    }

    /// Retire the replaced input's exact identities through the host's normal
    /// NOTE_END/retry path. A new port also starts new negotiated MIDI state.
    pub(super) fn reset_input(&mut self, rate: f64, home: u8, port_changed: bool) {
        self.reset_inner(rate, Some((home, port_changed)));
    }

    fn reset_inner(&mut self, rate: f64, input: Option<(u8, bool)>) {
        self.clear_note_modes(false);
        self.player.release_counter.close(self.player.now, self.player.rate);
        self.player.midi.reset_received(u16::MAX);
        self.mark_host_notes();
        self.player.host_notes.stop(None, None, self.player.now);
        self.release_script_pedals();
        self.cleanup_script_input_notes();
        self.player.clear_voices(self.bank.as_deref());
        self.observe_pending_notes();
        self.commands.clear();
        self.writes.clear();
        self.write_index.1.clear();
        self.fx.clear();
        self.state.program_amplifier.idle();
        self.player.reset_midi();
        if input.is_none() { self.player.host_notes.reset(); }
        if let Some((home, port_changed)) = input {
            // Cleanup uses the old delivery channel. Its writes cannot seed the
            // replacement port; authored initialization belongs to the new home.
            if port_changed {
                self.player.midi.reset_input_configuration();
                self.player.library_receiver.clear();
            }
            self.set_home_channel(home);
        }
        self.player.rate = rate;
        self.state.lfo_timings.refresh(&self.state.settings, None, rate as f32, self.player.tempo);
        if let Some(rt) = self.script.as_deref_mut() {
            rt.set_sample_rate(rate);
        }
        self.apply_init_controllers();
        if let Some(rt) = self.script.as_deref_mut() { rt.reset_controllers(Some(&self.player.midi.midi1_cc()[0])); }
        // A script's saved/default pedal-down value is restored with its other
        // init controllers. Its private cache must see that restored value too.
        self.notify_script_pedals([64usize, 66].map(|cc| {
            let value = self.player.midi.cc[0][cc].midi1();
            (value >= 64).then_some(value)
        }));
        self.cleanup_script_input_notes();
    }

    /// Accepted first dispatches survive cancellation of their queued voices.
    /// Commands already have stable timestamp order; never fabricate dropped rows.
    fn observe_pending_notes(&mut self) {
        let history = &mut self.player.note_history;
        self.commands.apply_observations(|note| history.observe(note));
    }

    fn cancel_notes(&mut self, channels: u16) {
        if let Some((rt, mut host)) = self.scripted(self.service_channel()) { rt.all_sound_off(&mut host, channels); }
        // Controller commands can carry UI edits; stopping notes must not lose them.
        self.commands.retain(|command, _| channels & (1 << command.channel) == 0
            || matches!(command.kind, script::Kind::Control { .. } | script::Kind::ObserveNote(_)));
    }

    fn cleanup_script_notes(&mut self) {
        if let Some((rt, mut host)) = self.scripted(0) { rt.cleanup_notes(&mut host); }
        // Cleanup runs under the normal callback budget; old performance
        // commands are cut now, and its resumed callbacks cannot start notes.
        self.commands.retain(|command, _| matches!(command.kind, script::Kind::Control { .. } | script::Kind::ObserveNote(_)));
    }

    fn cleanup_script_input_notes(&mut self) {
        if let Some((rt, mut host)) = self.scripted(0) { rt.cleanup_notes_from(&mut host, u16::MAX); }
        self.commands.retain(|command, _| command.input_channel.is_none()
            || matches!(command.kind, script::Kind::Control { .. }));
    }

    fn notify_script_pedals(&mut self, values: [Option<u8>; 2]) {
        if self.script.is_none() { return; }
        for channel in 0..16 {
            for (cc, value) in [64, 66].into_iter().zip(values) {
                if let Some(value) = value { self.cc(channel, cc, value); }
            }
        }
    }

    fn release_script_pedals(&mut self) {
        // Libraries cache pedal state in their own globals. Resetting %CC alone
        // leaves their next release callback believing the pedal is still down.
        if let Some(rt) = self.script.as_ref() {
            let down = [64usize, 66].map(|cc| rt.env.input.cc[cc] >= 64
                || self.player.midi.cc.iter().any(|row| row[cc].normalized() >= 0.5));
            self.notify_script_pedals(down.map(|down| down.then_some(0)));
        }
    }

    /// Stop every performance context in one pass, preserving script/UI setup.
    pub fn panic(&mut self) {
        self.player.midi.reset_received(u16::MAX);
        self.mark_host_notes();
        self.player.host_notes.stop(None, None, self.player.now);
        self.release_script_pedals();
        // Pedal-up callbacks may release, create or wait on notes; cancel their
        // remaining performance work too, so Panic cannot resurrect a voice.
        self.cleanup_script_notes();
        self.commands.retain(|command, _| !matches!(command.kind,
            script::Kind::Control { control: crate::midi::Control::Cc(1 | 11 | 64 | 66, _) | crate::midi::Control::Bend(_) | crate::midi::Control::Pressure(_), .. }));
        let defaults = self.defaults();
        for channel in 0..16 {
            self.player.cc(self.bank.as_deref(), &self.state, channel, 120, 0, defaults);
            self.player.cc(self.bank.as_deref(), &self.state, channel, 121, 0, defaults);
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
                gain: v.audible_gains().into_iter().map(f32::abs).fold(0.0, f32::max),
                envelope: v.env.level() * v.flex.as_ref().map_or(1.0, |f| f.level()) * v.fade.value(),
                sample: v.sample,
                wavetable: v.wavetable.map(|table| (table.first, table.cycles)),
                pos: v.pos,
                physical_pos: v.physical_pos(),
                loop_control_window: v.base_loop.and(v.map.looped).map(|l| (l.start, l.end, l.xfade)),
                step: v.step * v.tune * v.pitch.1
                    * self.player.midi.tuning[v.input_channel.unwrap_or(v.channel) as usize].ratio(),
                loop_control_deferred: v.loop_deferred.is_some(),
                loop_deferred_edits: v.loop_deferred_edits,
                filtered: self.bank.is_some() && self.state.settings[v.group as usize].filter.is_some(),
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
        self.player.dropped_commands.saturating_add(self.player.per_note.dropped)
    }

    pub(crate) fn unsupported_native_controls(&self) -> u64 {
        self.player.unsupported_native_controls
    }

    /// The runtime and its engine view, borrowed apart so scripts can drive voices.
    fn scripted_from(&mut self, channel: u8, input_channel: u8) -> Option<(&mut Runtime, Host<'_>)> {
        let (rt, host) = self.scripted(channel)?;
        rt.set_input_channel(input_channel);
        Some((rt, host))
    }

    fn scripted(&mut self, channel: u8) -> Option<(&mut Runtime, Host<'_>)> {
        let rt = self.script.as_deref_mut()?;
        rt.set_midi_channel(channel);
        let host = Host {
            frame_base: self.player.now,
            peer_voices: self.peer_voices,
            channel,
            bank: self.bank.as_deref(),
            state: &self.state,
            fx: &self.fx,
            player: &mut self.player,
            commands: &mut self.commands,
            writes: &mut self.writes,
            write_index: &mut self.write_index,
            ir_requests: &mut self.ir_requests,
            zone_requests: &mut self.zone_requests,
            zone_inflight: &mut self.zone_inflight,
        };
        Some((rt, host))
    }

    /// Note input takes effect at the start of the next [`render`](Self::render).
    pub(crate) fn admit_host_note(&mut self, note:HostNote) -> bool {
        if self.player.host_notes.admits(note) {
            return self.admit_input_note(InputAddress { port: note.port, channel: note.channel, key: note.key });
        }
        self.player.host_notes.dropped=self.player.host_notes.dropped.saturating_add(1); false
    }

    pub(crate) fn admit_input_note(&mut self, address: InputAddress) -> bool {
        if !self.shared_input_admitted() { self.reject_unsealed_source(); return false; }
        self.mark_per_notes();
        self.player.per_note.attach(address).is_some()
    }

    /// Exact host ingress. The route is captured once, before KSP can transpose.
    #[cfg(test)]
    pub(crate) fn host_note_on(&mut self, note: HostNote, channel: u8, key: u8, velocity: u8, tune: f32) -> bool {
        if velocity > 127 { return false; }
        self.host_note_on_velocity(note, channel, key, Velocity::from_midi1(velocity), tune)
    }

    pub(crate) fn host_note_on_velocity(&mut self, note: HostNote, channel: u8, key: u8, velocity: Velocity, tune: f32) -> bool {
        self.host_note_on_velocity_inner(note, channel, key, velocity, tune, true)
    }

    // The prepared bank's first slice rejects address-current per-note controls;
    // exact host overlays still belong to its canonical HostRef generation.
    fn host_note_on_velocity_inner(&mut self, note: HostNote, channel: u8, key: u8,
        velocity: Velocity, tune: f32, per_note_input: bool) -> bool {
        if !self.shared_input_admitted() { self.reject_unsealed_source(); return false; }
        if !tune.is_finite() { return false; }
        if note.channel >= 16 || note.key >= 128 || channel >= 16 || key >= 128 { return false; }
        if self.script.is_none() { self.admit_direct_start(); }
        let per_note = if per_note_input {
            self.mark_per_notes();
            let Some(input) = self.player.per_note.input(InputAddress { port: note.port, channel: note.channel, key: note.key }, self.player.midi.per_note_range[note.channel as usize], None) else { return false; };
            Some(input)
        } else { None };
        let event = self.player.next_id();
        let Some(host_note) = self.player.host_notes.allocate(note, channel, key, velocity, event, self.player.now) else { return false };
        self.player.host_notes.get_mut(host_note).unwrap().per_note = per_note;
        self.player.host_notes.change(host_note, HostExpression::Tune(tune));
        self.player.keys[channel as usize][key as usize] = velocity.midi1().max(1);
        self.player.key_on[channel as usize][key as usize] = self.player.now;
        if let Some((rt, mut host)) = self.scripted_from(channel, note.channel) {
            rt.note_on_owned_velocity(&mut host, 0, note.channel, key, velocity, Some(host_note), per_note);
        } else {
            let Some(bank) = self.bank.as_deref() else { return true; };
            let defaults = self.defaults();
            let ev = NoteEvent { host_note: Some(host_note), per_note, owner: Some((note.channel, key)),
                input_channel: Some(note.channel), ..NoteEvent::new_velocity(channel, key, velocity) };
            self.player.note_history.observe(key);
            self.player.start_input(bank, &self.state, &ev, event, note_mode::Root::Host(host_note), defaults);
        }
        true
    }

    fn release_host_note(&mut self, id: HostRef) {
        self.release_host_note_with_velocity(id, None);
    }

    fn release_host_note_with_velocity(&mut self, id: HostRef, release_velocity: Option<Velocity>) {
        let Some(owner) = self.player.host_notes.get(id).copied().filter(|o| o.held) else { return };
        let Some(event) = owner.event.live() else { return };
        let o = self.player.host_notes.get_mut(id).unwrap();
        if let Some(value) = release_velocity { o.release_velocity.get_or_insert(value); }
        let release_velocity = o.release_velocity;
        o.held = false; o.stop = Some(self.player.now);
        for v in self.player.voices.iter_mut().filter(|v| v.host_note == Some(id) && v.held && !v.release_trigger) {
            v.counter_stop.get_or_insert(self.player.now);
            if let Some(value) = release_velocity {
                v.release_velocity.get_or_insert(value);
                v.settled = None;
            }
        }
        self.commands.for_each_start_mut(|_, start| {
            if start.host_note == Some(id) {
                start.counter_stop.get_or_insert(self.player.now);
                if let Some(value) = release_velocity { start.release_velocity.get_or_insert(value); }
            }
        });
        if self.player.midi.mpe.member(owner.note.channel).is_some() {
            let expression = self.release_snapshot(owner.channel, owner.note.channel, owner.key);
            for v in self.player.voices.iter_mut().filter(|v| v.host_note == Some(id) && !v.release_trigger) { v.frozen_expression.get_or_insert(expression); }
            self.commands.for_each_start_mut(|_, start| {
                if start.host_note == Some(id) { start.expression.get_or_insert(expression); }
            });
        }
        // A newer same-key root owns the shared key indicator, not this release.
        let remaining = self.player.host_notes.active.iter().filter_map(|r| self.player.host_notes.get(*r))
            .filter(|o| o.held && o.channel == owner.channel && o.key == owner.key).map(|o| o.velocity.midi1().max(1)).max().unwrap_or(0);
        self.player.keys[owner.channel as usize][owner.key as usize] = remaining;
        if remaining == 0 { self.player.key_up[owner.channel as usize][owner.key as usize] = self.player.now; }
        if let Some((rt, mut host)) = self.scripted_from(owner.channel, owner.note.channel) {
            if let Some(value) = release_velocity {
                rt.note_off_host_velocity(&mut host, id, owner.note.channel, owner.key, value);
            } else { rt.note_off_host(&mut host, id, owner.note.channel, owner.key); }
        } else if !owner.silenced && let Some(bank) = self.bank.as_deref() {
            let defaults = self.defaults();
            let expression = self.player.release_expression(event, owner.channel, owner.key, Some(owner.note.channel));
            let allowed = self.player.allowed;
            self.player.accept_release(bank, &self.state, release::ReleaseRequest {
                source: event,
                release_velocity,
                capture: release::Capture::RootAdditional(owner.sostenuto),
                sample: Some(release::ReleaseSample::Snapshot(release::ReleaseSnapshot {
                    channel: owner.channel, note: owner.key, velocity: owner.velocity, groups: &allowed,
                    expression, input_channel: Some(owner.note.channel), host_note: Some(id),
                    per_note: owner.per_note, script_mods: None, held_ms: None,
                })),
            }, defaults);
        }
        let anonymous = self.player.input_keys.iter().filter(|r| r[owner.key as usize].channel == owner.channel)
            .filter_map(|r| r[owner.key as usize].onset_velocity.map(|v| v.midi1().max(1))).max().unwrap_or(0);
        let scripted = self.script.as_deref().is_some_and(|rt| (0..16).any(|input| rt.key_down_from(input,owner.channel,owner.key)));
        self.player.keys[owner.channel as usize][owner.key as usize] = remaining.max(anonymous).max(u8::from(scripted));
        if self.script.is_none() && let Some(bank) = self.bank.as_deref() {
            let defaults = self.defaults();
            self.player.host_key_up(bank, &self.state, owner.channel, id, defaults);
        }
    }

    pub(crate) fn host_note_off(&mut self, pattern: HostPattern) {
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            if self.player.host_notes.get(id).is_some_and(|o| pattern.matches(o.note)) { self.release_host_note(id); }
        }
    }

    pub(crate) fn host_note_off_velocity(&mut self, pattern: HostPattern, velocity: Velocity) {
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            if self.player.host_notes.get(id).is_some_and(|o| pattern.matches(o.note)) {
                self.release_host_note_with_velocity(id, Some(velocity));
            }
        }
    }

    fn choke_host_notes(&mut self) {
        if self.player.host_notes.active.is_empty() { return; }
        for clap in [true,false] { self.host_note_choke(HostPattern { port:-1, channel:-1, key:-1, id:-1, clap }); }
    }

    pub(crate) fn host_note_choke(&mut self, pattern: HostPattern) {
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            let Some(owner) = self.player.host_notes.get(id).copied().filter(|o| pattern.matches(o.note)) else { continue };
            let o = self.player.host_notes.get_mut(id).unwrap();
            o.held = false; o.silenced = true; o.stop.get_or_insert(self.player.now);
            if let Some((rt, mut host)) = self.scripted_from(owner.channel, owner.note.channel) { rt.choke_host_note(&mut host, id); }
            self.commands.retain(|c, note| match c.kind {
                script::Kind::Start(_) | script::Kind::Release(_) => note.is_none_or(|n| n.host_note() != Some(id)),
                script::Kind::HostPressure(owner, _) => owner != id,
                _ => true,
            });
            self.player.pending_releases.retain(|r| r.host_note != Some(id));
            let fade = self.player.fade_frames(STEAL_FADE);
            for v in self.player.voices.iter_mut().filter(|v| v.host_note == Some(id)) { v.fade.start(0., fade, true); v.held = false; }
            let remaining = self.player.host_notes.active.iter().filter_map(|r| self.player.host_notes.get(*r))
                .filter(|o| o.held && o.channel == owner.channel && o.key == owner.key).map(|o| o.velocity.midi1().max(1)).max().unwrap_or(0);
            let anonymous = self.player.input_keys.iter().filter(|row| row[owner.key as usize].channel == owner.channel)
                .filter_map(|row| row[owner.key as usize].onset_velocity.map(|v| v.midi1().max(1))).max().unwrap_or(0);
            let scripted = self.script.as_deref().is_some_and(|rt| (0..16).any(|input| rt.key_down_from(input,owner.channel,owner.key)));
            self.player.keys[owner.channel as usize][owner.key as usize] = remaining.max(anonymous).max(u8::from(scripted));
        }
    }

    pub(crate) fn host_expression(&mut self, pattern: HostPattern, expression: HostExpression) {
        let mut changed = false;
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            if self.player.host_notes.get(id).is_some_and(|o| pattern.matches(o.note)) {
                changed |= self.player.host_notes.change(id, expression);
            }
        }
        if changed { self.player.touch(); }
    }

    /// Wire per-note brightness follows retained physical host addresses,
    /// including notes whose script channel or pitch changed after input.
    /// While exact owners exist, an unmatched address must not fall through
    /// to the legacy shared expression row and alter a different owner.
    pub(crate) fn per_note_manage(&mut self, address: InputAddress, detach: bool, reset: bool) -> bool {
        self.mark_per_notes();
        let accepted = self.player.per_note.manage(address, detach, reset);
        if accepted { self.player.touch(); }
        accepted
    }

    /// Wire per-note controls keep their original generation through script waits.
    pub(crate) fn note_controller_from(&mut self, channel: u8, note: u8, address: InputAddress,
        value: crate::midi::NoteControl) -> bool {
        if channel >= 16 || note >= 128 || address.channel >= 16 || address.key >= 128 { return false; }
        if self.script.is_none() && !per_note::InputControls::supports_controller(value) {
            self.player.unsupported_native_controls = self.player.unsupported_native_controls.saturating_add(1);
            return false;
        }
        self.mark_per_notes();
        let range = self.player.midi.per_note_range[address.channel as usize];
        let Some(input) = self.player.per_note.input(address, range, None) else { return false; };
        if let Some((rt, mut host)) = self.scripted_from(channel, address.channel) {
            rt.native_note_controller(&mut host, 0, note, input, value);
            true
        } else {
            let accepted = self.player.per_note.note_controller(input, value);
            if accepted { self.player.touch(); }
            accepted
        }
    }

    fn mark_per_notes(&mut self) {
        if !self.player.per_note.needs_census() { return; }
        self.player.per_note.begin_mark();
        for key in self.player.input_keys.iter().flatten().filter(|key| key.onset_velocity.is_some()) {
            self.player.per_note.pin(key.per_note.map(|input| input.controls));
        }
        for v in &self.player.voices { self.player.per_note.pin(v.per_note.map(|input| input.controls)); }
        for r in &self.player.pending_releases { self.player.per_note.pin(r.per_note.map(|input| input.controls)); }
        for id in &self.player.host_notes.active {
            if let Some(owner) = self.player.host_notes.get(*id) {
                self.player.per_note.pin(owner.per_note.map(|input| input.controls));
            }
        }
        for command in self.commands.iter() {
            let input = match command.kind {
                script::Kind::Start(_) | script::Kind::Release(_) => self.commands.note(command.kind).and_then(commands::Note::per_note),
                script::Kind::PerNotePressure(input, _) | script::Kind::NoteController(input, _) => Some(input),
                _ => None,
            };
            self.player.per_note.pin(input.map(|input| input.controls));
        }
        if let Some(rt) = self.script.as_deref() { rt.visit_per_notes(|id| self.player.per_note.pin(Some(id))); }
        self.player.per_note.finish_mark();
    }

    /// Exact native pressure still runs the instrument's polyphonic callback.
    pub(crate) fn host_pressure(&mut self, pattern: HostPattern, value: crate::midi::ControlValue) {
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            let Some(owner) = self.player.host_notes.get(id).copied().filter(|o| pattern.matches(o.note)) else { continue; };
            if let Some((rt, mut host)) = self.scripted_from(owner.channel, owner.note.channel) {
                rt.poly_pressure_host(&mut host, 0, owner.key, value, id);
            } else if self.player.host_notes.change(id, HostExpression::Pressure(value)) { self.player.touch(); }
        }
    }

    pub(crate) fn audio_block_start(&self) -> u64 { self.block_start }

    /// Latest audible or callback completion, floored at physical key-up.
    pub(crate) fn host_note_end_at(&self, note: HostNote) -> Option<u64> {
        self.player.host_notes.active.iter().filter_map(|id| self.player.host_notes.get(*id))
            .filter(|owner| owner.note == note && !owner.pinned)
            .map(|owner| owner.pending_end.unwrap_or(owner.start).max(owner.stop.unwrap_or(owner.start)))
            .max()
    }

    /// Attached audio completion, independent of the held host NOTE_END gate.
    #[cfg(test)]
    pub(crate) fn host_audio_end_at(&self, note: HostNote) -> Option<u64> {
        self.player.host_notes.active.iter().filter_map(|id| self.player.host_notes.get(*id))
            .filter(|owner| owner.note == note)
            .filter_map(|owner| owner.pending_end)
            .max()
    }

    /// Census at host completion and before terminal stops, only with exact owners.
    /// Held, callback, queued, pedal and sounding references all delay NOTE_END.
    pub(crate) fn mark_host_notes(&mut self) {
        self.mark_per_notes();
        if self.player.host_notes.active.is_empty() { return; }
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            if let Some(o) = self.player.host_notes.get_mut(id) { o.pinned = o.held; }
        }
        for v in &self.player.voices { self.player.host_notes.pin(v.host_note); }
        for r in &self.player.pending_releases { self.player.host_notes.pin(r.host_note); }
        for c in self.commands.iter().filter(|command| !command.applied) {
            match c.kind {
                script::Kind::Start(_) | script::Kind::Release(_) => self.player.host_notes.pin(self.commands.note(c.kind).and_then(commands::Note::host_note)),
                script::Kind::HostPressure(owner, _) => self.player.host_notes.pin(Some(owner)),
                _ => {}
            }
        }
        if let Some(rt) = self.script.as_deref() { rt.visit_host_notes(|r| self.player.host_notes.pin(Some(r))); }
    }

    pub(crate) fn host_note_at(&self, index: usize) -> Option<(HostNote, bool)> {
        self.player.host_notes.active.get(index).and_then(|r| self.player.host_notes.get(*r)).map(|o| (o.note, o.pinned))
    }
    pub(crate) fn host_note_present(&self, note:HostNote) -> bool {
        self.player.host_notes.active.iter().any(|r| self.player.host_notes.get(*r).is_some_and(|o| o.note==note))
    }
    pub(crate) fn host_note_pending(&self, note: HostNote) -> bool {
        self.player.host_notes.active.iter().any(|r| self.player.host_notes.get(*r).is_some_and(|o| o.note == note && o.pinned))
    }
    pub(crate) fn retire_host_note(&mut self, note: HostNote) {
        let mut i = 0;
        while i < self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            if self.player.host_notes.get(id).is_some_and(|o| o.note == note && !o.pinned) { self.player.host_notes.retire(id); }
            else { i += 1; }
        }
    }
    pub(crate) fn host_key_held(&self, channel: u8, key: u8) -> bool {
        self.player.host_notes.active.iter().any(|r| self.player.host_notes.get(*r).is_some_and(|o| o.held && o.note.channel == channel && o.note.key == key))
    }
    pub(crate) fn held_host_route(&self, input:u8, key:u8) -> Option<(u8,u8)> {
        self.player.host_notes.active.iter().rev().filter_map(|r| self.player.host_notes.get(*r))
            .find(|o| o.held && o.note.channel==input && o.note.key==key).map(|o| (o.channel,o.key))
    }
    pub(crate) fn host_note_drops(&self) -> u64 { self.player.host_notes.dropped }
    /// Retained anonymous/script roots use the router's captured mapped key.
    pub(crate) fn input_key_down_from(&self, input:u8, channel:u8, key:u8) -> bool {
        if input>=16 || channel>=16 || key>=128 { return false; }
        let held=self.player.input_keys[input as usize][key as usize];
        (held.channel==channel && held.onset_velocity.is_some()) || self.script.as_deref().is_some_and(|rt| rt.key_down_from(input,channel,key))
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        self.note_on_from(channel, channel, note, velocity);
    }

    pub(crate) fn note_on_from(&mut self, channel: u8, owner: u8, note: u8, velocity: u8) {
        if velocity == 0 { return self.note_off_from_velocity(channel, owner, note, Velocity::from_midi1(64)); }
        self.note_on_from_velocity(channel, owner, note, Velocity::from_midi1(velocity));
    }

    pub fn note_on_velocity(&mut self, channel: u8, note: u8, velocity: Velocity) {
        self.note_on_from_velocity(channel, channel, note, velocity);
    }

    pub(crate) fn note_on_from_velocity(&mut self, channel: u8, owner: u8, note: u8, velocity: Velocity) {
        self.note_on_owned_velocity(channel, owner, note, velocity, None);
    }

    pub(crate) fn note_on_input_velocity_pitch(&mut self, address: InputAddress, channel: u8, note: u8, velocity: Velocity, attribute_pitch: Option<f64>) -> bool {
        if !self.shared_input_admitted() { self.reject_unsealed_source(); return false; }
        if channel >= 16 || note >= 128 || address.channel >= 16 { return false; }
        self.mark_per_notes();
        let Some(per_note) = self.player.per_note.input(address, self.player.midi.per_note_range[address.channel as usize], attribute_pitch) else { return false; };
        self.note_on_owned_velocity(channel, address.channel, note, velocity, Some(per_note));
        true
    }

    fn note_on_owned_velocity(&mut self, channel: u8, owner: u8, note: u8, velocity: Velocity, per_note: Option<PerNoteInput>) {
        if !self.shared_input_admitted() { self.reject_unsealed_source(); return; }
        if channel >= 16 || owner >= 16 || note >= 128 { return; }
        if self.script.is_none() { self.admit_direct_start(); }
        self.player.keys[channel as usize][note as usize] = velocity.midi1().max(1);
        self.player.key_on[channel as usize][note as usize] = self.player.now;
        if let Some((rt, mut host)) = self.scripted(channel) {
            return rt.note_on_owned_velocity(&mut host, 0, owner, note, velocity, None, per_note);
        }
        let mono = self.state.note_modes[channel as usize] == NoteMode::MonoRetrigger;
        let retain = mono && self.player.input_keys[owner as usize][note as usize].onset_velocity.is_some()
            && self.player.input_keys[owner as usize][note as usize].accepted.is_some();
        let mut prospective = player::InputKey { channel, onset_velocity: Some(velocity), per_note,
            started: self.player.now, sostenuto: false, accepted: None };
        if !mono { self.player.input_keys[owner as usize][note as usize] = prospective; }
        let mut admitted = false;
        if let Some(bank) = self.bank.as_deref() {
            let defaults = self.defaults();
            let event = self.player.next_id();
            self.player.note_history.observe(note);
            admitted = self.player.start_input(bank, &self.state, &NoteEvent { velocity: velocity.midi1().max(1),
                owner: Some((owner, note)), input_channel: Some(owner), per_note,
                ..NoteEvent::new_velocity(channel, note, velocity) }, event,
                note_mode::Root::Input { channel: owner, key: note }, defaults).is_some();
        }
        if mono {
            if admitted {
                prospective.accepted = self.player.input_keys[owner as usize][note as usize].accepted;
                self.player.input_keys[owner as usize][note as usize] = prospective;
            } else if !retain {
                self.player.input_keys[owner as usize][note as usize] = prospective;
            }
        }
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        self.note_off_from(channel, channel, note);
    }

    pub(crate) fn note_off_from(&mut self, channel: u8, owner: u8, note: u8) {
        self.note_off_from_with_velocity(channel, owner, note, None);
    }

    pub fn note_off_velocity(&mut self, channel: u8, note: u8, velocity: Velocity) {
        self.note_off_from_velocity(channel, channel, note, velocity);
    }

    pub(crate) fn note_off_from_velocity(&mut self, channel: u8, owner: u8, note: u8, velocity: Velocity) {
        self.note_off_from_with_velocity(channel, owner, note, Some(velocity));
    }

    fn note_off_from_with_velocity(&mut self, channel: u8, owner: u8, note: u8, release_velocity: Option<Velocity>) {
        if channel >= 16 || owner >= 16 || note >= 128 {
            return;
        }
        for v in self.player.voices.iter_mut().filter(|v| v.host_note.is_none() && v.channel == channel && v.owner == Some((owner, note)) && v.held && !v.release_trigger) {
            v.counter_stop.get_or_insert(self.player.now);
            if let Some(value) = release_velocity {
                v.release_velocity.get_or_insert(value);
                v.settled = None;
            }
        }
        self.commands.for_each_start_mut(|command, start| {
            if command.channel == channel && start.host_note.is_none() && start.owner == Some((owner, note)) {
                start.counter_stop.get_or_insert(self.player.now);
                if let Some(value) = release_velocity { start.release_velocity.get_or_insert(value); }
            }
        });
        // Capture at the physical key-up, before a delayed KSP release or
        // member reuse can replace this event's channel/key expression.
        if self.player.midi.mpe.member(owner).is_some() {
            let x = self.release_snapshot(channel, owner, note);
            self.commands.for_each_start_mut(|command, start| {
                if command.channel == channel && start.host_note.is_none() && start.owner == Some((owner, note)) {
                    start.expression.get_or_insert(x);
                }
            });
            for v in self.player.voices.iter_mut().filter(|v| v.host_note.is_none() && v.channel == channel && v.owner == Some((owner, note)) && v.held && !v.release_trigger) {
                v.frozen_expression.get_or_insert(x);
            }
        }
        if self.script.is_some() {
            if !self.script.as_ref().unwrap().key_down_except(owner, channel, note) {
                self.player.keys[channel as usize][note as usize] = 0;
                self.player.key_up[channel as usize][note as usize] = self.player.now;
            }
            if let Some((rt, mut host)) = self.scripted(channel) {
                if let Some(value) = release_velocity { rt.note_off_from_velocity(&mut host, 0, owner, note, value); }
                else { rt.note_off_from(&mut host, 0, owner, note); }
            }
            let exact = self.player.host_notes.active.iter().filter_map(|r| self.player.host_notes.get(*r))
                .filter(|o| o.held && o.channel==channel && o.key==note).map(|o| o.velocity.midi1().max(1)).max().unwrap_or(0);
            self.player.keys[channel as usize][note as usize]=self.player.keys[channel as usize][note as usize].max(exact);
            return;
        }
        let defaults = self.defaults();
        if let Some(bank) = self.bank.as_deref() {
            self.player.note_off_with_velocity(bank, &self.state, channel, note, Some(owner), release_velocity, defaults);
        } else {
            self.player.keys[channel as usize][note as usize] = self.player.host_notes.active.iter().filter_map(|r| self.player.host_notes.get(*r))
                .filter(|o| o.held && o.channel==channel && o.key==note).map(|o| o.velocity.midi1().max(1)).max().unwrap_or(0);
        }
    }

    /// Controllers pass through the scripts; channel mode messages (120 and up)
    /// act on the engine directly so a script can never swallow a panic.
    pub fn cc(&mut self, channel: u8, cc: u8, value: u8) {
        let batch = (123..=127).contains(&cc);
        if batch { self.player.note_off_depth += 1; }
        self.cc_inner(channel, cc, value);
        if batch { self.player.note_off_depth -= 1; self.finish_note_offs(); }
    }

    fn cc_inner(&mut self, channel: u8, cc: u8, value: u8) {
        if channel >= 16 || cc >= 128 { return; }
        self.player.library_receiver.control(channel,
            crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(value)));
        if cc == 120 {
            // This direct API keeps its logical-channel stop contract. Routed
            // wire messages use all_sound_off_from and captured physical input.
            let channels = self.player.midi.mpe.manager(channel).map_or(1 << channel, |zone| zone.channels());
            self.mark_host_notes();
            self.cancel_notes(channels);
            let fade = self.player.fade_frames(STEAL_FADE);
            for logical in (0..16u8).filter(|logical| channels & (1 << logical) != 0) {
                self.player.host_notes.silence(Some(logical), None, self.player.now);
                self.player.keys[logical as usize].fill(0);
                self.player.key_up[logical as usize].fill(self.player.now);
                self.player.midi.accept(crate::midi::ControlTarget::Physical(logical),
                    crate::midi::Control::Cc(120, crate::midi::ControlValue::from_midi1(value)));
            }
            for voice in self.player.voices.iter_mut().filter(|voice| channels & (1 << voice.channel) != 0) {
                voice.fade.start(0., fade, true);
                voice.held = false;
            }
            self.player.pending_releases.retain(|release| channels & (1 << release.channel) == 0);
            for row in self.player.input_keys.iter_mut() {
                for key in row.iter_mut().filter(|key| channels & (1 << key.channel) != 0) { *key = Default::default(); }
            }
            return;
        }
        if (123..=127).contains(&cc) {
            let channels = self.player.midi.mpe.manager(channel).map_or(1 << channel, |zone| zone.channels());
            for i in 0..self.player.host_notes.active.len() {
                let id = self.player.host_notes.active[i];
                if self.player.host_notes.get(id).is_some_and(|o| channels & (1 << o.channel) != 0) { self.release_host_note(id); }
            }
            // Audition and host cleanup address the logical home. Physical
            // inputs may share that home without sharing their pedal state.
            for logical in (0..16u8).filter(|logical| channels & (1 << logical) != 0) {
                for input in 0..16u8 {
                    for key in 0..128u8 {
                        if self.input_key_down_from(input, logical, key) {
                            self.note_off_from(logical, input, key);
                        }
                    }
                }
            }
            self.player.midi.accept(crate::midi::ControlTarget::Authored,
                crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(value)));
            return;
        }
        self.cc_from(channel, channel, cc, value);
    }

    pub(crate) fn cc_from(&mut self, channel: u8, input_channel: u8, cc: u8, value: u8) {
        if channel >= 16 || input_channel >= 16 || cc >= 128 { return; }
        let Some(received) = self.receive_controller(input_channel, cc, value) else { return; };
        self.control_from_received(channel, input_channel, received);
    }

    pub(crate) fn native_note_action_enabled(&self, cc: u8) -> bool {
        self.state.native_note_action(cc)
    }

    pub(crate) fn control_from_received(&mut self, channel: u8, input_channel: u8, received: crate::midi::ReceivedControl) {
        self.control_from_received_scoped(channel, input_channel, received, None, true);
    }

    pub(crate) fn control_from_received_on_port(&mut self, channel: u8, input_channel: u8,
        received: crate::midi::ReceivedControl, port: u8) {
        self.control_from_received_scoped(channel, input_channel, received, Some(port), true);
    }

    fn control_from_received_scoped(&mut self, channel: u8, input_channel: u8,
        received: crate::midi::ReceivedControl, port: Option<u8>, capture_raw: bool) {
        let batch = matches!(received.control, crate::midi::Control::Cc(123..=127, _));
        if batch { self.player.note_off_depth += 1; }
        self.control_from_received_scoped_inner(channel, input_channel, received, port, capture_raw);
        if batch { self.player.note_off_depth -= 1; self.finish_note_offs(); }
    }

    fn control_from_received_scoped_inner(&mut self, channel: u8, input_channel: u8,
        received: crate::midi::ReceivedControl, port: Option<u8>, capture_raw: bool) {
        let crate::midi::ReceivedControl { control, parameter } = received;
        if !control.valid() { return; }
        if !matches!(control, crate::midi::Control::Cc(..) | crate::midi::Control::Cc14 { .. }) { return; }
        let (cc, _) = control.legacy();
        if channel >= 16 || input_channel >= 16 || cc >= 128 { return; }
        if capture_raw { self.player.library_receiver.control(channel, control); }
        let native_control = if self.state.controller_policy == instrument::ControllerPolicy::Native {
            match control {
                crate::midi::Control::Cc(7, value) | crate::midi::Control::Cc14 { coarse: 7, value, .. } => Some((params::GroupPar::Volume, value)),
                crate::midi::Control::Cc(10, value) | crate::midi::Control::Cc14 { coarse: 10, value, .. } => Some((params::GroupPar::Pan, value)),
                _ => None,
            }
        } else { None };
        if self.script.is_none() && let Some((param, value)) = native_control { self.player.native_instrument_control(param, value); }
        let native_note_action = self.state.native_note_action(cc);
        let parameter = parameter.and_then(crate::midi::ParameterEvent::absolute);
        // MPE zones address physical inputs. `channel` belongs only to the
        // library delivery and can alias a manager of another physical zone.
        let input_mask = (1 << input_channel) | self.player.midi.mpe.manager(input_channel).map_or(0, |zone| zone.member_mask());
        if cc == 121 {
            self.player.midi.reset_received_controllers(input_mask);
            self.player.per_note.reset_pressure(input_mask, port);
            self.player.host_notes.sostenuto(0, input_mask);
            if let Some(rt) = self.script.as_deref_mut() { rt.clear_input_sostenuto(input_mask); }
            // A root's release may already have left Runtime. Physical reset
            // also retires its queued no-attack capture, at the original frame.
            self.commands.for_each_release_mut(|command, release| {
                let input = command.input_channel.unwrap_or(command.channel);
                if !command.applied && release.input_root && input < 16 && input_mask & (1 << input) != 0 {
                    release.input_sostenuto = false;
                }
            });
            self.player.touch();
        }
        if native_note_action && (123..=127).contains(&cc) {
            for i in 0..self.player.host_notes.active.len() {
                let id = self.player.host_notes.active[i];
                if self.player.host_notes.get(id).is_some_and(|o| input_mask & (1 << o.note.channel) != 0) { self.release_host_note(id); }
            }
        }
        if cc == 120 && native_note_action {
            self.all_sound_off_from(channel, input_mask);
            for input in (0..16u8).filter(|input| input_mask & (1 << input) != 0) {
                self.player.midi.accept(crate::midi::ControlTarget::Physical(input), control);
            }
            return;
        }
        if self.script.is_some() {
            if cc == 121 {
                let previous = self.player.midi.midi1_cc();
                let controller_defaults = self.player.midi.controller_defaults;
                let script_previous = self.script.as_deref().unwrap().env.input.cc;
                let deliveries = self.script.as_deref().unwrap().input_deliveries();
                self.commands.retain(|c, _| !c.input_channel.is_some_and(|input| input_mask & (1 << input) != 0)
                    || !match c.kind {
                        script::Kind::Control { control, .. } => crate::midi::reset_controller_value(control.reset_controller_number(), None).is_some(),
                        script::Kind::PerNotePressure(input, _) => port.is_none_or(|port| input.address.port == port),
                        _ => false,
                    });
                if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
                    if let Some(port) = port {
                        rt.cancel_performance_controllers_on_port(&mut host, input_mask, port);
                    } else {
                        rt.cancel_performance_controllers(&mut host, input_mask);
                    }
                }
                // Authors often mirror controllers in globals. Deliver the
                // same defaults the native reset uses before synchronizing %CC.
                for input in (0..16u8).filter(|&input| input_mask & (1 << input) != 0) {
                    let captured = deliveries[input as usize];
                    let channels = if captured == 0 { 1 << channel } else { captured };
                    for logical in (0..16u8).filter(|&logical| channels & (1 << logical) != 0) {
                        // Deselect parameter addresses before data-entry defaults:
                        // authored RPN/NRPN handlers must retain parameter values.
                        for cc in (98..102u8).chain(0..98).chain(102..120) {
                            let Some(value) = crate::midi::reset_controller_value(cc, controller_defaults[cc as usize]) else { continue; };
                            // Always refresh the standard performance caches; other
                            // supported CCs need a callback only when changed.
                            if matches!(cc, 1 | 11 | 64..=67 | 98..=101)
                                || previous[input as usize][cc as usize] != value
                                || script_previous[cc as usize] != i32::from(value)
                            {
                                self.control_from_received_scoped(logical, input,
                                    crate::midi::ReceivedControl::without_parameter(crate::midi::Control::Cc(cc,
                                        crate::midi::ControlValue::from_midi1(value))), port, false);
                            }
                        }
                        self.control_from_scoped(logical, input, crate::midi::Control::Bend(crate::midi::Bend::from_midi1(8192)), false);
                        self.control_from_scoped(logical, input, crate::midi::Control::ResetPressure, false);
                    }
                }
                self.script.as_deref_mut().unwrap().reset_controllers(None);
            }
            if cc < 120 || cc == 122 || !native_note_action {
                if !native_note_action {
                    self.player.midi.accept(crate::midi::ControlTarget::Physical(input_channel), control);
                }
                if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
                    if let Some((param, value)) = native_control { host.native_standard_control(param, value); }
                    rt.received_control(&mut host, control, parameter, native_control.is_some());
                }
                return;
            }
            if (123..=127).contains(&cc) {
                for owner in (0..16u8).filter(|owner| input_mask & (1 << owner) != 0) {
                    for logical in 0..16 {
                        for note in 0..128 {
                            if self.script.as_ref().is_some_and(|rt| rt.key_down_from(owner, logical, note)) {
                                self.note_off_from(logical, owner, note);
                            }
                        }
                    }
                    self.player.midi.accept(crate::midi::ControlTarget::Physical(owner), control);
                }
                return;
            }
        }
        let defaults = self.defaults();
        self.player.control_with_standard_provenance(self.bank.as_deref(), &self.state, channel,
            crate::midi::ControlTarget::Physical(input_channel), control, defaults, native_control.is_some(), false);
    }

    /// Cut a physical performance context, including notes a script rerouted.
    pub(crate) fn all_sound_off_from(&mut self, channel: u8, input_mask: u16) {
        if !self.state.native_note_action(120) { return; }
        let fade_seconds = if self.state.controller_policy == instrument::ControllerPolicy::Native { 0.010 } else { STEAL_FADE };
        self.stop_input_notes(channel, input_mask, fade_seconds);
    }

    fn stop_input_notes(&mut self, channel: u8, input_mask: u16, fade_seconds: f32) {
        if channel >= 16 || input_mask == 0 { return; }
        self.mark_host_notes();
        self.player.host_notes.silence(None, Some(input_mask), self.player.now);
        let selected = |input_channel: Option<u8>| input_channel.is_some_and(|input| input_mask & (1 << input.min(15)) != 0);
        let mut affected = 1 << channel;
        if let Some((rt, mut host)) = self.scripted(channel) { affected |= rt.all_sound_off_from(&mut host, input_mask); }
        self.commands.retain(|c, _| {
            if !selected(c.input_channel) { return true; }
            affected |= 1 << c.channel;
            false
        });
        let fade = self.player.fade_frames(fade_seconds);
        for v in self.player.voices.iter_mut().filter(|v| selected(v.input_channel)) {
            affected |= 1 << v.channel;
            v.fade.start(0.0, fade, true);
            // Relinquish key ownership during the fade, as the logical stop does.
            v.held = false;
        }
        self.player.pending_releases.retain(|r| {
            if !selected(r.input_channel) { return true; }
            affected |= 1 << r.channel;
            false
        });
        for input in (0..16).filter(|input| input_mask & (1 << input) != 0) {
            for key in self.player.input_keys[input].iter_mut().filter(|key| key.onset_velocity.is_some()) {
                affected |= 1 << key.channel;
                *key = Default::default();
            }
        }
        for channel in (0..16).filter(|channel| affected & (1 << channel) != 0) {
            for note in 0..128 {
                let held = (if let Some(rt) = self.script.as_deref() {
                    (0..16).any(|input| rt.key_down_from(input, channel as u8, note as u8))
                } else {
                    self.player.input_keys.iter().any(|row| row[note].channel == channel as u8 && row[note].onset_velocity.is_some())
                }) || self.player.host_notes.active.iter().filter_map(|id| self.player.host_notes.get(*id))
                    .any(|owner| owner.held && !owner.silenced && owner.channel == channel as u8 && owner.key == note as u8);
                if !held {
                    self.player.keys[channel][note] = 0;
                    self.player.key_up[channel][note] = self.player.now;
                }
            }
        }
        // Scoped physical cleanup can remove the selected root while an
        // accepted root from another input remains held on its delivered channel.
        self.player.note_off_pending |= affected;
        self.finish_note_offs();
    }

    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        self.pitch_bend_from(channel, channel, value);
    }

    pub(crate) fn pitch_bend_from(&mut self, channel: u8, input_channel: u8, value: u16) {
        self.control_from(channel, input_channel, crate::midi::Control::Bend(crate::midi::Bend::from_midi1(value)));
    }

    /// Channel pressure (mono aftertouch modulation), through the scripts.
    pub fn channel_pressure(&mut self, channel: u8, value: u8) {
        self.channel_pressure_from(channel, channel, value);
    }

    pub(crate) fn channel_pressure_from(&mut self, channel: u8, input_channel: u8, value: u8) {
        self.control_from(channel, input_channel, crate::midi::Control::Pressure(crate::midi::ControlValue::from_midi1(value)));
    }

    pub(crate) fn control_from(&mut self, channel: u8, input_channel: u8, control: crate::midi::Control) {
        self.control_from_scoped(channel, input_channel, control, true);
    }

    pub(crate) fn receive_library_control(&mut self, channel: u8, control: crate::midi::Control) {
        self.player.library_receiver.control(channel, control);
    }

    fn control_from_scoped(&mut self, channel: u8, input_channel: u8, control: crate::midi::Control, capture_raw: bool) {
        if channel >= 16 || input_channel >= 16 || !control.valid() { return; }
        if capture_raw { self.player.library_receiver.control(channel, control); }
        if let crate::midi::Control::Cc(cc, value) = control {
            if cc >= 128 { return; }
            let Some(mut received) = self.receive_controller(input_channel, cc, value.midi1()) else { return; };
            // The decoder observes the byte projection; delivery retains the
            // caller's original canonical control precision.
            received.control = control;
            self.control_from_received_scoped(channel, input_channel, received, None, capture_raw);
        } else if matches!(control, crate::midi::Control::Cc14 { .. }) {
            self.control_from_received_scoped(channel, input_channel,
                crate::midi::ReceivedControl::without_parameter(control), None, capture_raw);
        } else if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
            rt.control_from(&mut host, 0, control);
        } else {
            let defaults = self.defaults();
            self.player.control(self.bank.as_deref(), &self.state, channel,
                crate::midi::ControlTarget::Physical(input_channel), control, defaults);
        }
    }

    /// A host edit of script control `control` in script slot `slot`: sets its
    /// value and runs the script's `on ui_control`.
    pub fn ui_file_selection(&mut self, slot: usize, control: usize, path: &str) -> bool {
        let channel = self.service_channel();
        self.scripted(channel).is_some_and(|(rt, mut host)| rt.ui_file_selection(&mut host, slot, control, path))
    }

    pub fn native_control(&mut self, slot: usize, control: usize, index: usize, value: f64) -> bool {
        let channel = self.service_channel();
        if let Some((rt, mut host)) = self.scripted(channel) { rt.native_control(&mut host, slot, control, index, value) } else { false }
    }

    pub fn native_midi_learn(&mut self, slot: usize, control: usize, index: usize, active: bool) -> bool {
        self.script.as_deref_mut().is_some_and(|rt| rt.native_midi_learn(slot, control, index, active))
    }
    pub fn native_text(&mut self, slot:usize, control:usize, index:usize, text:&str)->bool {
        let channel=self.service_channel();
        if let Some((rt,mut host))=self.scripted(channel){rt.native_text(&mut host,slot,control,index,text)}else{false}
    }
    pub fn ui_control(&mut self, slot: usize, control: usize, value: i32) {
        let channel = self.service_channel();
        if let Some((rt, mut host)) = self.scripted(channel) {
            rt.ui_control(&mut host, slot, control, value);
        }
    }

    pub fn pop_array_job(&mut self) -> Option<Box<crate::ksp::ArrayJob>> {
        self.script.as_deref_mut()?.pop_array_job()
    }

    pub fn can_finish_array_job(&self) -> bool {
        self.script.as_deref().is_some_and(Runtime::can_finish_array_job)
    }

    pub fn finish_array_job(&mut self, request: Box<crate::ksp::ArrayJob>) -> Result<(), Box<crate::ksp::ArrayJob>> {
        match self.script.as_deref_mut() {
            Some(rt) => rt.finish_array_job(request),
            None => Err(request),
        }
    }

    pub fn pop_retired_array_job(&mut self) -> Option<Box<crate::ksp::ArrayJob>> {
        self.script.as_deref_mut()?.pop_retired_array_job()
    }

    fn queue_init_zone_edits(&mut self) {
        let Some(bank)=self.bank.as_deref() else { return };
        let Some(rt) = self.script.as_deref_mut() else { return };
        if !bank.zone_init_source_matches(rt.init_zone_source) || self.zone_queued_epoch==rt.service_epoch { return; }
        self.zone_queued_epoch=rt.service_epoch;
        for &edit in &rt.init_zone_edits {
            if edit.id<0 && bank.zone_init_epoch==rt.service_epoch {continue;}
            if self.zone_requests.len() < self.zone_requests.capacity() { self.zone_requests.push_back(edit); self.zone_inflight += 1; }
            else { rt.env.note("set_zone_par: init request queue exhausted"); }
        }
    }

    /// Prepare/install/acknowledge one zone batch for offline/headless hosts.
    /// Performs file I/O and allocates: never call this from an audio callback.
    /// Repeat while true after installing a script/bank or running an action.
    pub fn service_zone_edits(&mut self) -> anyhow::Result<bool> {
        let source=self.script.as_deref().map_or(0,|rt|rt.service_epoch);
        let (owner,mut prepared)=if let Some(p)=self.offline_zone.take() { p } else {
            let mut jobs=Vec::with_capacity(zone::QUEUE);
            while jobs.len()<zone::QUEUE {
                let Some(job)=self.pop_zone_job() else {break}; jobs.push(job);
            }
            if jobs.is_empty() {return Ok(false);}
            let base=jobs[0].base.clone();
            (source,zone::Prepared::build(jobs,base,&||false))
        };
        if owner!=source { return Ok(true); }
        if !prepared.installed {
            prepared.success=self.install_zone_map(&mut prepared);
            prepared.installed=true;
        }
        while let Some(edit)=prepared.edits.get(prepared.completed).copied() {
            if !self.finish_zone_edit(edit,prepared.success) {
                self.offline_zone=Some((owner,prepared));
                anyhow::bail!("Zone async completion queue is full; process the script before retrying");
            }
            prepared.completed+=1;
        }
        if let Some(error)=prepared.error.take() {anyhow::bail!("Zone mapping: {error}");}
        Ok(true)
    }

    pub(crate) fn pop_zone_job(&mut self) -> Option<zone::Job> {
        self.bank.as_deref()?.zone_job(self.zone_requests.pop_front()?)
    }

    pub(crate) fn install_zone_map(&mut self, prepared: &mut zone::Prepared) -> bool {
        let installed = self.bank.as_deref_mut().is_some_and(|bank|bank.install_zone_map(prepared));
        if !installed && prepared.success && let Some(rt) = self.script.as_deref_mut() {
            rt.env.note("set_zone_par: prepared mapping rejected after bank replacement or out-of-order installation");
        }
        installed
    }

    pub(crate) fn finish_zone_edit(&mut self, edit: crate::ksp::engine::ZoneEdit, success: bool) -> bool {
        if edit.id>=0 && let Some((rt,mut host)) = self.scripted(self.service_channel()) {
            if !rt.service_complete(&mut host,edit.slot,edit.id,success) { return false; }
            if !success { rt.env.note("set_zone_par: mapping was not installed"); }
        }
        self.zone_inflight = self.zone_inflight.saturating_sub(1);
        true
    }

    pub(crate) fn zone_upgrade_ready(&self, bank: &Bank) -> bool {
        if self.zone_inflight != 0 { return false; }
        self.bank.as_deref().is_none_or(|old|old.zone_state.as_ref().is_none_or(|state|
            state.revision == 0 || bank.zone_parent.as_ref().is_some_and(|parent|std::sync::Arc::ptr_eq(parent,state))))
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
        if id >= 0 && let Some((rt, mut host)) = self.scripted(self.service_channel()) {
            rt.async_complete(&mut host, slot, id, loaded);
        }
        retired
    }

    /// Polyphonic key pressure, forwarded through authored callbacks before modulation.
    pub fn poly_pressure(&mut self, channel: u8, note: u8, value: u8) {
        self.poly_pressure_from(channel, channel, note, value);
    }

    pub(crate) fn poly_pressure_from(&mut self, channel: u8, input_channel: u8, note: u8, value: u8) {
        if channel > 15 || input_channel > 15 || note > 127 { return; }
        if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
            rt.poly_pressure(&mut host, 0, note, value.min(127));
        } else {
            let mut expression = self.player.midi.expression[channel as usize][note as usize];
            expression.note_pressure = Some(crate::midi::ControlValue::from_midi1(value.min(127)));
            self.player.midi.set_expression(channel, note, expression);
        }
    }

    pub(crate) fn mpe_poly_pressure_from(&mut self, channel: u8, input_channel: u8, note: u8,
        value: crate::midi::ControlValue) {
        if channel >= 16 || input_channel >= 16 || note >= 128 { return; }
        if let Some((rt, mut host)) = self.scripted_from(channel, input_channel) {
            rt.poly_pressure_derived(&mut host, 0, note, value);
        }
    }

    pub(crate) fn per_note_pressure_from(&mut self, channel: u8, note: u8, address: InputAddress,
        value: crate::midi::ControlValue) -> bool {
        if channel >= 16 || note >= 128 || address.channel >= 16 || address.key >= 128 { return false; }
        self.mark_per_notes();
        let range = self.player.midi.per_note_range[address.channel as usize];
        let Some(input) = self.player.per_note.input(address, range, None) else { return false; };
        if let Some((rt, mut host)) = self.scripted_from(channel, address.channel) {
            rt.poly_pressure_input(&mut host, 0, input, note, value);
            true
        } else {
            let accepted = self.player.per_note.pressure(input, value);
            if accepted { self.player.touch(); }
            accepted
        }
    }

    /// Start every eligible zone for `event`; the id addresses all its voices.
    /// Returns `None` only for invalid input or when no bank is loaded.
    pub fn start_event(&mut self, event: &NoteEvent) -> Option<EventId> {
        if !self.shared_input_admitted() { self.reject_unsealed_source(); return None; }
        if event.channel >= 16 || event.note >= 128 || event.velocity > 127 { return None; }
        if self.state.note_modes[event.channel as usize] == NoteMode::MonoRetrigger { return None; }
        self.bank.as_deref()?;
        self.admit_direct_start();
        let defaults = self.defaults();
        let bank = self.bank.as_deref()?;
        self.player.note_history.observe(event.note);
        let id = self.player.next_id();
        self.player.start(bank, &self.state, event, id, false, defaults)
    }

    /// Release the event's voices (note-off by id), firing release triggers.
    pub fn release_event(&mut self, id: EventId) {
        let defaults = self.defaults();
        let Some(bank) = self.bank.as_deref() else {
            return;
        };
        let allowed = self.player.allowed;
        self.player.accept_release(bank, &self.state, release::ReleaseRequest {
            source: id, release_velocity: None, capture: release::Capture::Actual,
            sample: Some(release::ReleaseSample::FromVoice(&allowed)),
        }, defaults);
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
        self.player.midi.set_expression(channel, note, expression);
    }

    fn service_channel(&self) -> u8 {
        self.player.midi.mpe.zone(self.home_channel).map(|zone| zone.manager).or_else(|| self.player.midi.mpe.zones().into_iter().find(|zone| zone.members != 0).map(|zone| zone.manager)).unwrap_or(self.home_channel)
    }

    pub(crate) fn set_home_channel(&mut self, channel: u8) {
        self.home_channel = channel.min(15);
        let channel = self.service_channel();
        if let Some(rt) = self.script.as_deref_mut() { rt.set_service_channel(channel); }
    }

    pub(crate) fn mpe_zones(&self) -> crate::midi::MpeZones {
        self.player.midi.mpe
    }

    /// Authored route changes replace the authored setup. Wire negotiation
    /// changes one zone independently and never writes the route snapshot.
    pub(crate) fn configure_mpe(&mut self, configured: Option<(u8, u8, u8)>) {
        let mut mpe = crate::midi::MpeZones::default();
        if let Some((manager, members, range)) = configured
            && mpe.configure(manager, members, range).is_none() { return; }
        let channels = self.player.midi.mpe_role_changes(mpe);
        self.reconcile_mpe_channels(channels, None);
        self.player.midi.set_mpe(mpe);
        self.set_home_channel(self.home_channel);
    }

    /// Compatibility entry point for existing single-zone engine callers.
    #[cfg(test)]
    pub(crate) fn set_mpe_zone(&mut self, zone: Option<(u8, u16)>) {
        self.configure_mpe(zone.map(|(manager, mask)| (manager, (mask & !(1 << manager.min(15))).count_ones() as u8, 48)));
    }

    pub(crate) fn native_parameter_from(&mut self, channel: u8, input: u8, parameter: crate::midi::NativeParameter) -> bool {
        if channel >= 16 || input >= 16 || parameter.address >= 16384 {
            return false;
        }
        // Protocol configuration is established before the next physical On
        // captures its range, independently of library callback scheduling.
        let applied = self.player.midi.apply_native_parameter(input, parameter);
        if let Some((rt, mut host)) = self.scripted_from(channel, input) {
            rt.received_native_parameter(&mut host, 0, parameter);
            true
        } else {
            if !applied { self.player.unsupported_native_controls = self.player.unsupported_native_controls.saturating_add(1); }
            applied
        }
    }

    pub(crate) fn receive_controller(&mut self, input: u8, cc: u8, value: u8) -> Option<crate::midi::ReceivedControl> {
        if input >= 16 || cc >= 128 { return None; }
        let parameter = self.player.midi.receive_parameter(input, cc, value.min(127));
        self.apply_received_parameter(input, parameter);
        Some(crate::midi::ReceivedControl {
            control: crate::midi::Control::Cc(cc, crate::midi::ControlValue::from_midi1(value)), parameter,
        })
    }

    pub(crate) fn receive_midi1_cc(&mut self, input: u8, cc: u8, raw: u8) -> Option<crate::midi::ReceivedControl> {
        let received = self.player.midi.receive_midi1_cc(input, cc, raw)?;
        self.apply_received_parameter(input, received.parameter);
        Some(received)
    }

    pub(crate) fn midi1_velocity(&mut self, input: u8, raw: u8) -> Velocity {
        self.player.midi.midi1_velocity(input, raw).unwrap_or_else(|| Velocity::from_midi1(raw))
    }

    pub(crate) fn midi1_release_velocity(&mut self, input: u8, raw: u8) -> Velocity {
        self.player.midi.midi1_release_velocity(input, raw).unwrap_or_else(|| Velocity::from_midi1(raw))
    }

    fn apply_received_parameter(&mut self, input: u8, parameter: Option<crate::midi::ParameterEvent>) {
        if let Some(parameter) = parameter {
            self.player.midi.apply_midi1_parameter(input, parameter);
            let mut mpe = self.player.midi.mpe;
            if let Some(change) = mpe.parameter(input, parameter) {
                self.reconcile_mpe_channels(change.channels, mpe.manager(input).map(|zone| zone.manager));
                self.player.midi.set_mpe(mpe);
                if parameter.address == 0 && matches!(parameter.data,
                    crate::midi::ParameterData::Increment | crate::midi::ParameterData::Decrement) {
                    self.player.midi.sync_mpe_parameter_baselines(input);
                }
                self.set_home_channel(self.home_channel);
            }
        }
    }

    /// Stop the old physical owners before a channel changes its MPE role.
    fn reconcile_mpe_channels(&mut self, channels: u16, received_manager: Option<u8>) {
        if channels == 0 { return; }
        self.player.note_off_depth += 1;
        self.mark_host_notes();
        self.player.host_notes.stop(None, Some(channels), self.player.now);
        if let Some((rt, mut host)) = self.scripted(self.home_channel) {
            rt.cancel_mpe_controllers(&mut host, channels);
            rt.cleanup_notes_from(&mut host, channels);
        }
        // The just-received MCM leaves its manager's RPN selection intact;
        // old member transactions cannot retarget the reassigned channel.
        let transactions = channels & !received_manager.map_or(0, |manager| 1 << manager);
        self.player.midi.reset_received(transactions);
        for channel in (0..16u8).filter(|channel| channels & (1 << channel) != 0) {
            self.stop_input_notes(channel, 1 << channel, STEAL_FADE);
        }
        self.player.reset_mpe_channels(channels);
        self.player.note_off_depth -= 1;
        self.finish_note_offs();
    }

    /// Applied state plus earlier same-frame script commands, before the
    /// release callback or reused member can replace these controller values.
    fn release_snapshot(&self, channel: u8, physical: u8, note: u8) -> Expression {
        self.player.release_snapshot(self.commands.as_slice(), 0, channel, physical, note)
    }

    pub(crate) fn freeze_released_expression(&mut self, channel: u8, note: u8) {
        if channel >= 16 || note >= 128 { return; }
        let expression = self.release_snapshot(channel, channel, note);
        for v in &mut self.player.voices {
            if v.channel == channel && v.owner.map_or(v.note, |(_, key)| key) == note && v.frozen_expression.is_none()
                && (!v.held || self.commands.iter().any(|c| c.at == 0 && c.id == v.event
                    && matches!(c.kind, script::Kind::Release(_)))) {
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

    /// MIDI 1 compatibility view of accepted physical controller rows.
    pub fn cc_state(&self) -> [[u8; 128]; 16] {
        self.player.midi.midi1_cc()
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
    /// Consume the first streaming failure since the last explicit check.
    pub fn take_stream_error(&self) -> Option<StreamError> {
        self.player.stream_error.take()
    }

    /// Offline callers must propagate this result before accepting rendered PCM.
    pub fn render_checked(&mut self, left: &mut [f32], right: &mut [f32]) -> Result<(), StreamError> {
        self.render(left, right);
        self.take_stream_error().map_or(Ok(()), Err)
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let _ftz = FlushDenormals::new();
        // Advancing one engine alone cannot retain an aligned peer census.
        let peers = self.peer_voices.take();
        let n = left.len().min(right.len());
        left.fill(0.0);
        right.fill(0.0);
        if n != 0 && self.state.shared_ahdsr.is_some() {
            // Only the Rack/plugin accepted-range caller delivers the sealed
            // future receipts. Advancing a standalone Engine cannot consume it.
            self.reject_unsealed_source();
            self.peer_voices = peers;
            return;
        }
        self.render_begin(n);
        let mut at = 0;
        loop {
            self.render_boundary(at);
            if at == n { break; }
            let end = self.render_next_boundary(at);
            self.render_interval(at, &mut left[at..end], &mut right[at..end]);
            at = end;
        }
        self.render_finish();
        self.peer_voices = peers;
    }

    /// Only newly adopted, never-rendered engines receive the Rack frame origin.
    pub(super) fn initialize_frame(&mut self, now: u64) {
        debug_assert_eq!(self.player.now, 0);
        debug_assert!(self.player.voices.is_empty());
        self.player.now = now;
        self.block_start = now;
    }

    pub(super) fn render_begin(&mut self, frames: usize) {
        let start = self.player.now;
        self.player.release_counter.ensure(start, frames, self.player.rate);
        let empty = self.bank.is_none() && self.script.is_none() && self.fx.is_empty();
        if empty {
            self.commands.clear();
            self.writes.clear();
            self.write_index.1.clear();
        } else {
            if let Some((rt, mut host)) = self.scripted(self.service_channel()) {
                host.frame_base = start;
                rt.begin_process(&mut host);
            }
            self.fx.begin_meter_block();
        }
        let block_size = MAX_BLOCK.min(self.fx.block_size());
        let block_len = frames.min(block_size);
        if !empty && frames != 0 { self.fx.begin_group_sends(block_len); }
        self.render_state = Some(RenderState {
            start, frames, next: 0, written: 0, block_size, block_base: 0,
            block_len, defaults: self.defaults(), empty,
        });
    }

    pub(super) fn render_boundary(&mut self, at: usize) {
        let mut state = self.render_state.expect("outer render begun");
        self.player.release_counter.ensure(self.player.now, state.frames - at, self.player.rate);
        if !state.empty {
            // Preserve callback → writes → commands, including same-frame source init.
            // Outer endpoints are exclusive for callbacks and inclusive for commands.
            if at < state.frames && let Some((rt, mut host)) = self.scripted(self.service_channel()) {
                host.frame_base = state.start;
                rt.process_at(&mut host, at as u32);
            }
            self.apply_pending_at(at as u32, state.defaults, &mut state.next, &mut state.written);
            if at == state.block_base + state.block_len && state.block_len != 0 {
                self.fx.end_group_sends();
                if at < state.frames {
                    state.block_base = at;
                    state.block_len = (state.frames - at).min(state.block_size);
                    self.fx.begin_group_sends(state.block_len);
                }
            }
        }
        self.render_state = Some(state);
    }

    pub(super) fn render_next_boundary(&self, at: usize) -> usize {
        let state = self.render_state.expect("outer render begun");
        if state.empty { return state.frames; }
        let due = [
            self.commands.get(state.next).map(|c| c.at),
            self.writes.get(state.written).map(|w| w.at),
            self.script.as_deref().and_then(|rt| rt.next_due(state.frames as u32)),
            (self.state.controller_policy == instrument::ControllerPolicy::Native || !self.state.shared_lfos.is_empty() || self.state.shared_ahdsr.is_some())
                .then(|| self.player.release_counter.next_boundary()).flatten().map(|end|
                end.saturating_sub(state.start).min(u64::from(u32::MAX)) as u32),
        ];
        due.into_iter().flatten().fold(state.block_base + state.block_len,
            |end, due| end.min((due as usize).max(at)))
    }

    pub(super) fn render_interval(&mut self, at: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        self.render_interval_with_program_fresh(at,left,right,None)
    }

    pub(super) fn render_interval_with_program_fresh(&mut self, at:usize, left:&mut[f32], right:&mut[f32], fresh:Option<bool>)->bool {
        let state = self.render_state.expect("outer render begun");
        let n = left.len().min(right.len());
        left.fill(0.0);
        right.fill(0.0);
        if state.empty {
            self.player.now = self.player.now.saturating_add(n as u64);
            self.player.release_counter.advance(self.player.now, self.player.rate);
            return false;
        }
        let offset = at - state.block_base;
        let window = self.player.release_counter.span();
        let fed = if let Some(bank) = self.bank.as_deref() {
            self.player.render(bank, &self.state, (&mut *left, &mut *right), &mut self.fx,
                offset, self.blocking_streams, self.tune, state.written == self.writes.len())
        } else {
            self.player.now = self.player.now.saturating_add(n as u64);
            false
        };
        let bus_work = self.fx.mix_buses_segment(left, right, offset);
        self.player.output(&self.state, left, right, self.cutoff);
        let program_work = self.fx.process_group_segment(left, right, offset, fed || bus_work);
        if self.state.controller_policy == instrument::ControllerPolicy::Native {
            let controls = self.player.instrument_controls(&self.state);
            if let Some(fresh)=fresh {
                self.state.program_amplifier.process_shared(controls,window,self.player.rate as f32,
                    program_work,fresh,left,right);
            } else {
                self.state.program_amplifier.process(controls, window, self.player.rate as f32,
                    program_work, left, right);
            }
        }
        if !left.iter().chain(right.iter()).all(|x| x.is_finite()) {
            left.fill(0.0);
            right.fill(0.0);
            self.fx.clear();
            self.player.tone = [0.0; 2];
        }
        program_work
    }

    pub(super) fn render_output_offset(&self, at: usize) -> usize {
        at - self.render_state.expect("outer render begun").block_base
    }

    pub(super) fn render_finish(&mut self) {
        let state = self.render_state.take().expect("outer render begun");
        if state.empty { return; }
        if let Some(rt) = self.script.as_deref_mut() {
            rt.finish_process(state.frames as u32);
            rt.update_level_meters(|attachment| self.fx.level_meter(attachment));
        }
        // Only an empty render leaves changes behind: apply them now.
        for i in state.written..self.writes.len() {
            let w = self.writes[i];
            self.apply_write(w, state.defaults, state.next);
        }
        for index in state.next..self.commands.len() {
            self.apply_command(index, state.defaults);
        }
        // Recovery runs after every queued command: a pending pedal-down must
        // not undo it. Extreme stop pressure cuts this channel rather than
        // leaving an unreleaseable voice or pedal; the drop counter reports it.
        let overflow = std::mem::take(&mut self.player.stop_overflow);
        if overflow != 0 { self.mark_host_notes(); }
        for channel in (0..16).filter(|c| overflow & (1 << c) != 0) {
            self.player.pending_releases.retain(|r| r.channel != channel);
            self.player.cc(self.bank.as_deref(), &self.state, channel, 121, 0, state.defaults);
            self.player.cc(self.bank.as_deref(), &self.state, channel, 120, 0, state.defaults);
        }
        self.commands.clear();
        self.writes.clear();
        self.write_index.1.clear();
    }

    fn defaults(&self) -> Ahdsr {
        Ahdsr {
            attack: self.attack,
            curve: 0.0,
            hold: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: self.release,
            ahd_only: false,
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
    /// Resident wavetable source start and complete 2048-frame cycle count.
    pub wavetable: Option<(usize, usize)>,
    pub pos: f64,
    /// Main sample tap in physical frames, including the fractional cursor;
    /// crossfades also read a partner tap. None for wavetable or an ended path.
    pub physical_pos: Option<f64>,
    /// Actual (start, exclusive end, crossfade frames) for the eligible Constant
    /// loop-control path. None means this path does not process loop controls.
    pub loop_control_window: Option<(u64, u64, u64)>,
    pub step: f64,
    /// The group runs a per-voice filter or EQ.
    pub filtered: bool,
    pub phase: voice::Phase,
    /// Deferred live loop edits are counted once each and retried at control ticks.
    pub loop_control_deferred: bool,
    pub loop_deferred_edits: u32,
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

#[cfg(test)]
mod release_note_mono_tests {
    use super::*;

    fn engine(mono: bool, release_only: bool, until_release: bool) -> Engine {
        let groups = vec![
            crate::import::Group { muted: release_only, ..Default::default() },
            crate::import::Group { release_trigger: true, release_trigger_note_monophonic: mono, ..Default::default() },
            crate::import::Group { release_trigger: true, ..Default::default() },
        ];
        let zones = (0..groups.len()).map(|group| crate::import::Zone {
            group, loop_range: Some(crate::import::Loop { start:0, end:64, alternating:false, until_release, crossfade:0 }),
            ..Default::default()
        }).collect();
        let sample = crate::audio::Sample { rate:48000, frames:vec![[0.2;2];16384] };
        let bank = Bank::from_samples(groups, zones, vec![(std::path::PathBuf::new(),sample)]).unwrap();
        let mut engine = Engine { attack: 0.0001, release: 0.001, ..Engine::default() };
        engine.set_bank(Some(Box::new(bank))); engine
    }
    fn tick(e: &mut Engine) { e.render(&mut [0.;512], &mut [0.;512]); }
    fn tails(e: &Engine) -> usize { e.voice_census().iter().filter(|v|v.release_trigger && v.group==1).count() }

    #[test]
    fn all_sound_off_relinquishes_note_owners_before_late_key_up() {
        let mut e = engine(false, false, false);
        e.note_on_from(0, 1, 60, 100);
        e.note_on_from(1, 2, 61, 100);
        assert_eq!(crate::test_support::allocations(|| {
            e.cc_from(0, 1, 120, 0);
            assert!(e.player.voices.iter().filter(|v| v.channel == 0).all(|v| !v.held && v.fade.dying()));
            assert!(e.player.voices.iter().filter(|v| v.channel == 1).all(|v| v.held && !v.fade.dying()));
            // Key-up arrives before even one sample of the stop fade is rendered.
            e.note_off_from(0, 1, 60);
            assert!(!e.player.voices.iter().any(|v| v.release_trigger));
            tick(&mut e);
            assert!(!e.player.voices.iter().any(|v| v.channel == 0));
            assert!(e.key_down(1, 61));
            // Fresh notes retain their normal native release behavior. Neither
            // the stopped key nor another physical owner is released again.
            e.note_on_from(0, 1, 60, 100);
            e.note_off_from(0, 1, 60);
            assert_eq!(e.player.voices.iter().filter(|v| v.release_trigger).count(), 2);
            assert!(e.player.voices.iter().filter(|v| v.release_trigger).all(|v| v.channel == 0 && v.input_channel == Some(1)));
            assert!(e.player.voices.iter().any(|v| v.channel == 1 && v.held));
            tick(&mut e);
        }), 0);
        assert_eq!(e.dropped_commands(), 0);

        // Channel-mode inputs can share one logical home and the same key.
        // The physical stop must relinquish only its selected input owner.
        let mut e = engine(false, false, false);
        e.note_on_from(7, 1, 60, 100);
        e.note_on_from(7, 2, 60, 100);
        assert_eq!(crate::test_support::allocations(|| {
            e.all_sound_off_from(7, 1 << 1);
            assert!(e.player.voices.iter().filter(|v| v.input_channel == Some(1)).all(|v| !v.held && v.fade.dying()));
            assert!(e.player.voices.iter().filter(|v| v.input_channel == Some(2)).all(|v| v.held && !v.fade.dying()));
            e.note_off_from(7, 1, 60);
            assert!(!e.player.voices.iter().any(|v| v.release_trigger));
            assert!(e.key_down(7,60), "the other physical input still holds the shared key");
            tick(&mut e);
            assert!(!e.player.voices.iter().any(|v| v.input_channel == Some(1)));
            e.note_off_from(7, 2, 60);
            assert!(!e.key_down(7,60));
            assert_eq!(e.player.voices.iter().filter(|v| v.release_trigger).count(), 2);
            assert!(e.player.voices.iter().filter(|v| v.release_trigger).all(|v| v.input_channel == Some(2)));
            tick(&mut e);
        }),0);
        assert_eq!(e.dropped_commands(),0);
    }

    #[test]
    fn note_mono_scan_is_prepared_only_for_enabled_release_groups() {
        for (release_trigger, note_mono, expected) in [(false, true, false), (true, false, false), (true, true, true)] {
            let groups = vec![crate::import::Group { release_trigger, release_trigger_note_monophonic: note_mono, ..Default::default() }];
            let bank = Bank::from_samples(groups, Vec::new(), Vec::new()).unwrap();
            assert_eq!(bank.note_mono_releases, expected);
        }
    }

    #[test]
    fn note_mono_cuts_only_matching_sounding_release_tails() {
        for mono in [false,true] {
            for (release_only,until_release) in [(false,false),(true,false),(false,true),(true,true)] {
                let mut e=engine(mono,release_only,until_release);
                e.note_on_from(0,1,60,100); tick(&mut e);
                e.note_off_from(0,1,60); tick(&mut e);
                assert_eq!(tails(&e),1);
                e.note_on_from(0,2,60,100); tick(&mut e);
                e.note_on_from(1,1,60,100); tick(&mut e);
                e.note_on_from(0,1,61,100); tick(&mut e);
                assert_eq!(tails(&e),1,"other input owners, logical channels and pitches preserve the old tail");
                e.note_on_from(0,1,60,100); tick(&mut e);
                assert_eq!(tails(&e),usize::from(!mono),"a same-key restrike obeys the authored flag even in a release-only instrument");
                assert_eq!(e.voice_census().iter().filter(|v|v.release_trigger && v.group==2).count(),1,"polyphonic sibling group remains sounding");
            }
            for pedal in [64,66] {
                let mut e=engine(mono,false,false);
                e.note_on(0,60,100); tick(&mut e); e.note_off(0,60); tick(&mut e);
                if pedal==64 {e.cc(0,pedal,127);}
                e.note_on(0,60,100); tick(&mut e);
                if pedal==66 {e.cc(0,pedal,127);tick(&mut e);}
                e.note_off(0,60); tick(&mut e);
                assert_eq!(tails(&e),usize::from(!mono),"the new pedal-held key cuts a prior sounding tail");
                e.cc(0,pedal,0); tick(&mut e);
                assert_eq!(tails(&e),1+usize::from(!mono),"pedal-up starts the deferred release without coalescing events");
            }
            for duration in [-1,0,5000] {
                let mut e=engine(mono,false,false);
                let source=format!("on init\nSET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)\nend on\non note\ndisallow_group($ALL_GROUPS)\nallow_group(0)\nend on\non release\ndisallow_group($ALL_GROUPS)\nallow_group(1)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,{duration})\nend on");
                let (runtime,errors)=crate::ksp::Runtime::with_scripts(&[&source],&mut crate::ksp::LogEngine::new(Vec::new(),48000.),3,Vec::new());
                assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(runtime)));
                e.note_on(0,60,100);tick(&mut e);e.note_off(0,60);tick(&mut e);
                assert_eq!(tails(&e),1,"the authored manual release starts for duration {duration}");
                e.note_on(0,60,100);tick(&mut e);
                assert_eq!(tails(&e),usize::from(!mono),"whole, fixed-duration and following-parent release samples obey Note Mono");
                assert!(e.script().unwrap().diagnostics().is_empty());
            }
        }
    }
}

#[cfg(all(test, feature = "plugin"))]
mod release_mono_tests;

#[cfg(all(test, feature = "plugin"))]
mod channel_mode_tests;

#[cfg(all(test, feature = "plugin"))]
mod held_key_tests;

#[cfg(all(test, feature = "plugin"))]
mod mono_legato_tests;

#[cfg(test)]
mod service_home_tests {
    use super::*;
    use crate::modulation::{ModAssignment, ModSource, ModTarget};

    #[test]
    fn configured_service_controllers_reach_resident_home_and_mpe_voices_without_heap() {
        for mpe in [false, true] {
            let group = crate::import::Group {
                mods: vec![ModAssignment { cc_default: None, name: "CC1_VOLUME".into(), source: ModSource::MidiCc(1),
                    target: ModTarget::Volume, intensity: 1.0, invert: false, lag_ms: 0, shaper: None }],
                ..Default::default()
            };
            let zone = crate::import::Zone { root:60, low_key:60, high_key:60,
                loop_range: Some(crate::import::Loop { start:0, end:64, alternating:false, until_release:false, crossfade:0 }),
                ..Default::default() };
            let bank = Bank::from_samples(vec![group], vec![zone], vec![(std::path::PathBuf::new(),
                crate::audio::Sample { rate:48000, frames:vec![[0.25;2];4096] })]).unwrap();
            let source = "on init\ndeclare ui_slider $amount(0,127)\ndeclare ui_switch $timer\nset_listener($NI_SIGNAL_TIMER_MS,1000)\nend on\non ui_control($amount)\nset_controller(1,$amount)\nend on\non listener\nif ($timer=1)\nset_controller(1,64)\n$timer:=0\nend if\nend on\non controller\nif ($CC_NUM=2)\nset_controller(3,%CC[2])\nend if\nend on";
            let (runtime, errors) = Runtime::with_scripts(&[source], &mut crate::ksp::LogEngine::default(), 1, Vec::new());
            assert!(errors.iter().all(Option::is_none));
            let mut rack = Rack::with_slots(1);
            rack.set_controls(&Mix { parts:vec![PartControls { channel:7, ..Default::default() }], ..Default::default() });
            let e = rack.parts[0].instrument_mut().unwrap();
            e.set_bank(Some(Box::new(bank)));
            e.set_script(Some(Box::new(runtime)));
            if mpe { e.set_mpe_zone(Some((0, (1 << 1) | (1 << 2)))); }
            let (voice_channel, controller_channel) = if mpe { (1, 0) } else { (7, 7) };
            let (mut left, mut right) = ([0.;512], [0.;512]);
            assert_eq!(crate::test_support::allocations(|| {
                // The configured home is already available before any MIDI.
                e.ui_control(0,0,127);
                e.render(&mut left,&mut right);
                assert_eq!(e.player.midi.cc[controller_channel][1].midi1(),127);
                e.cc(controller_channel as u8,1,0);
                e.note_on(voice_channel as u8,60,100);
                e.render(&mut left,&mut right);
                assert!(left.iter().chain(&right).all(|v| v.abs() < 1e-7));
                // An unrelated ingress must not select the service channel.
                e.note_on_from(3,9,61,100);
                e.cc_from(3,9,2,77);
                e.ui_control(0,0,127);
                e.render(&mut left,&mut right);
                assert_eq!(e.player.midi.cc[3][3].midi1(),77);
                assert_eq!(e.player.midi.cc[controller_channel][3].midi1(),77,
                    "authored set_controller broadcasts even inside a physical callback");
                assert_eq!(e.player.midi.cc[controller_channel][1].midi1(),127);
                assert!(left.iter().chain(&right).all(|v| v.is_finite()));
                let full = left[511];
                assert!(full > 0.01,"UI CC reaches the resident voice's modulation input");
                e.ui_control(0,1,1);
                e.render(&mut left,&mut right);
                assert_eq!(e.player.midi.cc[controller_channel][1].midi1(),64);
                assert!((left[511] / full - 64.0 / 127.0).abs() < 0.015,"listener CC reaches the same voice");
                if mpe {
                    assert_eq!(e.player.midi.cc[1][1].midi1(),64);
                    assert_eq!(e.player.midi.cc[2][1].midi1(),64);
                }
            }),0);
            assert_eq!(e.dropped_commands(),0);
            assert!(e.script().unwrap().diagnostics().is_empty());
        }
    }
}

#[cfg(test)]
mod midi_boundary_tests {
    use super::*;

    #[test]
    fn invalid_stop_controllers_preserve_valid_note_owners() {
        use crate::{
            audio::Sample,
            engine::{Bank, HostNote},
            import::{Group, Zone},
        };
        let bank = Bank::from_samples(
            vec![Group::default()],
            vec![Zone::default()],
            vec![(
                std::path::PathBuf::new(),
                Sample {
                    rate: 48_000,
                    frames: vec![[0.25; 2]; 4096],
                },
            )],
        )
        .unwrap();
        let mut engine = Engine::default();
        engine.set_bank(Some(Box::new(bank)));
        for (id, channel) in [(1, 15), (2, 4)] {
            let note = HostNote {
                port: 0,
                channel,
                key: 60,
                id,
                clap: true,
            };
            assert!(engine.host_note_on(note, channel, 60, 127, 0.0));
        }
        assert_eq!(engine.active_voices(), 2);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for channel in [16, 255] {
                    for cc in [120, 123] {
                        engine.cc(channel, cc, 0);
                        engine.cc_from(channel, 4, cc, 0);
                        engine.cc_from(4, channel, cc, 0);
                    }
                    engine.all_sound_off_from(channel, 1 << 4);
                }
            }),
            0
        );
        assert!(engine.host_key_held(15, 60));
        assert!(engine.host_key_held(4, 60));
        assert!(
            engine
                .player
                .voices
                .iter()
                .all(|v| !v.released && !v.fade.dying())
        );
        engine.cc(15, 123, 0);
        assert!(!engine.host_key_held(15, 60));
        assert!(engine.host_key_held(4, 60));
    }
}

#[cfg(test)]
mod timer_dsp_tests {
    use super::*;
    use crate::{audio::Sample, fx::{Chain, Effect, Kind, Params, ProgramFx, params::Gainer}, import::{Group, Instrument, Zone}, ksp::Value};

    #[test]
    fn legacy_disabled_listener_initialization_and_timer_actions_reach_pcm() {
        // Regression for the reported TIMER_MS=-1 init failure. Original
        // instruments are unavailable; native disable evidence is in audits/KSP_LISTENER.md.
        let instrument = Instrument { groups: vec![Group::default()], zones: vec![Zone::default()], scripts: vec![r#"on init
make_perfview
declare ui_label $ready(1,1)
declare $voice
declare $ticks
make_persistent($ticks)
set_listener($NI_SIGNAL_TIMER_MS,-1)
set_text($ready,"ready")
end on
on note
$voice := $EVENT_ID
change_listener_par($NI_SIGNAL_TIMER_MS,1000)
end on
on listener
inc($ticks)
change_vol($voice,-6000*$ticks,0)
if ($ticks=2)
change_listener_par($NI_SIGNAL_TIMER_MS,-1)
end if
end on"#.into()], ..Default::default() };
        let create = |scripted| {
            let mut engine = Engine::default();
            engine.reset(48_000.);
            engine.attack = 0.;
            engine.set_bank(Some(Box::new(Bank::from_samples(instrument.groups.clone(), instrument.zones.clone(),
                vec![(Default::default(), Sample { rate: 48_000, frames: vec![[0.25,-0.125];1024] })]).unwrap()
                .with_prepared_settings(|settings| settings[0].envelope = Some(Ahdsr::UNITY)).unwrap())));
            if scripted {
                let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
                assert!(errors.is_empty(),"{errors:?}");
                engine.set_script(runtime);
            }
            engine
        };
        let (mut actual, mut oracle) = (create(true), create(false));
        let ui = actual.script().unwrap().interface(0);
        assert!(ui.performance);
        assert_eq!(ui.controls[0].properties["$CONTROL_PAR_TEXT"],Value::Text("ready".into()));
        assert!(!ui.listeners.contains_key("$NI_SIGNAL_TIMER_MS"));
        actual.begin_audio_block(512,1,true);
        let mut pcm = [[0.;512];2];
        let mut expected = [[0.;512];2];
        let mut event = EventId::default();
        for (stage, range) in [0..96,96..144,144..192,192..512].into_iter().enumerate() {
            assert_eq!(crate::plugin::tests::allocations(|| {
                if stage == 1 {
                    actual.note_on(0,60,127);
                    event = oracle.start_event(&NoteEvent::new(0,60,127)).unwrap();
                } else if stage >= 2 {
                    oracle.change_event(event,EventChange::Volume(if stage == 2 { 0.5011872 } else { 0.25118864 }));
                }
                let [left,right] = &mut pcm;
                actual.render_checked(&mut left[range.clone()],&mut right[range.clone()]).unwrap();
                let [left,right] = &mut expected;
                oracle.render_checked(&mut left[range.clone()],&mut right[range.clone()]).unwrap();
            }),0);
            assert_eq!(actual.script().unwrap().now(),range.end as u64);
            assert_eq!(actual.script().unwrap().persistence()[0]["$ticks"],Value::Int([0,0,1,2][stage]),
                "disabled initialization schedules nothing; active callbacks run at144 and192 and stop thereafter");
        }
        assert!(pcm.iter().flatten().zip(expected.iter().flatten()).all(|(a,b)| (a-b).abs()<1e-6),
            "listener gain actions must match independently scheduled final stereo PCM");
        assert!(pcm[0][96..144].iter().any(|x| *x>0.1),"note callback must reach audible sample playback");
        // The second gain change at192 settles after the existing240-frame
        // voice ramp, at432; inspect audible output beyond that horizon.
        assert!(pcm[0][480..].iter().all(|x| (*x-0.25*0.25118864).abs()<1e-6));
        let ui = actual.script().unwrap().interface(0);
        assert!(!ui.listeners.contains_key("$NI_SIGNAL_TIMER_MS"));
        assert_eq!(ui.controls[0].properties["$CONTROL_PAR_TEXT"],Value::Text("ready".into()));
        assert!(actual.script().unwrap().diagnostics().is_empty());
        assert_eq!(actual.underruns(),0);
    }

    #[test]
    fn equal_time_listener_waits_and_fuel_deferred_callbacks_follow_complete_dsp_without_heap() {
        const FRAMES: usize = 512;
        let instrument = Instrument { groups: vec![Group::default()], zones: vec![Zone::default()], scripts: vec![r#"on init
declare $first
declare $second
declare $which
declare $ticks
declare $order
declare %seen[6]
make_persistent($order)
make_persistent(%seen)
set_listener($NI_SIGNAL_TIMER_MS,1000)
end on
on note
inc($which)
if ($which=1)
$first := $EVENT_ID
else
$second := $EVENT_ID
end if
wait(1000)
if ($EVENT_ID=$first)
change_vol($EVENT_ID,-6000,0)
$order := $order*10+1
else
change_vol($EVENT_ID,-12000,0)
$order := $order*10+2
end if
wait(1000)
if ($EVENT_ID=$first)
change_vol($EVENT_ID,-12000,0)
$order := $order*10+3
else
change_vol($EVENT_ID,-24000,0)
$order := $order*10+4
end if
wait(0)
if ($EVENT_ID=$first)
%seen[4] := get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS)
else
%seen[5] := get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS)
end if
end on
on listener
inc($ticks)
if ($ticks=1)
%seen[0] := get_event_par($first,$EVENT_PAR_PLAY_POS)
%seen[1] := get_event_par($second,$EVENT_PAR_PLAY_POS)
else
%seen[2] := get_event_par($first,$EVENT_PAR_PLAY_POS)
%seen[3] := get_event_par($second,$EVENT_PAR_PLAY_POS)
change_listener_par($NI_SIGNAL_TIMER_MS,0)
end if
end on"#.into()], ..Default::default() };
        let create = |scripted| {
            let mut engine = Engine::default(); engine.reset(48_000.); engine.attack = 0.;
            engine.set_bank(Some(Box::new(Bank::from_samples(instrument.groups.clone(), instrument.zones.clone(),
                vec![(Default::default(), Sample { rate: 48_000, frames: vec![[0.25, -0.125]; 4096] })]).unwrap()
                .with_prepared_settings(|settings| settings[0].envelope = Some(Ahdsr::UNITY)).unwrap())));
            engine.set_fx(ProgramFx { main: Chain { slots: vec![Effect { slot: 0, kind: Kind::Gainer,
                version: 0, bypass: false, output_gain: 1., dry_level: 0., params: Params::Gainer(Gainer { gain: 0.8 }) }] },
                ..Default::default() }.processor(48_000., MAX_BLOCK));
            if scripted {
                let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
                assert!(errors.is_empty(), "{errors:?}"); engine.set_script(runtime);
            }
            engine
        };
        let mut recorded = [[[0.; FRAMES]; 2]; 2];
        for (mode, pcm) in recorded.iter_mut().enumerate() {
            let (mut actual, mut oracle) = (create(true), create(false));
            actual.begin_audio_block(FRAMES, 1, true);
            let mut events = [EventId::default(); 2];
            assert_eq!(crate::plugin::tests::allocations(|| {
                actual.note_on(0, 60, 127); actual.note_on(1, 60, 127);
                events[0] = oracle.start_event(&NoteEvent::new(0, 60, 127)).unwrap();
                events[1] = oracle.start_event(&NoteEvent::new(1, 60, 127)).unwrap();
            }), 0);
            let mut expected = [[0.; FRAMES]; 2];
            // The96-frame timer is deliberately fuel-deferred while audio
            // continues to192. Its callback must observe192, then wait(0)
            // resumes after one genuinely rendered frame, at193.
            for (stage, range) in [0..48, 48..96, 96..192, 192..193, 193..FRAMES].into_iter().enumerate() {
                assert_eq!(crate::plugin::tests::allocations(|| {
                    if stage == 1 {
                        oracle.change_event(events[0], EventChange::Volume(0.5011872));
                        oracle.change_event(events[1], EventChange::Volume(0.25118864));
                    } else if stage == 2 {
                        actual.script.as_deref_mut().unwrap().env.block_fuel = 0;
                    } else if stage == 3 {
                        actual.begin_audio_block(FRAMES - range.start, 1, true);
                        oracle.change_event(events[0], EventChange::Volume(0.25118864));
                        oracle.change_event(events[1], EventChange::Volume(0.06309573));
                    }
                    // No frame exists at this call, so a due timer remains
                    // pending and neither runtime nor DSP time advances.
                    actual.render(&mut [], &mut []);
                    oracle.render(&mut [], &mut []);
                    let sizes: &[usize] = if mode == 0 { &[128] } else { &[1, 7, 31, 128] };
                    let (mut frame, mut block) = (range.start, 0);
                    while frame < range.end {
                        let length = sizes[block % sizes.len()].min(range.end - frame);
                        let [left, right] = &mut *pcm;
                        actual.render_checked(&mut left[frame..frame + length], &mut right[frame..frame + length]).unwrap();
                        let [left, right] = &mut expected;
                        oracle.render_checked(&mut left[frame..frame + length], &mut right[frame..frame + length]).unwrap();
                        frame += length; block += 1;
                    }
                }), 0);
                assert_eq!(actual.script().unwrap().now(), range.end as u64);
                assert_eq!(actual.player.now, range.end as u64);
                let persisted = actual.script().unwrap().persistence();
                let state = &persisted[0];
                let order = [0, 12, 12, 1234, 1234][stage];
                assert_eq!(state["$order"], Value::Int(order), "same-time timers retain their sequence and render-end exclusivity");
                let seen = match &state["%seen"] { Value::IntArray(values) => &values[..], _ => unreachable!() };
                let positions = match stage {
                    0 => [0; 6], 1 | 2 => [1000, 1000, 0, 0, 0, 0],
                    3 => [1000, 1000, 4000, 4000, 0, 0],
                    _ => [1000, 1000, 4000, 4000, 4020, 4020],
                };
                assert_eq!(seen, positions, "listener and waits must observe preceding DSP, including fuel deferral");
            }
            assert!(pcm.iter().flatten().zip(expected.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-6),
                "waited gain actions differ from independently scheduled complete final PCM");
            let level = 0.25 * 0.8 * (0.25118864 + 0.06309573);
            assert!(pcm[0][448..].iter().all(|x| (*x - level).abs() < 1e-6));
            assert!(pcm[1][448..].iter().all(|x| (*x + level * 0.5).abs() < 1e-6));
            assert_eq!(actual.underruns(), 0);
        }
        assert!(recorded[0].iter().flatten().zip(recorded[1].iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
