//! Kontakt 8 KSP symbol statuses from `docs/re/ksp/SYMBOLS.tsv`, the catalogue that
//! `tools/re/ksp_symbols.py` generates. The doctor, corpus reports and the plugin's
//! Diagnostics all read this table, so they agree on a symbol's `KSP-*` code.
use std::{collections::HashMap, sync::OnceLock};

const TSV: &str = include_str!("../../../docs/re/ksp/SYMBOLS.tsv");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Full,
    Simplified,
    Stub,
    Unsupported,
    ModernExtension,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Simplified => "simplified",
            Self::Stub => "stub",
            Self::Unsupported => "unsupported",
            Self::ModernExtension => "modern-extension",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Full => "KSP-FULL",
            Self::Simplified => "KSP-SIMPLIFIED",
            Self::Stub => "KSP-STUB",
            Self::Unsupported => "KSP-UNSUPPORTED",
            Self::ModernExtension => "KSP-MODERN",
        }
    }

    /// `KSP-FULL`: implemented and named by a conformance test.
    pub fn is_full(self) -> bool {
        self == Self::Full
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: &'static str,
    pub status: Status,
    /// A conformance test names the symbol. `full` without one is `simplified`.
    pub tested: bool,
    pub corpus_rank: Option<u32>,
}

/// Every row of SYMBOLS.tsv. Templates keep their `#` placeholder as written.
pub fn table() -> &'static [Symbol] {
    static TABLE: OnceLock<Vec<Symbol>> = OnceLock::new();
    TABLE.get_or_init(|| parse(TSV))
}

/// The row for an exact script token (`on init`, `$CONTROL_PAR_HIDE`, `sh_left`).
pub fn lookup(name: &str) -> Option<&'static Symbol> {
    static BY_NAME: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let index = BY_NAME.get_or_init(|| {
        table().iter().enumerate().map(|(i, s)| (s.name, i)).collect()
    });
    index.get(name).map(|&i| &table()[i])
}

/// Status of a parity construct: the SYMBOLS.tsv row wins (the doctor's rule); names the
/// catalogue does not list (`@linked-source`) keep the ledger status their construct carries.
pub fn of_construct(name: &str, ledger: sampler_parity::Status) -> Status {
    match lookup(name) {
        Some(symbol) => symbol.status,
        None => match ledger {
            sampler_parity::Status::Implemented => Status::Full,
            sampler_parity::Status::Approximate => Status::Simplified,
            sampler_parity::Status::Stub => Status::Stub,
            sampler_parity::Status::Unsupported | sampler_parity::Status::Fault => {
                Status::Unsupported
            }
        },
    }
}

fn parse(text: &'static str) -> Vec<Symbol> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    let col = |want: &str| header.iter().position(|h| *h == want).expect("SYMBOLS.tsv column");
    let (c_name, c_class, c_status, c_tests, c_rank) =
        (col("name"), col("class"), col("status"), col("tests"), col("corpus_rank"));
    lines
        .filter(|l| !l.is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let field = |i: usize| f.get(i).copied().unwrap_or_default();
            let tested = field(c_tests) != "untested";
            let status = match field(c_class) {
                "" | "compat" => match field(c_status) {
                    "full" if tested => Status::Full,
                    "full" | "simplified" => Status::Simplified,
                    "stub" => Status::Stub,
                    _ => Status::Unsupported,
                },
                _ => Status::ModernExtension,
            };
            Symbol {
                name: field(c_name),
                status,
                tested,
                corpus_rank: field(c_rank).parse().ok(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_requires_a_conformance_test() {
        // Ledger Implemented, but no test names it in SYMBOLS.tsv: reported as simplified.
        let untested = lookup("sh_left").expect("row");
        assert!(!untested.tested);
        assert_eq!(untested.status, Status::Simplified);
        assert_eq!(untested.status.code(), "KSP-SIMPLIFIED");
        let tested = lookup("real").expect("row");
        assert!(tested.tested);
        assert!(tested.status.is_full());
        assert_eq!(tested.status.code(), "KSP-FULL");
    }

    #[test]
    fn catalogue_is_loaded_and_resolves_names() {
        assert!(table().len() > 1000);
        assert_eq!(lookup("on init").map(|s| s.status), Some(Status::Simplified));
        assert!(lookup("no_such_ksp_symbol").is_none());
    }
}
