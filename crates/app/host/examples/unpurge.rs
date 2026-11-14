//! A purged group loaded at runtime: loads an instrument as the plugin does
//! (heads deferred), lists its interface controls, sets those whose name
//! contains `KONTRA_WIDGET` to `KONTRA_VALUE` (default 1), and reports each
//! step's wall time, what the rack said, the groups still dormant and the
//! level of a held note (total and in the band around `KONTRA_HZ`, default
//! 8000) before and after.
//! `cargo run --release -p kontra-host --example unpurge -- <file> [key]`

use std::time::{Duration, Instant};

use kontra_engine::{Event, HostNote};
use kontra_host::rack::{Rack, RackAudio, Store, StoreConfig};

const BLOCK: usize = 512;
const RATE: f64 = 48_000.0;

/// Level (RMS dB) of `signal` and of its component at `hz` (Goertzel).
fn levels(signal: &[f32], hz: f64) -> (f64, f64) {
    let n = signal.len().max(1) as f64;
    let rms = (signal.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / n).sqrt();
    let k = 2.0 * (2.0 * std::f64::consts::PI * hz / RATE).cos();
    let (mut a, mut b) = (0.0f64, 0.0f64);
    for s in signal {
        let c = f64::from(*s) + k * a - b;
        b = a;
        a = c;
    }
    let band = (a * a + b * b - k * a * b).max(0.0).sqrt() * 2.0 / n;
    let db = |x: f64| 20.0 * x.max(1e-12).log10();
    (db(rms), db(band))
}

/// A note held for `seconds`; what the left channel rendered.
fn hold(audio: &mut RackAudio, key: u8, seconds: f64) -> Vec<f32> {
    let note = HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    };
    let on = Event::NoteOn {
        note,
        velocity: 0.8,
        tune: 0.0,
    };
    let blocks = (seconds * RATE / BLOCK as f64) as usize;
    let mut out = Vec::new();
    for b in 0..blocks {
        let input: Vec<(u32, Event)> = if b == 0 { vec![(0, on)] } else { Vec::new() };
        audio.process(BLOCK, &input);
        out.extend(audio.output(0, BLOCK).iter().map(|s| s[0]));
    }
    audio.process(
        BLOCK,
        &[(
            0,
            Event::NoteOff(kontra_engine::HostPattern {
                port: 0,
                channel: 0,
                key: i32::from(key),
                id: -1,
                clap: true,
            }),
        )],
    );
    for _ in 0..200 {
        audio.process(BLOCK, &[]);
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = args.next().ok_or("usage: unpurge <file> [key]")?.into();
    let key: u8 = args
        .next()
        .and_then(|k| k.to_str()?.parse().ok())
        .unwrap_or(60);
    let hz: f64 = std::env::var("KONTRA_HZ")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8000.0);
    let value: f64 = std::env::var("KONTRA_VALUE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let wanted = std::env::var("KONTRA_WIDGET")
        .unwrap_or_default()
        .to_lowercase();
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    store.defer_heads(true);
    let (mut rack, mut audio) = Rack::new(48_000, store, BLOCK);
    let began = Instant::now();
    rack.load(0, &path)?;
    println!("load {:?}", began.elapsed());
    let dormant = |rack: &Rack| {
        (rack.parts().first().and_then(Option::as_ref))
            .and_then(|p| p.plan.as_deref())
            .map_or(0, |plan| plan.dormant.len())
    };
    println!("dormant groups after load: {}", dormant(&rack));
    for line in rack.take_notices() {
        println!("  said: {line}");
    }
    let widgets: Vec<(usize, String)> = rack
        .parts()
        .first()
        .and_then(Option::as_ref)
        .and_then(|p| p.view.as_ref())
        .map(|v| {
            v.tree()
                .widgets
                .iter()
                .map(|w| w.name.clone())
                .enumerate()
                .collect()
        })
        .unwrap_or_default();
    println!("{} controls", widgets.len());
    if wanted.is_empty() {
        for (n, name) in &widgets {
            println!("  {n}: {name}");
        }
        return Ok(());
    }
    // Let the deferred heads of the loaded groups land.
    std::thread::sleep(Duration::from_secs(2));
    let before = hold(&mut audio, key, 1.0);
    let (rms, band) = levels(&before, hz);
    println!("before: rms {rms:.1} dB, {hz} Hz {band:.1} dB");
    for (n, name) in widgets
        .iter()
        .filter(|(_, n)| n.to_lowercase().contains(&wanted))
    {
        println!("set {n}: {name} = {value}");
        rack.control(0, *n, None, value, Default::default());
    }
    let asked = Instant::now();
    let mut settled = None;
    for _ in 0..2000 {
        rack.tick();
        for line in rack.take_notices() {
            println!("  said ({:?}): {line}", asked.elapsed());
        }
        audio.process(BLOCK, &[]);
        if settled.is_none() && dormant(&rack) == 0 {
            settled = Some(asked.elapsed());
        }
        std::thread::sleep(Duration::from_millis(5));
        if asked.elapsed() > Duration::from_secs(10) {
            break;
        }
    }
    println!(
        "dormant groups after: {} (plan grown after {settled:?})",
        dormant(&rack)
    );
    let after = hold(&mut audio, key, 1.0);
    let (rms, band) = levels(&after, hz);
    println!("after: rms {rms:.1} dB, {hz} Hz {band:.1} dB");
    Ok(())
}
