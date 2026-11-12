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
        .map(|(at, &op)| code.get(at..).and_then(fused).unwrap_or(op))
        .collect()
}

/// The fused op for the run at the start of `run`, longest first.
fn fused(run: &[Op]) -> Option<Op> {
    let at = |n: usize| run.get(n).copied();
    let src = |n: usize| at(n).and_then(Op::src);
    let bin = |n: usize| at(n).and_then(Op::bin);
    let branch = |n: usize| match (at(n), at(n + 1)) {
        (Some(Op::ICmp(cmp)), Some(Op::JumpIfZero(to))) => Some((cmp, to)),
        _ => None,
    };
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
    // a b ⊕
    if let (Some(a), Some(b), Some(bin)) = (src(0), src(1), bin(2)) {
        return Some(Op::Bin { bin, a, b });
    }
    // b cmp jz, on the value below
    if let (Some(b), Some((cmp, to))) = (src(0), branch(1)) {
        return Some(Op::BranchTop { cmp, b, to });
    }
    match (at(0), at(1)) {
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

impl Op {
    /// How many plain ops this op runs: 1, or a fused run's length.
    #[must_use]
    pub fn width(self) -> u32 {
        match self {
            Self::BinTo { .. } | Self::Branch { .. } => 4,
            Self::Bin { .. } | Self::BranchTop { .. } => 3,
            Self::BinTop { .. } | Self::LoadAtCell { .. } | Self::StoreAtSrc { .. } => 2,
            _ => 1,
        }
    }
}
