//! Kontakt 8 `native_ui` source (`.nui`) to plain Luau, token by token.
//!
//! The language is Lua with three additions, and only those are rewritten:
//! - `@F { a = 1  b = 2  @G{} }`: a node, `F` a component or primitive; its
//!   fields need no commas. It becomes `__nui(F, { a = 1, b = 2, __nui(G, {}) })`.
//! - `@{ expr }`: an expression standing as a node. It becomes `(expr)`.
//! - `.name(args)` after a node: a modifier. It becomes `:name(args)`, and
//!   named arguments (`.frame(width = 1)`) a table: `:name({width = 1})`.
//!
//! Anything else passes through. What the rules do not cover (an `@` before
//! anything but a name or `{`, unbalanced brackets) is an error with its
//! line: the module shows nothing rather than a guess.

/// One token: where it starts and ends in the source, its line, its kind.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Token {
    start: usize,
    end: usize,
    line: u32,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Name,
    Keyword,
    Number,
    Str,
    /// Punctuation and operators, by their text.
    Punct,
}

/// Why a module was not rewritten, and the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: u32,
    pub message: String,
}

/// Table braces, by name: a bare brace literal would unbalance brace-counting
/// tools (xtask debt) that skip string contents.
const LB: &str = "\u{7b}";
const RB: &str = "\u{7d}";

const KEYWORDS: [&str; 21] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn tokens(src: &str) -> Result<Vec<Token>, Error> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1u32);
    let error = |line, message: &str| Error {
        line,
        message: message.to_owned(),
    };
    while let Some(&c) = b.get(i) {
        let start = i;
        let first = line;
        // A long bracket `[[`, `[==[` from `at`: its length to the close.
        let long = |at: usize| -> Option<usize> {
            let eq = b.get(at + 1..)?.iter().take_while(|&&c| c == b'=').count();
            (b.get(at + 1 + eq) == Some(&b'[')).then_some(())?;
            let close = format!("]{}]", "=".repeat(eq));
            let body = src.get(at + 2 + eq..)?;
            Some(2 + eq + body.find(&close)? + close.len())
        };
        let kind = match c {
            b'\n' => {
                line += 1;
                i += 1;
                continue;
            }
            c if c.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                let n = if b.get(i + 2) == Some(&b'[') {
                    long(i + 2)
                } else {
                    None
                };
                i = match n {
                    Some(n) => i + 2 + n,
                    None => src
                        .get(i..)
                        .and_then(|s| s.find('\n'))
                        .map_or(b.len(), |n| i + n),
                };
                line += src
                    .get(start..i)
                    .map_or(0, |s| s.matches('\n').count() as u32);
                continue;
            }
            b'[' if matches!(b.get(i + 1), Some(b'[' | b'=')) && long(i).is_some() => {
                i += long(i).unwrap_or(1);
                Kind::Str
            }
            b'"' | b'\'' => {
                i += 1;
                loop {
                    match b.get(i) {
                        None | Some(b'\n') => return Err(error(first, "unfinished string")),
                        Some(b'\\') => i += 2,
                        Some(&q) if q == c => break,
                        Some(_) => i += 1,
                    }
                }
                i += 1;
                Kind::Str
            }
            b'`' => return Err(error(line, "interpolated strings are not supported")),
            c if c.is_ascii_digit()
                || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                while b
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_'))
                    || (matches!(b.get(i), Some(b'+' | b'-'))
                        && matches!(b.get(i - 1), Some(b'e' | b'E'))
                        && !src.get(start..i).is_some_and(|s| s.starts_with("0x")))
                {
                    i += 1;
                }
                Kind::Number
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                while b
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                {
                    i += 1;
                }
                if KEYWORDS.contains(&src.get(start..i).unwrap_or_default()) {
                    Kind::Keyword
                } else {
                    Kind::Name
                }
            }
            _ => {
                let three = src.get(i..i + 3).unwrap_or_default();
                let two = src.get(i..i + 2).unwrap_or_default();
                i += if three == "..." || three == "..=" {
                    3
                } else if matches!(
                    two,
                    "==" | "~=" | "<=" | ">=" | ".." | "::" | "//" | "+=" | "-=" | "*=" | "/="
                ) {
                    2
                } else {
                    1
                };
                Kind::Punct
            }
        };
        line += src
            .get(start..i)
            .map_or(0, |s| s.matches('\n').count() as u32);
        out.push(Token {
            start,
            end: i,
            line: first,
            kind,
        });
    }
    Ok(out)
}

