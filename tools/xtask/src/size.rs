//! `cargo xtask size`: production and test lines per crate under `src/`, counted by the
//! 00-BASELINE.md rule. Test = a `*test*.rs` file, anything under `tests/`, or a top-level
//! `#[cfg(test)] mod … { … }` block. Approximate: braces inside string literals are not parsed.
//! `--check` adds the ratchet: a crate's production lines may not grow against
//! `tools/xtask/size-baseline.json` (`--update` rewrites it).

use crate::Verdict;
use crate::args;
use crate::ratchet::{self, Counts};
use crate::walk::{self, Nesting};
use crate::workspace;
use cargo_metadata::Metadata;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;

const DEFAULT_MAX_CRATE: usize = 8_000;
const DEFAULT_MAX_FILE: usize = 800;
const DEFAULT_ALLOW: &str = "tools/xtask/size-allow.txt";
const BASELINE: &str = "tools/xtask/size-baseline.json";
const ALLOW_GROWTH_ENV: &str = "KONTRA_ALLOW_GROWTH";
const KNOWN: &[&str] = &[
    "--max-crate",
    "--max-file",
    "--allow",
    "--write-allow",
    "--check",
    "--update",
];

/// Line counts of one file, one package or the whole workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lines {
    pub production: usize,
    pub test: usize,
}

impl Lines {
    fn total(self) -> usize {
        self.production + self.test
    }

    fn add(&mut self, other: Lines) {
        self.production += other.production;
        self.test += other.test;
    }
}

/// One workspace package. `files` paths are relative to the repository root.
#[derive(Debug)]
struct CrateSize {
    name: String,
    lines: Lines,
    files: Vec<(String, Lines)>,
}

/// A crate or file over a limit, with the allow-list entry that would excuse it.
#[derive(Debug)]
struct Offender {
    entry: String,
    detail: String,
}

pub fn run(args: &[String]) -> Result<Verdict, String> {
    args::reject_unknown(args, KNOWN)?;
    let max_crate = args::number(args, "--max-crate", DEFAULT_MAX_CRATE)?;
    let max_file = args::number(args, "--max-file", DEFAULT_MAX_FILE)?;
    let allow_path = args::value(args, "--allow")?.unwrap_or_else(|| DEFAULT_ALLOW.to_string());

    let metadata = workspace::load()?;
    let root = workspace::root(&metadata);
    let mut crates = measure(&metadata, &root);
    crates.sort_by(|a, b| {
        b.lines
            .production
            .cmp(&a.lines.production)
            .then_with(|| a.name.cmp(&b.name))
    });
    print_table(&crates);

    let offenders = offenders(&crates, max_crate, max_file);
    let allow_file = root.join(&allow_path);
    if args::flag(args, "--write-allow") {
        write_allow(&allow_file, &offenders)?;
        println!("wrote {} entries to {allow_path}", offenders.len());
        return Ok(Verdict::Pass);
    }
    if args::flag(args, "--update") {
        ratchet::write(&root.join(BASELINE), &production_counts(&crates))?;
        println!("wrote {BASELINE}");
        return Ok(Verdict::Pass);
    }

    let limits = check_limits(&offenders, &allow_file, &allow_path)?;
    if !args::flag(args, "--check") {
        return Ok(limits);
    }
    let growth = check_growth(&root, &crates)?;
    Ok(if limits == Verdict::Pass && growth == Verdict::Pass {
        Verdict::Pass
    } else {
        Verdict::Fail
    })
}

fn check_limits(
    offenders: &[Offender],
    allow_file: &Path,
    allow_path: &str,
) -> Result<Verdict, String> {
    let allowed = load_allow(allow_file)?;
    let failures: Vec<&Offender> = offenders
        .iter()
        .filter(|offender| !allowed.contains(&offender.entry))
        .collect();
    if failures.is_empty() {
        println!("size: every crate and file is within limits or listed in {allow_path}");
        return Ok(Verdict::Pass);
    }
    for failure in failures {
        eprintln!(
            "size: {} (add `{}` to {allow_path} to allow it)",
            failure.detail, failure.entry
        );
    }
    Ok(Verdict::Fail)
}

/// Production lines per crate may not grow against `BASELINE`. CI sets `ALLOW_GROWTH_ENV=1`
/// when the PR has the `size-growth` label; the PR then also runs `size --update`.
fn check_growth(root: &Path, crates: &[CrateSize]) -> Result<Verdict, String> {
    let baseline = ratchet::read(&root.join(BASELINE))?;
    let current = production_counts(crates);
    let grew = ratchet::grown(&current, &baseline);
    for change in &grew {
        eprintln!(
            "size: crate {} grew from {} to {} production lines",
            change.key, change.before, change.after
        );
    }
    let shrank = ratchet::shrunk(&current, &baseline);
    for change in &shrank {
        println!(
            "size: crate {} shrank from {} to {} production lines",
            change.key, change.before, change.after
        );
    }
    if !shrank.is_empty() {
        println!("size: run `cargo xtask size --update` to tighten the ratchet");
    }
    if grew.is_empty() {
        println!("size: no crate grew against {BASELINE}");
        return Ok(Verdict::Pass);
    }
    if env::var(ALLOW_GROWTH_ENV).is_ok_and(|value| value == "1") {
        println!("size: growth allowed by the size-growth label; update {BASELINE} in this PR");
        return Ok(Verdict::Pass);
    }
    eprintln!("size: growth needs the `size-growth` label and an updated {BASELINE}");
    Ok(Verdict::Fail)
}

fn production_counts(crates: &[CrateSize]) -> Counts {
    crates
        .iter()
        .map(|krate| (krate.name.clone(), krate.lines.production))
        .collect()
}

