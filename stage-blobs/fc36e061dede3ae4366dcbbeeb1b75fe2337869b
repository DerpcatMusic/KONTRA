//! Worker-prepared primary AHDSR receipt roster and one sealed native source range.
//! Numeric MIDI state and live note metadata remain owned by chronological ingress.

use std::cell::Cell;
use super::{Ahdsr, Bank, GroupSettings, HostRef, NoteEvent, MAX_VOICES,
    ahdsr::{Candidate, Shared}, host_notes::{AcceptedStart, Owners}, pedals::Pedals,
    player::Player, source_note::SourceNote};
use crate::{articulate::In, midi::{Control, Midi1Decoder, ParameterData, ParameterEvent, Velocity}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloneRef { Active(u64), Starting { start: AcceptedStart, zone: u32 } }

struct SourceGeometry { step: f64, position: f64, length: u64 }

struct Clone {
    id: CloneRef,
    group: usize,
    note: SourceNote,
    velocity: u8,
    onset_velocity: Velocity,
    entry: usize,
    off: Option<u64>,
    source: Shared,
    actual: Cell<Option<u64>>,
    geometry: Option<SourceGeometry>,
}

struct Cache {
    group: usize,
    slot: u8,
    parameters: Option<Ahdsr>,
    candidate: Option<Candidate>,
    producer: Option<CloneRef>,
}

/// Prepared only for instruments with a verified raw shared-primary record.
/// The Owners copy is alias-admission projection storage, not another Commands
/// Start/Release payload pool. It never publishes an alias or mutates live MIDI.
pub(super) struct Range {
    roots: Owners,
    clones: Vec<Clone>,
    caches: Vec<Cache>,
    pedals: Pedals,
    topology: crate::midi::MpeZones,
    setup_reset: u16,
    received: [Midi1Decoder; 16],
    bend_peaks: [f32; 16],
    inputs: [u128; 16],
    input_free: usize,
    prospective: usize,
    generation: u64,
    window: Option<(u64, u64)>,
    sealed: bool,
    committed: Cell<bool>,
    current: Cell<Option<AcceptedStart>>,
    error: Cell<Option<&'static str>>,
}

impl Range {
    pub(super) fn new(settings: &[GroupSettings]) -> Option<Box<Self>> {
        let caches: Vec<_> = settings.iter().enumerate().filter_map(|(group, p)|
            p.native_shared_volume_slot.map(|slot| Cache { group, slot, parameters: p.envelope, candidate: None, producer: None })).collect();
        (!caches.is_empty()).then(|| Box::new(Self { roots: Owners::new(),
            clones: Vec::with_capacity(MAX_VOICES), caches, pedals: Pedals::default(), topology: Default::default(), setup_reset: 0,
            received: Default::default(), bend_peaks: [0.; 16], inputs: [0; 16], input_free: 0, prospective: 0,
            generation: 0, window: None, sealed: false, committed: Cell::new(false), current: Cell::new(None), error: Cell::new(None) }))
    }

    pub(super) fn invalidate(&mut self, reason: &'static str) {
        self.sealed = false; self.committed.set(false); self.current.set(None); self.error.set(Some(reason));
    }

    pub(super) fn reason(&self) -> Option<&'static str> { self.error.get() }
    pub(super) fn window(&self) -> Option<(u64, u64)> { self.window }
    pub(super) fn current(&self, token: Option<AcceptedStart>) { self.current.set(token); }

    pub(super) fn begin(&mut self, player: &Player, settings: &[GroupSettings],
        window: (u64, u64), topology: crate::midi::MpeZones) -> Result<(), &'static str> {
        self.sealed = false; self.committed.set(false); self.current.set(None); self.error.set(None);
        if window.0 != player.now || window.1 <= window.0 || window.1 - window.0 > 1024 {
            return Err("shared AHDSR requires the current balanced native source range");
        }
        self.window = Some(window);
        self.generation = self.generation.checked_add(1)
            .ok_or("shared AHDSR source generation exhausted")?;
        self.roots.copy_from(&player.host_notes); self.pedals = player.pedals; self.topology = topology;
        self.setup_reset = player.midi.mpe_role_changes(topology);
        self.received = player.midi.received_decoders(); self.inputs = [0; 16];
        self.bend_peaks = std::array::from_fn(|input| bend_bound(player, topology, input as u8));
        self.input_free = player.per_note.available(); self.prospective = player.voices.len();
        self.clones.clear();
        for cache in &mut self.caches {
            let p = &settings[cache.group];
            if p.native_shared_volume_slot != Some(cache.slot) {
                return Err("shared AHDSR source ownership changed after preparation");
            }
            if cache.parameters != p.envelope {
                if player.voices.iter().any(|v| v.group as usize == cache.group) {
                    return Err("shared AHDSR held-source parameter retargeting is not admitted");
                }
                cache.parameters = p.envelope;
            }
            cache.candidate = None; cache.producer = None;
        }
        for voice in &player.voices {
            let Some(slot) = settings[voice.group as usize].native_shared_volume_slot else { continue };
            let Some(cache) = self.caches.iter().find(|c| c.group == voice.group as usize && c.slot == slot) else {
                return Err("shared AHDSR source ownership changed after preparation");
            };
            if voice.onset != super::voice::Onset::Started || voice.fade.dying()
                || voice.stream.is_some() || voice.map.looped.is_some() || voice.counted.is_some() {
                return Err("shared AHDSR active source is delayed, streaming, looping or stopping");
            }
            let source = voice.env.shared().copied()
                .ok_or("shared AHDSR active voice has no admitted source owner")?;
            self.clones.push(Clone { id: CloneRef::Active(voice.admission), group: cache.group,
                note: voice.source_note(), velocity: voice.velocity, onset_velocity: voice.onset_velocity, entry: 0, off: None, source, actual: Cell::new(Some(voice.admission)), geometry: None });
        }
        self.clones.sort_unstable_by_key(|c| match c.id { CloneRef::Active(id) => id, _ => unreachable!() });
        Ok(())
    }

    /// This is called only after the real fixed-route/physical capacity preflight.
    pub(super) fn start(&mut self, bank: &Bank, settings: &[GroupSettings], rate: f32,
        token: AcceptedStart, at: u64, event: &NoteEvent, groups: &super::GroupMask) -> Result<(), &'static str> {
        let (start, end) = self.window.ok_or("shared AHDSR has no accepted source range")?;
        if at < start || at >= end { return Err("shared AHDSR delayed/end-offset starts are not admitted"); }
        let zones = bank.matching_zones(event.channel, event.note, event.velocity, false,
            event.groups.unwrap_or(groups));
        for zone in zones {
            self.prospective += 1;
            if self.prospective > bank.polyphony.max_voices.min(MAX_VOICES) {
                return Err("shared AHDSR voice pressure or stealing is not admitted");
            }
            let group = bank.zones()[zone as usize].group;
            if settings[group].native_shared_volume_slot.is_none() { continue; }
            if self.clones.len() == self.clones.capacity() { return Err("shared AHDSR accepted clone capacity exceeded"); }
            let p = settings[group].envelope.ok_or("shared AHDSR primary parameters are missing")?;
            let source = Shared::new(&p, rate);
            // Native source geometry and signed counter capacity are checked by
            // the source owner, including zero-stage entry arithmetic.
            source.stage(0, None)?;
            self.clones.push(Clone { id: CloneRef::Starting { start: token, zone }, group,
                note: SourceNote { channel: event.channel, note: event.note, owner: event.owner,
                    input_channel: event.input_channel, host_note: event.host_note,
                    held: true, captured: false, released: false, release_trigger: false },
                velocity: event.velocity, onset_velocity: event.onset_velocity,
                entry: (at - start) as usize, off: None, source, actual: Cell::new(None), geometry: None });
        }
        Ok(())
    }

    pub(super) fn reserve_root(&mut self, note: super::HostNote, velocity: crate::midi::Velocity,
        token: AcceptedStart, at: u64) -> Option<HostRef> {
        self.roots.reserve_start(note, note.channel, note.key, velocity, token, at)
    }

    fn key_up(&mut self, at: u64, matches: impl Fn(&SourceNote) -> bool) {
        for clone in &mut self.clones {
            if matches(&clone.note) && clone.note.key_up(&self.pedals) { clone.off = Some(at); }
        }
    }

    pub(super) fn anonymous_off(&mut self, channel: u8, key: u8, at: u64) {
        self.key_up(at, |note| note.matches_key_up(channel, key, Some(channel)));
    }

    pub(super) fn exact_off(&mut self, pattern: super::HostPattern, at: u64) {
        for index in 0..self.roots.active.len() {
            let id = self.roots.active[index];
            let Some(root) = self.roots.get_mut(id).filter(|root| root.held && pattern.matches(root.note)) else { continue };
            root.held = false;
            self.key_up(at, |note| note.host_note == Some(id));
        }
    }

    fn pedal_up(&mut self, input: u8, at: u64) {
        for clone in &mut self.clones {
            if clone.note.pedal_up(input, &self.pedals) { clone.off = Some(at); }
        }
    }

    pub(super) fn pedal(&mut self, input: u8, cc: u8, down: bool, at: u64, sustain: bool) {
        match cc {
            64 if sustain => {
                if self.pedals.set_sustain(input, down) { self.pedal_up(input, at); }
            }
            66 => {
                let change = self.pedals.set_sostenuto(1 << input, down);
                if change.changed == 0 { return; }
                self.roots.sostenuto(change.capture, change.up);
                for clone in &mut self.clones { clone.note.sostenuto(input, down); }
                if !down { self.pedal_up(input, at); }
            }
            121 => { self.pedal(input, 64, false, at, sustain); self.pedal(input, 66, false, at, sustain); }
            _ => {},
        }
    }

    /// Visit the same normalized input stream as live ingress, in arrival order.
    /// Only physical note identity/hold state and the one-shot velocity prefix
    /// are projected. All numeric controls and decoder effects stay live.
    pub(super) fn project(&mut self, player: &Player, bank: &Bank,
        state: &super::instrument::InstrumentState, part_tune: f32,
        token: AcceptedStart, at: u64, port: u8, input: In) -> Result<(), &'static str> {
        let (start, end) = self.window.ok_or("shared AHDSR has no accepted source range")?;
        if at < start { return Ok(()); }
        if at >= end { return Ok(()); }
        let channel = input.channel();
        if channel >= 16 { return Err("shared AHDSR received an invalid physical channel"); }
        let c = channel as usize;
        let first_velocity = if self.setup_reset != 0 {
            // Real note protocol decoding precedes first Router setup; CC
            // decoding follows it. Only copied acceptance storage changes here.
            let velocity = if let In::Midi1NoteOn(_, _, raw) = input {
                Some(self.received[c].velocity(raw).unwrap_or_else(|| Velocity::from_midi1(raw)))
            } else { None };
            if let In::Midi1NoteOff(_, _, raw) = input { self.received[c].release_velocity(raw); }
            if let In::Midi1ZeroNoteOff(..) = input { self.received[c].velocity(0); }
            self.pedals.clear(self.setup_reset);
            for input in (0..16).filter(|input| self.setup_reset & (1 << input) != 0) {
                self.received[input].reset();
            }
            self.setup_reset = 0;
            velocity
        } else { None };
        let mut pitch = None;
        let (key, velocity, host, tune) = match input {
            In::Midi1NoteOn(_, key, raw) => (key, first_velocity.unwrap_or_else(|| self.received[c].velocity(raw).unwrap_or_else(|| Velocity::from_midi1(raw))), None, 0.),
            In::NoteOn(_, key, raw) => (key, Velocity::from_midi1(raw), None, 0.),
            In::NoteOnVelocity(_, key, velocity) => (key, velocity, None, 0.),
            In::NoteOnVelocityPitch(_, key, velocity, value) => { pitch = Some(value); (key, velocity, None, 0.) },
            In::HostOn(note, velocity, tune) => (note.key, velocity, Some(note), tune),
            In::NoteOff(_, key) | In::NoteOffVelocity(_, key, _) => { self.anonymous_off(channel, key, at); return Ok(()); },
            In::Midi1NoteOff(_, key, raw) => { self.received[c].release_velocity(raw); self.anonymous_off(channel,key,at); return Ok(()); },
            In::Midi1ZeroNoteOff(_, key) => { self.received[c].velocity(0); self.anonymous_off(channel,key,at); return Ok(()); },
            In::HostOff(pattern) | In::HostOffVelocity(pattern, _) => { self.exact_off(pattern,at); return Ok(()); },
            In::Midi1Cc(_, cc, raw) => {
                let received = self.received[c].cc(cc,raw).ok_or("shared AHDSR controller data is invalid")?;
                self.parameter(player,channel,received.parameter)?;
                self.control(player,channel,received.control,at,state.uses_sustain())?; return Ok(());
            },
            In::Cc(_, cc, raw) => {
                let parameter = self.received[c].parameter(cc,raw.min(127));
                self.parameter(player,channel,parameter)?;
                self.control(player,channel,Control::Cc(cc,crate::midi::ControlValue::from_midi1(raw)),at,state.uses_sustain())?; return Ok(());
            },
            In::MidiControl(_, control @ (Control::Cc(..) | Control::Cc14 { .. })) => { self.control(player,channel,control,at,state.uses_sustain())?; return Ok(()); },
            In::Pressure(..) | In::PolyAt(..) | In::Midi1PolyAt(..)
            | In::MidiControl(_, Control::Pressure(_) | Control::ResetPressure) => return Ok(()),
            In::Bend(..) | In::MidiControl(_, Control::Bend(_)) => return Ok(()),
            In::HostExpression(_, super::HostExpression::Gain(value) | super::HostExpression::Pan(value))
                if value.is_finite() => return Ok(()),
            _ => return Err("shared AHDSR source-affecting expression, parameter, choke or management input is not admitted"),
        };
        if key >= 128 || !tune.is_finite() { return Err("shared AHDSR start has invalid physical geometry"); }
        let address = super::InputAddress { port, channel, key };
        if !player.per_note.contains_address(address) && self.inputs[c] & (1 << key) == 0 {
            self.input_free = self.input_free.checked_sub(1)
                .ok_or("shared AHDSR physical note-control capacity exceeded")?;
            self.inputs[c] |= 1 << key;
        }
        let range = player.midi.per_note_range[c];
        if !range.is_finite() || !(0.0..128.0).contains(&range)
            || pitch.is_some_and(|p| !p.is_finite() || !(0.0..128.0).contains(&p)) {
            return Err("shared AHDSR root pitch geometry is unrepresentable");
        }
        let host_note = if let Some(note) = host {
            Some(self.reserve_root(note,velocity,token,at).ok_or("shared AHDSR exact root admission capacity or tuple collision")?)
        } else { None };
        let event = NoteEvent { velocity: if host.is_some() { velocity.midi1() } else { velocity.midi1().max(1) },
            host_note, owner: Some((channel,key)), input_channel: Some(channel),
            ..NoteEvent::new_velocity(channel,key,velocity) };
        // Router::input_impl resets this delivered key to default immediately
        // before its real On. Direct Engine starts have no accepted token.
        let expression = player.per_note.preview_overlay(address,range,pitch,super::Expression::default());
        let first_clone = self.clones.len();
        self.start(bank,&state.settings,player.rate as f32,token,at,&event,&player.allowed)?;
        for clone in &mut self.clones[first_clone..] {
            let CloneRef::Starting { zone, .. } = clone.id else {
                return Err("shared AHDSR starting source geometry is missing");
            };
            let group = bank.zones()[zone as usize].group;
            let play = &bank.plays[zone as usize];
            let sample = &bank.samples[play.sample as usize];
            let key_ratio = if bank.groups()[group].key_tracking {
                2f64.powf((f64::from(key)-f64::from(bank.zones()[zone as usize].root))/12.)
            } else { 1. };
            let step = f64::from(sample.rate) / player.rate * bank.zones()[zone as usize].tune * key_ratio;
            let bound = pitch_bound(player,state,bank,group,channel,expression.tune.abs()+tune.abs(),part_tune);
            clone.geometry = Some(SourceGeometry { step: step * bound, position: 0., length: play.map.len(super::map::FOREVER) });
        }
        Ok(())
    }

    fn parameter(&mut self, player: &Player, input: u8, parameter: Option<ParameterEvent>) -> Result<(), &'static str> {
        let Some(parameter) = parameter else { return Ok(()); };
        if !parameter.registered { return Ok(()); }
        if parameter.address != 0 || self.topology.zone(input).is_none() {
            return Err("shared AHDSR only unchanged-role MPE RPN0 sensitivity is admitted");
        }
        let Some(change) = self.topology.parameter(input,parameter) else { return Ok(()); };
        if change.channels != 0 { return Err("shared AHDSR MPE role changes are not admitted"); }
        if matches!(parameter.data,ParameterData::Increment | ParameterData::Decrement) {
            super::midi_state::MidiState::seed_mpe_parameter_baselines(&mut self.received,self.topology,input);
        }
        for input in 0..16 {
            self.bend_peaks[input] = self.bend_peaks[input].max(bend_bound(player,self.topology,input as u8));
        }
        Ok(())
    }

    fn control(&mut self, player: &Player, input: u8, control: Control, at: u64, sustain: bool) -> Result<(), &'static str> {
        if !control.valid() { return Err("shared AHDSR controller data is invalid"); }
        let (cc, raw) = control.legacy();
        match cc {
            64 | 66 | 121 => {
                let channels = Player::control_channels_for(self.topology,crate::midi::ControlTarget::Physical(input),control);
                for input in (0..16u8).filter(|input| channels & (1 << input) != 0) {
                    self.pedal(input,cc,raw>=64,at,player.native_sustain && sustain);
                    if cc == 121 { self.received[input as usize].reset_controllers(&player.midi.controller_defaults); }
                }
            },
            120 | 123..=127 => return Err("shared AHDSR channel-mode source changes are not admitted"),
            _ => {},
        }
        Ok(())
    }

    pub(super) fn validate_active_sources(&mut self, player: &Player, bank: &Bank,
        state: &super::instrument::InstrumentState, part_tune: f32) -> Result<(), &'static str> {
        self.window.ok_or("shared AHDSR has no accepted source range")?;
        for voice in &player.voices {
            if voice.env.shared().is_none() { continue; }
            let input = voice.input_channel.unwrap_or(voice.channel);
            let key = voice.owner.map_or(voice.note, |(_,key)| key);
            let expression = voice.frozen_expression.unwrap_or(player.midi.expression[voice.channel as usize][key as usize]);
            let expression = player.per_note.overlay(voice.per_note,expression);
            let expression = player.host_notes.overlay(voice.host_note,expression);
            let bound = pitch_bound(player,state,bank,voice.group as usize,input,expression.tune.abs(),part_tune);
            let clone = self.clones.iter_mut().find(|clone| clone.id == CloneRef::Active(voice.admission))
                .ok_or("shared AHDSR active source geometry is missing")?;
            clone.geometry = Some(SourceGeometry { step: voice.step * voice.tune * bound,
                position: voice.pos, length: voice.length });
        }
        Ok(())
    }

    /// Build and validate every candidate and every clone before committing any.
    pub(super) fn seal(&mut self) -> Result<(), &'static str> {
        let (start, end) = self.window.ok_or("shared AHDSR has no accepted source range")?;
        let frames = (end - start) as usize;
        // Admission checks every sensitivity reached in this range, before a
        // producer commits. A later reduction cannot hide an earlier EOF risk.
        for clone in &self.clones {
            let geometry = clone.geometry.as_ref().ok_or("shared AHDSR source geometry is missing")?;
            let input = clone.note.input_channel.unwrap_or(clone.note.channel) as usize;
            let bend = 2f64.powf(f64::from(self.bend_peaks[input])/12.);
            check_source(geometry.step * bend,geometry.position,geometry.length,frames-clone.entry)?;
        }
        for cache in &mut self.caches {
            let Some(producer) = self.clones.iter().find(|c| c.group == cache.group) else { continue };
            let mut off = None;
            for clone in self.clones.iter().filter(|c| c.group == cache.group) {
                if let Some(at) = clone.off {
                    if at < start || at >= end { return Err("shared AHDSR effective Off is outside its accepted source range"); }
                    let signed = (at - start) as i64 - clone.entry as i64;
                    let count = i32::try_from(signed / 32)
                        .map_err(|_| "shared AHDSR release countdown is unrepresentable")?;
                    if count < 0 { return Err("shared AHDSR release countdown is negative"); }
                    // Native overwrites in clone admission order, not earliest Off.
                    off = Some(count);
                }
            }
            let count = (frames - producer.entry).div_ceil(32);
            let candidate = producer.source.stage(count, off)?;
            if candidate.points[..candidate.count].iter().any(|x| !x.is_finite()) {
                return Err("shared AHDSR candidate contains a nonfinite source point");
            }
            for clone in self.clones.iter().filter(|c| c.group == cache.group) {
                let remaining = frames - clone.entry;
                let suffix = count.checked_sub(remaining.div_ceil(32))
                    .ok_or("shared AHDSR clone precedes its source producer")?;
                clone.source.validate_binding(&candidate, self.generation, remaining, suffix)?;
            }
            cache.producer = Some(producer.id); cache.candidate = Some(candidate);
        }
        self.sealed = true;
        Ok(())
    }

    fn bind(&self, clone: &Clone, source: &mut Shared) -> Result<(), &'static str> {
        if !self.sealed { return Err(self.error.get().unwrap_or("shared AHDSR source range is not sealed")); }
        let (start, end) = self.window.unwrap();
        let cache = self.caches.iter().find(|c| c.group == clone.group)
            .ok_or("shared AHDSR source cache ownership is missing")?;
        let candidate = cache.candidate.as_ref().ok_or("shared AHDSR source candidate is missing")?;
        let remaining = (end - start) as usize - clone.entry;
        let suffix = candidate.count.checked_sub(remaining.div_ceil(32))
            .ok_or("shared AHDSR clone cache suffix is unrepresentable")?;
        if cache.producer == Some(clone.id) {
            source.commit_candidate(candidate, self.generation, remaining, suffix)
        } else { source.copy_shared_end(candidate, self.generation, remaining, suffix) }
    }

    pub(super) fn bind_active(&self, admission: u64, source: &mut Shared) -> Result<(), &'static str> {
        let clone = self.clones.iter().find(|c| c.id == CloneRef::Active(admission))
            .ok_or("shared AHDSR active clone receipt is missing")?;
        self.bind(clone, source)
    }

    pub(super) fn bind_start(&self, zone: u32, event: &NoteEvent, admission: u64, now: u64,
        source: &mut Shared) -> Result<(), &'static str> {
        let token = self.current.get().ok_or("shared AHDSR start lacks an accepted input receipt")?;
        let clone = self.clones.iter().find(|c| c.id == CloneRef::Starting { start: token, zone })
            .ok_or("shared AHDSR start does not match its ordered zone receipt")?;
        if self.window.is_none_or(|(start,_)| start + clone.entry as u64 != now)
            || clone.actual.get().is_some() || clone.note.channel != event.channel || clone.note.note != event.note
            || clone.note.owner != event.owner || clone.note.input_channel != event.input_channel
            || clone.note.host_note != event.host_note || !matches_start(clone,event) {
            return Err("shared AHDSR start changed its accepted physical root or zone identity");
        }
        self.bind(clone, source)?;
        clone.actual.set(Some(admission));
        Ok(())
    }

    pub(super) fn can_start(&self, zone: u32, event: &NoteEvent, now: u64) -> bool {
        let Some(token) = self.current.get() else { return false };
        self.sealed && self.window.is_some_and(|(start, end)| start <= now && now < end)
            && self.clones.iter().any(|c| c.id == CloneRef::Starting { start: token, zone }
                && self.window.is_some_and(|(start,_)| start + c.entry as u64 == now)
                && c.actual.get().is_none() && c.note.channel == event.channel && c.note.note == event.note
                && c.note.owner == event.owner && c.note.input_channel == event.input_channel
                && c.note.host_note == event.host_note && matches_start(c,event))
    }

    pub(super) fn off_is_sealed(&self, admission: u64, now: u64) -> bool {
        self.sealed && self.window.is_some_and(|(start,end)| start <= now && now < end)
            && self.clones.iter().any(|c| c.actual.get() == Some(admission) && c.off == Some(now))
    }

    pub(super) fn points(&self, group: usize, now: u64) -> Option<&[f32]> {
        if !self.sealed || !self.window.is_some_and(|(start,end)| start <= now && now < end) { return None; }
        self.caches.iter().find(|c| c.group == group).and_then(|c| c.candidate.as_ref())
            .map(|c| &c.points[..c.count])
    }
}

