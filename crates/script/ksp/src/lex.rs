//! Tokens, then Kontakt's source-order condition preprocessor.

use crate::{Error, Span};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok<'a> {
    /// Keyword, builtin, function or callback name.
    Word(&'a str),
    /// Variable name with its type prefix (`$ % ~ ? @ !`).
    Var(&'a str),
    /// Integer magnitude; the sign is a separate token.
    Int(u64),
    Real(f64),
    Str(&'a str),
    P(P),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Amp,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
}

#[derive(Clone, Copy, Debug)]
pub struct Token<'a> {
    pub tok: Tok<'a>,
    pub span: Span,
}

fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Tokenizes `source`. Newlines carry no meaning; `...` continuations are skipped.
pub fn lex(source: &str) -> Result<Vec<Token<'_>>, Error> {
    if u32::try_from(source.len()).is_err() {
        return Err(Error::new(Span::default(), "source exceeds 4 GiB"));
    }
    let s = source.as_bytes();
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let text = |a: usize, b: usize| source.get(a..b).unwrap_or_default();
    let mut out = Vec::with_capacity(s.len() / 5);
    let mut i = 0;
    while i < s.len() {
        let start = i;
        let b = at(i);
        let tok = match b {
            _ if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'{' => {
                let Some(len) = s.get(i..).and_then(|r| r.iter().position(|&c| c == b'}')) else {
                    return Err(Error::new(Span::new(i, i + 1), "unterminated comment"));
                };
                i += len + 1;
                continue;
            }
            b'.' if s.get(i..).is_some_and(|r| r.starts_with(b"...")) => {
                i += 3;
                continue;
            }
            b'"' => {
                let end = s
                    .get(i + 1..)
                    .and_then(|r| r.iter().position(|&c| c == b'"' || c == b'\n'));
                let Some(len) = end.filter(|&len| at(i + 1 + len) == b'"') else {
                    return Err(Error::new(Span::new(i, i + 1), "unterminated string"));
                };
                i += len + 2;
                Tok::Str(text(start + 1, i - 1))
            }
            b'.' => {
                let ops = [
                    ("and.", P::BitAnd),
                    ("or.", P::BitOr),
                    ("xor.", P::BitXor),
                    ("not.", P::BitNot),
                ];
                let rest = s.get(i + 1..).unwrap_or_default();
                let Some(&(word, p)) = ops.iter().find(|(w, _)| rest.starts_with(w.as_bytes()))
                else {
                    return Err(Error::new(Span::new(i, i + 1), "unexpected '.'"));
                };
                i += 1 + word.len();
                Tok::P(p)
            }
            b'0'..=b'9' => {
                while word_byte(at(i)) {
                    i += 1;
                }
                let digits = text(start, i);
                let decimal = digits.bytes().all(|c| c.is_ascii_digit());
                if decimal && at(i) == b'.' && at(i + 1).is_ascii_digit() {
                    i += 1;
                    while at(i).is_ascii_digit() {
                        i += 1;
                    }
                    if matches!(at(i), b'e' | b'E') {
                        i += 1;
                        if matches!(at(i), b'+' | b'-') {
                            i += 1;
                        }
                        while at(i).is_ascii_digit() {
                            i += 1;
                        }
                    }
                    match text(start, i).parse::<f64>() {
                        Ok(n) if n.is_finite() => Tok::Real(n),
                        _ => return Err(Error::new(Span::new(start, i), "invalid real")),
                    }
                } else if let Some(hex) = digits.strip_suffix(['h', 'H']) {
                    match u32::from_str_radix(hex, 16) {
                        Ok(n) => Tok::Int(u64::from(n)),
                        Err(_) => return Err(Error::new(Span::new(start, i), "invalid hex")),
                    }
                } else {
                    match digits.parse::<u64>() {
                        Ok(n) => Tok::Int(n),
                        Err(_) => return Err(Error::new(Span::new(start, i), "invalid integer")),
                    }
                }
            }
            b'$' | b'%' | b'@' | b'~' | b'?' | b'!' if word_byte(at(i + 1)) => {
                i += 1;
                while word_byte(at(i)) {
                    i += 1;
                }
                Tok::Var(text(start, i))
            }
            _ if word_byte(b) => {
                while word_byte(at(i)) {
                    i += 1;
                }
                Tok::Word(text(start, i))
            }
            _ => {
                let (p, len) = match (b, at(i + 1)) {
                    (b':', b'=') => (P::Assign, 2),
                    (b'<', b'=') => (P::Le, 2),
                    (b'>', b'=') => (P::Ge, 2),
                    (b'(', _) => (P::LParen, 1),
                    (b')', _) => (P::RParen, 1),
                    (b'[', _) => (P::LBracket, 1),
                    (b']', _) => (P::RBracket, 1),
                    (b',', _) => (P::Comma, 1),
                    (b'+', _) => (P::Plus, 1),
                    (b'-', _) => (P::Minus, 1),
                    (b'*', _) => (P::Star, 1),
                    (b'/', _) => (P::Slash, 1),
                    (b'&', _) => (P::Amp, 1),
                    (b'=', _) => (P::Eq, 1),
                    (b'#', _) => (P::Ne, 1),
                    (b'<', _) => (P::Lt, 1),
                    (b'>', _) => (P::Gt, 1),
                    _ => {
                        let width = source.get(i..).and_then(|r| r.chars().next());
                        let end = i + width.map_or(1, char::len_utf8);
                        return Err(Error::new(Span::new(i, end), "unexpected character"));
                    }
                };
                i += len;
                Tok::P(p)
            }
        };
        out.push(Token {
            tok,
            span: Span::new(start, i),
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span::new(s.len(), s.len()),
    });
    Ok(out)
}

/// Applies `SET_CONDITION`, `RESET_CONDITION`, `USE_CODE_IF(_NOT)` and
/// `END_USE_CODE` in source order (they ignore callback structure), starting
/// from `conditions`. Returns the final condition set.
pub fn preprocess(tokens: &mut Vec<Token<'_>>, conditions: &mut Vec<String>) -> Result<(), Error> {
    let mut regions: Vec<bool> = Vec::new();
    let mut kept = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while let Some(&token) = tokens.get(i) {
        let active = regions.last().copied().unwrap_or(true);
        let name = match token.tok {
            Tok::Word(
                w @ ("SET_CONDITION" | "RESET_CONDITION" | "USE_CODE_IF" | "USE_CODE_IF_NOT"
                | "END_USE_CODE"),
            ) => w,
            _ => {
                if active || token.tok == Tok::Eof {
                    kept.push(token);
                }
                i += 1;
                continue;
            }
        };
        if name == "END_USE_CODE" {
            if regions.pop().is_none() {
                return Err(Error::new(token.span, "END_USE_CODE without USE_CODE_IF"));
            }
            i += 1;
            continue;
        }
        let symbol = match tokens.get(i + 1..i + 4) {
            Some(
                [
                    Token {
                        tok: Tok::P(P::LParen),
                        ..
                    },
                    Token {
                        tok: Tok::Word(symbol),
                        ..
                    },
                    Token {
                        tok: Tok::P(P::RParen),
                        ..
                    },
                ],
            ) => *symbol,
            _ => return Err(Error::new(token.span, format!("{name} needs (SYMBOL)"))),
        };
        let set = conditions.iter().any(|c| c == symbol);
        match name {
            "SET_CONDITION" if active && !set => conditions.push(symbol.to_owned()),
            "RESET_CONDITION" if active => conditions.retain(|c| c != symbol),
            "USE_CODE_IF" => regions.push(active && set),
            "USE_CODE_IF_NOT" => regions.push(active && !set),
            _ => {}
        }
        i += 4;
    }
    if !regions.is_empty() {
        return Err(Error::new(
            Span::default(),
            "USE_CODE_IF without END_USE_CODE",
        ));
    }
    *tokens = kept;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(source: &str) -> Vec<Tok<'_>> {
        lex(source).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn literals_operators_comments_and_continuations() {
        let t = toks("if ($a<=0FFh .and. 1.5e2) { c\n }...\n\"x y\" !s # 7");
        assert_eq!(t[3], Tok::P(P::Le));
        assert_eq!(t[4], Tok::Int(255));
        assert_eq!(t[5], Tok::P(P::BitAnd));
        assert_eq!(t[6], Tok::Real(150.0));
        assert_eq!(t[8], Tok::Str("x y"));
        assert_eq!(t[9], Tok::Var("!s"));
        assert_eq!(t[10], Tok::P(P::Ne));
        assert_eq!(t[11], Tok::Int(7));
        assert!(lex("\"open").is_err());
        assert!(lex("{ open").is_err());
        assert!(lex("1FFFFFFFFh").is_err());
        assert!(lex("1 != 2").is_err());
    }

    #[test]
    fn conditions_apply_in_source_order() {
        let source = "SET_CONDITION(A) USE_CODE_IF(A) x END_USE_CODE USE_CODE_IF_NOT(A) y \
                      END_USE_CODE RESET_CONDITION(A) USE_CODE_IF(A) z END_USE_CODE";
        let mut tokens = lex(source).unwrap();
        let mut conditions = Vec::new();
        preprocess(&mut tokens, &mut conditions).unwrap();
        let words: Vec<_> = tokens.iter().map(|t| t.tok).collect();
        assert_eq!(words, [Tok::Word("x"), Tok::Eof]);
        assert!(conditions.is_empty());
    }
}
