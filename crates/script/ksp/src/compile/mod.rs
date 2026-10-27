//! Resolution, type checking and code generation, straight from the syntax tree.

mod declare;
mod expr;

use std::collections::HashMap;

use kontra_vm::{Cmp, Op, Program, Returns, Text};

use crate::builtins::{Arg, Builtin, Ret};
use crate::consts::{self, FIRST_UI_ID, SYMBOL_BASE};
use crate::parse::{Bin, Case, Declare, Expr, ExprKind, Item, Stmt, StmtKind, Storage, Un};
use crate::{
    Callback, CallbackKind, Cells, Control, ControlKind, Error, Persistence, Script, Span, Variable,
};

const MAX_ARRAY: u32 = 1_000_000;
/// Array fills longer than this run as a loop instead of unrolled stores.
const UNROLL: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Int,
    Real,
    Text,
    Bool,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Self::Int => "an integer",
            Self::Real => "a real",
            Self::Text => "a string",
            Self::Bool => "a condition",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Int(i32),
    Real(f64),
    Bool(bool),
}

impl Value {
    fn ty(self) -> Ty {
        match self {
            Self::Int(_) => Ty::Int,
            Self::Real(_) => Ty::Real,
            Self::Bool(_) => Ty::Bool,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Home {
    Cell(u32),
    Cells { base: u32, len: u32 },
    Text(u32),
    Texts { base: u32, len: u32 },
    Poly(u16),
    Const(Value),
}

struct Var {
    name: Box<str>,
    /// Element type.
    ty: Ty,
    home: Home,
    control: Option<u32>,
    persistence: Persistence,
}

impl Var {
    fn array(&self) -> Option<(u32, u32)> {
        match self.home {
            Home::Cells { base, len } | Home::Texts { base, len } => Some((base, len)),
            _ => None,
        }
    }
}

struct Function {
    span: Span,
    entry: Option<u32>,
    calls: Vec<usize>,
}

pub fn compile<'a>(items: &[Item<'a>], conditions: Vec<String>) -> Result<Script, Error> {
    let mut c = Compiler::<'a>::default();
    for item in items {
        if let Item::Function { name, span, .. } = item {
            let key = name.to_ascii_lowercase();
            if c.function_names.contains_key(&key) {
                return Err(Error::new(
                    *span,
                    format!("function {name} is defined twice"),
                ));
            }
            c.function_names.insert(key, c.functions.len());
            c.functions.push(Function {
                span: *span,
                entry: None,
                calls: Vec::new(),
            });
        }
    }
    for item in items {
        let body = match item {
            Item::Callback { body, .. } | Item::Function { body, .. } => body,
        };
        c.declarations(body)?;
    }
    let mut callbacks: Vec<Callback> = Vec::new();
    let mut roots = Vec::new();
    for item in items {
        let entry = c.here();
        c.current = None;
        c.called.clear();
        match item {
            Item::Callback {
                name,
                span,
                arg,
                body,
            } => {
                let kind = callback_kind(name)
                    .ok_or_else(|| Error::new(*span, format!("unknown callback {name}")))?;
                let control = match (kind, arg) {
                    (CallbackKind::UiControl, Some((var, at))) => {
                        let v = c.lookup(var, *at)?;
                        let control = c.vars.get(v).and_then(|v| v.control);
                        Some(control.ok_or_else(|| Error::new(*at, "not a UI control"))?)
                    }
                    (CallbackKind::UiControl, None) => {
                        return Err(Error::new(*span, "ui_control needs a control"));
                    }
                    (_, Some((_, at))) => {
                        return Err(Error::new(*at, "this callback takes no argument"));
                    }
                    (_, None) => None,
                };
                if callbacks
                    .iter()
                    .any(|cb| cb.kind == kind && cb.control == control)
                {
                    return Err(Error::new(
                        *span,
                        format!("callback {name} is defined twice"),
                    ));
                }
                c.block(body)?;
                c.emit(Op::Exit);
                callbacks.push(Callback {
                    kind,
                    control,
                    entry,
                });
                roots.push(std::mem::take(&mut c.called));
            }
            Item::Function { name, body, .. } => {
                let f = c.function_names.get(&name.to_ascii_lowercase()).copied();
                c.current = f;
                c.block(body)?;
                c.emit(Op::Ret);
                if let Some(f) = f.and_then(|f| c.functions.get_mut(f)) {
                    f.entry = Some(entry);
                    f.calls = std::mem::take(&mut c.called);
                }
            }
        }
    }
    for &(at, f) in &c.fixups {
        let entry = c.functions.get(f).and_then(|f| f.entry).unwrap_or(0);
        if let Some(op) = c.code.get_mut(at) {
            *op = Op::Call(entry);
        }
    }
    let depths = call_depths(&c.functions)?;
    let max_depth = roots
        .iter()
        .flat_map(|calls| calls.iter().map(|&f| depths.get(f).copied().unwrap_or(0)))
        .max()
        .unwrap_or(0);
    let variables = c
        .vars
        .iter()
        .map(|v| Variable {
            name: v.name.clone(),
            cells: match v.home {
                Home::Cell(base) if v.ty == Ty::Real => Cells::Real { base, len: 1 },
                Home::Cell(base) => Cells::Int { base, len: 1 },
                Home::Cells { base, len } if v.ty == Ty::Real => Cells::Real { base, len },
                Home::Cells { base, len } => Cells::Int { base, len },
                Home::Text(base) => Cells::Text { base, len: 1 },
                Home::Texts { base, len } => Cells::Text { base, len },
                Home::Poly(n) => Cells::Poly(n),
                Home::Const(_) => Cells::Const,
            },
            persistence: v.persistence,
        })
        .collect();
    Ok(Script {
        program: Program {
            code: c.code,
            texts: c.texts,
            cells: c.cells as usize,
            text_cells: c.text_cells as usize,
            poly_cells: usize::from(c.poly),
            max_depth: max_depth + 1,
            max_stack: c.max_stack + 2,
        },
        callbacks,
        controls: c.controls,
        variables,
        symbols: c.symbols,
        conditions,
    })
}

/// Longest call chain below each function; recursion is an error (Kontakt rejects it).
fn call_depths(functions: &[Function]) -> Result<Vec<usize>, Error> {
    const UNSEEN: usize = usize::MAX;
    const VISITING: usize = usize::MAX - 1;
    fn visit(f: usize, functions: &[Function], depth: &mut [usize]) -> Result<usize, Error> {
        match depth.get(f).copied() {
            Some(VISITING) => {
                let span = functions.get(f).map_or_else(Span::default, |f| f.span);
                return Err(Error::new(span, "recursive function call"));
            }
            Some(UNSEEN) => {}
            Some(d) => return Ok(d),
            None => return Ok(0),
        }
        if let Some(d) = depth.get_mut(f) {
            *d = VISITING;
        }
        let mut deepest = 0;
        for &callee in functions
            .get(f)
            .map(|f| f.calls.as_slice())
            .unwrap_or_default()
        {
            deepest = deepest.max(visit(callee, functions, depth)?);
        }
        if let Some(d) = depth.get_mut(f) {
            *d = deepest + 1;
        }
        Ok(deepest + 1)
    }
    let mut depth = vec![UNSEEN; functions.len()];
    for f in 0..functions.len() {
        visit(f, functions, &mut depth)?;
    }
    Ok(depth)
}

fn callback_kind(name: &str) -> Option<CallbackKind> {
    Some(match name {
        "init" => CallbackKind::Init,
        "note" => CallbackKind::Note,
        "release" => CallbackKind::Release,
        "controller" => CallbackKind::Controller,
        "poly_at" => CallbackKind::PolyAt,
        "ui_control" => CallbackKind::UiControl,
        "ui_controls" => CallbackKind::UiControls,
        "ui_update" => CallbackKind::UiUpdate,
        "listener" => CallbackKind::Listener,
        "pgs_changed" => CallbackKind::PgsChanged,
        "persistence_changed" => CallbackKind::PersistenceChanged,
        "async_complete" => CallbackKind::AsyncComplete,
        "rpn" => CallbackKind::Rpn,
        "nrpn" => CallbackKind::Nrpn,
        _ => return None,
    })
}

/// Element type and whether the name is an array, from its prefix.
fn prefix(name: &str) -> (Ty, bool) {
    match name.as_bytes().first() {
        Some(b'%') => (Ty::Int, true),
        Some(b'~') => (Ty::Real, false),
        Some(b'?') => (Ty::Real, true),
        Some(b'@') => (Ty::Text, false),
        Some(b'!') => (Ty::Text, true),
        _ => (Ty::Int, false),
    }
}

#[derive(Default)]
struct Compiler<'a> {
    code: Vec<Op>,
    texts: Vec<Text>,
    text_ids: HashMap<&'a str, u32>,
    vars: Vec<Var>,
    /// Lowercase name → variable (KSP names are case-insensitive).
    names: HashMap<String, usize>,
    /// Declaration span start → variable.
    declared: HashMap<u32, usize>,
    function_names: HashMap<String, usize>,
    functions: Vec<Function>,
    /// `Call` instructions waiting for their function's entry.
    fixups: Vec<(usize, usize)>,
    current: Option<usize>,
    called: Vec<usize>,
    /// `continue` targets of the enclosing loops.
    loops: Vec<u32>,
    cells: u32,
    text_cells: u32,
    poly: u16,
    scratch_poly: Option<u16>,
    scratch_text: Option<u32>,
    depth: usize,
    text_depth: usize,
    max_stack: usize,
    controls: Vec<Control>,
    symbols: Vec<Box<str>>,
    symbol_ids: HashMap<&'a str, i32>,
}

