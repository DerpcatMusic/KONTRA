//! Name resolution, declaration layout and type checking: `ast` -> `hir`.
use crate::ast::{self, BinOp, ExprKind as A, Storage, UnOp};
use crate::builtins::{self, Arg as K, Builtin, Ret, SysArray};
use crate::diag::{Result, Span, fault};
use crate::hir::*;
use crate::lexer::{Interner, Sym};
use crate::model::PerformanceControl;
use sampler_core::Comparison;
use std::collections::HashMap;

/// Declaration budgets; see `crate::Limits`.
#[derive(Clone, Copy)]
pub struct Budget {
    pub variables: usize,
    pub array_cells: usize,
}

pub const MAX_ARRAY: u32 = 1_000_000;

struct Sema<'a, 's> {
    syms: &'s Interner<'a>,
    hir: Hir,
    names: HashMap<Sym, VarId>,
    folded: HashMap<String, VarId>,
    /// Lowercased: KSP function names are case-insensitive too.
    functions: HashMap<String, FnId>,
    symbols: HashMap<Sym, i32>,
    budget: Budget,
    array_cells: usize,
    in_init: bool,
    loops: usize,
    /// Controls from the performance view file, once `load_performance_view` ran.
    performance_view: &'s [PerformanceControl],
    performance_loaded: bool,
}

pub fn analyze<'a, 's>(
    ast: ast::Ast,
    syms: &'s Interner<'a>,
    budget: Budget,
    performance_view: &'s [PerformanceControl],
) -> Result<Hir> {
    let mut s = Sema {
        syms,
        hir: Hir::default(),
        names: HashMap::new(),
        folded: HashMap::new(),
        functions: HashMap::new(),
        symbols: HashMap::new(),
        budget,
        array_cells: 0,
        in_init: false,
        loops: 0,
        performance_view,
        performance_loaded: false,
    };
    let mut bodies = Vec::new();
    let mut callbacks = Vec::new();
    for item in ast.items {
        match item {
            ast::Item::Function(f) => {
                let key = syms.name(f.name).to_ascii_lowercase();
                if s.functions.contains_key(&key) {
                    return fault(f.span, "duplicate function");
                }
                let id = FnId(s.hir.functions.len() as u32);
                s.functions.insert(key, id);
                s.hir.functions.push(Function {
                    name: syms.name(f.name).into(),
                    span: f.span,
                    body: Vec::new(),
                });
                bodies.push(f.body);
            }
            ast::Item::Callback(c) => callbacks.push(c),
        }
    }
    // Declarations live in `on init`; resolve it first so functions and
    // callbacks see every variable regardless of source order.
    let init = callbacks.iter().position(|c| syms.name(c.name) == "init");
    if let Some(index) = init {
        if callbacks[..index]
            .iter()
            .any(|c| syms.name(c.name) == "init")
            || callbacks[index + 1..]
                .iter()
                .any(|c| syms.name(c.name) == "init")
        {
            return fault(callbacks[index].span, "duplicate callback");
        }
        let c = callbacks.remove(index);
        s.in_init = true;
        let body = s.block(&c.body)?;
        s.in_init = false;
        s.hir.callbacks.push(Callback {
            kind: CallbackKind::Init,
            span: c.span,
            body,
        });
    }
    for (index, body) in bodies.into_iter().enumerate() {
        let body = s.block(&body)?;
        s.hir.functions[index].body = body;
    }
    let mut seen = HashMap::new();
    for c in callbacks {
        let kind = match (syms.name(c.name), c.arg) {
            ("note", None) => CallbackKind::Note,
            ("release", None) => CallbackKind::Release,
            ("controller", None) => CallbackKind::Controller,
            ("poly_at", None) => CallbackKind::PolyAt,
            ("ui_controls", None) => CallbackKind::UiControls,
            ("ui_update", None) => CallbackKind::UiUpdate,
            ("listener", None) => CallbackKind::Listener,
            ("pgs_changed", None) => CallbackKind::PgsChanged,
            ("persistence_changed", None) => CallbackKind::PersistenceChanged,
            ("async_complete", None) => CallbackKind::AsyncComplete,
            ("rpn", None) => CallbackKind::Rpn,
            ("nrpn", None) => CallbackKind::Nrpn,
            ("ui_control", Some((name, span))) => {
                let var = s.lookup(name, span)?;
                if s.hir.vars[var.0 as usize].ui.is_none() {
                    return fault(span, "ui_control callback requires a UI control variable");
                }
                CallbackKind::UiControl(var)
            }
            _ => return fault(c.span, "unsupported callback"),
        };
        let body = s.block(&c.body)?;
        if let Some(&index) = seen.get(&kind) {
            // A later ui_control callback replaces the earlier one, as in
            // Kontakt; generated frameworks override generic handlers this way.
            if !matches!(kind, CallbackKind::UiControl(_)) {
                return fault(c.span, "duplicate callback");
            }
            s.hir.warnings.push(crate::diag::Fault {
                span: c.span,
                builtin: None,
                message: "later ui_control callback replaces an earlier one".into(),
            });
            let callback: &mut Callback = &mut s.hir.callbacks[index];
            callback.body = body;
            callback.span = c.span;
            continue;
        }
        seen.insert(kind, s.hir.callbacks.len());
        if let CallbackKind::UiControl(var) = kind {
            let ui = s.hir.vars[var.0 as usize].ui.unwrap();
            s.hir.uis[ui as usize].callback = Some(s.hir.callbacks.len());
        }
        s.hir.callbacks.push(Callback {
            kind,
            span: c.span,
            body,
        });
    }
    if s.hir.callbacks.is_empty() {
        return fault(Span::default(), "script declares no callbacks");
    }
    s.hir.call_depth = call_depth(&s.hir)?;
    s.hir.symbols.shrink_to_fit();
    Ok(s.hir)
}

