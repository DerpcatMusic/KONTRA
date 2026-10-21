//! KSP -> Luau front end (universal script phase 3). Compiles the resolved HIR
//! to Luau source that runs on `sampler_script` VMs through [`host::Host`].
//!
//! Semantics mirror the interpreter in `eval.rs`; `tests.rs` is the contract.
//! Luau numbers are doubles, so i32 wrap, truncating division and real->int
//! conversion are done by the prelude below. Constructs outside the subset are
//! not emitted: they are counted by name in [`Compiled::unsupported`].

use crate::builtins::{self as b, Builtin, SysVar};
use crate::hir::{Arg, Arith, CallbackKind, Case, Const, Expr, ExprKind, Hir, Home, Logic, Place, Stmt, StmtKind, Ty, VarId};
use sampler_core::Comparison;
use std::collections::BTreeMap;
use std::fmt::Write;

pub mod host;
mod controls;
mod events;
mod keys;
mod pgs;
mod record;
mod request;
#[cfg(test)]
mod tests;

/// i32 and KSP operator semantics on Luau doubles. Exact for |x| < 2^52.
const PRELUDE: &str = "\
local function WAIT(us)
  if coroutine.isyieldable() then coroutine.yield(us < 0 and 0 or us) end
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
local function SGN(x)
  if x > 0 then return 1 elseif x < 0 then return -1 end
  return 0
end
local function INRANGE(x, lo, hi)
  return (lo <= x and x <= hi) and 1 or 0
end
local function TDIV(a, b)
  if a >= 0 then return math.floor(a / b) end
  return math.ceil(a / b)
end
local function ROUND(x)
  local t = TDIV(x, 1)
  local d = x - t
  if d >= 0.5 then return t + 1 elseif d <= -0.5 then return t - 1 end
  return t
end
local function SHL(a, n) return W(a * 2 ^ (n % 32)) end
local function SHR(a, n) return math.floor(a / 2 ^ (n % 32)) end
local function MSB(x) return math.floor(x / 128) % 128 end
local function MS_TO_TICKS(x) return W(TDIV(x * 960, 500000)) end
local function TICKS_TO_MS(x) return W(TDIV(x * 500000, 960)) end
local function SEARCH(c, off, len, x, lo, hi)
  if lo < 0 then lo = 0 end
  if hi > len - 1 then hi = len - 1 end
  for i = lo, hi do
    if c[off + i + 1] == x then return i end
  end
  return -1
end
local function SORT(c, off, len, lo, hi, desc)
  if lo < 0 then lo = 0 end
  if hi > len - 1 then hi = len - 1 end
  if lo > hi then return end
  local t = {}
  for i = lo, hi do t[#t + 1] = c[off + i + 1] end
  table.sort(t)
  if desc ~= 0 then
    for i = 1, math.floor(#t / 2) do t[i], t[#t + 1 - i] = t[#t + 1 - i], t[i] end
  end
  for i = lo, hi do c[off + i + 1] = t[i - lo + 1] end
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
    /// The resolved program, to run UI changes on the interpreter (`check_ui`).
    pub hir: Hir,
}

impl Lowered {
    /// Whether script cell `i` is real-typed (compared bit for bit).
    pub fn cell_is_real(&self, i: usize) -> bool {
        matches!(self.cell_types.get(i), Some(Ty::Real))
    }
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
    let compiled = compile(&hir);
    Ok(Lowered {
        compiled,
        cells: hir.cells,
        texts: hir.texts,
        note_cells: hir.note_cells,
        cell_types,
        init,
        hir,
    })
}

/// Run the Luau `on init` and compare it with the interpreter's: every script cell
/// (reals bit for bit), every text cell, and every `set_engine_par` write.
/// Returns the number of cells compared.
pub fn check_init(lowered: &Lowered) -> Result<usize, String> {
    let mut host = new_host(lowered)?;
    host.init()?;
    compare(&host, lowered, &lowered.init)
}

/// Run `on init`, then the user change of UI control `ui` to `value` on the Luau path, and
/// compare with the interpreter's `run_ui_change`. A UI callback that waits is refused:
/// the interpreter runs waits as no-ops, so only synchronous dispatches are comparable.
pub fn check_ui(lowered: &Lowered, ui: usize, value: i32) -> Result<usize, String> {
    let mut host = new_host(lowered)?;
    host.init()?;
    host.ui_change(ui, value)?;
    if host.pending() {
        return Err("a UI callback waits; the interpreter runs waits as no-ops".into());
    }
    let expected = crate::eval::run_ui_change(&lowered.hir, &crate::Environment::default(), ui, value)
        .map_err(|e| format!("interpreter: {e:?}"))?;
    compare(&host, lowered, &expected)
}

fn new_host(lowered: &Lowered) -> Result<host::Host, String> {
    if !lowered.compiled.is_complete() {
        return Err(format!("outside the subset: {:?}", lowered.compiled.unsupported));
    }
    host::Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000)
}

