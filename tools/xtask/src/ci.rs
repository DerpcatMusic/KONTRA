//! `cargo xtask ci`: the one local gate, the same checks CI runs on a PR. For the `.rs` files
//! a branch changed against `--base`, it runs rustfmt, then clippy and tests on their packages
//! (clippy warnings fail only in changed files), then the workspace checks `deps-check`,
//! `debt --check` and `size --check`. Uncommitted edits are not seen; commit first.
//! Cargo runs through `KONTRA_CARGO` (default `cargo`); the maintainer sets `build-slot cargo`.

use crate::Verdict;
use crate::args;
use crate::walk;
use crate::workspace;
use std::collections::BTreeSet;
use std::env;
use std::path::Path;
use std::process::Command;

const KNOWN: &[&str] = &["--base"];
const DEFAULT_BASE: &str = "origin/v3";
const DEFAULT_CARGO: &str = "cargo";
const EDITION: &str = "2024";

/// What one step did. `Fail` carries the detail to print above its line.
enum Outcome {
    Pass,
    Skip(String),
    Fail(String),
}

pub fn run(args: &[String]) -> Result<Verdict, String> {
    args::reject_unknown(args, KNOWN)?;
    let base = args::value(args, "--base")?.unwrap_or_else(|| DEFAULT_BASE.to_string());
    let cargo = env::var("KONTRA_CARGO").unwrap_or_else(|_| DEFAULT_CARGO.to_string());

    let metadata = workspace::load()?;
    let root = workspace::root(&metadata);
    let changed = changed_rs_files(&root, &base)?;
    let packages: Vec<(String, String)> = metadata
        .workspace_packages()
        .iter()
        .map(|package| {
            let dir = walk::relative(&workspace::package_dir(package), &root);
            (package.name.to_string(), dir)
        })
        .collect();
    let touched = packages_for(&packages, &changed);
    println!(
        "ci: {} changed .rs files in {} packages against {base}",
        changed.len(),
        touched.len()
    );

    let me =
        env::current_exe().map_err(|error| format!("cannot find the xtask binary: {error}"))?;
    // Every step runs, so one report shows all failures; each prints its PASS/FAIL line as it ends.
    let mut passed = true;
    passed &= report("rustfmt", rustfmt(&root, &changed));
    passed &= report("clippy", clippy(&cargo, &root, &touched, &changed));
    passed &= report("test", tests(&cargo, &root, &touched));
    passed &= report("deps-check", tool(&me, &root, &["deps-check"]));
    passed &= report("debt", tool(&me, &root, &["debt", "--check"]));
    passed &= report("size", tool(&me, &root, &["size", "--check"]));

    println!("ci: {}", if passed { "PASS" } else { "FAIL" });
    Ok(if passed { Verdict::Pass } else { Verdict::Fail })
}

/// Prints one step's line and returns whether it passed (a skip counts as a pass).
fn report(name: &str, result: Result<Outcome, String>) -> bool {
    match result.unwrap_or_else(Outcome::Fail) {
        Outcome::Pass => {
            println!("PASS {name}");
            true
        }
        Outcome::Skip(reason) => {
            println!("SKIP {name}: {reason}");
            true
        }
        Outcome::Fail(detail) => {
            println!("FAIL {name}: {detail}");
            false
        }
    }
}

fn rustfmt(root: &Path, changed: &[String]) -> Result<Outcome, String> {
    if changed.is_empty() {
        return Ok(Outcome::Skip("no changed .rs files".to_string()));
    }
    let status = Command::new("rustfmt")
        .current_dir(root)
        .args(["--check", "--edition", EDITION])
        .args(changed)
        .status()
        .map_err(|error| format!("cannot run rustfmt: {error}"))?;
    Ok(if status.success() {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "run `rustfmt --edition {EDITION}` on the files above"
        ))
    })
}

/// Clippy on the touched packages. Stdout is cargo's JSON stream; warnings fail only when they
/// sit in a changed file, and any error fails.
fn clippy(
    cargo: &str,
    root: &Path,
    packages: &BTreeSet<String>,
    changed: &[String],
) -> Result<Outcome, String> {
    if packages.is_empty() {
        return Ok(Outcome::Skip("no changed Rust package".to_string()));
    }
    let output = cargo_command(cargo)?
        .current_dir(root)
        .arg("clippy")
        .args(package_flags(packages))
        .args(["--all-targets", "--no-deps", "--message-format=json"])
        .output()
        .map_err(|error| format!("cannot run {cargo} clippy: {error}"))?;
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    let mut problems = clippy_problems(&String::from_utf8_lossy(&output.stdout), changed);
    if !output.status.success() {
        problems.push(format!("clippy exited with {}", output.status));
    }
    Ok(if problems.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(problems.join("\n"))
    })
}

