//! KSP -> Luau front end (universal script phase 3). Compiles the resolved HIR
//! to Luau source that runs on `sampler_script` VMs through [`host::Host`].
//!
//! Semantics mirror the interpreter in `eval.rs`; `tests.rs` is the contract.
//! Luau numbers are doubles, so i32 wrap, truncating division and real->int
//! conversion are done by the prelude below. Constructs outside the subset are
//! not emitted: they are counted by name in [`Compiled::unsupported`].

use crate::builtins::{Builtin, SysVar};
use crate::hir::{Arg, Arith, CallbackKind, Case, Const, Expr, ExprKind, Hir, Home, Logic, Place, Stmt, StmtKind, Ty, VarId};
use sampler_core::Comparison;
use std::collections::BTreeMap;
use std::fmt::Write;

pub mod host;
mod controls;
#[cfg(test)]
mod tests;

/// i32 and KSP operator semantics on Luau doubles. Exact for |x| < 2^52.
const PRELUDE: &str = "\
local function WAIT(us)
  if coroutine.isyieldable() then coroutine.yield(us) end
  return 0
end
local TWO31 = 2147483648
local TWO32 = 4294967296
local function W(x) return (x + TWO31) % TWO32 - TWO31 end
local function RI(x)
  if x ~= x then return 0 end
  if x >= TWO31 then return TWO31 - 1 end
  if x <= -TWO31 then return -TWO31 end
  if x >= 0 then return math.floor(x) end
  return math.ceil(x)
end
local function MUL(a, b)
  local au, bu = a % TWO32, b % TWO32
  local al = au % 65536
  local ah = (au - al) / 65536
  local bl = bu % 65536
  local bh = (bu - bl) / 65536
  return W(al * bl + ((ah * bl + al * bh) % 65536) * 65536)
end
local function DIV(a, b)
  if b == 0 then return 0 end
  return W((a - math.fmod(a, b)) / b)
end
local function MOD(a, b)
  if b == 0 then return 0 end
  return math.fmod(a, b) + 0
end
local function RD(c, off, len, i)
  if i < 0 or i >= len then return 0 end
  return c[off + i + 1]
end
local function WR(c, off, len, i, v)
  if i < 0 or i >= len then return end
  c[off + i + 1] = v
end
local function RDT(c, off, len, i)
  if i < 0 or i >= len then return '' end
  return c[off + i + 1]
end
";

/// One script lowered for both paths: the interpreter's `on init` result and the Luau chunk.
pub struct Lowered {
    pub compiled: Compiled,
    pub cells: u32,
    pub texts: u32,
    pub note_cells: u16,
    /// Type of each script cell, to compare reals bit for bit.
    pub cell_types: Vec<Ty>,
    /// Interpreter `on init` result (`eval::run`), the oracle for `Host::init`.
    pub init: crate::eval::Initial,
}

/// Front end for both paths: parse, resolve, run the interpreter's `on init`, and compile the Luau chunk.
pub fn lower(source: &str) -> Result<Lowered, String> {
    let initialized = crate::initialize(source, crate::Limits::LIBRARY, &crate::Environment::default())
        .map_err(|e| format!("{e:?}"))?;
    let crate::Initialized { hir, init, .. } = initialized;
    let mut cell_types = vec![Ty::Int; hir.cells as usize];
    for v in &hir.vars {
        match v.home {
            Home::Cell(c) => cell_types[c as usize] = v.ty,
            Home::Cells { offset, len } => {
                for i in offset..offset + len {
                    cell_types[i as usize] = v.ty;
                }
            }
            _ => {}
        }
    }
    Ok(Lowered {
        compiled: compile(&hir),
        cells: hir.cells,
        texts: hir.texts,
        note_cells: hir.note_cells,
        cell_types,
        init,
    })
}

