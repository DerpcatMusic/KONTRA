//! A live player on a library still loading, at a real host's pace: notes
//! while it loads, then single notes on keys whose samples past their
//! heads are still on disk, then dense chords and fast repeats at odd
//! frames. A note played before the instrument is in never sounds and
//! never sounds later; each note sounds as soon after its key as it does
//! in an offline render with every page read; nothing bursts; voices are
//! stolen only at the limit, and nothing is refused.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use moose::prelude::*;

use super::{
    BLOCK, In, KONTAKT, Played, RATE, UVI, activate, at, block_in, limited, off, on, state,
};
use crate::{Dsp, Kontra, KontraParams, Tick};

/// One Kontakt and one UVI program: big sample sets, streamed.
const PROGRAMS: [&str; 2] = [
    "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
    "UVI:VWinds - Clarinets/VWinds-AClarinet.ufs/Presets/Clarinet A.uvip",
];
/// The single notes' keys, each first played cold.
const PROBES: [u8; 6] = [41, 47, 53, 59, 66, 71];
/// Seconds between single notes, room for their tails.
const PROBE_GAP_S: f64 = 1.5;
/// How long after the load the first note waits, in silence.
const QUIET_S: f64 = 0.2;
/// The envelope's window, in frames.
const WINDOW: usize = 64;

/// Single notes at odd frames held 1.2 s, then ten-note chords every 80 ms held
/// 400 ms, then 30 repeats of one key 35 ms apart.
fn performance() -> Vec<(usize, In)> {
    let mut out = Vec::new();
    for (n, &key) in PROBES.iter().enumerate() {
        let t = at(QUIET_S + PROBE_GAP_S * n as f64) + n * 101 % 509;
        out.push((t, on(key)));
        // Held past its head, into pages read only now.
        out.push((t + at(1.2), off(key)));
    }
    let from = at(QUIET_S + PROBE_GAP_S * PROBES.len() as f64);
    let keys = [36, 40, 43, 48, 52, 55, 60, 64, 67, 72];
    for chord in 0..20u32 {
        let t = from + at(0.08 * f64::from(chord)) + chord as usize * 37 % 509;
        for (n, &key) in keys.iter().enumerate() {
            let key = key + (chord % 3) as u8;
            out.push((t + n * 3, on(key)));
            out.push((t + n * 3 + at(0.4), off(key)));
        }
    }
    let from = from + at(2.0);
    for n in 0..30u32 {
        let t = from + at(0.035 * f64::from(n)) + n as usize * 13 % 501;
        out.push((t, on(62)));
        out.push((t + at(0.03), off(62)));
    }
    out.sort_by_key(|e| e.0);
    out
}

