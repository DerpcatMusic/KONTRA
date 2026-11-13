//! A library that moved: the folder its missing samples were in, found again
//! under the folders the player keeps libraries in.

use std::path::{Path, PathBuf};

use kontra_engine::ir;

/// A folder found for a part's missing samples, waiting for the player's yes.
#[derive(Clone, Debug)]
pub struct Offer {
    /// What the report says; the player's click names it.
    pub line: String,
    pub part: usize,
    /// The instrument to load again once the folder is taken.
    pub path: PathBuf,
    pub from: PathBuf,
    pub to: PathBuf,
}

/// The folders (and one file in each) that `diagnostics` say were not found.
pub fn missing(diagnostics: &[ir::Diagnostic]) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for d in diagnostics
        .iter()
        .filter(|d| d.feature == "samples not found")
    {
        // A translator and prepare both say a folder's files are missing.
        if let Some((summary, first)) = d.detail.split_once("\nfirst: ")
            && !found.iter().any(|(s, _)| s == summary)
        {
            found.push((summary.to_owned(), PathBuf::from(first)));
        }
    }
    found
}

/// A folder under `roots` named like the one `file` was in that holds a file
/// of its name, as (the old folder, the new one).
// ponytail: a name match, not a size match (the size of a lost file is not
// known); depth and count are capped so a huge drive cannot stall a load.
pub fn find(roots: &[PathBuf], file: &Path) -> Option<(PathBuf, PathBuf)> {
    let (dir, name) = (file.parent()?, file.file_name()?);
    let wanted = dir.file_name()?;
    let mut visited = 0;
    for root in roots {
        let mut level = vec![root.clone()];
        for _ in 0..=4 {
            let mut next = Vec::new();
            for d in level {
                visited += 1;
                if visited > 5000 {
                    return None;
                }
                if d.file_name() == Some(wanted) && d.join(name).is_file() {
                    return Some((dir.to_owned(), d));
                }
                let Ok(entries) = std::fs::read_dir(&d) else {
                    continue;
                };
                next.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
            level = next;
        }
    }
    None
}

/// `file` where a moved folder put it.
fn remap(moved: &[(PathBuf, PathBuf)], file: &Path) -> Option<PathBuf> {
    let (from, to) = moved.iter().find(|(from, _)| file.starts_with(from))?;
    Some(to.join(file.strip_prefix(from).ok()?))
}

/// Points every sample file under a moved folder at its new place.
pub fn apply(moved: &[(PathBuf, PathBuf)], instrument: &mut ir::Instrument) {
    for s in &mut instrument.samples {
        if let Some(file) = remap(moved, &s.file) {
            s.file = file;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moved_folder_is_found_and_its_samples_follow() {
        let root = std::env::temp_dir().join(format!("kontra-relink-{}", std::process::id()));
        let new = root.join("libs/deep/Areia");
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("a.wav"), b"x").unwrap();
        let old = PathBuf::from("/nowhere/Areia");
        let (from, to) = find(&[root.join("libs")], &old.join("a.wav")).unwrap();
        assert_eq!((&from, &to), (&old, &new));
        assert!(find(&[root.join("libs")], &old.join("b.wav")).is_none());
        let moved = [(from, to)];
        assert_eq!(
            remap(&moved, &old.join("x/a.wav")),
            Some(new.join("x/a.wav"))
        );
        assert_eq!(remap(&moved, Path::new("/other/a.wav")), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
