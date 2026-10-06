//! Physical keys and effective gates. Pedal policy is native, not vendor emulation.
use super::{Error, Handle, Input, NoteId, Protocol, ReleaseCause, Runtime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelAddress {
    pub protocol: Protocol,
    pub port: u16,
    pub group: u8,
    pub channel: u8,
}

/// A subset of one protocol/port/group's sixteen physical controller channels.
/// Musical part/articulation selection is independent of this input scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelScope {
    pub protocol: Protocol,
    pub port: u16,
    pub group: u8,
    pub channels: u16,
}
impl ChannelScope {
    fn contains(self, address: ChannelAddress) -> bool {
        self.protocol == address.protocol
            && self.port == address.port
            && self.group == address.group
            && self.channels & (1 << address.channel) != 0
    }
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

    /// Downstream logical key state. For raw host ownership, use input_held.
    pub fn key_down(&self, note: NoteId) -> Result<bool, Error> {
        Ok(self.notes.get(note.0).ok_or(Error::StaleHandle)?.key_down())
    }

    /// Raw external key ownership, independent of downstream script note-off,
    /// callback faults, source EOF and gate state. Generated notes have no input.
    pub fn input_held(&self, note: NoteId) -> Result<bool, Error> {
        Ok(self.notes.get(note.0).ok_or(Error::StaleHandle)?.input_down)
    }

    pub fn sustain(&mut self, channel: ChannelId, down: bool) -> Result<(), Error> {
        self.schedule_event(self.now, super::Event::Sustain(channel, down))
    }

    pub fn sostenuto(&mut self, channel: ChannelId, down: bool) -> Result<(), Error> {
        self.schedule_event(self.now, super::Event::Sostenuto(channel, down))
    }

    /// Apply a pedal to a channel set as one event. Pedal-down reserves every
    /// missing channel before mutation; pedal-up never needs capacity. Empty sets
    /// are no-ops. Due work runs before admission and cannot be rolled back.
    pub fn sustain_scope(&mut self, scope: ChannelScope, down: bool) -> Result<(), Error> {
        self.pedal_scope(scope, down, false, None)
    }

    pub fn sostenuto_scope(&mut self, scope: ChannelScope, down: bool) -> Result<(), Error> {
        self.pedal_scope(scope, down, true, None)
    }

    /// Commit an accepted downstream CC64/66 value and its physical pedal scope
    /// together. Capacity is checked before publication; release selection observes
    /// the new value. Pedal-up needs no free channel, event or snapshot slot.
    pub fn set_pedal_controller(
        &mut self,
        performance: super::PerformanceId,
        scope: ChannelScope,
        controller: u8,
        value: u32,
    ) -> Result<(), Error> {
        let performance = self.performance_index(performance)?;
        if !matches!(controller, 64 | 66) {
            return Err(Error::InvalidInput);
        }
        self.pedal_scope(
            scope,
            value >= 0x8000_0000,
            controller == 66,
            Some((performance, controller, value)),
        )
    }

