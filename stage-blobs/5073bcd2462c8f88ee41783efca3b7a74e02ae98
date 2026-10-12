//! Translate the legacy NativeUI additions to Lua while preserving Lua strings/comments.
//! Node bodies remain closures: properties are evaluated in their component context.
use anyhow::{Result, bail, ensure};

#[derive(Clone)]
struct Token {
    text: String,
    line: usize,
}

fn tokens(source: &str) -> Result<Vec<Token>> {
    let b = source.as_bytes();
    let (mut at, mut line, mut out) = (0, 1, Vec::new());
    while at < b.len() {
        if b[at].is_ascii_whitespace() {
            if b[at] == b'\n' {
                line += 1;
            }
            at += 1;
            continue;
        }
        let start = at;
        let first_line = line;
        let comment = b[at..].starts_with(b"--");
        if comment {
            at += 2;
        }
        // Lua long strings and long comments support arbitrary '=' delimiters.
        let long = if b.get(at) == Some(&b'[') {
            let mut end = at + 1;
            while b.get(end) == Some(&b'=') {
                end += 1;
            }
            (b.get(end) == Some(&b'[')).then_some(end - at - 1)
        } else {
            None
        };
        if let Some(level) = long {
            let closing = format!("]{}]", "=".repeat(level));
            at += level + 2;
            let Some(end) = source[at..].find(&closing) else {
                bail!("Unclosed long string at line {line}")
            };
            at += end + closing.len();
        } else if comment {
            while at < b.len() && b[at] != b'\n' {
                at += 1;
            }
        } else if matches!(b[at], b'\'' | b'"') {
            let quote = b[at];
            at += 1;
            while at < b.len() {
                if b[at] == b'\\' {
                    at += 2;
                } else if b[at] == quote {
                    at += 1;
                    break;
                } else {
                    at += 1;
                }
            }
            ensure!(
                at <= b.len() && b[at - 1] == quote,
                "Unclosed string at line {line}"
            );
        } else if b[at].is_ascii_alphabetic() || b[at] == b'_' {
            at += 1;
            while at < b.len() && (b[at].is_ascii_alphanumeric() || b[at] == b'_') {
                at += 1;
            }
        } else if b[at].is_ascii_digit() {
            at += 1;
            while at < b.len() && (b[at].is_ascii_alphanumeric() || b[at] == b'.') {
                at += 1;
            }
        } else {
            at += source[at..].chars().next().unwrap().len_utf8();
            if at < b.len()
                && b[at].is_ascii()
                && matches!(
                    &source[start..at + 1],
                    "==" | "~=" | "<=" | ">=" | ".." | "//" | "<<" | ">>"
                )
            {
                at += 1;
                if &source[start..at] == ".." && b.get(at) == Some(&b'.') {
                    at += 1;
                }
            }
        }
        line += source[start..at].bytes().filter(|&c| c == b'\n').count();
        if !comment {
            out.push(Token {
                text: source[start..at].into(),
                line: first_line,
            });
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}
impl Parser {
    fn peek(&self, n: usize) -> &str {
        self.tokens.get(self.at + n).map_or("", |t| &t.text)
    }
    fn take(&mut self) -> String {
        let t = self.peek(0).to_owned();
        self.at += 1;
        t
    }
    fn node(&mut self) -> Result<String> {
        self.at += 1; // '@'
        if self.peek(0) == "{" {
            self.at += 1;
            let body = self.body("}", false)?;
            return Ok(format!("__dynamic(function() return {body} end)"));
        }
        let mut name = self.take();
        while self.peek(0) == "." {
            name.push_str(&self.take());
            name.push_str(&self.take());
        }
        ensure!(
            self.take() == "{",
            "Expected NativeUI node body after {name}"
        );
        let body = self.body("}", true)?;
        Ok(format!("__node({name}, function() return {{{body}}} end)"))
    }
    fn body(&mut self, closing: &str, fields: bool) -> Result<String> {
        let (mut out, mut block, mut previous, mut previous_line) = (
            String::new(),
            0usize,
            String::new(),
            self.tokens.get(self.at).map_or(0, |t| t.line),
        );
        while self.at < self.tokens.len() {
            let text = self.peek(0).to_owned();
            let line = self.tokens[self.at].line;
            if text == closing && !closing.is_empty() {
                self.at += 1;
                return Ok(out);
            }
            let field = text == "@"
                || (self.peek(1) == "="
                    && text
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_'));
            let complete = !previous.is_empty()
                && !matches!(
                    previous.as_str(),
                    "," | ";"
                        | "="
                        | "and"
                        | "or"
                        | "not"
                        | "return"
                        | "+"
                        | "-"
                        | "*"
                        | "/"
                        | "^"
                        | ".."
                        | "~="
                        | "=="
                        | "<"
                        | ">"
                        | "<="
                        | ">="
                );
            if fields && block == 0 && field && complete {
                out.push_str(", ");
            }
            if line > previous_line {
                out.push_str(&"\n".repeat(line - previous_line));
            } else {
                out.push(' ');
            }
            let emitted = if text == "@" {
                self.node()?
            } else if text == "." && modifier(self.peek(1)) && self.peek(2) == "(" {
                self.at += 1;
                let method = self.take();
                self.at += 1;
                let mut named = false;
                let mut nesting = 0usize;
                for token in &self.tokens[self.at..] {
                    match token.text.as_str() {
                        "(" | "{" | "[" => nesting += 1,
                        ")" if nesting == 0 => break,
                        ")" | "}" | "]" => nesting = nesting.saturating_sub(1),
                        "=" if nesting == 0 => named = true,
                        _ => {}
                    }
                }
                let args = self.body(")", named)?;
                if named {
                    format!(":{method}({{{args}}})")
                } else {
                    format!(":{method}({args})")
                }
            } else if matches!(text.as_str(), "{" | "(" | "[") {
                self.at += 1;
                let end = match text.as_str() {
                    "{" => "}",
                    "(" => ")",
                    _ => "]",
                };
                let body = self.body(end, text == "{")?;
                format!("{text}{body}{end}")
            } else {
                self.at += 1;
                match text.as_str() {
                    "function" | "if" | "for" | "while" | "repeat" => block += 1,
                    "end" | "until" => block = block.saturating_sub(1),
                    _ => {}
                }
                text.clone()
            };
            out.push_str(&emitted);
            previous = if emitted.starts_with("__")
                || emitted.ends_with(')')
                || emitted.ends_with('}')
                || emitted.ends_with(']')
            {
                "value".into()
            } else {
                text
            };
            previous_line = self
                .tokens
                .get(self.at.saturating_sub(1))
                .map_or(line, |t| t.line);
        }
        ensure!(closing.is_empty(), "Unclosed NativeUI {closing}");
        Ok(out)
    }
}
fn modifier(name: &str) -> bool {
    matches!(
        name,
        "position"
            | "frame"
            | "offset"
            | "align"
            | "padding"
            | "hidden"
            | "opacity"
            | "background"
            | "overlay"
            | "foreground_color"
            | "font_size"
            | "font"
            | "font_family"
            | "rotation"
            | "disabled"
            | "bold"
            | "multiline_text_alignment"
            | "line_limit"
            | "context"
            | "on_hover_gesture"
            | "on_tap_gesture"
            | "on_drag_gesture"
            | "on_drag"
            | "on_drop"
            | "popover"
    )
}
pub fn translate(source: &str) -> Result<String> {
    ensure!(source.len() <= 1 << 20, "NativeUI module exceeds 1 MiB");
    let tokens = tokens(source)?;
    let mut nesting = 0usize;
    for token in &tokens {
        match token.text.as_str() {
            "(" | "{" | "[" => {
                nesting += 1;
                ensure!(nesting <= 192, "NativeUI syntax nesting exceeds 192");
            }
            ")" | "}" | "]" => nesting = nesting.saturating_sub(1),
            _ => {}
        }
    }
    Parser { tokens, at: 0 }.body("", false)
}
