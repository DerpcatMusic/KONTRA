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

    /// Release every physically held input in one channel domain, respecting pedals.
    /// Cleanup never needs a new channel slot or command slot. Returns keys released.
    pub fn all_notes_off(&mut self, address: ChannelAddress) -> Result<usize, Error> {
        if address.group >= 16 || address.channel >= 16 {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        let sustained = self.channels.slots.iter().any(|slot| {
            slot.value
                .is_some_and(|c| c.address == address && c.sustain)
        });
        let mut released = 0;
        for slot in &mut self.notes.slots {
            let Some(note) = &mut slot.value else {
                continue;
            };
            if note.key_down
                && note
                    .input
                    .is_some_and(|input| input.channel_address() == address)
            {
                note.key_down = false;
                if !sustained && !note.sostenuto {
                    note.gate = false;
                }
                released += 1;
            }
        }
        self.propagate_release();
        self.cleanup_closed_notes();
        Ok(released)
    }

    /// Hard-silence one input domain, including descendants and pending callbacks.
    /// Physical input keys remain owned until key-up; pedals and other domains remain.
    /// Returns the number of stopped source voices, including delayed starts.
    pub fn all_sound_off(&mut self, address: ChannelAddress) -> Result<usize, Error> {
        if address.group >= 16 || address.channel >= 16 {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        for slot in &mut self.notes.slots {
            if let Some(note) = &mut slot.value
                && note.address == address
            {
                note.gate = false;
                note.sostenuto = false;
                if note.input.is_none() {
                    note.key_down = false;
                }
            }
        }
        for slot in &mut self.behaviors.slots {
            if let Some(callback) = &mut slot.value
                && callback.outcome.is_none()
                && self
                    .notes
                    .get(callback.note.0)
                    .is_some_and(|n| n.address == address)
            {
                callback.outcome = Some(super::Outcome::Cancelled);
            }
        }
        let mut stopped = 0;
        for i in 0..self.voices.slots.len() {
            let matches = self.voices.slots[i]
                .value
                .and_then(|v| self.families.get(v.family.0))
                .and_then(|f| self.notes.get(f.note.0))
                .is_some_and(|n| n.address == address);
            if matches {
                self.end_voice(super::VoiceId(self.voices.id(i)));
                stopped += 1;
            }
        }
        self.cleanup_closed_notes();
        Ok(stopped)
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
        } else {
            // Key-up may resolve a physically retained but already silenced input.
            // Remove later key-up commands before that owner can retire.
            self.cancel_closed_work();
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
