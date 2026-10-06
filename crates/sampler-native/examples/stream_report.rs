//! Streamed playback of a real Kontakt instrument: resident memory and render
//! time. `stream_report INPUT.nki [SECONDS] [VOICES]` loads it streamed, plays
//! overlapping notes across its range in 64-frame blocks (servicing the page
//! cache before each), then purges heads idle for ten seconds.
use sampler_core::{Limits, PAGE_FRAMES, Runtime};

use sampler_midi::{Ingress, Packets, TimedPacket, Version};
use std::{path::Path, time::Instant};

fn status(field: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(field))
                .map(|l| l[field.len()..].trim().to_string())
        })
        .unwrap_or_else(|| "n/a".into())
}

fn mb(bytes: impl Into<u64>) -> f64 {
    bytes.into() as f64 / 1e6
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = Path::new(
        args.first()
            .expect("usage: stream_report INPUT.nki [SECONDS] [VOICES]"),
    );
    let seconds: f64 = args.get(1).map_or(30.0, |s| s.parse().unwrap());
    let voices: usize = args.get(2).map_or(256, |s| s.parse().unwrap());
    let begin = Instant::now();
    let policy = sampler_kontakt::StreamPolicy {
        voices,
        ..Default::default()
    };
    let streamed = sampler_kontakt::load_streamed(path, &Default::default(), &policy, |_| {})
        .unwrap_or_else(|e| panic!("{e}"));
    let report = streamed.report;
    println!(
        "loaded in {:.1} s: {} assets; read latency p50 {:.2} ms p95 {:.2} ms p99 {:.2} ms -> heads of {} frames x zone step",
        begin.elapsed().as_secs_f64(),
        streamed.assets.len(),
        report.latency_p50.as_secs_f64() * 1e3,
        report.latency_p95.as_secs_f64() * 1e3,
        report.latency_p99.as_secs_f64() * 1e3,
        report.head_frames,
    );
    println!(
        "resident: heads {:.1} MB + page pool {:.1} MB ({} pages); full decode would be {:.1} MB; RSS {}",
        mb(report.head_bytes as u64),
        mb(report.pool_bytes as u64),
        report.pool_pages,
        mb(report.full_bytes),
        status("VmRSS:"),
    );
    let sampler_kontakt::Streamed {
        loaded,
        assets,
        cache,
        streamer,
        ..
    } = streamed;
    let plan = loaded.plan;
    let rate = plan.sample_rate();
    let x: usize = std::env::var("LIMITS_X").map_or(1, |v| v.parse().unwrap());
    let pick =
        |name: &str| std::env::var("LIMITS_ONLY").map_or(x, |o| if o == name { x } else { 1 });
    let limits = Limits {
        notes: 256 * pick("notes"),
        channels: 16,
        performances: 1,
        expressions: 256 * pick("expressions"),
        families: 256 * pick("families"),
        decisions: 1024 * pick("decisions"),
        voices,
        commands: 1024 * pick("commands"),
        behaviors: 64 * pick("behaviors"),
        behavior_fuel: (1 << 20) * pick("fuel"),
        behavior_cells: plan
            .behavior_local_count()
            .saturating_mul(64 * pick("behavior_cells")),
        note_cells: plan.note_cell_count().saturating_mul(256),
    };
    let mut rt = Runtime::new(plan, limits)
        .unwrap_or_else(|e| panic!("{e}"))
        .with_stream_cache(cache);
    rt.set_release_stealing(true);
    let (low, high) = loaded
        .instrument
        .zones
        .iter()
        .fold((127u8, 0u8), |(l, h), z| {
            (l.min(z.keys.low), h.max(z.keys.high))
        });
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let ingress = Ingress::new(0, groups);
    // A new note every 125 ms, held 2 s: about 16 notes sound at once.
    let span = u32::from(high.saturating_sub(low)) + 1;
    let frames = (seconds * f64::from(rate)) as usize;
    let step = rate as usize / 8;
    let mut events: Vec<(usize, u32)> = Vec::new();
    for (n, at) in (0..frames).step_by(step).enumerate() {
        let key = u32::from(low) + (n as u32 * 7) % span;
        let velocity = 40 + (n as u32 * 29) % 88;
        let word = 0x2000_0000 | key << 8 | velocity;
        events.push((at, word | 0x0090_0000));
        events.push((at + 2 * rate as usize, word | 0x0080_0000));
    }
    events.sort_by_key(|&(at, word)| (at, word & 0x0010_0000));
    let words: Vec<[u32; 1]> = events.iter().map(|&(_, w)| [w]).collect();
    let packets: Vec<TimedPacket> = events
        .iter()
        .zip(&words)
        .map(|(&(offset, _), word)| TimedPacket {
            offset,
            packet: Packets::new(word).next().unwrap().unwrap(),
        })
        .collect();
    let mut buffer = [[0.0f32; 2]; 64];
    // Heads bound only starts; running voices request a page ahead.
    let horizon = (report.head_frames.max(PAGE_FRAMES) + buffer.len()) as u32;
    let (mut next, mut times, mut refused, mut reloaded) = (0, Vec::new(), 0, 0);
    let (mut peak, mut most) = (0.0f32, 0);
    let (mut pending, mut service_errors) = (0, std::collections::BTreeMap::new());
    let play = Instant::now();
    for start in (0..frames + 3 * rate as usize).step_by(buffer.len()) {
        let mut batch = Vec::new();
        while next < packets.len() && packets[next].offset < start + buffer.len() {
            batch.push(TimedPacket {
                offset: packets[next].offset - start,
                ..packets[next]
            });
            next += 1;
        }
        // Pace blocks to real time, as an audio callback would be.
        let due = play + std::time::Duration::from_secs_f64(start as f64 / f64::from(rate));
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let t = Instant::now();
        match rt.service_streaming(horizon) {
            Ok(true) => {}
            Ok(false) => pending += 1,
            Err(e) => *service_errors.entry(format!("{e:?}")).or_insert(0) += 1,
        }
        ingress
            .render(&mut rt, &mut buffer, &batch, batch.len(), |_, result| {
                if let Err(e) = &result {
                    if refused < 5 {
                        eprintln!("refused: {e:?}");
                    }
                    refused += 1;
                }
            })
            .unwrap();
        times.push(t.elapsed().as_secs_f64() * 1e6);
        most = most.max(rt.stats().voices);
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| true);
        peak = buffer.iter().flatten().fold(peak, |p, x| p.max(x.abs()));
        if start % (PAGE_FRAMES * 4) == 0 {
            reloaded += streamer.reload(&assets).unwrap();
        }
    }
    times.sort_by(f64::total_cmp);
    let stats = rt.stats();
    let budget = 64e6 / f64::from(rate);
    println!(
        "played {seconds} s, {} notes, peak {peak:.3}: block service+render median {:.0} us, p99 {:.0} us, max {:.0} us (budget {budget:.0} us)",
        events.len() / 2,
        times[times.len() / 2],
        times[times.len() * 99 / 100],
        times.last().unwrap(),
    );
    println!(
        "voices peak {most}, stream underruns {}, voice drops {}, nonfinite {}, refused events {refused}, heads reloaded {reloaded}; RSS {} (peak {})",
        stats.stream_underruns,
        stats.voice_drops,
        stats.nonfinite_frames,
        status("VmRSS:"),
        status("VmHWM:"),
    );
    // Decode thread CPU time (utime + stime, clock ticks) against wall time.
    let decode_ticks: u64 = std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|t| {
            std::fs::read_to_string(t.path().join("comm"))
                .is_ok_and(|c| c.starts_with("sampler-stream"))
        })
        .filter_map(|t| std::fs::read_to_string(t.path().join("stat")).ok())
        .map(|stat| {
            let fields: Vec<&str> = stat
                .rsplit(')')
                .next()
                .unwrap()
                .split_whitespace()
                .collect();
            fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
        })
        .sum();
    println!("service: {pending} blocks with pages pending, errors {service_errors:?}");
    println!(
        "decode threads busy {:.1} s over {:.1} s of playback",
        decode_ticks as f64 / 100.0,
        play.elapsed().as_secs_f64()
    );
    let idle = rt.now().saturating_sub(10 * u64::from(rate));
    let freed = streamer.purge(&assets, idle);
    let heads: usize = assets.iter().map(sampler_core::Pcm::resident_bytes).sum();
    println!(
        "purged heads idle 10 s: freed {:.1} MB, heads now {:.1} MB",
        mb(freed as u64),
        mb(heads as u64)
    );
}
