//! One instrument, one note, one WAV: the offline render parity is measured against.
//!
//! Loads an instrument the way a user would (a quarter second of silence before the
//! first note, so a script that waits while it loads has run), plays a note or a MIDI
//! file through the engine, and writes the result as 32-bit float WAV. `kontra-render`
//! is the command-line front end; `crates/app/host/examples/render.rs` is a shim that
//! keeps the older positional invocation working.
//!
//! The summary ([`Summary`]) is the stable part: it is what `tools/oracle-mcp/parity.py`
//! and the sweep read, so a field is never renamed or repurposed without bumping
//! [`Summary::FORMAT`].
//!
//! Beyond the front-end flags, the same `KONTRA_*` environment variables the example
//! grew for research still work: `KONTRA_CC`, `KONTRA_CONTROL`, `KONTRA_WIDGET`,
//! `KONTRA_SCRIPT_DUMP`, `KONTRA_SCRIPT_SOURCE`, `KONTRA_FX`, `KONTRA_MODS`,
//! `KONTRA_ZONE`, `KONTRA_SAMPLES`, `KONTRA_WATCH`, `KONTRA_BEND`, `KONTRA_REPEATS`,
//! `KONTRA_CHORDS`, `KONTRA_LIVE`, `KONTRA_VELOCITY`, `KONTRA_HOLD`, `KONTRA_TAIL`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, Prepared, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};

/// The rate every comparison assumes; `tools/oracle/compare.py` asserts 48 kHz.
pub const DEFAULT_RATE: u32 = 48_000;
/// The host block the engine is driven in, in frames.
const BLOCK: usize = 512;
/// The note `tools/oracle/compare.py` compares on: velocity 100 held 1.5 s.
pub const ORACLE_VELOCITY: u8 = 100;
/// See [`ORACLE_VELOCITY`].
pub const ORACLE_HOLD_S: f32 = 1.5;
/// Silence after the last key-up by default, so a release and its tail are in the WAV.
pub const DEFAULT_TAIL_S: f32 = 2.0;

/// A diagnostic line, to wherever the front end points them.
macro_rules! say {
    ($w:expr, $($arg:tt)*) => {{
        writeln!($w, $($arg)*).map_err(|e| format!("writing diagnostics: {e}"))?;
    }};
}

/// The note a single-note render plays.
#[derive(Clone, Copy, Debug)]
pub struct Note {
    /// MIDI key number.
    pub key: u8,
    /// MIDI velocity, 1..=127.
    pub velocity: u8,
    /// How long the key is held down, in seconds.
    pub hold_s: f32,
}

impl Default for Note {
    fn default() -> Self {
        Self {
            key: 60,
            velocity: ORACLE_VELOCITY,
            hold_s: ORACLE_HOLD_S,
        }
    }
}

/// What to render.
#[derive(Clone, Debug)]
pub struct Request {
    /// The instrument: a Kontakt instrument, or a UVI program as `bank.ufs/Name.uvip`.
    pub instrument: PathBuf,
    /// Where the WAV goes.
    pub out: PathBuf,
    /// Output rate in Hz.
    pub rate: u32,
    /// The note, when no MIDI file is given.
    pub note: Note,
    /// A standard MIDI file to play instead of [`Request::note`].
    pub midi: Option<PathBuf>,
    /// The mod wheel moves to this value one second into the render.
    pub wheel: Option<u8>,
    /// Seconds rendered after the last key-up.
    pub tail_s: f32,
    /// Render in real time without waiting for the disk (`KONTRA_LIVE`).
    pub live: bool,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            instrument: PathBuf::new(),
            out: PathBuf::from("render.wav"),
            rate: DEFAULT_RATE,
            note: Note::default(),
            midi: None,
            wheel: None,
            tail_s: DEFAULT_TAIL_S,
            live: false,
        }
    }
}

/// What one render produced. Field order is the JSON's key order, so keep it stable.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Summary {
    /// The format version of this structure; bump when a field changes meaning.
    pub format: u32,
    /// The instrument that was loaded.
    pub instrument: String,
    /// The WAV that was written.
    pub wav: String,
    /// The instrument's name as the engine reports it.
    pub name: String,
    /// Output rate in Hz.
    pub rate: u32,
    /// Interleaved channels in the WAV.
    pub channels: u16,
    /// Frames per channel written.
    pub frames: u64,
    /// `frames / rate`.
    pub duration_s: f64,
    /// Largest absolute sample.
    pub peak: f64,
    /// [`Summary::peak`] in dBFS.
    pub peak_dbfs: f64,
    /// RMS over every written frame and channel.
    pub rms: f64,
    /// [`Summary::rms`] in dBFS.
    pub rms_dbfs: f64,
    /// SHA-256 of the WAV file's bytes.
    pub sha256: String,
    /// The note played, when the render came from a note rather than a MIDI file.
    pub note: Option<NoteSummary>,
    /// The MIDI file played, when one was given.
    pub midi: Option<String>,
    /// Note-on events the played input held.
    pub notes: u64,
    /// Zones the loaded program has.
    pub zones: u64,
    /// Seconds the load took.
    pub load_s: f64,
    /// Milliseconds inside `Engine::process` alone, for the whole render.
    pub cpu_ms: f64,
    /// CPU time over audio time, the figure the example prints as `rtf`.
    pub realtime_factor: f64,
}

impl Summary {
    /// The current [`Summary::format`].
    pub const FORMAT: u32 = 1;
}

/// The note that was rendered, as it went in.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct NoteSummary {
    /// MIDI key number.
    pub key: u8,
    /// MIDI velocity.
    pub velocity: u8,
    /// Held, in seconds.
    pub hold_s: f32,
}

