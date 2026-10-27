//! Loads an instrument, plays one note and writes what the engine renders to
//! a WAV file, for listening and A/B against other samplers. Prints load time,
//! gaps and the script actions the engine does not answer yet.
//! `cargo run --release -p kontra-host --example render -- <file> <key> <out.wav>`

use std::path::PathBuf;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path: PathBuf = args
        .next()
        .ok_or("usage: render <file> <key> <out.wav>")?
        .into();
    let key: u8 = args
        .next()
        .and_then(|k| k.to_str()?.parse().ok())
        .unwrap_or(60);
    let out: PathBuf = args.next().unwrap_or_else(|| "render.wav".into()).into();
    // Optional: the mod wheel moves to this value one second in.
    let wheel: Option<u8> = args.next().and_then(|a| a.to_str()?.parse().ok());

    // KONTRA_SCRIPT_DUMP=<dir> writes each script's source there.
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_DUMP") {
        for (n, s) in kontra_format_kontakt::load(&path)?
            .scripts
            .iter()
            .enumerate()
        {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{n}.ksp")),
                &s.source,
            )?;
            let data: Vec<_> = s.data.iter().map(|(name, b)| (name, b.len())).collect();
            println!("  script {n} data files: {data:?}");
        }
    }
    let t = std::time::Instant::now();
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let loaded = kontra_host::instrument::load(&path, RATE, &mut store)?;
    println!(
        "{}: {} zones, loaded in {:?}",
        loaded.name,
        loaded.plan.zones.len(),
        t.elapsed()
    );
    println!("  memory: {}", memory());
    if !loaded.plan.switches.is_empty() {
        println!(
            "  switches {:?}, default {:?}",
            loaded.plan.switches, loaded.plan.default_keyswitch
        );
    }
    for d in &loaded.diagnostics {
        println!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    // The zones that could answer this key at the played velocity (101), and what they wait on.
    let candidates: Vec<_> = loaded
        .plan
        .zones
        .iter()
        .filter(|z| z.keys.contains(&key) && z.velocities.contains(&101))
        .collect();
    println!("  key {key}: {} zones", candidates.len());
    let mut kinds = std::collections::BTreeMap::new();
    for z in &candidates {
        let mods: Vec<_> = loaded
            .plan
            .groups
            .get(z.group as usize)
            .map(|g| g.mods.iter().map(|m| format!("{:?}", m.kind)).collect())
            .unwrap_or_default();
        let what = format!(
            "{:?} {:?} gain {:.2} mods {mods:?}",
            z.trigger, z.start, z.gain
        );
        *kinds.entry(what).or_insert(0) += 1;
    }
    for (what, n) in kinds.iter().take(8) {
        println!("    {n}× {what}");
    }
    let stream = store.client();
    let mut engine = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    println!("init: unanswered {:?}", engine.unhandled_actions());

    let host = HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    };
    let on = Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: host,
            velocity: 0.8,
            tune: 0.0,
        },
    };
    let pattern = HostPattern {
        port: 0,
        channel: 0,
        key: i32::from(key),
        id: -1,
        clap: true,
    };
    let off = Timed {
        at: SampleTime(0),
        body: Event::NoteOff(pattern),
    };
    let live = std::env::var_os("KONTRA_LIVE").is_some();
    let mut audio = Vec::new();
    let mut block = vec![[0.0f32; 2]; 512];
    let cc = wheel.map(|v| Timed {
        at: SampleTime(0),
        body: Event::Ump([0x20B0_0100 | u32::from(v), 0]),
    });
    for n in 0..(4 * RATE as usize / 512) {
        let input: &[Timed<Event>] = match n {
            0 => std::slice::from_ref(&on),
            n if n == RATE as usize / 512 => cc.as_slice(),
            n if n == 2 * RATE as usize / 512 => std::slice::from_ref(&off),
            _ => &[],
        };
        // A bounce waits for the disk; KONTRA_LIVE=1 plays in real time
        // without waiting, as a host would, so late pages show as missing frames.
        let started = std::time::Instant::now();
        if !live {
            stream.settle(std::time::Duration::from_secs(5));
        }
        engine.process(&mut block, input);
        if live {
            let period = std::time::Duration::from_secs_f64(512.0 / f64::from(RATE));
            std::thread::sleep(period.saturating_sub(started.elapsed()));
        }
        audio.extend_from_slice(&block);
    }
    let peak = audio.iter().flatten().fold(0f32, |p, s| p.max(s.abs()));
    let rms = |from: usize, to: usize| {
        let s = audio.get(from..to).unwrap_or_default();
        (s.iter().flatten().map(|x| x * x).sum::<f32>() / (2 * s.len().max(1)) as f32).sqrt()
    };
    let r = RATE as usize;
    println!(
        "rms 0.5-1 s {:.4}, 1.5-2 s {:.4}",
        rms(r / 2, r),
        rms(r * 3 / 2, 2 * r)
    );
    println!("peak {peak:.3}, {:?}", engine.stats());
    println!("all actions: {:?}", engine.unhandled_actions());
    println!("last message: {}", engine.last_message());
    println!("script faults: {:?}", engine.script_faults());
    write_wav(&out, &audio)?;
    println!("wrote {}", out.display());
    Ok(())
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

fn write_wav(path: &std::path::Path, frames: &[[f32; 2]]) -> std::io::Result<()> {
    let data = (frames.len() * 8) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(3u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(RATE.to_le_bytes());
    b.extend((RATE * 8).to_le_bytes());
    b.extend(8u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for s in frames.iter().flatten() {
        b.extend(s.to_le_bytes());
    }
    std::fs::write(path, b)
}
