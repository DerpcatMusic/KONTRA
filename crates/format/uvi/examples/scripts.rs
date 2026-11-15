//! Writes an instrument's script sources and saved values to a folder,
//! one pair of files per slot, and under `<slot>/modules/` the bank modules
//! that slot's script may load:
//! `cargo run -p kontra-format-uvi --example scripts -- <file> <dir>`

use std::path::{Component, Path, PathBuf};

/// `name` below `dir`, or an error for one that would leave it (`..`, a
/// root, a drive).
fn inside(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let name = Path::new(name);
    let plain = name.components().all(|c| matches!(c, Component::Normal(_)));
    if plain && name.components().next().is_some() {
        Ok(dir.join(name))
    } else {
        Err(format!("{}: not a name inside the folder", name.display()))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(file), Some(dir)) = (args.next(), args.next()) else {
        return Err("usage: scripts <file> <dir>".into());
    };
    let dir = Path::new(&dir);
    let i = kontra_format_uvi::load(Path::new(&file))?;
    std::fs::create_dir_all(dir)?;
    for s in &i.scripts {
        let out = inside(dir, &format!("{} {}.lua", s.slot, s.name.replace('/', "_")))?;
        std::fs::write(&out, &s.source)?;
        std::fs::write(out.with_extension("saved"), s.saved.join("\n"))?;
        // The modules `require` may load, under their bank paths (rooted
        // at the bank: `/Scripts/lib.lua`).
        let modules = dir.join(s.slot.to_string()).join("modules");
        for (path, bytes) in &s.data {
            let to = inside(&modules, path.trim_start_matches('/'))?;
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(to, bytes)?;
        }
        println!("{}", out.display());
    }
    Ok(())
}
