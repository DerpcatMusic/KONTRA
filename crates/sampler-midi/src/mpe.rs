//! Fixed-zone MPE 1.1 note/expression projection after raw-event interception.
use crate::{Applied, ApplyError, Message, Packet, Value, Version};
use sampler_core::{
    ChannelScope, Error, Expression, ExpressionId, Input, NoteId, NotePitch, PerformanceId,
    Protocol, Runtime, RuntimeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Lower,
    Upper,
}
impl Zone {
    fn manager(self) -> u8 {
        match self {
            Self::Lower => 0,
            Self::Upper => 15,
        }
    }
    fn contains(self, channel: u8, members: u8) -> bool {
        channel.abs_diff(self.manager()) <= members
    }
}

#[derive(Clone, Copy)]
struct Binding {
    note: NoteId,
    channel: u8,
    member: Controls,
}

#[derive(Clone, Copy)]
struct Controls {
    pitch: f64,
    pressure: u8,
    timbre: u8,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            pitch: 0.0,
            pressure: 0,
            timbre: 64,
        }
    }
}
impl Controls {
    fn expression(self, manager: Self) -> Expression {
        Expression {
            pitch_semitones: self.pitch + manager.pitch,
            pressure: Value::Bits7(self.pressure.max(manager.pressure)).full_scale(),
            timbre: Value::Bits7(
                (i16::from(self.timbre) + i16::from(manager.timbre) - 64).clamp(0, 127) as u8,
            )
            .full_scale(),
            ..Expression::default()
        }
    }
    fn with(mut self, control: Control) -> Self {
        match control {
            Control::Pitch(value) => self.pitch = value,
            Control::Pressure(value) => self.pressure = value,
            Control::Timbre(value) => self.timbre = value,
        }
        self
    }
}
#[derive(Clone, Copy)]
enum Control {
    Pitch(f64),
    Pressure(u8),
    Timbre(u8),
}

#[derive(Clone, Copy)]
struct Parameter {
    rpn: [u8; 2],
    registered: bool,
}
impl Default for Parameter {
    fn default() -> Self {
        Self {
            rpn: [127; 2],
            registered: false,
        }
    }
}

/// One explicitly configured MIDI 1.0 zone in one runtime/port/group domain.
/// Construct and drop on the control thread. The binding budget includes tails
/// and unaccepted terminal notes. No pins are added and no heap work occurs in apply.
///
/// This is not a complete MPE receiver: MCM, fractional/relative RPN and channel modes are
/// reported Unsupported. Do not forward those messages to ordinary channel
/// ingress as a substitute for zone semantics. Raw-event consumers run before
/// this adapter; a consumed message must not be passed to apply.
pub struct Mpe {
    runtime: RuntimeId,
    performance: PerformanceId,
    port: u16,
    group: u8,
    zone: Zone,
    members: u8,
    limit: usize,
    controls: [Controls; 16],
    bends: [u16; 16],
    ranges: [u8; 2],
    parameters: [Parameter; 16],
    bindings: Vec<Binding>,
    changes: Vec<(ExpressionId, Expression)>,
}
impl Mpe {
    pub fn new(
        runtime: &Runtime,
        port: u16,
        group: u8,
        zone: Zone,
        members: u8,
        notes: usize,
    ) -> Result<Self, Error> {
        Self::new_in(
            runtime,
            runtime.performance(0)?,
            port,
            group,
            zone,
            members,
            notes,
        )
    }

    /// Bind all expressive members to one explicit musical routing domain.
    pub fn new_in(
        runtime: &Runtime,
        performance: PerformanceId,
        port: u16,
        group: u8,
        zone: Zone,
        members: u8,
        notes: usize,
    ) -> Result<Self, Error> {
        runtime.articulation(performance)?;
        if group >= 16 || !(1..=15).contains(&members) || notes == 0 {
            return Err(Error::InvalidInput);
        }
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(notes)
            .map_err(|_| Error::Capacity)?;
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(notes)
            .map_err(|_| Error::Capacity)?;
        Ok(Self {
            runtime: runtime.id(),
            performance,
            port,
            group,
            zone,
            members,
            limit: notes,
            controls: [Controls::default(); 16],
            bends: [8192; 16],
            ranges: [2, 48],
            parameters: [Parameter::default(); 16],
            bindings,
            changes,
        })
    }

