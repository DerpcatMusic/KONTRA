//! Prints what the reader makes of an instrument: groups, residency, zones,
//! samples and diagnostics. `cargo run -p kontra-format-kontakt --example inspect -- <file>`

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os().nth(1).ok_or("usage: inspect <file>")?.into();
    let t = std::time::Instant::now();
    let i = kontra_format_kontakt::load(&path)?;
    let zones = |purged: bool| -> usize {
        i.groups.iter().filter(|g| g.purged == purged).map(|g| g.zones.len()).sum()
    };
    println!("{} ({:?})", i.name, t.elapsed());
    println!(
        "groups {} ({} purged), zones resident {} purged {}, samples {}, scripts {}",
        i.groups.len(),
        i.groups.iter().filter(|g| g.purged).count(),
        zones(false),
        zones(true),
        i.samples.len(),
        i.scripts.len()
    );
    for d in &i.diagnostics {
        println!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    Ok(())
}
