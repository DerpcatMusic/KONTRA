//! `kontra-inspect`: what an instrument loads, plays and drops, as data, on
//! the engine the plugin ships (no plugin, no window).
//!
//! ```sh
//! kontra-inspect load <instrument> [--json FILE]
//! ```
//!
//! `<instrument>` is a Kontakt instrument or a UVI program as
//! `bank.ufs/Presets/Name.uvip`. A one-page summary goes to stdout; `--json`
//! writes everything. Exit 2 on a usage or load error.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};

mod bands;
mod load;
mod midi;
mod oracle;
mod play;
mod routes;
mod scenario;
mod trace_output;
mod ui;

/// The engine's sample rate for every subcommand.
const RATE: u32 = 48_000;

const USAGE: &str = "usage: kontra-inspect load <instrument> [--json FILE] | ui <instrument> [--json FILE] | play <instrument> [--scenario user|FILE] [--oracle] [--json FILE]";

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
    if matches!(command.as_str(), "--help" | "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    let mut path = None;
    let mut json = None;
    let mut scenario = String::from("user");
    let mut oracle = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--oracle" => oracle = true,
            "--scenario" => scenario = args.next().context("--scenario needs a value")?,
            "--json" => json = Some(PathBuf::from(args.next().context("--json needs a file")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if a.starts_with('-') => bail!("unknown option {a}\n{USAGE}"),
            _ if path.is_none() => path = Some(PathBuf::from(a)),
            _ => bail!("unexpected argument {a}\n{USAGE}"),
        }
    }
    let path = path.context(USAGE)?;
    ensure!(
        !oracle || command == "play",
        "--oracle is only available for play"
    );
    let (report, summary) = match command.as_str() {
        "load" => load::run(&path)?,
        "play" => play::run(&path, &scenario, oracle)?,
        "ui" => ui::run(&path)?,
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
    ensure!(
        report
            .get("result")
            .and_then(|r| r.get("render_complete"))
            .and_then(serde_json::Value::as_bool)
            != Some(false),
        "inspection incomplete: see report for UI, trace, or render errors"
    );
    if oracle {
        let status = report
            .get("result")
            .and_then(|r| r.get("oracle"))
            .and_then(|r| r.get("status"))
            .and_then(serde_json::Value::as_str);
        ensure!(
            status == Some("ok"),
            "oracle comparison unavailable: see report for the reason"
        );
    }
    Ok(())
}
