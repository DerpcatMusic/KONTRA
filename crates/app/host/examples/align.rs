//! Measures how late an instrument sounds, as auto-align does at load: a
//! first note and a legato at three velocities, as loaded and per keyswitch.
//! With `KONTRA_GRID=1` it then plays a first note and a legato through
//! the rack, held as auto-align holds them, and prints how far each attack
//! lands from the beat plus the reported latency.
//! `cargo run --release -p kontra-host --features align --example align -- <file> [key]`

use std::sync::Arc;

use kontra_align::{Delay, ONSET_DB, Timing, onset_ms, transition_ms};
use kontra_host::rack::{Event, HostNote, HostPattern, Rack};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: align <file> [key]")?;
    let key = args.next().and_then(|k| k.parse().ok()).unwrap_or(60);
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let loaded = kontra_host::instrument::build(
        kontra_host::instrument::read(path.as_ref())?,
        RATE,
        &mut store,
    );
    let switches: Vec<u8> = loaded
        .plan
        .switches
        .iter()
        .flat_map(|r| r.clone())
        .take(16)
        .collect();
    let rows = switches.len();
    let mut player =
        kontra_host::align::Measured::new(Arc::new(loaded.plan), store.client(), RATE, switches);
    let start = std::time::Instant::now();
    let timing = kontra_align::measure(&mut player, rows, key);
    let show = |d: &kontra_align::Delay| {
        let steps: Vec<String> = (d.steps.iter())
            .map(|s| format!("≤{} first {:?} legato {:?}", s.top, s.first, s.legato))
            .collect();
        format!("{} mono {}", steps.join(", "), d.mono())
    };
    println!(
        "{}: key {key}, measured in {:.1} s",
        loaded.name,
        start.elapsed().as_secs_f64()
    );
    println!("  as loaded: {}", show(&timing.loaded));
    for (row, d) in timing.articulations.iter().enumerate() {
        println!("  switch {row}: {}", show(d));
    }
    println!("  latest {:.1} ms", timing.latest());
    if std::env::var_os("KONTRA_GRID").is_some() {
        grid(&path, key, &timing.loaded)?;
    }
    Ok(())
}

/// Plays `key` and then a legato a third up through a rack holding them by
/// `delay`, at velocities between and on the measured ones; prints each
/// attack's distance from the beat plus the latency, in ms.
fn grid(path: &str, key: u8, delay: &Delay) -> Result<(), Box<dyn std::error::Error>> {
    let timing = Timing {
        loaded: delay.clone(),
        ..Timing::default()
    };
    let reported = timing.latest();
    let rate = f64::from(RATE);
    let at = |ms: f64| (ms * rate / 1000.0) as usize;
    let second = key + 4;
    let note = |key| HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    };
    let off = |key: u8| {
        Event::NoteOff(HostPattern {
            port: 0,
            channel: 0,
            key: key.into(),
            id: -1,
            clap: true,
        })
    };
    println!("  grid, latency {reported:.1} ms (attack − beat − latency):");
    for velocity in [20u8, 48, 80, 100, 120, 127] {
        let (mut rack, mut audio) = Rack::new(RATE, Store::new(StoreConfig::DEFAULT)?, BLOCK);
        rack.load(0, path.as_ref())?;
        rack.set_hold(0, kontra_host::align::hold(&timing, reported, rate));
        let v = f64::from(velocity) / 127.0;
        let on = |key| Event::NoteOn {
            note: note(key),
            velocity: v,
            tune: 0.0,
        };
        let (t0, t1, t2) = (at(500.0), at(1500.0), at(2700.0));
        // Dynamics and expression as the measurement leaves them.
        let mut events = vec![
            (0, Event::midi1(0xB0, 1, 100)),
            (0, Event::midi1(0xB0, 11, 127)),
            (t0, on(key)),
            (t1, on(second)),
            (t1 + at(50.0), off(key)),
            (t2, off(second)),
        ];
        let mut out = Vec::new();
        while out.len() < t2 + at(600.0) {
            let now = out.len();
            let block: Vec<(u32, Event)> = events
                .iter()
                .filter(|(t, _)| (now..now + BLOCK).contains(t))
                .map(|&(t, e)| ((t - now) as u32, e))
                .collect();
            events.retain(|(t, _)| *t >= now + BLOCK);
            rack.tick();
            audio.settle();
            audio.process(BLOCK, &block);
            out.extend(audio.output(0, BLOCK).iter().map(|[l, r]| l + r));
        }
        let first = onset_ms(out.get(t0..t1).unwrap_or_default(), rate, ONSET_DB);
        let lead = at(kontra_align::LEAD_MS);
        let legato = transition_ms(
            out.get(t1 - lead..t1 + at(1000.0)).unwrap_or_default(),
            rate,
            second,
            ONSET_DB,
        );
        let err = |ms: Option<f32>| ms.map(|ms| format!("{:+.1}", ms - reported));
        println!(
            "    velocity {velocity:3}: first {:?} legato {:?}",
            err(first),
            err(legato)
        );
    }
    Ok(())
}