fn matches_start(clone: &Clone, event: &NoteEvent) -> bool {
    clone.velocity == event.velocity && clone.onset_velocity == event.onset_velocity
        && event.release_velocity.is_none() && event.counter_stop.is_none()
        && event.release_held_ms.is_none() && !event.group_start_applied && !event.native_release
        && event.script_mods.is_empty() && event.offset_us == 0 && event.volume == 1.
        && event.tune == 0. && event.pan == 0. && event.frozen_expression.is_none()
}

fn bend_bound(player: &Player, topology: crate::midi::MpeZones, input: u8) -> f32 {
    // A conservative bound, not a second pitch renderer. CC121 may remove the
    // current bend; absolute components bound both the current and reset steps.
    let range = |channel: u8| player.midi.channel_bend_range[channel as usize]
        .map_or(0., |range| range.semitones() as f32);
    if let Some(zone) = topology.zone(input) {
        // Bound every legal bend in the sealed span, including reset and a
        // prospective first setup. Live expression/FreezeExpression stays live.
        let member = if input == zone.manager { 0. } else { range(input).max(zone.member_range.semitones() as f32) };
        member + range(zone.manager).max(zone.manager_range.map_or(0., |r| r.semitones() as f32))
    } else { range(input) }
}

fn pitch_bound(player: &Player, state: &super::instrument::InstrumentState, bank: &Bank,
    group: usize, input: u8, expression: f32, part_tune: f32) -> f64 {
    let semitones = state.settings[group].tune.abs() + state.controls.tune.abs()
        + f32::from(bank.transpose).abs() + part_tune.abs() + expression;
    2f64.powf(f64::from(semitones)/12.) * player.midi.tuning[input as usize].ratio()
}

