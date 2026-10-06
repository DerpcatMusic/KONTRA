//! Untyped syntax tree. Names stay interned symbols; resolution is in `sema`.
use crate::diag::Span;
use crate::lexer::Sym;

#[derive(Debug)]
pub struct Ast {
    pub items: Vec<Item>,
}

#[derive(Debug)]
pub enum Item {
    Callback(Callback),
    Function(Function),
}

#[derive(Debug)]
pub struct Callback {
    pub name: Sym,
    pub span: Span,
    /// `on ui_control($var)` argument.
    pub arg: Option<(Sym, Span)>,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Function {
    pub name: Sym,
    pub span: Span,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Stmt {
    pub span: Span,
    pub kind: StmtKind,
}

#[derive(Debug)]
pub enum StmtKind {
    Declare(Box<Declare>),
    /// Target is always `Expr::Var`.
    Assign(Expr, Expr),
    /// Builtin command or parameterless user function invoked by name.
    Command(Sym, Vec<Expr>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    Select(Expr, Vec<Case>),
    Call(Sym),
}

#[derive(Debug)]
pub struct Case {
    pub low: Expr,
    pub high: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    Plain,
    Const,
    Polyphonic,
    /// `pers`: persistent with the instrument/snapshot.
    Persistent,
    /// `instpers`: persistent with the instrument only.
    InstrumentPersistent,
}

#[derive(Debug)]
pub struct Declare {
    pub storage: Storage,
    /// `ui_*` widget keyword.
    pub ui: Option<Sym>,
    pub name: Sym,
    pub name_span: Span,
    pub size: Option<Expr>,
    pub params: Vec<Expr>,
    /// Scalar initializer has one element; `:= (a, b, ...)` array lists keep all.
    pub init: Vec<Expr>,
    pub init_list: bool,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Literal magnitude; sign and 32-bit range are checked during resolution.
    Int(u64, bool),
    Real(f64),
    Str(Sym),
    /// Bare identifier argument (PGS key, condition symbol).
    Ident(Sym),
    Var(Sym, Option<Box<Expr>>),
    Call(Sym, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    Xor,
    And,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    BitOr,
    BitXor,
    BitAnd,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}
impl BinOp {
    pub fn binding_power(self) -> u8 {
        match self {
            Self::Or | Self::Xor => 1,
            Self::And => 2,
            Self::Concat => 4,
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Le | Self::Ge => 5,
            Self::BitOr | Self::BitXor => 6,
            Self::BitAnd => 7,
            Self::Add | Self::Sub => 8,
            Self::Mul | Self::Div | Self::Mod => 9,
        }
    }
}
pub const NOT_OPERAND_BP: u8 = 3;
pub const UNARY_BP: u8 = 10;
