//! `kontra-probe`: plays an instrument through the playback contracts and reports them.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail, ensure};
use kontra_probe::{BLOCK, CONTRACTS, Fixture, Mode, Pattern, Report, Session, Verdict, check};
use serde::Serialize;

const SYNOPSIS: &str = "usage: kontra-probe <instrument> | --fixture <name> [--pattern live|dense|offsets|cc] \
[--contracts C1,C3,...] [--json <out.json>]";
const HELP: &str =
    "usage: kontra-probe <instrument> | --fixture <name> [--pattern live|dense|offsets|cc] \
[--contracts C1,C3,...] [--json <out.json>]

Plays <instrument> through the playback contracts C1-C10 and prints one verdict each.

  --fixture NAME   play a synthetic fixture built in the target directory instead of a
                   file: cc_start, ui_load, key_position, dense or streamed
                   (see tools/probe/README.md)
  --pattern P      pattern C1 and C6 measure, and the one in the JSON report (default live).
                   C4 and C5 always measure live; C8 always measures dense.
  --contracts IDS  comma-separated contract ids to run (default: all ten)
  --json FILE      also write the report and verdicts to FILE as JSON
  -h, --help       print this help

exit: 0 every selected contract passed or is unsupported; 1 a contract failed;
      2 usage or load error";
const RATE: f64 = 48_000.0;

/// What the probe plays: a file, or a fixture built on demand.
enum Input {
    File(PathBuf),
    Fixture(Fixture),
}

/// Parsed command line.
struct Args {
    input: Input,
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
    let mut fixture = None;
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
            "--fixture" => {
                let value = raw
                    .next()
                    .context("--fixture needs a name, e.g. cc_start")?;
                fixture = Some(value.parse().map_err(anyhow::Error::msg)?);
            }
            flag if flag.starts_with("--") => bail!("unknown flag `{flag}`\n{SYNOPSIS}"),
            _ => {
                ensure!(instrument.is_none(), "one instrument per run\n{SYNOPSIS}");
                instrument = Some(PathBuf::from(arg));
            }
        }
    }
    let input = match (instrument, fixture) {
        (Some(path), None) => Input::File(path),
        (None, Some(fixture)) => Input::Fixture(fixture),
        _ => bail!("give one instrument or --fixture, not both or neither\n{SYNOPSIS}"),
    };
    Ok(Some(Args {
        input,
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
    let (instrument, fixture) = match args.input {
        Input::File(path) => (path, None),
        Input::Fixture(fixture) => {
            let gaps = fixture.gaps();
            if !gaps.is_empty() {
                println!("fixture {fixture} is missing: {gaps}");
            }
            (fixture.build()?, Some(fixture))
        }
    };
    let mut session = Session::load(&instrument, RATE)?;
    if let Some(fixture) = fixture {
        session = session
            .expect_voices(fixture.expected_voices())
            .expect_ui_writes(fixture.ui_writes_per_key());
    }
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
        "{:?} pattern: {} steps, {audible} audible, {dropouts} dropouts, {} script slot(s); {zones}",
        args.pattern,
        report.notes.len(),
        session.scripts()
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
            instrument: instrument.display().to_string(),
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

    #[test]
    fn fixture_flag_replaces_the_instrument_path() {
        let args = ["--fixture", "cc_start"].map(str::to_owned);
        let parsed = parse(args.into_iter()).expect("parses").expect("not help");
        assert!(matches!(parsed.input, Input::Fixture(Fixture::CcStart)));
        let both = ["song.nki", "--fixture", "dense"].map(str::to_owned);
        assert!(parse(both.into_iter()).is_err(), "one input only");
        let unknown = ["--fixture", "chords"].map(str::to_owned);
        assert!(parse(unknown.into_iter()).is_err(), "unknown fixture");
    }
}
