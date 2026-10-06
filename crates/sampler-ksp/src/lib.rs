#![forbid(unsafe_code)]
//! Clean-sheet, control-thread KSP 8.12 source subset. No vendor VM dependency.
//! Native sample-time lowering is explicit; this is not a Kontakt fidelity claim.
use sampler_core::{
    Comparison, ControlDefinition, ControlDomain, ControlId, ControlValue, Inheritance,
    Instruction, Prepared, Program, ScriptInstanceId, WaitLifetime,
};
use std::collections::BTreeMap;
mod expression;

pub const PROFILE: &str = "ksp-8.12-note-release-subset-v1";

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub instructions: usize,
    pub variables: usize,
}

/// Control-owned executable callbacks, UI metadata and declared state layout.
pub struct Script {
    programs: Vec<Program>,
    on_note: Option<usize>,
    on_release: Option<usize>,
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
            .with_script_instances(vec![self.globals])?;
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
        match self.on_release {
            Some(program) => plan.with_release_program(program),
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
    Symbol(u8),
    Comparison(Comparison),
    End,
}
#[derive(Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    offset: usize,
}
enum Block {
    If { branch: usize, has_else: bool },
    While { start: usize, branch: usize },
}
#[derive(Clone, Copy)]
enum Variable {
    Global(u16),
    Note(u16),
    Control(usize),
}

