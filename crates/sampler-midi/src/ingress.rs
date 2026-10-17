use crate::{Attribute, Controllers, Message, Packet, Value, Version};
use sampler_core::{Error, Expression, Input, NoteId, NotePitch, PerformanceId, Protocol, Runtime};

/// Protocol selection belongs to the connection/control plane, never inferred from
/// incoming notes. Disabled groups and protocol mismatches cannot mutate the core.
///
/// Plain (non-MPE) channel pitch bend and channel/poly pressure become the
/// per-note expression of every note admitted from that channel or key, the
/// same core path MPE uses, so native pressure/timbre defaults and IR routes
/// apply. New notes start at the channel's current bend and pressure. Bend
/// range is RPN 0 on the channel, else the instrument's default.
pub struct Ingress {
    port: u16,
    groups: [Option<Version>; 16],
    /// Indexed by group * 16 + channel.
    channels: [Channel; 256],
    /// The user's bend range in semitones; beats RPN 0 and the instrument.
    bend_override: Option<f64>,
    polyphonic: Option<Polyphonic>,
}

struct Polyphonic {
    runtime: sampler_core::RuntimeId,
    limit: usize,
    owners: Vec<PerNoteOwner>,
    changes: Vec<(sampler_core::ExpressionId, Expression)>,
    retained: Vec<(usize, crate::per_note::Snapshot)>,
    controls: crate::per_note::PerNoteState,
}
#[derive(Clone, Copy)]
struct PerNoteOwner {
    note: NoteId,
    address: sampler_core::ChannelAddress,
    key: u8,
    state: crate::per_note::Snapshot,
}

