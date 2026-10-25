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

pub mod fixtures;
pub use fixtures::Fixture;

/// Host block size the contracts run at.
pub const BLOCK: usize = 128;
/// Host block sizes C4 compares against [`BLOCK`]. Sizes above [`MAX_BLOCK`] are
/// chunked to [`MAX_BLOCK`] by the engine, so they only test event splitting.
pub const BLOCK_SWEEP: [usize; 6] = [32, 64, 97, 128, 512, 1024];
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
/// Reference peak below this (linear, about -80 dBFS) is silence: a null against it proves nothing.
pub const SILENCE_PEAK: f32 = 1e-4;
/// Audible notes a null reference needs (half the steps).
pub const NULL_MIN_AUDIBLE: usize = 32;
/// Null-test limit for C4 and C5, in dB.
pub const NULL_LIMIT_DB: f64 = -120.0;
/// MIDI velocity of every note the probe sends.
const VELOCITY: u8 = 100;
/// CC113 (sample start) value sent before notes of patterns that do not sweep it.
const DEFAULT_START: u8 = 64;
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

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Live => "live",
            Self::Dense => "dense",
            Self::Offsets => "offsets",
            Self::Cc => "cc",
        })
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
    /// MIDI velocity sent with every key of the step.
    pub velocity: u8,
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
    /// Each recent voice's `(authored start, first non-silent output frame)`, absolute (C3).
    pub first_frames: Vec<(u64, u64)>,
    /// Script effects the run drained from the instrument: UI writes, `message`
    /// and other script output, not split by kind. C7 reads it as load evidence.
    pub script_effects: u64,
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
    /// The pattern the contract measured.
    pub pattern: Pattern,
}

impl Verdict {
    /// A measured pass.
    pub fn passed(id: &'static str, pattern: Pattern, detail: impl Into<String>) -> Self {
        Self {
            id,
            pass: Some(true),
            detail: detail.into(),
            pattern,
        }
    }

    /// A measured failure.
    pub fn failed(id: &'static str, pattern: Pattern, detail: impl Into<String>) -> Self {
        Self {
            id,
            pass: Some(false),
            detail: detail.into(),
            pattern,
        }
    }

    /// The contract is not measurable yet; `reason` says what is missing.
    pub fn unsupported(id: &'static str, pattern: Pattern, reason: &str) -> Self {
        Self {
            id,
            pass: None,
            detail: format!("unsupported: {reason}"),
            pattern,
        }
    }
}

/// One zone of the loaded instrument: the inclusive key and velocity ranges it plays.
#[derive(Debug, Clone, Copy)]
struct Zone {
    keys: (u8, u8),
    velocities: (u8, u8),
}

