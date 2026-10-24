//! Playback contracts C1–C10 (docs/architecture-v3/10-TESTING.md §1) over the
//! real product path: [`V2Core`] fed host-style notes, one host block at a time.
//!
//! [`Session::load`] reads an instrument, [`Session::play`] renders a [`Pattern`]
//! into a [`Report`], and [`check`] turns the renders into [`Verdict`]s. Only
//! public `kontakto` APIs are used.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use kontakto::sound::event::{Event, HostNote, HostPattern};
use kontakto::sound::v2::{V2Core, V2Loader};
use kontakto::sound::{BlockInfo, Core, CoreLoader, LoadRequest, Loaded, MAX_BLOCK, ScriptUi};
use serde::Serialize;

/// Host block size the contracts run at.
pub const BLOCK: usize = 128;
/// Host block sizes C4 compares against [`BLOCK`]. Sizes above [`MAX_BLOCK`] are rendered in chunks.
pub const BLOCK_SWEEP: [usize; 6] = [32, 64, 128, 512, 1024, 97];
/// Frames per step: 40 blocks of 128, one 16th at 140 bpm (about 107 ms at 48 kHz).
const STEP: usize = 40 * BLOCK;
/// A note is held this long, then released; the rest of its step is its tail.
const HELD: usize = 19 * BLOCK;
/// Frames rendered before the first note so init and listeners settle.
const SETTLE: usize = 400 * BLOCK;
/// Steps per pattern.
const STEPS: usize = 64;
/// Output level that counts as audible.
const AUDIBLE: f32 = 1e-3;
/// Null-test limit for C4 and C5, in dB.
pub const NULL_LIMIT_DB: f64 = -120.0;
/// Chord shapes. A dense step holds one chord; a live step plays one key of it.
const CHORDS: [[u8; 4]; 4] = [
    [48, 55, 60, 64],
    [50, 57, 62, 65],
    [52, 59, 64, 67],
    [53, 60, 65, 69],
];
/// Contract ids in run order.
pub const CONTRACTS: [&str; 10] = ["C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"];

/// What the probe plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Pattern {
    /// 64 steps, one note each, like a player.
    Live,
    /// 64 steps of 4-note chords, like a quantized DAW part.
    Dense,
    /// Live notes, each preceded by a CC113 (sample start) value sweeping 0..127.
    Offsets,
    /// Live notes, each preceded by a CC1 (dynamics) value sweeping 0..127.
    Cc,
}

impl FromStr for Pattern {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "live" => Ok(Self::Live),
            "dense" => Ok(Self::Dense),
            "offsets" => Ok(Self::Offsets),
            "cc" => Ok(Self::Cc),
            other => Err(format!(
                "unknown pattern `{other}` (live, dense, offsets, cc)"
            )),
        }
    }
}

/// How the host paces blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Sleeps one block of audio time after each block, as a host does.
    RealTime,
    /// Renders as fast as possible; the engine waits for disk instead of shedding load.
    Offline,
}

/// What happened to one step: a note, or a chord that starts in one callback.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteReport {
    pub step: usize,
    pub keys: Vec<u8>,
    /// Frame the note-on was scheduled at, counted from the start of the run.
    pub scheduled: u64,
    /// First frame over the audible level inside the step. Measured on the mixed
    /// output, so every key of a chord shares it; per-voice onsets are not public yet.
    pub first_audible: Option<u64>,
    /// Peak active voices during the step minus the count when it began.
    pub voices: usize,
    /// Stream underruns and dropped commands during the step (counted as events).
    pub dropouts: u64,
}

impl NoteReport {
    /// Frames from the scheduled onset to the first audible frame; `None` if never audible.
    pub fn start_error(&self) -> Option<i64> {
        self.first_audible
            .map(|first| first as i64 - self.scheduled as i64)
    }
}

/// One run of a pattern: per-step measurements and the mixed stereo output.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub pattern: Pattern,
    pub mode: Mode,
    pub block: usize,
    pub notes: Vec<NoteReport>,
    /// Mixed stereo output of the whole run, settle included. Not serialized.
    #[serde(skip)]
    pub audio: Vec<[f32; 2]>,
}