/// What an open bracket or block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    /// `@F{`: its fields get commas; the close gains `)`.
    Node,
    /// `@{`: an expression; it closes with `)`.
    Expr,
    /// A modifier's named arguments: its close gains `}`.
    Named,
    /// A modifier's plain arguments.
    Modifier,
    Bracket,
    Block,
}

/// The source of a `.nui` module as Luau.
///
/// # Errors
/// The first construct the rules do not cover, with its line.
pub fn transform(src: &str) -> Result<String, Error> {
    let toks = tokens(src)?;
    let text = |t: &Token| src.get(t.start..t.end).unwrap_or_default();
    let is = |n: usize, s: &str| toks.get(n).is_some_and(|t| text(t) == s);
    // Edits as (byte offset, text inserted, bytes replaced), in order.
    let mut edits: Vec<(usize, String, usize)> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    // The last token closed a node, an `@{}` or a modifier call: a `.name(`
    // now is a modifier.
    let mut chain = false;
    let mut n = 0;
    while let Some(t) = toks.get(n) {
        let s = text(t);
        let in_node = open.last() == Some(&Open::Node);
        // A new field: the token before ends an expression, this one starts
        // an item (a name before `=`, a child `@`, or anything on a new line).
        if in_node && n > 0 {
            let prev = toks.get(n - 1);
            let ends = prev.is_some_and(|p| {
                matches!(p.kind, Kind::Name | Kind::Number | Kind::Str)
                    || matches!(
                        text(p),
                        RB | ")" | "]" | "end" | "true" | "false" | "nil" | "..."
                    )
            });
            let starts = matches!(t.kind, Kind::Name | Kind::Number | Kind::Str)
                || matches!(
                    s,
                    "@" | LB | "[" | "function" | "not" | "nil" | "true" | "false" | "-"
                );
            let named = t.kind == Kind::Name && is(n + 1, "=");
            let new_line = prev.is_some_and(|p| p.line < t.line);
            if ends && starts && (named || s == "@" || new_line) {
                edits.push((t.start, ",".into(), 0));
            }
        }
        let was_chain = std::mem::replace(&mut chain, false);
        match (t.kind, s) {
            (Kind::Punct, "@") => {
                let Some(next) = toks.get(n + 1) else {
                    return Err(Error {
                        line: t.line,
                        message: "`@` at the end".into(),
                    });
                };
                if text(next) == LB {
                    edits.push((t.start, "(".into(), next.end - t.start));
                    open.push(Open::Expr);
                    n += 2;
                    continue;
                }
                // `@a.b.c{`: a dotted name, then its braces.
                let mut m = n + 1;
                while toks.get(m).is_some_and(|t| t.kind == Kind::Name) && is(m + 1, ".") {
                    m += 2;
                }
                if toks.get(m).is_none_or(|t| t.kind != Kind::Name) || !is(m + 1, LB) {
                    return Err(Error {
                        line: t.line,
                        message: "`@` before something other than a name and a table".into(),
                    });
                }
                let brace = toks.get(m + 1).map_or(0, |b| b.start);
                edits.push((t.start, "__nui(".into(), 1));
                edits.push((brace, ", ".into(), 0));
                open.push(Open::Node);
                n = m + 2;
                continue;
            }
            (Kind::Punct, ".")
                if was_chain
                    && toks.get(n + 1).is_some_and(|t| t.kind == Kind::Name)
                    && is(n + 2, "(") =>
            {
                edits.push((t.start, ":".into(), 1));
                // Named arguments: a name and `=` at the call's own depth.
                let mut depth = 0i32;
                let mut named = false;
                let mut m = n + 3;
                while let Some(a) = toks.get(m) {
                    match text(a) {
                        "(" | LB | "[" => depth += 1,
                        ")" | RB | "]" if depth == 0 => break,
                        ")" | RB | "]" => depth -= 1,
                        "function" | "do" | "if" | "repeat" => depth += 1,
                        "end" | "until" => depth -= 1,
                        _ => {}
                    }
                    named |= depth == 0 && a.kind == Kind::Name && is(m + 1, "=");
                    m += 1;
                }
                if named {
                    edits.push((toks.get(n + 2).map_or(0, |p| p.end), LB.into(), 0));
                    open.push(Open::Named);
                } else {
                    open.push(Open::Modifier);
                }
                n += 3;
                // Its close continues the chain.
                continue;
            }
            (Kind::Punct, "(" | LB | "[") => open.push(Open::Bracket),
            (Kind::Keyword, "function" | "do" | "if" | "repeat") => open.push(Open::Block),
            (Kind::Keyword, "end" | "until") => {
                if open.pop() != Some(Open::Block) {
                    return Err(Error {
                        line: t.line,
                        message: format!("`{s}` closes no block"),
                    });
                }
            }
            (Kind::Punct, ")" | RB | "]") => {
                let Some(o) = open.pop().filter(|o| *o != Open::Block) else {
                    return Err(Error {
                        line: t.line,
                        message: format!("`{s}` closes nothing"),
                    });
                };
                match o {
                    Open::Node => edits.push((t.end, ")".into(), 0)),
                    Open::Expr => edits.push((t.start, ")".into(), 1)),
                    Open::Named => edits.push((t.start, RB.into(), 0)),
                    Open::Modifier | Open::Bracket | Open::Block => {}
                }
                // A node, an `@{}` or a modifier call chains.
                chain = o != Open::Bracket;
            }
            _ => {}
        }
        n += 1;
    }
    if let Some(t) = toks.last().filter(|_| !open.is_empty()) {
        return Err(Error {
            line: t.line,
            message: "unclosed bracket or block".into(),
        });
    }
    let mut out = String::with_capacity(src.len() + edits.len() * 4);
    let mut at = 0;
    edits.sort_by_key(|e| e.0);
    for (pos, insert, replaced) in edits {
        out.push_str(src.get(at..pos).unwrap_or_default());
        out.push_str(&insert);
        at = pos + replaced;
    }
    out.push_str(src.get(at..).unwrap_or_default());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::transform;

    /// Without whitespace, which the rewrite does not keep exactly.
    fn squash(s: &str) -> String {
        s.split_whitespace().collect()
    }

    #[test]
    fn a_node_gets_its_call_and_commas_between_fields() {
        let out = transform("return @ui.HStack {\n spacing = 4\n align = \"top\"\n}").unwrap();
        assert_eq!(
            squash(&out),
            squash(r#"return __nui(ui.HStack, { spacing = 4 , align = "top" })"#)
        );
    }

    #[test]
    fn children_and_function_fields_stay_whole() {
        let src = "@ui.ForEach {\n data = items\n content = function (item)\n local a = 1\n b = a\n return @Row { x = a }\n end\n @ui.Spacer{}\n}";
        let out = transform(src).unwrap();
        assert_eq!(
            squash(&out),
            squash(
                "__nui(ui.ForEach, { data = items , content = function (item) local a = 1 b = a \
                 return __nui(Row, { x = a }) end , __nui(ui.Spacer, {}) })"
            )
        );
    }

    #[test]
    fn an_expression_node_is_parenthesised() {
        let out = transform("@{ shown and @A{} or @B{} }").unwrap();
        assert_eq!(
            squash(&out),
            squash("( shown and __nui(A, {}) or __nui(B, {}) )")
        );
    }

    #[test]
    fn modifiers_chain_and_named_arguments_become_a_table() {
        let src = "@ui.Text{ text = \"a\" }\n.frame(width = 2, height = f(1))\n.padding(24)\n.align(\"top\")";
        let out = transform(src).unwrap();
        assert_eq!(
            squash(&out),
            squash(
                r#"__nui(ui.Text, { text = "a" }) :frame({width = 2, height = f(1)}) :padding(24) :align("top")"#
            )
        );
        // A plain call's `.name(` is not a modifier.
        assert_eq!(transform("util.map(x).y(1)").unwrap(), "util.map(x).y(1)");
    }

    #[test]
    fn what_the_rules_do_not_cover_is_an_error_with_its_line() {
        let err = transform("local a = 1\nreturn @ 3").unwrap_err();
        assert_eq!(err.line, 2);
        assert_eq!(transform("@A{ x = (1 }").unwrap_err().line, 1);
    }
}
