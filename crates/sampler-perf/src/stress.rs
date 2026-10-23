//! `sampler-perf stress [--only INSTRUMENT,..] [--pattern NAME,..] [--block N] [--out FILE]`
//!
//! Deterministic streaming counters on real libraries: each instrument runs in its own
//! process so its peak RSS (VmHWM) is its own, and each pattern reports what the runtime
//! counted while it played. Latencies are in output frames of the render clock, not wall
//! time. Only notes that start after 2048 frames of silence are timed, so overlapping
//! notes do not hide each other's onset. Wall-clock block overruns are reported as well,
//! but they depend on host load: read them as context, not as a verdict.
//! No `perf`: every number comes from the runtime, /proc, or the streamer's own counter.

use super::*;

/// Instrument name, then the library-relative path (or program index).
const INSTRUMENTS: &[&str] = &["dolce-violins", "analog-strings", "augmented-orchestra"];

fn instrument(name: &str) -> Option<Source> {
    Some(match name {
        "dolce-violins" => Source::Kontakt(
            "Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 02 7 1st Violins - Sustained.nki",
        ),
        "analog-strings" => Source::Kontakt("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki"),
        "augmented-orchestra" => Source::Uvi(
            "UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs",
            "PAD Angela.uvip",
        ),
        _ => return None,
    })
}

struct Pattern {
    name: &'static str,
    seconds: f64,
    notes: fn() -> Vec<Message>,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        name: "chord10",
        seconds: 12.0,
        notes: chord10,
    },
    Pattern {
        name: "sixteenths",
        seconds: 8.0,
        notes: sixteenths,
    },
    Pattern {
        name: "velocity-sweep",
        seconds: 8.0,
        notes: velocity_sweep,
    },
    Pattern {
        name: "round-robin",
        seconds: 5.0,
        notes: round_robin,
    },
    // Load, play the first half, drop the runtime, load again, play the first half again.
    Pattern {
        name: "patch-switch",
        seconds: 8.0,
        notes: chord10,
    },
];

fn sorted(mut m: Vec<Message>) -> Vec<Message> {
    m.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then((a.1[0] & 0xf0 == 0x90).cmp(&(b.1[0] & 0xf0 == 0x90)))
    });
    m
}

fn expression_cc() -> Vec<Message> {
    vec![(0.0, [0xb0, 1, 110]), (0.0, [0xb0, 11, 127])]
}

/// Ten-note chord every second, held 0.9 s: each chord onset starts from silence.
fn chord10() -> Vec<Message> {
    let mut m = expression_cc();
    for beat in 0..12 {
        for key in [40, 44, 47, 52, 55, 59, 62, 64, 67, 72] {
            on_off(&mut m, key, 100, f64::from(beat), 0.9);
        }
    }
    m
}

/// Sixteenth notes at 120 BPM (0.125 s), 0.2 s long: continuous overlapping starts.
fn sixteenths() -> Vec<Message> {
    let mut m = expression_cc();
    for i in 0..64 {
        let key = 48 + [0, 3, 7, 12, 7, 3, 0, 5][i % 8];
        on_off(&mut m, key, 96, i as f64 * 0.125, 0.2);
    }
    m
}

/// One key stepped through velocity layers, 0.5 s apart.
fn velocity_sweep() -> Vec<Message> {
    let mut m = expression_cc();
    for (i, vel) in (8u8..=120).step_by(8).enumerate() {
        on_off(&mut m, 60, vel, i as f64 * 0.5, 0.5);
    }
    m
}

/// The same key twelve times, 0.4 s apart: round-robin sample alternatives.
fn round_robin() -> Vec<Message> {
    let mut m = expression_cc();
    for i in 0..12 {
        on_off(&mut m, 60, 100, i as f64 * 0.4, 0.3);
    }
    m
}

fn source_of(name: &str) -> Source {
    // Re-create the instrument per use: `Source` owns no heap data but is not Copy.
    instrument(name).expect("known instrument")
}

