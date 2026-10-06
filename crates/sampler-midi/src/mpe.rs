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

/// Pressure and timbre at 32 bits (MIDI 1.0 values upscaled, MIDI 2.0 as sent).
#[derive(Clone, Copy)]
struct Controls {
    pitch: f64,
    /// Raw bend position, -1..=1.
    bend: f64,
    pressure: u32,
    timbre: Value,
}
const CENTER: u32 = 0x8000_0000;
impl Default for Controls {
    fn default() -> Self {
        Self {
            pitch: 0.0,
            bend: 0.0,
            pressure: 0,
            timbre: Value::Bits7(64),
        }
    }
}
impl Controls {
    fn expression(self, manager: Self) -> Expression {
        // Offsets from centre add; MIDI 1.0 pairs combine at their own 7 bits.
        let timbre = match (self.timbre, manager.timbre) {
            (Value::Bits7(a), Value::Bits7(b)) => {
                Value::Bits7((i16::from(a) + i16::from(b) - 64).clamp(0, 127) as u8).full_scale()
            }
            (a, b) => {
                let sum = i64::from(a.full_scale()) + i64::from(b.full_scale()) - i64::from(CENTER);
                sum.clamp(0, i64::from(u32::MAX)) as u32
            }
        };
        Expression {
            pitch_semitones: self.pitch + manager.pitch,
            bend: (self.bend + manager.bend).clamp(-1.0, 1.0),
            pressure: self.pressure.max(manager.pressure),
            timbre,
            ..Expression::default()
        }
    }
    fn with(mut self, control: Control) -> Self {
        match control {
            Control::Pitch(value, bend) => (self.pitch, self.bend) = (value, bend),
            Control::Pressure(value) => self.pressure = value,
            Control::Timbre(value) => self.timbre = value,
        }
        self
    }
}
#[derive(Clone, Copy)]
enum Control {
    /// Semitones and the raw bend position.
    Pitch(f64, f64),
    Pressure(u32),
    Timbre(Value),
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

/// One explicitly configured zone in one runtime/port/group domain. MIDI 1.0
/// and 2.0 channel voice messages both play it; 2.0 values keep their
/// precision (velocity, controllers, pressure, pitch bend).
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
    bends: [Value; 16],
    ranges: [u8; 2],
    parameters: [Parameter; 16],
    bindings: Vec<Binding>,
    changes: Vec<(ExpressionId, Expression)>,
    transpose: f64,
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
            bends: [Value::Bits14(8192); 16],
            ranges: [2, 48],
            parameters: [Parameter::default(); 16],
            bindings,
            changes,
            transpose: 0.0,
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
        let midi2 = voice.version == Version::Midi2;
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
            } if attribute.kind == 0 => Applied::Started(self.admit(
                runtime,
                voice.channel,
                Input { key, ..input },
                // A MIDI 2.0 velocity of 0 still starts a note.
                if midi2 {
                    velocity.normalized().max(1.0 / 65535.0)
                } else {
                    velocity.normalized()
                },
            )?),
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
            Message::PitchBend(value @ (Value::Bits14(_) | Value::Bits32(_))) => {
                let range = self.ranges[usize::from(voice.channel != self.zone.manager())];
                let applied = self.control(
                    runtime,
                    voice.channel,
                    Control::Pitch(pitch(value, f64::from(range)), pitch(value, 1.0)),
                )?;
                self.bends[usize::from(voice.channel)] = value;
                applied
            }
            Message::ChannelPressure(value @ (Value::Bits7(_) | Value::Bits32(_))) => self
                .control(
                    runtime,
                    voice.channel,
                    Control::Pressure(value.full_scale()),
                )?,
            // MIDI 2.0 Registered Controller 0:0, pitch bend sensitivity
            // (semitones in the top 7 bits).
            Message::ChannelControl {
                space: crate::Controllers::Registered,
                bank: 0,
                index: 0,
                value,
            } => self.range(runtime, voice.channel, (value >> 25) as u8)?,
            Message::Control {
                index: index @ (64 | 66),
                value: value @ (Value::Bits7(_) | Value::Bits32(_)),
            } => {
                if voice.channel != self.zone.manager() {
                    Applied::Ignored
                } else {
                    let scope = self.manager_scope();
                    runtime.dispatch_controller(
                        self.performance,
                        input.channel_address(),
                        scope.channels,
                        index,
                        value.full_scale(),
                    )?;
                    Applied::Pedal
                }
            }
            Message::Control {
                index: 74,
                value: value @ (Value::Bits7(_) | Value::Bits32(_)),
            } => self.control(runtime, voice.channel, Control::Timbre(value))?,
            // MIDI 2.0 carries RPNs as Registered Controllers, not CC 6/38/98..=101.
            Message::Control {
                index: index @ (6 | 38 | 98..=101),
                value: Value::Bits7(value),
            } => self.parameter(runtime, voice.channel, index, value)?,
            Message::Control { index, value }
                if index < 120
                    && !matches!(index, 96 | 97)
                    && voice.channel == self.zone.manager() =>
            {
                runtime.dispatch_controller(
                    self.performance,
                    input.channel_address(),
                    self.manager_scope().channels,
                    index,
                    value.full_scale(),
                )?;
                Applied::Controller
            }
            _ => Applied::Unsupported,
        })
    }

    /// Admit a note whose identity another transport owns (a host note ID) as
    /// if it arrived on `channel` of this zone, so the zone's pedals and gestures
    /// reach it. `input` must lie in this zone's MIDI 1.0 port and group for
    /// pedals to hold it; its `external_id` keeps it distinct from wire notes.
    pub fn trigger(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        velocity: f64,
    ) -> Result<NoteId, ApplyError> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        if !self.zone.contains(channel, self.members) {
            return Err(Error::InvalidInput.into());
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        Ok(self.admit(runtime, channel, input, velocity)?)
    }

    /// Offset every note of the zone, held or not, by `semitones` on top of its
    /// bends (a part's tuning). Commits only if every owner accepts it.
    pub fn transpose(
        &mut self,
        runtime: &mut Runtime,
        semitones: f64,
    ) -> Result<Applied, ApplyError> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        if !semitones.is_finite() {
            return Err(Error::InvalidInput.into());
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let previous = std::mem::replace(&mut self.transpose, semitones);
        let manager = Some(self.zone.manager());
        self.project(runtime, self.controls, manager, Control::Pitch(0.0, 0.0))
            .inspect_err(|_| self.transpose = previous)
            .map_err(Into::into)
    }

    fn admit(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        velocity: f64,
    ) -> Result<NoteId, Error> {
        if self.bindings.len() == self.limit {
            return Err(Error::Capacity);
        }
        let member = if channel == self.zone.manager() {
            Controls::default()
        } else {
            self.controls[usize::from(channel)]
        };
        let mut expression = member.expression(self.controls[usize::from(self.zone.manager())]);
        expression.pitch_semitones += self.transpose;
        let note = runtime.trigger_in(
            self.performance,
            input,
            NotePitch::Key(input.key),
            velocity,
            expression,
        )?;
        self.bindings.push(Binding {
            note,
            channel,
            member,
        });
        Ok(note)
    }

    fn manager_scope(&self) -> ChannelScope {
        let width = (1u32 << (self.members + 1)) - 1;
        ChannelScope {
            protocol: Protocol::Midi1,
            port: self.port,
            group: self.group,
            channels: match self.zone {
                Zone::Lower => width as u16,
                Zone::Upper => (width << (15 - self.members)) as u16,
            },
        }
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
                let mut combined = member.expression(manager_controls);
                combined.pitch_semitones += self.transpose;
                // Each gesture owns only its dimension. Preserve native gain,
                // pan and all expression fields not addressed by this control.
                match control {
                    Control::Pitch(..) => {
                        expression.pitch_semitones = combined.pitch_semitones;
                        expression.bend = combined.bend;
                    }
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
            Control::Pitch(0.0, 0.0),
        )?;
        self.ranges[usize::from(!manager)] = value;
        Ok(applied)
    }
}

fn pitch(value: Value, range: f64) -> f64 {
    let (value, center) = match value {
        Value::Bits32(v) => (f64::from(v), f64::from(CENTER)),
        Value::Bits14(v) => (f64::from(v), 8192.0),
        other => (f64::from(other.full_scale()), f64::from(CENTER)),
    };
    let centered = value - center;
    // Exact center and endpoints; MPE permits meaningful receiver combination.
    range * centered / if centered < 0.0 { center } else { center - 1.0 }
}
