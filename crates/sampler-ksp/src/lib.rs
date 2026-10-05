#![forbid(unsafe_code)]
//! Clean-sheet, control-thread KSP 8.12 source subset. No vendor VM dependency.
//! Native sample-time lowering is explicit; this is not a Kontakt fidelity claim.
use sampler_core::{Duration, Inheritance, Instruction, Program, Velocity, WaitLifetime};

pub const PROFILE: &str = "ksp-8.12-note-subset-v0";

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    pub instructions: usize,
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
    End,
}
#[derive(Clone, Copy)]
struct Token<'a> {
    kind: Kind<'a>,
    offset: usize,
}
struct Parser<'a> {
    source: &'a str,
    offset: usize,
    rate: u32,
    limit: usize,
    code: Vec<Instruction>,
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
        if self.code.len() == self.limit {
            return Err(self.error("instruction budget exceeded"));
        }
        self.code.push(op);
        Ok(())
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

/// Supported shape: one note callback beginning with ignore_event($EVENT_ID),
/// then literal waits and fixed-velocity, positive-duration play_note commands.
/// Microseconds round upward to engine frames. Negative/whole-source duration,
/// variable expressions, additional callbacks and implicit forwarding are rejected.
pub fn compile(source: &str, rate: u32, limits: Limits) -> Result<Program, Error> {
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
        code: Vec::new(),
    };
    p.expect(Kind::Word("on"), "expected on note callback")?;
    p.expect(Kind::Word("note"), "only one on note callback is supported")?;
    p.expect(
        Kind::Word("ignore_event"),
        "explicit leading ignore_event($EVENT_ID) is required; implicit forwarding is unsupported",
    )?;
    p.symbol(b'(')?;
    p.expect(
        Kind::Word("$EVENT_ID"),
        "only suppression of the originating event is supported",
    )?;
    p.symbol(b')')?;
    loop {
        let token = p.next()?;
        match token.kind {
            Kind::Word("wait") => {
                p.symbol(b'(')?;
                let (micros, offset) = p.number()?;
                p.symbol(b')')?;
                p.emit(Instruction::Wait(p.frames(micros, offset)?))?;
            }
            Kind::Word("play_note") => p.play()?,
            Kind::Word("end") => {
                p.expect(Kind::Word("on"), "expected end on")?;
                p.expect(
                    Kind::End,
                    "additional callbacks or trailing tokens are unsupported",
                )?;
                p.emit(Instruction::End)?;
                return Program::new(p.code)
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
