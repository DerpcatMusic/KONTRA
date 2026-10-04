//! Playback state: voice admission, key/pedal lifetimes and rendering.
//!
//! The Engine schedules commands and borrows prepared bank/effect data; Player
//! retains each voice, deferred release and preallocated rendering workspace.

use super::voice::{Context, Envelope, Fade, Scratch, Stream, Voice, balance};
use super::{
    Ahdsr, Bank, DECLICK, EventChange, EventId, Expression, GroupMask, MAX_BLOCK,
    MAX_VOICES, NoteEvent, STEAL_FADE,
    filter::{self, VoiceFilter},
    host_notes,
    instrument::InstrumentState,
    lfo,
    map::{self, FOREVER},
    midi_state, params, release::{Capture, PendingRelease, ReleaseRequest, ReleaseSample, ReleaseSnapshot},
    script, stream, voice, wavetable,
};
use crate::fx::FxProcessor;
use crate::midi::Velocity;
use std::sync::atomic::Ordering;

/// Unscripted physical key state, including release-only note provenance.
#[derive(Clone, Copy, Default)]
pub(super) struct InputKey {
    pub(super) channel: u8,
    pub(super) onset_velocity: Option<Velocity>,
    pub(super) per_note: Option<super::PerNoteInput>,
    pub(super) started: u64,
    pub(super) sostenuto: bool,
    pub(super) accepted: Option<super::note_mode::AcceptedNote>,
}

/// Engine state apart from the bank, so voices can mutate while the bank is borrowed.
/// Authored output survives an instrument disabling standard MIDI controls.
/// An authored write also wins over the preceding physical override.
/// Received standard controls keep canonical values until the instrument's
/// policy chooses compatibility output or native instrument controls.
struct StandardControl {
    authored: f32,
    physical: Option<crate::midi::ControlValue>,
}

impl StandardControl {
    fn new(authored: f32) -> Self { Self { authored, physical: None } }

    fn receive(&mut self, value: crate::midi::ControlValue, target: crate::midi::ControlTarget, authored: f32) {
        match target {
            crate::midi::ControlTarget::Authored => {
                self.authored = authored;
                self.physical = None;
            }
            crate::midi::ControlTarget::Physical(_) => self.physical = Some(value),
        }
    }

    fn output(&self, state: &InstrumentState) -> f32 {
        if state.controller_policy == super::instrument::ControllerPolicy::Compatibility
            && state.controller_options.standard_volume_pan {
            self.physical.map_or(self.authored, |value| value.dsp().powi(2))
        } else {
            self.authored
        }
    }
}

pub(super) struct Player {
    pub(super) host_notes: host_notes::Owners,
    pub(super) per_note: super::per_note::InputControls,
    pub(super) voices: Vec<Voice>,
    /// Prepared route rows not currently owned by a voice.
    pub(super) route_spares: Vec<voice::VoiceRoutes>,
    /// Free stream slots of the current bank.
    pub(super) free: Vec<u16>,
    /// Samples voices play, gathered to swap heads safely.
    pub(super) in_use: Vec<u32>,
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
    pub(super) shared: bool,
    pub(super) rate: f64,
    pub(super) tempo: f32,
    /// Previous globally accepted native source refresh BPM; bank install preserves it.
    pub(super) native_refresh_tempo: Option<f32>,
    /// KSP preprocessor flags bypass only these native system-script actions.
    pub(super) native_sustain: bool,
    pub(super) native_mpe_pressure: bool,
    pub(super) native_release_triggers: bool,
    pub(super) native_group_start: bool,
    pub(super) group_clock: super::group_start::Clock,
    pub(super) library_receiver: Box<super::group_start::Receiver>,
    pub(super) release_counter: super::rtc::Counter,
    pub(super) note_history: super::instrument::NoteHistory,
    pub(super) pedals: super::pedals::Pedals,
    /// Release samples deferred by sustain or their own sostenuto capture.
    pub(super) pending_releases: Vec<PendingRelease>,
    pub(super) midi: midi_state::MidiState,
    /// Velocity of keys that are down.
    pub(super) keys: [[u8; 128]; 16],
    /// Unscripted physical inputs: engine channel and velocity. Prepared off-thread.
    pub(super) input_keys: Box<[[InputKey; 128]; 16]>,
    /// Frames rendered so far.
    pub(super) now: u64,
    /// Frame each key went down (or its release counter was reset) and up:
    /// the release-trigger counter's start and stop. Boxed: a rack's engines
    /// are built on the stack.
    pub(super) key_on: Box<[[u64; 128]; 16]>,
    pub(super) key_up: Box<[[u64; 128]; 16]>,
    pub(super) allowed: GroupMask,
    next_event: u32,
    event_wrapped: bool,
    pub(super) note_selected: [Option<super::note_mode::Selected>; 16],
    pub(super) note_ordinal: u64,
    pub(super) note_off_depth: u32,
    pub(super) note_off_pending: u16,
    clock: u64,
    pub(super) admission_clock: u64,
    /// Authored output controls and the most recent physical CC7/CC10 override.
    volume: StandardControl,
    pan: StandardControl,
    /// Next sample's output gains; control changes ramp over five milliseconds.
    pub(super) out_gains: [f32; 2],
    out_start: [f32; 2],
    out_target: [f32; 2],
    out_remaining: u32,
    out_frames: u32,
    pub(super) tone: [f32; 2],
    pub(super) underruns: u64,
    pub(super) stream_error: std::cell::Cell<Option<stream::StreamError>>,
    pub(super) dropped_commands: u64,
    pub(super) unsupported_native_controls: u64,
    /// Channels requiring click-free recovery after the bounded stop quota fills.
    pub(super) stop_overflow: u16,
}

impl Player {
    pub(super) fn new(rate: f64) -> Self {
        let mut player = Self {
            host_notes: host_notes::Owners::new(),
            per_note: super::per_note::InputControls::new(),
            voices: Vec::with_capacity(MAX_VOICES),
            route_spares: Vec::new(),
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
            tempo: 120.,
            native_refresh_tempo: None,
            native_sustain: true,
            native_mpe_pressure: true,
            native_release_triggers: true,
            native_group_start: true,
            group_clock: Default::default(),
            library_receiver: Default::default(),
            release_counter: Default::default(),
            note_history: Default::default(),
            pedals: Default::default(),
            pending_releases: Vec::with_capacity(crate::ksp::EVENT_CAPACITY),
            midi: midi_state::MidiState::default(),
            keys: [[0; 128]; 16],
            input_keys: Vec::from_iter((0..16).map(|_| [InputKey::default(); 128]))
                .into_boxed_slice().try_into().unwrap_or_else(|_| unreachable!("sixteen input rows")),
            now: 0,
            key_on: Box::new([[0; 128]; 16]),
            key_up: Box::new([[0; 128]; 16]),
            allowed: GroupMask::all(),
            next_event: 0,
            event_wrapped: false,
            note_selected: [None; 16],
            note_ordinal: 0,
            note_off_depth: 0,
            note_off_pending: 0,
            clock: 0,
            admission_clock: 0,
            volume: StandardControl::new(1.0),
            pan: StandardControl::new(0.0),
            out_gains: [1.0; 2],
            out_start: [1.0; 2],
            out_target: [1.0; 2],
            out_remaining: 0,
            out_frames: 1,
            tone: [0.0; 2],
            underruns: 0,
            stream_error: std::cell::Cell::new(None),
            dropped_commands: 0,
            unsupported_native_controls: 0,
            stop_overflow: 0,
        };
        player.reset_midi();
        player
    }

    pub(super) fn reset_midi(&mut self) {
        self.stop_overflow = 0;
        self.pedals = Default::default();
        self.pending_releases.clear();
        self.midi.reset();
        self.per_note.reset();
        self.keys = [[0; 128]; 16];
        self.input_keys.fill([InputKey::default(); 128]);
        self.volume = StandardControl::new(1.0);
        self.pan = StandardControl::new(0.0);
        self.tone = [0.0; 2];
    }

    /// Something modulation reads changed: settled voices modulate again.
    pub(super) fn touch(&mut self) {
        self.midi.touch();
    }

    pub(super) fn clear_voices(&mut self, bank: Option<&Bank>) {
        let slots = bank.map_or(&[][..], Bank::slots);
        for v in self.voices.drain(..) {
            self.host_notes.record_end(v.host_note, self.now);
            if let Some(stream) = v.stream {
                slots[stream.slot as usize].stop();
                self.free.push(stream.slot);
            }
            self.route_spares.push(v.into_routes());
        }
        // Note ownership belongs to these voices and their deferred releases,
        // not to a replacement bank. Physical controller/PCK state survives.
        self.pending_releases.clear();
        self.keys.fill([0; 128]);
        self.input_keys.fill([InputKey::default(); 128]);
        self.key_on.fill([self.now; 128]);
        self.key_up.fill([self.now; 128]);
    }

    /// Move playing voices from `old` to `new`, which holds the same samples
    /// with other residency: each finds its resident span and limit as at a
    /// start, and streams through a slot of `new` from where it is.
    pub(super) fn rebind(&mut self, old: &Bank, new: &Bank) {
        let slots = old.slots();
        self.free.clear();
        self.free.extend((0..new.slots().len() as u16).rev());
        for v in &mut self.voices {
            if let Some(stream) = v.stream.take() {
                slots[stream.slot as usize].stop();
            }
            let sample = &new.samples[v.sample as usize];
            if let Some(table) = v.wavetable {
                v.span = sample
                    .wavetable_span(table)
                    .expect("resident wavetable after bank upgrade");
                continue;
            }
            let first = (v.pos as u64).saturating_sub(1);
            v.span = (v.map.run(first, v.wraps))
                .and_then(|run| sample.span_at(run.frame))
                .unwrap_or(0);
            let span = &sample.spans[v.span as usize];
            v.limit = if v.counted.is_some() { 0 } else { v.map.resident_limit(first, v.wraps, span.start, span.end()) };
            // Paused, the next render configures it from the position, as
            // when a muted voice returns; a loop reserves it as at a start.
            let slot = if sample.streamed {
                self.free.pop()
            } else {
                None
            };
            v.stream = slot.map(|slot| voice::Stream {
                slot,
                tag: 0,
                trusted: 0,
                paused: v.limit != FOREVER,
                sparse: None,
            });
        }
    }

    fn remove(&mut self, bank: &Bank, index: usize) {
        let v = self.voices.swap_remove(index);
        self.host_notes.record_end(v.host_note, self.now);
        if let Some(stream) = v.stream {
            bank.slots()[stream.slot as usize].stop();
            self.free.push(stream.slot);
        }
        self.route_spares.push(v.into_routes());
    }

    pub(super) fn fade_frames(&self, seconds: f32) -> u32 {
        self.frames(seconds).max(1)
    }

    pub(super) fn frames(&self, seconds: f32) -> u32 {
        (seconds.max(0.0) * self.rate as f32) as u32
    }

    /// How long the key was held when its release-trigger counter stopped:
    /// until now while it is down, else until it went up.
    fn held_ms(&self, channel: u8, note: u8) -> f32 {
        let (c, n) = (channel as usize & 15, note as usize & 127);
        let end = if self.keys[c][n] > 0 {
            self.now
        } else {
            self.key_up[c][n]
        };
        end.saturating_sub(self.key_on[c][n]) as f32 * 1000.0 / self.rate as f32
    }

    pub(super) fn next_id(&mut self) -> EventId {
        // Every protected ID occupies at most one distinct candidate. The
        // prepared ownership bounds make this search finite even across wrap.
        // Local script-free mono roots are protected through wrap. KSP-only
        // queued identities without roots/audio retain their existing policy.
        let bound = self.voices.len() + self.pending_releases.len()
            + self.host_notes.active.len() + 16 * 128 + 16 + 1;
        for _ in 0..bound {
            self.event_wrapped |= self.next_event == u32::MAX;
            self.next_event = self.next_event.wrapping_add(1).max(1);
            let event = EventId(self.next_event);
            if !self.event_wrapped || !self.event_retained(event) { return event; }
        }
        unreachable!("bounded occupied event identities")
    }