/// Renders `req`, writes the WAV, and returns what it measured.
///
/// `diag` receives one line per fact; the summary is returned, not printed, so the
/// front end decides where each goes.
///
/// # Errors
/// The instrument does not read or build, a sample does not open, the MIDI file does
/// not parse, the WAV cannot be written, or `KONTRA_INIT_ONLY` asked for no render.
pub fn run(req: &Request, diag: &mut dyn Write) -> Result<Summary, Box<dyn std::error::Error>> {
    let rate = req.rate;
    let instrument = req.instrument.clone();
    // KONTRA_SCRIPT_DUMP=<dir> writes each script's source, data and saved values there.
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_DUMP") {
        for (n, s) in crate::instrument::read(&instrument)?
            .scripts
            .iter()
            .enumerate()
        {
            std::fs::write(Path::new(&dir).join(format!("{n}.ksp")), &s.source)?;
            for (name, bytes) in &s.data {
                std::fs::write(
                    // A UVI module's name is a path in its bank.
                    Path::new(&dir).join(format!("{n}-{}.nka", name.replace('/', "_"))),
                    bytes,
                )?;
            }
            // Its saved persistent values, one per line.
            std::fs::write(
                Path::new(&dir).join(format!("{n}-saved.txt")),
                s.saved.join("\n"),
            )?;
            let data: Vec<_> = s.data.iter().map(|(name, b)| (name, b.len())).collect();
            say!(diag, "  script {n} data files: {data:?}");
        }
    }
    // KONTRA_SAMPLES=<part of a name>: those samples' peaks, decoded directly.
    if let Ok(part) = std::env::var("KONTRA_SAMPLES") {
        let ir = crate::instrument::read(&instrument)?;
        for s in ir
            .samples
            .iter()
            .filter(|s| s.name.contains(&part))
            .take(12)
        {
            let Ok((source, fmt)) = kontra_stream::open(s) else {
                say!(diag, "  {}: does not open", s.name);
                continue;
            };
            let mut bytes = vec![0u8; s.frames as usize * fmt.bytes_per_frame()];
            let read = source.read(0, &mut bytes);
            let width = fmt.encoding.bytes();
            let peak = bytes.chunks_exact(width).fold(0f32, |p, b| {
                let x = match b {
                    [a, b] => f32::from(i16::from_le_bytes([*a, *b])) / 32768.0,
                    [a, b, c] => (i32::from_le_bytes([0, *a, *b, *c]) >> 8) as f32 / 8_388_608.0,
                    [a, b, c, d] => f32::from_le_bytes([*a, *b, *c, *d]),
                    _ => 0.0,
                };
                p.max(x.abs())
            });
            say!(
                diag,
                "  {} {:?} {} frames: peak {peak:.4} {read:?}",
                s.name,
                fmt,
                s.frames
            );
        }
    }
    let t = std::time::Instant::now();
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let mut ir = crate::instrument::read(&instrument)?;
    // KONTRA_SCRIPT_SOURCE=<dir>: script n's source is replaced by <dir>/n.ksp
    // when that file exists (edit a dump, add messages, render again).
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_SOURCE") {
        for (n, s) in ir.scripts.iter_mut().enumerate() {
            if let Ok(source) = std::fs::read_to_string(Path::new(&dir).join(format!("{n}.ksp"))) {
                s.source = source;
            }
        }
        // <dir>/append.ksp plays as one more script slot (a probe the oracle runs too).
        if let Ok(source) = std::fs::read_to_string(Path::new(&dir).join("append.ksp")) {
            ir.scripts.push(kontra_engine::ir::Script {
                name: "append".into(),
                slot: ir.scripts.len(),
                dialect: kontra_engine::ir::Dialect::Ksp,
                source,
                saved: Vec::new(),
                data: Vec::new(),
                links: Vec::new(),
            });
        }
    }
    let scripts: Vec<String> = ir.scripts.iter().map(|s| s.source.clone()).collect();
    let script_names: Vec<String> = ir.scripts.iter().map(|s| s.name.clone()).collect();
    let loaded = crate::instrument::build(ir, rate, &mut store);
    let name = loaded.name.clone();
    let pages = loaded.pages.clone();
    let load_s = t.elapsed().as_secs_f64();
    say!(
        diag,
        "{name}: {} zones, loaded in {:?}",
        loaded.plan.zones.len(),
        t.elapsed()
    );
    // The script slots the reader found, by name. Kontakt 8 keeps them in the
    // program's private data, and a program whose slots do not read looks
    // exactly like one with no scripts: Cherry Electric Bass loses 44 dB of
    // level that way, with nothing else in the report to say why.
    say!(
        diag,
        "  scripts: {} slots {script_names:?}",
        script_names.len()
    );
    say!(
        diag,
        "  memory: {}, pinned samples {} MB",
        memory(),
        store.pinned_bytes() / 1_000_000
    );
    if !loaded.plan.switches.is_empty() {
        say!(
            diag,
            "  switches {:?}, default {:?}",
            loaded.plan.switches,
            loaded.plan.default_keyswitch
        );
    }
    for d in &loaded.diagnostics {
        say!(diag, "  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    let plan = Arc::new(loaded.plan);
    let stream = store.client();
    let mut engine = Engine::new(Arc::clone(&plan), store.client(), rate);
    say!(diag, "init: unanswered {:?}", engine.unhandled_actions());
    if std::env::var_os("KONTRA_MODS").is_some() {
        let mut seen = Vec::new();
        for m in engine.mods().filter(|m| !m.bypass) {
            let line = format!("{} {:?}", m.name, m.kind);
            if !seen.contains(&line) {
                say!(diag, "live mod {line}");
                seen.push(line);
            }
        }
    }
    // The program as its scripts leave it, without playing it.
    if std::env::var_os("KONTRA_INIT_ONLY").is_some() {
        return Err("KONTRA_INIT_ONLY is set: nothing was rendered".into());
    }
    let (chains, chained) = engine.eq_chains();
    if chains > 0 {
        say!(diag, "eq: {chains} chains over {chained} zones");
    }
    let purged: Vec<usize> = engine.purged().collect();
    if !purged.is_empty() {
        let zones = (plan.zones.iter())
            .filter(|z| purged.contains(&(z.group as usize)))
            .count();
        say!(
            diag,
            "purged: {} groups, {zones} of {} zones",
            purged.len(),
            plan.zones.len()
        );
    }
    // A quarter second after load before the first note, as a player gives a
    // program: a script may wait while it loads (UVI `onLoad`).
    let mut silent = vec![[0.0f32; 2]; BLOCK];
    for _ in 0..(rate as usize / 4 / BLOCK) {
        engine.process(&mut silent, &[]);
        install_requests(&mut engine, &plan, rate, diag);
    }
    let env = |name: &str| std::env::var(name).ok()?.parse::<f32>().ok();
    // KONTRA_VELOCITY (1..=127) and KONTRA_HOLD (seconds) override the note:
    // every note this render plays, the single note included (see
    // [`note_events`]), so a knob sweep can move velocity without editing the
    // command line. Unset, the front end's own `--velocity` / `--hold` stand.
    // A mistyped override must not silently measure a different note: velocity
    // outside 1..=127 would leave the MIDI range, and a hold that is not a
    // positive finite duration would schedule the NoteOff at frame 0 (or wrap).
    // An invalid value is ignored, with a line saying so, rather than clamped --
    // clamping would render a note nobody asked for and call it a measurement.
    let asked_velocity = env("KONTRA_VELOCITY");
    let asked_hold = env("KONTRA_HOLD");
    let velocity_override = checked_velocity(asked_velocity);
    let hold_override = checked_hold(asked_hold);
    if asked_velocity.is_some() && velocity_override.is_none() {
        invalid(diag, "KONTRA_VELOCITY", "1..=127")?;
    }
    if asked_hold.is_some() && hold_override.is_none() {
        invalid(diag, "KONTRA_HOLD", "a positive number of seconds")?;
    }
    let velocity = f64::from(velocity_override.unwrap_or(100.0)) / 127.0;
    // KONTRA_CONTROL=name=value,...: each script control (a KSP one named
    // without its type prefix, a UVI widget or macro by its name) moves there
    // in turn and its callbacks run, as a user's edit would; its effect loads
    // install over a quarter second. "name=value@1.5" moves it 1.5 s into
    // the render instead, at the start of that block.
    let mut timed_controls = Vec::new();
    for entry in std::env::var("KONTRA_CONTROL")
        .iter()
        .flat_map(|s| s.split(','))
    {
        let Some((name, rest)) = entry.split_once('=') else {
            continue;
        };
        let (value, at) = rest.split_once('@').unwrap_or((rest, "0"));
        let (Some((slot, id, value)), Ok(at)) =
            (control(&plan, &scripts, name, value), at.parse::<f32>())
        else {
            say!(diag, "control {name}: no such control or value {rest}");
            continue;
        };
        if at > 0.0 {
            timed_controls.push(((at * rate as f32) as usize / BLOCK, slot, id, value));
            continue;
        }
        say!(diag, "control {name} (script {slot}, id {id}) := {value}");
        engine.set_control(slot, id, value);
        for _ in 0..(rate as usize / 4 / BLOCK) {
            engine.process(&mut silent, &[]);
            install_requests(&mut engine, &plan, rate, diag);
        }
    }
    // KONTRA_WIDGET=name=value,...: a Lua script's own widget moves there,
    // as the editor moves it, and its `changed` runs.
    for pair in std::env::var("KONTRA_WIDGET")
        .iter()
        .flat_map(|s| s.split(','))
    {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let Ok(value) = value.parse::<f64>() else {
            continue;
        };
        let found = pages.iter().enumerate().find_map(|(slot, page)| {
            let n = page.as_ref()?.widgets.iter().position(|w| w.name == name)?;
            Some((u8::try_from(slot).ok()?, u32::try_from(n).ok()?))
        });
        let Some((slot, n)) = found else {
            say!(diag, "widget {name}: no such widget");
            continue;
        };
        say!(diag, "widget {name} (script {slot}, number {n}) := {value}");
        engine.set_widget(slot, n, value, None);
        for _ in 0..(rate as usize / 4 / BLOCK) {
            engine.process(&mut silent, &[]);
            install_requests(&mut engine, &plan, rate, diag);
        }
    }
    let before: std::collections::HashMap<_, _> = engine.action_calls().into_iter().collect();

    // The input: a MIDI file's events, or the one note plus whatever the KONTRA_*
    // controller variables add. Frames count from the first block.
    let mut events: Vec<FrameEvent> = Vec::new();
    let single = match &req.midi {
        Some(path) => {
            let bytes = std::fs::read(path)?;
            let (from_midi, _length_s) = midi_events(&bytes, rate)?;
            events.extend(from_midi);
            None
        }
        None => Some(req.note),
    };
    // The note that is actually played. `Summary.note` reports this, not the
    // request: a run with KONTRA_VELOCITY set once recorded the requested
    // velocity beside a WAV rendered at another, which is a measurement that
    // cannot be reproduced from its own description.
    let effective = single.map(|note| effective_note(note, velocity_override, hold_override));
    if let Some(note) = effective {
        events.extend(note_events(note, rate as f32, None, None));
    }
    // The wheel (the example's fourth positional argument) moves at 1 s.
    if let Some(value) = req.wheel {
        events.push((rate as usize, Event::midi1(0xB0, 1, value & 127)));
    }
    // KONTRA_WATCH=102,104: the controllers the block lines print (110, 111).
    let watched: Vec<usize> = std::env::var("KONTRA_WATCH")
        .unwrap_or_else(|_| "110,111".into())
        .split(',')
        .filter_map(|c| c.trim().parse().ok())
        .collect();
    // KONTRA_BEND=0: pitch bend (its high 7 bits) sent with the wheel at 1 s.
    if let Some(bend) = std::env::var("KONTRA_BEND")
        .ok()
        .and_then(|b| b.parse::<u16>().ok())
    {
        let value = bend & 0x3fff;
        events.push((
            rate as usize,
            Event::midi1(0xE0, (value & 127) as u8, (value >> 7 & 127) as u8),
        ));
    }
    // KONTRA_CC="11=127,1=100": controllers sent with the note-on;
    // "7:64@1.5" (or "7=64@1.5") sends one 1.5 s into the render, on its frame;
    // "pb:16383@1" is a 14-bit pitch bend.
    for entry in std::env::var("KONTRA_CC")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let Some((cc, rest)) = entry.split_once(['=', ':']) else {
            continue;
        };
        let (v, at) = rest.split_once('@').unwrap_or((rest, "0"));
        let (Ok(v), Ok(at)) = (v.trim().parse::<u16>(), at.trim().parse::<f32>()) else {
            say!(diag, "cc {entry}: not `cc=value[@seconds]`");
            continue;
        };
        let body = match cc.trim() {
            "pb" => Event::midi1(0xE0, (v & 127) as u8, (v >> 7 & 127) as u8),
            cc => match cc.parse::<u8>() {
                Ok(cc) => Event::midi1(0xB0, cc & 127, (v & 127) as u8),
                Err(_) => {
                    say!(diag, "cc {entry}: not a controller number");
                    continue;
                }
            },
        };
        events.push(((at * rate as f32) as usize, body));
    }
    // KONTRA_REPEATS=62:16:70:45: after the note, 16 notes on key 62, one
    // every 70 ms from 0.1 s, each 45 ms long (a fast repeat).
    if let Some([k, count, period, len]) = std::env::var("KONTRA_REPEATS").ok().and_then(|r| {
        let v: Vec<u32> = r.split(':').filter_map(|x| x.parse().ok()).collect();
        <[u32; 4]>::try_from(v).ok()
    }) {
        let frames = |ms: u32| ms as usize * rate as usize / 1000;
        let key = u8::try_from(k).unwrap_or(u8::MAX);
        for i in 0..count {
            let at = frames(100 + i * period);
            events.push((
                at,
                Event::NoteOn {
                    note: host_note(key, 0),
                    velocity,
                    tune: 0.0,
                },
            ));
            events.push((at + frames(len), Event::NoteOff(host_pattern(key, 0))));
        }
    }
    // KONTRA_CHORDS=1: instead of the one note, 64 steps of 4-note chords
    // 128 ms apart across the keyboard, each held two steps (contracts C2/C8;
    // with KONTRA_LIVE=1 and a cold page cache, late pages show as missing).
    let chords = std::env::var_os("KONTRA_CHORDS").is_some();
    if chords {
        let keys = |step: usize| (0..4).map(move |i| 36 + ((step * 7 + i * 5) % 60) as u8);
        let step_frames = 12 * BLOCK;
        for step in 0..64usize {
            let at = step * step_frames;
            for key in keys(step) {
                events.push((
                    at,
                    Event::NoteOn {
                        note: host_note(key, 0),
                        velocity,
                        tune: 0.0,
                    },
                ));
                events.push((at + 2 * step_frames, Event::NoteOff(host_pattern(key, 0))));
            }
        }
    }
    // KONTRA_TAIL: seconds rendered after the last key-up (2); the voices left
    // at the end are printed with their state.
    let tail = env("KONTRA_TAIL").unwrap_or(req.tail_s);
    events.sort_by_key(|e| e.0);
    let last = if chords {
        64 * 12 * BLOCK + 2 * 12 * BLOCK
    } else {
        events.iter().map(|e| e.0).max().unwrap_or(0)
    };
    let blocks = (last + (tail * rate as f32) as usize)
        .div_ceil(BLOCK)
        .max(1);

    let mut audio: Vec<[f32; 2]> = Vec::with_capacity(blocks * BLOCK);
    let mut block = vec![[0.0f32; 2]; BLOCK];
    // Time inside `Engine::process` alone (not disk waits): the render's CPU.
    let mut busy = std::time::Duration::ZERO;
    let mut next = 0usize;
    let live = req.live || std::env::var_os("KONTRA_LIVE").is_some();
    for n in 0..blocks {
        // The KONTRA_CONTROL entries scheduled for this block.
        for &(_, slot, id, value) in timed_controls.iter().filter(|c| c.0 == n) {
            engine.set_control(slot, id, value);
        }
        let now = engine.now().0;
        let from = n * BLOCK;
        let mut input: Vec<Timed<Event>> = Vec::new();
        while let Some(&(at, body)) = events.get(next) {
            if at >= from + BLOCK {
                break;
            }
            if at >= from {
                input.push(Timed {
                    at: SampleTime(now + (at - from) as u64),
                    body,
                });
            }
            next += 1;
        }
        // A bounce waits for the disk; KONTRA_LIVE=1 plays in real time
        // without waiting, as a host would, so late pages show as missing frames.
        let started = std::time::Instant::now();
        if !live {
            stream.settle(std::time::Duration::from_secs(5));
        }
        let processing = std::time::Instant::now();
        engine.process(&mut block, &input);
        busy += processing.elapsed();
        install_requests(&mut engine, &plan, rate, diag);
        if n == 0 || n == 3 * rate as usize / BLOCK {
            let during: Vec<_> = engine
                .action_calls()
                .into_iter()
                .filter_map(|(a, c)| {
                    let d = c - before.get(&a).copied().unwrap_or(0);
                    (d > 0).then_some((a, d))
                })
                .collect();
            say!(diag, "  calls by block {n}: {during:?}");
        }
        if n == 1 || n == rate as usize / BLOCK + 1 || n == 2 * rate as usize / BLOCK + 20 {
            say!(
                diag,
                "  block {n}: {} voices, {} playing, controllers {:?}",
                engine.active_voices(),
                engine.playing_voices(),
                watched
                    .iter()
                    .map(|&cc| (cc, engine.controller(cc)))
                    .collect::<Vec<_>>()
            );
            for (zone, at, gain) in engine.voices() {
                say!(diag, "    zone {zone} at {at:.0} gain {gain:?}");
            }
            let (filtered, cutoffs) = engine.voice_filters();
            if filtered > 0 {
                say!(diag, "    voice filter cutoffs {cutoffs:?}");
            }
        }
        if live {
            let period =
                std::time::Duration::from_secs_f64(f64::from(BLOCK as u32) / f64::from(rate));
            std::thread::sleep(period.saturating_sub(started.elapsed()));
        }
        audio.extend_from_slice(&block);
    }
    for v in engine.inspect_voices() {
        say!(diag, "  left: {v:?}");
    }
    let peak = audio.iter().flatten().fold(0f32, |p, s| p.max(s.abs()));
    // The RMS in the summary is over everything the WAV holds.
    let rms = (audio.iter().flatten().map(|x| x * x).sum::<f32>()
        / (2 * audio.len()).max(1) as f32)
        .sqrt();
    let wav = write_wav(&req.out, &audio, rate)?;
    let seconds = audio.len() as f64 / f64::from(rate);
    let realtime_factor = busy.as_secs_f64() / seconds.max(1e-9);
    let sha256 = sha256_hex(&wav);
    say!(
        diag,
        "peak {peak:.3}, {seconds:.3} s, rms {rms:.4}, sha256 {sha256}"
    );
    say!(diag, "stream: {:?}", stream.stats());
    say!(diag, "all actions: {:?}", engine.unhandled_actions());
    say!(diag, "last message: {}", engine.last_message());
    say!(diag, "calls: {:?}", engine.action_calls());
    say!(diag, "script faults: {:?}", engine.script_faults());
    say!(
        diag,
        "script arena spills: {}, memory after render: {}",
        kontra_lua::alloc::spills(),
        memory()
    );
    say!(diag, "wrote {}", req.out.display());
    Ok(Summary {
        format: Summary::FORMAT,
        instrument: req.instrument.display().to_string(),
        wav: req.out.display().to_string(),
        name,
        rate,
        channels: 2,
        frames: audio.len() as u64,
        duration_s: round(seconds),
        peak: round(f64::from(peak)),
        peak_dbfs: round(dbfs(f64::from(peak))),
        rms: round(f64::from(rms)),
        rms_dbfs: round(dbfs(f64::from(rms))),
        sha256,
        note: effective.map(|n| NoteSummary {
            key: n.key,
            velocity: n.velocity,
            hold_s: n.hold_s,
        }),
        midi: req.midi.as_ref().map(|p| p.display().to_string()),
        notes: events
            .iter()
            .filter(|e| matches!(e.1, Event::NoteOn { .. }))
            .count() as u64,
        zones: plan.zones.len() as u64,
        load_s: round(load_s),
        cpu_ms: round(busy.as_secs_f64() * 1e3),
        realtime_factor: round(realtime_factor),
    })
}

