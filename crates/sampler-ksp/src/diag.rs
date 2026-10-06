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

/// A compile failure positioned in the source. Line and column are 1-based;
/// the column counts characters, not bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// UTF-8 byte offset into the supplied source; always a char boundary.
    pub offset: usize,
    pub line: u32,
    pub column: u32,
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
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Fault>;

pub fn fault<T>(span: Span, message: impl Into<String>) -> Result<T> {
    Err(Fault {
        span,
        message: message.into(),
    })
}

impl Fault {
    pub fn locate(self, source: &str) -> Error {
        let mut offset = (self.span.start as usize).min(source.len());
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &source[..offset];
        let line = before.bytes().filter(|&b| b == b'\n').count() as u32 + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = source[line_start..offset].chars().count() as u32 + 1;
        Error {
            offset,
            line,
            column,
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
            message: "m".into(),
        }
        .locate(source);
        assert_eq!((error.line, error.column, error.offset), (2, 5, at));
        assert_eq!(error.to_string(), "2:5: m");
    }
}
