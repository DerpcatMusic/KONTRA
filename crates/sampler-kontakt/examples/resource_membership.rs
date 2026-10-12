//! Bounded resource facts only; only caller-supplied names are emitted.
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected resource container path")?;
    let mut container = sampler_kontakt::ResourceContainer::open(Path::new(&path))?;
    let names: Vec<_> = container.names().into_iter().map(str::to_owned).collect();
    let (mut readable, mut unreadable) = (0, 0);
    for name in &names {
        match container.read(name) {
            Ok(Some(_)) => readable += 1,
            _ => unreadable += 1,
        }
    }
    println!("MEMBERS\t{}\t{readable}\t{unreadable}", names.len());
    for name in std::env::args().skip(2) {
        let matches = names
            .iter()
            .filter(|n| n.replace(['|', '\\'], "/").eq_ignore_ascii_case(&name))
            .count();
        let result = match container.read(&name) {
            Ok(Some(_)) => "readable",
            Ok(None) => "absent",
            Err(_) => "unreadable",
        };
        println!("REQUEST\t{name}\t{matches}\t{result}");
    }
    Ok(())
}