/// The rendered diagnostics that fail the clippy step: warnings in changed files, and errors.
pub fn clippy_problems(json: &str, changed: &[String]) -> Vec<String> {
    json.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["reason"] == "compiler-message")
        .filter_map(|value| {
            let message = &value["message"];
            let level = message["level"].as_str()?;
            if level != "warning" && level != "error" {
                return None;
            }
            let in_changed = message["spans"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|span| span["file_name"].as_str())
                .any(|file| changed.iter().any(|path| path == file));
            if in_changed || level == "error" {
                message["rendered"].as_str().map(str::to_string)
            } else {
                None
            }
        })
        .collect()
}

fn tests(cargo: &str, root: &Path, packages: &BTreeSet<String>) -> Result<Outcome, String> {
    if packages.is_empty() {
        return Ok(Outcome::Skip("no changed Rust package".to_string()));
    }
    let status = cargo_command(cargo)?
        .current_dir(root)
        .arg("test")
        .args(package_flags(packages))
        .status()
        .map_err(|error| format!("cannot run {cargo} test: {error}"))?;
    Ok(if status.success() {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("cargo test exited with {status}"))
    })
}

/// Runs this xtask binary with `args`, so the workspace checks need no second build.
fn tool(me: &Path, root: &Path, args: &[&str]) -> Result<Outcome, String> {
    let status = Command::new(me)
        .current_dir(root)
        .args(args)
        .status()
        .map_err(|error| format!("cannot run xtask {}: {error}", args.join(" ")))?;
    Ok(if status.success() {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("xtask {} exited with {status}", args.join(" ")))
    })
}

/// `KONTRA_CARGO` split on whitespace: `build-slot cargo` runs `build-slot` with `cargo` first.
fn cargo_command(cargo: &str) -> Result<Command, String> {
    let mut words = cargo.split_whitespace();
    let program = words.next().ok_or("KONTRA_CARGO is empty")?;
    let mut command = Command::new(program);
    command.args(words);
    Ok(command)
}

fn package_flags(packages: &BTreeSet<String>) -> Vec<&str> {
    packages
        .iter()
        .flat_map(|name| ["-p", name.as_str()])
        .collect()
}

/// `.rs` files the branch changed against `base`, relative to `root`. Deleted files are skipped.
fn changed_rs_files(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let range = format!("{base}...HEAD");
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "diff",
            "--name-only",
            "--relative",
            "--diff-filter=d",
            &range,
        ])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff {range} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|file| file.ends_with(".rs"))
        .map(str::to_string)
        .collect())
}

/// The packages that own the changed files. A file belongs to the package whose directory is
/// its longest path prefix; the root package (empty directory) owns what no other package does.
pub fn packages_for(packages: &[(String, String)], files: &[String]) -> BTreeSet<String> {
    files
        .iter()
        .filter_map(|file| {
            packages
                .iter()
                .filter(|(_, dir)| owns(dir, file))
                .max_by_key(|(_, dir)| dir.len())
                .map(|(name, _)| name.clone())
        })
        .collect()
}

fn owns(dir: &str, file: &str) -> bool {
    dir.is_empty()
        || file
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packages() -> Vec<(String, String)> {
        [
            ("kontakto", ""),
            ("xtask", "tools/xtask"),
            ("sampler-core", "crates/sampler-core"),
        ]
        .map(|(name, dir)| (name.to_string(), dir.to_string()))
        .to_vec()
    }

    #[test]
    fn a_file_belongs_to_its_longest_matching_package() {
        let files = [
            "tools/xtask/src/ci.rs",
            "crates/sampler-core/src/bus.rs",
            "src/lib.rs",
        ]
        .map(String::from);
        assert_eq!(
            packages_for(&packages(), &files),
            BTreeSet::from([
                "kontakto".to_string(),
                "sampler-core".to_string(),
                "xtask".to_string()
            ])
        );
    }

    #[test]
    fn a_directory_prefix_must_end_at_a_path_separator() {
        let files = ["tools/xtaskish/x.rs".to_string()];
        assert_eq!(
            packages_for(&packages(), &files),
            BTreeSet::from(["kontakto".to_string()])
        );
    }

    #[test]
    fn clippy_fails_on_warnings_in_changed_files_and_on_every_error() {
        let json = [
            r#"{"reason":"compiler-message","message":{"level":"warning","rendered":"warning: changed\n","spans":[{"file_name":"tools/xtask/src/ci.rs"}]}}"#,
            r#"{"reason":"compiler-message","message":{"level":"warning","rendered":"warning: untouched\n","spans":[{"file_name":"crates/x/src/lib.rs"}]}}"#,
            r#"{"reason":"compiler-message","message":{"level":"error","rendered":"error: broken\n","spans":[]}}"#,
            r#"{"reason":"build-finished","success":false}"#,
            "not json",
        ]
        .join("\n");
        assert_eq!(
            clippy_problems(&json, &["tools/xtask/src/ci.rs".to_string()]),
            vec![
                "warning: changed\n".to_string(),
                "error: broken\n".to_string()
            ]
        );
    }
}
