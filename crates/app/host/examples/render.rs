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

    let t = std::time::Instant::now();
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let loaded = kontra_host::instrument::load(&path, RATE, &mut store)?;
    println!(
        "{}: {} zones, loaded in {:?}",
        loaded.name,
        loaded.plan.zones.len(),
        t.elapsed()
    );
    for d in &loaded.diagnostics {
        println!("  {:?} {}: {}", d.impact, d.feature, d.detail);
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
    let mut audio = Vec::new();
    let mut block = vec![[0.0f32; 2]; 512];
    for n in 0..(4 * RATE as usize / 512) {
        let input: &[Timed<Event>] = match n {
            0 => std::slice::from_ref(&on),
            n if n == 2 * RATE as usize / 512 => std::slice::from_ref(&off),
            _ => &[],
        };
        // A bounce waits for the disk; a live host never would.
        stream.settle(std::time::Duration::from_secs(5));
        engine.process(&mut block, input);
        audio.extend_from_slice(&block);
    }
    let peak = audio.iter().flatten().fold(0f32, |p, s| p.max(s.abs()));
    println!("peak {peak:.3}, {:?}", engine.stats());
    println!("all actions: {:?}", engine.unhandled_actions());
    write_wav(&out, &audio)?;
    println!("wrote {}", out.display());
    Ok(())
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
