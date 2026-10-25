//! `cargo xtask`: measurements and gates for the v3 refactor (docs/architecture-v3/09-BUILD.md §3).
//! Tooling only; not shipped.

mod args;
mod ci;
mod debt;
mod deps;
mod ratchet;
mod size;
mod timings;
mod walk;
mod workspace;

use std::process::ExitCode;

/// Result of a command. `Fail` means a gate was not met; a tool error is an `Err` instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

const USAGE: &str = "usage: cargo xtask <ci|size|debt|deps-check|timings> [options]";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = argv.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let outcome = match command.as_str() {
        "ci" => ci::run(rest),
        "size" => size::run(rest),
        "debt" => debt::run(rest),
        "deps-check" => deps::run(rest),
        "timings" => timings::run(rest),
        other => Err(format!("unknown command `{other}`\n{USAGE}")),
    };
    match outcome {
        Ok(Verdict::Pass) => ExitCode::SUCCESS,
        Ok(Verdict::Fail) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::from(2)
        }
    }
}
