#![forbid(unsafe_code)]
//! Clean-sheet, control-thread KSP 8.12 source subset. No vendor VM dependency.
//! Native sample-time lowering is explicit; this is not a Kontakt fidelity claim.
use sampler_core::{
    Comparison, ControlDefinition, ControlDomain, ControlId, ControlValue, Inheritance,
    Instruction, Prepared, Program, ScriptInstanceId, WaitLifetime,
};
use std::collections::BTreeMap;
mod expression;
mod functions;
mod state;

// Opaque source constant; numeric value retained from the recorded v1 reference.
const ALL_GROUPS: i64 = 0x3fff_ffff;

pub const PROFILE: &str = "ksp-8.12-note-release-subset-v1";

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub instructions: usize,
    pub variables: usize,
    /// Total declared array elements; independent of the declaration-count budget.
    pub array_cells: usize,
}

/// Control-owned executable callbacks, UI metadata and declared state layout.
pub struct Script {
    programs: Vec<Program>,
    on_note: Option<usize>,
    on_release: Option<usize>,
    on_controller: Option<usize>,
    rate: u32,
    globals: Vec<i64>,
    note_cells: usize,
    controls: Vec<Control>,
    performance_view: bool,
}

/// Authored presentation metadata; changing presentation never replaces the value owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    Knob { display_ratio: i32 },
    Slider,
    Button,
    Switch,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub variable: String,
    pub widget: Widget,
    pub definition: ControlDefinition,
    pub callback: Option<usize>,
}

impl Script {
    pub fn has_performance_view(&self) -> bool {
        self.performance_view
    }
    pub fn controls(&self) -> &[Control] {
        &self.controls
    }

    pub fn global_cells(&self) -> usize {
        self.globals.len()
    }

    pub fn note_cells(&self) -> usize {
        self.note_cells
    }

