//! Native bounded musical instructions, independent of any vendor language VM.
use super::{Action, Error, Handle, Inheritance, NoteId, Runtime};

#[derive(Clone, Copy, Debug)]
pub enum Velocity {
    Scale(f64),
    Fixed(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duration {
    /// Releases only when the originating note's effective gate closes.
    Gate,
    /// Independent duration; can outlive the originating gate.
    Frames(u32),
    /// Native bounded duration that also follows the originating gate.
    FramesOrGate(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaitLifetime {
    #[default]
    Gate,
    /// The continuation retains its input until completion or explicit cancellation.
    Callback,
}

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    /// Generate a mapped child with an explicit release policy.
    Play {
        transpose: i8,
        velocity: Velocity,
        inheritance: Inheritance,
        duration: Duration,
    },
    /// Sample-clock wait. Zero advances inline and still consumes instruction fuel.
    Wait(u32),
    End,
    SetLocal {
        local: u16,
        value: i64,
    },
    AddLocal {
        local: u16,
        value: i64,
    },
    ReadKey {
        local: u16,
    },
    /// Copy between callback-local registers and the originating note's state.
    ReadNoteCell {
        local: u16,
        cell: u16,
    },
    WriteNoteCell {
        cell: u16,
        local: u16,
    },
    Jump {
        target: usize,
    },
    JumpIfZero {
        local: u16,
        target: usize,
    },
}

pub struct Program {
    pub(super) code: Box<[Instruction]>,
    pub(super) locals: usize,
    pub(super) note_cells: usize,
    pub(super) wait_lifetime: WaitLifetime,
}
impl Program {
    pub fn with_wait_lifetime(mut self, lifetime: WaitLifetime) -> Self {
        self.wait_lifetime = lifetime;
        self
    }

    pub fn new(code: Vec<Instruction>) -> Result<Self, Error> {
        let mut locals = 0;
        let mut note_cells = 0;
        for op in &code {
            match *op {
                Instruction::Play {
                    velocity: Velocity::Scale(value) | Velocity::Fixed(value),
                    ..
                } if !value.is_finite() || !(0.0..=1.0).contains(&value) => {
                    return Err(Error::InvalidInput);
                }
                Instruction::Jump { target } | Instruction::JumpIfZero { target, .. }
                    if target > code.len() =>
                {
                    return Err(Error::InvalidInput);
                }
                _ => {}
            }
            if let Instruction::SetLocal { local, .. }
            | Instruction::AddLocal { local, .. }
            | Instruction::ReadKey { local }
            | Instruction::ReadNoteCell { local, .. }
            | Instruction::WriteNoteCell { local, .. }
            | Instruction::JumpIfZero { local, .. } = *op
            {
                locals = locals.max(usize::from(local) + 1);
            }
            if let Instruction::ReadNoteCell { cell, .. }
            | Instruction::WriteNoteCell { cell, .. } = *op
            {
                note_cells = note_cells.max(usize::from(cell) + 1);
            }
        }
        Ok(Self {
            code: code.into_boxed_slice(),
            locals,
            note_cells,
            wait_lifetime: WaitLifetime::Gate,
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
        self.start_behavior_now(note, program)
    }

    fn start_behavior_now(&mut self, note: NoteId, program: usize) -> Result<BehaviorId, Error> {
        let n = self.notes.get_mut(note.0).ok_or(Error::StaleHandle)?;
        let plan = &self.plans.get(n.plan.0).unwrap().prepared;
        if program >= plan.programs.len() {
            return Err(Error::InvalidInput);
        }
        if !n.gate() && plan.programs[program].wait_lifetime == WaitLifetime::Gate {
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
        let begin = id.0.index * self.behavior_stride;
        self.behavior_locals[begin..begin + plan.programs[program].locals].fill(0);
        self.resume_behavior(id);
        Ok(id)
    }

    pub(super) fn run_release_behavior(&mut self, note: NoteId, musical: bool) {
        // Consume before execution: faults can re-enter release cleanup, and must
        // neither start another callback nor relinquish the reservation twice.
        if !std::mem::take(&mut self.release_times[note.0.index].release_behavior) {
            return;
        }
        self.behaviors.unreserve(1);
        if musical {
            let plan = self.notes.get(note.0).unwrap().plan;
            let program = self
                .plans
                .get(plan.0)
                .unwrap()
                .prepared
                .release_program
                .unwrap();
            self.start_behavior_now(note, program)
                .expect("owned release continuation reservation");
        }
    }

    pub fn behavior_outcome(&self, id: BehaviorId) -> Result<Option<Outcome>, Error> {
        Ok(self.behaviors.get(id.0).ok_or(Error::StaleHandle)?.outcome)
    }

    /// Callback-local integer state remains readable through waits and completion
    /// backpressure. Handles and register bounds are checked before indexing.
    pub fn behavior_local(&self, id: BehaviorId, local: u16) -> Result<i64, Error> {
        let c = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let local = usize::from(local);
        let plan = &self
            .plans
            .get(self.notes.get(c.note.0).unwrap().plan.0)
            .unwrap()
            .prepared;
        if local >= plan.programs[c.program].locals {
            return Err(Error::InvalidInput);
        }
        self.behavior_locals
            .get(id.0.index * self.behavior_stride + local)
            .copied()
            .ok_or(Error::InvalidInput)
    }

    /// Native abort policy: release the originating note and linked children.
    /// Pending work cancels without queue admission; existing envelope tails finish.
    pub fn cancel_behavior(&mut self, id: BehaviorId) -> Result<(), Error> {
        self.apply_due();
        let c = self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?;
        if c.outcome.is_none() {
            c.outcome = Some(Outcome::Cancelled);
            let note = c.note;
            self.release_now(note, super::ReleaseCause::BehaviorCancelled)?;
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
            let plan = &self
                .plans
                .get(self.notes.get(c.note.0).unwrap().plan.0)
                .unwrap()
                .prepared;
            let Some(op) = plan.programs[c.program].code.get(c.pc).copied() else {
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
        let plan = &self
            .plans
            .get(self.notes.get(c.note.0).unwrap().plan.0)
            .unwrap()
            .prepared;
        if c.pc == plan.programs[c.program].code.len() {
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
            Instruction::SetLocal { local, value } => *self.local_cell_mut(id, local)? = value,
            Instruction::AddLocal { local, value } => {
                let cell = self.local_cell_mut(id, local)?;
                *cell = cell.checked_add(value).ok_or(Error::ArithmeticOverflow)?;
            }
            Instruction::ReadKey { local } => {
                let key = self
                    .notes
                    .get(note.0)
                    .ok_or(Error::StaleHandle)?
                    .pitch
                    .key();
                *self.local_cell_mut(id, local)? = i64::from(key);
            }
            Instruction::ReadNoteCell { local, cell } => {
                *self.local_cell_mut(id, local)? = self.note_cell(note, cell)?;
            }
            Instruction::WriteNoteCell { cell, local } => {
                let index = self.note_cell_index(note, cell)?;
                self.note_values[index] = *self.local_cell_mut(id, local)?;
            }
            Instruction::Jump { target } => {
                self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?.pc = target
            }
            Instruction::JumpIfZero { local, target } => {
                if *self.local_cell_mut(id, local)? == 0 {
                    self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?.pc = target;
                }
            }
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
                if self.available_commands() == 0 {
                    return Err(Error::Capacity);
                }
                self.queue(at, Action::Resume(id));
                return Ok(true);
            }
            Instruction::Play {
                transpose,
                velocity,
                inheritance,
                duration,
            } => {
                let n = self.notes.get(note.0).unwrap();
                let pitch = n.pitch.transpose(transpose)?;
                let velocity = match velocity {
                    Velocity::Scale(scale) => n.velocity * scale,
                    Velocity::Fixed(value) => value,
                };
                let frames = match duration {
                    Duration::Gate => None,
                    Duration::Frames(frames) | Duration::FramesOrGate(frames) => Some(frames),
                };
                let at = frames
                    .map(|frames| {
                        self.now
                            .checked_add(u64::from(frames))
                            .ok_or(Error::ClockOverflow)
                    })
                    .transpose()?;
                if at.is_some_and(|at| at != self.now) && self.available_commands() == 0 {
                    return Err(Error::Capacity);
                }
                let linked = !matches!(duration, Duration::Frames(_));
                self.reclaim_internal_notes(0);
                // Protect the duration command while child selection reserves its
                // own later release families and commands. Neither may consume the other.
                let command = usize::from(at.is_some_and(|at| at != self.now));
                self.reserved_commands += command;
                let child = self.trigger_child(note, pitch, velocity, linked, inheritance);
                self.reserved_commands -= command;
                let child = child?;
                if let Some(at) = at {
                    self.release_at(child, at)?;
                }
            }
        }
        Ok(false)
    }

    /// Note-owned integer state survives callback completion and release until the
    /// logical note retires. Its original prepared plan defines the cell bounds.
    pub fn note_cell(&self, note: NoteId, cell: u16) -> Result<i64, Error> {
        let index = self.note_cell_index(note, cell)?;
        Ok(self.note_values[index])
    }

    fn note_cell_index(&self, note: NoteId, cell: u16) -> Result<usize, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if usize::from(cell) >= self.plans.get(n.plan.0).unwrap().prepared.note_cells {
            return Err(Error::InvalidInput);
        }
        Ok(note.0.index * self.note_stride + usize::from(cell))
    }

    fn local_cell_mut(&mut self, id: BehaviorId, local: u16) -> Result<&mut i64, Error> {
        // Program operands are validated at preparation; physical access is still
        // checked here so instruction execution reports a fault instead of indexing.
        self.behavior_locals
            .get_mut(id.0.index * self.behavior_stride + usize::from(local))
            .ok_or(Error::InvalidInput)
    }

    fn fail_behavior(&mut self, id: BehaviorId, outcome: Outcome) {
        let c = self.behaviors.get_mut(id.0).unwrap();
        c.outcome = Some(outcome);
        let note = c.note;
        self.release_now(note, super::ReleaseCause::BehaviorFault)
            .expect("continuation retains originating note");
    }
}