/// Run the Luau `on init` and compare it with the interpreter's: every script cell
/// (reals bit for bit), every text cell, and every `set_engine_par` write.
/// Returns the number of cells compared.
pub fn check_init(lowered: &Lowered) -> Result<usize, String> {
    if !lowered.compiled.is_complete() {
        return Err(format!("outside the subset: {:?}", lowered.compiled.unsupported));
    }
    let mut host = host::Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000)?;
    host.init()?;
    let luau = host.cells(lowered.cells);
    for (i, (ty, bits)) in lowered.cell_types.iter().zip(&lowered.init.cells).enumerate() {
        let expected = match ty {
            Ty::Real => f64::from_bits(*bits as u64),
            _ => f64::from(*bits as i32),
        };
        let got = luau[i];
        let same = if expected.is_nan() { got.is_nan() } else { got == expected };
        if !same {
            return Err(format!("cell {i} ({ty:?}): interpreter {expected:?}, luau {got:?}"));
        }
    }
    if host.texts(lowered.texts) != lowered.init.texts {
        return Err("text cells differ".into());
    }
    if host.controls() != lowered.init.controls {
        return Err(format!("controls differ: interpreter {:?}, luau {:?}", lowered.init.controls, host.controls()));
    }
    if host.properties() != lowered.init.properties {
        return Err("control properties differ".into());
    }
    if host.text_properties() != lowered.init.text_properties {
        return Err("control text properties differ".into());
    }
    let engine: std::collections::BTreeMap<[i64; 4], i64> = lowered
        .init
        .engine
        .iter()
        .map(|(k, v)| ([i64::from(k[0]), i64::from(k[1]), i64::from(k[2]), i64::from(k[3])], i64::from(*v)))
        .collect();
    if host.engine() != engine {
        return Err("engine writes differ".into());
    }
    Ok(luau.len())
}

/// Result of compiling one script.
#[derive(Debug, Default)]
pub struct Compiled {
    /// Luau chunk. Called with `(cells, texts, host)`; returns the callback table.
    pub source: String,
    /// Constructs outside the subset, by name, with the number of sites.
    pub unsupported: BTreeMap<String, usize>,
    /// UI controls by index, for the host's control state.
    pub ui: Vec<controls::UiMeta>,
}

impl Compiled {
    pub fn is_complete(&self) -> bool {
        self.unsupported.is_empty()
    }
}

/// Compile every callback and function of `hir`. Never fails: unsupported sites
/// are emitted as no-ops and counted, so the caller decides what to run.
pub fn compile(hir: &Hir) -> Compiled {
    let mut c = Compiler { hir, out: String::new(), unsupported: BTreeMap::new(), loops: 0 };
    controls::guard(hir, &mut c.unsupported);
    c.out.push_str(PRELUDE);
    if !hir.functions.is_empty() {
        let names: Vec<String> = (0..hir.functions.len()).map(|i| format!("F{i}")).collect();
        let _ = writeln!(c.out, "local {}", names.join(", "));
    }
    for (i, f) in hir.functions.iter().enumerate() {
        let _ = writeln!(c.out, "F{i} = function()");
        c.block(&f.body);
        let _ = writeln!(c.out, "end -- {}", f.name);
    }
    let mut table = Vec::new();
    for cb in &hir.callbacks {
        let (key, params) = match cb.kind {
            CallbackKind::Init => ("init", ""),
            CallbackKind::Note => ("note", "NOTE, VEL"),
            CallbackKind::Release => ("release", "NOTE, VEL"),
            other => {
                c.unsupported(&format!("callback {other:?}"));
                continue;
            }
        };
        let _ = writeln!(c.out, "local function cb_{key}({params})");
        c.block(&cb.body);
        let _ = writeln!(c.out, "end");
        table.push(format!("{key} = cb_{key}"));
    }
    let _ = writeln!(c.out, "return {{ {}, select_note = function(t) NV = t end }}", table.join(", "));
    Compiled { source: format!("local C, T, NV, HOST = ...\n{}", c.out), unsupported: c.unsupported, ui: controls::meta(hir) }
}

struct Compiler<'h> {
    hir: &'h Hir,
    out: String,
    unsupported: BTreeMap<String, usize>,
    /// Enclosing `while` loops in the current function body.
    loops: usize,
}

