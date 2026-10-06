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
    /// Independent gate; retire after voices/tails and all other owned work finish.
    /// No synthetic note-off or release samples at source completion. Loops may run forever.
    UntilSilent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationValue {
    Fixed(Duration),
    /// Positive sample-frame count in an integer register, bounded by u32::MAX.
    Frames(u16),
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
    ForwardReleaseGroups,
    SuppressAttack,
    SuppressRelease,
    /// Accept or consume a controller event before downstream state changes.
    ForwardController,
    SuppressController,
    ReadControllerNumber {
        local: u16,
    },
    /// Captured input address, independent of the downstream target channel mask.
    ReadControllerPort {
        local: u16,
    },
    ReadControllerGroup {
        local: u16,
    },
    ReadControllerChannel {
        local: u16,
    },
    /// The captured full-resolution value of this callback's input event.
    ReadControllerValue {
        local: u16,
    },
    /// Latest input value in the callback's performance domain, including consumed CCs.
    ReadInputController {
        controller: u16,
        local: u16,
    },
    /// Publish a full-resolution CC downstream without reentering the creating callback.
    WriteController {
        controller: u16,
        value: u16,
    },
    ControllerToMidi7 {
        local: u16,
    },
    ControllerFromMidi7 {
        local: u16,
    },
    /// Generate a mapped child with an explicit release policy.
    Play {
        transpose: i8,
        velocity: Velocity,
        inheritance: Inheritance,
        duration: Duration,
    },
    /// Generate a key-mapped event from integer registers in a routed callback. Key 0..127,
    /// MIDI 1 velocity 1..127; duration policy is explicit.
    PlayMidi {
        key: u16,
        velocity: u16,
        duration: DurationValue,
        /// Optional nonnegative source-time offset in microseconds; None means zero.
        offset_micros: Option<u16>,
        inheritance: Inheritance,
        /// Optional source-event ID destination; aliasing an argument is allowed.
        result: Option<u16>,
    },
    ReadEventId {
        local: u16,
    },
    /// Key-up a source ID in this program's plan. Unknown/closed IDs are no-ops.
    /// None preserves a generated fixed duration; Some replaces it with a nonnegative
    /// frame delay, including zero. May run in a plan-owned control callback.
    KeyUpEvent {
        event: u16,
        delay: Option<u16>,
    },
    /// Quantize script-visible velocity to nearest MIDI 1 value without changing it.
    ReadVelocity7 {
        local: u16,
    },
    /// Replace script-visible pitch with a tuning-table key in 0..127.
    /// None targets the callback note; Some reads a plan-scoped source ID.
    /// Unknown/retired IDs are no-ops; running audio keeps its committed properties.
    WriteEventKey {
        event: Option<u16>,
        local: u16,
    },
    /// Replace script-visible velocity with a MIDI 1 value in 1..127.
    WriteEventVelocity7 {
        event: Option<u16>,
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
    /// Read the script-visible region-selection key.
    ReadKey {
        local: u16,
    },
    /// Downstream logical key state, independently of raw input and sustained gate.
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
    /// Select a group from a register, or all declared groups with None.
    /// Pending-only edits become no-ops once the original attack is forwarded.
    WriteGroup {
        group: Option<u16>,
        allowed: bool,
        pending_only: bool,
    },
    ReadGroupCount {
        local: u16,
    },
    ReadScriptCell {
        local: u16,
        cell: u32,
    },
    WriteScriptCell {
        cell: u32,
        local: u16,
    },
    ReadScriptArray {
        array: super::ScriptArray,
        index: u16,
        local: u16,
    },
    WriteScriptArray {
        array: super::ScriptArray,
        index: u16,
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
    /// Reals, text, subroutines, keyed state and effects; see `ops`.
    Op(super::ops::Op),
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
impl Comparison {
    pub fn apply(self, left: i64, right: i64) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Less => left < right,
            Self::LessEqual => left <= right,
            Self::Greater => left > right,
            Self::GreaterEqual => left >= right,
        }
    }
}

pub struct Program {
    pub(super) code: Box<[Instruction]>,
    pub(super) locals: usize,
    pub(super) note_cells: usize,
    pub(super) note_base: usize,
    pub(super) script_cells: usize,
    pub(super) script_instance: Option<super::ScriptInstanceId>,
    pub(super) wait_lifetime: WaitLifetime,
    pub(super) requires_note: bool,
    pub(super) requires_controller: bool,
    pub(super) requires_performance: bool,
    pub(super) texts: Box<[super::ops::Text]>,
    pub(super) text_constants: usize,
    pub(super) script_texts: usize,
}
impl Program {
    /// Text constants addressed by `TextPart::Constant`.
    pub fn with_texts(mut self, texts: &[&str]) -> Result<Self, Error> {
        if texts.len() < self.text_constants
            || texts.iter().any(|t| t.len() > super::ops::TEXT_CAPACITY)
        {
            return Err(Error::InvalidInput);
        }
        self.texts = texts.iter().map(|t| super::ops::Text::new(t)).collect();
        Ok(self)
    }

    pub fn with_script_instance(mut self, instance: super::ScriptInstanceId) -> Self {
        self.script_instance = Some(instance);
        self
    }

    /// Whether any instruction needs a musical note context, including dead code.
    pub fn requires_note(&self) -> bool {
        self.requires_note
    }

    pub fn requires_controller(&self) -> bool {
        self.requires_controller
    }

    /// Needs a routed note/controller/UI performance domain, not a bare plan callback.
    pub fn requires_performance(&self) -> bool {
        self.requires_performance
    }

    pub fn with_wait_lifetime(mut self, lifetime: WaitLifetime) -> Self {
        self.wait_lifetime = lifetime;
        self
    }

    pub fn new(code: Vec<Instruction>) -> Result<Self, Error> {
        let mut locals = 0;
        let mut note_cells = 0;
        let mut script_cells = 0;
        let (mut script_texts, mut text_constants) = (0, 0);
        for op in &code {
            if let Instruction::Op(op) = op {
                locals = locals.max(op.locals());
                let (cells, constants) = op.texts()?;
                script_texts = script_texts.max(cells);
                text_constants = text_constants.max(constants);
                match *op {
                    super::ops::Op::Call { target } if target as usize > code.len() => {
                        return Err(Error::InvalidInput);
                    }
                    super::ops::Op::Emit { count, .. }
                        if usize::from(count) > super::ops::EFFECT_ARGS =>
                    {
                        return Err(Error::InvalidInput);
                    }
                    _ => {}
                }
            }
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
            | Instruction::ReadGroupCount { local }
            | Instruction::WriteGroup {
                group: Some(local), ..
            }
            | Instruction::ReadControllerNumber { local }
            | Instruction::ReadControllerPort { local }
            | Instruction::ReadControllerGroup { local }
            | Instruction::ReadControllerChannel { local }
            | Instruction::ReadControllerValue { local }
            | Instruction::ControllerToMidi7 { local }
            | Instruction::ControllerFromMidi7 { local }
            | Instruction::ReadEventId { local }
            | Instruction::ReadVelocity7 { local }
            | Instruction::WriteEventKey { local, .. }
            | Instruction::WriteEventVelocity7 { local, .. }
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
                duration,
                offset_micros,
                result,
                ..
            } = *op
            {
                locals = locals.max(usize::from(key.max(velocity)) + 1);
                if let Some(offset) = offset_micros {
                    locals = locals.max(usize::from(offset) + 1);
                }
                if let Some(result) = result {
                    locals = locals.max(usize::from(result) + 1);
                }
                if let DurationValue::Frames(frames) = duration {
                    locals = locals.max(usize::from(frames) + 1);
                }
            }
            if let Instruction::CompareLocal { lhs, rhs, .. }
            | Instruction::Binary32 { lhs, rhs, .. } = *op
            {
                locals = locals.max(usize::from(lhs.max(rhs)) + 1);
            }
            if let Instruction::ReadInputController { controller, local }
            | Instruction::WriteController {
                controller,
                value: local,
            } = *op
            {
                locals = locals.max(usize::from(controller.max(local)) + 1);
            }
            if let Instruction::WriteEventKey {
                event: Some(event), ..
            }
            | Instruction::WriteEventVelocity7 {
                event: Some(event), ..
            } = *op
            {
                locals = locals.max(usize::from(event) + 1);
            }
            if let Instruction::KeyUpEvent { event, delay } = *op {
                locals = locals.max(usize::from(event.max(delay.unwrap_or(event))) + 1);
            }
            if let Instruction::ReadNoteCell { cell, .. }
            | Instruction::WriteNoteCell { cell, .. } = *op
            {
                note_cells = note_cells.max(usize::from(cell) + 1);
            }
            if let Instruction::ReadScriptCell { cell, .. }
            | Instruction::WriteScriptCell { cell, .. } = *op
            {
                let end = usize::try_from(cell)
                    .ok()
                    .and_then(|cell| cell.checked_add(1))
                    .ok_or(Error::Capacity)?;
                script_cells = script_cells.max(end);
            }
            if let Instruction::ReadScriptArray {
                array,
                index,
                local,
            }
            | Instruction::WriteScriptArray {
                array,
                index,
                local,
            } = *op
            {
                locals = locals.max(usize::from(index.max(local)) + 1);
                script_cells = script_cells.max(array.end()?);
            }
        }
        let requires_note = code.iter().any(|op| {
            matches!(
                op,
                Instruction::ForwardAttack
                    | Instruction::ForwardReleaseGroups
                    | Instruction::SuppressAttack
                    | Instruction::SuppressRelease
                    | Instruction::Play { .. }
                    | Instruction::PlayMidi {
                        inheritance: Inheritance::Linked | Inheritance::Snapshot,
                        ..
                    }
                    | Instruction::PlayMidi {
                        duration: DurationValue::Fixed(Duration::Gate | Duration::FramesOrGate(_)),
                        ..
                    }
                    | Instruction::ReadEventId { .. }
                    | Instruction::ReadVelocity7 { .. }
                    | Instruction::WriteEventKey { event: None, .. }
                    | Instruction::WriteEventVelocity7 { event: None, .. }
                    | Instruction::WriteGroup { .. }
                    | Instruction::ReadKey { .. }
                    | Instruction::ReadKeyDown { .. }
                    | Instruction::ReadNoteCell { .. }
                    | Instruction::WriteNoteCell { .. }
            )
        });
        let requires_controller = code.iter().any(|op| {
            matches!(
                op,
                Instruction::ForwardController
                    | Instruction::SuppressController
                    | Instruction::ReadControllerNumber { .. }
                    | Instruction::ReadControllerPort { .. }
                    | Instruction::ReadControllerGroup { .. }
                    | Instruction::ReadControllerChannel { .. }
                    | Instruction::ReadControllerValue { .. }
            )
        });
        let requires_performance = requires_controller
            || code.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ReadInputController { .. }
                        | Instruction::WriteController { .. }
                        | Instruction::PlayMidi { .. }
                )
            });
        if requires_note && requires_controller {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            requires_performance,
            requires_controller,
            requires_note,
            code: code.into_boxed_slice(),
            locals,
            note_cells,
            note_base: 0,
            script_cells,
            script_instance: None,
            wait_lifetime: WaitLifetime::Gate,
            texts: Box::new([]),
            text_constants,
            script_texts,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehaviorId(pub(super) Handle);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Finished,
    Cancelled,
    /// Still running after one second of preemption (a runaway loop).
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
    pub(super) fn note(self) -> Result<NoteId, Error> {
        match self {
            Self::Note(note) => Ok(note),
            Self::Plan(_) => Err(Error::InvalidInput),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) enum NoteStage {
    Attack(usize),
    Release(usize),
}
impl NoteStage {
    pub fn index(self) -> usize {
        match self {
            Self::Attack(stage) | Self::Release(stage) => stage,
        }
    }
    pub fn groups(self) -> super::groups::GroupView {
        match self {
            Self::Attack(stage) => super::groups::GroupView::Note(stage),
            Self::Release(stage) => super::groups::GroupView::Release(stage),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum PlanContext {
    Bare,
    Controller(super::controller_event::ControllerEvent),
    Control(super::control::ControlEvent),
}
impl PlanContext {
    fn stage(self) -> Option<usize> {
        match self {
            Self::Bare => None,
            Self::Controller(event) => Some(event.stage),
            Self::Control(event) => Some(event.stage),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Continuation {
    pub owner: BehaviorOwner,
    pub program: usize,
    pub pc: usize,
    pub outcome: Option<Outcome>,
    pub context: PlanContext,
    pub note_stage: Option<NoteStage>,
    pub frames: super::ops::Frames,
    /// Sample time of the first preemption.
    pub yielded_at: Option<u64>,
}

#[derive(Clone, Copy)]
pub(super) enum Ready {
    Resume { id: BehaviorId, fuel: usize },
    Release { note: NoteId, stage: Option<usize> },
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
        self.start_note_context(note, program, None)
    }

    pub(super) fn start_note_context(
        &mut self,
        note: NoteId,
        program: usize,
        note_stage: Option<NoteStage>,
    ) -> Result<BehaviorId, Error> {
        let id = self.admit_note_context(note, program, note_stage)?;
        self.resume_behavior(id);
        Ok(id)
    }

    pub(super) fn admit_note_context(
        &mut self,
        note: NoteId,
        program: usize,
        note_stage: Option<NoteStage>,
    ) -> Result<BehaviorId, Error> {
        let n = self.notes.get_mut(note.0).ok_or(Error::StaleHandle)?;
        let plan = &self.plans.get(n.plan.0).unwrap().prepared;
        if program >= plan.programs.len() || plan.programs[program].requires_controller {
            return Err(Error::InvalidInput);
        }
        if !n.gate() && plan.programs[program].wait_lifetime == WaitLifetime::Gate {
            return Err(Error::ClosedNote);
        }
        let work = n.work.checked_add(1).ok_or(Error::Capacity)?;
        let id = BehaviorId(self.behaviors.insert(Continuation {
            owner: BehaviorOwner::Note(note),
            context: PlanContext::Bare,
            note_stage,
            program,
            pc: 0,
            outcome: None,
            frames: Default::default(),
            yielded_at: None,
        })?);
        n.work = work;
        let begin = id.0.index * self.behavior_stride;
        self.behavior_locals[begin..begin + plan.programs[program].locals].fill(0);
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

    pub(super) fn validate_plan_context(
        &self,
        plan: super::PlanId,
        program: usize,
        context: PlanContext,
    ) -> Result<(), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let program = generation
            .prepared
            .programs
            .get(program)
            .ok_or(Error::InvalidInput)?;
        if program.requires_note
            || (program.requires_performance && matches!(context, PlanContext::Bare))
            || (program.requires_controller && !matches!(context, PlanContext::Controller(_)))
            || program.wait_lifetime != WaitLifetime::Callback
        {
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
        self.start_plan_context(plan, program, PlanContext::Bare)
    }

    pub(super) fn start_plan_context(
        &mut self,
        plan: super::PlanId,
        program: usize,
        context: PlanContext,
    ) -> Result<BehaviorId, Error> {
        self.validate_plan_context(plan, program, context)?;
        let generation = self.plans.get_mut(plan.0).unwrap();
        let id = BehaviorId(self.behaviors.insert(Continuation {
            owner: BehaviorOwner::Plan(plan),
            context,
            note_stage: None,
            program,
            pc: 0,
            outcome: None,
            frames: Default::default(),
            yielded_at: None,
        })?);
        generation.callbacks += 1;
        let begin = id.0.index * self.behavior_stride;
        self.behavior_locals[begin..begin + generation.prepared.programs[program].locals].fill(0);
        self.resume_behavior(id);
        Ok(id)
    }

    pub(super) fn behavior_stage(&self, id: BehaviorId) -> Result<usize, Error> {
        let c = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        Ok(c.note_stage
            .map(|stage| stage.index())
            .or(c.context.stage())
            .unwrap_or_else(|| match c.owner {
                BehaviorOwner::Note(note) => self.note_events[note.0.index].entry,
                BehaviorOwner::Plan(_) => 0,
            }))
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
            self.release_controller_reserve(id);
            self.behaviors.remove(id.0);
            match c.owner {
                BehaviorOwner::Note(note) => self.notes.get_mut(note.0).unwrap().work -= 1,
                BehaviorOwner::Plan(plan) => self.plans.get_mut(plan.0).unwrap().callbacks -= 1,
            }
        }
    }

    pub(super) fn resume_behavior(&mut self, id: BehaviorId) {
        self.queue_behavior(id);
        self.drain_behavior();
    }

    pub(super) fn queue_behavior(&mut self, id: BehaviorId) {
        // Behind a preempted callback of the same instrument: keep event order.
        let plan = self.behaviors.get(id.0).map(|c| c.owner);
        let plan = plan.and_then(|o| self.behavior_plan(o).ok());
        if self
            .yielded
            .iter()
            .any(|&y| self.yielded_plan(y).is_some() && self.yielded_plan(y) == plan)
        {
            self.yielded.push_back(id);
            return;
        }
        self.push_behavior_work(Ready::Resume {
            id,
            fuel: self.behavior_fuel,
        });
    }

    pub(super) fn drain_behavior(&mut self) {
        if self.dispatching_behavior {
            return;
        }
        self.dispatching_behavior = true;
        // A bounded explicit stack preserves synchronous nested callback ordering.
        // Each suspended caller retains its own remaining fuel, never a Rust frame.
        while let Some(ready) = self.behavior_ready.last().copied() {
            let index = self.behavior_ready.len() - 1;
            let (id, fuel) = match ready {
                Ready::Resume { id, fuel } => (id, fuel),
                Ready::Release { note, stage } => {
                    self.behavior_ready.pop();
                    self.note_events[note.0.index].release_queued = false;
                    if self.notes.get(note.0).unwrap().gate() {
                        if let Some(stage) = stage {
                            self.advance_release_stage(note, stage);
                        } else {
                            self.key_up_with_cause(note, None, super::ReleaseCause::Script)
                                .expect("retained generated note release");
                        }
                    }
                    self.notes.get_mut(note.0).unwrap().work -= 1;
                    continue;
                }
            };
            let c = *self.behaviors.get(id.0).unwrap();
            if c.outcome.is_some() {
                self.release_controller_reserve(id);
                self.behavior_ready.pop();
                continue;
            }
            let plan = &self
                .plans
                .get(self.behavior_plan(c.owner).unwrap().0)
                .unwrap()
                .prepared;
            let Some(op) = plan.programs[c.program].code.get(c.pc).copied() else {
                self.behaviors.get_mut(id.0).unwrap().outcome = Some(Outcome::Finished);
                self.release_controller_reserve(id);
                self.behavior_ready.remove(index);
                continue;
            };
            if fuel == 0 {
                self.behavior_ready.remove(index);
                self.yield_behavior(id);
                continue;
            }
            self.behavior_ready[index] = Ready::Resume { id, fuel: fuel - 1 };
            self.behaviors.get_mut(id.0).unwrap().pc += 1;
            match self.behavior_step(id, c.owner, op) {
                Ok(true) => {
                    if self.behaviors.get(id.0).unwrap().outcome.is_some() {
                        self.release_controller_reserve(id);
                    }
                    self.behavior_ready.remove(index);
                }
                Ok(false) => {}
                Err(error) => {
                    self.fail_behavior(id, Outcome::Fault(error));
                    self.behavior_ready.remove(index);
                }
            }
        }
        self.dispatching_behavior = false;
    }

    fn yielded_plan(&self, id: BehaviorId) -> Option<super::PlanId> {
        let c = self.behaviors.get(id.0)?;
        if c.outcome.is_some() {
            return None;
        }
        self.behavior_plan(c.owner).ok()
    }

    /// Out of fuel for this block: continue next block, unless it has been
    /// running for a second, which only a runaway loop does.
    fn yield_behavior(&mut self, id: BehaviorId) {
        let now = self.now;
        let c = self.behaviors.get_mut(id.0).unwrap();
        let since = *c.yielded_at.get_or_insert(now);
        if now - since >= u64::from(self.rate) {
            self.fail_behavior(id, Outcome::FuelExhausted);
            return;
        }
        self.preemptions += 1;
        self.longest_preempted = self.longest_preempted.max(now - since);
        // Capacity is the behavior arena's; each callback is queued at most once.
        self.yielded.push_back(id);
    }

    /// Resume preempted callbacks with fresh fuel, oldest first. A callback
    /// stays queued while an earlier one of its instrument preempted again
    /// in this pass.
    pub(super) fn resume_yielded(&mut self) {
        let pending = self.yielded.len();
        for done in 0..pending {
            let Some(id) = self.yielded.pop_front() else {
                break;
            };
            let Some(plan) = self.yielded_plan(id) else {
                continue;
            };
            let requeued = self.yielded.len() - (pending - done - 1);
            let blocked = self
                .yielded
                .iter()
                .rev()
                .take(requeued)
                .any(|&y| self.yielded_plan(y) == Some(plan));
            if blocked {
                self.yielded.push_back(id);
                continue;
            }
            self.push_behavior_work(Ready::Resume {
                id,
                fuel: self.behavior_fuel,
            });
            self.drain_behavior();
        }
    }

    pub(super) fn push_behavior_work(&mut self, ready: Ready) {
        assert!(
            self.behavior_ready.len() < self.behavior_ready.capacity(),
            "reserved callback dispatch capacity"
        );
        self.behavior_ready.push(ready);
    }

    /// true means suspended or finished, false means continue synchronously.
    fn behavior_step(
        &mut self,
        id: BehaviorId,
        owner: BehaviorOwner,
        op: Instruction,
    ) -> Result<bool, Error> {
        match op {
            Instruction::ForwardController => {
                self.forward_controller(id)?;
            }
            Instruction::SuppressController => {
                self.controller_event_mut(id)?.pending = false;
                self.release_controller_reserve(id);
            }
            Instruction::ReadControllerNumber { local } => {
                let value = self.controller_event_mut(id)?.number;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::ReadControllerPort { local } => {
                let value = self.controller_event_mut(id)?.origin.port;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::ReadControllerGroup { local } => {
                let value = self.controller_event_mut(id)?.origin.group;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::ReadControllerChannel { local } => {
                let value = self.controller_event_mut(id)?.origin.channel;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::ReadControllerValue { local } => {
                let value = self.controller_event_mut(id)?.value;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::ReadInputController { controller, local } => {
                let number = u8::try_from(*self.local_cell_mut(id, controller)?)
                    .map_err(|_| Error::InvalidInput)?;
                let value = self.behavior_input_controller(id, number)?;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }

            Instruction::WriteController { controller, value } => {
                let number = u8::try_from(*self.local_cell_mut(id, controller)?)
                    .map_err(|_| Error::InvalidInput)?;
                let value = u32::try_from(*self.local_cell_mut(id, value)?)
                    .map_err(|_| Error::InvalidInput)?;
                self.write_behavior_controller(id, number, value)?;
            }
            Instruction::ControllerToMidi7 { local } => {
                let cell = self.local_cell_mut(id, local)?;
                let value = u32::try_from(*cell).map_err(|_| Error::InvalidInput)?;
                *cell = ((u64::from(value) * 127 + u64::from(u32::MAX) / 2) / u64::from(u32::MAX))
                    as i64;
            }
            Instruction::ControllerFromMidi7 { local } => {
                let cell = self.local_cell_mut(id, local)?;
                if !(0..=127).contains(cell) {
                    return Err(Error::InvalidInput);
                }
                *cell = (*cell * i64::from(u32::MAX)) / 127;
            }
            Instruction::ForwardAttack => {
                let note = owner.note()?;
                if let Some(stage) = self.behaviors.get(id.0).unwrap().note_stage {
                    self.forward_note_stage(note, stage.index())?;
                } else {
                    self.forward_attack(note)?;
                }
            }
            Instruction::ForwardReleaseGroups => {
                if let Some(NoteStage::Release(stage)) =
                    self.behaviors.get(id.0).unwrap().note_stage
                {
                    self.forward_release_stage(owner.note()?, stage)?;
                } else {
                    self.forward_release_groups(owner.note()?)?;
                }
            }
            Instruction::SuppressAttack => {
                let note = owner.note()?;
                let stage = self.behaviors.get(id.0).unwrap().note_stage;
                if stage.is_none_or(|stage| {
                    !self
                        .plans
                        .get(self.notes.get(note.0).unwrap().plan.0)
                        .unwrap()
                        .projections
                        .get(note.0.index, stage.index())
                        .unwrap()
                        .forwarded
                }) {
                    self.suppress_attack(note)?;
                }
            }
            Instruction::SuppressRelease => {
                if let Some(NoteStage::Release(stage)) =
                    self.behaviors.get(id.0).unwrap().note_stage
                {
                    self.suppress_release_stage(owner.note()?, stage)?;
                } else {
                    self.suppress_release(owner.note()?)?;
                }
            }
            Instruction::WriteGroup {
                group,
                allowed,
                pending_only,
            } => {
                let note = owner.note()?;
                // Match pre-forward source edits without changing a running voice.
                let forwarded = if let Some(stage) = self.behaviors.get(id.0).unwrap().note_stage {
                    self.plans
                        .get(self.notes.get(note.0).unwrap().plan.0)
                        .unwrap()
                        .projections
                        .get(note.0.index, stage.index())?
                        .forwarded
                } else {
                    self.notes.get(note.0).ok_or(Error::StaleHandle)?.attack
                        == super::AttackStatus::Forwarded
                };
                if !pending_only || !forwarded {
                    let group = group
                        .map(|local| {
                            u32::try_from(*self.local_cell_mut(id, local)?)
                                .map_err(|_| Error::InvalidInput)
                        })
                        .transpose()?;
                    let view = self.behaviors.get(id.0).unwrap().note_stage.map_or(
                        super::groups::GroupView::Note(self.behavior_stage(id)?),
                        |stage| stage.groups(),
                    );
                    self.set_group_view(note, view, group, allowed)?;
                }
            }
            Instruction::ReadGroupCount { local } => {
                let plan = self.behavior_plan(owner)?;
                let count = self.plans.get(plan.0).unwrap().prepared.group_count;
                *self.local_cell_mut(id, local)? = i64::from(count);
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
            Instruction::ReadEventId { local } => {
                let value = self.source_event_id(owner.note()?)?;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::KeyUpEvent { event, delay } => {
                let event = i32::try_from(*self.local_cell_mut(id, event)?)
                    .map_err(|_| Error::InvalidInput)?;
                let frames = delay
                    .map(|local| {
                        u32::try_from(*self.local_cell_mut(id, local)?)
                            .map_err(|_| Error::InvalidInput)
                    })
                    .transpose()?;
                let at = self
                    .now
                    .checked_add(u64::from(frames.unwrap_or(0)))
                    .ok_or(Error::ClockOverflow)?;
                let plan = self.behavior_plan(owner)?;
                if let Some(note) = self.resolve_source_event(plan, event)?
                    && (frames.is_some() || !self.note_events[note.0.index].fixed_duration)
                {
                    if self.key_down(note)? {
                        self.replace_script_key_up_at(note, at)?;
                    } else if self.release_times[note.0.index].held {
                        self.replace_release_forward_at(note, at)?;
                    }
                }
            }
            Instruction::ReadVelocity7 { local } => {
                let note = self
                    .note_event_at(owner.note()?, self.behavior_stage(id)?)?
                    .ok_or(Error::InvalidInput)?;
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
                let key = self
                    .note_event_at(owner.note()?, self.behavior_stage(id)?)?
                    .ok_or(Error::InvalidInput)?
                    .pitch
                    .key();
                *self.local_cell_mut(id, local)? = i64::from(key);
            }
            Instruction::WriteEventKey {
                event: target,
                local,
            }
            | Instruction::WriteEventVelocity7 {
                event: target,
                local,
            } => {
                let value = *self.local_cell_mut(id, local)?;
                let key = matches!(op, Instruction::WriteEventKey { .. });
                if !(i64::from(!key)..=127).contains(&value) {
                    return Err(Error::InvalidInput);
                }
                let note = match target {
                    None => owner.note()?,
                    Some(local) => {
                        let source = i32::try_from(*self.local_cell_mut(id, local)?)
                            .map_err(|_| Error::InvalidInput)?;
                        let Some(note) =
                            self.resolve_source_event(self.behavior_plan(owner)?, source)?
                        else {
                            return Ok(false);
                        };
                        note
                    }
                };
                let stage = self.behavior_stage(id)?;
                let Some(mut event) = self.note_event_at(note, stage)? else {
                    return Ok(false);
                };
                if key {
                    event.pitch = super::NotePitch::Key(value as u8);
                } else {
                    event.velocity = value as f64 / 127.;
                }
                self.edit_note_event_at(note, stage, event)?;
            }
            Instruction::ReadNoteCell { local, cell } => {
                let index = self.behavior_note_cell_index(id, cell)?;
                *self.local_cell_mut(id, local)? = self.note_values[index];
            }
            Instruction::ReadKeyDown { local } => {
                let note = owner.note()?;
                let down = if let Some(stage) = self.behaviors.get(id.0).unwrap().note_stage {
                    self.plans
                        .get(self.notes.get(note.0).unwrap().plan.0)
                        .unwrap()
                        .projections
                        .get(note.0.index, stage.index())?
                        .release
                        == super::note_event::ReleaseStage::Unreached
                } else {
                    self.key_down(note)?
                };
                *self.local_cell_mut(id, local)? = i64::from(down);
            }
            Instruction::CompareLocal {
                lhs,
                rhs,
                comparison,
            } => {
                let right = *self.local_cell_mut(id, rhs)?;
                let left = self.local_cell_mut(id, lhs)?;
                *left = i64::from(comparison.apply(*left, right));
            }
            Instruction::WriteNoteCell { cell, local } => {
                let index = self.behavior_note_cell_index(id, cell)?;
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
            Instruction::ReadScriptArray {
                array,
                index,
                local,
            } => {
                let cell = array.cell(*self.local_cell_mut(id, index)?)?;
                let value = *self.behavior_script_cell_mut(id, cell)?;
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::WriteScriptArray {
                array,
                index,
                local,
            } => {
                let cell = array.cell(*self.local_cell_mut(id, index)?)?;
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
            Instruction::Op(op) => return self.op_step(id, owner, op),
            Instruction::PlayMidi {
                key,
                velocity,
                duration,
                offset_micros,
                inheritance,
                result,
            } => {
                let key = *self.local_cell_mut(id, key)?;
                let velocity = *self.local_cell_mut(id, velocity)?;
                let offset_micros = offset_micros.map_or(Ok(0), |local| {
                    u32::try_from(*self.local_cell_mut(id, local)?).map_err(|_| Error::InvalidInput)
                })?;
                let duration = match duration {
                    DurationValue::Fixed(duration) => duration,
                    DurationValue::Frames(local) => {
                        let frames = u32::try_from(*self.local_cell_mut(id, local)?)
                            .map_err(|_| Error::InvalidInput)?;
                        if frames == 0 {
                            return Err(Error::InvalidInput);
                        }
                        Duration::Frames(frames)
                    }
                };
                if !(0..128).contains(&key) || !(1..128).contains(&velocity) {
                    return Err(Error::InvalidInput);
                }
                // Reserve the external identity before publishing any child/audio.
                // Failed admission may leave a numeric gap, never a reused ID.
                let source_id = result
                    .map(|_| self.reserve_source_id(self.behavior_plan(owner)?))
                    .transpose()?;
                let child = self.play_behavior(
                    id,
                    super::NotePitch::Key(key as u8),
                    velocity as f64 / 127.,
                    inheritance,
                    duration,
                    offset_micros,
                )?;
                if let (Some(local), Some(source_id)) = (result, source_id) {
                    self.publish_source_id(child, source_id)?;
                    *self.local_cell_mut(id, local)? = i64::from(source_id);
                }
            }
            Instruction::Play {
                transpose,
                velocity,
                inheritance,
                duration,
            } => {
                let note = owner.note()?;
                let n = self
                    .note_event_at(note, self.behavior_stage(id)?)?
                    .ok_or(Error::InvalidInput)?;
                let pitch = n.pitch.transpose(transpose)?;
                let velocity = match velocity {
                    Velocity::Scale(scale) => n.velocity * scale,
                    Velocity::Fixed(value) => value,
                };
                self.play_behavior(id, pitch, velocity, inheritance, duration, 0)?;
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
        id: BehaviorId,
        pitch: super::NotePitch,
        velocity: f64,
        inheritance: Inheritance,
        duration: Duration,
        offset_micros: u32,
    ) -> Result<NoteId, Error> {
        let frames = match duration {
            Duration::Gate | Duration::UntilSilent => None,
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
        let linked = matches!(duration, Duration::Gate | Duration::FramesOrGate(_));
        let callback = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let source_stage = callback
            .note_stage
            .or(callback.context.stage().map(NoteStage::Attack));
        let origin = match callback.owner {
            BehaviorOwner::Note(note) => super::NoteOrigin::Child(note, linked, inheritance),
            BehaviorOwner::Plan(plan) => {
                if linked || inheritance != Inheritance::Independent {
                    return Err(Error::InvalidInput);
                }
                let (origin, performance) = if let PlanContext::Controller(event) = callback.context
                {
                    (event.origin, event.performance)
                } else if let PlanContext::Control(event) = callback.context {
                    (event.origin, event.performance)
                } else {
                    return Err(Error::InvalidInput);
                };
                super::NoteOrigin::Generated(plan, origin, performance)
            }
        };
        self.reclaim_internal_notes(super::ReleaseReserve::default());
        // Protect the duration command while child selection reserves its
        // own later release families and commands. Neither may consume the other.
        let command = usize::from(at.is_some_and(|at| at != self.now));
        self.reserved_commands += command;
        let ready_begin = self.behavior_ready.len();
        let child = self.select(origin, pitch, velocity, offset_micros, source_stage);
        self.reserved_commands -= command;
        let child = child?;
        if linked && let Some(stage) = source_stage {
            let parent = callback.owner.note()?;
            self.notes.get_mut(child.0).unwrap().release_link =
                super::ReleaseLink::Stage(stage.index());
            let plan = self.notes.get(parent.0).unwrap().plan;
            if self
                .plans
                .get(plan.0)
                .unwrap()
                .projections
                .get(parent.0.index, stage.index())?
                .release
                != super::note_event::ReleaseStage::Unreached
            {
                self.queue_note_release(child, None, ready_begin);
            }
        }
        self.note_events[child.0.index].fixed_duration = frames.is_some();
        self.notes.get_mut(child.0).unwrap().retire_when_silent = duration == Duration::UntilSilent;
        if let Some(at) = at {
            self.schedule_event(at, super::Event::ScriptKeyUp(child))?;
        }
        Ok(child)
    }

    /// Note-owned integer state survives callback completion and release until the
    /// logical note retires. This inspects the flattened native layout; use
    /// program_note_cell for a program's script-instance namespace.
    pub fn note_cell(&self, note: NoteId, cell: u16) -> Result<i64, Error> {
        let index = self.note_cell_index(note, usize::from(cell))?;
        Ok(self.note_values[index])
    }

    /// Read a cell in a program's script-instance namespace, using the note's
    /// original plan. Programs in one instance share cells; other instances do not.
    pub fn program_note_cell(&self, note: NoteId, program: usize, cell: u16) -> Result<i64, Error> {
        let index = self.program_note_cell_index(note, program, cell)?;
        Ok(self.note_values[index])
    }

    fn behavior_note_cell_index(&self, id: BehaviorId, cell: u16) -> Result<usize, Error> {
        let c = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        self.program_note_cell_index(c.owner.note()?, c.program, cell)
    }

    fn program_note_cell_index(
        &self,
        note: NoteId,
        program: usize,
        cell: u16,
    ) -> Result<usize, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        let p = self
            .plans
            .get(n.plan.0)
            .unwrap()
            .prepared
            .programs
            .get(program)
            .ok_or(Error::InvalidInput)?;
        if usize::from(cell) >= p.note_cells {
            return Err(Error::InvalidInput);
        }
        self.note_cell_index(note, p.note_base + usize::from(cell))
    }

    fn note_cell_index(&self, note: NoteId, cell: usize) -> Result<usize, Error> {
        let n = self.notes.get(note.0).ok_or(Error::StaleHandle)?;
        if cell >= self.plans.get(n.plan.0).unwrap().prepared.note_cells {
            return Err(Error::InvalidInput);
        }
        Ok(note.0.index * self.note_stride + cell)
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
        self.release_controller_reserve(id);
    }
}
