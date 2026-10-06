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
        }
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
        let slot = usize::from(voice.group) * 16 + usize::from(voice.channel);
        let channel = self.channels[slot];
        let range = channel.range.unwrap_or_else(|| runtime.bend_range());
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
                Expression {
                    pitch_semitones: channel.bend * range,
                    pressure: channel.pressure,
                    ..Expression::default()
                },
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
            Message::PitchBend(value) => {
                let bend = bend(value);
                let owners = runtime.set_input_expressions(input.channel_address(), None, |e| {
                    Expression {
                        pitch_semitones: bend * range,
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
                Applied::Expression {
                    owners: runtime.set_input_expressions(
                        input.channel_address(),
                        Some(key),
                        |e| Expression { pressure, ..e },
                    )?,
                }
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
                // RP-015: pedals (64-67) up, modulation (1) to 0, expression
                // (11) to full, bend centred, channel and poly pressure 0,
                // RPN/NRPN selection null. Volume, pan, bank, effects, sound
                // controllers and the bend range keep their values. Pedals
                // first: a failed later admission must not leave notes
                // sustained.
                for (index, value) in [(64, 0), (65, 0), (66, 0), (67, 0), (1, 0), (11, u32::MAX)] {
                    runtime.dispatch_controller(
                        performance,
                        input.channel_address(),
                        1 << input.channel,
                        index,
                        value,
                    )?;
                }
                runtime.set_input_expressions(input.channel_address(), None, |e| Expression {
                    pitch_semitones: 0.0,
                    pressure: 0,
                    ..e
                })?;
                let channel = &mut self.channels[slot];
                (channel.bend, channel.pressure, channel.rpn) = (0.0, 0, [127; 2]);
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
        let bend = self.channels[slot].bend;
        runtime.set_input_expressions(address, None, |e| Expression {
            pitch_semitones: bend * range,
            ..e
        })?;
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