impl<'a> Compiler<'a> {
    // ---- Emission ----

    fn here(&self) -> u32 {
        self.code.len() as u32
    }

    fn emit(&mut self, op: Op) {
        let (ints, texts): (isize, isize) = match op {
            Op::Int(_) | Op::Real(_) | Op::Dup | Op::Load(_) | Op::LoadPoly(_) => (1, 0),
            Op::Text(_) | Op::TLoad(_) => (0, 1),
            Op::Pop | Op::Store(_) | Op::StorePoly(_) | Op::JumpIfZero(_) | Op::Wait => (-1, 0),
            Op::StoreAt { .. } => (-2, 0),
            Op::TStore(_) | Op::TConcat => (0, -1),
            Op::TLoadAt { .. } | Op::TFromInt | Op::TFromReal => (-1, 1),
            Op::TStoreAt { .. } => (-1, -1),
            Op::TEq => (1, -2),
            Op::IAdd
            | Op::ISub
            | Op::IMul
            | Op::IDiv
            | Op::IMod
            | Op::IAnd
            | Op::IOr
            | Op::IXor
            | Op::IShl
            | Op::IShr
            | Op::FAdd
            | Op::FSub
            | Op::FMul
            | Op::FDiv
            | Op::ICmp(_)
            | Op::FCmp(_) => (-1, 0),
            Op::Host {
                ints,
                texts,
                returns,
                ..
            } => (
                isize::from(matches!(returns, Returns::Int | Returns::Real)) - ints as isize,
                isize::from(returns == Returns::Text) - texts as isize,
            ),
            _ => (0, 0),
        };
        self.depth = self.depth.saturating_add_signed(ints);
        self.text_depth = self.text_depth.saturating_add_signed(texts);
        self.max_stack = self.max_stack.max(self.depth).max(self.text_depth);
        self.code.push(op);
    }