fn lua_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {
                let _ = write!(out, "\\{}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn lua_num(x: f64) -> String {
    if x.is_nan() {
        "(0/0)".into()
    } else if x.is_infinite() {
        if x > 0.0 { "math.huge".into() } else { "(-math.huge)".into() }
    } else {
        format!("({x:?})")
    }
}

impl Compiler<'_> {
    fn unsupported(&mut self, what: &str) -> String {
        *self.unsupported.entry(what.to_string()).or_default() += 1;
        "0".into()
    }

    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }

    /// An integer-context value: reals truncate, text is 0 (as the interpreter's `V::int`).
    fn int(&mut self, e: &Expr) -> String {
        match e.ty {
            Ty::Str => "0".into(),
            Ty::Real => format!("RI({})", self.expr(e)),
            _ => self.expr(e),
        }
    }

    /// Value stored into a variable of type `ty` (the interpreter's `write_var` conversion).
    fn coerce(&mut self, e: &Expr, ty: Ty) -> String {
        match ty {
            Ty::Real => match e.ty {
                Ty::Str => "0.0".into(),
                _ => self.expr(e),
            },
            Ty::Str => match e.ty {
                Ty::Str => self.expr(e),
                Ty::Real => self.unsupported("text from real"),
                _ => format!("tostring({})", self.int(e)),
            },
            _ => self.int(e),
        }
    }

    fn cond(&mut self, e: &Expr) -> String {
        format!("({}) ~= 0", self.int(e))
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Assign(place, value) => self.assign(place, value),
            StmtKind::Fill(var, values) => self.fill(*var, values),
            StmtKind::If(cond, yes, no) => {
                let c = self.cond(cond);
                self.line(&format!("if {c} then"));
                self.block(yes);
                if !no.is_empty() {
                    self.line("else");
                    self.block(no);
                }
                self.line("end");
            }
            StmtKind::While(cond, body) => {
                let c = self.cond(cond);
                self.line(&format!("while {c} do"));
                self.loops += 1;
                self.block(body);
                self.loops -= 1;
                self.line("end");
            }
            StmtKind::Select(value, cases) => self.select(value, cases),
            StmtKind::Call(f) => self.line(&format!("F{}()", f.0)),
            // `exit` leaves the enclosing callback or function, even from inside a
            // loop (interpreter: `Flow::Exit` propagates through `While`).
            StmtKind::Builtin(Builtin::Exit, _) => self.line("do return end"),
            StmtKind::Builtin(Builtin::Continue, _) => {
                let s = if self.loops > 0 { "do continue end" } else { "do return end" };
                self.line(s);
            }
            StmtKind::Builtin(Builtin::Inc | Builtin::Dec, args) => self.inc(s, args),
            StmtKind::Builtin(builtin, args) => {
                let call = self.builtin(*builtin, args);
                self.line(&format!("local _ = {call}"));
            }
        }
    }

    fn assign(&mut self, place: &Place, value: &Expr) {
        let v = match place {
            Place::Var(var) => self.var_ty(*var),
            Place::Elem(var, _) => self.var_ty(*var),
        };
        let Some(ty) = v else { return };
        match place {
            Place::Var(var) => {
                let value = self.coerce(value, ty);
                if let Home::Control(ui) = self.hir.vars[var.0 as usize].home {
                    self.line(&format!("HOST.ctl_set({ui}, {value})"));
                    return;
                }
                let target = self.var_target(*var);
                self.line(&format!("{target} = {value}"));
            }
            Place::Elem(var, index) => {
                let idx = self.int(index);
                let value = self.coerce(value, ty);
                let (base, off, len) = self.elem_base(*var);
                self.line(&format!("WR({base}, {off}, {len}, {idx}, {value})"));
            }
        }
    }

    fn fill(&mut self, var: VarId, values: &[Expr]) {
        let v = &self.hir.vars[var.0 as usize];
        let Some(ty) = self.var_ty(var) else { return };
        let len = v.len.unwrap_or(1);
        let (base, off, _) = self.elem_base(var);
        self.line("do");
        self.line("local last = 0");
        for i in 0..len {
            if let Some(e) = values.get(i as usize) {
                let value = self.coerce(e, ty);
                self.line(&format!("last = {value}"));
            }
            self.line(&format!("{base}[{off} + {i} + 1] = last"));
        }
        self.line("end");
    }

    /// `inc`/`dec` on a scalar or array element; the step is `+1` or `-1`.
    fn inc(&mut self, s: &Stmt, args: &[Arg]) {
        let step = if matches!(s.kind, StmtKind::Builtin(Builtin::Inc, _)) { "+ 1" } else { "- 1" };
        let Some(Arg::Place(place)) = args.first() else {
            self.unsupported("inc without place");
            return;
        };
        match place {
            Place::Var(var) => {
                let Some(ty) = self.var_ty(*var) else { return };
                if let Home::Control(ui) = self.hir.vars[var.0 as usize].home {
                    self.line(&format!("HOST.ctl_set({ui}, W({} {step}))", Self::ctl_get(ui)));
                    return;
                }
                let target = self.var_target(*var);
                let next = if ty == Ty::Real { format!("{target} {step}") } else { format!("W({target} {step})") };
                self.line(&format!("{target} = {next}"));
            }
            Place::Elem(var, index) => {
                let Some(ty) = self.var_ty(*var) else { return };
                let idx = self.int(index);
                let (base, off, len) = self.elem_base(*var);
                let current = format!("RD({base}, {off}, {len}, {idx})");
                let next = if ty == Ty::Real { format!("{current} {step}") } else { format!("W({current} {step})") };
                self.line(&format!("WR({base}, {off}, {len}, {idx}, {next})"));
            }
        }
    }

    fn select(&mut self, value: &Expr, cases: &[Case]) {
        let v = self.int(value);
        self.line("do");
        self.line(&format!("local sel = {v}"));
        let mut first = true;
        for case in cases {
            let kw = if first { "if" } else { "elseif" };
            first = false;
            self.line(&format!("{kw} sel >= {} and sel <= {} then", case.low, case.high));
            self.block(&case.body);
        }
        if !first {
            self.line("end");
        }
        self.line("end");
    }

    /// `(table, offset, length)` for an array variable (interpreter: `Home::Cells`/`Texts`).
    fn elem_base(&self, var: VarId) -> (&'static str, u32, u32) {
        let v = &self.hir.vars[var.0 as usize];
        let len = v.len.unwrap_or(0);
        match v.home {
            Home::Cells { offset, .. } => ("C", offset, len),
            Home::Texts { offset, .. } => ("T", offset, len),
            _ => ("C", 0, 0),
        }
    }

    /// Scalar value type of a variable this subset can address, or None (unsupported; counted).
    fn var_ty(&mut self, var: VarId) -> Option<Ty> {
        let v = &self.hir.vars[var.0 as usize];
        match v.home {
            Home::Note(_) if v.ty == Ty::Str => {
                self.unsupported("polyphonic text");
                None
            }
            _ => Some(v.ty),
        }
    }

    /// Lua l-value for a scalar variable.
    fn var_target(&self, var: VarId) -> String {
        let v = &self.hir.vars[var.0 as usize];
        match v.home {
            Home::Cell(c) => format!("C[{}]", c + 1),
            Home::Text(t) => format!("T[{}]", t + 1),
            Home::Note(i) => format!("NV[{}]", i + 1),
            _ => "_".into(),
        }
    }

    fn expr(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(n) => format!("{n}"),
            ExprKind::Real(r) => lua_num(*r),
            ExprKind::Str(s) => lua_str(s),
            ExprKind::Load(var) => self.load(*var),
            ExprKind::LoadElem(var, index) => {
                let idx = self.int(index);
                match self.var_ty(*var) {
                    Some(_) => {
                        let (base, off, len) = self.elem_base(*var);
                        if base == "T" { format!("RDT({base}, {off}, {len}, {idx})") } else { format!("RD({base}, {off}, {len}, {idx})") }
                    }
                    None => "0".into(),
                }
            }
            ExprKind::Sys(sys) => match sys {
                SysVar::EventNote => "NOTE".into(),
                SysVar::EventVelocity => "VEL".into(),
                other => self.unsupported(&format!("sys {other:?}")),
            },
            ExprKind::SysElem(array, _) => self.unsupported(&format!("sys array {array:?}")),
            ExprKind::Neg(inner) => match e.ty {
                Ty::Real => format!("(-{})", self.expr(inner)),
                _ => format!("W(-({}))", self.int(inner)),
            },
            ExprKind::BitNot(inner) => format!("(-({}) - 1)", self.int(inner)),
            ExprKind::Not(inner) => format!("((({}) == 0) and 1 or 0)", self.int(inner)),
            ExprKind::Cast(inner) => {
                if e.ty == Ty::Bool {
                    format!("((({}) ~= 0) and 1 or 0)", self.int(inner))
                } else {
                    self.int(inner)
                }
            }
            ExprKind::Arith(op, l, r) => self.arith(*op, e, l, r),
            ExprKind::Compare(c, l, r) => self.compare(*c, l, r),
            ExprKind::Logic(op, l, r) => {
                let (a, b) = (self.int(l), self.int(r));
                match op {
                    Logic::And => format!("(((({a}) ~= 0) and (({b}) ~= 0)) and 1 or 0)"),
                    Logic::Or => format!("(((({a}) ~= 0) or (({b}) ~= 0)) and 1 or 0)"),
                    Logic::Xor => format!("(((({a}) ~= 0) ~= (({b}) ~= 0)) and 1 or 0)"),
                }
            }
            ExprKind::Concat(parts) => {
                let mut pieces = Vec::new();
                for p in parts {
                    pieces.push(match p.ty {
                        Ty::Str => self.expr(p),
                        Ty::Real => self.unsupported("concat real"),
                        _ => format!("tostring({})", self.int(p)),
                    });
                }
                if pieces.is_empty() { "\"\"".into() } else { format!("({})", pieces.join(" .. ")) }
            }
            ExprKind::Builtin(builtin, args) => self.builtin(*builtin, args),
        }
    }

    fn load(&mut self, var: VarId) -> String {
        let v = &self.hir.vars[var.0 as usize];
        match v.home {
            Home::Cell(c) => format!("C[{}]", c + 1),
            Home::Text(t) => format!("T[{}]", t + 1),
            Home::Note(i) => format!("NV[{}]", i + 1),
            Home::Const(c) => match &self.hir.consts[c as usize] {
                Const::Int(n) => format!("{n}"),
                Const::Real(r) => lua_num(*r),
                Const::Str(s) => lua_str(s),
            },
            Home::Control(ui) => Self::ctl_get(ui),
            Home::Cells { .. } | Home::Texts { .. } => self.unsupported("whole array value"),
        }
    }

    fn arith(&mut self, op: Arith, e: &Expr, l: &Expr, r: &Expr) -> String {
        if l.ty == Ty::Real && r.ty == Ty::Real {
            let (a, b) = (self.expr(l), self.expr(r));
            let sym = match op {
                Arith::Add => "+",
                Arith::Sub => "-",
                Arith::Mul => "*",
                Arith::Div => "/",
                _ => return "0.0".into(),
            };
            return format!("(({a}) {sym} ({b}))");
        }
        let _ = e;
        let (a, b) = (self.int(l), self.int(r));
        match op {
            Arith::Add => format!("W(({a}) + ({b}))"),
            Arith::Sub => format!("W(({a}) - ({b}))"),
            Arith::Mul => format!("MUL({a}, {b})"),
            Arith::Div => format!("DIV({a}, {b})"),
            Arith::Mod => format!("MOD({a}, {b})"),
            Arith::BitAnd => format!("W(bit32.band(({a}) % TWO32, ({b}) % TWO32))"),
            Arith::BitOr => format!("W(bit32.bor(({a}) % TWO32, ({b}) % TWO32))"),
            Arith::BitXor => format!("W(bit32.bxor(({a}) % TWO32, ({b}) % TWO32))"),
        }
    }

    fn compare(&mut self, c: Comparison, l: &Expr, r: &Expr) -> String {
        let sym = match c {
            Comparison::Equal => "==",
            Comparison::NotEqual => "~=",
            Comparison::Less => "<",
            Comparison::LessEqual => "<=",
            Comparison::Greater => ">",
            Comparison::GreaterEqual => ">=",
        };
        let text_both = l.ty == Ty::Str && r.ty == Ty::Str;
        let (a, b) = if text_both || (l.ty == Ty::Real && r.ty == Ty::Real) {
            (self.expr(l), self.expr(r))
        } else {
            (self.int(l), self.int(r))
        };
        if text_both && !matches!(c, Comparison::Equal | Comparison::NotEqual) {
            return "0".into();
        }
        format!("((({a}) {sym} ({b})) and 1 or 0)")
    }

    fn builtin(&mut self, builtin: Builtin, args: &[Arg]) -> String {
        match builtin {
            Builtin::Wait => {
                let micros = match args.first() {
                    Some(Arg::Expr(e)) => self.int(e),
                    _ => "0".into(),
                };
                format!("WAIT({micros})")
            }
            Builtin::SetEnginePar | Builtin::GetEnginePar => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(match a {
                        Arg::Expr(e) => self.int(e),
                        Arg::Key(_) | Arg::Var(..) | Arg::SysArray(_) | Arg::Place(_) => "0".into(),
                    });
                }
                let name = if builtin == Builtin::SetEnginePar { "set_engine_par" } else { "get_engine_par" };
                format!("HOST.{name}({})", parts.join(", "))
            }
            other => match self.control_builtin(other, args) {
                Some(call) => call,
                None => self.unsupported(&format!("builtin {}", other.name())),
            },
        }
    }
}
