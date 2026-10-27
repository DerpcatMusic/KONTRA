//! KSP front end: Kontakt script source → a [`kontra_vm::Program`] plus what
//! the engine needs to run it (callbacks, UI controls, variables, symbols).
//!
//! The pipeline is lexer → condition preprocessor → parser → one compiler pass.
//! Builtins that are not lowered to bytecode become [`kontra_vm::Action`]s,
//! the front-end-neutral vocabulary the engine implements.

mod builtins;
mod compile;
mod consts;
mod lex;
mod parse;
mod persist;

use std::fmt;

pub use builtins::{Arg, Builtin, Ret, Sig};
pub use consts::{FIRST_UI_ID, SYMBOL_BASE};
use kontra_vm::Program;
pub use persist::Restore;

/// Compiles a script. `conditions` are the `SET_CONDITION` symbols already set
/// (Kontakt's system-script switches).
pub fn compile(source: &str, conditions: &[&str]) -> Result<Script, Error> {
    let mut tokens = lex::lex(source)?;
    let mut set = conditions.iter().map(|&c| c.to_owned()).collect();
    lex::preprocess(&mut tokens, &mut set)?;
    let items = parse::parse(&tokens)?;
    compile::compile(&items, set)
}

/// A compiled script. Immutable; one per loaded script slot.
#[derive(Debug)]
pub struct Script {
    pub program: Program,
    pub callbacks: Vec<Callback>,
    /// UI controls in declaration order; control `n` has id `FIRST_UI_ID + n`.
    pub controls: Vec<Control>,
    pub variables: Vec<Variable>,
    /// Catalog names used by identity; symbol `n` has value `SYMBOL_BASE + n`.
    pub symbols: Vec<Box<str>>,
    /// Conditions set when the source ends.
    pub conditions: Vec<String>,
}

impl Script {
    /// The entry of a callback.
    #[must_use]
    pub fn callback(&self, kind: CallbackKind, control: Option<u32>) -> Option<u32> {
        self.callbacks
            .iter()
            .find(|c| c.kind == kind && c.control == control)
            .map(|c| c.entry)
    }

    /// The name a symbol value stands for.
    #[must_use]
    pub fn symbol(&self, value: i32) -> Option<&str> {
        let n = usize::try_from(value.checked_sub(SYMBOL_BASE)?).ok()?;
        self.symbols.get(n).map(|s| &**s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackKind {
    Init,
    Note,
    Release,
    Controller,
    PolyAt,
    UiControl,
    UiControls,
    UiUpdate,
    Listener,
    PgsChanged,
    PersistenceChanged,
    AsyncComplete,
    Rpn,
    Nrpn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callback {
    pub kind: CallbackKind,
    /// The control of an `on ui_control`.
    pub control: Option<u32>,
    pub entry: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub kind: ControlKind,
    /// The variable holding its value.
    pub variable: u32,
    /// Declaration parameters (range, size); 0 where set at run time.
    pub params: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: Box<str>,
    pub cells: Cells,
    pub persistence: Persistence,
}

/// Where a variable lives in the [`kontra_vm::Instance`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cells {
    Int {
        base: u32,
        len: u32,
    },
    Real {
        base: u32,
        len: u32,
    },
    Text {
        base: u32,
        len: u32,
    },
    /// Per-callback cell.
    Poly(u16),
    /// Folded at compile time.
    Const,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persistence {
    None,
    /// Saved with the instrument and snapshots.
    Snapshot,
    /// Saved with the instrument only.
    Instrument,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    fn new(start: usize, end: usize) -> Self {
        let at = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            start: at(start),
            end: at(end),
        }
    }

    fn to(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// A compile error at a source span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub span: Span,
    pub message: String,
}

impl Error {
    fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    /// 1-based line and column of the error in `source`.
    #[must_use]
    pub fn line_column(&self, source: &str) -> (usize, usize) {
        let before = source.get(..self.span.start as usize).unwrap_or(source);
        let line = before.matches('\n').count() + 1;
        let column = before.len() - before.rfind('\n').map_or(0, |n| n + 1) + 1;
        (line, column)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.span.start)
    }
}

impl std::error::Error for Error {}
