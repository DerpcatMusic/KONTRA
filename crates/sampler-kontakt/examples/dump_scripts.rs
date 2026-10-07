//! Extract every distinct KSP script of a library tree, keeping the counts.
//! Usage: dump_scripts ROOT OUT_DIR
//! Writes OUT_DIR/<hash>.ksp once per distinct script and OUT_DIR/manifest.tsv
//! with one `hash<TAB>instrument path` line per use. Library content stays out
//! of the repository: point OUT_DIR at a cache directory and delete it after.
use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write,
    path::Path,
};

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "nki") {
            out.push(path);
        }
    }
}

fn main() {
    let (root, out) = (
        std::env::args().nth(1).unwrap(),
        std::env::args().nth(2).unwrap(),
    );
    std::fs::create_dir_all(&out).unwrap();
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.sort();
    let mut manifest = std::fs::File::create(format!("{out}/manifest.tsv")).unwrap();
    let mut seen = HashSet::new();
    for path in files {
        let Ok(loaded) = sampler_kontakt::read(&path) else {
            continue;
        };
        for b in &loaded.instrument.behaviors {
            let mut h = DefaultHasher::new();
            b.source.hash(&mut h);
            let hash = format!("{:016x}", h.finish());
            if seen.insert(hash.clone()) {
                std::fs::write(format!("{out}/{hash}.ksp"), &b.source).unwrap();
            }
            writeln!(manifest, "{hash}\t{}", path.display()).unwrap();
        }
    }
}
