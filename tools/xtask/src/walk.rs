//! Source discovery shared by the commands.

use std::fs;
use std::path::{Path, PathBuf};

/// Whether directories with their own `Cargo.toml` (other packages) are walked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nesting {
    Skip,
    Include,
}

/// Every `.rs` file under `dir`, sorted. Build output and VCS directories are never entered.
pub fn rs_files(dir: &Path, nesting: Nesting) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, nesting, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, nesting: Nesting, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skipped_name = matches!(
                entry.file_name().to_str(),
                Some("target" | ".git" | "node_modules")
            );
            let nested_package = nesting == Nesting::Skip && path.join("Cargo.toml").is_file();
            if !skipped_name && !nested_package {
                collect(&path, nesting, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// `path` relative to `base`, with forward slashes, for reports and allow-lists.
pub fn relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