    /// Whole-semitone manager and shared member bend sensitivities.
    pub fn pitch_ranges(&self) -> (u8, u8) {
        (self.ranges[0], self.ranges[1])
    }

    /// Apply at the current sample boundary, after due native work. Only this
    /// adapter should admit external notes in its configured input domain.
    pub fn apply(
        &mut self,
        runtime: &mut Runtime,
        packet: Packet<'_>,
    ) -> Result<Applied, ApplyError> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        let Some(voice) = packet.channel_voice() else {
            return Ok(Applied::Unsupported);
        };
        if voice.group != self.group {
            return Err(ApplyError::DisabledGroup);
        }
        if voice.version != Version::Midi1 {
            return Err(ApplyError::ProtocolMismatch);
        }
        if !self.zone.contains(voice.channel, self.members) {
            return Ok(Applied::Unsupported);
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let input = Input {
            protocol: Protocol::Midi1,
            port: self.port,
            group: self.group,
            channel: voice.channel,
            key: 0,
            external_id: None,
        };
        Ok(match voice.message {
            Message::NoteOn {
                key,
                velocity,
                attribute,
            } if attribute.kind == 0 => {
                if self.bindings.len() == self.limit {
                    return Err(Error::Capacity.into());
                }
                let member = if voice.channel == self.zone.manager() {
                    Controls::default()
                } else {
                    self.controls[usize::from(voice.channel)]
                };
                let expression = member.expression(self.controls[usize::from(self.zone.manager())]);
                let note = runtime.trigger_in(
                    self.performance,
                    Input { key, ..input },
                    NotePitch::Key(key),
                    velocity.normalized(),
                    expression,
                )?;
                self.bindings.push(Binding {
                    note,
                    channel: voice.channel,
                    member,
                });
                Applied::Started(note)
            }
            Message::NoteOff {
                key,
                velocity,
                attribute,
            } => Applied::Released {
                note: runtime.note_off_in(
                    self.performance,
                    Input { key, ..input },
                    velocity.map(crate::Value::normalized),
                )?,
                velocity,
                attribute,
            },
            Message::PitchBend(Value::Bits14(value)) => {
                let range = self.ranges[usize::from(voice.channel != self.zone.manager())];
                let applied = self.control(
                    runtime,
                    voice.channel,
                    Control::Pitch(pitch(value, f64::from(range))),
                )?;
                self.bends[usize::from(voice.channel)] = value;
                applied
            }
            Message::ChannelPressure(Value::Bits7(value)) => {
                self.control(runtime, voice.channel, Control::Pressure(value))?
            }
            Message::Control {
                index: index @ (64 | 66),
                value: Value::Bits7(value),
            } => {
                if voice.channel != self.zone.manager() {
                    Applied::Ignored
                } else {
                    let width = (1u32 << (self.members + 1)) - 1;
                    let channels = match self.zone {
                        Zone::Lower => width as u16,
                        Zone::Upper => (width << (15 - self.members)) as u16,
                    };
                    let scope = ChannelScope {
                        protocol: Protocol::Midi1,
                        port: self.port,
                        group: self.group,
                        channels,
                    };
                    runtime.set_pedal_controller(
                        self.performance,
                        scope,
                        index,
                        Value::Bits7(value).full_scale(),
                    )?;
                    Applied::Pedal
                }
            }
            Message::Control {
                index: 74,
                value: Value::Bits7(value),
            } => self.control(runtime, voice.channel, Control::Timbre(value))?,
            Message::Control {
                index: index @ (6 | 38 | 98..=101),
                value: Value::Bits7(value),
            } => self.parameter(runtime, voice.channel, index, value)?,
            Message::Control { index, value }
                if index < 120
                    && !matches!(index, 96 | 97)
                    && voice.channel == self.zone.manager() =>
            {
                runtime.set_controller(self.performance, index, value.full_scale())?;
                Applied::Controller
            }
            _ => Applied::Unsupported,
        })
    }