    pub(super) fn change_event(&mut self, id: EventId, change: EventChange) {
        for v in self.voices.iter_mut().filter(|v| v.event == id) {
            match change {
                EventChange::Volume(gain) => v.volume = gain.max(0.0),
                EventChange::Tune(semitones) => v.tune = 2f64.powf(semitones / 12.0),
                EventChange::Pan(pan) => v.pan = pan,
                EventChange::ScriptMod { source, value } => {
                    if let Some(x) = v.script_mods.get_mut(source as usize) { *x = value; v.settled = None; }
                },
                EventChange::ReleaseVelocity(value) => {
                    v.release_velocity = Some(value);
                    v.settled = None;
                }
            }
        }
        if let EventChange::ReleaseVelocity(value) = change {
            for release in self.pending_releases.iter_mut().filter(|r| r.source == id) {
                release.release_velocity = Some(value);
            }
        }
    }

    /// Start `ev` under the caller-assigned `id`.
    pub(super) fn completion_supported(&self, bank: &Bank, state: &InstrumentState, zone: u32) -> bool {
        let group_id = bank.zones()[zone as usize].group;
        let group = &bank.groups()[group_id];
        let settings = &state.settings[group_id];
        let Some((slot, false)) = settings.primary_bypass else { return false; };
        group.native_primary_bypass_slot == Some(slot)
            && slot == 0 && group.modulators.len() == 1 && group.modulators[0].volume_env
            && group.native_volume_env && group.volume_env.is_some() && group.flex_env.is_none()
            && group.mods.is_empty() && group.envelopes.is_empty() && group.fx.slots.is_empty()
            && group.pitch_lfos.is_empty() && group.volume_lfos.is_empty() && group.pitch_glides.is_empty()
            && group.source_mode == Some(0) && !group.reverse && group.wavetable.is_none()
            && settings.native_volume_env && settings.flex.is_none() && settings.filter.is_none()
            && settings.mods.mods.is_empty() && settings.pitch_envelopes.is_empty()
            && settings.pitch_lfos.is_empty() && settings.volume_lfos.is_empty()
            && settings.pitch_glides.is_empty() && settings.wavetable.is_none()
            && settings.envelope.is_some_and(|p| super::ahdsr::Native::supports_completion(&p, self.rate as f32))
    }

    pub(super) fn completion_has_release(&self, bank: &Bank, channel: u8, note: u8, velocity: u8, groups: &GroupMask, whole: bool) -> bool {
        (whole || !self.native_release_triggers)
            && bank.matching_zones(channel, note, velocity, true, groups).next().is_some()
    }

    pub(super) fn start(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        ev: &NoteEvent,
        id: EventId,
        release_trigger: bool,
        defaults: Ahdsr,
    ) -> Option<EventId> {
        self.start_with_completion(bank, state, ev, id, release_trigger, defaults, false)
    }

    pub(super) fn start_with_completion(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        ev: &NoteEvent,
        id: EventId,
        release_trigger: bool,
        defaults: Ahdsr,
        force_completion: bool,
    ) -> Option<EventId> {
        self.start_admission(bank, state, ev, id, release_trigger, defaults, force_completion, None)
    }

