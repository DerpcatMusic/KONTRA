//! `kontra-probe`: plays an instrument through the playback contracts and reports them.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};
use kontra_probe::{BLOCK, CONTRACTS, Mode, Pattern, Report, Session, Verdict, check};
use serde::Serialize;

const SYNOPSIS: &str = "usage: kontra-probe <instrument> [--pattern live|dense|offsets|cc] \
[--contracts C1,C3,...] [--json <out.json>]";
const HELP: &str = "usage: kontra-probe <instrument> [--pattern live|dense|offsets|cc] \
[--contracts C1,C3,...] [--json <out.json>]

Plays <instrument> through the playback contracts C1-C10 and prints one verdict each.

  --pattern P      pattern C1 and C6 measure, and the one in the JSON report (default live).
                   C4 and C5 always measure live; C8 always measures dense.
  --contracts IDS  comma-separated contract ids to run (default: all ten)
  --json FILE      also write the report and verdicts to FILE as JSON
  -h, --help       print this help

exit: 0 every selected contract passed or is unsupported; 1 a contract failed;
      2 usage or load error";
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

/// Parses the command line; `None` when help was asked for.
fn parse(mut raw: impl Iterator<Item = String>) -> Result<Option<Args>> {
    let mut instrument = None;
    let mut pattern = Pattern::Live;
    let mut contracts: Vec<String> = CONTRACTS.iter().map(|id| (*id).to_owned()).collect();
    let mut json = None;
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
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
            flag if flag.starts_with("--") => bail!("unknown flag `{flag}`\n{SYNOPSIS}"),
            _ => {
                ensure!(instrument.is_none(), "one instrument per run\n{SYNOPSIS}");
                instrument = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(Some(Args {
        instrument: instrument.context(SYNOPSIS)?,
        pattern,
        contracts,
        json,
    }))
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
    let Some(args) = parse(std::env::args().skip(1))? else {
        println!("{HELP}");
        return Ok(true);
    };
    let mut session = Session::load(&args.instrument, RATE)?;
    let report = session.play(args.pattern, Mode::RealTime, BLOCK)?;
    let audible = report
        .notes
        .iter()
        .filter(|n| n.first_audible.is_some())
        .count();
    let dropouts: u64 = report.notes.iter().map(|n| n.dropouts).sum();
    let zones = match session.key_span() {
        None => "no zone map".to_owned(),
        Some((low, high)) => {
            let outside = report
                .notes
                .iter()
                .filter(|n| session.uncovered(n).is_some_and(|keys| !keys.is_empty()))
                .count();
            format!("zone keys {low}..={high}, {outside} steps with a key no zone plays")
        }
    };
    println!(
        "{:?} pattern: {} steps, {audible} audible, {dropouts} dropouts; {zones}",
        args.pattern,
        report.notes.len()
    );

    let mut verdicts = Vec::with_capacity(args.contracts.len());
    for id in &args.contracts {
        let verdict = check(&mut session, id, args.pattern)?;
        println!(
            "{} {:<4} [{}] {}",
            verdict.id,
            label(verdict.pass),
            verdict.pattern,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_flag_asks_for_help_instead_of_failing() {
        let args = ["--help"].map(str::to_owned);
        assert!(parse(args.into_iter()).expect("parses").is_none());
    }

    #[test]
    fn pattern_flag_selects_the_pattern() {
        let args = ["song.nki", "--pattern", "dense"].map(str::to_owned);
        let parsed = parse(args.into_iter()).expect("parses").expect("not help");
        assert_eq!(parsed.pattern, Pattern::Dense);
    }
}
