//! Fixed-zone MPE 1.1 note/expression projection after raw-event interception.
use crate::{Applied, ApplyError, Message, Packet, Value, Version};
use sampler_core::{Error, Expression, ExpressionId, Input, NoteId, Protocol, Runtime, RuntimeId};

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
            pressure: expand(self.pressure.max(manager.pressure)),
            timbre: expand(
                (i16::from(self.timbre) + i16::from(manager.timbre) - 64).clamp(0, 127) as u8,
            ),
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

/// One explicitly configured MIDI 1.0 zone in one runtime/port/group domain.
/// Construct and drop on the control thread. The binding budget includes tails
/// and unaccepted terminal notes. No pins are added and no heap work occurs in apply.
///
/// This is not a complete MPE receiver: RPN/MCM, pedals and other channel modes are
/// reported Unsupported. Do not forward those messages to ordinary channel
/// ingress as a substitute for zone semantics. Raw-event consumers run before
/// this adapter; a consumed message must not be passed to apply.
pub struct Mpe {
    runtime: RuntimeId,
    port: u16,
    group: u8,
    zone: Zone,
    members: u8,
    limit: usize,
    controls: [Controls; 16],
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
            port,
            group,
            zone,
            members,
            limit: notes,
            controls: [Controls::default(); 16],
            bindings,
            changes,
        })
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
                let note = runtime.trigger_with_expression(
                    Input { key, ..input },
                    key,
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
                note: runtime.note_off(Input { key, ..input })?,
                velocity,
                attribute,
            },
            Message::PitchBend(Value::Bits14(value)) => {
                let range = if voice.channel == self.zone.manager() {
                    2.0
                } else {
                    48.0
                };
                self.control(runtime, voice.channel, Control::Pitch(pitch(value, range)))?
            }
            Message::ChannelPressure(Value::Bits7(value)) => {
                self.control(runtime, voice.channel, Control::Pressure(value))?
            }
            Message::Control {
                index: 74,
                value: Value::Bits7(value),
            } => self.control(runtime, voice.channel, Control::Timbre(value))?,
            _ => Applied::Unsupported,
        })
    }

    fn control(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        control: Control,
    ) -> Result<Applied, Error> {
        let manager = channel == self.zone.manager();
        let manager_controls = self.controls[usize::from(self.zone.manager())];
        let manager_controls = if manager {
            manager_controls.with(control)
        } else {
            manager_controls
        };
        self.changes.clear();
        for binding in &self.bindings {
            let active_member = binding.channel == channel && runtime.key_down(binding.note)?;
            if manager || active_member {
                let owner = runtime.expression_id(binding.note)?;
                let mut expression = runtime.expression(owner)?;
                let member = if !manager && active_member {
                    binding.member.with(control)
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
        self.controls[usize::from(channel)] = self.controls[usize::from(channel)].with(control);
        if !manager {
            for binding in &mut self.bindings {
                if binding.channel == channel && runtime.key_down(binding.note)? {
                    binding.member = binding.member.with(control);
                }
            }
        }
        Ok(Applied::Expression {
            owners: self.changes.len(),
        })
    }
}

fn pitch(value: u16, range: f64) -> f64 {
    let centered = f64::from(value) - 8192.0;
    // Exact center and endpoints; MPE permits meaningful receiver combination.
    range * centered / if value < 8192 { 8192.0 } else { 8191.0 }
}

// Exact integer projection to the native full-scale domain, not UMP bit-depth
// translation. Original 7-bit values remain in the adapter's channel snapshots.
fn expand(value: u8) -> u32 {
    (u64::from(value) * u64::from(u32::MAX) / 127) as u32
}
