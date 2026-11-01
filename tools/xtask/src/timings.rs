//! `cargo xtask timings`: rebuild time after a one-line edit to a hot crate (09-BUILD.md §2).
//! Each scenario builds once untouched, then once with a comment appended to the edit file.
//! The edit is restored by a drop guard, so it is undone on error too.

use crate::Verdict;
use crate::args;
use crate::workspace;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const KNOWN: &[&str] = &[];
const EDIT_FILE: &str = "crates/engine/engine/src/lib.rs";
const EDIT_TEXT: &str = "\n// xtask timings: gate edit, restored after the run\n";

/// The 09-BUILD.md §2 rows this tree can measure today.
struct Scenario {
    label: &'static str,
    cargo: &'static [&'static str],
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        label: "core tests after a core edit",
        cargo: &["cargo", "test", "-p", "kontra-engine", "--lib", "--no-run"],
    },
    Scenario {
        label: "dev lib after a core edit",
        cargo: &["cargo", "build", "-p", "kontra-engine", "--lib"],
    },
    Scenario {
        label: "release lib after a core edit",
        cargo: &[
            "cargo",
            "build",
            "--release",
            "-p",
            "kontra-engine",
            "--lib",
        ],
    },
];

/// Puts the edit file back to its saved bytes when dropped.
struct Edit {
    path: PathBuf,
    original: Vec<u8>,
}

impl Edit {
    fn apply(path: PathBuf) -> io::Result<Self> {
        let original = fs::read(&path)?;
        let mut edited = original.clone();
        edited.extend_from_slice(EDIT_TEXT.as_bytes());
        fs::write(&path, edited)?;
        Ok(Self { path, original })
    }
}

impl Drop for Edit {
    fn drop(&mut self) {
        if let Err(error) = fs::write(&self.path, &self.original) {
            eprintln!(
                "xtask: could not restore {}: {error}; restore it with git checkout",
                self.path.display()
            );
        }
    }
}

pub fn run(args: &[String]) -> Result<Verdict, String> {
    args::reject_unknown(args, KNOWN)?;
    let metadata = workspace::load()?;
    let root = workspace::root(&metadata);
    let edit_path = root.join(EDIT_FILE);

    let mut rows = Vec::new();
    for scenario in SCENARIOS {
        build(&root, scenario.cargo)?;
        let elapsed = {
            let _edit = Edit::apply(edit_path.clone())
                .map_err(|error| format!("cannot edit {EDIT_FILE}: {error}"))?;
            build(&root, scenario.cargo)?
        };
        rows.push((scenario, elapsed));
    }

    println!("| Gate edit | Command | Rebuild after edit |");
    println!("|---|---|---:|");
    for (scenario, elapsed) in rows {
        println!(
            "| {} | `build-slot {}` | {:.1} s |",
            scenario.label,
            scenario.cargo.join(" "),
            elapsed.as_secs_f64()
        );
    }
    Ok(Verdict::Pass)
}

fn build(root: &std::path::Path, cargo: &[&str]) -> Result<Duration, String> {
    let start = Instant::now();
    // build-slot shares build capacity on the maintainer's machine; elsewhere run cargo directly.
    let status = match Command::new("build-slot")
        .args(cargo)
        .current_dir(root)
        .status()
    {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Command::new(cargo[0])
            .args(&cargo[1..])
            .current_dir(root)
            .status(),
        other => other,
    }
    .map_err(|error| format!("cannot run {}: {error}", cargo[0]))?;
    if !status.success() {
        return Err(format!(
            "`build-slot {}` failed ({status})",
            cargo.join(" ")
        ));
    }
    Ok(start.elapsed())
}