/// Every cell, text, control, property and engine write of `host` against the interpreter's `expected`.
fn compare(host: &host::Host, lowered: &Lowered, expected: &crate::eval::Initial) -> Result<usize, String> {
    let luau = host.cells(lowered.cells);
    for (i, (ty, bits)) in lowered.cell_types.iter().zip(&expected.cells).enumerate() {
        let want = match ty {
            Ty::Real => f64::from_bits(*bits as u64),
            _ => f64::from(*bits as i32),
        };
        let got = luau[i];
        let same = if want.is_nan() { got.is_nan() } else { got == want };
        if !same {
            return Err(format!("cell {i} ({ty:?}): interpreter {want:?}, luau {got:?}"));
        }
    }
    if host.texts(lowered.texts) != expected.texts {
        return Err("text cells differ".into());
    }
    if host.controls() != expected.controls {
        return Err(format!("controls differ: interpreter {:?}, luau {:?}", expected.controls, host.controls()));
    }
    let want: Vec<Vec<(String, i32, bool)>> = (0..lowered.compiled.ui.len())
        .map(|ui| {
            expected.model.interface.widgets.get(ui).map_or_else(Vec::new, |w| {
                w.menu.iter().map(|item| (item.text.clone(), item.value, item.visible)).collect()
            })
        })
        .collect();
    if host.menus() != want {
        return Err(format!("menu items differ: interpreter {want:?}, luau {:?}", host.menus()));
    }
    // `set_engine_par` is also a request in the interpreter; its writes are compared by `engine`.
    let want: Vec<(String, Vec<crate::model::Value>)> = expected
        .model
        .requests
        .iter()
        .filter(|r| r.command != "set_engine_par")
        .map(|r| (r.command.to_string(), r.args.clone()))
        .collect();
    if host.requests() != want {
        return Err(format!("host requests differ: interpreter {want:?}, luau {:?}", host.requests()));
    }
    if host.properties() != expected.properties {
        return Err("control properties differ".into());
    }
    if host.text_properties() != expected.text_properties {
        return Err("control text properties differ".into());
    }
    let engine: std::collections::BTreeMap<[i64; 4], i64> = expected
        .engine
        .iter()
        .map(|(k, v)| ([i64::from(k[0]), i64::from(k[1]), i64::from(k[2]), i64::from(k[3])], i64::from(*v)))
        .collect();
    if host.engine() != engine {
        return Err("engine writes differ".into());
    }
    if host.pgs() != expected.model.pgs {
        return Err(format!("PGS keys differ: interpreter {:?}, luau {:?}", expected.model.pgs, host.pgs()));
    }
    if host.pgs_text() != expected.model.pgs_text {
        return Err("PGS text keys differ".into());
    }
    let (keys, key_ranges, key_pressed_support) = host.key_state();
    let want = &expected.model.interface;
    if keys != want.keys || key_ranges != want.key_ranges || key_pressed_support != want.key_pressed_support {
        return Err("key-switch state differs".into());
    }
    let ui = host.interface();
    let want = &expected.model.interface;
    let got = (ui.width_px, ui.height_px, ui.height_grid, ui.skin_offset, ui.performance_view, &ui.messages);
    let wanted = (want.width_px, want.height_px, want.height_grid, want.skin_offset, want.performance_view, &want.messages);
    if got != wanted {
        return Err(format!("interface differs: interpreter {wanted:?}, luau {got:?}"));
    }
    if host.title() != expected.model.interface.title {
        return Err(format!("script title differs: interpreter {:?}, luau {:?}", expected.model.interface.title, host.title()));
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
    /// Some callback or function contains a `while` loop: the native-code opt-in.
    pub has_loops: bool,
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
    let mut c = Compiler { hir, out: String::new(), unsupported: BTreeMap::new(), loops: 0, loops_seen: false, ui_id: None, note_ctx: false };
    controls::guard(hir, &mut c.unsupported);
    c.out.push_str(PRELUDE);
    c.out.push_str(pgs::PRELUDE);
    c.out.push_str(record::PRELUDE);
    c.out.push_str(keys::PRELUDE);
    c.out.push_str("local CB = {}\nlocal EV = 0\n");
    c.out.push_str("local SEP, GEP = HOST.set_engine_par, HOST.get_engine_par\n");
    if !hir.functions.is_empty() {
        let names: Vec<String> = (0..hir.functions.len()).map(|i| format!("F{i}")).collect();
        let _ = writeln!(c.out, "local {}", names.join(", "));
    }
    for (i, f) in hir.functions.iter().enumerate() {
        let _ = writeln!(c.out, "F{i} = function()");
        c.block(&f.body);
        let _ = writeln!(c.out, "end -- {}", f.name);
    }
    for cb in &hir.callbacks {
        // `$UI_ID` as the runtime's dispatcher sets it (`lower.rs`): a control's own callback
        // has the control's id, `on ui_controls` gets the changed control's id as a parameter.
        let (key, params, ui_id): (String, &str, String) = match cb.kind {
            CallbackKind::Init => ("init".into(), "", "0".into()),
            CallbackKind::Note => ("note".into(), "NOTE, VEL", "0".into()),
            CallbackKind::Release => ("release".into(), "NOTE, VEL", "0".into()),
            CallbackKind::UiControls => ("ui_controls".into(), "UIID", "UIID".into()),
            CallbackKind::UiControl(var) => match hir.vars[var.0 as usize].ui {
                Some(ui) => (format!("ui_{ui}"), "", (b::FIRST_UI_ID + ui as i32).to_string()),
                None => {
                    c.unsupported("ui_control without control");
                    continue;
                }
            },
            CallbackKind::UiUpdate => ("ui_update".into(), "", "0".into()),
            // Run by the host right after `on init` (the load path); a body that
            // may wait would be scheduled by the interpreter, so it is refused here.
            CallbackKind::PersistenceChanged if !crate::eval::may_suspend(hir, &cb.body, true) => {
                ("persistence_changed".into(), "", "0".into())
            }
            other => {
                c.unsupported(&format!("callback {other:?}"));
                continue;
            }
        };
        let _ = writeln!(c.out, "CB.{key} = function({params})");
        c.ui_id = Some(ui_id);
        c.note_ctx = matches!(cb.kind, CallbackKind::Note);
        c.block(&cb.body);
        c.ui_id = None;
        c.note_ctx = false;
        let _ = writeln!(c.out, "end");
    }
    // Pooled thread body: runs one callback, reports done with -1, then waits for the
    // next job, so a finished coroutine is reused instead of created per event.
    c.out.push_str("local function RUNNER(f, t, n, v, e)\n  while true do\n    NV, EV = t, e\n    f(n, v)\n    f, t, n, v, e = coroutine.yield(-1)\n  end\nend\n");
    let _ = writeln!(c.out, "CB.select_note = function(t, e) NV, EV = t, e end");
    let _ = writeln!(c.out, "CB.runner = RUNNER");
    let _ = writeln!(c.out, "CB.pgs, CB.pgs_text, CB.model = PGS, PGS_TEXT, MODEL");
    let _ = writeln!(c.out, "return CB");
    let has_loops = c.loops_seen;
    Compiled { source: format!("local C, T, NV, HOST = ...\n{}", c.out), unsupported: c.unsupported, has_loops, ui: controls::meta(hir) }
}

/// i32 wrap of a double-valued Lua expression, inline (no call, no interrupt).
/// Exact while the operand is below 2^52 in magnitude, which i32 operands are.
fn wrap(expr: &str) -> String {
    format!("((({expr}) + 2147483648) % 4294967296 - 2147483648)")
}

/// No calls, waits or engine access: safe to evaluate twice.
fn is_pure(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Real(_) | ExprKind::Str(_) | ExprKind::Load(_) | ExprKind::Sys(_) => true,
        ExprKind::LoadElem(_, i) | ExprKind::Neg(i) | ExprKind::BitNot(i) | ExprKind::Not(i) | ExprKind::Cast(i) => is_pure(i),
        ExprKind::Arith(_, l, r) | ExprKind::Compare(_, l, r) | ExprKind::Logic(_, l, r) => is_pure(l) && is_pure(r),
        ExprKind::Concat(parts) => parts.iter().all(is_pure),
        ExprKind::SysElem(..) | ExprKind::Builtin(..) => false,
    }
}