/// Outcome of one contract. `pass` is `None` when the contract cannot be measured yet.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub id: &'static str,
    pub pass: Option<bool>,
    pub detail: String,
}

impl Verdict {
    /// A measured pass.
    pub fn passed(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            pass: Some(true),
            detail: detail.into(),
        }
    }

    /// A measured failure.
    pub fn failed(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            pass: Some(false),
            detail: detail.into(),
        }
    }

    /// The contract is not measurable yet; `reason` says what is missing.
    pub fn unsupported(id: &'static str, reason: &str) -> Self {
        Self {
            id,
            pass: None,
            detail: format!("unsupported: {reason}"),
        }
    }
}

/// One loaded instrument. Each [`play`](Self::play) renders from a fresh core, so runs are independent.
pub struct Session {
    path: PathBuf,
    rate: f64,
    primed: Option<Loaded<<V2Core as Core>::Prepared>>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("path", &self.path)
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Loads `path` at `rate` once to check it; later runs prepare it again.
    ///
    /// # Errors
    /// Fails when the instrument cannot be read or prepared.
    pub fn load(path: &Path, rate: f64) -> Result<Self> {
        let primed = prepare(path, rate)?;
        Ok(Self {
            path: path.to_path_buf(),
            rate,
            primed: Some(primed),
        })
    }

    /// Sample rate the instrument was loaded at.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Renders `pattern` in `mode` with host blocks of `block` frames.
    ///
    /// # Errors
    /// Fails when `block` is zero or the instrument cannot be prepared again.
    pub fn play(&mut self, pattern: Pattern, mode: Mode, block: usize) -> Result<Report> {
        ensure!(block > 0, "host block size must be at least one frame");
        let loaded = match self.primed.take() {
            Some(loaded) => loaded,
            None => prepare(&self.path, self.rate)?,
        };
        let mut core = V2Core::with_parts(1, self.rate);
        core.install(0, loaded.part);
        // Held for the run: the script UI must outlive the core that calls it.
        let _scripts: ScriptUi = loaded.scripts;

        let plan = plan(pattern);
        let mut run = Run {
            audio: Vec::with_capacity(plan.frames),
            samples: Vec::new(),
        };
        let mut pending = plan.events.iter().peekable();
        let mut at = 0;
        while at < plan.frames {
            let frames = block.min(plan.frames - at);
            let end = at + frames;
            core.begin_block(&BlockInfo {
                frames,
                offline: mode == Mode::Offline,
                ..Default::default()
            });
            // Events split the host block at their frame, like the plugin's process loop.
            while let Some(&(when, event)) = pending.next_if(|&&(when, _)| when < end) {
                run.render_to(&mut core, when);
                core.event(0, event);
            }
            run.render_to(&mut core, end);
            core.end_block(frames, &mut |_| true);
            core.take_effects(0, &mut |_, _| true);
            if mode == Mode::RealTime {
                std::thread::sleep(Duration::from_secs_f64(frames as f64 / self.rate));
            }
            at = end;
        }

        let mut notes = plan.notes;
        for note in &mut notes {
            run.measure(note);
        }
        Ok(Report {
            pattern,
            mode,
            block,
            notes,
            audio: run.audio,
        })
    }
}

/// Runs contract `id` (`"C1"`..`"C10"`) against `session`.
///
/// # Errors
/// Fails on an unknown id or when a render cannot be prepared.
pub fn check(session: &mut Session, id: &str) -> Result<Verdict> {
    Ok(match id {
        "C1" => c1_on_time(session)?,
        "C2" => Verdict::unsupported(
            "C2",
            "no degraded-disk fixture: the probe cannot throttle storage yet",
        ),
        "C3" => Verdict::unsupported(
            "C3",
            "per-voice first output is not public; only the mixed output is measured",
        ),
        "C4" => c4_block_size(session)?,
        "C5" => c5_offline_matches_realtime(session)?,
        "C6" => c6_voices_per_note(session)?,
        "C7" => Verdict::unsupported(
            "C7",
            "no script-load fixture: needs 300 UI writes per note (Areia-style)",
        ),
        "C8" => c8_dense_first_block(session)?,
        "C9" => Verdict::unsupported(
            "C9",
            "start offsets are sent (Pattern::Offsets) but playPos is not observable",
        ),
        "C10" => Verdict::unsupported(
            "C10",
            "render time per block is not instrumented in this version",
        ),
        other => bail!("unknown contract `{other}`; expected one of {CONTRACTS:?}"),
    })
}