/// Measures the root package and `crates/*`, counting their `src/` trees only, as
/// 00-BASELINE.md does. `vendor/` forks and `tools/` are not part of the codebase figure.
fn measure(metadata: &Metadata, root: &Path) -> Vec<CrateSize> {
    metadata
        .workspace_packages()
        .into_iter()
        .filter_map(|package| {
            let dir = workspace::package_dir(package);
            let rel_dir = walk::relative(&dir, root);
            if !(rel_dir.is_empty() || rel_dir.starts_with("crates/")) {
                return None;
            }
            let src = dir.join("src");
            let mut krate = CrateSize {
                name: package.name.to_string(),
                lines: Lines::default(),
                files: Vec::new(),
            };
            for file in walk::rs_files(&src, Nesting::Skip) {
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                let lines = classify(&walk::relative(&file, &src), &text);
                krate.lines.add(lines);
                krate.files.push((walk::relative(&file, root), lines));
            }
            Some(krate)
        })
        .collect()
}

/// Splits one file into production and test lines. `package_rel` is relative to the package.
pub fn classify(package_rel: &str, text: &str) -> Lines {
    let total = text.lines().count();
    if is_test_file(package_rel) {
        return Lines {
            production: 0,
            test: total,
        };
    }
    let test = test_module_lines(text);
    Lines {
        production: total - test,
        test,
    }
}

fn is_test_file(package_rel: &str) -> bool {
    let file = package_rel.rsplit('/').next().unwrap_or(package_rel);
    file.contains("test") || package_rel.split('/').any(|part| part == "tests")
}

/// Lines of top-level `#[cfg(test)] mod … { … }` blocks, attribute included.
fn test_module_lines(text: &str) -> usize {
    let mut test_lines = 0;
    let mut depth = 0_i64;
    let mut armed = false;
    let mut module_closes_at: Option<i64> = None;
    for line in text.lines() {
        let outer = depth;
        depth += brace_delta(line);
        if let Some(closes_at) = module_closes_at {
            test_lines += 1;
            if depth <= closes_at {
                module_closes_at = None;
            }
            continue;
        }
        if outer != 0 {
            continue;
        }
        let starts_mod = line.starts_with("mod ") || line.starts_with("pub mod ");
        if armed && starts_mod && !line.trim_end().ends_with(';') {
            test_lines += 2;
            if depth > 0 {
                module_closes_at = Some(0);
            }
        }
        armed = line.starts_with("#[cfg(test)]");
    }
    test_lines
}

fn brace_delta(line: &str) -> i64 {
    let code = line.split("//").next().unwrap_or(line);
    code.chars().fold(0, |acc, c| match c {
        '{' => acc + 1,
        '}' => acc - 1,
        _ => acc,
    })
}

fn offenders(crates: &[CrateSize], max_crate: usize, max_file: usize) -> Vec<Offender> {
    let mut out = Vec::new();
    for krate in crates {
        if krate.lines.production > max_crate {
            out.push(Offender {
                entry: format!("crate {}", krate.name),
                detail: format!(
                    "crate {} has {} production lines (max {max_crate})",
                    krate.name,
                    grouped(krate.lines.production)
                ),
            });
        }
        for (path, lines) in &krate.files {
            if lines.production > max_file {
                out.push(Offender {
                    entry: format!("file {path}"),
                    detail: format!(
                        "{path} has {} production lines (max {max_file})",
                        lines.production
                    ),
                });
            }
        }
    }
    out
}

fn print_table(crates: &[CrateSize]) {
    println!("| Crate | Production | Test | Total |");
    println!("|---|---:|---:|---:|");
    let mut sum = Lines::default();
    for krate in crates {
        println!(
            "| `{}` | {} | {} | {} |",
            krate.name,
            grouped(krate.lines.production),
            grouped(krate.lines.test),
            grouped(krate.lines.total())
        );
        sum.add(krate.lines);
    }
    println!(
        "| **Total** | {} | {} | {} |",
        grouped(sum.production),
        grouped(sum.test),
        grouped(sum.total())
    );
}

fn load_allow(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}

fn write_allow(path: &Path, offenders: &[Offender]) -> Result<(), String> {
    let mut entries: Vec<&str> = offenders
        .iter()
        .map(|offender| offender.entry.as_str())
        .collect();
    entries.sort_unstable();
    let mut text = String::from(
        "# Crates and files over the size limits (cargo xtask size). Format: `crate <name>` or\n\
         # `file <repo-relative path>`. Shrink this list as code splits; never grow it to pass a gate.\n",
    );
    for entry in entries {
        text.push_str(entry);
        text.push('\n');
    }
    fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// `1234567` → `1,234,567`.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_are_counted_as_test_lines() {
        let text = "fn a() {}\nfn b() {}\n";
        assert_eq!(
            classify("src/engine_tests.rs", text),
            Lines {
                production: 0,
                test: 2
            }
        );
        assert_eq!(
            classify("tests/one.rs", text),
            Lines {
                production: 0,
                test: 2
            }
        );
        assert_eq!(
            classify("src/lib.rs", text),
            Lines {
                production: 2,
                test: 0
            }
        );
    }

    #[test]
    fn trailing_test_module_is_split_from_production() {
        let text = "fn a() {\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n    }\n}\n";
        assert_eq!(
            classify("src/a.rs", text),
            Lines {
                production: 2,
                test: 5
            }
        );
    }

    #[test]
    fn cfg_test_on_a_non_module_item_is_not_a_block() {
        let text = "#[cfg(test)]\nuse helpers::x;\nfn a() {}\n";
        assert_eq!(
            classify("src/a.rs", text),
            Lines {
                production: 3,
                test: 0
            }
        );
    }

    #[test]
    fn grouped_adds_separators() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(271_100), "271,100");
    }
}