    fn pedal_scope(
        &mut self,
        scope: ChannelScope,
        down: bool,
        sostenuto: bool,
        controller: Option<(usize, u8, u32)>,
    ) -> Result<(), Error> {
        if scope.group >= 16 {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        if down {
            let existing = self
                .channels
                .slots
                .iter()
                .filter_map(|slot| slot.value)
                .filter(|channel| scope.contains(channel.address))
                .fold(0u16, |mask, channel| mask | (1 << channel.address.channel));
            let mut missing = scope.channels & !existing;
            if missing.count_ones() as usize > self.channels.available() {
                return Err(Error::Capacity);
            }
            while missing != 0 {
                let channel = missing.trailing_zeros() as u8;
                missing &= missing - 1;
                self.channels
                    .insert(Channel {
                        address: ChannelAddress {
                            protocol: scope.protocol,
                            port: scope.port,
                            group: scope.group,
                            channel,
                        },
                        sustain: false,
                        sostenuto: false,
                    })
                    .expect("preflighted controller-domain capacity");
            }
        }
        if let Some((performance, controller, value)) = controller {
            self.controller_now(performance, controller, value);
        }
        let mut sustained = 0;
        let mut rising = 0;
        for slot in &mut self.channels.slots {
            let Some(channel) = &mut slot.value else {
                continue;
            };
            if !scope.contains(channel.address) {
                continue;
            }
            let bit = 1 << channel.address.channel;
            if sostenuto {
                if down && !channel.sostenuto {
                    rising |= bit
                }
                channel.sostenuto = down;
            } else {
                channel.sustain = down;
            }
            if channel.sustain {
                sustained |= bit
            }
        }
        self.update_pedal_notes(scope, sustained, rising, sostenuto, down);
        Ok(())
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
        for i in 0..self.notes.slots.len() {
            let Some(note) = &mut self.notes.slots[i].value else {
                continue;
            };
            if note.input_down
                && note
                    .input
                    .is_some_and(|input| input.channel_address() == address)
            {
                let held = !self.selections[i].consumed_switch && (sustained || note.sostenuto);
                self.release_key(NoteId(self.notes.id(i)), ReleaseCause::AllNotesOff, None);
                if !held && !self.release_times[i].held {
                    self.close_gate(super::NoteId(self.notes.id(i)), ReleaseCause::AllNotesOff);
                }
                released += 1;
            }
        }
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
        for i in 0..self.notes.slots.len() {
            if let Some(note) = &mut self.notes.slots[i].value
                && note.address == address
            {
                note.sostenuto = false;
                if note.input.is_none() {
                    self.release_key(NoteId(self.notes.id(i)), ReleaseCause::AllSoundOff, None);
                }
                self.close_gate(super::NoteId(self.notes.id(i)), ReleaseCause::AllSoundOff);
            }
        }
        for slot in &mut self.behaviors.slots {
            if let Some(callback) = &mut slot.value
                && callback.outcome.is_none()
                && let super::BehaviorOwner::Note(note) = callback.owner
                && self.notes.get(note.0).is_some_and(|n| n.address == address)
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

    /// Key release respects pedals; velocity is normalized, None means absent.
    /// Explicit release() bypasses pedals. Repeated key-up retains the first record.
    pub fn key_up(&mut self, note: NoteId, velocity: Option<f64>) -> Result<(), Error> {
        self.apply_due();
        self.key_up_now(note, velocity)
    }

    pub(super) fn key_up_now(&mut self, note: NoteId, velocity: Option<f64>) -> Result<(), Error> {
        self.key_up_with_cause(note, velocity, ReleaseCause::KeyUp)
    }

    pub(super) fn key_up_with_cause(
        &mut self,
        note: NoteId,
        velocity: Option<f64>,
        cause: ReleaseCause,
    ) -> Result<(), Error> {
        super::release::validate_velocity(velocity)?;
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let sustained = n.input.is_some_and(|i| {
            self.channels.slots.iter().any(|s| {
                s.value
                    .is_some_and(|c| c.address == i.channel_address() && c.sustain)
            })
        });
        let held = !self.selections[note.0.index].consumed_switch && (sustained || n.sostenuto);
        self.release_key(note, cause, velocity);
        if !held && !self.release_times[note.0.index].held {
            self.close_gate(note, cause);
            self.cleanup_closed_notes();
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
        let address = c.address;
        let bit = 1 << address.channel;
        let sustained = if c.sustain { bit } else { 0 };
        let rising = if rising { bit } else { 0 };
        let scope = ChannelScope {
            protocol: address.protocol,
            port: address.port,
            group: address.group,
            channels: bit,
        };
        self.update_pedal_notes(scope, sustained, rising, sostenuto, down);
    }

    fn update_pedal_notes(
        &mut self,
        scope: ChannelScope,
        sustained: u16,
        rising: u16,
        sostenuto: bool,
        down: bool,
    ) {
        for i in 0..self.notes.slots.len() {
            let Some(n) = &mut self.notes.slots[i].value else {
                continue;
            };
            let Some(input) = n
                .input
                .filter(|input| scope.contains(input.channel_address()))
            else {
                continue;
            };
            let bit = 1 << input.channel;
            if rising & bit != 0 && n.input_down && n.gate() && !self.selections[i].consumed_switch
            {
                n.sostenuto = true;
            }
            if sostenuto && !down {
                n.sostenuto = false;
            }
            if !n.key_down() && sustained & bit == 0 && !n.sostenuto && !self.release_times[i].held
            {
                self.close_gate(super::NoteId(self.notes.id(i)), ReleaseCause::Pedal);
            }
        }
        self.cleanup_closed_notes();
    }
}
