//! What every format reads into the instrument model: diagnostics folded by
//! feature, so a format reports each gap once with a count (06-FORMATS.md).

pub mod timing;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use kontra_ir as ir;

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
#[must_use]
pub fn moved(file: &Path) -> PathBuf {
    let Ok(moved) = MOVED.read() else {
        return file.to_path_buf();
    };
    (moved.iter())
        .find_map(|(from, to)| Some(to.join(file.strip_prefix(from).ok()?)))
        .unwrap_or_else(|| file.to_path_buf())
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
}