/// Renders `events` block by block until `frames` and every voice has
/// ended, the control tick about 20 times a second; paced like a host,
/// pages read as they come, when `live`.
fn render(
    params: &KontraParams,
    dsp: &mut Dsp,
    events: &[(usize, In)],
    frames: usize,
    last: &mut Played,
    live: bool,
    peak_voices: &mut u32,
) -> Vec<f32> {
    let mode = if live {
        ProcessMode::Realtime
    } else {
        ProcessMode::Offline
    };
    let started = Instant::now();
    let mut audio = Vec::new();
    let mut frame = 0;
    while frame < frames || last.voices > 0 && frame < frames + at(20.0) {
        let inside: Vec<_> = (events.iter())
            .filter(|e| (frame..frame + BLOCK).contains(&e.0))
            .map(|&(f, e)| (f - frame, e))
            .collect();
        *last = block_in(params, dsp, &inside, last, mode);
        *peak_voices = (*peak_voices).max(last.voices);
        audio.extend_from_slice(&last.audio);
        frame += BLOCK;
        if (frame / BLOCK).is_multiple_of(10) {
            Tick.run(params);
        }
        if live {
            let due = Duration::from_secs_f64(frame as f64 / RATE);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }
    audio
}

/// Each `WINDOW` frames' peak.
fn envelope(audio: &[f32]) -> Vec<f32> {
    (audio.chunks(WINDOW))
        .map(|w| w.iter().fold(0.0f32, |p, x| p.max(x.abs())))
        .collect()
}

/// Frames where `audio` jumps 12 dB or more to within 20 dB of its
/// loudest with no key going down in the `hold` before (a late burst).
fn bursts(audio: &[f32], downs: &[usize], hold: usize) -> Vec<usize> {
    let peaks = envelope(audio);
    let loudest = peaks.iter().copied().fold(0.0f32, f32::max);
    (peaks.windows(5).enumerate())
        .filter(|(_, w)| {
            let before = w[..4].iter().copied().fold(0.0, f32::max);
            w[4] > 1e-3 && w[4] > 4.0 * before && w[4] > 0.1 * loudest
        })
        .map(|(b, _)| (b + 4) * WINDOW)
        .filter(|&t| !downs.iter().any(|&d| d <= t + WINDOW && t <= d + hold))
        .collect()
}

/// A host loading `state` on the tick's own thread while the audio thread
/// plays on, paced; with `notes`, a short note every 400 ms. The instance,
/// the load's seconds, its audio and the frames its keys went down.
fn load_live(state: &str, notes: bool) -> (KontraParams, Dsp, Played, f64, Vec<f32>, Vec<usize>) {
    let params = KontraParams::new();
    *params.saved.write().unwrap() = state.to_owned();
    params.restore();
    params.align.set_value(true);
    let mut dsp = Dsp::default();
    <Kontra as PluginLogic>::reset(&mut dsp, &params, &AudioConfig::new(RATE, BLOCK));
    let mut last = Played::default();
    let (mut audio, mut downs) = (Vec::new(), Vec::new());
    let started = Instant::now();
    std::thread::scope(|s| {
        let loader = s.spawn(|| Tick.run(&params));
        let (every, length) = (at(0.4), at(0.03));
        let mut frame = 0;
        while !loader.is_finished() {
            let mut events = Vec::new();
            for t in (frame..frame + BLOCK).filter(|_| notes) {
                if t % every == 17 {
                    events.push((t - frame, on(60)));
                    downs.push(t);
                } else if t % every == 17 + length {
                    events.push((t - frame, off(60)));
                }
            }
            last = block_in(&params, &mut dsp, &events, &last, ProcessMode::Realtime);
            audio.extend_from_slice(&last.audio);
            frame += BLOCK;
            let due = Duration::from_secs_f64(frame as f64 / RATE);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    });
    let load_s = started.elapsed().as_secs_f64();
    Tick.run(&params);
    (params, dsp, last, load_s, audio, downs)
}

/// Each single note's onset in `audio`, in ms after its key went down.
fn onsets(audio: &[f32], events: &[(usize, In)]) -> Vec<Option<f32>> {
    let span = at(PROBE_GAP_S) - at(0.05);
    (events.iter().filter(|e| e.1.starts()).take(PROBES.len()))
        .map(|&(t, _)| {
            let x = audio.get(t..(t + span).min(audio.len()))?;
            kontra_align::onset_ms(x, RATE, kontra_align::ONSET_DB)
        })
        .collect()
}

/// Plays the program at `path` live while it loads; its failures.
fn play_live(path: PathBuf) -> Vec<String> {
    let name = (path.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let state = state(path);
    let mut failed = Vec::new();
    let mut fail = |what: String| failed.push(format!("{name} (live): {what}"));

    // Keys while it loads: each sound starts within the hold after one
    // (a key before the engine is in never sounds; none waits for it).
    let (params, mut dsp, mut last, _, mut audio, loading) = load_live(&state, true);
    let hold = <Kontra as PluginLogic>::latency(&dsp) as usize + at(0.03);
    let mut voices = 0;
    audio.extend(render(
        &params,
        &mut dsp,
        &[],
        at(0.5),
        &mut last,
        true,
        &mut voices,
    ));
    let windows = envelope(&audio);
    let mut sounded = 0;
    for (w, pair) in windows.windows(5).enumerate() {
        let t = (w + 4) * WINDOW;
        if pair[4] > 1e-4 && pair[..4].iter().all(|p| *p < 1e-5) {
            sounded += 1;
            if !loading.iter().any(|&d| d <= t + WINDOW && t <= d + hold) {
                fail(format!(
                    "a sound at {:.3} s no key started: late",
                    t as f64 / RATE
                ));
            }
        }
    }
    drop((params, dsp));

    // Loaded with no keys: silence, then the performance, cold.
    let (params, mut dsp, mut last, load_s, _, _) = load_live(&state, false);
    let (limited, pool) = limited(&params);
    let hold_ms = <Kontra as PluginLogic>::latency(&dsp) as f32 * 1000.0 / RATE as f32;
    let events = performance();
    let end = events.last().map_or(0, |e| e.0) + 1;
    let before = (last.stolen, last.cut, last.refused, last.missing);
    let mut voices = 0;
    let live = render(
        &params,
        &mut dsp,
        &events,
        end,
        &mut last,
        true,
        &mut voices,
    );
    let (stolen, cut, refused, missing) = (
        last.stolen - before.0,
        last.cut - before.1,
        last.refused - before.2,
        last.missing - before.3,
    );
    if live
        .get(..at(QUIET_S))
        .is_some_and(|q| q.iter().any(|x| x.abs() > 1e-5))
    {
        fail("sounds before its first key".into());
    }
    drop((params, dsp));

    // The same performance on a loaded, settled instance, every page read.
    let (params, mut dsp) = activate(&state);
    params.align.set_value(true);
    Tick.run(&params);
    let mut warm = Played::default();
    let mut warm_voices = 0;
    let reference = render(
        &params,
        &mut dsp,
        &events,
        end,
        &mut warm,
        false,
        &mut warm_voices,
    );

    let (heard, settled) = (onsets(&live, &events), onsets(&reference, &events));
    for ((key, live_ms), settled_ms) in PROBES.iter().zip(&heard).zip(&settled) {
        match (live_ms, settled_ms) {
            (Some(l), Some(s)) if *l > s + 5.0 => fail(format!(
                "key {key} sounds {l:.1} ms after it went down, {s:.1} ms offline (hold {hold_ms:.1} ms)"
            )),
            (None, Some(_)) => fail(format!("key {key} never sounded")),
            _ => {}
        }
    }
    let downs: Vec<_> = (events.iter().filter(|e| e.1.starts()))
        .map(|e| e.0)
        .collect();
    let hold = at(f64::from(hold_ms) / 1000.0 + 0.05);
    if let Some(&t) = bursts(&live, &downs, hold).first() {
        fail(format!("a burst at {:.3} s", t as f64 / RATE));
    }
    if refused > 0 {
        fail(format!("{refused} notes refused or events dropped"));
    }
    if stolen > 0 && !limited && (voices as usize) < pool {
        fail(format!("{stolen} voices stolen at {voices} of {pool}"));
    }
    let ms = |o: &[Option<f32>]| {
        o.iter()
            .map(|x| x.map_or_else(|| "-".into(), |x| format!("{x:.1}")))
            .collect::<Vec<_>>()
            .join(" ")
    };
    println!(
        "{name} (live): loaded in {load_s:.1} s, {} keys while loading, {sounded} sounded; \
         {} notes, peak {voices} voices of {pool} (offline {warm_voices}), {stolen} stolen \
         (offline {}), {cut} cut, {refused} refused, {:.2} voice-s of data late; single-note \
         onsets live [{}] offline [{}] ms, hold {hold_ms:.1} ms",
        loading.len(),
        downs.len(),
        warm.stolen,
        missing as f64 / RATE,
        ms(&heard),
        ms(&settled),
    );
    failed
}

#[test]
#[ignore = "real libraries, live pace: run with --ignored (release)"]
fn a_loading_library_played_live_never_plays_late() {
    let mut failed = Vec::new();
    // KONTRA_LIVE=<path under the Kontakt libraries> plays just that program.
    let only = std::env::var("KONTRA_LIVE").ok();
    let programs = only.iter().map(String::as_str).chain(PROGRAMS);
    for program in programs.take(if only.is_some() { 1 } else { PROGRAMS.len() }) {
        let path = match program.strip_prefix("UVI:") {
            Some(p) => Path::new(UVI).join(p),
            None => Path::new(KONTAKT).join(program),
        };
        if !path.ancestors().any(Path::is_file) {
            eprintln!("skipped: no {}", path.display());
            continue;
        }
        failed.extend(play_live(path));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