fn check_source(step: f64, position: f64, length: u64, frames: usize) -> Result<(), &'static str> {
    if !step.is_finite() || step < 0. || !position.is_finite() || position < 0.
        || position + step * frames as f64 + 8. >= length as f64 {
        return Err("shared AHDSR source EOF/readiness is not sealed for the complete range");
    }
    Ok(())
}

pub(super) fn validate_group(group: &crate::import::Group, settings: &GroupSettings) -> Result<(), &'static str> {
    if group.native_shared_volume_slot != settings.native_shared_volume_slot {
        return Err("shared AHDSR prepared source ownership differs from the saved roster");
    }
    let Some(slot) = group.native_shared_volume_slot else { return Ok(()); };
    if group.modulators.get(slot as usize).is_none_or(|source|
        !source.volume_env || source.bypassed || source.flex || source.assignments.is_some()) {
        return Err("shared AHDSR requires its original active primary source slot");
    }
    if settings.native_shared_volume_slot != Some(slot) || slot >= 16
        || group.source_mode != Some(0) || group.reverse || group.release_trigger
        || group.voice_group.is_some() || group.release_trigger_note_monophonic
        || group.start_criteria.mask != 0 || !group.start_criteria.items.is_empty()
        || !group.start_criteria.unknown_tail.is_empty() || !group.fx.slots.is_empty()
        || group.wavetable.is_some() || !group.mods.is_empty() || !group.envelopes.is_empty()
        || !group.pitch_lfos.is_empty() || !group.volume_lfos.is_empty() || !group.pitch_glides.is_empty()
        || settings.interp_quality != 0 || settings.native_volume_env || settings.primary_bypass.is_some() || settings.flex.is_some()
        || settings.filter.is_some() || settings.wavetable.is_some() || settings.voice_group.is_some()
        || !settings.pitch_envelopes.is_empty() || !settings.pitch_lfos.is_empty()
        || !settings.volume_lfos.is_empty() || !settings.pitch_glides.is_empty()
        || !settings.mods.same_routes(&super::params::ModTable::default()) {
        return Err("shared AHDSR requires its sole active ordinary Sampler primary source");
    }
    let p = settings.envelope.ok_or("shared AHDSR primary envelope is missing")?;
    if p.ahd_only || [p.attack,p.hold,p.decay,p.release].iter().any(|t| !t.is_finite() || *t < 0.)
        || !p.curve.is_finite() || !(-1.0..=1.0).contains(&p.curve)
        || !p.sustain.is_finite() || !(0.0..=1.0).contains(&p.sustain) {
        return Err("shared AHDSR source parameters are outside the verified native geometry");
    }
    Ok(())
}

