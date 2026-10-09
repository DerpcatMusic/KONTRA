//! Feature-gated phase observations. No diagnostic text or source is retained.
use crate::{
    Error,
    diag::{Fault, Span},
};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub phase: &'static str,
    pub kind: &'static str,
    pub category: &'static str,
    pub builtin: Option<&'static str>,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
}
#[derive(Clone, Debug)]
pub struct Phase {
    pub present: Option<bool>,
    pub completion: &'static str,
    pub fault: Option<Diagnostic>,
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub slot: u8,
    pub attempt: &'static str,
    pub compile_ok: bool,
    pub init_ok: Option<bool>,
    pub init: Phase,
    pub persistence_changed: Phase,
    pub error: Option<Diagnostic>,
}
#[derive(Clone, Copy)]
struct Site {
    span: Span,
    category: &'static str,
    builtin: Option<&'static str>,
}
static ENABLED: AtomicBool = AtomicBool::new(false);
// ponytail: capture belongs to one isolated instrument process, not a multi-client service.
static RECORDS: Mutex<Vec<Observation>> = Mutex::new(Vec::new());
thread_local! {
    static ATTEMPT:Cell<&'static str>=const{Cell::new("standalone")};
    static INITIALIZED:Cell<Option<bool>>=const{Cell::new(None)};
    static CONTEXT:Cell<(&'static str,&'static str,Option<&'static str>)>=const{Cell::new(("lex","stage-error",None))};
    static PRESENT:Cell<(Option<bool>,Option<bool>)>=const{Cell::new((None,None))};
    static PHASES:RefCell<Vec<(&'static str,Option<Site>)>>=const{RefCell::new(Vec::new())};
}
/// Keeps one initializer's observation through deferred callback lowering.
#[derive(Clone)]
pub(crate) struct Checkpoint {
    attempt: &'static str,
    initialized: Option<bool>,
    context: (&'static str, &'static str, Option<&'static str>),
    present: (Option<bool>, Option<bool>),
    phases: Vec<(&'static str, Option<Site>)>,
}
pub(crate) fn checkpoint() -> Checkpoint {
    Checkpoint {
        attempt: ATTEMPT.get(),
        initialized: INITIALIZED.get(),
        context: CONTEXT.get(),
        present: PRESENT.get(),
        phases: PHASES.with(|p| p.borrow().clone()),
    }
}
pub(crate) fn restore(checkpoint: Checkpoint) {
    ATTEMPT.set(checkpoint.attempt);
    INITIALIZED.set(checkpoint.initialized);
    CONTEXT.set(checkpoint.context);
    PRESENT.set(checkpoint.present);
    PHASES.with(|p| *p.borrow_mut() = checkpoint.phases);
}
pub fn begin() {
    RECORDS.lock().unwrap().clear();
    ENABLED.store(true, Ordering::Relaxed);
}
pub fn attempt(name: &'static str) {
    ATTEMPT.set(name);
}
pub fn take() -> Vec<Observation> {
    std::mem::take(&mut *RECORDS.lock().unwrap())
}
pub(crate) fn reset_script() {
    INITIALIZED.set(None);
    PRESENT.set((None, None));
    PHASES.with(|p| p.borrow_mut().clear());
    stage("lex");
}
pub(crate) fn initialized(ok: bool) {
    INITIALIZED.set(Some(ok));
}
pub(crate) fn stage(phase: &'static str) {
    CONTEXT.set((phase, "stage-error", None));
}
pub(crate) fn category(c: &'static str) {
    let (p, _, b) = CONTEXT.get();
    CONTEXT.set((p, c, b));
}
pub(crate) fn builtin(b: Option<&'static str>) {
    let (p, c, _) = CONTEXT.get();
    CONTEXT.set((p, c, b));
}
pub(crate) fn present(init: bool, persist: bool) {
    PRESENT.set((Some(init), Some(persist)));
}
pub(crate) fn phase(name: &'static str, fault: Option<&Fault>) {
    let (_, category, builtin) = CONTEXT.get();
    PHASES.with(|p| {
        p.borrow_mut().push((
            name,
            fault.map(|f| Site {
                span: f.span,
                category,
                builtin: f.builtin.or(builtin),
            }),
        ))
    });
}
pub(crate) fn record<T>(result: &Result<T, Error>, source: &str, slot: u8) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let diag = |phase, site: Site| {
        let e = Fault {
            span: site.span,
            builtin: site.builtin,
            message: String::new(),
        }
        .locate(source);
        Diagnostic {
            phase,
            kind: "Error",
            category: site.category,
            builtin: site.builtin,
            offset: e.offset,
            line: e.line,
            column: e.column,
        }
    };
    let phase = |name, present| {
        PHASES.with(|p| {
            match p.borrow().iter().find(|(n, _)| {
                *n == name || name == "persistence_changed" && *n == "persistence_scheduled"
            }) {
                Some((phase, fault)) => Phase {
                    present,
                    completion: if *phase == "persistence_scheduled" {
                        "scheduled"
                    } else if fault.is_none() {
                        "completed"
                    } else {
                        "failed"
                    },
                    fault: fault.map(|s| diag(name, s)),
                },
                None => Phase {
                    present,
                    completion: if present == Some(false) {
                        "not_present"
                    } else {
                        "not_reached"
                    },
                    fault: None,
                },
            }
        })
    };
    let (ip, pp) = PRESENT.get();
    let (name, category, builtin) = CONTEXT.get();
    RECORDS.lock().unwrap().push(Observation {
        slot,
        attempt: ATTEMPT.get(),
        compile_ok: result.is_ok(),
        init_ok: INITIALIZED.get(),
        init: phase("init", ip),
        persistence_changed: phase("persistence_changed", pp),
        error: result.as_ref().err().map(|e| Diagnostic {
            phase: name,
            kind: match e.kind {
                crate::diag::Kind::Error => "Error",
                crate::diag::Kind::Warning => "Warning",
                crate::diag::Kind::Approximate => "Approximate",
                crate::diag::Kind::Unsupported => "Unsupported",
            },
            category,
            builtin: e.builtin.or(builtin),
            offset: e.offset,
            line: e.line,
            column: e.column,
        }),
    });
}
