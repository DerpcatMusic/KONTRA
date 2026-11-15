//! Writes an instrument's script sources and saved values to a folder,
//! one pair of files per slot:
//! `cargo run -p kontra-format-kontakt --example scripts -- <file> <dir>`

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
    let i = kontra_format_kontakt::load(Path::new(&file))?;
    std::fs::create_dir_all(dir)?;
    for s in &i.scripts {
        let out = inside(dir, &format!("{} {}.ksp", s.slot, s.name.replace('/', "_")))?;
        std::fs::write(&out, &s.source)?;
        std::fs::write(out.with_extension("saved"), s.saved.join("\n"))?;
        println!("{}", out.display());
    }
    Ok(())
}
