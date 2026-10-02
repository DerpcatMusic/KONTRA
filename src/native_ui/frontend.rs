//! Strict lowering of the proven legacy component syntax. Lua remains the VM.
use anyhow::{Result, bail, ensure};

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    line: usize,
}
struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
    nodes: usize,
    depth: usize,
}

fn ident(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn tokenize(source: &str) -> Result<Vec<Token<'_>>> {
    ensure!(
        source.len() <= 1024 * 1024,
        "Native UI module exceeds 1 MiB"
    );
    let bytes = source.as_bytes();
    let (mut at, mut line, mut out) = (0, 1, Vec::new());
    while at < bytes.len() {
        let start = at;
        let comment = source[at..].starts_with("--");
        let bracket = at + if comment { 2 } else { 0 };
        let long = bytes.get(bracket) == Some(&b'[');
        let mut equals = bracket + 1;
        if long {
            while bytes.get(equals) == Some(&b'=') {
                equals += 1;
            }
        }
        if long && bytes.get(equals) == Some(&b'[') {
            let end = format!("]{}]", "=".repeat(equals - bracket - 1));
            at = source[equals + 1..]
                .find(&end)
                .map(|n| equals + 1 + n + end.len())
                .ok_or_else(|| {
                    anyhow::anyhow!("Native UI line {line}: unclosed long string/comment")
                })?;
        } else if comment {
            at = source[at..].find('\n').map_or(bytes.len(), |n| at + n);
        } else if matches!(bytes[at], b'\'' | b'"') {
            let quote = bytes[at];
            at += 1;
            while at < bytes.len() && bytes[at] != quote {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            ensure!(at < bytes.len(), "Native UI line {line}: unclosed string");
            at += 1;
        } else if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' {
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
        } else if bytes[at].is_ascii_whitespace() && bytes[at] != b'\n' {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() && bytes[at] != b'\n' {
                at += 1;
            }
        } else {
            at += source[at..].chars().next().unwrap().len_utf8();
        }
        let text = &source[start..at];
        out.push(Token { text, line });
        line += text.bytes().filter(|&b| b == b'\n').count();
    }
    Ok(out)
}
impl Parser<'_> {
    fn token(&self) -> &str {
        self.tokens.get(self.at).map_or("", |t| t.text)
    }
    fn skip(&mut self) {
        while self.at < self.tokens.len()
            && (self.token().trim().is_empty() || self.token().starts_with("--"))
        {
            self.at += 1;
        }
    }
    fn require(&mut self, expected: &str) -> Result<()> {
        self.skip();
        ensure!(
            self.token() == expected,
            "Native UI line {}: expected {expected:?}",
            self.tokens.get(self.at).map_or(0, |t| t.line)
        );
        self.at += 1;
        Ok(())
    }
    fn expression(&mut self, closing: &str) -> Result<String> {
        let (mut out, mut delimiters, mut blocks) = (String::new(), Vec::new(), 0i32);
        while self.at < self.tokens.len() {
            let t = self.token().to_owned();
            if delimiters.is_empty()
                && blocks == 0
                && (t == closing || matches!(t.as_str(), "," | ";" | "\n"))
            {
                break;
            }
            if t == "@" {
                out.push_str(&self.component()?);
                continue;
            }
            if t.starts_with("--") {
                out.push_str(&t);
                self.at += 1;
                // A short comment must end before generated closing syntax.
                if self.token() == "\n" {
                    out.push('\n');
                    self.at += 1;
                }
                if delimiters.is_empty() && blocks == 0 {
                    break;
                }
                continue;
            }
            match t.as_str() {
                "(" => delimiters.push(")"),
                "{" => delimiters.push("}"),
                "[" => delimiters.push("]"),
                ")" | "}" | "]" => ensure!(
                    delimiters.pop() == Some(t.as_str()),
                    "Unbalanced native UI expression"
                ),
                "function" | "if" | "do" | "repeat" => blocks += 1,
                "end" | "until" => {
                    blocks -= 1;
                    ensure!(blocks >= 0, "Unexpected native UI block ending");
                }
                _ => (),
            }
            out.push_str(&t);
            self.at += 1;
        }
        ensure!(
            delimiters.is_empty() && blocks == 0 && !out.trim().is_empty(),
            "Incomplete native UI component expression"
        );
        Ok(out)
    }
    fn fields(&mut self, closing: &str, children: bool) -> Result<(String, String)> {
        let (mut props, mut content, mut seen) =
            (Vec::new(), Vec::new(), std::collections::BTreeSet::new());
        loop {
            self.skip();
            ensure!(self.at < self.tokens.len(), "Unclosed native UI component");
            if self.token() == closing {
                self.at += 1;
                return Ok((props.join(","), content.join(",")));
            }
            if matches!(self.token(), "," | ";") {
                self.at += 1;
                continue;
            }
            let start = self.at;
            let name = self.token().to_owned();
            self.at += 1;
            self.skip();
            if ident(&name) && self.token() == "=" {
                ensure!(
                    seen.insert(name.clone()),
                    "Duplicate native UI property {name}"
                );
                self.at += 1;
                props.push(format!("{name}={}", self.expression(closing)?));
            } else if children {
                self.at = start;
                content.push(self.expression(closing)?);
            } else {
                bail!("Native UI modifiers require named arguments");
            }
        }
    }
    fn component(&mut self) -> Result<String> {
        let start = self.at;
        self.depth += 1;
        ensure!(self.depth <= 64, "Native UI component nesting exceeds 64");
        self.require("@")?;
        self.skip();
        let mut name = "native_ui_host.ZStack".to_owned();
        if self.token() != "{" {
            name = self.token().to_owned();
            ensure!(ident(&name), "Invalid native UI component name");
            self.at += 1;
            while self.token() == "." {
                self.at += 1;
                ensure!(ident(self.token()), "Incomplete native UI component name");
                name.push('.');
                name.push_str(self.token());
                self.at += 1;
            }
        }
        self.require("{")?;
        let (props, children) = self.fields("}", true)?;
        self.nodes += 1;
        ensure!(self.nodes <= 4096, "Native UI component count exceeds 4096");
        let mut out = format!("native_component({name},{{{props}}},{{{children}}})");
        loop {
            let checkpoint = self.at;
            self.skip();
            if self.token() != "." {
                self.at = checkpoint;
                break;
            }
            self.at += 1;
            let modifier = self.token().to_owned();
            ensure!(ident(&modifier), "Incomplete native UI modifier name");
            self.at += 1;
            self.require("(")?;
            let (props, arguments) = self.fields(")", true)?;
            out = format!("native_modifier({out},\"{modifier}\",{{{props}}},{{{arguments}}})");
        }
        // Keep later Lua locations on their original resource lines. Locations
        // inside reordered property/child tables are explicitly named lowered.lua.
        let original_lines = self.tokens[start..self.at]
            .iter()
            .map(|t| t.text.bytes().filter(|&b| b == b'\n').count())
            .sum::<usize>();
        let lowered_lines = out.bytes().filter(|&b| b == b'\n').count();
        out.extend(std::iter::repeat_n(
            '\n',
            original_lines.saturating_sub(lowered_lines),
        ));
        self.depth -= 1;
        Ok(out)
    }
}
pub(super) fn lower(source: &str) -> Result<String> {
    let mut p = Parser {
        tokens: tokenize(source)?,
        at: 0,
        nodes: 0,
        depth: 0,
    };
    let mut out = String::new();
    while p.at < p.tokens.len() {
        if p.token() == "@" {
            out.push_str(&p.component()?);
        } else {
            out.push_str(p.token());
            p.at += 1;
        }
    }
    Ok(out)
}
