//! Native bounded musical instructions, independent of any vendor language VM.
use super::{Action, Error, Handle, NoteId, Runtime};

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    /// Generate a mapped child linked to the originating note's effective gate.
    Play {
        transpose: i8,
        velocity_scale: f64,
        duration: u32,
    },
    /// Sample-clock wait. Zero advances inline and still consumes instruction fuel.
    Wait(u32),
    End,
}

pub struct Program {
    pub(super) code: Box<[Instruction]>,
}
impl Program {
    pub fn new(code: Vec<Instruction>) -> Result<Self, Error> {
        if code.iter().any(|op| {
            matches!(op, Instruction::Play { velocity_scale, .. }
            if !velocity_scale.is_finite() || !(0.0..=1.0).contains(velocity_scale))
        }) {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            code: code.into_boxed_slice(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviorId(pub(super) Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Finished,
    Cancelled,
    FuelExhausted,
    Fault(Error),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Continuation {
    pub note: NoteId,
    pub program: usize,
    pub pc: usize,
    pub outcome: Option<Outcome>,
}

impl Runtime {
    /// Run against an existing logical note. A caller can suppress default playback
    /// by admitting with note_on instead of trigger. Completion owns a private pin
    /// until flush_behaviors accepts it, including synchronous completion/failure.
    pub fn start_behavior(&mut self, note: NoteId, program: usize) -> Result<BehaviorId, Error> {
        self.apply_due();
        if program >= self.plan.programs.len() {
            return Err(Error::InvalidInput);
        }
        let n = self.notes.get_mut(note.0).ok_or(Error::StaleHandle)?;
        if !n.gate {
            return Err(Error::ClosedNote);
        }
        let work = n.work.checked_add(1).ok_or(Error::Capacity)?;
        let id = BehaviorId(self.behaviors.insert(Continuation {
            note,
            program,
            pc: 0,
            outcome: None,
        })?);
        n.work = work;
        self.resume_behavior(id);
        Ok(id)
    }

    pub fn behavior_outcome(&self, id: BehaviorId) -> Result<Option<Outcome>, Error> {
        Ok(self.behaviors.get(id.0).ok_or(Error::StaleHandle)?.outcome)
    }

    /// Native abort policy: release the originating note and linked children.
    /// Pending work cancels without queue admission; existing envelope tails finish.
    pub fn cancel_behavior(&mut self, id: BehaviorId) -> Result<(), Error> {
        self.apply_due();
        let c = self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?;
        if c.outcome.is_none() {
            c.outcome = Some(Outcome::Cancelled);
            let note = c.note;
            self.release_now(note)?;
        }
        Ok(())
    }

    pub fn flush_behaviors(&mut self, mut accept: impl FnMut(BehaviorId, NoteId, Outcome) -> bool) {
        for i in 0..self.behaviors.slots.len() {
            let Some(c) = self.behaviors.slots[i].value else {
                continue;
            };
            let Some(outcome) = c.outcome else {
                continue;
            };
            let id = BehaviorId(self.behaviors.id(i));
            if !accept(id, c.note, outcome) {
                return;
            }
            self.behaviors.remove(id.0);
            self.notes.get_mut(c.note.0).unwrap().work -= 1;
        }
    }

    pub(super) fn resume_behavior(&mut self, id: BehaviorId) {
        for _ in 0..self.behavior_fuel {
            let c = self.behaviors.get_mut(id.0).unwrap();
            if c.outcome.is_some() {
                return;
            }
            let Some(op) = self.plan.programs[c.program].code.get(c.pc).copied() else {
                c.outcome = Some(Outcome::Finished);
                return;
            };
            c.pc += 1;
            let note = c.note;
            match self.behavior_step(id, note, op) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    self.fail_behavior(id, Outcome::Fault(error));
                    return;
                }
            }
        }
        let c = self.behaviors.get_mut(id.0).unwrap();
        if c.pc == self.plan.programs[c.program].code.len() {
            c.outcome = Some(Outcome::Finished);
        } else {
            self.fail_behavior(id, Outcome::FuelExhausted);
        }
    }

    /// true means suspended or finished, false means continue synchronously.
    fn behavior_step(
        &mut self,
        id: BehaviorId,
        note: NoteId,
        op: Instruction,
    ) -> Result<bool, Error> {
        match op {
            Instruction::End => {
                self.behaviors.get_mut(id.0).unwrap().outcome = Some(Outcome::Finished);
                return Ok(true);
            }
            Instruction::Wait(0) => {}
            Instruction::Wait(frames) => {
                let at = self
                    .now
                    .checked_add(u64::from(frames))
                    .ok_or(Error::ClockOverflow)?;
                if self.commands.len() == self.command_limit {
                    return Err(Error::Capacity);
                }
                self.queue(at, Action::Resume(id));
                return Ok(true);
            }
            Instruction::Play {
                transpose,
                velocity_scale,
                duration,
            } => {
                let n = self.notes.get(note.0).unwrap();
                let key = i16::from(n.key) + i16::from(transpose);
                if !(0..128).contains(&key) {
                    return Err(Error::InvalidInput);
                }
                let velocity = n.velocity * velocity_scale;
                let at = self
                    .now
                    .checked_add(u64::from(duration))
                    .ok_or(Error::ClockOverflow)?;
                if duration != 0 && self.commands.len() == self.command_limit {
                    return Err(Error::Capacity);
                }
                let child = self.trigger_child(note, key as u8, velocity)?;
                self.release_at(child, at)?;
            }
        }
        Ok(false)
    }

    fn fail_behavior(&mut self, id: BehaviorId, outcome: Outcome) {
        let c = self.behaviors.get_mut(id.0).unwrap();
        c.outcome = Some(outcome);
        let note = c.note;
        self.release_now(note)
            .expect("continuation retains originating note");
    }
}
