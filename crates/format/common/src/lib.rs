//! What every format reads into the instrument model: the shared [`Format`]
//! trait and [`Registry`], [`ProtectionStatus`] diagnostics, positioned
//! [`io::ReadAt`] helpers, cross-platform [`paths`] resolution, consolidated
//! [`header`] probing, and [`Notes`] folded by feature (`06-FORMATS.md`).

pub mod builder;
pub mod format;
pub mod header;
pub mod io;
pub mod math;
pub mod paths;
pub mod timing;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use kontra_ir as ir;

pub use format::{
    Confidence, EntryRef, Format, FormatError, FormatId, LoadCx, LoadOptions, ProtectionStatus,
    ProtectionSystem, Registry, ScanCollector, ScanEntry, ScanSink,
};

/// Library folders the player moved, as (old, new). Every reader looks for a
/// sample under the new folder when its own path lies under the old one.
// ponytail: process-wide, because every instance shares the player's one
// settings file and the readers run deep inside each format; a parameter
// through every reader when a second source of moves appears.
static MOVED: RwLock<Vec<(PathBuf, PathBuf)>> = RwLock::new(Vec::new());

/// Sets the moved folders ([`moved`]).
pub fn set_moved(moved: &[(PathBuf, PathBuf)]) {
    if let Ok(mut now) = MOVED.write()
        && *now != moved
    {
        moved.clone_into(&mut now);
    }
}

/// `file` where a moved folder put it; `file` itself when none did.
///
/// When several moved folders match `file`, the longest (most specific) source
/// prefix wins. Matching normalizes `\`/`|` separators and accepts ASCII
/// case-insensitive prefix matches on Linux so paths authored on Windows/macOS
/// still remap cleanly.
#[must_use]
pub fn moved(file: &Path) -> PathBuf {
    moved_with_case(file, None).unwrap_or_else(|| file.to_path_buf())
}

/// Remaps `file` through registered moved folders, returning `Some(new_path)`
/// only when a mapping matched. When `new_path` does not exist directly on a
/// case-sensitive filesystem, attempts case-insensitive resolution under the
/// destination root.
#[must_use]
pub fn moved_with_case(file: &Path, cache: Option<&mut paths::DirCache>) -> Option<PathBuf> {
    let moved = MOVED.read().ok()?;
    let norm_file = paths::normalize_separators(&file.to_string_lossy());
    let mut best: Option<(usize, &Path, PathBuf)> = None;

    for (from, to) in moved.iter() {
        if let Ok(rel) = file.strip_prefix(from) {
            let score = from.as_os_str().len();
            if best.as_ref().is_none_or(|(b, _, _)| score > *b) {
                best = Some((score, to.as_path(), rel.to_path_buf()));
            }
            continue;
        }
        let norm_from = paths::normalize_separators(&from.to_string_lossy());
        let norm_from = norm_from.trim_end_matches('/');
        if norm_from.is_empty() {
            continue;
        }
        if let Some(rel) = strip_prefix_ascii_ci(&norm_file, norm_from) {
            let score = norm_from.len();
            if best.as_ref().is_none_or(|(b, _, _)| score > *b) {
                best = Some((score, to.as_path(), PathBuf::from(rel)));
            }
        }
    }

    let (_, to, rel) = best?;
    let direct = to.join(&rel);
    if direct.exists() || rel.as_os_str().is_empty() {
        return Some(direct);
    }
    rel.to_str()
        .and_then(|rel| paths::resolve_case_insensitive_bounded(to, to, rel, cache))
        .or(Some(direct))
}

fn strip_prefix_ascii_ci<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path.len() < prefix.len() {
        return None;
    }
    let head = path.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = path.get(prefix.len()..)?;
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix('/')
    }
}

/// Diagnostics folded by feature: one entry with a count and the first detail.
#[derive(Default)]
pub struct Notes(
    BTreeMap<(ir::Impact, String), (usize, String)>,
    /// Sample files not found: per folder, a count and the first file.
    BTreeMap<PathBuf, (usize, PathBuf)>,
);

impl Notes {
    pub fn add(&mut self, impact: ir::Impact, feature: &str, detail: impl ToString) {
        let entry = self
            .0
            .entry((impact, feature.into()))
            .or_insert((0, detail.to_string()));
        entry.0 += 1;
    }

    /// Records a DRM-protected container or member cleanly as a [`ir::Impact::Broken`]
    /// diagnostic with the protection system name and sanitized detail.
    pub fn add_protected(&mut self, status: &ProtectionStatus, path: &Path) {
        if let ProtectionStatus::Protected { system, detail } = status {
            let msg = if detail.is_empty() {
                format!("{} ({system})", path.display())
            } else {
                format!("{} ({system}): {detail}", path.display())
            };
            self.add(ir::Impact::Broken, "protected container", msg);
        }
    }

