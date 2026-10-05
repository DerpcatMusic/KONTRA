#![forbid(unsafe_code)]
//! Clean-sheet, control-thread KSP 8.12 source subset. No vendor VM dependency.
//! Native sample-time lowering is explicit; this is not a Kontakt fidelity claim.
use sampler_core::{
    Comparison, Duration, Inheritance, Instruction, Prepared, Program, Velocity, WaitLifetime,
};
use std::collections::BTreeMap;

pub const PROFILE: &str = "ksp-8.12-note-release-subset-v1";

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub instructions: usize,
    pub variables: usize,
}

/// Control-owned executable callbacks and their declared note-state layout.
pub struct Script {
    programs: Vec<Program>,
    on_note: Option<usize>,
    on_release: Option<usize>,
    rate: u32,
    note_cells: usize,
}

impl Script {
    pub fn note_cells(&self) -> usize {
        self.note_cells
    }

    /// Install the complete script on a prepared instrument of the compiled rate.
    pub fn bind(self, plan: Prepared) -> Result<Prepared, sampler_core::Error> {
        if plan.sample_rate() != self.rate {
            return Err(sampler_core::Error::InvalidInput);
        }
        let plan = plan.with_programs(self.programs, self.on_note)?;
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
struct Parser<'a> {
    source: &'a str,
    offset: usize,
    rate: u32,
    limit: usize,
    emitted: usize,
    code: Vec<Instruction>,
    variables: BTreeMap<&'a str, u16>,
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
        } else if b"(),+-".contains(&byte) {
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
                Kind::Word("declare") => {
                    self.expect(
                        Kind::Word("polyphonic"),
                        "only polyphonic integer declarations are supported",
                    )?;
                    let token = self.next()?;
                    let Kind::Word(name) = token.kind else {
                        return Err(Error {
                            offset: token.offset,
                            message: "expected polyphonic variable name",
                        });
                    };
                    if !name.strip_prefix('$').is_some_and(|name| {
                        name.as_bytes()
                            .first()
                            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                    }) || matches!(name, "$EVENT_NOTE" | "$EVENT_ID" | "$NOTE_HELD")
                        || [
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
                    let cell = u16::try_from(self.variables.len())
                        .map_err(|_| self.error("native note-cell index range exceeded"))?;
                    if self.variables.len() >= self.variable_limit {
                        return Err(self.error("variable budget exceeded"));
                    }
                    self.variables.insert(name, cell);
                }
                _ => {
                    return Err(Error {
                        offset: token.offset,
                        message: "only declarations are supported in on init",
                    });
                }
            }
        }
    }

    fn variable(&self, name: &str, offset: usize) -> Result<u16, Error> {
        self.variables.get(name).copied().ok_or(Error {
            offset,
            message: "undeclared or unsupported variable",
        })
    }

    fn assignment(&mut self, name: &str, offset: usize) -> Result<(), Error> {
        let cell = self.variable(name, offset)?;
        self.symbol(b':')?;
        self.scalar(0)?;
        self.emit(Instruction::WriteNoteCell { cell, local: 0 })
    }

    fn scalar(&mut self, local: u16) -> Result<(), Error> {
        let token = self.next()?;
        let read = match token.kind {
            Kind::Word("$EVENT_NOTE") => Instruction::ReadKey { local },
            Kind::Word("$NOTE_HELD") => Instruction::ReadKeyDown { local },
            Kind::Word(name) => Instruction::ReadNoteCell {
                local,
                cell: self.variable(name, token.offset)?,
            },
            kind => {
                let (token, negative) = match kind {
                    Kind::Symbol(sign @ (b'-' | b'+')) => (self.next()?, sign == b'-'),
                    _ => (token, false),
                };
                let Kind::Number(value) = token.kind else {
                    return Err(Error {
                        offset: token.offset,
                        message: "expected integer literal or variable",
                    });
                };
                if value > i32::MAX as u64 + u64::from(negative) {
                    return Err(Error {
                        offset: token.offset,
                        message: "literal exceeds KSP signed 32-bit range",
                    });
                }
                Instruction::SetLocal {
                    local,
                    value: if negative {
                        -(value as i64)
                    } else {
                        value as i64
                    },
                }
            }
        };
        self.emit(read)
    }

    fn callback(&mut self, note: bool) -> Result<Program, Error> {
        if note {
            self.expect(Kind::Word("ignore_event"),
                "explicit leading ignore_event($EVENT_ID) is required; implicit forwarding is unsupported")?;
            self.symbol(b'(')?;
            self.expect(
                Kind::Word("$EVENT_ID"),
                "only suppression of the originating event is supported",
            )?;
            self.symbol(b')')?;
        }
        // Each open branch has already emitted budgeted instructions. This
        // control-only patch stack is bounded by code/source limits, not recursion.
        let mut branches = Vec::new();
        loop {
            let token = self.next()?;
            match token.kind {
                Kind::Word("wait") => {
                    self.symbol(b'(')?;
                    let (micros, offset) = self.number()?;
                    self.symbol(b')')?;
                    self.emit(Instruction::Wait(self.frames(micros, offset)?))?;
                }
                Kind::Word("play_note") => self.play()?,
                Kind::Word("exit") => self.emit(Instruction::End)?,
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

    fn frames(&self, micros: u64, offset: usize) -> Result<u32, Error> {
        let frames = (u128::from(micros) * u128::from(self.rate)).div_ceil(1_000_000);
        u32::try_from(frames).map_err(|_| Error {
            offset,
            message: "time exceeds native frame-instruction range",
        })
    }
    fn play(&mut self) -> Result<(), Error> {
        self.symbol(b'(')?;
        self.expect(
            Kind::Word("$EVENT_NOTE"),
            "only $EVENT_NOTE with an optional constant transpose is supported",
        )?;
        let token = self.next()?;
        let transpose = match token.kind {
            Kind::Symbol(b',') => 0,
            Kind::Symbol(sign @ (b'+' | b'-')) => {
                let (value, offset) = self.number()?;
                let value = i8::try_from(value).map_err(|_| Error {
                    offset,
                    message: "transpose must be within -127..127",
                })?;
                self.symbol(b',')?;
                if sign == b'-' { -value } else { value }
            }
            _ => {
                return Err(Error {
                    offset: token.offset,
                    message: "unsupported note expression",
                });
            }
        };
        let (velocity, offset) = self.number()?;
        if !(1..=127).contains(&velocity) {
            return Err(Error {
                offset,
                message: "play_note velocity must be 1..127",
            });
        }
        self.symbol(b',')?;
        let (sample_offset, offset) = self.number()?;
        if sample_offset != 0 {
            return Err(Error {
                offset,
                message: "sample offsets are unsupported",
            });
        }
        self.symbol(b',')?;
        let (duration, offset) = self.number()?;
        if duration == 0 {
            return Err(Error {
                offset,
                message: "whole-source duration is unsupported; provide a positive duration",
            });
        }
        self.symbol(b')')?;
        self.emit(Instruction::Play {
            transpose,
            velocity: Velocity::Fixed(velocity as f64 / 127.),
            inheritance: Inheritance::Independent,
            duration: Duration::Frames(self.frames(duration, offset)?),
        })
    }
}

/// Compile optional polyphonic declarations followed by note/release callbacks.
/// Note callbacks require leading ignore_event($EVENT_ID). Bodies accept scalar
/// assignment, scalar conditionals, exit, literal waits and fixed-velocity play_note.
/// Microseconds round upward to frames; broader expressions/services are rejected.
pub fn compile(source: &str, rate: u32, limits: Limits) -> Result<Script, Error> {
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
    let mut p = Parser {
        source,
        offset: 0,
        rate,
        limit: limits.instructions,
        emitted: 0,
        code: Vec::new(),
        variables: BTreeMap::new(),
        variable_limit: limits.variables,
    };
    let mut programs = Vec::new();
    let (mut on_note, mut on_release) = (None, None);
    let mut initialized = false;
    loop {
        let token = p.next()?;
        if token.kind == Kind::End && !programs.is_empty() {
            return Ok(Script {
                programs,
                on_note,
                on_release,
                rate,
                note_cells: p.variables.len(),
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
