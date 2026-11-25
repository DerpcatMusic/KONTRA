//! Prints every group's source record field, and the group's key tracking
//! and interpolation quality, one tab-separated row each: file, group,
//! stored mode, record version, field, offset, value. The decoder
//! `tools/kontakt-oracle/tm_sweep.py` diffs (`TM_SOURCE_DUMP`).
//! `cargo run -p kontra-format-kontakt --example source_dump -- <file>...`

use ni_file::kontakt::objects::GroupList;

fn main() {
    for path in std::env::args_os().skip(1) {
        let path = std::path::PathBuf::from(path);
        let file = path.display();
        let Some(list) = kontra_format_kontakt::chunks(&path)
            .ok()
            .and_then(|c| c.program()?.ok())
            .and_then(|p| GroupList::try_from(p.0.find_first(0x33)?).ok())
        else {
            eprintln!("{file}: no groups");
            continue;
        };
        for (g, group) in list.groups.iter().enumerate() {
            let Ok((record, _)) = group.source_params() else {
                eprintln!("{file}: group {g}: unreadable source");
                continue;
            };
            let (mode, version) = (record.mode, record.version);
            for f in &record.fields {
                println!(
                    "{file}\t{g}\t{mode}\t{version}\t{}\t{}\t{:?}",
                    f.name, f.offset, f.value
                );
            }
            if let Ok(p) = group.params() {
                println!(
                    "{file}\t{g}\t{mode}\t{version}\tkey_tracking\t-\t{}",
                    p.key_tracking
                );
                println!(
                    "{file}\t{g}\t{mode}\t{version}\tinterp_quality\t-\t{}",
                    p.interp_quality
                );
            }
        }
    }
}