struct Parser<'a> {
    source: &'a str,
    offset: usize,
    limit: usize,
    emitted: usize,
    code: Vec<Instruction>,
    variables: BTreeMap<&'a str, Variable>,
    bindings: BTreeMap<&'a str, ControlId>,
    controls: Vec<Control>,
    globals: Vec<i64>,
    note_cells: usize,
    performance_view: bool,
    variable_limit: usize,
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
        let kind = if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
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
        } else if b"(),+-*/".contains(&byte) {
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
    fn number(&mut self) -> Result<(u64, usize), Error> {
        let token = self.next()?;
        match token.kind {
            Kind::Number(value) if value <= i32::MAX as u64 => Ok((value, token.offset)),
            Kind::Number(_) => Err(Error {
                offset: token.offset,
                message: "positive literal exceeds KSP signed 32-bit range",
            }),
            _ => Err(Error {
                offset: token.offset,
                message: "expected unsigned integer literal; dynamic expressions are unsupported",
            }),
        }
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
                            kind @ ("polyphonic" | "ui_knob" | "ui_slider" | "ui_button"
                            | "ui_switch"),
                        ) => (kind, self.next()?),
                        Kind::Word(name) if name.starts_with('$') => ("integer", token),
                        _ => return Err(self.error("unsupported declaration type")),
                    };
                    let Kind::Word(name) = token.kind else {
                        return Err(Error {
                            offset: token.offset,
                            message: "expected integer variable name",
                        });
                    };
                    if !name.strip_prefix('$').is_some_and(|name| {
                        name.as_bytes()
                            .first()
                            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                    }) || matches!(
                        name,
                        "$EVENT_NOTE" | "$EVENT_VELOCITY" | "$EVENT_ID" | "$NOTE_HELD"
                    ) || [
                        "$NI_",
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
                    let variable = if kind == "integer" {
                        let cell = u16::try_from(self.globals.len())
                            .map_err(|_| self.error("native script-cell index range exceeded"))?;
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
                    if kind == "integer" {
                        let checkpoint = self.offset;
                        if self.next()?.kind == Kind::Symbol(b':') {
                            let Variable::Global(cell) = variable else {
                                unreachable!()
                            };
                            let value = i64::from(self.integer()?);
                            self.globals[usize::from(cell)] = value;
                        } else {
                            self.offset = checkpoint;
                        }
                    }
                }
                Kind::Word(name) if name.starts_with('$') => {
                    let variable = self.variable(name, token.offset)?;
                    self.symbol(b':')?;
                    let value = i64::from(self.integer()?);
                    match variable {
                        Variable::Global(cell) => self.globals[usize::from(cell)] = value,
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
                    }
                }
                _ => {
                    return Err(Error {
                        offset: token.offset,
                        message: "only declarations and literal scalar initialization are supported in on init",
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

    fn callback(&mut self, note: bool) -> Result<Program, Error> {
        // Each open branch has already emitted budgeted instructions. This
        // control-only patch stack is bounded by code/source limits, not recursion.
        let mut branches = Vec::new();
        loop {
            let token = self.next()?;
            match token.kind {
                Kind::Word(command @ ("change_note" | "change_velo")) if note => {
                    self.symbol(b'(')?;
                    self.expect(
                        Kind::Word("$EVENT_ID"),
                        "only edits of the originating event are supported",
                    )?;
                    self.symbol(b',')?;
                    self.scalar(0)?;
                    self.symbol(b')')?;
                    self.emit(if command == "change_note" {
                        Instruction::WriteEventKey { local: 0 }
                    } else {
                        Instruction::WriteEventVelocity7 { local: 0 }
                    })?;
                }
                Kind::Word("ignore_event") if note => {
                    self.symbol(b'(')?;
                    self.expect(
                        Kind::Word("$EVENT_ID"),
                        "only suppression of the originating event is supported",
                    )?;
                    self.symbol(b')')?;
                    self.emit(Instruction::SuppressAttack)?;
                }
                Kind::Word("wait") => {
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    self.symbol(b')')?;
                    self.emit(Instruction::MicrosToFrames { local: 0 })?;
                    if note {
                        self.emit(Instruction::ForwardAttack)?;
                    }
                    self.emit(Instruction::WaitLocal { local: 0 })?;
                }
                Kind::Word("play_note") => self.play()?,
                Kind::Word(command @ ("inc" | "dec")) => self.increment(command == "inc")?,
                Kind::Word("exit") => {
                    if note {
                        self.emit(Instruction::ForwardAttack)?;
                    }
                    self.emit(Instruction::End)?;
                }
                Kind::Word(kind @ ("if" | "while")) => {
                    let start = self.code.len();
                    self.symbol(b'(')?;
                    self.scalar(0)?;
                    let token = self.next()?;
                    let Kind::Comparison(comparison) = token.kind else {
                        return Err(Error {
                            offset: token.offset,
                            message: "expected scalar comparison",
                        });
                    };
                    self.scalar(1)?;
                    self.symbol(b')')?;
                    self.emit(Instruction::CompareLocal {
                        lhs: 0,
                        rhs: 1,
                        comparison,
                    })?;
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
                Kind::Word(name) if name.starts_with('$') => self.assignment(name, token.offset)?,
                Kind::Word("end") => {
                    let token = self.next()?;
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
                    if token.kind != Kind::Word("on") || !branches.is_empty() {
                        return Err(Error {
                            offset: token.offset,
                            message: "expected matching end if, end while or end on",
                        });
                    }
                    if note {
                        self.emit(Instruction::ForwardAttack)?;
                    }
                    self.emit(Instruction::End)?;
                    return Program::new(std::mem::take(&mut self.code))
                        .map(|p| p.with_wait_lifetime(WaitLifetime::Callback))
                        .map_err(|_| Error {
                            offset: token.offset,
                            message: "invalid lowered native program",
                        });
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

    fn play(&mut self) -> Result<(), Error> {
        self.symbol(b'(')?;
        self.scalar(0)?;
        self.symbol(b',')?;
        self.scalar(1)?;
        self.symbol(b',')?;
        let (sample_offset, offset) = self.number()?;
        if sample_offset != 0 {
            return Err(Error {
                offset,
                message: "sample offsets are unsupported",
            });
        }
        self.symbol(b',')?;
        self.scalar(2)?;
        self.symbol(b')')?;
        self.emit(Instruction::MicrosToFrames { local: 2 })?;
        self.emit(Instruction::PlayMidi {
            key: 0,
            velocity: 1,
            frames: 2,
            inheritance: Inheritance::Independent,
        })
    }
}

/// Compile optional polyphonic declarations followed by note/release callbacks.
/// Note callbacks require leading ignore_event($EVENT_ID). Bodies accept scalar
/// assignment, scalar conditionals, exit, literal waits and fixed-velocity play_note.
/// Microseconds round upward to frames; broader expressions/services are rejected.
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
        variables: BTreeMap::new(),
        bindings,
        controls: Vec::new(),
        globals: Vec::new(),
        note_cells: 0,
        performance_view: false,
        variable_limit: limits.variables,
    };
    let mut programs = Vec::new();
    let (mut on_note, mut on_release) = (None, None);
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
                rate,
                globals: p.globals,
                note_cells: p.note_cells,
                controls: p.controls,
                performance_view: p.performance_view,
            });
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
                let program = p.callback(false)?;
                if program.requires_note() {
                    return Err(Error {
                        offset: token.offset,
                        message: "note-dependent operands are unsupported in UI callbacks",
                    });
                }
                p.controls[index].callback = Some(programs.len());
                programs.push(program);
            }
            Kind::Word(kind @ ("note" | "release")) => {
                let binding = if kind == "note" {
                    &mut on_note
                } else {
                    &mut on_release
                };
                if binding.is_some() {
                    return Err(Error {
                        offset: token.offset,
                        message: "duplicate callback",
                    });
                }
                *binding = Some(programs.len());
                programs.push(p.callback(kind == "note")?);
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
