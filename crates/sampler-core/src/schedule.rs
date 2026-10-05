//! One stable sample-time queue for source starts and native musical changes.
use super::{ChannelId, Error, Expression, FamilyId, NoteId, Runtime, VoiceId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Physical key-up with optional normalized release velocity.
    KeyUp(NoteId, Option<f64>),
    Release(NoteId),
    /// Independent family gate, using each source's envelope and loop release.
    ReleaseFamily(FamilyId),
    /// Fade a family over at most this many frames. Natural completion before
    /// execution cancels the action; it never pins or retargets a reused family.
    ChokeFamily(FamilyId, u32),
    /// Resolves the note's expression owner at execution, including explicit detach.
    Expression(NoteId, Expression),
    Sustain(ChannelId, bool),
    Sostenuto(ChannelId, bool),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    Start(VoiceId),
    Event(Event),
    Resume(super::BehaviorId),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Scheduled {
    pub at: u64,
    pub action: Action,
}

impl Runtime {
    pub(super) fn available_commands(&self) -> usize {
        self.command_limit - self.commands.len() - self.reserved_commands
    }

    /// Equal timestamps execute in submission order. New immediate operations first
    /// drain previously submitted work due now. Future capacity failure is explicit;
    /// immediate key/pedal release never requires a free queue entry.
    pub fn schedule_event(&mut self, at: u64, event: Event) -> Result<(), Error> {
        self.check_time(at)?;
        if at == self.now {
            self.apply_due();
        }
        match event {
            Event::KeyUp(id, velocity) => {
                super::release::validate_velocity(velocity)?;
                if !self.notes.get(id.0).ok_or(Error::StaleHandle)?.key_down() {
                    return Err(Error::ClosedNote);
                }
            }
            Event::Release(id) | Event::Expression(id, _) => {
                if !self.notes.get(id.0).ok_or(Error::StaleHandle)?.gate() {
                    return Err(Error::ClosedNote);
                }
            }
            Event::Sustain(id, _) | Event::Sostenuto(id, _) => {
                self.channels.get(id.0).ok_or(Error::StaleHandle)?;
            }
            Event::ReleaseFamily(id) => {
                if !self.families.get(id.0).ok_or(Error::StaleHandle)?.gate {
                    return Err(Error::ClosedFamily);
                }
            }
            Event::ChokeFamily(id, _) => {
                self.families.get(id.0).ok_or(Error::StaleHandle)?;
            }
        }
        if let Event::Expression(note, e) = event {
            if !e.valid() {
                return Err(Error::InvalidInput);
            }
            let owner = self.notes.get(note.0).unwrap().expression;
            self.validate_expression_change(owner, e)?;
        }
        if at == self.now {
            self.apply_event(event);
            return Ok(());
        }
        if self.available_commands() == 0 {
            return Err(Error::Capacity);
        }
        if let Event::Expression(id, _) = event {
            let n = self.notes.get_mut(id.0).unwrap();
            n.work = n.work.checked_add(1).ok_or(Error::Capacity)?;
        }
        self.queue(at, Action::Event(event));
        Ok(())
    }

    pub fn release_at(&mut self, note: NoteId, at: u64) -> Result<(), Error> {
        self.schedule_event(at, Event::Release(note))
    }

    pub(super) fn check_time(&self, at: u64) -> Result<(), Error> {
        if at < self.now {
            Err(Error::PastEvent)
        } else {
            Ok(())
        }
    }

    pub(super) fn queue(&mut self, at: u64, action: Action) {
        // ponytail: bounded sorted Vec; use a heap if measured command traffic needs it.
        let index = self.commands.partition_point(|c| c.at <= at);
        self.commands.insert(index, Scheduled { at, action });
    }

    fn apply_event(&mut self, event: Event) {
        match event {
            Event::KeyUp(id, velocity) => {
                self.key_up_now(id, velocity).unwrap();
            }
            Event::Release(id) => {
                self.release_now(id, super::ReleaseCause::Explicit).unwrap();
            }
            Event::ReleaseFamily(id) => {
                self.release_family_now(id);
                self.cancel_closed_work();
            }
            Event::ChokeFamily(id, frames) => self.choke_family_now(id, frames),
            Event::Expression(id, value) => {
                let owner = self.notes.get(id.0).unwrap().expression;
                self.set_expression_now(owner, value).unwrap();
            }
            Event::Sustain(id, down) => self.pedal_now(id, down, false),
            Event::Sostenuto(id, down) => self.pedal_now(id, down, true),
        }
    }

    pub(super) fn apply_due(&mut self) {
        if self.executing_due {
            return;
        }
        self.executing_due = true;
        // Resumes have finite instruction fuel and only enqueue strictly future waits.
        // Immediate native operations cannot recursively drain later equal-time work.
        while self.commands.first().is_some_and(|c| c.at <= self.now) {
            match self.commands.remove(0).action {
                Action::Resume(id) => self.resume_behavior(id),
                Action::Start(id) => {
                    self.voices.get_mut(id.0).unwrap().started = true;
                }
                Action::Event(event) => {
                    if let Event::Expression(id, _) = event {
                        self.notes.get_mut(id.0).unwrap().work -= 1;
                    }
                    self.apply_event(event);
                }
            }
        }
        self.executing_due = false;
    }

    pub(super) fn cancel_closed_work(&mut self) {
        let notes = &mut self.notes;
        self.commands.retain(|c| match c.action {
            Action::Resume(id) => {
                let c = self.behaviors.get_mut(id.0).unwrap();
                if c.outcome.is_some() {
                    false
                } else if notes.get(c.note.0).unwrap().gate()
                    || self
                        .plans
                        .get(notes.get(c.note.0).unwrap().plan.0)
                        .unwrap()
                        .prepared
                        .programs[c.program]
                        .wait_lifetime
                        == super::WaitLifetime::Callback
                {
                    true
                } else {
                    c.outcome = Some(super::Outcome::Cancelled);
                    false
                }
            }
            Action::Start(v) => self.voices.get(v.0).is_some(),
            Action::Event(Event::KeyUp(n, _)) => notes.get(n.0).is_some_and(|n| n.key_down()),
            Action::Event(Event::Release(n)) => notes.get(n.0).is_some_and(|n| n.gate()),
            Action::Event(Event::ChokeFamily(id, _)) => self.families.get(id.0).is_some(),
            Action::Event(Event::ReleaseFamily(id)) => {
                self.families.get(id.0).is_some_and(|f| f.gate)
            }
            Action::Event(Event::Expression(id, _)) => {
                let n = notes.get_mut(id.0).unwrap(); // Work pins cannot be consumed by public unpin().
                if n.gate() {
                    true
                } else {
                    n.work -= 1;
                    false
                }
            }
            Action::Event(Event::Sustain(..) | Event::Sostenuto(..)) => true,
        });
    }
}
