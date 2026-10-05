use crate::{Attribute, Message, Packet, Value, Version};
use sampler_core::{Error, Input, NoteId, Protocol, Runtime};

/// Protocol selection belongs to the connection/control plane, never inferred from
/// incoming notes. Disabled groups and protocol mismatches cannot mutate the core.
pub struct Ingress {
    port: u16,
    groups: [Option<Version>; 16],
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
    Released {
        note: NoteId,
        velocity: Option<Value>,
        attribute: Attribute,
    },
    Pedal,
    Unsupported,
}

impl Ingress {
    pub fn new(port: u16, groups: [Option<Version>; 16]) -> Self {
        Self { port, groups }
    }

    /// Apply at Runtime::now(). The host must split rendering at event timestamps.
    /// Declared musical scope: un-attributed notes, sustain and sostenuto. Other
    /// decoded messages are reported as unsupported, not silently approximated.
    pub fn apply(&self, runtime: &mut Runtime, packet: Packet<'_>) -> Result<Applied, ApplyError> {
        let Some(voice) = packet.channel_voice() else {
            return Ok(Applied::Unsupported);
        };
        let configured = self.groups[usize::from(voice.group)].ok_or(ApplyError::DisabledGroup)?;
        if configured != voice.version {
            return Err(ApplyError::ProtocolMismatch);
        }
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
            } if attribute.kind == 0 => Applied::Started(runtime.trigger(
                Input { key, ..input },
                key,
                velocity.normalized(),
            )?),
            Message::NoteOff {
                key,
                velocity,
                attribute,
            } => {
                // Unknown release attributes must not strand a sounding note.
                let note = runtime.note_off(Input { key, ..input })?;
                Applied::Released {
                    note,
                    velocity,
                    attribute,
                }
            }
            Message::Control {
                index: 64 | 66,
                value,
            } => {
                let channel = runtime.register_channel(input.channel_address())?;
                let down = value.normalized() >= 0.5;
                if matches!(voice.message, Message::Control { index: 64, .. }) {
                    runtime.sustain(channel, down)?;
                } else {
                    runtime.sostenuto(channel, down)?;
                }
                Applied::Pedal
            }
            _ => Applied::Unsupported,
        })
    }
}
