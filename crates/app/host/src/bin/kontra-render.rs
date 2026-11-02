//! `kontra-render`: render one instrument, through KONTRA, to a WAV, and say what came out.
//!
//! ```text
//! kontra-render --instrument "Electric Mint.nki" --key 60 --out mint.wav
//! kontra-render --instrument "Electric Mint.nki" --midi parity.mid --out mint.wav --rate 48000
//! ```
//!
//! Stdout is one JSON object (the [`Summary`] the parity tools parse) and nothing
//! else; diagnostics go to stderr, or to `--diag`. Exit status is 0 when a WAV was
//! written, 2 for a usage error, and 1 when the render failed.

use std::path::PathBuf;

use kontra_host::render::{DEFAULT_RATE, Request};

/// What the command line asked for.
#[derive(Debug)]
struct Args {
    request: Request,
    /// Where diagnostics go.
    diag: Diagnostics,
    /// Print the usage and stop.
    help: bool,
}

/// Where the diagnostics go.
///
/// The default is the process's own stderr, **not** the path `/dev/stderr`: a
/// redirect (`2>run.log`) truncates the target once, when the shell opens it,
/// and opening `/dev/stderr` on top of that with `File::create` — which is
/// `O_TRUNC` — threw away whatever the log already held. `--diag` names a file
/// the run creates fresh, and `--quiet` writes none.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Diagnostics {
    /// The process's stderr, inherited from the shell.
    Stderr,
    /// A file this run creates.
    File(PathBuf),
    /// Nothing.
    None,
}

