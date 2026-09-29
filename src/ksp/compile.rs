//! Compiles parsed KSP into one flat, typed instruction array. Variables resolve to
//! memory slots, builtins to enum values, constants fold; the VM never sees a name.

use super::builtins::{self, Arg, Builtin, Ret, SysArray, SysVar};
use super::lexer::{Interner, Sym, lex};
use super::parser::{BinOp, Block, Declare, Expr, Stmt, StmtKind, UnOp, parse};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{BTreeSet, HashMap};

/// Per-array element ceiling (Kontakt's own limit) and per-script total.
pub const MAX_ARRAY_LEN: u32 = 1_000_000;
pub const MAX_TOTAL_ELEMENTS: u32 = 16_000_000;
const MAX_VARS: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Real,
    Str,
}

impl Ty {
    fn of(name: &str) -> Self {
        match name.as_bytes()[0] {
            b'~' | b'?' => Self::Real,
            b'@' | b'!' => Self::Str,
            _ => Self::Int,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Const {
    Int(i32),
    Real(f64),
}

pub type VarId = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    PushI(i32),
    PushR(u32),
    PushS(u32),
    LdI(u32),
    StI(u32),
    LdR(u32),
    StR(u32),
    LdS(u32),
    StS(u32),
    LdPoly(u32),
    StPoly(u32),
    /// Array element access; the operand is a `VarId`, the index is on the int stack.
    LdIA(VarId),
    StIA(VarId),
    LdRA(VarId),
    StRA(VarId),
    LdSA(VarId),
    StSA(VarId),
    Sys(SysVar),
    UiId(VarId),
    /// Pushes a variable or key reference for the next builtin.
    Ref(u32),
    PopI,
    PopR,
    PopS,
    IToS,
    RToS,
    IAdd,
    ISub,
    IMul,
    IDiv,
    IMod,
    INeg,
    IBitAnd,
    IBitOr,
    IBitXor,
    IBitNot,
    INot,
    IEq,
    INe,
    ILt,
    IGt,
    ILe,
    IGe,
    RAdd,
    RSub,
    RMul,
    RDiv,
    RMod,
    RNeg,
    REq,
    RNe,
    RLt,
    RGt,
    RLe,
    RGe,
    SEq,
    SNe,
    Concat,
    Jump(u32),
    JumpIfZero(u32),
    JumpIfNonZero(u32),
    /// Select arm: matches the int on top of the stack against `cases[n]`.
    Case(u32),
    Call(u32),
    Ret,
    Exit,
    Halt,
    Builtin(Builtin, u8),
    Declare(VarId),
    InitArray(u32),
    // Superinstructions written by `fuse` over the first op of a sequence.
    /// `LdI a; PushI n; IAdd; StI a`.
    AddVarImm(u32, i32),
    /// `PushI n; IAdd`.
    AddImm(i32),
    /// `LdI a; LdIA v`, as `(v, a)`.
    LdIAVar(VarId, u32),
    /// `PushI n; <cmp>; JumpIfZero t`: pop x and jump to `t` unless `x cmp n`.
    BrImm(Cmp, i32, u32),
    /// `LdI a; PushI n; <cmp>; JumpIfZero t`.
    BrVarImm(Cmp, u32, i32, u32),
    /// The test of a loop the runtime can run natively: `loops[n]`.
    Loop(u32),
}

/// Integer comparison of a fused branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl Cmp {
    pub fn of(op: Op) -> Option<Self> {
        Some(match op {
            Op::IEq => Self::Eq,
            Op::INe => Self::Ne,
            Op::ILt => Self::Lt,
            Op::IGt => Self::Gt,
            Op::ILe => Self::Le,
            Op::IGe => Self::Ge,
            _ => return None,
        })
    }

    #[inline(always)]
    pub fn test(self, a: i32, b: i32) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            Self::Lt => a < b,
            Self::Gt => a > b,
            Self::Le => a <= b,
            Self::Ge => a >= b,
        }
    }
}

/// Jump threading: a jump into an unconditional jump, or into a branch on a
/// constant (the `PushI 0; JumpIfZero` a false `and` ends with), goes straight
/// to the final target, and such a constant branch reached by falling through
/// becomes a plain jump. Ops stay in place, so no other target moves.
fn thread(code: &mut [Op]) {
    let hop = |code: &[Op], t: u32| match code.get(t as usize..)? {
        [Op::Jump(u), ..] => Some(*u),
        [Op::PushI(k), Op::JumpIfZero(u), ..] => Some(if *k == 0 { *u } else { t + 2 }),
        [Op::PushI(k), Op::JumpIfNonZero(u), ..] => Some(if *k != 0 { *u } else { t + 2 }),
        _ => None,
    };
    // Bounded so a `while (1)` jumping to itself ends.
    let last = |code: &[Op], mut t: u32| {
        for _ in 0..16 {
            match hop(code, t) {
                Some(u) if u != t => t = u,
                _ => break,
            }
        }
        t
    };
    for i in 0..code.len() {
        match code[i] {
            Op::PushI(_) if let Some(t) = hop(code, i as u32) => code[i] = Op::Jump(last(code, t)),
            Op::Jump(t) => code[i] = Op::Jump(last(code, t)),
            Op::JumpIfZero(t) => code[i] = Op::JumpIfZero(last(code, t)),
            Op::JumpIfNonZero(t) => code[i] = Op::JumpIfNonZero(last(code, t)),
            _ => {}
        }
    }
}

