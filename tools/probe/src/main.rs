//! `kontra-probe`: plays an instrument through the playback contracts and reports them.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};
use kontra_probe::{BLOCK, CONTRACTS, Mode, Pattern, Report, Session, Verdict, check};
use serde::Serialize;

const USAGE: &str = "usage: kontra-probe <instrument> [--pattern live|dense|offsets|cc] \
[--contracts C1,C3,...] [--json <out.json>]";
const RATE: f64 = 48_000.0;

/// Parsed command line.
struct Args {
    instrument: PathBuf,
    pattern: Pattern,
    contracts: Vec<String>,
    json: Option<PathBuf>,
}

/// The `--json` file: the report for `--pattern` and every verdict that ran.
#[derive(Serialize)]
struct Out<'a> {
    instrument: String,
    rate: f64,
    report: &'a Report,
    verdicts: &'a [Verdict],
}

fn parse(mut raw: impl Iterator<Item = String>) -> Result<Args> {
    let mut instrument = None;
    let mut pattern = Pattern::Live;
    let mut contracts: Vec<String> = CONTRACTS.iter().map(|id| (*id).to_owned()).collect();
    let mut json = None;
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--pattern" => {
                let value = raw.next().context("--pattern needs a value")?;
                pattern = value.parse().map_err(anyhow::Error::msg)?;
            }
            "--contracts" => {
                let value = raw.next().context("--contracts needs a list, e.g. C1,C4")?;
                contracts = value
                    .split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            "--json" => json = Some(PathBuf::from(raw.next().context("--json needs a path")?)),
            flag if flag.starts_with("--") => bail!("unknown flag `{flag}`\n{USAGE}"),
            _ => {
                ensure!(instrument.is_none(), "one instrument per run\n{USAGE}");
                instrument = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(Args {
        instrument: instrument.context(USAGE)?,
        pattern,
        contracts,
        json,
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("kontra-probe: {error:#}");
            ExitCode::from(2)
        }
    }
}

/// Runs the probe; `Ok(false)` when a selected contract fails.
fn run() -> Result<bool> {
    let args = parse(std::env::args().skip(1))?;
    let mut session = Session::load(&args.instrument, RATE)?;
    let report = session.play(args.pattern, Mode::RealTime, BLOCK)?;
    let audible = report
        .notes
        .iter()
        .filter(|n| n.first_audible.is_some())
        .count();
    let dropouts: u64 = report.notes.iter().map(|n| n.dropouts).sum();
    println!(
        "{:?} pattern: {} steps, {audible} audible, {dropouts} dropouts",
        args.pattern,
        report.notes.len()
    );

    let mut verdicts = Vec::with_capacity(args.contracts.len());
    for id in &args.contracts {
        let verdict = check(&mut session, id)?;
        println!(
            "{} {:<4} {}",
            verdict.id,
            label(verdict.pass),
            verdict.detail
        );
        verdicts.push(verdict);
    }

    if let Some(path) = &args.json {
        let out = Out {
            instrument: args.instrument.display().to_string(),
            rate: RATE,
            report: &report,
            verdicts: &verdicts,
        };
        let text = serde_json::to_string_pretty(&out).context("serializing the report")?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(verdicts.iter().all(|v| v.pass != Some(false)))
}

fn label(pass: Option<bool>) -> &'static str {
    match pass {
        Some(true) => "PASS",
        Some(false) => "FAIL",
        None => "N/A",
    }
}