    /// Install the complete script on a prepared instrument of the compiled rate.
    pub fn bind(self, plan: Prepared) -> Result<Prepared, sampler_core::Error> {
        if plan.sample_rate() != self.rate {
            return Err(sampler_core::Error::InvalidInput);
        }
        let callbacks = self
            .controls
            .iter()
            .filter_map(|c| c.callback.map(|program| (c.definition.id, program)))
            .collect();
        let plan = plan
            .with_programs(Vec::new(), None)?
            .with_script_instances(vec![self.globals])?
            // Keep the upper source-ID bits available for marked/all-event selectors.
            .with_source_event_limit(0x0fff_ffff)?;
        let plan = plan.with_controls(self.controls.into_iter().map(|c| c.definition).collect())?;
        let plan = plan
            .with_programs(
                self.programs
                    .into_iter()
                    .map(|p| p.with_script_instance(ScriptInstanceId(0)))
                    .collect(),
                self.on_note,
            )?
            .with_control_programs(callbacks)?;
        let plan = match self.on_release {
            Some(program) => plan.with_release_program(program)?,
            None => plan,
        };
        match self.on_controller {
            Some(program) => plan.with_controller_program(program),
            None => Ok(plan),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    /// UTF-8 byte offset into the supplied source.
    pub offset: usize,
    pub message: &'static str,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "byte {}: {}", self.offset, self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind<'a> {
    Word(&'a str),
    Number(u64),
    Hex(u32),
    Symbol(u8),
    Comparison(Comparison),
    End,
}
#[derive(Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    offset: usize,
}
#[derive(Clone, Copy)]
enum CallbackKind {
    Note,
    Release,
    Control,
    Controller,
}
impl CallbackKind {
    fn accepts(self, program: &Program) -> bool {
        match self {
            Self::Note | Self::Release => !program.requires_controller(),
            Self::Control => !program.requires_note() && !program.requires_performance(),
            Self::Controller => !program.requires_note(),
        }
    }
}

enum Block {
    If {
        branch: usize,
        has_else: bool,
    },
    While {
        start: usize,
        branch: usize,
    },
    Select {
        misses: [Option<usize>; 2],
        exits: Vec<usize>,
        has_case: bool,
    },
}
#[derive(Clone, Copy)]
enum Variable {
    Global(u32),
    Note(u16),
    Control(usize),
    Constant(i32),
    Array(sampler_core::ScriptArray),
}

struct Parser<'a> {
    source: &'a str,
    offset: usize,
    limit: usize,
    emitted: usize,
    code: Vec<Instruction>,
    functions: BTreeMap<&'a str, [Result<Vec<Instruction>, Error>; 4]>,
    function_instructions: usize,
    variables: BTreeMap<&'a str, Variable>,
    bindings: BTreeMap<&'a str, ControlId>,
    controls: Vec<Control>,
    globals: Vec<i64>,
    note_cells: usize,
    performance_view: bool,
    variable_limit: usize,
    array_limit: usize,
    array_cells: usize,
}
impl<'a> Parser<'a> {
    fn error(&self, message: &'static str) -> Error {
        Error {
            offset: self.offset,
            message,
        }
    }
    fn next(&mut self) -> Result<Token<'a>, Error> {
        let bytes = self.source.as_bytes();
        loop {
            while bytes.get(self.offset).is_some_and(u8::is_ascii_whitespace) {
                self.offset += 1;
            }
            if self.source[self.offset..].starts_with("...") {
                self.offset += 3;
                continue;
            }
            if bytes.get(self.offset) != Some(&b'{') {
                break;
            }
            let start = self.offset;
            self.offset += 1;
            loop {
                match bytes.get(self.offset) {
                    Some(b'}') => {
                        self.offset += 1;
                        break;
                    }
                    Some(b'{') => return Err(self.error("nested comments are outside this subset")),
                    Some(_) => self.offset += 1,
                    None => {
                        return Err(Error {
                            offset: start,
                            message: "unterminated comment",
                        });
                    }
                }
            }
        }
        let start = self.offset;
        let Some(&byte) = bytes.get(start) else {
            return Ok(Token {
                kind: Kind::End,
                offset: start,
            });
        };
        let kind = if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte == b'%' {
            self.offset += 1;
            while bytes
                .get(self.offset)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                self.offset += 1;
            }
            Kind::Word(
                self.source
                    .get(start..self.offset)
                    .ok_or_else(|| self.error("invalid text boundary"))?,
            )
        } else if byte.is_ascii_digit() {
            let mut end = start;
            while bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
                end += 1;
            }
            if matches!(bytes.get(end), Some(b'H' | b'h')) {
                if byte != b'0' {
                    return Err(self.error("hexadecimal literals require a leading zero"));
                }
                let value = u32::from_str_radix(&self.source[start..end], 16)
                    .map_err(|_| self.error("hexadecimal literal exceeds 32-bit range"))?;
                self.offset = end + 1;
                return Ok(Token {
                    kind: Kind::Hex(value),
                    offset: start,
                });
            }
            while bytes.get(self.offset).is_some_and(u8::is_ascii_digit) {
                self.offset += 1;
            }
            let text = self
                .source
                .get(start..self.offset)
                .ok_or_else(|| self.error("invalid text boundary"))?;
            Kind::Number(text.parse().map_err(|_| Error {
                offset: start,
                message: "integer exceeds 64-bit literal range",
            })?)
        } else if byte == b':' && bytes.get(start + 1) == Some(&b'=') {
            self.offset += 2;
            Kind::Symbol(b':')
        } else if b"=#<>".contains(&byte) {
            self.offset += 1;
            let equal = matches!(byte, b'<' | b'>') && bytes.get(self.offset) == Some(&b'=');
            self.offset += usize::from(equal);
            Kind::Comparison(match (byte, equal) {
                (b'=', _) => Comparison::Equal,
                (b'#', _) => Comparison::NotEqual,
                (b'<', false) => Comparison::Less,
                (b'<', true) => Comparison::LessEqual,
                (b'>', false) => Comparison::Greater,
                _ => Comparison::GreaterEqual,
            })
        } else if byte == b'.' {
            let operator = [".and.", ".or.", ".xor.", ".not."]
                .into_iter()
                .find(|op| self.source[start..].starts_with(op))
                .ok_or_else(|| self.error("unsupported dotted operator"))?;
            self.offset += operator.len();
            Kind::Word(operator)
        } else if b"()[],+-*/".contains(&byte) {
            self.offset += 1;
            Kind::Symbol(byte)
        } else {
            return Err(self.error("unsupported character or expression"));
        };
        Ok(Token {
            kind,
            offset: start,
        })
    }
    fn expect(&mut self, kind: Kind<'_>, message: &'static str) -> Result<(), Error> {
        let token = self.next()?;
        if token.kind != kind {
            return Err(Error {
                offset: token.offset,
                message,
            });
        }
        Ok(())
    }
    fn symbol(&mut self, symbol: u8) -> Result<(), Error> {
        self.expect(
            Kind::Symbol(symbol),
            "unexpected token in command arguments",
        )
    }
    fn emit(&mut self, op: Instruction) -> Result<(), Error> {
        if self.emitted == self.limit {
            return Err(self.error("instruction budget exceeded"));
        }
        self.code.push(op);
        self.emitted += 1;
        Ok(())
    }
    fn declarations(&mut self) -> Result<(), Error> {
        loop {
            let token = self.next()?;
            match token.kind {
                Kind::Word("end") => return self.expect(Kind::Word("on"), "expected end on"),
                Kind::Word("make_perfview") => self.performance_view = true,
                Kind::Word("declare") => {
                    let token = self.next()?;
                    let (kind, token) = match token.kind {
                        Kind::Word(
                            kind @ ("const" | "polyphonic" | "ui_knob" | "ui_slider" | "ui_button"
                            | "ui_switch"),
                        ) => (kind, self.next()?),
                        Kind::Word(name) if name.starts_with('$') => ("integer", token),
                        Kind::Word(name) if name.starts_with('%') => ("array", token),
                        _ => return Err(self.error("unsupported declaration type")),
                    };
                    let Kind::Word(name) = token.kind else {
                        return Err(Error {
                            offset: token.offset,
                            message: "expected integer variable name",
                        });
                    };
                    if !name
                        .strip_prefix(if kind == "array" { '%' } else { '$' })
                        .is_some_and(|name| {
                            name.as_bytes()
                                .first()
                                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                        })
                        || matches!(
                            name,
                            "$EVENT_NOTE"
                                | "$EVENT_VELOCITY"
                                | "$EVENT_ID"
                                | "$NOTE_HELD"
                                | "$ALL_GROUPS"
                                | "$NUM_GROUPS"
                                | "%CC"
                                | "%CC_TOUCHED"
                                | "%KEY_DOWN"
                                | "%GROUPS_AFFECTED"
                        )
                        || [
                            "$NI_",
                            "%NI_",
                            "$CONTROL_PAR_",
                            "$EVENT_PAR_",
                            "$ENGINE_PAR_",
                            "$ZONE_PAR_",
                            "$LOOP_PAR_",
                        ]
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
                    {
                        return Err(Error {
                            offset: token.offset,
                            message: "invalid or reserved variable name",
                        });
                    }
                    if self.variables.contains_key(name) {
                        return Err(Error {
                            offset: token.offset,
                            message: "duplicate variable declaration",
                        });
                    }
                    if self.variables.len() >= self.variable_limit {
                        return Err(self.error("variable budget exceeded"));
                    }
                    let variable = if kind == "const" {
                        self.symbol(b':')?;
                        Variable::Constant(self.constant()?)
                    } else if kind == "array" {
                        Variable::Array(self.array_declaration()?)
                    } else if kind == "integer" {
                        let cell = u32::try_from(self.globals.len())
                            .ok()
                            .filter(|cell| *cell < u32::MAX)
                            .ok_or_else(|| self.error("native script-cell index range exceeded"))?;
                        self.globals.push(0);
                        Variable::Global(cell)
                    } else if kind == "polyphonic" {
                        let cell = u16::try_from(self.note_cells)
                            .map_err(|_| self.error("native note-cell index range exceeded"))?;
                        self.note_cells += 1;
                        Variable::Note(cell)
                    } else {
                        let id = self.bindings.remove(name).ok_or_else(|| {
                            self.error("UI control requires a persistent identity binding")
                        })?;
                        let (min, max, widget) = match kind {
                            "ui_button" => (0, 1, Widget::Button),
                            "ui_switch" => (0, 1, Widget::Switch),
                            _ => {
                                self.symbol(b'(')?;
                                let min = self.integer()?;
                                self.symbol(b',')?;
                                let max = self.integer()?;
                                if min > max {
                                    return Err(self.error("reversed control range"));
                                }
                                let widget = if kind == "ui_knob" {
                                    self.symbol(b',')?;
                                    let display_ratio = self.integer()?;
                                    if display_ratio == 0 {
                                        return Err(self.error("zero knob display ratio"));
                                    }
                                    Widget::Knob { display_ratio }
                                } else {
                                    Widget::Slider
                                };
                                self.symbol(b')')?;
                                (min, max, widget)
                            }
                        };
                        let index = self.controls.len();
                        self.controls.push(Control {
                            variable: name.to_owned(),
                            callback: None,
                            widget,
                            definition: ControlDefinition {
                                id,
                                domain: ControlDomain::Integer {
                                    min: i64::from(min),
                                    max: i64::from(max),
                                },
                                default: ControlValue::Integer(i64::from(0.clamp(min, max))),
                            },
                        });
                        Variable::Control(index)
                    };
                    self.variables.insert(name, variable);
                    if kind == "integer" || kind == "array" {
                        let checkpoint = self.offset;
                        if self.next()?.kind == Kind::Symbol(b':') {
                            match variable {
                                Variable::Global(cell) => {
                                    let value = i64::from(self.constant()?);
                                    self.globals[cell as usize] = value;
                                }
                                Variable::Array(array) => self.array_initializer(array)?,
                                _ => unreachable!(),
                            }
                        } else {
                            self.offset = checkpoint;
                        }
                    }
                }
                Kind::Word(name) if name.starts_with('$') || name.starts_with('%') => {
                    let variable = self.variable(name, token.offset)?;
                    if let Variable::Array(array) = variable {
                        self.initial_array_write(array)?;
                        continue;
                    }
                    self.symbol(b':')?;
                    let value = i64::from(self.constant()?);
                    match variable {
                        Variable::Global(cell) => self.globals[cell as usize] = value,
                        Variable::Note(_) => {
                            return Err(
                                self.error("polyphonic state cannot be initialized in on init")
                            );
                        }
                        Variable::Control(index) => {
                            let control = &mut self.controls[index].definition;
                            let ControlDomain::Integer { min, max } = control.domain else {
                                unreachable!()
                            };
                            if !(min..=max).contains(&value) {
                                return Err(
                                    self.error("initial control value outside declared range")
                                );
                            }
                            control.default = ControlValue::Integer(value);
                        }
                        Variable::Constant(_) => {
                            return Err(self.error("cannot assign to a constant"));
                        }
                        Variable::Array(_) => unreachable!(),
                    }
                }
                _ => {
                    return Err(Error {
                        offset: token.offset,
                        message: "only declarations and constant-expression initialization are supported in on init",
                    });
                }
            }
        }
    }

    fn variable(&self, name: &str, offset: usize) -> Result<Variable, Error> {
        self.variables.get(name).copied().ok_or(Error {
            offset,
            message: "undeclared or unsupported variable",
        })
    }

    fn assignment(&mut self, name: &str, offset: usize) -> Result<(), Error> {
        let variable = self.variable(name, offset)?;
        if let Variable::Array(array) = variable {
            self.symbol(b'[')?;
            self.scalar(0)?;
            self.symbol(b']')?;
            self.symbol(b':')?;
            self.scalar(1)?;
            return self.emit(Instruction::WriteScriptArray {
                array,
                index: 0,
                local: 1,
            });
        }
        self.symbol(b':')?;
        self.scalar(0)?;
        self.write_variable(variable, 0)
    }

    fn write_variable(&mut self, variable: Variable, local: u16) -> Result<(), Error> {
        self.emit(match variable {
            Variable::Global(cell) => Instruction::WriteScriptCell { cell, local },
            Variable::Note(cell) => Instruction::WriteNoteCell { cell, local },
            Variable::Control(index) => Instruction::WriteControl {
                control: self.controls[index].definition.id,
                local,
            },
            Variable::Constant(_) => return Err(self.error("cannot assign to a constant")),
            Variable::Array(_) => return Err(self.error("array assignment requires an index")),
        })
    }

    fn integer(&mut self) -> Result<i32, Error> {
        let token = self.next()?;
        self.integer_token(token)
    }
    fn integer_token(&mut self, token: Token<'a>) -> Result<i32, Error> {
        let (token, negative) = match token.kind {
            Kind::Symbol(sign @ (b'-' | b'+')) => (self.next()?, sign == b'-'),
            _ => (token, false),
        };
        if let Kind::Hex(value) = token.kind {
            let value = value as i32;
            return Ok(if negative {
                value.wrapping_neg()
            } else {
                value
            });
        }
        let Kind::Number(value) = token.kind else {
            return Err(Error {
                offset: token.offset,
                message: "expected integer literal",
            });
        };
        if value > i32::MAX as u64 + u64::from(negative) {
            return Err(Error {
                offset: token.offset,
                message: "literal exceeds KSP signed 32-bit range",
            });
        }
        Ok(if negative {
            -(value as i64)
        } else {
            value as i64
        } as i32)
    }

    fn forward(&mut self, kind: &CallbackKind) -> Result<(), Error> {
        match kind {
            CallbackKind::Note => self.emit(Instruction::ForwardAttack),
            CallbackKind::Release => self.emit(Instruction::ForwardReleaseGroups),
            CallbackKind::Control => Ok(()),
            CallbackKind::Controller => self.emit(Instruction::ForwardController),
        }
    }

    fn callback(&mut self, kind: CallbackKind) -> Result<Program, Error> {
        let code = self.body(kind, false)?;
        Program::new(code)
            .map(|p| p.with_wait_lifetime(WaitLifetime::Callback))
            .map_err(|_| self.error("invalid lowered native program"))
    }

    fn body(&mut self, kind: CallbackKind, function: bool) -> Result<Vec<Instruction>, Error> {
        let note = matches!(kind, CallbackKind::Note);
        // Each open branch has already emitted budgeted instructions. This
        // control-only patch stack is bounded by code/source limits, not recursion.
        let mut branches = Vec::new();
        loop {
            let token = self.next()?;
            if matches!(
                branches.last(),
                Some(Block::Select {
                    has_case: false,
                    ..
                })
            ) && !matches!(token.kind, Kind::Word("case" | "end"))
            {
                return Err(self.error("expected case or end select"));
            }
            match token.kind {
                Kind::Word("call") => self.call_function(kind)?,
                Kind::Word(command @ ("change_note" | "change_velo")) if note => {
                    self.symbol(b'(')?;
                    let begin = self.code.len();
                    self.scalar(0)?;
                    let event =
                        if matches!(self.code[begin..], [Instruction::ReadEventId { local: 0 }]) {
                            self.code.pop();
                            self.emitted -= 1;
                            None
                        } else {
                            Some(0)
                        };
                    self.symbol(b',')?;
                    let local = u16::from(event.is_some());
                    self.scalar(local)?;
                    self.symbol(b')')?;
                    self.emit(if command == "change_note" {
                        Instruction::WriteEventKey { event, local }
                    } else {
                        Instruction::WriteEventVelocity7 { event, local }
                    })?;
                }
                Kind::Word("ignore_controller") if matches!(kind, CallbackKind::Controller) => {
                    self.emit(Instruction::SuppressController)?;
                }
                Kind::Word("set_controller") if !matches!(kind, CallbackKind::Control) => {
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    self.symbol(b',')?;
                    self.scalar(1)?;
                    self.symbol(b')')?;
                    self.emit(Instruction::ControllerFromMidi7 { local: 1 })?;
                    self.emit(Instruction::WriteController {
                        controller: 0,
                        value: 1,
                    })?;
                }
                Kind::Word("ignore_event")
                    if matches!(kind, CallbackKind::Note | CallbackKind::Release) =>
                {
                    self.symbol(b'(')?;
                    self.expect(
                        Kind::Word("$EVENT_ID"),
                        "only suppression of the originating event is supported",
                    )?;
                    self.symbol(b')')?;
                    self.emit(if note {
                        Instruction::SuppressAttack
                    } else {
                        Instruction::SuppressRelease
                    })?;
                }
                Kind::Word("wait") => {
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    self.symbol(b')')?;
                    self.emit(Instruction::MicrosToFrames { local: 0 })?;
                    self.forward(&kind)?;
                    self.emit(Instruction::WaitLocal { local: 0 })?;
                }
                Kind::Word("play_note") => self.play(0, false, 0)?,
                Kind::Word("note_off") => {
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    let delay = match self.next()?.kind {
                        Kind::Symbol(b',') => {
                            self.scalar(1)?;
                            self.symbol(b')')?;
                            self.emit(Instruction::MicrosToFrames { local: 1 })?;
                            Some(1)
                        }
                        Kind::Symbol(b')') => None,
                        _ => return Err(self.error("expected comma or closing parenthesis")),
                    };
                    self.emit(Instruction::KeyUpEvent { event: 0, delay })?;
                }
                Kind::Word(command @ ("allow_group" | "disallow_group")) => {
                    self.group(command == "allow_group", note)?
                }
                Kind::Word(command @ ("inc" | "dec")) => self.increment(command == "inc")?,
                Kind::Word("exit") => {
                    self.forward(&kind)?;
                    self.emit(Instruction::End)?;
                }
                Kind::Word(kind @ ("if" | "while")) => {
                    let start = self.code.len();
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    self.symbol(b')')?;
                    let branch = self.code.len();
                    self.emit(Instruction::JumpIfZero {
                        local: 0,
                        target: 0,
                    })?;
                    branches.push(if kind == "if" {
                        Block::If {
                            branch,
                            has_else: false,
                        }
                    } else {
                        Block::While { start, branch }
                    });
                }
                Kind::Word("select") => {
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    self.symbol(b')')?;
                    branches.push(Block::Select {
                        misses: [None; 2],
                        exits: Vec::new(),
                        has_case: false,
                    });
                }
                Kind::Word("case") => {
                    let Some(Block::Select {
                        misses,
                        exits,
                        has_case,
                    }) = branches.last_mut()
                    else {
                        return Err(self.error("case without matching select"));
                    };
                    if *has_case {
                        exits.push(self.code.len());
                        self.emit(Instruction::Jump { target: 0 })?;
                    }
                    for branch in misses.iter().flatten() {
                        self.code[*branch] = Instruction::JumpIfZero {
                            local: 1,
                            target: self.code.len(),
                        };
                    }
                    *misses = self.case()?;
                    *has_case = true;
                }
                Kind::Word("continue") => {
                    let start = branches
                        .iter()
                        .rev()
                        .find_map(|block| match block {
                            Block::While { start, .. } => Some(*start),
                            _ => None,
                        })
                        .ok_or_else(|| self.error("continue outside while"))?;
                    self.emit(Instruction::Jump { target: start })?;
                }
                Kind::Word("else") => {
                    let Some(Block::If { branch, has_else }) = branches.last_mut() else {
                        return Err(self.error("else without if"));
                    };
                    if *has_else {
                        return Err(self.error("duplicate else"));
                    }
                    let end_jump = self.code.len();
                    self.emit(Instruction::Jump { target: 0 })?;
                    self.code[*branch] = Instruction::JumpIfZero {
                        local: 0,
                        target: self.code.len(),
                    };
                    *branch = end_jump;
                    *has_else = true;
                }
                Kind::Word(name) if name.starts_with('$') || name.starts_with('%') => {
                    self.assignment(name, token.offset)?
                }
                Kind::Word("end") => {
                    let token = self.next()?;
                    if token.kind == Kind::Word("select") {
                        let Some(Block::Select { misses, exits, .. }) = branches.pop() else {
                            return Err(self.error("end select without matching select"));
                        };
                        let target = self.code.len();
                        for branch in misses.into_iter().flatten() {
                            self.code[branch] = Instruction::JumpIfZero { local: 1, target };
                        }
                        for branch in exits {
                            self.code[branch] = Instruction::Jump { target };
                        }
                        continue;
                    }
                    if token.kind == Kind::Word("if") {
                        let Some(Block::If { branch, has_else }) = branches.pop() else {
                            return Err(self.error("end if without if"));
                        };
                        let target = self.code.len();
                        self.code[branch] = if has_else {
                            Instruction::Jump { target }
                        } else {
                            Instruction::JumpIfZero { local: 0, target }
                        };
                        continue;
                    }
                    if token.kind == Kind::Word("while") {
                        let Some(Block::While { start, branch }) = branches.pop() else {
                            return Err(self.error("end while without matching while"));
                        };
                        self.emit(Instruction::Jump { target: start })?;
                        self.code[branch] = Instruction::JumpIfZero {
                            local: 0,
                            target: self.code.len(),
                        };
                        continue;
                    }
                    if token.kind != Kind::Word(if function { "function" } else { "on" })
                        || !branches.is_empty()
                    {
                        return Err(Error {
                            offset: token.offset,
                            message: "expected matching block end or end on",
                        });
                    }
                    if !function {
                        self.forward(&kind)?;
                        self.emit(Instruction::End)?;
                    }
                    return Ok(std::mem::take(&mut self.code));
                }
                _ => {
                    return Err(Error {
                        offset: token.offset,
                        message: "unsupported statement or missing end on",
                    });
                }
            }
        }
    }

    fn case(&mut self) -> Result<[Option<usize>; 2], Error> {
        let low = self.constant()?;
        let checkpoint = self.offset;
        let high = if self.next()?.kind == Kind::Word("to") {
            self.constant()?
        } else {
            self.offset = checkpoint;
            low
        };
        // Register 0 is the selector. Only mismatch dispatch reaches another
        // case; matching bodies jump directly to the select's end, even after waits.
        let mut misses = [None; 2];
        let bounds = if low == high {
            [(low, Comparison::Equal), (high, Comparison::Equal)]
        } else {
            [
                (low.min(high), Comparison::LessEqual),
                (low.max(high), Comparison::GreaterEqual),
            ]
        };
        for (slot, (value, comparison)) in
            misses
                .iter_mut()
                .zip(bounds)
                .take(if low == high { 1 } else { 2 })
        {
            self.emit(Instruction::SetLocal {
                local: 1,
                value: i64::from(value),
            })?;
            self.emit(Instruction::CompareLocal {
                lhs: 1,
                rhs: 0,
                comparison,
            })?;
            *slot = Some(self.code.len());
            self.emit(Instruction::JumpIfZero {
                local: 1,
                target: 0,
            })?;
        }
        Ok(misses)
    }

    fn group(&mut self, allowed: bool, pending_only: bool) -> Result<(), Error> {
        self.symbol(b'(')?;
        self.scalar(0)?;
        self.symbol(b')')?;
        self.emit(Instruction::SetLocal {
            local: 1,
            value: ALL_GROUPS,
        })?;
        self.emit(Instruction::CompareLocal {
            lhs: 1,
            rhs: 0,
            comparison: sampler_core::Comparison::Equal,
        })?;
        let branch = self.code.len();
        self.emit(Instruction::JumpIfZero {
            local: 1,
            target: 0,
        })?;
        self.emit(Instruction::WriteGroup {
            group: None,
            allowed,
            pending_only,
        })?;
        let end = self.code.len();
        self.emit(Instruction::Jump { target: 0 })?;
        self.code[branch] = Instruction::JumpIfZero {
            local: 1,
            target: self.code.len(),
        };
        self.emit(Instruction::WriteGroup {
            group: Some(0),
            allowed,
            pending_only,
        })?;
        self.code[end] = Instruction::Jump {
            target: self.code.len(),
        };
        Ok(())
    }

    fn play(&mut self, local: u16, result: bool, depth: u8) -> Result<(), Error> {
        let velocity = self.temporary(local)?;
        let offset = self.temporary(velocity)?;
        let result = result.then_some(local);
        self.symbol(b'(')?;
        self.expression(local, 0, depth + 1)?;
        self.symbol(b',')?;
        self.expression(velocity, 0, depth + 1)?;
        self.symbol(b',')?;
        let offset_start = self.code.len();
        self.expression(offset, 0, depth + 1)?;
        let offset_micros = if matches!(self.code[offset_start..],
            [Instruction::SetLocal { local: register, value: 0 }] if register == offset
        ) {
            self.code.pop();
            self.emitted -= 1;
            None
        } else {
            Some(offset)
        };
        let frames = if offset_micros.is_some() {
            self.temporary(offset)?
        } else {
            offset
        };
        let scratch = self.temporary(frames)?;
        self.symbol(b',')?;
        let duration_start = self.code.len();
        self.expression(frames, 0, depth + 1)?;
        self.symbol(b')')?;
        if let [
            Instruction::SetLocal {
                local: register,
                value,
            },
        ] = self.code[duration_start..]
            && register == frames
        {
            let duration = match value {
                0 => sampler_core::DurationValue::Fixed(sampler_core::Duration::UntilSilent),
                -1 => sampler_core::DurationValue::Fixed(sampler_core::Duration::Gate),
                _ => {
                    self.emit(Instruction::MicrosToFrames { local: frames })?;
                    sampler_core::DurationValue::Frames(frames)
                }
            };
            return self.play_duration(local, velocity, duration, offset_micros, result);
        }
        // Translate sentinel lifetimes without clobbering earlier arguments or
        // the enclosing expression/array index in lower-numbered registers.
        let mut exits = [0; 2];
        for (exit, (value, duration)) in exits.iter_mut().zip([
            (0, sampler_core::Duration::UntilSilent),
            (-1, sampler_core::Duration::Gate),
        ]) {
            self.emit(Instruction::SetLocal {
                local: scratch,
                value,
            })?;
            self.emit(Instruction::CompareLocal {
                lhs: scratch,
                rhs: frames,
                comparison: sampler_core::Comparison::Equal,
            })?;
            let branch = self.code.len();
            self.emit(Instruction::JumpIfZero {
                local: scratch,
                target: 0,
            })?;
            self.play_duration(
                local,
                velocity,
                sampler_core::DurationValue::Fixed(duration),
                offset_micros,
                result,
            )?;
            *exit = self.code.len();
            self.emit(Instruction::Jump { target: 0 })?;
            self.code[branch] = Instruction::JumpIfZero {
                local: scratch,
                target: self.code.len(),
            };
        }
        self.emit(Instruction::MicrosToFrames { local: frames })?;
        self.play_duration(
            local,
            velocity,
            sampler_core::DurationValue::Frames(frames),
            offset_micros,
            result,
        )?;
        for exit in exits {
            self.code[exit] = Instruction::Jump {
                target: self.code.len(),
            };
        }
        Ok(())
    }

    fn play_duration(
        &mut self,
        key: u16,
        velocity: u16,
        duration: sampler_core::DurationValue,
        offset_micros: Option<u16>,
        result: Option<u16>,
    ) -> Result<(), Error> {
        self.emit(Instruction::PlayMidi {
            result,
            key,
            velocity,
            duration,
            offset_micros,
            inheritance: Inheritance::Independent,
        })
    }
}

