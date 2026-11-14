//! What the browser shows of a library's preset paths: the noise every row
//! repeats taken out. The library's own name, a sound bank's `Presets`
//! folder, folders every preset shares and a name prefix every row of a
//! folder shares go; what is left is the folder as breadcrumbs and the name.

/// A preset as the browser lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Shown {
    /// Its folders, outermost first; a name prefix its folder's rows share
    /// is the last.
    pub crumbs: Vec<String>,
    pub name: String,
}

/// Letters and digits only, lower case: `ANALOG STRINGS` is `analog-strings`.
fn key(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A library's name without a version or release tag at its end
/// (`Augmented Orchestra v1.1.2-R2R` is `Augmented Orchestra`).
pub(super) fn library(name: &str) -> &str {
    let version = |w: &str| {
        let w = w.strip_prefix(['v', 'V']).unwrap_or(w);
        w.starts_with(|c: char| c.is_ascii_digit()) && w.contains('.')
    };
    match name.split(' ').position(version) {
        Some(at) if at > 0 => {
            let cut: usize = name.split(' ').take(at).map(|w| w.len() + 1).sum();
            name.get(..cut.saturating_sub(1)).unwrap_or(name).trim_end()
        }
        _ => name,
    }
}

/// `presets` (paths `Folder/Name` inside library `name`) as the browser
/// lists them, in the same order.
pub(super) fn presets(name: &str, presets: &[String]) -> Vec<Shown> {
    let own = key(library(name));
    let folders: Vec<Vec<String>> = (presets.iter())
        .map(|p| {
            let mut out: Vec<String> = Vec::new();
            let mut bank = false;
            let segs: Vec<&str> = p.split('/').collect();
            for &seg in segs.get(..segs.len().saturating_sub(1)).unwrap_or_default() {
                let stem = seg.strip_suffix(".ufs").unwrap_or(seg);
                let after_bank = std::mem::replace(&mut bank, stem.len() != seg.len());
                let k = key(stem);
                // The library's name, or its end (`UVI - Name` names `Name`).
                if k == own || k.len() > 3 && own.ends_with(&k) {
                    continue;
                }
                if after_bank && k == "presets" {
                    continue;
                }
                out.push(stem.to_owned());
            }
            out
        })
        .collect();
    // Folders every preset shares say nothing.
    let shared = (folders.iter().skip(1)).fold(folders.first().map_or(0, Vec::len), |n, f| {
        (folders.first().into_iter().flatten().zip(f))
            .take(n)
            .take_while(|(a, b)| a == b)
            .count()
    });
    let mut shown: Vec<Shown> = (presets.iter().zip(folders))
        .map(|(p, f)| {
            let file = p.rsplit('/').next().unwrap_or(p);
            Shown {
                crumbs: f.into_iter().skip(shared).collect(),
                name: unprefixed(file, &own).to_owned(),
            }
        })
        .collect();
    // A name prefix every row of a folder shares becomes its last crumb.
    let mut at = 0;
    while at < shown.len() {
        let crumbs = shown.get(at).map(|s| s.crumbs.clone()).unwrap_or_default();
        let end = at
            + (shown.get(at..).unwrap_or_default().iter())
                .take_while(|s| s.crumbs == crumbs)
                .count();
        if let Some(rows) = shown.get_mut(at..end) {
            share(rows);
        }
        at = end.max(at + 1);
    }
    for s in &mut shown {
        distinct(&mut s.crumbs);
        s.name = unrepeated(&s.name, &s.crumbs);
    }
    shown
}

/// `name` without leading words its folders already say:
/// `Snapshots › Shreddage 3.5 E-Upright` lists `- E-Upright 3.5 INIT -` as
/// `INIT`, and `06 Ensemble Patches` lists `01 Ensemble - Sustained` as
/// `01 Sustained`. A leading number stays; the whole name when nothing
/// would be left.
fn unrepeated(name: &str, crumbs: &[String]) -> String {
    let said: Vec<String> = (crumbs.iter())
        .flat_map(|c| c.split(' ').map(key))
        .filter(|k| !k.is_empty())
        .collect();
    let mut words = name.split(' ').filter(|w| !w.is_empty()).peekable();
    let number = words.next_if(|w| w.chars().all(|c| c.is_ascii_digit()));
    let mut dropped = false;
    while let Some(w) = words.next_if(|w| key(w).is_empty() || said.contains(&key(w))) {
        dropped |= !key(w).is_empty();
    }
    let separators = [' ', '-', '_', ':', '.', '–'];
    let rest = words.collect::<Vec<_>>().join(" ");
    let rest = rest.trim_matches(separators);
    if !dropped || rest.is_empty() {
        return name.trim_matches(separators).to_owned();
    }
    number.map_or_else(|| rest.to_owned(), |n| format!("{n} {rest}"))
}

/// Crumbs without one its neighbour repeats: `3 Trumpets › 3 Trumpets` is
/// one crumb, and `Mutes › 3 Trumpets Mutes` keeps the fuller.
fn distinct(crumbs: &mut Vec<String>) {
    let mut at = 1;
    while at < crumbs.len() {
        let (a, b) = match (crumbs.get(at - 1), crumbs.get(at)) {
            (Some(a), Some(b)) => (key(a), key(b)),
            _ => break,
        };
        if b.contains(&a) {
            crumbs.remove(at - 1);
            at = at.saturating_sub(1).max(1);
        } else if a.contains(&b) {
            crumbs.remove(at);
        } else {
            at += 1;
        }
    }
}

/// A preset's name alone, as a list of several libraries' shows it: its
/// file name without its library's name in front.
pub(super) fn name<'a>(library_name: &str, path: &'a str) -> &'a str {
    let file = path.rsplit('/').next().unwrap_or(path);
    unprefixed(file, &key(library(library_name)))
}

