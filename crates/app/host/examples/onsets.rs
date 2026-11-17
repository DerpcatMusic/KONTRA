//! Renders a note pattern twice, in fixed 512-frame blocks and in random
//! 64..=1024-frame blocks (a host such as Bitwig), events on their exact
//! frames, and reports whether the two renders differ on either channel and how many onsets
//! each note has within 200 ms (two is a double attack).
//! `cargo run --release -p kontra-host --example onsets -- <file> <key> [keyswitch]`
//! KONTRA_PATTERN=solo|repeats: six notes 600 ms apart (default), or 16 notes
//! every 70 ms, 45 ms long.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, Prepared, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;
const MS: usize = RATE as usize / 1000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path: PathBuf = args.next().ok_or("usage: onsets <file> <key> [keyswitch]")?.into();
    let key: u8 = args.next().and_then(|k| k.to_str()?.parse().ok()).unwrap_or(60);
    let switch: Option<u8> = args.next().and_then(|k| k.to_str()?.parse().ok());
    let repeats = std::env::var("KONTRA_PATTERN").is_ok_and(|p| p == "repeats");
    // (frame, key, on): the keyswitch at 0, notes from 0.3 s.
    let mut notes: Vec<(usize, u8, bool)> = Vec::new();
    if let Some(k) = switch {
        notes.extend([(0, k, true), (50 * MS, k, false)]);
    }
    let (count, period, length) = if repeats { (16, 70, 45) } else { (6, 600, 400) };
    for i in 0..count {
        let at = (300 + i * period) * MS;
        notes.extend([(at, key, true), (at + length * MS, key, false)]);
    }
    notes.sort_by_key(|n| n.0);
    let total = (300 + count * period + 1000) * MS;
    let fixed = render(&path, &notes, total, |_| 512)?;
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let varied = render(&path, &notes, total, move |_| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        64 + (seed % 961) as usize
    })?;
    // Per channel: a pan or stereo-width difference must not cancel in l+r.
    let diff = (fixed.iter().zip(&varied)).map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs())).enumerate();
    let (first, worst) = diff.fold((None, 0f32), |(first, worst), (n, d)| {
        (first.or((d > 1e-4).then_some(n)), worst.max(d))
    });
    println!("fixed vs varied blocks (L and R): max diff {worst:.6}, first differing frame {first:?}");
    let starts: Vec<usize> = (notes.iter()).filter(|n| n.2 && Some(n.1) != switch).map(|n| n.0).collect();
    for (name, audio) in [("fixed", &fixed), ("varied", &varied)] {
        let counts: Vec<usize> = (starts.iter().enumerate())
            .map(|(i, &s)| onsets(audio, s, starts.get(i + 1).copied()))
            .collect();
        println!("{name}: onsets per note {counts:?}");
    }
    Ok(())
}

/// Onsets in the 200 ms after `start`, or up to the next note's start when
/// that is sooner (so fast repeats each count their own): a 1 ms RMS rising
/// 12 dB or more over 5 ms, above −50 dB re the render's peak, at least
/// 20 ms apart. Level is channel energy (L² + R²), so out-of-phase or
/// hard-panned onsets still count.
fn onsets(audio: &[[f32; 2]], start: usize, next: Option<usize>) -> usize {
    let window = next.map_or(200, |n| (n.saturating_sub(start) / MS).min(200));
    let energy = |[l, r]: &[f32; 2]| l * l + r * r;
    let peak = audio.iter().map(energy).fold(0f32, f32::max).sqrt().max(1e-9);
    let db = |at: usize| {
        let s = audio.get(at..at + MS).unwrap_or_default();
        let rms = (s.iter().map(energy).sum::<f32>() / MS as f32).sqrt();
        20.0 * (rms / peak).max(1e-9).log10()
    };
    let (mut count, mut last) = (0, None::<usize>);
    for ms in 0..window {
        let at = start + ms * MS;
        let before = db(at.saturating_sub(5 * MS));
        let now = db(at);
        if now > -50.0 && now - before >= 12.0 && last.is_none_or(|l| ms - l >= 20) {
            count += 1;
            last = Some(ms);
        }
    }
    count
}

/// Stereo render of `notes` in blocks of `size(n)` frames.
fn render(
    path: &Path,
    notes: &[(usize, u8, bool)],
    total: usize,
    mut size: impl FnMut(usize) -> usize,
) -> Result<Vec<[f32; 2]>, Box<dyn std::error::Error>> {
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let ir = kontra_host::instrument::read(path)?;
    let loaded = kontra_host::instrument::build(ir, RATE, &mut store);
    let stream = store.client();
    let plan = Arc::new(loaded.plan);
    let mut engine = Engine::new(Arc::clone(&plan), store.client(), RATE);
    if let Some(p) = kontra_prepare::tables(engine.plan(), &stream) {
        engine.swap_plan(&mut Arc::new(p), &[]);
    }
    // Between blocks, as a host does: effect loads, then zone edits
    // (`set_zone_par` and the like) applied to a new plan and swapped in.
    let install = |engine: &mut Engine| {
        let mut asked = Vec::new();
        engine.drain_requests(|r| asked.push(r));
        for r in asked {
            engine.install(Prepared::new(r, Some(&plan), RATE));
        }
        let mut edits = Vec::new();
        engine.drain_zone_edits(|e| {
            edits.push(e);
            true
        });
        if !edits.is_empty() {
            let mut next = Plan::clone(engine.plan());
            let done: Vec<_> = (edits.iter()).map(|e| (*e, e.apply(&mut next))).collect();
            let next = kontra_prepare::tables(&next, &stream).unwrap_or(next);
            engine.swap_plan(&mut Arc::new(next), &done);
        }
    };
    let mut block = vec![[0.0f32; 2]; 1024];
    let mut silent = vec![[0.0f32; 2]; 512];
    for _ in 0..RATE as usize / 2 / 512 {
        engine.process(&mut silent, &[]);
        install(&mut engine);
    }
    // A timed-out settle renders silence for unstreamed samples: fail
    // rather than report onsets from an incomplete render.
    let settle = |secs| match stream.settle(std::time::Duration::from_secs(secs)) {
        true => Ok(()),
        false => Err(format!("stream did not settle within {secs} s; render incomplete")),
    };
    settle(10)?;
    let mut out = Vec::with_capacity(total);
    let mut n = 0;
    while out.len() < total {
        let len = size(n).clamp(1, 1024).min(total - out.len());
        let (from, now) = (out.len(), engine.now().0);
        let events: Vec<Timed<Event>> = (notes.iter())
            .filter(|e| e.0 >= from && e.0 < from + len)
            .map(|&(at, key, on)| Timed {
                at: SampleTime(now + (at - from) as u64),
                body: if on {
                    Event::NoteOn {
                        note: HostNote { port: 0, channel: 0, key, id: -1, clap: true },
                        velocity: (100.0f32 / 127.0).into(),
                        tune: 0.0,
                    }
                } else {
                    Event::NoteOff(HostPattern { port: 0, channel: 0, key: i32::from(key), id: -1, clap: true })
                },
            })
            .collect();
        settle(5)?;
        let part = block.get_mut(..len).unwrap_or_default();
        engine.process(part, &events);
        install(&mut engine);
        out.extend_from_slice(part);
        n += 1;
    }
    Ok(out)
}
