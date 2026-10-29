//! Prints what the reader makes of an instrument: groups, residency, zones,
//! samples and diagnostics. `cargo run -p kontra-format-kontakt --example inspect -- <file>`

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .ok_or("usage: inspect <file>")?
        .into();
    let t = std::time::Instant::now();
    let i = kontra_format_kontakt::load(&path)?;
    let zones = |purged: bool| -> usize {
        i.groups
            .iter()
            .filter(|g| g.purged == purged)
            .map(|g| g.zones.len())
            .sum()
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
    let mut mods = std::collections::BTreeMap::new();
    for m in i.groups.iter().filter(|g| !g.purged).flat_map(|g| &g.mods) {
        *mods
            .entry(format!("{} @{} {:?}", m.name, m.slot, m.kind))
            .or_insert(0) += 1;
    }
    for (m, n) in mods {
        println!("  mod {n}× {m}");
    }
    // KONTRA_GROUPS=<a>..<b> lists those groups' modulations one by one.
    if let Some((a, b)) = std::env::var("KONTRA_GROUPS")
        .ok()
        .and_then(|r| {
            r.split_once("..")
                .map(|(a, b)| (a.parse::<usize>(), b.parse::<usize>()))
        })
        .and_then(|(a, b)| Some((a.ok()?, b.ok()?)))
    {
        for (g, group) in i.groups.iter().enumerate().take(b).skip(a) {
            for m in &group.mods {
                println!(
                    "  group {g} {} @{} bypass {} {:?}",
                    m.name, m.slot, m.bypass, m.kind
                );
            }
        }
    }
    // KONTRA_SCRIPT_DIR=<dir> writes each script's source there.
    if let Some(dir) = std::env::var_os("KONTRA_SCRIPT_DIR") {
        for (n, script) in i.scripts.iter().enumerate() {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{n}.txt")),
                &script.source,
            )?;
        }
    }
    for d in &i.diagnostics {
        println!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    Ok(())
}
