//! The header follows the instrument's own preset controls by name. After
//! the player changed a part, a text its script began to show or a file it
//! loaded that names exactly one preset of the part's instrument (a
//! snapshot or user preset of its base, or the instrument file itself) is
//! the preset that plays, unchanged. Names compare without case, runs of
//! spaces or leading list labels. No match keeps the name, marked changed.

use std::collections::HashSet;
use std::path::Path;

use kontra_host::browser::Browser;
use kontra_host::rack::Rack;

/// A name as compared: lower case, single spaces, without leading list
/// labels: a number, or one word and a number (ANALOG's menu shows
/// `002. Blown Fuse` and `NEO 99. Oscillating Current` for the snapshots
/// `Blown Fuse` and `Oscillating Current`). One word always stays.
fn key(name: &str) -> String {
    let numbered = |w: &&str| {
        let digits = w.trim_end_matches(['.', ')']);
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    };
    let mut words: Vec<&str> = name.split_whitespace().collect();
    while let Some(at) =
        (words.iter().take(2).position(numbered)).filter(|&at| at + 1 < words.len())
    {
        words.drain(..=at);
    }
    words.join(" ").to_lowercase()
}

/// Whether `file` is a preset of the instrument at `base`.
fn of(file: &Path, base: &Path) -> bool {
    file == base
        || kontra_host::presets::read(file).is_some_and(|u| u.base == base)
        || kontra_host::instrument::preset(file).is_some_and(|s| s.base == base)
}

/// Makes the preset each changed part's script just named the one it plays.
pub fn follow(rack: &mut Rack, browser: &Browser) {
    for part in 0..rack.parts().len() {
        let names = rack.take_named(part);
        let Some(p) = rack.parts().get(part).and_then(Option::as_ref) else {
            continue;
        };
        if names.is_empty() || !p.modified {
            continue;
        }
        let (Some(path), Some(base)) = (p.path.as_deref(), p.source.as_deref()) else {
            continue;
        };
        let library = browser
            .find(path)
            .and_then(|(l, _)| browser.catalog.libraries.get(l));
        let Some(library) = library else {
            continue;
        };
        let wanted: HashSet<String> = names.iter().map(|n| key(n)).collect();
        let named = |f: &&std::path::PathBuf| {
            f.file_stem()
                .is_some_and(|s| wanted.contains(&key(&s.to_string_lossy())))
        };
        let found: Vec<&std::path::PathBuf> = library.presets.iter().filter(named).collect();
        // Read only the files a name matched: a snapshot's base is in it.
        let mut found = found.into_iter().filter(|f| of(f, base));
        if let (Some(one), None) = (found.next(), found.next()) {
            let one = one.clone();
            rack.set_preset(part, &one);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_compare_without_case_or_runs_of_spaces() {
        assert_eq!(super::key("  Blown   FUSE "), super::key("blown fuse"));
        assert_eq!(super::key("002. Blown Fuse"), super::key("Blown Fuse"));
        assert_eq!(
            super::key("NEO 99. Oscillating  Current"),
            "oscillating current"
        );
        assert_eq!(super::key("015. 9001 Punch"), super::key("9001 Punch"));
        assert_eq!(super::key("9001"), "9001");
        assert_eq!(super::key("Bass 2"), "bass 2");
        assert_ne!(super::key("Blown Fuses"), super::key("Blown Fuse"));
    }
}