    fn patch(&mut self, at: u32, target: u32) {
        if let Some(Op::Jump(t) | Op::JumpIfZero(t) | Op::LoopCheck { exit: t }) =
            self.code.get_mut(at as usize)
        {
            *t = target
        }
    }

    fn text(&mut self, s: &'a str) -> u32 {
        if let Some(&id) = self.text_ids.get(s) {
            return id;
        }
        let id = self.texts.len() as u32;
        self.texts.push(Text::from_str_cut(s).0);
        self.text_ids.insert(s, id);
        id
    }

    fn symbol(&mut self, name: &'a str) -> i32 {
        if let Some(&id) = self.symbol_ids.get(name) {
            return id;
        }
        let id = SYMBOL_BASE + self.symbols.len() as i32;
        self.symbols.push(name.into());
        self.symbol_ids.insert(name, id);
        id
    }

    fn constant(&mut self, v: Value) -> Ty {
        match v {
            Value::Int(n) => self.emit(Op::Int(n)),
            Value::Real(n) => self.emit(Op::Real(n)),
            Value::Bool(b) => self.emit(Op::Int(i32::from(b))),
        }
        v.ty()
    }

    fn scratch_poly(&mut self) -> u16 {
        if let Some(n) = self.scratch_poly {
            return n;
        }
        let n = self.poly;
        self.poly += 1;
        self.scratch_poly = Some(n);
        n
    }

