//! Bounded integer expression lowering. Temporary registers are reused by depth.
use crate::{Error, Kind, Parser, Token, Variable};
use sampler_core::{Instruction, IntegerBinary, IntegerUnary};

fn binary(kind: Kind<'_>) -> Option<(u8, IntegerBinary)> {
    Some(match kind {
        Kind::Word(".or.") => (1, IntegerBinary::Or),
        Kind::Word(".xor.") => (1, IntegerBinary::Xor),
        Kind::Word(".and.") => (2, IntegerBinary::And),
        Kind::Symbol(b'+') => (3, IntegerBinary::Add),
        Kind::Symbol(b'-') => (3, IntegerBinary::Subtract),
        Kind::Symbol(b'*') => (4, IntegerBinary::Multiply),
        Kind::Symbol(b'/') => (4, IntegerBinary::Divide),
        Kind::Word("mod") => (4, IntegerBinary::Remainder),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    pub(super) fn scalar(&mut self, local: u16) -> Result<(), Error> {
        self.expression(local, 0, 0)
    }

    fn expression(&mut self, local: u16, minimum: u8, depth: u8) -> Result<(), Error> {
        if depth == 64 {
            return Err(self.error("integer expression nesting limit exceeded"));
        }
        let token = self.next()?;
        match token.kind {
            Kind::Symbol(b'(') => {
                self.expression(local, 0, depth + 1)?;
                self.symbol(b')')?;
            }
            Kind::Symbol(sign @ (b'-' | b'+')) => {
                // Preserve the one signed literal whose positive magnitude is
                // outside i32: -2147483648. Unary operations on values still wrap.
                let checkpoint = self.offset;
                let literal = matches!(self.next()?.kind, Kind::Number(_));
                self.offset = checkpoint;
                if literal {
                    let value = i64::from(self.integer_token(token)?);
                    self.emit(Instruction::SetLocal { local, value })?;
                } else {
                    self.expression(local, 5, depth + 1)?;
                    if sign == b'-' {
                        self.emit(Instruction::Unary32 {
                            local,
                            operation: IntegerUnary::Negate,
                        })?;
                    }
                }
            }
            Kind::Word(".not.") => {
                self.expression(local, 5, depth + 1)?;
                self.emit(Instruction::Unary32 {
                    local,
                    operation: IntegerUnary::Not,
                })?;
            }
            Kind::Word(function @ ("abs" | "sgn" | "signbit")) => {
                self.symbol(b'(')?;
                self.expression(local, 0, depth + 1)?;
                self.symbol(b')')?;
                self.emit(Instruction::Unary32 {
                    local,
                    operation: match function {
                        "abs" => IntegerUnary::Absolute,
                        "sgn" => IntegerUnary::Sign,
                        _ => IntegerUnary::SignBit,
                    },
                })?;
            }
            _ => self.operand(token, local)?,
        }
        loop {
            let checkpoint = self.offset;
            let token = self.next()?;
            let Some((precedence, operation)) = binary(token.kind).filter(|(p, _)| *p >= minimum)
            else {
                self.offset = checkpoint;
                return Ok(());
            };
            let rhs = local
                .checked_add(1)
                .ok_or_else(|| self.error("integer register range exceeded"))?;
            self.expression(rhs, precedence + 1, depth + 1)?;
            self.emit(Instruction::Binary32 {
                lhs: local,
                rhs,
                operation,
            })?;
        }
    }

    fn operand(&mut self, token: Token<'a>, local: u16) -> Result<(), Error> {
        let instruction = match token.kind {
            Kind::Word("$EVENT_VELOCITY") => Instruction::ReadVelocity7 { local },
            Kind::Word("$EVENT_NOTE") => Instruction::ReadKey { local },
            Kind::Word("$NOTE_HELD") => Instruction::ReadKeyDown { local },
            Kind::Word(name) => match self.variable(name, token.offset)? {
                Variable::Global(cell) => Instruction::ReadScriptCell { local, cell },
                Variable::Note(cell) => Instruction::ReadNoteCell { local, cell },
                Variable::Control(index) => Instruction::ReadControl {
                    local,
                    control: self.controls[index].definition.id,
                },
            },
            _ => Instruction::SetLocal {
                local,
                value: i64::from(self.integer_token(token)?),
            },
        };
        self.emit(instruction)
    }

    pub(super) fn increment(&mut self, add: bool) -> Result<(), Error> {
        self.symbol(b'(')?;
        let token = self.next()?;
        let Kind::Word(name) = token.kind else {
            return Err(self.error("inc/dec requires an integer variable"));
        };
        let variable = self.variable(name, token.offset)?;
        self.symbol(b')')?;
        self.operand(token, 0)?;
        self.emit(Instruction::SetLocal { local: 1, value: 1 })?;
        self.emit(Instruction::Binary32 {
            lhs: 0,
            rhs: 1,
            operation: if add {
                IntegerBinary::Add
            } else {
                IntegerBinary::Subtract
            },
        })?;
        self.write_variable(variable, 0)
    }
}
