//! Articulation drivers in front of note ingress: velocity, channel, CC or
//! program change select the articulation a keyswitch would, from the active
//! plan's [`sampler_core::Switching`].
//!
//! This is a raw-event stage: run it on every packet before [`crate::Ingress`]
//! or [`crate::Mpe`], and pass on only what it forwards. It never edits voices
//! or behavior state directly. A native articulation is set on the performance
//! exactly as a native keyswitch would set it; behavior-owned switching is
//! driven by tapping the switch key through ordinary note admission, so the
//! scripts that own it see the same note-on/off they would for a played key.
//! The note that carried a velocity or channel selection is then forwarded and
//! reaches behaviors unchanged. No allocation or heap work occurs in `intercept`.
use crate::{Applied, ApplyError, Message, Packet, Value, Version};
use sampler_core::{
    Driver, Error, Expression, Input, NotePitch, PerformanceId, Protocol, Runtime, RuntimeId,
    Switch, SwitchKeys,
};

/// What to do with the packet after interception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intercept {
    /// Pass the packet to note ingress.
    Forward,
    /// The packet was a driver or a swallowed switch key; do not forward it.
    Consumed(Applied),
}

/// One performance's driver state. Construct on the control thread.
pub struct Articulator {
    runtime: RuntimeId,
    performance: PerformanceId,
    port: u16,
    /// Last key tapped into behavior-owned switching; behaviors keep their own
    /// state, so a repeat selection is not tapped again.
    tapped: Option<u8>,
    /// Keys whose switch note-on was swallowed / forwarded, so a remap while
    /// held cannot strand the release or leave a note on.
    // ponytail: per key, not per channel; two channels on one switch key share a bit.
    swallowed: u128,
    played: u128,
}

impl Articulator {
    pub fn new(runtime: &Runtime, performance: PerformanceId, port: u16) -> Result<Self, Error> {
        runtime.articulation(performance)?;
        Ok(Self {
            runtime: runtime.id(),
            performance,
            port,
            tapped: None,
            swallowed: 0,
            played: 0,
        })
    }

    /// Apply the driver part of `packet` at `Runtime::now()`.
    pub fn intercept(
        &mut self,
        runtime: &mut Runtime,
        packet: Packet<'_>,
    ) -> Result<Intercept, ApplyError> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        let Some(voice) = packet.channel_voice() else {
            return Ok(Intercept::Forward);
        };
        let switching = runtime.switching();
        let driver = switching.driver();
        // A switch key's release follows its press, whatever the driver is now.
        match voice.message {
            Message::NoteOff { key, .. } if key < 128 => {
                let bit = 1u128 << key;
                if self.swallowed & bit != 0 {
                    self.swallowed &= !bit;
                    return Ok(Intercept::Consumed(Applied::Ignored));
                }
                if self.played & bit != 0 {
                    self.played &= !bit;
                    return Ok(Intercept::Forward);
                }
            }
            Message::NoteOn { key, .. } if switching.is_switch_key(key) => {
                let bit = 1u128 << key;
                if driver != Driver::Keys && switching.keys() == SwitchKeys::Swallow {
                    self.swallowed |= bit;
                } else {
                    self.played |= bit;
                }
            }
            _ => {}
        }
        if driver == Driver::Keys {
            return Ok(Intercept::Forward);
        }
        let (controller, value, velocity) = match voice.message {
            Message::NoteOn { key, .. } | Message::NoteOff { key, .. }
                if switching.is_switch_key(key) =>
            {
                if switching.keys() == SwitchKeys::Swallow {
                    return Ok(Intercept::Consumed(Applied::Ignored));
                }
                // A played switch moves behavior state behind the driver's back.
                self.tapped = None;
                return Ok(Intercept::Forward);
            }
            Message::NoteOn { velocity, .. } if driver == Driver::Velocity => {
                (0, seven(velocity), velocity.normalized())
            }
            Message::NoteOn { velocity, .. } if driver == Driver::Channel => {
                (0, voice.channel, velocity.normalized())
            }
            Message::Control { index, value }
                if driver == Driver::Controller && switching.listens(index) =>
            {
                (index, seven(value), 1.0)
            }
            Message::Program { program, .. } if driver == Driver::Program => (0, program, 1.0),
            _ => return Ok(Intercept::Forward),
        };
        let note = matches!(voice.message, Message::NoteOn { .. });
        if let Some(switch) = switching.select(controller, value) {
            self.switch(
                runtime,
                switch,
                voice.version,
                voice.group,
                voice.channel,
                velocity,
            )?;
        }
        Ok(if note {
            Intercept::Forward
        } else {
            Intercept::Consumed(Applied::Configuration)
        })
    }

    fn switch(
        &mut self,
        runtime: &mut Runtime,
        switch: Switch,
        version: Version,
        group: u8,
        channel: u8,
        velocity: f64,
    ) -> Result<(), Error> {
        match switch {
            Switch::Articulation(id) => {
                if runtime.articulation(self.performance)? != id {
                    runtime.set_articulation(self.performance, id)?;
                }
            }
            Switch::Tap(key) if self.tapped != Some(key) => {
                let input = Input {
                    protocol: match version {
                        Version::Midi1 => Protocol::Midi1,
                        Version::Midi2 => Protocol::Midi2,
                    },
                    port: self.port,
                    group,
                    channel,
                    key,
                    external_id: None,
                };
                runtime.trigger_in(
                    self.performance,
                    input,
                    NotePitch::Key(key),
                    velocity,
                    Expression::default(),
                )?;
                runtime.note_off_in(self.performance, input, None)?;
                self.tapped = Some(key);
            }
            Switch::Tap(_) => {}
        }
        Ok(())
    }
}

/// MIDI 1.0 resolution of a velocity or controller value.
fn seven(value: Value) -> u8 {
    match value {
        Value::Bits7(v) => v,
        Value::Bits14(v) => (v >> 7) as u8,
        Value::Bits16(v) => (v >> 9) as u8,
        Value::Bits32(v) => (v >> 25) as u8,
    }
}
