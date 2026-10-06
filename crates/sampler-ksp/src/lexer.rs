//! Byte-level KSP tokenizer with spans, followed by Kontakt's source-order
//! condition preprocessor. Identifiers and strings are interned slices of the
//! source; nothing is allocated per token.
use crate::diag::{Result, Span, fault};
use std::collections::{BTreeSet, HashMap};

pub type Sym = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok {
    /// Bare word: keyword, builtin, function, callback or PGS key name.
    Ident(Sym),
    /// Typed variable name including its prefix (`$ % @ ~ ? !`).
    Var(Sym),
    /// Decimal (`false`) or hexadecimal (`true`) literal magnitude.
    Int(u64, bool),
    Real(f64),
    Str(Sym),
    Punct(Punct),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Punct {
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
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

#[derive(Default)]
pub struct Interner<'a> {
    map: HashMap<&'a str, Sym>,
    names: Vec<&'a str>,
}
impl<'a> Interner<'a> {
    pub fn intern(&mut self, s: &'a str) -> Sym {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }
        let sym = self.names.len() as Sym;
        self.names.push(s);
        self.map.insert(s, sym);
        sym
    }
    pub fn get(&self, s: &str) -> Option<Sym> {
        self.map.get(s).copied()
    }
    pub fn name(&self, sym: Sym) -> &'a str {
        self.names[sym as usize]
    }
}

fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Tokenize the whole source. Newlines are not tokens: KSP statements are
/// unambiguous without them, and `...` continuations are simply skipped.
pub fn lex<'a>(source: &'a str, syms: &mut Interner<'a>) -> Result<Vec<Token>> {
    if source.len() > u32::MAX as usize {
        return fault(Span::default(), "source exceeds 4 GiB");
    }
    let s = source.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 5);
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        let start = i;
        let tok = match b {
            _ if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'{' => {
                let Some(len) = s[i..].iter().position(|&c| c == b'}') else {
                    return fault(Span::new(i, i + 1), "unterminated comment");
                };
                i += len + 1;
                continue;
            }
            b'.' if s[i..].starts_with(b"...") => {
                i += 3;
                continue;
            }
            b'"' => {
                let Some(len) = s[i + 1..].iter().position(|&c| c == b'"' || c == b'\n') else {
                    return fault(Span::new(i, i + 1), "unterminated string literal");
                };
                if s[i + 1 + len] != b'"' {
                    return fault(Span::new(i, i + 1), "unterminated string literal");
                }
                i += len + 2;
                Tok::Str(syms.intern(&source[start + 1..i - 1]))
            }
            b'.' => {
                let ops: [(&[u8], Punct); 4] = [
                    (b".and.", Punct::BitAnd),
                    (b".or.", Punct::BitOr),
                    (b".xor.", Punct::BitXor),
                    (b".not.", Punct::BitNot),
                ];
                let Some(&(text, p)) = ops.iter().find(|(t, _)| s[i..].starts_with(t)) else {
                    return fault(Span::new(i, i + 1), "unexpected '.'");
                };
                i += text.len();
                Tok::Punct(p)
            }
            b'0'..=b'9' => {
                while i < s.len() && s[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let digits = &source[start..i];
                if s.get(i) == Some(&b'.')
                    && s.get(i + 1).is_some_and(u8::is_ascii_digit)
                    && digits.bytes().all(|c| c.is_ascii_digit())
                {
                    i += 1;
                    while s.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    if matches!(s.get(i), Some(b'e' | b'E')) {
                        i += 1;
                        if matches!(s.get(i), Some(b'+' | b'-')) {
                            i += 1;
                        }
                        while s.get(i).is_some_and(u8::is_ascii_digit) {
                            i += 1;
                        }
                    }
                    match source[start..i].parse::<f64>() {
                        Ok(n) if n.is_finite() => Tok::Real(n),
                        _ => return fault(Span::new(start, i), "invalid real literal"),
                    }
                } else if let Some(hex) = digits.strip_suffix(['h', 'H']) {
                    match u32::from_str_radix(hex, 16) {
                        Ok(n) => Tok::Int(u64::from(n), true),
                        Err(_) => {
                            return fault(
                                Span::new(start, i),
                                "hexadecimal literal exceeds 32-bit range",
                            );
                        }
                    }
                } else {
                    match digits.parse::<u64>() {
                        Ok(n) => Tok::Int(n, false),
                        Err(_) => return fault(Span::new(start, i), "invalid integer literal"),
                    }
                }
            }
            b'$' | b'%' | b'@' | b'~' | b'?' | b'!'
                if s.get(i + 1).is_some_and(|&c| ident_byte(c)) =>
            {
                i += 1;
                while i < s.len() && ident_byte(s[i]) {
                    i += 1;
                }
                Tok::Var(syms.intern(&source[start..i]))
            }
            _ if ident_byte(b) => {
                while i < s.len() && ident_byte(s[i]) {
                    i += 1;
                }
                Tok::Ident(syms.intern(&source[start..i]))
            }
            _ => {
                let (p, len) = match (b, s.get(i + 1).copied()) {
                    (b':', Some(b'=')) => (Punct::Assign, 2),
                    (b'<', Some(b'=')) => (Punct::Le, 2),
                    (b'>', Some(b'=')) => (Punct::Ge, 2),
                    (b'(', _) => (Punct::LParen, 1),
                    (b')', _) => (Punct::RParen, 1),
                    (b'[', _) => (Punct::LBracket, 1),
                    (b']', _) => (Punct::RBracket, 1),
                    (b',', _) => (Punct::Comma, 1),
                    (b'+', _) => (Punct::Plus, 1),
                    (b'-', _) => (Punct::Minus, 1),
                    (b'*', _) => (Punct::Star, 1),
                    (b'/', _) => (Punct::Slash, 1),
                    (b'&', _) => (Punct::Amp, 1),
                    (b'=', _) => (Punct::Eq, 1),
                    (b'#', _) => (Punct::Ne, 1),
                    (b'<', _) => (Punct::Lt, 1),
                    (b'>', _) => (Punct::Gt, 1),
                    _ => {
                        let width = source[i..].chars().next().map_or(1, char::len_utf8);
                        return fault(Span::new(i, i + width), "unexpected character");
                    }
                };
                i += len;
                Tok::Punct(p)
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

/// Resolve `SET_CONDITION`/`RESET_CONDITION`/`USE_CODE_IF(_NOT)`/`END_USE_CODE`
/// in source order, including inside callbacks that never run. Returns the
/// final condition set (Kontakt's system-script switches live here).
pub fn preprocess(
    tokens: &mut Vec<Token>,
    syms: &Interner<'_>,
    inherited: &BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let mut conditions = inherited.clone();
    let mut regions: Vec<bool> = Vec::new();
    let (mut read, mut write) = (0, 0);
    while read < tokens.len() {
        let active = regions.last().copied().unwrap_or(true);
        let name = match tokens[read].tok {
            Tok::Ident(s) => syms.name(s),
            _ => "",
        };
        let span = tokens[read].span;
        if matches!(
            name,
            "SET_CONDITION"
                | "RESET_CONDITION"
                | "USE_CODE_IF"
                | "USE_CODE_IF_NOT"
                | "END_USE_CODE"
        ) {
            if name == "END_USE_CODE" {
                if regions.pop().is_none() {
                    return fault(span, "END_USE_CODE without USE_CODE_IF");
                }
                read += 1;
                continue;
            }
            let Some(
                [
                    Token {
                        tok: Tok::Punct(Punct::LParen),
                        ..
                    },
                    Token {
                        tok: Tok::Ident(symbol),
                        ..
                    },
                    Token {
                        tok: Tok::Punct(Punct::RParen),
                        ..
                    },
                ],
            ) = tokens.get(read + 1..read + 4)
            else {
                return fault(span, format!("{name} requires a condition symbol"));
            };
            let symbol = syms.name(*symbol);
            match name {
                "SET_CONDITION" if active => {
                    conditions.insert(symbol.to_owned());
                }
                "RESET_CONDITION" if active => {
                    conditions.remove(symbol);
                }
                "USE_CODE_IF" | "USE_CODE_IF_NOT" => {
                    if regions.len() == 256 {
                        return fault(span, "preprocessor nesting limit");
                    }
                    regions.push(active && conditions.contains(symbol) == (name == "USE_CODE_IF"));
                }
                _ => {}
            }
            read += 4;
            continue;
        }
        if active || tokens[read].tok == Tok::Eof {
            tokens[write] = tokens[read];
            write += 1;
        }
        read += 1;
    }
    if !regions.is_empty() {
        return fault(tokens[write - 1].span, "unclosed USE_CODE_IF region");
    }
    tokens.truncate(write);
    Ok(conditions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<Tok> {
        let mut syms = Interner::default();
        lex(source, &mut syms)
            .unwrap()
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    #[test]
    fn literals_operators_comments_and_continuations() {
        let toks = kinds("if ($a<=0FFh .and. 1.5e2) { c\n }...\n\"x y\" !s # 7");
        assert!(matches!(toks[4], Tok::Int(255, true)));
        assert!(matches!(toks[6], Tok::Real(r) if r == 150.0));
        assert!(matches!(toks[8], Tok::Str(_)));
        assert!(matches!(toks[10], Tok::Punct(Punct::Ne)));
        assert!(matches!(toks[11], Tok::Int(7, false)));
        let mut syms = Interner::default();
        assert!(lex("\"open", &mut syms).is_err());
        assert!(lex("{ open", &mut syms).is_err());
        assert!(lex("0FFFFFFFFFh", &mut syms).is_err());
        assert!(lex("1 != 2", &mut syms).is_err());
    }

    #[test]
    fn conditions_are_resolved_in_source_order() {
        let source = "SET_CONDITION(A) USE_CODE_IF(A) x END_USE_CODE USE_CODE_IF_NOT(A) y END_USE_CODE RESET_CONDITION(A) USE_CODE_IF(A) z END_USE_CODE";
        let mut syms = Interner::default();
        let mut tokens = lex(source, &mut syms).unwrap();
        let conditions = preprocess(&mut tokens, &syms, &BTreeSet::new()).unwrap();
        let names: Vec<_> = tokens
            .iter()
            .filter_map(|t| match t.tok {
                Tok::Ident(s) => Some(syms.name(s)),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["x"]);
        assert!(conditions.is_empty());
    }
}