impl super::Engine {
    pub(crate) fn shared_source_required(&self) -> bool { self.state.shared_ahdsr.is_some() }

    pub(crate) fn shared_source_window(&self) -> Option<(u64,u64)> {
        self.state.shared_ahdsr.as_ref()?;
        self.player.release_counter.span()
    }

    pub(crate) fn shared_source_current(&self, ordinal: Option<u32>) {
        if let Some(range) = &self.state.shared_ahdsr { range.current(ordinal.map(AcceptedStart)); }
    }

    pub(crate) fn shared_source_reason(&self) -> Option<&'static str> {
        self.state.shared_ahdsr.as_ref().and_then(|range| range.reason())
    }

    pub(crate) fn seal_shared_source(&mut self, topology: Option<crate::midi::MpeZones>, part_tune: f32,
        visit: impl FnOnce(&mut dyn FnMut(u32,u64,u8,In)) -> Result<(), &'static str>) -> Result<(), &'static str> {
        if self.state.shared_ahdsr.is_none() { return Ok(()); }
        let window = self.player.release_counter.span()
            .ok_or("shared AHDSR requires an outer audio/source range")?;
        if self.state.shared_ahdsr.as_ref().is_some_and(|range| range.window() == Some(window) && range.sealed) {
            return Ok(());
        }
        let mut range = self.state.shared_ahdsr.take().unwrap();
        let result = (|| {
            let topology = topology.ok_or("shared AHDSR requires direct static input routing")?;
            if self.script.is_some() || self.commands.iter().any(|command| !command.applied) {
                return Err("shared AHDSR requires script-free identity ingress without pending commands");
            }
            if topology != self.player.midi.mpe
                && (!self.player.voices.is_empty() || !self.player.host_notes.active.is_empty()) {
                return Err("shared AHDSR MPE setup changes existing physical ownership");
            }
            // Only already-due writes are settled. No future callback is run.
            self.admit_direct_start();
            self.mark_per_notes();
            let bank = self.bank.as_deref().ok_or("shared AHDSR has no installed bank")?;
            // A pedal-held Off also retains a release lookup in attack-only
            // banks. It cannot create a voice without a release-trigger group.
            if bank.groups().iter().any(|group| group.release_trigger) {
                return Err("shared AHDSR release-generator siblings are not sealed for this range");
            }
            for (group, settings) in bank.groups().iter().zip(&self.state.settings) {
                validate_group(group,settings)?;
            }
            range.begin(&self.player,&self.state.settings,window,topology)?;
            range.validate_active_sources(&self.player,bank,&self.state,part_tune)?;
            let mut failure = None;
            visit(&mut |ordinal,at,port,input| {
                if failure.is_none() {
                    failure = range.project(&self.player,bank,&self.state,part_tune,
                        AcceptedStart(ordinal),at,port,input).err();
                }
            })?;
            if let Some(reason) = failure { return Err(reason); }
            range.seal()?;
            Ok(())
        })();
        if let Err(reason) = result {
            range.invalidate(reason);
            self.player.stream_error.set(Some(super::stream::StreamError {
                sample:0,frame:self.player.now,kind:super::stream::StreamErrorKind::UnsupportedWindow }));
        }
        self.state.shared_ahdsr = Some(range);
        result
    }

    pub(crate) fn commit_shared_source(&mut self) -> Result<(), &'static str> {
        let Some(range) = self.state.shared_ahdsr.as_deref() else { return Ok(()); };
        if range.committed.get() { return Ok(()); }
        // Every candidate and all bindings across all Rack parts were checked
        // before the first source owner is changed.
        for voice in &mut self.player.voices {
            if let Some(source) = voice.env.shared_mut() { range.bind_active(voice.admission,source)?; }
        }
        range.committed.set(true);
        Ok(())
    }

    /// A public/direct note cannot manufacture a future receipt. Plugin ingress
    /// establishes this only after preflighting the complete source range.
    pub(super) fn shared_input_admitted(&self) -> bool {
        self.state.shared_ahdsr.as_ref().is_none_or(|range| range.sealed && range.current.get().is_some())
    }

    pub(crate) fn reject_unsealed_source(&mut self) {
        if let Some(range) = &mut self.state.shared_ahdsr {
            range.invalidate("shared AHDSR note input lacks a sealed accepted range");
            self.player.stream_error.set(Some(super::stream::StreamError {
                sample:0,frame:self.player.now,kind:super::stream::StreamErrorKind::UnsupportedWindow }));
        }
    }
}

pub(super) fn release_admitted(range: Option<&Range>, voice: &super::voice::Voice,
    now: u64, error: &Cell<Option<super::stream::StreamError>>) -> bool {
    if voice.env.shared().is_none() { return true; }
    if range.is_some_and(|range| range.off_is_sealed(voice.admission,now)) && voice.env.release_is_sealed() { return true; }
    error.set(Some(super::stream::StreamError { sample:voice.sample,frame:now,
        kind:super::stream::StreamErrorKind::UnsupportedWindow }));
    false
}