/// `name` without a leading library name `own` (as keyed) and the
/// separators after it; the whole name when nothing would be left.
fn unprefixed<'a>(name: &'a str, own: &str) -> &'a str {
    if own.is_empty() {
        return name;
    }
    let mut seen = String::new();
    for (i, c) in name.char_indices() {
        if c.is_ascii_alphanumeric() {
            seen.push(c.to_ascii_lowercase());
        }
        if seen.len() == own.len() {
            if seen != own {
                return name;
            }
            let rest = name.get(i + c.len_utf8()..).unwrap_or_default();
            // A word only: `Una Cordas` keeps its name.
            if rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
                return name;
            }
            let rest = rest.trim_start_matches([' ', '-', '_', ':', '.', '–']);
            return if rest.is_empty() { name } else { rest };
        }
    }
    name
}

/// The rows of one folder: words every name starts with (three rows or
/// more, each keeping a word) move into a last crumb.
fn share(rows: &mut [Shown]) {
    if rows.len() < 3 {
        return;
    }
    let words: Vec<Vec<&str>> = rows.iter().map(|r| r.name.split(' ').collect()).collect();
    let Some(first) = words.first() else { return };
    let n = (0..first.len())
        .take_while(|&i| {
            words
                .iter()
                .all(|w| w.len() > i + 1 && w.get(i) == first.get(i))
        })
        .count();
    if n == 0 {
        return;
    }
    let prefix = first.get(..n).unwrap_or_default().join(" ");
    let names: Vec<String> = words
        .iter()
        .map(|w| w.get(n..).unwrap_or_default().join(" "))
        .collect();
    for (r, name) in rows.iter_mut().zip(names) {
        r.crumbs.push(prefix.clone());
        r.name = name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(lib: &str, paths: &[&str]) -> Vec<(String, String)> {
        let paths: Vec<String> = paths.iter().map(|&p| p.to_owned()).collect();
        (presets(lib, &paths).into_iter())
            .map(|s| (s.crumbs.join(" › "), s.name))
            .collect()
    }

    #[test]
    fn the_library_name_bank_folders_and_shared_prefixes_go() {
        let rows = shown(
            "Una Corda",
            &[
                "Instruments/Una Corda Felt",
                "Instruments/Una Corda Pure",
                "Snapshots/Una Corda/Factory/Soft",
            ],
        );
        let want = [
            ("Instruments", "Felt"),
            ("Instruments", "Pure"),
            ("Snapshots › Factory", "Soft"),
        ];
        assert_eq!(rows, want.map(|(a, b)| (a.to_owned(), b.to_owned())));
        let rows = shown(
            "UVI - Augmented Orchestra v1.1.2-R2R",
            &[
                "Augmented Orchestra.ufs/Presets/01 Strings/V Strings Bartok",
                "Augmented Orchestra.ufs/Presets/01 Strings/V Strings Col Legno",
                "Augmented Orchestra.ufs/Presets/01 Strings/V Strings Marcato",
                "Augmented Orchestra.ufs/Presets/02 Brass/Horns",
            ],
        );
        let want = [
            ("01 Strings › V Strings", "Bartok"),
            ("01 Strings › V Strings", "Col Legno"),
            ("01 Strings › V Strings", "Marcato"),
            ("02 Brass", "Horns"),
        ];
        assert_eq!(rows, want.map(|(a, b)| (a.to_owned(), b.to_owned())));
    }

    #[test]
    fn words_a_folder_already_says_go_from_its_names() {
        let rows = shown(
            "Shreddage 3 E-Upright",
            &[
                "Snapshots/Shreddage 3.5 E-Upright/- E-Upright 3.5 INIT -",
                "Snapshots/Shreddage 3.5 E-Upright/E-Upright 3.5 - MWAH -",
                "Snapshots/Shreddage 3.5 E-Upright/E-Upright 3.5 Aggressive Swirl",
                "Instruments/Bass",
            ],
        );
        let names: Vec<&str> = rows.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, ["INIT", "MWAH", "Aggressive Swirl", "Bass"]);
        let rows = shown(
            "Lib",
            &[
                "06 Ensemble Patches/01 Ensemble - Sustained",
                "06 Ensemble Patches/03 Ensemble - Harmonics",
                "07 Solo/Ensemble",
            ],
        );
        let names: Vec<&str> = rows.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(names, ["01 Sustained", "03 Harmonics", "Ensemble"]);
    }

    #[test]
    fn a_crumb_its_neighbour_repeats_goes() {
        let mut crumbs = ["Brass", "3 Trumpets", "Mutes", "3 Trumpets Mutes"]
            .map(String::from)
            .to_vec();
        distinct(&mut crumbs);
        assert_eq!(crumbs, ["Brass", "3 Trumpets Mutes"]);
        let mut crumbs = ["3 Trumpets", "3 Trumpets"].map(String::from).to_vec();
        distinct(&mut crumbs);
        assert_eq!(crumbs, ["3 Trumpets"]);
    }

    #[test]
    fn a_name_that_is_only_the_library_stays_and_versions_drop() {
        assert_eq!(
            shown("ANALOG STRINGS", &["Instruments/ANALOG STRINGS"]),
            [(String::new(), "ANALOG STRINGS".to_owned())]
        );
        assert_eq!(
            library("UVI - Augmented Orchestra v1.1.2-R2R"),
            "UVI - Augmented Orchestra"
        );
        assert_eq!(library("Conflux"), "Conflux");
    }
}
