//! Explicit signed-32 integer operations; native scheduling counters remain i64.

/// Wrapping two's-complement arithmetic. Division truncates toward zero; remainder
/// keeps the dividend's sign. A zero divisor produces zero for both operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    And,
    Or,
    Xor,
}

impl IntegerBinary {
    pub fn apply(self, left: i32, right: i32) -> i32 {
        match self {
            Self::Add => left.wrapping_add(right),
            Self::Subtract => left.wrapping_sub(right),
            Self::Multiply => left.wrapping_mul(right),
            Self::Divide if right != 0 => left.wrapping_div(right),
            Self::Remainder if right != 0 => left.wrapping_rem(right),
            Self::Divide | Self::Remainder => 0,
            Self::And => left & right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerUnary {
    Negate,
    Absolute,
    Sign,
    SignBit,
    Not,
}

impl IntegerUnary {
    pub fn apply(self, value: i32) -> i32 {
        match self {
            Self::Negate => value.wrapping_neg(),
            Self::Absolute => value.wrapping_abs(),
            Self::Sign => value.signum(),
            Self::SignBit => i32::from(value < 0),
            Self::Not => !value,
        }
    }
}
