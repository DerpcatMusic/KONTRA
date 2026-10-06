//! One stable sample-time queue for source starts and native musical changes.
use super::{ChannelId, Error, Expression, FamilyId, NoteId, Runtime, VoiceId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A typed value write at sample time, without invoking a UI callback.
    /// Future admission retains this exact plan and reserves its revision increment.
    Control(super::PlanId, super::ControlWrite),
    /// Change a musical domain without changing any expressive channel identity.
    Articulation(super::PerformanceId, u32),
    /// Effective downstream controller state with full native 32-bit precision.
    Controller(super::PerformanceId, u8, u32),
    /// Physical key-up with optional normalized release velocity.
    KeyUp(NoteId, Option<f64>),
    /// Downstream logical key-up; retains the independent host input pairing.
    ScriptKeyUp(NoteId),
    /// Forward a script-suppressed key release without another key-up callback.
    ForwardRelease(NoteId),
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

impl Scheduled {
    fn ends_note(&self, note: NoteId, physical: bool) -> bool {
        match self.action {
            Action::Event(Event::KeyUp(id, _)) => physical && id == note,
            Action::Event(
                Event::ScriptKeyUp(id) | Event::Release(id) | Event::ForwardRelease(id),
            ) => id == note,
            _ => false,
        }
    }
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
            Event::Control(plan, write) => {
                self.validate_controls(plan, None, &[write])?;
            }
            Event::Controller(_, controller, _) if controller >= 128 => {
                return Err(Error::InvalidInput);
            }
            Event::Articulation(id, _) | Event::Controller(id, ..) => {
                self.performance_index(id)?;
            }
            Event::KeyUp(id, velocity) => {
                super::release::validate_velocity(velocity)?;
                let n = self.notes.get(id.0).ok_or(Error::StaleHandle)?;
                if !n.input_down && !n.key_down() {
                    return Err(Error::ClosedNote);
                }
            }
            Event::ScriptKeyUp(id) => {
                let n = self.notes.get(id.0).ok_or(Error::StaleHandle)?;
                if !n.key_down() && !(n.gate() && self.release_times[id.0.index].held) {
                    return Err(Error::ClosedNote);
                }
            }
            Event::Release(id) | Event::Expression(id, _) => {
                if !self.notes.get(id.0).ok_or(Error::StaleHandle)?.gate() {
                    return Err(Error::ClosedNote);
                }
            }
            Event::ForwardRelease(id) => {
                if !self.notes.get(id.0).ok_or(Error::StaleHandle)?.gate()
                    || !self.release_times[id.0.index].held
                {
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
        if let Event::Expression(id, _)
        | Event::KeyUp(id, _)
        | Event::ScriptKeyUp(id)
        | Event::Release(id)
        | Event::ForwardRelease(id) = event
        {
            let n = self.notes.get_mut(id.0).unwrap();
            n.work = n.work.checked_add(1).ok_or(Error::Capacity)?;
        }
        if let Event::Control(plan, _) = event {
            let state = &mut self.plans.get_mut(plan.0).unwrap().controls;
            state.pending = state.pending.checked_add(1).ok_or(Error::Capacity)?;
        }
        self.queue(at, Action::Event(event));
        Ok(())
    }

    pub fn release_at(&mut self, note: NoteId, at: u64) -> Result<(), Error> {
        self.schedule_event(at, Event::Release(note))
    }

    /// Replace this note's queued key-up/release deadlines with one key-up.
    /// Validation and capacity failure preserve the previous deadlines. Equal-time
    /// work already admitted precedes the replacement; pedals still govern the gate.
    pub fn replace_key_up_at(
        &mut self,
        note: NoteId,
        at: u64,
        velocity: Option<f64>,
    ) -> Result<(), Error> {
        super::release::validate_velocity(velocity)?;
        self.replace_note_end_at(note, at, Event::KeyUp(note, velocity))
    }

    /// Replace a suppressed release's pending deadline, retaining its original owner.
    pub fn replace_release_forward_at(&mut self, note: NoteId, at: u64) -> Result<(), Error> {
        self.replace_note_end_at(note, at, Event::ForwardRelease(note))
    }

    /// Replace downstream note-end deadlines without consuming scheduled host key-ups.
    pub fn replace_script_key_up_at(&mut self, note: NoteId, at: u64) -> Result<(), Error> {
        self.replace_note_end_at(note, at, Event::ScriptKeyUp(note))
    }

    fn replace_note_end_at(&mut self, note: NoteId, at: u64, event: Event) -> Result<(), Error> {
        self.check_time(at)?;
        if at == self.now {
            self.apply_due();
        }
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let ready = match event {
            Event::KeyUp(..) => n.input_down || n.key_down(),
            Event::ScriptKeyUp(..) => {
                n.key_down() || (n.gate() && self.release_times[note.0.index].held)
            }
            Event::ForwardRelease(..) => n.gate() && self.release_times[note.0.index].held,
            _ => return Err(Error::InvalidInput),
        };
        if !ready {
            return Err(Error::ClosedNote);
        }
        let physical = matches!(event, Event::KeyUp(..));
        let replaced = self
            .commands
            .iter()
            .filter(|c| c.ends_note(note, physical))
            .count();
        let future = usize::from(at != self.now);
        if future != 0 && replaced == 0 && self.available_commands() == 0 {
            return Err(Error::Capacity);
        }
        let work = n
            .work
            .checked_sub(replaced)
            .and_then(|n| n.checked_add(future))
            .ok_or(Error::Capacity)?;
        self.commands
            .retain(|command| !command.ends_note(note, physical));
        self.notes.get_mut(note.0).unwrap().work = work;
        if future != 0 {
            self.queue(at, Action::Event(event));
        } else {
            self.apply_event(event);
        }
        Ok(())
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
            Event::Control(plan, write) => {
                self.edit_controls_now(plan, None, &[write])
                    .expect("validated control and reserved revision");
            }
            Event::Articulation(id, value) => {
                let index = self.performance_index(id).unwrap();
                self.articulation_now(index, value);
            }
            Event::Controller(id, controller, value) => {
                let index = self.performance_index(id).unwrap();
                self.controller_now(index, controller, value);
            }
            Event::KeyUp(id, velocity) => {
                self.key_up_now(id, velocity).unwrap();
            }
            Event::ScriptKeyUp(id) => {
                if self.release_times[id.0.index].held {
                    self.resume_release(id).unwrap();
                } else {
                    self.key_up_with_cause(id, None, super::ReleaseCause::Script)
                        .unwrap();
                }
            }
            Event::Release(id) => {
                self.release_now(id, super::ReleaseCause::Explicit).unwrap();
            }
            Event::ForwardRelease(id) => {
                self.resume_release(id).unwrap();
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
                    if let Event::Expression(id, _)
                    | Event::KeyUp(id, _)
                    | Event::ScriptKeyUp(id)
                    | Event::Release(id)
                    | Event::ForwardRelease(id) = event
                    {
                        self.notes.get_mut(id.0).unwrap().work -= 1;
                    }
                    if let Event::Control(plan, _) = event {
                        self.plans.get_mut(plan.0).unwrap().controls.pending -= 1;
                    }
                    self.apply_event(event);
                }
            }
        }
        self.executing_due = false;
    }

    pub(super) fn cancel_closed_work(&mut self) {
        let notes = &mut self.notes;
        let mut controller_reserves = 0;
        self.commands.retain(|c| match c.action {
            Action::Resume(id) => {
                let c = self.behaviors.get_mut(id.0).unwrap();
                if c.outcome.is_some() {
                    if let super::behavior::PlanContext::Controller(event) = &mut c.context {
                        controller_reserves += std::mem::take(&mut event.reserved);
                    }
                    false
                } else {
                    let keep = match c.owner {
                        super::BehaviorOwner::Plan(_) => true,
                        super::BehaviorOwner::Note(note) => {
                            let note = notes.get(note.0).unwrap();
                            note.gate()
                                || self.plans.get(note.plan.0).unwrap().prepared.programs[c.program]
                                    .wait_lifetime
                                    == super::WaitLifetime::Callback
                        }
                    };
                    if !keep {
                        c.outcome = Some(super::Outcome::Cancelled);
                    }
                    keep
                }
            }
            Action::Start(v) => self.voices.get(v.0).is_some(),
            Action::Event(Event::ChokeFamily(id, _)) => self.families.get(id.0).is_some(),
            Action::Event(Event::ReleaseFamily(id)) => {
                self.families.get(id.0).is_some_and(|f| f.gate)
            }
            Action::Event(
                event @ (Event::Expression(id, _)
                | Event::KeyUp(id, _)
                | Event::ScriptKeyUp(id)
                | Event::Release(id)
                | Event::ForwardRelease(id)),
            ) => {
                let n = notes.get_mut(id.0).unwrap(); // Work pins cannot be consumed by public unpin().
                let keep = match event {
                    Event::KeyUp(..) => n.input_down || n.key_down(),
                    Event::ScriptKeyUp(..) => {
                        n.key_down() || (n.gate() && self.release_times[id.0.index].held)
                    }
                    Event::ForwardRelease(..) => n.gate() && self.release_times[id.0.index].held,
                    _ => n.gate(),
                };
                if keep {
                    true
                } else {
                    n.work -= 1;
                    false
                }
            }
            Action::Event(
                Event::Control(..)
                | Event::Sustain(..)
                | Event::Sostenuto(..)
                | Event::Articulation(..)
                | Event::Controller(..),
            ) => true,
        });
        self.behaviors.unreserve(controller_reserves);
    }

    /// Remove queued control writes, including an unexecuted boundary event. Does not change
    /// current values, active ramps, musical notes, or events targeting another plan.
    pub fn cancel_control_events(&mut self, plan: super::PlanId) -> Result<usize, Error> {
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let count = generation.controls.pending;
        self.commands.retain(|command| {
            !matches!(command.action,
            Action::Event(Event::Control(target, _)) if target == plan)
        });
        generation.controls.pending = 0;
        Ok(count)
    }
}