    // ---- Names ----

    fn lookup(&self, name: &str, span: Span) -> Result<usize, Error> {
        self.names
            .get(&name.to_ascii_lowercase())
            .copied()
            .ok_or_else(|| Error::new(span, format!("{name} is not declared")))
    }

    fn var(&self, v: usize) -> Result<&Var, Error> {
        self.vars
            .get(v)
            .ok_or_else(|| Error::new(Span::default(), "bad variable"))
    }

    // ---- Statements ----

    fn block(&mut self, body: &[Stmt<'a>]) -> Result<(), Error> {
        for s in body {
            self.statement(s)?;
        }
        Ok(())
    }

    fn statement(&mut self, s: &Stmt<'a>) -> Result<(), Error> {
        match &s.kind {
            StmtKind::Declare(d) => self.declaration(d),
            StmtKind::Assign(target, value) => self.assign(target, value),
            StmtKind::Call(name) => self.call(name, s.span),
            StmtKind::Command(name, args) => {
                let key = name.to_ascii_lowercase();
                if self.function_names.contains_key(&key) {
                    if !args.is_empty() {
                        return Err(Error::new(s.span, "functions take no arguments"));
                    }
                    return self.call(name, s.span);
                }
                let b = Builtin::from_name(name)
                    .ok_or_else(|| Error::new(s.span, format!("unknown command {name}")))?;
                match self.builtin(b, args, s.span)? {
                    None => {}
                    Some(Ty::Text) => {
                        let t = match self.scratch_text {
                            Some(t) => t,
                            None => {
                                let t = Self::cells(&mut self.text_cells, 1, s.span)?;
                                self.scratch_text = Some(t);
                                t
                            }
                        };
                        self.emit(Op::TStore(t));
                    }
                    Some(_) => self.emit(Op::Pop),
                }
                Ok(())
            }
            StmtKind::If(cond, yes, no) => {
                self.condition(cond)?;
                let skip = self.here();
                self.emit(Op::JumpIfZero(0));
                self.block(yes)?;
                if no.is_empty() {
                    let end = self.here();
                    self.patch(skip, end);
                } else {
                    let over = self.here();
                    self.emit(Op::Jump(0));
                    let other = self.here();
                    self.patch(skip, other);
                    self.block(no)?;
                    let end = self.here();
                    self.patch(over, end);
                }
                Ok(())
            }
            StmtKind::While(cond, body) => {
                let top = self.here();
                self.emit(Op::LoopCheck { exit: 0 });
                self.condition(cond)?;
                let leave = self.here();
                self.emit(Op::JumpIfZero(0));
                self.loops.push(top);
                self.block(body)?;
                self.loops.pop();
                self.emit(Op::Jump(top));
                let end = self.here();
                self.emit(Op::LoopExit);
                self.patch(top, end);
                self.patch(leave, end);
                Ok(())
            }
            StmtKind::Select(value, cases) => self.select(value, cases),
        }
    }

    fn call(&mut self, name: &str, span: Span) -> Result<(), Error> {
        let f = self
            .function_names
            .get(&name.to_ascii_lowercase())
            .copied()
            .ok_or_else(|| Error::new(span, format!("unknown function {name}")))?;
        self.fixups.push((self.code.len(), f));
        if !self.called.contains(&f) {
            self.called.push(f);
        }
        self.emit(Op::Call(0));
        Ok(())
    }

    /// The selector stays on the stack while cases are tested and is popped
    /// before the chosen case runs.
    fn select(&mut self, value: &Expr<'a>, cases: &[Case<'a>]) -> Result<(), Error> {
        let got = self.expr(value)?;
        self.convert(got, Ty::Int, value.span)?;
        let base = self.depth;
        let mut ends = Vec::with_capacity(cases.len());
        for case in cases {
            self.depth = base;
            let mut misses = Vec::with_capacity(2);
            self.emit(Op::Dup);
            let got = self.expr(&case.low)?;
            self.convert(got, Ty::Int, case.low.span)?;
            if let Some(high) = &case.high {
                self.emit(Op::ICmp(Cmp::Ge));
                misses.push(self.here());
                self.emit(Op::JumpIfZero(0));
                self.emit(Op::Dup);
                let got = self.expr(high)?;
                self.convert(got, Ty::Int, high.span)?;
                self.emit(Op::ICmp(Cmp::Le));
            } else {
                self.emit(Op::ICmp(Cmp::Eq));
            }
            misses.push(self.here());
            self.emit(Op::JumpIfZero(0));
            self.emit(Op::Pop);
            self.block(&case.body)?;
            ends.push(self.here());
            self.emit(Op::Jump(0));
            let next = self.here();
            for miss in misses {
                self.patch(miss, next);
            }
        }
        self.depth = base;
        self.emit(Op::Pop);
        let end = self.here();
        for at in ends {
            self.patch(at, end);
        }
        Ok(())
    }

