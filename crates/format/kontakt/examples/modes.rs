//! Counts group source modes (0 Sampler, 3 DFD) and their zones over NKIs.
//! `cargo run -p kontra-format-kontakt --example modes -- <file>...`

use std::collections::BTreeMap;

use ni_file::kontakt::objects::{GroupList, ZoneList};

fn main() {
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let Ok(chunks) = kontra_format_kontakt::chunks(&path) else {
            println!("{}: unreadable", path.display());
            continue;
        };
        let Some(Ok(program)) = chunks.program() else {
            println!("{}: no program", path.display());
            continue;
        };
        let mut modes: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        let Some(list) = program
            .0
            .find_first(0x33)
            .and_then(|c| GroupList::try_from(c).ok())
        else {
            println!("{}: no groups", path.display());
            continue;
        };
        let zones: Vec<u32> = program
            .0
            .find_first(0x34)
            .and_then(|c| ZoneList::try_from(c).ok())
            .map(|z| z.group_ids)
            .unwrap_or_default();
        for (i, g) in list.groups.iter().enumerate() {
            let mode = g.source_identity().map_or(u32::MAX, |s| s.mode);
            let e = modes.entry(mode).or_default();
            e.0 += 1;
            e.1 += zones.iter().filter(|&&z| z as usize == i).count();
        }
        println!(
            "{}: (mode: groups, zones) {modes:?}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
    }
}
