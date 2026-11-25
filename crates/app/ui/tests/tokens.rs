//! The chrome spaces, sizes and sets type from the theme's tokens
//! (`kontra_ui::theme`: HAIR, NOTCH, MARK, TIGHT, SPACE, INSET, TEXT, SMALL,
//! CONTROL…), never a bare pixel count at a call site. `TIGHT * 0.5` scales a
//! token and passes; `.gap(4)` or `.pad((2., 5.))` fails. `0` (no space) and
//! anything naming a value (`Len::Pct(100.)`) pass. Code after `#[cfg(test)]`
//! in a file is not checked.

use std::path::Path;

/// Calls whose arguments are spacing, sizes, corners or type.
const CALLS: [&str; 16] = [
    ".gap(",
    ".line_gap(",
    ".pad(",
    ".radius(",
    ".text_size(",
    ".w(",
    ".h(",
    ".square(",
    ".min_w(",
    ".min_h(",
    ".max_size(",
    ".min_size(",
    ".value_reserve(",
    ".stroke_width(",
    "block(",
    "edges(",
];

/// The text between `open`'s parenthesis and its match.
fn argument(s: &str) -> &str {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return &s[..i],
            ')' => depth -= 1,
            _ => {}
        }
    }
    s
}

/// The argument split at its top-level commas, tuple parentheses unwrapped.
fn parts(arg: &str) -> Vec<String> {
    let arg = arg.trim();
    let arg = arg
        .strip_prefix('(')
        .and_then(|a| a.strip_suffix(')'))
        .unwrap_or(arg);
    let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, c) in arg.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(arg[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(arg[start..].trim().to_owned());
    out
}

/// A pixel count: digits alone, not zero.
fn literal(part: &str) -> bool {
    let bare = part.trim_end_matches(['.', '0']).trim_end_matches('.');
    !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
        && part.chars().any(|c| c.is_ascii_digit() && c != '0')
        && !bare.is_empty()
}

fn scan(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, found);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let code = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (n, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for call in CALLS {
                for (at, _) in line.match_indices(call) {
                    // `block(` as a whole word, not `unblock(`.
                    let before = line[..at].chars().next_back();
                    if !call.starts_with('.')
                        && before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                    {
                        continue;
                    }
                    let arg = argument(&line[at + call.len()..]);
                    if parts(arg).iter().any(|p| literal(p)) {
                        found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
    }
}

#[test]
fn the_chrome_takes_its_spacing_from_tokens() {
    let mut found = Vec::new();
    scan(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(
        found.is_empty(),
        "pixel literals at call sites; use a theme token (HAIR, NOTCH, MARK, TIGHT, SPACE, INSET, TEXT…):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_guard_tells_literals_from_tokens() {
    let hits = |line: &str| {
        CALLS.iter().any(|call| {
            line.match_indices(call).any(|(at, _)| {
                parts(argument(&line[at + call.len()..]))
                    .iter()
                    .any(|p| literal(p))
            })
        })
    };
    assert!(hits("row.gap(4)"));
    assert!(hits("x.pad((2., 5.))"));
    assert!(hits("block(3, Len::Pct(100.))"));
    assert!(!hits("row.gap(TIGHT * 0.5)"));
    assert!(!hits("x.min_w(0)"));
    assert!(!hits("block(Len::Pct(100.), HAIR)"));
}
