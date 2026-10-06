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
    /// Commit/suppress the owner's pending original attack, without another note ID.
    ForwardAttack,
    SuppressAttack,
    /// Generate a mapped child with an explicit release policy.
    Play {
        transpose: i8,
        velocity: Velocity,
        inheritance: Inheritance,
        duration: Duration,
    },
    /// Generate a timed key-mapped child from integer registers. Key 0..127,
    /// MIDI 1 velocity 1..127, duration 1..=u32::MAX sample frames.
    PlayMidi {
        key: u16,
        velocity: u16,
        frames: u16,
        inheritance: Inheritance,
    },
    /// Quantize onset velocity to nearest MIDI 1 value, without changing note state.
    ReadVelocity7 {
        local: u16,
    },
    /// Convert nonnegative microseconds to sample frames, rounding upward.
    MicrosToFrames {
        local: u16,
    },
    /// Read a nonnegative u32 sample-frame delay from a register.
    WaitLocal {
        local: u16,
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
    /// Signed-32 operands/results; rejects out-of-range native register inputs.
    Binary32 {
        lhs: u16,
        rhs: u16,
        operation: super::IntegerBinary,
    },
    Unary32 {
        local: u16,
        operation: super::IntegerUnary,
    },
    ReadKey {
        local: u16,
    },
    /// Physical key state of this owner, independently of its sustained gate.
    ReadKeyDown {
        local: u16,
    },
    /// Compare signed locals without subtraction/overflow; replace lhs with 0 or 1.
    CompareLocal {
        lhs: u16,
        rhs: u16,
        comparison: Comparison,
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
    /// Access the program's own script instance, shared across its callbacks.
    ReadScriptCell {
        local: u16,
        cell: u16,
    },
    WriteScriptCell {
        cell: u16,
        local: u16,
    },
    /// Read/write integer controls in the originating plan generation.
    ReadControl {
        local: u16,
        control: super::ControlId,
    },
    WriteControl {
        control: super::ControlId,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

pub struct Program {
    pub(super) code: Box<[Instruction]>,
    pub(super) locals: usize,
    pub(super) note_cells: usize,
    pub(super) script_cells: usize,
    pub(super) script_instance: Option<super::ScriptInstanceId>,
    pub(super) wait_lifetime: WaitLifetime,
    pub(super) requires_note: bool,
}
impl Program {
    pub fn with_script_instance(mut self, instance: super::ScriptInstanceId) -> Self {
        self.script_instance = Some(instance);
        self
    }

    /// Whether any instruction needs a musical note context, including dead code.
    pub fn requires_note(&self) -> bool {
        self.requires_note
    }

    pub fn with_wait_lifetime(mut self, lifetime: WaitLifetime) -> Self {
        self.wait_lifetime = lifetime;
        self
    }

    pub fn new(code: Vec<Instruction>) -> Result<Self, Error> {
        let mut locals = 0;
        let mut note_cells = 0;
        let mut script_cells = 0;
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
            | Instruction::Unary32 { local, .. }
            | Instruction::ReadScriptCell { local, .. }
            | Instruction::WriteScriptCell { local, .. }
            | Instruction::ReadControl { local, .. }
            | Instruction::WriteControl { local, .. }
            | Instruction::ReadVelocity7 { local }
            | Instruction::MicrosToFrames { local }
            | Instruction::WaitLocal { local }
            | Instruction::ReadKey { local }
            | Instruction::ReadKeyDown { local }
            | Instruction::ReadNoteCell { local, .. }
            | Instruction::WriteNoteCell { local, .. }
            | Instruction::JumpIfZero { local, .. } = *op
            {
                locals = locals.max(usize::from(local) + 1);
            }
            if let Instruction::PlayMidi {
                key,
                velocity,
                frames,
                ..
            } = *op
            {
                locals = locals.max(usize::from(key.max(velocity).max(frames)) + 1);
            }
            if let Instruction::CompareLocal { lhs, rhs, .. }
            | Instruction::Binary32 { lhs, rhs, .. } = *op
            {
                locals = locals.max(usize::from(lhs.max(rhs)) + 1);
            }
            if let Instruction::ReadNoteCell { cell, .. }
            | Instruction::WriteNoteCell { cell, .. } = *op
            {
                note_cells = note_cells.max(usize::from(cell) + 1);
            }
            if let Instruction::ReadScriptCell { cell, .. }
            | Instruction::WriteScriptCell { cell, .. } = *op
            {
                script_cells = script_cells.max(usize::from(cell) + 1);
            }
        }
        let requires_note = code.iter().any(|op| {
            matches!(
                op,
                Instruction::ForwardAttack
                    | Instruction::SuppressAttack
                    | Instruction::Play { .. }
                    | Instruction::PlayMidi { .. }
                    | Instruction::ReadVelocity7 { .. }
                    | Instruction::ReadKey { .. }
                    | Instruction::ReadKeyDown { .. }
                    | Instruction::ReadNoteCell { .. }
                    | Instruction::WriteNoteCell { .. }
            )
        });
        Ok(Self {
            requires_note,
            code: code.into_boxed_slice(),
            locals,
            note_cells,
            script_cells,
            script_instance: None,
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

/// A callback can retain an instrument generation without inventing a MIDI note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BehaviorOwner {
    Note(NoteId),
    Plan(super::PlanId),
}
impl BehaviorOwner {
    fn note(self) -> Result<NoteId, Error> {
        match self {
            Self::Note(note) => Ok(note),
            Self::Plan(_) => Err(Error::InvalidInput),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Continuation {
    pub owner: BehaviorOwner,
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
            owner: BehaviorOwner::Note(note),
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

    /// Execute an instrument-owned callback without reserving a note/voice. Note
    /// operands and gate-lifetime waits are rejected before acquiring ownership.
    pub fn start_plan_behavior(
        &mut self,
        plan: super::PlanId,
        program: usize,
    ) -> Result<BehaviorId, Error> {
        self.apply_due();
        self.start_plan_behavior_now(plan, program)
    }

    pub(super) fn validate_plan_behavior(
        &self,
        plan: super::PlanId,
        program: usize,
    ) -> Result<(), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let program = generation
            .prepared
            .programs
            .get(program)
            .ok_or(Error::InvalidInput)?;
        if program.requires_note || program.wait_lifetime != WaitLifetime::Callback {
            return Err(Error::InvalidInput);
        }
        if self.behaviors.available() == 0 || generation.callbacks == usize::MAX {
            return Err(Error::Capacity);
        }
        Ok(())
    }

    pub(super) fn start_plan_behavior_now(
        &mut self,
        plan: super::PlanId,
        program: usize,
    ) -> Result<BehaviorId, Error> {
        self.validate_plan_behavior(plan, program)?;
        let generation = self.plans.get_mut(plan.0).unwrap();
        let id = BehaviorId(self.behaviors.insert(Continuation {
            owner: BehaviorOwner::Plan(plan),
            program,
            pc: 0,
            outcome: None,
        })?);
        generation.callbacks += 1;
        let begin = id.0.index * self.behavior_stride;
        self.behavior_locals[begin..begin + generation.prepared.programs[program].locals].fill(0);
        self.resume_behavior(id);
        Ok(id)
    }

    pub(super) fn behavior_plan(&self, owner: BehaviorOwner) -> Result<super::PlanId, Error> {
        match owner {
            BehaviorOwner::Note(note) => self.note_plan(note),
            BehaviorOwner::Plan(plan) => {
                self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
                Ok(plan)
            }
        }
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
            .get(self.behavior_plan(c.owner).unwrap().0)
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
            if let BehaviorOwner::Note(note) = c.owner {
                self.release_now(note, super::ReleaseCause::BehaviorCancelled)?;
            }
            self.cancel_closed_work();
        }
        Ok(())
    }

    pub fn flush_behaviors(
        &mut self,
        mut accept: impl FnMut(BehaviorId, BehaviorOwner, Outcome) -> bool,
    ) {
        for i in 0..self.behaviors.slots.len() {
            let Some(c) = self.behaviors.slots[i].value else {
                continue;
            };
            let Some(outcome) = c.outcome else {
                continue;
            };
            let id = BehaviorId(self.behaviors.id(i));
            if !accept(id, c.owner, outcome) {
                return;
            }
            self.behaviors.remove(id.0);
            match c.owner {
                BehaviorOwner::Note(note) => self.notes.get_mut(note.0).unwrap().work -= 1,
                BehaviorOwner::Plan(plan) => self.plans.get_mut(plan.0).unwrap().callbacks -= 1,
            }
        }
    }

    pub(super) fn resume_behavior(&mut self, id: BehaviorId) {
        for _ in 0..self.behavior_fuel {
            let c = *self.behaviors.get(id.0).unwrap();
            if c.outcome.is_some() {
                return;
            }
            let plan = &self
                .plans
                .get(self.behavior_plan(c.owner).unwrap().0)
                .unwrap()
                .prepared;
            let Some(op) = plan.programs[c.program].code.get(c.pc).copied() else {
                self.behaviors.get_mut(id.0).unwrap().outcome = Some(Outcome::Finished);
                return;
            };
            self.behaviors.get_mut(id.0).unwrap().pc += 1;
            match self.behavior_step(id, c.owner, op) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    self.fail_behavior(id, Outcome::Fault(error));
                    return;
                }
            }
        }
        let c = *self.behaviors.get(id.0).unwrap();
        let plan = &self
            .plans
            .get(self.behavior_plan(c.owner).unwrap().0)
            .unwrap()
            .prepared;
        if c.pc == plan.programs[c.program].code.len() {
            self.behaviors.get_mut(id.0).unwrap().outcome = Some(Outcome::Finished);
        } else {
            self.fail_behavior(id, Outcome::FuelExhausted);
        }
    }

    /// true means suspended or finished, false means continue synchronously.
    fn behavior_step(
        &mut self,
        id: BehaviorId,
        owner: BehaviorOwner,
        op: Instruction,
    ) -> Result<bool, Error> {
        match op {
            Instruction::ForwardAttack => {
                self.forward_attack(owner.note()?)?;
            }
            Instruction::SuppressAttack => {
                self.suppress_attack(owner.note()?)?;
            }
            Instruction::SetLocal { local, value } => *self.local_cell_mut(id, local)? = value,
            Instruction::AddLocal { local, value } => {
                let cell = self.local_cell_mut(id, local)?;
                *cell = cell.checked_add(value).ok_or(Error::ArithmeticOverflow)?;
            }
            Instruction::Binary32 {
                lhs,
                rhs,
                operation,
            } => {
                let right = i32::try_from(*self.local_cell_mut(id, rhs)?)
                    .map_err(|_| Error::ArithmeticOverflow)?;
                let left = self.local_cell_mut(id, lhs)?;
                let value = i32::try_from(*left).map_err(|_| Error::ArithmeticOverflow)?;
                *left = i64::from(operation.apply(value, right));
            }
            Instruction::Unary32 { local, operation } => {
                let cell = self.local_cell_mut(id, local)?;
                let value = i32::try_from(*cell).map_err(|_| Error::ArithmeticOverflow)?;
                *cell = i64::from(operation.apply(value));
            }
            Instruction::ReadVelocity7 { local } => {
                let note = self.notes.get(owner.note()?.0).ok_or(Error::StaleHandle)?;
                let value = (note.velocity * 127.).round() as i64;
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::MicrosToFrames { local } => {
                let micros = u64::try_from(*self.local_cell_mut(id, local)?)
                    .map_err(|_| Error::InvalidInput)?;
                let frames = (u128::from(micros) * u128::from(self.rate)).div_ceil(1_000_000);
                let frames = u32::try_from(frames).map_err(|_| Error::ArithmeticOverflow)?;
                *self.local_cell_mut(id, local)? = i64::from(frames);
            }
            Instruction::ReadKey { local } => {
                let note = owner.note()?;
                let key = self
                    .notes
                    .get(note.0)
                    .ok_or(Error::StaleHandle)?
                    .pitch
                    .key();
                *self.local_cell_mut(id, local)? = i64::from(key);
            }
            Instruction::ReadNoteCell { local, cell } => {
                *self.local_cell_mut(id, local)? = self.note_cell(owner.note()?, cell)?;
            }
            Instruction::ReadKeyDown { local } => {
                *self.local_cell_mut(id, local)? = i64::from(self.key_down(owner.note()?)?);
            }
            Instruction::CompareLocal {
                lhs,
                rhs,
                comparison,
            } => {
                let right = *self.local_cell_mut(id, rhs)?;
                let left = self.local_cell_mut(id, lhs)?;
                *left = i64::from(match comparison {
                    Comparison::Equal => *left == right,
                    Comparison::NotEqual => *left != right,
                    Comparison::Less => *left < right,
                    Comparison::LessEqual => *left <= right,
                    Comparison::Greater => *left > right,
                    Comparison::GreaterEqual => *left >= right,
                });
            }
            Instruction::WriteNoteCell { cell, local } => {
                let index = self.note_cell_index(owner.note()?, cell)?;
                self.note_values[index] = *self.local_cell_mut(id, local)?;
            }
            Instruction::ReadScriptCell { local, cell } => {
                let value = *self.behavior_script_cell_mut(id, cell)?;
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::WriteScriptCell { cell, local } => {
                let value = *self.local_cell_mut(id, local)?;
                *self.behavior_script_cell_mut(id, cell)? = value;
            }
            Instruction::ReadControl { local, control } => {
                let plan = self.behavior_plan(owner)?;
                let super::ControlValue::Integer(value) = self.control_value(plan, control)? else {
                    return Err(Error::InvalidInput);
                };
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::WriteControl { control, local } => {
                let plan = self.behavior_plan(owner)?;
                let value = super::ControlValue::Integer(*self.local_cell_mut(id, local)?);
                self.edit_controls_now(plan, None, &[super::ControlWrite { id: control, value }])?;
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
            Instruction::WaitLocal { local } => {
                let frames = u32::try_from(*self.local_cell_mut(id, local)?)
                    .map_err(|_| Error::InvalidInput)?;
                return self.wait_behavior(id, frames);
            }
            Instruction::Wait(frames) => return self.wait_behavior(id, frames),
            Instruction::PlayMidi {
                key,
                velocity,
                frames,
                inheritance,
            } => {
                let key = *self.local_cell_mut(id, key)?;
                let velocity = *self.local_cell_mut(id, velocity)?;
                let frames = u32::try_from(*self.local_cell_mut(id, frames)?)
                    .map_err(|_| Error::InvalidInput)?;
                if !(0..128).contains(&key) || !(1..128).contains(&velocity) || frames == 0 {
                    return Err(Error::InvalidInput);
                }
                self.play_behavior(
                    owner.note()?,
                    super::NotePitch::Key(key as u8),
                    velocity as f64 / 127.,
                    inheritance,
                    Duration::Frames(frames),
                )?;
            }
            Instruction::Play {
                transpose,
                velocity,
                inheritance,
                duration,
            } => {
                let note = owner.note()?;
                let n = self.notes.get(note.0).unwrap();
                let pitch = n.pitch.transpose(transpose)?;
                let velocity = match velocity {
                    Velocity::Scale(scale) => n.velocity * scale,
                    Velocity::Fixed(value) => value,
                };
                self.play_behavior(note, pitch, velocity, inheritance, duration)?;
            }
        }
        Ok(false)
    }

    fn wait_behavior(&mut self, id: BehaviorId, frames: u32) -> Result<bool, Error> {
        if frames == 0 {
            return Ok(false);
        }
        let at = self
            .now
            .checked_add(u64::from(frames))
            .ok_or(Error::ClockOverflow)?;
        if self.available_commands() == 0 {
            return Err(Error::Capacity);
        }
        self.queue(at, Action::Resume(id));
        Ok(true)
    }

    fn play_behavior(
        &mut self,
        note: NoteId,
        pitch: super::NotePitch,
        velocity: f64,
        inheritance: Inheritance,
        duration: Duration,
    ) -> Result<(), Error> {
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
        Ok(())
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
        if let BehaviorOwner::Note(note) = c.owner {
            self.release_now(note, super::ReleaseCause::BehaviorFault)
                .expect("continuation retains originating note");
        }
        self.cancel_closed_work();
    }
}
