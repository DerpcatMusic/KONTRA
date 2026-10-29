//! A snapshot (.nksn): saved script values for an instrument it names but
//! does not hold. Read as its base instrument with those values, found the
//! way Kontakt's library browser pairs them: by name, near the snapshot.
//! Ported from v2 `crates/sampler-kontakt/src/snapshot.rs`
//! (`resolve_snapshot_base`, `find_snapshot_base`, `snapshot_binding_name`).

use std::path::{Path, PathBuf};

use kontra_ir as ir;
use ni_file::kontakt::objects::{Snapshot, snapshot_metadata_names};

use crate::{Error, container, translate};

const SNAPSHOT: u16 = 0x4f;
const METADATA: u16 = 0x51;
/// Directory entries the base search may read before it gives up.
const BUDGET: usize = 16_384;

pub(crate) fn load(path: &Path) -> Result<ir::Instrument, Error> {
    let decode = |what| move |e: ni_file::Error| Error::new(path, what, e);
    let chunks = container::chunks(path)?;
    let state =
        (chunks.find_first(SNAPSHOT)).ok_or_else(|| Error::new(path, "snapshot", "missing"))?;
    let state = Snapshot::try_from(state).map_err(decode("snapshot"))?;
    let names = (chunks.find_first(METADATA))
        .ok_or_else(|| Error::new(path, "snapshot metadata", "missing"))?;
    let names = snapshot_metadata_names(names).map_err(decode("snapshot metadata"))?;
    let base = base(path, &names)?;
    let (base_chunks, monolith) = container::opened(&base)?;
    let mut out = translate::instrument(&base, &base_chunks, monolith, &state.persistent)?;
    if let Some(stem) = path.file_stem() {
        out.name = stem.to_string_lossy().into_owned();
    }
    Ok(out)
}

/// The name of the instrument a snapshot was saved from. Factory snapshots
/// may keep Kontakt's template name first and the base's second.
fn binding(names: &(String, String)) -> &str {
    if names.0 == "Kontakt" && !names.1.is_empty() {
        &names.1
    } else {
        &names.0
    }
}

/// The base instrument: an .nki named for the snapshot's binding or for its
/// folder under `Snapshots/`, in the nearest folder (up to the library) that
/// has one within three levels. Two candidates there is an error, not a
/// guess; directory links are not followed.
fn base(path: &Path, names: &(String, String)) -> Result<PathBuf, Error> {
    let under = |p: &Path, name: &str| {
        (p.parent().and_then(Path::file_name)).is_some_and(|s| s.eq_ignore_ascii_case(name))
    };
    let mut wanted = vec![binding(names).to_owned()];
    if let Some(folder) =
        (path.ancestors().find(|p| under(p, "Snapshots"))).and_then(Path::file_name)
    {
        wanted.push(folder.to_string_lossy().into_owned());
    }
    let library = (path.ancestors())
        .find(|p| {
            p.file_name()
                .is_some_and(|s| s.eq_ignore_ascii_case("Snapshots"))
        })
        .and_then(Path::parent);
    let folders: Vec<&Path> = match library {
        Some(library) => (path.ancestors().skip(1))
            .take_while(|d| d.starts_with(library))
            .collect(),
        None => path.ancestors().skip(1).take(4).collect(),
    };
    let mut left = BUDGET;
    for folder in folders {
        let mut found = Vec::new();
        find(folder, &wanted, 3, &mut left, &mut found)
            .map_err(|e| Error::new(path, "base instrument", e))?;
        found.sort();
        found.dedup();
        match found.as_slice() {
            [] => {}
            [one] => return Ok(one.clone()),
            _ => return Err(Error::new(path, "base instrument", "ambiguous")),
        }
    }
    Err(Error::new(
        path,
        "base instrument",
        format!("no {:?} near it", wanted.join(" or ")),
    ))
}

fn find(
    dir: &Path,
    wanted: &[String],
    depth: usize,
    left: &mut usize,
    found: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        *left = left
            .checked_sub(1)
            .ok_or(std::io::ErrorKind::QuotaExceeded)?;
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        let named = |p: &Path| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki"))
                && (p.file_stem()).is_some_and(|s| wanted.iter().any(|w| s.eq_ignore_ascii_case(w)))
        };
        if kind.is_dir() {
            folders.push(path);
        } else if kind.is_file() && named(&path) {
            found.push(path);
        }
    }
    if depth > 0 {
        folders.sort();
        for dir in folders {
            find(&dir, wanted, depth - 1, left, found)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_name_binds_by_its_second_name() {
        let named = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(binding(&named("Kontakt", "Strings")), "Strings");
        assert_eq!(binding(&named("Kontakt", "")), "Kontakt");
        assert_eq!(binding(&named("Piano", "Other")), "Piano");
    }

    #[test]
    fn a_snapshot_finds_its_base_by_its_folder_and_never_guesses() {
        let root = std::env::temp_dir().join(format!("kontra-snapshot-{}", std::process::id()));
        let snap = root.join("Snapshots/Lib/Factory/a.nksn");
        let inst = root.join("Instruments");
        std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(inst.join("lib.nki"), b"").unwrap();
        let names = ("Kontakt".to_owned(), String::new());
        assert_eq!(base(&snap, &names).unwrap(), inst.join("lib.nki"));
        std::fs::create_dir_all(root.join("More")).unwrap();
        std::fs::write(root.join("More/Lib.nki"), b"").unwrap();
        assert!(base(&snap, &names).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