/// Null test in dB: energy of `a - b` relative to `a`.
///
/// Silent `a`, or mismatched lengths, give infinity so the contract fails.
pub fn null_db(a: &[[f32; 2]], b: &[[f32; 2]]) -> f64 {
    if a.len() != b.len() {
        return f64::INFINITY;
    }
    let signal: f64 = a.iter().flatten().map(|x| f64::from(*x).powi(2)).sum();
    if signal == 0.0 {
        return f64::INFINITY;
    }
    let residual: f64 = a
        .iter()
        .zip(b)
        .flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| f64::from(p - q).powi(2)))
        .sum();
    10.0 * (residual / signal).log10()
}

fn prepare(path: &Path, rate: f64) -> Result<Loaded<<V2Core as Core>::Prepared>> {
    let request = LoadRequest {
        path: path.to_path_buf(),
        sample_rate: rate,
        ..Default::default()
    };
    V2Loader
        .prepare(&request, &mut |_| {}, &|| false)
        .with_context(|| format!("preparing {}", path.display()))
}

/// Events and per-step expectations for one pattern, on one absolute frame clock.
struct Plan {
    events: Vec<(usize, Event)>,
    notes: Vec<NoteReport>,
    frames: usize,
}

fn plan(pattern: Pattern) -> Plan {
    let mut events = Vec::new();
    let mut notes = Vec::with_capacity(STEPS);
    let mut id = 0;
    for step in 0..STEPS {
        let at = SETTLE + step * STEP;
        let chord = CHORDS[(step / 4) % 4];
        let keys: Vec<u8> = match pattern {
            Pattern::Dense => chord.to_vec(),
            Pattern::Live | Pattern::Offsets | Pattern::Cc => vec![chord[step % 4]],
        };
        let sweep = (step * 2 % 128) as u8;
        match pattern {
            Pattern::Offsets => events.push((at, Event::midi1(0xb0, 113, sweep))),
            Pattern::Cc => events.push((at, Event::midi1(0xb0, 1, sweep))),
            Pattern::Live | Pattern::Dense => {}
        }
        for &key in &keys {
            id += 1;
            events.push((
                at,
                Event::NoteOn {
                    note: HostNote {
                        port: 0,
                        channel: 0,
                        key,
                        id,
                        clap: false,
                    },
                    velocity: 100.0 / 127.0,
                    tune: 0.0,
                },
            ));
        }
        for &key in &keys {
            events.push((
                at + HELD,
                Event::NoteOff(HostPattern {
                    port: -1,
                    channel: 0,
                    key: i32::from(key),
                    id: -1,
                    clap: false,
                }),
            ));
        }
        notes.push(NoteReport {
            step,
            keys,
            scheduled: at as u64,
            first_audible: None,
            voices: 0,
            dropouts: 0,
        });
    }
    // Stable: same-frame events keep their order (a sweep CC goes before its note).
    events.sort_by_key(|(frame, _)| *frame);
    Plan {
        events,
        notes,
        frames: SETTLE + STEPS * STEP,
    }
}

/// One voice-count and dropout reading, taken at an event or block boundary.
#[derive(Clone, Copy)]
struct Sample {
    frame: usize,
    active: usize,
    dropouts: u64,
}

/// Output and readings accumulated while a plan plays.
struct Run {
    audio: Vec<[f32; 2]>,
    samples: Vec<Sample>,
}

