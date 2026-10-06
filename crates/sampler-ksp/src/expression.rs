//! Bounded integer expression lowering. Temporary registers are reused by depth.
use crate::{Error, Kind, Parser, Token, Variable};
use sampler_core::{Instruction, IntegerBinary, IntegerUnary};

#[derive(Clone, Copy)]
enum Binary {
    Integer(IntegerBinary),
    Compare(sampler_core::Comparison),
    And,
    Or,
    Xor,
}
fn binary(kind: Kind<'_>) -> Option<(u8, Binary)> {
    Some(match kind {
        Kind::Word("or") => (1, Binary::Or),
        Kind::Word("xor") => (1, Binary::Xor),
        Kind::Word("and") => (2, Binary::And),
        Kind::Comparison(value) => (5, Binary::Compare(value)),
        Kind::Word(".or.") => (6, Binary::Integer(IntegerBinary::Or)),
        Kind::Word(".xor.") => (6, Binary::Integer(IntegerBinary::Xor)),
        Kind::Word(".and.") => (7, Binary::Integer(IntegerBinary::And)),
        Kind::Symbol(b'+') => (8, Binary::Integer(IntegerBinary::Add)),
        Kind::Symbol(b'-') => (8, Binary::Integer(IntegerBinary::Subtract)),
        Kind::Symbol(b'*') => (9, Binary::Integer(IntegerBinary::Multiply)),
        Kind::Symbol(b'/') => (9, Binary::Integer(IntegerBinary::Divide)),
        Kind::Word("mod") => (9, Binary::Integer(IntegerBinary::Remainder)),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    pub(super) fn scalar(&mut self, local: u16) -> Result<(), Error> {
        self.expression(local, 0, 0)
    }

    pub(super) fn expression(&mut self, local: u16, minimum: u8, depth: u8) -> Result<(), Error> {
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
                let literal = matches!(self.next()?.kind, Kind::Number(_) | Kind::Hex(_));
                self.offset = checkpoint;
                if literal {
                    let value = i64::from(self.integer_token(token)?);
                    self.emit(Instruction::SetLocal { local, value })?;
                } else {
                    self.expression(local, 10, depth + 1)?;
                    if sign == b'-' {
                        self.emit(Instruction::Unary32 {
                            local,
                            operation: IntegerUnary::Negate,
                        })?;
                    }
                }
            }
            Kind::Word("not") => {
                self.expression(local, 3, depth + 1)?;
                self.boolean(local, sampler_core::Comparison::Equal)?;
            }
            Kind::Word("in_range") => {
                let low = self.temporary(local)?;
                let high = self.temporary(low)?;
                self.symbol(b'(')?;
                self.expression(local, 0, depth + 1)?;
                self.symbol(b',')?;
                self.expression(low, 0, depth + 1)?;
                self.symbol(b',')?;
                self.expression(high, 0, depth + 1)?;
                self.symbol(b')')?;
                self.emit(Instruction::CompareLocal {
                    lhs: low,
                    rhs: local,
                    comparison: sampler_core::Comparison::LessEqual,
                })?;
                self.emit(Instruction::CompareLocal {
                    lhs: local,
                    rhs: high,
                    comparison: sampler_core::Comparison::LessEqual,
                })?;
                self.emit(Instruction::Binary32 {
                    lhs: local,
                    rhs: low,
                    operation: IntegerBinary::And,
                })?;
            }
            Kind::Word(".not.") => {
                self.expression(local, 10, depth + 1)?;
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
            _ => self.operand(token, local, depth)?,
        }
        loop {
            let checkpoint = self.offset;
            let token = self.next()?;
            let Some((precedence, operation)) = binary(token.kind).filter(|(p, _)| *p >= minimum)
            else {
                self.offset = checkpoint;
                return Ok(());
            };
            if matches!(operation, Binary::And | Binary::Or) {
                self.boolean(local, sampler_core::Comparison::NotEqual)?;
                let branch = self.code.len();
                self.emit(Instruction::JumpIfZero { local, target: 0 })?;
                let short = if matches!(operation, Binary::Or) {
                    let end = self.code.len();
                    self.emit(Instruction::Jump { target: 0 })?;
                    self.code[branch] = Instruction::JumpIfZero {
                        local,
                        target: self.code.len(),
                    };
                    Some(end)
                } else {
                    None
                };
                self.expression(local, precedence + 1, depth + 1)?;
                self.boolean(local, sampler_core::Comparison::NotEqual)?;
                let target = self.code.len();
                if let Some(end) = short {
                    self.code[end] = Instruction::Jump { target };
                } else {
                    self.code[branch] = Instruction::JumpIfZero { local, target };
                }
                continue;
            }
            let rhs = self.temporary(local)?;
            if matches!(operation, Binary::Xor) {
                self.boolean(local, sampler_core::Comparison::NotEqual)?;
            }
            self.expression(rhs, precedence + 1, depth + 1)?;
            let instruction = match operation {
                Binary::Integer(operation) => Instruction::Binary32 {
                    lhs: local,
                    rhs,
                    operation,
                },
                Binary::Compare(comparison) => Instruction::CompareLocal {
                    lhs: local,
                    rhs,
                    comparison,
                },
                Binary::Xor => {
                    self.boolean(rhs, sampler_core::Comparison::NotEqual)?;
                    Instruction::CompareLocal {
                        lhs: local,
                        rhs,
                        comparison: sampler_core::Comparison::NotEqual,
                    }
                }
                Binary::And | Binary::Or => unreachable!(),
            };
            self.emit(instruction)?;
        }
    }

    pub(super) fn temporary(&self, local: u16) -> Result<u16, Error> {
        local
            .checked_add(1)
            .ok_or_else(|| self.error("integer register range exceeded"))
    }

    fn boolean(&mut self, local: u16, comparison: sampler_core::Comparison) -> Result<(), Error> {
        let rhs = self.temporary(local)?;
        self.emit(Instruction::SetLocal {
            local: rhs,
            value: 0,
        })?;
        self.emit(Instruction::CompareLocal {
            lhs: local,
            rhs,
            comparison,
        })
    }

    fn operand(&mut self, token: Token<'a>, local: u16, depth: u8) -> Result<(), Error> {
        let instruction = match token.kind {
            Kind::Word("play_note") => return self.play(local, true, depth),
            Kind::Word("$EVENT_ID") => Instruction::ReadEventId { local },
            Kind::Word("$CC_NUM") => Instruction::ReadControllerNumber { local },
            Kind::Word("%CC") => {
                self.symbol(b'[')?;
                self.expression(local, 0, depth + 1)?;
                self.symbol(b']')?;
                self.emit(Instruction::ReadInputController {
                    controller: local,
                    local,
                })?;
                Instruction::ControllerToMidi7 { local }
            }
            Kind::Word("num_elements") => {
                self.symbol(b'(')?;
                let token = self.next()?;
                let Kind::Word(name) = token.kind else {
                    return Err(self.error("expected integer array name"));
                };
                let Variable::Array(array) = self.variable(name, token.offset)? else {
                    return Err(self.error("num_elements requires an integer array"));
                };
                self.symbol(b')')?;
                Instruction::SetLocal {
                    local,
                    value: i64::from(array.len),
                }
            }
            Kind::Word("$ALL_GROUPS") => Instruction::SetLocal {
                local,
                value: super::ALL_GROUPS,
            },
            Kind::Word("$NUM_GROUPS") => Instruction::ReadGroupCount { local },
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
                Variable::Constant(value) => Instruction::SetLocal {
                    local,
                    value: i64::from(value),
                },
                Variable::Array(array) => {
                    self.symbol(b'[')?;
                    self.expression(local, 0, depth + 1)?;
                    self.symbol(b']')?;
                    Instruction::ReadScriptArray {
                        array,
                        index: local,
                        local,
                    }
                }
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
        if let Variable::Array(array) = variable {
            self.symbol(b'[')?;
            self.scalar(0)?;
            self.symbol(b']')?;
            self.symbol(b')')?;
            self.emit(Instruction::ReadScriptArray {
                array,
                index: 0,
                local: 1,
            })?;
            self.emit(Instruction::SetLocal { local: 2, value: 1 })?;
            self.emit(Instruction::Binary32 {
                lhs: 1,
                rhs: 2,
                operation: if add {
                    IntegerBinary::Add
                } else {
                    IntegerBinary::Subtract
                },
            })?;
            return self.emit(Instruction::WriteScriptArray {
                array,
                index: 0,
                local: 1,
            });
        }
        self.symbol(b')')?;
        self.operand(token, 0, 0)?;
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