/// Longest call chain; KSP forbids recursion, so the graph must be acyclic.
fn call_depth(hir: &Hir) -> Result<usize> {
    fn calls(body: &[Stmt], out: &mut Vec<usize>) {
        for s in body {
            match &s.kind {
                StmtKind::Call(f) => out.push(f.0 as usize),
                StmtKind::If(_, a, b) => {
                    calls(a, out);
                    calls(b, out);
                }
                StmtKind::While(_, a) => calls(a, out),
                StmtKind::Select(_, cases) => cases.iter().for_each(|c| calls(&c.body, out)),
                _ => {}
            }
        }
    }
    let edges: Vec<Vec<usize>> = hir
        .functions
        .iter()
        .map(|f| {
            let mut out = Vec::new();
            calls(&f.body, &mut out);
            out.sort_unstable();
            out.dedup();
            out
        })
        .collect();
    // 0 = unvisited, 1 = on stack, 2 = done; explicit stack, no Rust recursion.
    let mut state = vec![0u8; edges.len()];
    let mut depth = vec![0usize; edges.len()];
    for root in 0..edges.len() {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            if let Some(&callee) = edges[node].get(*next) {
                *next += 1;
                match state[callee] {
                    0 => {
                        state[callee] = 1;
                        stack.push((callee, 0));
                    }
                    1 => {
                        return fault(
                            hir.functions[callee].span,
                            format!("recursive call to function {}", hir.functions[callee].name),
                        );
                    }
                    _ => {}
                }
            } else {
                depth[node] = 1 + edges[node].iter().map(|&c| depth[c]).max().unwrap_or(0);
                state[node] = 2;
                stack.pop();
            }
        }
    }
    Ok(depth.into_iter().max().unwrap_or(0))
}

fn ty_name(ty: Ty) -> &'static str {
    match ty {
        Ty::Int => "integer",
        Ty::Real => "real",
        Ty::Str => "string",
        Ty::Bool => "condition",
    }
}

fn prefix_type(name: &str) -> (Ty, bool) {
    match name.as_bytes()[0] {
        b'$' => (Ty::Int, false),
        b'~' => (Ty::Real, false),
        b'@' => (Ty::Str, false),
        b'%' => (Ty::Int, true),
        b'?' => (Ty::Real, true),
        _ => (Ty::Str, true),
    }
}