impl Run {
    /// Renders mixed output up to absolute frame `target`, then takes a reading there.
    fn render_to(&mut self, core: &mut V2Core, target: usize) {
        while self.audio.len() < target {
            let n = (target - self.audio.len()).min(MAX_BLOCK);
            let rendered = core.render(n);
            let live = rendered.live;
            for i in 0..n {
                let mut lr = [0.0f32; 2];
                for (bus, live) in rendered.buses.iter().zip(live) {
                    if live {
                        lr[0] += bus[0][i];
                        lr[1] += bus[1][i];
                    }
                }
                self.audio.push(lr);
            }
        }
        let voices = core.voices();
        self.samples.push(Sample {
            frame: self.audio.len(),
            active: voices.active,
            dropouts: voices.dropouts,
        });
    }

    /// Fills in onset, voice count and dropouts for `note` from the rendered run.
    fn measure(&self, note: &mut NoteReport) {
        let at = note.scheduled as usize;
        let window = at..at + STEP;
        note.first_audible = first_audible(&self.audio, window.start, window.end).map(|f| f as u64);
        let from = self.samples.partition_point(|s| s.frame < window.start);
        let to = self.samples.partition_point(|s| s.frame < window.end);
        let (Some(base), Some(last)) = (
            self.samples.get(from),
            self.samples.get(to.saturating_sub(1)),
        ) else {
            return;
        };
        let peak = self.samples[from..to]
            .iter()
            .map(|s| s.active)
            .max()
            .unwrap_or(0);
        note.voices = peak.saturating_sub(base.active);
        note.dropouts = last.dropouts.saturating_sub(base.dropouts);
    }
}

/// Earliest frame of `audio` in `from..to` above the audible level; absolute frame index.
fn first_audible(audio: &[[f32; 2]], from: usize, to: usize) -> Option<usize> {
    audio[from..to]
        .iter()
        .position(|lr| lr.iter().any(|x| x.abs() > AUDIBLE))
        .map(|i| from + i)
}

fn c1_on_time(session: &mut Session) -> Result<Verdict> {
    let report = session.play(Pattern::Live, Mode::RealTime, BLOCK)?;
    let missing = report
        .notes
        .iter()
        .filter(|n| n.first_audible.is_none())
        .count();
    let worst = report
        .notes
        .iter()
        .filter_map(NoteReport::start_error)
        .map(i64::abs)
        .max()
        .unwrap_or(0);
    let detail = format!(
        "worst start error {worst} frames over {} notes; {missing} never audible",
        report.notes.len()
    );
    Ok(if missing == 0 && worst == 0 {
        Verdict::passed("C1", detail)
    } else {
        Verdict::failed("C1", detail)
    })
}

fn c4_block_size(session: &mut Session) -> Result<Verdict> {
    let reference = session.play(Pattern::Live, Mode::Offline, BLOCK)?;
    let mut worst = (f64::NEG_INFINITY, BLOCK);
    for block in BLOCK_SWEEP.into_iter().filter(|&b| b != BLOCK) {
        let other = session.play(Pattern::Live, Mode::Offline, block)?;
        let db = null_db(&reference.audio, &other.audio);
        if db > worst.0 {
            worst = (db, block);
        }
    }
    let detail = format!(
        "worst null {:.1} dB at block {} (limit {NULL_LIMIT_DB} dB)",
        worst.0, worst.1
    );
    Ok(if worst.0 < NULL_LIMIT_DB {
        Verdict::passed("C4", detail)
    } else {
        Verdict::failed("C4", detail)
    })
}

fn c5_offline_matches_realtime(session: &mut Session) -> Result<Verdict> {
    let realtime = session.play(Pattern::Live, Mode::RealTime, BLOCK)?;
    let offline = session.play(Pattern::Live, Mode::Offline, BLOCK)?;
    let db = null_db(&offline.audio, &realtime.audio);
    let detail = format!("null {db:.1} dB (limit {NULL_LIMIT_DB} dB)");
    Ok(if db < NULL_LIMIT_DB {
        Verdict::passed("C5", detail)
    } else {
        Verdict::failed("C5", detail)
    })
}

