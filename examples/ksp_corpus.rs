//! Dump every script slot of every `.nki` under a directory into a corpus
//! directory for frontend measurement. Scripts stay outside the repository.
//! `cargo run --release --example ksp_corpus -- <library-root> <out-dir>`
use std::{collections::BTreeSet, fs, path::Path};

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            out.push(path);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (root, out) = (Path::new(&args[1]), Path::new(&args[2]));
    let mut presets = Vec::new();
    walk(root, &mut presets);
    presets.sort();
    let (mut read, mut failed, mut seen) = (0, 0, BTreeSet::new());
    for preset in &presets {
        match sampler_kontakt::read(preset) {
            Ok(loaded) => {
                read += 1;
                for (slot, text) in loaded
                    .instrument
                    .behaviors
                    .iter()
                    .map(|b| &b.source)
                    .enumerate()
                {
                    if text.trim().is_empty()
                        || !seen.insert(*blake3::hash(text.as_bytes()).as_bytes())
                    {
                        continue;
                    }
                    let name = format!("{}-{slot}.ksp", seen.len());
                    fs::write(out.join(&name), text)?;
                    let relative = preset.strip_prefix(root).unwrap_or(preset);
                    println!("{name}\t{}", relative.display());
                }
            }
            Err(error) => {
                failed += 1;
                eprintln!("unreadable\t{}\t{error}", preset.display());
            }
        }
    }
    eprintln!(
        "{} presets, {read} read, {failed} unreadable, {} distinct scripts",
        presets.len(),
        seen.len()
    );
    Ok(())
}