impl<'a> Sema<'a, '_> {
    fn name(&self, sym: Sym) -> &'a str {
        self.syms.name(sym)
    }

    fn block(&mut self, body: &[ast::Stmt]) -> Result<Vec<Stmt>> {
        let mut out = Vec::with_capacity(body.len());
        for s in body {
            if let Some(kind) = self.stmt(s)? {
                out.push(Stmt { span: s.span, kind });
            }
        }
        Ok(out)
    }

    fn stmt(&mut self, s: &ast::Stmt) -> Result<Option<StmtKind>> {
        Ok(Some(match &s.kind {
            ast::StmtKind::Declare(d) => {
                if !self.in_init {
                    return fault(s.span, "declarations are only allowed in on init");
                }
                return self.declare(d);
            }
            ast::StmtKind::Assign(target, value) => {
                if let A::Var(sym, Some(index)) = &target.kind
                    && self.resolve(*sym).is_none()
                    && SysArray::from_name(self.name(*sym)) == Some(SysArray::EventPar)
                {
                    let index = self.expr(index)?;
                    let value = self.expr(value)?;
                    return Ok(Some(StmtKind::Builtin(
                        Builtin::SetEventParArr,
                        vec![
                            Arg::Expr(Expr {
                                ty: Ty::Int,
                                span: target.span,
                                kind: ExprKind::Sys(builtins::SysVar::EventId),
                            }),
                            Arg::Expr(Expr {
                                ty: Ty::Int,
                                span: target.span,
                                kind: ExprKind::Int(builtins::event_par::CUSTOM),
                            }),
                            Arg::Expr(self.coerce(value, Ty::Int)?),
                            Arg::Expr(self.coerce(index, Ty::Int)?),
                        ],
                    )));
                }
                if let A::Var(sym, Some(index)) = &target.kind
                    && self.resolve(*sym).is_none()
                    && SysArray::from_name(self.name(*sym)) == Some(SysArray::Cc)
                {
                    // Kontakt accepts writes to %CC; the nearest native meaning
                    // is publishing that controller value.
                    self.hir.warnings.push(crate::diag::Fault {
                        span: target.span,
                        builtin: None,
                        message: "write to %CC lowered as set_controller".into(),
                    });
                    let index = self.expr(index)?;
                    let value = self.expr(value)?;
                    return Ok(Some(StmtKind::Builtin(
                        Builtin::SetController,
                        vec![
                            Arg::Expr(self.coerce(index, Ty::Int)?),
                            Arg::Expr(self.coerce(value, Ty::Int)?),
                        ],
                    )));
                }
                let place = self.place(target)?;
                let ty = self.hir.vars[place.var().0 as usize].ty;
                self.check_writable(place.var(), target.span)?;
                let value = self.expr(value)?;
                StmtKind::Assign(place, self.coerce(value, ty)?)
            }
            ast::StmtKind::If(cond, yes, no) => {
                let cond = self.condition(cond)?;
                StmtKind::If(cond, self.block(yes)?, self.block(no)?)
            }
            ast::StmtKind::While(cond, body) => {
                let cond = self.condition(cond)?;
                self.loops += 1;
                let body = self.block(body);
                self.loops -= 1;
                StmtKind::While(cond, body?)
            }
            ast::StmtKind::Select(value, cases) => {
                let value = self.expr(value)?;
                let value = self.coerce(value, Ty::Int)?;
                let mut out = Vec::with_capacity(cases.len());
                for case in cases {
                    let low = self.const_int(&case.low)?;
                    let high = match &case.high {
                        Some(h) => self.const_int(h)?,
                        None => low,
                    };
                    out.push(Case {
                        low: low.min(high),
                        high: low.max(high),
                        body: self.block(&case.body)?,
                    });
                }
                StmtKind::Select(value, out)
            }
            ast::StmtKind::Call(name) => match self.function(*name) {
                Some(f) => StmtKind::Call(f),
                None => return fault(s.span, format!("unknown function {}", self.name(*name))),
            },
            ast::StmtKind::Command(name, args) => {
                if let Some(f) = self.function(*name)
                    && args.is_empty()
                {
                    return Ok(Some(StmtKind::Call(f)));
                }
                let Some(b) = Builtin::from_name(self.name(*name)) else {
                    return fault(s.span, format!("unknown command {}", self.name(*name)));
                };
                if b == Builtin::Continue && self.loops == 0 {
                    return fault(s.span, "continue outside while");
                }
                let (args, _) = self.builtin(b, args, s.span)?;
                if self.in_init {
                    self.declare_effect(b, &args);
                }
                if b == Builtin::LoadPerformanceView {
                    self.load_performance_view(s.span)?;
                }
                StmtKind::Builtin(b, args)
            }
        }))
    }

    /// Persistence flags are declarations in disguise; record them statically.
    fn declare_effect(&mut self, b: Builtin, args: &[Arg]) {
        let persistence = match b {
            Builtin::MakePersistent => Persistence::Snapshot,
            Builtin::MakeInstrPersistent => Persistence::Instrument,
            _ => return,
        };
        if let [Arg::Var(v, _)] = args {
            self.hir.vars[v.0 as usize].persistence = persistence;
        }
    }

    fn check_writable(&self, var: VarId, span: Span) -> Result<()> {
        if matches!(self.hir.vars[var.0 as usize].home, Home::Const(_)) {
            return fault(span, "cannot assign to a constant");
        }
        Ok(())
    }

    fn new_var(&mut self, name: Sym, span: Span, var: Var) -> Result<VarId> {
        let id = self.push_var(span, var)?;
        self.names.insert(name, id);
        Ok(id)
    }

    fn add_cells(&self, n: usize, span: Span) -> Result<usize> {
        let total = self.array_cells.saturating_add(n);
        if total > self.budget.array_cells {
            return fault(
                span,
                format!(
                    "array cell budget exceeded: {total} cells declared so far, limit {}",
                    self.budget.array_cells
                ),
            );
        }
        Ok(total)
    }

    fn push_var(&mut self, span: Span, var: Var) -> Result<VarId> {
        let folded = var.name.to_ascii_lowercase();
        if self.folded.contains_key(&folded) {
            return fault(span, "duplicate variable declaration");
        }
        if self.hir.vars.len() >= self.budget.variables {
            return fault(
                span,
                format!(
                    "variable budget exceeded: more than {} declarations",
                    self.budget.variables
                ),
            );
        }
        let id = VarId(self.hir.vars.len() as u32);
        self.hir.vars.push(var);
        self.folded.insert(folded, id);
        Ok(id)
    }

    /// `load_performance_view` declares the file's controls as UI variables.
    fn load_performance_view(&mut self, span: Span) -> Result<()> {
        self.performance_loaded = true;
        for control in self.performance_view {
            if !self.folded.contains_key(&control.name.to_ascii_lowercase()) {
                self.performance_widget(control, span, false)?;
            }
        }
        Ok(())
    }

    fn performance_widget(
        &mut self,
        c: &PerformanceControl,
        span: Span,
        unresolved: bool,
    ) -> Result<VarId> {
        let (ty, array) = prefix_type(&c.name);
        let expected = match c.kind {
            WidgetKind::Table => (Ty::Int, true),
            WidgetKind::Xy => (Ty::Real, true),
            WidgetKind::TextEdit => (Ty::Str, false),
            _ => (Ty::Int, false),
        };
        let len = array.then_some(c.len);
        if (ty, array) != expected || len.is_some_and(|n| !(1..=MAX_ARRAY).contains(&n)) {
            return fault(
                span,
                format!(
                    "performance view control {} has an unsupported type or size",
                    c.name
                ),
            );
        }
        let home = match len {
            _ if !unresolved && c.kind.has_control() => Home::Control(self.hir.uis.len() as u32),
            Some(len) => {
                self.array_cells = self.add_cells(len as usize, span)?;
                Home::Cells {
                    offset: self.cells(len, false, span)?,
                    len,
                }
            }
            None if ty == Ty::Str => Home::Text(self.cells(1, true, span)?),
            None => Home::Cell(self.cells(1, false, span)?),
        };
        let var = self.push_var(
            span,
            Var {
                name: c.name.as_str().into(),
                ty,
                len,
                home,
                ui: Some(self.hir.uis.len() as u32),
                persistence: Persistence::None,
                span,
            },
        )?;
        self.hir.uis.push(Ui {
            unresolved,
            kind: c.kind,
            var,
            params: c.params.clone(),
            callback: None,
        });
        Ok(var)
    }

    /// After `load_performance_view`, unknown `$`, `%` and `@` names are performance
    /// unbound handles when the loaded view does not describe them.
    fn resolve_or_declare(&mut self, sym: Sym, span: Span) -> Result<Option<VarId>> {
        if let Some(v) = self.resolve(sym) {
            return Ok(Some(v));
        }
        let name = self.name(sym);
        let kind = match name.as_bytes().first() {
            Some(b'$') => WidgetKind::Knob,
            Some(b'%') => WidgetKind::Table,
            Some(b'@') => WidgetKind::TextEdit,
            _ => return Ok(None),
        };
        if !self.performance_loaded
            || builtins::sys_var(name).is_some()
            || SysArray::from_name(name).is_some()
            || builtins::constant(name).is_some()
            || builtins::control_par(name).is_some()
            || builtins::real_constant(name).is_some()
            || vendor_name(name)
        {
            return Ok(None);
        }
        let control = PerformanceControl::assumed(name, kind);
        self.hir.warnings.push(crate::diag::Fault {
            span,
            builtin: None,
            message: format!(
                "{name} is not in the performance view description; retained as an unbound handle"
            ),
        });
        self.performance_widget(&control, span, true).map(Some)
    }

    fn cells(&mut self, len: u32, text: bool, span: Span) -> Result<u32> {
        let counter = if text {
            &mut self.hir.texts
        } else {
            &mut self.hir.cells
        };
        let offset = *counter;
        *counter =
            counter
                .checked_add(len)
                .filter(|n| *n < u32::MAX)
                .ok_or(crate::diag::Fault {
                    span,
                    builtin: None,
                    message: "script cell range exceeded".into(),
                })?;
        Ok(offset)
    }

    fn declare(&mut self, d: &ast::Declare) -> Result<Option<StmtKind>> {
        let text = self.name(d.name);
        let span = d.name_span;
        let valid = text.len() > 1
            && (text.as_bytes()[1].is_ascii_alphanumeric() || text.as_bytes()[1] == b'_');
        if !valid
            || builtins::sys_var(text).is_some()
            || SysArray::from_name(text).is_some()
            || builtins::constant(text).is_some()
            || builtins::control_par(text).is_some()
            || vendor_name(text)
        {
            return fault(span, "invalid or reserved variable name");
        }
        let (ty, array) = prefix_type(text);
        let name: Box<str> = text.into();
        let persistence = match d.storage {
            Storage::Persistent => Persistence::Snapshot,
            Storage::InstrumentPersistent => Persistence::Instrument,
            _ => Persistence::None,
        };
        let len = match &d.size {
            Some(size) if array => {
                let n = self.const_int(size)?;
                if !(1..=MAX_ARRAY as i32).contains(&n) {
                    return fault(size.span, "array size must be between 1 and 1000000");
                }
                self.array_cells = self.add_cells(n as usize, size.span)?;
                Some(n as u32)
            }
            Some(size) => return fault(size.span, "only array variables take a size"),
            None if array => return fault(span, "array declaration requires a size"),
            None => None,
        };
        if d.storage == Storage::Const {
            if array || d.ui.is_some() || d.init.len() != 1 {
                return fault(span, "constant requires one scalar initializer");
            }
            let value = self.expr(&d.init[0])?;
            let value = self.coerce(value, ty)?;
            let Some(value) = fold(&self.hir, &value) else {
                return fault(d.init[0].span, "constant expression required");
            };
            let index = self.hir.consts.len() as u32;
            self.hir.consts.push(value);
            self.new_var(
                d.name,
                span,
                Var {
                    name,
                    ty,
                    len: None,
                    home: Home::Const(index),
                    ui: None,
                    persistence,
                    span,
                },
            )?;
            return Ok(None);
        }
        let ui = match d.ui {
            Some(word) => {
                let word = self.name(word);
                let Some(kind) = WidgetKind::from_keyword(word) else {
                    return fault(span, format!("unknown declaration keyword {word}"));
                };
                let expected = match kind {
                    WidgetKind::Table => (Ty::Int, true),
                    WidgetKind::Xy => (Ty::Real, true),
                    WidgetKind::TextEdit => (Ty::Str, false),
                    _ => (Ty::Int, false),
                };
                if (ty, array) != expected {
                    return fault(span, format!("{word} requires a different variable type"));
                }
                let mut params = Vec::with_capacity(d.params.len());
                for p in &d.params {
                    params.push(self.const_int(p)?);
                }
                Some((kind, params))
            }
            None if !d.params.is_empty() => {
                return fault(d.params[0].span, "only UI declarations take parameters");
            }
            None => None,
        };
        let home = match (&ui, d.storage, len) {
            (Some((kind, _)), _, None) if kind.has_control() => {
                Home::Control(self.hir.uis.len() as u32)
            }
            (_, Storage::Polyphonic, None) => {
                if ty == Ty::Str {
                    return fault(span, "polyphonic strings are unsupported");
                }
                let cell = self.hir.note_cells;
                self.hir.note_cells = cell.checked_add(1).ok_or(crate::diag::Fault {
                    span,
                    builtin: None,
                    message: "note cell range exceeded".into(),
                })?;
                Home::Note(cell)
            }
            (_, Storage::Polyphonic, Some(_)) => {
                return fault(span, "polyphonic arrays are unsupported");
            }
            (_, _, None) if ty == Ty::Str => Home::Text(self.cells(1, true, span)?),
            (_, _, None) => Home::Cell(self.cells(1, false, span)?),
            (_, _, Some(len)) if ty == Ty::Str => Home::Texts {
                offset: self.cells(len, true, span)?,
                len,
            },
            (_, _, Some(len)) => Home::Cells {
                offset: self.cells(len, false, span)?,
                len,
            },
        };
        let ui_index = ui.as_ref().map(|_| self.hir.uis.len() as u32);
        let var = self.new_var(
            d.name,
            span,
            Var {
                name,
                ty,
                len,
                home,
                ui: ui_index,
                persistence,
                span,
            },
        )?;
        if let Some((kind, params)) = ui {
            self.hir.uis.push(Ui {
                unresolved: false,
                kind,
                var,
                params,
                callback: None,
            });
        }
        if d.init.is_empty() {
            return Ok(None);
        }
        if d.storage == Storage::Polyphonic {
            self.hir.warnings.push(crate::diag::Fault {
                span,
                builtin: None,
                message: "polyphonic variables start at 0; initializer ignored".into(),
            });
            return Ok(None);
        }
        let mut values = Vec::with_capacity(d.init.len());
        for e in &d.init {
            let value = self.expr(e)?;
            values.push(self.coerce(value, ty)?);
        }
        Ok(Some(if let Some(len) = len {
            if values.len() > len as usize {
                return fault(d.init[0].span, "too many array initializers");
            }
            StmtKind::Fill(var, values)
        } else {
            if d.init_list {
                return fault(d.init[0].span, "scalar takes one initializer");
            }
            StmtKind::Assign(Place::Var(var), values.pop().unwrap())
        }))
    }

    fn function(&self, sym: Sym) -> Option<FnId> {
        self.functions
            .get(&self.name(sym).to_ascii_lowercase())
            .copied()
    }

    /// KSP variable names are case-insensitive.
    fn resolve(&self, sym: Sym) -> Option<VarId> {
        if let Some(&v) = self.names.get(&sym) {
            return Some(v);
        }
        self.folded
            .get(&self.name(sym).to_ascii_lowercase())
            .copied()
    }

    fn lookup(&mut self, sym: Sym, span: Span) -> Result<VarId> {
        self.resolve_or_declare(sym, span)?
            .ok_or_else(|| crate::diag::Fault {
                span,
                builtin: None,
                message: format!("undeclared variable {}", self.name(sym)),
            })
    }

    fn place(&mut self, target: &ast::Expr) -> Result<Place> {
        let A::Var(sym, index) = &target.kind else {
            return fault(target.span, "expected variable");
        };
        let Some(var) = self.resolve_or_declare(*sym, target.span)? else {
            if builtins::sys_var(self.name(*sym)).is_some()
                || SysArray::from_name(self.name(*sym)).is_some()
            {
                return fault(target.span, "built-in variables are read-only");
            }
            return fault(
                target.span,
                format!("undeclared variable {}", self.name(*sym)),
            );
        };
        let is_array = self.hir.vars[var.0 as usize].len.is_some();
        match (index, is_array) {
            (None, false) => Ok(Place::Var(var)),
            (Some(i), true) => {
                let i = self.expr(i)?;
                Ok(Place::Elem(var, Box::new(self.coerce(i, Ty::Int)?)))
            }
            (None, true) => fault(target.span, "array requires an index"),
            (Some(_), false) => fault(target.span, "only arrays can be indexed"),
        }
    }

    fn const_int(&mut self, e: &ast::Expr) -> Result<i32> {
        let value = self.expr(e)?;
        let value = self.coerce(value, Ty::Int)?;
        match fold(&self.hir, &value) {
            Some(Const::Int(n)) => Ok(n),
            _ => fault(e.span, "constant integer expression required"),
        }
    }

    fn condition(&mut self, e: &ast::Expr) -> Result<Expr> {
        let e = self.expr(e)?;
        self.coerce(e, Ty::Bool)
    }

    /// Implicit conversions KSP performs: condition <-> integer, anything -> text.
    fn coerce(&self, e: Expr, want: Ty) -> Result<Expr> {
        if e.ty == want {
            return Ok(e);
        }
        let span = e.span;
        match (e.ty, want) {
            (Ty::Bool, Ty::Int) | (Ty::Int, Ty::Bool) => Ok(Expr {
                ty: want,
                span,
                kind: ExprKind::Cast(Box::new(e)),
            }),
            (_, Ty::Str) => Ok(Expr {
                ty: Ty::Str,
                span,
                kind: ExprKind::Concat(vec![e]),
            }),
            (have, want) => fault(
                span,
                format!("expected {}, found {}", ty_name(want), ty_name(have)),
            ),
        }
    }

    fn symbol(&mut self, sym: Sym) -> i32 {
        if let Some(&v) = self.symbols.get(&sym) {
            return v;
        }
        let v = OPAQUE_BASE + self.hir.symbols.len() as i32;
        self.hir.symbols.push(self.name(sym).into());
        self.symbols.insert(sym, v);
        v
    }

    fn expr(&mut self, e: &ast::Expr) -> Result<Expr> {
        let span = e.span;
        let (ty, kind) = match &e.kind {
            A::Int(n, hex) => {
                let value = if *hex {
                    *n as u32 as i32
                } else {
                    // Kontakt wraps decimal literals up to 2^32 - 1 into 32 bits.
                    match u32::try_from(*n) {
                        Ok(v) => v as i32,
                        Err(_) => return fault(span, "literal exceeds 32-bit range"),
                    }
                };
                (Ty::Int, ExprKind::Int(value))
            }
            A::Real(r) => (Ty::Real, ExprKind::Real(*r)),
            A::Str(s) => (Ty::Str, ExprKind::Str(self.name(*s).into())),
            A::Ident(sym) => {
                return fault(span, format!("unexpected identifier {}", self.name(*sym)));
            }
            A::Var(sym, index) => return self.var(*sym, index.as_deref(), span),
            A::Call(name, args) => {
                let Some(b) = Builtin::from_name(self.name(*name)) else {
                    if self.function(*name).is_some() {
                        return fault(span, "functions do not return values");
                    }
                    return fault(span, format!("unknown function {}", self.name(*name)));
                };
                if b.sig().ret == Ret::Void {
                    return fault(span, format!("{} returns no value", b.name()));
                }
                let (args, ret) = self.builtin(b, args, span)?;
                (ret, ExprKind::Builtin(b, args))
            }
            A::Unary(UnOp::Neg, inner) => {
                if let A::Int(2_147_483_648, false) = inner.kind {
                    return Ok(Expr {
                        ty: Ty::Int,
                        span,
                        kind: ExprKind::Int(i32::MIN),
                    });
                }
                let inner = self.expr(inner)?;
                let inner = if inner.ty == Ty::Bool {
                    self.coerce(inner, Ty::Int)?
                } else {
                    inner
                };
                match inner.kind {
                    ExprKind::Int(n) => (Ty::Int, ExprKind::Int(n.wrapping_neg())),
                    ExprKind::Real(n) => (Ty::Real, ExprKind::Real(-n)),
                    _ if matches!(inner.ty, Ty::Int | Ty::Real) => {
                        (inner.ty, ExprKind::Neg(Box::new(inner)))
                    }
                    _ => return fault(span, "negation requires a number"),
                }
            }
            A::Unary(UnOp::BitNot, inner) => {
                let inner = self.expr(inner)?;
                (
                    Ty::Int,
                    ExprKind::BitNot(Box::new(self.coerce(inner, Ty::Int)?)),
                )
            }
            A::Unary(UnOp::Not, inner) => {
                let inner = self.expr(inner)?;
                (
                    Ty::Bool,
                    ExprKind::Not(Box::new(self.coerce(inner, Ty::Bool)?)),
                )
            }
            A::Binary(op, l, r) => return self.binary(*op, l, r, span),
        };
        Ok(Expr { ty, span, kind })
    }

    fn binary(&mut self, op: BinOp, l: &ast::Expr, r: &ast::Expr, span: Span) -> Result<Expr> {
        if op == BinOp::Concat {
            let mut parts = Vec::new();
            for side in [l, r] {
                let e = self.expr(side)?;
                match e.kind {
                    ExprKind::Concat(inner) if e.ty == Ty::Str => parts.extend(inner),
                    _ => parts.push(e),
                }
            }
            return Ok(Expr {
                ty: Ty::Str,
                span,
                kind: ExprKind::Concat(parts),
            });
        }
        let l = self.expr(l)?;
        let r = self.expr(r)?;
        let logic = match op {
            BinOp::And => Some(Logic::And),
            BinOp::Or => Some(Logic::Or),
            BinOp::Xor => Some(Logic::Xor),
            _ => None,
        };
        if let Some(logic) = logic {
            let l = self.coerce(l, Ty::Bool)?;
            let r = self.coerce(r, Ty::Bool)?;
            return Ok(Expr {
                ty: Ty::Bool,
                span,
                kind: ExprKind::Logic(logic, Box::new(l), Box::new(r)),
            });
        }
        let comparison = match op {
            BinOp::Eq => Some(Comparison::Equal),
            BinOp::Ne => Some(Comparison::NotEqual),
            BinOp::Lt => Some(Comparison::Less),
            BinOp::Gt => Some(Comparison::Greater),
            BinOp::Le => Some(Comparison::LessEqual),
            BinOp::Ge => Some(Comparison::GreaterEqual),
            _ => None,
        };
        // Conditions compare as integers.
        let unify = |s: &Self, l: Expr, r: Expr| -> Result<(Expr, Expr)> {
            let l = if l.ty == Ty::Bool {
                s.coerce(l, Ty::Int)?
            } else {
                l
            };
            let r = if r.ty == Ty::Bool {
                s.coerce(r, Ty::Int)?
            } else {
                r
            };
            if l.ty == Ty::Str || r.ty == Ty::Str {
                let l = s.coerce(l, Ty::Str)?;
                let r = s.coerce(r, Ty::Str)?;
                return Ok((l, r));
            }
            if l.ty != r.ty {
                return fault(
                    r.span,
                    format!("operands differ: {} and {}", ty_name(l.ty), ty_name(r.ty)),
                );
            }
            Ok((l, r))
        };
        if let Some(comparison) = comparison {
            let (l, r) = unify(self, l, r)?;
            if l.ty == Ty::Str && !matches!(comparison, Comparison::Equal | Comparison::NotEqual) {
                return fault(span, "strings compare only for equality");
            }
            return Ok(Expr {
                ty: Ty::Bool,
                span,
                kind: ExprKind::Compare(comparison, Box::new(l), Box::new(r)),
            });
        }
        let arith = match op {
            BinOp::Add => Arith::Add,
            BinOp::Sub => Arith::Sub,
            BinOp::Mul => Arith::Mul,
            BinOp::Div => Arith::Div,
            BinOp::Mod => Arith::Mod,
            BinOp::BitAnd => Arith::BitAnd,
            BinOp::BitOr => Arith::BitOr,
            BinOp::BitXor => Arith::BitXor,
            _ => unreachable!(),
        };
        let (l, r) = unify(self, l, r)?;
        let integer_only = !matches!(arith, Arith::Add | Arith::Sub | Arith::Mul | Arith::Div);
        if l.ty == Ty::Str || (l.ty == Ty::Real && integer_only) {
            return fault(
                span,
                format!("operator requires integers, found {}", ty_name(l.ty)),
            );
        }
        let ty = l.ty;
        let e = Expr {
            ty,
            span,
            kind: ExprKind::Arith(arith, Box::new(l), Box::new(r)),
        };
        // Fold literal arithmetic so constants stay cheap at runtime.
        Ok(match fold(&self.hir, &e) {
            Some(Const::Int(n)) if is_literal(&e) => Expr {
                ty,
                span,
                kind: ExprKind::Int(n),
            },
            Some(Const::Real(n)) if is_literal(&e) => Expr {
                ty,
                span,
                kind: ExprKind::Real(n),
            },
            _ => e,
        })
    }

    fn var(&mut self, sym: Sym, index: Option<&ast::Expr>, span: Span) -> Result<Expr> {
        let name = self.name(sym);
        if let Some(var) = self.resolve_or_declare(sym, span)? {
            let v = &self.hir.vars[var.0 as usize];
            let ty = v.ty;
            return Ok(match (index, v.len.is_some()) {
                (None, false) => {
                    if let Home::Const(c) = v.home {
                        let kind = match &self.hir.consts[c as usize] {
                            Const::Int(n) => ExprKind::Int(*n),
                            Const::Real(n) => ExprKind::Real(*n),
                            Const::Str(s) => ExprKind::Str(s.clone()),
                        };
                        return Ok(Expr { ty, span, kind });
                    }
                    Expr {
                        ty,
                        span,
                        kind: ExprKind::Load(var),
                    }
                }
                (Some(i), true) => {
                    let i = self.expr(i)?;
                    let i = self.coerce(i, Ty::Int)?;
                    Expr {
                        ty,
                        span,
                        kind: ExprKind::LoadElem(var, Box::new(i)),
                    }
                }
                (None, true) => return fault(span, "array requires an index"),
                (Some(_), false) => return fault(span, "only arrays can be indexed"),
            });
        }
        if let Some(sys) = builtins::sys_var(name) {
            if index.is_some() {
                return fault(span, "only arrays can be indexed");
            }
            return Ok(Expr {
                ty: Ty::Int,
                span,
                kind: ExprKind::Sys(sys),
            });
        }
        if let Some(array) = SysArray::from_name(name) {
            let Some(i) = index else {
                return fault(span, "array requires an index");
            };
            let i = self.expr(i)?;
            let i = self.coerce(i, Ty::Int)?;
            return Ok(Expr {
                ty: if array.drop_kind().is_some() {
                    Ty::Str
                } else {
                    Ty::Int
                },
                span,
                kind: ExprKind::SysElem(array, Box::new(i)),
            });
        }
        if index.is_none() {
            if let Some(r) = builtins::real_constant(name) {
                return Ok(Expr {
                    ty: Ty::Real,
                    span,
                    kind: ExprKind::Real(r),
                });
            }
            let value = builtins::constant(name).or_else(|| builtins::control_par(name));
            let value = match value {
                Some(v) => Some(v),
                // Undeclared uppercase names are vendor constants; only identity matters.
                None if name.starts_with('$')
                    && name.as_bytes().get(1).is_some_and(u8::is_ascii_uppercase) =>
                {
                    if !self.symbols.contains_key(&sym) {
                        self.hir.warnings.push(crate::diag::Fault {
                            span,
                            builtin: None,
                            message: format!(
                                "undeclared {name} treated as an opaque vendor constant"
                            ),
                        });
                    }
                    Some(self.symbol(sym))
                }
                None => None,
            };
            if let Some(v) = value {
                return Ok(Expr {
                    ty: Ty::Int,
                    span,
                    kind: ExprKind::Int(v),
                });
            }
        }
        fault(span, format!("undeclared variable {name}"))
    }

    fn var_arg(&mut self, e: &ast::Expr, array: bool) -> Result<Arg> {
        let A::Var(sym, None) = &e.kind else {
            return fault(e.span, "expected a variable");
        };
        if array
            && self.resolve(*sym).is_none()
            && let Some(sys) = SysArray::from_name(self.name(*sym))
        {
            return Ok(Arg::SysArray(sys));
        }
        let var = self.lookup(*sym, e.span)?;
        if array && self.hir.vars[var.0 as usize].len.is_none() {
            return fault(e.span, "expected an array");
        }
        Ok(Arg::Var(var, e.span))
    }

    fn builtin(&mut self, b: Builtin, args: &[ast::Expr], span: Span) -> Result<(Vec<Arg>, Ty)> {
        let sig = b.sig();
        let max = sig.args.len();
        let min = max - sig.optional as usize;
        if !(min..=max).contains(&args.len()) {
            return fault(
                span,
                format!(
                    "{} expects {min}..={max} arguments, got {}",
                    b.name(),
                    args.len()
                ),
            );
        }
        let mut num = None;
        let mut out = Vec::with_capacity(args.len());
        for (e, kind) in args.iter().zip(sig.args) {
            out.push(match kind {
                K::I | K::R | K::S => {
                    let want = match kind {
                        K::I => Ty::Int,
                        K::R => Ty::Real,
                        _ => Ty::Str,
                    };
                    let value = self.expr(e)?;
                    Arg::Expr(self.coerce(value, want)?)
                }
                K::N => {
                    let value = self.expr(e)?;
                    let value = if value.ty == Ty::Bool {
                        self.coerce(value, Ty::Int)?
                    } else {
                        value
                    };
                    if !matches!(value.ty, Ty::Int | Ty::Real) {
                        return fault(e.span, format!("{} requires numbers", b.name()));
                    }
                    if num.is_some_and(|n| n != value.ty) {
                        return fault(e.span, format!("{} mixes integer and real", b.name()));
                    }
                    num = Some(value.ty);
                    Arg::Expr(value)
                }
                K::V => self.var_arg(e, false)?,
                K::A => self.var_arg(e, true)?,
                K::P => {
                    let place = self.place(e)?;
                    self.check_writable(place.var(), e.span)?;
                    if self.hir.vars[place.var().0 as usize].ty != Ty::Int {
                        return fault(e.span, "expected an integer variable");
                    }
                    Arg::Place(place)
                }
                K::K => match &e.kind {
                    A::Ident(s) | A::Str(s) => Arg::Key(self.name(*s).into()),
                    _ => return fault(e.span, "expected a key name"),
                },
            });
        }
        if b == Builtin::GetUiId {
            let Arg::Var(v, s) = out[0] else {
                unreachable!()
            };
            if self.hir.vars[v.0 as usize].ui.is_none() {
                return fault(s, "get_ui_id requires a UI control");
            }
        }
        if matches!(b, Builtin::Search | Builtin::Sort) && args.len() == 3 {
            return fault(span, format!("{} requires both range endpoints", b.name()));
        }
        if b == Builtin::Search
            && let Arg::Var(v, _) = out[0]
            && self.hir.vars[v.0 as usize].ty == Ty::Real
        {
            return fault(span, "search does not accept real arrays");
        }
        if b == Builtin::Search
            && let Arg::Var(v, _) = out[0]
            && num.is_some_and(|n| n != self.hir.vars[v.0 as usize].ty)
        {
            return fault(span, "search value type differs from the array");
        }
        let ty = match sig.ret {
            Ret::Void => Ty::Int,
            Ret::Int => Ty::Int,
            Ret::Real => Ty::Real,
            Ret::Str => Ty::Str,
            Ret::Bool => Ty::Bool,
            Ret::Num => num.unwrap_or(Ty::Int),
        };
        Ok((out, ty))
    }
}

