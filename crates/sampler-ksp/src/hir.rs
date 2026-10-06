//! Resolved, typed program. Every name is an index; every expression has a type.
use crate::builtins::{Builtin, SysArray, SysVar};
use crate::diag::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Real,
    Str,
    /// Condition value; converts to 0/1 where an integer is required.
    Bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FnId(pub u32);

/// Compile-time value.
#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Int(i32),
    Real(f64),
    Str(Box<str>),
}

/// Where a variable's value lives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Home {
    /// Script-instance integer cell; reals are stored as IEEE-754 bits.
    Cell(u32),
    /// Contiguous script cells.
    Cells { offset: u32, len: u32 },
    /// Script-instance text cell(s).
    Text(u32),
    Texts { offset: u32, len: u32 },
    /// Note-owned (polyphonic) cell.
    Note(u16),
    /// Value owned by a host-visible control; index into `Hir::uis`.
    Control(u32),
    /// Folded at compile time; index into `Hir::consts`.
    Const(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persistence {
    None,
    /// Saved with the instrument and snapshots (`make_persistent`, `pers`).
    Snapshot,
    /// Saved with the instrument only (`make_instr_persistent`, `instpers`).
    Instrument,
}

#[derive(Clone, Debug)]
pub struct Var {
    pub name: Box<str>,
    /// Element type.
    pub ty: Ty,
    pub len: Option<u32>,
    pub home: Home,
    pub ui: Option<u32>,
    pub persistence: Persistence,
    pub span: Span,
}

/// Widget keyword of a UI declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Button,
    Knob,
    Menu,
    ValueEdit,
    Label,
    Table,
    Waveform,
    Wavetable,
    Slider,
    TextEdit,
    FileSelector,
    Switch,
    Xy,
    LevelMeter,
    MouseArea,
    Panel,
}
impl WidgetKind {
    pub fn from_keyword(word: &str) -> Option<Self> {
        Some(match word {
            "ui_button" => Self::Button,
            "ui_knob" => Self::Knob,
            "ui_menu" => Self::Menu,
            "ui_value_edit" => Self::ValueEdit,
            "ui_label" => Self::Label,
            "ui_table" => Self::Table,
            "ui_waveform" => Self::Waveform,
            "ui_wavetable" => Self::Wavetable,
            "ui_slider" => Self::Slider,
            "ui_text_edit" => Self::TextEdit,
            "ui_file_selector" => Self::FileSelector,
            "ui_switch" => Self::Switch,
            "ui_xy" => Self::Xy,
            "ui_level_meter" => Self::LevelMeter,
            "ui_mouse_area" => Self::MouseArea,
            "ui_panel" => Self::Panel,
            _ => return None,
        })
    }
    /// `$NI_CONTROL_TYPE_*` value.
    pub fn control_type(self) -> i32 {
        match self {
            Self::Button => 1,
            Self::Knob => 2,
            Self::Menu => 3,
            Self::ValueEdit => 4,
            Self::Label => 5,
            Self::Table => 6,
            Self::Waveform => 7,
            Self::Wavetable => 8,
            Self::Slider => 9,
            Self::TextEdit => 10,
            Self::FileSelector => 11,
            Self::Switch => 12,
            Self::Xy => 13,
            Self::LevelMeter => 14,
            Self::MouseArea => 15,
            Self::Panel => 16,
        }
    }
    /// Scalar integer value owned by a host control (automatable, recallable).
    pub fn has_control(self) -> bool {
        matches!(
            self,
            Self::Button | Self::Knob | Self::Menu | Self::ValueEdit | Self::Slider | Self::Switch
        )
    }
}

#[derive(Clone, Debug)]
pub struct Ui {
    pub kind: WidgetKind,
    pub var: VarId,
    /// Folded declaration parameters, e.g. `(min, max, display_ratio)`.
    pub params: Vec<i32>,
    pub callback: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub ty: Ty,
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logic {
    And,
    Or,
    Xor,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i32),
    Real(f64),
    Str(Box<str>),
    Load(VarId),
    LoadElem(VarId, Box<Expr>),
    Sys(SysVar),
    SysElem(SysArray, Box<Expr>),
    /// Integer or real negation, by type.
    Neg(Box<Expr>),
    BitNot(Box<Expr>),
    Not(Box<Expr>),
    Arith(Arith, Box<Expr>, Box<Expr>),
    /// Operand type is the left operand's type.
    Compare(sampler_core::Comparison, Box<Expr>, Box<Expr>),
    Logic(Logic, Box<Expr>, Box<Expr>),
    /// Text concatenation of any-typed parts.
    Concat(Vec<Expr>),
    /// Bool -> Int (0/1) or Int -> Bool (!= 0).
    Cast(Box<Expr>),
    Builtin(Builtin, Vec<Arg>),
}

#[derive(Clone, Debug)]
pub enum Arg {
    Expr(Expr),
    Var(VarId, Span),
    /// Runtime-maintained array passed by reference (`search(%KEY_DOWN, 1)`).
    SysArray(SysArray, Span),
    Place(Place),
    Key(Box<str>),
}

#[derive(Clone, Debug)]
pub enum Place {
    Var(VarId),
    Elem(VarId, Box<Expr>),
}
impl Place {
    pub fn var(&self) -> VarId {
        match self {
            Self::Var(v) | Self::Elem(v, _) => *v,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub span: Span,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub struct Case {
    pub low: i32,
    pub high: i32,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Assign(Place, Expr),
    /// Declaration list initializer; short lists repeat their last value.
    Fill(VarId, Vec<Expr>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    Select(Expr, Vec<Case>),
    Call(FnId),
    Builtin(Builtin, Vec<Arg>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallbackKind {
    Init,
    Note,
    Release,
    Controller,
    PolyAt,
    UiControl(VarId),
    Listener,
    PgsChanged,
    PersistenceChanged,
    AsyncComplete,
    Rpn,
    Nrpn,
}

#[derive(Debug)]
pub struct Callback {
    pub kind: CallbackKind,
    pub span: Span,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Function {
    pub name: Box<str>,
    pub span: Span,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Default)]
pub struct Hir {
    pub vars: Vec<Var>,
    pub consts: Vec<Const>,
    pub uis: Vec<Ui>,
    pub functions: Vec<Function>,
    pub callbacks: Vec<Callback>,
    /// Integer/real script cells, text cells and polyphonic note cells.
    pub cells: u32,
    pub texts: u32,
    pub note_cells: u16,
    /// Undeclared uppercase vendor names, valued `OPAQUE_BASE + index`.
    pub symbols: Vec<Box<str>>,
    pub conditions: std::collections::BTreeSet<String>,
    /// Non-fatal resolution findings.
    pub warnings: Vec<crate::diag::Fault>,
    /// Longest static `call` chain, for the VM's bounded frame stack.
    pub call_depth: usize,
}

/// Values of opaque vendor symbols not in `builtins::CONTROL_PARS`.
pub const OPAQUE_BASE: i32 = 0x0200_0000;
