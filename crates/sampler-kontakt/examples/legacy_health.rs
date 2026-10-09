//! Header/version and supplemental state/resource census. Presets/audio stay in memory.
//! Run under kontakto-heavy; stdout is TSV metadata, never library content.
use ni_file::{NIFile, nis::ItemType};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek},
    path::{Path, PathBuf},
};

fn version(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut head = [0; 16];
    file.read_exact(&mut head).map_err(|e| e.to_string())?;
    if matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("nkr" | "nkx")
    ) {
        return Ok(format!(
            "directory-0x{:x}",
            u16::from_le_bytes(head[4..6].try_into().unwrap())
        ));
    }
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
    {
        return Ok("FileContainer-resource".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    fn nis_version(item: &ni_file::nis::ItemContainer, depth: usize) -> Result<String, String> {
        if depth > 3 {
            return Err("nested version header".into());
        }
        if let Some(frame) = item.find_data(&ItemType::BNISoundHeader) {
            let h = ni_file::nis::BNISoundHeader::try_from(frame).map_err(|e| e.to_string())?;
            return Ok(format!("NIS-Kontakt-{:?}", h.0.patch_version));
        }
        if let Some(frame) = item.find_data(&ItemType::AppSpecific) {
            let app =
                ni_file::nis::AppSpecificProperties::try_from(frame).map_err(|e| e.to_string())?;
            let inner = app.subtree_item.item().map_err(|e| e.to_string())?;
            return nis_version(&inner, depth + 1);
        }
        Ok("NIS-no-sound-header".into())
    }
    match NIFile::read(file).map_err(|e| e.to_string())? {
        NIFile::NISoundContainer(item) => nis_version(&item, 0),
        NIFile::NKSContainer(n) => Ok(match n.header {
            ni_file::kontakt::objects::BPatchHeader::BPatchHeaderV1(_) => "NKS-v1".into(),
            ni_file::kontakt::objects::BPatchHeader::BPatchHeaderV2(h) => {
                format!("NKS-{:?}", h.patch_version)
            }
            ni_file::kontakt::objects::BPatchHeader::BPatchHeaderV42(h) => {
                format!("NKS-{:?}", h.patch_version)
            }
        }),
        NIFile::Monolith(_) => Ok("FileContainer-preset".into()),
        _ => Err("not a preset".into()),
    }
}
fn probe(path: &Path, ext: &str) -> Result<(usize, usize), String> {
    match ext {
        "nksn" => {
            let s = sampler_kontakt::read_snapshot(path).map_err(|e| e.to_string())?;
            Ok((s.groups.len(), s.persistent.len()))
        }
        "nkr" | "nicnt" => {
            let mut r =
                sampler_kontakt::ResourceContainer::open(path).map_err(|e| e.to_string())?;
            let names: Vec<_> = r.names().into_iter().map(str::to_owned).collect();
            let mut total = 0;
            for name in &names {
                total += r
                    .read(name)
                    .map_err(|e| e.to_string())?
                    .ok_or("indexed resource missing")?
                    .len();
            }
            Ok((names.len(), total))
        }
        "nkx" => {
            let a = ni_file::nkr::Archive::read_index(File::open(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if !a.issues.is_empty() {
                return Err(a.issues.join("; "));
            }
            Ok((a.entries.len(), 0))
        }
        _ => Ok((0, 0)), // Instrument load counts come from corpus-health, not this probe.
    }
}
fn walk(path: &Path, seen: &mut HashSet<PathBuf>, paths: &mut Vec<PathBuf>) {
    if path.is_file() {
        paths.push(path.into());
        return;
    }
    if path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains("KONTRA project recovery"))
        || !seen.insert(path.canonicalize().unwrap_or_else(|_| path.into()))
    {
        return;
    }
    for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
        if e.path().is_dir() {
            walk(&e.path(), seen, paths);
        } else {
            paths.push(e.path());
        }
    }
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for root in std::env::args_os().skip(1) {
        walk(Path::new(&root), &mut seen, &mut files);
    }
    assert!(!files.is_empty(), "library roots or files required");
    files.sort();
    files.dedup();
    println!("path\text\tversion\tprobe\trecords\tbytes_or_slots\terror");
    for path in files {
        let ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !["nki", "nkm", "nksn", "nkb", "nkp", "nkr", "nicnt", "nkx"].contains(&ext.as_str()) {
            continue;
        }
        let result = std::panic::catch_unwind(|| (version(&path), probe(&path, &ext)));
        let (v, p) = result.unwrap_or_else(|_| {
            (
                Err("panic while reading header".into()),
                Err("panic while probing".into()),
            )
        });
        let v = v.unwrap_or_else(|e| format!("unknown: {e}"));
        let (status, n, b, error) = match p {
            Ok((n, b)) => ("ok", n, b, String::new()),
            Err(e) => ("error", 0, 0, e),
        };
        let clean = |s: &str| s.replace(['\n', '\r', '\t'], " ");
        println!(
            "{}\t{ext}\t{}\t{status}\t{n}\t{b}\t{}",
            clean(&path.display().to_string()),
            clean(&v),
            clean(&error)
        );
    }
}