/// One loaded instrument. Each [`play`](Self::play) renders from a fresh core, so runs are independent.
pub struct Session {
    path: PathBuf,
    rate: f64,
    /// The instrument's zones; `None` for a plain audio file.
    zones: Option<Vec<Zone>>,
    /// Script slots that compiled; a slot that fails to compile is silently absent.
    scripts: usize,
    /// Voices one played key should start, when the fixture knows.
    expected: Option<usize>,
    /// UI writes one played key drives, when the fixture declares its load.
    /// `None` for a real instrument: the probe cannot tell those writes apart.
    ui_writes: Option<usize>,
    primed: Option<Loaded<<V2Core as Core>::Prepared>>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("path", &self.path)
            .field("rate", &self.rate)
            .field("zones", &self.zones.as_ref().map(Vec::len))
            .field("scripts", &self.scripts)
            .field("expected", &self.expected)
            .field("ui_writes", &self.ui_writes)
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
            zones: zones_of(&primed),
            scripts: primed.scripts.views.len(),
            expected: None,
            ui_writes: None,
            primed: Some(primed),
        })
    }

    /// Sample rate the instrument was loaded at.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Script slots that compiled when the instrument was loaded.
    pub fn scripts(&self) -> usize {
        self.scripts
    }

    /// Sets the voice count one played key is expected to start, which
    /// [`check`] C6 and C7 compare against.
    pub fn expect_voices(mut self, count: usize) -> Self {
        self.expected = Some(count);
        self
    }

    /// Declares the UI writes one played key drives, which [`check`] C7 needs as
    /// evidence that the load ran. Zero means the fixture has no UI load.
    pub fn expect_ui_writes(mut self, per_key: usize) -> Self {
        self.ui_writes = Some(per_key);
        self
    }

    /// Lowest and highest key any zone plays; `None` without a zone map.
    pub fn key_span(&self) -> Option<(u8, u8)> {
        let zones = self.zones.as_ref()?;
        Some((
            zones.iter().map(|z| z.keys.0).min()?,
            zones.iter().map(|z| z.keys.1).max()?,
        ))
    }

    /// Keys of `note` that no zone plays at its velocity; `None` without a zone map.
    /// Ignores conditions and articulation switches, so `Some(vec![])` is necessary, not sufficient.
    pub fn uncovered(&self, note: &NoteReport) -> Option<Vec<u8>> {
        let zones = self.zones.as_ref()?;
        Some(
            note.keys
                .iter()
                .copied()
                .filter(|&key| {
                    !zones.iter().any(|z| {
                        (z.keys.0..=z.keys.1).contains(&key)
                            && (z.velocities.0..=z.velocities.1).contains(&note.velocity)
                    })
                })
                .collect(),
        )
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
        let mut script_effects = 0;
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
            core.take_effects(0, &mut |_, _| {
                script_effects += 1;
                true
            });
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
            first_frames: core.voice_first_frames(),
            script_effects,
            audio: run.audio,
        })
    }
}

