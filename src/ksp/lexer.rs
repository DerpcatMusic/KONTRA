//! Byte-level KSP tokenizer. One pass, no per-token allocation: identifiers and
//! string literals are interned, reals live in a side table.

use anyhow::{Result, bail, ensure};
use std::collections::HashMap;

/// Largest accepted script. Local libraries top out near 19 MiB (Areia).
pub const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOKENS: usize = 32 * 1024 * 1024;

/// Interned identifier, variable name (with its type prefix) or string literal.
pub type Sym = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok {
    Ident(Sym),
    Var(Sym),
    Int(i32),
    Real(u32),
    Str(Sym),
    Punct(Punct),
    Newline,
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

/// Keywords are pre-interned so the parser compares integers, not text.
pub mod kw {
    use super::Sym;
    pub const ON: Sym = 0;
    pub const END: Sym = 1;
    pub const FUNCTION: Sym = 2;
    pub const IF: Sym = 3;
    pub const ELSE: Sym = 4;
    pub const WHILE: Sym = 5;
    pub const SELECT: Sym = 6;
    pub const CASE: Sym = 7;
    pub const TO: Sym = 8;
    pub const CALL: Sym = 9;
    pub const DECLARE: Sym = 10;
    pub const AND: Sym = 11;
    pub const OR: Sym = 12;
    pub const NOT: Sym = 13;
    pub const XOR: Sym = 14;
    pub const MOD: Sym = 15;
    pub const CONST: Sym = 16;
    pub const POLYPHONIC: Sym = 17;
    pub const PERS: Sym = 18;
    pub const INSTPERS: Sym = 19;
    pub const ALL: [&str; 20] = [
        "on",
        "end",
        "function",
        "if",
        "else",
        "while",
        "select",
        "case",
        "to",
        "call",
        "declare",
        "and",
        "or",
        "not",
        "xor",
        "mod",
        "const",
        "polyphonic",
        "pers",
        "instpers",
    ];
}

#[derive(Default)]
pub struct Interner {
    map: HashMap<Box<str>, Sym>,
    names: Vec<Box<str>>,
}

impl Interner {
    pub fn new() -> Self {
        let mut i = Self::default();
        for k in kw::ALL {
            i.intern(k);
        }
        i
    }

    pub fn intern(&mut self, s: &str) -> Sym {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }
        let sym = self.names.len() as Sym;
        self.names.push(s.into());
        self.map.insert(s.into(), sym);
        sym
    }

    pub fn name(&self, sym: Sym) -> &str {
        &self.names[sym as usize]
    }
}

pub struct Tokens {
    pub toks: Vec<Tok>,
    pub lines: Vec<u32>,
    pub reals: Vec<f64>,
    pub syms: Interner,
}

