//! Script parity contract: how an authored script goes in and what KONTRA makes of it.
//!
//! Producers: sampler-ksp (Kontakt KSP) and sampler-uvi (Falcon/UVI Luau).
//! Consumers: the editor's Script view and the `kontakto parity` CLI.
//!
//! Privacy: a report never holds source text. Spans index into the source the
//! consumer already has in memory; serialized reports carry names and positions only.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    Ksp,
    Luau,
}

/// Byte range into the script source, with its 1-based line for display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenClass {
    Keyword,
    Callback,
    Builtin,
    Variable,
    Constant,
    Number,
    String,
    Comment,
    Operator,
    Punctuation,
    Preprocessor,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    pub span: Span,
    pub class: TokenClass,
}

/// Ordered from best to worst; a line shows its worst construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Status {
    /// Behaves like the native host (tested against a law, reference or v1).
    Implemented,
    /// Runs, but a documented approximation differs from the native host.
    Approximate,
    /// Accepted and ignored: no effect where the native host has one.
    Stub,
    /// Not supported: compile/translate drops it or the load reports it.
    Unsupported,
    /// Supported, but faulted at runtime in this instrument.
    Fault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstructKind {
    Callback,
    Builtin,
    Command,
    Variable,
    Constant,
    ControlPar,
    EnginePar,
    Syntax,
    LuaApi,
    LuaModule,
    LuaParameter,
}

/// One use of a language construct at one place in the script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Construct {
    pub span: Span,
    pub kind: ConstructKind,
    /// Canonical construct name, e.g. `set_control_par`, `$CONTROL_PAR_HIDE`, `setParameter Bypass`.
    pub name: String,
    pub status: Status,
    /// Why this status: the specific limitation, fault text or approximation (no authored values).
    pub detail: String,
    /// Where KONTRA implements or would implement it, `crate/path.rs:line`.
    pub owner: Option<String>,
    /// Spec or law reference, e.g. `docs/architecture-v2/KSP_SEMANTICS.md#...`.
    pub spec: Option<String>,
}

/// What KONTRA turned a source line into ("how it comes out").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lowered {
    pub span: Span,
    /// Human-readable lowered operations or host bindings, one per entry.
    pub ops: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptReport {
    /// Stable per instrument, e.g. `ksp:slot0` or `uvi:/Program/EventProcessors/0`.
    pub id: String,
    pub title: String,
    pub lang: Lang,
    pub source_len: u32,
    pub tokens: Vec<Token>,
    pub constructs: Vec<Construct>,
    pub lowered: Vec<Lowered>,
}

/// Runtime observation, keyed by span start; collected only while an inspector asks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub script_id: String,
    /// (span.start, executions) for every executed statement or call.
    pub hits: Vec<(u32, u64)>,
    /// (span.start, fault text) for runtime faults.
    pub faults: Vec<(u32, String)>,
}

impl ScriptReport {
    /// Worst status per 1-based line, for gutter marks.
    pub fn line_status(&self) -> Vec<(u32, Status)> {
        let mut out: Vec<(u32, Status)> = Vec::new();
        for c in &self.constructs {
            match out.iter_mut().find(|(l, _)| *l == c.span.line) {
                Some((_, s)) => *s = (*s).max(c.status),
                None => out.push((c.span.line, c.status)),
            }
        }
        out.sort_unstable_by_key(|(l, _)| *l);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_shows_its_worst_construct() {
        let at = |line, status| Construct {
            span: Span { start: 0, end: 1, line },
            kind: ConstructKind::Builtin,
            name: "x".into(),
            status,
            detail: String::new(),
            owner: None,
            spec: None,
        };
        let r = ScriptReport {
            id: "ksp:slot0".into(),
            title: "t".into(),
            lang: Lang::Ksp,
            source_len: 1,
            tokens: vec![],
            constructs: vec![at(3, Status::Implemented), at(3, Status::Unsupported), at(1, Status::Stub)],
            lowered: vec![],
        };
        assert_eq!(r.line_status(), vec![(1, Status::Stub), (3, Status::Unsupported)]);
    }
}
