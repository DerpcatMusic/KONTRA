//! Source spans and positioned diagnostics.

/// Byte range in the supplied source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}
impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// What a diagnostic reports, for the load report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// The script does not compile.
    Error,
    /// Compiles, but Kontakt would behave differently or report a problem.
    Warning,
    /// A builtin runs with simplified semantics (first call site per builtin).
    Approximate,
    /// A builtin is accepted but has no effect.
    Unsupported,
}

/// A positioned diagnostic: the compile error, or an entry in
/// `Script::warnings`. Line and column are 1-based; the column counts
/// characters, not bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// UTF-8 byte offset into the supplied source; always a char boundary.
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub kind: Kind,
    /// KSP builtin the diagnostic is about, e.g. `set_engine_par`.
    pub builtin: Option<&'static str>,
    pub message: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}
impl std::error::Error for Error {}

/// Unpositioned failure raised deep in a pass; positioned once at the boundary.
#[derive(Clone, Debug)]
pub struct Fault {
    pub span: Span,
    pub builtin: Option<&'static str>,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Fault>;

pub fn fault<T>(span: Span, message: impl Into<String>) -> Result<T> {
    Err(Fault {
        span,
        builtin: None,
        message: message.into(),
    })
}

impl Fault {
    pub fn locate(self, source: &str) -> Error {
        self.locate_as(source, Kind::Error)
    }
    pub fn locate_as(self, source: &str, kind: Kind) -> Error {
        self.locate_indexed(source, kind, &[])
    }
    /// A shared newline index avoids rescanning the source for every warning.
    pub(crate) fn locate_indexed(self, source: &str, kind: Kind, starts: &[usize]) -> Error {
        let mut offset = (self.span.start as usize).min(source.len());
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &source[..offset];
        let (line, line_start) = if starts.is_empty() {
            (
                before.bytes().filter(|&b| b == b'\n').count() as u32 + 1,
                before.rfind('\n').map_or(0, |i| i + 1),
            )
        } else {
            let line = starts.partition_point(|&start| start <= offset);
            (line as u32, starts[line - 1])
        };
        let column = source[line_start..offset].chars().count() as u32 + 1;
        Error {
            offset,
            line,
            column,
            kind,
            builtin: self.builtin,
            message: self.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_line_and_character_column() {
        let source = "on init\n  é $x\nend on";
        let at = source.find("$x").unwrap();
        let error = Fault {
            span: Span::new(at, at + 2),
            builtin: None,
            message: "m".into(),
        }
        .locate(source);
        assert_eq!((error.line, error.column, error.offset), (2, 5, at));
        assert_eq!(error.to_string(), "2:5: m");
    }
    #[test]
    fn indexed_positions_match_single_errors_including_unicode_and_truncation() {
        let source = "on init\n  é $x\nend on\n";
        let starts: Vec<_> = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
            )
            .collect();
        for offset in 0..source.len() + 4 {
            let fault = Fault {
                span: Span::new(offset, offset),
                builtin: None,
                message: "m".into(),
            };
            assert_eq!(
                fault.clone().locate_as(source, Kind::Warning),
                fault.locate_indexed(source, Kind::Warning, &starts)
            );
        }
    }
}
