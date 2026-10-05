//! Physical keys and effective gates. Pedal policy is native, not vendor emulation.
use super::{Error, Handle, Input, NoteId, Protocol, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelAddress {
    pub protocol: Protocol,
    pub port: u16,
    pub group: u8,
    pub channel: u8,
}

impl Input {
    pub fn channel_address(self) -> ChannelAddress {
        ChannelAddress {
            protocol: self.protocol,
            port: self.port,
            group: self.group,
            channel: self.channel,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelId(pub(super) Handle);

#[derive(Clone, Copy, Debug)]
pub(super) struct Channel {
    pub address: ChannelAddress,
    pub sustain: bool,
    pub sostenuto: bool,
}

impl Runtime {
    /// Register a controller domain before admitting its control traffic. Registration
    /// is idempotent and bounded; addresses remain stable for this runtime's lifetime.
    /// Panic resets values, not identity. No dynamic allocation occurs here.
    pub fn register_channel(&mut self, address: ChannelAddress) -> Result<ChannelId, Error> {
        if address.group >= 16 || address.channel >= 16 {
            return Err(Error::InvalidInput);
        }
        if let Some((i, _)) = self
            .channels
            .slots
            .iter()
            .enumerate()
            .find(|(_, s)| s.value.is_some_and(|c| c.address == address))
        {
            return Ok(ChannelId(self.channels.id(i)));
        }
        self.channels
            .insert(Channel {
                address,
                sustain: false,
                sostenuto: false,
            })
            .map(ChannelId)
    }

    pub fn pedals(&self, id: ChannelId) -> Result<(bool, bool), Error> {
        let c = self.channels.get(id.0).ok_or(Error::StaleHandle)?;
        Ok((c.sustain, c.sostenuto))
    }

    pub fn key_down(&self, note: NoteId) -> Result<bool, Error> {
        Ok(self.notes.get(note.0).ok_or(Error::StaleHandle)?.key_down)
    }

    pub fn sustain(&mut self, channel: ChannelId, down: bool) -> Result<(), Error> {
        self.schedule_event(self.now, super::Event::Sustain(channel, down))
    }

    pub fn sostenuto(&mut self, channel: ChannelId, down: bool) -> Result<(), Error> {
        self.schedule_event(self.now, super::Event::Sostenuto(channel, down))
    }

    /// Physical key release respects pedals. Explicit release() bypasses them.
    pub fn key_up(&mut self, note: NoteId) -> Result<(), Error> {
        self.apply_due();
        self.key_up_now(note)
    }

    pub(super) fn key_up_now(&mut self, note: NoteId) -> Result<(), Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let sustained = n.input.is_some_and(|i| {
            self.channels.slots.iter().any(|s| {
                s.value
                    .is_some_and(|c| c.address == i.channel_address() && c.sustain)
            })
        });
        let held = sustained || n.sostenuto;
        self.notes.get_mut(note.0).unwrap().key_down = false;
        if !held {
            self.release_now(note)?;
        }
        Ok(())
    }

    pub(super) fn pedal_now(&mut self, id: ChannelId, down: bool, sostenuto: bool) {
        let c = self.channels.get_mut(id.0).unwrap();
        let rising = sostenuto && down && !c.sostenuto;
        if sostenuto {
            c.sostenuto = down;
        } else {
            c.sustain = down;
        }
        let channel = *c;
        for slot in &mut self.notes.slots {
            let Some(n) = &mut slot.value else { continue };
            if !n
                .input
                .is_some_and(|i| i.channel_address() == channel.address)
            {
                continue;
            }
            if rising && n.key_down && n.gate {
                n.sostenuto = true;
            }
            if sostenuto && !down {
                n.sostenuto = false;
            }
            if !n.key_down && !channel.sustain && !n.sostenuto {
                n.gate = false;
            }
        }
        self.propagate_release();
        self.cleanup_closed_notes();
    }
}