    /// A stored field or chunk the reader parsed and the translator drops:
    /// `what` names it (`group.midi_channel`), `detail` is the first value.
    /// Its impact is unknown, so it is `Latent`: kept out of the audible
    /// counts, ranked by `ignored ` prefix and count.
    pub fn ignored(&mut self, what: &str, detail: impl ToString) {
        self.add(ir::Impact::Latent, &format!("ignored {what}"), detail);
    }

    /// Reports every field of the parsed record `v` (a struct that prints
    /// as `{:#?}`) that holds a value and is in neither `used` (the
    /// translator maps it) nor `cosmetic` (nothing audible). A field left
    /// at its zero, -1, `false`, empty or `None` is not a stored choice.
    pub fn ignored_fields(
        &mut self,
        level: &str,
        v: &impl std::fmt::Debug,
        used: &[&str],
        cosmetic: &[&str],
    ) {
        for line in format!("{v:#?}").lines() {
            let Some((name, value)) = line.strip_prefix("    ").and_then(|l| l.split_once(": "))
            else {
                continue;
            };
            let value = value.trim_end_matches(',');
            let plain = name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            let empty = matches!(value, "0" | "0.0" | "false" | "\"\"" | "[]" | "None" | "-1");
            if plain && !empty && ![used, cosmetic].iter().any(|l| l.contains(&name)) {
                let shown = if value.ends_with(['[', '{', '(']) {
                    "(nested)"
                } else {
                    value
                };
                self.ignored(&format!("{level}.{name}"), shown);
            }
        }
    }

    /// A sample file that is not there: told once per folder, with the
    /// first file, which is how the player searches for the folder.
    pub fn add_missing(&mut self, file: &Path) {
        let dir = file.parent().unwrap_or(Path::new("")).to_path_buf();
        self.1.entry(dir).or_insert((0, file.to_path_buf())).0 += 1;
    }

    #[must_use]
    pub fn finish(self) -> Vec<ir::Diagnostic> {
        let missing = self
            .1
            .into_iter()
            .map(|(dir, (count, first))| ir::Diagnostic {
                impact: ir::Impact::Broken,
                feature: "samples not found".into(),
                detail: format!(
                    "{count} sample{} not found in {}\nfirst: {}",
                    if count == 1 { "" } else { "s" },
                    dir.display(),
                    first.display()
                ),
            });
        self.0
            .into_iter()
            .rev()
            .map(|((impact, feature), (count, first))| ir::Diagnostic {
                impact,
                detail: format!("{count}× (first: {first})"),
                feature,
            })
            .chain(missing)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Record {
        used: i32,
        spare: i32,
        zero: i32,
        ui: bool,
        nested: Vec<u8>,
    }

    #[test]
    fn only_stored_unmapped_fields_are_reported() {
        let mut notes = Notes::default();
        let r = Record {
            used: 1,
            spare: 7,
            zero: 0,
            ui: true,
            nested: vec![1],
        };
        assert!(r.ui && r.used + r.spare + r.zero == 8 && !r.nested.is_empty());
        notes.ignored_fields("rec", &r, &["used"], &["ui"]);
        notes.ignored_fields("rec", &r, &["used"], &["ui"]);
        let got: Vec<_> = notes.finish().into_iter().map(|d| d.feature).collect();
        assert_eq!(got, ["ignored rec.spare", "ignored rec.nested"]);
    }

    #[test]
    fn moved_picks_longest_prefix_and_handles_case_and_separators() {
        let root = std::env::temp_dir().join(format!("kontra-moved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("NewLib/Samples");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("Violin_C4.WAV"), b"ok").unwrap();

        set_moved(&[
            (PathBuf::from("/old"), root.join("Wrong")),
            (PathBuf::from("/old/Lib"), root.join("NewLib")),
        ]);
        let remapped = moved(Path::new("/OLD/lib/samples/violin_c4.wav"));
        assert_eq!(remapped, target.join("Violin_C4.WAV"));
        assert_eq!(
            moved(Path::new("/elsewhere/x.wav")),
            Path::new("/elsewhere/x.wav")
        );

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::OsStr::from_bytes(b"note-\xff.wav");
            assert_eq!(
                moved(&Path::new("/old/Lib/Samples").join(name)),
                target.join(name)
            );
        }

        set_moved(&[]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn notes_records_protected_container_diagnostic() {
        let mut notes = Notes::default();
        let status = ProtectionStatus::protected(
            ProtectionSystem::PaceIlok,
            "encrypted bank requires authorization",
        );
        notes.add_protected(&status, Path::new("/libs/Locked.ufs"));
        let diags = notes.finish();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].impact, ir::Impact::Broken);
        assert_eq!(diags[0].feature, "protected container");
        assert!(diags[0].detail.contains("PACE / iLok"));
    }
}
