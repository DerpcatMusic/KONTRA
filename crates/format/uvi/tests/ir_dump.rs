//! Dumps every program's IR (`{:#?}`) of the banks under `KONTRA_UVI_DUMP`
//! (`;`-separated `.ufs` paths) into `KONTRA_UVI_DUMP_OUT`, so a mechanical
//! rewrite can diff the IR before and after: one file per program, or with
//! `KONTRA_UVI_DUMP_HASH` set one `<bank>.hashes` line per program (its
//! text's hash and length) for a bank too large to keep as text, written as
//! each program loads. `KONTRA_UVI_DUMP_LIMIT` takes the first N programs;
//! `KONTRA_UVI_DUMP_EFFECTS` dumps only the effect side (buses, racks, slot
//! models, knob routes, each group's inserts, diagnostics), which is what
//! an effect-table rewrite can change and ~100× faster on large programs.
//! Skipped when either variable is unset (CI has no vendor files).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

#[test]
fn dump_ir_of_installed_banks() {
    let (Some(banks), Some(out)) = (
        std::env::var("KONTRA_UVI_DUMP").ok(),
        std::env::var_os("KONTRA_UVI_DUMP_OUT").map(PathBuf::from),
    ) else {
        return;
    };
    let hashed = std::env::var_os("KONTRA_UVI_DUMP_HASH").is_some();
    let effects = std::env::var_os("KONTRA_UVI_DUMP_EFFECTS").is_some();
    let limit = std::env::var("KONTRA_UVI_DUMP_LIMIT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(usize::MAX);
    std::fs::create_dir_all(&out).unwrap();
    for bank in banks.split(';').map(PathBuf::from) {
        let stem = bank.file_stem().and_then(|s| s.to_str()).unwrap_or("bank");
        let hashes = out.join(format!("{stem}.hashes"));
        let _ = std::fs::remove_file(&hashes);
        for program in kontra_format_uvi::programs(&bank)
            .unwrap()
            .into_iter()
            .take(limit)
        {
            let text = match kontra_format_uvi::load(&bank.join(&program)) {
                Ok(inst) if effects => {
                    let groups: Vec<_> = inst.groups.iter().map(|g| &g.inserts).collect();
                    format!(
                        "{:#?}",
                        (
                            (&inst.buses, &inst.inserts, &inst.sends, &inst.main),
                            (&inst.slot_models, &inst.knob_mods, &inst.knob_blends),
                            (groups, &inst.impulses, &inst.diagnostics),
                        )
                    )
                }
                Ok(inst) => format!("{inst:#?}"),
                Err(e) => format!("error: {e}"),
            };
            if hashed {
                let mut h = DefaultHasher::new();
                text.hash(&mut h);
                let line = format!("{:016x} {:>10} {program}\n", h.finish(), text.len());
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&hashes)
                    .unwrap();
                std::io::Write::write_all(&mut f, line.as_bytes()).unwrap();
            } else {
                let name = format!("{stem}--{}.txt", program.replace(['/', ' '], "_"));
                // Two programs mapping to one file would drop one from the diff.
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(out.join(&name));
                std::io::Write::write_all(&mut file.expect(&name), text.as_bytes()).unwrap();
            }
        }
    }
}
