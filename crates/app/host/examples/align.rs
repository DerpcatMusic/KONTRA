//! Measures how late an instrument sounds, as auto-align does at load: a
//! first note and a legato at three velocities, as loaded and per keyswitch.
//! `cargo run --release -p kontra-host --features align --example align -- <file> [key]`

use std::sync::Arc;

use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;

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
        format!(
            "first {:?} legato {:?} mono {}",
            d.first,
            d.legato,
            d.mono()
        )
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
    Ok(())
}
