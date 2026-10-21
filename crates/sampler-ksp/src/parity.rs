//! Source-free script inspection. Reporting never runs initialization or file services.
use crate::{
    Environment, Limits,
    builtins::{self, Builtin},
    diag, hir,
    lexer::{self, Interner, Punct, Tok},
    lower,
};
use sampler_parity::{
    Construct, ConstructKind as Kind, Coverage, Lang, Lowered, ScriptReport, Span, Status, Token,
    TokenClass as Class,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug)]
pub struct LedgerEntry {
    pub name: &'static str,
    pub status: Status,
    pub owner: &'static str,
    pub spec: &'static str,
    pub detail: &'static str,
}
#[path = "parity_ledger.rs"]
mod ledger;
pub fn ledger() -> &'static [LedgerEntry] {
    ledger::TABLE
}
pub fn ledger_markdown() -> String {
    let mut out = String::from(
        "# KSP parity ledger\n\nGenerated from `sampler_ksp::parity::ledger()` by `cargo run -p sampler-ksp --example parity_ledger`. Do not edit by hand.\n\nStatuses describe the supported subset, not parsing success. Per-use lowering may worsen a status through the `@lower:*` rows. Opaque constants and unverified host/DSP bindings remain Approximate. Compile failures add `@syntax`; authored values and source text are never included. Historical corpus counts in KSP_COVERAGE.md and the manual inventory in KSP_SURFACE.json are not implementation status authorities.\n\n| Construct | Status | Owner | Spec | Detail |\n|---|---|---|---|---|\n",
    );
    for e in ledger() {
        out.push_str(&format!(
            "| `{}` | {:?} | `{}` | {} | {} |\n",
            e.name, e.status, e.owner, e.spec, e.detail
        ));
    }
    out
}
fn entry(name: &str) -> Option<&'static LedgerEntry> {
    static INDEX: OnceLock<BTreeMap<&'static str, &'static LedgerEntry>> = OnceLock::new();
    let index = INDEX.get_or_init(|| ledger().iter().map(|e| (e.name, e)).collect());
    index.get(name).copied().or_else(|| {
        let mut key = String::new();
        let mut digits = false;
        for c in name.chars() {
            if !c.is_ascii_digit() {
                key.push(c);
            } else if !digits {
                key.push('#');
            }
            digits = c.is_ascii_digit();
        }
        index.get(key.as_str()).copied()
    })
}
pub(crate) fn lowering_status(coverage: lower::Coverage) -> Option<&'static LedgerEntry> {
    match coverage {
        lower::Coverage::Ignored => entry("@lower:ignored"),
        lower::Coverage::Approximate => entry("@lower:approximate"),
        _ => None,
    }
}

/// Opt-in, control-thread evaluator observations. Creating a handle enables collection;
/// omit it from Environment when the Script view is closed. Snapshots contain no values.
#[derive(Clone, Debug)]
pub struct RuntimeCoverage(Arc<Mutex<Coverage>>);
impl RuntimeCoverage {
    pub fn new(script_id: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(Coverage {
            script_id: script_id.into(),
            ..Default::default()
        })))
    }
    pub fn snapshot(&self) -> Coverage {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub(crate) fn hit(&self, span: diag::Span) {
        let mut c = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match c
            .hits
            .binary_search_by_key(&span.start, |&(start, _)| start)
        {
            Ok(i) => c.hits[i].1 = c.hits[i].1.saturating_add(1),
            Err(i) => c.hits.insert(i, (span.start, 1)),
        }
    }
    pub(crate) fn fault(&self, span: diag::Span) {
        let mut c = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !c.faults.iter().any(|(s, _)| *s == span.start) {
            c.faults.push((
                span.start,
                "KSP evaluator fault (authored values omitted)".into(),
            ));
        }
    }
}

pub fn parity_report(source: &str, id: &str, title: &str) -> ScriptReport {
    parity_report_with(source, id, title, Limits::LIBRARY, &Environment::default())
}
pub fn parity_report_with(
    source: &str,
    id: &str,
    title: &str,
    limits: Limits,
    environment: &Environment,
) -> ScriptReport {
    parity_report_with_detail(source, id, title, limits, environment, true)
}

/// The same coverage and diagnostics as `parity_report_with`, without token
/// highlighting or lowered operation prose. Use for aggregate inspection that
/// discards those large source maps.
pub fn parity_report_summary_with(
    source: &str,
    id: &str,
    title: &str,
    limits: Limits,
    environment: &Environment,
) -> ScriptReport {
    parity_report_with_detail(source, id, title, limits, environment, false)
}