#[derive(Clone, Copy)]
struct Channel {
    /// Normalized bend, -1..=1.
    bend: f64,
    pressure: u32,
    /// RPN 0 override in semitones.
    range: Option<f64>,
    /// Selected RPN (MSB, LSB); [127, 127] is null.
    rpn: [u8; 2],
}
impl Default for Channel {
    fn default() -> Self {
        Self {
            bend: 0.0,
            pressure: 0,
            range: None,
            rpn: [127; 2],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyError {
    DisabledGroup,
    ProtocolMismatch,
    Core(Error),
}
impl From<Error> for ApplyError {
    fn from(value: Error) -> Self {
        Self::Core(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Started(NoteId),
    Expression {
        owners: usize,
    },
    Released {
        note: NoteId,
        velocity: Option<Value>,
        attribute: Attribute,
    },
    Pedal,
    /// Accepted effective selection CC; no implicit note or modulation is generated.
    Controller,
    /// A controller selector/state change, not a claim of full device configuration.
    Configuration,
    AllNotesOff {
        released: usize,
    },
    AllSoundOff {
        stopped: usize,
    },
    /// Reset All Controllers (CC121): the channel's controllers are back at
    /// their RP-015 reset values.
    ResetControllers,
    /// A recognized message intentionally has no effect in this receiver mode.
    Ignored,
    Unsupported,
}

impl Ingress {
    pub fn new(port: u16, groups: [Option<Version>; 16]) -> Self {
        Self {
            port,
            groups,
            channels: [Channel::default(); 256],
            bend_override: None,
            polyphonic: None,
        }
    }

    /// Full MIDI2 note-number receiver with explicitly prepared ownership and
    /// protocol-state capacity. The existing new() remains channel-only and
    /// reports PN messages Unsupported; neither constructor allocates in apply.
    pub fn new_polyphonic(runtime: &Runtime, port: u16, groups: [Option<Version>; 16], notes: usize) -> Result<Self, Error> {
        if notes == 0 { return Err(Error::InvalidInput); }
        let owner_bytes = std::alloc::Layout::array::<PerNoteOwner>(notes).map_err(|_| Error::Capacity)?.size();
        let change_bytes = std::alloc::Layout::array::<(sampler_core::ExpressionId, Expression)>(notes).map_err(|_| Error::Capacity)?.size();
        let retained_bytes = std::alloc::Layout::array::<(usize, crate::per_note::Snapshot)>(notes).map_err(|_| Error::Capacity)?.size();
        owner_bytes.checked_add(change_bytes).and_then(|n| n.checked_add(retained_bytes)).ok_or(Error::Capacity)?;
        let mut group_mask = 0;
        for (index, version) in groups.iter().enumerate() { if *version == Some(Version::Midi2) { group_mask |= 1 << index; } }
        owner_bytes.checked_add(change_bytes).and_then(|n|n.checked_add(retained_bytes))
            .and_then(|n|n.checked_add(crate::per_note::PerNoteState::required_bytes(group_mask).ok()?)).ok_or(Error::Capacity)?;
        let controls = crate::per_note::PerNoteState::new(group_mask)?;
        let mut owners = Vec::new(); owners.try_reserve_exact(notes).map_err(|_| Error::Capacity)?;
        let mut changes = Vec::new(); changes.try_reserve_exact(notes).map_err(|_| Error::Capacity)?;
        let mut retained = Vec::new(); retained.try_reserve_exact(notes).map_err(|_| Error::Capacity)?;
        let bytes=std::alloc::Layout::array::<PerNoteOwner>(owners.capacity()).map_err(|_|Error::Capacity)?.size();
        let bytes=bytes.checked_add(std::alloc::Layout::array::<(sampler_core::ExpressionId,Expression)>(changes.capacity()).map_err(|_|Error::Capacity)?.size()).ok_or(Error::Capacity)?;
        bytes.checked_add(std::alloc::Layout::array::<(usize,crate::per_note::Snapshot)>(retained.capacity()).map_err(|_|Error::Capacity)?.size()).and_then(|n|n.checked_add(controls.retained_bytes().ok()?)).ok_or(Error::Capacity)?;
        let mut receiver = Self::new(port, groups);
        receiver.polyphonic = Some(Polyphonic { runtime: runtime.id(), limit: notes, owners, changes, retained, controls });
        Ok(receiver)
    }

    /// Fix the bend range in semitones for every channel, whatever RPN 0 says;
    /// `None` returns to RPN 0, else the instrument's own. Held bends keep
    /// their pitch until the next bend message.
    pub fn set_bend_range(&mut self, semitones: Option<f64>) {
        self.bend_override = semitones.filter(|r| (0.0..=96.0).contains(r));
    }

    /// Apply at Runtime::now(). The host must split rendering at event timestamps.
    /// Note attributes support absolute Pitch 7.9 in addition to ordinary notes. Other
    /// decoded messages are reported as unsupported, not silently approximated.
    pub fn apply(
        &mut self,
        runtime: &mut Runtime,
        packet: Packet<'_>,
    ) -> Result<Applied, ApplyError> {
        self.apply_in(runtime, runtime.performance(0)?, packet)
    }

    /// Route note pairing/selection to a musical domain. Pedals/channel modes retain
    /// their declared physical channel scope, independent of articulation routing.
    pub fn apply_in(
        &mut self,
        runtime: &mut Runtime,
        performance: PerformanceId,
        packet: Packet<'_>,
    ) -> Result<Applied, ApplyError> {
        runtime.articulation(performance)?;
        let Some(voice) = packet.channel_voice() else {
            return Ok(Applied::Unsupported);
        };
        let configured = self.groups[usize::from(voice.group)].ok_or(ApplyError::DisabledGroup)?;
        if configured != voice.version {
            return Err(ApplyError::ProtocolMismatch);
        }
        if let Some(polyphonic) = self.polyphonic.as_mut() {
            if runtime.id() != polyphonic.runtime { return Err(Error::StaleHandle.into()); }
            runtime.render(&mut [])?;
            polyphonic.owners.retain(|owner| runtime.note(owner.note).is_ok());
        }
        let slot = usize::from(voice.group) * 16 + usize::from(voice.channel);
        let channel = self.channels[slot];
        let range = self
            .bend_override
            .or(channel.range)
            .unwrap_or_else(|| runtime.bend_range());
        let input = Input {
            protocol: match voice.version {
                Version::Midi1 => Protocol::Midi1,
                Version::Midi2 => Protocol::Midi2,
            },
            port: self.port,
            group: voice.group,
            channel: voice.channel,
            key: 0,
            external_id: None,
        };
        Ok(match voice.message {
            Message::NoteOn {
                key,
                velocity,
                attribute,
            } => {
                let state = if voice.version == Version::Midi2 {
                    self.polyphonic.as_ref().map(|polyphonic| polyphonic.controls.snapshot(voice.group, voice.channel, key)).transpose()?
                } else { None };
                if state.is_some() && self.polyphonic.as_ref().is_some_and(|polyphonic| polyphonic.owners.len() == polyphonic.limit) {
                    return Err(Error::Capacity.into());
                }
                let note = runtime.trigger_in(performance, Input { key, ..input },
                    if attribute.kind == 3 { NotePitch::Absolute(f64::from(attribute.data) / 512.) } else { NotePitch::Key(key) },
                    velocity.normalized(), Expression { pitch_semitones: channel.bend * range + state.map_or(0., |state| state.pitch),
                        bend: channel.bend, pressure: channel.pressure, ..Expression::default() })?;
                if let (Some(polyphonic), Some(state)) = (self.polyphonic.as_mut(), state) {
                    polyphonic.owners.push(PerNoteOwner { note, address: input.channel_address(), key, state });
                }
                Applied::Started(note)
            },
            Message::NoteOff {
                key,
                velocity,
                attribute,
            } => {
                // Unknown release attributes must not strand a sounding note.
                let note = runtime.note_off_in(
                    performance,
                    Input { key, ..input },
                    velocity.map(crate::Value::normalized),
                )?;
                Applied::Released {
                    note,
                    velocity,
                    attribute,
                }
            }
            Message::PitchBend(value) => {
                let bend = bend(value);
                let preserve_note = self.polyphonic.is_some() && voice.version == Version::Midi2;
                let owners = runtime.set_input_expressions(input.channel_address(), None, |e| {
                    Expression {
                        pitch_semitones: if preserve_note { e.pitch_semitones + (bend - channel.bend) * range } else { bend * range },
                        bend,
                        ..e
                    }
                })?;
                self.channels[slot].bend = bend;
                Applied::Expression { owners }
            }
            Message::ChannelPressure(value) => {
                let pressure = value.full_scale();
                let owners = runtime.set_input_expressions(input.channel_address(), None, |e| {
                    Expression { pressure, ..e }
                })?;
                self.channels[slot].pressure = pressure;
                Applied::Expression { owners }
            }
            Message::PolyPressure { key, value } => {
                let pressure = value.full_scale();
                let (_, owners) = runtime.dispatch_poly_pressure(performance, input.channel_address(), key, pressure)?;
                Applied::Expression {
                    owners,
                }
            }
            Message::PerNotePitch { key, value } if voice.version == Version::Midi2 && self.polyphonic.is_some() => {
                self.per_note_bend(runtime, input.channel_address(), key, value)?
            }
            Message::PerNoteManagement { key, detach, reset } if voice.version == Version::Midi2 && self.polyphonic.is_some() => {
                self.per_note_management(runtime, input.channel_address(), key, detach, reset)?
            }
            Message::ChannelControl { space: Controllers::Registered, bank: 0, index: 7, value }
                if voice.version == Version::Midi2 && self.polyphonic.is_some() => {
                self.per_note_range(runtime, input.channel_address(), f64::from(value) / 33_554_432.)?
            }
            Message::ChannelControl {
                space: Controllers::Registered,
                bank: 0,
                index: 0,
                value,
            } => {
                // MIDI 2.0 RPN 0: semitones in the top 7 bits, cents in the next 7.
                let range = f64::from(value >> 25) + f64::from((value >> 18) & 127) / 100.0;
                self.set_range(runtime, slot, input.channel_address(), range)?;
                Applied::Configuration
            }
            Message::Control {
                index: index @ (64 | 66),
                value,
            } => {
                runtime.dispatch_controller(
                    performance,
                    input.channel_address(),
                    1 << input.channel,
                    index,
                    value.full_scale(),
                )?;
                Applied::Pedal
            }
            Message::Control { index, .. }
                if voice.version == Version::Midi2 && crate::is_midi2_legacy_control(index) =>
            {
                Applied::Ignored
            }
            Message::Control { index, value } if index < 120 => {
                runtime.dispatch_controller(
                    performance,
                    input.channel_address(),
                    1 << input.channel,
                    index,
                    value.full_scale(),
                )?;
                // Track RPN 0 (bend range) after scripts and routes saw the CC.
                let data = (value.full_scale() >> 25) as u8;
                let rpn = &mut self.channels[slot].rpn;
                match index {
                    101 => rpn[0] = data,
                    100 => rpn[1] = data,
                    98 | 99 => *rpn = [127; 2],
                    6 if *rpn == [0, 0] => {
                        self.set_range(runtime, slot, input.channel_address(), f64::from(data))?;
                    }
                    38 if *rpn == [0, 0] => {
                        let semitones = range.trunc();
                        let range = semitones + f64::from(data) / 100.0;
                        self.set_range(runtime, slot, input.channel_address(), range)?;
                    }
                    _ => {}
                }
                Applied::Controller
            }
            Message::Control {
                index: 123,
                value: Value::Bits7(0) | Value::Bits32(0),
            } => Applied::AllNotesOff {
                released: runtime.all_notes_off(input.channel_address())?,
            },
            Message::Control {
                index: 121,
                value: Value::Bits7(0) | Value::Bits32(0),
            } => {
                // Drain the existing immediate boundary before computing the
                // entire proposed reset. No six fabricated controller inputs.
                runtime.render(&mut [])?;
                let scope=sampler_core::ChannelScope{protocol:input.protocol,port:input.port,group:input.group,channels:1<<input.channel};
                runtime.validate_protocol_controller_reset_targets(performance,scope,None)?;
                let preserve_note=self.polyphonic.is_some()&&voice.version==Version::Midi2;
                let update=|e:Expression|Expression{
                    pitch_semitones:if preserve_note {e.pitch_semitones-channel.bend*range} else {0.},
                    bend:0.,pressure:0,..e};
                runtime.validate_input_expressions(input.channel_address(),None,update)?;
                runtime.set_input_expressions(input.channel_address(),None,update)?;
                // Expressions cannot change controller row admission/epochs.
                // The checked selective commit publishes every target bank
                // before pedal-up can invoke a fresh release callback.
                runtime.reset_protocol_controller_targets(performance,scope,None)?;
                let channel=&mut self.channels[slot];
                (channel.bend,channel.pressure,channel.rpn)=(0.,0,[127;2]);
                Applied::ResetControllers
            }
            Message::Control {
                index: 120,
                value: Value::Bits7(0) | Value::Bits32(0),
            } => Applied::AllSoundOff {
                stopped: runtime.all_sound_off(input.channel_address())?,
            },
            _ => Applied::Unsupported,
        })
    }
}

impl Ingress {
    fn per_note_bend(&mut self, runtime: &mut Runtime, address: sampler_core::ChannelAddress, key: u8, value: u32) -> Result<Applied, Error> {
        let polyphonic = self.polyphonic.as_mut().ok_or(Error::InvalidInput)?;
        Self::project_note_bend(polyphonic, runtime, address, key, value)
    }

    fn project_note_bend(polyphonic: &mut Polyphonic, runtime: &mut Runtime, address: sampler_core::ChannelAddress, key: u8, bend: u32) -> Result<Applied, Error> {
        let (id, before) = polyphonic.controls.key(address.group, address.channel, key)?;
        let channel = polyphonic.controls.channel(address.group, address.channel)?;
        let next = crate::per_note::KeyState { bend, ..before };
        let projected = crate::per_note::PerNoteState::project(next, polyphonic.controls.range(channel));
        polyphonic.changes.clear(); polyphonic.retained.clear();
        for (index, owner) in polyphonic.owners.iter().enumerate() {
            if owner.address != address || owner.key != key || owner.state.cohort != before.cohort { continue; }
            let expression = runtime.expression_id(owner.note)?; let mut value = runtime.expression(expression)?;
            value.pitch_semitones += projected.pitch - owner.state.pitch;
            polyphonic.changes.push((expression, value)); polyphonic.retained.push((index, projected));
        }
        runtime.set_expressions(&polyphonic.changes)?;
        for (index, state) in polyphonic.retained.drain(..) { polyphonic.owners[index].state = state; }
        polyphonic.controls.commit_key(id, next);
        Ok(Applied::Expression { owners: polyphonic.changes.len() })
    }

    fn per_note_management(&mut self, runtime: &mut Runtime, address: sampler_core::ChannelAddress, key: u8, detach: bool, reset: bool) -> Result<Applied, Error> {
        if !detach && !reset { return Ok(Applied::Ignored); }
        let polyphonic = self.polyphonic.as_mut().ok_or(Error::InvalidInput)?;
        let (id, before) = polyphonic.controls.key(address.group, address.channel, key)?;
        let next = crate::per_note::PerNoteState::managed(before, detach, reset)?;
        if !detach && reset { Self::project_note_bend(polyphonic, runtime, address, key, crate::per_note::CENTER)?; }
        // Combined D+S changes only the future cohort; previous owners freeze.
        polyphonic.controls.commit_key(id, next);
        Ok(Applied::Configuration)
    }

    fn per_note_range(&mut self, runtime: &mut Runtime, address: sampler_core::ChannelAddress, range: f64) -> Result<Applied, Error> {
        let polyphonic = self.polyphonic.as_mut().ok_or(Error::InvalidInput)?;
        let channel = polyphonic.controls.channel(address.group, address.channel)?;
        polyphonic.changes.clear(); polyphonic.retained.clear();
        for (index, owner) in polyphonic.owners.iter().enumerate() {
            if owner.address != address { continue; }
            let (_, key) = polyphonic.controls.key(address.group, address.channel, owner.key)?;
            if owner.state.cohort != key.cohort { continue; }
            let next = crate::per_note::PerNoteState::project(key, range);
            let expression = runtime.expression_id(owner.note)?; let mut value = runtime.expression(expression)?;
            value.pitch_semitones += next.pitch - owner.state.pitch;
            polyphonic.changes.push((expression, value)); polyphonic.retained.push((index, next));
        }
        runtime.set_expressions(&polyphonic.changes)?;
        for (index, state) in polyphonic.retained.drain(..) { polyphonic.owners[index].state = state; }
        polyphonic.controls.commit_range(channel, range);
        Ok(Applied::Configuration)
    }

    /// Re-project the channel's held bend at a new range, then keep it.
    fn set_range(
        &mut self,
        runtime: &mut Runtime,
        slot: usize,
        address: sampler_core::ChannelAddress,
        range: f64,
    ) -> Result<(), Error> {
        // Out-of-spec sensitivities are ignored, as hardware receivers do.
        if range > 96.0 {
            return Ok(());
        }
        if self.bend_override.is_none() {
            let bend = self.channels[slot].bend;
            let previous = self.channels[slot].range.unwrap_or_else(|| runtime.bend_range());
            let preserve_note = self.polyphonic.is_some() && address.protocol == Protocol::Midi2;
            runtime.set_input_expressions(address, None, |e| Expression {
                pitch_semitones: if preserve_note { e.pitch_semitones + bend * (range - previous) } else { bend * range },
                bend,
                ..e
            })?;
        }
        // Retain a protocol configuration only after its current musical
        // projection accepts. An explicit override intentionally stores the
        // RPN for later without changing held expression, as before.
        self.channels[slot].range = Some(range);
        Ok(())
    }
}

/// Normalized bend with an exact centre and endpoints.
fn bend(value: Value) -> f64 {
    let (value, centre) = match value {
        Value::Bits7(v) => (f64::from(v), 64.0),
        Value::Bits14(v) => (f64::from(v), 8192.0),
        Value::Bits16(v) => (f64::from(v), 32768.0),
        Value::Bits32(v) => (f64::from(v), 2_147_483_648.0),
    };
    let centered = value - centre;
    centered / if centered < 0.0 { centre } else { centre - 1.0 }
}

/// A framed packet at a sample offset within the current host block.
#[derive(Clone, Copy, Debug)]
pub struct TimedPacket<'a> {
    pub offset: usize,
    pub packet: Packet<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    EventBudget,
    InvalidOffset { index: usize },
    Core(Error),
}
impl From<Error> for BlockError {
    fn from(value: Error) -> Self {
        Self::Core(value)
    }
}

impl Ingress {
    /// Render a sorted, bounded batch. Validate the entire timing envelope before
    /// mutating audio or runtime. Equal offsets preserve input order. Offset ==
    /// block length belongs to the next block, except offset zero in an empty block.
    /// Internal work already queued at a boundary runs before external input there.
    /// Musical admission failures are reported individually; later events still run.
    /// The caller owns outcome handling and must keep `report` realtime-safe.
    pub fn render(
        &mut self,
        runtime: &mut Runtime,
        output: &mut [sampler_core::Frame],
        events: &[TimedPacket<'_>],
        event_limit: usize,
        mut report: impl FnMut(usize, Result<Applied, ApplyError>),
    ) -> Result<(), BlockError> {
        if events.len() > event_limit {
            return Err(BlockError::EventBudget);
        }
        runtime
            .now()
            .checked_add(output.len() as u64)
            .ok_or(Error::ClockOverflow)?;
        let mut previous = 0;
        for (index, event) in events.iter().enumerate() {
            if event.offset < previous || event.offset >= output.len().max(1) {
                return Err(BlockError::InvalidOffset { index });
            }
            previous = event.offset;
        }
        let mut offset = 0;
        for (index, event) in events.iter().enumerate() {
            runtime.render(&mut output[offset..event.offset])?;
            runtime.render(&mut [])?;
            report(index, self.apply(runtime, event.packet));
            offset = event.offset;
        }
        runtime.render(&mut output[offset..])?;
        Ok(())
    }
}
