//! Compares the script values Kontakt saved after loading an instrument
//! (`tools/kontakt-oracle/probes/snapshot_values.lua` writes that file) with
//! the values KONTRA's scripts hold after loading the same instrument.
//! `cargo run --release -p kontra-host --example saved_diff -- <kontakt-saved.nki> <instrument>`
//!
//! Kontakt writes an array's trailing run of equal values once, so both
//! sides are compared with that run folded. A Kontakt-saved file of an
//! encoded library must sit beside the library's `.nicnt` (a symlink is
//! enough) to be read.

use std::collections::HashMap;
use std::path::PathBuf;

use kontra_host::rack::Rack;
use kontra_stream::{Store, StoreConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let usage = "usage: saved_diff <kontakt-saved.nki> <instrument>";
    let (oracle, instrument) = (args.next().ok_or(usage)?, args.next().ok_or(usage)?);
    let kontakt: Vec<String> = kontra_host::instrument::read(&oracle)?
        .scripts
        .into_iter()
        .flat_map(|s| s.saved)
        .collect();
    let (mut rack, mut audio) = Rack::new(48_000, Store::new(StoreConfig::DEFAULT)?, 256);
    rack.load(0, &instrument)?;
    for _ in 0..4 {
        audio.process(256, &[]);
        rack.tick();
    }
    let saved = rack.saved_scripts(0);
    let ours: HashMap<&str, Vec<&str>> = (saved.iter().flatten())
        .map(|e| (name(e), folded(e)))
        .collect();
    let (mut same, mut differ, mut missing) = (0, 0, 0);
    for entry in &kontakt {
        let want = folded(entry);
        match ours.get(name(entry)) {
            Some(got) if *got == want => same += 1,
            Some(got) => {
                differ += 1;
                let at = want.iter().zip(got).take_while(|(a, b)| a == b).count();
                let (k, o) = (
                    want.get(at..).unwrap_or_default(),
                    got.get(at..).unwrap_or_default(),
                );
                let (k, o) = (&k[..k.len().min(4)], &o[..o.len().min(4)]);
                println!(
                    "differ  {} from value {at}: kontakt {k:?} kontra {o:?}",
                    name(entry)
                );
            }
            None => {
                missing += 1;
                println!("missing {}", name(entry));
            }
        }
    }
    let total = kontakt.len();
    println!("{same} of {total} Kontakt values match; {differ} differ, {missing} missing");
    Ok(())
}

/// A saved entry's variable name.
fn name(entry: &str) -> &str {
    entry.split(' ').next().unwrap_or_default()
}

/// A saved entry's values with the trailing run of equal values folded.
fn folded(entry: &str) -> Vec<&str> {
    let mut values: Vec<&str> = entry.split(' ').skip(1).collect();
    while values.len() > 1 && values.last() == values.get(values.len() - 2) {
        values.pop();
    }
    values
}