fn compare_sym(c: Comparison) -> &'static str {
    match c {
        Comparison::Equal => "==",
        Comparison::NotEqual => "~=",
        Comparison::Less => "<",
        Comparison::LessEqual => "<=",
        Comparison::Greater => ">",
        Comparison::GreaterEqual => ">=",
    }
}

struct Compiler<'h> {
    hir: &'h Hir,
    out: String,
    unsupported: BTreeMap<String, usize>,
    /// Enclosing `while` loops in the current function body.
    loops: usize,
    /// `$UI_ID` expression of the callback being compiled; `None` in functions (refused).
    ui_id: Option<String>,
    /// Compiling an `on note` body: `$EVENT_ID` and `play_note` are the note's own.
    note_ctx: bool,
    loops_seen: bool,
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

    /// A Lua boolean for a KSP condition. Comparisons and logic are emitted as
    /// Lua booleans directly, without the `and 1 or 0` round trip.
    fn cond(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Compare(c, l, r) if !(l.ty == Ty::Str && r.ty == Ty::Str && !matches!(c, Comparison::Equal | Comparison::NotEqual)) => {
                let (a, b) = self.compare_operands(l, r);
                format!("({a}) {} ({b})", compare_sym(*c))
            }
            _ => format!("({}) ~= 0", self.int(e)),
        }
    }

    /// Element `index` of `var` as an inline read when the index is pure: a literal
    /// in range, or a pure expression checked twice. Calls cost an interrupt checkpoint
    /// each, so the common cases avoid `RD`.
    fn elem_read(&mut self, var: VarId, index: &Expr) -> String {
        let (base, off, len) = self.elem_base(var);
        let text = base == "T";
        let zero = if text { "''" } else { "0" };
        if let ExprKind::Int(n) = index.kind {
            return if (0..len as i64).contains(&i64::from(n)) {
                format!("{base}[{}]", off + n as u32 + 1)
            } else {
                zero.into()
            };
        }
        let idx = self.int(index);
        if is_pure(index) && idx.len() <= 160 {
            format!("(({idx} >= 0 and {idx} < {len}) and {base}[{off} + {idx} + 1] or {zero})")
        } else if text {
            format!("RDT({base}, {off}, {len}, {idx})")
        } else {
            format!("RD({base}, {off}, {len}, {idx})")
        }
    }

    /// Store `value` into element `index` of `var` without a call: the index is
    /// evaluated first, then the value, as `WR` does.
    fn elem_write(&mut self, var: VarId, index: &Expr, value: String) {
        let (base, off, len) = self.elem_base(var);
        if let ExprKind::Int(n) = index.kind {
            if (0..len as i64).contains(&i64::from(n)) {
                self.line(&format!("{base}[{}] = {value}", off + n as u32 + 1));
            } else {
                self.line(&format!("local _ = {value}"));
            }
            return;
        }
        let idx = self.int(index);
        self.line(&format!(
            "do local i, v = {idx}, {value} if i >= 0 and i < {len} then {base}[{off} + i + 1] = v end end"
        ));
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
                self.loops_seen = true;
                self.line(&format!("while {c} do"));
                self.loops += 1;
                self.block(body);
                self.loops -= 1;
                self.line("end");
            }
            StmtKind::Select(value, cases) => self.select(value, cases),
            // The host has no range setter; the interpreter runs such scripts.
            StmtKind::UiParams(..) => {
                self.unsupported("init-time UI declaration range");
            }
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
                let value = self.coerce(value, ty);
                self.elem_write(*var, index, value);
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
                let next = if ty == Ty::Real { format!("{target} {step}") } else { wrap(&format!("{target} {step}")) };
                self.line(&format!("{target} = {next}"));
            }
            Place::Elem(var, index) => {
                let Some(ty) = self.var_ty(*var) else { return };
                let current = self.elem_read(*var, index);
                let next = if ty == Ty::Real { format!("{current} {step}") } else { wrap(&format!("{current} {step}")) };
                self.elem_write(*var, index, next);
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
            ExprKind::LoadElem(var, index) => match self.var_ty(*var) {
                Some(_) => self.elem_read(*var, index),
                None => "0".into(),
            },
            ExprKind::Sys(sys) => match sys {
                SysVar::EventNote => "NOTE".into(),
                SysVar::EventId if self.note_ctx => "EV".into(),
                SysVar::EventVelocity => "VEL".into(),
                SysVar::UiId => match self.ui_id.clone() {
                    Some(id) => id,
                    None => self.unsupported("sys UiId in function"),
                },
                other => self.unsupported(&format!("sys {other:?}")),
            },
            ExprKind::SysElem(array, _) => self.unsupported(&format!("sys array {array:?}")),
            ExprKind::Neg(inner) => match e.ty {
                Ty::Real => format!("(-{})", self.expr(inner)),
                _ => wrap(&format!("-({})", self.int(inner))),
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
            Arith::Add => wrap(&format!("({a}) + ({b})")),
            Arith::Sub => wrap(&format!("({a}) - ({b})")),
            Arith::Mul if is_pure(l) && is_pure(r) && a.len() + b.len() <= 160 => {
                // Exact in doubles while |a*b| <= 2^53; otherwise the split multiply.
                format!("((({a}) * ({b}) >= -9007199254740992 and ({a}) * ({b}) <= 9007199254740992) and {} or MUL({a}, {b}))", wrap(&format!("({a}) * ({b})")))
            }
            Arith::Mul => format!("MUL({a}, {b})"),
            Arith::Div => format!("DIV({a}, {b})"),
            Arith::Mod => format!("MOD({a}, {b})"),
            Arith::BitAnd => wrap(&format!("bit32.band(({a}) % 4294967296, ({b}) % 4294967296)")),
            Arith::BitOr => wrap(&format!("bit32.bor(({a}) % 4294967296, ({b}) % 4294967296)")),
            Arith::BitXor => wrap(&format!("bit32.bxor(({a}) % 4294967296, ({b}) % 4294967296)")),
        }
    }

    fn compare_operands(&mut self, l: &Expr, r: &Expr) -> (String, String) {
        let text_both = l.ty == Ty::Str && r.ty == Ty::Str;
        if text_both || (l.ty == Ty::Real && r.ty == Ty::Real) {
            (self.expr(l), self.expr(r))
        } else {
            (self.int(l), self.int(r))
        }
    }

    fn compare(&mut self, c: Comparison, l: &Expr, r: &Expr) -> String {
        let text_both = l.ty == Ty::Str && r.ty == Ty::Str;
        if text_both && !matches!(c, Comparison::Equal | Comparison::NotEqual) {
            return "0".into();
        }
        let (a, b) = self.compare_operands(l, r);
        format!("((({a}) {} ({b})) and 1 or 0)", compare_sym(c))
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
                // Aliases declared at the top of the chunk (`SEP`, `GEP`): no table lookup per call.
                let name = if builtin == Builtin::SetEnginePar { "SEP" } else { "GEP" };
                format!("{name}({})", parts.join(", "))
            }
            // Note callbacks only; the offset form is refused (not yet in the engine sink).
            Builtin::PlayNote => match (ex(args, 0), ex(args, 1), ex(args, 2), ex(args, 3)) {
                (Some(key), Some(vel), offset, Some(micros))
                    if self.note_ctx && offset.is_none_or(|o| matches!(o.kind, ExprKind::Int(0))) =>
                {
                    let (key, vel, micros) = (self.int(key), self.int(vel), self.int(micros));
                    format!("HOST.play(EV, {key}, {vel}, {micros})")
                }
                _ => self.unsupported("play_note outside a note callback or with an offset"),
            },
            Builtin::NoteOff => match (ex(args, 0), ex(args, 1)) {
                (Some(event), delay) => {
                    let event = self.int(event);
                    let delay = delay.map_or_else(|| "nil".to_string(), |d| self.int(d));
                    format!("HOST.note_off({event}, {delay})")
                }
                _ => self.unsupported("note_off without event"),
            },
            // Saved values are not part of this host yet: a fresh instrument has none, so
            // `read_persistent_var` restores nothing (interpreter: `restore`, empty `persisted`).
            Builtin::MakePersistent | Builtin::MakeInstrPersistent | Builtin::ReadPersistentVar => "0".into(),
            Builtin::Abs => match ex(args, 0) {
                Some(e) if e.ty == Ty::Real => format!("math.abs({})", self.expr(e)),
                Some(e) => format!("W(math.abs({}))", self.int(e)),
                None => self.unsupported("builtin abs"),
            },
            Builtin::Min | Builtin::Max => {
                let f = if builtin == Builtin::Min { "math.min" } else { "math.max" };
                match (ex(args, 0), ex(args, 1)) {
                    (Some(a), Some(b)) if a.ty == Ty::Real && b.ty == Ty::Real => {
                        format!("{f}({}, {})", self.expr(a), self.expr(b))
                    }
                    (Some(a), Some(b)) => format!("{f}({}, {})", self.int(a), self.int(b)),
                    _ => self.unsupported("builtin min/max"),
                }
            }
            Builtin::InRange => match (ex(args, 0), ex(args, 1), ex(args, 2)) {
                (Some(x), Some(lo), Some(hi)) if x.ty == Ty::Real => {
                    format!("INRANGE({}, {}, {})", self.expr(x), self.expr(lo), self.expr(hi))
                }
                (Some(x), Some(lo), Some(hi)) => {
                    format!("INRANGE({}, {}, {})", self.int(x), self.int(lo), self.int(hi))
                }
                _ => self.unsupported("builtin in_range"),
            },
            Builtin::Sgn => match ex(args, 0) {
                Some(e) if e.ty == Ty::Real => format!("SGN({})", self.expr(e)),
                Some(e) => format!("SGN({})", self.int(e)),
                None => self.unsupported("builtin sgn"),
            },
            Builtin::ShLeft | Builtin::ShRight => match (ex(args, 0), ex(args, 1)) {
                (Some(a), Some(n)) => {
                    let f = if builtin == Builtin::ShLeft { "SHL" } else { "SHR" };
                    format!("{f}({}, {})", self.int(a), self.int(n))
                }
                _ => self.unsupported("builtin sh_left/sh_right"),
            },
            Builtin::Msb => match ex(args, 0) {
                Some(e) => format!("MSB({})", self.int(e)),
                None => self.unsupported("builtin msb"),
            },
            Builtin::Lsb => match ex(args, 0) {
                Some(e) => format!("(({}) % 128)", self.int(e)),
                None => self.unsupported("builtin lsb"),
            },
            Builtin::MsToTicks | Builtin::TicksToMs => match ex(args, 0) {
                Some(e) => {
                    let f = if builtin == Builtin::MsToTicks { "MS_TO_TICKS" } else { "TICKS_TO_MS" };
                    format!("{f}({})", self.int(e))
                }
                None => self.unsupported("builtin ms_to_ticks"),
            },
            Builtin::IntToReal | Builtin::Real => match ex(args, 0) {
                Some(e) => format!("({})", self.int(e)),
                None => self.unsupported("builtin int_to_real"),
            },
            Builtin::RealToInt | Builtin::Int => match ex(args, 0) {
                Some(e) if e.ty == Ty::Real => format!("RI({})", self.expr(e)),
                Some(e) => self.int(e),
                None => self.unsupported("builtin real_to_int"),
            },
            Builtin::Round => match ex(args, 0) {
                Some(e) => format!("ROUND({})", self.expr(e)),
                None => self.unsupported("builtin round"),
            },
            Builtin::Floor | Builtin::Ceil | Builtin::Sqrt | Builtin::Exp | Builtin::Log | Builtin::Log10
            | Builtin::Sin | Builtin::Cos | Builtin::Tan | Builtin::Asin | Builtin::Acos | Builtin::Atan => {
                let f = match builtin {
                    Builtin::Floor => "math.floor",
                    Builtin::Ceil => "math.ceil",
                    Builtin::Sqrt => "math.sqrt",
                    Builtin::Exp => "math.exp",
                    Builtin::Log => "math.log",
                    Builtin::Log10 => "math.log10",
                    Builtin::Sin => "math.sin",
                    Builtin::Cos => "math.cos",
                    Builtin::Tan => "math.tan",
                    Builtin::Asin => "math.asin",
                    Builtin::Acos => "math.acos",
                    _ => "math.atan",
                };
                match ex(args, 0) {
                    Some(e) => format!("{f}({})", self.expr(e)),
                    None => self.unsupported("builtin math"),
                }
            }
            Builtin::Pow => match (ex(args, 0), ex(args, 1)) {
                (Some(x), Some(e)) => format!("(({}) ^ ({}))", self.expr(x), self.expr(e)),
                _ => self.unsupported("builtin pow"),
            },
            Builtin::NumElements => match array_var(args, 0) {
                Some(var) => format!("{}", self.hir.vars[var.0 as usize].len.unwrap_or(1)),
                None => self.unsupported("builtin num_elements"),
            },
            Builtin::Search => self.search(args),
            Builtin::Sort => self.sort(args),
            other => match self.control_builtin(other, args).or_else(|| self.pgs_builtin(other, args)).or_else(|| self.record_builtin(other, args)).or_else(|| self.request_builtin(other, args)).or_else(|| self.key_builtin(other, args)).or_else(|| self.event_builtin(other, args)) {
                Some(call) => call,
                None => self.unsupported(&format!("builtin {}", other.name())),
            },
        }
    }

    /// `search(array, needle [, lo, hi])`: index of the first element equal to `needle`, or -1.
    fn search(&mut self, args: &[Arg]) -> String {
        let (Some(var), Some(needle)) = (array_var(args, 0), ex(args, 1)) else {
            return self.unsupported("builtin search");
        };
        let (base, off, len) = self.elem_base(var);
        let ty = self.hir.vars[var.0 as usize].ty;
        let x = match (ty, needle.ty) {
            (Ty::Str, Ty::Str) => self.expr(needle),
            (Ty::Real, Ty::Real | Ty::Int | Ty::Bool) => self.expr(needle),
            (Ty::Int | Ty::Bool, _) => self.int(needle),
            _ => return self.unsupported("search needle type"),
        };
        let (lo, hi) = match (ex(args, 2), ex(args, 3)) {
            (Some(lo), Some(hi)) => (self.int(lo), self.int(hi)),
            _ => ("0".into(), format!("{}", len as i64 - 1)),
        };
        format!("SEARCH({base}, {off}, {len}, {x}, {lo}, {hi})")
    }

    /// `sort(array, descending [, lo, hi])` over an integer array. Real and text arrays
    /// stay outside the subset: Kontakt orders reals by `total_cmp` and texts by bytes.
    fn sort(&mut self, args: &[Arg]) -> String {
        let Some(var) = array_var(args, 0) else {
            return self.unsupported("builtin sort");
        };
        let ty = self.hir.vars[var.0 as usize].ty;
        if !matches!(ty, Ty::Int | Ty::Bool) {
            return self.unsupported("sort of real or text array");
        }
        let (base, off, len) = self.elem_base(var);
        let desc = match ex(args, 1) {
            Some(e) => self.int(e),
            None => return self.unsupported("builtin sort"),
        };
        let (lo, hi) = match (ex(args, 2), ex(args, 3)) {
            (Some(lo), Some(hi)) => (self.int(lo), self.int(hi)),
            _ => ("0".into(), format!("{}", len as i64 - 1)),
        };
        format!("SORT({base}, {off}, {len}, {lo}, {hi}, {desc})")
    }
}

/// The expression of argument `i`, when it is a plain expression.
fn ex(args: &[Arg], i: usize) -> Option<&Expr> {
    match args.get(i) {
        Some(Arg::Expr(e)) => Some(e),
        _ => None,
    }
}

/// The array variable named by argument `i`.
fn array_var(args: &[Arg], i: usize) -> Option<VarId> {
    match args.get(i) {
        Some(Arg::Var(v, _)) => Some(*v),
        Some(Arg::Place(p)) => Some(p.var()),
        _ => None,
    }
}
