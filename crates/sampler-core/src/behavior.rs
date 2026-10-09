//! Native bounded musical instructions, independent of any vendor language VM.
use super::{Action, Error, Handle, Inheritance, NoteId, Runtime};
use std::{ops::Range, sync::Arc};

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
    /// Script units: pitch bend -8192..8191; other controllers 0..127.
    ControllerToScript {
        controller: u16,
        local: u16,
    },
    ControllerFromScript {
        controller: u16,
        local: u16,
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
    /// Enumerate live source IDs of this prepared generation, ascending; append a zero sentinel.
    ReadEventIds {
        array: super::ScriptArray,
    },
    ReadEventMark {
        event: u16,
        mark: u16,
        local: u16,
    },
    WriteEventMark {
        event: u16,
        mark: u16,
        delete: bool,
    },
    ReadEventGroup {
        event: u16,
        group: u16,
        local: u16,
    },
    /// Read one eligible physical group index, or the dynamic length when index is None.
    ReadAffectedGroup {
        index: Option<u16>,
        local: u16,
    },
    ResetReleaseCounter {
        event: u16,
    },
    ReadCallbackId {
        local: u16,
    },
    StopWait {
        callback: u16,
        disable: u16,
    },
    /// Key-up a source ID in this program's plan. Unknown/closed IDs are no-ops.
    /// None preserves a generated fixed duration; Some replaces it with a nonnegative
    /// frame delay, including zero. May run in a plan-owned control callback.
    KeyUpEvent {
        event: u16,
        delay: Option<u16>,
    },
    /// Discard another source without key-up; an aliased current ID still suppresses its callback event.
    DiscardEvent {
        event: u16,
        current_release: bool,
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
    /// Wait for the microseconds in a register. The overshoot of the frame
    /// rounding is credited to the callback's next wait, so a loop of `wait(1)` costs 1 us each, as in
    /// Kontakt, instead of a frame each.
    WaitMicros {
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
    /// `set_event_par_arr(event, $EVENT_PAR_ALLOW_GROUP, allowed, group)`: edit
    /// the selection of a note this callback played and that has not started
    /// yet (it starts when the callback waits or ends). `group` None is
    /// `$ALL_GROUPS`. Edits to a note that already started do nothing.
    WriteEventGroup {
        event: u16,
        group: Option<u16>,
        allowed: u16,
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
    /// Set a runtime effect slot parameter (see [`super::SlotKind`]) addressed
    /// by locals. `Bypass` reads an integer; the gains read real bits. A slot
    /// the plan has no control for is ignored.
    WriteSlot {
        kind: super::SlotKind,
        group: u16,
        slot: u16,
        generic: u16,
        local: u16,
    },
    /// Route group `group` to the bus at source address `address`
    /// (`set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, ...)`). Voices that start
    /// afterwards use it.
    WriteGroupBus {
        group: u16,
        address: u16,
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
    /// Set (or with `relative`, add to) a script voice parameter. `index` holds
    /// the source event ID or group index (see [`super::ParamScope`]); `target`
    /// is Decibels (millidecibels), Pan (-1000..=1000), Pitch (millicents) or
    /// Attenuate (0..=1000 gain factor), with [`super::ModTarget`]'s laws.
    WriteParam {
        scope: super::ParamScope,
        index: u16,
        target: super::ModTarget,
        local: u16,
        relative: bool,
    },
    /// Set the "from script" modulator value (locals: source event ID,
    /// modulator id, value) that [`super::ModSource::Script`] reads.
    WriteModValue {
        event: u16,
        id: u16,
        local: u16,
    },
    /// Read it back into `local` (0 when unset).
    ReadModValue {
        event: u16,
        id: u16,
        local: u16,
    },
    /// Read what an event is doing (`get_event_par` of a built-in parameter)
    /// into `local`; 0 for a retired or unknown event.
    ReadEventInfo {
        event: u16,
        info: super::EventInfo,
        local: u16,
    },
    /// Read a script layer's own value, in `WriteParam` units.
    ReadParam {
        scope: super::ParamScope,
        index: u16,
        target: super::ModTarget,
        local: u16,
    },
    /// Set a stage of group `group`'s amplitude envelope for voices that
    /// start afterwards: frames, or for Sustain a 0..=1000 level.
    WriteEnvelope {
        group: u16,
        stage: super::EnvelopeStage,
        local: u16,
    },
    /// Fade a source event in from silence, or out from its current level,
    /// over the frame count in `frames`. With `stop`, its voices end at silence.
    FadeEvent {
        event: u16,
        frames: u16,
        out: bool,
        stop: bool,
    },
    /// Start plan-owned program `program` in this callback's plan and
    /// performance context; it runs before this callback continues. Full
    /// callback capacity skips the start.
    StartProgram {
        program: u32,
    },
    /// Start every program `Prepared::with_signal_programs` binds to `signal`.
    Signal {
        signal: u16,
    },
    /// 1 when an input key equal to `local`'s value is held on a note of
    /// this callback's plan, else 0 (KSP `%KEY_DOWN`).
    ReadKeyHeld {
        local: u16,
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

#[derive(Clone)]
pub struct Program {
    pub(super) code: Arc<[Instruction]>,
    pub(super) entry: usize,
    pub(super) ui_id: i32,
    pub(super) locals: usize,
    pub(super) note_cells: usize,
    pub(super) note_base: usize,
    pub(super) script_cells: usize,
    pub(super) script_instance: Option<super::ScriptInstanceId>,
    pub(super) source_slot: i32,
    pub(super) wait_lifetime: WaitLifetime,
    pub(super) requires_note: bool,
    pub(super) requires_controller: bool,
    pub(super) requires_performance: bool,
    pub(super) texts: Arc<[Box<str>]>,
    pub(super) engine_symbols: Arc<[(i32, u16)]>,
    pub(super) text_constants: usize,
    pub(super) script_texts: usize,
}
impl Program {
    pub fn with_engine_symbols(mut self, symbols: Vec<(i32, u16)>) -> Self {
        self.engine_symbols = symbols.into();
        self
    }

    /// Whether it may set runtime effect slot parameters. Shared engine
    /// addresses are computed in registers, so writes conservatively retain
    /// live FX lanes even when their eventual address is not an effect.
    pub fn writes_slots(&self) -> bool {
        self.code.iter().any(|op| {
            matches!(
                op,
                Instruction::WriteSlot { .. }
                    | Instruction::WriteGroupBus { .. }
                    | Instruction::Op(super::ops::Op::EngineParameter { write: true, .. })
            )
        })
    }

    /// Text constants addressed by `TextPart::Constant`.
    pub fn with_texts(mut self, texts: &[&str]) -> Result<Self, Error> {
        if texts.len() < self.text_constants
            || texts.iter().any(|t| t.len() > super::ops::TEXT_CAPACITY)
        {
            return Err(Error::InvalidInput);
        }
        self.texts = texts.iter().map(|t| Box::<str>::from(*t)).collect();
        Ok(self)
    }

    /// Offset `StartProgram` targets by `base`, for a program table that
    /// concatenates several modules.
    pub fn with_program_base(mut self, base: usize) -> Self {
        if self
            .code
            .iter()
            .any(|op| matches!(op, Instruction::StartProgram { .. }))
        {
            for op in Arc::make_mut(&mut self.code) {
                if let Instruction::StartProgram { program } = op {
                    *program = program.saturating_add(base as u32);
                }
            }
        }
        self
    }

    pub fn with_source_slot(mut self, slot: u8) -> Self {
        self.source_slot = i32::from(slot);
        self
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

    /// An entry into shared code, with admission requirements from its entire
    /// body and reachable functions, including authored dead code.
    pub fn with_entry(
        mut self,
        entry: usize,
        ranges: &[Range<usize>],
        ui_id: i32,
    ) -> Result<Self, Error> {
        if entry >= self.code.len()
            || !ranges.iter().any(|r| r.contains(&entry))
            || ranges
                .iter()
                .any(|r| r.start > r.end || r.end > self.code.len())
        {
            return Err(Error::InvalidInput);
        }
        let instructions = ranges
            .iter()
            .flat_map(|range| self.code[range.clone()].iter());
        (
            self.requires_note,
            self.requires_controller,
            self.requires_performance,
        ) = Self::requirements(instructions)?;
        self.entry = entry;
        self.ui_id = ui_id;
        Ok(self)
    }

    fn requirements<'a>(
        instructions: impl Iterator<Item = &'a Instruction> + Clone,
    ) -> Result<(bool, bool, bool), Error> {
        let requires_note = instructions.clone().any(|op| {
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
                    | Instruction::ReadAffectedGroup { .. }
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
        let requires_controller = instructions.clone().any(|op| {
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
            || instructions.clone().any(|op| {
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
        Ok((requires_note, requires_controller, requires_performance))
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
            | Instruction::ReadCallbackId { local }
            | Instruction::ReadVelocity7 { local }
            | Instruction::WriteEventKey { local, .. }
            | Instruction::WriteEventVelocity7 { local, .. }
            | Instruction::MicrosToFrames { local }
            | Instruction::WaitLocal { local }
            | Instruction::WaitMicros { local }
            | Instruction::ReadKey { local }
            | Instruction::ReadKeyDown { local }
            | Instruction::ReadKeyHeld { local }
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
            if let Instruction::ControllerToScript { controller, local }
            | Instruction::ControllerFromScript { controller, local }
            | Instruction::ReadInputController { controller, local }
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
            if let Instruction::DiscardEvent { event, .. } = *op {
                locals = locals.max(usize::from(event) + 1);
            }
            if let Instruction::WriteParam {
                index,
                target,
                local,
                ..
            }
            | Instruction::ReadParam {
                index,
                target,
                local,
                ..
            } = *op
            {
                use super::ModTarget as T;
                if !matches!(target, T::Decibels | T::Pan | T::Pitch | T::Attenuate) {
                    return Err(Error::InvalidInput);
                }
                locals = locals.max(usize::from(index.max(local)) + 1);
            }
            if let Instruction::WriteModValue { event, id, local }
            | Instruction::ReadModValue { event, id, local } = *op
            {
                locals = locals.max(usize::from(event.max(id).max(local)) + 1);
            }
            if let Instruction::ResetReleaseCounter { event } = *op {
                locals = locals.max(usize::from(event) + 1);
            }
            if let Instruction::StopWait { callback, disable } = *op {
                locals = locals.max(usize::from(callback.max(disable)) + 1);
            }
            if let Instruction::WriteEventGroup {
                event,
                group,
                allowed,
            } = *op
            {
                locals = locals.max(usize::from(event.max(allowed).max(group.unwrap_or(0))) + 1);
            }
            if let Instruction::ReadEventIds { array } = *op {
                script_cells = script_cells.max(array.end()?);
            }
            if let Instruction::ReadEventMark { event, mark, local }
            | Instruction::ReadEventGroup {
                event,
                group: mark,
                local,
            } = *op
            {
                locals = locals.max(usize::from(event.max(mark).max(local)) + 1);
            }
            if let Instruction::WriteEventMark { event, mark, .. } = *op {
                locals = locals.max(usize::from(event.max(mark)) + 1);
            }
            if let Instruction::ReadAffectedGroup { index, local } = *op {
                locals = locals.max(usize::from(index.unwrap_or(local).max(local)) + 1);
            }
            if let Instruction::ReadEventInfo { event, local, .. } = *op {
                locals = locals.max(usize::from(event.max(local)) + 1);
            }
            if let Instruction::WriteGroupBus { group, address } = *op {
                locals = locals.max(usize::from(group.max(address)) + 1);
            }
            if let Instruction::WriteSlot {
                group,
                slot,
                generic,
                local,
                ..
            } = *op
            {
                locals = locals.max(usize::from(group.max(slot).max(generic).max(local)) + 1);
            }
            if let Instruction::WriteEnvelope { group, local, .. } = *op {
                locals = locals.max(usize::from(group.max(local)) + 1);
            }
            if let Instruction::FadeEvent { event, frames, .. } = *op {
                locals = locals.max(usize::from(event.max(frames)) + 1);
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
        let (requires_note, requires_controller, requires_performance) =
            Self::requirements(code.iter())?;
        Ok(Self {
            requires_performance,
            requires_controller,
            requires_note,
            code: code.into(),
            entry: 0,
            ui_id: 0,
            locals,
            note_cells,
            note_base: 0,
            script_cells,
            script_instance: None,
            source_slot: -1,
            wait_lifetime: WaitLifetime::Gate,
            texts: Arc::from([]),
            engine_symbols: Arc::from([]),
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
    /// Frames the waits so far overshot their exact time by, in micro-frames (< 1_000_000).
    pub wait_carry: u32,
    pub callback_id: i32,
    pub waiting: bool,
    pub disable_wait: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Ready {
    Resume { id: BehaviorId, fuel: usize },
    Release { note: NoteId, stage: Option<usize> },
}

impl Runtime {
    /// Run against an existing logical note. A caller can suppress default playback
    /// by admitting with note_on instead of trigger. Completion owns a private pin
    /// until flush_behaviors accepts it, including synchronous completion/failure,
    /// except that admission may reclaim a finished one (see behavior_room).
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
        n.work.checked_add(1).ok_or(Error::Capacity)?;
        if !self.behavior_room(1) {
            return Err(Error::Capacity);
        }
        let n = self.notes.get_mut(note.0).unwrap();
        let work = n.work + 1;
        let plan = &self.plans.get(n.plan.0).unwrap().prepared;
        let id = BehaviorId(self.behaviors.insert(Continuation {
            owner: BehaviorOwner::Note(note),
            context: PlanContext::Bare,
            note_stage,
            program,
            pc: plan.programs[program].entry,
            outcome: None,
            frames: Default::default(),
            yielded_at: None,
            wait_carry: 0,
            callback_id: {
                self.last_callback_id = self
                    .last_callback_id
                    .checked_add(1)
                    .ok_or(Error::Capacity)?;
                self.last_callback_id
            },
            waiting: false,
            disable_wait: false,
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
        &mut self,
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
        if generation.callbacks == usize::MAX {
            return Err(Error::Capacity);
        }
        if !self.behavior_room(1) {
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
        let id = self.admit_plan_context(plan, program, context)?;
        self.resume_behavior(id);
        Ok(id)
    }

    /// Reserve ownership before a transaction runs any authored callback.
    pub(super) fn admit_plan_context(
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
            pc: generation.prepared.programs[program].entry,
            outcome: None,
            frames: Default::default(),
            yielded_at: None,
            wait_carry: 0,
            callback_id: {
                self.last_callback_id = self
                    .last_callback_id
                    .checked_add(1)
                    .ok_or(Error::Capacity)?;
                self.last_callback_id
            },
            waiting: false,
            disable_wait: false,
        })?);
        generation.callbacks += 1;
        let begin = id.0.index * self.behavior_stride;
        self.behavior_locals[begin..begin + generation.prepared.programs[program].locals].fill(0);
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

    /// [`Runtime::flush_behaviors`], also telling which program (in the plan's
    /// program table) each behavior ran, so a fault can be named after its callback.
    pub fn flush_behaviors_at(
        &mut self,
        mut accept: impl FnMut(BehaviorId, BehaviorOwner, Outcome, usize) -> bool,
    ) {
        self.flush_behaviors_inner(&mut |id, owner, outcome, program| {
            accept(id, owner, outcome, program)
        });
    }

    /// The latest callback fault (plan program, error), once. Allocation-free.
    pub fn take_fault(&mut self) -> Option<(usize, Error)> {
        self.fault.take()
    }

    pub fn flush_behaviors(
        &mut self,
        mut accept: impl FnMut(BehaviorId, BehaviorOwner, Outcome) -> bool,
    ) {
        self.flush_behaviors_inner(&mut |id, owner, outcome, _| accept(id, owner, outcome));
    }

    fn flush_behaviors_inner(
        &mut self,
        accept: &mut dyn FnMut(BehaviorId, BehaviorOwner, Outcome, usize) -> bool,
    ) {
        let mut next = self.behaviors.first;
        while let Some(i) = next {
            next = self.behaviors.slots[i].next;
            let Some(c) = self.behaviors.slots[i].value else {
                continue;
            };
            let Some(outcome) = c.outcome else {
                continue;
            };
            let id = BehaviorId(self.behaviors.id(i));
            if !accept(id, c.owner, outcome, c.program) {
                return;
            }
            self.release_controller_reserve(id);
            self.record_script_state_outcome(id);
            self.behaviors.remove(id.0);
            match c.owner {
                BehaviorOwner::Note(note) => self.notes.get_mut(note.0).unwrap().work -= 1,
                BehaviorOwner::Plan(plan) => self.plans.get_mut(plan.0).unwrap().callbacks -= 1,
            }
        }
    }

    /// Finished work has nothing to report, so admission takes its slot rather
    /// than fail: a chord through chained scripts never waits on the host's
    /// flush. Finished outcomes stay observable while there is room; faults and
    /// cancellations always wait for flush_behaviors. Allocation-free; the scan
    /// runs only when the arena is short.
    pub(super) fn behavior_room(&mut self, needed: usize) -> bool {
        let available = self.behaviors.available();
        if available >= needed {
            return true;
        }
        // A refused admission reclaims nothing.
        let finished = self
            .behaviors
            .slots
            .iter()
            .filter(|s| {
                s.value
                    .is_some_and(|c| c.outcome == Some(Outcome::Finished))
            })
            .count();
        if available + finished < needed {
            return false;
        }
        let mut i = 0;
        while self.behaviors.available() < needed && i < self.behaviors.slots.len() {
            if let Some(c) = self.behaviors.slots[i].value
                && c.outcome == Some(Outcome::Finished)
            {
                let id = BehaviorId(self.behaviors.id(i));
                self.release_controller_reserve(id);
                self.record_script_state_outcome(id);
                self.behaviors.remove(id.0);
                match c.owner {
                    BehaviorOwner::Note(note) => self.notes.get_mut(note.0).unwrap().work -= 1,
                    BehaviorOwner::Plan(plan) => self.plans.get_mut(plan.0).unwrap().callbacks -= 1,
                }
            }
            i += 1;
        }
        self.behaviors.available() >= needed
    }

    pub(super) fn resume_behavior(&mut self, id: BehaviorId) {
        if let Some(c) = self.behaviors.get_mut(id.0) {
            c.waiting = false;
        }
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
            // A finished callback retires at once; a stale entry is skipped.
            let Some(c) = self.behaviors.get(id.0).copied() else {
                self.behavior_ready.pop();
                continue;
            };
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
                self.flush_deferred(id);
                self.behaviors.get_mut(id.0).unwrap().outcome = Some(Outcome::Finished);
                self.release_controller_reserve(id);
                self.behavior_ready.remove(index);
                continue;
            };
            if fuel == 0 || self.block_fuel_left == 0 {
                self.behavior_ready.remove(index);
                self.yield_behavior(id);
                continue;
            }
            let ran = self.run_straight(id, fuel.min(self.block_fuel_left));
            if ran > 0 {
                self.block_fuel_left = self.block_fuel_left.saturating_sub(ran);
                self.behavior_ready[index] = Ready::Resume {
                    id,
                    fuel: fuel - ran,
                };
                continue;
            }
            self.behavior_ready[index] = Ready::Resume { id, fuel: fuel - 1 };
            self.block_fuel_left = self.block_fuel_left.saturating_sub(1);
            self.behaviors.get_mut(id.0).unwrap().pc += 1;
            let stepped = self.behavior_step(id, c.owner, op);
            if !matches!(stepped, Ok(false) | Err(Error::ClosedNote)) {
                // The callback waited, ended or faulted: notes it played start now.
                self.flush_deferred(id);
            }
            match stepped {
                Ok(true) => {
                    if self.behaviors.get(id.0).unwrap().outcome.is_some() {
                        self.release_controller_reserve(id);
                    }
                    self.behavior_ready.remove(index);
                }
                Ok(false) => {}
                // Kontakt ignores a script's call on a note that already ended
                // (play_note on a released parent, forwarding a dropped note).
                Err(Error::ClosedNote) => {}
                Err(error) => {
                    self.fail_behavior(id, Outcome::Fault(error));
                    self.behavior_ready.remove(index);
                }
            }
        }
        self.dispatching_behavior = false;
    }

    /// Run straight-line local and script-cell instructions in a tight loop,
    /// resolving the callback's locals, plan and cell bank once instead of
    /// per instruction. Stops before anything else, a would-be fault or when
    /// `fuel` is spent; the general path then runs or reports that instruction.
    /// Returns the instructions run.
    fn run_straight(&mut self, id: BehaviorId, fuel: usize) -> usize {
        let Some(c) = self.behaviors.get(id.0) else {
            return 0;
        };
        let (owner, program, mut pc) = (c.owner, c.program, c.pc);
        let Ok(plan) = self.behavior_plan(owner) else {
            return 0;
        };
        let Some(generation) = self.plans.get_mut(plan.0) else {
            return 0;
        };
        let code = &generation.prepared.programs[program].code;
        let instance = generation.prepared.programs[program].script_instance;
        let mut cells = instance
            .and_then(|i| generation.scripts.get_mut(usize::from(i.0)))
            .map(|bank| &mut bank.cells[..]);
        let base = id.0.index * self.behavior_stride;
        let Some(locals) = self
            .behavior_locals
            .get_mut(base..base + self.behavior_stride)
        else {
            return 0;
        };
        let mut steps = 0;
        macro_rules! local {
            ($l:expr) => {
                match locals.get_mut(usize::from($l)) {
                    Some(cell) => cell,
                    None => break,
                }
            };
        }
        macro_rules! cell {
            ($c:expr) => {
                match cells
                    .as_deref_mut()
                    .and_then(|cells| cells.get_mut($c as usize))
                {
                    Some(cell) => cell,
                    None => break,
                }
            };
        }
        while steps < fuel {
            let Some(op) = code.get(pc).copied() else {
                break;
            };
            let mut next = pc + 1;
            match op {
                // v1 dispatches subroutine frames inside the local interpreter loop.
                Instruction::Op(super::ops::Op::Call { target }) => {
                    let c = self.behaviors.get_mut(id.0).unwrap();
                    let depth = usize::from(c.frames.depth);
                    if depth == super::ops::CALL_DEPTH {
                        break;
                    }
                    let Ok(return_pc) = u32::try_from(next) else {
                        break;
                    };
                    c.frames.returns[depth] = return_pc;
                    c.frames.depth += 1;
                    next = target as usize;
                }
                Instruction::Op(super::ops::Op::Return) => {
                    let c = self.behaviors.get_mut(id.0).unwrap();
                    if c.frames.depth == 0 {
                        break;
                    }
                    c.frames.depth -= 1;
                    next = c.frames.returns[usize::from(c.frames.depth)] as usize;
                }
                Instruction::SetLocal { local, value } => *local!(local) = value,
                Instruction::AddLocal { local, value } => {
                    let cell = local!(local);
                    let Some(sum) = cell.checked_add(value) else {
                        break;
                    };
                    *cell = sum;
                }
                Instruction::Binary32 {
                    lhs,
                    rhs,
                    operation,
                } => {
                    let Ok(right) = i32::try_from(*local!(rhs)) else {
                        break;
                    };
                    let left = local!(lhs);
                    let Ok(value) = i32::try_from(*left) else {
                        break;
                    };
                    *left = i64::from(operation.apply(value, right));
                }
                Instruction::Unary32 { local, operation } => {
                    let cell = local!(local);
                    let Ok(value) = i32::try_from(*cell) else {
                        break;
                    };
                    *cell = i64::from(operation.apply(value));
                }
                Instruction::CompareLocal {
                    lhs,
                    rhs,
                    comparison,
                } => {
                    let right = *local!(rhs);
                    let left = local!(lhs);
                    *left = i64::from(comparison.apply(*left, right));
                }
                Instruction::Jump { target } => next = target,
                Instruction::JumpIfZero { local, target } => {
                    if *local!(local) == 0 {
                        next = target;
                    }
                }
                Instruction::ReadScriptCell { local, cell } => {
                    let value = *cell!(cell);
                    *local!(local) = value;
                }
                Instruction::WriteScriptCell { cell, local } => {
                    let value = *local!(local);
                    *cell!(cell) = value;
                }
                Instruction::ReadScriptArray {
                    array,
                    index,
                    local,
                } => {
                    let Ok(at) = array.cell(*local!(index)) else {
                        break;
                    };
                    let value = *cell!(at);
                    *local!(local) = value;
                }
                Instruction::WriteScriptArray {
                    array,
                    index,
                    local,
                } => {
                    let Ok(at) = array.cell(*local!(index)) else {
                        break;
                    };
                    let value = *local!(local);
                    *cell!(at) = value;
                }
                _ => break,
            }
            pc = next;
            steps += 1;
        }
        if steps > 0 {
            self.behaviors.get_mut(id.0).unwrap().pc = pc;
        }
        steps
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
                self.flush_deferred(id);
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
            Instruction::ControllerToScript { controller, local } => {
                let number = *self.local_cell_mut(id, controller)?;
                let cell = self.local_cell_mut(id, local)?;
                let value = u32::try_from(*cell).map_err(|_| Error::InvalidInput)?;
                *cell = if number == 128 {
                    i64::from(value >> 18) - 8192
                } else {
                    ((u64::from(value) * 127 + u64::from(u32::MAX) / 2) / u64::from(u32::MAX))
                        as i64
                };
            }
            Instruction::ControllerFromScript { controller, local } => {
                let number = *self.local_cell_mut(id, controller)?;
                let cell = self.local_cell_mut(id, local)?;
                if number == 128 {
                    if !(-8192..=8191).contains(cell) {
                        return Err(Error::InvalidInput);
                    }
                    let value = (*cell + 8192) as u32;
                    // MIDI's min/centre/max 14-bit expansion, as used by ingress.
                    let shifted = value << 18;
                    let low = value & 8191;
                    *cell = i64::from(if value <= 8192 {
                        shifted
                    } else {
                        shifted | (low << 5) | (low >> 8)
                    });
                } else {
                    if !(0..=127).contains(cell) {
                        return Err(Error::InvalidInput);
                    }
                    *cell = (*cell * i64::from(u32::MAX)) / 127;
                }
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
                self.flush_deferred(id);
            }
            Instruction::ForwardReleaseGroups => {
                if let Some(NoteStage::Release(stage)) =
                    self.behaviors.get(id.0).unwrap().note_stage
                {
                    self.forward_release_stage(owner.note()?, stage)?;
                } else {
                    self.forward_release_groups(owner.note()?)?;
                }
                self.flush_deferred(id);
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
                // A release the script itself asked for (note_off) cannot be
                // ignored: nothing would ever resume it.
                let note = owner.note()?;
                if self.note_events[note.0.index].script_stop {
                    return Ok(false);
                }
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
            Instruction::WriteEventGroup {
                event,
                group,
                allowed,
            } => {
                let plan = self.behavior_plan(owner)?;
                let event = *self.local_cell_mut(id, event)?;
                let allowed = *self.local_cell_mut(id, allowed)? != 0;
                let group = group
                    .map(|local| {
                        u32::try_from(*self.local_cell_mut(id, local)?)
                            .map_err(|_| Error::InvalidInput)
                    })
                    .transpose()?;
                let current = owner
                    .note()
                    .ok()
                    .filter(|&note| self.note_events[note.0.index].source_id_is(event))
                    .map(|note| {
                        (
                            note,
                            self.behaviors.get(id.0).unwrap().note_stage.map_or(
                                super::groups::GroupView::Note(self.behavior_stage(id).unwrap()),
                                |s| s.groups(),
                            ),
                        )
                    });
                if let Some((note, view)) = current {
                    let stage = self.behaviors.get(id.0).unwrap().note_stage;
                    let forwarded = stage.map_or(
                        self.notes.get(note.0).unwrap().attack == super::AttackStatus::Forwarded,
                        |stage| {
                            self.plans
                                .get(plan.0)
                                .unwrap()
                                .projections
                                .get(note.0.index, stage.index())
                                .unwrap()
                                .forwarded
                        },
                    );
                    let editable = match stage {
                        Some(NoteStage::Release(stage)) => {
                            self.plans
                                .get(plan.0)
                                .unwrap()
                                .projections
                                .get(note.0.index, stage)
                                .unwrap()
                                .release
                                == super::note_event::ReleaseStage::Pending
                        }
                        _ => !forwarded,
                    };
                    if editable {
                        match self.set_group_view(note, view, group, allowed) {
                            Err(Error::InvalidInput) => {}
                            result => result?,
                        }
                    }
                } else {
                    self.write_event_group(plan, event, group, allowed)?;
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
            Instruction::ResetReleaseCounter { event } => {
                let event = *self.local_cell_mut(id, event)?;
                let plan = self.behavior_plan(owner)?;
                if let Ok(event) = i32::try_from(event)
                    && let Some(note) = self.resolve_source_event(plan, event)?
                {
                    self.reset_release_counter(note)?;
                }
            }
            Instruction::ReadCallbackId { local } => {
                *self.local_cell_mut(id, local)? =
                    i64::from(self.behaviors.get(id.0).unwrap().callback_id);
            }
            Instruction::StopWait { callback, disable } => {
                let callback = *self.local_cell_mut(id, callback)?;
                let disable = *self.local_cell_mut(id, disable)? != 0;
                let plan = self.behavior_plan(owner)?;
                let target = self
                    .behaviors
                    .slots
                    .iter()
                    .enumerate()
                    .find_map(|(index, slot)| {
                        let c = slot.value?;
                        (i64::from(c.callback_id) == callback
                            && c.waiting
                            && c.outcome.is_none()
                            && self.behavior_plan(c.owner).ok() == Some(plan))
                        .then(|| BehaviorId(self.behaviors.id(index)))
                    });
                if let Some(target) = target {
                    self.commands.retain(|command| !matches!(command.action, Action::Resume(other) if other == target));
                    let c = self.behaviors.get_mut(target.0).unwrap();
                    c.waiting = false;
                    c.disable_wait = disable;
                    self.queue_behavior(target);
                }
            }
            Instruction::ReadEventId { local } => {
                let value = self.source_event_id(owner.note()?)?;
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::DiscardEvent { event, current_release } => {
                let event = i32::try_from(*self.local_cell_mut(id, event)?)
                    .map_err(|_| Error::InvalidInput)?;
                let plan = self.behavior_plan(owner)?;
                let many = event == 0x3fff_fffe || (event > 0 && event & 0x2000_0000 != 0);
                let single = if many { None } else { self.resolve_source_event(plan, event)? };
                let range = if many { 0..self.notes.slots.len() }
                    else if let Some(note) = single { note.0.index..note.0.index + 1 }
                    else { 0..0 };
                for index in range {
                    let Some(n) = self.notes.slots[index].value else { continue };
                    let note = NoteId(self.notes.id(index));
                    let selected = if many {
                        n.plan == plan && (event == 0x3fff_fffe
                            || self.note_events[index].marks & (event as u32 & 0x0fff_ffff) != 0)
                    } else { single == Some(note) };
                    if !selected { continue; }
                    if owner.note().ok() == Some(note) {
                        self.behavior_step(id, owner, if current_release {
                            Instruction::SuppressRelease
                        } else { Instruction::SuppressAttack })?;
                    } else {
                        self.discard_note(note)?;
                    }
                }
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
                // Port v1 src/ksp/runtime.rs::targets: plain ID, mark union or all.
                // Release callbacks are queued until this instruction finishes, so
                // the bounded slot scan cannot select their newly generated notes.
                let many = event == 0x3fff_fffe || (event > 0 && event & 0x2000_0000 != 0);
                let single = if many {
                    None
                } else {
                    self.resolve_source_event(plan, event)?
                };
                let range = if many {
                    0..self.notes.slots.len()
                } else if let Some(note) = single {
                    note.0.index..note.0.index + 1
                } else {
                    0..0
                };
                for index in range {
                    let Some(n) = self.notes.slots[index].value else {
                        continue;
                    };
                    let note = NoteId(self.notes.id(index));
                    let selected = if many {
                        n.plan == plan
                            && (event == 0x3fff_fffe
                                || self.note_events[index].marks & (event as u32 & 0x0fff_ffff)
                                    != 0)
                    } else {
                        single == Some(note)
                    };
                    if selected && (frames.is_some() || !self.note_events[index].fixed_duration) {
                        if self.key_down(note)? {
                            self.replace_script_key_up_at(note, at)?;
                        } else if self.release_times[index].held {
                            self.replace_release_forward_at(note, at)?;
                        }
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
                // Kontakt reads an index outside the array as 0 and drops a write
                // there instead of failing the callback (Dolce's rr table is read
                // one past its end).
                let value = match array.cell(*self.local_cell_mut(id, index)?) {
                    Ok(cell) => *self.behavior_script_cell_mut(id, cell)?,
                    Err(_) => 0,
                };
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::WriteScriptArray {
                array,
                index,
                local,
            } => {
                if let Ok(cell) = array.cell(*self.local_cell_mut(id, index)?) {
                    let value = *self.local_cell_mut(id, local)?;
                    *self.behavior_script_cell_mut(id, cell)? = value;
                }
            }
            Instruction::ReadControl { local, control } => {
                let plan = self.behavior_plan(owner)?;
                let super::ControlValue::Integer(value) = self.control_value(plan, control)? else {
                    return Err(Error::InvalidInput);
                };
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::WriteParam {
                scope,
                index,
                target,
                local,
                relative,
            } => {
                let plan = self.behavior_plan(owner)?;
                let index = *self.local_cell_mut(id, index)?;
                let value = *self.local_cell_mut(id, local)?;
                self.write_param(plan, scope, index, target, value, relative)?;
            }
            Instruction::WriteModValue {
                event,
                id: slot,
                local,
            } => {
                let plan = self.behavior_plan(owner)?;
                let event = *self.local_cell_mut(id, event)?;
                let slot = *self.local_cell_mut(id, slot)?;
                let value = *self.local_cell_mut(id, local)?;
                self.write_mod_value(plan, event, slot, value)?;
            }
            Instruction::ReadModValue {
                event,
                id: slot,
                local,
            } => {
                let plan = self.behavior_plan(owner)?;
                let event = *self.local_cell_mut(id, event)?;
                let slot = *self.local_cell_mut(id, slot)?;
                *self.local_cell_mut(id, local)? = self.read_mod_value(plan, event, slot)?;
            }
            Instruction::ReadEventIds { array } => {
                let plan = self.behavior_plan(owner)?;
                // Source aliases are normally lazy; enumeration must include a note
                // before any callback has read its EVENT_ID.
                for i in 0..self.notes.slots.len() {
                    if self.notes.slots[i].value.is_some_and(|n| n.plan == plan) {
                        self.source_event_id(NoteId(self.notes.id(i)))?;
                    }
                }
                let mut at = 0;
                for i in 0..self.source_ids.len() {
                    let (source, note) = self.source_ids[i];
                    if self.notes.get(note.0).is_some_and(|n| n.plan == plan) {
                        if at == array.len {
                            break;
                        }
                        *self.behavior_script_cell_mut(id, array.offset + at)? = i64::from(source);
                        at += 1;
                    }
                }
                if at < array.len {
                    *self.behavior_script_cell_mut(id, array.offset + at)? = 0;
                }
            }
            Instruction::ReadEventMark { event, mark, local } => {
                let plan = self.behavior_plan(owner)?;
                let event = i32::try_from(*self.local_cell_mut(id, event)?).ok();
                let mark = *self.local_cell_mut(id, mark)? as u32 & 0x0fff_ffff;
                let note = event
                    .map(|e| self.resolve_source_event(plan, e))
                    .transpose()?
                    .flatten();
                let value = note.is_some_and(|n| self.note_events[n.0.index].marks & mark != 0);
                *self.local_cell_mut(id, local)? = i64::from(value);
            }
            Instruction::WriteEventMark {
                event,
                mark,
                delete,
            } => {
                let plan = self.behavior_plan(owner)?;
                let event = i32::try_from(*self.local_cell_mut(id, event)?).ok();
                let mark = *self.local_cell_mut(id, mark)? as u32 & 0x0fff_ffff;
                if let Some(note) = event
                    .map(|e| self.resolve_source_event(plan, e))
                    .transpose()?
                    .flatten()
                {
                    let marks = &mut self.note_events[note.0.index].marks;
                    *marks = if delete {
                        *marks & !mark
                    } else {
                        *marks | mark
                    };
                }
            }
            Instruction::ReadEventGroup {
                event,
                group,
                local,
            } => {
                let plan = self.behavior_plan(owner)?;
                let event = i32::try_from(*self.local_cell_mut(id, event)?).ok();
                let group = u32::try_from(*self.local_cell_mut(id, group)?).ok();
                let note = event
                    .map(|e| self.resolve_source_event(plan, e))
                    .transpose()?
                    .flatten();
                let mut allowed = false;
                if let (Some(note), Some(group)) = (note, group) {
                    let view = self.event_group_view(id, note)?;
                    let generation = self.plans.get(plan.0).unwrap();
                    allowed = group < generation.prepared.group_count
                        && generation.groups.view(note.0.index, view)[group as usize / 64]
                            & (1 << (group % 64))
                            != 0;
                }
                *self.local_cell_mut(id, local)? = i64::from(allowed);
            }
            Instruction::ReadAffectedGroup { index, local } => {
                let note = owner.note()?;
                let view = self.event_group_view(id, note)?;
                let index = index
                    .map(|index| self.local_cell_mut(id, index).copied())
                    .transpose()?;
                let value = self.affected_group(note, view, index);
                *self.local_cell_mut(id, local)? = value;
            }
            Instruction::ReadEventInfo { event, info, local } => {
                let plan = self.behavior_plan(owner)?;
                let event = *self.local_cell_mut(id, event)?;
                *self.local_cell_mut(id, local)? = self.read_event_info(plan, event, info)?;
            }
            Instruction::ReadParam {
                scope,
                index,
                target,
                local,
            } => {
                let plan = self.behavior_plan(owner)?;
                let index = *self.local_cell_mut(id, index)?;
                *self.local_cell_mut(id, local)? = self.read_param(plan, scope, index, target)?;
            }
            Instruction::WriteEnvelope {
                group,
                stage,
                local,
            } => {
                let plan = self.behavior_plan(owner)?;
                let group = *self.local_cell_mut(id, group)?;
                let value = *self.local_cell_mut(id, local)?;
                self.write_envelope(plan, group, stage, value)?;
            }
            Instruction::ReadKeyHeld { local } => {
                let plan = self.behavior_plan(owner)?;
                let key = *self.local_cell_mut(id, local)?;
                // ponytail: scans every note slot; a per-key count if notes grow large.
                let held = (0..128).contains(&key)
                    && self.input_keys >> key & 1 == 1
                    && self
                        .notes
                        .slots
                        .iter()
                        .filter_map(|s| s.value.as_ref())
                        .any(|n| {
                            n.plan == plan
                                && n.key_down()
                                && n.input.is_some_and(|i| i64::from(i.key) == key)
                        });
                *self.local_cell_mut(id, local)? = i64::from(held);
            }
            Instruction::Signal { signal } => {
                let plan = self.behavior_plan(owner)?;
                self.signal_programs(id, plan, signal)?;
            }
            Instruction::StartProgram { program } => {
                let plan = self.behavior_plan(owner)?;
                let context = self.behaviors.get(id.0).unwrap().context;
                match self.start_plan_context(plan, program as usize, context) {
                    Ok(_) | Err(Error::Capacity) => {}
                    Err(error) => return Err(error),
                }
            }
            Instruction::FadeEvent {
                event,
                frames,
                out,
                stop,
            } => {
                let plan = self.behavior_plan(owner)?;
                let event = *self.local_cell_mut(id, event)?;
                let frames = u32::try_from(*self.local_cell_mut(id, frames)?)
                    .map_err(|_| Error::InvalidInput)?;
                self.fade_event(plan, event, frames, out, stop)?;
            }
            Instruction::WriteControl { control, local } => {
                let plan = self.behavior_plan(owner)?;
                let value = super::ControlValue::Integer(*self.local_cell_mut(id, local)?);
                self.edit_controls_now(plan, None, &[super::ControlWrite { id: control, value }])?;
            }
            Instruction::WriteSlot {
                kind,
                group,
                slot,
                generic,
                local,
            } => {
                let plan = self.behavior_plan(owner)?;
                let mut address = [0i32; 3];
                for (out, l) in address.iter_mut().zip([group, slot, generic]) {
                    *out = i32::try_from(*self.local_cell_mut(id, l)?)
                        .map_err(|_| Error::InvalidInput)?;
                }
                let [group, slot, generic] = address;
                let control = super::slot_control(kind, group, slot, generic);
                let raw = *self.local_cell_mut(id, local)?;
                let value = if kind == super::SlotKind::Bypass {
                    f64::from(raw != 0)
                } else {
                    let value = crate::ops::real(raw);
                    if value.is_nan() {
                        0.
                    } else {
                        value.clamp(0., kind.max())
                    }
                };
                let known = self
                    .plans
                    .get(plan.0)
                    .ok_or(Error::StaleHandle)?
                    .prepared
                    .control_index(control)
                    .is_ok();
                if known {
                    let value = super::ControlValue::Real(value);
                    self.edit_controls_now(
                        plan,
                        None,
                        &[super::ControlWrite { id: control, value }],
                    )?;
                }
            }
            Instruction::WriteGroupBus { group, address } => {
                let plan = self.behavior_plan(owner)?;
                let group = *self.local_cell_mut(id, group)?;
                let address = *self.local_cell_mut(id, address)?;
                if let Ok(group) = usize::try_from(group) {
                    self.plans
                        .get_mut(plan.0)
                        .ok_or(Error::StaleHandle)?
                        .script
                        .set_route(group, address);
                }
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
            Instruction::WaitMicros { local } => {
                let micros = u64::try_from(*self.local_cell_mut(id, local)?)
                    .map_err(|_| Error::InvalidInput)?;
                let c = self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?;
                // A wait never ends early: it rounds up to whole frames. Only a
                // wait shorter than a frame is credited, so `wait(1)` loops
                // cost 1 us each (as in Kontakt) instead of a frame each.
                let exact = u128::from(micros) * u128::from(self.rate);
                let credit = u128::from(c.wait_carry);
                let (frames, left) = if exact >= 1_000_000 {
                    (exact.div_ceil(1_000_000), credit)
                } else if exact <= credit {
                    (0, credit - exact)
                } else {
                    (1, 1_000_000 - (exact - credit))
                };
                let frames = u32::try_from(frames).map_err(|_| Error::ArithmeticOverflow)?;
                c.wait_carry = left as u32;
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

    /// Start the notes `id` played since it last waited, with the group
    /// selection it left them (Kontakt starts played notes when the callback
    /// yields, so scripts adjust them right after `play_note`).
    fn flush_deferred(&mut self, id: BehaviorId) {
        while let Some(at) = self.deferred.iter().position(|d| d.0 == id) {
            let (_, note, entry) = self.deferred.remove(at);
            let ready_begin = self.behavior_ready.len();
            let callbacks = std::mem::take(&mut self.note_events[note.0.index].pending_callbacks);
            if callbacks != 0 {
                self.behaviors.unreserve(callbacks);
                self.begin_note_stages(note, entry);
            } else {
                match self.commit_note_attack(note, entry) {
                    Ok(true) => {
                        let plan = self.notes.get(note.0).unwrap().plan;
                        let end = self.plans.get(plan.0).unwrap().prepared.stages.len();
                        self.project_note(note, entry, end);
                        let _ = self
                            .plans
                            .get_mut(plan.0)
                            .unwrap()
                            .projections
                            .get_mut(note.0.index, end)
                            .map(|p| p.forwarded = true);
                    }
                    Ok(false) | Err(Error::ClosedNote) => {}
                    // No room once the selection was edited: drop the note.
                    Err(_) => {
                        let _ = self.suppress_attack(note);
                    }
                }
            }
            let child = *self.notes.get(note.0).unwrap();
            if let (Some(parent), super::ReleaseLink::Stage(stage)) =
                (child.parent, child.release_link)
            {
                let projection = self.plans.get(child.plan.0).unwrap().projections
                    .get(parent.0.index, stage).unwrap();
                if projection.release != super::note_event::ReleaseStage::Unreached {
                    // A linked release follows the child's edited note route.
                    self.queue_note_release(note, None, ready_begin);
                }
            }
            self.notes.get_mut(note.0).unwrap().work -= 1;
        }
    }

    fn event_group_view(
        &self,
        id: BehaviorId,
        note: NoteId,
    ) -> Result<super::groups::GroupView, Error> {
        let callback = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        if callback.owner.note().ok() == Some(note) {
            return Ok(callback.note_stage.map_or(
                super::groups::GroupView::Note(self.behavior_stage(id)?),
                |stage| stage.groups(),
            ));
        }
        let entry = self
            .deferred
            .iter()
            .find(|d| d.1 == note)
            .map_or(self.note_events[note.0.index].entry, |d| d.2);
        Ok(super::groups::GroupView::Note(entry))
    }

    pub(crate) fn write_event_group(
        &mut self,
        plan: crate::PlanId,
        event: i64,
        group: Option<u32>,
        allowed: bool,
    ) -> Result<(), Error> {
        let Ok(event) = i32::try_from(event) else {
            return Ok(());
        };
        let Some(note) = self.resolve_source_event(plan, event)? else {
            return Ok(());
        };
        let Some(&(_, _, entry)) = self.deferred.iter().find(|d| d.1 == note) else {
            return Ok(());
        };
        match self.set_group_view(note, super::groups::GroupView::Note(entry), group, allowed) {
            // A group the instrument lacks is ignored, as in Kontakt.
            Err(Error::InvalidInput) => Ok(()),
            other => other,
        }
    }

    fn wait_behavior(&mut self, id: BehaviorId, frames: u32) -> Result<bool, Error> {
        if frames == 0 || self.behaviors.get(id.0).unwrap().disable_wait {
            return Ok(false);
        }
        let at = self
            .now
            .checked_add(u64::from(frames))
            .ok_or(Error::ClockOverflow)?;
        if self.available_commands() == 0 {
            return Err(Error::Capacity);
        }
        self.behaviors.get_mut(id.0).unwrap().waiting = true;
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
                if linked
                    || !matches!(
                        inheritance,
                        Inheritance::Independent | Inheritance::Expression
                    )
                {
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
        let child = self.select(
            origin,
            pitch,
            velocity,
            offset_micros,
            source_stage,
            Some(id),
        );
        self.reserved_commands -= command;
        let child = child?;
        if linked && let Some(stage) = source_stage {
            self.notes.get_mut(child.0).unwrap().release_link =
                super::ReleaseLink::Stage(stage.index());
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
        if let Outcome::Fault(error) = outcome {
            self.fault = Some((c.program, error));
        }
        if let BehaviorOwner::Note(note) = c.owner {
            self.release_now(note, super::ReleaseCause::BehaviorFault)
                .expect("continuation retains originating note");
        }
        self.cancel_closed_work();
        self.release_controller_reserve(id);
    }
}

#[cfg(test)]
mod shared_program_tests {
    use super::*;

    #[test]
    fn entries_share_code_and_unpadded_text_but_keep_admission_requirements() {
        let shared = Program::new(vec![
            Instruction::End,
            Instruction::ReadInputController {
                controller: 0,
                local: 0,
            },
            Instruction::End,
        ])
        .unwrap()
        .with_texts(&["short", "é"])
        .unwrap();
        let bare = shared.clone().with_entry(0, &[0..1], 1).unwrap();
        let routed = shared.clone().with_entry(1, &[1..3], 2).unwrap();
        assert!(Arc::ptr_eq(&bare.code, &routed.code));
        assert!(Arc::ptr_eq(&bare.texts, &routed.texts));
        assert_eq!(bare.texts.iter().map(|t| t.len()).sum::<usize>(), 7);
        assert!(!bare.requires_performance());
        assert!(routed.requires_performance());
        assert_eq!((bare.ui_id, routed.ui_id), (1, 2));
        assert!(shared.clone().with_entry(3, &[0..3], 0).is_err());
        assert!(shared.clone().with_entry(0, &[0..4], 0).is_err());
        assert!(shared.with_entry(0, &[], 0).is_err());
    }
}
