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

use std::io::Write as _;
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
    ///
    /// A rate that is not the one comparisons use is said **here**, through this
    /// writer, because it is a line about the run: `--diag` must file it with
    /// the run and `--quiet` must not print it, the same rule `render.rs`'s
    /// `invalid` follows. Said from `parse`, it went to stderr before the writer
    /// existed, so a `--quiet --rate 44100` run printed it anyway and a `--diag`
    /// log was missing the one line that says the WAV is not comparable.
    fn open(&self, rate: u32) -> Result<Box<dyn std::io::Write>, String> {
        let mut sink: Box<dyn std::io::Write> = match self {
            Self::Stderr => Box::new(std::io::stderr()),
            Self::File(path) => Box::new(
                std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?,
            ),
            Self::None => Box::new(std::io::sink()),
        };
        if rate != DEFAULT_RATE {
            writeln!(
                sink,
                "kontra-render: warning: --rate {rate} is not the {DEFAULT_RATE} Hz comparisons use"
            )
            .map_err(|e| format!("diagnostics: {e}"))?;
        }
        Ok(sink)
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
    let mut sink = match parsed.diag.open(parsed.request.rate) {
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
  --hold <seconds>      how long the single note is held (default 1.5; must be positive)
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
            "--hold" => request.note.hold_s = hold(value()?)?,
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

/// A positive seconds argument: `--hold` is the length of one note.
///
/// Zero is refused rather than accepted. A zero hold schedules the NoteOff on
/// the same frame as the NoteOn, and `render.rs` refuses `KONTRA_HOLD=0` for the
/// same reason ("a zero hold is a click"): the two ways to set the hold must not
/// disagree about what is a note. `--tail` keeps `seconds`, where 0 is a real
/// answer (stop at the last key-up).
fn hold(v: &str) -> Result<f32, String> {
    let s = seconds(v)?;
    if s > 0.0 {
        Ok(s)
    } else {
        Err(format!("--hold {v}: a note held for no time is a click"))
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

    /// The non-48 kHz warning is a line about the run, so it goes through the
    /// run's diagnostics writer: `--diag` files it and `--quiet` (a sink) drops
    /// it. It used to be an `eprintln!` in `parse`, which printed past `--quiet`
    /// and was missing from a `--diag` log.
    #[test]
    fn the_rate_warning_goes_to_the_diagnostics_writer() {
        let dir = std::env::temp_dir();
        let log = dir.join(format!("kontra-render-rate-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);

        Diagnostics::File(log.clone())
            .open(44_100)
            .expect("the log opens");
        let text = std::fs::read_to_string(&log).expect("the warning was written");
        assert!(
            text.contains("44100") && text.contains("comparisons use"),
            "the warning did not reach the log: {text:?}"
        );

        // The rate comparisons use says nothing: no warning to file.
        Diagnostics::File(log.clone())
            .open(DEFAULT_RATE)
            .expect("the log opens");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "");

        // `--quiet` hands the run a sink: the line is dropped like every other.
        Diagnostics::None.open(44_100).expect("the sink takes it");
        let _ = std::fs::remove_file(&log);
    }

    /// `--hold 0` would put the NoteOff on the NoteOn's frame; the environment
    /// override already refuses it, and the two ways to set the hold agree.
    /// `--tail 0` is a real request (stop at the last key-up) and stands.
    #[test]
    fn a_zero_hold_is_refused_but_a_zero_tail_is_not() {
        let mut args = required();
        args.extend(["--hold", "0"].map(str::to_owned));
        let err = parse(&args).unwrap_err();
        assert!(err.contains("click"), "unexpected message: {err}");

        let mut args = required();
        args.extend(["--hold", "0.5", "--tail", "0"].map(str::to_owned));
        let parsed = parse(&args).expect("a positive hold and a zero tail are valid");
        assert_eq!(parsed.request.note.hold_s, 0.5);
        assert_eq!(parsed.request.tail_s, 0.0);
    }
}