/// Peephole superinstructions for the hottest sequences (loop tests,
/// counters, indexed loads). Each is written over the first op of its
/// sequence and skips the rest, which stay in place: jumps into the middle
/// still land on the original ops, so no target moves.
fn fuse(code: &mut [Op]) {
    for i in 0..code.len() {
        code[i] = match code[i..] {
            [Op::LdI(a), Op::PushI(n), op, Op::JumpIfZero(t), ..]
                if let Some(cmp) = Cmp::of(op) =>
            {
                Op::BrVarImm(cmp, a, n, t)
            }
            [Op::LdI(a), Op::PushI(n), Op::IAdd, Op::StI(b), ..] if a == b => Op::AddVarImm(a, n),
            [Op::PushI(n), op, Op::JumpIfZero(t), ..] if let Some(cmp) = Cmp::of(op) => {
                Op::BrImm(cmp, n, t)
            }
            [Op::LdI(a), Op::LdIA(v), ..] => Op::LdIAVar(v, a),
            [Op::PushI(n), Op::IAdd, ..] => Op::AddImm(n),
            _ => continue,
        };
    }
}

#[derive(Debug)]
pub struct Var {
    pub name: Box<str>,
    pub ty: Ty,
    /// Element count for arrays, `None` for scalars.
    pub len: Option<u32>,
    pub slot: u32,
    pub poly: bool,
    pub constant: Option<Const>,
    pub ui: Option<Box<str>>,
    pub params: Box<[Ty]>,
}

impl Var {
    pub fn is_array(&self) -> bool {
        self.len.is_some()
    }
}

#[derive(Debug)]
pub enum InitData {
    Int(Box<[i32]>),
    Real(Box<[f64]>),
    Str(Box<[u32]>),
}

#[derive(Debug)]
pub struct ArrayInit {
    pub var: VarId,
    pub data: InitData,
}

#[derive(Clone, Copy, Debug)]
pub struct CaseArm {
    pub low: i32,
    pub high: i32,
    pub miss: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    Init,
    Note,
    Release,
    Controller,
    PolyAt,
    Rpn,
    Nrpn,
    Listener,
    UiUpdate,
    PgsChanged,
    PersistenceChanged,
    AsyncComplete,
}

impl Callback {
    const COUNT: usize = 12;

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "init" => Self::Init,
            "note" => Self::Note,
            "release" => Self::Release,
            "controller" => Self::Controller,
            "poly_at" => Self::PolyAt,
            "rpn" => Self::Rpn,
            "nrpn" => Self::Nrpn,
            "listener" => Self::Listener,
            "ui_update" => Self::UiUpdate,
            "pgs_changed" => Self::PgsChanged,
            "persistence_changed" => Self::PersistenceChanged,
            "async_complete" => Self::AsyncComplete,
            _ => return None,
        })
    }

    pub fn type_id(self) -> i32 {
        use builtins::cb;
        match self {
            Self::Init => cb::INIT,
            Self::Note => cb::NOTE,
            Self::Release => cb::RELEASE,
            Self::Controller => cb::CONTROLLER,
            Self::PolyAt => cb::POLY_AT,
            Self::Rpn => cb::RPN,
            Self::Nrpn => cb::NRPN,
            Self::Listener => cb::LISTENER,
            Self::UiUpdate => cb::UI_UPDATE,
            Self::PgsChanged => cb::PGS_CHANGED,
            Self::PersistenceChanged => cb::PERSISTENCE_CHANGED,
            Self::AsyncComplete => cb::ASYNC_COMPLETE,
        }
    }
}

/// Immutable compiled script.
#[derive(Debug, Default)]
pub struct Program {
    pub code: Vec<Op>,
    pub lines: Vec<u32>,
    pub reals: Vec<f64>,
    pub strings: Vec<Box<str>>,
    pub vars: Vec<Var>,
    pub functions: Vec<u32>,
    pub inits: Vec<ArrayInit>,
    pub cases: Vec<CaseArm>,
    pub loops: Vec<super::idiom::Loop>,
    callbacks: [Option<u32>; Callback::COUNT],
    /// Entry per `VarId` for `on ui_control`.
    pub ui_callbacks: Vec<Option<u32>>,
    pub ints: u32,
    pub real_slots: u32,
    pub strs: u32,
    pub poly: u32,
    pub sys_arrays: [Option<VarId>; 5],
    /// Script-local names for undeclared uppercase constants.
    pub auto_symbols: Vec<Box<str>>,
    /// Blocks that failed to compile, disabled at runtime.
    pub errors: Vec<String>,
    pub diagnostics: BTreeSet<String>,
}

/// Script-local automatic symbol values start here.
pub const AUTO_SYMBOL_BASE: i32 = 0x0400_0000;

impl Program {
    pub fn callback(&self, cb: Callback) -> Option<u32> {
        self.callbacks[cb as usize]
    }

    pub fn symbol_name(&self, value: i32) -> Option<&str> {
        builtins::symbol_name(value).or_else(|| {
            let i = usize::try_from(value.wrapping_sub(AUTO_SYMBOL_BASE)).ok()?;
            self.auto_symbols.get(i).map(|s| &**s)
        })
    }

    pub fn line(&self, pc: u32) -> u32 {
        self.lines.get(pc as usize).copied().unwrap_or(0)
    }
}

pub struct Setup {
    pub groups: usize,
    pub outputs: usize,
}

struct Unit {
    calls: Vec<u32>,
    error: Option<String>,
}

/// `%EVENT_PAR[i]` reads and writes parameters of the current event.
const EVENT_PAR: &str = "%EVENT_PAR";

struct Compiler<'a> {
    syms: &'a Interner,
    setup: &'a Setup,
    p: Program,
    var_ids: HashMap<Sym, VarId>,
    string_ids: HashMap<Sym, u32>,
    auto_ids: HashMap<Sym, i32>,
    fn_ids: HashMap<Sym, u32>,
    elements: u32,
    line: u32,
    /// Code before this index may be a jump target; peephole folding stops here.
    barrier: usize,
    calls: Vec<u32>,
}