fn is_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Real(_) => true,
        ExprKind::Arith(_, l, r) => is_literal(l) && is_literal(r),
        _ => false,
    }
}

/// Compile-time evaluation of constant expressions with KSP integer semantics.
pub fn fold(hir: &Hir, e: &Expr) -> Option<Const> {
    use Const::*;
    Some(match &e.kind {
        ExprKind::Int(n) => Int(*n),
        ExprKind::Real(n) => Real(*n),
        ExprKind::Str(s) => Str(s.clone()),
        ExprKind::Neg(inner) => match fold(hir, inner)? {
            Int(n) => Int(n.wrapping_neg()),
            Real(n) => Real(-n),
            Str(_) => return None,
        },
        ExprKind::BitNot(inner) => match fold(hir, inner)? {
            Int(n) => Int(!n),
            _ => return None,
        },
        ExprKind::Cast(inner) => match (e.ty, fold(hir, inner)?) {
            (Ty::Int, Int(n)) => Int(n),
            (Ty::Bool, Int(n)) => Int(i32::from(n != 0)),
            _ => return None,
        },
        ExprKind::Not(inner) => match fold(hir, inner)? {
            Int(n) => Int(i32::from(n == 0)),
            _ => return None,
        },
        ExprKind::Arith(op, l, r) => match (fold(hir, l)?, fold(hir, r)?) {
            (Int(a), Int(b)) => Int(crate::eval::int_arith(*op, a, b)),
            (Real(a), Real(b)) => Real(crate::eval::real_arith(*op, a, b)?),
            _ => return None,
        },
        ExprKind::Compare(c, l, r) => match (fold(hir, l)?, fold(hir, r)?) {
            (Int(a), Int(b)) => Int(i32::from(c.apply(i64::from(a), i64::from(b)))),
            (Real(a), Real(b)) => Int(i32::from(crate::eval::compare_real(*c, a, b))),
            _ => return None,
        },
        ExprKind::Logic(op, l, r) => {
            let (Int(a), Int(b)) = (fold(hir, l)?, fold(hir, r)?) else {
                return None;
            };
            let (a, b) = (a != 0, b != 0);
            Int(i32::from(match op {
                Logic::And => a && b,
                Logic::Or => a || b,
                Logic::Xor => a != b,
            }))
        }
        ExprKind::Concat(parts) => {
            let mut s = String::new();
            for p in parts {
                match fold(hir, p)? {
                    Int(n) => s.push_str(&n.to_string()),
                    Real(n) => s.push_str(&crate::eval::real_text(n)),
                    Str(t) => s.push_str(&t),
                }
            }
            Str(s.into())
        }
        ExprKind::Builtin(b, args) => {
            let int = |i: usize| match args.get(i) {
                Some(Arg::Expr(e)) => match fold(hir, e) {
                    Some(Int(n)) => Some(n),
                    _ => None,
                },
                _ => None,
            };
            let real = |i: usize| match args.get(i) {
                Some(Arg::Expr(e)) => match fold(hir, e) {
                    Some(Real(n)) => Some(n),
                    _ => None,
                },
                _ => None,
            };
            match b {
                Builtin::ShLeft => Int(int(0)?.wrapping_shl(int(1)? as u32)),
                Builtin::ShRight => Int(int(0)?.wrapping_shr(int(1)? as u32)),
                Builtin::IntToReal | Builtin::Real => Real(f64::from(int(0)?)),
                Builtin::RealToInt | Builtin::Int => Int(crate::eval::real_to_int(real(0)?)),
                Builtin::Abs if e.ty == Ty::Int => Int(int(0)?.wrapping_abs()),
                Builtin::Abs => Real(real(0)?.abs()),
                Builtin::Min if e.ty == Ty::Int => Int(int(0)?.min(int(1)?)),
                Builtin::Max if e.ty == Ty::Int => Int(int(0)?.max(int(1)?)),
                Builtin::Min => Real(real(0)?.min(real(1)?)),
                Builtin::Max => Real(real(0)?.max(real(1)?)),
                Builtin::InRange => match (int(0), real(0)) {
                    (Some(x), _) => Int(i32::from(int(1)? <= x && x <= int(2)?)),
                    (_, Some(x)) => Int(i32::from(real(1)? <= x && x <= real(2)?)),
                    _ => return None,
                },
                Builtin::NumElements => match args.first()? {
                    Arg::Var(v, _) => Int(hir.vars[v.0 as usize].len? as i32),
                    Arg::SysArray(SysArray::GroupsAffected) => return None,
                    Arg::SysArray(a) if a.drop_kind().is_none() => Int(a.len() as i32),
                    _ => return None,
                },
                Builtin::GetUiId => {
                    let Some(Arg::Var(v, _)) = args.first() else {
                        return None;
                    };
                    Int(builtins::FIRST_UI_ID + hir.vars[v.0 as usize].ui? as i32)
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Kontakt's reserved constant families, including members this frontend
/// does not know (`$CONTROL_PAR_NKS_TYPE`): never variables or controls.
fn vendor_name(name: &str) -> bool {
    [
        "NI_",
        "CONTROL_PAR_",
        "EVENT_PAR_",
        "ENGINE_PAR_",
        "ZONE_PAR_",
        "LOOP_PAR_",
    ]
    .iter()
    .any(|p| name.get(1..).is_some_and(|n| n.starts_with(p)))
}
