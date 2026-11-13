//! Superinstructions: common runs of stack ops folded into one dispatch.
//!
//! A fused op replaces only the first op of its run and then skips the rest,
//! which stay in place. Addresses never move, so jump targets, call fixups
//! and code positions in faults are unchanged, and a jump into the middle of
//! a run still executes the plain ops. A fused op does exactly what its run
//! does, except that it never holds its operands on the stack.

use crate::program::{Bin, Op, Src};

impl Op {
    /// The plain op `src` stands for.
    fn src(self) -> Option<Src> {
        match self {
            Self::Load(n) => Some(Src::Cell(n)),
            Self::LoadPoly(n) => Some(Src::Poly(n)),
            Self::Int(v) => Some(Src::Int(v)),
            _ => None,
        }
    }

    /// The binary integer op that never faults.
    fn bin(self) -> Option<Bin> {
        Some(match self {
            Self::IAdd => Bin::Add,
            Self::ISub => Bin::Sub,
            Self::IMul => Bin::Mul,
            Self::IAnd => Bin::And,
            Self::IOr => Bin::Or,
            Self::IXor => Bin::Xor,
            Self::IShl => Bin::Shl,
            Self::IShr => Bin::Shr,
            _ => return None,
        })
    }

    /// A store `dst` stands for.
    fn dst(self) -> Option<Src> {
        match self {
            Self::Store(n) => Some(Src::Cell(n)),
            Self::StorePoly(n) => Some(Src::Poly(n)),
            _ => None,
        }
    }
}

/// `code` with every run [`fused`] matches at its start replaced; same length.
#[must_use]
pub fn fuse(code: &[Op]) -> Vec<Op> {
    (code.iter().enumerate())
        .map(|(at, &op)| fused(code, at).unwrap_or(op))
        .collect()
}

/// The fused op for the run that starts at `start`, longest first.
fn fused(code: &[Op], start: usize) -> Option<Op> {
    let at = |n: usize| code.get(start + n).copied();
    let src = |n: usize| at(n).and_then(Op::src);
    let bin = |n: usize| at(n).and_then(Op::bin);
    let branch = |n: usize| match (at(n), at(n + 1)) {
        (Some(Op::ICmp(cmp)), Some(Op::JumpIfZero(to))) => Some((cmp, to)),
        _ => None,
    };
    // x k ± store(x) jump
    if let (Some(x), Some(Src::Int(k)), Some(bin), Some(to), Some(Op::Jump(top))) =
        (src(0), src(1), bin(2), at(3).and_then(Op::dst), at(4))
        && x == to
        && let Some(by) = match bin {
            Bin::Add => Some(k),
            Bin::Sub => Some(k.wrapping_neg()),
            _ => None,
        }
    {
        let check = match code.get(top as usize) {
            Some(&Op::LoopCheck { exit }) => Some(exit),
            _ => None,
        };
        return Some(Op::NextTurn { x, by, top, check });
    }
    // a b ⊕ store
    if let (Some(a), Some(b), Some(bin), Some(to)) =
        (src(0), src(1), bin(2), at(3).and_then(Op::dst))
    {
        return Some(Op::BinTo { bin, a, b, to });
    }
    // a b cmp jz
    if let (Some(a), Some(b), Some((cmp, to))) = (src(0), src(1), branch(2)) {
        return Some(Op::Branch { cmp, a, b, to });
    }
    // a b ⊕ load_at
    if let (Some(a), Some(b), Some(bin), Some(Op::LoadAt { base, len })) =
        (src(0), src(1), bin(2), at(3))
    {
        return Some(Op::LoadAtBin {
            base,
            len,
            bin,
            a,
            b,
        });
    }
    // a b ⊕
    if let (Some(a), Some(b), Some(bin)) = (src(0), src(1), bin(2)) {
        return Some(Op::Bin { bin, a, b });
    }
    // b icmp iand, on the two values below
    if let (Some(b), Some(Op::ICmp(cmp)), Some(Op::IAnd)) = (src(0), at(1), at(2)) {
        return Some(Op::CmpTopAnd { cmp, b });
    }
    // b iand jz, on the value below
    if let (Some(b), Some(Op::IAnd), Some(Op::JumpIfZero(to))) = (src(0), at(1), at(2)) {
        return Some(Op::AndTopJump { b, to });
    }
    // b cmp jz, on the value below
    if let (Some(b), Some((cmp, to))) = (src(0), branch(1)) {
        return Some(Op::BranchTop { cmp, b, to });
    }
    // b ⊕ load_at, on the value below
    if let (Some(b), Some(bin), Some(Op::LoadAt { base, len })) = (src(0), bin(1), at(2)) {
        return Some(Op::LoadAtTop { base, len, bin, b });
    }
    match (at(0), at(1)) {
        // b icmp, on the value below
        (Some(op), Some(Op::ICmp(cmp))) if op.src().is_some() => {
            Some(Op::CmpTop { cmp, b: op.src()? })
        }
        // icmp iand
        (Some(Op::ICmp(cmp)), Some(Op::IAnd)) => Some(Op::CmpAnd { cmp }),
        // iand jz
        (Some(Op::IAnd), Some(Op::JumpIfZero(to))) => Some(Op::AndJump { to }),
        // b ⊕, on the value below
        (Some(op), Some(next)) if op.src().is_some() && next.bin().is_some() => Some(Op::BinTop {
            bin: next.bin()?,
            b: op.src()?,
        }),
        // i load_at
        (Some(Op::Load(i)), Some(Op::LoadAt { base, len })) => {
            Some(Op::LoadAtCell { base, len, i })
        }
        // v store_at, the index below
        (Some(op), Some(Op::StoreAt { base, len })) if op.src().is_some() => Some(Op::StoreAtSrc {
            base,
            len,
            v: op.src()?,
        }),
        // cmp jz
        (Some(Op::ICmp(cmp)), Some(Op::JumpIfZero(to))) => Some(Op::CmpJump { cmp, to }),
        // ⊕ store
        (Some(op), Some(next)) if op.bin().is_some() && next.dst().is_some() => {
            Some(Op::BinStore {
                bin: op.bin()?,
                to: next.dst()?,
            })
        }
        // ⊕ ⊕
        (Some(op), Some(next)) if op.bin().is_some() && next.bin().is_some() => Some(Op::BinBin {
            inner: op.bin()?,
            outer: next.bin()?,
        }),
        // a jump back to a loop's check runs the check
        (Some(Op::Jump(top)), _) => match code.get(top as usize) {
            Some(&Op::LoopCheck { exit }) => Some(Op::LoopJump { top, exit }),
            _ => None,
        },
        _ => None,
    }
}

impl Bin {
    /// `a ⊕ b` on 32-bit integers, wrapping as the plain op does.
    #[must_use]
    pub fn apply(self, a: i32, b: i32) -> i32 {
        match self {
            Self::Add => a.wrapping_add(b),
            Self::Sub => a.wrapping_sub(b),
            Self::Mul => a.wrapping_mul(b),
            Self::And => a & b,
            Self::Or => a | b,
            Self::Xor => a ^ b,
            Self::Shl => a.wrapping_shl(b as u32),
            Self::Shr => a.wrapping_shr(b as u32),
        }
    }
}