impl Diagnostics {
    /// The writer for this run; `Err` carries a message for the front end.
    fn open(&self) -> Result<Box<dyn std::io::Write>, String> {
        match self {
            Self::Stderr => Ok(Box::new(std::io::stderr())),
            Self::File(path) => std::fs::File::create(path)
                .map(|file| Box::new(file) as Box<dyn std::io::Write>)
                .map_err(|e| format!("{}: {e}", path.display())),
            Self::None => Ok(Box::new(std::io::sink())),
        }
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match parse(&args) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("kontra-render: {e}\n\n{USAGE}");
            return std::process::ExitCode::from(2);
        }
    };
    if parsed.help {
        println!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    let mut sink = match parsed.diag.open() {
        Ok(sink) => sink,
        Err(e) => {
            eprintln!("kontra-render: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match kontra_host::render::run(&parsed.request, &mut *sink) {
        Ok(summary) => match serde_json::to_string(&summary) {
            Ok(json) => {
                println!("{json}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("kontra-render: summary: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("kontra-render: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
usage: kontra-render --instrument <file> --out <wav> [options]

  --instrument <file>   the Kontakt instrument (or `bank.ufs/Name.uvip`) to render
  --out <wav>           where the 32-bit float stereo WAV goes (default render.wav)
  --midi <file>         play this standard MIDI file instead of a single note
  --key <0..127>        key of the single note (default 60)
  --velocity <1..127>   velocity of the single note (default 100)
  --hold <seconds>      how long the single note is held (default 1.5)
  --wheel <0..127>      move the mod wheel to this value one second in
  --tail <seconds>      silence after the last key-up (default 2)
  --rate <hz>           output rate (default 48000; comparisons require 48000)
  --live                render in real time instead of waiting for the disk
  --diag <path>         write the diagnostics there instead of stderr
  --quiet               write no diagnostics
  -h, --help            this text

stdout is one JSON object: format, instrument, wav, name, rate, channels, frames,
duration_s, peak, peak_dbfs, rms, rms_dbfs, sha256, note, midi, notes, zones,
load_s, cpu_ms, realtime_factor.";

/// Parses the command line; `Err` carries the message for a usage error.
fn parse(args: &[String]) -> Result<Args, String> {
    let mut request = Request::default();
    let mut diag = Diagnostics::Stderr;
    let mut help = false;
    let mut given_instrument = false;
    let mut given_out = false;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i += 1;
        let mut value = || -> Result<&String, String> {
            let v = args.get(i).ok_or_else(|| format!("{arg} needs a value"))?;
            i += 1;
            Ok(v)
        };
        match arg.as_str() {
            "--instrument" | "--nki" => {
                request.instrument = PathBuf::from(value()?);
                given_instrument = true;
            }
            "--out" | "--wav" => {
                request.out = PathBuf::from(value()?);
                given_out = true;
            }
            "--midi" => request.midi = Some(PathBuf::from(value()?)),
            "--key" => request.note.key = number(value()?, "--key", 0, 127)?,
            "--velocity" => request.note.velocity = number(value()?, "--velocity", 1, 127)?,
            "--hold" => request.note.hold_s = seconds(value()?)?,
            "--tail" => request.tail_s = seconds(value()?)?,
            "--wheel" => request.wheel = Some(number(value()?, "--wheel", 0, 127)?),
            "--rate" => request.rate = rate(value()?)?,
            "--live" => request.live = true,
            "--diag" => diag = Diagnostics::File(PathBuf::from(value()?)),
            "--quiet" => diag = Diagnostics::None,
            "-h" | "--help" => help = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if !help && !given_instrument {
        return Err("--instrument is required".into());
    }
    if !help && !given_out {
        return Err("--out is required: a render without a WAV measures nothing".into());
    }
    if request.rate != DEFAULT_RATE && !help {
        eprintln!(
            "kontra-render: warning: --rate {} is not the {DEFAULT_RATE} Hz comparisons use",
            request.rate
        );
    }
    if request.tail_s < 0.0 {
        return Err("--tail cannot be negative".into());
    }
    Ok(Args {
        request,
        diag,
        help,
    })
}

/// An integer argument within `lo..=hi`.
fn number(v: &str, flag: &str, lo: u8, hi: u8) -> Result<u8, String> {
    let n: u16 = v.parse().map_err(|_| format!("{flag} {v}: not a number"))?;
    u8::try_from(n)
        .ok()
        .filter(|n| (lo..=hi).contains(n))
        .ok_or_else(|| format!("{flag} {n} is outside {lo}..={hi}"))
}

/// A sample rate argument within what a WAV writer and the engine can hold.
fn rate(v: &str) -> Result<u32, String> {
    let n: u32 = v.parse().map_err(|_| format!("--rate {v}: not a number"))?;
    if (1_000..=384_000).contains(&n) {
        Ok(n)
    } else {
        Err(format!("--rate {n} is outside 1000..=384000"))
    }
}

/// A non-negative seconds argument.
fn seconds(v: &str) -> Result<f32, String> {
    let s: f32 = v
        .parse()
        .map_err(|_| format!("{v}: not a number of seconds"))?;
    if s.is_finite() && s >= 0.0 {
        Ok(s)
    } else {
        Err(format!("{v}: not a number of seconds"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two arguments a render needs, so `parse` gets past its checks.
    fn required() -> Vec<String> {
        ["--instrument", "x.nki", "--out", "x.wav"]
            .map(str::to_owned)
            .to_vec()
    }

    /// Diagnostics default to the process's own stderr, never to the path
    /// `/dev/stderr`: a shell redirect (`2>run.log`) truncates the file once,
    /// and opening that path over it with `File::create` (which is `O_TRUNC`)
    /// emptied the log the redirect had just created.
    #[test]
    fn diagnostics_go_to_stderr_unless_a_file_is_named() {
        let mut args = required();
        assert_eq!(parse(&args).unwrap().diag, Diagnostics::Stderr);

        args.extend(["--diag", "run.log"].map(str::to_owned));
        assert_eq!(
            parse(&args).unwrap().diag,
            Diagnostics::File(PathBuf::from("run.log"))
        );

        let mut quiet = required();
        quiet.push("--quiet".to_owned());
        assert_eq!(parse(&quiet).unwrap().diag, Diagnostics::None);
    }
}