fn parity_report_with_detail(
    source: &str,
    id: &str,
    title: &str,
    limits: Limits,
    environment: &Environment,
    source_map: bool,
) -> ScriptReport {
    let starts: Vec<_> = std::iter::once(0)
        .chain(
            source
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
        )
        .collect();
    let span = |s: diag::Span| Span {
        start: s.start,
        end: s.end,
        line: starts.partition_point(|&i| i <= s.start as usize) as u32,
    };
    let mut report = ScriptReport {
        id: id.into(),
        title: title.into(),
        lang: Lang::Ksp,
        source_len: source.len().min(u32::MAX as usize) as u32,
        tokens: Vec::new(),
        constructs: Vec::new(),
        lowered: Vec::new(),
    };
    if source.trim().is_empty() {
        return report;
    }
    let fail = |report: &mut ScriptReport, fault: diag::Fault| {
        // Diagnostic prose can contain authored literals: retain only its category and position.
        let key = format!("@syntax:{}", fault.message);
        let e = entry(&key).unwrap_or_else(|| entry("@syntax").unwrap());
        report
            .constructs
            .push(construct(span(fault.span), Kind::Syntax, e.name, Some(e)));
    };
    if source.len() > limits.source_bytes {
        fail(
            &mut report,
            diag::Fault {
                span: diag::Span::default(),
                builtin: None,
                message: String::new(),
            },
        );
        return report;
    }
    let mut syms = Interner::default();
    let mut toks = match lexer::lex_highlight(source, &mut syms) {
        Ok(t) => t,
        Err(f) => {
            fail(&mut report, f);
            return report;
        }
    };
    let mut previous = [None, None];
    // Advance a single cursor through comments. Repeated suffix searches can
    // become quadratic for a long series of call-like identifiers/comments.
    let mut next_non_comment = 1usize;
    for (i, t) in toks.iter().enumerate() {
        next_non_comment = next_non_comment.max(i + 1);
        while next_non_comment < toks.len() && toks[next_non_comment].tok == Tok::Comment {
            next_non_comment += 1;
        }
        let mut use_kind = None;
        let mut name = "";
        let class = match t.tok {
            Tok::Comment => Class::Comment,
            Tok::Ident(s) => {
                name = syms.name(s);
                if matches!(previous[0], Some(Tok::Ident(p)) if syms.name(p) == "on")
                    && !matches!(previous[1], Some(Tok::Ident(p)) if syms.name(p) == "end")
                {
                    use_kind = Some(Kind::Callback);
                    Class::Callback
                } else if Builtin::from_name(name).is_some() {
                    use_kind = Some(Kind::Builtin);
                    Class::Builtin
                } else if PREPROCESSOR.contains(&name) {
                    Class::Preprocessor
                } else if KEYWORDS.contains(&name) || hir::WidgetKind::from_keyword(name).is_some()
                {
                    Class::Keyword
                } else if toks.get(next_non_comment).map(|t| t.tok)
                    == Some(Tok::Punct(Punct::LParen))
                {
                    use_kind = Some(Kind::Command);
                    Class::Builtin
                } else {
                    Class::Other
                }
            }
            Tok::Var(s) => {
                name = syms.name(s);
                if name.starts_with("$CONTROL_PAR_") {
                    use_kind = Some(Kind::ControlPar);
                    Class::Constant
                } else if name.starts_with("$ENGINE_PAR_") {
                    use_kind = Some(Kind::EnginePar);
                    Class::Constant
                } else if builtins::sys_var(name).is_some()
                    || builtins::SysArray::from_name(name).is_some()
                {
                    use_kind = Some(Kind::Variable);
                    Class::Variable
                } else if builtins::constant(name).is_some()
                    || builtins::real_constant(name).is_some()
                    || builtins::control_par(name).is_some()
                    || builtins::is_vendor_constant(name)
                {
                    use_kind = Some(Kind::Constant);
                    Class::Constant
                } else {
                    Class::Variable
                }
            }
            Tok::Int(..) | Tok::Real(..) => Class::Number,
            Tok::Str(..) => Class::String,
            Tok::Punct(
                Punct::LParen | Punct::RParen | Punct::LBracket | Punct::RBracket | Punct::Comma,
            ) => Class::Punctuation,
            Tok::Punct(_) => Class::Operator,
            Tok::Eof => continue,
        };
        if source_map {
            report.tokens.push(Token {
                span: span(t.span),
                class,
            });
        }
        if t.tok != Tok::Comment {
            previous = [Some(t.tok), previous[0]];
        }
        if let Some(kind) = use_kind {
            let canonical = if kind == Kind::Callback {
                format!(
                    "on {}",
                    if name == "_pgs_changed" {
                        "pgs_changed"
                    } else {
                        name
                    }
                )
            } else if let Some(b) = Builtin::from_name(name) {
                b.name().into()
            } else {
                name.into()
            };
            report
                .constructs
                .push(construct(span(t.span), kind, &canonical, entry(&canonical)));
        }
    }
    toks.retain(|t| t.tok != Tok::Comment);
    let mut declarations = Vec::new();
    let frontend = (|| {
        lexer::preprocess(&mut toks, &syms, &Default::default())?;
        let ast = crate::parser::parse(&toks, &syms)?;
        report
            .constructs
            .retain(|c| !matches!(c.kind, Kind::Builtin | Kind::Command));
        ast_calls(
            &ast,
            &syms,
            &span,
            &mut report.constructs,
            &mut declarations,
            source_map,
        );
        report.constructs.sort_by_key(|c| c.span.start);
        crate::sema::analyze(
            ast,
            &syms,
            crate::sema::Budget {
                variables: limits.variables,
                array_cells: limits.array_cells,
            },
            &environment.performance_view.controls,
        )
    })();
    let hir = match frontend {
        Ok(h) => h,
        Err(f) => {
            fail(&mut report, f);
            return report;
        }
    };
    if source_map {
        report.lowered.extend(declarations);
        let constants: std::collections::BTreeSet<_> = hir
            .vars
            .iter()
            .filter(|v| matches!(v.home, hir::Home::Const(_)))
            .filter_map(|v| syms.get(&v.name))
            .collect();
        let constant_tokens: std::collections::BTreeSet<_> = toks
            .iter()
            .filter_map(|t| match t.tok {
                Tok::Var(s) if constants.contains(&s) => Some(t.span.start),
                _ => None,
            })
            .collect();
        for t in &mut report.tokens {
            if constant_tokens.contains(&t.span.start) {
                t.class = Class::Constant;
            }
        }
    }
    let ids: Vec<_> = hir
        .uis
        .iter()
        .map(|ui| {
            Some(crate::derived_control_id(
                environment.slot,
                &hir.vars[ui.var.0 as usize].name,
            ))
        })
        .collect();
    let mut unit = lower::Unit {
        hir: &hir,
        controls: &ids,
        groups: &environment.groups,
            folders: &environment.folders,
        slot: environment.slot,
        budget: limits.instructions,
        limit: limits.instructions,
        services: Vec::new(),
        array_files: Default::default(),
        coverage: Default::default(),
        warnings: Vec::new(),
        scratch: 0,
        modules: Vec::new(),
        inspection: Some(Inspection {
            record_ops: source_map,
            ..Default::default()
        }),
    };
    let mut programs = Vec::new();
    for cb in &hir.callbacks {
        if cb.kind == hir::CallbackKind::Init {
            if source_map {
                init_statements(&cb.body, &span, &mut report.lowered);
                init_functions(
                    &hir,
                    &cb.body,
                    &span,
                    &mut report.lowered,
                    &mut Default::default(),
                );
            }
            for c in report
                .constructs
                .iter_mut()
                .filter(|c| cb.span.start <= c.span.start && c.span.start < cb.span.end)
            {
                if let Some(e) = entry(&format!("@init:{}", c.name)) {
                    c.status = c.status.max(e.status);
                    c.detail = e.detail.into();
                }
            }
            continue;
        }
        let ctx = match cb.kind {
            hir::CallbackKind::Note => lower::Context::Note,
            hir::CallbackKind::Release => lower::Context::Release,
            hir::CallbackKind::Controller => lower::Context::Controller,
            _ => lower::Context::Plan,
        };
        match unit.program(programs.len(), &cb.body, cb.span, ctx, cb.kind, None) {
            Ok(p) => programs.push(p),
            Err(f) => fail(&mut report, f),
        }
    }
    if let Err(f) = unit.finish(&mut programs, source) {
        fail(&mut report, f);
    }
    let inspection = unit.inspection.take().unwrap();
    if source_map {
        let mut ops: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
        for (s, op) in inspection.ops {
            ops.entry((s.start, s.end)).or_default().push(op);
        }
        report
            .lowered
            .extend(ops.into_iter().map(|((start, end), ops)| Lowered {
                span: span(diag::Span { start, end }),
                ops,
            }));
    }
    let mut uses: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, c) in report.constructs.iter().enumerate() {
        uses.entry(c.name.clone()).or_default().push(i);
    }
    for (s, b, c) in inspection.calls {
        if let Some(e) = lowering_status(c) {
            if let Some(indices) = uses.get(b.name()) {
                let begin = indices.partition_point(|&i| report.constructs[i].span.start < s.start);
                for &i in &indices[begin..] {
                    let construct = &mut report.constructs[i];
                    if construct.span.start >= s.end {
                        break;
                    }
                    if construct.status < e.status {
                        construct.status = e.status;
                        construct.detail = e.detail.into();
                    }
                }
            }
        }
    }
    let mut statements: BTreeMap<(u32, u32, u32), Vec<String>> = BTreeMap::new();
    for lowered in std::mem::take(&mut report.lowered) {
        statements
            .entry((lowered.span.start, lowered.span.end, lowered.span.line))
            .or_default()
            .extend(lowered.ops);
    }
    report.lowered = statements
        .into_iter()
        .map(|((start, end, line), ops)| Lowered {
            span: Span { start, end, line },
            ops,
        })
        .collect();
    report
}
fn construct(span: Span, kind: Kind, name: &str, e: Option<&LedgerEntry>) -> Construct {
    let fallback = entry(if kind == Kind::Syntax {
        "@syntax"
    } else {
        "@unknown"
    })
    .unwrap();
    let e = e.unwrap_or(fallback);
    Construct {
        span,
        kind,
        name: name.into(),
        status: e.status,
        detail: e.detail.into(),
        owner: Some(e.owner.into()),
        spec: Some(e.spec.into()),
    }
}
fn ast_calls(
    ast: &crate::ast::Ast,
    syms: &Interner<'_>,
    span: &impl Fn(diag::Span) -> Span,
    out: &mut Vec<Construct>,
    declarations: &mut Vec<Lowered>,
    source_map: bool,
) {
    use crate::ast::{ExprKind as E, Item, StmtKind as S};
    let functions: std::collections::BTreeSet<_> = ast
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) => Some(syms.name(f.name).to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    struct Calls<'a, 's, 'o, F> {
        syms: &'s Interner<'a>,
        functions: std::collections::BTreeSet<String>,
        span: F,
        out: &'o mut Vec<Construct>,
        declarations: &'o mut Vec<Lowered>,
        source_map: bool,
    }
    impl<F: Fn(diag::Span) -> Span> Calls<'_, '_, '_, F> {
        fn call(&mut self, name: lexer::Sym, at: diag::Span, explicit: bool) {
            let name = self.syms.name(name);
            let b = Builtin::from_name(name);
            let user = self.functions.contains(&name.to_ascii_lowercase());
            let kind = if b.is_some() && !explicit && !user {
                Kind::Builtin
            } else {
                Kind::Command
            };
            let canonical = if kind == Kind::Builtin {
                b.unwrap().name()
            } else {
                name
            };
            self.out.push(construct(
                (self.span)(at),
                kind,
                canonical,
                if user {
                    entry("@command")
                } else {
                    entry(canonical)
                },
            ));
        }
        fn expr(&mut self, e: &crate::ast::Expr) {
            match &e.kind {
                E::Call(n, args) => {
                    self.call(*n, e.span, false);
                    for a in args {
                        self.expr(a);
                    }
                }
                E::Var(_, Some(index)) | E::Unary(_, index) => self.expr(index),
                E::Binary(_, a, b) => {
                    self.expr(a);
                    self.expr(b);
                }
                _ => {}
            }
        }
        fn body(&mut self, body: &[crate::ast::Stmt]) {
            for s in body {
                match &s.kind {
                    S::Command(n, args) => {
                        self.call(*n, s.span, false);
                        for a in args {
                            self.expr(a);
                        }
                    }
                    S::Call(n) => self.call(*n, s.span, true),
                    S::Declare(d) => {
                        if self.source_map {
                            let op = if let Some(ui) = d.ui {
                                format!("declare {}: allocate UI model/control", self.syms.name(ui))
                            } else {
                                match d.storage {
                                    crate::ast::Storage::Const => "bind compile-time constant".into(),
                                    crate::ast::Storage::Polyphonic => {
                                        "allocate note-owned state".into()
                                    }
                                    _ => "allocate typed script state".into(),
                                }
                            };
                            self.declarations.push(Lowered {
                                span: (self.span)(s.span),
                                ops: vec![op],
                            });
                        }
                        if let Some(e) = &d.size {
                            self.expr(e);
                        }
                        for e in d.params.iter().chain(&d.init) {
                            self.expr(e);
                        }
                    }
                    S::Assign(a, b) => {
                        self.expr(a);
                        self.expr(b);
                    }
                    S::If(e, a, b) => {
                        self.expr(e);
                        self.body(a);
                        self.body(b);
                    }
                    S::While(e, b) => {
                        self.expr(e);
                        self.body(b);
                    }
                    S::Select(e, cases) => {
                        self.expr(e);
                        for c in cases {
                            self.expr(&c.low);
                            if let Some(e) = &c.high {
                                self.expr(e);
                            }
                            self.body(&c.body);
                        }
                    }
                }
            }
        }
    }
    let mut calls = Calls {
        syms,
        functions,
        span,
        out,
        declarations,
        source_map,
    };
    for item in &ast.items {
        match item {
            Item::Callback(c) => calls.body(&c.body),
            Item::Function(f) => calls.body(&f.body),
        }
    }
}
fn init_functions(
    hir: &hir::Hir,
    body: &[hir::Stmt],
    span: &impl Fn(diag::Span) -> Span,
    out: &mut Vec<Lowered>,
    seen: &mut std::collections::BTreeSet<hir::FnId>,
) {
    for s in body {
        match &s.kind {
            hir::StmtKind::Call(f) if seen.insert(*f) => {
                let body = &hir.functions[f.0 as usize].body;
                init_statements(body, span, out);
                init_functions(hir, body, span, out, seen);
            }
            hir::StmtKind::If(_, a, b) => {
                init_functions(hir, a, span, out, seen);
                init_functions(hir, b, span, out, seen);
            }
            hir::StmtKind::While(_, b) => init_functions(hir, b, span, out, seen),
            hir::StmtKind::Select(_, cases) => {
                for c in cases {
                    init_functions(hir, &c.body, span, out, seen);
                }
            }
            _ => {}
        }
    }
}
fn init_statements(body: &[hir::Stmt], span: &impl Fn(diag::Span) -> Span, out: &mut Vec<Lowered>) {
    use hir::StmtKind::*;
    for s in body {
        let label = match &s.kind {
            Assign(..) => "init: write typed state/control",
            Fill(..) => "init: fill typed array",
            UiParams(..) => "init: evaluate declaration parameters",
            If(_, a, b) => {
                init_statements(a, span, out);
                init_statements(b, span, out);
                "init: conditional"
            }
            While(_, b) => {
                init_statements(b, span, out);
                "init: loop"
            }
            Select(_, cases) => {
                for c in cases {
                    init_statements(&c.body, span, out);
                }
                "init: select"
            }
            Call(_) => "init: call user function",
            Builtin(b, _) => {
                out.push(Lowered {
                    span: span(s.span),
                    ops: vec![format!(
                        "init: {} (typed evaluator/UI/engine binding)",
                        b.name()
                    )],
                });
                continue;
            }
        };
        out.push(Lowered {
            span: span(s.span),
            ops: vec![label.into()],
        });
    }
}
const PREPROCESSOR: &[&str] = &[
    "SET_CONDITION",
    "RESET_CONDITION",
    "USE_CODE_IF",
    "USE_CODE_IF_NOT",
    "END_USE_CODE",
];
const KEYWORDS: &[&str] = &[
    "on",
    "end",
    "declare",
    "const",
    "polyphonic",
    "pers",
    "instpers",
    "if",
    "else",
    "while",
    "select",
    "case",
    "to",
    "function",
    "call",
    "and",
    "or",
    "xor",
    "not",
    "mod",
];

#[derive(Default)]
pub(crate) struct Inspection {
    pub ops: Vec<(diag::Span, String)>,
    pub calls: Vec<(diag::Span, Builtin, lower::Coverage)>,
    pub record_ops: bool,
}
pub(crate) fn operation_name(op: &sampler_core::Instruction) -> String {
    // Debug contains authored operands; keep ONLY the enum variant name.
    let text = format!("{op:?}");
    let text = text.strip_prefix("Op(").unwrap_or(&text);
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap_or("operation")
        .into()
}