pub fn compile(source: &str, setup: &Setup) -> Result<Program> {
    let tokens = lex(source)?;
    let blocks = parse(&tokens)?;
    let mut c = Compiler {
        syms: &tokens.syms,
        setup,
        p: Program::default(),
        var_ids: HashMap::new(),
        string_ids: HashMap::new(),
        auto_ids: HashMap::new(),
        fn_ids: HashMap::new(),
        elements: 0,
        line: 0,
        barrier: 0,
        calls: Vec::new(),
    };
    let init = blocks
        .iter()
        .position(|b| !b.function && c.name(b.name) == "init")
        .context("No KSP init callback")?;
    ensure!(
        blocks
            .iter()
            .filter(|b| !b.function && c.name(b.name) == "init")
            .count()
            == 1,
        "Duplicate init callback"
    );
    for b in blocks.iter().filter(|b| b.function) {
        let id = c.fn_ids.len() as u32;
        ensure!(
            c.fn_ids.insert(b.name, id).is_none(),
            "Duplicate KSP function {}",
            c.name(b.name)
        );
    }
    c.p.functions = vec![u32::MAX; c.fn_ids.len()];
    // Declarations first, so every body sees every variable regardless of order.
    let init_body = blocks[init]
        .body
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    c.declare_all(init_body)?;
    for b in blocks.iter().filter(|b| b.function) {
        if let Ok(body) = &b.body {
            c.declare_all(body)?;
        }
    }
    let mut units: Vec<(usize, Unit)> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let unit = c.unit(b);
        units.push((i, unit));
    }
    // A unit that calls a failed function fails too.
    let fn_unit: HashMap<u32, usize> = units
        .iter()
        .enumerate()
        .filter(|(_, (b, _))| blocks[*b].function)
        .map(|(u, (b, _))| (c.fn_ids[&blocks[*b].name], u))
        .collect();
    loop {
        let mut changed = false;
        for u in 0..units.len() {
            if units[u].1.error.is_some() {
                continue;
            }
            let failed = units[u].1.calls.iter().find_map(|f| {
                let callee = &units[fn_unit[f]];
                callee
                    .1
                    .error
                    .as_ref()
                    .map(|e| (blocks[callee.0].name, e.clone()))
            });
            if let Some((name, e)) = failed {
                units[u].1.error = Some(format!("Function {}: {e}", c.name(name)));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (b, unit) in &units {
        let block = &blocks[*b];
        if let Some(e) = &unit.error {
            if !block.function && c.name(block.name) == "init" {
                bail!("{e}");
            }
            if !block.function {
                c.p.errors.push(format!(
                    "on {} (line {}): {e}",
                    c.name(block.name),
                    block.line
                ));
                c.disable(block);
            }
        }
    }
    // Idioms first: they match loops by their unthreaded shape.
    for at in 0..c.p.code.len() {
        if let Some(l) = super::idiom::find(&c.p.code, at) {
            c.p.code[at] = Op::Loop(c.p.loops.len() as u32);
            c.p.loops.push(l);
        }
    }
    thread(&mut c.p.code);
    fuse(&mut c.p.code);
    Ok(c.p)
}

impl<'a> Compiler<'a> {
    fn name(&self, sym: Sym) -> &'a str {
        self.syms.name(sym)
    }

    fn emit(&mut self, op: Op) {
        self.p.code.push(op);
        self.p.lines.push(self.line);
    }

    fn here(&self) -> u32 {
        self.p.code.len() as u32
    }

    fn label(&mut self) -> u32 {
        self.barrier = self.p.code.len();
        self.here()
    }

    fn patch(&mut self, at: u32) {
        let target = self.label();
        match &mut self.p.code[at as usize] {
            Op::Jump(t) | Op::JumpIfZero(t) | Op::JumpIfNonZero(t) => *t = target,
            Op::Case(c) => self.p.cases[*c as usize].miss = target,
            _ => unreachable!("patching a non-jump"),
        }
    }

    fn string(&mut self, sym: Sym) -> u32 {
        if let Some(&id) = self.string_ids.get(&sym) {
            return id;
        }
        let id = self.p.strings.len() as u32;
        self.p.strings.push(self.syms.name(sym).into());
        self.string_ids.insert(sym, id);
        id
    }

    fn disable(&mut self, block: &Block) {
        if let Some(cb) = Callback::from_name(self.name(block.name)) {
            self.p.callbacks[cb as usize] = None;
        } else if let Some(&v) = block.arg.and_then(|a| self.var_ids.get(&a)) {
            self.p.ui_callbacks[v as usize] = None;
        }
    }

    fn unit(&mut self, block: &Block) -> Unit {
        self.calls.clear();
        let entry = self.label();
        let result = match &block.body {
            Err(e) => Err(anyhow::anyhow!("{e}")),
            Ok(body) => self.stmts(body).and_then(|()| {
                self.emit(if block.function { Op::Ret } else { Op::Halt });
                if block.function {
                    self.p.functions[self.fn_ids[&block.name] as usize] = entry;
                } else {
                    self.register(block, entry)?;
                }
                Ok(())
            }),
        };
        Unit {
            calls: std::mem::take(&mut self.calls),
            error: result.err().map(|e| format!("{e:#}")),
        }
    }

    fn register(&mut self, block: &Block, entry: u32) -> Result<()> {
        let name = self.name(block.name);
        if name == "ui_control" {
            let arg = block.arg.context("ui_control callback without a control")?;
            let v = *self
                .var_ids
                .get(&arg)
                .with_context(|| format!("Unknown control {}", self.name(arg)))?;
            ensure!(
                self.p.vars[v as usize].ui.is_some(),
                "{} is not a UI control",
                self.name(arg)
            );
            ensure!(
                self.p.ui_callbacks[v as usize].replace(entry).is_none(),
                "Duplicate ui_control callback"
            );
        } else if let Some(cb) = Callback::from_name(name) {
            ensure!(
                self.p.callbacks[cb as usize].replace(entry).is_none(),
                "Duplicate {name} callback"
            );
        } else {
            self.p
                .diagnostics
                .insert(format!("Callback on {name} is not dispatched"));
        }
        Ok(())
    }

    // ---- Declarations ----------------------------------------------------------------

    fn declare_all(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            self.line = s.line;
            match &s.kind {
                StmtKind::Declare(d) => self
                    .declare(d)
                    .with_context(|| format!("KSP line {}", s.line))?,
                StmtKind::If(_, a, b) => {
                    self.declare_all(a)?;
                    self.declare_all(b)?;
                }
                StmtKind::While(_, a) => self.declare_all(a)?,
                StmtKind::Select(_, cases) => {
                    for c in cases {
                        self.declare_all(&c.body)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn declare(&mut self, d: &Declare) -> Result<()> {
        let name = self.name(d.name);
        ensure!(
            !self.var_ids.contains_key(&d.name),
            "Duplicate variable {name}"
        );
        ensure!(self.p.vars.len() < MAX_VARS, "KSP variable limit");
        let ty = Ty::of(name);
        let array = name.starts_with(['%', '?', '!']);
        ensure!(
            array == d.size.is_some(),
            "Array syntax does not match variable type of {name}"
        );
        let len = match &d.size {
            Some(size) => {
                let Some(Const::Int(n)) = self.fold(size) else {
                    bail!("Array size of {name} must be a constant integer")
                };
                let n = u32::try_from(n)
                    .ok()
                    .filter(|n| *n <= MAX_ARRAY_LEN)
                    .context("KSP array memory limit")?;
                ensure!(
                    self.elements + n <= MAX_TOTAL_ELEMENTS,
                    "KSP array memory limit"
                );
                self.elements += n;
                Some(n)
            }
            None => None,
        };
        let constant = if d.constant && !array {
            d.init.first().and_then(|e| self.fold(e))
        } else {
            None
        };
        let ui = d.ui.map(|u| self.name(u).into());
        ensure!(
            d.ui.is_none() || self.name(d.ui.unwrap_or_default()).starts_with("ui_"),
            "Unknown declaration keyword {}",
            self.name(d.ui.unwrap_or_default())
        );
        let poly = d.polyphonic;
        ensure!(
            !poly || (ty == Ty::Int && !array),
            "Only integer scalars can be polyphonic: {name}"
        );
        let slot = self.alloc(ty, poly, len.unwrap_or(1));
        let id = self.p.vars.len() as VarId;
        self.p.vars.push(Var {
            name: name.into(),
            ty,
            len,
            slot,
            poly,
            constant,
            ui,
            params: Box::new([]),
        });
        self.p.ui_callbacks.push(None);
        self.var_ids.insert(d.name, id);
        Ok(())
    }

    fn alloc(&mut self, ty: Ty, poly: bool, n: u32) -> u32 {
        let counter = match (ty, poly) {
            (_, true) => &mut self.p.poly,
            (Ty::Int, _) => &mut self.p.ints,
            (Ty::Real, _) => &mut self.p.real_slots,
            (Ty::Str, _) => &mut self.p.strs,
        };
        let slot = *counter;
        *counter += n;
        slot
    }

    fn sys_array(&mut self, a: SysArray) -> VarId {
        if let Some(v) = self.p.sys_arrays[a as usize] {
            return v;
        }
        let len = a.len(self.setup.groups);
        let slot = self.alloc(Ty::Int, false, len);
        let id = self.p.vars.len() as VarId;
        let name = match a {
            SysArray::KeyDown => "%KEY_DOWN",
            SysArray::Cc => "%CC",
            SysArray::CcTouched => "%CC_TOUCHED",
            SysArray::PolyAt => "%POLY_AT",
            SysArray::GroupsSelected => "%GROUPS_SELECTED",
        };
        self.p.vars.push(Var {
            name: name.into(),
            ty: Ty::Int,
            len: Some(len),
            slot,
            poly: false,
            constant: None,
            ui: None,
            params: Box::new([]),
        });
        self.p.ui_callbacks.push(None);
        self.p.sys_arrays[a as usize] = Some(id);
        id
    }

    /// Resolve a variable reference to a declared or runtime-maintained variable.
    fn var(&mut self, sym: Sym) -> Option<VarId> {
        if let Some(&v) = self.var_ids.get(&sym) {
            return Some(v);
        }
        let a = SysArray::from_name(self.name(sym))?;
        Some(self.sys_array(a))
    }

    fn named_constant(&mut self, sym: Sym) -> Option<i32> {
        let name = self.syms.name(sym);
        match name {
            "$NUM_GROUPS" => return i32::try_from(self.setup.groups).ok(),
            "$NUM_OUTPUT_CHANNELS" => return i32::try_from(self.setup.outputs).ok(),
            _ => {}
        }
        if let Some(v) = builtins::constant(name).or_else(|| builtins::symbol(name)) {
            return Some(v);
        }
        // Undeclared uppercase names are Kontakt constants; only identity matters.
        if !(name.starts_with('$') && name.as_bytes().get(1).is_some_and(u8::is_ascii_uppercase)) {
            return None;
        }
        if let Some(&v) = self.auto_ids.get(&sym) {
            return Some(v);
        }
        let v = AUTO_SYMBOL_BASE + self.p.auto_symbols.len() as i32;
        self.p.auto_symbols.push(name.into());
        self.auto_ids.insert(sym, v);
        Some(v)
    }

    fn fold(&mut self, e: &Expr) -> Option<Const> {
        Some(match e {
            Expr::Int(n) => Const::Int(*n),
            Expr::Real(n) => Const::Real(*n),
            Expr::Var(sym, None) => match self.var_ids.get(sym) {
                Some(&v) => self.p.vars[v as usize].constant?,
                None if builtins::sys_var(self.name(*sym)).is_some() => return None,
                None => match builtins::real_constant(self.name(*sym)) {
                    Some(x) => Const::Real(x),
                    None => Const::Int(self.named_constant(*sym)?),
                },
            },
            Expr::Unary(op, e) => match (op, self.fold(e)?) {
                (UnOp::Neg, Const::Int(n)) => Const::Int(n.wrapping_neg()),
                (UnOp::Neg, Const::Real(n)) => Const::Real(-n),
                (UnOp::Not, Const::Int(n)) => Const::Int((n == 0) as i32),
                (UnOp::BitNot, Const::Int(n)) => Const::Int(!n),
                _ => return None,
            },
            Expr::Binary(op, a, b) => match (self.fold(a)?, self.fold(b)?) {
                (Const::Int(a), Const::Int(b)) => Const::Int(fold_int(*op, a, b)?),
                (Const::Real(a), Const::Real(b)) => Const::Real(match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div if b != 0.0 => a / b,
                    _ => return None,
                }),
                _ => return None,
            },
            Expr::Call(name, args) => {
                let arg = |c: &mut Self, i: usize| args.get(i).and_then(|e| c.fold(e));
                match (self.name(*name), args.len()) {
                    ("sh_left", 2) => match (arg(self, 0)?, arg(self, 1)?) {
                        (Const::Int(a), Const::Int(b)) if (0..32).contains(&b) => {
                            Const::Int(a.wrapping_shl(b as u32))
                        }
                        _ => return None,
                    },
                    ("sh_right", 2) => match (arg(self, 0)?, arg(self, 1)?) {
                        (Const::Int(a), Const::Int(b)) if (0..32).contains(&b) => {
                            Const::Int(a >> b)
                        }
                        _ => return None,
                    },
                    ("int_to_real" | "real", 1) => match arg(self, 0)? {
                        Const::Int(a) => Const::Real(a as f64),
                        _ => return None,
                    },
                    ("num_elements", 1) => match &args[0] {
                        Expr::Var(sym, None) => {
                            let v = self.var(*sym)?;
                            Const::Int(self.p.vars[v as usize].len? as i32)
                        }
                        _ => return None,
                    },
                    _ => return None,
                }
            }
            _ => return None,
        })
    }

    // ---- Statements ------------------------------------------------------------------

    fn stmts(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            self.line = s.line;
            self.stmt(s)
                .with_context(|| format!("KSP line {}", s.line))?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<()> {
        match &s.kind {
            StmtKind::Declare(d) => self.declaration(d),
            StmtKind::Assign(target, value) => {
                let Expr::Var(sym, index) = target else {
                    unreachable!("parser produces variable targets")
                };
                self.assign(*sym, index.as_deref(), |c| c.expr(value))
            }
            StmtKind::Command(name, args) => match self.name(*name) {
                "inc" | "dec" => {
                    let delta = if self.name(*name) == "inc" { 1 } else { -1 };
                    let [Expr::Var(sym, index)] = args.as_slice() else {
                        bail!("inc/dec requires one integer variable")
                    };
                    self.assign(*sym, index.as_deref(), |c| {
                        c.expr_ty(&args[0], Ty::Int)?;
                        c.emit(Op::PushI(delta));
                        c.emit(Op::IAdd);
                        Ok(Ty::Int)
                    })
                }
                _ => self.call(*name, args, false).map(drop),
            },
            StmtKind::If(cond, yes, no) => {
                self.expr_ty(cond, Ty::Int)?;
                let skip = self.here();
                self.emit(Op::JumpIfZero(0));
                self.stmts(yes)?;
                if no.is_empty() {
                    self.patch(skip);
                } else {
                    let end = self.here();
                    self.emit(Op::Jump(0));
                    self.patch(skip);
                    self.stmts(no)?;
                    self.patch(end);
                }
                Ok(())
            }
            StmtKind::While(cond, body) => {
                let top = self.label();
                self.expr_ty(cond, Ty::Int)?;
                let exit = self.here();
                self.emit(Op::JumpIfZero(0));
                self.stmts(body)?;
                self.emit(Op::Jump(top));
                self.patch(exit);
                Ok(())
            }
            StmtKind::Select(value, cases) => {
                self.expr_ty(value, Ty::Int)?;
                let mut ends = Vec::new();
                for case in cases {
                    self.line = case.line;
                    let low = self.case_label(&case.low)?;
                    let high = case
                        .high
                        .as_ref()
                        .map(|h| self.case_label(h))
                        .transpose()?
                        .unwrap_or(low);
                    ensure!(low <= high, "Reversed case range at line {}", case.line);
                    let arm = self.here();
                    self.p.cases.push(CaseArm { low, high, miss: 0 });
                    self.emit(Op::Case(self.p.cases.len() as u32 - 1));
                    self.stmts(&case.body)?;
                    ends.push(self.here());
                    self.emit(Op::Jump(0));
                    self.patch(arm);
                }
                self.emit(Op::PopI);
                for e in ends {
                    self.patch(e);
                }
                Ok(())
            }
            StmtKind::Call(name) => {
                let f = *self
                    .fn_ids
                    .get(name)
                    .with_context(|| format!("Unknown function {}", self.name(*name)))?;
                self.calls.push(f);
                self.emit(Op::Call(f));
                Ok(())
            }
        }
    }

    fn case_label(&mut self, e: &Expr) -> Result<i32> {
        match self.fold(e) {
            Some(Const::Int(n)) => Ok(n),
            _ => bail!("Case labels must be constant integers"),
        }
    }

    fn declaration(&mut self, d: &Declare) -> Result<()> {
        let v = self.var_ids[&d.name];
        let (ty, slot, len, poly) = {
            let var = &self.p.vars[v as usize];
            (var.ty, var.slot, var.len, var.poly)
        };
        let mut params = Vec::with_capacity(d.params.len());
        if self.p.vars[v as usize].ui.is_some() {
            for e in &d.params {
                params.push(self.expr(e)?);
            }
        } else {
            ensure!(d.params.is_empty(), "Unexpected declaration parameters");
        }
        self.p.vars[v as usize].params = params.into();
        self.emit(Op::Declare(v));
        if d.persistent {
            self.emit(Op::Ref(v));
            self.emit(Op::Builtin(Builtin::MakePersistent, 1));
        }
        if d.init.is_empty() {
            return Ok(());
        }
        let Some(len) = len else {
            ensure!(d.init.len() == 1, "Scalar initializer must be one value");
            self.expr_ty(&d.init[0], ty)?;
            self.emit(match (ty, poly) {
                (_, true) => Op::StPoly(slot),
                (Ty::Int, _) => Op::StI(slot),
                (Ty::Real, _) => Op::StR(slot),
                (Ty::Str, _) => Op::StS(slot),
            });
            return Ok(());
        };
        ensure!(
            d.init.len() as u32 <= len.max(1),
            "Too many array initializers"
        );
        let folded: Option<Vec<Const>> = d.init.iter().map(|e| self.fold(e)).collect();
        let data = match (ty, folded) {
            (Ty::Int, Some(c)) => c
                .iter()
                .map(|c| match c {
                    Const::Int(n) => Some(*n),
                    Const::Real(_) => None,
                })
                .collect::<Option<Box<[i32]>>>()
                .map(InitData::Int),
            (Ty::Real, Some(c)) => c
                .iter()
                .map(|c| match c {
                    Const::Real(n) => Some(*n),
                    Const::Int(_) => None,
                })
                .collect::<Option<Box<[f64]>>>()
                .map(InitData::Real),
            (Ty::Str, _) => d
                .init
                .iter()
                .map(|e| match e {
                    Expr::Str(s) => Some(self.string(*s)),
                    _ => None,
                })
                .collect::<Option<Box<[u32]>>>()
                .map(InitData::Str),
            _ => None,
        };
        if let Some(data) = data {
            self.p.inits.push(ArrayInit { var: v, data });
            self.emit(Op::InitArray(self.p.inits.len() as u32 - 1));
            return Ok(());
        }
        // Computed initializers: element stores, the last value filling the tail.
        for i in 0..len {
            let e = &d.init[(i as usize).min(d.init.len() - 1)];
            self.emit(Op::PushI(i as i32));
            self.expr_ty(e, ty)?;
            self.emit(match ty {
                Ty::Int => Op::StIA(v),
                Ty::Real => Op::StRA(v),
                Ty::Str => Op::StSA(v),
            });
        }
        Ok(())
    }

    fn assign(
        &mut self,
        sym: Sym,
        index: Option<&Expr>,
        value: impl FnOnce(&mut Self) -> Result<Ty>,
    ) -> Result<()> {
        let name = self.name(sym);
        if let (EVENT_PAR, Some(index)) = (name, index) {
            self.emit(Op::Sys(SysVar::EventId));
            self.expr_ty(index, Ty::Int)?;
            self.value_as(value, Ty::Int)?;
            self.emit(Op::Builtin(Builtin::SetEventPar, 3));
            return Ok(());
        }
        let v = self
            .var(sym)
            .with_context(|| format!("Undeclared variable {name}"))?;
        let (ty, slot, array, poly, constant) = {
            let var = &self.p.vars[v as usize];
            (
                var.ty,
                var.slot,
                var.is_array(),
                var.poly,
                var.constant.is_some(),
            )
        };
        ensure!(!constant, "Assignment to constant {name}");
        match (array, index) {
            (true, Some(index)) => {
                self.expr_ty(index, Ty::Int)?;
                self.value_as(value, ty)?;
                self.emit(match ty {
                    Ty::Int => Op::StIA(v),
                    Ty::Real => Op::StRA(v),
                    Ty::Str => Op::StSA(v),
                });
            }
            (false, None) => {
                self.value_as(value, ty)?;
                self.emit(match (ty, poly) {
                    (_, true) => Op::StPoly(slot),
                    (Ty::Int, _) => Op::StI(slot),
                    (Ty::Real, _) => Op::StR(slot),
                    (Ty::Str, _) => Op::StS(slot),
                });
            }
            (true, None) => bail!("Whole-array assignment to {name} is unsupported"),
            (false, Some(_)) => bail!("{name} is not an array"),
        }
        Ok(())
    }

    fn value_as(&mut self, value: impl FnOnce(&mut Self) -> Result<Ty>, want: Ty) -> Result<()> {
        let got = value(self)?;
        self.coerce(got, want)
    }

    fn coerce(&mut self, got: Ty, want: Ty) -> Result<()> {
        match (got, want) {
            (a, b) if a == b => {}
            (Ty::Int, Ty::Str) => self.emit(Op::IToS),
            (Ty::Real, Ty::Str) => self.emit(Op::RToS),
            (Ty::Int, Ty::Real) | (Ty::Real, Ty::Int) => {
                bail!("Expected {want:?}, found {got:?}; use int_to_real/real_to_int")
            }
            _ => bail!("Expected {want:?}, found {got:?}"),
        }
        Ok(())
    }

    // ---- Expressions -----------------------------------------------------------------

    fn expr_ty(&mut self, e: &Expr, want: Ty) -> Result<()> {
        let got = self.expr(e)?;
        self.coerce(got, want)
    }

    fn expr(&mut self, e: &Expr) -> Result<Ty> {
        match e {
            Expr::Int(n) => {
                self.emit(Op::PushI(*n));
                Ok(Ty::Int)
            }
            Expr::Real(n) => {
                self.p.reals.push(*n);
                self.emit(Op::PushR(self.p.reals.len() as u32 - 1));
                Ok(Ty::Real)
            }
            Expr::Str(s) => {
                let id = self.string(*s);
                self.emit(Op::PushS(id));
                Ok(Ty::Str)
            }
            Expr::Ident(sym) => bail!("Unexpected identifier {}", self.name(*sym)),
            Expr::Var(sym, index) => self.load(*sym, index.as_deref()),
            Expr::Call(name, args) => self
                .call(*name, args, true)?
                .context("Function has no value"),
            Expr::Unary(op, inner) => {
                let ty = self.expr(inner)?;
                match (op, ty) {
                    (UnOp::Neg, Ty::Int) => self.fold_or_emit(Op::INeg),
                    (UnOp::Neg, Ty::Real) => self.emit(Op::RNeg),
                    (UnOp::Not, Ty::Int) => self.fold_or_emit(Op::INot),
                    (UnOp::BitNot, Ty::Int) => self.fold_or_emit(Op::IBitNot),
                    _ => bail!("Invalid operand type {ty:?} for {op:?}"),
                }
                Ok(ty)
            }
            Expr::Binary(op, a, b) => self.binary(*op, a, b),
        }
    }

    fn load(&mut self, sym: Sym, index: Option<&Expr>) -> Result<Ty> {
        let name = self.name(sym);
        if let Some(v) = self.var(sym) {
            let (ty, slot, array, poly, constant) = {
                let var = &self.p.vars[v as usize];
                (var.ty, var.slot, var.is_array(), var.poly, var.constant)
            };
            match (array, index) {
                (true, Some(index)) => {
                    self.expr_ty(index, Ty::Int)?;
                    self.emit(match ty {
                        Ty::Int => Op::LdIA(v),
                        Ty::Real => Op::LdRA(v),
                        Ty::Str => Op::LdSA(v),
                    });
                }
                (false, None) => match constant {
                    Some(Const::Int(n)) => self.emit(Op::PushI(n)),
                    Some(Const::Real(n)) => {
                        self.p.reals.push(n);
                        self.emit(Op::PushR(self.p.reals.len() as u32 - 1));
                    }
                    None => self.emit(match (ty, poly) {
                        (_, true) => Op::LdPoly(slot),
                        (Ty::Int, _) => Op::LdI(slot),
                        (Ty::Real, _) => Op::LdR(slot),
                        (Ty::Str, _) => Op::LdS(slot),
                    }),
                },
                (true, None) => bail!("Whole-array expressions are unsupported: {name}"),
                (false, Some(_)) => bail!("{name} is not an array"),
            }
            return Ok(ty);
        }
        if let (EVENT_PAR, Some(index)) = (name, index) {
            self.emit(Op::Sys(SysVar::EventId));
            self.expr_ty(index, Ty::Int)?;
            self.emit(Op::Builtin(Builtin::GetEventPar, 2));
            return Ok(Ty::Int);
        }
        ensure!(index.is_none(), "Undeclared array {name}");
        if let Some(x) = builtins::real_constant(name) {
            self.p.reals.push(x);
            self.emit(Op::PushR(self.p.reals.len() as u32 - 1));
            return Ok(Ty::Real);
        }
        if let Some(s) = builtins::sys_var(name) {
            self.emit(Op::Sys(s));
            return Ok(Ty::Int);
        }
        let n = self
            .named_constant(sym)
            .with_context(|| format!("Undeclared variable {}", self.name(sym)))?;
        self.emit(Op::PushI(n));
        Ok(Ty::Int)
    }

    /// Emit an integer op, folding it when its operands are constants.
    fn fold_or_emit(&mut self, op: Op) {
        let code = &self.p.code;
        let n = code.len();
        let unary = matches!(op, Op::INeg | Op::INot | Op::IBitNot);
        if unary && n > self.barrier {
            if let Op::PushI(a) = code[n - 1] {
                let v = match op {
                    Op::INeg => a.wrapping_neg(),
                    Op::INot => (a == 0) as i32,
                    _ => !a,
                };
                self.p.code[n - 1] = Op::PushI(v);
                return;
            }
        }
        if !unary && n >= 2 && n - 2 >= self.barrier {
            if let (Op::PushI(a), Op::PushI(b)) = (code[n - 2], code[n - 1]) {
                if let Some(v) = int_op(op).and_then(|bop| fold_int(bop, a, b)) {
                    self.p.code.truncate(n - 1);
                    self.p.lines.truncate(n - 1);
                    self.p.code[n - 2] = Op::PushI(v);
                    return;
                }
            }
        }
        self.emit(op);
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr) -> Result<Ty> {
        match op {
            BinOp::Concat => {
                self.expr_ty(a, Ty::Str)?;
                self.expr_ty(b, Ty::Str)?;
                self.emit(Op::Concat);
                return Ok(Ty::Str);
            }
            BinOp::And | BinOp::Or => {
                // Short-circuit: never faults on a guarded array index.
                self.expr_ty(a, Ty::Int)?;
                let short = self.here();
                self.emit(if op == BinOp::And {
                    Op::JumpIfZero(0)
                } else {
                    Op::JumpIfNonZero(0)
                });
                self.expr_ty(b, Ty::Int)?;
                self.emit(Op::PushI(0));
                self.emit(Op::INe);
                let end = self.here();
                self.emit(Op::Jump(0));
                self.patch(short);
                self.emit(Op::PushI((op == BinOp::Or) as i32));
                self.patch(end);
                return Ok(Ty::Int);
            }
            BinOp::Xor => {
                for e in [a, b] {
                    self.expr_ty(e, Ty::Int)?;
                    self.emit(Op::PushI(0));
                    self.emit(Op::INe);
                }
                self.emit(Op::INe);
                return Ok(Ty::Int);
            }
            _ => {}
        }
        let ta = self.expr(a)?;
        let tb = self.expr(b)?;
        ensure!(ta == tb, "Mixed {ta:?}/{tb:?} operands; convert explicitly");
        let compare = matches!(
            op,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
        );
        let result = if compare { Ty::Int } else { ta };
        match ta {
            Ty::Int => {
                let code = match op {
                    BinOp::Add => Op::IAdd,
                    BinOp::Sub => Op::ISub,
                    BinOp::Mul => Op::IMul,
                    BinOp::Div => Op::IDiv,
                    BinOp::Mod => Op::IMod,
                    BinOp::BitAnd => Op::IBitAnd,
                    BinOp::BitOr => Op::IBitOr,
                    BinOp::BitXor => Op::IBitXor,
                    BinOp::Eq => Op::IEq,
                    BinOp::Ne => Op::INe,
                    BinOp::Lt => Op::ILt,
                    BinOp::Gt => Op::IGt,
                    BinOp::Le => Op::ILe,
                    BinOp::Ge => Op::IGe,
                    _ => unreachable!(),
                };
                self.fold_or_emit(code);
            }
            Ty::Real => self.emit(match op {
                BinOp::Add => Op::RAdd,
                BinOp::Sub => Op::RSub,
                BinOp::Mul => Op::RMul,
                BinOp::Div => Op::RDiv,
                BinOp::Mod => Op::RMod,
                BinOp::Eq => Op::REq,
                BinOp::Ne => Op::RNe,
                BinOp::Lt => Op::RLt,
                BinOp::Gt => Op::RGt,
                BinOp::Le => Op::RLe,
                BinOp::Ge => Op::RGe,
                _ => bail!("Invalid real operator {op:?}"),
            }),
            Ty::Str => self.emit(match op {
                BinOp::Eq => Op::SEq,
                BinOp::Ne => Op::SNe,
                _ => bail!("Invalid string operator {op:?}"),
            }),
        }
        Ok(result)
    }

    fn call(&mut self, name: Sym, args: &[Expr], want_value: bool) -> Result<Option<Ty>> {
        let fname = self.name(name);
        match (fname, args) {
            ("get_ui_id", [Expr::Var(sym, None)]) => {
                let v = self
                    .var(*sym)
                    .with_context(|| format!("Undeclared variable {}", self.name(*sym)))?;
                self.emit(Op::UiId(v));
                return Ok(Some(Ty::Int));
            }
            ("num_elements", [e]) => {
                let Some(Const::Int(n)) = self.fold(&Expr::Call(name, vec![e.clone()])) else {
                    bail!("num_elements requires an array")
                };
                self.emit(Op::PushI(n));
                return Ok(Some(Ty::Int));
            }
            ("exit", []) => {
                self.emit(Op::Exit);
                return Ok(None);
            }
            _ => {}
        }
        let b = Builtin::from_name(fname)
            .with_context(|| format!("Unsupported KSP function: {fname}"))?;
        let sig = b.sig();
        let max = sig.args.len();
        let min = max - sig.optional as usize;
        ensure!(
            (min..=max).contains(&args.len()),
            "{fname} expects {min}..{max} arguments, got {}",
            args.len()
        );
        let mut num = None;
        for (e, kind) in args.iter().zip(sig.args) {
            match kind {
                Arg::I => self.expr_ty(e, Ty::Int)?,
                Arg::R => self.expr_ty(e, Ty::Real)?,
                Arg::S => self.expr_ty(e, Ty::Str)?,
                Arg::N => {
                    let ty = self.expr(e)?;
                    ensure!(ty != Ty::Str, "{fname} requires numbers");
                    ensure!(
                        num.is_none_or(|n| n == ty),
                        "{fname} mixes integer and real arguments"
                    );
                    num = Some(ty);
                }
                Arg::V | Arg::A => {
                    let Expr::Var(sym, None) = e else {
                        bail!("{fname} requires a variable")
                    };
                    let v = self
                        .var(*sym)
                        .with_context(|| format!("Undeclared variable {}", self.name(*sym)))?;
                    ensure!(
                        *kind == Arg::V || self.p.vars[v as usize].is_array(),
                        "{fname} requires an array"
                    );
                    self.emit(Op::Ref(v));
                }
                Arg::K => {
                    let (Expr::Ident(s) | Expr::Str(s)) = e else {
                        bail!("{fname} requires a key name")
                    };
                    let id = self.string(*s);
                    self.emit(Op::Ref(id));
                }
            }
        }
        if b == Builtin::Search {
            let Expr::Var(sym, None) = &args[0] else {
                unreachable!()
            };
            let v = self.var(*sym).unwrap_or_default();
            ensure!(
                Some(self.p.vars[v as usize].ty) == num,
                "search value type must match the array"
            );
        }
        let b = match (b, num) {
            (Builtin::Abs, Some(Ty::Real)) => Builtin::AbsReal,
            (Builtin::Min, Some(Ty::Real)) => Builtin::MinReal,
            (Builtin::Max, Some(Ty::Real)) => Builtin::MaxReal,
            (Builtin::InRange, Some(Ty::Real)) => Builtin::InRangeReal,
            _ => b,
        };
        self.emit(Op::Builtin(b, args.len() as u8));
        let ret = match sig.ret {
            Ret::Void => None,
            Ret::Int => Some(Ty::Int),
            Ret::Real => Some(Ty::Real),
            Ret::Str => Some(Ty::Str),
            Ret::Num => num,
        };
        if !want_value {
            match ret {
                Some(Ty::Int) => self.emit(Op::PopI),
                Some(Ty::Real) => self.emit(Op::PopR),
                Some(Ty::Str) => self.emit(Op::PopS),
                None => {}
            }
        }
        Ok(ret)
    }
}

fn int_op(op: Op) -> Option<BinOp> {
    Some(match op {
        Op::IAdd => BinOp::Add,
        Op::ISub => BinOp::Sub,
        Op::IMul => BinOp::Mul,
        Op::IDiv => BinOp::Div,
        Op::IMod => BinOp::Mod,
        Op::IBitAnd => BinOp::BitAnd,
        Op::IBitOr => BinOp::BitOr,
        Op::IBitXor => BinOp::BitXor,
        Op::IEq => BinOp::Eq,
        Op::INe => BinOp::Ne,
        Op::ILt => BinOp::Lt,
        Op::IGt => BinOp::Gt,
        Op::ILe => BinOp::Le,
        Op::IGe => BinOp::Ge,
        _ => return None,
    })
}

fn fold_int(op: BinOp, a: i32, b: i32) -> Option<i32> {
    Some(match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div if b != 0 => a.wrapping_div(b),
        BinOp::Mod if b != 0 => a.wrapping_rem(b),
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        BinOp::Eq => (a == b) as i32,
        BinOp::Ne => (a != b) as i32,
        BinOp::Lt => (a < b) as i32,
        BinOp::Gt => (a > b) as i32,
        BinOp::Le => (a <= b) as i32,
        BinOp::Ge => (a >= b) as i32,
        _ => return None,
    })
}
