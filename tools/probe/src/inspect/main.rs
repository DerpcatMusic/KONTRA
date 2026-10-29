//! `kontra-inspect`: what an instrument loads, plays and drops, as data, on
//! the engine the plugin ships (no plugin, no window).
//!
//! ```sh
//! kontra-inspect load <instrument> [--json FILE]
//! ```
//!
//! `<instrument>` is a Kontakt instrument, an SFZ, or a UVI program as
//! `bank.ufs/Presets/Name.uvip`. A one-page summary goes to stdout; `--json`
//! writes everything. Exit 2 on a usage or load error.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

mod load;

/// The engine's sample rate for every subcommand.
const RATE: u32 = 48_000;

const USAGE: &str = "usage: kontra-inspect load <instrument> [--json FILE]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kontra-inspect: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().context(USAGE)?;
    let mut path = None;
    let mut json = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = Some(PathBuf::from(args.next().context("--json needs a file")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if path.is_none() => path = Some(PathBuf::from(a)),
            _ => bail!("unexpected argument {a}\n{USAGE}"),
        }
    }
    let path = path.context(USAGE)?;
    let (report, summary) = match command.as_str() {
        "load" => load::run(&path)?,
        "--help" | "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        other => bail!("unknown command {other}\n{USAGE}"),
    };
    print!("{summary}");
    if let Some(json) = json {
        let text = serde_json::to_string_pretty(&report)?;
        std::fs::write(&json, text).with_context(|| format!("writing {}", json.display()))?;
    }
    Ok(())
}