    fn assign(&mut self, target: &Expr<'a>, value: &Expr<'a>) -> Result<(), Error> {
        let ExprKind::Var(name, index) = &target.kind else {
            return Err(Error::new(target.span, "expected a variable"));
        };
        let Ok(v) = self.lookup(name, target.span) else {
            if Builtin::from_name(name).is_some() || self.named_constant(name).is_some() {
                return Err(Error::new(target.span, format!("{name} is read-only")));
            }
            return Err(Error::new(target.span, format!("{name} is not declared")));
        };
        let var = self.var(v)?;
        let (ty, home) = (var.ty, var.home);
        match (home, index) {
            (Home::Const(_), _) => Err(Error::new(target.span, format!("{name} is a constant"))),
            (Home::Cells { base, len } | Home::Texts { base, len }, Some(i)) => {
                let got = self.expr(i)?;
                self.convert(got, Ty::Int, i.span)?;
                let got = self.expr(value)?;
                self.convert(got, ty, value.span)?;
                self.emit(if ty == Ty::Text {
                    Op::TStoreAt { base, len }
                } else {
                    Op::StoreAt { base, len }
                });
                Ok(())
            }
            (Home::Cells { .. } | Home::Texts { .. }, None) => {
                Err(Error::new(target.span, "an array needs an index"))
            }
            (_, Some(i)) => Err(Error::new(i.span, format!("{name} is not an array"))),
            (_, None) => {
                let got = self.expr(value)?;
                self.convert(got, ty, value.span)?;
                self.store_scalar(home);
                Ok(())
            }
        }
    }

    fn store_scalar(&mut self, home: Home) {
        match home {
            Home::Cell(n) => self.emit(Op::Store(n)),
            Home::Text(n) => self.emit(Op::TStore(n)),
            Home::Poly(n) => self.emit(Op::StorePoly(n)),
            _ => {}
        }
    }
}

fn int_literal(n: u64, span: Span) -> Result<i32, Error> {
    u32::try_from(n)
        .map(|n| n as i32)
        .map_err(|_| Error::new(span, "integer out of range"))
}