#[derive(Default)]
struct Totals {
    underruns: u64,
    cold_starts: u64,
    refused_starts: u64,
    voice_pool_refusals: u64,
    stream_capacity_refusals: u64,
    bytes_read: u64,
    read_calls: u64,
    minor_faults: u64,
    major_faults: u64,
    load_ms: u128,
    overrun_blocks: u64,
    blocks: u64,
    max_block_us: u64,
    /// Render time spent inside `service_streaming` (midi instruments only).
    service_overrun_blocks: u64,
    service_max_us: u64,
    onsets: u64,
    isolated: Vec<usize>,
    silent_onsets: u64,
    /// Current RSS after each phase's runtime is dropped: growth here is retained memory.
    rss_after_drop_kb: Vec<u64>,
    /// Largest output sample over the pattern: near zero means the render was silent.
    peak: f32,
}

/// Minor and major faults of this process, from /proc/self/stat (fields 10 and 12).
fn faults() -> (u64, u64) {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return (0, 0);
    };
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map_or(Vec::new(), |(_, rest)| rest.split_whitespace().collect());
    let at = |n: usize| fields.get(n - 3).and_then(|v| v.parse().ok()).unwrap_or(0);
    (at(10), at(12))
}

/// Read syscalls and bytes requested by this process (/proc/self/io): (syscr, rchar).
fn io() -> (u64, u64) {
    (
        proc_value("/proc/self/io", "syscr:"),
        proc_value("/proc/self/io", "rchar:"),
    )
}

/// Isolated note-ons as frames from their onset to first sound, and the count of
/// isolated note-ons that never sounded within the render.
fn onsets(messages: &[Message], loud: &[bool]) -> (Vec<usize>, u64) {
    let (mut isolated, mut silent) = (Vec::new(), 0);
    for &(at, [status, _, vel]) in messages {
        if status & 0xf0 != 0x90 || vel == 0 {
            continue;
        }
        let o = (at * f64::from(RATE)).round() as usize;
        if o >= loud.len() || loud[o.saturating_sub(2048)..o].iter().any(|&l| l) {
            continue;
        }
        match loud[o..].iter().position(|&l| l) {
            Some(d) => isolated.push(d),
            None => silent += 1,
        }
    }
    (isolated, silent)
}

fn percentile_usize(v: &mut [usize], p: f64) -> Option<usize> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[((v.len() - 1) as f64 * p).round() as usize])
}

fn block_stats(cell: &Cell_, block: usize, t: &mut Totals) {
    let deadline = block as u64 * 1_000_000_000 / u64::from(RATE);
    t.blocks += cell.times.len() as u64;
    t.overrun_blocks += cell.times.iter().filter(|&&ns| ns > deadline).count() as u64;
    t.max_block_us = t
        .max_block_us
        .max(cell.times.iter().copied().max().unwrap_or(0) / 1000);
    t.service_overrun_blocks += cell.service.iter().filter(|&&ns| ns > deadline).count() as u64;
    t.service_max_us = t
        .service_max_us
        .max(cell.service.iter().copied().max().unwrap_or(0) / 1000);
}

/// Plays one pattern on one instrument and adds everything it counted to `t`.
fn play_pattern(inst: &str, pattern: &Pattern, block: usize, t: &mut Totals) -> Result<(), String> {
    let phases = if pattern.name == "patch-switch" { 2 } else { 1 };
    let seconds = if phases == 2 {
        pattern.seconds / 2.0
    } else {
        pattern.seconds
    };
    let frames = (seconds * f64::from(RATE)) as usize;
    for _ in 0..phases {
        let messages = sorted(
            (pattern.notes)()
                .into_iter()
                .filter(|m| m.0 < seconds)
                .collect(),
        );
        let scenario = Scenario {
            name: pattern.name,
            source: source_of(inst),
            seconds,
            notes: pattern.notes,
        };
        let started = Instant::now();
        let mut loaded = load(&scenario, &messages, block)?;
        t.load_ms += started.elapsed().as_millis();
        let (bytes0, syscr0, (min0, maj0)) = (
            sampler_kontakt::DISK_READ.load(std::sync::atomic::Ordering::Relaxed),
            io().0,
            faults(),
        );
        let cell = if matches!(loaded.player, Player::Uvi(_)) {
            play_uvi(&mut loaded.player, &messages, block, frames)
        } else {
            play(&mut loaded.player, &messages, block, frames)
        };
        let (min1, maj1) = faults();
        t.bytes_read +=
            sampler_kontakt::DISK_READ.load(std::sync::atomic::Ordering::Relaxed) - bytes0;
        t.read_calls += io().0 - syscr0;
        t.minor_faults += min1 - min0;
        t.major_faults += maj1 - maj0;
        block_stats(&cell, block, t);
        t.peak = t.peak.max(cell.peak);
        let st = stats(&loaded.player);
        t.underruns += st.stream_underruns;
        t.cold_starts += st.cold_starts;
        t.refused_starts += st.refused_starts;
        t.voice_pool_refusals += st.voice_pool_refusals;
        t.stream_capacity_refusals += st.stream_capacity_refusals;
        let (iso, silent) = onsets(&messages, &cell.loud);
        t.onsets += messages
            .iter()
            .filter(|m| m.1[0] & 0xf0 == 0x90 && m.1[2] > 0)
            .count() as u64;
        t.isolated.extend(iso);
        t.silent_onsets += silent;
        drop(loaded);
        t.rss_after_drop_kb
            .push(proc_value("/proc/self/status", "VmRSS:"));
    }
    Ok(())
}