fn ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub fn lex(source: &str) -> Result<Tokens> {
    ensure!(
        source.len() <= MAX_SOURCE_BYTES,
        "KSP source exceeds {} MiB",
        MAX_SOURCE_BYTES >> 20
    );
    let s = source.as_bytes();
    let mut out = Tokens {
        toks: Vec::with_capacity(s.len() / 4),
        lines: Vec::with_capacity(s.len() / 4),
        reals: Vec::new(),
        syms: Interner::new(),
    };
    let mut line = 1u32;
    let mut continued = false;
    let mut i = 0;
    let push = |out: &mut Tokens, tok, line| {
        out.toks.push(tok);
        out.lines.push(line);
    };
    while i < s.len() {
        let b = s[i];
        let start = i;
        match b {
            b'\n' => {
                if !continued && out.toks.last().is_some_and(|t| *t != Tok::Newline) {
                    push(&mut out, Tok::Newline, line);
                }
                continued = false;
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => i += 1,
            b'{' => {
                let Some(len) = s[i..].iter().position(|&c| c == b'}') else {
                    bail!("Unterminated KSP comment at line {line}")
                };
                let newlines = s[i..i + len].iter().filter(|&&c| c == b'\n').count() as u32;
                if newlines > 0 && !continued && out.toks.last().is_some_and(|t| *t != Tok::Newline)
                {
                    push(&mut out, Tok::Newline, line);
                }
                line += newlines;
                i += len + 1;
            }
            b'"' => {
                let Some(len) = s[i + 1..].iter().position(|&c| c == b'"' || c == b'\n') else {
                    bail!("Unterminated KSP string at line {line}")
                };
                ensure!(
                    s[i + 1 + len] == b'"',
                    "Unterminated KSP string at line {line}"
                );
                let sym = out.syms.intern(&source[i + 1..i + 1 + len]);
                push(&mut out, Tok::Str(sym), line);
                i += len + 2;
            }
            b'.' if s[i..].starts_with(b"...") => {
                continued = true;
                i += 3;
            }
            b'.' => {
                let ops: [(&[u8], Punct); 4] = [
                    (b".and.", Punct::BitAnd),
                    (b".or.", Punct::BitOr),
                    (b".xor.", Punct::BitXor),
                    (b".not.", Punct::BitNot),
                ];
                let Some((text, p)) = ops.iter().find(|(t, _)| s[i..].starts_with(t)) else {
                    bail!("Unexpected '.' at line {line}")
                };
                push(&mut out, Tok::Punct(*p), line);
                i += text.len();
            }
            b'0'..=b'9' => {
                while i < s.len() && s[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let digits = &source[start..i];
                if i + 1 < s.len()
                    && s[i] == b'.'
                    && s[i + 1].is_ascii_digit()
                    && digits.bytes().all(|c| c.is_ascii_digit())
                {
                    i += 1;
                    while i < s.len() && s[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i < s.len() && matches!(s[i], b'e' | b'E') {
                        i += 1;
                        if i < s.len() && matches!(s[i], b'+' | b'-') {
                            i += 1;
                        }
                        while i < s.len() && s[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let n: f64 = source[start..i]
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Invalid real literal at line {line}"))?;
                    ensure!(n.is_finite(), "Nonfinite real literal at line {line}");
                    out.reals.push(n);
                    let real = out.reals.len() as u32 - 1;
                    push(&mut out, Tok::Real(real), line);
                } else {
                    let n = if let Some(hex) = digits.strip_suffix(['h', 'H']) {
                        u32::from_str_radix(hex, 16).ok().map(|n| n as i32)
                    } else {
                        digits.parse::<u32>().ok().map(|n| n as i32)
                    };
                    let Some(n) = n else {
                        bail!("Invalid integer literal {digits} at line {line}")
                    };
                    push(&mut out, Tok::Int(n), line);
                }
            }
            b'$' | b'%' | b'@' | b'~' | b'?' | b'!'
                if s.get(i + 1).is_some_and(|&c| ident_byte(c)) =>
            {
                i += 1;
                while i < s.len() && ident_byte(s[i]) {
                    i += 1;
                }
                let sym = out.syms.intern(&source[start..i]);
                push(&mut out, Tok::Var(sym), line);
            }
            _ if ident_byte(b) => {
                while i < s.len() && ident_byte(s[i]) {
                    i += 1;
                }
                let sym = out.syms.intern(&source[start..i]);
                push(&mut out, Tok::Ident(sym), line);
            }
            _ => {
                let two = s.get(i + 1).copied();
                let (p, len) = match (b, two) {
                    (b':', Some(b'=')) => (Punct::Assign, 2),
                    (b'<', Some(b'=')) => (Punct::Le, 2),
                    (b'>', Some(b'=')) => (Punct::Ge, 2),
                    (b'!', Some(b'=')) => (Punct::Ne, 2),
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
                    _ => bail!(
                        "Unexpected character {:?} at line {line}",
                        source[i..].chars().next().unwrap_or('?')
                    ),
                };
                push(&mut out, Tok::Punct(p), line);
                i += len;
            }
        }
        ensure!(out.toks.len() <= MAX_TOKENS, "KSP token limit exceeded");
    }
    push(&mut out, Tok::Newline, line);
    push(&mut out, Tok::Eof, line);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_continuations() {
        let t = lex("if ($a<=0FFh .and. 1.5e2) { c\n }...\n\"x y\" !s !=").unwrap();
        let kinds: Vec<_> = t.toks.iter().map(|t| format!("{t:?}")).collect();
        assert_eq!(t.toks[0], Tok::Ident(kw::IF));
        assert_eq!(t.toks[4], Tok::Int(255));
        assert_eq!(t.reals, [150.0]);
        assert!(
            kinds.iter().filter(|k| *k == "Newline").count() == 2,
            "{kinds:?}"
        );
        assert!(matches!(t.toks[t.toks.len() - 3], Tok::Punct(Punct::Ne)));
        assert!(lex("\"open").is_err());
        assert!(lex("{ open").is_err());
    }
}