/// Compile the documented native KSP subset into bounded shared-core programs.
/// Declarations and expressions are prepared off audio; unsupported syntax is
/// rejected. Microsecond arguments round upward to sample frames.
pub fn compile(
    source: &str,
    rate: u32,
    limits: Limits,
    controls: &[(&str, ControlId)],
) -> Result<Script, Error> {
    if source.len() > limits.source_bytes {
        return Err(Error {
            offset: 0,
            message: "source byte budget exceeded",
        });
    }
    if rate == 0 {
        return Err(Error {
            offset: 0,
            message: "sample rate must be positive",
        });
    }
    if controls.len() > limits.variables {
        return Err(Error {
            offset: 0,
            message: "control binding budget exceeded",
        });
    }
    let mut bindings = BTreeMap::new();
    let mut identities = std::collections::BTreeSet::new();
    for &(name, id) in controls {
        if bindings.insert(name, id).is_some() || !identities.insert(id) {
            return Err(Error {
                offset: 0,
                message: "duplicate control name or persistent identity",
            });
        }
    }
    let mut p = Parser {
        source,
        offset: 0,
        limit: limits.instructions,
        emitted: 0,
        code: Vec::new(),
        functions: BTreeMap::new(),
        function_instructions: 0,
        variables: BTreeMap::new(),
        bindings,
        controls: Vec::new(),
        globals: Vec::new(),
        note_cells: 0,
        performance_view: false,
        variable_limit: limits.variables,
        array_limit: limits.array_cells,
        array_cells: 0,
    };
    let mut programs = Vec::new();
    let (mut on_note, mut on_release, mut on_controller) = (None, None, None);
    let mut initialized = false;
    loop {
        let token = p.next()?;
        if token.kind == Kind::End && (initialized || !programs.is_empty()) {
            if !p.bindings.is_empty() {
                return Err(p.error("unused control identity binding"));
            }
            return Ok(Script {
                programs,
                on_note,
                on_release,
                on_controller,
                rate,
                globals: p.globals,
                note_cells: p.note_cells,
                controls: p.controls,
                performance_view: p.performance_view,
            });
        }
        if token.kind == Kind::Word("function") {
            p.function()?;
            continue;
        }
        if token.kind != Kind::Word("on") {
            return Err(Error {
                offset: token.offset,
                message: "expected note or release callback",
            });
        }
        let token = p.next()?;
        match token.kind {
            Kind::Word("init") if !initialized && programs.is_empty() => {
                initialized = true;
                p.declarations()?;
            }
            Kind::Word("ui_control") => {
                p.symbol(b'(')?;
                let name = p.next()?;
                let Kind::Word(name) = name.kind else {
                    return Err(p.error("expected UI control variable"));
                };
                let Variable::Control(index) = p.variable(name, token.offset)? else {
                    return Err(p.error("UI callback requires a control variable"));
                };
                p.symbol(b')')?;
                if p.controls[index].callback.is_some() {
                    return Err(p.error("duplicate UI control callback"));
                }
                let program = p.callback(CallbackKind::Control)?;
                if !CallbackKind::Control.accepts(&program) {
                    return Err(Error {
                        offset: token.offset,
                        message: "event-dependent operands are unsupported in UI callbacks",
                    });
                }
                p.controls[index].callback = Some(programs.len());
                programs.push(program);
            }
            Kind::Word(kind @ ("note" | "release" | "controller")) => {
                let binding = match kind {
                    "note" => &mut on_note,
                    "release" => &mut on_release,
                    _ => &mut on_controller,
                };
                if binding.is_some() {
                    return Err(Error {
                        offset: token.offset,
                        message: "duplicate callback",
                    });
                }
                *binding = Some(programs.len());
                let context = match kind {
                    "note" => CallbackKind::Note,
                    "release" => CallbackKind::Release,
                    _ => CallbackKind::Controller,
                };
                let program = p.callback(context)?;
                if !context.accepts(&program) {
                    return Err(p.error("operand requires a different event context"));
                }
                programs.push(program);
            }
            _ => {
                return Err(Error {
                    offset: token.offset,
                    message: "unsupported callback or misplaced on init",
                });
            }
        }
    }
}