fn report(inst: &str, pattern: &str, t: &mut Totals) -> Value {
    let mut sorted_onsets = t.isolated.clone();
    let p50 = percentile_usize(&mut sorted_onsets, 0.5);
    let max = t.isolated.iter().copied().max();
    json!({
        "instrument": inst, "pattern": pattern,
        "late_reads_counted": false,
        "underruns": t.underruns, "cold_starts": t.cold_starts, "refused_starts": t.refused_starts,
        "voice_pool_refusals": t.voice_pool_refusals, "stream_capacity_refusals": t.stream_capacity_refusals,
        "bytes_read": t.bytes_read, "read_calls": t.read_calls,
        "minor_faults": t.minor_faults, "major_faults": t.major_faults,
        "rss_peak_kb": proc_value("/proc/self/status", "VmHWM:"),
        "load_ms": t.load_ms, "rss_after_drop_kb": t.rss_after_drop_kb,
        "peak_dbfs": 20.0 * f64::from(t.peak).max(1e-9).log10(),
        "onsets": t.onsets, "isolated_onsets": t.isolated.len(), "silent_onsets": t.silent_onsets,
        "note_on_to_sound_frames_p50": p50, "note_on_to_sound_frames_max": max,
        "overrun_blocks": t.overrun_blocks, "blocks": t.blocks, "max_block_us": t.max_block_us,
        "service_overrun_blocks": t.service_overrun_blocks, "service_max_us": t.service_max_us,
    })
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    let (mut only, mut patterns, mut block, mut out) = (
        None::<Vec<String>>,
        None::<Vec<String>>,
        256usize,
        None::<PathBuf>,
    );
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().ok_or(format!("{a} needs a value"))?;
        match a.as_str() {
            "--only" => only = Some(v.split(',').map(String::from).collect()),
            "--pattern" => patterns = Some(v.split(',').map(String::from).collect()),
            "--block" => block = v.parse().map_err(fail)?,
            "--out" => out = Some(PathBuf::from(v)),
            _ => return Err(format!("unknown option {a}")),
        }
    }
    let mut rows = Vec::new();
    for inst in INSTRUMENTS
        .iter()
        .filter(|i| only.as_ref().is_none_or(|o| o.iter().any(|n| n == *i)))
    {
        for pattern in PATTERNS.iter().filter(|p| {
            patterns
                .as_ref()
                .is_none_or(|o| o.iter().any(|n| n == p.name))
        }) {
            let mut t = Totals::default();
            match play_pattern(inst, pattern, block, &mut t) {
                Ok(()) => {
                    let row = report(inst, pattern.name, &mut t);
                    eprintln!("{row}");
                    rows.push(row);
                }
                Err(e) => {
                    eprintln!("{inst}/{}: skipped: {e}", pattern.name);
                    rows.push(json!({ "instrument": inst, "pattern": pattern.name, "skipped": e }));
                }
            }
        }
    }
    let text = serde_json::to_string_pretty(
        &json!({ "format": "stress-1", "rate": RATE, "block": block, "rows": rows }),
    )
    .map_err(fail)?;
    match out {
        Some(path) => std::fs::write(path, text).map_err(fail)?,
        None => println!("{text}"),
    }
    Ok(())
}
