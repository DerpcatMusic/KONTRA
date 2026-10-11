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
    let fail = |report: &mut ScriptReport, fault: diag::Fault| {
        // Diagnostic prose can contain authored literals: retain only its category and position.
        let e = entry("@syntax").unwrap();
        report.constructs.push(construct(
            span(fault.span),
            Kind::Syntax,
            "@syntax",
            Some(e),
        ));
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
    for (i, t) in toks.iter().enumerate() {
        let mut use_kind = None;
        let mut name = "";
        let class = match t.tok {
            Tok::Comment => Class::Comment,
            Tok::Ident(s) => {
                name = syms.name(s);
                if i > 0
                    && matches!(toks[i-1].tok, Tok::Ident(p) if syms.name(p) == "on")
                    && !(i > 1 && matches!(toks[i-2].tok, Tok::Ident(p) if syms.name(p)=="end"))
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
                } else if toks
                    .get(i + 1)
                    .is_some_and(|t| t.tok == Tok::Punct(Punct::LParen))
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
        report.tokens.push(Token {
            span: span(t.span),
            class,
        });
        if let Some(kind) = use_kind {
            let canonical = if kind == Kind::Callback {
                format!("on {}", name.trim_start_matches('_'))
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
    let frontend = (|| {
        lexer::preprocess(&mut toks, &syms, &Default::default())?;
        let ast = crate::parser::parse(&toks, &syms)?;
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
        slot: environment.slot,
        budget: limits.instructions,
        limit: limits.instructions,
        services: Vec::new(),
        array_files: Default::default(),
        coverage: Default::default(),
        warnings: Vec::new(),
        scratch: 0,
        modules: Vec::new(),
        inspection: Some(Default::default()),
    };
    for (i, cb) in hir.callbacks.iter().enumerate() {
        if cb.kind == hir::CallbackKind::Init {
            init_statements(&cb.body, &span, &mut report.lowered);
            continue;
        }
        let ctx = match cb.kind {
            hir::CallbackKind::Note => lower::Context::Note,
            hir::CallbackKind::Release => lower::Context::Release,
            hir::CallbackKind::Controller => lower::Context::Controller,
            _ => lower::Context::Plan,
        };
        if let Err(f) = unit.program(i, &cb.body, cb.span, ctx, cb.kind, None) {
            fail(&mut report, f);
        }
    }
    let inspection = unit.inspection.take().unwrap();
    let mut statements = Vec::new();
    for cb in &hir.callbacks {
        collect_statements(&cb.body, &mut statements);
    }
    for f in &hir.functions {
        collect_statements(&f.body, &mut statements);
    }
    let mut ops: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
    for (s, op) in inspection.ops {
        let owner = statements
            .iter()
            .filter(|stmt| stmt.start <= s.start && s.end <= stmt.end)
            .min_by_key(|stmt| stmt.end - stmt.start)
            .copied()
            .unwrap_or(s);
        ops.entry((owner.start, owner.end)).or_default().push(op);
    }
    report
        .lowered
        .extend(ops.into_iter().map(|((start, end), ops)| Lowered {
            span: span(diag::Span { start, end }),
            ops,
        }));
    for (s, b, c) in inspection.calls {
        if let Some(e) = lowering_status(c) {
            for construct in report
                .constructs
                .iter_mut()
                .filter(|x| x.name == b.name() && s.start <= x.span.start && x.span.start < s.end)
            {
                if construct.status < e.status {
                    construct.status = e.status;
                    construct.detail = e.detail.into();
                }
            }
        }
    }
    report.lowered.sort_by_key(|l| l.span.start);
    report
}
fn construct(span: Span, kind: Kind, name: &str, e: Option<&LedgerEntry>) -> Construct {
    let fallback = entry("@syntax").unwrap();
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
fn collect_statements(body: &[hir::Stmt], out: &mut Vec<diag::Span>) {
    for s in body {
        out.push(s.span);
        match &s.kind {
            hir::StmtKind::If(_, a, b) => {
                collect_statements(a, out);
                collect_statements(b, out);
            }
            hir::StmtKind::While(_, b) => collect_statements(b, out),
            hir::StmtKind::Select(_, cases) => {
                for c in cases {
                    collect_statements(&c.body, out);
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