/// A host note on `channel`, as the tools address them (no note id, CLAP note end).
fn host_note(key: u8, channel: u8) -> HostNote {
    HostNote {
        port: 0,
        channel,
        key,
        id: -1,
        clap: true,
    }
}

/// The pattern that ends [`host_note`]'s note.
fn host_pattern(key: u8, channel: i32) -> HostPattern {
    HostPattern {
        port: 0,
        channel,
        key: i32::from(key),
        id: -1,
        clap: true,
    }
}

/// Rounds to six decimals, so a summary line is stable across runs.
fn round(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// A linear amplitude as dBFS, floored at −200 dB so silence is a number.
fn dbfs(v: f64) -> f64 {
    if v <= 0.0 {
        return -200.0;
    }
    20.0 * v.log10()
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// One played event, in frames from the start of the render.
type FrameEvent = (usize, Event);

/// The single note's on and off, in frames.
///
/// `velocity` and `hold` are the `KONTRA_VELOCITY` and `KONTRA_HOLD` overrides
/// (velocity 1..=127, hold in seconds); `None` keeps what the front end asked
/// for. They exist so a sweep can move velocity or length without rebuilding a
/// command line, and a render that names neither is exactly the front end's
/// note.
/// A velocity override inside the MIDI range, or `None`. 0 is a note-off and
/// 128 does not exist, so neither is a velocity a caller can have meant.
fn checked_velocity(v: Option<f32>) -> Option<f32> {
    v.filter(|x| (1.0..=127.0).contains(x))
}

/// A hold override that is a positive, finite number of seconds, or `None`.
/// Anything else would schedule the NoteOff at frame 0 or wrap the index.
fn checked_hold(v: Option<f32>) -> Option<f32> {
    v.filter(|x| x.is_finite() && *x > 0.0)
}

/// The note that will actually be played: the request with any override applied.
/// The events and [`Summary::note`] both come from this, so a run cannot describe
/// an input it did not use.
fn effective_note(note: Note, velocity: Option<f32>, hold: Option<f32>) -> Note {
    Note {
        key: note.key,
        velocity: velocity.map_or(note.velocity, |v| v as u8),
        hold_s: hold.unwrap_or(note.hold_s),
    }
}

/// Says an override was ignored, and what it should have been. A silently
/// dropped override is how a sweep renders the wrong note for an hour.
///
/// It goes through the render's own diagnostics writer, like every other line
/// the render says: `--diag` must file the warning with the run, and `--quiet`
/// must not print it — otherwise the one line that says the run measured a
/// different note than the one asked for is exactly the line missing from the
/// file, or the line that leaks past the silence.
fn invalid(diag: &mut dyn Write, name: &str, want: &str) -> Result<(), Box<dyn std::error::Error>> {
    say!(
        diag,
        "{name}: ignoring an out-of-range value; it must be {want}"
    );
    Ok(())
}

fn note_events(note: Note, rate: f32, velocity: Option<f32>, hold: Option<f32>) -> [FrameEvent; 2] {
    let velocity = velocity.unwrap_or(f32::from(note.velocity));
    let hold = hold.unwrap_or(note.hold_s);
    [
        (
            0,
            Event::NoteOn {
                note: host_note(note.key, 0),
                velocity: f64::from(velocity) / 127.0,
                tune: 0.0,
            },
        ),
        (
            (hold * rate) as usize,
            Event::NoteOff(host_pattern(note.key, 0)),
        ),
    ]
}

/// Reads a standard MIDI file as frame-stamped engine events.
///
/// Channel voice messages only, as the oracle's own writer emits: note on/off,
/// controllers and pitch bend. Tempo is honoured through the file's `FF 51`
/// events (500 000 µs per quarter when the file carries none, the SMF default).
/// Returns the events in time order and the file's own length in seconds.
///
/// # Errors
/// The file is not a standard MIDI file, its division is SMPTE, or a track is
/// truncated.
fn midi_events(
    bytes: &[u8],
    rate: u32,
) -> Result<(Vec<FrameEvent>, f64), Box<dyn std::error::Error>> {
    if bytes.get(..4) != Some(b"MThd".as_slice()) {
        return Err("midi: no MThd header".into());
    }
    let header = be_u32(bytes, 4).ok_or("midi: truncated header length")? as usize;
    let division = be_u16(bytes, 12).ok_or("midi: truncated header")?;
    if division == 0 || division & 0x8000 != 0 {
        return Err(format!("midi: division {division:#06x} is not ticks per quarter").into());
    }
    let division = f64::from(division);
    let mut raw: Vec<(u64, u8, u8, u8)> = Vec::new();
    let mut tempos: Vec<(u64, f64)> = vec![(0, 500_000.0)];
    let mut at = 8usize.saturating_add(header);
    while let Some(chunk) = bytes.get(at..) {
        if chunk.get(..4) != Some(b"MTrk".as_slice()) {
            break;
        }
        let len = be_u32(chunk, 4).ok_or("midi: truncated track length")? as usize;
        let end = at.saturating_add(8).saturating_add(len).min(bytes.len());
        let body = bytes
            .get(at.saturating_add(8)..end)
            .ok_or("midi: truncated track")?;
        read_track(body, &mut raw, &mut tempos)?;
        at = end;
    }
    raw.sort_by_key(|e| e.0);
    // Ticks to quarter notes once, so the tempo map and the events share a unit.
    let mut tempos: Vec<(f64, f64)> = tempos
        .into_iter()
        .map(|(tick, us)| (tick as f64 / division, us))
        .collect();
    tempos.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut events = Vec::with_capacity(raw.len());
    let mut last_frame = 0usize;
    for (tick, status, d1, d2) in raw {
        let seconds = seconds_at(&tempos, tick as f64 / division);
        let frame = (seconds * f64::from(rate)).round().max(0.0) as usize;
        last_frame = last_frame.max(frame);
        let body = match status & 0xf0 {
            0x90 if d2 > 0 => Event::NoteOn {
                note: host_note(d1, status & 0x0f),
                velocity: f64::from(d2) / 127.0,
                tune: 0.0,
            },
            0x80 | 0x90 => Event::NoteOff(HostPattern {
                port: 0,
                channel: i32::from(status & 0x0f),
                key: i32::from(d1),
                id: -1,
                clap: true,
            }),
            status => Event::midi1(status, d1, d2),
        };
        events.push((frame, body));
    }
    Ok((events, last_frame as f64 / f64::from(rate)))
}

/// Walks one `MTrk` body, appending channel events and tempo changes.
fn read_track(
    body: &[u8],
    raw: &mut Vec<(u64, u8, u8, u8)>,
    tempos: &mut Vec<(u64, f64)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut at = 0usize;
    let mut tick = 0u64;
    let mut running = 0u8;
    while at < body.len() {
        let (delta, used) = vlq(body.get(at..).ok_or("midi: truncated delta")?)?;
        at += used;
        tick += u64::from(delta);
        let Some(&first) = body.get(at) else { break };
        let status = if first & 0x80 != 0 {
            at += 1;
            first
        } else {
            if running & 0x80 == 0 {
                return Err(format!("midi: running status with none set at byte {at}").into());
            }
            running
        };
        match status {
            0xff => {
                let kind = *body.get(at).ok_or("midi: truncated meta")?;
                at += 1;
                let (len, used) = vlq(body.get(at..).ok_or("midi: truncated meta length")?)?;
                at += used;
                let data = body
                    .get(at..at.saturating_add(len as usize))
                    .ok_or("midi: truncated meta body")?;
                if kind == 0x51
                    && let [a, b, c] = *data
                {
                    let us = f64::from((u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c));
                    tempos.push((tick, us));
                }
                at = at.saturating_add(len as usize);
            }
            0xf0 | 0xf7 => {
                let (len, used) = vlq(body.get(at..).ok_or("midi: truncated sysex")?)?;
                at += used + len as usize;
            }
            status => {
                running = status;
                let d1 = *body.get(at).ok_or("midi: truncated data")?;
                at += 1;
                let two = status & 0xf0 != 0xc0 && status & 0xf0 != 0xd0;
                let d2 = if two {
                    let b = *body.get(at).ok_or("midi: truncated data")?;
                    at += 1;
                    b
                } else {
                    0
                };
                raw.push((tick, status, d1, d2));
            }
        }
    }
    Ok(())
}

/// Variable-length quantity, as SMF writes delta times.
fn vlq(bytes: &[u8]) -> Result<(u32, usize), Box<dyn std::error::Error>> {
    let mut value = 0u32;
    for (i, &b) in bytes.iter().enumerate().take(4) {
        value = (value << 7) | u32::from(b & 0x7f);
        if b & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err("midi: bad variable-length quantity".into())
}

/// Seconds at quarter-note `beat`, following the tempo map (`beat`, µs per quarter).
fn seconds_at(tempos: &[(f64, f64)], beat: f64) -> f64 {
    let mut seconds = 0.0;
    let mut last_beat = 0.0;
    let mut us = 500_000.0;
    for &(change, tempo) in tempos {
        if change > beat {
            break;
        }
        seconds += (change - last_beat) * us / 1e6;
        last_beat = change;
        us = tempo;
    }
    seconds + (beat - last_beat) * us / 1e6
}

/// Big-endian `u16` at `at`, or `None` when the slice ends first.
fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let field: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_be_bytes(field))
}

/// Big-endian `u32` at `at`, or `None` when the slice ends first.
fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(field))
}

