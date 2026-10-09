//! Resource names and counts only; no script, sample or decoded payload output.
use std::{collections::BTreeMap, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("expected NKI path")?;
    let kontakt = sampler_kontakt::read(Path::new(&path))?;
    let i = &kontakt.instrument;
    println!("COUNTS\t{}\t{}\t{}", i.zones.len(), i.assets.len(), i.unsupported.len());
    let mut features = BTreeMap::new();
    for issue in &i.unsupported {
        *features.entry(issue.feature.as_str()).or_insert(0usize) += 1;
        if issue.feature == "missing sample" || issue.feature.ends_with("impulse response") {
            let hex: String = issue.value.bytes().map(|b| format!("{b:02x}")).collect();
            println!("RESOURCE\t{}\t{}\t{hex}", issue.feature, issue.location);
        }
    }
    for (feature, count) in features {
        println!("FEATURE\t{count}\t{feature}");
    }
    Ok(())
}
