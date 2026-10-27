//! Bytecode: what a front end produces and the interpreter runs.

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
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// Division by zero gives 0 and is counted.
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
    /// Truncates toward zero; NaN → 0.
    RealToInt,

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
    /// Calls the host: pops `ints` values and `texts` texts (in push order).
    Host {
        id: u16,
        ints: u8,
        texts: u8,
        returns: Returns,
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
