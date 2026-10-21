//! Local debug probe (untracked): group names and zone counts of an .nki.
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let k = sampler_kontakt::read(std::path::Path::new(&path)).expect("read");
    for (i, g) in k.instrument.groups.iter().enumerate() {
        let zones = k.instrument.zones.iter().filter(|z| z.group.map(|g| g.0) == Some(i)).count();
        println!("{i}\t{zones}\t{}", g.name);
    }
}