/// Resident and peak memory from `/proc/self/status`.
fn memory() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .filter(|l| l.starts_with("VmRSS") || l.starts_with("VmHWM"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Writes interleaved stereo `frames` as 32-bit float WAV; returns its bytes.
///
/// # Errors
/// The path cannot be written.
fn write_wav(path: &Path, frames: &[[f32; 2]], rate: u32) -> std::io::Result<Vec<u8>> {
    let data = (frames.len() * 8) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(3u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 8).to_le_bytes());
    b.extend(8u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for s in frames.iter().flatten() {
        b.extend(s.to_le_bytes());
    }
    std::fs::write(path, &b)?;
    Ok(b)
}

/// Script control `name` and `value` as `Engine::set_control` takes them:
/// a UVI script's by its name among its controls (the last of that name, a
/// widget before a macro), its value an `f64`; a KSP one's UI id is the first
/// one plus its variable's index (persist.rs), its value an integer.
fn control(plan: &Plan, scripts: &[String], name: &str, value: &str) -> Option<(u8, i32, i64)> {
    plan.scripts
        .iter()
        .zip(scripts)
        .enumerate()
        .find_map(|(n, (script, source))| {
            let slot = u8::try_from(n).ok()?;
            if let Some(foreign) = &script.foreign {
                let id = foreign.controls().iter().rposition(|c| c == name)?;
                let value = value.parse::<f64>().ok()?.to_bits() as i64;
                return Some((slot, i32::try_from(id).ok()?, value));
            }
            let compiled = kontra_ksp::compile(source, &[]).ok()?;
            let bare = |v: &str| v.trim_start_matches(['$', '%', '~', '?', '@', '!']) == name;
            let at = compiled.variables.iter().position(|v| bare(&v.name))?;
            let id = kontra_ksp::FIRST_UI_ID + i32::try_from(at).ok()?;
            Some((slot, id, value.parse().ok()?))
        })
}

/// Builds and installs the effects and responses scripts asked for, between
/// blocks, as the host's rack does off the audio thread.
fn install_requests(engine: &mut Engine, plan: &Plan, rate: u32, diag: &mut dyn Write) {
    let mut asked = Vec::new();
    engine.drain_requests(|r| asked.push(r));
    for r in asked {
        if std::env::var_os("KONTRA_FX").is_some() {
            let kind = r.effect.as_ref().map(|e| &e.kind);
            let _ = writeln!(
                diag,
                "installs {:?} slot {} type {:?}: {kind:?}",
                r.at, r.slot, r.ty
            );
        }
        engine.install(Prepared::new(r, Some(plan), rate));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mistyped override is ignored, not clamped: rendering 127 for a requested
    /// 200 would be a measurement of a note nobody asked for.
    #[test]
    fn an_out_of_range_override_is_rejected_rather_than_clamped() {
        assert_eq!(checked_velocity(Some(200.0)), None, "200 is not a velocity");
        assert_eq!(checked_velocity(Some(0.0)), None, "0 is a note-off");
        assert_eq!(checked_velocity(Some(-5.0)), None);
        assert_eq!(checked_velocity(Some(40.0)), Some(40.0));
        assert_eq!(checked_hold(Some(0.0)), None, "a zero hold is a click");
        assert_eq!(checked_hold(Some(-1.0)), None);
        assert_eq!(checked_hold(Some(f32::NAN)), None);
        assert_eq!(checked_hold(Some(0.5)), Some(0.5));
    }

    /// A refused override is a diagnostic like every other line the render says,
    /// so it goes to the writer the run was handed: `--diag` files it with the
    /// run, `--quiet` (a sink) drops it, and stderr is only the default. It used
    /// to be an `eprintln!`, which is why a `--diag` log could be missing the one
    /// line that said the run measured a different note than the one asked for.
    #[test]
    fn a_refused_override_goes_to_the_diagnostics_writer() {
        let mut out = Vec::new();
        invalid(&mut out, "KONTRA_VELOCITY", "1..=127").expect("the writer takes it");
        assert_eq!(
            String::from_utf8(out).expect("utf-8"),
            "KONTRA_VELOCITY: ignoring an out-of-range value; it must be 1..=127\n"
        );
        // `--quiet` hands the run a sink: the line is dropped like every other.
        let mut quiet: Box<dyn Write> = Box::new(std::io::sink());
        invalid(&mut *quiet, "KONTRA_HOLD", "a positive number of seconds")
            .expect("the sink takes it");
    }

    /// The summary describes the note that was rendered, not the one requested.
    #[test]
    fn the_summary_reports_the_effective_note() {
        let asked = Note {
            key: 60,
            velocity: 100,
            hold_s: 1.5,
        };
        let played = effective_note(asked, Some(40.0), Some(0.5));
        assert_eq!(played.velocity, 40, "the override reached the events");
        assert_eq!(played.hold_s, 0.5);
        assert_eq!(played.key, 60, "the key is not overridable");
        // and with no overrides the request stands
        assert_eq!(effective_note(asked, None, None).velocity, 100);
        assert_eq!(effective_note(asked, None, None).hold_s, 1.5);
    }

    /// The oracle's own MIDI writer shape: one tempo event, note on at 0 and off at 720 ticks.
    fn one_note_midi() -> Vec<u8> {
        let body: Vec<u8> = [
            vec![0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20], // 500 000 µs per quarter
            vec![0x00, 0x90, 60, 100],
            vec![0x85, 0x50, 0x80, 60, 0], // delta 720
            vec![0x00, 0xff, 0x2f, 0x00],
        ]
        .concat();
        let mut out = b"MThd".to_vec();
        out.extend(6u32.to_be_bytes());
        out.extend([0, 0, 0, 1, 1, 0xe0]); // one track, 480 ticks per quarter
        out.extend(b"MTrk");
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn a_midi_file_lands_its_note_on_and_off_on_the_seconds_it_names() {
        let (events, length) = midi_events(&one_note_midi(), 48_000).expect("parses");
        assert_eq!(events.len(), 2, "one on, one off: {events:?}");
        assert_eq!(events.first().map(|e| e.0), Some(0));
        // 720 ticks at 480/quarter and 500 000 µs/quarter = 0.75 s.
        assert_eq!(events.get(1).map(|e| e.0), Some(36_000));
        assert!((length - 0.75).abs() < 1e-6, "length {length}");
        assert!(matches!(
            events.first().map(|e| e.1),
            Some(Event::NoteOn { velocity, .. }) if (velocity - 100.0 / 127.0).abs() < 1e-9
        ));
    }

    #[test]
    fn a_smpte_division_is_refused_rather_than_guessed() {
        let mut bytes = one_note_midi();
        bytes[12] = 0xe8; // SMPTE, 24 frames/s
        assert!(midi_events(&bytes, 48_000).is_err());
    }

    #[test]
    fn tempo_changes_are_followed() {
        // 480 ticks at 120 bpm, then 480 more at 60 bpm: 0.5 s + 1.0 s.
        let tempos = [(0.0, 500_000.0), (1.0, 1_000_000.0)];
        assert!((seconds_at(&tempos, 1.0) - 0.5).abs() < 1e-9);
        assert!((seconds_at(&tempos, 2.0) - 1.5).abs() < 1e-9);
    }

    /// `KONTRA_VELOCITY` and `KONTRA_HOLD` reach the single note, not only the
    /// repeat and chord sweeps: a velocity the render was asked for must be the
    /// velocity the note is played at, or a sweep measures the wrong note.
    #[test]
    fn the_velocity_and_hold_overrides_reach_the_single_note() {
        let note = Note {
            key: 60,
            velocity: 100,
            hold_s: 1.5,
        };
        // Nothing overridden: the front end's own note, unchanged.
        let [on, off] = note_events(note, 48_000.0, None, None);
        assert_eq!(on.0, 0);
        assert!(
            matches!(on.1, Event::NoteOn { velocity, .. } if (velocity - 100.0 / 127.0).abs() < 1e-9)
        );
        assert_eq!(off.0, 72_000, "1.5 s at 48 kHz");

        // Both overridden: 40/127 and half a second.
        let [on, off] = note_events(note, 48_000.0, Some(40.0), Some(0.5));
        assert!(
            matches!(on.1, Event::NoteOn { velocity, .. } if (velocity - 40.0 / 127.0).abs() < 1e-9),
            "the override did not reach the note: {:?}",
            on.1
        );
        assert_eq!(off.0, 24_000, "0.5 s at 48 kHz");
    }

    #[test]
    fn the_summary_rounds_to_six_decimals() {
        assert_eq!(round(1.234_567_89), 1.234_568);
        assert_eq!(dbfs(0.0), -200.0);
        assert!(dbfs(1.0).abs() < 1e-12);
        assert!((dbfs(0.5) + 6.020_6).abs() < 1e-3);
    }
}
