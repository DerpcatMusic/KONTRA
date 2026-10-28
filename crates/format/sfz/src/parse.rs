//! SFZ text into regions: comments out, `#define` and `#include` expanded,
//! opcodes inherited control/global → master → group → region (sfzformat.com).
//! Nothing here knows the instrument model.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::Path;

/// Opcodes by name; the last value of a name wins.
pub(crate) type Opcodes = BTreeMap<String, String>;

/// How deep `#include` may nest.
const MAX_INCLUDE_DEPTH: u8 = 8;

/// The text as the translator reads it.
pub(crate) struct Document {
    /// The opcodes of `<control>`.
    pub control: Opcodes,
    /// One per `<region>`: every opcode it inherits, its own last.
    pub regions: Vec<Opcodes>,
    /// What the text holds that is not read (feature, detail): other headers,
    /// bad includes, stray text.
    pub problems: Vec<(String, String)>,
}

/// Reads a file's text. SFZ files are often Latin-1, so bytes that are not
/// UTF-8 are replaced rather than failing the read.
pub(crate) fn read(path: &Path) -> std::io::Result<String> {
    std::fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Parses `text`; `include` names resolve from `base`.
pub(crate) fn document(text: &str, base: &Path) -> Document {
    let mut pre = Preprocess::default();
    pre.expand(text, base, 0);
    let mut chunks = pre.out.split('<');
    let mut doc = Document {
        control: Opcodes::new(),
        regions: Vec::new(),
        problems: pre.problems,
    };
    if let Some(stray) = chunks.next().filter(|s| !s.trim().is_empty()) {
        let detail = stray.trim().chars().take(60).collect();
        doc.problems.push(("text outside headers".into(), detail));
    }
    // Scopes by level: global, master, group, region. A header replaces its
    // own level and clears every level below it.
    let mut scopes: [Opcodes; 4] = Default::default();
    for chunk in chunks {
        let (name, body) = chunk.split_once('>').unwrap_or((chunk, ""));
        let ops = opcodes(body);
        let level = match name.trim() {
            "control" => {
                doc.control.extend(ops);
                continue;
            }
            "global" => 0,
            "master" => 1,
            "group" => 2,
            "region" => 3,
            other => {
                doc.problems
                    .push((format!("header <{other}>"), "opcodes not read".into()));
                continue;
            }
        };
        for scope in scopes.iter_mut().skip(level) {
            scope.clear();
        }
        if let Some(scope) = scopes.get_mut(level) {
            *scope = ops;
        }
        if level == 3 {
            doc.regions.push(merged(&scopes));
        }
    }
    doc
}

/// Every scope's opcodes, later scopes overriding earlier ones.
fn merged(scopes: &[Opcodes; 4]) -> Opcodes {
    let mut all = Opcodes::new();
    for scope in scopes {
        all.extend(scope.clone());
    }
    all
}

/// The `name=value` pairs of a header body. A value runs to the next `name=`
/// (so it may hold spaces) or the end of the body.
fn opcodes(body: &str) -> Opcodes {
    // (name start, name end, value start) for each opcode.
    let mut starts = Vec::new();
    let mut at_word = true;
    for (i, c) in body.char_indices() {
        if at_word {
            let rest = body.get(i..).unwrap_or_default();
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if len > 0 && rest.get(len..).is_some_and(|tail| tail.starts_with('=')) {
                starts.push((i, i + len, i + len + 1));
            }
        }
        at_word = c.is_whitespace();
    }
    let mut ops = Opcodes::new();
    for (n, &(start, name_end, value_start)) in starts.iter().enumerate() {
        let end = starts.get(n + 1).map_or(body.len(), |next| next.0);
        let name = body.get(start..name_end).unwrap_or_default();
        let value = body.get(value_start..end).unwrap_or_default().trim();
        ops.insert(name.into(), value.into());
    }
    ops
}

/// Comments, `#define` and `#include`, applied in file order. Defines are
/// global, as in SFZ, so an include sees the defines before it.
#[derive(Default)]
struct Preprocess {
    out: String,
    /// `$NAME` to its value, longest name first so `$A` never eats `$AB`.
    defines: Vec<(String, String)>,
    problems: Vec<(String, String)>,
}

impl Preprocess {
    fn expand(&mut self, text: &str, dir: &Path, depth: u8) {
        for line in strip_comments(text).lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("#define") {
                match rest.trim().split_once(char::is_whitespace) {
                    Some((name, value)) if name.starts_with('$') => {
                        self.defines.push((name.into(), value.trim().into()));
                        self.defines.sort_by_key(|(name, _)| Reverse(name.len()));
                    }
                    _ => self.problems.push(("#define".into(), trimmed.into())),
                }
            } else if let Some(rest) = trimmed.strip_prefix("#include") {
                self.include(rest.trim().trim_matches('"'), dir, depth);
            } else {
                let mut line = line.to_string();
                for (name, value) in &self.defines {
                    line = line.replace(name.as_str(), value);
                }
                self.out.push_str(&line);
                self.out.push('\n');
            }
        }
    }

    fn include(&mut self, name: &str, dir: &Path, depth: u8) {
        let path = dir.join(name.replace('\\', "/"));
        if depth >= MAX_INCLUDE_DEPTH {
            self.problems
                .push(("#include depth".into(), path.display().to_string()));
            return;
        }
        match read(&path) {
            Ok(text) => self.expand(&text, path.parent().unwrap_or(dir), depth + 1),
            Err(e) => self
                .problems
                .push(("#include".into(), format!("{}: {e}", path.display()))),
        }
    }
}

/// Removes `//` and `/* */` comments. An unterminated block comment runs to the end.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('/') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let tail = rest.get(at..).unwrap_or_default();
        if tail.starts_with("//") {
            rest = tail
                .find('\n')
                .map_or("", |n| tail.get(n..).unwrap_or_default());
        } else if tail.starts_with("/*") {
            rest = tail
                .find("*/")
                .map_or("", |n| tail.get(n + 2..).unwrap_or_default());
        } else {
            out.push('/');
            rest = tail.get(1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    out
}
