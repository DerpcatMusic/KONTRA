use crate::{Attribute, Message, Packet, Value, Version};
use sampler_core::{Error, Expression, Input, NoteId, NotePitch, PerformanceId, Protocol, Runtime};

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
        Self { port, groups }
    }

    /// Apply at Runtime::now(). The host must split rendering at event timestamps.
    /// Note attributes support absolute Pitch 7.9 in addition to ordinary notes. Other
    /// decoded messages are reported as unsupported, not silently approximated.
    pub fn apply(&self, runtime: &mut Runtime, packet: Packet<'_>) -> Result<Applied, ApplyError> {
        self.apply_in(runtime, runtime.performance(0)?, packet)
    }

    /// Route note pairing/selection to a musical domain. Pedals/channel modes retain
    /// their declared physical channel scope, independent of articulation routing.
    pub fn apply_in(
        &self,
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
            } => Applied::Started(runtime.trigger_in(
                performance,
                Input { key, ..input },
                if attribute.kind == 3 {
                    NotePitch::Absolute(f64::from(attribute.data) / 512.0)
                } else {
                    NotePitch::Key(key)
                },
                velocity.normalized(),
                Expression::default(),
            )?),
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
            Message::Control { index, value } if index < 120 => {
                runtime.dispatch_controller(
                    performance,
                    input.channel_address(),
                    1 << input.channel,
                    index,
                    value.full_scale(),
                )?;
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
                // RP-015: pedals (64-67) up, modulation (1) to 0, expression
                // (11) to full. Volume, pan, bank, effects and sound
                // controllers keep their values. This receiver has no pitch
                // bend, pressure or RPN state to reset. Pedals first: a
                // failed later admission must not leave notes sustained.
                for (index, value) in [(64, 0), (65, 0), (66, 0), (67, 0), (1, 0), (11, u32::MAX)] {
                    runtime.dispatch_controller(
                        performance,
                        input.channel_address(),
                        1 << input.channel,
                        index,
                        value,
                    )?;
                }
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
        &self,
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