    pub(super) fn start_admission(
        &mut self, bank: &Bank, state: &InstrumentState, ev: &NoteEvent, id: EventId,
        release_trigger: bool, defaults: Ahdsr, force_completion: bool,
        receipt: Option<&mut GroupMask>,
    ) -> Option<EventId> {
        if ev.channel >= 16 || ev.note >= 128 || ev.velocity > 127 {
            return None;
        }
        if let Some(range) = &state.shared_ahdsr {
            let groups = ev.groups.unwrap_or(&self.allowed);
            if bank.matching_zones(ev.channel,ev.note,ev.velocity,release_trigger,groups)
                .any(|zone| state.settings[bank.zones()[zone as usize].group].native_shared_volume_slot.is_some()
                    && (release_trigger || force_completion || !range.can_start(zone,ev,self.now))) {
                self.stream_error.set(Some(super::stream::StreamError { sample:0,frame:self.now,
                    kind:super::stream::StreamErrorKind::UnsupportedWindow }));
                return None;
            }
        }
        if force_completion {
            // Marked script Starts already own final Group-slot admission.
            // Recheck live source edits before releasing/stealing any sibling.
            let groups = ev.groups.unwrap_or(&self.allowed);
            let mut zones = bank.matching_zones(ev.channel, ev.note, ev.velocity, release_trigger, groups).peekable();
            if zones.peek().is_none() { return None; }
            if !ev.group_start_applied || zones.any(|z| !self.completion_supported(bank, state, z)) {
                self.unsupported_native_controls = self.unsupported_native_controls.saturating_add(1);
                return None;
            }
        }
        let mono = !release_trigger && state.note_modes[ev.channel as usize] == super::NoteMode::MonoRetrigger;
        if !mono { self.admission_prelude(bank, state, ev, release_trigger); self.clock += 1; }
        let mut group_clock = self.group_clock;
        let mut mask = *ev.groups.unwrap_or(&self.allowed);
        if !ev.group_start_applied {
            mask = if mono {
                bank.group_start.select(&mut group_clock, &self.library_receiver.cc[ev.channel as usize],
                    ev.note, ev.native_release, self.native_release_triggers, self.native_group_start, mask)
            } else { self.select_note_groups(bank, ev.channel, ev.note, ev.native_release, mask) };
        }
        self.pending.clear();
        for z in bank
            .matching_zones(ev.channel, ev.note, ev.velocity, release_trigger, &mask)
            .take(self.pending.capacity())
        {
            let zone = &bank.zones()[z as usize];
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
        let before = self.admission_clock;
        let mut admitted = GroupMask::none();
        let mut prelude = mono.then_some(group_clock);
        for i in 0..self.pending.len() {
            let (zone, gain) = self.pending[i];
            let prior = self.admission_clock;
            self.spawn(bank, state, zone, ev, id, gain, release_trigger, defaults, force_completion, &mut prelude);
            if self.admission_clock != prior { admitted.set(bank.zones()[zone as usize].group, true); }
        }
        if self.admission_clock != before && let Some(receipt) = receipt { *receipt = admitted; }
        Some(id)
    }

    fn admission_prelude(&mut self, bank: &Bank, state: &InstrumentState,
        ev: &NoteEvent, release_trigger: bool) {
        if state.controller_policy == super::instrument::ControllerPolicy::Native
            && self.native_release_triggers && !ev.native_release {
            // The ordinary final Group prelude resets even when no zone
            // matches. Ignored input never reaches this admission owner.
            self.release_counter.reset(self.now, ev.note, self.rate);
        }
        if !release_trigger && bank.note_mono_releases {
            // Kontakt Source module Note Mono cuts a still-sounding release
            // on the next same-key attack. Deferred pedal releases are not
            // playing yet, so their independent event identities stay intact.
            let input = ev
                .input_channel
                .or(ev.owner.map(|(channel, _)| channel))
                .unwrap_or(ev.channel);
            let fade = self.fade_frames(DECLICK);
            for voice in &mut self.voices {
                if voice.release_trigger
                    && voice.note == ev.note
                    && voice.channel == ev.channel
                    && voice
                        .input_channel
                        .or(voice.owner.map(|(channel, _)| channel))
                        .unwrap_or(voice.channel)
                        == input
                    && bank.groups()[voice.group as usize].release_trigger_note_monophonic
                    && !voice.fade.dying()
                {
                    voice.fade.start(0., fade, true);
                }
            }
        }
    }

    pub(super) fn select_note_groups(&mut self, bank: &Bank, channel: u8, note: u8,
        native_release: bool, groups: GroupMask) -> GroupMask {
        if channel >= 16 || note >= 128 { return groups; }
        bank.group_start.select(&mut self.group_clock, &self.library_receiver.cc[channel as usize], note,
            native_release, self.native_release_triggers, self.native_group_start, groups)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        z: u32,
        ev: &NoteEvent,
        event: EventId,
        gain: f32,
        release_trigger: bool,
        defaults: Ahdsr,
        force_completion: bool,
        prelude: &mut Option<super::group_start::Clock>,
    ) {
        let zone = &bank.zones()[z as usize];
        let play = &bank.plays[z as usize];
        let group = &bank.groups()[zone.group];
        let settings = &state.settings[zone.group];
        let sample = &bank.samples[play.sample as usize];
        // Bind the exact sealed receipt before usage, pressure or prepared
        // route ownership changes. A failed bind cannot drop a route row in RT.
        let mut sealed_amplitude = if settings.native_shared_volume_slot.is_some() {
            let mut amplitude = voice::Amplitude::new_shared(
                &settings.envelope.expect("prepared shared primary source"), self.rate as f32);
            let Some(admission) = self.admission_clock.checked_add(1) else {
                self.stream_error.set(Some(super::stream::StreamError { sample:play.sample,frame:self.now,
                    kind:super::stream::StreamErrorKind::UnsupportedWindow }));
                return;
            };
            if state.shared_ahdsr.as_ref().unwrap().bind_start(z,ev,admission,self.now,
                amplitude.shared_mut().unwrap()).is_err() {
                self.stream_error.set(Some(super::stream::StreamError { sample:play.sample,frame:self.now,
                    kind:super::stream::StreamErrorKind::UnsupportedWindow }));
                return;
            }
            Some(amplitude)
        } else { None };
        let mono = !release_trigger && state.note_modes[ev.channel as usize] == super::NoteMode::MonoRetrigger;
        if !mono {
            // One writer, the audio thread: no read-modify-write needed.
            let played = &bank.usage[play.sample as usize];
            played.store(
                played.load(Ordering::Relaxed).wrapping_add(1),
                Ordering::Relaxed,
            );
            self.make_room(bank, settings.voice_group);
        }

        let key = if group.key_tracking {
            2f64.powf((f64::from(ev.note) - f64::from(zone.root)) / 12.0)
        } else {
            1.0
        };
        let step = if play.wavetable.is_some() {
            wavetable::step(ev.note, self.rate) * zone.tune
        } else {
            f64::from(sample.rate) / self.rate * zone.tune * key
        };
        let physical = ev.input_channel.unwrap_or(ev.channel);
        let c = physical as usize;
        let expression_channel = ev.channel as usize;
        let expression_key = ev.owner.map_or(ev.note, |(_, key)| key);
        let expression = ev
            .frozen_expression
            .unwrap_or(self.midi.expression[expression_channel][expression_key as usize & 127]);
        let mut expression = expression;
        if ev.frozen_expression.is_none() {
            if let Some(zone) = self.midi.mpe.member(physical) {
                expression.tune += self.midi.bend[c].dsp() * self.midi.channel_bend_range[c].map(|range| range.semitones())
                    .unwrap_or_else(|| f64::from(zone.member_range.semitones())) as f32;
                if self.native_mpe_pressure && self.midi.pressure_active[c] {
                    expression.gain *= 10f32.powf((self.midi.pressure[c].dsp() - 1.) * 12. / 20.);
                }
            }
        }
        if self.midi.mpe.zone(physical).is_none() {
            expression.tune += self.midi.channel_bend_range[c].map_or(0., |range|
                self.midi.bend[c].dsp() * range.semitones() as f32);
        }
        let expression = self.per_note.overlay(ev.per_note, expression);
        let expression = self.host_notes.overlay(ev.host_note, expression);
        let mut inputs = self.midi.modulation_inputs(
            physical, ev.note, ev.onset_velocity, ev.release_velocity, expression);
        inputs.script_mods = ev.script_mods;
        inputs.per_note_cc = self.per_note.controllers(ev.per_note);
        inputs.counter = if release_trigger && state.controller_policy == super::instrument::ControllerPolicy::Native {
            // Native release admission samples the current shared key,
            // including after a callback wait or deferred pedal release.
            self.release_counter.factor(ev.note, group.release_counter_ms)
        } else if release_trigger {
            params::release_counter(
                group.release_counter_ms,
                ev.release_held_ms
                    .unwrap_or_else(|| self.held_ms(ev.channel, ev.note)),
            )
        } else {
            0.0
        };
        let bend_pitch = self.midi.mpe.zone(physical).map(|zone| {
            if zone.manager_range.is_some() || self.midi.channel_bend_range[zone.manager as usize].is_some() { 0. }
            else { self.midi.bend[zone.manager as usize].dsp() }
        }).or_else(|| self.midi.channel_bend_range[c].map(|_| 0.));
        if mono && self.route_spares.is_empty() { return; }
        let mut routes = self.route_spares.pop().expect("prepared voice modulation row");
        settings.mods.start(&mut routes.values, &inputs, bend_pitch);
        settings.mods.targets(&mut routes.targets, &inputs, bend_pitch);
        let modulated = (settings.mods.start_offset(&inputs) * play.start_mod as f32) as u64;
        let offset = if play.wavetable.is_some() {
            0
        } else {
            ((ev.offset_us as f64 * f64::from(sample.rate) / 1e6) as u64 + modulated)
                .min(play.start_mod)
        };
        let loop_offsets = settings.mods.loop_offsets();
        // Keep the saved width before play_map's static-start clamp: modulation
        // can move a zero-start loop far enough to admit its saved crossfade.
        let base_map = map::PlayMap {
            looped: play.map.looped.map(|mut l| {
                l.xfade = zone
                    .loop_range
                    .as_ref()
                    .map_or(l.xfade, |l| l.crossfade as u64);
                l
            }),
            ..play.map
        };
        let base_loop = (settings.mods.has_loop_controls()
            && zone.loop_range.as_ref().is_some_and(|l| !l.alternating)
            && base_map.controlled_loop([0.; 2], sample.frames).is_some())
        .then_some(base_map.looped)
        .flatten();
        let controlled =
            base_loop.and_then(|_| base_map.controlled_loop(loop_offsets, sample.frames));
        let loop_deferred = (base_loop.is_some() && controlled.is_none()).then_some(loop_offsets);
        let map = controlled.unwrap_or(play.map);
        let counted = play.counted.map(|rows| super::counted_loop::Cursor::new(rows,
            &bank.counted_loops[rows as usize], map.start as f64 + offset as f64, release_trigger));
        // A release-triggered voice starts with its key already up.
        let wraps = map.wraps(if release_trigger { offset } else { FOREVER });
        // The resident span holding the voice's first window frame (one
        // before the start, for the cubic's left tap), if any does.
        let first = offset.saturating_sub(1);
        let span_index = if let Some(table) = play.wavetable {
            sample
                .wavetable_span(table)
                .expect("prepared complete wavetable")
        } else {
            map.run(first, wraps)
                .and_then(|run| sample.span_at(run.frame))
                .unwrap_or(0)
        };
        let span = &sample.spans[span_index as usize];
        let limit = if counted.is_some() { 0 } else { map.resident_limit(first, wraps, span.start, span.end()) };
        let stream = if sample.streamed && play.wavetable.is_none() {
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
                    sparse: None,
                }
            } else {
                // From the voice's start on (`limit >= first`): an offset past
                // the resident range streams from there, not from the zone start.
                let stream_map = if counted.is_some() { map::PlayMap { start:0, end:sample.frames,
                    reverse:false, looped:None } } else { map };
                let tag = bank.slots()[slot as usize].configure(play.sample, &stream_map, wraps,
                    if counted.is_some() { 0 } else { limit }, if counted.is_some() { 0 } else { first });
                Stream {
                    slot,
                    tag,
                    trusted: limit,
                    paused: false,
                    sparse: None,
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
        let mut amplitude = sealed_amplitude.take().unwrap_or_else(||
            voice::Amplitude::new(&envelope, self.rate as f32, settings.native_volume_env));
        if force_completion { amplitude.force_completion(); }
        let admission = self.admission_clock + 1;
        if !mono { self.admission_clock = admission; }
        let mut voice = Voice {
            event,
            host_note: ev.host_note,
            per_note: ev.per_note,
            zone_id: play.zone_id,
            group: zone.group as u32,
            voice_group: settings.voice_group,
            channel: ev.channel,
            note: ev.note,
            velocity: ev.velocity,
            onset_velocity: ev.onset_velocity,
            release_velocity: ev.release_velocity,
            counter_start: self.now,
            counter_stop: ev.counter_stop,
            owner: ev.owner,
            input_channel: ev.input_channel,
            held: !release_trigger,
            sostenuto: false,
            released: false,
            release_trigger,
            frozen_expression: ev.frozen_expression,
            age: self.clock + u64::from(prelude.is_some()),
            admission,
            sample: play.sample,
            wavetable: play.wavetable,
            span: span_index,
            map,
            counted,
            base_loop,
            loop_offsets: if loop_deferred.is_some() {
                [f32::NAN; 2]
            } else {
                loop_offsets
            },
            loop_deferred,
            loop_deferred_edits: u32::from(loop_deferred.is_some()),
            wraps,
            length: if play.wavetable.is_some() || counted.is_some() {
                FOREVER
            } else {
                map.len(wraps)
            },
            limit,
            pos: if play.wavetable.is_some() {
                f64::from(settings.wavetable.unwrap().phase).rem_euclid(1.)
                    * wavetable::CYCLE as f64
            } else {
                offset as f64
            },
            step,
            tune: 2f64.powf(ev.tune / 12.0),
            pitch: (f32::NAN, 1.0),
            mods: routes.values,
            mod_targets: routes.targets,
            mod_preview: routes.preview,
            script_mods: {
                let mut values = [0; super::SCRIPT_MOD_SOURCES];
                let n = ev.script_mods.len().min(values.len());
                values[..n].copy_from_slice(&ev.script_mods[..n]);
                values
            },
            pitch_lfo: lfo::Clock::new(settings, state.lfo_timings.group(zone.group)),
            pitch_glide: super::glide::Clock::new(&settings.pitch_glides,
                self.note_history.interval(), self.rate as f32),
            pitch_envs: {
                let mut states =
                    [Envelope::new(&Ahdsr::UNITY, self.rate as f32); params::PITCH_ENVS];
                for (state, envelope) in states.iter_mut().zip(&settings.pitch_envelopes) {
                    *state = Envelope::new(&envelope.env, self.rate as f32);
                }
                states
            },
            modulated: (1.0, 0.0),
            settled: None,
            stream,
            env: amplitude,
            primary_amp: settings.primary_bypass.map(|p|
                super::envelope::PrimaryAmplitude::new(p.1, self.rate as f32)),
            flex: settings.flex.as_ref().map(|_| Envelope::flex()),
            fade: Fade::FULL,
            base_level,
            volume: ev.volume.max(0.0),
            base_pan: zone.pan,
            pan: ev.pan,
            gains: [0.0; 2],
            gain_start: [0.; 2],
            gain_target: [0.; 2],
            gain_remaining: 0,
            gain_frames: 1,
            muted: 0,
            onset: voice::Onset::New,
            filter: VoiceFilter::new_prepared(
                settings.filter.as_deref(),
                &settings.mods,
                &inputs,
                self.rate as f32,
                routes.external,
            ),
            plan: Default::default(),
        };
        // Start at the voice's first-block gains, so it does not ramp in.
        let (modulation, ..) = settings
            .mods
            .modulate(&mut voice.mods, &inputs, 0, 1.0, bend_pitch);
        let instrument = self.voice_controls(state);
        let pan = ((zone.pan + instrument.pan).clamp(-1.0, 1.0)
            + settings.pan
            + ev.pan
            + expression.pan)
            .clamp(-1.0, 1.0);
        voice.gains = balance(
            base_level
                * instrument.gain
                * settings.gain
                * modulation
                * voice.volume
                * expression.gain,
            pan,
        );
        if offset > 0 {
            voice.fade.fade_in(self.fade_frames(DECLICK));
        }
        if mono {
            if settings.pitch_lfos.iter().any(|source| source.shared) {
                let Some(window) = self.release_counter.span_at(self.now) else {
                    self.stream_error.set(Some(super::stream::StreamError {
                        sample: play.sample,
                        frame: self.now,
                        kind: super::stream::StreamErrorKind::UnsupportedWindow,
                    }));
                    self.route_spares.push(voice.into_routes());
                    return;
                };
                state.shared_lfos.stage_preview(zone.group, settings, self.now, window,
                    self.rate as f32, self.tempo, state.lfo_timings.group(zone.group));
            }
            let cx = Context::new(bank, state, &self.midi, &self.host_notes, &self.per_note,
                self.voice_controls(state), self.native_mpe_pressure, self.rate as f32,
                self.tempo, &self.stream_error, self.now, false, 0.);
            // First-sample admission only. Future input or larger trajectories
            // retain the ordinary render-time validation boundary.
            match voice.prepare_source(&cx, 1) {
                Ok(prepared) if prepared.has_output() => {},
                result => {
                    if let Err(voice::Start::Failed(error)) = result { self.stream_error.set(Some(error)); }
                    self.route_spares.push(voice.into_routes());
                    return;
                }
            }
            // A failed candidate leaves the live clocks and release cutters
            // untouched. The first successful candidate commits them before
            // pressure/victim selection, in the ordinary admission order.
            if let Some(group_clock) = prelude.take() {
                self.admission_prelude(bank, state, ev, release_trigger);
                self.group_clock = group_clock;
                self.clock += 1;
            }
            let played = &bank.usage[play.sample as usize];
            played.store(played.load(Ordering::Relaxed).wrapping_add(1), Ordering::Relaxed);
            self.make_room(bank, settings.voice_group);
            self.admission_clock = admission;
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
                if live >= rule.max_voices
                    && let Some(i) = victim
                {
                    let fade = self.fade_frames(rule.fade);
                    self.voices[i].fade.start(0.0, fade, true);
                }
            }
        }
        let rule = &bank.polyphony;
        if self.voices.len() >= rule.max_voices {
            let (live, victim) =
                victim(&self.voices, |_| true, rule.kill_mode, rule.prefer_released);
            if live >= rule.max_voices
                && let Some(i) = victim
            {
                let fade = self.fade_frames(rule.fade);
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

    pub(super) fn note_off(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        channel: u8,
        note: u8,
        owner: Option<u8>,
        defaults: Ahdsr,
    ) {
        self.note_off_with_velocity(bank, state, channel, note, owner, None, defaults);
    }

    pub(super) fn note_off_with_velocity(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        channel: u8,
        note: u8,
        owner: Option<u8>,
        release_velocity: Option<Velocity>,
        defaults: Ahdsr,
    ) {
        if channel >= 16 || note >= 128 {
            return;
        }
        let (c, n) = (channel as usize, note as usize);
        let input = if let Some(owner) = owner {
            let input = &mut self.input_keys[owner as usize][n];
            if input.channel == channel {
                let captured = *input;
                input.onset_velocity = None;
                input.per_note = None;
                captured
            } else {
                InputKey::default()
            }
        } else {
            let onset = self
                .input_keys
                .iter()
                .filter(|r| r[n].channel == channel)
                .find_map(|r| r[n].onset_velocity.map(|_| r[n]))
                .unwrap_or_default();
            for row in self.input_keys.iter_mut().filter(|r| r[n].channel == channel) {
                row[n].onset_velocity = None;
                row[n].per_note = None;
            }
            onset
        };
        let remaining = self
            .input_keys
            .iter()
            .filter(|r| r[n].channel == channel)
            .filter_map(|r| r[n].onset_velocity.map(|v| v.midi1().max(1)))
            .max()
            .unwrap_or(0);
        let exact = self
            .host_notes
            .active
            .iter()
            .filter_map(|r| self.host_notes.get(*r))
            .filter(|o| o.held && o.channel == channel && o.key == note)
            .map(|o| o.velocity.midi1().max(1))
            .max()
            .unwrap_or(0);
        // Direct start_event callers may have no physical-input record.
        let velocity = input.onset_velocity.or_else(|| {
            (remaining == 0 && exact == 0 && self.keys[c][n] > 0)
                .then(|| Velocity::from_midi1(self.keys[c][n]))
        });
        self.keys[c][n] = remaining.max(exact);
        if velocity.is_some() && remaining == 0 && exact == 0 {
            self.key_up[c][n] = self.now;
        }
        let allowed = self.allowed;
        let mut found = false;
        // Layered voices share an event. Releasing that event clears held on
        // every layer, so each distinct physical retrigger fires exactly once.
        // ponytail: scan per event; index event voices if dense retrigger releases dominate.
        while let Some(source) = self
            .voices
            .iter()
            .find(|v| v.source_note().matches_key_up(channel, note, owner))
            .map(|v| v.event)
        {
            found |= self.accept_release(bank, state, ReleaseRequest {
                source, release_velocity, capture: Capture::Actual,
                sample: Some(ReleaseSample::FromVoice(&allowed)),
            }, defaults);
        }
        // Release-only instruments can have a key but no attack voice.
        if !found && let Some(velocity) = velocity {
            let source = self.next_id();
            let expression = self.release_expression(source, channel, note, owner);
            self.accept_release(bank, state, ReleaseRequest {
                source, release_velocity, capture: Capture::RootAdditional(input.sostenuto),
                sample: Some(ReleaseSample::Snapshot(ReleaseSnapshot {
                    channel, note, velocity, groups: &allowed, expression, input_channel: owner,
                    host_note: None, per_note: input.per_note, script_mods: None,
                    held_ms: input.onset_velocity.map(|_| self.now.saturating_sub(input.started) as f32 * 1000. / self.rate as f32),
                })),
            }, defaults);
        }
        self.input_key_up(bank, state, channel, note, owner, defaults);
    }

    // ponytail: release-only or exhausted attack events use the key clock; retain
    // event timing outside voices if those libraries need independent retrigger clocks.
    pub(super) fn event_held_ms(&self, source: EventId, channel: u8, note: u8) -> f32 {
        self.voices.iter().find(|v| v.event == source).map_or_else(
            || self.held_ms(channel, note),
            |v| {
                v.counter_stop
                    .unwrap_or(self.now)
                    .saturating_sub(v.counter_start) as f32
                    * 1000.
                    / self.rate as f32
            },
        )
    }

    pub(super) fn release_snapshot(&self, commands: &[script::Command], at: u32, logical: u8, physical: u8, note: u8) -> Expression {
        use crate::midi::{Control, ControlTarget};
        let c = physical as usize & 15;
        let mut x = self.member_snapshot(logical, physical, note);
        let mut pressure_active = self.midi.pressure_active[c];
        for command in commands.iter().filter(|command| !command.applied && command.at <= at) {
            if let script::Kind::Control { target, control, .. } = command.kind {
                let applies = match target {
                    ControlTarget::Authored => true,
                    ControlTarget::Physical(input) => input == physical,
                };
                if !applies { continue; }
                match control {
                    Control::Cc(74, value) => { x.member_cc74 = Some(value); x.member_cc74_received = Some(true); },
                    Control::Pressure(value) => { x.member_pressure = Some(value); pressure_active = true; },
                    Control::ResetPressure => { x.member_pressure = Some(crate::midi::ControlValue::ZERO); pressure_active = false; },
                    Control::Bend(value) => x.member_bend = Some(value),
                    _ => {},
                }
            }
        }
        if let Some(zone) = self.midi.mpe.member(physical) {
            x.tune += x.member_bend.unwrap_or(self.midi.bend[c]).dsp() * self.midi.channel_bend_range[c].map(|range| range.semitones())
                .unwrap_or_else(|| f64::from(zone.member_range.semitones())) as f32;
            if self.native_mpe_pressure && pressure_active {
                x.gain *= 10f32.powf((x.member_pressure.unwrap_or(self.midi.pressure[c]).dsp() - 1.) * 12. / 20.);
            }
        }
        x
    }

    fn member_snapshot(&self, logical: u8, physical: u8, note: u8) -> Expression {
        let c = physical as usize & 15;
        Expression {
            member_cc74: Some(self.midi.cc[c][74]),
            member_cc74_received: Some(params::controller_received(&self.midi.cc_received[c], 74)),
            member_pressure: Some(self.midi.pressure[c]),
            member_bend: Some(self.midi.bend[c]),
            ..self.midi.expression[logical as usize & 15][note as usize & 127]
        }
    }

    /// The originating voice may have been frozen at an earlier physical
    /// key-up while the script delayed its release callback.
    pub(super) fn release_expression(
        &self,
        source: EventId,
        channel: u8,
        note: u8,
        input_channel: Option<u8>,
    ) -> Option<Expression> {
        let voice = self.voices.iter().find(|v| v.event == source);
        let key = voice.and_then(|v| v.owner).map_or(note, |(_, key)| key);
        let physical = voice.and_then(|v| v.input_channel).or(input_channel).unwrap_or(channel);
        let logical = voice.map_or(channel, |v| v.channel);
        voice.and_then(|v| v.frozen_expression).or_else(|| {
            self.midi.mpe.member(physical).map(|_| self.release_snapshot(&[], 0, logical, physical, key))
        })
    }

    pub(super) fn reset_mpe_channels(&mut self, channels: u16) {
        self.pedals.clear(channels);
        self.midi.reset_mpe_channels(channels);
    }

    /// Legacy internal entry points still identify one physical row.
    pub(super) fn cc(&mut self, bank: Option<&Bank>, state: &InstrumentState,
        channel: u8, cc: u8, value: u8, defaults: Ahdsr) {
        if let Some(control) = crate::midi::Control::from_legacy(cc, i32::from(value)) {
            self.control(bank, state, channel, crate::midi::ControlTarget::Physical(channel), control, defaults);
        }
    }

    pub(super) fn control(&mut self, bank: Option<&Bank>, state: &InstrumentState,
        logical: u8, target: crate::midi::ControlTarget, control: crate::midi::Control, defaults: Ahdsr) {
        self.control_with_standard_provenance(bank, state, logical, target, control, defaults, false, false);
    }

    pub(super) fn native_instrument_control(&mut self, param: super::params::GroupPar, value: crate::midi::ControlValue) {
        match param {
            super::params::GroupPar::Volume => self.volume.physical = Some(value),
            super::params::GroupPar::Pan => self.pan.physical = Some(value),
            _ => return,
        }
        self.touch();
    }

    pub(super) fn sostenuto_channels(&self) -> u16 {
        self.pedals.sostenuto_channels()
    }

    pub(super) fn control_channels(&self, target: crate::midi::ControlTarget, control: crate::midi::Control) -> u16 {
        Self::control_channels_for(self.midi.mpe, target, control)
    }

    pub(super) fn control_channels_for(mpe: crate::midi::MpeZones, target: crate::midi::ControlTarget, control: crate::midi::Control) -> u16 {
        use crate::midi::{Control, ControlTarget};
        match target {
            ControlTarget::Authored => u16::MAX,
            ControlTarget::Physical(channel) if channel >= 16 => 0,
            ControlTarget::Physical(channel) => (1 << channel) | match control {
                Control::Cc(cc, _) if !matches!(cc, 6 | 38 | 74 | 96..=101) =>
                    mpe.manager(channel).map_or(0, |zone| zone.member_mask()),
                Control::Cc14 { coarse, .. } if coarse != 6 =>
                    mpe.manager(channel).map_or(0, |zone| zone.member_mask()),
                _ => 0,
            },
        }
    }

    pub(super) fn control_with_standard_provenance(&mut self, bank: Option<&Bank>, state: &InstrumentState,
        logical: u8, target: crate::midi::ControlTarget, control: crate::midi::Control, defaults: Ahdsr,
        native_standard_handled: bool, root_pedal_handled: bool) {
        use crate::midi::{Control, ControlTarget};
        if logical >= 16 || matches!(target, ControlTarget::Physical(channel) if channel >= 16)
            || matches!(control, Control::Cc(cc, _) if cc >= 128) { return; }
        let channels = self.control_channels(target, control);
        let mut accepted = false;
        for channel in (0..16u8).filter(|channel| channels & (1 << channel) != 0) {
            accepted |= self.midi.accept(ControlTarget::Physical(channel), control);
        }
        if !accepted { return; }
        let native = match control {
            Control::Cc(cc, _) if matches!(target, ControlTarget::Physical(_)) && !state.native_note_action(cc) => None,
            Control::Cc(cc, value) => Some((cc, value)),
            Control::Cc14 { coarse, value, .. } if matches!(coarse, 7 | 10) => Some((coarse, value)),
            _ => None,
        };
        if let Some((cc, value)) = native {
            // Record physical controls even while this instrument ignores their
            // native action. Authored controls retain the chronological baseline.
            match cc {
                7 if !native_standard_handled => self.volume.receive(value, target, value.dsp().powi(2)),
                10 if !native_standard_handled => self.pan.receive(value, target, value.pan()),
                7 | 10 => {},
                _ => {
                for channel in (0..16u8).filter(|channel| channels & (1 << channel) != 0) {
                    self.native_cc(bank, state, channel, cc, value, defaults, root_pedal_handled);
                }
                }
            }
        }
    }

    fn native_cc(
        &mut self,
        bank: Option<&Bank>,
        state: &InstrumentState,
        channel: u8,
        cc: u8,
        value: crate::midi::ControlValue,
        defaults: Ahdsr,
        root_pedal_handled: bool,
    ) {
        let c = channel as usize;
        let on = value.normalized() >= 0.5;
        match cc {
            64 if self.native_sustain && state.uses_sustain() => {
                if self.pedals.set_sustain(channel, on) {
                    if let Some(bank) = bank {
                        self.pedal_up(bank, state, channel, defaults);
                    }
                }
            }
            // Sostenuto latches the keys down now; its release lets go of
            // those no longer held by key or sustain.
            66 => {
                if self.pedals.set_sostenuto(1 << channel, on).changed == 0 {
                    return;
                }
                for input in &mut self.input_keys[c] {
                    if !on { input.sostenuto = false; }
                    else if input.onset_velocity.is_some() { input.sostenuto = true; }
                }
                if !root_pedal_handled {
                    for i in 0..self.host_notes.active.len() {
                        let id = self.host_notes.active[i];
                        if let Some(o) = self.host_notes.get_mut(id).filter(|o| o.note.channel == channel) {
                            if !on {
                                o.sostenuto = false;
                            } else if o.held {
                                o.sostenuto = true;
                            }
                        }
                    }
                }
                if on {
                    // Scripts may queue these commands while input keys already
                    // reflect later note-offs. Capture the voices sounding at
                    // this command's time, only on the pedal's down edge.
                    for v in &mut self.voices {
                        v.source_sostenuto(channel, true);
                    }
                } else {
                    for v in &mut self.voices {
                        v.source_sostenuto(channel, false);
                    }
                    for r in self
                        .pending_releases
                        .iter_mut()
                        .filter(|r| r.input_channel.unwrap_or(r.channel) == channel)
                    {
                        r.captured = false;
                    }
                    if let Some(bank) = bank {
                        self.pedal_up(bank, state, channel, defaults);
                    }
                }
            }
            // All sound off: a click-free cut, well short of any release.
            120 => {
                let fade = self.fade_frames(STEAL_FADE);
                for v in self.voices.iter_mut().filter(|v| v.input_channel.unwrap_or(v.channel) == channel) {
                    v.fade.start(0.0, fade, true);
                    // The stop fade may outlive a physical key-up. This voice
                    // no longer owns a held key and must not fire a new release.
                    v.held = false;
                }
                self.pending_releases.retain(|r| r.input_channel.unwrap_or(r.channel) != channel);
                self.host_notes.stop(None, Some(1 << channel), self.now);
                self.input_keys[c].fill(InputKey::default());
                for logical in 0..16 {
                    for note in 0..128 {
                        let held = self.input_keys.iter().any(|row| row[note].channel == logical as u8 && row[note].onset_velocity.is_some())
                            || self.host_notes.active.iter().filter_map(|id| self.host_notes.get(*id)).any(|owner| owner.held && owner.channel == logical as u8 && owner.key == note as u8);
                        if !held { self.keys[logical][note] = 0; self.key_up[logical][note] = self.now; }
                    }
                }
            }
            121 => {
                self.native_cc(bank, state, channel, 64, crate::midi::ControlValue::ZERO, defaults, root_pedal_handled);
                self.native_cc(bank, state, channel, 66, crate::midi::ControlValue::ZERO, defaults, root_pedal_handled);
                self.midi.reset_controllers(channel);
            }
            // Mode switching is not implemented; all modes still release
            // MIDI keys through the same pedal-respecting note-off path.
            123..=127 => {
                if let Some(bank) = bank {
                    for note in 0..128 {
                        let input = self.input_keys[c][note];
                        if input.onset_velocity.is_some() { self.note_off(bank, state, input.channel, note as u8, Some(channel), defaults); }
                    }
                }
            }
            _ => {}
        }
    }

    /// A pedal let go: release voices whose own key and pedal holds are gone.
    pub(super) fn pedal_up(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        channel: u8,
        defaults: Ahdsr,
    ) {
        if self.pedals.holds(channel, false, false) {
            return;
        }
        for v in &mut self.voices {
            if v.source_pedal_up(channel, &self.pedals)
                && super::ahdsr_shared::release_admitted(state.shared_ahdsr.as_deref(),v,self.now,&self.stream_error) {
                v.release(bank, &state.settings, &mut self.free);
            }
        }
        // Compact survivors in place, preserving release order without a
        // temporary allocation or quadratic shifts for a full pedal history.
        let mut keep = 0;
        for i in 0..self.pending_releases.len() {
            let r = self.pending_releases[i];
            if r.input_channel.unwrap_or(r.channel) == channel && !r.captured {
                let id = self.next_id();
                let event = NoteEvent {
                    script_mods: &r.script_mods,
                    host_note: r.host_note,
                    per_note: r.per_note,
                    velocity: if r.host_note.is_some() {
                        r.onset_velocity.midi1()
                    } else {
                        r.onset_velocity.midi1().max(1)
                    },
                    input_channel: r.input_channel,
                    groups: Some(&r.groups),
                    native_release: true,
                    frozen_expression: r.expression,
                    release_held_ms: Some(r.held_ms),
                    release_velocity: r.release_velocity,
                    ..NoteEvent::new_velocity(r.channel, r.note, r.onset_velocity)
                };
                self.start(bank, state, &event, id, true, defaults);
            } else {
                self.pending_releases[keep] = r;
                keep += 1;
            }
        }
        self.pending_releases.truncate(keep);
    }

    /// Render every voice into `left`/`right`, or into its group's bus input
    /// at frame `offset` of the current block. `steady`: no bus fader moves
    /// before the block ends.
    pub(super) fn render(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        (left, right): (&mut [f32], &mut [f32]),
        fx: &mut FxProcessor,
        offset: usize,
        blocking: bool,
        tune: f32,
        steady: bool,
    ) -> bool {
        if !bank.has_native_controls() {
            return self.render_block(
                bank,
                state,
                (left, right),
                fx,
                offset,
                blocking,
                tune,
                steady,
            );
        }
        // Native loop-control buffers address audio_frame / 32. Split source
        // planning only; the instrument's effects keep their original block.
        let mut at = 0;
        let mut fed = false;
        while at < left.len() {
            let n = (32 - (self.now as usize & 31)).min(left.len() - at);
            fed |= self.render_block(
                bank,
                state,
                (&mut left[at..at + n], &mut right[at..at + n]),
                fx,
                offset + at,
                blocking,
                tune,
                steady,
            );
            at += n;
        }
        fed
    }

    #[allow(clippy::too_many_arguments)]
    fn render_block(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        (left, right): (&mut [f32], &mut [f32]),
        fx: &mut FxProcessor,
        offset: usize,
        blocking: bool,
        tune: f32,
        steady: bool,
    ) -> bool {
        let shared_sources = !state.shared_lfos.is_empty();
        if shared_sources && !left.is_empty() && let Some(window) = self.release_counter.span() {
            for voice in &self.voices {
                state.shared_lfos.stage(voice.group as usize, &state.settings[voice.group as usize],
                    self.now, window, self.rate as f32, self.tempo,
                    state.lfo_timings.group(voice.group as usize));
            }
        }
        let cx = Context::new(bank, state, &self.midi, &self.host_notes, &self.per_note,
            self.voice_controls(state), self.native_mpe_pressure, self.rate as f32,
            self.tempo, &self.stream_error, self.now, blocking, tune);
        let n = left.len();
        // Plan every voice, then take them lane by lane: voices sharing one
        // sum into its window, the rest (and lanes of one) render alone.
        self.lanes.clear();
        self.dead.clear();
        let mut fed = false;
        let mut successful = [0_u64; super::MAX_VOICES / 64];
        for (i, voice) in self.voices.iter_mut().enumerate() {
            let bus = state.settings[voice.group as usize].bus;
            let through = bus.filter(|_| steady).and_then(|b| fx.bus_gains(b));
            let lane = match voice.plan(&cx, n, bus, through) {
                Ok(lane) => lane,
                Err(start) => {
                    match start {
                        voice::Start::Pending => {},
                        voice::Start::Canceled => self.dead.push(i as u16),
                        voice::Start::Failed(error) => {
                            // No source or musical clock was committed by this plan.
                            voice.plan.terminal = Some(0);
                            if self.stream_error.get().is_none() { self.stream_error.set(Some(error)); }
                            self.dead.push(i as u16);
                        }
                    }
                    self.lanes.skip(i as u16);
                    continue;
                }
            };
            if shared_sources { successful[i / 64] |= 1 << (i % 64); }
            match lane.filter(|_| self.shared) {
                Some(lane) => self
                    .lanes
                    .add(lane, voice.plan.class, &voice.filter.held, i as u16),
                None => self.lanes.skip(i as u16),
            }
        }
        // Planning failures/cancellations are ordered by voice index. Later
        // render deaths append to this list without changing the saved prefix.
        let planned_dead = self.dead.len();
        if shared_sources {
            for (i, voice) in self.voices.iter().enumerate() {
                if n != 0 && voice.onset == voice::Onset::Started
                    && successful[i / 64] & (1 << (i % 64)) != 0 {
                    state.shared_lfos.nominate(voice.group as usize, voice.admission);
                }
            }
            state.shared_lfos.commit();
        }
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if self.dead[..planned_dead].binary_search(&(i as u16)).is_ok()
                || self.lanes.shared(i as u16) || voice.onset != voice::Onset::Started {
                continue;
            }
            let bus = state.settings[voice.group as usize].bus;
            let program_input = fx.feeds_program(bus);
            let (input, mut sends) = fx.voice_inputs(bus, offset..offset + n);
            let (l, r) = input.unwrap_or((left, right));
            let voice::Rendered { alive, underrun, work } =
                voice.render(&cx, &mut self.scratch, l, r, false, Some(&mut sends));
            fed |= work && program_input;
            self.underruns += u64::from(underrun);
            if !alive {
                self.dead.push(i as u16);
            }
        }
        for &(lane, _, first, _, count) in self.lanes.lanes.iter().filter(|l| l.1 == 0 && l.4 > 1) {
            let Scratch {
                window, acc, amp, ..
            } = &mut self.scratch;
            let acc = &mut acc[..lane.count(n)];
            acc.fill([0.0; 2]);
            let program_input = fx.feeds_program(lane.bus());
            for i in self.lanes.members(first, count) {
                let voice::Rendered { alive, underrun, work } = self.voices[i as usize].accumulate(&cx, window, acc, None);
                fed |= work && program_input;
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
        let filtered = self
            .lanes
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.1 != 0);
        self.order.extend(filtered.map(|(i, l)| (l.1, i as u16)));
        self.order.sort_unstable();
        let mut at = 0;
        while let Some(&(class, head)) = self.order.get(at) {
            let key = self.lanes.keys[head as usize];
            let bus = self.lanes.lanes[head as usize].0.bus();
            let program_input = fx.feeds_program(bus);
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
                    let voice::Rendered { alive, underrun, work } = match lane.is_solo() {
                        true => voice.render(&cx, &mut self.scratch, sl, sr, true, None),
                        false => {
                            let Scratch {
                                window, acc, amp, ..
                            } = &mut self.scratch;
                            let acc = &mut acc[..lane.count(n)];
                            acc.fill([0.0; 2]);
                            let rendered = voice.accumulate(&cx, window, acc, None);
                            lane.mix(acc, amp, sl, sr);
                            rendered
                        }
                    };
                    lf.dots_out(sl, sr, [&mut *ol, &mut *or]);
                    lf.end(&mut voice.filter, [1.0; 2]);
                    fed |= work && program_input;
                    self.underruns += u64::from(underrun);
                    if !alive {
                        self.dead.push(first);
                    }
                    continue;
                }
                let Scratch {
                    window, acc, amp, ..
                } = &mut self.scratch;
                let acc = &mut acc[..lane.count(n)];
                acc.fill([0.0; 2]);
                lane.curve(&mut amp[..n]);
                lf.kernel(&amp[..n], lane.base(), lane.step(), acc.len(), lane.native());
                for i in self.lanes.members(first, count) {
                    let voice::Rendered { alive, underrun, work } =
                        self.voices[i as usize].accumulate(&cx, window, acc, Some(&mut *lf));
                    fed |= work && program_input;
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
            self.host_notes.record_end(v.host_note,
                self.now.saturating_add(v.terminal_offset().unwrap_or(n) as u64));
            if let Some(stream) = v.stream {
                bank.slots()[stream.slot as usize].stop();
                self.free.push(stream.slot);
            }
            self.route_spares.push(v.into_routes());
        }
        self.now += n as u64;
        fed
    }

    /// Effective instrument parameters retain getter/physical-override authority.
    pub(super) fn instrument_controls(&self, state: &InstrumentState) -> super::instrument::InstrumentControls {
        let mut controls = state.controls;
        if let Some(value) = self.physical_instrument_control(state, super::params::GroupPar::Volume) {
            controls.gain = super::params::native_cc7(value, state.controller_options.cc7_max_db);
        }
        if let Some(value) = self.physical_instrument_control(state, super::params::GroupPar::Pan) {
            controls.pan = super::params::native_cc10(value);
        }
        controls
    }

    fn voice_controls(&self, state: &InstrumentState) -> super::instrument::InstrumentControls {
        let controls = self.instrument_controls(state);
        if state.controller_policy == super::instrument::ControllerPolicy::Native {
            // The program amplifier owns Native gain/pan after effects. Voice
            // and direct-output amplifiers retain only their own parameters.
            super::instrument::InstrumentControls { gain: 1.0, pan: 0.0, ..controls }
        } else { controls }
    }

    pub(super) fn physical_instrument_control(&self, state: &InstrumentState, param: super::params::GroupPar) -> Option<crate::midi::ControlValue> {
        (state.controller_policy == super::instrument::ControllerPolicy::Native
            && state.controller_options.standard_volume_pan).then(|| match param {
                super::params::GroupPar::Volume => self.volume.physical,
                super::params::GroupPar::Pan => self.pan.physical,
                _ => None,
            }).flatten()
    }

    pub(super) fn authored_instrument_control(&mut self, param: super::params::GroupPar) {
        match param {
            super::params::GroupPar::Volume => self.volume.physical = None,
            super::params::GroupPar::Pan => self.pan.physical = None,
            _ => {},
        }
    }

    pub(super) fn output(&mut self, state: &InstrumentState, left: &mut [f32], right: &mut [f32], cutoff: f32) {
        if left.is_empty() || right.is_empty() {
            return;
        }
        let gain = self.volume.output(state);
        let pan = if state.controller_policy == super::instrument::ControllerPolicy::Compatibility
            && state.controller_options.standard_volume_pan {
            self.pan.physical.map_or(self.pan.authored, |value| value.pan())
        } else { self.pan.authored };
        let target = if gain.is_finite() && pan.is_finite() {
            balance(gain, pan.clamp(-1.0, 1.0))
        } else {
            self.out_target
        };
        // Invalid control values do not poison the retained ramp or its output.
        if target != self.out_target {
            self.out_start = self.out_gains;
            self.out_target = target;
            self.out_frames = (self.rate * 0.005).round().max(1.) as u32;
            self.out_remaining = self.out_frames;
        }
        if self.out_gains != [1.0; 2] || self.out_target != [1.0; 2] {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                *l *= self.out_gains[0];
                *r *= self.out_gains[1];
                if self.out_remaining > 0 {
                    self.out_remaining -= 1;
                    let at = (self.out_frames - self.out_remaining) as f32 / self.out_frames as f32;
                    self.out_gains = if self.out_remaining == 0 {
                        self.out_target
                    } else {
                        std::array::from_fn(|c| {
                            self.out_start[c] + (self.out_target[c] - self.out_start[c]) * at
                        })
                    };
                }
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

#[cfg(test)]
mod output_smoothing_tests {
    use super::Player;
    use crate::{engine::instrument::InstrumentState, midi::ControlTarget};

    #[test]
    fn bank_policy_selects_authored_and_retained_physical_output_without_heap() {
        use crate::{audio::Sample, engine::{Ahdsr, Bank, Engine}, import::{Group, Zone}, ksp::{LogEngine, Runtime}};

        fn bank(native: bool) -> Box<Bank> {
            let path = std::path::PathBuf::from("policy-output");
            let mut bank = Bank::from_samples(vec![Group::default()],
                vec![Zone { sample: path.clone(), ..Zone::default() }],
                vec![(path, Sample { rate: 48_000, frames: vec![[0.5; 2]; 8192] })]).unwrap()
                .with_prepared_settings(|rows| rows[0].envelope = Some(Ahdsr::UNITY)).unwrap();
            bank.initial_state.as_mut().unwrap().controller_options.standard_volume_pan = native;
            Box::new(bank)
        }

        fn settle(engine: &mut Engine, gain: u8, pan: u8, fragments: &[usize]) {
            let (mut left, mut right) = ([0.; 768], [0.; 768]);
            let mut at = 0;
            let mut block = 0;
            while at < left.len() {
                let end = (at + fragments[block % fragments.len()]).min(left.len());
                engine.render(&mut left[at..end], &mut right[at..end]);
                at = end;
                block += 1;
            }
            let gain = (f32::from(gain) / 127.).powi(2) * 0.5;
            let pan = ((f32::from(pan) - 64.) / 63.).clamp(-1., 1.);
            let expected = [gain * (1. - pan.max(0.)), gain * (1. + pan.min(0.))];
            for frame in 512..768 {
                assert!((left[frame] - expected[0]).abs() < 2e-6, "left frame {frame}: {} vs {}", left[frame], expected[0]);
                assert!((right[frame] - expected[1]).abs() < 2e-6, "right frame {frame}: {} vs {}", right[frame], expected[1]);
            }
        }

        for fragments in [&[128][..], &[1, 7, 31, 128][..]] {
            let (enabled, disabled, reenabled) = (bank(true), bank(false), bank(true));
            let source = "on init\ndeclare ui_knob $volume(0,127,1)\ndeclare ui_knob $pan(0,127,1)\nend on\non ui_control($volume)\nset_controller(7,$volume)\nend on\non ui_control($pan)\nset_controller(10,$pan)\nend on";
            let (runtime, errors) = Runtime::with_scripts(&[source], &mut LogEngine::default(), 0, Vec::new());
            assert!(errors.iter().all(Option::is_none), "{errors:?}");
            let mut engine = Engine::default();
            engine.reset(48_000.);
            engine.set_bank(Some(enabled));
            engine.set_script(Some(Box::new(runtime)));
            let mut retired = Vec::with_capacity(2);
            assert_eq!(crate::plugin::tests::allocations(|| {
                engine.ui_control(0, 0, 80);
                engine.ui_control(0, 1, 32);
                engine.note_on_from(0, 4, 60, 127);
                settle(&mut engine, 80, 32, fragments);
                engine.cc_from(0, 4, 7, 40);
                engine.cc_from(0, 4, 10, 127);
                settle(&mut engine, 40, 127, fragments);

                let reached = engine.player.out_gains;
                retired.push(engine.upgrade_bank(disabled).unwrap());
                assert_eq!(engine.active_voices(), 0, "changed native policy replaces the old voice lifetime");
                assert!(!engine.key_down(0, 60));
                assert_eq!(engine.player.out_gains, reached, "policy changes retain the smoothing clock");
                engine.note_on_from(0, 4, 60, 127);
                settle(&mut engine, 80, 32, fragments);
                engine.cc_from(0, 4, 7, 100);
                engine.cc_from(0, 4, 10, 0);
                settle(&mut engine, 80, 32, fragments);
                assert_eq!((engine.cc_state()[4][7], engine.cc_state()[4][10]), (100, 0));

                engine.ui_control(0, 0, 90);
                engine.ui_control(0, 1, 96);
                settle(&mut engine, 90, 96, fragments);
                engine.cc_from(0, 4, 7, 60);
                engine.cc_from(0, 4, 10, 0);
                settle(&mut engine, 90, 96, fragments);
                let reached = engine.player.out_gains;
                retired.push(engine.upgrade_bank(reenabled).unwrap());
                assert_eq!(engine.active_voices(), 0);
                assert!(!engine.key_down(0, 60));
                assert_eq!(engine.player.out_gains, reached);
                engine.note_on_from(0, 4, 60, 127);
                settle(&mut engine, 60, 0, fragments);
                engine.ui_control(0, 0, 70);
                settle(&mut engine, 70, 0, fragments);
                engine.ui_control(0, 1, 64);
                settle(&mut engine, 70, 64, fragments);
            }), 0);
            assert_eq!(engine.dropped_commands(), 0);
        }
    }

    #[test]
    fn native_cc7_uses_verified_instrument_gain_and_retains_continuous_precision_without_heap() {
        use crate::{audio::Sample, engine::{Ahdsr, Bank, Engine, params::{Address, GroupPar},
            instrument::ControllerPolicy}, import::{Group, Zone}, midi::{Control, ControlValue}};

        fn bank(policy: ControllerPolicy, maximum_db: i32) -> Box<Bank> {
            let path = std::path::PathBuf::from("native-cc7-scalar");
            let mut bank = Bank::from_samples(vec![Group::default()],
                vec![Zone { sample: path.clone(), ..Zone::default() }],
                vec![(path, Sample { rate: 48_000, frames: vec![[0.5; 2]; 32_768] })]).unwrap()
                .with_prepared_settings(|rows| rows[0].envelope = Some(Ahdsr::UNITY)).unwrap();
            let state = bank.initial_state.as_mut().unwrap();
            state.controller_policy = policy;
            state.controller_options.cc7_max_db = maximum_db;
            state.controls.gain = 0.25;
            Box::new(bank)
        }

        fn level(engine: &mut Engine, expected: f32, fragments: &[usize], retained: &mut Option<f32>) {
            let (mut left, mut right) = ([0.; 768], [0.; 768]);
            let native=engine.state.controller_policy==ControllerPolicy::Native;
            let (mut at, mut block) = (0, 0);
            let mut window_moves=false;
            while at < left.len() {
                let boundary=((at/256)+1)*256;
                if at%256==0 {engine.begin_audio_block(256,1,false);}
                let end = (at + fragments[block % fragments.len()]).min(boundary).min(left.len());
                // Decision belongs to the balanced host window, not each render
                // fragment. Constant source0.5 lets this test retain output units.
                let current=retained.get_or_insert(expected);
                let quotient=(*current+0.5e-12).abs()/(expected+0.5e-12).abs();
                if at%256==0 {window_moves=quotient>1.001 || quotient<0.999;}
                let mut oracle=[0.;256];
                for value in &mut oracle[..end-at] {
                    *value=if native && window_moves {*current} else {expected};
                    if native && window_moves {*current+=(expected-*current)*(1f32/1800.);}
                }
                engine.render(&mut left[at..end], &mut right[at..end]);
                if native {
                    for (i,&value) in oracle[..end-at].iter().enumerate() {
                        assert!((left[at+i]-value).abs()<2e-6 && (right[at+i]-value).abs()<2e-6,
                            "native amplitude frame{} expected {value}, observed {}",at+i,left[at+i]);
                    }
                }
                at = end;
                block += 1;
            }
            if !native {
                for channel in [&left,&right] {assert!(channel[512..].iter().all(|v|(*v-expected).abs()<2e-6));}
            }
        }

        // Independent float converter outputs recovered from the native
        // integer multipliers, with a 0.5 source and no authored output gain.
        let maxima = [(-12, 0.12358862), (-6, 0.24999848), (0, 0.49869505),
            (6, 0.9968301), (12, 1.9999878)];
        for fragments in [&[128][..], &[1, 7, 31, 127, 13, 64][..]] {
            for (maximum_db, expected) in maxima {
                let mut retained=None;
            let mut engine = Engine::default();
                engine.reset(48_000.);
                engine.set_bank(Some(bank(ControllerPolicy::Native, maximum_db)));
                assert_eq!(crate::plugin::tests::allocations(|| {
                    engine.note_on_from(0, 4, 60, 127);
                    level(&mut engine, 0.125, fragments, &mut retained);
                    engine.cc_from(0, 4, 7, 127);
                    level(&mut engine, expected, fragments, &mut retained);
                    assert!(engine.write(Address::Instrument(GroupPar::Volume), 0.2));
                    level(&mut engine, 0.1, fragments, &mut retained);
                    engine.cc_from(0, 4, 7, 127);
                    level(&mut engine, expected, fragments, &mut retained);
                    engine.state.controller_options.standard_volume_pan = false;
                    level(&mut engine, 0.1, fragments, &mut retained);
                    engine.state.controller_options.standard_volume_pan = true;
                    level(&mut engine, expected, fragments, &mut retained);
                }), 0);
            }

            let mut retained=None;
            let mut engine = Engine::default();
            engine.reset(48_000.);
            engine.set_bank(Some(bank(ControllerPolicy::Native, 0)));
            let (a, b) = (ControlValue::new(0.5001).unwrap(), ControlValue::new(0.5030).unwrap());
            assert_eq!(a.midi1(), b.midi1());
            let generic = bank(ControllerPolicy::Compatibility, 0);
            let mut retired = None;
            assert_eq!(crate::plugin::tests::allocations(|| {
                engine.note_on_from(0, 4, 60, 127);
                engine.control_from(0, 4, Control::Cc(7, a));
                level(&mut engine, 0.062374286, fragments, &mut retained);
                engine.control_from(0, 4, Control::Cc(7, b));
                level(&mut engine, 0.06346569, fragments, &mut retained);
                retired = engine.upgrade_bank(generic);
                assert!(retired.is_some(), "same option fields with different provenance replace the lifetime");
                assert_eq!(engine.active_voices(), 0);
                engine.note_on_from(0, 4, 60, 127);
                level(&mut engine, 0.125 * b.dsp().powi(2), fragments, &mut retained);
            }), 0);
            drop(retired);
        }
    }

    #[test]
    fn output_controls_follow_five_milliseconds_and_retarget_without_heap() {
        for rate in [48_000., 96_000.] {
            let mut player = Player::new(rate);
            let state = InstrumentState::default();
            let frames = (rate * 0.005) as usize;
            let (mut left, mut right) = ([1.; 481], [1.; 481]);
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    player.output(&state, &mut left[..1], &mut right[..1], 20_000.);
                    assert_eq!((left[0], right[0]), (1., 1.), "steady unity is transparent");
                    player.volume.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, 0.);
                    player.output(&state, &mut [], &mut [], 20_000.);
                    assert_eq!(
                        player.out_gains, [1.; 2],
                        "empty output cannot advance or start a ramp"
                    );
                    player.output(&state, &mut left[..frames + 1], &mut right[..frames + 1], 20_000.);
                    assert_eq!(left[0], 1.);
                    assert_eq!(left[frames / 4], 0.75);
                    assert_eq!(left[frames / 2], 0.5);
                    assert_eq!(left[frames], 0., "silence is exact after five milliseconds");
                    assert_eq!(player.out_gains, [0.; 2]);

                    player.volume.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, 1.);
                    left.fill(1.);
                    right.fill(1.);
                    player.output(&state, &mut left[..frames / 4], &mut right[..frames / 4], 20_000.);
                    assert_eq!(player.out_gains, [0.25; 2]);
                    player.pan.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, 1.);
                    left.fill(1.);
                    right.fill(1.);
                    player.output(&state, &mut left[..frames + 1], &mut right[..frames + 1], 20_000.);
                    assert_eq!(
                        (left[0], right[0]),
                        (0.25, 0.25),
                        "retarget starts at the reached gain"
                    );
                    assert_eq!((left[frames / 2], right[frames / 2]), (0.125, 0.625));
                    assert_eq!((left[frames], right[frames]), (0., 1.));
                    assert_eq!(player.out_gains, [0., 1.]);
                }),
                0
            );
        }
    }

    #[test]
    fn output_controls_are_partition_invariant_and_ignore_nonfinite_targets() {
        let (mut whole, mut split) = (Player::new(48_000.), Player::new(48_000.));
        let state = InstrumentState::default();
        let (mut left, mut right, mut split_left, mut split_right) =
            ([1.; 640], [1.; 640], [1.; 640], [1.; 640]);
        // Independent known sample times: retarget halfway through a ramp,
        // then automate authored output pan, then restore unity after it settles.
        // Authored receive uses the supplied output scalar; its canonical wire
        // argument is unused, including deliberate nonfinite scalar injection.
        let events = [
            (0, 0.25, 0.),
            (73, 0.75, 0.5),
            (211, 0.5, -1.),
            (481, 1., 0.),
        ];
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for (index, &(start, gain, pan)) in events.iter().enumerate() {
                    let end = events.get(index + 1).map_or(640, |e| e.0);
                    whole.volume.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, gain);
                    whole.pan.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, pan);
                    split.volume.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, gain);
                    split.pan.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, pan);
                    whole.output(&state, &mut left[start..end], &mut right[start..end], 20_000.);
                    let mut at = start;
                    while at < end {
                        let next = (at + [1, 7, 19, 32, 61][at % 5]).min(end);
                        split.output(&state, &mut [], &mut [], 20_000.);
                        split.output(&state,
                            &mut split_left[at..next],
                            &mut split_right[at..next],
                            20_000.,
                        );
                        at = next;
                    }
                    assert_eq!(whole.out_gains, split.out_gains);
                    assert_eq!(whole.out_remaining, split.out_remaining);
                }
                assert_eq!(left, split_left);
                assert_eq!(right, split_right);
                // Invalid controls during an unfinished ramp preserve its target
                // and progression instead of contaminating the subsequent audio.
                for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                    whole.volume.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, invalid);
                    whole.pan.receive(crate::midi::ControlValue::ZERO, ControlTarget::Authored, invalid);
                    let (mut a, mut b, mut c, mut d) = ([1.; 8], [1.; 8], [1.; 8], [1.; 8]);
                    whole.output(&state, &mut a, &mut b, 20_000.);
                    split.output(&state, &mut c, &mut d, 20_000.);
                    assert_eq!(a, c);
                    assert_eq!(b, d);
                    assert!(a.iter().chain(&b).all(|x| x.is_finite()));
                    assert_eq!(whole.out_gains, split.out_gains);
                    assert_eq!(whole.out_target, split.out_target);
                }
            }),
            0
        );
    }
}