fn c6_voices_per_note(session: &mut Session) -> Result<Verdict> {
    let report = session.play(Pattern::Live, Mode::Offline, BLOCK)?;
    let counts: Vec<usize> = report.notes.iter().map(|n| n.voices).collect();
    let first = counts.first().copied().unwrap_or(0);
    Ok(if first > 0 && counts.iter().all(|&c| c == first) {
        Verdict::passed(
            "C6",
            format!("{first} voice(s) on every one of {} notes", counts.len()),
        )
    } else {
        Verdict::failed("C6", format!("voices per note vary: {counts:?}"))
    })
}

fn c8_dense_first_block(session: &mut Session) -> Result<Verdict> {
    let report = session.play(Pattern::Dense, Mode::RealTime, BLOCK)?;
    let on_first = report
        .notes
        .iter()
        .filter(|n| {
            n.start_error()
                .is_some_and(|e| (0..BLOCK as i64).contains(&e))
        })
        .count();
    let detail = format!(
        "{on_first}/{} chords audible in their first block",
        report.notes.len()
    );
    Ok(if on_first == report.notes.len() {
        Verdict::passed("C8", detail)
    } else {
        Verdict::failed("C8", detail)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_db_is_relative_energy_of_the_difference() {
        let a = vec![[1.0f32, -1.0]; 8];
        let half: Vec<[f32; 2]> = a.iter().map(|lr| lr.map(|x| x * 0.5)).collect();
        // Residual is half the signal in amplitude: a quarter in energy, about -6.02 dB.
        assert!((null_db(&a, &half) - -6.0206).abs() < 1e-3);
        assert_eq!(
            null_db(&a, &a),
            f64::NEG_INFINITY,
            "identical renders null completely"
        );
        assert_eq!(
            null_db(&[[0.0; 2]; 4], &[[0.0; 2]; 4]),
            f64::INFINITY,
            "silence fails"
        );
        assert_eq!(null_db(&a, &a[..4]), f64::INFINITY, "length mismatch fails");
    }

    #[test]
    fn first_audible_skips_silence_and_reports_absolute_frame() {
        let mut audio = vec![[0.0f32; 2]; 10];
        audio[6][1] = 0.5;
        assert_eq!(first_audible(&audio, 2, 10), Some(6));
        assert_eq!(first_audible(&audio, 2, 6), None);
    }

    #[test]
    fn dense_steps_hold_chords_and_live_steps_one_key() {
        let dense = plan(Pattern::Dense);
        assert_eq!(dense.notes.len(), STEPS);
        assert!(dense.notes.iter().all(|n| n.keys.len() == 4));
        let live = plan(Pattern::Live);
        assert!(live.notes.iter().all(|n| n.keys.len() == 1));
        let note_ons = live
            .events
            .iter()
            .filter(|(_, e)| matches!(e, Event::NoteOn { .. }))
            .count();
        assert_eq!(note_ons, STEPS, "one note-on per live step");
    }

    #[test]
    fn sweep_controllers_precede_their_note_on_the_same_frame() {
        let offsets = plan(Pattern::Offsets);
        let first_note = offsets.notes[0].scheduled as usize;
        let at_start: Vec<&Event> = offsets
            .events
            .iter()
            .filter(|(frame, _)| *frame == first_note)
            .map(|(_, event)| event)
            .collect();
        assert_eq!(at_start.len(), 2, "one sweep CC and one note-on");
        assert!(
            !matches!(at_start[0], Event::NoteOn { .. }),
            "the CC leads the note"
        );
        assert!(matches!(at_start[1], Event::NoteOn { .. }));
    }

    #[test]
    fn unknown_contract_is_an_error() {
        let mut session = Session {
            path: PathBuf::new(),
            rate: 48_000.0,
            primed: None,
        };
        assert!(check(&mut session, "C11").is_err());
    }

    #[test]
    fn pattern_parses_its_cli_names() {
        assert_eq!("dense".parse::<Pattern>(), Ok(Pattern::Dense));
        assert!("chords".parse::<Pattern>().is_err());
    }
}
