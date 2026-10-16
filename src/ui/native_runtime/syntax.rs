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
        } else if b[at].is_ascii_digit()
            || (b[at] == b'.' && b.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            // Keep the source numeral intact. Unary signs remain operators;
            // only the exponent sign belongs to the numeral. A hex digit 'e'
            // is not an exponent marker (0x1e-2 is subtraction).
            let exponent = if b[start..].starts_with(b"0x") || b[start..].starts_with(b"0X") {
                b'p'
            } else {
                b'e'
            };
            at += 1;
            while at < b.len()
                && (b[at].is_ascii_alphanumeric()
                    || b[at] == b'.'
                    || (matches!(b[at], b'+' | b'-')
                        && b[at - 1].eq_ignore_ascii_case(&exponent)))
            {
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


#[cfg(test)]
mod numeral_tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn leading_dot_and_exponent_numbers_match_untranslated_lua_bits() {
        let lua = Lua::new();
        lua.enable_jit(false);
        for expression in [
            ".5", "-.5", "- .5", ".5+.25", "1-.5", "1- .5", ".5*.25",
            ".5/.25", ".5e-2", ".5E+2", "1e-2+.5", "0x1e-2",
        ] {
            let source = format!("return {expression}");
            let expected = lua.load(&source).eval::<f64>().unwrap();
            let translated = translate(&source).unwrap();
            let actual = lua.load(&translated).eval::<f64>().unwrap();
            assert_eq!(actual.to_bits(), expected.to_bits(), "{expression}");
        }
    }

    #[test]
    fn numeral_signs_do_not_swallow_operators_or_concat_tokens() {
        let spelling = |source| tokens(source).unwrap().into_iter().map(|t| t.text).collect::<Vec<_>>();
        assert_eq!(spelling("- .5 + .25"), ["-", ".5", "+", ".25"]);
        assert_eq!(spelling(".5e-2+.25"), [".5e-2", "+", ".25"]);
        assert_eq!(spelling("0x1e-2"), ["0x1e", "-", "2"]);
        assert_eq!(spelling("0x1p-2"), ["0x1p-2"]);
        assert_eq!(spelling("a..5"), ["a", "..", "5"]);
        assert_eq!(spelling("..."), ["..."]);
        let lua = Lua::new();
        for source in ["return .5 .. .25", "return 'value='.. .5", "local a='v' return a..5"] {
            let expected = lua.load(source).eval::<String>().unwrap();
            let actual = lua.load(translate(source).unwrap()).eval::<String>().unwrap();
            assert_eq!(actual, expected, "{source}");
        }
    }

    #[test]
    fn malformed_numerals_stay_invalid_and_quoted_text_stays_opaque() {
        let lua = Lua::new();
        for source in ["return .5e+", "return .5.25", "return .e2", "local a={} return a.5"] {
            assert!(lua.load(source).eval::<f64>().is_err(), "native invalid fixture: {source}");
            assert!(lua.load(translate(source).unwrap()).eval::<f64>().is_err(), "must not repair: {source}");
        }
        // Preserve the actual configured Lua grammar at an ambiguous numeral/
        // concat boundary rather than assuming another Lua version's policy.
        let original = lua.load("return 1..2").eval::<String>();
        let rewritten = lua.load(translate("return 1..2").unwrap()).eval::<String>();
        match (original, rewritten) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            (Err(_), Err(_)) => {},
            _ => panic!("translation changed configured Lua concat admission"),
        }
        let source = "return '.5e-2', '--.5', [[.5]] -- .25e-3";
        let expected = lua.load(source).eval::<(String,String,String)>().unwrap();
        let actual = lua.load(translate(source).unwrap()).eval::<(String,String,String)>().unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn actual_native_ui_props_and_modifiers_keep_leading_dot_values() {
        use super::super::{Package, Session};
        use std::sync::Arc;
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("Samples")).unwrap();
        let resources = directory.path().join("Resources/native_ui");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("main.nui"), r#"
            local ui=require('native_ui')
            return function()
                return @ui.Rectangle {color=ui.Color(80,120,160)}
                    .frame(width=16+.5,height=32-.5).opacity(.5)
            end
        "#).unwrap();
        let package = Arc::new(Package::load(&directory.path().join("authored.nki")).unwrap());
        let session = Session::new(package, "main", vec![]).unwrap();
        let graph = session.render().unwrap();
        let modifiers = graph.get::<mlua::Table>("modifiers").unwrap();
        let mut seen = 0;
        for modifier in modifiers.sequence_values::<mlua::Table>() {
            let modifier = modifier.unwrap();
            match modifier.get::<String>("name").unwrap().as_str() {
                "frame" => {
                    let frame = modifier.get::<mlua::Table>("value").unwrap();
                    assert_eq!(frame.get::<f64>("width").unwrap(),16.5);
                    assert_eq!(frame.get::<f64>("height").unwrap(),31.5);
                    seen += 1;
                }
                "opacity" => { assert_eq!(modifier.get::<f64>("value").unwrap(),0.5); seen += 1; }
                _ => {}
            }
        }
        assert_eq!(seen,2,"both actual properties consumed by the runtime graph");
    }
}