#[cfg(test)]
mod velocity_tests {
    use super::*;
    use crate::{
        audio::Sample,
        engine::Engine,
        import::{Group, Zone},
    };

    #[test]
    fn bank_replacement_forgets_input_roots_but_keeps_controls_and_unrelated_ui_without_heap() {
        use super::super::{HostNote, HostPattern, InputAddress};
        let prepare = |replacement| Box::new(Bank::from_samples(
            vec![Group { release_trigger: replacement, ..Group::default() }, Group::default()],
            vec![Zone { low_key: 60, high_key: 61, ..Zone::default() },
                Zone { group: 1, low_key: 62, high_key: 64, ..Zone::default() }],
            vec![(Default::default(), Sample { rate: 48000, frames: vec![[0.75; 2]; 8192] })],
        ).unwrap());
        let source = "on init\ndeclare ui_switch $go\ndeclare ui_slider $done(0,100)\nend on\non note\nif ($EVENT_NOTE = 60)\nplay_note(62,127,0,-1)\nwait(10000)\nplay_note(63,127,0,0)\nend if\nend on\non release\nif ($EVENT_NOTE = 60)\nwait(20000)\nplay_note(64,127,0,0)\nend if\nend on\non ui_control($go)\nwait(12000)\ninc($done)\nend on";
        for scripted in [false, true] {
            for reset in [false, true] {
                for fragmented in [false, true] {
                    let mut engine = Engine::default(); engine.attack = 0.; engine.release = 0.001;
                    engine.set_bank(Some(prepare(false)));
                    if scripted {
                        let (rt, errors) = crate::ksp::Runtime::with_scripts(&[source], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
                        assert!(errors.iter().all(Option::is_none), "{errors:?}");
                        engine.set_script(Some(Box::new(rt)));
                    }
                    let (address, other) = (InputAddress { port: 0, channel: 2, key: 60 }, InputAddress { port: 0, channel: 5, key: 61 });
                    let exact = HostNote { port: 0, channel: 7, key: 61, id: 900, clap: true };
                    let pattern = HostPattern { port: 0, channel: 7, key: 61, id: 900, clap: true };
                    let (mut replacement, mut retired) = (Some(prepare(true)), None);
                    let (mut left, mut right) = ([0.; 128], [0.; 128]);
                    let mut audible = 0_f32;
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        engine.cc_from(0, 2, 1, 37);
                        assert!(engine.note_controller_from(0, 61, other, crate::midi::NoteControl::Registered { index: 10, value: u32::MAX }));
                        assert!(engine.note_on_input_velocity_pitch(address, 0, 60, Velocity::FULL, None));
                        assert!(engine.note_on_input_velocity_pitch(other, 0, 61, Velocity::FULL, None));
                        assert!(engine.host_note_on_velocity(exact, 0, 61, Velocity::FULL, 0.));
                        engine.render(&mut left, &mut right);
                        assert!(left.iter().chain(&right).any(|&sample| sample.abs() > 0.05));
                        if scripted { engine.ui_control(0, 0, 1); }
                        if reset { engine.reset(48000.); }
                        retired = engine.set_bank(replacement.take());
                        assert!(!engine.key_down(0, 60) && !engine.key_down(0, 61));
                        assert_eq!(engine.player.midi.cc[2][1].midi1(), if reset { 0 } else { 37 });
                        engine.mark_host_notes();
                        assert!(!engine.host_note_pending(exact), "replaced exact owner remained pinned");
                        if !reset { assert_eq!(engine.host_note_end_at(exact), Some(128)); }
                        engine.note_off_from(0, 2, 60); engine.note_off_from(0, 5, 61); engine.host_note_off(pattern);
                        let (mut rendered, mut chunk) = (0, 0);
                        while rendered < 1024 {
                            let n = if fragmented { [1, 7, 31, 128][chunk % 4] } else { 128 }.min(1024 - rendered);
                            engine.render(&mut left[..n], &mut right[..n]);
                            assert!(left[..n].iter().chain(&right[..n]).all(|&sample| sample == 0.),
                                "scripted={scripted} reset={reset}: old off/held child/wait resurrected audio in replacement bank");
                            rendered += n; chunk += 1;
                        }
                        assert_eq!(engine.active_voices(), 0);
                        assert_eq!(engine.player.pending_releases.len(), 0);
                        // CC121 is not involved in bank replacement: native PCK
                        // state remains available to new notes at this address.
                        if reset { assert!(engine.note_controller_from(0, 61, other, crate::midi::NoteControl::Registered { index: 10, value: u32::MAX })); }
                        assert!(engine.note_on_input_velocity_pitch(other, 0, 61, Velocity::FULL, None));
                        engine.note_off_from(0, 5, 61);
                        engine.render(&mut left, &mut right);
                        assert!(left.iter().all(|&sample| sample == 0.), "retained native per-note pan changed on replacement");
                        audible = right.iter().copied().fold(0., f32::max);
                    }), 0);
                    assert!(audible > 0.5, "fresh On/Off failed to start the replacement's release sample");
                    if scripted {
                        assert_eq!(engine.script().unwrap().interface(0).controls[1].properties["$CONTROL_PAR_VALUE"], crate::ksp::Value::Int(1),
                            "unrelated authored UI wait was canceled by note ownership cleanup");
                    }
                    drop(retired);
                }
            }
        }
    }

    #[test]
    fn layered_partial_steals_keep_other_layers_children_and_deferred_release_owners_without_heap() {
        use super::super::{HostExpression, HostNote, HostPattern};
        let make = |omit_stolen: bool, limited: bool, scripted: bool| {
            let groups = vec![
                Group { voice_group: Some(0), muted: omit_stolen, key_tracking: false, ..Group::default() },
                Group { key_tracking: false, ..Group::default() },
                Group { release_trigger: true, key_tracking: false, ..Group::default() },
                Group { key_tracking: false, ..Group::default() },
            ];
            let zones = (0..4).map(|group| {
                let key = if group == 3 { 61 } else { 60 };
                Zone { group, sample: format!("layer{group}").into(), low_key: key, high_key: key, ..Zone::default() }
            }).collect();
            let samples = [0.125, 0.25, 0.375, 0.0625].into_iter().enumerate().map(|(group, value)| (
                format!("layer{group}").into(), Sample { rate: 48000, frames: vec![[value; 2]; if group == 2 { 2048 } else { 8192 }] },
            )).collect();
            let mut bank = Bank::from_samples(groups, zones, samples).unwrap();
            if limited {
                bank.voice_groups = vec![Some(super::super::bank::VoiceGroup::from(&crate::import::VoiceLimit {
                    max_voices: 1, kill_mode: 1, prefer_released: true, fade_ms: 10, exclusion_group: -1,
                }))];
                // Sample-only banks do not configure instrument voice-group
                // assignments. Prepare the effective row as normal import does.
                bank = bank.with_prepared_settings(|settings| settings[0].voice_group = Some(0)).unwrap();
            }
            let mut engine = Engine::default();
            engine.attack = 0.; engine.release = 0.001;
            engine.set_bank(Some(Box::new(bank)));
            assert_eq!(engine.settings().unwrap()[0].voice_group, limited.then_some(0));
            if scripted {
                let source = "on note\nif ($EVENT_NOTE = 60)\nplay_note(61,127,0,-1)\nend if\nend on";
                let (rt, errors) = crate::ksp::Runtime::with_scripts(&[source], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
                assert!(errors.iter().all(Option::is_none), "{errors:?}");
                engine.set_script(Some(Box::new(rt)));
            }
            engine
        };
        let old = HostNote { port: 0, channel: 4, key: 60, id: 70, clap: true };
        let new = HostNote { channel: 7, id: 71, ..old };
        let pattern = |note: HostNote| HostPattern { port: i32::from(note.port), channel: i32::from(note.channel),
            key: i32::from(note.key), id: note.id, clap: note.clap };
        for scripted in [false, true] {
            for fragmented in [false, true] {
                // The old reference omits the layer that will be stolen. The
                // new reference has both layers, with no admission limit.
                let (mut actual, mut left, mut right) = (make(false, true, scripted), make(true, false, scripted), make(false, false, scripted));
                let (mut a, mut ar, mut l, mut lr, mut r, mut rr) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                let (mut frame, mut chunk) = (0, 0);
                let (mut error, mut peak) = (0_f32, 0_f32);
                assert_eq!(crate::plugin::tests::allocations(|| {
                    for stop in [0, 256, 768, 1024, 1280, 2048, 3327, 3328, 3456] {
                        while frame < stop {
                            let n = if fragmented { [1, 7, 31, 128][chunk % 4] } else { 128 }.min(stop - frame);
                            actual.render(&mut a[..n], &mut ar[..n]); left.render(&mut l[..n], &mut lr[..n]); right.render(&mut r[..n], &mut rr[..n]);
                            if frame >= 768 {
                                for i in 0..n {
                                    error = error.max((a[i] - l[i] - r[i]).abs()).max((ar[i] - lr[i] - rr[i]).abs());
                                    peak = peak.max(a[i].abs()).max(ar[i].abs());
                                }
                            }
                            frame += n; chunk += 1;
                        }
                        match stop {
                            0 => for e in [&mut actual, &mut left] {
                                assert!(e.host_note_on_velocity(old, 0, 60, Velocity::FULL, 0.));
                                e.host_expression(pattern(old), HostExpression::Pan(-1.));
                            },
                            256 => for e in [&mut actual, &mut right] {
                                assert!(e.host_note_on_velocity(new, 0, 60, Velocity::FULL, 0.));
                                e.host_expression(pattern(new), HostExpression::Pan(1.));
                            },
                            768 => {
                                let old_layers = actual.player.voices.iter().filter(|v|
                                    v.host_note.and_then(|id| actual.player.host_notes.get(id)).is_some_and(|o| o.note == old));
                                assert_eq!(old_layers.filter(|v| !v.fade.dying()).count(), if scripted { 2 } else { 1 });
                                for e in [&mut actual, &mut left] {
                                    e.cc(4, 64, 127); e.host_note_off(pattern(old));
                                    e.host_expression(pattern(old), HostExpression::Gain(0.25));
                                }
                                actual.mark_host_notes();
                                assert!(actual.host_note_pending(old), "partial steal retired surviving layers or deferred release");
                            }
                            1024 => {
                                actual.host_note_off(pattern(old));
                                assert_eq!(actual.player.pending_releases.iter().filter(|release| release.note == 60 &&
                                    release.host_note.and_then(|id| actual.player.host_notes.get(id)).is_some_and(|o| o.note == old)).count(), 1,
                                    "duplicate physical off queued another native release family");
                                actual.mark_host_notes(); assert!(actual.host_note_pending(old));
                            }
                            1280 => for e in [&mut actual, &mut left] { e.cc(4, 64, 0); },
                            2048 | 3327 => { actual.mark_host_notes(); assert!(actual.host_note_pending(old), "release PCM ended before its exclusive EOF"); },
                            3328 => {
                                actual.mark_host_notes();
                                assert!(!actual.host_note_pending(old), "completed old layers, child and release retained an owner");
                                assert_eq!(actual.host_note_end_at(old), Some(3328));
                                assert!(actual.host_note_pending(new), "old release retired the new same-key root");
                                actual.retire_host_note(old);
                                assert!(!actual.host_note_present(old) && actual.host_note_present(new));
                            }
                            _ => {}
                        }
                    }
                }), 0);
                assert!(peak > 0.05 && error < 1e-6, "scripted={scripted} fragmented={fragmented}: partial-steal PCM differs from independent layers: {error}");
            }
        }
    }

    #[test]
    fn release_only_counter_and_sostenuto_keep_physical_root_pcm_without_heap() {
        use crate::import::{ModAssignment, ModSource, ModTarget};
        let make = |counter: bool, gain: f32| {
            let group = Group {
                release_trigger: true,
                release_counter_ms: 1000,
                gain,
                mods: if counter { vec![ModAssignment { cc_default: None,
                    name: "Root release counter".into(), source: ModSource::ReleaseTriggerCounter,
                    target: ModTarget::Volume, intensity: 1., invert: false, lag_ms: 0, shaper: None,
                }] } else { Vec::new() },
                ..Group::default()
            };
            let bank = Bank::from_samples(vec![group], vec![Zone::default()], vec![(
                Default::default(), Sample { rate: 48000, frames: vec![[0.25; 2]; 8192] },
            )]).unwrap();
            let mut engine = Engine::default();
            engine.attack = 0.;
            engine.set_bank(Some(Box::new(bank)));
            engine
        };
        for pedal in [64, 66] {
            for reset in [false, true] {
                for fragmented in [false, true] {
                    // The independent references have constant authored gains:
                    // T=1000ms, physical2 held500ms (or125ms after reset),
                    // physical5 held375ms. Pedal-up is deliberately later.
                    let left_gain = if reset { 0.875 } else { 0.5 };
                    let (mut actual, mut left, mut right) = (make(true, 1.), make(false, left_gain), make(false, 0.625));
                    let address = |channel| super::super::InputAddress { port: 0, channel, key: 60 };
                    let mut frame = 0;
                    let mut chunk = 0;
                    let (mut a, mut ar, mut l, mut lr, mut r, mut rr) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                    let (mut error, mut peak) = (0_f32, 0_f32);
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        let pan = |value| crate::midi::NoteControl::Registered { index: 10, value };
                        assert!(actual.note_controller_from(0, 60, address(2), pan(0)));
                        assert!(actual.note_controller_from(0, 60, address(5), pan(u32::MAX)));
                        assert!(left.note_controller_from(0, 60, address(2), pan(0)));
                        assert!(right.note_controller_from(0, 60, address(5), pan(u32::MAX)));
                        for stop in [0, 12000, 18000, 24000, 30000, 32000, 36000, 40096] {
                            while frame < stop {
                                let n = if fragmented { [1, 7, 31, 128][chunk % 4] } else { 128 }.min(stop - frame);
                                actual.render(&mut a[..n], &mut ar[..n]);
                                left.render(&mut l[..n], &mut lr[..n]);
                                right.render(&mut r[..n], &mut rr[..n]);
                                if frame < 36000 {
                                    assert!(a[..n].iter().chain(&ar[..n]).all(|&sample| sample == 0.), "release-only pedal={pedal} must defer audio");
                                } else {
                                    for i in 0..n {
                                        error = error.max((a[i] - l[i] - r[i]).abs()).max((ar[i] - lr[i] - rr[i]).abs());
                                        peak = peak.max(l[i].abs()).max(rr[i].abs());
                                    }
                                }
                                frame += n;
                                chunk += 1;
                            }
                            match stop {
                                0 => {
                                    assert!(actual.note_on_input_velocity_pitch(address(2), 0, 60, Velocity::FULL, None));
                                    assert!(left.note_on_input_velocity_pitch(address(2), 0, 60, Velocity::FULL, None));
                                }
                                12000 => {
                                    assert!(actual.note_on_input_velocity_pitch(address(5), 0, 60, Velocity::FULL, None));
                                    assert!(right.note_on_input_velocity_pitch(address(5), 0, 60, Velocity::FULL, None));
                                }
                                18000 => {
                                    if reset {
                                        let command = script::Command { at: 0, channel: 0, input_channel: Some(2),
                                            id: EventId::default(), kind: script::Kind::ResetCounter(60), applied: false };
                                        let defaults = actual.defaults();
                                        actual.player.apply(actual.bank.as_deref(), &actual.state, &command, None, defaults);
                                    }
                                    actual.cc(2, pedal, 127); left.cc(2, pedal, 127);
                                    actual.cc(5, pedal, 127); right.cc(5, pedal, 127);
                                }
                                24000 => { actual.note_off_from(0, 2, 60); left.note_off_from(0, 2, 60); }
                                30000 => { actual.note_off_from(0, 5, 60); right.note_off_from(0, 5, 60); }
                                32000 => { actual.note_off_from(0, 2, 60); assert_eq!(actual.player.pending_releases.len(), 2); }
                                36000 => {
                                    actual.cc(2, pedal, 0); left.cc(2, pedal, 0);
                                    actual.cc(5, pedal, 0); right.cc(5, pedal, 0);
                                    assert!(actual.player.pending_releases.is_empty());
                                }
                                _ => {}
                            }
                        }
                    }), 0);
                    assert!(peak > 0.1, "the independent release oracle must be audible");
                    assert!(error < 1e-6, "pedal={pedal} reset={reset} fragmented={fragmented}: root duration PCM error={error}");
                }
            }
        }
    }

    #[test]
    fn release_only_keys_retain_precise_onset_and_first_release_through_pedals() {
        for onset in [Velocity::ZERO, Velocity::new(0.5001).unwrap()] {
            let bank = Bank::from_samples(
                vec![Group {
                    release_trigger: true,
                    ..Group::default()
                }],
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
            let release = Velocity::new(0.23456789).unwrap();
            let mut exercise = || {
                engine.cc(0, 64, 127);
                engine.note_on_velocity(0, 60, onset);
                assert!(
                    engine.input_key_down_from(0, 0, 60),
                    "zero attack is still a held note"
                );
                assert_eq!(
                    engine.active_voices(),
                    0,
                    "this instrument has only release zones"
                );
                engine.note_off_velocity(0, 60, release);
                engine.note_off_velocity(0, 60, Velocity::FULL);
                assert!(!engine.input_key_down_from(0, 0, 60));
                assert_eq!(engine.player.pending_releases.len(), 1);
                let pending = engine.player.pending_releases[0];
                assert_eq!(pending.onset_velocity, onset);
                assert_eq!(pending.release_velocity, Some(release));
                engine.cc(0, 64, 0);
                assert!(engine.player.pending_releases.is_empty());
                assert_eq!(engine.player.voices.len(), 1);
                let voice = &engine.player.voices[0];
                assert!(voice.release_trigger);
                assert_eq!(voice.onset_velocity, onset);
                assert_eq!(
                    voice.release_velocity,
                    Some(release),
                    "pedal-up must retain physical key-up data"
                );
            };
            #[cfg(feature = "plugin")]
            assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
            #[cfg(not(feature = "plugin"))]
            exercise();
        }
    }

    #[test]
    fn release_velocity_modulates_pcm_and_pitch_without_seven_bit_rounding() {
        use crate::modulation::{ModAssignment, ModSource, ModTarget};
        let onset = Velocity::new(0.87654321).unwrap();
        let releases = [
            Velocity::new(0.5001).unwrap(),
            Velocity::new(0.5002).unwrap(),
        ];
        assert_eq!(releases[0].midi1(), releases[1].midi1());
        let make = || {
            let route = |source, target, intensity| ModAssignment { cc_default: None,
                name: "velocity".into(),
                source,
                target,
                intensity,
                invert: false,
                lag_ms: 0,
                shaper: None,
            };
            let group = Group {
                release_trigger: true,
                mods: vec![
                    route(ModSource::Velocity, ModTarget::Volume, 1.),
                    route(ModSource::ReleaseVelocity, ModTarget::Volume, 1.),
                    route(ModSource::ReleaseVelocity, ModTarget::Pitch, 0.5),
                ],
                ..Group::default()
            };
            let bank = Bank::from_samples(
                vec![group],
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
            engine
        };
        let (mut a, mut b) = (make(), make());
        let (mut left, mut right, mut other, mut other_right) =
            ([0.; 512], [0.; 512], [0.; 512], [0.; 512]);
        let mut exercise = || {
            for (engine, release, l, r) in [
                (&mut a, releases[0], &mut left[..], &mut right[..]),
                (&mut b, releases[1], &mut other[..], &mut other_right[..]),
            ] {
                engine.cc(0, 64, 127);
                engine.note_on_velocity(0, 60, onset);
                engine.note_off_velocity(0, 60, release);
                engine.note_off_velocity(0, 60, Velocity::FULL);
                engine.cc(0, 64, 0);
                engine.render(l, r);
                let voice = &engine.player.voices[0];
                assert_eq!(voice.onset_velocity, onset);
                assert_eq!(voice.release_velocity, Some(release));
                assert!((voice.pitch.0 - 6. * release.dsp()).abs() < 1e-6);
            }
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
        #[cfg(not(feature = "plugin"))]
        exercise();
        for (pcm, release) in [(&left, releases[0]), (&other, releases[1])] {
            let expected = 0.25 * onset.dsp() * release.dsp();
            assert!(
                pcm[256..].iter().all(|v| (*v - expected).abs() < 1e-6),
                "release gain lost original precision"
            );
        }
        assert_ne!(left[511], other[511]);
        assert!((left[511] / other[511] - releases[0].dsp() / releases[1].dsp()).abs() < 1e-6);
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
    let mut best: Option<((u8, u64, u64, u64), usize)> = None;
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
        // Storage compaction must not change ties, including layers of one note.
        let priority = (rank, key, v.age, v.admission);
        if best.is_none_or(|(b, _)| priority < b) {
            best = Some((priority, i));
        }
    }
    (live, best.map(|(_, i)| i))
}

#[cfg(all(test, feature = "plugin"))]
#[path = "victim_tests.rs"]
mod victim_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "note_mode_tests.rs"]
mod note_mode_tests;
