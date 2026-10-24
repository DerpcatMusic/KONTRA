//! `cargo xtask debt`: counts of structural debt. With `--baseline` a count may not grow.

use crate::Verdict;
use crate::args;
use crate::walk::{self, Nesting};
use crate::workspace;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const KNOWN: &[&str] = &["--baseline", "--write-baseline"];
/// Trees scanned for debt. `tools/xtask` is skipped: it is the checker, and its own
/// patterns would count against it.
const SCAN_ROOTS: &[&str] = &["src", "crates", "tools"];
const SKIP_DIRS: &[&str] = &["tools/xtask"];
/// Real-time crates: a `Mutex` here is debt on the audio path.
const REALTIME_DIRS: &[&str] = &["crates/engine", "crates/dsp", "crates/sampler-core"];
/// Plain substring counts. Names are the baseline keys, so keep them stable.
const PATTERNS: &[(&str, &str)] = &[
    ("path-attributes", "#[path"),
    ("allow-attributes", "#[allow("),
    ("processor-matches", "Processor::"),
    ("impl-default", "impl Default for"),
];
const COUNT_KEYS: &[&str] = &["cfg-test-in-fn", "mutex-in-realtime"];

pub type Counts = BTreeMap<String, usize>;

pub fn run(args: &[String]) -> Result<Verdict, String> {
    args::reject_unknown(args, KNOWN)?;
    let baseline_arg = args::value(args, "--baseline")?;
    let write = args::flag(args, "--write-baseline");

    let metadata = workspace::load()?;
    let root = workspace::root(&metadata);
    let counts = count(&root);

    let Some(baseline_arg) = baseline_arg else {
        if write {
            return Err("--write-baseline needs --baseline <path>".to_string());
        }
        print_table(&counts, None);
        return Ok(Verdict::Pass);
    };
    let baseline_path = root.join(&baseline_arg);
    if write {
        let json = serde_json::to_string_pretty(&counts)
            .map_err(|error| format!("cannot encode counts: {error}"))?;
        fs::write(&baseline_path, json + "\n")
            .map_err(|error| format!("cannot write {}: {error}", baseline_path.display()))?;
        print_table(&counts, None);
        println!("wrote {baseline_arg}");
        return Ok(Verdict::Pass);
    }

    let baseline = read_baseline(&baseline_path)?;
    print_table(&counts, Some(&baseline));
    let mut grew = false;
    for (key, &count) in &counts {
        let before = baseline.get(key).copied().unwrap_or(0);
        if count > before {
            grew = true;
            eprintln!("debt: {key} grew from {before} to {count}");
        }
    }
    Ok(if grew { Verdict::Fail } else { Verdict::Pass })
}

/// Every debt count in the workspace. Keys with no hits are still present, at zero.
pub fn count(root: &Path) -> Counts {
    let mut counts: Counts = PATTERNS
        .iter()
        .map(|(key, _)| *key)
        .chain(COUNT_KEYS.iter().copied())
        .map(|key| (key.to_string(), 0))
        .collect();
    for scan in SCAN_ROOTS {
        for file in walk::rs_files(&root.join(scan), Nesting::Include) {
            let rel = walk::relative(&file, root);
            if SKIP_DIRS.iter().any(|dir| in_dir(&rel, dir)) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            for (key, pattern) in PATTERNS {
                add(&mut counts, key, text.matches(pattern).count());
            }
            add(&mut counts, "cfg-test-in-fn", cfg_test_in_fn_lines(&text));
            if REALTIME_DIRS.iter().any(|dir| in_dir(&rel, dir)) {
                add(&mut counts, "mutex-in-realtime", word_count(&text, "Mutex"));
            }
        }
    }
    counts
}

fn add(counts: &mut Counts, key: &str, amount: usize) {
    *counts.entry(key.to_string()).or_default() += amount;
}

fn in_dir(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Lines carrying the test-only attribute inside a function body. Approximate: a body runs
/// from its opening brace to the matching close, found by counting braces. Strings are not parsed.
pub fn cfg_test_in_fn_lines(text: &str) -> usize {
    let attribute = concat!("#[cfg(", "test)]");
    let mut depth = 0_usize;
    let mut bodies: Vec<usize> = Vec::new();
    let mut pending_fn = false;
    let mut parens = 0_usize;
    let mut found = 0;
    for line in text.lines() {
        let code = line.split("//").next().unwrap_or(line);
        if !bodies.is_empty() && line.trim_start().starts_with(attribute) {
            found += 1;
        }
        if has_fn_keyword(code) {
            pending_fn = true;
        }
        for c in code.chars() {
            match c {
                '(' | '[' => parens += 1,
                ')' | ']' => parens = parens.saturating_sub(1),
                ';' if pending_fn && parens == 0 => pending_fn = false,
                '{' => {
                    depth += 1;
                    if pending_fn {
                        bodies.push(depth);
                        pending_fn = false;
                    }
                }
                '}' => {
                    if bodies.last() == Some(&depth) {
                        bodies.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }
    found
}

fn has_fn_keyword(code: &str) -> bool {
    code.match_indices("fn ").any(|(at, _)| {
        code[..at]
            .chars()
            .last()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })
}

/// Whole-word occurrences of `word`.
fn word_count(text: &str, word: &str) -> usize {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|token| *token == word)
        .count()
}

fn read_baseline(path: &Path) -> Result<Counts, String> {
    let text = fs::read_to_string(path).map_err(|error| {
        format!(
            "cannot read {}: {error} (create it with --write-baseline)",
            path.display()
        )
    })?;
    serde_json::from_str(&text).map_err(|error| format!("cannot parse {}: {error}", path.display()))
}

fn print_table(counts: &Counts, baseline: Option<&Counts>) {
    let Some(baseline) = baseline else {
        println!("| Debt | Count |");
        println!("|---|---:|");
        for (key, count) in counts {
            println!("| `{key}` | {count} |");
        }
        return;
    };
    println!("| Debt | Count | Baseline |");
    println!("|---|---:|---:|");
    for (key, count) in counts {
        let before = baseline.get(key).copied().unwrap_or(0);
        println!("| `{key}` | {count} | {before} |");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cfg_test_inside_a_fn_body_is_counted() {
        let text =
            "fn outer() {\n    #[cfg(test)]\n    let x = 1;\n}\n#[cfg(test)]\nmod tests {}\n";
        assert_eq!(cfg_test_in_fn_lines(text), 1);
    }

    #[test]
    fn trait_method_declarations_do_not_open_a_body() {
        let text =
            "trait T {\n    fn f(x: [u8; 3]);\n}\nimpl A {\n    #[cfg(test)]\n    fn g() {}\n}\n";
        assert_eq!(cfg_test_in_fn_lines(text), 0);
    }

    #[test]
    fn mutex_is_counted_as_a_whole_word_only() {
        assert_eq!(
            word_count("Mutex<u8> RwMutex MutexGuard Mutex::new()", "Mutex"),
            2
        );
    }
}
