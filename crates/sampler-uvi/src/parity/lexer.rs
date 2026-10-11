use sampler_parity::{Span, Token, TokenClass as C};

pub(super) fn lex(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            line += u32::from(bytes[i] == b'\n');
            i += 1;
            continue;
        }
        let start = i;
        let at_line = line;
        let class;
        if bytes[i..].starts_with(b"--") {
            i += 2;
            i = long_end(bytes, i).unwrap_or_else(|| {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                i
            });
            class = C::Comment;
        } else if let Some(end) = long_end(bytes, i) {
            i = end;
            class = C::String;
        } else if matches!(bytes[i], b'\'' | b'"' | b'`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else {
                    i += 1;
                    if bytes[i - 1] == quote {
                        break;
                    }
                }
            }
            // shortcut: interpolated backticks are one string token; split expressions when the editor needs them.
            class = C::String;
        } else if bytes[i].is_ascii_digit()
            || bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
        {
            let hex = bytes[i..].starts_with(b"0x") || bytes[i..].starts_with(b"0X");
            let binary = bytes[i..].starts_with(b"0b") || bytes[i..].starts_with(b"0B");
            if hex || binary {
                i += 2;
            }
            while i < bytes.len() {
                let b = bytes[i];
                if b.is_ascii_digit() || b == b'_' || hex && b.is_ascii_hexdigit() {
                    i += 1;
                } else if !binary && b == b'.' && bytes.get(i + 1) != Some(&b'.') {
                    i += 1;
                } else if !binary
                    && (hex && matches!(b, b'p' | b'P') || !hex && matches!(b, b'e' | b'E'))
                {
                    i += 1;
                    if bytes.get(i).is_some_and(|b| matches!(b, b'+' | b'-')) {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            class = C::Number;
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &source[start..i];
            let type_declaration = name == "type"
                && bytes[i..]
                    .iter()
                    .copied()
                    .find(|b| !b.is_ascii_whitespace())
                    .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
            class = match name {
                "type" if type_declaration => C::Keyword,
                "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for"
                | "function" | "if" | "in" | "local" | "nil" | "not" | "or" | "repeat"
                | "return" | "then" | "true" | "until" | "while" | "continue" | "export" => {
                    C::Keyword
                }
                _ if super::ledger::is_callback(name) => C::Callback,
                _ if super::ledger::entry(name).is_some()
                    || matches!(
                        name,
                        "_G" | "assert"
                            | "error"
                            | "pcall"
                            | "xpcall"
                            | "pairs"
                            | "ipairs"
                            | "next"
                            | "select"
                            | "tostring"
                            | "tonumber"
                            | "rawget"
                            | "rawset"
                            | "rawequal"
                            | "getmetatable"
                            | "setmetatable"
                            | "unpack"
                            | "math"
                            | "string"
                            | "table"
                            | "coroutine"
                            | "bit32"
                    ) =>
                {
                    C::Builtin
                }
                _ => C::Variable,
            };
        } else {
            class = if matches!(
                bytes[i],
                b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b';' | b':'
            ) {
                C::Punctuation
            } else if bytes[i].is_ascii() {
                C::Operator
            } else {
                C::Other
            };
            i += source[i..].chars().next().unwrap().len_utf8();
        }
        line += bytes[start..i].iter().filter(|b| **b == b'\n').count() as u32;
        out.push(Token {
            span: Span {
                start: start as u32,
                end: i as u32,
                line: at_line,
            },
            class,
        });
    }
    out
}

fn long_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'[') {
        return None;
    }
    let mut open = start + 1;
    while bytes.get(open) == Some(&b'=') {
        open += 1;
    }
    if bytes.get(open) != Some(&b'[') {
        return None;
    }
    let equals = open - start - 1;
    let mut i = open + 1;
    while i < bytes.len() {
        if bytes[i] == b']'
            && bytes
                .get(i + 1..i + 1 + equals)
                .is_some_and(|s| s.iter().all(|b| *b == b'='))
            && bytes.get(i + 1 + equals) == Some(&b']')
        {
            return Some(i + equals + 2);
        }
        i += 1;
    }
    Some(bytes.len())
}

/// Decode only names; callers must never copy arbitrary string tokens into a report.
pub(super) fn literal(text: &str) -> Option<String> {
    let body = if text.starts_with('[') {
        let open = text[1..].find('[')? + 1;
        let close_len = open + 1;
        text.get(close_len..text.len().checked_sub(close_len)?)?
    } else if text.starts_with(['\'', '"']) && text.len() >= 2 {
        text.get(1..text.len() - 1)?
    } else {
        return None;
    };
    if body.contains('\\') {
        return None;
    }
    Some(body.trim_start_matches('\n').to_owned())
}
