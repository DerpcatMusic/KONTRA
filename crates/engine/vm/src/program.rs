//! Bytecode: what a front end produces and the interpreter runs.

use crate::Action;
use crate::text::Text;

/// Comparison of two numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// An integer operand a fused op reads in place of the op that pushed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    /// Instance cell `n` ([`Op::Load`]).
    Cell(u32),
    /// Per-callback cell `n` ([`Op::LoadPoly`]).
    Poly(u16),
    /// A constant ([`Op::Int`]).
    Int(i32),
}

/// An integer op that never faults, as a fused op runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

/// What a host call leaves on the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Returns {
    Nothing,
    Int,
    Real,
    Text,
}

/// One instruction. Integers are 32-bit and wrap (KSP); reals are f64 stored
/// as bits in the same 64-bit stack slots; texts live on a separate text stack.
/// A plain one-byte tag: the interpreter dispatches on it directly.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Op {
    Int(i32),
    Real(f64),
    /// Pushes text constant `n` onto the text stack.
    Text(u32),
    Pop,
    Dup,

    /// Instance cell `n`.
    Load(u32),
    Store(u32),
    /// Element of the array at cells `base..base + len`; the index is on the stack.
    /// Out of range reads 0 and writes nothing, and both are counted.
    LoadAt {
        base: u32,
        len: u32,
    },
    /// Pops the value, then the index.
    StoreAt {
        base: u32,
        len: u32,
    },
    /// Per-callback (polyphonic) cell `n` of the running coroutine.
    LoadPoly(u16),
    StorePoly(u16),

    TLoad(u32),
    TStore(u32),
    TLoadAt {
        base: u32,
        len: u32,
    },
    /// Pops the index; stores the top text.
    TStoreAt {
        base: u32,
        len: u32,
    },
    /// Integer → text.
    TFromInt,
    /// Real → text (KSP prints six decimals).
    TFromReal,
    /// Pops two texts, pushes their concatenation.
    TConcat,
    /// Pops two texts, pushes 1 if equal else 0.
    TEq,

    IAdd,
    ISub,
    IMul,
    /// Division by zero gives 0 and is counted; so does `mod 0`, and the
    /// callback goes on, as in Kontakt (oracle probe, Kontakt 8,
    /// 2026-11-14; a real `7.0 / 0.0` gives inf there too).
    IDiv,
    IMod,
    IAnd,
    IOr,
    IXor,
    INeg,
    IBitNot,
    IShl,
    /// Arithmetic shift right.
    IShr,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FNeg,
    ICmp(Cmp),
    FCmp(Cmp),
    /// Logical not: 0 → 1, anything else → 0.
    Not,
    IntToReal,
    /// Truncates toward zero, saturating at the integer range; NaN gives
    /// the operand (the front end's measured value).
    RealToInt(i32),

    Jump(u32),
    /// Pops; jumps when 0.
    JumpIfZero(u32),
    /// Top of a `while`: counts an iteration and jumps to `exit` at the limit.
    LoopCheck {
        exit: u32,
    },
    /// After a `while`: resets the shared iteration count.
    LoopExit,
    Call(u32),
    Ret,
    /// Ends the callback (KSP `exit`), from any call depth.
    Exit,
    /// Pops microseconds and suspends the callback.
    Wait,
    /// Runs an action (pure ones in the VM, the rest on the host): pops `ints`
    /// values and `texts` texts (in push order).
    Host {
        action: Action,
        ints: u8,
        texts: u8,
        returns: Returns,
    },

    // Fused runs ([`crate::fuse`]): each stands for its first `width` plain
    // ops, which follow it in the code, and continues after them.
    /// `a b ⊕ store`.
    BinTo {
        bin: Bin,
        a: Src,
        b: Src,
        /// A [`Src::Cell`] or [`Src::Poly`].
        to: Src,
    },
    /// `a b cmp jump_if_zero(to)`.
    Branch {
        cmp: Cmp,
        a: Src,
        b: Src,
        to: u32,
    },
    /// `a b ⊕`.
    Bin {
        bin: Bin,
        a: Src,
        b: Src,
    },
    /// `b cmp jump_if_zero(to)`, against the value on the stack.
    BranchTop {
        cmp: Cmp,
        b: Src,
        to: u32,
    },
    /// `b ⊕`, on the value on the stack.
    BinTop {
        bin: Bin,
        b: Src,
    },
    /// `load(i) load_at`.
    LoadAtCell {
        base: u32,
        len: u32,
        i: u32,
    },
    /// `a b ⊕ load_at`.
    LoadAtBin {
        base: u32,
        len: u32,
        bin: Bin,
        a: Src,
        b: Src,
    },
    /// `b ⊕ load_at`, on the value on the stack.
    LoadAtTop {
        base: u32,
        len: u32,
        bin: Bin,
        b: Src,
    },
    /// `cmp jump_if_zero(to)`.
    CmpJump {
        cmp: Cmp,
        to: u32,
    },
    /// `⊕ store`.
    BinStore {
        bin: Bin,
        to: Src,
    },
    /// `inner outer`: two binary ops on the top three values.
    BinBin {
        inner: Bin,
        outer: Bin,
    },
    /// `jump(top)` onto `loop_check(exit)`: the check, then the loop body.
    LoopJump {
        top: u32,
        exit: u32,
    },
    /// `x k ⊕ store(x) jump(top)` with ⊕ an add or subtract: a loop's
    /// counter steps `by`, then the loop goes round, through its
    /// `loop_check(exit)` when `top` is one.
    NextTurn {
        x: Src,
        by: i32,
        top: u32,
        check: Option<u32>,
    },
    /// `b icmp`, on the value on the stack: pushes the comparison.
    CmpTop {
        cmp: Cmp,
        b: Src,
    },
    /// `icmp iand`: `p & (a cmp b)` from the top three values.
    CmpAnd {
        cmp: Cmp,
    },
    /// `b icmp iand`: `p & (a cmp b)`, `b` read in place, `a` and `p` on the stack.
    CmpTopAnd {
        cmp: Cmp,
        b: Src,
    },
    /// `iand jump_if_zero(to)`.
    AndJump {
        to: u32,
    },
    /// `b iand jump_if_zero(to)`, on the value on the stack.
    AndTopJump {
        b: Src,
        to: u32,
    },
    /// `v store_at`, the index on the stack.
    StoreAtSrc {
        base: u32,
        len: u32,
        v: Src,
    },
}

/// A compiled script. Immutable; shared by every instance.
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub code: Vec<Op>,
    pub texts: Vec<Text>,
    /// Instance integer/real cells and their initial values.
    pub cells: usize,
    pub text_cells: usize,
    /// Per-callback cells (KSP polyphonic variables).
    pub poly_cells: usize,
    /// Deepest call chain a callback can reach.
    pub max_depth: usize,
    /// Value-stack slots a callback needs.
    pub max_stack: usize,
}
