//! `kontra-inspect`: what an instrument loads, plays and drops, as data, on
//! the engine the plugin ships (no plugin, no window).
//!
//! ```sh
//! kontra-inspect load <instrument> [--json FILE]
//! kontra-inspect law [NAME]
//! ```
//!
//! `<instrument>` is a Kontakt instrument or a UVI program as
//! `bank.ufs/Presets/Name.uvip`. A one-page summary goes to stdout; `--json`
//! writes everything. Exit 2 on a usage or load error.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};

mod bands;
mod laws;
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

const USAGE: &str = "usage: kontra-inspect law [NAME] | load <instrument> [--json FILE] | ui <instrument> [--json FILE] | play <instrument> [--scenario user|FILE] [--fresh-state] [--oracle] [--json FILE]";

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
    if command == "law" {
        print!("{}", laws::run(args.next().as_deref())?);
        return Ok(());
    }
    let mut path = None;
    let mut json = None;
    let mut scenario = String::from("user");
    let mut oracle = false;
    let mut fresh = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--oracle" => oracle = true,
            "--fresh-state" => fresh = true,
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
    ensure!(
        !fresh || (command == "play" && scenario == "user"),
        "--fresh-state requires play --scenario user"
    );
    let (report, summary) = match command.as_str() {
        "load" => load::run(&path)?,
        "play" if fresh => fresh_play(&path, oracle)?,
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
    if oracle && fresh {
        ensure!(
            report
                .get("result")
                .and_then(|r| r.get("oracle_complete"))
                .and_then(serde_json::Value::as_bool)
                == Some(true),
            "oracle comparison unavailable: see segment reports"
        );
    }
    if oracle && !fresh {
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

fn fresh_play(path: &std::path::Path, oracle: bool) -> Result<(serde_json::Value, String)> {
    let mut segments = Vec::new();
    let mut summary = String::from("Fresh-state segments (first-onset alignment):\n");
    let mut complete = true;
    let mut oracle_complete = true;
    for part in 0..6 {
        let (report, text) = play::segment(path, part, oracle)?;
        let result = report.get("result");
        complete &= result
            .and_then(|v| v.get("render_complete"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        oracle_complete &= !oracle
            || result
                .and_then(|v| v.get("oracle"))
                .and_then(|v| v.get("status"))
                .and_then(serde_json::Value::as_str)
                == Some("ok");
        summary.push_str(&text);
        segments.push(report);
    }
    Ok((
        serde_json::json!({"schema_version":1, "command":"play", "engine":"v3", "instrument":path,
        "result":{"scenario":"user", "fresh_state":true, "segments":segments, "render_complete":complete, "oracle_complete":oracle_complete}}),
        summary,
    ))
}