    fn control(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        control: Control,
    ) -> Result<Applied, Error> {
        let mut controls = self.controls;
        controls[usize::from(channel)] = controls[usize::from(channel)].with(control);
        self.project(runtime, controls, Some(channel), control)
    }

    fn project(
        &mut self,
        runtime: &mut Runtime,
        controls: [Controls; 16],
        channel: Option<u8>,
        control: Control,
    ) -> Result<Applied, Error> {
        let manager = channel == Some(self.zone.manager());
        let manager_controls = controls[usize::from(self.zone.manager())];
        self.changes.clear();
        for binding in &self.bindings {
            let active_member = binding.channel != self.zone.manager()
                && channel.is_none_or(|channel| binding.channel == channel)
                && runtime.input_held(binding.note)?;
            if manager || active_member {
                let owner = runtime.expression_id(binding.note)?;
                let mut expression = runtime.expression(owner)?;
                let member = if !manager && active_member {
                    controls[usize::from(binding.channel)]
                } else {
                    binding.member
                };
                let combined = member.expression(manager_controls);
                // Each gesture owns only its dimension. Preserve native gain,
                // pan and all expression fields not addressed by this control.
                match control {
                    Control::Pitch(_) => expression.pitch_semitones = combined.pitch_semitones,
                    Control::Pressure(_) => expression.pressure = combined.pressure,
                    Control::Timbre(_) => expression.timbre = combined.timbre,
                }
                self.changes.push((owner, expression));
            }
        }
        // Controller state and per-note snapshots commit only after every owner
        // accepts the gesture. Rejected pitch must not leak into the next note.
        runtime.set_expressions(&self.changes)?;
        self.controls = controls;
        if !manager {
            for binding in &mut self.bindings {
                if binding.channel != self.zone.manager()
                    && channel.is_none_or(|channel| binding.channel == channel)
                    && runtime.input_held(binding.note)?
                {
                    binding.member = controls[usize::from(binding.channel)];
                }
            }
        }
        Ok(Applied::Expression {
            owners: self.changes.len(),
        })
    }
    fn parameter(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        index: u8,
        value: u8,
    ) -> Result<Applied, Error> {
        let parameter = &mut self.parameters[usize::from(channel)];
        match index {
            100 | 101 => {
                parameter.rpn[usize::from(index == 100)] = value;
                parameter.registered = true;
                Ok(Applied::Configuration)
            }
            98 | 99 => {
                parameter.registered = false;
                Ok(Applied::Configuration)
            }
            6 if parameter.registered && parameter.rpn == [0, 0] => {
                self.range(runtime, channel, value)
            }
            38 if parameter.registered && parameter.rpn == [0, 0] && value == 0 => {
                Ok(Applied::Configuration)
            }
            _ => Ok(Applied::Unsupported),
        }
    }

    fn range(&mut self, runtime: &mut Runtime, channel: u8, value: u8) -> Result<Applied, Error> {
        if value > 96 {
            return Err(Error::InvalidInput);
        }
        let manager = channel == self.zone.manager();
        let mut controls = self.controls;
        for (index, control) in controls.iter_mut().enumerate() {
            let channel = index as u8;
            if (manager && channel == self.zone.manager())
                || (!manager
                    && channel != self.zone.manager()
                    && self.zone.contains(channel, self.members))
            {
                control.pitch = pitch(self.bends[index], f64::from(value));
            }
        }
        let applied = self.project(
            runtime,
            controls,
            manager.then_some(channel),
            Control::Pitch(0.0),
        )?;
        self.ranges[usize::from(!manager)] = value;
        Ok(applied)
    }
}

fn pitch(value: u16, range: f64) -> f64 {
    let centered = f64::from(value) - 8192.0;
    // Exact center and endpoints; MPE permits meaningful receiver combination.
    range * centered / if value < 8192 { 8192.0 } else { 8191.0 }
}