/// Runs contract `id` (`"C1"`..`"C10"`) against `session`.
///
/// `pattern` is the user's choice: C1 and C6 measure it. C4 and C5 always measure
/// live and C8 always dense; unsupported contracts record `pattern` as well.
///
/// # Errors
/// Fails on an unknown id or when a render cannot be prepared.
pub fn check(session: &mut Session, id: &str, pattern: Pattern) -> Result<Verdict> {
    Ok(match id {
        "C1" => c1_on_time(session, pattern)?,
        "C2" => Verdict::unsupported(
            "C2",
            pattern,
            "no degraded-disk fixture: the probe cannot throttle storage yet",
        ),
        "C3" => c3_start_spread(session, pattern)?,
        "C4" => c4_block_size(session)?,
        "C5" => c5_offline_matches_realtime(session)?,
        "C6" => c6_voices_per_note(session, pattern)?,
        "C7" => c7_script_load(session, pattern)?,
        "C8" => c8_dense_first_block(session)?,
        "C9" => c9_start_offsets(session)?,
        "C10" => Verdict::unsupported(
            "C10",
            pattern,
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

fn zones_of<P>(loaded: &Loaded<P>) -> Option<Vec<Zone>> {
    loaded.instrument.as_ref().map(|instrument| {
        instrument
            .zones
            .iter()
            .map(|z| Zone {
                keys: (z.keys.low, z.keys.high),
                velocities: (z.velocities.low, z.velocities.high),
            })
            .collect()
    })
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
        // The sweep starts late, not at zero: a first note at offset zero loads
        // the instrument's pages and hides the audited start-offset stall.
        let sweep = ((step * 2 + 96) % 128) as u8;
        let velocity = f64::from(VELOCITY) / 127.0;
        // CC113 is the start offset of the cc_start fixture: every pattern gives
        // it a value so a script that reads it never sees a default of 0.
        match pattern {
            Pattern::Offsets => events.push((at, Event::midi1(0xb0, 113, sweep))),
            Pattern::Cc => {
                events.push((at, Event::midi1(0xb0, 113, DEFAULT_START)));
                events.push((at, Event::midi1(0xb0, 1, sweep)));
            }
            Pattern::Live | Pattern::Dense => {
                events.push((at, Event::midi1(0xb0, 113, DEFAULT_START)));
            }
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
                    velocity,
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
            velocity: VELOCITY,
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

fn c1_on_time(session: &mut Session, pattern: Pattern) -> Result<Verdict> {
    on_time(session, "C1", pattern, "")
}

/// C9 is C1 with a start offset on every note: CC113 sweeps 0..126, which the
/// fixture turns into a source offset of up to 30 % of the sample.
fn c9_start_offsets(session: &mut Session) -> Result<Verdict> {
    on_time(session, "C9", Pattern::Offsets, "CC113 0..126 every note; ")
}

fn on_time(
    session: &mut Session,
    id: &'static str,
    pattern: Pattern,
    note: &str,
) -> Result<Verdict> {
    let report = session.play(pattern, Mode::RealTime, BLOCK)?;
    Ok(on_time_verdict(&report, id, pattern, note))
}

/// C1 on a real-time render: every note audible at its scheduled frame.
fn on_time_verdict(report: &Report, id: &'static str, pattern: Pattern, note: &str) -> Verdict {
    let missing = audible_missing(report);
    let worst = report
        .notes
        .iter()
        .filter_map(NoteReport::start_error)
        .map(i64::abs)
        .max()
        .unwrap_or(0);
    let detail = format!(
        "{note}worst start error {worst} frames over {} notes; {missing} never audible",
        report.notes.len()
    );
    if missing == 0 && worst == 0 {
        Verdict::passed(id, pattern, detail)
    } else {
        Verdict::failed(id, pattern, detail)
    }
}

/// Null test in C4 and C5 on the live pattern at real-time block size.
fn c4_block_size(session: &mut Session) -> Result<Verdict> {
    let reference = session.play(Pattern::Live, Mode::Offline, BLOCK)?;
    if let Some(gap) = null_gap(&reference) {
        return Ok(Verdict::failed(
            "C4",
            Pattern::Live,
            format!("null not measured: {gap}"),
        ));
    }
    let mut worst = (f64::NEG_INFINITY, BLOCK);
    for block in BLOCK_SWEEP.into_iter().filter(|&b| b != BLOCK) {
        let other = session.play(Pattern::Live, Mode::Offline, block)?;
        let db = null_db(&reference.audio, &other.audio);
        if db > worst.0 {
            worst = (db, block);
        }
    }
    let (native, chunked): (Vec<usize>, Vec<usize>) =
        BLOCK_SWEEP.into_iter().partition(|&b| b <= MAX_BLOCK);
    let detail = format!(
        "reference {}; worst null {:.1} dB at block {} (limit {NULL_LIMIT_DB} dB); \
         blocks {native:?} run natively, {chunked:?} chunked to {MAX_BLOCK} by the engine",
        level_text(&reference),
        worst.0,
        worst.1
    );
    Ok(if worst.0 < NULL_LIMIT_DB {
        Verdict::passed("C4", Pattern::Live, detail)
    } else {
        Verdict::failed("C4", Pattern::Live, detail)
    })
}

fn c5_offline_matches_realtime(session: &mut Session) -> Result<Verdict> {
    let realtime = session.play(Pattern::Live, Mode::RealTime, BLOCK)?;
    if let Some(gap) = null_gap(&realtime) {
        return Ok(Verdict::failed(
            "C5",
            Pattern::Live,
            format!("null not measured: {gap}"),
        ));
    }
    let offline = session.play(Pattern::Live, Mode::Offline, BLOCK)?;
    let db = null_db(&offline.audio, &realtime.audio);
    let detail = format!(
        "reference {}; null {db:.1} dB (limit {NULL_LIMIT_DB} dB)",
        level_text(&realtime)
    );
    Ok(if db < NULL_LIMIT_DB {
        Verdict::passed("C5", Pattern::Live, detail)
    } else {
        Verdict::failed("C5", Pattern::Live, detail)
    })
}

fn c6_voices_per_note(session: &mut Session, pattern: Pattern) -> Result<Verdict> {
    let report = session.play(pattern, Mode::Offline, BLOCK)?;
    Ok(voices_verdict(session, &report, pattern))
}

/// Largest allowed spread, in frames, between the first output frames of the
/// voices of one note. Zero: every voice of a note starts on one sample.
pub const C3_TOLERANCE: u64 = 0;

/// C3 on a real-time render: the voices of each note start together. Each
/// voice's first non-silent frame is read from the engine, so a mic group that
/// starts alone shows up as a nonzero spread.
fn c3_start_spread(session: &mut Session, pattern: Pattern) -> Result<Verdict> {
    let report = session.play(pattern, Mode::RealTime, BLOCK)?;
    Ok(c3_verdict(&report, pattern))
}

/// C3 on a rendered run. Voices are grouped by their authored start instant, so a
/// release-triggered voice (its own instant, inside the same step) never counts
/// against the attack voices of its note.
fn c3_verdict(report: &Report, pattern: Pattern) -> Verdict {
    let mut worst = 0u64;
    let mut silent = Vec::new();
    let mut voices = 0;
    for note in &report.notes {
        let window = note.scheduled..note.scheduled + STEP as u64;
        let mut starts: Vec<(u64, u64)> = report
            .first_frames
            .iter()
            .copied()
            .filter(|(start, _)| window.contains(start))
            .collect();
        if starts.is_empty() {
            silent.push(note.step);
            continue;
        }
        voices += starts.len();
        starts.sort_unstable();
        for same in starts.chunk_by(|a, b| a.0 == b.0) {
            let lo = same.iter().map(|s| s.1).min().unwrap_or(0);
            let hi = same.iter().map(|s| s.1).max().unwrap_or(0);
            worst = worst.max(hi - lo);
        }
    }
    let detail = format!(
        "{} notes, {voices} voices with a first frame; worst spread {worst} frame(s) between voices of one start, tolerance {C3_TOLERANCE}; notes with no voice: {silent:?}",
        report.notes.len(),
    );
    if silent.is_empty() && worst <= C3_TOLERANCE {
        Verdict::passed("C3", pattern, detail)
    } else {
        Verdict::failed("C3", pattern, detail)
    }
}

/// C6 on a real-time render, paced like a host. Offline C6 cannot show a double
/// attack because the cold start is off offline, so the real-time variant is the
/// one that catches a mic group starting alone.
///
/// # Errors
/// Fails when the render cannot be prepared.
pub fn check_c6_realtime(session: &mut Session, pattern: Pattern) -> Result<Verdict> {
    let report = session.play(pattern, Mode::RealTime, BLOCK)?;
    Ok(voices_verdict(session, &report, pattern))
}

/// C6 on a rendered run. A note's voices are counted per key: a chord of four
/// keys that starts one voice per key sounds four voices, which is correct.
fn voices_verdict(session: &Session, report: &Report, pattern: Pattern) -> Verdict {
    // `None` when the voices do not split evenly over the note's keys.
    let per_key: Vec<Option<usize>> = report
        .notes
        .iter()
        .map(|n| (n.voices % n.keys.len() == 0).then(|| n.voices / n.keys.len()))
        .collect();
    let first = per_key.first().copied().flatten().unwrap_or(0);
    let (ok, expected) = match session.expected {
        Some(n) => (
            per_key.iter().all(|&c| c == Some(n)),
            format!("{n} voice(s) expected per key on every note"),
        ),
        None => (
            first > 0 && per_key.iter().all(|&c| c == Some(first)),
            "voices per key equal on every note and above zero".to_owned(),
        ),
    };
    if ok {
        Verdict::passed(
            "C6",
            pattern,
            format!(
                "{first} voice(s) per key on every one of {} notes; {expected}",
                per_key.len()
            ),
        )
    } else {
        let silent: Vec<String> = report
            .notes
            .iter()
            .filter(|n| n.voices == 0)
            .map(|n| silent_note(session, n))
            .collect();
        let mut detail = format!("{expected}; voices per key: {per_key:?}");
        if !silent.is_empty() {
            detail.push_str(&format!(
                "; {} with no voices: {}",
                silent.len(),
                silent.join("; ")
            ));
        }
        Verdict::failed("C6", pattern, detail)
    }
}

/// What was sent for a note that sounded no voices, and whether a zone plays it.
fn silent_note(session: &Session, note: &NoteReport) -> String {
    let zones = match session.uncovered(note) {
        None => "no zone map to check against".to_owned(),
        Some(keys) if keys.is_empty() => "a zone plays every key".to_owned(),
        Some(keys) => format!("no zone for keys {keys:?}"),
    };
    format!(
        "step {} keys {:?} velocity {}: {zones}",
        note.step, note.keys, note.velocity
    )
}

/// C7 is C1 and C6 under the fixture's UI load. Passing needs evidence that the
/// load ran: the script must have drained at least its declared writes for every
/// key played, in both renders. Without that evidence the contract is unsupported,
/// never a pass.
fn c7_script_load(session: &mut Session, pattern: Pattern) -> Result<Verdict> {
    let writes = match session.ui_writes {
        None => {
            return Ok(Verdict::unsupported(
                "C7",
                pattern,
                "a real instrument declares no UI load, and the public API cannot tell \
                 its UI writes from its other script effects",
            ));
        }
        Some(0) => {
            return Ok(Verdict::unsupported(
                "C7",
                pattern,
                "this fixture declares no UI load",
            ));
        }
        Some(writes) => writes,
    };
    let realtime = session.play(pattern, Mode::RealTime, BLOCK)?;
    let offline = session.play(pattern, Mode::Offline, BLOCK)?;
    let keys: usize = realtime.notes.iter().map(|n| n.keys.len()).sum();
    let wanted = (writes * keys) as u64;
    if realtime.script_effects < wanted || offline.script_effects < wanted {
        // The fixture declares the load, so a shortfall means writes were lost, not unmeasured.
        return Ok(Verdict::failed(
            "C7",
            pattern,
            format!(
                "UI writes lost: {} and {} script effects drained (real time and offline), \
                 {wanted} expected from {writes} UI writes on each of {keys} keys",
                realtime.script_effects, offline.script_effects
            ),
        ));
    }
    let on_time = on_time_verdict(&realtime, "C1", pattern, "");
    let voices = voices_verdict(session, &offline, pattern);
    let pass = on_time.pass == Some(true) && voices.pass == Some(true);
    let detail = format!(
        "{} script effects drained (at least {wanted} expected); C1: {}; C6: {}",
        realtime.script_effects.min(offline.script_effects),
        on_time.detail,
        voices.detail
    );
    Ok(if pass {
        Verdict::passed("C7", pattern, detail)
    } else {
        Verdict::failed("C7", pattern, detail)
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
        Verdict::passed("C8", Pattern::Dense, detail)
    } else {
        Verdict::failed("C8", Pattern::Dense, detail)
    })
}

/// Notes that never reached the audible level.
fn audible_missing(report: &Report) -> usize {
    report
        .notes
        .iter()
        .filter(|n| n.first_audible.is_none())
        .count()
}

/// Why `reference` cannot anchor a null test, or `None` when it can.
fn null_gap(reference: &Report) -> Option<String> {
    let (peak, _) = level(&reference.audio);
    let audible = report_audible(reference);
    if peak < SILENCE_PEAK {
        Some(format!("reference silent ({})", level_text(reference)))
    } else if audible < NULL_MIN_AUDIBLE {
        Some(format!(
            "reference needs {NULL_MIN_AUDIBLE} audible notes ({})",
            level_text(reference)
        ))
    } else {
        None
    }
}

fn report_audible(report: &Report) -> usize {
    report.notes.len() - audible_missing(report)
}

/// Peak and RMS of the stereo output as linear amplitudes.
fn level(audio: &[[f32; 2]]) -> (f32, f64) {
    let peak = audio
        .iter()
        .flatten()
        .fold(0.0f32, |peak, x| peak.max(x.abs()));
    let energy: f64 = audio.iter().flatten().map(|x| f64::from(*x).powi(2)).sum();
    let samples = audio.len() * 2;
    let rms = if samples == 0 {
        0.0
    } else {
        (energy / samples as f64).sqrt()
    };
    (peak, rms)
}

/// Peak, RMS and audible count of a reference render, for a null detail.
fn level_text(report: &Report) -> String {
    let (peak, rms) = level(&report.audio);
    format!(
        "peak {:.1} dBFS, rms {:.1} dBFS, {}/{} notes audible",
        20.0 * f64::from(peak).log10(),
        20.0 * rms.log10(),
        report_audible(report),
        report.notes.len()
    )
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
    fn every_live_step_carries_a_start_offset_value() {
        let live = plan(Pattern::Live);
        let first_note = live.notes[0].scheduled as usize;
        let controllers: Vec<(u8, u8)> = live
            .events
            .iter()
            .filter(|(frame, _)| *frame == first_note)
            .filter_map(|(_, event)| match event {
                Event::Ump([word, _]) => Some((((word >> 8) & 0x7f) as u8, (word & 0x7f) as u8)),
                _ => None,
            })
            .collect();
        assert_eq!(
            controllers,
            vec![(113, DEFAULT_START)],
            "a script that reads CC113 always sees a value"
        );
    }

    #[test]
    fn unknown_contract_is_an_error() {
        let mut session = Session {
            path: PathBuf::new(),
            rate: 48_000.0,
            zones: None,
            scripts: 0,
            expected: None,
            ui_writes: None,
            primed: None,
        };
        assert!(check(&mut session, "C11", Pattern::Live).is_err());
    }

    #[test]
    fn null_reference_must_be_audible_enough() {
        let note = |step: usize, audible: bool| NoteReport {
            step,
            keys: vec![60],
            velocity: VELOCITY,
            scheduled: 0,
            first_audible: audible.then_some(0),
            voices: 1,
            dropouts: 0,
        };
        let report = |audio: f32, audible: usize| Report {
            pattern: Pattern::Live,
            mode: Mode::Offline,
            block: BLOCK,
            notes: (0..STEPS).map(|s| note(s, s < audible)).collect(),
            first_frames: Vec::new(),
            script_effects: 0,
            audio: vec![[audio, audio]; 8],
        };
        let silent = null_gap(&report(0.0, STEPS)).expect("silence is refused");
        assert!(silent.starts_with("reference silent"), "{silent}");
        let sparse = null_gap(&report(0.5, NULL_MIN_AUDIBLE - 1)).expect("sparse is refused");
        assert!(sparse.contains("audible notes"), "{sparse}");
        assert!(null_gap(&report(0.5, NULL_MIN_AUDIBLE)).is_none());
    }

    #[test]
    fn uncovered_keys_are_those_no_zone_plays_at_the_velocity() {
        let session = Session {
            path: PathBuf::new(),
            rate: 48_000.0,
            zones: Some(vec![Zone {
                keys: (55, 96),
                velocities: (1, 127),
            }]),
            scripts: 0,
            expected: None,
            ui_writes: None,
            primed: None,
        };
        let note = NoteReport {
            step: 0,
            keys: vec![48, 55, 97],
            velocity: VELOCITY,
            scheduled: 0,
            first_audible: None,
            voices: 0,
            dropouts: 0,
        };
        assert_eq!(session.uncovered(&note), Some(vec![48, 97]));
        assert_eq!(session.key_span(), Some((55, 96)));
        let quiet = NoteReport {
            velocity: 0,
            ..note
        };
        assert_eq!(session.uncovered(&quiet), Some(vec![48, 55, 97]));
    }

    #[test]
    fn pattern_parses_its_cli_names() {
        assert_eq!("dense".parse::<Pattern>(), Ok(Pattern::Dense));
        assert!("chords".parse::<Pattern>().is_err());
    }
}

#[cfg(test)]
mod c3_tests {
    use super::*;

    fn report(first_frames: Vec<(u64, u64)>) -> Report {
        Report {
            pattern: Pattern::Live,
            mode: Mode::RealTime,
            block: BLOCK,
            notes: vec![NoteReport {
                step: 0,
                keys: vec![60],
                velocity: 100,
                scheduled: 1000,
                first_audible: Some(1000),
                voices: 3,
                dropouts: 0,
            }],
            first_frames,
            script_effects: 0,
            audio: Vec::new(),
        }
    }

    #[test]
    fn c3_compares_only_voices_of_one_authored_start() {
        // A release voice starts at its own instant inside the same step: not a spread.
        let with_release = report(vec![(1000, 1000), (1000, 1000), (1000, 1000), (1500, 1400)]);
        assert_eq!(c3_verdict(&with_release, Pattern::Live).pass, Some(true));
        // Two voices of one instant that sound apart: a split.
        let split = report(vec![(1000, 1000), (1000, 1100)]);
        assert_eq!(c3_verdict(&split, Pattern::Live).pass, Some(false));
    }
}
